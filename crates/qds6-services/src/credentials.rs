//! 功能：macOS Keychain 凭据隔离，错误不回显秘密；时间：2026-11-10；作者：lq。

use qds6_core::CredentialRef;
use zeroize::Zeroizing;

/// 功能：凭据库边界，仅暴露经过清理的错误；时间：2026-11-10；作者：lq。
pub struct CredentialStore;

/// 功能：仅测试构建的显式内存密钥覆盖，不读取环境变量，生产构建无此入口；时间：2026-11-10；作者：lq。
#[cfg(feature = "test-support")]
static FIXTURE_DRAFT_KEY: std::sync::OnceLock<[u8; 32]> = std::sync::OnceLock::new();

/// 功能：真实 Keychain 联调使用独立测试引用，避免触碰已有应用草稿密钥；时间：2026-11-11；作者：lq。
#[cfg(feature = "test-support")]
static FIXTURE_DRAFT_REFERENCE: std::sync::OnceLock<CredentialRef> = std::sync::OnceLock::new();

/// 功能：只在测试 feature 下选择独立 Keychain 密钥引用，仍调用真实系统凭据库；时间：2026-11-11；作者：lq。
#[cfg(feature = "test-support")]
pub fn use_fixture_draft_reference(reference: CredentialRef) {
    let _ = FIXTURE_DRAFT_REFERENCE.set(reference);
}

/// 功能：自动化 Metal 测试不触发 Keychain 跨二进制授权；真实 Keychain 另由服务验收覆盖；时间：2026-11-10；作者：lq。
#[cfg(feature = "test-support")]
pub fn use_fixture_draft_key(key: [u8; 32]) {
    let _ = FIXTURE_DRAFT_KEY.set(key);
}

impl CredentialStore {
    /// 功能：草稿密钥仅在 Keychain 缺失条目时生成，不将锁定/拒绝访问误认为首次运行；时间：2026-11-10；作者：lq。
    pub fn draft_key() -> Result<Zeroizing<Vec<u8>>, String> {
        #[cfg(feature = "test-support")]
        if let Some(key) = FIXTURE_DRAFT_KEY.get() {
            return Ok(Zeroizing::new(key.to_vec()));
        }
        use base64::Engine;
        use rand::RngCore;
        let id = CredentialRef(uuid::Uuid::from_u128(0x71647336000040008000000000000001));
        #[cfg(feature = "test-support")]
        let id = FIXTURE_DRAFT_REFERENCE.get().cloned().unwrap_or(id);
        let entry = Self::entry(&id)?;
        match entry.get_password() {
            Ok(encoded) => {
                let encoded = Zeroizing::new(encoded);
                let decoded = base64::engine::general_purpose::STANDARD
                    .decode(encoded.as_bytes())
                    .map_err(|_| "草稿密钥无效，已有草稿未覆盖")?;
                if decoded.len() != 32 {
                    return Err("草稿密钥长度无效".into());
                }
                Ok(Zeroizing::new(decoded))
            }
            Err(keyring::Error::NoEntry) => {
                let mut key = Zeroizing::new(vec![0; 32]);
                rand::rngs::OsRng.fill_bytes(&mut key);
                let encoded =
                    Zeroizing::new(base64::engine::general_purpose::STANDARD.encode(&key));
                entry
                    .set_password(&encoded)
                    .map_err(|_| "无法保存草稿密钥，草稿将只保留本次会话")?;
                Ok(key)
            }
            Err(_) => Err("Keychain 不可用，草稿将只保留本次会话".into()),
        }
    }
    /// 功能：根据不透明引用定位本应用 Keychain 条目；时间：2026-11-10；作者：lq。
    fn entry(id: &CredentialRef) -> Result<keyring::Entry, String> {
        keyring::Entry::new("QDS6", &id.0.to_string()).map_err(|_| "系统凭据库不可用".into())
    }

    /// 功能：显式存储用户秘密，不退化到明文配置；时间：2026-11-10；作者：lq。
    pub fn set(id: &CredentialRef, secret: &str) -> Result<(), String> {
        Self::entry(id)?
            .set_password(secret)
            .map_err(|_| "无法保存凭据，请检查 Keychain 权限".into())
    }

    /// 功能：读取秘密并在最后释放时清零；时间：2026-11-10；作者：lq。
    pub fn get(id: &CredentialRef) -> Result<Zeroizing<String>, String> {
        Self::entry(id)?
            .get_password()
            .map(Zeroizing::new)
            .map_err(|_| "凭据缺失或 Keychain 已锁定，请重新输入".into())
    }

    /// 功能：移除本应用引用的单个秘密，其他应用条目不受影响；时间：2026-11-10；作者：lq。
    pub fn delete(id: &CredentialRef) -> Result<(), String> {
        match Self::entry(id)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err("无法删除系统凭据".into()),
        }
    }
}
