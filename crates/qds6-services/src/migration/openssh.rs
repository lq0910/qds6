//! 功能：从选定的 OpenSSH 配置静态预览连接与单跳引用；时间：2026-11-27；作者：lq。

use super::{ConnectionProfile, Preview, Protocol, add, imported_id};
use std::collections::{BTreeMap, BTreeSet};

/// 功能：保留 Host 段顺序，按 OpenSSH 的首个匹配值应用配置；时间：2026-11-27；作者：lq。
#[derive(Default)]
struct Block {
    patterns: Vec<String>,
    values: BTreeMap<String, String>,
}

/// 功能：读取含引号、注释和可选等号的配置行；时间：2026-11-27；作者：lq。
fn words(line: &str) -> Result<Vec<String>, String> {
    let boundary = line
        .find(|ch: char| ch.is_whitespace() || ch == '=')
        .unwrap_or(line.len());
    let mut words = vec![line[..boundary].to_ascii_lowercase()];
    let mut word = String::new();
    let mut quote = None;
    let mut escaped = false;
    for ch in line[boundary..]
        .trim_start()
        .trim_start_matches('=')
        .chars()
    {
        match ch {
            ch if escaped => {
                word.push(ch);
                escaped = false;
            }
            '\\' => escaped = true,
            ch if Some(ch) == quote => quote = None,
            ch if quote.is_some() => word.push(ch),
            '\'' | '"' => quote = Some(ch),
            '#' => break,
            ch if ch.is_whitespace() => {
                if !word.is_empty() {
                    words.push(std::mem::take(&mut word));
                }
            }
            ch => word.push(ch),
        }
    }
    if escaped || quote.is_some() {
        return Err("OpenSSH 配置有未闭合的引号或转义".into());
    }
    if !word.is_empty() {
        words.push(word);
    }
    Ok(words)
}

/// 功能：Host 通配符只用于求有效配置，通配表达式本身不生成连接；时间：2026-11-27；作者：lq。
fn matches(pattern: &str, host: &str) -> bool {
    let (mut pattern, mut host) = (pattern.as_bytes(), host.as_bytes());
    let mut star = None;
    while let Some((&ch, rest)) = host.split_first() {
        match pattern.split_first() {
            Some((b'*', after)) => {
                star = Some((after, host));
                pattern = after;
            }
            Some((&p, after)) if p == b'?' || p.eq_ignore_ascii_case(&ch) => {
                pattern = after;
                host = rest;
            }
            _ => match star {
                Some((after, [_, rest @ ..])) => {
                    star = Some((after, rest));
                    pattern = after;
                    host = rest;
                }
                _ => return false,
            },
        }
    }
    pattern.iter().all(|&ch| ch == b'*')
}

