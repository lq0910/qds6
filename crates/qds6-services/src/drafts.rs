//! 功能：Keychain 密钥保护远程编辑草稿，私有目录加密恢复；时间：2026-11-10；作者：lq。

use crate::{
    backup::{read_bounded, write_new},
    credentials::CredentialStore,
    sftp::RemoteDocument,
};
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit, Payload},
};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
};
use zeroize::Zeroizing;

// 功能：本进程串行草稿读写与密钥首次创建，避免同时生成不同密钥；时间：2026-11-10；作者：lq。
static LOCK: Mutex<()> = Mutex::new(());
const AAD: &[u8] = b"QDS6 remote draft v1";

/// 功能：恢复时保留原基线和用户草稿，仍需重新连接并比较远端；时间：2026-11-10；作者：lq。
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Draft {
    pub schema: u32,
    pub connection: uuid::Uuid,
    pub baseline: RemoteDocument,
    pub text: String,
}

impl Draft {
    /// 功能：草稿文件名仅包含稳定连接 ID，不暴露远程路径；每个连接保留当前编辑文档；时间：2026-11-10；作者：lq。
    pub fn path(&self, directory: &Path) -> PathBuf {
        directory.join(format!("{}.qds6draft", self.connection))
    }
}

/// 功能：有界草稿经认证加密并原子替换本地缓存，失败不写明文；时间：2026-11-10；作者：lq。
pub fn save(directory: &Path, draft: &Draft) -> Result<(), String> {
    save_current(directory, draft, &AtomicU64::new(0), 0)
}

/// 功能：在同一写锁内核对编辑代次，取消后的后台任务不能复活旧草稿；时间：2026-11-11；作者：lq。
pub fn save_current(
    directory: &Path,
    draft: &Draft,
    revision: &AtomicU64,
    expected: u64,
) -> Result<(), String> {
    let _guard = LOCK.lock().map_err(|_| "草稿状态异常")?;
    if revision.load(Ordering::SeqCst) != expected {
        return Ok(());
    }
    qds6_core::TextLimits::default()
        .validate(draft.text.as_bytes())
        .map_err(|e| e.to_string())?;
    qds6_core::TextLimits::default()
        .validate(draft.baseline.text.as_bytes())
        .map_err(|e| e.to_string())?;
    std::fs::create_dir_all(directory).map_err(|_| "无法创建草稿目录")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700))
            .map_err(|_| "草稿目录权限设置失败")?;
    }
    let key = CredentialStore::draft_key()?;
    let cipher = XChaCha20Poly1305::new_from_slice(&key).map_err(|_| "草稿密钥不可用")?;
    let mut nonce = [0; 24];
    rand::rngs::OsRng.fill_bytes(&mut nonce);
    let plain = Zeroizing::new(serde_json::to_vec(draft).map_err(|_| "草稿编码失败")?);
    let encrypted = cipher
        .encrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: &plain,
                aad: AAD,
            },
        )
        .map_err(|_| "草稿加密失败")?;
    let mut bytes = b"QDS6D1".to_vec();
    bytes.extend_from_slice(&nonce);
    bytes.extend(encrypted);
    let temporary = directory.join(format!("{}.tmp", uuid::Uuid::new_v4()));
    write_new(&temporary, &bytes)?;
    if revision.load(Ordering::SeqCst) != expected {
        let _ = std::fs::remove_file(temporary);
        return Ok(());
    }
    if std::fs::rename(&temporary, draft.path(directory)).is_err() {
        let _ = std::fs::remove_file(temporary);
        return Err("草稿缓存替换失败".into());
    }
    Ok(())
}

/// 功能：仅恢复当前连接的加密草稿，不自动联网或重放保存；时间：2026-11-10；作者：lq。
pub fn load(directory: &Path, connection: uuid::Uuid) -> Result<Option<Draft>, String> {
    let _guard = LOCK.lock().map_err(|_| "草稿状态异常")?;
    let path = directory.join(format!("{connection}.qds6draft"));
    if !path.exists() {
        return Ok(None);
    }
    let bytes = read_bounded(&path)?;
    if bytes.len() < 46 || &bytes[..6] != b"QDS6D1" {
        return Err("草稿格式无效，原缓存已保留".into());
    }
    let key = CredentialStore::draft_key()?;
    let cipher = XChaCha20Poly1305::new_from_slice(&key).map_err(|_| "草稿密钥不可用")?;
    let plain = Zeroizing::new(
        cipher
            .decrypt(
                XNonce::from_slice(&bytes[6..30]),
                Payload {
                    msg: &bytes[30..],
                    aad: AAD,
                },
            )
            .map_err(|_| "草稿认证失败，原缓存已保留")?,
    );
    let draft: Draft = serde_json::from_slice(&plain).map_err(|_| "草稿内容无效")?;
    if draft.schema != 1 || draft.connection != connection {
        return Err("草稿版本或连接引用无效".into());
    }
    qds6_core::TextLimits::default()
        .validate(draft.text.as_bytes())
        .map_err(|e| e.to_string())?;
    Ok(Some(draft))
}

/// 功能：成功保存或明确丢弃后移除单个草稿缓存；时间：2026-11-10；作者：lq。
pub fn remove(directory: &Path, connection: uuid::Uuid) -> Result<(), String> {
    let _guard = LOCK.lock().map_err(|_| "草稿状态异常")?;
    match std::fs::remove_file(directory.join(format!("{connection}.qds6draft"))) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err("无法清理草稿缓存".into()),
    }
}
