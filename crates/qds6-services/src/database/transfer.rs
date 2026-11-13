//! 功能：数据库对象属性、整表 CSV/Excel/SQL 交换，后台流式导出与事务导入；时间：2026-11-12；作者：lq。
use super::*;
use calamine::{Data, Reader, open_workbook_auto};
use std::{collections::HashMap, fs::OpenOptions, io::Write, path::Path};
mod dump;
mod operations;

/// 功能：对象列表的真实属性，行数使用数据库统计估计而非阻塞 COUNT；时间：2026-11-12；作者：lq。
#[derive(Clone, Default)]
pub struct TableProperties {
    pub oid: String,
    pub owner: String,
    pub rows: String,
    pub bytes: String,
    pub primary_key: String,
    pub engine: String,
    pub comment: String,
}
/// 功能：表菜单只提供实际实现的文件格式与两种 SQL 转储范围；时间：2026-11-12；作者：lq。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TransferFormat {
    Csv,
    Excel,
    SqlStructure,
    SqlData,
}
impl TransferFormat {
    /// 功能：默认扩展名供原生保存对话框使用；时间：2026-11-12；作者：lq。
    pub fn extension(self) -> &'static str {
        match self {
            Self::Csv => "csv",
            Self::Excel => "xlsx",
            _ => "sql",
        }
    }
}

impl DatabaseSession {
    /// 功能：批量读取目录属性，不逐表统计业务数据；时间：2026-11-12；作者：lq。
    pub fn table_properties(&self) -> Result<HashMap<(String, String), TableProperties>, String> {
        let sql = if self.profile.protocol == Protocol::PostgreSql {
            "SELECT n.nspname, c.relname, c.oid::text, pg_get_userbyid(c.relowner), GREATEST(c.reltuples,0)::bigint::text, CASE WHEN c.relkind IN ('r','m') THEN pg_total_relation_size(c.oid)::text ELSE '' END, COALESCE((SELECT string_agg(a.attname, ', ' ORDER BY k.ord) FROM pg_index i CROSS JOIN LATERAL unnest(i.indkey) WITH ORDINALITY k(attnum,ord) JOIN pg_attribute a ON a.attrelid=c.oid AND a.attnum=k.attnum WHERE i.indrelid=c.oid AND i.indisprimary),''), CASE c.relkind WHEN 'p' THEN '分区表' WHEN 'v' THEN '视图' ELSE '常规' END, COALESCE(obj_description(c.oid,'pg_class'),'') FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname NOT IN ('pg_catalog','information_schema') AND n.nspname !~ '^pg_toast' AND c.relkind IN ('r','p','v') ORDER BY n.nspname,c.relname"
        } else {
            "SELECT t.table_schema,t.table_name,'','',COALESCE(CAST(t.table_rows AS CHAR),''),COALESCE(CAST(t.data_length+t.index_length AS CHAR),''),COALESCE((SELECT GROUP_CONCAT(k.column_name ORDER BY k.ordinal_position SEPARATOR ', ') FROM information_schema.key_column_usage k WHERE k.table_schema=t.table_schema AND k.table_name=t.table_name AND k.constraint_name='PRIMARY'),''),COALESCE(t.engine,t.table_type),COALESCE(t.table_comment,'') FROM information_schema.tables t WHERE t.table_schema=DATABASE() ORDER BY t.table_name"
        };
        let r = self.query(sql)?;
        if r.truncated {
            return Err("对象属性超过查询上限".into());
        }
        Ok(r.sets
            .into_iter()
            .flat_map(|s| s.rows)
            .map(|r| {
                let s = |i| text(&r[i]);
                (
                    (s(0), s(1)),
                    TableProperties {
                        oid: s(2),
                        owner: s(3),
                        rows: s(4),
                        bytes: s(5),
                        primary_key: s(6),
                        engine: s(7),
                        comment: s(8),
                    },
                )
            })
            .collect())
    }

