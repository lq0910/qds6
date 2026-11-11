//! 功能：非秘密连接配置的事务存储；由调用方在后台线程使用；时间：2026-11-10；作者：lq。

use qds6_core::ConnectionProfile;
use rusqlite::{Connection, params};
use std::path::Path;
use thiserror::Error;

// 功能：软件偏好独立存储，不混入连接导出或凭据；时间：2026-11-11；作者：lq。
mod preferences;
pub use preferences::{AppPreferences, ColorTheme};

// 功能：独立分组目录及原子归组操作，不接触凭据；时间：2026-11-11；作者：lq。
mod groups;
pub use groups::{ConnectionGroup, GroupScope, validate_group_name};

/// 功能：区分数据库、模型与未来格式错误；时间：2026-11-10；作者：lq。
#[derive(Debug, Error)]
pub enum StoreError {
    #[error("本地配置数据库操作失败：{0}")]
    Database(#[from] rusqlite::Error),
    #[error("本地连接配置格式不正确")]
    Format(#[from] serde_json::Error),
    #[error(transparent)]
    Validation(#[from] qds6_core::ProfileError),
    #[error("配置来自更新版本，当前版本不能修改")]
    NewerSchema,
    #[error("导入包含重复连接 ID")]
    DuplicateId,
    #[error("软件设置值超出支持范围")]
    InvalidPreferences,
    #[error("分组名称须为 1–256 个字符，不能含控制字符或使用“未分组”")]
    InvalidGroup,
    #[error("此分组名称已存在")]
    DuplicateGroup,
    #[error("分组或连接已不存在，请刷新列表")]
    MissingGroupTarget,
}

/// 功能：拥有独立 SQLite 连接，不跨 UI 线程共享；时间：2026-11-10；作者：lq。
pub struct ProfileStore(Connection);

impl ProfileStore {
    /// 功能：创建/打开数据库并原子初始化 schema，拒绝未来版本；时间：2026-11-10；作者：lq。
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        let mut db = Connection::open(path)?;
        db.busy_timeout(std::time::Duration::from_secs(3))?;
        let version: u32 = db.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if version > 1 {
            return Err(StoreError::NewerSchema);
        }
        // 功能：启动并发读取连接和偏好时先取得初始化写锁，避免延迟事务升级互锁导致主题读取失败；时间：2026-11-11；作者：lq。
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        tx.execute_batch("CREATE TABLE IF NOT EXISTS profiles (id TEXT PRIMARY KEY NOT NULL, body TEXT NOT NULL); PRAGMA user_version = 1;")?;
        // 功能：增量建立软件偏好表，保留所有已有连接；时间：2026-11-11；作者：lq。
        tx.execute_batch("CREATE TABLE IF NOT EXISTS preferences (id INTEGER PRIMARY KEY CHECK (id = 1), body TEXT NOT NULL);")?;
        // 功能：增量兼容旧库，按协议分区回填已有分组，保留空文件夹与收起状态；时间：2026-11-11；作者：lq。
        tx.execute_batch("CREATE TABLE IF NOT EXISTS connection_groups (scope TEXT NOT NULL, name TEXT NOT NULL, collapsed INTEGER NOT NULL DEFAULT 0, PRIMARY KEY(scope, name));
            INSERT OR IGNORE INTO connection_groups(scope, name)
            SELECT CASE WHEN json_extract(body, '$.protocol') IN ('ssh', 'sftp') THEN 'servers' ELSE 'databases' END, json_extract(body, '$.group')
            FROM profiles WHERE json_valid(body) AND json_extract(body, '$.group') <> '';")?;
        tx.commit()?;
        Ok(Self(db))
    }

    /// 功能：读取并验证全部连接，损坏数据不得静默丢弃；时间：2026-11-10；作者：lq。
    pub fn list(&self) -> Result<Vec<ConnectionProfile>, StoreError> {
        let mut statement = self.0.prepare("SELECT body FROM profiles ORDER BY rowid")?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        rows.map(|row| {
            let profile: ConnectionProfile = serde_json::from_str(&row?)?;
            profile.validate()?;
            Ok(profile)
        })
        .collect()
    }

    /// 功能：通过稳定 ID 更新单项配置，成功提交后才返回；时间：2026-11-10；作者：lq。
    pub fn save(&mut self, profile: &ConnectionProfile) -> Result<(), StoreError> {
        profile.validate()?;
        let body = serde_json::to_string(profile)?;
        let tx = self.0.transaction()?;
        tx.execute("INSERT INTO profiles(id, body) VALUES (?1, ?2) ON CONFLICT(id) DO UPDATE SET body=excluded.body", params![profile.id.to_string(), body])?;
        groups::ensure_profile_group(&tx, profile)?;
        tx.commit()?;
        Ok(())
    }

    /// 功能：原子删除一个配置，凭据由上层显式处理；时间：2026-11-10；作者：lq。
    pub fn delete(&mut self, id: &str) -> Result<(), StoreError> {
        self.0.execute("DELETE FROM profiles WHERE id=?1", [id])?;
        Ok(())
    }

    /// 功能：预先验证全部配置，再事务合并或替换；任意失败保留原库；时间：2026-11-10；作者：lq。
    pub fn restore(
        &mut self,
        profiles: &[ConnectionProfile],
        replace: bool,
    ) -> Result<(), StoreError> {
        let mut ids = std::collections::HashSet::new();
        let mut bodies = Vec::with_capacity(profiles.len());
        for profile in profiles {
            profile.validate()?;
            if !ids.insert(profile.id) {
                return Err(StoreError::DuplicateId);
            }
            bodies.push((profile.id.to_string(), serde_json::to_string(profile)?));
        }
        let tx = self.0.transaction()?;
        if replace {
            tx.execute("DELETE FROM profiles", [])?;
        }
        for (id, body) in bodies {
            tx.execute("INSERT INTO profiles(id,body) VALUES (?1,?2) ON CONFLICT(id) DO UPDATE SET body=excluded.body", params![id, body])?;
        }
        for profile in profiles {
            groups::ensure_profile_group(&tx, profile)?;
        }
        tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use qds6_core::Protocol;

    /// 功能：验证精确往返、稳定 ID 更新及无效写入保护；时间：2026-11-10；作者：lq。
    #[test]
    fn round_trip_and_failed_write_preserve_existing_profile() {
        let mut store = ProfileStore::open(":memory:").unwrap();
        let mut profile = ConnectionProfile::new(Protocol::PostgreSql);
        profile.name = "开发库".into();
        profile.host = "::1".into();
        profile.username = "测试用户".into();
        store.save(&profile).unwrap();
        assert_eq!(store.list().unwrap(), vec![profile.clone()]);
        profile.favorite = true;
        store.save(&profile).unwrap();
        assert_eq!(store.list().unwrap(), vec![profile.clone()]);
        profile.port = 0;
        assert!(store.save(&profile).is_err());
        assert_eq!(store.list().unwrap()[0].port, 5432);
        assert!(store.list().unwrap()[0].favorite);
    }

    /// 功能：拒绝未来 schema 且不覆盖其版本；时间：2026-11-10；作者：lq。
    #[test]
    fn future_schema_is_not_downgraded() {
        let path = std::env::temp_dir().join(format!(
            "qds6-schema-{}-{}.sqlite3",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let db = Connection::open(&path).unwrap();
        db.pragma_update(None, "user_version", 99).unwrap();
        drop(db);
        assert!(matches!(
            ProfileStore::open(&path),
            Err(StoreError::NewerSchema)
        ));
        let db = Connection::open(&path).unwrap();
        assert_eq!(
            db.pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))
                .unwrap(),
            99
        );
        drop(db);
        std::fs::remove_file(path).unwrap();
    }
}
