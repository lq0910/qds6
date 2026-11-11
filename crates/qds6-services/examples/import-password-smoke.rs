//! 功能：隔离配置验证密码导入、真实 Keychain 和可选回环服务认证，不读取用户秘密；时间：2026-11-11；作者：lq。

use qds6_core::{ConnectionProfile, CredentialRef, Protocol, TlsMode};
use qds6_services::{
    backup::Configuration,
    credentials::CredentialStore,
    migration::{self, Source},
};
use qds6_storage::ProfileStore;
use std::path::Path;

/// 功能：无论成功或失败，都只清理本次验收创建的随机 Keychain 引用；时间：2026-11-11；作者：lq。
#[derive(Default)]
struct Credentials(Vec<CredentialRef>);

impl Credentials {
    /// 功能：记录新导入后的凭据，不触碰正常应用已有记录；时间：2026-11-11；作者：lq。
    fn track(&mut self, profiles: &[ConnectionProfile]) {
        for profile in profiles {
            if let Some(id) = &profile.credential
                && !self.0.contains(id)
            {
                self.0.push(id.clone());
            }
        }
    }
}

impl Drop for Credentials {
    /// 功能：析构清理用于意外错误兜底，正常路径另行验证删除成功；时间：2026-11-11；作者：lq。
    fn drop(&mut self) {
        for id in &self.0 {
            let _ = CredentialStore::delete(id);
        }
    }
}

/// 功能：仅从 JDK 公开合成向量选取隔离服务专用口令；时间：2026-11-11；作者：lq。
fn vector(format: &str) -> String {
    let vectors: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("../src/migration/fixtures/passwords.json")).unwrap();
    vectors
        .iter()
        .find(|v| v["format"] == format && v["plaintext"] == "qds6-fixture-only")
        .unwrap()["encoded"]
        .as_str()
        .unwrap()
        .to_string()
}

/// 功能：可选认证只访问固定回环测试端口，数据库 TLS 放宽仅作用于合成验收配置；时间：2026-11-11；作者：lq。
fn authenticate_fixture(
    profile: &ConnectionProfile,
    secret: &str,
    root: &Path,
) -> Result<(), String> {
    if profile.protocol == Protocol::Ssh {
        let known = root.join("fixture-known-hosts.json");
        match qds6_services::ssh::connect(profile, Some(secret), &known) {
            Err(qds6_services::ssh::SshError::UnknownHost { fingerprint }) => {
                qds6_services::ssh::trust_host(&known, profile, &fingerprint)
                    .map_err(|_| "fixture host trust failed")?;
            }
            _ => return Err("fixture first host confirmation did not occur".into()),
        }
        qds6_services::ssh::connect(profile, Some(secret), &known)
            .map_err(|_| "imported SSH password authentication failed")?;
    } else {
        let mut fixture = profile.clone();
        fixture.db.tls = TlsMode::Disabled;
        qds6_services::database::DatabaseSession::connect(&fixture, secret, None)
            .map_err(|_| "imported database password authentication failed")?
            .query("SELECT 1")
            .map_err(|_| "fixture query failed")?;
    }
    Ok(())
}

/// 功能：验证确认前无写入、确认后真实凭据关联、重复导入、明文排除和凭据清理；时间：2026-11-11；作者：lq。
fn main() -> Result<(), String> {
    let services = std::env::args().any(|arg| arg == "--services");
    if services && std::env::var("QDS6_FIXTURES").as_deref() != Ok("1") {
        return Err("real service checks require explicit QDS6_FIXTURES=1".into());
    }
    let finalshell = serde_json::to_vec(&serde_json::json!({
        "name":"fixture SSH","host":"127.0.0.1","port":22222,"user_name":"qds6",
        "conection_type":100,"proxy_id":"0","authentication_type":1,"password":vector("finalshell")
    }))
    .unwrap();
    let navicat = format!(
        "<Connections><Connection ConnectionName='fixture MySQL' ConnType='MySQL' Host='127.0.0.1' Port='23306' UserName='qds6' InitialDatabase='qds6' Password='{}'/><Connection ConnectionName='fixture PostgreSQL' ConnType='PostgreSQL' Host='127.0.0.1' Port='25432' UserName='qds6' InitialDatabase='qds6' Password='{}'/></Connections>",
        vector("navicat-blowfish"),
        vector("navicat-aes")
    );
    let temporary = tempfile::tempdir().map_err(|_| "temporary directory failed")?;
    let mut cleanup = Credentials::default();
    for (index, (source, bytes)) in [
        (Source::FinalShell, finalshell),
        (Source::Navicat, navicat.into_bytes()),
    ]
    .into_iter()
    .enumerate()
    {
        let directory = temporary.path().join(index.to_string());
        std::fs::create_dir(&directory).map_err(|_| "fixture directory failed")?;
        let mut store = ProfileStore::open(directory.join("fixture.sqlite3"))
            .map_err(|_| "fixture store failed")?;
        let preview = migration::preview(source, &bytes)?;
        assert!(store.list().unwrap().is_empty());
        assert!(
            preview
                .config
                .profiles
                .iter()
                .all(|p| p.credential.is_none())
        );
        assert!(preview.config.secrets.len() == preview.config.profiles.len());
        for attempt in 0..2 {
            preview.config.restore(
                &mut store,
                &directory.join(format!("before-{attempt}.json")),
                false,
            )?;
            let profiles = store.list().map_err(|_| "fixture list failed")?;
            cleanup.track(&profiles);
            assert!(profiles.len() == preview.config.profiles.len());
            for profile in &profiles {
                let secret = CredentialStore::get(
                    profile
                        .credential
                        .as_ref()
                        .ok_or("credential reference missing")?,
                )?;
                assert!(
                    secret.as_str() == "qds6-fixture-only",
                    "imported credential mismatch"
                );
                if services && attempt == 0 {
                    authenticate_fixture(profile, &secret, &directory)?;
                }
            }
        }
        let export = Configuration::new(&store.list().map_err(|_| "fixture list failed")?);
        assert!(export.secrets.is_empty());
        let encoded = qds6_services::backup::encode(&export, None)?;
        assert!(!String::from_utf8_lossy(&encoded).contains("qds6-fixture-only"));
        // 功能：快照和 SQLite 文件均不得保存导入密码的明文；时间：2026-11-11；作者：lq。
        for entry in std::fs::read_dir(&directory).map_err(|_| "fixture directory read failed")? {
            let path = entry.map_err(|_| "fixture entry failed")?.path();
            let bytes = std::fs::read(path).map_err(|_| "fixture content read failed")?;
            assert!(
                !bytes
                    .windows(b"qds6-fixture-only".len())
                    .any(|s| s == b"qds6-fixture-only"),
                "plaintext leaked to fixture files"
            );
        }
    }
    for id in &cleanup.0 {
        CredentialStore::delete(id)?;
        assert!(CredentialStore::get(id).is_err());
    }
    cleanup.0.clear();
    println!(
        "PASS FinalShell + Navicat MySQL/PostgreSQL: preview, real Keychain, repeat import, plaintext exclusion and cleanup"
    );
    if services {
        println!(
            "PASS imported passwords authenticated against isolated SSH, MySQL and PostgreSQL services"
        );
    }
    Ok(())
}
