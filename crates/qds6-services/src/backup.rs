//! 功能：有界原生配置导出、Argon2id + XChaCha20-Poly1305 备份及事务恢复；时间：2026-11-10；作者：lq。

use argon2::{Algorithm, Argon2, Params, Version};
use base64::{Engine, engine::general_purpose::STANDARD};
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit, Payload},
};
use qds6_core::ConnectionProfile;
use qds6_storage::ProfileStore;
use rand::{RngCore, rngs::OsRng};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::Path,
};
use uuid::Uuid;
use zeroize::{Zeroize, Zeroizing};

/// 功能：统一资源上限，解密及解析前拒绝过大输入；时间：2026-11-10；作者：lq。
pub const MAX_FILE: usize = 16 * 1024 * 1024;
const MAX_PROFILES: usize = 10_000;
const AAD: &[u8] = b"QDS6 backup v1;argon2id:m=65536,t=3,p=1;xchacha20poly1305";

/// 功能：可移植非秘密配置，格式版本与应用版本分离；时间：2026-11-10；作者：lq。
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Configuration {
    pub format: String,
    pub schema: u32,
    pub profiles: Vec<ConnectionProfile>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub secrets: std::collections::BTreeMap<Uuid, String>,
}

/// 功能：固定算法的加密容器，不允许外部输入控制 KDF 资源消耗；时间：2026-11-10；作者：lq。
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    format: String,
    schema: u32,
    salt: String,
    nonce: String,
    ciphertext: String,
}

/// 功能：仅供同一 Mac 回滚的恢复前快照，保留 Keychain 引用但不包含密码；时间：2026-11-11；作者：lq。
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LocalSnapshot {
    format: String,
    schema: u32,
    profiles: Vec<ConnectionProfile>,
}

/// 功能：确认后从本机快照回滚，并先保存当前状态，解析失败不改数据库；时间：2026-11-11；作者：lq。
pub fn restore_local_snapshot(
    store: &mut ProfileStore,
    source: &Path,
    before: &Path,
) -> Result<(), String> {
    let snapshot: LocalSnapshot =
        serde_json::from_slice(&read_bounded(source)?).map_err(|_| "不是有效的 QDS6 本机快照")?;
    if snapshot.format != "qds6-local-snapshot"
        || snapshot.schema != 1
        || snapshot.profiles.len() > MAX_PROFILES
    {
        return Err("本机快照版本或连接数量无效".into());
    }
    for profile in &snapshot.profiles {
        profile.validate().map_err(|e| e.to_string())?;
    }
    write_snapshot(store, before)?;
    store
        .restore(&snapshot.profiles, true)
        .map_err(|e| e.to_string())
}

/// 功能：排他写入本机快照，替换或回滚前保留当前连接状态；时间：2026-11-11；作者：lq。
fn write_snapshot(store: &ProfileStore, path: &Path) -> Result<(), String> {
    let snapshot = LocalSnapshot {
        format: "qds6-local-snapshot".into(),
        schema: 1,
        profiles: store.list().map_err(|e| e.to_string())?,
    };
    write_new(
        path,
        &serde_json::to_vec_pretty(&snapshot).map_err(|_| "快照序列化失败")?,
    )
}

impl Configuration {
    /// 功能：普通导出移除机器绑定凭据引用，保留连接稳定 ID；时间：2026-11-10；作者：lq。
    pub fn new(profiles: &[ConnectionProfile]) -> Self {
        let mut profiles = profiles.to_vec();
        for profile in &mut profiles {
            profile.credential = None;
        }
        Self {
            format: "qds6-config".into(),
            schema: 1,
            profiles,
            secrets: Default::default(),
        }
    }

    /// 功能：仅显式选择时从 Keychain 收集可移植密码，Agent/私钥文件不包含；时间：2026-11-10；作者：lq。
    pub fn with_credentials(profiles: &[ConnectionProfile]) -> Result<Self, String> {
        let mut config = Self::new(profiles);
        for profile in profiles {
            if let Some(reference) = &profile.credential {
                config.secrets.insert(
                    profile.id,
                    crate::credentials::CredentialStore::get(reference)?.to_string(),
                );
            }
        }
        Ok(config)
    }

