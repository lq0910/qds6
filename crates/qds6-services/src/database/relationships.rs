//! 功能：只读查询 MySQL/PostgreSQL 的一跳外键和有序字段映射；时间：2026-11-28；作者：lq。

use super::{Adapter, DatabaseSession, Table, safe_error};
use std::{collections::BTreeMap, fmt, sync::atomic::Ordering};

/// 功能：按 schema 和名称区分图中的表，例如两个不同 schema 下的同名表；时间：2026-11-28；作者：lq。
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct TableName {
    pub schema: String,
    pub name: String,
}

impl From<&Table> for TableName {
    fn from(table: &Table) -> Self {
        Self {
            schema: table.schema.clone(),
            name: table.name.clone(),
        }
    }
}

impl fmt::Display for TableName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.schema, self.name)
    }
}

/// 功能：一个复合外键以声明顺序保留每一对列；时间：2026-11-28；作者：lq。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForeignKeyColumn {
    pub source: String,
    pub target: String,
}

/// 功能：外键方向始终由引用表指向被引用表，自关联也使用同一模型；时间：2026-11-28；作者：lq。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForeignKey {
    pub name: String,
    pub source: TableName,
    pub target: TableName,
    pub columns: Vec<ForeignKeyColumn>,
    pub update_rule: String,
    pub delete_rule: String,
}

/// 功能：关系图快照仅含所选表及直接相关约束，不含业务行；时间：2026-11-28；作者：lq。
#[derive(Debug)]
pub struct TableRelationships {
    pub table: TableName,
    pub keys: Vec<ForeignKey>,
}

/// 功能：数据库行携带字段位置，确保同名约束及复合列的分组正确；时间：2026-11-28；作者：lq。
type KeyRow = (
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    i64,
);
const MAX_COLUMNS: usize = 4096;

// 功能：使用 PostgreSQL 约束 OID 与字段序号配对，消除同名约束歧义；时间：2026-11-28；作者：lq。
const POSTGRES: &str = "
    WITH focus AS (
        SELECT t.oid FROM pg_catalog.pg_class t
        JOIN pg_catalog.pg_namespace s ON s.oid = t.relnamespace
        WHERE s.nspname = $1 AND t.relname = $2
    )
    SELECT fk.conname::text, sn.nspname::text, src.relname::text,
           tn.nspname::text, dst.relname::text, sc.attname::text, tc.attname::text,
           fk.confupdtype::text, fk.confdeltype::text, cols.position
    FROM focus
    JOIN pg_catalog.pg_constraint fk ON fk.contype = 'f'
        AND (fk.conrelid = focus.oid OR fk.confrelid = focus.oid)
    JOIN pg_catalog.pg_class src ON src.oid = fk.conrelid
    JOIN pg_catalog.pg_namespace sn ON sn.oid = src.relnamespace
    JOIN pg_catalog.pg_class dst ON dst.oid = fk.confrelid
    JOIN pg_catalog.pg_namespace tn ON tn.oid = dst.relnamespace
    CROSS JOIN LATERAL unnest(fk.conkey, fk.confkey)
        WITH ORDINALITY AS cols(source_number, target_number, position)
    JOIN pg_catalog.pg_attribute sc ON sc.attrelid = src.oid AND sc.attnum = cols.source_number
    JOIN pg_catalog.pg_attribute tc ON tc.attrelid = dst.oid AND tc.attnum = cols.target_number
    LIMIT $3";

// 功能：MySQL 保留真实引用列，并显式将二进制排序的系统目录文本转为 UTF-8；时间：2026-11-28；作者：lq。
const MYSQL: &str = "
    SELECT CONVERT(k.CONSTRAINT_NAME USING utf8mb4) COLLATE utf8mb4_general_ci,
           CONVERT(k.TABLE_SCHEMA USING utf8mb4) COLLATE utf8mb4_general_ci,
           CONVERT(k.TABLE_NAME USING utf8mb4) COLLATE utf8mb4_general_ci,
           CONVERT(k.REFERENCED_TABLE_SCHEMA USING utf8mb4) COLLATE utf8mb4_general_ci,
           CONVERT(k.REFERENCED_TABLE_NAME USING utf8mb4) COLLATE utf8mb4_general_ci,
           CONVERT(k.COLUMN_NAME USING utf8mb4) COLLATE utf8mb4_general_ci,
           CONVERT(k.REFERENCED_COLUMN_NAME USING utf8mb4) COLLATE utf8mb4_general_ci,
           CONVERT(r.UPDATE_RULE USING utf8mb4) COLLATE utf8mb4_general_ci,
           CONVERT(r.DELETE_RULE USING utf8mb4) COLLATE utf8mb4_general_ci,
           CAST(k.ORDINAL_POSITION AS SIGNED)
    FROM information_schema.KEY_COLUMN_USAGE k
    JOIN information_schema.REFERENTIAL_CONSTRAINTS r
      ON r.CONSTRAINT_SCHEMA = k.CONSTRAINT_SCHEMA
     AND r.CONSTRAINT_NAME = k.CONSTRAINT_NAME AND r.TABLE_NAME = k.TABLE_NAME
    WHERE k.REFERENCED_TABLE_NAME IS NOT NULL
      AND ((k.TABLE_SCHEMA = ? AND k.TABLE_NAME = ?)
        OR (k.REFERENCED_TABLE_SCHEMA = ? AND k.REFERENCED_TABLE_NAME = ?))
    LIMIT ?";

