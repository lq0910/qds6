//! 功能：服务器/数据库分组目录、持久折叠状态及事务更新；时间：2026-11-11；作者：lq。

use super::{ProfileStore, StoreError};
use qds6_core::{ConnectionProfile, Protocol};
use rusqlite::{Transaction, params};

/// 功能：同名分组按工作区隔离，服务器改名不会修改数据库配置；时间：2026-11-11；作者：lq。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupScope {
    Servers,
    Databases,
}
impl GroupScope {
    /// 功能：统一协议所属工作区；时间：2026-11-11；作者：lq。
    pub fn for_protocol(protocol: Protocol) -> Self {
        match protocol {
            Protocol::Ssh | Protocol::Sftp => Self::Servers,
            _ => Self::Databases,
        }
    }
    /// 功能：数据库稳定分区键；时间：2026-11-11；作者：lq。
    pub fn key(self) -> &'static str {
        match self {
            Self::Servers => "servers",
            Self::Databases => "databases",
        }
    }
}

/// 功能：独立保存空分组，空名称仅作为系统“未分组”的折叠状态；时间：2026-11-11；作者：lq。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConnectionGroup {
    pub scope: GroupScope,
    pub name: String,
    pub collapsed: bool,
}

/// 功能：连接保存/导入时同事务注册文件夹，不覆盖其折叠状态；时间：2026-11-11；作者：lq。
pub(super) fn ensure_profile_group(
    tx: &Transaction<'_>,
    profile: &ConnectionProfile,
) -> Result<(), StoreError> {
    if !profile.group.is_empty() {
        tx.execute(
            "INSERT OR IGNORE INTO connection_groups(scope, name) VALUES (?1, ?2)",
            params![
                GroupScope::for_protocol(profile.protocol).key(),
                profile.group
            ],
        )?;
    }
    Ok(())
}

/// 功能：新名称去除首尾空白，拒绝空名称、保留名与不可见控制字符；时间：2026-11-11；作者：lq。
pub fn validate_group_name(name: &str) -> Result<String, StoreError> {
    let name = name.trim();
    if name.is_empty()
        || name == "未分组"
        || name.chars().count() > 256
        || name.chars().any(char::is_control)
    {
        return Err(StoreError::InvalidGroup);
    }
    Ok(name.to_owned())
}

/// 功能：事务内读取并验证配置，防止并发进程写入被旧快照覆盖；时间：2026-11-11；作者：lq。
fn transaction_profiles(tx: &Transaction<'_>) -> Result<Vec<ConnectionProfile>, StoreError> {
    let mut statement = tx.prepare("SELECT body FROM profiles ORDER BY rowid")?;
    let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
    rows.map(|row| {
        let profile: ConnectionProfile = serde_json::from_str(&row?)?;
        profile.validate()?;
        Ok(profile)
    })
    .collect()
}

impl ProfileStore {
    /// 功能：读取全部分组及收起状态，排序稳定；时间：2026-11-11；作者：lq。
    pub fn groups(&self) -> Result<Vec<ConnectionGroup>, StoreError> {
        let mut statement = self
            .0
            .prepare("SELECT scope, name, collapsed FROM connection_groups ORDER BY scope, name")?;
        let groups = statement.query_map([], |row| {
            let scope: String = row.get(0)?;
            Ok(ConnectionGroup {
                scope: if scope == "servers" {
                    GroupScope::Servers
                } else {
                    GroupScope::Databases
                },
                name: row.get(1)?,
                collapsed: row.get(2)?,
            })
        })?;
        groups
            .map(|group| group.map_err(StoreError::from))
            .collect()
    }

    /// 功能：独立创建空文件夹，同区同名不隐式合并；时间：2026-11-11；作者：lq。
    pub fn create_group(&self, scope: GroupScope, name: &str) -> Result<(), StoreError> {
        let name = validate_group_name(name)?;
        let count = self.0.execute(
            "INSERT OR IGNORE INTO connection_groups(scope, name) VALUES (?1, ?2)",
            params![scope.key(), name],
        )?;
        if count == 0 {
            return Err(StoreError::DuplicateGroup);
        }
        Ok(())
    }

