//! 功能：保存本机 SQL 草稿、收藏、按库历史及快速入口；时间：2026-11-27；作者：lq。

use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};
use uuid::Uuid;
use zeroize::Zeroizing;

const AAD: &[u8] = b"QDS6 local tools v1";
pub const MAX_DRAFT: usize = 512 * 1024;
pub const MAX_SNIPPET: usize = 64 * 1024;

/// 功能：收藏和历史保留完整 SQL；列表标题按需从 SQL 派生；时间：2026-11-27；作者：lq。
#[derive(Clone, Serialize, Deserialize)]
pub struct SqlEntry {
    pub id: Uuid,
    pub connection: Option<Uuid>,
    pub sql: String,
}

/// 功能：目录书签绑定连接的稳定 ID 和绝对路径；时间：2026-11-27；作者：lq。
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bookmark {
    pub connection: Uuid,
    pub path: String,
}

/// 功能：数据独立于配置及备份，默认按生产标记决定历史开关；时间：2026-11-27；作者：lq。
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LocalTools {
    pub recent: Vec<Uuid>,
    pub bookmarks: Vec<Bookmark>,
    pub draft: Option<String>,
    pub favorites: Vec<SqlEntry>,
    pub history: Vec<SqlEntry>,
    pub history_enabled: BTreeMap<Uuid, bool>,
}

/// 功能：无文件时跳过凭据库；认证失败时交由调用方保留原文件；时间：2026-11-27；作者：lq。
pub fn load(path: &Path) -> Result<LocalTools, String> {
    if !path.try_exists().map_err(|_| "无法访问本机快捷数据")? {
        return Ok(LocalTools::default());
    }
    serde_json::from_slice(&crate::drafts::open(path, AAD)?)
        .map_err(|_| "本机快捷数据格式不受支持".into())
}

/// 功能：后台原子更新加密文件，读取失败和取消不会写明文；时间：2026-11-27；作者：lq。
pub fn save(path: &Path, data: &LocalTools) -> Result<(), String> {
    let plain = Zeroizing::new(serde_json::to_vec(data).map_err(|_| "本机快捷数据编码失败")?);
    if plain.len() > crate::backup::MAX_FILE - 46 {
        return Err("本机快捷数据超过容量，请清理部分历史或收藏".into());
    }
    let bytes = crate::drafts::seal(&plain, AAD)?;
    let temporary = path.with_extension(format!("{}.tmp", Uuid::new_v4()));
    crate::backup::write_new(&temporary, &bytes)?;
    if std::fs::rename(&temporary, path).is_err() {
        let _ = std::fs::remove_file(temporary);
        return Err("本机快捷数据保存失败".into());
    }
    Ok(())
}

#[cfg(all(test, feature = "test-support"))]
mod tests {
    use super::*;

    /// 功能：验证默认空数据、加密重开、更新和损坏认证的持久化链路；时间：2026-11-27；作者：lq。
    #[test]
    fn encrypted_local_tools_roundtrip() {
        crate::credentials::use_fixture_draft_key([42; 32]);
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("tools.qds6");
        let mut data = load(&path).unwrap();
        let connection = Uuid::new_v4();
        data.recent.push(connection);
        data.bookmarks.push(Bookmark {
            connection,
            path: "/srv/中文".into(),
        });
        data.draft = Some("SELECT 'secret fixture'".into());
        data.history_enabled.insert(connection, false);
        save(&path, &data).unwrap();
        let encrypted = std::fs::read(&path).unwrap();
        assert!(!encrypted.windows(14).any(|span| span == b"secret fixture"));
        let mut restored = load(&path).unwrap();
        assert!(restored.bookmarks == data.bookmarks);
        assert_eq!(restored.draft, data.draft);
        assert_eq!(restored.history_enabled.get(&connection), Some(&false));
        restored.draft = Some("SELECT 2".into());
        save(&path, &restored).unwrap();
        assert_eq!(load(&path).unwrap().draft, restored.draft);
        let mut corrupted = std::fs::read(&path).unwrap();
        *corrupted.last_mut().unwrap() ^= 1;
        std::fs::write(&path, &corrupted).unwrap();
        assert!(load(&path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), corrupted);
    }
}
