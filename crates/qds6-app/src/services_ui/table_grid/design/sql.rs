//! 功能：字段行草稿、数据库类型预置及批量结构 SQL 计划；时间：2026-11-12；作者：lq。
use super::*;
use qds6_services::database::GridColumn;

/// 功能：可直接编辑的字段属性，长度与小数位独立保存；时间：2026-11-12；作者：lq。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct FieldValues {
    pub name: String,
    pub data_type: String,
    pub length: String,
    pub scale: String,
    pub nullable: bool,
    pub default: String,
    pub comment: String,
}
impl FieldValues {
    /// 功能：只拆分数字类型参数，保留枚举、自定义类型与时间类型后缀；时间：2026-11-12；作者：lq。
    pub fn from_column(c: &GridColumn) -> Self {
        let (data_type, length, scale) = split_type(&c.data_type);
        Self {
            name: c.name.clone(),
            data_type,
            length,
            scale,
            nullable: c.nullable,
            default: c.default.clone().unwrap_or_default(),
            comment: c.comment.clone(),
        }
    }
    /// 功能：校验长度及精度并还原方言类型，参数保持数字以避免拼错类型；时间：2026-11-12；作者：lq。
    fn type_sql(&self) -> Result<String, String> {
        let base = self.data_type.trim();
        if base.is_empty() {
            return Err("请选择或填写字段类型".into());
        }
        let length = self.length.trim();
        let scale = self.scale.trim();
        for number in [length, scale] {
            if !number.is_empty() && number.parse::<u32>().is_err() {
                return Err("长度和小数位必须是非负整数".into());
            }
        }
        if !scale.is_empty() && length.is_empty() {
            return Err("填写小数位时需要同时填写长度 / 精度".into());
        }
        if length.is_empty() {
            return Ok(base.into());
        }
        if base.contains('(') {
            return Err("类型已有参数，请直接修改类型，或将参数移到长度 / 小数位列".into());
        }
        let parameters = if scale.is_empty() {
            length.to_string()
        } else {
            format!("{length},{scale}")
        };
        // 功能：PostgreSQL 时间精度放在时区限定词之前；时间：2026-11-12；作者：lq。
        for suffix in [
            " with time zone",
            " without time zone",
            " unsigned",
            " zerofill",
        ] {
            if let Some(head) = base.strip_suffix(suffix) {
                return Ok(format!("{head}({parameters}){suffix}"));
            }
        }
        Ok(format!("{base}({parameters})"))
    }
}

/// 功能：读取类型的数字参数，无法拆分的定义原样保留；时间：2026-11-12；作者：lq。
fn split_type(raw: &str) -> (String, String, String) {
    if let Some((head, tail)) = raw.split_once('(')
        && let Some((parameters, suffix)) = tail.split_once(')')
    {
        let parts: Vec<_> = parameters.split(',').map(str::trim).collect();
        if (1..=2).contains(&parts.len()) && parts.iter().all(|n| n.parse::<u32>().is_ok()) {
            return (
                format!("{head}{suffix}"),
                parts[0].into(),
                parts.get(1).copied().unwrap_or("").into(),
            );
        }
    }
    (raw.into(), String::new(), String::new())
}

/// 功能：完整原字段快照保留数据库附加属性，草稿不会覆盖原始定义；时间：2026-11-12；作者：lq。
#[derive(Clone)]
pub(super) struct Definition {
    pub values: FieldValues,
    pub raw_type: String,
    pub primary: bool,
    pub generated: bool,
    pub collation: Option<String>,
    pub extra: String,
}
impl From<&GridColumn> for Definition {
    fn from(c: &GridColumn) -> Self {
        Self {
            values: FieldValues::from_column(c),
            raw_type: c.data_type.clone(),
            primary: c.primary,
            generated: c.generated,
            collation: c.collation.clone(),
            extra: c.extra.clone(),
        }
    }
}

/// 功能：每一行独立记录修改或删除，供统一 SQL 预览；时间：2026-11-12；作者：lq。
pub(super) struct ColumnChange {
    pub original: Option<Definition>,
    pub values: FieldValues,
    pub deleted: bool,
}
impl ColumnChange {
    /// 功能：显式插入的新行参与草稿保护，删除新行时不产生 DDL；时间：2026-11-12；作者：lq。
    pub fn dirty(&self) -> bool {
        match &self.original {
            Some(old) => self.deleted || old.values != self.values,
            None => !self.deleted,
        }
    }
}

