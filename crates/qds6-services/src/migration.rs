//! 功能：四类连接配置预览及 FinalShell/Navicat 本机密码迁移，确认前不写凭据；时间：2026-11-11；作者：lq。

use crate::backup::{Configuration, MAX_FILE};
use qds6_core::{ConnectionProfile, Protocol, TlsMode};
use quick_xml::{Reader, events::Event};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use zeroize::Zeroizing;

// 功能：将旧密码格式兼容限制在独立模块，业务导入只接收临时凭据；时间：2026-11-11；作者：lq。
mod passwords;

/// 功能：明确用户选择的来源，不根据扩展名猜测加密格式；时间：2026-11-10；作者：lq。
#[derive(Clone, Copy)]
pub enum Source {
    Putty,
    FinalShell,
    FileZilla,
    Navicat,
}

/// 功能：解析结果与待补全报告先展示，确认前绝不写入数据库；时间：2026-11-10；作者：lq。
pub struct Preview {
    pub config: Configuration,
    pub warnings: Vec<String>,
}

impl Preview {
    /// 功能：重新导入时复用旧导入器遗漏 Database 的精确稳定 ID，避免新增重复连接；时间：2026-11-11；作者：lq。
    pub fn reuse_legacy_navicat_ids(&mut self, existing: &[ConnectionProfile]) {
        let mut updated = 0;
        let mut ids: std::collections::HashSet<_> =
            self.config.profiles.iter().map(|p| p.id).collect();
        for profile in &mut self.config.profiles {
            if profile.database.is_empty() || existing.iter().any(|p| p.id == profile.id) {
                continue;
            }
            let mut legacy = profile.clone();
            legacy.database.clear();
            let legacy_id = imported_id(&legacy);
            if ids.contains(&legacy_id)
                || !existing.iter().any(|p| {
                    let mut original = p.clone();
                    original.database.clear();
                    p.id == legacy_id
                        && (p.database.is_empty() || p.database == profile.database)
                        && imported_id(&original) == legacy_id
                })
            {
                continue;
            }
            if let Some(secret) = self.config.secrets.remove(&profile.id) {
                self.config.secrets.insert(legacy_id, secret);
            }
            profile.id = legacy_id;
            ids.insert(legacy_id);
            updated += 1;
        }
        if updated > 0 {
            self.warnings.push(format!(
                "{updated} 个旧版导入连接将在确认后原位更新数据库配置，不新增重复项。"
            ));
        }
    }
}

/// 功能：限制输入并按来源解析，兼容密码只留内存，外部文件不自动读取；时间：2026-11-11；作者：lq。
pub fn preview(source: Source, bytes: &[u8]) -> Result<Preview, String> {
    if bytes.len() > MAX_FILE {
        return Err("导入超过 16 MiB 上限".into());
    }
    let text = if bytes.starts_with(&[0xff, 0xfe]) {
        if !(bytes.len() - 2).is_multiple_of(2) {
            return Err("UTF-16 文件长度无效".into());
        }
        String::from_utf16(
            &bytes[2..]
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| u16::from_le_bytes([b[0], b[1]]))
                .collect::<Vec<_>>(),
        )
        .map_err(|_| "UTF-16 文件编码无效")?
    } else {
        std::str::from_utf8(bytes)
            .map_err(|_| "仅支持 UTF-8 或 UTF-16LE BOM 配置")?
            .trim_start_matches('\u{feff}')
            .to_string()
    };
    let notice = match source {
        Source::FinalShell | Source::Navicat => {
            "已识别的保存密码将随账号一并迁移，确认后写入系统凭据库；无法识别的密码需补填，私钥文件需重新选择。仅支持已验证字段和密码格式。"
        }
        _ => {
            "当前来源仅迁移受支持的连接字段，密码需补填，私钥文件需重新选择。仅支持当前解析字段，非完整版本兼容声明。"
        }
    };
    let mut result = Preview {
        config: Configuration::new(&[]),
        warnings: vec![notice.into()],
    };
    match source {
        Source::Putty => putty(&text, &mut result)?,
        Source::FinalShell => final_shell(&text, &mut result)?,
        Source::FileZilla => xml_import(&text, false, &mut result)?,
        Source::Navicat => xml_import(&text, true, &mut result)?,
    }
    if result.config.profiles.is_empty() {
        // 功能：无可导入项时仍展示跳过原因，不把兼容报告藏在成功预览之后；时间：2026-11-11；作者：lq。
        let reasons = result
            .warnings
            .iter()
            .skip(1)
            .take(5)
            .cloned()
            .collect::<Vec<_>>()
            .join("；");
        return Err(format!(
            "文件中没有可安全导入的受支持连接{}",
            if reasons.is_empty() {
                "，请使用原工具重新导出".into()
            } else {
                format!("：{reasons}")
            }
        ));
    }
    let mut ids = std::collections::HashSet::new();
    result.config.profiles.retain(|p| ids.insert(p.id));
    result.config.validate()?;
    Ok(result)
}

