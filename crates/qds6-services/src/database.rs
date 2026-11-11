//! 功能：独立 MySQL/PostgreSQL 连接、流式有界结果及服务端查询取消；时间：2026-11-10；作者：lq。

use futures_util::TryStreamExt;
use qds6_core::{ConnectionProfile, Protocol, TlsMode};
use sqlx::{
    Column, ConnectOptions, Connection, Row, TypeInfo, ValueRef,
    mysql::{MySqlConnectOptions, MySqlConnection, MySqlRow, MySqlSslMode},
    postgres::{PgConnectOptions, PgConnection, PgRow, PgSslMode},
    types::{
        BigDecimal,
        chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc},
    },
};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};
use tokio::runtime::Runtime;
use zeroize::Zeroizing;

/// 功能：区分 NULL、文本和二进制，显示精确数值而不转浮点；时间：2026-11-10；作者：lq。
#[derive(Debug, Clone, PartialEq)]
pub enum Cell {
    Null,
    Text(String),
    Binary(Vec<u8>),
}
impl Cell {
    /// 功能：界面文字保留 NULL 与二进制标记；时间：2026-11-10；作者：lq。
    pub fn display(&self) -> String {
        match self {
            Self::Null => "NULL".into(),
            Self::Text(s) => s.clone(),
            Self::Binary(bytes) => format!("0x{}", hex(bytes)),
        }
    }
    /// 功能：核对结果缓存总量；时间：2026-11-10；作者：lq。
    fn bytes(&self) -> usize {
        match self {
            Self::Null => 0,
            Self::Text(s) => s.len(),
            Self::Binary(b) => b.len(),
        }
    }
}

/// 功能：单结果集，用户 SQL 不拼接 LIMIT 或改写；时间：2026-11-10；作者：lq。
#[derive(Default, Clone)]
pub struct ResultSet {
    pub columns: Vec<String>,
    pub types: Vec<String>,
    pub rows: Vec<Vec<Cell>>,
    pub affected: u64,
}

/// 功能：执行结果保留多结果集、耗时及截断标记；时间：2026-11-10；作者：lq。
#[derive(Clone, Default)]
pub struct QueryResult {
    pub sets: Vec<ResultSet>,
    pub elapsed_ms: u128,
    pub truncated: bool,
}

/// 功能：基础表身份由元数据确定，不允许把任意查询网格当可写表；时间：2026-11-10；作者：lq。
#[derive(Clone)]
pub struct Table {
    pub schema: String,
    pub name: String,
    pub kind: String,
}

/// 功能：单项网格写入计划，提交前可显示目标列、键和新值；时间：2026-11-10；作者：lq。
#[derive(Clone)]
pub struct Edit {
    pub table: Table,
    pub key_column: String,
    pub key: String,
    pub column: String,
    pub value: Option<String>,
    pub delete: bool,
}

/// 功能：连接只由后台查询 worker 持有，手动事务始终使用同一连接；时间：2026-11-10；作者：lq。
enum Adapter {
    Pg(PgConnection),
    MySql(MySqlConnection),
}

/// 功能：数据库会话与独立取消连接共享配置，不在诊断中打印秘密；时间：2026-11-10；作者：lq。
pub struct DatabaseSession {
    runtime: Option<Runtime>,
    connection: Mutex<Adapter>,
    profile: ConnectionProfile,
    secret: Zeroizing<String>,
    server_id: u64,
    running: AtomicBool,
    closed: AtomicBool,
    // 功能：原始 SQL 的事务控制不经过 SQLx 事务深度，保守阻止网格另起事务；时间：2026-11-11；作者：lq。
    raw_transaction: AtomicBool,
    session_transaction_mode: AtomicBool,
    _tunnel: Option<Arc<crate::ssh::TunnelLease>>,
}

