//! 功能：连接配置与受控文本校验，不持有持久密码、不发起连接；时间：2026-11-10；作者：lq。

use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

// 功能：终端主题模型由偏好、状态机与原生预览共用；时间：2026-11-11；作者：lq。
mod terminal_theme;
pub use terminal_theme::{TerminalPalette, TerminalTheme};

/// 功能：首版支持的协议及各自默认端口；时间：2026-11-10；作者：lq。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Protocol {
    Ssh,
    Sftp,
    MySql,
    PostgreSql,
}

impl Protocol {
    /// 功能：统一协议选择顺序；时间：2026-11-10；作者：lq。
    pub const ALL: [Self; 4] = [Self::Ssh, Self::Sftp, Self::MySql, Self::PostgreSql];

    /// 功能：获取新连接的默认端口；时间：2026-11-10；作者：lq。
    pub const fn default_port(self) -> u16 {
        match self {
            Self::Ssh | Self::Sftp => 22,
            Self::MySql => 3306,
            Self::PostgreSql => 5432,
        }
    }

    /// 功能：用于界面的协议名称；时间：2026-11-10；作者：lq。
    pub const fn label(self) -> &'static str {
        match self {
            Self::Ssh => "SSH",
            Self::Sftp => "SFTP",
            Self::MySql => "MySQL",
            Self::PostgreSql => "PostgreSQL",
        }
    }
}

/// 功能：凭据库中的不透明引用，禁止把密码放入普通连接模型；时间：2026-11-10；作者：lq。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CredentialRef(pub Uuid);

/// 功能：SSH 非秘密选项，旧版配置通过默认值迁移；时间：2026-11-10；作者：lq。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct SshOptions {
    pub private_key: String,
    pub use_agent: bool,
    pub jump_profile: Option<Uuid>,
}

/// 功能：数据库证书验证策略，默认严格验证；时间：2026-11-10；作者：lq。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TlsMode {
    #[default]
    VerifyFull,
    Disabled,
}

/// 功能：数据库 TLS 和单跳隧道引用；时间：2026-11-10；作者：lq。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct DatabaseOptions {
    pub tls: TlsMode,
    pub ca_file: String,
    pub tunnel_profile: Option<Uuid>,
}

/// 功能：可持久化的非秘密连接配置，生产标记由用户明确选择；时间：2026-11-10；作者：lq。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectionProfile {
    pub id: Uuid,
    pub name: String,
    pub group: String,
    pub protocol: Protocol,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub database: String,
    pub favorite: bool,
    pub production: bool,
    pub credential: Option<CredentialRef>,
    #[serde(default)]
    pub ssh: SshOptions,
    #[serde(default)]
    pub db: DatabaseOptions,
}

/// 功能：对无效配置提供可显示错误，不回显秘密；时间：2026-11-10；作者：lq。
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ProfileError {
    #[error("连接名称不能为空且不能超过 128 个字符")]
    InvalidName,
    #[error("主机必须是主机名或 IP，不包含协议、路径、空白或用户信息")]
    InvalidHost,
    #[error("端口必须在 1–65535 之间")]
    InvalidPort,
    #[error("分组、用户名或数据库名过长或含控制字符")]
    InvalidField,
}

impl ConnectionProfile {
    /// 功能：创建断开状态使用的配置；时间：2026-11-10；作者：lq。
    pub fn new(protocol: Protocol) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: String::new(),
            group: String::new(),
            protocol,
            host: String::new(),
            port: protocol.default_port(),
            username: String::new(),
            database: String::new(),
            favorite: false,
            production: false,
            credential: None,
            ssh: SshOptions::default(),
            db: DatabaseOptions::default(),
        }
    }

    /// 功能：写入前验证配置，IPv6 地址允许冒号；时间：2026-11-10；作者：lq。
    pub fn validate(&self) -> Result<(), ProfileError> {
        if self.name.trim().is_empty()
            || self.name.chars().count() > 128
            || self.name.chars().any(char::is_control)
        {
            return Err(ProfileError::InvalidName);
        }
        if self.host.trim().is_empty()
            || self.host.starts_with('-')
            || self.host.len() > 253
            || self
                .host
                .chars()
                .any(|c| c.is_whitespace() || c.is_control() || "/\\@?#".contains(c))
        {
            return Err(ProfileError::InvalidHost);
        }
        if self.port == 0 {
            return Err(ProfileError::InvalidPort);
        }
        if [&self.group, &self.username, &self.database]
            .iter()
            .any(|s| s.chars().count() > 256 || s.chars().any(char::is_control))
        {
            return Err(ProfileError::InvalidField);
        }
        if [&self.ssh.private_key, &self.db.ca_file]
            .iter()
            .any(|path| path.len() > 4096 || path.chars().any(char::is_control))
            || self.ssh.jump_profile == Some(self.id)
            || self.db.tunnel_profile == Some(self.id)
        {
            return Err(ProfileError::InvalidField);
        }
        Ok(())
    }

    /// 功能：本地搜索连接名称、分组、协议、主机与用户；时间：2026-11-10；作者：lq。
    pub fn matches(&self, query: &str) -> bool {
        let query = query.trim().to_lowercase();
        [
            &self.name,
            &self.group,
            &self.host,
            &self.username,
            self.protocol.label(),
        ]
        .iter()
        .any(|part| part.to_lowercase().contains(&query))
    }
}

