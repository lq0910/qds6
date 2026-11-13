//! 功能：仅在专用回环数据库验证文件往返、整表导出、表副本和官方整库备份恢复；时间：2026-11-12；作者：lq。
use qds6_core::{ConnectionProfile, Protocol, TlsMode};
use qds6_services::database::{
    Cell, DatabaseSession, ResultSet, Table, TransferFormat, read_import,
};
use std::path::Path;

/// 功能：固定测试端口与凭据，不读取用户业务连接；时间：2026-11-12；作者：lq。
fn session(p: Protocol, database: &str) -> DatabaseSession {
    let mut profile = ConnectionProfile::new(p);
    profile.name = "transfer fixture".into();
    profile.host = "127.0.0.1".into();
    profile.port = if p == Protocol::PostgreSql {
        25433
    } else {
        23307
    };
    profile.username = "qds6".into();
    profile.database = database.into();
    profile.db.tls = TlsMode::Disabled;
    DatabaseSession::connect(&profile, "qds6-fixture-only", None).unwrap()
}
/// 功能：跨方言实测精确整数、中文、引号、换行、反斜线、NULL 和空字符串；时间：2026-11-12；作者：lq。
fn check(p: Protocol, root: &Path) {
    let db = session(p, "qds6");
    let pg = p == Protocol::PostgreSql;
    let schema = if pg { "public" } else { "qds6" };
    let table = Table {
        schema: schema.into(),
        name: format!("transfer_{}", uuid::Uuid::new_v4().simple()),
        kind: "BASE TABLE".into(),
    };
    let target = format!("{}.{}", schema, table.name);
    db.query(&format!("CREATE TABLE {target}(id BIGINT PRIMARY KEY,name TEXT,amount DECIMAL(30,8),note TEXT); CREATE INDEX transfer_name_idx ON {target}(name{});",if pg{""}else{"(64)"})).unwrap();
    if pg {
        db.query(&format!("COMMENT ON TABLE {target} IS E'中文备注 \\\\ path'; COMMENT ON COLUMN {target}.name IS '名称';")).unwrap();
    }
    let mut data = ResultSet {
        columns: vec!["id".into(), "name".into(), "amount".into(), "note".into()],
        ..Default::default()
    };
    for i in 0..10010 {
        data.rows.push(vec![
            Cell::Text((9007199254740993u64 + i).to_string()),
            Cell::Text(if i % 3 == 0 {
                "中文,\"引号\"\n换行 \\N".into()
            } else if i % 3 == 1 {
                String::new()
            } else {
                "=1+2".into()
            }),
            Cell::Text("12345678901234567890.12345678".into()),
            if i % 2 == 0 {
                Cell::Null
            } else {
                Cell::Text("\\N".into())
            },
        ]);
    }
    assert_eq!(db.import_table(&table, &data).unwrap(), 10010);
    let props = db.table_properties().unwrap();
    let props = props.get(&(schema.into(), table.name.clone())).unwrap();
    assert_eq!(props.primary_key, "id");
    assert!(!props.bytes.is_empty());
    for format in [TransferFormat::Csv, TransferFormat::Excel] {
        let file = root.join(format!(
            "{}-{}.{}",
            if pg { "pg" } else { "mysql" },
            table.name,
            format.extension()
        ));
        assert_eq!(db.export_table(&table, format, &file).unwrap(), 10010);
        assert!(db.export_table(&table, format, &file).is_err());
        let parsed = read_import(&file).unwrap();
        assert_eq!(parsed.rows, data.rows);
        assert_eq!(parsed.columns, data.columns);
        db.query(&format!("TRUNCATE TABLE {target}")).unwrap();
        assert_eq!(db.import_table(&table, &parsed).unwrap(), 10010);
    }
    // 功能：重复主键导致整个批次回滚，前面成功插入的行不能泄漏；时间：2026-11-12；作者：lq。
    let mut bad = data.clone();
    bad.rows = vec![
        vec![Cell::Text("1".into()), Cell::Null, Cell::Null, Cell::Null],
        data.rows[0].clone(),
    ];
    assert!(db.import_table(&table, &bad).unwrap_err().contains("回滚"));
    assert_eq!(
        db.query(
            &format!("SELECT COUNT(*)::text FROM {target}")
                .replace("::text", if pg { "::text" } else { "" })
        )
        .unwrap()
        .sets[0]
            .rows[0][0],
        Cell::Text("10010".into())
    );
    let structure = root.join(format!("{}-structure.sql", table.name));
    db.export_table(&table, TransferFormat::SqlStructure, &structure)
        .unwrap();
    assert!(
        !std::fs::read_to_string(structure)
            .unwrap()
            .contains("INSERT INTO")
    );
    let dump = root.join(format!("{}.sql", table.name));
    db.export_table(&table, TransferFormat::SqlData, &dump)
        .unwrap();
    db.query(&format!("DROP TABLE {target}")).unwrap();
    db.query(&std::fs::read_to_string(&dump).unwrap()).unwrap();
    assert_eq!(
        db.browse_page(&table, &Default::default())
            .unwrap()
            .data
            .rows[0],
        data.rows[0]
    );
    if pg {
        assert_eq!(
            db.table_properties()
                .unwrap()
                .get(&(schema.into(), table.name.clone()))
                .unwrap()
                .comment,
            "中文备注 \\ path"
        );
    }
    // 功能：身份、serial 和计算列副本可继续插入，连续复制自动避重名；时间：2026-11-12；作者：lq。
    let generated = Table {
        name: format!("{}_generated", table.name),
        ..table.clone()
    };
    let genq = format!("{}.{}", schema, generated.name);
    db.query(&format!("CREATE TABLE {genq}(id {} PRIMARY KEY, value INT DEFAULT 7, computed INT GENERATED ALWAYS AS (value*2) STORED); INSERT INTO {genq}(value) VALUES (8),(9);",if pg{"BIGINT GENERATED ALWAYS AS IDENTITY"}else{"BIGINT AUTO_INCREMENT"})).unwrap();
    let copy = db.copy_table(&generated, true).unwrap();
    let copy2 = db.copy_table(&generated, false).unwrap();
    assert_ne!(copy.name, copy2.name);
    let cq = format!("{}.{}", schema, copy.name);
    db.query(&format!("INSERT INTO {cq}(value) VALUES(10)"))
        .unwrap();
    assert_eq!(
        db.browse_page(&copy, &Default::default())
            .unwrap()
            .data
            .rows
            .len(),
        3
    );
    assert_eq!(
        db.browse_page(&generated, &Default::default())
            .unwrap()
            .data
            .rows
            .len(),
        2
    );
    let generated_dump = root.join(format!("{}-generated.sql", table.name));
    db.export_table(&generated, TransferFormat::SqlData, &generated_dump)
        .unwrap();
    db.query(&format!("DROP TABLE {genq}")).unwrap();
    db.query(&std::fs::read_to_string(generated_dump).unwrap())
        .unwrap();
    db.query(&format!("INSERT INTO {genq}(value) VALUES(11)"))
        .unwrap();
    if pg {
        let serial = Table {
            name: format!("{}_serial", table.name),
            ..table.clone()
        };
        db.query(&format!("CREATE TABLE public.{}(id BIGSERIAL PRIMARY KEY,value TEXT); INSERT INTO public.{}(value) VALUES ('serial');",serial.name,serial.name)).unwrap();
        let cp = db.copy_table(&serial, true).unwrap();
        db.query(&format!(
            "INSERT INTO public.{}(value) VALUES('new');",
            cp.name
        ))
        .unwrap();
        db.query(&format!(
            "DROP TABLE public.{}; DROP TABLE public.{};",
            serial.name, cp.name
        ))
        .unwrap();
    }
    if let Ok(dir) = std::env::var("QDS6_TEST_CLIENT_DIR") {
        let file = root.join(if pg { "full-pg.sql" } else { "full-mysql.sql" });
        db.database_sql_file(&file, false, Some(Path::new(&dir)))
            .unwrap();
        let restore = session(p, "qds6_restore");
        restore
            .database_sql_file(&file, true, Some(Path::new(&dir)))
            .unwrap();
        let restored = Table {
            schema: if pg {
                "public".into()
            } else {
                "qds6_restore".into()
            },
            ..table.clone()
        };
        assert_eq!(
            restore
                .browse_page(&restored, &Default::default())
                .unwrap()
                .data
                .rows[0],
            data.rows[0]
        );
        println!("PASS {} official full database dump/restore", p.label());
    }
    for t in [&table, &generated, &copy, &copy2] {
        db.query(&format!("DROP TABLE {}.{}", schema, t.name))
            .unwrap();
    }
    println!(
        "PASS {}: 10010-row CSV/XLSX roundtrip, precise decimal/integer, NULL/empty/backslash/formula text, transactional rollback, no overwrite, SQL DDL/data restore, independent copies/identity/generated/serial, real properties",
        p.label()
    );
}
/// 功能：仅将证据写入输出目录，不标记安装验收；时间：2026-11-12；作者：lq。
fn main() {
    let root = Path::new("output/database-transfer");
    std::fs::create_dir_all(root).unwrap();
    check(Protocol::PostgreSql, root);
    check(Protocol::MySql, root);
}