impl DatabaseSession {
    /// 功能：按数据库方言读取对象目录，限制为基础表/视图；时间：2026-11-10；作者：lq。
    pub fn tables(&self) -> Result<Vec<Table>, String> {
        let sql = match self.profile.protocol {
            Protocol::PostgreSql => {
                "SELECT table_schema, table_name, table_type FROM information_schema.tables WHERE table_schema NOT IN ('pg_catalog','information_schema') ORDER BY table_schema, table_name"
            }
            _ => {
                "SELECT table_schema, table_name, table_type FROM information_schema.tables WHERE table_schema=DATABASE() ORDER BY table_schema, table_name"
            }
        };
        let result = self.query(sql)?;
        if result.truncated {
            return Err("对象目录超过当前上限，请缩小数据库范围".into());
        }
        Ok(result
            .sets
            .into_iter()
            .flat_map(|s| s.rows)
            .filter_map(|row| {
                if row.len() >= 3 {
                    let metadata = |cell: &Cell| match cell {
                        Cell::Text(text) => text.clone(),
                        Cell::Binary(bytes) => String::from_utf8(bytes.clone()).unwrap_or_default(),
                        Cell::Null => String::new(),
                    };
                    Some(Table {
                        schema: metadata(&row[0]),
                        name: metadata(&row[1]),
                        kind: metadata(&row[2]),
                    })
                } else {
                    None
                }
            })
            .collect())
    }

    /// 功能：按需读取字段与约束/索引元数据，字符串文字由方言转义；时间：2026-11-10；作者：lq。
    pub fn structure(&self, table: &Table) -> Result<QueryResult, String> {
        let schema = literal(&table.schema);
        let name = literal(&table.name);
        let columns = format!(
            "SELECT column_name, data_type, is_nullable, column_default FROM information_schema.columns WHERE table_schema={schema} AND table_name={name} ORDER BY ordinal_position"
        );
        let indexes = if self.profile.protocol == Protocol::PostgreSql {
            format!(
                "SELECT indexname, indexdef FROM pg_indexes WHERE schemaname={schema} AND tablename={name}"
            )
        } else {
            format!(
                "SELECT index_name, column_name, non_unique FROM information_schema.statistics WHERE table_schema={schema} AND table_name={name} ORDER BY index_name, seq_in_index"
            )
        };
        self.query(&format!("{columns}; {indexes}"))
    }

    /// 功能：安全生成基础表浏览语句，用户原始 SQL 不会被改写；时间：2026-11-10；作者：lq。
    pub fn browse(&self, table: &Table) -> Result<QueryResult, String> {
        self.query(&format!(
            "SELECT * FROM {}.{} LIMIT 1000",
            identifier(self.profile.protocol, &table.schema)?,
            identifier(self.profile.protocol, &table.name)?
        ))
    }