/// 功能：构造稳定连接 ID 并关联临时密码，只保留首条重复连接的配置与认证；时间：2026-11-11；作者：lq。
fn add(out: &mut Preview, mut profile: ConnectionProfile, secret: Option<Zeroizing<String>>) {
    if out.config.profiles.len() >= 10000 {
        out.warnings.push("超过 10000 项的连接已跳过".into());
        return;
    }
    if profile.validate().is_err() {
        out.warnings
            .push("一项连接缺少有效名称、主机或端口，已跳过".into());
        return;
    }
    profile.id = imported_id(&profile);
    // 功能：重复来源保留首条配置及密码，避免后条密码静默覆盖首条认证；时间：2026-11-11；作者：lq。
    if out.config.profiles.iter().any(|p| p.id == profile.id) {
        out.warnings
            .push("重复连接已合并，保留首条连接及其密码状态".into());
        return;
    }
    if let Some(mut secret) = secret {
        out.config
            .secrets
            .insert(profile.id, std::mem::take(&mut *secret));
    }
    out.config.profiles.push(profile);
}

/// 功能：沿用导入连接的稳定身份算法，供旧版 Database 字段修复时精确匹配；时间：2026-11-11；作者：lq。
fn imported_id(profile: &ConnectionProfile) -> uuid::Uuid {
    let identity = serde_json::to_vec(&(
        profile.protocol,
        &profile.name,
        &profile.group,
        profile.host.to_lowercase(),
        profile.port,
        &profile.username,
        &profile.database,
    ))
    .expect("identity serialization");
    let digest = Sha256::digest(identity);
    let mut id = [0; 16];
    id.copy_from_slice(&digest[..16]);
    // 功能：RFC 9562 自定义 UUID v8，仅用于稳定去重，不用作秘密标识；时间：2026-11-10；作者：lq。
    id[6] = (id[6] & 0x0f) | 0x80;
    id[8] = (id[8] & 0x3f) | 0x80;
    uuid::Uuid::from_bytes(id)
}

/// 功能：读取 PuTTY Sessions .reg 文本，不导入注册表、不执行 LocalCommand/ProxyCommand；时间：2026-11-10；作者：lq。
fn putty(text: &str, out: &mut Preview) -> Result<(), String> {
    if !text.contains("Windows Registry Editor") && !text.starts_with("REGEDIT4") {
        return Err("不是 PuTTY 注册表导出文本".into());
    }
    let mut records = Vec::new();
    let mut name = None;
    let mut fields = BTreeMap::new();
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            if let Some(name) = name.take() {
                records.push((name, std::mem::take(&mut fields)));
            }
            name = line
                .strip_prefix("[HKEY_CURRENT_USER\\Software\\SimonTatham\\PuTTY\\Sessions\\")
                .and_then(|v| v.strip_suffix(']'))
                .map(str::to_string);
        } else if name.is_some()
            && line.starts_with('"')
            && let Some((key, value)) = line.split_once("\"=")
        {
            fields.insert(key.trim_start_matches('"').to_string(), value.to_string());
        }
    }
    if let Some(name) = name {
        records.push((name, fields));
    }
    for (name, fields) in records {
        let get = |key: &str| fields.get(key).map(|s| reg_string(s)).unwrap_or_default();
        if fields
            .get("ProxyMethod")
            .is_some_and(|v| v != "dword:00000000")
        {
            out.warnings
                .push("PuTTY 代理/跳板配置尚未映射，该连接已跳过".into());
            continue;
        }
        if fields.contains_key("Protocol") && get("Protocol") != "ssh" {
            out.warnings.push("非 SSH PuTTY 会话已跳过".into());
            continue;
        }
        let mut profile = ConnectionProfile::new(Protocol::Ssh);
        profile.name = percent_decode(&name)?;
        profile.host = get("HostName");
        profile.username = get("UserName");
        if let Some(port) = fields.get("PortNumber") {
            profile.port = u16::from_str_radix(
                port.strip_prefix("dword:").ok_or("PuTTY 端口不是 DWORD")?,
                16,
            )
            .map_err(|_| "PuTTY 端口无效")?;
        }
        if !get("PublicKeyFile").is_empty() {
            out.warnings
                .push("PuTTY 外部密钥路径需重新选择；未自动打开 .ppk 文件".into());
        }
        let supported = [
            "HostName",
            "UserName",
            "PortNumber",
            "Protocol",
            "ProxyMethod",
            "PublicKeyFile",
        ];
        if fields.keys().any(|k| !supported.contains(&k.as_str())) {
            out.warnings
                .push("PuTTY 会话含未映射选项，未执行任何来源命令".into());
        }
        add(out, profile, None);
    }
    Ok(())
}

