//! 功能：后台 libssh2 会话、首次指纹确认、密钥变化阻断与单跳转发；时间：2026-11-10；作者：lq。

use base64::{Engine, engine::general_purpose::STANDARD_NO_PAD};
use qds6_core::ConnectionProfile;
use ssh2::Session;
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    net::{TcpListener, TcpStream, ToSocketAddrs},
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
use thiserror::Error;

/// 功能：结构化区分首次信任、指纹变化和一般错误；时间：2026-11-10；作者：lq。
#[derive(Debug, Error)]
pub enum SshError {
    #[error("首次连接，请核对主机 SHA256 指纹：{fingerprint}")]
    UnknownHost { fingerprint: String },
    #[error("主机密钥已变化，连接被阻止。当前指纹：{fingerprint}；请核实服务端后手动移除旧记录")]
    ChangedHost { fingerprint: String },
    #[error("{0}")]
    Other(String),
}

/// 功能：经过确认的指纹记录，按主机及端口分别保存；时间：2026-11-10；作者：lq。
fn known(path: &Path) -> Result<BTreeMap<String, String>, SshError> {
    if !path.exists() {
        return Ok(BTreeMap::new());
    }
    let bytes = crate::backup::read_bounded(path).map_err(SshError::Other)?;
    serde_json::from_slice(&bytes).map_err(|_| SshError::Other("主机指纹文件损坏，禁止连接".into()))
}

/// 功能：明确确认后添加指纹；现有不匹配记录不得自动覆盖；时间：2026-11-10；作者：lq。
pub fn trust_host(
    path: &Path,
    profile: &ConnectionProfile,
    fingerprint: &str,
) -> Result<(), SshError> {
    let mut records = known(path)?;
    let key = format!("[{}]:{}", profile.host, profile.port);
    if records.get(&key).is_some_and(|old| old != fingerprint) {
        return Err(SshError::ChangedHost {
            fingerprint: fingerprint.into(),
        });
    }
    records.insert(key, fingerprint.into());
    let bytes = serde_json::to_vec_pretty(&records)
        .map_err(|_| SshError::Other("指纹序列化失败".into()))?;
    let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    crate::backup::write_new(&temporary, &bytes).map_err(SshError::Other)?;
    std::fs::rename(&temporary, path).map_err(|_| SshError::Other("无法保存主机指纹".into()))
}

/// 功能：使用明确超时建立 TCP，遍历解析地址但限制总连接时间；时间：2026-11-10；作者：lq。
pub fn tcp_connect(host: &str, port: u16) -> Result<TcpStream, SshError> {
    let start = Instant::now();
    let addresses = (host, port)
        .to_socket_addrs()
        .map_err(|_| SshError::Other("主机解析失败".into()))?;
    for address in addresses.take(8) {
        let remaining = Duration::from_secs(12).saturating_sub(start.elapsed());
        if remaining.is_zero() {
            break;
        }
        if let Ok(socket) =
            TcpStream::connect_timeout(&address, remaining.min(Duration::from_secs(4)))
        {
            let _ = socket.set_nodelay(true);
            return Ok(socket);
        }
    }
    Err(SshError::Other("TCP 连接失败或超时".into()))
}

/// 功能：握手后先验证指纹，再发送任何认证秘密；仅后台调用；时间：2026-11-10；作者：lq。
pub fn connect(
    profile: &ConnectionProfile,
    secret: Option<&str>,
    known_hosts: &Path,
) -> Result<Session, SshError> {
    if profile.ssh.jump_profile.is_some() {
        return Err(SshError::Other("跳板连接需先建立专属 TunnelLease".into()));
    }
    connect_socket(
        profile,
        secret,
        known_hosts,
        tcp_connect(&profile.host, profile.port)?,
    )
}

/// 功能：对已建立的回环/直连 socket 校验原目标身份，不以回环地址替代主机身份；时间：2026-11-10；作者：lq。
pub fn connect_socket(
    profile: &ConnectionProfile,
    secret: Option<&str>,
    known_hosts: &Path,
    socket: TcpStream,
) -> Result<Session, SshError> {
    profile
        .validate()
        .map_err(|e| SshError::Other(e.to_string()))?;
    let mut session = Session::new().map_err(|_| SshError::Other("SSH 初始化失败".into()))?;
    session.set_tcp_stream(socket);
    session.set_timeout(12_000);
    session
        .handshake()
        .map_err(|_| SshError::Other("SSH 握手失败".into()))?;
    let hash = session
        .host_key_hash(ssh2::HashType::Sha256)
        .ok_or_else(|| SshError::Other("服务器没有可用主机指纹".into()))?;
    let fingerprint = format!("SHA256:{}", STANDARD_NO_PAD.encode(hash));
    match known(known_hosts)?.get(&format!("[{}]:{}", profile.host, profile.port)) {
        None => return Err(SshError::UnknownHost { fingerprint }),
        Some(expected) if expected != &fingerprint => {
            return Err(SshError::ChangedHost { fingerprint });
        }
        Some(_) => {}
    }
    let result = if profile.ssh.use_agent {
        session.userauth_agent(&profile.username)
    } else if !profile.ssh.private_key.is_empty() {
        session.userauth_pubkey_file(
            &profile.username,
            None,
            Path::new(&profile.ssh.private_key),
            secret.filter(|s| !s.is_empty()),
        )
    } else {
        session.userauth_password(&profile.username, secret.unwrap_or(""))
    };
    result.map_err(|_| SshError::Other("SSH 认证失败，请检查用户名、密码、私钥或 Agent".into()))?;
    if !session.authenticated() {
        return Err(SshError::Other("SSH 未通过认证".into()));
    }
    session.set_keepalive(true, 20);
    Ok(session)
}