/// 功能：按 PostgreSQL / MySQL 提供常见类型，选中后允许继续输入自定义定义；时间：2026-11-12；作者：lq。
pub(super) fn type_presets(protocol: Protocol) -> &'static [&'static str] {
    if protocol == Protocol::PostgreSql {
        &[
            "smallint",
            "integer",
            "bigint",
            "numeric",
            "real",
            "double precision",
            "boolean",
            "character varying",
            "character",
            "text",
            "uuid",
            "json",
            "jsonb",
            "date",
            "time without time zone",
            "time with time zone",
            "timestamp without time zone",
            "timestamp with time zone",
            "interval",
            "bytea",
            "inet",
            "cidr",
            "macaddr",
            "bit",
            "bit varying",
            "text[]",
            "integer[]",
        ]
    } else {
        &[
            "tinyint",
            "smallint",
            "mediumint",
            "int",
            "bigint",
            "decimal",
            "float",
            "double",
            "bit",
            "boolean",
            "char",
            "varchar",
            "tinytext",
            "text",
            "mediumtext",
            "longtext",
            "json",
            "date",
            "time",
            "datetime",
            "timestamp",
            "year",
            "binary",
            "varbinary",
            "tinyblob",
            "blob",
            "mediumblob",
            "longblob",
            "enum('value')",
            "set('value')",
        ]
    }
}

/// 功能：为多个编辑行生成统一 DDL，原字段按变化属性更新并保留 MySQL 完整定义；时间：2026-11-12；作者：lq。
#[cfg(test)]
pub(super) fn generate_sql(
    protocol: Protocol,
    table: &Table,
    changes: &[ColumnChange],
) -> Result<String, String> {
    generate_plan(protocol, table, changes, true)
}

/// 功能：直接保存使用无事务关键字的计划，由服务统一管理事务和失败回滚；时间：2026-11-13；作者：lq。
pub(super) fn save_sql(
    protocol: Protocol,
    table: &Table,
    changes: &[ColumnChange],
) -> Result<String, String> {
    generate_plan(protocol, table, changes, false)
}

