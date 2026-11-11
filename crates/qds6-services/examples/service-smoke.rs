//! 功能：仅对 scripts/service-fixtures.sh 的隔离服务验收，输出不包含密码；时间：2026-11-10；作者：lq。

use qds6_core::{ConnectionProfile, Protocol, TlsMode};
use qds6_services::{
    database::{Cell, DatabaseSession, Edit},
    sftp::{Client, Transfer},
    ssh::{self, SshError},
    terminal::TerminalSession,
};
use std::{
    path::Path,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

/// 功能：创建专用回环服务配置，不读取用户配置或业务服务器；时间：2026-11-10；作者：lq。
fn profile(protocol: Protocol, port: u16) -> ConnectionProfile {
    let mut profile = ConnectionProfile::new(protocol);
    profile.name = format!("fixture {}", protocol.label());
    profile.host = "127.0.0.1".into();
    profile.port = port;
    profile.username = "qds6".into();
    profile.database = "qds6".into();
    profile.db.tls = TlsMode::Disabled;
    profile
}

/// 功能：等待可复现条件，限制单项验收时间；时间：2026-11-10；作者：lq。
fn eventually(mut condition: impl FnMut() -> bool) {
    let start = Instant::now();
    while !condition() {
        assert!(
            start.elapsed() < Duration::from_secs(15),
            "fixture check timed out"
        );
        thread::sleep(Duration::from_millis(50));
    }
}

/// 功能：验证两种真实数据库的类型、事务、上限、错误和服务端取消；时间：2026-11-10；作者：lq。
fn database_smoke(protocol: Protocol, port: u16) {
    let profile = profile(protocol, port);
    let session = Arc::new(
        DatabaseSession::connect(&profile, "qds6-fixture-only", None)
            .expect("connect fixture database"),
    );
    let sql = if protocol == Protocol::PostgreSql {
        "SELECT NULL AS n, ''::text AS empty_value, 9007199254740993::bigint AS big, 123456789.123456789::numeric AS exact, '中文'::text AS chinese, decode('00ff','hex') AS bin_value"
    } else {
        "SELECT NULL AS n, '' AS empty_value, CAST(18446744073709551615 AS UNSIGNED) AS big, CAST('123456789.123456789' AS DECIMAL(30,9)) AS exact, '中文' AS chinese, UNHEX('00ff') AS bin_value"
    };
    let result = session.query(sql).expect("query runtime types");
    let set = result
        .sets
        .iter()
        .find(|s| !s.rows.is_empty())
        .expect("rows");
    assert_eq!(set.rows[0][0], Cell::Null);
    assert_eq!(set.rows[0][1], Cell::Text(String::new()));
    assert_eq!(
        set.rows[0][2].display(),
        if protocol == Protocol::PostgreSql {
            "9007199254740993"
        } else {
            "18446744073709551615"
        }
    );
    assert_eq!(set.rows[0][3].display(), "123456789.123456789");
    assert_eq!(set.rows[0][4].display(), "中文");
    assert_eq!(set.rows[0][5], Cell::Binary(vec![0, 255]));
    session
        .query("CREATE TEMPORARY TABLE qds6_smoke (id INTEGER PRIMARY KEY, value VARCHAR(40))")
        .unwrap();
    session.query("BEGIN").unwrap();
    session
        .query("INSERT INTO qds6_smoke VALUES(1, 'rollback')")
        .unwrap();
    session.query("ROLLBACK").unwrap();
    let result = session
        .query("SELECT COUNT(*) AS total FROM qds6_smoke")
        .unwrap();
    assert_eq!(
        result
            .sets
            .iter()
            .find(|s| !s.rows.is_empty())
            .unwrap()
            .rows[0][0]
            .display(),
        "0"
    );
    assert!(
        session
            .query("SELECT definitely_missing_column FROM qds6_smoke")
            .is_err()
    );
    let worker = session.clone();
    let running = thread::spawn(move || {
        worker.query(if protocol == Protocol::PostgreSql {
            "SELECT pg_sleep(20)"
        } else {
            "SELECT SLEEP(20), 42"
        })
    });
    thread::sleep(Duration::from_millis(500));
    let start = Instant::now();
    session.cancel().expect("server cancel");
    let canceled = running.join().unwrap();
    if protocol == Protocol::PostgreSql {
        assert!(canceled.is_err());
    } else {
        // 功能：MySQL SLEEP 被 KILL QUERY 中断会返回 1，此结果加耗时证明服务端停止执行；时间：2026-11-10；作者：lq。
        let result = canceled.expect("MySQL interrupted SLEEP status");
        assert_eq!(
            result
                .sets
                .iter()
                .find(|s| !s.rows.is_empty())
                .unwrap()
                .rows[0][0]
                .display(),
            "1"
        );
    }
    assert!(start.elapsed() < Duration::from_secs(5));
    assert!(session.query("SELECT 42 AS recovered").is_ok());
    if protocol == Protocol::PostgreSql {
        let result = session
            .query("SELECT n FROM generate_series(1,100000) AS n")
            .unwrap();
        assert!(result.truncated);
        assert_eq!(
            result.sets.iter().map(|s| s.rows.len()).sum::<usize>(),
            10000
        );
    } else {
        // 功能：十万行真实 MySQL 数据源也按全局上限截断并取消；时间：2026-11-11；作者：lq。
        let digits = "(SELECT 0 n UNION ALL SELECT 1 UNION ALL SELECT 2 UNION ALL SELECT 3 UNION ALL SELECT 4 UNION ALL SELECT 5 UNION ALL SELECT 6 UNION ALL SELECT 7 UNION ALL SELECT 8 UNION ALL SELECT 9)";
        let result=session.query(&format!("SELECT a.n FROM {digits} a CROSS JOIN {digits} b CROSS JOIN {digits} c CROSS JOIN {digits} d CROSS JOIN {digits} e")).unwrap();
        assert!(result.truncated);
        assert_eq!(
            result.sets.iter().map(|s| s.rows.len()).sum::<usize>(),
            10000
        );
    }
    println!(
        "PASS {}: exact values, NULL/empty/binary, transaction rollback, errors, real server cancellation and reuse",
        protocol.label()
    );

    // 功能：专用测试表的元数据、参数化主键写入及缺少主键保护；时间：2026-11-10；作者：lq。
    session
        .query("DROP TABLE IF EXISTS qds6_grid_smoke")
        .unwrap();
    session
        .query("CREATE TABLE qds6_grid_smoke(id INTEGER PRIMARY KEY, value VARCHAR(80))")
        .unwrap();
    session
        .query("INSERT INTO qds6_grid_smoke VALUES(1,'original')")
        .unwrap();
    let tables = session.tables().unwrap();
    let table = tables
        .iter()
        .find(|t| t.name == "qds6_grid_smoke")
        .unwrap()
        .clone();
    assert!(!session.structure(&table).unwrap().sets.is_empty());
    let mut edit = Edit {
        table: table.clone(),
        key_column: "id".into(),
        key: "1".into(),
        column: "value".into(),
        value: Some("中文 ' ; DROP TABLE x; --".into()),
        delete: false,
    };
    session.apply_edit(&edit).unwrap();
    // 功能：原始 BEGIN 不得被网格内部事务提交，验证拒绝后仍可回滚；时间：2026-11-11；作者：lq。
    session.query("BEGIN").unwrap();
    assert!(session.apply_edit(&edit).is_err());
    session.query("ROLLBACK").unwrap();
    let rows = session.browse(&table).unwrap();
    assert_eq!(
        rows.sets.iter().find(|s| !s.rows.is_empty()).unwrap().rows[0][1].display(),
        "中文 ' ; DROP TABLE x; --"
    );
    edit.key_column = "value".into();
    assert!(session.apply_edit(&edit).is_err());
    edit.key_column = "id".into();
    edit.key = "999".into();
    assert!(session.apply_edit(&edit).is_err());
    edit.key = "1".into();
    edit.delete = true;
    session.apply_edit(&edit).unwrap();
    session.query("DROP TABLE qds6_grid_smoke").unwrap();
    let mut secure = profile.clone();
    secure.host = "localhost".into();
    secure.db.tls = TlsMode::VerifyFull;
    secure.db.ca_file = "output/fixtures/ca.crt".into();
    let tls = DatabaseSession::connect(&secure, "qds6-fixture-only", None)
        .expect("verified TLS localhost");
    tls.query("SELECT 1").unwrap();
    secure.host = "127.0.0.1".into();
    assert!(
        DatabaseSession::connect(&secure, "qds6-fixture-only", None).is_err(),
        "hostname mismatch must fail"
    );
    println!(
        "PASS {}: metadata, structure/indexes, bound grid update/delete, invalid primary key guard, verified TLS and hostname rejection",
        protocol.label()
    );
    // 功能：主动断开仅本次测试连接，验证旧会话停用而非隐式重连重放；时间：2026-11-11；作者：lq。
    let target = DatabaseSession::connect(&profile, "qds6-fixture-only", None).unwrap();
    let id_sql = if protocol == Protocol::PostgreSql {
        "SELECT pg_backend_pid()"
    } else {
        "SELECT CONNECTION_ID()"
    };
    let id = target
        .query(id_sql)
        .unwrap()
        .sets
        .iter()
        .find(|s| !s.rows.is_empty())
        .unwrap()
        .rows[0][0]
        .display()
        .parse::<u64>()
        .unwrap();
    session
        .query(&if protocol == Protocol::PostgreSql {
            format!("SELECT pg_terminate_backend({id})")
        } else {
            format!("KILL CONNECTION {id}")
        })
        .unwrap();
    assert!(target.query("SELECT 1").is_err());
    assert_eq!(
        target.query("SELECT 1").err().as_deref(),
        Some("连接已失效，请重新连接")
    );
    println!(
        "PASS {}: 100000 source bounded at 10000; disconnected dedicated session invalidated",
        protocol.label()
    );
}

/// 功能：验证真实 SSH 指纹、PTY、全屏程序与 SFTP 文件冲突；时间：2026-11-10；作者：lq。
fn ssh_smoke(root: &Path) {
    let profile = profile(Protocol::Ssh, 22222);
    let known = root.join("known.json");
    let fingerprint = match ssh::connect(&profile, Some("qds6-fixture-only"), &known) {
        Err(SshError::UnknownHost { fingerprint }) => fingerprint,
        _ => panic!("first host must be untrusted"),
    };
    ssh::trust_host(&known, &profile, &fingerprint).unwrap();
    let mut key_profile = profile.clone();
    key_profile.ssh.private_key = "output/fixtures/id_ed25519".into();
    assert!(
        ssh::connect(&key_profile, None, &known).is_ok(),
        "private key auth"
    );
    // 功能：仅在专用临时 Agent 环境验收，不读取用户常用 Agent；时间：2026-11-11；作者：lq。
    if std::env::var("QDS6_FIXTURE_AGENT").as_deref() == Ok("1") {
        let mut agent_profile = profile.clone();
        agent_profile.ssh.use_agent = true;
        assert!(
            ssh::connect(&agent_profile, None, &known).is_ok(),
            "fixture Agent auth"
        );
        println!("PASS isolated SSH Agent authentication");
    }

    // 功能：数据库隧道保持独立连接并允许同时接入控制取消连接；时间：2026-11-10；作者：lq。
    for (protocol, port) in [(Protocol::PostgreSql, 25432), (Protocol::MySql, 23306)] {
        let tunnel = Arc::new(
            ssh::TunnelLease::start(
                ssh::connect(&profile, Some("qds6-fixture-only"), &known).unwrap(),
                "host.docker.internal".into(),
                port,
            )
            .unwrap(),
        );
        let mut database_profile = crate::profile(protocol, port);
        database_profile.db.tunnel_profile = Some(profile.id);
        let database = Arc::new(
            DatabaseSession::connect(&database_profile, "qds6-fixture-only", Some(tunnel))
                .expect("database through SSH tunnel"),
        );
        database.query("SELECT 42").unwrap();
        let worker = database.clone();
        let running = thread::spawn(move || {
            worker.query(if protocol == Protocol::PostgreSql {
                "SELECT pg_sleep(20)"
            } else {
                "SELECT SLEEP(20),42"
            })
        });
        thread::sleep(Duration::from_millis(500));
        let start = Instant::now();
        database
            .cancel()
            .expect("cancel over second tunnel channel");
        let _ = running.join().unwrap();
        assert!(start.elapsed() < Duration::from_secs(5));
    }
    println!(
        "PASS SSH: private key authentication and both databases over dedicated SSH tunnel with server cancellation"
    );
    let session = ssh::connect(&profile, Some("qds6-fixture-only"), &known).unwrap();
    let terminal = TerminalSession::start(session, 100, 30).unwrap();
    terminal
        .input("printf 'QDS6终端验证\\n'; stty size\r".as_bytes().to_vec())
        .unwrap();
    eventually(|| terminal.model.lock().unwrap().text().contains("30 100"));
    terminal.resize(80, 24);
    thread::sleep(Duration::from_millis(200));
    terminal.input(b"stty size\r".to_vec()).unwrap();
    eventually(|| terminal.model.lock().unwrap().text().contains("24 80"));
    terminal
        .input(b"vim -Nu NONE -n /tmp/qds6-vim-smoke\r".to_vec())
        .unwrap();
    thread::sleep(Duration::from_millis(700));
    terminal
        .input("i中文 full screen\x1b:wq\r".as_bytes().to_vec())
        .unwrap();
    thread::sleep(Duration::from_millis(500));
    terminal.input(b"cat /tmp/qds6-vim-smoke; tmux -L qds6-smoke new-session -d -s smoke 'sleep 10'; tmux -L qds6-smoke list-sessions\r".to_vec()).unwrap();
    eventually(|| terminal.model.lock().unwrap().text().contains("smoke:"));
    let client =
        Client::new(ssh::connect(&profile, Some("qds6-fixture-only"), &known).unwrap()).unwrap();
    let remote_dir = format!("/home/qds6/qds6-smoke-{}", std::process::id());
    client.mkdir(Path::new(&remote_dir)).unwrap();
    let local = root.join("source.txt");
    std::fs::write(&local, b"first\r\nline\r\n").unwrap();
    let remote = Path::new(&remote_dir).join("中文.txt");
    client
        .upload(&local, &remote, &Transfer::default())
        .unwrap();
    assert!(
        client
            .upload(&local, &remote, &Transfer::default())
            .is_err()
    );
    let entries = client.list(Path::new(&remote_dir)).expect("directory list");
    assert_eq!(entries.len(), 1);
    let document = client.open_document(&remote).unwrap();
    assert!(document.crlf);
    // 功能：真实 Keychain 草稿加密、恢复、篡改拒绝和含凭据备份往返；时间：2026-11-10；作者：lq。
    let drafts = root.join("drafts");
    let draft = qds6_services::drafts::Draft {
        schema: 1,
        connection: profile.id,
        baseline: document.clone(),
        text: "private draft 中文".into(),
    };
    qds6_services::drafts::save(&drafts, &draft).unwrap();
    assert!(
        !String::from_utf8_lossy(&std::fs::read(draft.path(&drafts)).unwrap())
            .contains("private draft")
    );
    assert_eq!(
        qds6_services::drafts::load(&drafts, profile.id)
            .unwrap()
            .unwrap()
            .text,
        draft.text
    );
    let mut bytes = std::fs::read(draft.path(&drafts)).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 1;
    std::fs::write(draft.path(&drafts), bytes).unwrap();
    assert!(qds6_services::drafts::load(&drafts, profile.id).is_err());
    qds6_services::drafts::remove(&drafts, profile.id).unwrap();
    let mut portable = qds6_services::backup::Configuration::new(std::slice::from_ref(&profile));
    portable
        .secrets
        .insert(profile.id, "fixture portable secret".into());
    assert!(qds6_services::backup::encode(&portable, None).is_err());
    let encrypted = qds6_services::backup::encode(&portable, Some("fixture-password")).unwrap();
    let recovered = qds6_services::backup::decode(&encrypted, Some("fixture-password")).unwrap();
    let mut store = qds6_storage::ProfileStore::open(root.join("restored.db")).unwrap();
    recovered
        .restore(&mut store, &root.join("snapshot.json"), true)
        .unwrap();
    let reference = store.list().unwrap()[0].credential.clone().unwrap();
    assert_eq!(
        qds6_services::credentials::CredentialStore::get(&reference)
            .unwrap()
            .as_str(),
        "fixture portable secret"
    );
    qds6_services::credentials::CredentialStore::delete(&reference).unwrap();
    println!(
        "PASS encrypted drafts and credential backup: private cache, Keychain key, authenticated recovery, tamper refusal and portable credential restore"
    );
    let updated = client
        .save_document(&document, "second\r\nline\r\n")
        .expect("atomic remote save");
    assert_eq!(
        client.list(Path::new(&remote_dir)).unwrap()[0].permissions & 0o777,
        entries[0].permissions & 0o777
    );
    assert!(
        client
            .save_document(&document, "must not overwrite")
            .is_err()
    );
    let download = root.join("download.txt");
    client
        .download(&remote, &download, &Transfer::default())
        .unwrap();
    assert_eq!(std::fs::read_to_string(download).unwrap(), updated.text);
    let canceled = Transfer::default();
    canceled
        .cancel
        .store(true, std::sync::atomic::Ordering::Relaxed);
    assert!(
        client
            .upload(
                &local,
                &Path::new(&remote_dir).join("canceled.txt"),
                &canceled
            )
            .is_err()
    );
    client.chmod(&remote, 0o640).unwrap();
    client.remove(&remote).unwrap();
    client.remove(Path::new(&remote_dir)).unwrap();
    drop(terminal);
    assert!(
        client.list(Path::new("/home/qds6")).is_ok(),
        "closing terminal must not disconnect SFTP"
    );
    let mut changed: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&known).unwrap()).unwrap();
    changed["[127.0.0.1]:22222"] = "SHA256:changed".into();
    std::fs::write(&known, serde_json::to_vec(&changed).unwrap()).unwrap();
    assert!(matches!(
        ssh::connect(&profile, Some("qds6-fixture-only"), &known),
        Err(SshError::ChangedHost { .. })
    ));
    println!(
        "PASS SSH/SFTP: first-host confirmation, changed-host blocking, PTY resize, Chinese/Vim/tmux, upload/download, no-overwrite, conflict blocking, permission, cancel and independent lifecycle"
    );
}