    /// 功能：仅对有单列主键的基础表执行参数化更新/删除，事务内核对受影响行；时间：2026-11-10；作者：lq。
    pub fn apply_edit(&self, edit: &Edit) -> Result<(), String> {
        if self.closed.load(Ordering::SeqCst) {
            return Err("数据库连接已停用".into());
        }
        if self.raw_transaction.load(Ordering::SeqCst)
            || self.session_transaction_mode.load(Ordering::SeqCst)
        {
            return Err("请先 COMMIT / ROLLBACK 手动事务；修改过 autocommit 或调用存储过程时需重连后再使用网格写入".into());
        }
        if edit.key.len() > 65536 || edit.value.as_ref().is_some_and(|v| v.len() > 65536) {
            return Err("网格修改值超过 64 KiB".into());
        }
        let mut conn = self.connection.try_lock().map_err(|_| "连接正在执行查询")?;
        self.runtime.as_ref().expect("live runtime").block_on(async {
            let schema = &edit.table.schema; let table = &edit.table.name;
            let key = &edit.key_column; let column = &edit.column;
            let qualified = format!("{}.{}",identifier(self.profile.protocol,schema)?,identifier(self.profile.protocol,table)?);
            let key_name = identifier(self.profile.protocol,key)?; let column_name = identifier(self.profile.protocol,column)?;
            match &mut *conn {
                Adapter::Pg(conn) => {
                    if conn.is_in_transaction() { return Err("请先结束手动事务，再提交网格修改".into()); }
                    let mut tx = conn.begin().await.map_err(safe_error)?;
                    let keys: Vec<String> = sqlx::query_scalar("SELECT k.column_name FROM information_schema.table_constraints t JOIN information_schema.key_column_usage k ON t.constraint_catalog=k.constraint_catalog AND t.constraint_schema=k.constraint_schema AND t.constraint_name=k.constraint_name AND t.table_name=k.table_name WHERE t.constraint_type='PRIMARY KEY' AND t.table_schema=$1 AND t.table_name=$2 ORDER BY k.ordinal_position").bind(schema).bind(table).fetch_all(&mut *tx).await.map_err(safe_error)?;
                    if keys != [key.clone()] { return Err("仅支持单列主键定位，主键不匹配或不存在".into()); }
                    let key_type: String = sqlx::query_scalar("SELECT udt_name FROM information_schema.columns WHERE table_schema=$1 AND table_name=$2 AND column_name=$3").bind(schema).bind(table).bind(key).fetch_one(&mut *tx).await.map_err(safe_error)?;
                    if !["text","varchar","bpchar","int2","int4","int8","uuid"].contains(&key_type.as_str()) { return Err("此主键类型暂不支持安全网格写入".into()); }
                    let count: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM {qualified} WHERE {key_name}=CAST($1 AS {})",identifier(Protocol::PostgreSql,&key_type)?)).bind(&edit.key).fetch_one(&mut *tx).await.map_err(safe_error)?;
                    if count != 1 { return Err("主键未精确匹配一行，未提交修改".into()); }
                    let result = if edit.delete {
                        sqlx::query(&format!("DELETE FROM {qualified} WHERE {key_name}=CAST($1 AS {})",identifier(Protocol::PostgreSql,&key_type)?)).bind(&edit.key).execute(&mut *tx).await.map_err(safe_error)?
                    } else {
                        let value_type: String = sqlx::query_scalar("SELECT udt_name FROM information_schema.columns WHERE table_schema=$1 AND table_name=$2 AND column_name=$3").bind(schema).bind(table).bind(column).fetch_one(&mut *tx).await.map_err(safe_error)?;
                        if !["text","varchar","bpchar","int2","int4","int8","uuid","numeric","bool","date","timestamp","timestamptz","json","jsonb"].contains(&value_type.as_str()) { return Err("此列类型暂不支持网格文本写入".into()); }
                        sqlx::query(&format!("UPDATE {qualified} SET {column_name}=CAST($1 AS {}) WHERE {key_name}=CAST($2 AS {})",identifier(Protocol::PostgreSql,&value_type)?,identifier(Protocol::PostgreSql,&key_type)?)).bind(edit.value.as_deref()).bind(&edit.key).execute(&mut *tx).await.map_err(safe_error)?
                    };
                    if result.rows_affected()!=1 { return Err("受影响行数不是一行，事务已回滚".into()); }
                    tx.commit().await.map_err(safe_error)?;
                }
                Adapter::MySql(conn) => {
                    if conn.is_in_transaction() { return Err("请先结束手动事务，再提交网格修改".into()); }
                    let mut tx = conn.begin().await.map_err(safe_error)?;
                    let engine: Option<String> = sqlx::query_scalar("SELECT ENGINE FROM information_schema.tables WHERE table_schema=? AND table_name=? AND table_type='BASE TABLE'").bind(schema).bind(table).fetch_optional(&mut *tx).await.map_err(safe_error)?.flatten();
                    if engine.as_deref()!=Some("InnoDB") { return Err("只支持 InnoDB 基础表的事务网格写入".into()); }
                    let keys: Vec<String> = sqlx::query_scalar("SELECT column_name FROM information_schema.key_column_usage WHERE constraint_name='PRIMARY' AND table_schema=? AND table_name=? ORDER BY ordinal_position").bind(schema).bind(table).fetch_all(&mut *tx).await.map_err(safe_error)?;
                    if keys != [key.clone()] { return Err("仅支持单列主键定位，主键不匹配或不存在".into()); }
                    let count: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM {qualified} WHERE {key_name}=?")).bind(&edit.key).fetch_one(&mut *tx).await.map_err(safe_error)?;
                    if count!=1 { return Err("主键未精确匹配一行，未提交修改".into()); }
                    let result = if edit.delete { sqlx::query(&format!("DELETE FROM {qualified} WHERE {key_name}=?")).bind(&edit.key).execute(&mut *tx).await } else { sqlx::query(&format!("UPDATE {qualified} SET {column_name}=? WHERE {key_name}=?")).bind(edit.value.as_deref()).bind(&edit.key).execute(&mut *tx).await }.map_err(safe_error)?;
                    if result.rows_affected()!=1 { return Err("受影响行数不是一行，事务已回滚".into()); }
                    tx.commit().await.map_err(safe_error)?;
                }
            }
            Ok(())
        })
    }
    /// 功能：直连默认严格 TLS；隧道在上层建立并由会话持有；时间：2026-11-10；作者：lq。
    pub fn connect(
        profile: &ConnectionProfile,
        secret: &str,
        tunnel: Option<Arc<crate::ssh::TunnelLease>>,
    ) -> Result<Self, String> {
        profile.validate().map_err(|e| e.to_string())?;
        if profile.db.tunnel_profile.is_some() && tunnel.is_none() {
            return Err("数据库依赖的 SSH 隧道尚未建立".into());
        }
        // 功能：SQLx 的回环 host 会影响证书主机校验，禁止静默降级 TLS；时间：2026-11-10；作者：lq。
        if tunnel.is_some() && profile.db.tls == TlsMode::VerifyFull {
            return Err(
                "当前隧道尚不支持原目标 TLS 名称覆盖；请选择直接 TLS 连接或显式关闭隧道内 TLS"
                    .into(),
            );
        }
        let mut effective = profile.clone();
        if let Some(tunnel) = &tunnel {
            effective.host = "127.0.0.1".into();
            effective.port = tunnel.port;
        }
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .map_err(|_| "数据库运行时初始化失败")?;
        let connection = runtime.block_on(open(&effective, secret))?;
        let (connection, server_id) = runtime.block_on(async move {
            match connection {
                Adapter::Pg(mut conn) => {
                    let id: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
                        .fetch_one(&mut conn)
                        .await
                        .map_err(safe_error)?;
                    Ok::<_, String>((Adapter::Pg(conn), id as u64))
                }
                Adapter::MySql(mut conn) => {
                    let id: u64 = sqlx::query_scalar("SELECT CONNECTION_ID()")
                        .fetch_one(&mut conn)
                        .await
                        .map_err(safe_error)?;
                    Ok((Adapter::MySql(conn), id))
                }
            }
        })?;
        Ok(Self {
            runtime: Some(runtime),
            connection: Mutex::new(connection),
            profile: effective,
            secret: Zeroizing::new(secret.into()),
            server_id,
            running: AtomicBool::new(false),
            closed: AtomicBool::new(false),
            raw_transaction: AtomicBool::new(false),
            session_transaction_mode: AtomicBool::new(false),
            _tunnel: tunnel,
        })
    }