    /// 功能：恢复预览前校验版本、数量、连接与依赖引用；时间：2026-11-10；作者：lq。
    pub fn validate(&self) -> Result<(), String> {
        if self.format != "qds6-config" || self.schema != 1 {
            return Err("不支持的配置格式或 schema".into());
        }
        if self.profiles.len() > MAX_PROFILES {
            return Err("连接数量超过 10000 上限".into());
        }
        let ids: HashSet<_> = self.profiles.iter().map(|p| p.id).collect();
        if ids.len() != self.profiles.len() {
            return Err("连接 ID 重复".into());
        }
        if self
            .secrets
            .iter()
            .any(|(id, secret)| !ids.contains(id) || secret.len() > 65536)
        {
            return Err("凭据引用或大小无效".into());
        }
        for profile in &self.profiles {
            profile.validate().map_err(|e| e.to_string())?;
            if profile.credential.is_some() {
                return Err("可移植配置不能包含机器绑定凭据引用".into());
            }
            for reference in [profile.ssh.jump_profile, profile.db.tunnel_profile]
                .into_iter()
                .flatten()
            {
                let target = self
                    .profiles
                    .iter()
                    .find(|p| p.id == reference)
                    .ok_or("缺少 SSH 依赖连接")?;
                if target.protocol != qds6_core::Protocol::Ssh || target.ssh.jump_profile.is_some()
                {
                    return Err("仅支持一个 SSH 跳板层级".into());
                }
            }
        }
        Ok(())
    }

    /// 功能：仅在明确确认后落库，先生成不覆盖的恢复前快照；时间：2026-11-10；作者：lq。
    pub fn restore(
        &self,
        store: &mut ProfileStore,
        snapshot: &Path,
        replace: bool,
    ) -> Result<(), String> {
        self.validate()?;
        // 功能：快照保留本机凭据引用，不作为可移植导出；时间：2026-11-10；作者：lq。
        let existing = store.list().map_err(|e| e.to_string())?;
        write_snapshot(store, snapshot)?;
        let mut profiles = self.profiles.clone();
        let mut created = Vec::new();
        for profile in &mut profiles {
            if let Some(secret) = self.secrets.get(&profile.id) {
                let reference = qds6_core::CredentialRef(Uuid::new_v4());
                if crate::credentials::CredentialStore::set(&reference, secret).is_err() {
                    for reference in &created {
                        let _ = crate::credentials::CredentialStore::delete(reference);
                    }
                    return Err("恢复凭据失败，现有连接未修改".into());
                }
                created.push(reference.clone());
                profile.credential = Some(reference);
            } else if !replace {
                profile.credential = existing
                    .iter()
                    .find(|p| {
                        p.id == profile.id
                            && p.protocol == profile.protocol
                            && p.host == profile.host
                            && p.port == profile.port
                            && p.username == profile.username
                    })
                    .and_then(|p| p.credential.clone());
            }
        }
        if let Err(error) = store.restore(&profiles, replace) {
            for reference in &created {
                let _ = crate::credentials::CredentialStore::delete(reference);
            }
            return Err(error.to_string());
        }
        Ok(())
    }
}

impl Drop for Configuration {
    /// 功能：预览/备份结束清理秘密文本；时间：2026-11-10；作者：lq。
    fn drop(&mut self) {
        for secret in self.secrets.values_mut() {
            secret.zeroize();
        }
    }
}

/// 功能：有界读取文件，避免依赖可能过期的 metadata 大小；时间：2026-11-10；作者：lq。
pub fn read_bounded(path: &Path) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|_| "无法打开所选文件")?
        .take(MAX_FILE as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "无法读取所选文件")?;
    if bytes.len() > MAX_FILE {
        return Err("文件超过 16 MiB 上限".into());
    }
    Ok(bytes)
}