/// 功能：显式环境开关防止意外服务联调，输出可保存为验收记录；时间：2026-11-10；作者：lq。
fn main() {
    assert_eq!(
        std::env::var("QDS6_FIXTURES").as_deref(),
        Ok("1"),
        "only run against dedicated qds6 fixtures"
    );
    let root = tempfile::tempdir().unwrap();
    // 功能：独立测试引用不读取或修改正常应用草稿密钥，完成后清理；时间：2026-11-11；作者：lq。
    #[cfg(feature = "test-support")]
    let draft_reference = {
        let id = qds6_core::CredentialRef(uuid::Uuid::new_v4());
        qds6_services::credentials::use_fixture_draft_reference(id.clone());
        id
    };
    let credential = qds6_core::CredentialRef(uuid::Uuid::new_v4());
    qds6_services::credentials::CredentialStore::set(&credential, "fixture-credential-only")
        .expect("Keychain write");
    assert_eq!(
        qds6_services::credentials::CredentialStore::get(&credential)
            .unwrap()
            .as_str(),
        "fixture-credential-only"
    );
    qds6_services::credentials::CredentialStore::delete(&credential).unwrap();
    assert!(qds6_services::credentials::CredentialStore::get(&credential).is_err());
    println!("PASS Keychain: isolated credential write/read/delete and missing credential error");
    database_smoke(Protocol::PostgreSql, 25432);
    database_smoke(Protocol::MySql, 23306);
    ssh_smoke(root.path());
    #[cfg(feature = "test-support")]
    qds6_services::credentials::CredentialStore::delete(&draft_reference).unwrap();
    println!("QDS6 service fixture smoke passed");
}