/// 功能：解析 .reg 引号和反斜杠，不调用系统注册表；时间：2026-11-10；作者：lq。
fn reg_string(value: &str) -> String {
    value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .unwrap_or("")
        .replace("\\\"", "\"")
        .replace("\\\\", "\\")
}

/// 功能：PuTTY 会话名按百分比字节解码，拒绝无效 UTF-8；时间：2026-11-10；作者：lq。
fn percent_decode(text: &str) -> Result<String, String> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len() {
                return Err("会话名转义无效".into());
            }
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).map_err(|_| "会话名转义无效")?;
            out.push(u8::from_str_radix(hex, 16).map_err(|_| "会话名转义无效")?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| "会话名不是 UTF-8".into())
}

/// 功能：FinalShell JSON 连接与已兼容保存密码一并预览，私钥引用仍提示补全；时间：2026-11-11；作者：lq。
fn final_shell(text: &str, out: &mut Preview) -> Result<(), String> {
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|_| "FinalShell 文件不是支持的 JSON；不解析加密归档")?;
    let items = value.as_array().cloned().unwrap_or_else(|| vec![value]);
    for (index, item) in items.into_iter().enumerate() {
        let get = |key: &str| {
            item.get(key)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        };
        if item
            .get("proxy")
            .is_some_and(|v| !v.is_null() && v != &serde_json::Value::Bool(false))
            || item.get("proxy_id").is_some_and(|v| {
                // 功能：真实连接配置用字符串 0 表示无代理，有效代理引用仍阻止直连迁移；时间：2026-11-11；作者：lq。
                !(v.is_null()
                    || v.as_str().is_some_and(|s| s.is_empty() || s == "0")
                    || v.as_u64() == Some(0))
            })
        {
            out.warnings
                .push("FinalShell 代理配置未映射，该连接已跳过".into());
            continue;
        }
        // 功能：连接目录 JSON 的 conection_type=100 是 SSH，旧兼容入口 type=1001 保留；时间：2026-11-11；作者：lq。
        if item
            .get("conection_type")
            .is_some_and(|v| v.as_i64() != Some(100))
            || item.get("type").is_some_and(|v| v.as_i64() != Some(1001))
        {
            out.warnings.push("FinalShell 非 SSH 类型已跳过".into());
            continue;
        }
        let mut profile = ConnectionProfile::new(Protocol::Ssh);
        profile.name = get("name");
        profile.host = get("host");
        profile.username = get("user_name");
        profile.group = get("group");
        if let Some(port) = item.get("port") {
            profile.port = port
                .as_u64()
                .and_then(|n| n.try_into().ok())
                .ok_or("FinalShell 端口无效")?;
        }
        let key_auth = !get("private_key_path").is_empty()
            || !get("secret_key_id").is_empty()
            || item.get("authentication_type").and_then(|v| v.as_u64()) == Some(2);
        let secret = if key_auth {
            out.warnings
                .push(format!("FinalShell 第 {} 项：私钥需重新选择，本次未读取外部文件；密码字段未作为私钥口令迁移", index + 1));
            None
        } else if item
            .get("authentication_type")
            .is_some_and(|v| v.as_u64() != Some(1))
        {
            out.warnings.push(format!(
                "FinalShell 第 {} 项：认证类型暂不支持，密码需手动补填",
                index + 1
            ));
            None
        } else {
            source_password(
                item.get("password").and_then(|v| v.as_str()),
                "FinalShell",
                index + 1,
                passwords::finalshell,
                out,
            )
        };
        if let Some(object) = item.as_object()
            && object.keys().any(|k| {
                ![
                    "name",
                    "host",
                    "port",
                    "user_name",
                    "group",
                    "type",
                    "password",
                    "private_key_path",
                    "proxy_id",
                    "conection_type",
                    "authentication_type",
                    "secret_key_id",
                ]
                .contains(&k.as_str())
            })
        {
            out.warnings
                .push("FinalShell 含未映射字段，已保留可映射的基础连接".into());
        }
        add(out, profile, secret);
    }
    Ok(())
}

/// 功能：逐项报告密码状态，不展示密文或明文，成功值由 Configuration 自动清零；时间：2026-11-11；作者：lq。
fn source_password(
    encoded: Option<&str>,
    source: &str,
    index: usize,
    decode: fn(&str) -> Result<passwords::Password, &'static str>,
    out: &mut Preview,
) -> Option<Zeroizing<String>> {
    let Some(encoded) = encoded.filter(|s| !s.is_empty()) else {
        out.warnings.push(format!(
            "{source} 第 {index} 项：未提供保存密码，需手动补填"
        ));
        return None;
    };
    match decode(encoded) {
        Ok(password) => {
            out.warnings.push(format!("{source} 第 {index} 项：密码可迁移（{}），确认后写入系统凭据库；旧格式密码需在连接时验证", password.format));
            Some(password.value)
        }
        Err(error) => {
            out.warnings
                .push(format!("{source} 第 {index} 项：{error}"));
            None
        }
    }
}