/// 功能：拥有独立 SSH 会话的回环本地隧道，引用释放后停止接入；时间：2026-11-10；作者：lq。
pub struct TunnelLease {
    pub port: u16,
    stop: Arc<AtomicBool>,
}

impl TunnelLease {
    /// 功能：每个隧道独立于终端与 SFTP，仅绑定 127.0.0.1；时间：2026-11-10；作者：lq。
    pub fn start(session: Session, host: String, port: u16) -> Result<Self, String> {
        let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(|_| "无法建立回环隧道")?;
        listener.set_nonblocking(true).map_err(|_| "无法设置隧道")?;
        let local_port = listener
            .local_addr()
            .map_err(|_| "无法读取隧道端口")?
            .port();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        // 功能：最多八个并发转发通道，保留数据库独立取消连接的接入能力；时间：2026-11-10；作者：lq。
        session.set_blocking(false);
        thread::spawn(move || {
            let mut forwards: Vec<Forward> = Vec::new();
            let mut last_keepalive = Instant::now();
            while !worker_stop.load(Ordering::Relaxed) {
                if forwards.len() < 8 {
                    match listener.accept() {
                        Ok((local, _)) => {
                            let deadline = Instant::now() + Duration::from_secs(8);
                            loop {
                                match session.channel_direct_tcpip(&host, port, None) {
                                    Ok(remote) => {
                                        let _ = local.set_nonblocking(true);
                                        forwards.push(Forward {
                                            local,
                                            remote,
                                            to_remote: Vec::new(),
                                            to_local: Vec::new(),
                                        });
                                        break;
                                    }
                                    Err(error)
                                        if error.code() == ssh2::ErrorCode::Session(-37)
                                            && Instant::now() < deadline =>
                                    {
                                        thread::sleep(Duration::from_millis(2))
                                    }
                                    Err(_) => break,
                                }
                            }
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                        Err(_) => break,
                    }
                }
                forwards.retain_mut(Forward::pump);
                if last_keepalive.elapsed() > Duration::from_secs(20) {
                    let _ = session.keepalive_send();
                    last_keepalive = Instant::now();
                }
                thread::sleep(Duration::from_millis(2));
            }
        });
        Ok(Self {
            port: local_port,
            stop,
        })
    }
}

/// 功能：单个转发的数据缓冲硬限制，读写不会阻塞其他通道；时间：2026-11-10；作者：lq。
struct Forward {
    local: TcpStream,
    remote: ssh2::Channel,
    to_remote: Vec<u8>,
    to_local: Vec<u8>,
}
impl Forward {
    /// 功能：每次最多推进一个有界块，EOF/错误结束当前通道；时间：2026-11-10；作者：lq。
    fn pump(&mut self) -> bool {
        let mut buffer = [0; 32768];
        if self.to_remote.len() < 65536 {
            match self.local.read(&mut buffer) {
                Ok(0) => return false,
                Ok(n) => self.to_remote.extend_from_slice(&buffer[..n]),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(_) => return false,
            }
        }
        if self.to_local.len() < 65536 {
            match self.remote.read(&mut buffer) {
                Ok(0) if self.remote.eof() => return false,
                Ok(n) => self.to_local.extend_from_slice(&buffer[..n]),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(_) => return false,
            }
        }
        if !self.to_remote.is_empty() {
            match self.remote.write(&self.to_remote) {
                Ok(n) => {
                    self.to_remote.drain(..n);
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(_) => return false,
            }
        }
        if !self.to_local.is_empty() {
            match self.local.write(&self.to_local) {
                Ok(n) => {
                    self.to_local.drain(..n);
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(_) => return false,
            }
        }
        true
    }
}

impl Drop for TunnelLease {
    /// 功能：结束隧道不影响任何其他独立 SSH 会话；时间：2026-11-10；作者：lq。
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}