impl DatabaseSession {
    /// 功能：在当前会话后台读取有界的外键目录，不执行用户 SQL 或扫描业务数据；时间：2026-11-28；作者：lq。
    pub fn table_relationships(&self, table: &Table) -> Result<TableRelationships, String> {
        if self.closed.load(Ordering::SeqCst) {
            return Err("数据库连接已失效，请重新连接".into());
        }
        let focus = TableName::from(table);
        let mut connection = self.connection.try_lock().map_err(|_| "已有查询正在执行")?;
        let limit = (MAX_COLUMNS + 1) as i64;
        let rows: Vec<KeyRow> = self
            .runtime
            .as_ref()
            .expect("live runtime")
            .block_on(async {
                match &mut *connection {
                    Adapter::Pg(conn) => {
                        sqlx::query_as(POSTGRES)
                            .bind(&focus.schema)
                            .bind(&focus.name)
                            .bind(limit)
                            .fetch_all(conn)
                            .await
                    }
                    Adapter::MySql(conn) => {
                        sqlx::query_as(MYSQL)
                            .bind(&focus.schema)
                            .bind(&focus.name)
                            .bind(&focus.schema)
                            .bind(&focus.name)
                            .bind(limit)
                            .fetch_all(conn)
                            .await
                    }
                }
            })
            .map_err(safe_error)?;
        assemble(focus, rows)
    }
}

/// 功能：按表对和约束名去重分组，以服务端列序重建复合关系；时间：2026-11-28；作者：lq。
fn assemble(table: TableName, rows: Vec<KeyRow>) -> Result<TableRelationships, String> {
    if rows.len() > MAX_COLUMNS {
        return Err("此表的外键字段映射超过 4096 项，无法完整显示关系图".into());
    }
    let mut groups: BTreeMap<_, (_, _, BTreeMap<_, _>)> = BTreeMap::new();
    for (
        name,
        source_schema,
        source_name,
        target_schema,
        target_name,
        source_column,
        target_column,
        update,
        delete,
        position,
    ) in rows
    {
        let source = TableName {
            schema: source_schema,
            name: source_name,
        };
        let target = TableName {
            schema: target_schema,
            name: target_name,
        };
        if source != table && target != table {
            continue;
        }
        let (_, _, columns) = groups
            .entry((source, target, name))
            .or_insert_with(|| (action(update), action(delete), BTreeMap::new()));
        columns.insert(
            position,
            ForeignKeyColumn {
                source: source_column,
                target: target_column,
            },
        );
    }
    let keys = groups
        .into_iter()
        .map(
            |((source, target, name), (update_rule, delete_rule, columns))| ForeignKey {
                name,
                source,
                target,
                update_rule,
                delete_rule,
                columns: columns.into_values().collect(),
            },
        )
        .collect();
    Ok(TableRelationships { table, keys })
}

