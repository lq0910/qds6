//! 功能：只读核对用户指定 NCX，显式 --connect 时仅连接并执行 SELECT 1；时间：2026-11-11；作者：lq。

use qds6_core::TlsMode;
use qds6_services::{
    backup,
    database::DatabaseSession,
    migration::{self, Source},
};
use std::sync::atomic::{AtomicUsize, Ordering};

/// 功能：秘密仅保留在自动清零的预览内存，不写 SQLite、Keychain 或输出；时间：2026-11-11；作者：lq。
fn main() -> Result<(), String> {
    let mut arguments = std::env::args_os().skip(1);
    let path = arguments
        .next()
        .ok_or("usage: navicat-check <selected.ncx> [--connect]")?;
    let connect = match arguments.next() {
        None => false,
        Some(flag) if flag == "--connect" => true,
        _ => return Err("only optional --connect is supported".into()),
    };
    if arguments.next().is_some() {
        return Err("too many arguments".into());
    }
    let bytes = backup::read_bounded(std::path::Path::new(&path))?;
    let preview = migration::preview(Source::Navicat, &bytes)?;
    println!(
        "profiles={} passwords={} tls_disabled={} pg_database_present={}",
        preview.config.profiles.len(),
        preview.config.secrets.len(),
        preview
            .config
            .profiles
            .iter()
            .filter(|p| p.db.tls == TlsMode::Disabled)
            .count(),
        preview
            .config
            .profiles
            .iter()
            .filter(|p| p.protocol == qds6_core::Protocol::PostgreSql && !p.database.is_empty())
            .count()
    );
    if !connect {
        return Ok(());
    }
    let passed = AtomicUsize::new(0);
    let failed = AtomicUsize::new(0);
    // 功能：最多四个独立后台连接，驱动超时及会话释放沿用产品实现；时间：2026-11-11；作者：lq。
    std::thread::scope(|scope| {
        for profiles in preview
            .config
            .profiles
            .chunks(preview.config.profiles.len().div_ceil(4))
        {
            let secrets = &preview.config.secrets;
            let passed = &passed;
            let failed = &failed;
            scope.spawn(move || {
                for profile in profiles {
                    let result = secrets
                        .get(&profile.id)
                        .ok_or_else(|| "来源密码未解析，未尝试连接".to_string())
                        .and_then(|secret| {
                            DatabaseSession::connect(profile, secret, None)?.query("SELECT 1")?;
                            Ok(())
                        });
                    match result {
                        Ok(()) => {
                            passed.fetch_add(1, Ordering::Relaxed);
                            println!("PASS {} · connected; SELECT 1", profile.name);
                        }
                        Err(error) => {
                            failed.fetch_add(1, Ordering::Relaxed);
                            println!("FAIL {} · {error}", profile.name);
                        }
                    }
                }
            });
        }
    });
    println!(
        "read-only checks: passed={} failed={}; no saved configuration or credentials changed",
        passed.load(Ordering::Relaxed),
        failed.load(Ordering::Relaxed)
    );
    Ok(())
}