/// 功能：限制待打开文本的内存与单行规模；时间：2026-11-10；作者：lq。
#[derive(Debug, Clone, Copy)]
pub struct TextLimits {
    pub bytes: usize,
    pub lines: usize,
    pub line_bytes: usize,
}

impl Default for TextLimits {
    /// 功能：M0 初始限制，后续以性能实测调整；时间：2026-11-10；作者：lq。
    fn default() -> Self {
        Self {
            bytes: 5 * 1024 * 1024,
            lines: 100_000,
            line_bytes: 64 * 1024,
        }
    }
}

/// 功能：文本拒绝原因；时间：2026-11-10；作者：lq。
#[derive(Debug, Error, PartialEq, Eq)]
pub enum TextError {
    #[error("文件超过可编辑大小限制")]
    TooLarge,
    #[error("文件不是有效 UTF-8；需要先明确选择编码")]
    Encoding,
    #[error("文件包含二进制 NUL 字节")]
    Binary,
    #[error("文本行数或单行长度超过限制")]
    LineLimit,
}

impl TextLimits {
    /// 功能：在创建 UI 文本缓冲前验证；保留原始 CRLF，不静默转换；时间：2026-11-10；作者：lq。
    pub fn validate(self, bytes: &[u8]) -> Result<&str, TextError> {
        if bytes.len() > self.bytes {
            return Err(TextError::TooLarge);
        }
        let text = std::str::from_utf8(bytes).map_err(|_| TextError::Encoding)?;
        if bytes.contains(&0) {
            return Err(TextError::Binary);
        }
        let mut count = 0;
        for line in text.split('\n') {
            count += 1;
            if count > self.lines || line.len() > self.line_bytes {
                return Err(TextError::LineLimit);
            }
        }
        Ok(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 功能：验证地址边界、IPv6 与非法协议字符串；时间：2026-11-10；作者：lq。
    #[test]
    fn profile_validation_rejects_ambiguous_addresses() {
        let mut profile = ConnectionProfile::new(Protocol::Ssh);
        profile.name = "生产服务器".into();
        profile.host = "2001:db8::1".into();
        assert!(profile.validate().is_ok());
        for host in ["ssh://example.com", "user@example.com", "a b", "a\n", ""] {
            profile.host = host.into();
            assert_eq!(profile.validate(), Err(ProfileError::InvalidHost));
        }
        profile.host = "example.com".into();
        profile.port = 0;
        assert_eq!(profile.validate(), Err(ProfileError::InvalidPort));
    }

    /// 功能：保证普通配置拒绝意外密码字段并可中文搜索；时间：2026-11-10；作者：lq。
    #[test]
    fn profiles_reject_secret_fields() {
        let mut profile = ConnectionProfile::new(Protocol::PostgreSql);
        profile.name = "开发数据库".into();
        profile.host = "localhost".into();
        assert!(profile.matches("开发"));
        assert!(profile.matches("POSTGRES"));
        let mut value = serde_json::to_value(&profile).unwrap();
        value["password"] = "must-not-be-stored".into();
        assert!(serde_json::from_value::<ConnectionProfile>(value).is_err());
    }

    /// 功能：验证大小、编码、二进制及超长行边界；时间：2026-11-10；作者：lq。
    #[test]
    fn bounded_text_preserves_line_endings() {
        let limits = TextLimits {
            bytes: 16,
            lines: 2,
            line_bytes: 8,
        };
        assert_eq!(limits.validate(b"a\r\nb"), Ok("a\r\nb"));
        assert_eq!(limits.validate(&[b'a'; 17]), Err(TextError::TooLarge));
        assert_eq!(limits.validate(&[0xff]), Err(TextError::Encoding));
        assert_eq!(limits.validate(b"a\0b"), Err(TextError::Binary));
        assert_eq!(limits.validate(b"123456789"), Err(TextError::LineLimit));
        assert_eq!(limits.validate(b"a\nb\nc"), Err(TextError::LineLimit));
        assert!(limits.validate("中文\n".as_bytes()).is_ok());
    }
}