    /// 功能：执行任意 SQL，结果总计 10000 行/16 MiB；达到上限真实取消服务端；时间：2026-11-10；作者：lq。
    pub fn query(&self, sql: &str) -> Result<QueryResult, String> {
        if self.closed.load(Ordering::SeqCst) {
            return Err("连接已失效，请重新连接".into());
        }
        if sql.trim().is_empty() || sql.len() > 5 * 1024 * 1024 {
            return Err("SQL 为空或超过 5 MiB".into());
        }
        let mut conn = self.connection.try_lock().map_err(|_| "已有查询正在执行")?;
        // 功能：任意 SQL 保持原样执行；事务关键字保守识别（包括注释/文字中的同词），防止隐式提交；时间：2026-11-11；作者：lq。
        let upper = sql.to_ascii_uppercase();
        let tokens: Vec<_> = upper
            .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
            .collect();
        if tokens
            .iter()
            .any(|t| ["BEGIN", "START", "SAVEPOINT", "TRANSACTION"].contains(t))
        {
            self.raw_transaction.store(true, Ordering::SeqCst);
        }
        if tokens.iter().any(|t| ["AUTOCOMMIT", "CALL"].contains(t)) {
            self.session_transaction_mode.store(true, Ordering::SeqCst);
        }
        self.running.store(true, Ordering::SeqCst);
        let start = Instant::now();
        let result = self.runtime.as_ref().expect("live runtime").block_on(async {
            let mut result = QueryResult::default(); let mut set = ResultSet::default();
            let mut rows = 0usize; let mut bytes = 0usize;
            macro_rules! stream {
                ($conn:expr, $decode:ident) => {{
                    let mut stream = sqlx::raw_sql(sql).fetch_many($conn);
                    while let Some(item) = stream.try_next().await.map_err(|error| {
                        // 功能：管理端终止和连接类服务端错误同样使会话失效；普通 SQL 错误仍允许回滚；时间：2026-11-11；作者：lq。
                        let disconnected = error.as_database_error().is_none() || error.as_database_error().is_some_and(|db| {
                            db.code().is_some_and(|code| code.starts_with("08") || ["57P01","57P02","57P03","2006","2013"].contains(&code.as_ref()))
                        });
                        if disconnected { self.closed.store(true, Ordering::SeqCst); }
                        safe_error(error)
                    })? {
                        match item {
                            sqlx::Either::Right(row) => {
                                if set.columns.is_empty() {
                                    set.columns = row.columns().iter().map(|c| c.name().to_string()).collect();
                                    set.types = row.columns().iter().map(|c| c.type_info().name().to_string()).collect();
                                }
                                let cells = $decode(&row)?;
                                let row_bytes: usize = cells.iter().map(Cell::bytes).sum();
                                if rows >= 10000 || bytes.saturating_add(row_bytes) > 16 * 1024 * 1024 {
                                    result.truncated = true;
                                    if self.cancel_inner().await.is_err() { self.closed.store(true, Ordering::SeqCst); return Err("结果上限已到，但服务端取消失败；连接已停用，请检查服务端后重连".into()); }
                                    break;
                                }
                                rows += 1; bytes += row_bytes; set.rows.push(cells);
                            }
                            sqlx::Either::Left(done) => { set.affected = done.rows_affected(); result.sets.push(std::mem::take(&mut set)); }
                        }
                    }
                }};
            }
            match &mut *conn { Adapter::Pg(conn) => stream!(conn, decode_pg), Adapter::MySql(conn) => stream!(conn, decode_mysql) }
            if !set.columns.is_empty() || !set.rows.is_empty() { result.sets.push(set); }
            result.elapsed_ms = start.elapsed().as_millis();
            Ok(result)
        });
        self.running.store(false, Ordering::SeqCst);
        if result.is_ok()
            && ["COMMIT", "ROLLBACK"].contains(&upper.trim().trim_end_matches(';').trim())
        {
            self.raw_transaction.store(false, Ordering::SeqCst);
        }
        result
    }