/// 功能：新文件排他创建并刷盘，失败不覆盖既有文件；时间：2026-11-10；作者：lq。
pub fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if bytes.len() > MAX_FILE {
        return Err("输出超过 16 MiB 上限".into());
    }
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|_| "无法创建文件；目标可能已存在，请选择新名称")?;
    if file.write_all(bytes).and_then(|_| file.sync_all()).is_err() {
        drop(file);
        let _ = std::fs::remove_file(path);
        return Err("写入失败，原有文件未被替换".into());
    }
    Ok(())
}

/// 功能：导出 JSON 或口令加密配置，绝不接受空加密口令；时间：2026-11-10；作者：lq。
pub fn encode(config: &Configuration, password: Option<&str>) -> Result<Vec<u8>, String> {
    config.validate()?;
    let plain = Zeroizing::new(serde_json::to_vec_pretty(config).map_err(|_| "配置编码失败")?);
    if plain.len() > MAX_FILE / 2 {
        return Err("配置超过备份负载上限".into());
    }
    let Some(password) = password else {
        if !config.secrets.is_empty() {
            return Err("包含凭据的备份必须加密".into());
        }
        return Ok(plain.to_vec());
    };
    if password.chars().count() < 8 || password.len() > 1024 {
        return Err("备份口令需 8 个字符以上且不超过 1024 字节".into());
    }
    let mut salt = [0; 16];
    let mut nonce = [0; 24];
    OsRng.fill_bytes(&mut salt);
    OsRng.fill_bytes(&mut nonce);
    let key = derive(password, &salt)?;
    let cipher = XChaCha20Poly1305::new_from_slice(key.as_ref()).map_err(|_| "加密初始化失败")?;
    let encrypted = cipher
        .encrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: &plain,
                aad: AAD,
            },
        )
        .map_err(|_| "备份加密失败")?;
    serde_json::to_vec_pretty(&Envelope {
        format: "qds6-encrypted".into(),
        schema: 1,
        salt: STANDARD.encode(salt),
        nonce: STANDARD.encode(nonce),
        ciphertext: STANDARD.encode(encrypted),
    })
    .map_err(|_| "容器编码失败".into())
}

/// 功能：先验证容器边界再认证解密，错误口令与篡改均无写入；时间：2026-11-10；作者：lq。
pub fn decode(bytes: &[u8], password: Option<&str>) -> Result<Configuration, String> {
    if bytes.len() > MAX_FILE {
        return Err("备份文件超过上限".into());
    }
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| "配置不是有效 JSON")?;
    let encrypted = value.get("format").and_then(|v| v.as_str()) == Some("qds6-encrypted");
    let config: Configuration = if value.get("format").and_then(|v| v.as_str())
        == Some("qds6-encrypted")
    {
        let envelope: Envelope = serde_json::from_value(value).map_err(|_| "加密容器字段无效")?;
        if envelope.schema != 1 {
            return Err("不支持的加密 schema".into());
        }
        let password = password
            .filter(|s| !s.is_empty() && s.len() <= 1024)
            .ok_or("请提供备份口令")?;
        let salt = STANDARD.decode(envelope.salt).map_err(|_| "备份盐无效")?;
        let nonce = STANDARD
            .decode(envelope.nonce)
            .map_err(|_| "备份 nonce 无效")?;
        if salt.len() != 16 || nonce.len() != 24 {
            return Err("备份头长度无效".into());
        }
        let ciphertext = STANDARD
            .decode(envelope.ciphertext)
            .map_err(|_| "备份负载无效")?;
        let key = derive(password, &salt)?;
        let cipher =
            XChaCha20Poly1305::new_from_slice(key.as_ref()).map_err(|_| "解密初始化失败")?;
        let plain = Zeroizing::new(
            cipher
                .decrypt(
                    XNonce::from_slice(&nonce),
                    Payload {
                        msg: &ciphertext,
                        aad: AAD,
                    },
                )
                .map_err(|_| "口令错误或备份已损坏".to_string())?,
        );
        serde_json::from_slice(&plain).map_err(|_| "备份内容无效")?
    } else {
        serde_json::from_value(value).map_err(|_| "配置字段无效")?
    };
    config.validate()?;
    if !encrypted && !config.secrets.is_empty() {
        return Err("拒绝含明文凭据的配置，请使用加密备份".into());
    }
    Ok(config)
}

