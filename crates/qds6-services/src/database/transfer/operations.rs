//! 功能：同级副本表和原生客户端整库 SQL 备份/恢复，秘密不放命令行；时间：2026-11-12；作者：lq。
use super::*;
use std::{
    fs::File,
    path::PathBuf,
    process::{Command, Stdio},
};

impl DatabaseSession {
    /// 功能：生成同模式不重名副本，复制字段、索引、默认值和数据，失败回滚/删除新表；时间：2026-11-12；作者：lq。
    pub fn copy_table(&self, source: &Table, with_data: bool) -> Result<Table, String> {
        if source.kind != "BASE TABLE" {
            return Err("仅基础表可复制".into());
        }
        let db = Self::connect(&self.profile, &self.secret, self._tunnel.clone())?;
        let tables = db.tables()?;
        let p = self.profile.protocol;
        let mut base = source.name.clone();
        let max = if p == Protocol::PostgreSql { 50 } else { 51 };
        while base.len() > max {
            base.pop();
        }
        let mut target = source.clone();
        for i in 1..=10000 {
            target.name = if i == 1 {
                format!("{base}_copy")
            } else {
                format!("{base}_copy_{i}")
            };
            if !tables
                .iter()
                .any(|t| t.schema == target.schema && t.name == target.name)
            {
                break;
            }
        }
        let q = |t: &Table| {
            Ok::<_, String>(format!(
                "{}.{}",
                identifier(p, &t.schema)?,
                identifier(p, &t.name)?
            ))
        };
        let src = q(source)?;
        let dst = q(&target)?;
        let columns = db.grid_columns(source)?;
        let fields = columns
            .iter()
            .filter(|c| !c.extra.contains("GENERATED"))
            .map(|c| identifier(p, &c.name))
            .collect::<Result<Vec<_>, _>>()?
            .join(", ");
        if p == Protocol::PostgreSql {
            db.query("BEGIN")?;
        }
        let mut created = false;
        let result = (|| {
            db.query(&if p == Protocol::PostgreSql {
                format!("CREATE TABLE {dst} (LIKE {src} INCLUDING ALL)")
            } else {
                format!("CREATE TABLE {dst} LIKE {src}")
            })?;
            created = true;
            // 功能：serial 默认序列不能与源表共享，复制后创建独立所属序列；时间：2026-11-12；作者：lq。
            if p == Protocol::PostgreSql {
                for c in &columns {
                    if c.default
                        .as_ref()
                        .is_some_and(|d| d.starts_with("nextval("))
                    {
                        let seq = format!("qds6_copy_{}", uuid::Uuid::new_v4().simple());
                        let seq_name = format!(
                            "{}.{}",
                            identifier(p, &target.schema)?,
                            identifier(p, &seq)?
                        );
                        let col = identifier(p, &c.name)?;
                        db.query(&format!("CREATE SEQUENCE {seq_name} OWNED BY {dst}.{col}; ALTER TABLE {dst} ALTER COLUMN {col} SET DEFAULT nextval({}::regclass)",literal(&seq_name)))?;
                    }
                }
                let fk=db.query(&format!("SELECT pg_get_constraintdef(oid,true) FROM pg_constraint WHERE conrelid={}::regclass AND contype='f'",literal(&src)))?;
                for row in fk.sets.into_iter().flat_map(|s| s.rows) {
                    db.query(&format!("ALTER TABLE {dst} ADD {}", text(&row[0])))?;
                }
            } else {
                // 功能：CREATE LIKE 不复制外键，按有序元数据补齐并生成唯一约束名；时间：2026-11-12；作者：lq。
                let fk=db.query(&format!("SELECT k.constraint_name,GROUP_CONCAT(CONCAT('`',REPLACE(k.column_name,'`','``'),'`') ORDER BY k.ordinal_position),k.referenced_table_schema,k.referenced_table_name,GROUP_CONCAT(CONCAT('`',REPLACE(k.referenced_column_name,'`','``'),'`') ORDER BY k.ordinal_position),r.update_rule,r.delete_rule FROM information_schema.key_column_usage k JOIN information_schema.referential_constraints r ON r.constraint_schema=k.constraint_schema AND r.constraint_name=k.constraint_name AND r.table_name=k.table_name WHERE k.table_schema={} AND k.table_name={} AND k.referenced_table_name IS NOT NULL GROUP BY k.constraint_name,k.referenced_table_schema,k.referenced_table_name,r.update_rule,r.delete_rule",literal(&source.schema),literal(&source.name)))?;
                for row in fk.sets.into_iter().flat_map(|s| s.rows) {
                    let id = identifier(p, &format!("fk_copy_{}", uuid::Uuid::new_v4().simple()))?;
                    db.query(&format!("ALTER TABLE {dst} ADD CONSTRAINT {id} FOREIGN KEY ({}) REFERENCES {}.{} ({}) ON UPDATE {} ON DELETE {}",text(&row[1]),identifier(p,&text(&row[2]))?,identifier(p,&text(&row[3]))?,text(&row[4]),text(&row[5]),text(&row[6])))?;
                }
            }
            if with_data {
                db.query(&format!(
                    "INSERT INTO {dst} ({fields}){} SELECT {fields} FROM {src}",
                    if p == Protocol::PostgreSql {
                        " OVERRIDING SYSTEM VALUE"
                    } else {
                        ""
                    }
                ))?;
            }
            if p == Protocol::PostgreSql {
                for c in &columns {
                    if c.extra == "IDENTITY"
                        || c.default
                            .as_ref()
                            .is_some_and(|d| d.starts_with("nextval("))
                    {
                        let col = identifier(p, &c.name)?;
                        // 功能：空副本保留初始序列状态；有数据时确保下一次插入不会撞主键；时间：2026-11-12；作者：lq。
                        if with_data {
                            db.query(&format!("SELECT setval(pg_get_serial_sequence({},{}), GREATEST(COALESCE(MAX({col}),1),1), COUNT(*)>0) FROM {dst}",literal(&dst),literal(&c.name)))?;
                        }
                    }
                }
                db.query("COMMIT")?;
            }
            Ok(target)
        })();
        if result.is_err() {
            if p == Protocol::PostgreSql {
                let _ = db.query("ROLLBACK");
            } else if created {
                let _ = db.query(&format!("DROP TABLE {dst}"));
            }
        }
        result
    }