/// 功能：提取可确定的主机、认证与跳板配置，动态规则交由用户核对；时间：2026-11-27；作者：lq。
pub(super) fn parse(text: &str, out: &mut Preview) -> Result<(), String> {
    let mut blocks = vec![Block::default()];
    let mut aliases = BTreeSet::new();
    for line in text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
    {
        let tokens = words(line)?;
        let (key, values) = tokens.split_first().expect("directive");
        match key.as_str() {
            "host" => {
                if values.is_empty() {
                    return Err("OpenSSH 配置存在空 Host 段".into());
                }
                aliases.extend(
                    values
                        .iter()
                        .filter(|host| !host.contains(['*', '?', '!']))
                        .map(|host| host.to_ascii_lowercase()),
                );
                blocks.push(Block {
                    patterns: values.to_vec(),
                    ..Default::default()
                });
                if aliases.len() > 1024 || blocks.len() > 2048 {
                    return Err("OpenSSH 配置条目过多，请分批导入".into());
                }
            }
            "include" | "match" => {
                return Err(format!(
                    "OpenSSH 配置包含 {key}；请另选仅包含待迁移 Host 段及其静态设置的文件"
                ));
            }
            _ => {
                if let Some(value) = values.first() {
                    blocks
                        .last_mut()
                        .expect("global block")
                        .values
                        .entry(key.clone())
                        .or_insert_with(|| value.to_owned());
                }
            }
        }
    }
    let username = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_default();
    let mut records = BTreeMap::new();
    for alias in aliases {
        let mut settings = BTreeMap::new();
        for block in &blocks {
            if !block.patterns.is_empty()
                && (!block
                    .patterns
                    .iter()
                    .any(|pattern| !pattern.starts_with('!') && matches(pattern, &alias))
                    || block.patterns.iter().any(|pattern| {
                        pattern
                            .strip_prefix('!')
                            .is_some_and(|pattern| matches(pattern, &alias))
                    }))
            {
                continue;
            }
            for (key, value) in &block.values {
                settings.entry(key.as_str()).or_insert(value.as_str());
            }
        }
        let field = |key, default| settings.get(key).copied().unwrap_or(default);
        if !field("proxycommand", "none").eq_ignore_ascii_case("none")
            || settings.contains_key("hostkeyalias")
            || !field("canonicalizehostname", "no").eq_ignore_ascii_case("no")
        {
            out.warnings
                .push(format!("{alias} 使用未支持的代理或地址规则，已跳过"));
            continue;
        }
        let mut profile = ConnectionProfile::new(Protocol::Ssh);
        profile.name = alias.clone();
        profile.host = field("hostname", &alias).replace("%h", &alias);
        profile.username = field("user", &username).into();
        profile.port = field("port", "22").parse().unwrap_or(0);
        let identity = field("identityfile", "none");
        if !identity.eq_ignore_ascii_case("none") {
            let resolved = match identity.strip_prefix("~/") {
                Some(relative) => std::env::var("HOME")
                    .ok()
                    .map(|home| std::path::Path::new(&home).join(relative)),
                None => Some(identity.into()),
            };
            if let Some(path) =
                resolved.filter(|path| path.is_absolute() && !identity.contains(['%', '$']))
            {
                profile.ssh.private_key = path.to_string_lossy().into_owned();
            } else {
                out.warnings.push(format!("{alias} 的私钥路径需重新选择"));
            }
        }
        if profile.host.contains('%') || profile.validate().is_err() {
            out.warnings
                .push(format!("{alias} 的主机或端口无效，已跳过"));
            continue;
        }
        let route = field("proxyjump", "none").to_ascii_lowercase();
        records.insert(alias, (profile, route));
    }
    for (alias, (profile, route)) in &records {
        let mut profile = profile.clone();
        if route != "none" {
            let Some((jump, _)) = records.get(route).filter(|(_, next)| next == "none") else {
                out.warnings.push(format!(
                    "{alias} 的 ProxyJump 不是可导入的单跳 Host 别名，已跳过"
                ));
                continue;
            };
            profile.ssh.jump_profile = Some(imported_id(jump));
        }
        add(out, profile, None);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::migration::{Source, preview};

    /// 功能：验证全局、通配与别名规则、带空格密钥路径及跳板引用；时间：2026-11-27；作者：lq。
    #[test]
    fn imports_static_aliases_and_resolves_one_jump() {
        let config = b"User operator\nHost app jump\n  Port 2222\nHost app\n HostName 192.0.2.2\n IdentityFile=\"/tmp/example key\"\n ProxyJump jump\nHost jump\n HostName 192.0.2.1\nHost * !app\n Port 2200\nHost bad\n ProxyCommand false\n";
        let result = preview(Source::OpenSsh, config).unwrap();
        let [app, jump] = &result.config.profiles[..] else {
            panic!("two aliases")
        };
        assert_eq!(
            (app.host.as_str(), app.port, app.username.as_str()),
            ("192.0.2.2", 2222, "operator")
        );
        assert_eq!(app.ssh.private_key, "/tmp/example key");
        assert_eq!(app.ssh.jump_profile, Some(jump.id));
        assert!(
            result
                .warnings
                .iter()
                .any(|message| message.contains("bad"))
        );
        assert!(preview(Source::OpenSsh, b"Include another\nHost ignored\n").is_err());
    }
}