/// 功能：共用字段校验及 DDL 生成，按调用方式决定是否输出事务包装；时间：2026-11-13；作者：lq。
fn generate_plan(
    protocol: Protocol,
    table: &Table,
    changes: &[ColumnChange],
    wrap: bool,
) -> Result<String, String> {
    let quote = |name: &str| qds6_services::database::identifier(protocol, name);
    let target = format!("{}.{}", quote(&table.schema)?, quote(&table.name)?);
    let literal = |text: &str| {
        format!(
            "'{}'",
            if protocol == Protocol::PostgreSql {
                text.replace('\'', "''")
            } else {
                text.replace('\\', "\\\\").replace('\'', "''")
            }
        )
    };
    let mut names = BTreeSet::new();
    for c in changes.iter().filter(|c| !c.deleted) {
        if c.original.is_none() && !c.dirty() {
            continue;
        }
        quote(&c.values.name)?;
        let key = if protocol == Protocol::PostgreSql {
            c.values.name.clone()
        } else {
            c.values.name.to_lowercase()
        };
        if !names.insert(key) {
            return Err(format!("字段名称重复：{}", c.values.name));
        }
        if c.original
            .as_ref()
            .is_some_and(|old| old.values.name != c.values.name)
            && changes
                .iter()
                .filter(|other| !other.deleted)
                .filter_map(|other| other.original.as_ref())
                .any(|old| old.values.name == c.values.name)
        {
            return Err("新字段名仍被原字段占用，请分次保存重命名".into());
        }
    }
    let mut sql = Vec::new();
    // 功能：先删除标记字段，允许随后添加同名的新字段；时间：2026-11-12；作者：lq。
    for c in changes.iter().filter(|c| c.deleted) {
        if let Some(old) = &c.original {
            sql.push(format!(
                "ALTER TABLE {target} DROP COLUMN {};",
                quote(&old.values.name)?
            ));
        }
    }
    for c in changes.iter().filter(|c| !c.deleted && c.dirty()) {
        let v = &c.values;
        let field = quote(&v.name)?;
        let old = c.original.as_ref();
        if old.is_some_and(|old| old.generated) {
            return Err(format!("{} 是生成列，请通过 SQL 编辑完整定义", v.name));
        }
        let type_changed = old.is_none_or(|o| {
            o.values.data_type != v.data_type
                || o.values.length != v.length
                || o.values.scale != v.scale
        });
        let data_type = if type_changed {
            v.type_sql()?
        } else {
            old.unwrap().raw_type.clone()
        };
        if protocol == Protocol::PostgreSql {
            if let Some(old) = old {
                let previous = quote(&old.values.name)?;
                if old.values.name != v.name {
                    sql.push(format!(
                        "ALTER TABLE {target} RENAME COLUMN {previous} TO {field};"
                    ));
                }
                if type_changed {
                    sql.push(format!(
                        "ALTER TABLE {target} ALTER COLUMN {field} TYPE {data_type};"
                    ));
                }
                if old.values.nullable != v.nullable {
                    sql.push(format!(
                        "ALTER TABLE {target} ALTER COLUMN {field} {} NOT NULL;",
                        if v.nullable { "DROP" } else { "SET" }
                    ));
                }
                if old.values.default != v.default {
                    sql.push(format!(
                        "ALTER TABLE {target} ALTER COLUMN {field} {};",
                        if v.default.is_empty() {
                            "DROP DEFAULT".into()
                        } else {
                            format!("SET DEFAULT {}", v.default)
                        }
                    ));
                }
            } else {
                sql.push(format!(
                    "ALTER TABLE {target} ADD COLUMN {field} {data_type}{}{};",
                    if v.nullable { "" } else { " NOT NULL" },
                    if v.default.is_empty() {
                        String::new()
                    } else {
                        format!(" DEFAULT {}", v.default)
                    }
                ));
            }
            if old.is_none_or(|old| old.values.comment != v.comment)
                && (old.is_some() || !v.comment.is_empty())
            {
                sql.push(format!(
                    "COMMENT ON COLUMN {target}.{field} IS {};",
                    literal(&v.comment)
                ));
            }
        } else {
            let action = if let Some(old) = old {
                format!("CHANGE COLUMN {}", quote(&old.values.name)?)
            } else {
                "ADD COLUMN".into()
            };
            let collation = old
                .and_then(|old| old.collation.as_deref())
                .filter(|_| {
                    !type_changed
                        || ["char", "text", "enum", "set"]
                            .iter()
                            .any(|part| data_type.to_lowercase().contains(part))
                })
                .map(|c| quote(c).map(|c| format!(" COLLATE {c}")))
                .transpose()?
                .unwrap_or_default();
            let extra = old
                .map(|old| old.extra.as_str())
                .filter(|s| !s.is_empty())
                .map(|s| format!(" {s}"))
                .unwrap_or_default();
            sql.push(format!("ALTER TABLE {target} {action} {field} {data_type}{collation} {}{} COMMENT {}{extra};", if v.nullable { "NULL" } else { "NOT NULL" }, if v.default.is_empty() { String::new() } else { format!(" DEFAULT {}", v.default) }, literal(&v.comment)));
        }
    }
    if sql.is_empty() {
        return Err("没有待生成的字段变更".into());
    }
    if wrap && protocol == Protocol::PostgreSql && sql.len() > 1 {
        sql.insert(0, "BEGIN;".into());
        sql.push("COMMIT;".into());
    }
    Ok(format!(
        "-- 功能：表字段批量变更预览；时间：2026-11-12；作者：lq。\n{}",
        sql.join("\n")
    ))
}

