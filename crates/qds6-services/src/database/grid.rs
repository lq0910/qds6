//! 功能：表浏览元数据、分页筛选与原子网格写入，支持复合主键和并发检查；时间：2026-11-11；作者：lq。
use super::*;
use sqlx::QueryBuilder;

/// 功能：字段默认值、生成列及主键身份来自服务端；时间：2026-11-11；作者：lq。
#[derive(Clone, Debug, PartialEq)]
pub struct GridColumn {
    pub name: String,
    pub data_type: String,
    // 功能：保留数据库字段注释，供数据表头与设计器显示；时间：2026-11-12；作者：lq。
    pub comment: String,
    // 功能：MySQL 完整定义保留排序规则和额外属性，避免修改备注时重置；时间：2026-11-12；作者：lq。
    pub collation: Option<String>,
    pub extra: String,
    pub nullable: bool,
    pub default: Option<String>,
    pub generated: bool,
    pub primary: bool,
    pub writable: bool,
    pub(super) cast: String,
}

/// 功能：每张表独立持有已提交的分页与筛选条件；时间：2026-11-11；作者：lq。
#[derive(Clone, Default)]
pub struct BrowseRequest {
    pub page: usize,
    pub filters: Vec<GridFilter>,
    pub match_any: bool,
    pub order: Vec<(String, bool)>,
}
/// 功能：结构化筛选运算符，SQL 片段仅来自封闭枚举；时间：2026-11-12；作者：lq。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FilterOp {
    #[default]
    Contains,
    Equal,
    NotEqual,
    Greater,
    GreaterEqual,
    Less,
    LessEqual,
    StartsWith,
    EndsWith,
    IsNull,
    IsNotNull,
}
impl FilterOp {
    pub const ALL: [Self; 11] = [
        Self::Contains,
        Self::Equal,
        Self::NotEqual,
        Self::Greater,
        Self::GreaterEqual,
        Self::Less,
        Self::LessEqual,
        Self::StartsWith,
        Self::EndsWith,
        Self::IsNull,
        Self::IsNotNull,
    ];
    /// 功能：界面和查询共用明确的筛选含义；时间：2026-11-12；作者：lq。
    pub fn label(self) -> &'static str {
        match self {
            Self::Contains => "包含",
            Self::Equal => "等于",
            Self::NotEqual => "不等于",
            Self::Greater => "大于",
            Self::GreaterEqual => "大于等于",
            Self::Less => "小于",
            Self::LessEqual => "小于等于",
            Self::StartsWith => "开头是",
            Self::EndsWith => "结尾是",
            Self::IsNull => "为 NULL",
            Self::IsNotNull => "非 NULL",
        }
    }
    /// 功能：NULL 判断禁用输入但保留输入草稿；时间：2026-11-12；作者：lq。
    pub fn needs_value(self) -> bool {
        !matches!(self, Self::IsNull | Self::IsNotNull)
    }
}
/// 功能：字段和值分别校验及绑定，空字符串可作为精确筛选值；时间：2026-11-12；作者：lq。
#[derive(Clone)]
pub struct GridFilter {
    pub column: String,
    pub op: FilterOp,
    pub value: String,
}
pub const PAGE_SIZE: usize = 200;

/// 功能：空表仍返回完整字段信息，下一页通过多取一行判断；时间：2026-11-11；作者：lq。
#[derive(Clone)]
pub struct TablePage {
    pub columns: Vec<GridColumn>,
    pub data: ResultSet,
    pub has_next: bool,
    pub editable: bool,
    pub sql: String,
    // 功能：详情侧栏保留真实索引定义；时间：2026-11-11；作者：lq。
    pub indexes: ResultSet,
}

/// 功能：None 值使用数据库默认值，Cell::Null 显式写 NULL；原记录用于并发检查；时间：2026-11-11；作者：lq。
#[derive(Clone, Debug)]
pub struct RowChange {
    pub original: Option<Vec<Cell>>,
    pub values: Vec<Option<Cell>>,
    pub delete: bool,
}

impl DatabaseSession {
    /// 功能：读取当前数据库和模式，查询工具栏不推测连接上下文；时间：2026-11-11；作者：lq。
    pub fn connection_context(&self) -> Result<(String, String), String> {
        let sql = if self.profile.protocol == Protocol::PostgreSql {
            "SELECT current_database(), current_schema()"
        } else {
            "SELECT DATABASE(), DATABASE()"
        };
        let result = self.query(sql)?;
        let row = result
            .sets
            .iter()
            .find_map(|s| s.rows.first())
            .ok_or("未能读取数据库上下文")?;
        let text = |c: &Cell| match c {
            Cell::Text(v) => v.clone(),
            Cell::Binary(v) => String::from_utf8_lossy(v).into_owned(),
            Cell::Null => String::new(),
        };
        Ok((text(&row[0]), text(&row[1])))
    }