    /// 功能：完整数据库备份/恢复委托官方客户端，包含表、视图、函数、触发器等，不拼装伪全库备份；时间：2026-11-12；作者：lq。
    pub fn database_sql_file(
        &self,
        path: &Path,
        restore: bool,
        tool_dir: Option<&Path>,
    ) -> Result<String, String> {
        let pg = self.profile.protocol == Protocol::PostgreSql;
        let name = match (pg, restore) {
            (true, false) => "pg_dump",
            (true, true) => "psql",
            (false, false) => "mysqldump",
            (false, true) => "mysql",
        };
        let program = find_client(name, tool_dir)?;
        let mut cmd = Command::new(program);
        let (host, port) = if let Some(t) = &self._tunnel {
            ("127.0.0.1", t.port)
        } else {
            (self.profile.host.as_str(), self.profile.port)
        };
        let database = &self.profile.database;
        if database.is_empty() {
            return Err("请在连接配置中指定数据库名称".into());
        }
        if pg {
            cmd.env("PGPASSWORD", self.secret.as_str()).env(
                "PGSSLMODE",
                if self.profile.db.tls == TlsMode::VerifyFull {
                    "verify-full"
                } else {
                    "disable"
                },
            );
            if !self.profile.db.ca_file.is_empty() {
                cmd.env("PGSSLROOTCERT", &self.profile.db.ca_file);
            }
            cmd.args([
                "--host",
                host,
                "--port",
                &port.to_string(),
                "--username",
                &self.profile.username,
                "--dbname",
                database,
                "--no-password",
            ]);
            if restore {
                cmd.args([
                    "-X",
                    "--set",
                    "ON_ERROR_STOP=1",
                    "--single-transaction",
                    "--file",
                ])
                .arg(path);
            } else {
                cmd.args(["--format=plain", "--no-owner", "--no-acl"]);
            }
        } else {
            cmd.env("MYSQL_PWD", self.secret.as_str());
            cmd.args([
                "--host",
                host,
                "--port",
                &port.to_string(),
                "--user",
                &self.profile.username,
                "--protocol=TCP",
                "--default-character-set=utf8mb4",
            ]);
            if self.profile.db.tls == TlsMode::VerifyFull {
                cmd.arg("--ssl-mode=VERIFY_IDENTITY");
                if !self.profile.db.ca_file.is_empty() {
                    cmd.arg(format!("--ssl-ca={}", self.profile.db.ca_file));
                }
            } else {
                cmd.arg("--ssl-mode=DISABLED");
            }
            if restore {
                cmd.arg(database)
                    .stdin(Stdio::from(File::open(path).map_err(io_error)?));
            } else {
                cmd.args([
                    "--single-transaction",
                    "--routines",
                    "--events",
                    "--triggers",
                    "--hex-blob",
                    "--no-tablespaces",
                    "--set-gtid-purged=OFF",
                    database,
                ]);
            }
        }
        if !restore {
            cmd.stdout(Stdio::from(
                OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(path)
                    .map_err(|e| format!("无法创建备份，已有文件不覆盖：{e}"))?,
            ));
        }
        // 功能：恢复进度以退出码和 stderr 为准，不缓存海量 INSERT 的标准输出；时间：2026-11-12；作者：lq。
        if restore {
            cmd.stdout(Stdio::null());
        }
        let result = cmd.output();
        let result = match result {
            Ok(output) if output.status.success() => {
                let info = String::from_utf8_lossy(&output.stderr);
                Ok(format!(
                    "{}完成 · {}{}",
                    if restore {
                        "SQL 文件执行"
                    } else {
                        "整库 SQL 备份"
                    },
                    path.display(),
                    if info.trim().is_empty() {
                        String::new()
                    } else {
                        format!("\n{}", info.chars().take(4000).collect::<String>())
                    }
                ))
            }
            Ok(output) => Err(format!(
                "{name} 执行失败：{}{}",
                String::from_utf8_lossy(&output.stderr)
                    .chars()
                    .take(8000)
                    .collect::<String>(),
                if restore && !pg {
                    "\nMySQL DDL 会隐式提交，错误前可能已有部分语句生效，请核对目标库。"
                } else {
                    ""
                }
            )),
            Err(e) => Err(format!("无法启动 {name}：{e}")),
        };
        if !restore && result.is_err() {
            let _ = std::fs::remove_file(path);
        }
        // 功能：客户端诊断兜底脱敏，密码仅放子进程环境而不进入参数/日志；时间：2026-11-12；作者：lq。
        let redact = |s: String| {
            if self.secret.is_empty() {
                s
            } else {
                s.replace(self.secret.as_str(), "[REDACTED]")
            }
        };
        result.map(redact).map_err(redact)
    }
}
/// 功能：优先显式客户端目录，再查应用随附目录、常见安装目录与 PATH，兼容 Finder 启动；时间：2026-11-12；作者：lq。
fn find_client(name: &str, explicit: Option<&Path>) -> Result<PathBuf, String> {
    let executable = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.into()
    };
    let mut dirs = Vec::new();
    if let Some(dir) = explicit
        && !dir.as_os_str().is_empty()
    {
        dirs.push(dir.to_path_buf());
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        dirs.push(dir.join("database-tools"));
    }
    dirs.extend(
        [
            "/opt/homebrew/opt/libpq/bin",
            "/opt/homebrew/bin",
            "/usr/local/bin",
            "/opt/homebrew/opt/mysql-client/bin",
            "/usr/local/opt/mysql-client/bin",
        ]
        .into_iter()
        .map(PathBuf::from),
    );
    if let Some(path) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&path));
    }
    dirs.into_iter().map(|d|d.join(&executable)).find(|p|p.is_file()).ok_or_else(||format!("未找到 {name}，请安装对应版本的 PostgreSQL/MySQL 客户端，在“客户端目录”中选择其 bin 目录"))
}