/// 功能：有界 XML 节点，禁止 DTD 和外部实体，最大深度 64；时间：2026-11-10；作者：lq。
#[derive(Default)]
struct Node {
    name: String,
    attributes: BTreeMap<String, String>,
    text: String,
    children: Vec<Node>,
}
impl Node {
    fn get(&self, name: &str) -> String {
        self.attributes
            .get(&name.to_lowercase())
            .cloned()
            .or_else(|| {
                self.children
                    .iter()
                    .find(|n| n.name.eq_ignore_ascii_case(name))
                    .map(|n| n.text.clone())
            })
            .unwrap_or_default()
    }
}

/// 功能：构建受控 XML 树，不启用任何实体网络解析；时间：2026-11-10；作者：lq。
fn parse_xml(text: &str) -> Result<Node, String> {
    let mut reader = Reader::from_str(text);
    let mut stack = vec![Node::default()];
    let mut count = 0;
    loop {
        match reader.read_event().map_err(|_| "XML 结构无效")? {
            Event::Start(event) | Event::Empty(event) => {
                count += 1;
                if count > 200000 || stack.len() > 64 {
                    return Err("XML 节点或深度超过限制".into());
                }
                let mut node = Node {
                    name: String::from_utf8_lossy(event.name().as_ref()).to_string(),
                    ..Default::default()
                };
                for attribute in event.attributes() {
                    let attribute = attribute.map_err(|_| "XML 属性无效")?;
                    node.attributes.insert(
                        String::from_utf8_lossy(attribute.key.as_ref()).to_lowercase(),
                        attribute
                            .decode_and_unescape_value(reader.decoder())
                            .map_err(|_| "XML 属性实体无效")?
                            .to_string(),
                    );
                }
                // 功能：使用 reader buffer_position 判断自闭合结尾，避免合并分支丢失 Empty 语义；时间：2026-11-10；作者：lq。
                let position = reader.buffer_position() as usize;
                if text[..position].ends_with("/>") {
                    stack.last_mut().expect("root").children.push(node);
                } else {
                    stack.push(node);
                }
            }
            Event::End(_) => {
                if stack.len() <= 1 {
                    return Err("XML 层级错误".into());
                }
                let node = stack.pop().expect("node");
                stack.last_mut().expect("parent").children.push(node);
            }
            Event::Text(event) => {
                let text = event.xml_content().map_err(|_| "XML 编码无效")?;
                stack
                    .last_mut()
                    .expect("node")
                    .text
                    .push_str(&quick_xml::escape::unescape(&text).map_err(|_| "XML 实体无效")?);
            }
            Event::CData(event) => stack
                .last_mut()
                .expect("node")
                .text
                .push_str(std::str::from_utf8(&event).map_err(|_| "XML CDATA 编码无效")?),
            Event::DocType(_) => return Err("禁止包含 DTD/实体声明的导入 XML".into()),
            Event::Eof => break,
            _ => {}
        }
    }
    if stack.len() != 1 {
        return Err("XML 未闭合".into());
    }
    Ok(stack.pop().expect("root"))
}