    /// 功能：查询真实字段和复合主键，生成列与不支持的类型只读；时间：2026-11-11；作者：lq。
    pub fn grid_columns(&self, table: &Table) -> Result<Vec<GridColumn>, String> {
        let schema = literal(&table.schema);
        let name = literal(&table.name);
        let sql = if self.profile.protocol == Protocol::PostgreSql {
            format!(
                "SELECT c.column_name, pg_catalog.format_type(a.atttypid,a.atttypmod), c.is_nullable, c.column_default, c.is_generated, c.is_identity, c.udt_schema, c.udt_name, CASE WHEN EXISTS (SELECT 1 FROM information_schema.table_constraints t JOIN information_schema.key_column_usage k ON t.constraint_catalog=k.constraint_catalog AND t.constraint_schema=k.constraint_schema AND t.constraint_name=k.constraint_name AND t.table_name=k.table_name WHERE t.constraint_type='PRIMARY KEY' AND t.table_schema=c.table_schema AND t.table_name=c.table_name AND k.column_name=c.column_name) THEN 'YES' ELSE 'NO' END, COALESCE(pg_catalog.col_description(cl.oid, a.attnum), '') FROM information_schema.columns c JOIN pg_catalog.pg_namespace ns ON ns.nspname=c.table_schema JOIN pg_catalog.pg_class cl ON cl.relnamespace=ns.oid AND cl.relname=c.table_name JOIN pg_catalog.pg_attribute a ON a.attrelid=cl.oid AND a.attname=c.column_name AND a.attnum>0 AND NOT a.attisdropped WHERE c.table_schema={schema} AND c.table_name={name} ORDER BY c.ordinal_position"
            )
        } else {
            format!(
                "SELECT column_name, column_type, is_nullable, column_default, extra, column_key, column_comment, collation_name FROM information_schema.columns WHERE table_schema={schema} AND table_name={name} ORDER BY ordinal_position"
            )
        };
        let result = self.query(&sql)?;
        let mut columns = Vec::new();
        for row in result.sets.into_iter().flat_map(|s| s.rows) {
            let text = |i: usize| match &row[i] {
                Cell::Null => String::new(),
                Cell::Binary(v) => String::from_utf8_lossy(v).into_owned(),
                cell => cell.display(),
            };
            let pg = self.profile.protocol == Protocol::PostgreSql;
            let data_type = text(1);
            let generated = if pg {
                text(4) == "ALWAYS" || text(5) == "YES"
            } else {
                text(4).contains("GENERATED") && !text(4).contains("DEFAULT_GENERATED")
                    || text(4).contains("auto_increment")
            };
            let writable = if pg {
                [
                    "text",
                    "varchar",
                    "bpchar",
                    "int2",
                    "int4",
                    "int8",
                    "uuid",
                    "numeric",
                    "float4",
                    "float8",
                    "bool",
                    "date",
                    "time",
                    "timestamp",
                    "timestamptz",
                    "json",
                    "jsonb",
                ]
                .contains(&text(7).as_str())
            } else {
                ![
                    "binary",
                    "varbinary",
                    "tinyblob",
                    "blob",
                    "mediumblob",
                    "longblob",
                    "geometry",
                    "bit",
                ]
                .contains(&data_type.split('(').next().unwrap_or(&data_type))
            } && !generated;
            columns.push(GridColumn {
                name: text(0),
                data_type,
                comment: text(if pg { 9 } else { 6 }),
                collation: if pg || row[7] == Cell::Null {
                    None
                } else {
                    Some(text(7))
                },
                // 功能：转储区分身份列与计算列，身份值恢复后保留；时间：2026-11-12；作者：lq。
                extra: if pg {
                    if text(5) == "YES" {
                        "IDENTITY".into()
                    } else if text(4) == "ALWAYS" {
                        "GENERATED".into()
                    } else {
                        String::new()
                    }
                } else {
                    text(4).replace("DEFAULT_GENERATED", "").trim().to_string()
                },
                nullable: text(2) == "YES",
                default: if row[3] == Cell::Null {
                    None
                } else {
                    Some(if !pg && !text(4).contains("DEFAULT_GENERATED") {
                        format!("'{}'", text(3).replace('\\', "\\\\").replace('\'', "''"))
                    } else {
                        text(3)
                    })
                },
                generated,
                primary: if pg {
                    text(8) == "YES"
                } else {
                    text(5) == "PRI"
                },
                writable,
                cast: if pg {
                    format!(
                        "{}.{}",
                        identifier(Protocol::PostgreSql, &text(6))?,
                        identifier(Protocol::PostgreSql, &text(7))?
                    )
                } else {
                    String::new()
                },
            });
        }
        if columns.is_empty() {
            return Err("表已不存在或没有读取字段的权限".into());
        }
        Ok(columns)
    }