    /// 功能：通过独立控制连接调用服务端取消，绝非仅停止本地等待；时间：2026-11-10；作者：lq。
    pub fn cancel(&self) -> Result<(), String> {
        if !self.running.load(Ordering::SeqCst) {
            return Err("当前没有执行中的查询".into());
        }
        self.runtime
            .as_ref()
            .expect("live runtime")
            .block_on(self.cancel_inner())
    }

    /// 功能：按数据库方言取消当前 dedicated session；时间：2026-11-10；作者：lq。
    async fn cancel_inner(&self) -> Result<(), String> {
        match open(&self.profile, &self.secret).await? {
            Adapter::Pg(mut conn) => {
                let canceled: bool = sqlx::query_scalar("SELECT pg_cancel_backend($1)")
                    .bind(self.server_id as i32)
                    .fetch_one(&mut conn)
                    .await
                    .map_err(safe_error)?;
                conn.close().await.map_err(safe_error)?;
                if !canceled {
                    return Err("服务端未确认查询取消".into());
                }
            }
            Adapter::MySql(mut conn) => {
                sqlx::query(&format!("KILL QUERY {}", self.server_id))
                    .execute(&mut conn)
                    .await
                    .map_err(safe_error)?;
                conn.close().await.map_err(safe_error)?;
            }
        }
        Ok(())
    }
}

impl Drop for DatabaseSession {
    /// 功能：UI 释放连接时不等待 Tokio worker 退出，网络工作不会阻塞窗口；时间：2026-11-10；作者：lq。
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_background();
        }
    }
}