/// 功能：将 PostgreSQL 动作代码统一为 MySQL 同样的 SQL 关键字；时间：2026-11-28；作者：lq。
fn action(value: String) -> String {
    match value.as_str() {
        "a" => "NO ACTION",
        "r" => "RESTRICT",
        "c" => "CASCADE",
        "n" => "SET NULL",
        "d" => "SET DEFAULT",
        _ => return value,
    }
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 功能：验证无序复合键、同名约束、跨 schema 与自关联均保留；时间：2026-11-28；作者：lq。
    #[test]
    fn assembles_ordered_columns_and_separates_same_named_constraints() {
        let row = |schema: &str, table: &str, source: &str, target: &str, ordinal| {
            (
                "shared_fk".into(),
                schema.into(),
                table.into(),
                "app".into(),
                "orders".into(),
                source.into(),
                target.into(),
                "c".into(),
                "n".into(),
                ordinal,
            )
        };
        let focus = TableName {
            schema: "app".into(),
            name: "orders".into(),
        };
        let result = assemble(
            focus.clone(),
            vec![
                row("billing", "detail", "order_id", "id", 2),
                row("audit", "detail", "order_id", "id", 1),
                row("billing", "detail", "tenant", "tenant_id", 1),
                row("app", "orders", "previous_id", "id", 1),
            ],
        )
        .unwrap();
        assert_eq!(result.keys.len(), 3);
        assert_eq!(result.keys[0].source, focus);
        assert_eq!(result.keys[2].columns[0].target, "tenant_id");
        assert_eq!(result.keys[2].columns[1].target, "id");
        assert_eq!(result.keys[2].update_rule, "CASCADE");
        assert_eq!(result.keys[2].delete_rule, "SET NULL");
    }

    /// 功能：在专用回环数据库验证入边、出边、自关联、复合外键和级联规则；时间：2026-11-28；作者：lq。
    #[test]
    #[ignore = "仅针对显式启动的本机隔离数据库"]
    fn relationship_fixture() {
        use crate::database::identifier;
        use qds6_core::{ConnectionProfile, Protocol, TlsMode};

        let (protocol, port, schema) = match std::env::var("QDS6_RELATIONSHIP_FIXTURE").as_deref() {
            Ok("postgres") => (Protocol::PostgreSql, 25433, "public"),
            Ok("mysql") => (Protocol::MySql, 23307, "qds6"),
            _ => panic!("set QDS6_RELATIONSHIP_FIXTURE=postgres or mysql"),
        };
        let mut profile = ConnectionProfile::new(protocol);
        profile.name = "relationship fixture".into();
        profile.host = "127.0.0.1".into();
        profile.port = port;
        profile.username = "qds6".into();
        profile.database = "qds6".into();
        profile.db.tls = TlsMode::Disabled;
        let db = DatabaseSession::connect(&profile, "qds6-fixture-only", None).unwrap();
        let tag = uuid::Uuid::new_v4().simple().to_string();
        let table = Table {
            schema: schema.into(),
            name: format!("order_{}", &tag[..10]),
            kind: "BASE TABLE".into(),
        };
        let name = |value: &str| format!("{schema}.{}", identifier(protocol, value).unwrap());
        let detail = name(&format!("detail_{}", &tag[..10]));
        let parent = name(&format!("parent_{}", &tag[..10]));
        let order = name(&table.name);
        let parent_key = format!("parent_fk_{}", &tag[..10]);
        db.query(&format!("CREATE TABLE {parent} (tenant_id INT NOT NULL, id INT NOT NULL, PRIMARY KEY (tenant_id, id))")).unwrap();
        db.query(&format!("CREATE TABLE {order} (tenant_id INT NOT NULL, id INT NOT NULL, parent_id INT, previous_id INT, PRIMARY KEY (tenant_id, id), CONSTRAINT {parent_key} FOREIGN KEY (tenant_id, parent_id) REFERENCES {parent} (tenant_id, id) ON UPDATE CASCADE ON DELETE CASCADE, FOREIGN KEY (tenant_id, previous_id) REFERENCES {order} (tenant_id, id))")).unwrap();
        db.query(&format!("CREATE TABLE {detail} (id INT PRIMARY KEY, tenant_id INT, order_id INT, FOREIGN KEY (tenant_id, order_id) REFERENCES {order} (tenant_id, id))")).unwrap();
        let result = db.table_relationships(&table);
        for object in [&detail, &order, &parent] {
            db.query(&format!("DROP TABLE {object}")).unwrap();
        }
        let result = result.unwrap();
        assert_eq!(result.keys.len(), 3);
        let outgoing = result
            .keys
            .iter()
            .find(|key| key.name == parent_key)
            .unwrap();
        assert_eq!(
            outgoing
                .columns
                .iter()
                .map(|column| (column.source.as_str(), column.target.as_str()))
                .collect::<Vec<_>>(),
            [("tenant_id", "tenant_id"), ("parent_id", "id")]
        );
        assert_eq!(
            (&outgoing.update_rule[..], &outgoing.delete_rule[..]),
            ("CASCADE", "CASCADE")
        );
        assert!(
            result
                .keys
                .iter()
                .any(|key| key.source == result.table && key.target == result.table)
        );
        assert!(
            result
                .keys
                .iter()
                .any(|key| key.source != result.table && key.target == result.table)
        );
    }
}