    /// 功能：独立只读快照连接导出整表，流式读取不受查询页行数上限影响，失败删除半成品；时间：2026-11-12；作者：lq。
    pub fn export_table(
        &self,
        table: &Table,
        format: TransferFormat,
        path: &Path,
    ) -> Result<u64, String> {
        let snapshot = Self::connect(&self.profile, &self.secret, self._tunnel.clone())?;
        snapshot.query(if self.profile.protocol == Protocol::PostgreSql {
            "BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY"
        } else { "SET TRANSACTION ISOLATION LEVEL REPEATABLE READ; START TRANSACTION WITH CONSISTENT SNAPSHOT, READ ONLY" })?;
        let qualified = format!(
            "{}.{}",
            identifier(self.profile.protocol, &table.schema)?,
            identifier(self.profile.protocol, &table.name)?
        );
        // 功能：快照内先锁定对象元数据，阻止导出期间其他会话修改字段顺序；时间：2026-11-12；作者：lq。
        snapshot.query(&format!("SELECT * FROM {qualified} LIMIT 0"))?;
        let (prefix, suffix) = if matches!(
            format,
            TransferFormat::SqlStructure | TransferFormat::SqlData
        ) {
            snapshot.dump_structure(table)?
        } else {
            (String::new(), String::new())
        };
        let qualified = format!(
            "{}.{}",
            identifier(self.profile.protocol, &table.schema)?,
            identifier(self.profile.protocol, &table.name)?
        );
        let columns = snapshot.grid_columns(table)?;
        // 功能：新建文件拒绝覆盖，SQL 结构元数据校验完成后才创建；时间：2026-11-12；作者：lq。
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|e| format!("无法创建导出文件（已有文件不会覆盖）：{e}"))?;
        let result = (|| {
            let mut workbook = rust_xlsxwriter::Workbook::new();
            let worksheet = workbook.add_worksheet_with_constant_memory();
            if format == TransferFormat::Csv {
                // 功能：UTF-8 BOM 便于 Excel 直接打开中文 CSV；时间：2026-11-12；作者：lq。
                file.write_all(b"\xEF\xBB\xBF").map_err(io_error)?;
                file.write_all(&csv_record(columns.iter().map(|c| c.name.clone()))?)
                    .map_err(io_error)?;
            } else if format == TransferFormat::Excel {
                if columns.len() > 16384 {
                    return Err("字段数超过 Excel 上限".into());
                }
                for (i, c) in columns.iter().enumerate() {
                    worksheet
                        .write_string(0, i as u16, &c.name)
                        .map_err(io_error)?;
                }
            } else {
                file.write_all(prefix.as_bytes()).map_err(io_error)?;
            }
            let mut count = 0u64;
            if format != TransferFormat::SqlStructure
                && !(format == TransferFormat::SqlData && table.kind != "BASE TABLE")
            {
                // 功能：PostgreSQL 非 bytea 字段用服务端文本表示，保留数组/枚举等特殊类型的可恢复文字；时间：2026-11-12；作者：lq。
                let projection = if self.profile.protocol == Protocol::PostgreSql {
                    columns
                        .iter()
                        .map(|c| {
                            let name = identifier(Protocol::PostgreSql, &c.name)?;
                            Ok::<_, String>(if c.cast == "\"pg_catalog\".\"bytea\"" {
                                name.clone()
                            } else {
                                format!("{name}::text AS {name}")
                            })
                        })
                        .collect::<Result<Vec<_>, _>>()?
                        .join(", ")
                } else {
                    "*".into()
                };
                let sql = format!("SELECT {projection} FROM {qualified}");
                let mut connection = snapshot.connection.lock().map_err(|_| "导出连接不可用")?;
                snapshot.runtime.as_ref().expect("live runtime").block_on(async {
                    macro_rules! stream {
                        ($conn:expr, $decode:ident) => {{
                            let mut rows = sqlx::raw_sql(&sql).fetch($conn);
                            while let Some(row) = rows.try_next().await.map_err(safe_error)? {
                                let cells = $decode(&row)?;
                                count += 1;
                                match format {
                                    TransferFormat::Csv => {
                                        file.write_all(&csv_record(cells.iter().map(exchange_text))?).map_err(io_error)?;
                                    }
                                    TransferFormat::Excel => {
                                        if count > 1_048_575 { return Err("行数超过 Excel 单工作表上限，请使用 CSV 或 SQL".into()); }
                                        for (i,cell) in cells.iter().enumerate() {
                                            // 功能：全部写文本保留大整数、前导零并避免公式执行；时间：2026-11-12；作者：lq。
                                            worksheet.write_string(count as u32,i as u16,exchange_text(cell)).map_err(io_error)?;
                                        }
                                    }
                                    _ => { file.write_all(snapshot.dump_insert(table,&columns,&cells)?.as_bytes()).map_err(io_error)?; }
                                }
                            }
                        }};
                    }
                    match &mut *connection { Adapter::Pg(c) => stream!(c,decode_pg), Adapter::MySql(c) => stream!(c,decode_mysql) }
                    Ok::<(),String>(())
                })?;
            }
            match format {
                TransferFormat::Csv => {}
                TransferFormat::Excel => {
                    workbook.save_to_writer(&mut file).map_err(io_error)?;
                }
                _ => {
                    file.write_all(suffix.as_bytes()).map_err(io_error)?;
                }
            }
            file.sync_all().map_err(io_error)?;
            Ok(count)
        })();
        drop(file);
        if result.is_err() {
            let _ = std::fs::remove_file(path);
        }
        // 功能：独立导出会话结束时回滚只读快照，不触碰用户手动事务；时间：2026-11-12；作者：lq。
        result
    }

    /// 功能：表头按名称映射，允许无主键表插入，单事务整批提交，失败全部回滚；时间：2026-11-12；作者：lq。
    pub fn import_table(&self, table: &Table, data: &ResultSet) -> Result<u64, String> {
        if self.closed.load(Ordering::SeqCst)
            || self.raw_transaction.load(Ordering::SeqCst)
            || self.session_transaction_mode.load(Ordering::SeqCst)
        {
            return Err("请先结束手动事务，失效连接或修改过 autocommit 的连接需重连后导入".into());
        }
        if table.kind != "BASE TABLE" {
            return Err("仅基础表支持导入数据".into());
        }
        validate_import(data)?;
        let columns = self.grid_columns(table)?;
        let fields = data
            .columns
            .iter()
            .map(|name| {
                columns
                    .iter()
                    .find(|c| c.name == *name)
                    .ok_or_else(|| format!("表中不存在字段：{name}"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        if fields.iter().any(|c| c.generated || !c.writable) {
            return Err(
                "导入包含生成列、自动身份列或暂不支持的字段类型，请移除这些列后重试".into(),
            );
        }
        let p = self.profile.protocol;
        let qualified = format!(
            "{}.{}",
            identifier(p, &table.schema)?,
            identifier(p, &table.name)?
        );
        let names = data
            .columns
            .iter()
            .map(|s| identifier(p, s))
            .collect::<Result<Vec<_>, _>>()?
            .join(", ");
        let mut conn = self.connection.try_lock().map_err(|_| "连接正在执行查询")?;
        self.runtime.as_ref().expect("live runtime").block_on(async {
            macro_rules! insert {
                ($db:ty, $conn:expr, $pg:expr) => {{
                    if $conn.is_in_transaction() { return Err("请先结束手动事务".into()); }
                    let mut tx = $conn.begin().await.map_err(safe_error)?;
                    if !$pg {
                        let engine: Option<String> = sqlx::query_scalar("SELECT ENGINE FROM information_schema.tables WHERE table_schema=? AND table_name=?").bind(&table.schema).bind(&table.name).fetch_one(&mut *tx).await.map_err(safe_error)?;
                        if engine.as_deref()!=Some("InnoDB") { return Err("导入仅支持 InnoDB 事务表".into()); }
                    }
                    for (row_index,row) in data.rows.iter().enumerate() {
                        let mut q = sqlx::QueryBuilder::<$db>::new(format!("INSERT INTO {qualified} ({names}) VALUES ("));
                        for (i,cell) in row.iter().enumerate() {
                            if i>0 { q.push(", "); }
                            // 功能：数据库类型取可信元数据，值参数化防止 SQL 注入；时间：2026-11-12；作者：lq。
                            if $pg { q.push("CAST("); }
                            match cell {
                                Cell::Null => { q.push_bind(None::<String>); }
                                Cell::Text(s) => { q.push_bind(Some(s.clone())); }
                                Cell::Binary(_) => return Err("二进制列请使用 SQL 转储导入".into()),
                            }
                            if $pg {
                                q.push(format!(" AS {})",fields[i].cast));
                            }
                        }
                        q.push(")");
                        q.build().execute(&mut *tx).await.map_err(|e| format!("第 {} 行导入失败，全部回滚：{}",row_index+2,safe_error(e)))?;
                    }
                    tx.commit().await.map_err(safe_error)?;
                }};
            }
            match &mut *conn { Adapter::Pg(c) => insert!(sqlx::Postgres,c,true), Adapter::MySql(c) => insert!(sqlx::MySql,c,false) }
            Ok(data.rows.len() as u64)
        })
    }
}

/// 功能：统一文本交换转义，NULL=\N、二进制=\x十六进制、文本反斜线加倍；时间：2026-11-12；作者：lq。
fn exchange_text(cell: &Cell) -> String {
    match cell {
        Cell::Null => "\\N".into(),
        Cell::Binary(b) => format!("\\x{}", hex(b)),
        Cell::Text(s) if s.starts_with('\\') => format!("\\{s}"),
        Cell::Text(s) => s.clone(),
    }
}
/// 功能：CSV 与 Excel 共用 NULL 和反斜线规则，空单元格保留空字符串；时间：2026-11-12；作者：lq。
fn exchange_cell(s: String) -> Cell {
    if s == "\\N" {
        Cell::Null
    } else if s.starts_with("\\\\") {
        Cell::Text(s[1..].into())
    } else {
        Cell::Text(s)
    }
}
/// 功能：读取前限制文件大小，解析后核对表头和总数据量；时间：2026-11-12；作者：lq。
pub fn read_import(path: &Path) -> Result<ResultSet, String> {
    if std::fs::metadata(path).map_err(io_error)?.len() > 64 * 1024 * 1024 {
        return Err("导入文件超过 64 MiB".into());
    }
    let ext = path
        .extension()
        .and_then(|v| v.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let mut set = ResultSet::default();
    if ext == "csv" {
        let mut reader = csv::ReaderBuilder::new()
            .from_path(path)
            .map_err(io_error)?;
        set.columns = reader
            .headers()
            .map_err(io_error)?
            .iter()
            .map(str::to_owned)
            .collect();
        let mut bytes = 0;
        for record in reader.records() {
            let record = record.map_err(io_error)?;
            let row: Vec<_> = record.iter().map(|s| exchange_cell(s.into())).collect();
            bytes += row.iter().map(Cell::bytes).sum::<usize>();
            if set.rows.len() >= 100_000 || bytes > 64 * 1024 * 1024 {
                return Err("导入超过 100000 行或解码后 64 MiB".into());
            }
            set.rows.push(row);
        }
    } else if ["xlsx", "xls"].contains(&ext.as_str()) {
        // 功能：Excel 只导入首个工作表，首行必须为字段名，不执行公式；时间：2026-11-12；作者：lq。
        let mut book = open_workbook_auto(path).map_err(io_error)?;
        let range = book
            .worksheet_range_at(0)
            .ok_or("Excel 没有工作表")?
            .map_err(io_error)?;
        if range.height() > 100_001 || range.width() > 16384 {
            return Err("Excel 超过 100000 行或字段数上限".into());
        }
        let mut rows = range.rows();
        let header = rows.next().ok_or("Excel 为空")?;
        set.columns = header.iter().map(ToString::to_string).collect();
        let mut bytes = 0;
        for row in rows {
            if row.iter().any(|v| matches!(v, Data::Error(_))) {
                return Err("Excel 包含错误单元格，请修正后导入".into());
            }
            let row: Vec<_> = row.iter().map(|v| exchange_cell(v.to_string())).collect();
            bytes += row.iter().map(Cell::bytes).sum::<usize>();
            if bytes > 64 * 1024 * 1024 {
                return Err("Excel 解码后超过 64 MiB".into());
            }
            set.rows.push(row);
        }
    } else {
        return Err("请选择 CSV、XLSX 或 XLS 文件；SQL 文件请使用运行 SQL 文件菜单".into());
    }
    validate_import(&set)?;
    Ok(set)
}
/// 功能：拒绝空、重复、不完整表头和不规则行，避免错列写入；时间：2026-11-12；作者：lq。
fn validate_import(set: &ResultSet) -> Result<(), String> {
    let mut names = std::collections::HashSet::new();
    if set.columns.is_empty() || set.rows.is_empty() || set.rows.len() > 100_000 {
        return Err("需要字段表头和 1–100000 行数据".into());
    }
    for name in &set.columns {
        if name.is_empty() || !names.insert(name) {
            return Err("表头有空字段或重复字段".into());
        }
    }
    if set.rows.iter().any(|r| r.len() != set.columns.len()) {
        return Err("数据行字段数与表头不匹配".into());
    }
    Ok(())
}
/// 功能：元数据 NULL 和二进制文字统一转换，不用于业务值；时间：2026-11-12；作者：lq。
fn text(cell: &Cell) -> String {
    match cell {
        Cell::Null => String::new(),
        Cell::Text(s) => s.clone(),
        Cell::Binary(b) => String::from_utf8_lossy(b).into_owned(),
    }
}
/// 功能：文件/解析错误保留可读原因；时间：2026-11-12；作者：lq。
fn io_error(error: impl std::fmt::Display) -> String {
    format!("文件交换失败：{error}")
}

/// 功能：逐条编码 CSV，内存只保留当前行；时间：2026-11-12；作者：lq。
fn csv_record(values: impl IntoIterator<Item = String>) -> Result<Vec<u8>, String> {
    let mut writer = csv::Writer::from_writer(Vec::new());
    writer.write_record(values).map_err(io_error)?;
    writer.into_inner().map_err(io_error)
}
