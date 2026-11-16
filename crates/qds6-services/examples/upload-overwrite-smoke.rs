//! 功能：专用回环 SSH 实测覆盖、目录合并、权限、类型冲突和传输中取消；时间：2026-11-16；作者：lq。

use qds6_core::{ConnectionProfile, Protocol};
use qds6_services::{
    sftp::{Client, Transfer},
    ssh::{self, SshError},
};
use std::{
    path::Path,
    sync::atomic::Ordering,
    thread,
    time::{Duration, Instant},
};

/// 功能：只清理本次 UUID 根目录，不跟随远端符号链接；时间：2026-11-16；作者：lq。
fn cleanup(client: &Client, path: &Path) {
    for entry in client.list(path).unwrap() {
        if entry.directory && !entry.symlink {
            cleanup(client, &entry.path);
        } else {
            client.remove(&entry.path).unwrap();
        }
    }
    client.remove(path).unwrap();
}

/// 功能：仅使用公开 fixture 凭据及隔离配置，不读取用户业务连接；时间：2026-11-16；作者：lq。
fn main() {
    assert_eq!(std::env::var("QDS6_FIXTURES").as_deref(), Ok("1"));
    let root = tempfile::tempdir().unwrap();
    let mut profile = ConnectionProfile::new(Protocol::Ssh);
    profile.name = "overwrite fixture".into();
    profile.host = "127.0.0.1".into();
    profile.port = 22222;
    profile.username = "qds6".into();
    let known = root.path().join("known.json");
    let fingerprint = match ssh::connect(&profile, Some("qds6-fixture-only"), &known) {
        Err(SshError::UnknownHost { fingerprint }) => fingerprint,
        Err(error) => panic!("fixture connection failed: {error}"),
        Ok(_) => panic!("fixture must request host trust"),
    };
    ssh::trust_host(&known, &profile, &fingerprint).unwrap();
    let session = ssh::connect(&profile, Some("qds6-fixture-only"), &known).unwrap();
    let raw = session.sftp().unwrap();
    let client = Client::new(session).unwrap();
    let remote = std::path::PathBuf::from(format!("/home/qds6/overwrite-{}", uuid::Uuid::new_v4()));
    client.mkdir(&remote).unwrap();
    let seed = root.path().join("seed");
    std::fs::write(&seed, "old content\n").unwrap();
    let target = remote.join("index.html");
    client.upload(&seed, &target, &Transfer::default()).unwrap();
    client.chmod(&target, 0o644).unwrap();
    let before = raw.lstat(&target).unwrap();
    std::fs::write(&seed, "new 中文\n").unwrap();
    client.upload(&seed, &target, &Transfer::default()).unwrap();
    assert_eq!(client.open_document(&target).unwrap().text, "new 中文\n");
    let after = raw.lstat(&target).unwrap();
    assert_eq!(
        (after.perm, after.uid, after.gid),
        (before.perm, before.uid, before.gid)
    );
    std::fs::write(&seed, "").unwrap();
    client.upload(&seed, &target, &Transfer::default()).unwrap();
    assert_eq!(client.open_document(&target).unwrap().text, "");
    println!("PASS file overwrite: shorter/empty content and exact permission/owner preservation");

    // 功能：同名文件夹逐文件合并，新建嵌套目录，保留远端独有项；时间：2026-11-16；作者：lq。
    let tree = root.path().join("tree");
    std::fs::create_dir_all(tree.join("nested/empty")).unwrap();
    std::fs::write(tree.join("index.html"), "tree original").unwrap();
    std::fs::write(tree.join("nested/file.txt"), "nested original").unwrap();
    let remote_tree = remote.join("tree");
    client
        .upload_path(&tree, &remote_tree, &Transfer::default())
        .unwrap();
    std::fs::write(&seed, "remote only").unwrap();
    client
        .upload(
            &seed,
            &remote_tree.join("remote-only.txt"),
            &Transfer::default(),
        )
        .unwrap();
    std::fs::write(tree.join("index.html"), "tree replacement").unwrap();
    std::fs::write(tree.join("nested/file.txt"), "nested replacement").unwrap();
    std::fs::create_dir_all(tree.join("new/empty")).unwrap();
    std::fs::write(tree.join("new/file.txt"), "new file").unwrap();
    client
        .upload_path(&tree, &remote_tree, &Transfer::default())
        .unwrap();
    for (path, expected) in [
        ("index.html", "tree replacement"),
        ("nested/file.txt", "nested replacement"),
        ("remote-only.txt", "remote only"),
        ("new/file.txt", "new file"),
    ] {
        assert_eq!(
            client.open_document(&remote_tree.join(path)).unwrap().text,
            expected
        );
    }
    assert!(
        client
            .list(&remote_tree.join("new/empty"))
            .unwrap()
            .is_empty()
    );
    println!(
        "PASS directory merge: nested replacements, additions, empty directories, remote-only preservation"
    );

    // 功能：拒绝文件/目录和链接冲突；预检失败不能先覆盖其他文件；时间：2026-11-16；作者：lq。
    assert!(
        client
            .upload(&seed, &remote_tree, &Transfer::default())
            .is_err()
    );
    assert!(
        client
            .upload_path(&tree, &target, &Transfer::default())
            .is_err()
    );
    raw.symlink(&target, &remote.join("link")).unwrap();
    assert!(
        client
            .upload(&seed, &remote.join("link"), &Transfer::default())
            .is_err()
    );
    assert_eq!(client.open_document(&target).unwrap().text, "");
    raw.symlink(&remote_tree.join("nested"), &remote_tree.join("linked-dir"))
        .unwrap();
    std::fs::create_dir_all(tree.join("linked-dir")).unwrap();
    std::fs::write(tree.join("linked-dir/no-follow"), "must not upload").unwrap();
    std::fs::write(tree.join("index.html"), "must remain local").unwrap();
    assert!(
        client
            .upload_path(&tree, &remote_tree, &Transfer::default())
            .is_err()
    );
    assert_eq!(
        client
            .open_document(&remote_tree.join("index.html"))
            .unwrap()
            .text,
        "tree replacement"
    );
    client.mkdir(&remote.join("locked")).unwrap();
    client
        .upload(&seed, &remote.join("locked/keep"), &Transfer::default())
        .unwrap();
    client.chmod(&remote.join("locked"), 0o500).unwrap();
    assert!(
        client
            .upload(&seed, &remote.join("locked/keep"), &Transfer::default())
            .is_err()
    );
    client.chmod(&remote.join("locked"), 0o700).unwrap();
    assert_eq!(
        client
            .open_document(&remote.join("locked/keep"))
            .unwrap()
            .text,
        "remote only"
    );
    println!(
        "PASS conflict/failure: type conflicts, symlink rejection, merge preflight, permission failure preserve targets"
    );

    // 功能：数据已开始写入临时文件后取消，原目标内容不变且无临时残留；时间：2026-11-16；作者：lq。
    let large = root.path().join("large.bin");
    std::fs::File::create(&large)
        .unwrap()
        .set_len(64 * 1024 * 1024)
        .unwrap();
    let progress = Transfer::default();
    let monitor = progress.clone();
    let watcher = thread::spawn(move || {
        let start = Instant::now();
        while monitor.completed.load(Ordering::Relaxed) == 0 {
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "upload did not start"
            );
            thread::sleep(Duration::from_millis(1));
        }
        monitor.cancel.store(true, Ordering::Relaxed);
    });
    let error = client.upload(&large, &target, &progress).unwrap_err();
    watcher.join().unwrap();
    assert!(error.contains("取消"));
    assert!(progress.completed.load(Ordering::Relaxed) > 0);
    assert!(progress.completed.load(Ordering::Relaxed) < progress.total.load(Ordering::Relaxed));
    assert_eq!(client.open_document(&target).unwrap().text, "");
    assert!(
        client
            .list(&remote)
            .unwrap()
            .iter()
            .all(|entry| !entry.name.starts_with(".qds6-"))
    );
    println!(
        "PASS in-flight cancellation: original target preserved, partial temporary upload removed"
    );
    cleanup(&client, &remote);
}
