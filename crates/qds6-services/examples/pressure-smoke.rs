//! 功能：隔离服务的万项目录和 1 GiB 传输组合验收，不访问业务服务；时间：2026-11-11；作者：lq。
use qds6_core::{ConnectionProfile, Protocol};
use qds6_services::{
    sftp::{Client, Transfer},
    ssh::{self, SshError},
    terminal::TerminalSession,
};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Write},
    path::Path,
    sync::atomic::Ordering,
    thread,
    time::{Duration, Instant},
};

/// 功能：流式摘要核对，不把大文件读入内存；时间：2026-11-11；作者：lq。
fn digest(path: &Path) -> Vec<u8> {
    let mut file = File::open(path).unwrap();
    let mut hash = Sha256::new();
    let mut bytes = [0; 65536];
    loop {
        let n = file.read(&mut bytes).unwrap();
        if n == 0 {
            break;
        }
        hash.update(&bytes[..n]);
    }
    hash.finalize().to_vec()
}

/// 功能：明确开关和固定回环端口限定压力验收范围；时间：2026-11-11；作者：lq。
fn main() {
    assert_eq!(std::env::var("QDS6_FIXTURES").as_deref(), Ok("1"));
    let root = tempfile::tempdir().unwrap();
    let mut profile = ConnectionProfile::new(Protocol::Ssh);
    profile.name = "pressure fixture".into();
    profile.host = "127.0.0.1".into();
    profile.port = 22222;
    profile.username = "qds6".into();
    let known = root.path().join("known.json");
    let fingerprint = match ssh::connect(&profile, Some("qds6-fixture-only"), &known) {
        Err(SshError::UnknownHost { fingerprint }) => fingerprint,
        _ => panic!("untrusted fixture"),
    };
    ssh::trust_host(&known, &profile, &fingerprint).unwrap();
    let session = ssh::connect(&profile, Some("qds6-fixture-only"), &known).unwrap();
    let directory = format!("/home/qds6/qds6-pressure-{}", uuid::Uuid::new_v4());
    let mut channel = session.channel_session().unwrap();
    channel.exec(&format!("mkdir '{directory}' && i=0; while [ $i -lt 10000 ]; do : > '{directory}/item-'$i; i=$((i+1)); done")).unwrap();
    let mut output = String::new();
    channel.read_to_string(&mut output).unwrap();
    channel.wait_close().unwrap();
    assert_eq!(channel.exit_status().unwrap(), 0);
    drop(channel);
    let client = Client::new(session).unwrap();
    let start = Instant::now();
    let entries = client.list(Path::new(&directory)).unwrap();
    assert_eq!(entries.len(), 10000);
    println!(
        "PASS directory: {} entries in {} ms",
        entries.len(),
        start.elapsed().as_millis()
    );
    let terminal = TerminalSession::start(
        ssh::connect(&profile, Some("qds6-fixture-only"), &known).unwrap(),
        80,
        24,
    )
    .unwrap();
    let local = root.path().join("source-1g.bin");
    let mut file = File::create(&local).unwrap();
    // 功能：确定性非零内容，保证完整往返校验能发现截断和空洞错误；时间：2026-11-11；作者：lq。
    let block: Vec<_> = (0..65536).map(|i| ((i * 31 + 17) % 251) as u8).collect();
    for _ in 0..16384 {
        file.write_all(&block).unwrap();
    }
    file.sync_all().unwrap();
    drop(file);
    let remote = Path::new(&directory).join("large.bin");
    let progress = Transfer::default();
    thread::scope(|scope| {
        let worker = scope.spawn(|| client.upload(&local, &remote, &progress));
        let start = Instant::now();
        while progress.completed.load(Ordering::Relaxed) < 1024 * 1024 {
            assert!(start.elapsed() < Duration::from_secs(30));
            thread::sleep(Duration::from_millis(10));
        }
        terminal
            .input(b"printf 'QDS6_RESPONSIVE\\n'\r".to_vec())
            .unwrap();
        while !terminal
            .model
            .lock()
            .unwrap()
            .text()
            .lines()
            .any(|line| line.trim() == "QDS6_RESPONSIVE")
        {
            assert!(start.elapsed() < Duration::from_secs(10));
            thread::sleep(Duration::from_millis(10));
        }
        println!(
            "PASS terminal model during upload: {} ms",
            start.elapsed().as_millis()
        );
        worker.join().unwrap().unwrap();
    });
    assert_eq!(progress.completed.load(Ordering::Relaxed), 1 << 30);
    let start = Instant::now();
    let downloaded = root.path().join("download-1g.bin");
    client
        .download(&remote, &downloaded, &Transfer::default())
        .unwrap();
    assert_eq!(digest(&local), digest(&downloaded));
    println!(
        "PASS 1 GiB roundtrip: exact SHA256, download+hash {} ms",
        start.elapsed().as_millis()
    );
    client.remove(&remote).unwrap();
    for entry in entries {
        client.remove(&entry.path).unwrap();
    }
    client.remove(Path::new(&directory)).unwrap();
    println!(
        "QDS6 isolated pressure checks passed; terminal model responsiveness is separate from desktop IME/frame acceptance"
    );
}
