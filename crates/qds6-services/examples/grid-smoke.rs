//! 功能：仅针对本机专用容器验证表网格读写、回滚及并发冲突；时间：2026-11-11；作者：lq。
use qds6_core::{ConnectionProfile, Protocol, TlsMode};
use qds6_services::database::{BrowseRequest, Cell, DatabaseSession, PAGE_SIZE, RowChange, Table};

/// 功能：专用测试库使用固定回环端口，不读取任何业务配置；时间：2026-11-11；作者：lq。
fn session(protocol: Protocol, port: u16) -> DatabaseSession {
    let mut p = ConnectionProfile::new(protocol);
    p.name = "grid fixture".into();
    p.host = "127.0.0.1".into();
    p.port = port;
    p.username = "qds6".into();
    p.database = "qds6".into();
    p.db.tls = TlsMode::Disabled;
    DatabaseSession::connect(&p, "qds6-fixture-only", None).unwrap()
}
/// 功能：测试默认值、空字符串、NULL、复合主键更新及批次原子性；时间：2026-11-11；作者：lq。
fn check(protocol: Protocol, port: u16) {
    let db = session(protocol, port);
    let mut table = Table {
        schema: if protocol == Protocol::PostgreSql {
            "public"
        } else {
            "qds6"
        }
        .into(),
        name: format!("grid_{}", uuid::Uuid::new_v4().simple()),
        kind: "BASE TABLE".into(),
    };
    let q = |table: &Table| format!("{}.{}", table.schema, table.name);
    db.query(&format!("CREATE TABLE {} (a INT NOT NULL, b INT NOT NULL, value TEXT, amount INT NOT NULL DEFAULT 7, PRIMARY KEY(a,b))", q(&table))).unwrap();
    let empty = db.browse_page(&table, &BrowseRequest::default()).unwrap();
    assert!(empty.editable);
    assert_eq!(empty.data.columns.len(), 4);
    assert_eq!(empty.data.rows.len(), 0);
    assert_eq!(empty.columns.iter().filter(|c| c.primary).count(), 2);
    let insert = |a: i32, b: i32, v: Cell| RowChange {
        original: None,
        values: vec![
            Some(Cell::Text(a.to_string())),
            Some(Cell::Text(b.to_string())),
            Some(v),
            None,
        ],
        delete: false,
    };
    db.save_rows(
        &table,
        &empty.columns,
        &[
            insert(1, 1, Cell::Text("中文 ' quote % _".into())),
            insert(1, 2, Cell::Text(String::new())),
            insert(1, 3, Cell::Null),
        ],
    )
    .unwrap();
    let page = db.browse_page(&table, &BrowseRequest::default()).unwrap();
    assert_eq!(page.data.rows.len(), 3);
    assert_eq!(page.data.rows[0][3], Cell::Text("7".into()));
    assert_eq!(page.data.rows[1][2], Cell::Text(String::new()));
    assert_eq!(page.data.rows[2][2], Cell::Null);
    let mut update = RowChange {
        original: Some(page.data.rows[0].clone()),
        values: vec![
            None,
            Some(Cell::Text("11".into())),
            Some(Cell::Text("changed".into())),
            None,
        ],
        delete: false,
    };
    db.save_rows(&table, &page.columns, &[update.clone()])
        .unwrap();
    // 功能：第二次使用旧快照拒绝，不能误覆盖其他会话修改；时间：2026-11-11；作者：lq。
    assert!(
        db.save_rows(&table, &page.columns, &[update.clone()])
            .unwrap_err()
            .contains("其他会话")
    );
    let page = db.browse_page(&table, &BrowseRequest::default()).unwrap();
    update.original = Some(page.data.rows[2].clone());
    update.values = vec![None, None, Some(Cell::Text("rollback-me".into())), None];
    assert!(
        db.save_rows(&table, &page.columns, &[update, insert(1, 2, Cell::Null)])
            .is_err()
    );
    let after = db.browse_page(&table, &BrowseRequest::default()).unwrap();
    assert_eq!(after.data.rows, page.data.rows);
    let filtered = db
        .browse_page(
            &table,
            &BrowseRequest {
                page: 0,
                filter: Some(("value".into(), "changed".into())),
            },
        )
        .unwrap();
    assert_eq!(filtered.data.rows.len(), 1);
    assert_eq!(
        db.browse_page(
            &table,
            &BrowseRequest {
                page: 0,
                filter: Some(("value".into(), "' OR 1=1 --".into()))
            }
        )
        .unwrap()
        .data
        .rows
        .len(),
        0
    );
    db.save_rows(
        &table,
        &page.columns,
        &[RowChange {
            original: Some(page.data.rows[0].clone()),
            values: vec![None; 4],
            delete: true,
        }],
    )
    .unwrap();
    assert_eq!(
        db.browse_page(&table, &BrowseRequest::default())
            .unwrap()
            .data
            .rows
            .len(),
        2
    );
    db.query("BEGIN").unwrap();
    assert!(
        db.save_rows(&table, &page.columns, &[insert(9, 9, Cell::Null)])
            .unwrap_err()
            .contains("手动事务")
    );
    db.query("ROLLBACK").unwrap();
    let rows: Vec<_> = (2..(PAGE_SIZE + 5))
        .map(|i| insert(i as i32, 1, Cell::Null))
        .collect();
    db.save_rows(&table, &page.columns, &rows).unwrap();
    let first = db.browse_page(&table, &BrowseRequest::default()).unwrap();
    assert_eq!(first.data.rows.len(), PAGE_SIZE);
    assert!(first.has_next);
    let second = db
        .browse_page(
            &table,
            &BrowseRequest {
                page: 1,
                filter: None,
            },
        )
        .unwrap();
    assert!(!second.data.rows.is_empty());
    assert!(!second.has_next);
    db.query(&format!(
        "CREATE VIEW {}_view AS SELECT * FROM {}",
        q(&table),
        q(&table)
    ))
    .unwrap();
    let view = Table {
        name: format!("{}_view", table.name),
        kind: "VIEW".into(),
        ..table.clone()
    };
    assert!(
        !db.browse_page(&view, &BrowseRequest::default())
            .unwrap()
            .editable
    );
    assert!(
        db.save_rows(&view, &page.columns, &[insert(999, 1, Cell::Null)])
            .is_err()
    );
    db.query(&format!(
        "DROP VIEW {}_view; DROP TABLE {}",
        q(&table),
        q(&table)
    ))
    .unwrap();
    table.name.push_str("_identity");
    db.query(&format!(
        "CREATE TABLE {} (id {} PRIMARY KEY, value TEXT)",
        q(&table),
        if protocol == Protocol::PostgreSql {
            "BIGINT GENERATED ALWAYS AS IDENTITY"
        } else {
            "BIGINT AUTO_INCREMENT"
        }
    ))
    .unwrap();
    let identity = db.browse_page(&table, &BrowseRequest::default()).unwrap();
    assert!(identity.editable);
    db.save_rows(
        &table,
        &identity.columns,
        &[RowChange {
            original: None,
            values: vec![None, Some(Cell::Text("default id".into()))],
            delete: false,
        }],
    )
    .unwrap();
    assert_eq!(
        db.browse_page(&table, &BrowseRequest::default())
            .unwrap()
            .data
            .rows
            .len(),
        1
    );
    db.query(&format!("DROP TABLE {}", q(&table))).unwrap();
    println!(
        "PASS {}: empty columns, composite keys, default/empty/NULL, insert/update/delete, key edits, conflict rejection, whole-batch rollback, SQL filter binding, pagination, read-only views, manual transaction protection, identity defaults",
        protocol.label()
    );
}
/// 功能：分别验收两种数据库，不将编译成功等同真实写入；时间：2026-11-11；作者：lq。
fn main() {
    check(Protocol::PostgreSql, 25433);
    check(Protocol::MySql, 23307);
}