    /// 功能：固定页长、有主键时稳定排序、筛选值参数绑定，空表保留列头；时间：2026-11-11；作者：lq。
    pub fn browse_page(&self, table: &Table, request: &BrowseRequest) -> Result<TablePage, String> {
        let columns = self.grid_columns(table)?;
        let schema = literal(&table.schema);
        let name = literal(&table.name);
        let index_sql = if self.profile.protocol == Protocol::PostgreSql {
            format!(
                "SELECT indexname, indexdef FROM pg_indexes WHERE schemaname={schema} AND tablename={name}"
            )
        } else {
            format!(
                "SELECT index_name, column_name, non_unique FROM information_schema.statistics WHERE table_schema={schema} AND table_name={name} ORDER BY index_name, seq_in_index"
            )
        };
        let indexes = self
            .query(&index_sql)?
            .sets
            .into_iter()
            .next()
            .unwrap_or_default();
        let protocol = self.profile.protocol;
        let qualified = format!(
            "{}.{}",
            identifier(protocol, &table.schema)?,
            identifier(protocol, &table.name)?
        );
        let mut conn = self.connection.try_lock().map_err(|_| "连接正在执行查询")?;
        self.runtime.as_ref().expect("live runtime").block_on(async {
            macro_rules! fetch {
                ($db:ty, $conn:expr, $decode:ident, $text_type:expr) => {{
                    let mut query = QueryBuilder::<$db>::new(format!("SELECT * FROM {qualified}"));
                    // 功能：多条件绑定、原生类型比较和稳定多列排序；时间：2026-11-12；作者：lq。
                    if request.filters.len() > 32 || request.order.len() > 16 { return Err("筛选或排序条件过多".into()); }
                    for (i, filter) in request.filters.iter().enumerate() {
                        let column = columns.iter().find(|c| c.name == filter.column).ok_or("筛选字段不存在")?;
                        query.push(if i == 0 { " WHERE (" } else if request.match_any { " OR " } else { " AND " });
                        let field = identifier(protocol, &column.name)?;
                        match filter.op {
                            FilterOp::IsNull => { query.push(format!("{field} IS NULL")); }
                            FilterOp::IsNotNull => { query.push(format!("{field} IS NOT NULL")); }
                            FilterOp::Contains | FilterOp::StartsWith | FilterOp::EndsWith => {
                                query.push(format!("CAST({field} AS {}) LIKE ", $text_type));
                                let escaped = filter.value.replace('!', "!!").replace('%', "!%").replace('_', "!_");
                                query.push_bind(match filter.op {
                                    FilterOp::StartsWith => format!("{escaped}%"),
                                    FilterOp::EndsWith => format!("%{escaped}"),
                                    _ => format!("%{escaped}%"),
                                });
                                query.push(" ESCAPE '!' ");
                            }
                            op => {
                                let operator = match op { FilterOp::Equal => "=", FilterOp::NotEqual => "<>", FilterOp::Greater => ">", FilterOp::GreaterEqual => ">=", FilterOp::Less => "<", _ => "<=" };
                                query.push(format!("{field} {operator} "));
                                if protocol == Protocol::PostgreSql { query.push("CAST("); }
                                query.push_bind(filter.value.clone());
                                if protocol == Protocol::PostgreSql { query.push(format!(" AS {})", column.cast)); }
                            }
                        }
                    }
                    if !request.filters.is_empty() { query.push(")"); }
                    let mut order = Vec::new();
                    for (field, descending) in &request.order {
                        if !columns.iter().any(|c| c.name == *field) { return Err("排序字段不存在".into()); }
                        order.push(format!("{} {}", identifier(protocol, field)?, if *descending { "DESC" } else { "ASC" }));
                    }
                    for key in columns.iter().filter(|c| c.primary && !request.order.iter().any(|(n, _)| *n == c.name)) {
                        order.push(identifier(protocol, &key.name)?);
                    }
                    if !order.is_empty() { query.push(format!(" ORDER BY {}", order.join(", "))); }
                    query.push(format!(" LIMIT {} OFFSET {}", PAGE_SIZE + 1, request.page.saturating_mul(PAGE_SIZE)));
                    let sql = query.sql().to_string();
                    let rows = query.build().fetch_all($conn).await.map_err(safe_error)?;
                    let has_next = rows.len() > PAGE_SIZE;
                    let mut values = Vec::new();
                    let mut bytes = 0usize;
                    for row in rows.iter().take(PAGE_SIZE) {
                        let cells = $decode(row)?;
                        bytes += cells.iter().map(Cell::bytes).sum::<usize>();
                        if bytes > 16 * 1024 * 1024 { return Err("此页数据超过 16 MiB，请增加字段筛选以缩小范围".into()); }
                        values.push(cells);
                    }
                    TablePage {
                        data: ResultSet { columns: columns.iter().map(|c| c.name.clone()).collect(), types: columns.iter().map(|c| c.data_type.clone()).collect(), rows: values, affected: 0 },
                        editable: table.kind == "BASE TABLE" && columns.iter().any(|c| c.primary) && columns.iter().filter(|c| c.primary).all(|c| c.writable || c.generated),
                        columns, has_next, sql, indexes,
                    }
                }};
            }
            Ok(match &mut *conn {
                Adapter::Pg(conn) => fetch!(sqlx::Postgres, conn, decode_pg, "TEXT"),
                Adapter::MySql(conn) => fetch!(sqlx::MySql, conn, decode_mysql, "CHAR"),
            })
        })
    }