// 功能：验证批量 DDL 的方言差异、参数重组与草稿保护；时间：2026-11-12；作者：lq。
#[cfg(test)]
mod tests {
    use super::{
        ColumnChange, Definition, FieldValues, Protocol, Table, generate_sql, save_sql, split_type,
    };
    use qds6_core::{ConnectionProfile, TlsMode};
    use qds6_services::database::DatabaseSession;
    /// 功能：构造受控字段定义，不访问用户数据库；时间：2026-11-12；作者：lq。
    fn existing(name: &str, raw_type: &str) -> ColumnChange {
        let (data_type, length, scale) = split_type(raw_type);
        let values = FieldValues {
            name: name.into(),
            data_type,
            length,
            scale,
            nullable: true,
            ..Default::default()
        };
        ColumnChange {
            original: Some(Definition {
                values: values.clone(),
                raw_type: raw_type.into(),
                primary: false,
                generated: false,
                collation: None,
                extra: String::new(),
            }),
            values,
            deleted: false,
        }
    }
    /// 功能：生成测试表目标，无需真实连接即可检查 SQL；时间：2026-11-12；作者：lq。
    fn table() -> Table {
        Table {
            schema: "public".into(),
            name: "sample".into(),
            kind: "BASE TABLE".into(),
        }
    }
    #[test]
    fn split_and_rebuild_parameterized_types() {
        for raw in [
            "numeric(12,2)",
            "character varying(40)",
            "timestamp(6) with time zone",
            "int(11) unsigned",
            "enum('a','b')",
            "text[]",
        ] {
            let c = existing("value", raw);
            assert_eq!(c.values.type_sql().unwrap(), raw);
        }
        let mut c = existing("value", "numeric(12,2)");
        c.values.length = "wrong".into();
        assert!(c.values.type_sql().is_err());
        c.values.length.clear();
        assert!(c.values.type_sql().is_err());
    }
    #[test]
    fn postgres_multiple_edits_only_change_needed_properties() {
        let mut a = existing("name", "character varying(40)");
        a.values.comment = "用户's 昵称".into();
        let mut b = existing("balance", "numeric(10,2)");
        b.values.name = "amount".into();
        b.values.length = "12".into();
        b.values.nullable = false;
        b.values.default = "0".into();
        let sql = generate_sql(Protocol::PostgreSql, &table(), &[a, b]).unwrap();
        assert!(sql.contains("BEGIN;") && sql.ends_with("COMMIT;"));
        assert!(sql.contains("IS '用户''s 昵称'"));
        assert!(!sql.contains("\"name\" TYPE"));
        assert!(sql.contains("RENAME COLUMN \"balance\" TO \"amount\""));
        assert!(
            sql.contains("TYPE numeric(12,2)")
                && sql.contains("SET NOT NULL")
                && sql.contains("SET DEFAULT 0")
        );
    }
    #[test]
    fn blank_new_row_is_protected_and_deleted_new_row_has_no_ddl() {
        let blank = ColumnChange {
            original: None,
            values: FieldValues {
                data_type: "text".into(),
                nullable: true,
                ..Default::default()
            },
            deleted: false,
        };
        assert!(blank.dirty());
        assert!(generate_sql(Protocol::PostgreSql, &table(), &[blank]).is_err());
        let mut a = existing("new", "text");
        a.original = None;
        a.deleted = true;
        assert!(!a.dirty());
        assert!(generate_sql(Protocol::MySql, &table(), &[a]).is_err());
    }
    #[test]
    fn duplicate_and_conflicting_rename_are_rejected() {
        let a = existing("id", "integer");
        let mut b = existing("other", "integer");
        b.values.name = "id".into();
        assert!(generate_sql(Protocol::PostgreSql, &table(), &[a, b]).is_err());
        let mut a = existing("a", "integer");
        let mut b = existing("b", "integer");
        a.values.name = "b".into();
        b.values.name = "a".into();
        assert!(generate_sql(Protocol::PostgreSql, &table(), &[a, b]).is_err());
    }
    #[test]
    fn mysql_preserves_collation_extra_and_full_type() {
        let mut a = existing("id", "int unsigned");
        a.original.as_mut().unwrap().extra = "auto_increment".into();
        a.values.comment = "主键".into();
        let mut b = existing("name", "varchar(64)");
        b.original.as_mut().unwrap().collation = Some("utf8mb4_bin".into());
        b.values.comment = "备注".into();
        let sql = generate_sql(Protocol::MySql, &table(), &[a, b]).unwrap();
        assert!(sql.contains("int unsigned") && sql.contains("auto_increment"));
        assert!(sql.contains("varchar(64) COLLATE `utf8mb4_bin`"));
        assert!(!sql.contains("BEGIN;"));
    }
    #[test]
    fn generated_columns_are_protected_and_deletions_are_reversible() {
        let mut a = existing("computed", "integer");
        a.original.as_mut().unwrap().generated = true;
        a.values.comment = "changed".into();
        assert!(generate_sql(Protocol::PostgreSql, &table(), &[a]).is_err());
        let mut b = existing("obsolete", "text");
        b.deleted = true;
        assert!(
            generate_sql(Protocol::PostgreSql, &table(), &[b])
                .unwrap()
                .contains("DROP COLUMN \"obsolete\";")
        );
        let b = existing("obsolete", "text");
        assert!(generate_sql(Protocol::PostgreSql, &table(), &[b]).is_err());
    }
    /// 功能：只连接本轮独立容器，执行设计器实际生成的增改删 SQL 并重读元数据；时间：2026-11-12；作者：lq。
    #[test]
    #[ignore = "requires isolated PostgreSQL 25433 and MySQL 23307"]
    fn real_database_design_roundtrip() {
        for (protocol, port, schema) in [
            (Protocol::PostgreSql, 25433, "public"),
            (Protocol::MySql, 23307, "qds6"),
        ] {
            let mut profile = ConnectionProfile::new(protocol);
            profile.name = "行内设计隔离验收".into();
            profile.host = "127.0.0.1".into();
            profile.port = port;
            profile.username = "qds6".into();
            profile.database = "qds6".into();
            profile.db.tls = TlsMode::Disabled;
            let db = DatabaseSession::connect(&profile, "qds6-fixture-only", None).unwrap();
            let table = Table {
                schema: schema.into(),
                name: format!("design_{}", uuid::Uuid::new_v4().simple()),
                kind: "BASE TABLE".into(),
            };
            let target = format!("{schema}.{}", table.name);
            db.query(&format!(
                "CREATE TABLE {target}(id INT PRIMARY KEY, label VARCHAR(40), obsolete TEXT)"
            ))
            .unwrap();
            // 功能：先建立旧表结果缓存，验证 DDL 后重新读取不会触发 cached plan 错误；时间：2026-11-13；作者：lq。
            db.browse_page(&table, &Default::default()).unwrap();
            let columns = db.grid_columns(&table).unwrap();
            let mut changes: Vec<_> = columns
                .iter()
                .map(|c| ColumnChange {
                    original: Some(Definition::from(c)),
                    values: FieldValues::from_column(c),
                    deleted: false,
                })
                .collect();
            changes[1].values.name = "title".into();
            changes[1].values.length = "80".into();
            changes[1].values.comment = "姓名's 备注".into();
            changes[1].values.nullable = false;
            changes[2].deleted = true;
            changes.push(ColumnChange {
                original: None,
                values: FieldValues {
                    name: "amount".into(),
                    data_type: if protocol == Protocol::PostgreSql {
                        "numeric"
                    } else {
                        "decimal"
                    }
                    .into(),
                    length: "12".into(),
                    scale: "2".into(),
                    nullable: false,
                    default: "0".into(),
                    comment: "金额".into(),
                },
                deleted: false,
            });
            // 功能：真实服务验证保存事务隔离、失败回滚以及成功后不会残留手动事务；时间：2026-11-13；作者：lq。
            let sql = save_sql(protocol, &table, &changes).unwrap();
            db.query("BEGIN").unwrap();
            assert!(db.save_structure(&table, &columns, &sql).is_err());
            db.query("ROLLBACK").unwrap();
            if protocol == Protocol::PostgreSql {
                let invalid = format!(
                    "ALTER TABLE {target} ADD COLUMN rolled_back INT; SELECT qds6_missing_function();"
                );
                assert!(db.save_structure(&table, &columns, &invalid).is_err());
                assert_eq!(db.grid_columns(&table).unwrap().len(), columns.len());
            }
            db.save_structure(&table, &columns, &sql).unwrap();
            assert!(db.save_structure(&table, &columns, &sql).is_err());
            db.browse_page(&table, &Default::default()).unwrap();
            let fresh = db.grid_columns(&table).unwrap();
            assert_eq!(fresh.len(), 3);
            assert_eq!(fresh[1].name, "title");
            assert!(!fresh[1].nullable);
            assert!(fresh[1].data_type.contains("80"));
            assert_eq!(fresh[1].comment, "姓名's 备注");
            assert_eq!(fresh[2].name, "amount");
            assert!(fresh[2].data_type.contains("12,2"));
            assert_eq!(fresh[2].comment, "金额");
            let unchanged: Vec<_> = fresh
                .iter()
                .map(|c| ColumnChange {
                    original: Some(Definition::from(c)),
                    values: FieldValues::from_column(c),
                    deleted: false,
                })
                .collect();
            assert!(generate_sql(protocol, &table, &unchanged).is_err());
            db.query(&format!("DROP TABLE {target}")).unwrap();
        }
    }
}