    /// 功能：改名或删除时只更新此工作区成员，所有修改同事务提交；时间：2026-11-11；作者：lq。
    pub fn change_group(
        &mut self,
        scope: GroupScope,
        old: &str,
        new: Option<&str>,
    ) -> Result<(), StoreError> {
        if old.is_empty() {
            return Err(StoreError::InvalidGroup);
        }
        let new = new.map(validate_group_name).transpose()?;
        let tx = self.0.transaction()?;
        let mut profiles = transaction_profiles(&tx)?;
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM connection_groups WHERE scope=?1 AND name=?2)",
            params![scope.key(), old],
            |row| row.get(0),
        )?;
        if !exists {
            return Err(StoreError::MissingGroupTarget);
        }
        if let Some(new) = &new {
            if new == old {
                return Ok(());
            }
            let duplicate: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM connection_groups WHERE scope=?1 AND name=?2)",
                params![scope.key(), new],
                |row| row.get(0),
            )?;
            if duplicate {
                return Err(StoreError::DuplicateGroup);
            }
            tx.execute(
                "UPDATE connection_groups SET name=?3 WHERE scope=?1 AND name=?2",
                params![scope.key(), old, new],
            )?;
        } else {
            tx.execute(
                "DELETE FROM connection_groups WHERE scope=?1 AND name=?2",
                params![scope.key(), old],
            )?;
        }
        for profile in &mut profiles {
            if GroupScope::for_protocol(profile.protocol) == scope && profile.group == old {
                profile.group = new.clone().unwrap_or_default();
                tx.execute(
                    "UPDATE profiles SET body=?2 WHERE id=?1",
                    params![profile.id.to_string(), serde_json::to_string(profile)?],
                )?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// 功能：单独记录展开状态，普通分组不存在时不复活已删除分组；时间：2026-11-11；作者：lq。
    pub fn set_group_collapsed(
        &self,
        scope: GroupScope,
        name: &str,
        collapsed: bool,
    ) -> Result<(), StoreError> {
        if name.is_empty() {
            self.0.execute("INSERT INTO connection_groups(scope, name, collapsed) VALUES (?1, '', ?2) ON CONFLICT(scope, name) DO UPDATE SET collapsed=excluded.collapsed", params![scope.key(), collapsed])?;
        } else if self.0.execute(
            "UPDATE connection_groups SET collapsed=?3 WHERE scope=?1 AND name=?2",
            params![scope.key(), name, collapsed],
        )? == 0
        {
            return Err(StoreError::MissingGroupTarget);
        }
        Ok(())
    }

    /// 功能：按最新磁盘记录只修改归组，不覆盖名称、认证、隧道或凭据引用；时间：2026-11-11；作者：lq。
    pub fn move_to_group(&mut self, id: &str, target: &str) -> Result<(), StoreError> {
        let tx = self.0.transaction()?;
        let mut profile = transaction_profiles(&tx)?
            .into_iter()
            .find(|profile| profile.id.to_string() == id)
            .ok_or(StoreError::MissingGroupTarget)?;
        let scope = GroupScope::for_protocol(profile.protocol);
        if !target.is_empty() {
            let exists: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM connection_groups WHERE scope=?1 AND name=?2)",
                params![scope.key(), target],
                |row| row.get(0),
            )?;
            if !exists {
                return Err(StoreError::MissingGroupTarget);
            }
        }
        profile.group = target.into();
        // 功能：归组成功后自动展开目标文件夹，立即可见移动后的连接；时间：2026-11-11；作者：lq。
        tx.execute(
            "UPDATE connection_groups SET collapsed=0 WHERE scope=?1 AND name=?2",
            params![scope.key(), target],
        )?;
        tx.execute(
            "UPDATE profiles SET body=?2 WHERE id=?1",
            params![id, serde_json::to_string(&profile)?],
        )?;
        tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 功能：注入更新失败后，目录和全部连接同时回滚，杜绝半完成改名/删除；时间：2026-11-11；作者：lq。
    #[test]
    fn failed_group_update_rolls_back_folder_and_members() {
        let mut store = ProfileStore::open(":memory:").unwrap();
        let mut profile = ConnectionProfile::new(Protocol::Ssh);
        profile.name = "测试".into();
        profile.host = "localhost".into();
        profile.group = "原组".into();
        store.save(&profile).unwrap();
        store.0.execute_batch("CREATE TRIGGER reject_group_change BEFORE UPDATE ON profiles BEGIN SELECT RAISE(ABORT, 'test disk failure'); END;").unwrap();
        let before = store.groups().unwrap();
        assert!(
            store
                .change_group(GroupScope::Servers, "原组", Some("新组"))
                .is_err()
        );
        assert_eq!(store.groups().unwrap(), before);
        assert_eq!(store.list().unwrap(), vec![profile.clone()]);
        assert!(
            store
                .change_group(GroupScope::Servers, "原组", None)
                .is_err()
        );
        assert_eq!(store.groups().unwrap(), before);
        assert_eq!(store.list().unwrap(), vec![profile]);
    }

    /// 功能：验证改名/删除范围、空目录、重复名拒绝及连接全部非分组字段保留；时间：2026-11-11；作者：lq。
    #[test]
    fn scoped_group_operations_preserve_connections() {
        let mut store = ProfileStore::open(":memory:").unwrap();
        let mut ssh = ConnectionProfile::new(Protocol::Ssh);
        ssh.name = "中文服务器".into();
        ssh.host = "localhost".into();
        ssh.group = "开发".into();
        let mut db = ConnectionProfile::new(Protocol::MySql);
        db.name = "库".into();
        db.host = "localhost".into();
        db.group = "开发".into();
        store.save(&ssh).unwrap();
        store.save(&db).unwrap();
        store.create_group(GroupScope::Servers, "空组").unwrap();
        assert!(matches!(
            store.create_group(GroupScope::Servers, " 开发 "),
            Err(StoreError::DuplicateGroup)
        ));
        assert!(
            store
                .change_group(GroupScope::Servers, "开发", Some("空组"))
                .is_err()
        );
        assert_eq!(store.list().unwrap(), vec![ssh.clone(), db.clone()]);
        store
            .set_group_collapsed(GroupScope::Servers, "开发", true)
            .unwrap();
        store
            .change_group(GroupScope::Servers, "开发", Some("生产"))
            .unwrap();
        ssh.group = "生产".into();
        assert_eq!(store.list().unwrap(), vec![ssh.clone(), db.clone()]);
        assert!(
            store
                .groups()
                .unwrap()
                .iter()
                .any(|group| group.name == "生产" && group.collapsed)
        );
        store.move_to_group(&ssh.id.to_string(), "空组").unwrap();
        ssh.group = "空组".into();
        assert_eq!(store.list().unwrap(), vec![ssh.clone(), db.clone()]);
        store
            .change_group(GroupScope::Servers, "空组", None)
            .unwrap();
        ssh.group.clear();
        assert_eq!(store.list().unwrap(), vec![ssh, db]);
        assert!(store.move_to_group("missing", "").is_err());
        assert!(
            store
                .set_group_collapsed(GroupScope::Servers, "空组", true)
                .is_err()
        );
    }

    /// 功能：模拟旧版本库升级，验证空目录和“未分组”状态跨重开持久化；时间：2026-11-11；作者：lq。
    #[test]
    fn legacy_groups_migrate_and_empty_folders_survive_reopen() {
        let path = std::env::temp_dir().join(format!(
            "qds6-groups-{}-{}.sqlite3",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut store = ProfileStore::open(&path).unwrap();
        let mut profile = ConnectionProfile::new(Protocol::Sftp);
        profile.name = "旧连接".into();
        profile.host = "localhost".into();
        profile.group = "旧分组".into();
        store.save(&profile).unwrap();
        store.0.execute("DROP TABLE connection_groups", []).unwrap();
        drop(store);
        let store = ProfileStore::open(&path).unwrap();
        assert_eq!(store.groups().unwrap()[0].name, "旧分组");
        store.create_group(GroupScope::Servers, "空文件夹").unwrap();
        store
            .set_group_collapsed(GroupScope::Servers, "", true)
            .unwrap();
        drop(store);
        let store = ProfileStore::open(&path).unwrap();
        assert_eq!(store.groups().unwrap().len(), 3);
        assert!(
            store
                .groups()
                .unwrap()
                .iter()
                .any(|group| group.name.is_empty() && group.collapsed)
        );
        for name in ["", "  ", "未分组", "a\nb", &"字".repeat(257)] {
            assert!(validate_group_name(name).is_err());
        }
        drop(store);
        std::fs::remove_file(path).unwrap();
    }
}