    /// 功能：同一事务保存全部变更，锁定原记录并比较快照，失败全部回滚；时间：2026-11-11；作者：lq。
    pub fn save_rows(
        &self,
        table: &Table,
        expected: &[GridColumn],
        changes: &[RowChange],
    ) -> Result<(), String> {
        if self.closed.load(Ordering::SeqCst) {
            return Err("连接已失效，请重新连接".into());
        }
        if self.raw_transaction.load(Ordering::SeqCst)
            || self.session_transaction_mode.load(Ordering::SeqCst)
        {
            return Err("请先结束手动事务；修改过 autocommit 的连接需重连后使用网格保存".into());
        }
        let columns = self.grid_columns(table)?;
        if columns != expected {
            return Err("表结构已变化，请保留修改后重新打开表".into());
        }
        validate_changes(table, &columns, changes)?;
        let protocol = self.profile.protocol;
        let qualified = format!(
            "{}.{}",
            identifier(protocol, &table.schema)?,
            identifier(protocol, &table.name)?
        );
        let mut conn = self.connection.try_lock().map_err(|_| "连接正在执行查询")?;
        self.runtime.as_ref().expect("live runtime").block_on(async {
            macro_rules! save {
                ($db:ty, $conn:expr, $decode:ident, $pg:expr) => {{
                    if $conn.is_in_transaction() { return Err("请先结束手动事务".into()); }
                    let mut tx = $conn.begin().await.map_err(safe_error)?;
                    if !$pg {
                        let engine: Option<String> = sqlx::query_scalar("SELECT ENGINE FROM information_schema.tables WHERE table_schema=? AND table_name=? AND table_type='BASE TABLE'").bind(&table.schema).bind(&table.name).fetch_optional(&mut *tx).await.map_err(safe_error)?.flatten();
                        if engine.as_deref() != Some("InnoDB") { return Err("网格保存仅支持 InnoDB 事务表".into()); }
                    }
                    for change in changes {
                        // 功能：依据原始复合主键定位，主键编辑不会丢失目标；时间：2026-11-11；作者：lq。
                        if let Some(original) = &change.original {
                            let mut lock = QueryBuilder::<$db>::new(format!("SELECT * FROM {qualified} WHERE "));
                            let mut first = true;
                            for (i, column) in columns.iter().enumerate().filter(|(_, c)| c.primary) {
                                if !first { lock.push(" AND "); } first = false;
                                lock.push(format!("{} = ", identifier(protocol, &column.name)?));
                                bind_cell!(lock, &original[i], column, $pg);
                            }
                            lock.push(" FOR UPDATE");
                            let rows = lock.build().fetch_all(&mut *tx).await.map_err(safe_error)?;
                            if rows.len() != 1 || $decode(&rows[0])? != *original { return Err("记录已被其他会话修改或删除；本次保存全部回滚，请取消修改后刷新".into()); }
                        }
                        let mut query = QueryBuilder::<$db>::new(if change.delete { format!("DELETE FROM {qualified}") } else if change.original.is_some() { format!("UPDATE {qualified} SET ") } else { format!("INSERT INTO {qualified}") });
                        if !change.delete {
                            let fields: Vec<_> = change.values.iter().enumerate().filter_map(|(i, value)| value.as_ref().map(|v| (i, v))).collect();
                            if change.original.is_none() {
                                if fields.is_empty() {
                                    query.push(if $pg { " DEFAULT VALUES" } else { " () VALUES ()" });
                                } else {
                                    query.push(format!(" ({}) VALUES (", fields.iter().map(|(i, _)| identifier(protocol, &columns[*i].name)).collect::<Result<Vec<_>, _>>()?.join(", ")));
                                }
                            }
                            for (n, (i, value)) in fields.iter().enumerate() {
                                if n > 0 { query.push(", "); }
                                if change.original.is_some() { query.push(format!("{} = ", identifier(protocol, &columns[*i].name)?)); }
                                bind_cell!(query, value, &columns[*i], $pg);
                            }
                            if change.original.is_none() && !fields.is_empty() { query.push(")"); }
                        }
                        if let Some(original) = &change.original {
                            query.push(" WHERE ");
                            let mut first = true;
                            for (i, column) in columns.iter().enumerate().filter(|(_, c)| c.primary) {
                                if !first { query.push(" AND "); } first = false;
                                query.push(format!("{} = ", identifier(protocol, &column.name)?));
                                bind_cell!(query, &original[i], column, $pg);
                            }
                        }
                        let count = query.build().execute(&mut *tx).await.map_err(safe_error)?.rows_affected();
                        if count != 1 { return Err("受影响行数不是一行，本次保存全部回滚".into()); }
                    }
                    tx.commit().await.map_err(safe_error)?;
                }};
            }
            // 功能：文本参数在 PostgreSQL 按真实元数据类型转换，NULL 不混同空字符串；时间：2026-11-11；作者：lq。
            macro_rules! bind_cell {
                ($query:expr, $cell:expr, $column:expr, $pg:expr) => {{
                    let value = match $cell { Cell::Text(v) => Some(v.clone()), Cell::Null => None, Cell::Binary(_) => return Err("二进制字段暂不支持编辑".into()) };
                    if $pg { $query.push("CAST("); }
                    $query.push_bind(value);
                    if $pg { $query.push(format!(" AS {})", $column.cast)); }
                }};
            }
            match &mut *conn {
                Adapter::Pg(conn) => save!(sqlx::Postgres, conn, decode_pg, true),
                Adapter::MySql(conn) => save!(sqlx::MySql, conn, decode_mysql, false),
            }
            Ok(())
        })
    }
}