/// 功能：仅生成可信方言对象标识符，禁止控制字符并双写引用符；时间：2026-11-10；作者：lq。
pub fn identifier(protocol: Protocol, name: &str) -> Result<String, String> {
    if name.is_empty() || name.len() > 256 || name.chars().any(char::is_control) {
        return Err("数据库对象名无效".into());
    }
    let delimiter = if protocol == Protocol::MySql {
        '`'
    } else {
        '"'
    };
    Ok(format!(
        "{delimiter}{}{delimiter}",
        name.replace(delimiter, &format!("{delimiter}{delimiter}"))
    ))
}

/// 功能：仅用于受控元数据查询的字符串文字，不用于用户值写入；时间：2026-11-10；作者：lq。
fn literal(value: &str) -> String {
    format!("'{}'", value.replace('\\', "\\\\").replace('\'', "''"))
}

/// 功能：连接参数通过类型化 API 设置，密码不进入 URL 或日志；时间：2026-11-10；作者：lq。
async fn open(profile: &ConnectionProfile, secret: &str) -> Result<Adapter, String> {
    let future = async {
        match profile.protocol {
            Protocol::PostgreSql => {
                let mut options = PgConnectOptions::new()
                    .host(&profile.host)
                    .port(profile.port)
                    .username(&profile.username)
                    .password(secret)
                    .ssl_mode(if profile.db.tls == TlsMode::VerifyFull {
                        PgSslMode::VerifyFull
                    } else {
                        PgSslMode::Disable
                    });
                if !profile.database.is_empty() {
                    options = options.database(&profile.database);
                }
                if !profile.db.ca_file.is_empty() {
                    options = options.ssl_root_cert(&profile.db.ca_file);
                }
                let options = options.disable_statement_logging();
                PgConnection::connect_with(&options)
                    .await
                    .map(Adapter::Pg)
                    .map_err(safe_error)
            }
            Protocol::MySql => {
                let mut options = MySqlConnectOptions::new()
                    .host(&profile.host)
                    .port(profile.port)
                    .username(&profile.username)
                    .password(secret)
                    .ssl_mode(if profile.db.tls == TlsMode::VerifyFull {
                        MySqlSslMode::VerifyIdentity
                    } else {
                        MySqlSslMode::Disabled
                    });
                if !profile.database.is_empty() {
                    options = options.database(&profile.database);
                }
                if !profile.db.ca_file.is_empty() {
                    options = options.ssl_ca(&profile.db.ca_file);
                }
                let options = options.disable_statement_logging();
                MySqlConnection::connect_with(&options)
                    .await
                    .map(Adapter::MySql)
                    .map_err(safe_error)
            }
            _ => Err("所选连接不是数据库".into()),
        }
    };
    tokio::time::timeout(std::time::Duration::from_secs(12), future)
        .await
        .map_err(|_| "数据库连接超时".to_string())?
}

/// 功能：仅显示服务端错误码与位置，不回显可能含秘密的 SQL/URL；时间：2026-11-10；作者：lq。
fn safe_error(error: sqlx::Error) -> String {
    match error {
        sqlx::Error::Database(error) => format!(
            "数据库错误 {}{}；请检查 SQL、权限或事务状态",
            error.code().as_deref().unwrap_or("unknown"),
            error
                .try_downcast_ref::<sqlx::postgres::PgDatabaseError>()
                .and_then(|e| e.position())
                .map(|p| format!("，位置 {p:?}"))
                .unwrap_or_default()
        ),
        sqlx::Error::Tls(_) => "TLS 握手或证书验证失败".into(),
        sqlx::Error::Io(_) => "数据库网络中断，请重新连接".into(),
        _ => "数据库请求失败或返回了不支持的类型".into(),
    }
}

