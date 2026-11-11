//! 功能：四类外部连接配置的保守导入预览，不执行来源内容、不解密来源密码；时间：2026-11-10；作者：lq。

use crate::backup::{Configuration, MAX_FILE};
use qds6_core::{ConnectionProfile, Protocol};
use quick_xml::{Reader, events::Event};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

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

/// 功能：限制输入并按来源解析，统一报告秘密和外部文件不迁移；时间：2026-11-10；作者：lq。
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
    let mut result = Preview { config: Configuration::new(&[]), warnings: vec!["来源密码和私钥本体不会迁移；导入后请重新输入认证信息。仅支持当前解析字段，非完整版本兼容声明。".into()] };
    match source {
        Source::Putty => putty(&text, &mut result)?,
        Source::FinalShell => final_shell(&text, &mut result)?,
        Source::FileZilla => xml_import(&text, false, &mut result)?,
        Source::Navicat => xml_import(&text, true, &mut result)?,
    }
    if result.config.profiles.is_empty() {
        return Err("文件中没有可安全导入的受支持连接，请检查兼容报告或使用原工具重新导出".into());
    }
    let mut ids = std::collections::HashSet::new();
    result.config.profiles.retain(|p| ids.insert(p.id));
    result.config.validate()?;
    Ok(result)
}

/// 功能：已验证字段构造统一模型，ID 由可映射连接身份确定以保持重复导入幂等；时间：2026-11-10；作者：lq。
fn add(out: &mut Preview, mut profile: ConnectionProfile) {
    if out.config.profiles.len() >= 10000 {
        out.warnings.push("超过 10000 项的连接已跳过".into());
        return;
    }
    if profile.validate().is_err() {
        out.warnings
            .push("一项连接缺少有效名称、主机或端口，已跳过".into());
        return;
    }
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
    profile.id = uuid::Uuid::from_bytes(id);
    out.config.profiles.push(profile);
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
        add(out, profile);
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

/// 功能：FinalShell JSON 单项/数组；保护字段仅报告，未支持代理配置跳过；时间：2026-11-10；作者：lq。
fn final_shell(text: &str, out: &mut Preview) -> Result<(), String> {
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|_| "FinalShell 文件不是支持的 JSON；不解析加密归档")?;
    let items = value.as_array().cloned().unwrap_or_else(|| vec![value]);
    for item in items {
        let get = |key: &str| {
            item.get(key)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        };
        if item
            .get("proxy")
            .is_some_and(|v| !v.is_null() && v != &serde_json::Value::Bool(false))
            || item.get("proxy_id").is_some()
        {
            out.warnings
                .push("FinalShell 代理配置未映射，该连接已跳过".into());
            continue;
        }
        if item
            .get("type")
            .and_then(|v| v.as_i64())
            .is_some_and(|t| t != 1001)
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
        if item.get("password").is_some() {
            out.warnings
                .push("FinalShell 保护密码不可迁移，需要重新输入".into());
        }
        if item.get("private_key_path").is_some() {
            out.warnings
                .push("FinalShell 私钥需重新选择，本次未读取外部文件".into());
        }
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
                ]
                .contains(&k.as_str())
            })
        {
            out.warnings
                .push("FinalShell 含未映射字段，已保留可映射的基础连接".into());
        }
        add(out, profile);
    }
    Ok(())
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

/// 功能：FileZilla SFTP 与 Navicat MySQL/Pg 基础字段映射，TLS/隧道未支持项阻断导入；时间：2026-11-10；作者：lq。
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
            let port = node.get("Port");
            if !port.is_empty() {
                profile.port = port.parse().map_err(|_| "XML 连接端口无效")?;
            }
            if navicat {
                out.warnings.push("Navicat 导入默认启用严格 TLS；请核对源连接的证书与隧道设置。未映射字段不会自动执行。".into());
            }
            add(out, profile);
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