/// 功能：FileZilla SFTP 与 Navicat MySQL/Pg 映射，Navicat 密码本机解码，未支持隧道仍阻断；时间：2026-11-11；作者：lq。
fn xml_import(text: &str, navicat: bool, out: &mut Preview) -> Result<(), String> {
    let root = parse_xml(text)?;
    fn visit(node: &Node, group: &str, navicat: bool, out: &mut Preview) -> Result<(), String> {
        if (!navicat && node.name == "Server") || (navicat && node.name == "Connection") {
            let kind = if navicat {
                node.get("ConnType").to_lowercase()
            } else {
                node.get("Protocol")
            };
            let protocol = if navicat {
                match kind.as_str() {
                    "mysql" => Protocol::MySql,
                    "postgresql" | "pgsql" => Protocol::PostgreSql,
                    _ => {
                        out.warnings
                            .push("Navicat 非 MySQL/PostgreSQL 连接已跳过".into());
                        return Ok(());
                    }
                }
            } else if kind == "1" {
                Protocol::Sftp
            } else {
                out.warnings
                    .push("FileZilla 非 SFTP 站点已跳过（不支持 FTP/FTPS）".into());
                return Ok(());
            };
            if navicat
                && ["SSH", "UseSSH", "SSHHost", "SSL", "UseSSL", "SSLCA"]
                    .iter()
                    .any(|key| {
                        let v = node.get(key);
                        !v.is_empty() && v != "0" && !v.eq_ignore_ascii_case("false")
                    })
            {
                out.warnings
                    .push("Navicat 含尚未映射的 SSH/TLS 配置，该连接已跳过，避免静默降级".into());
                return Ok(());
            }
            let mut profile = ConnectionProfile::new(protocol);
            profile.name = node.get(if navicat { "ConnectionName" } else { "Name" });
            profile.host = node.get("Host");
            profile.username = node.get(if navicat { "UserName" } else { "User" });
            profile.group = group.into();
            profile.database = node.get("InitialDatabase");
            // 功能：兼容真实 NCX 的 Database 属性，旧版 InitialDatabase 仍作回退；时间：2026-11-11；作者：lq。
            if navicat && !node.get("Database").is_empty() {
                profile.database = node.get("Database");
            }
            let port = node.get("Port");
            if !port.is_empty() {
                profile.port = port.parse().map_err(|_| "XML 连接端口无效")?;
            }
            if navicat {
                // 功能：只在来源明确关闭 SSL 时沿用关闭状态，缺失字段不猜测；时间：2026-11-11；作者：lq。
                let explicitly_disabled = ["SSL", "UseSSL"].iter().any(|key| {
                    let value = node.get(key);
                    value.trim() == "0" || value.trim().eq_ignore_ascii_case("false")
                });
                if explicitly_disabled {
                    profile.db.tls = TlsMode::Disabled;
                    out.warnings.push(
                        "Navicat 连接已沿用源文件明确的 SSL=false 设置；数据库 TLS 已关闭。".into(),
                    );
                } else {
                    out.warnings.push(
                        "Navicat 未提供明确的 SSL 开关，保留严格 TLS；请核对源连接设置。".into(),
                    );
                }
            }
            let secret = if navicat {
                source_password(
                    Some(&node.get("Password")),
                    "Navicat",
                    out.config.profiles.len() + 1,
                    passwords::navicat,
                    out,
                )
            } else {
                None
            };
            add(out, profile, secret);
        }
        let next_group = if node.name == "Folder" {
            let name = node.get("Name");
            if name.is_empty() {
                node.text.trim().to_string()
            } else {
                name
            }
        } else {
            group.into()
        };
        for child in &node.children {
            visit(child, &next_group, navicat, out)?;
        }
        Ok(())
    }
    visit(&root, "", navicat, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 功能：回归 NCX 的关闭 SSL、Database 字段及未知 TLS 不降级；时间：2026-11-11；作者：lq。
    #[test]
    fn navicat_preserves_explicit_ssl_and_database_fields() {
        let xml = b"<Connections><Connection ConnectionName='pg' ConnType='POSTGRESQL' Host='localhost' UserName='login' SSL='false' Database='actual_database'/><Connection ConnectionName='mysql' ConnType='MYSQL' Host='localhost' UseSSL='0'><Database>actual_mysql</Database></Connection><Connection ConnectionName='legacy' ConnType='PostgreSQL' Host='localhost' InitialDatabase='legacy_db'/></Connections>";
        let result = preview(Source::Navicat, xml).unwrap();
        let profiles = &result.config.profiles;
        assert_eq!(profiles[0].db.tls, TlsMode::Disabled);
        assert_eq!(profiles[0].database, "actual_database");
        assert_eq!(profiles[1].db.tls, TlsMode::Disabled);
        assert_eq!(profiles[1].database, "actual_mysql");
        assert_eq!(profiles[2].db.tls, TlsMode::VerifyFull);
        assert_eq!(profiles[2].database, "legacy_db");
        for extra in [
            "SSL='true'",
            "SSL='unknown'",
            "SSL='false' UseSSL='true'",
            "SSL='false' SSH='true'",
        ] {
            let xml = format!(
                "<Connections><Connection ConnectionName='unsupported' ConnType='MYSQL' Host='localhost' {extra}/></Connections>"
            );
            assert!(preview(Source::Navicat, xml.as_bytes()).is_err());
        }
    }

    /// 功能：旧版空库名连接合并补全、不重复添加，密码关联随 ID 迁移，手动连接不误匹配；时间：2026-11-11；作者：lq。
    #[test]
    fn navicat_reimport_repairs_legacy_identity_without_duplicate_profiles() {
        let xml = b"<Connections><Connection ConnectionName='pg' ConnType='POSTGRESQL' Host='localhost' UserName='login' SSL='false' Database='actual_database'/></Connections>";
        let mut result = preview(Source::Navicat, xml).unwrap();
        let correct_id = result.config.profiles[0].id;
        let mut legacy = result.config.profiles[0].clone();
        legacy.database.clear();
        legacy.db.tls = TlsMode::VerifyFull;
        legacy.id = imported_id(&legacy);
        result
            .config
            .secrets
            .insert(correct_id, "public-fixture".into());
        result.reuse_legacy_navicat_ids(&[legacy.clone()]);
        assert_eq!(result.config.profiles[0].id, legacy.id);
        assert_eq!(result.config.profiles[0].database, "actual_database");
        assert_eq!(
            result.config.secrets.get(&legacy.id).unwrap(),
            "public-fixture"
        );
        assert!(!result.config.secrets.contains_key(&correct_id));
        result.config.validate().unwrap();

        let temporary = tempfile::tempdir().unwrap();
        let mut store =
            qds6_storage::ProfileStore::open(temporary.path().join("fixture.sqlite3")).unwrap();
        store.save(&legacy).unwrap();
        result.config.secrets.clear();
        result
            .config
            .restore(&mut store, &temporary.path().join("before.json"), false)
            .unwrap();
        let saved = store.list().unwrap();
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].id, legacy.id);
        assert_eq!(saved[0].database, "actual_database");
        assert_eq!(saved[0].db.tls, TlsMode::Disabled);
        let mut repeated = preview(Source::Navicat, xml).unwrap();
        repeated.reuse_legacy_navicat_ids(&saved);
        assert_eq!(repeated.config.profiles[0].id, legacy.id);

        legacy.id = uuid::Uuid::new_v4();
        let mut manual = preview(Source::Navicat, xml).unwrap();
        manual.reuse_legacy_navicat_ids(&[legacy]);
        assert_eq!(manual.config.profiles[0].id, correct_id);
    }

    /// 功能：显式指定的用户 NCX 只读验收，不输出密码、不写配置或连接业务服务；时间：2026-11-11；作者：lq。
    #[test]
    #[ignore = "requires explicitly selected QDS6_NAVICAT_SAMPLE"]
    fn navicat_selected_sample_preview() {
        let path = std::env::var_os("QDS6_NAVICAT_SAMPLE").expect("explicit sample path");
        let bytes = crate::backup::read_bounded(std::path::Path::new(&path)).unwrap();
        let result = preview(Source::Navicat, &bytes).unwrap();
        assert_eq!(result.config.profiles.len(), 21);
        assert_eq!(result.config.secrets.len(), 21);
        assert!(result.warnings.iter().any(|w| w.contains("SSH/TLS")));
        assert!(
            result
                .config
                .profiles
                .iter()
                .all(|p| p.db.tls == TlsMode::Disabled)
        );
        assert!(
            result
                .config
                .profiles
                .iter()
                .filter(|p| p.protocol == Protocol::PostgreSql)
                .all(|p| !p.database.is_empty())
        );
        println!(
            "PASS selected NCX: 21 direct MySQL/PostgreSQL profiles and passwords, explicit SSL=false preserved, PostgreSQL database names present; one SSH tunnel skipped; read-only preview only"
        );
    }

    /// 功能：复现真实连接配置的无代理标记和密钥引用，基础连接可预览且秘密不落入配置；时间：2026-11-11；作者：lq。
    #[test]
    fn finalshell_connection_config_accepts_no_proxy_and_reports_missing_key() {
        let input = serde_json::json!({
            "name": "测试服务器", "host": "localhost", "port": 2222,
            "user_name": "test", "conection_type": 100, "proxy_id": "0",
            "authentication_type": 2, "secret_key_id": "source-key-reference",
            "password": "opaque-source-password", "parent_id": "source-group-reference",
            "remote_port_forwarding": {}, "port_forwarding_list": [],
            "exec_channel_enable": true, "terminal_encoding": "UTF-8"
        });
        let first = preview(Source::FinalShell, &serde_json::to_vec(&input).unwrap()).unwrap();
        assert_eq!(first.config.profiles.len(), 1);
        let profile = &first.config.profiles[0];
        assert_eq!(profile.protocol, Protocol::Ssh);
        assert_eq!(profile.name, "测试服务器");
        assert_eq!(profile.host, "localhost");
        assert_eq!(profile.port, 2222);
        assert_eq!(profile.username, "test");
        assert!(profile.credential.is_none());
        assert!(profile.ssh.private_key.is_empty());
        assert!(first.warnings.iter().any(|w| w.contains("私钥需重新选择")));
        assert!(
            first
                .warnings
                .iter()
                .any(|w| w.contains("未作为私钥口令迁移"))
        );
        let serialized = serde_json::to_string(&first.config).unwrap();
        assert!(!serialized.contains("opaque-source-password"));
        assert!(!serialized.contains("source-key-reference"));
        let second = preview(Source::FinalShell, &serde_json::to_vec(&input).unwrap()).unwrap();
        assert_eq!(first.config.profiles, second.config.profiles);
        // 功能：合成预览经正式恢复服务写入隔离 SQLite，重复导入不新增且先保存恢复点；时间：2026-11-11；作者：lq。
        let temporary = tempfile::tempdir().unwrap();
        let mut store =
            qds6_storage::ProfileStore::open(temporary.path().join("profiles.sqlite3")).unwrap();
        for (index, config) in [&first.config, &second.config].into_iter().enumerate() {
            let snapshot = temporary.path().join(format!("before-import-{index}.json"));
            config.restore(&mut store, &snapshot, false).unwrap();
            assert!(snapshot.exists());
            assert_eq!(store.list().unwrap(), first.config.profiles);
        }
        for proxy in [
            serde_json::Value::Null,
            serde_json::json!(""),
            serde_json::json!(0),
        ] {
            let mut input = input.clone();
            input["proxy_id"] = proxy;
            assert!(preview(Source::FinalShell, &serde_json::to_vec(&input).unwrap()).is_ok());
        }
    }

    /// 功能：有效代理与非 SSH 类型继续跳过，无有效项仍给出原因且不回显来源秘密；时间：2026-11-11；作者：lq。
    #[test]
    fn finalshell_rejects_real_proxy_and_non_ssh_with_reason() {
        let base = serde_json::json!({
            "name": "测试", "host": "localhost", "user_name": "test",
            "conection_type": 100, "proxy_id": "0", "password": "opaque-secret"
        });
        for (key, value, reason) in [
            (
                "proxy_id",
                serde_json::json!("actual-proxy-reference"),
                "代理配置未映射",
            ),
            (
                "proxy",
                serde_json::json!({"host": "proxy.invalid"}),
                "代理配置未映射",
            ),
            ("conection_type", serde_json::json!(101), "非 SSH 类型"),
            (
                "conection_type",
                serde_json::json!("unknown"),
                "非 SSH 类型",
            ),
            ("type", serde_json::json!(1002), "非 SSH 类型"),
        ] {
            let mut input = base.clone();
            input[key] = value;
            let bytes = serde_json::to_vec(&input).unwrap();
            let error = preview(Source::FinalShell, &bytes).err().unwrap();
            assert!(error.contains(reason), "{error}");
            assert!(!error.contains("opaque-secret"));
            let mixed = serde_json::to_vec(&serde_json::json!([input, base])).unwrap();
            let result = preview(Source::FinalShell, &mixed).unwrap();
            assert_eq!(result.config.profiles.len(), 1);
            assert!(result.warnings.iter().any(|w| w.contains(reason)));
        }
    }

    /// 功能：仅在人工指定样例路径时只读验证真实 FinalShell 文件，不输出主机、用户名或秘密；时间：2026-11-11；作者：lq。
    #[test]
    #[ignore = "须通过 QDS6_FINALSHELL_SAMPLE 显式指定用户选择的文件"]
    fn finalshell_selected_sample_preview() {
        let path = std::env::var_os("QDS6_FINALSHELL_SAMPLE").expect("explicit sample path");
        let bytes = crate::backup::read_bounded(std::path::Path::new(&path)).unwrap();
        let result = preview(Source::FinalShell, &bytes).unwrap();
        assert!(!result.config.profiles.is_empty());
        assert!(
            result
                .config
                .profiles
                .iter()
                .all(|p| p.protocol == Protocol::Ssh
                    && p.credential.is_none()
                    && p.ssh.private_key.is_empty())
        );
        println!(
            "FinalShell 只读预览通过：{} 个 SSH 连接，{} 个密码可迁移；未写入配置或凭据",
            result.config.profiles.len(),
            result.config.secrets.len()
        );
    }

    /// 功能：从 JDK 独立向量选取公开合成密码，不读取外部配置；时间：2026-11-11；作者：lq。
    fn password_vector(format: &str, plaintext: &str) -> String {
        let vectors: Vec<serde_json::Value> =
            serde_json::from_str(include_str!("migration/fixtures/passwords.json")).unwrap();
        vectors
            .iter()
            .find(|v| v["format"] == format && v["plaintext"] == plaintext)
            .unwrap()["encoded"]
            .as_str()
            .unwrap()
            .to_string()
    }

    /// 功能：FinalShell 密码关联稳定 ID，预览不产生机器凭据，普通导出阻止带明文秘密；时间：2026-11-11；作者：lq。
    #[test]
    fn finalshell_password_is_carried_only_in_transient_credentials() {
        let encoded = password_vector("finalshell", " 密码 🚀 ");
        let input = serde_json::json!({"name":"fixture","host":"localhost","user_name":"test","password":encoded,"authentication_type":1});
        let first = preview(Source::FinalShell, &serde_json::to_vec(&input).unwrap()).unwrap();
        let profile = &first.config.profiles[0];
        assert!(profile.credential.is_none());
        assert!(first.config.secrets[&profile.id] == " 密码 🚀 ");
        assert!(first.warnings.iter().any(|w| w.contains("密码可迁移")));
        assert!(!first.warnings.join(" ").contains(" 密码 🚀 "));
        assert!(crate::backup::encode(&first.config, None).is_err());
        let export = crate::backup::Configuration::new(&first.config.profiles);
        assert!(export.secrets.is_empty());
        let serialized = serde_json::to_string(&export).unwrap();
        assert!(!serialized.contains(" 密码 🚀 ") && !serialized.contains(&encoded));
        // 功能：同一连接首条认证获保留，重复和无效配置不留下孤立凭据；时间：2026-11-11；作者：lq。
        let mut second = input.clone();
        second["password"] = password_vector("finalshell", "123456").into();
        let mut invalid = input.clone();
        invalid["host"] = "".into();
        let mixed = preview(
            Source::FinalShell,
            &serde_json::to_vec(&serde_json::json!([input, second, invalid])).unwrap(),
        )
        .unwrap();
        assert_eq!(mixed.config.profiles.len(), 1);
        assert_eq!(mixed.config.secrets.len(), 1);
        assert!(mixed.config.secrets[&profile.id] == " 密码 🚀 ");
    }

    /// 功能：MySQL/PostgreSQL 的 Navicat 属性或子节点密码均迁移，坏密码保留账号配置并报告；时间：2026-11-11；作者：lq。
    #[test]
    fn navicat_database_passwords_support_both_formats_and_partial_failure() {
        let aes = password_vector("navicat-aes", " 密码 🚀 ");
        let blowfish = password_vector("navicat-blowfish", "qds6-fixture-only");
        let xml = format!(
            "<Connections><Connection ConnectionName='mysql' ConnType='MySQL' Host='localhost' UserName='mysql-user' InitialDatabase='fixture' Password='{blowfish}'/><Connection ConnectionName='pg' ConnType='PostgreSQL' Host='localhost' UserName='pg-user'><Password>{aes}</Password></Connection><Connection ConnectionName='bad-password' ConnType='MySQL' Host='localhost' UserName='saved-user' Password='not-supported'/></Connections>"
        );
        let result = preview(Source::Navicat, xml.as_bytes()).unwrap();
        assert_eq!(result.config.profiles.len(), 3);
        assert_eq!(result.config.secrets.len(), 2);
        let mysql = &result.config.profiles[0];
        let pg = &result.config.profiles[1];
        assert_eq!(mysql.protocol, Protocol::MySql);
        assert_eq!(mysql.username, "mysql-user");
        assert_eq!(mysql.database, "fixture");
        assert!(result.config.secrets[&mysql.id] == "qds6-fixture-only");
        assert_eq!(pg.protocol, Protocol::PostgreSql);
        assert_eq!(pg.username, "pg-user");
        assert!(result.config.secrets[&pg.id] == " 密码 🚀 ");
        assert!(
            result
                .config
                .profiles
                .iter()
                .all(|p| p.credential.is_none())
        );
        assert!(
            result
                .warnings
                .iter()
                .any(|w| w.contains("第 3 项") && w.contains("需手动补填"))
        );
        assert!(!result.warnings.join(" ").contains("qds6-fixture-only"));
        let tunneled = format!(
            "<Connections><Connection ConnectionName='tunnel' ConnType='MySQL' Host='localhost' UseSSH='true' Password='{blowfish}'/></Connections>"
        );
        assert!(preview(Source::Navicat, tunneled.as_bytes()).is_err());
    }

    /// 功能：合成脱敏四格式样例、幂等 ID 与恶意 XML 拒绝；不代表源工具全版本验收；时间：2026-11-10；作者：lq。
    #[test]
    fn synthetic_formats_are_conservative_and_idempotent() {
        let cases = [
            (
                Source::Putty,
                "Windows Registry Editor Version 5.00\n[HKEY_CURRENT_USER\\Software\\SimonTatham\\PuTTY\\Sessions\\dev%20ssh]\n\"HostName\"=\"localhost\"\n\"UserName\"=\"test\"\n\"PortNumber\"=dword:00000016\n\"Protocol\"=\"ssh\"",
            ),
            (
                Source::FinalShell,
                r#"{"name":"测试","host":"localhost","port":22,"user_name":"test","type":1001,"password":"opaque"}"#,
            ),
            (
                Source::FileZilla,
                "<FileZilla3><Servers><Server><Host>localhost</Host><Port>22</Port><Protocol>1</Protocol><User>test</User><Name>中文</Name></Server></Servers></FileZilla3>",
            ),
            (
                Source::Navicat,
                "<Connections><Connection ConnectionName=\"dev\" ConnType=\"PostgreSQL\" Host=\"localhost\" Port=\"5432\" UserName=\"test\" /></Connections>",
            ),
        ];
        for (source, text) in cases {
            let first = preview(source, text.as_bytes()).unwrap();
            let second = preview(source, text.as_bytes()).unwrap();
            assert_eq!(first.config.profiles, second.config.profiles);
            assert_eq!(first.config.profiles.len(), 1);
            assert!(first.config.profiles[0].credential.is_none());
            assert!(!first.warnings.is_empty());
        }
        assert!(
            preview(
                Source::FileZilla,
                b"<!DOCTYPE x [<!ENTITY e SYSTEM 'file:///etc/passwd'>]><x>&e;</x>"
            )
            .is_err()
        );
        assert!(preview(Source::Navicat,b"<Connections><Connection ConnectionName='dev' ConnType='MySQL' Host='localhost' UseSSH='true'/></Connections>").is_err());
    }
}