/// 功能：PostgreSQL 按运行时类型精确读取，不强制把二进制协议当 UTF-8；时间：2026-11-10；作者：lq。
fn decode_pg(row: &PgRow) -> Result<Vec<Cell>, String> {
    (0..row.len())
        .map(|i| {
            let raw = row.try_get_raw(i).map_err(safe_error)?;
            if raw.is_null() {
                return Ok(Cell::Null);
            }
            let kind = raw.type_info().name().to_string();
            macro_rules! text {
                ($ty:ty) => {
                    row.try_get::<$ty, _>(i)
                        .map(|v| Cell::Text(v.to_string()))
                        .map_err(safe_error)
                };
            }
            match kind.as_str() {
                "BOOL" => text!(bool),
                "INT2" => text!(i16),
                "INT4" => text!(i32),
                "INT8" => text!(i64),
                "FLOAT4" => text!(f32),
                "FLOAT8" => text!(f64),
                "NUMERIC" => text!(BigDecimal),
                "DATE" => text!(NaiveDate),
                "TIME" => text!(NaiveTime),
                "TIMESTAMP" => text!(NaiveDateTime),
                "TIMESTAMPTZ" => text!(DateTime<Utc>),
                "JSON" | "JSONB" => text!(serde_json::Value),
                "UUID" => text!(uuid::Uuid),
                "BYTEA" => row
                    .try_get::<Vec<u8>, _>(i)
                    .map(Cell::Binary)
                    .map_err(safe_error),
                "TEXT" | "VARCHAR" | "CHAR" | "BPCHAR" | "NAME" | "UNKNOWN" => text!(String),
                _ => Ok(Cell::Binary(
                    raw.as_bytes().map_err(|_| "无法读取结果类型")?.to_vec(),
                )),
            }
        })
        .collect()
}

/// 功能：MySQL 保留无符号大整数、DECIMAL、NULL 与字节类型；时间：2026-11-10；作者：lq。
fn decode_mysql(row: &MySqlRow) -> Result<Vec<Cell>, String> {
    (0..row.len())
        .map(|i| {
            let raw = row.try_get_raw(i).map_err(safe_error)?;
            if raw.is_null() {
                return Ok(Cell::Null);
            }
            let kind = raw.type_info().name().to_string();
            macro_rules! text {
                ($ty:ty) => {
                    row.try_get::<$ty, _>(i)
                        .map(|v| Cell::Text(v.to_string()))
                        .map_err(safe_error)
                };
            }
            match kind.as_str() {
                "BOOLEAN" => text!(bool),
                "TINYINT" | "SMALLINT" | "MEDIUMINT" | "INT" | "BIGINT" => text!(i64),
                "TINYINT UNSIGNED" | "SMALLINT UNSIGNED" | "MEDIUMINT UNSIGNED"
                | "INT UNSIGNED" | "BIGINT UNSIGNED" => text!(u64),
                "FLOAT" => text!(f32),
                "DOUBLE" => text!(f64),
                "DECIMAL" => text!(BigDecimal),
                "DATE" => text!(NaiveDate),
                "TIME" => text!(NaiveTime),
                "DATETIME" | "TIMESTAMP" => text!(NaiveDateTime),
                "JSON" => text!(serde_json::Value),
                "CHAR" | "VARCHAR" | "TEXT" | "TINYTEXT" | "MEDIUMTEXT" | "LONGTEXT" | "ENUM"
                | "SET" => text!(String),
                _ => row
                    .try_get::<Vec<u8>, _>(i)
                    .map(Cell::Binary)
                    .map_err(safe_error),
            }
        })
        .collect()
}

/// 功能：二进制转十六进制且不损失原始字节；时间：2026-11-10；作者：lq。
fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8] = b"0123456789abcdef";
    bytes
        .iter()
        .flat_map(|b| {
            [
                DIGITS[(b >> 4) as usize] as char,
                DIGITS[(b & 15) as usize] as char,
            ]
        })
        .collect()
}

/// 功能：CSV 规范引用，NULL 与空字符串通过单独参数约定；时间：2026-11-10；作者：lq。
pub fn csv(result: &ResultSet) -> Vec<u8> {
    let quote = |value: &str| format!("\"{}\"", value.replace('"', "\"\""));
    let mut output = result
        .columns
        .iter()
        .map(|v| quote(v))
        .collect::<Vec<_>>()
        .join(",");
    output.push_str("\r\n");
    for row in &result.rows {
        output.push_str(
            &row.iter()
                .map(|cell| match cell {
                    Cell::Null => String::new(),
                    _ => quote(&cell.display()),
                })
                .collect::<Vec<_>>()
                .join(","),
        );
        output.push_str("\r\n");
    }
    output.into_bytes()
}