/// 功能：写入计划校验独立于 UI，禁止无主键修改及生成列赋值；时间：2026-11-11；作者：lq。
fn validate_changes(
    table: &Table,
    columns: &[GridColumn],
    changes: &[RowChange],
) -> Result<(), String> {
    if table.kind != "BASE TABLE"
        || !columns.iter().any(|c| c.primary)
        || columns
            .iter()
            .any(|c| c.primary && !c.writable && !c.generated)
    {
        return Err("视图、无可写主键的表只读".into());
    }
    if changes.is_empty() || changes.len() > 1000 {
        return Err("保存变更数量必须为 1–1000 行".into());
    }
    for change in changes {
        if change.values.len() != columns.len()
            || change
                .original
                .as_ref()
                .is_some_and(|v| v.len() != columns.len())
            || change.delete && change.original.is_none()
        {
            return Err("网格变更与字段数量不匹配".into());
        }
        if change.original.is_some() && !change.delete && change.values.iter().all(Option::is_none)
        {
            return Err("记录没有变化".into());
        }
        for (i, value) in change.values.iter().enumerate() {
            if let Some(value) = value {
                if !columns[i].writable || matches!(value, Cell::Binary(_)) {
                    return Err(format!("{} 为只读字段", columns[i].name));
                }
                if matches!(value, Cell::Null) && !columns[i].nullable {
                    return Err(format!("{} 不允许 NULL", columns[i].name));
                }
                if value.bytes() > 65536 {
                    return Err("单个修改值超过 64 KiB".into());
                }
            }
        }
    }
    Ok(())
}