/// 功能：固定 Argon2id 参数 64 MiB / 3 次 / 单通道，密钥释放清零；时间：2026-11-10；作者：lq。
fn derive(password: &str, salt: &[u8]) -> Result<Zeroizing<[u8; 32]>, String> {
    let params = Params::new(65536, 3, 1, Some(32)).map_err(|_| "KDF 参数错误")?;
    let mut key = Zeroizing::new([0u8; 32]);
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(password.as_bytes(), salt, key.as_mut())
        .map_err(|_| "口令派生失败")?;
    Ok(key)
}

/// 功能：为不覆盖的快照生成稳定可追踪文件名；时间：2026-11-10；作者：lq。
pub fn snapshot_name() -> String {
    format!("before-restore-{}.json", Uuid::new_v4())
}

#[cfg(test)]
mod tests {
    use super::*;
    use qds6_core::Protocol;

    /// 功能：验证错误口令、篡改、schema、失败回滚及幂等合并；时间：2026-11-10；作者：lq。
    #[test]
    fn authenticated_roundtrip_and_transactional_restore() {
        let mut profile = ConnectionProfile::new(Protocol::Ssh);
        profile.name = "中文连接".into();
        profile.host = "localhost".into();
        let config = Configuration::new(&[profile.clone()]);
        let encrypted = encode(&config, Some("test-password-2026")).unwrap();
        assert!(!String::from_utf8_lossy(&encrypted).contains("localhost"));
        assert!(decode(&encrypted, Some("wrong-password")).is_err());
        let recovered = decode(&encrypted, Some("test-password-2026")).unwrap();
        assert_eq!(recovered.profiles, vec![profile]);
        let mut tampered: serde_json::Value = serde_json::from_slice(&encrypted).unwrap();
        let mut cipher = STANDARD
            .decode(tampered["ciphertext"].as_str().unwrap())
            .unwrap();
        cipher[0] ^= 1;
        tampered["ciphertext"] = STANDARD.encode(cipher).into();
        assert!(
            decode(
                &serde_json::to_vec(&tampered).unwrap(),
                Some("test-password-2026")
            )
            .is_err()
        );
        let dir = tempfile::tempdir().unwrap();
        let mut store = ProfileStore::open(dir.path().join("test.db")).unwrap();
        for _ in 0..2 {
            recovered
                .restore(&mut store, &dir.path().join(snapshot_name()), false)
                .unwrap();
        }
        assert_eq!(store.list().unwrap().len(), 1);
        // 功能：验证本机快照能够回滚替换恢复，并保留回滚前状态；时间：2026-11-11；作者：lq。
        let snapshot = dir.path().join(snapshot_name());
        Configuration::new(&[])
            .restore(&mut store, &snapshot, true)
            .unwrap();
        assert!(store.list().unwrap().is_empty());
        restore_local_snapshot(&mut store, &snapshot, &dir.path().join(snapshot_name())).unwrap();
        assert_eq!(store.list().unwrap(), recovered.profiles);
        let mut invalid = recovered.clone();
        invalid.profiles[0].port = 0;
        assert!(
            invalid
                .restore(&mut store, &dir.path().join(snapshot_name()), true)
                .is_err()
        );
        assert_eq!(store.list().unwrap(), recovered.profiles);
        let mut future = recovered;
        future.schema = 99;
        assert!(future.validate().is_err());
        assert!(write_new(&dir.path().join("test.db"), b"overwrite").is_err());
    }
}
