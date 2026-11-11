//! 功能：有界 SFTP 浏览、流式传输与冲突检测保存，所有方法仅后台调用；时间：2026-11-10；作者：lq。

// 功能：目录递归传输规划与清理；时间：2026-11-11；作者：lq。
mod tree_transfer;

use qds6_core::TextLimits;
use sha2::{Digest, Sha256};
use ssh2::{FileStat, OpenFlags, OpenType, RenameFlags, Session, Sftp};
use std::{
    fs::OpenOptions,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

/// 功能：远程目录条目保留路径和类型，拒绝静默转换非 UTF-8；时间：2026-11-10；作者：lq。
#[derive(Clone)]
pub struct Entry {
    pub path: PathBuf,
    pub name: String,
    pub size: u64,
    pub permissions: u32,
    pub directory: bool,
    pub symlink: bool,
    // 功能：保留服务端原始时间和属主编号，缺失属性不伪造；时间：2026-11-11；作者：lq。
    pub modified: Option<u64>,
    pub uid: Option<u32>,
    pub gid: Option<u32>,
}

/// 功能：远程文本基线保留内容摘要及精确原始字节；时间：2026-11-10；作者：lq。
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct RemoteDocument {
    pub path: PathBuf,
    pub text: String,
    pub crlf: bool,
    digest: [u8; 32],
    permissions: u32,
    // 功能：原子替换时保留所属用户/组，无法保留则拒绝提交；时间：2026-11-11；作者：lq。
    #[serde(default)]
    uid: Option<u32>,
    #[serde(default)]
    gid: Option<u32>,
}

/// 功能：传输取消与有界进度，可从 UI 无阻塞读取；时间：2026-11-10；作者：lq。
#[derive(Clone, Default)]
pub struct Transfer {
    pub cancel: Arc<AtomicBool>,
    pub completed: Arc<AtomicU64>,
    pub total: Arc<AtomicU64>,
}

impl Transfer {
    /// 功能：每个传输块检查用户取消；时间：2026-11-10；作者：lq。
    fn check(&self) -> Result<(), String> {
        if self.cancel.load(Ordering::Relaxed) {
            Err("传输已取消，目标未替换".into())
        } else {
            Ok(())
        }
    }
}

/// 功能：SFTP 与终端使用独立连接，避免关闭终端影响传输；时间：2026-11-10；作者：lq。
pub struct Client {
    _session: Session,
    sftp: Sftp,
    _tunnel: Option<Arc<crate::ssh::TunnelLease>>,
}

impl Client {
    /// 功能：从已验证的 SSH 连接创建 SFTP 子系统；时间：2026-11-10；作者：lq。
    pub fn new(session: Session) -> Result<Self, String> {
        let sftp = session.sftp().map_err(|_| "服务器不支持 SFTP 或启动失败")?;
        Ok(Self {
            _session: session,
            sftp,
            _tunnel: None,
        })
    }

    /// 功能：文件服务独立持有跳板隧道，不与终端生命周期绑定；时间：2026-11-10；作者：lq。
    pub fn with_tunnel(mut self, tunnel: Option<Arc<crate::ssh::TunnelLease>>) -> Self {
        self._tunnel = tunnel;
        self
    }

    /// 功能：解析远程真实目录作为树根，避免相对路径在双栏传输中歧义；时间：2026-11-11；作者：lq。
    pub fn canonical_directory(&self, path: &Path) -> Result<PathBuf, String> {
        let path = self.sftp.realpath(path).map_err(|_| "无法解析远程目录")?;
        if !self
            .sftp
            .stat(&path)
            .map_err(|_| "无法读取远程目录属性")?
            .is_dir()
        {
            return Err("远程路径不是文件夹".into());
        }
        if path.to_str().is_none() {
            return Err("远程目录不是 UTF-8".into());
        }
        Ok(path)
    }

    /// 功能：限制目录数量、保留链接标记并按目录和名称排序；时间：2026-11-10；作者：lq。
    pub fn list(&self, path: &Path) -> Result<Vec<Entry>, String> {
        let mut handle = self.sftp.opendir(path).map_err(|_| "无法打开远程目录")?;
        let mut entries = Vec::new();
        loop {
            match handle.readdir() {
                Ok((name, stat)) => {
                    let name = name
                        .to_str()
                        .ok_or("目录包含非 UTF-8 文件名，当前版本不能安全显示")?
                        .to_string();
                    if name == "." || name == ".." {
                        continue;
                    }
                    if entries.len() >= 20000 {
                        return Err("目录超过 20000 项，请使用更小的子目录".into());
                    }
                    entries.push(Entry {
                        path: path.join(&name),
                        name,
                        size: stat.size.unwrap_or(0),
                        permissions: stat.perm.unwrap_or(0),
                        directory: stat.is_dir(),
                        symlink: stat.file_type() == ssh2::FileType::Symlink,
                        modified: stat.mtime,
                        uid: stat.uid,
                        gid: stat.gid,
                    });
                }
                Err(e) if e.code() == ssh2::ErrorCode::Session(-16) => break,
                Err(_) => return Err("读取远程目录失败".into()),
            }
        }
        entries.sort_by(|a, b| b.directory.cmp(&a.directory).then(a.name.cmp(&b.name)));
        Ok(entries)
    }

    /// 功能：查询真实属性，编辑和替换时拒绝符号链接与非常规文件；时间：2026-11-10；作者：lq。
    fn regular(&self, path: &Path) -> Result<FileStat, String> {
        let stat = self.sftp.lstat(path).map_err(|_| "无法读取远程文件属性")?;
        if !stat.is_file() {
            return Err("此操作只支持普通文件；符号链接和目录需单独处理".into());
        }
        Ok(stat)
    }

    /// 功能：有界下载文本，读取后再次验证大小和文本边界；时间：2026-11-10；作者：lq。
    fn read_text(&self, path: &Path) -> Result<(String, FileStat), String> {
        let stat = self.regular(path)?;
        let limits = TextLimits::default();
        if stat.size.unwrap_or(u64::MAX) > limits.bytes as u64 {
            return Err("远程文本超过 5 MiB，请下载后处理".into());
        }
        let mut bytes = Vec::new();
        self.sftp
            .open(path)
            .map_err(|_| "无法打开远程文件")?
            .take(limits.bytes as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "远程文本读取失败")?;
        let text = limits
            .validate(&bytes)
            .map_err(|e| e.to_string())?
            .to_string();
        Ok((text, stat))
    }

    /// 功能：建立内容摘要基线，不将 mtime 当成强版本；时间：2026-11-10；作者：lq。
    pub fn open_document(&self, path: &Path) -> Result<RemoteDocument, String> {
        let (text, stat) = self.read_text(path)?;
        let digest = Sha256::digest(text.as_bytes()).into();
        let crlf = text.contains("\r\n");
        Ok(RemoteDocument {
            path: path.into(),
            text,
            crlf,
            digest,
            permissions: stat.perm.ok_or("服务器未返回权限，不能安全保存")? & 0o7777,
            uid: stat.uid,
            gid: stat.gid,
        })
    }

    /// 功能：重读比较基线、同目录临时上传、保留权限后请求原子替换；失败保留草稿；时间：2026-11-10；作者：lq。
    pub fn save_document(
        &self,
        baseline: &RemoteDocument,
        text: &str,
    ) -> Result<RemoteDocument, String> {
        TextLimits::default()
            .validate(text.as_bytes())
            .map_err(|e| e.to_string())?;
        let current = self.open_document(&baseline.path)?;
        if baseline.uid.is_none() || baseline.gid.is_none() {
            return Err(
                "基线缺少属主信息，请复制草稿后重新打开；无法保留属主时不替换原文件".into(),
            );
        }
        if current.digest != baseline.digest
            || current.permissions != baseline.permissions
            || current.uid != baseline.uid
            || current.gid != baseline.gid
        {
            return Err("远端内容、权限或属主已变化，保存被阻止；请复制草稿并重新打开比较".into());
        }
        let temporary = temporary_path(&baseline.path)?;
        let result = (|| {
            let mut file = self
                .sftp
                .open_mode(
                    &temporary,
                    OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::EXCLUSIVE,
                    baseline.permissions as i32,
                    OpenType::File,
                )
                .map_err(|_| "无法创建远端临时文件")?;
            file.write_all(text.as_bytes())
                .map_err(|_| "远程上传失败，原文件未替换")?;
            file.setstat(FileStat {
                perm: Some(baseline.permissions),
                size: None,
                uid: baseline.uid,
                gid: baseline.gid,
                atime: None,
                mtime: None,
            })
            .map_err(|_| "无法保留远端权限/属主；原文件未替换")?;
            file.close().map_err(|_| "关闭远端临时文件失败")?;
            // 功能：提交前再次比较，仍不宣称具备跨服务端 CAS 保证；时间：2026-11-10；作者：lq。
            let check = self.open_document(&baseline.path)?;
            if check.digest != baseline.digest
                || check.permissions != baseline.permissions
                || check.uid != baseline.uid
                || check.gid != baseline.gid
            {
                return Err("上传期间远端发生变化，未替换原文件".into());
            }
            self.posix_replace(&temporary, &baseline.path)?;
            let mut saved = baseline.clone();
            saved.text = text.into();
            saved.digest = Sha256::digest(text.as_bytes()).into();
            Ok(saved)
        })();
        if result.is_err() {
            let _ = self.sftp.unlink(&temporary);
        }
        result
    }

    /// 功能：创建目录，权限默认仅当前用户；时间：2026-11-10；作者：lq。
    pub fn mkdir(&self, path: &Path) -> Result<(), String> {
        self.sftp
            .mkdir(path, 0o700)
            .map_err(|_| "创建远程目录失败".into())
    }

    /// 功能：在独立 SFTP v3 子通道协商并调用 OpenSSH posix-rename 扩展；无能力则明确失败；时间：2026-11-10；作者：lq。
    fn posix_replace(&self, source: &Path, target: &Path) -> Result<(), String> {
        // 功能：ssh2 0.9 未公开 libssh2 新增 posix_rename API；仅实现此标准扩展封包，不进入不安全 FFI；时间：2026-11-10；作者：lq。
        let mut channel = self
            ._session
            .channel_session()
            .map_err(|_| "无法打开原子保存通道")?;
        channel
            .subsystem("sftp")
            .map_err(|_| "服务器拒绝原子保存通道")?;
        write_packet(&mut channel, &[1, 0, 0, 0, 3])?;
        let version = read_packet(&mut channel)?;
        if version.len() < 5 || version[0] != 2 || version[1..5] != [0, 0, 0, 3] {
            return Err("服务器未协商 SFTP v3，未替换原文件".into());
        }
        let mut extensions = &version[5..];
        let mut supported = false;
        while !extensions.is_empty() {
            let name = read_string(&mut extensions)?;
            let value = read_string(&mut extensions)?;
            if name == b"posix-rename@openssh.com" && value == b"1" {
                supported = true;
            }
        }
        if !supported {
            return Err("服务器未提供 posix-rename 原子替换扩展，草稿已保留".into());
        }
        let source = source.to_str().ok_or("源路径不是 UTF-8")?;
        let target = target.to_str().ok_or("目标路径不是 UTF-8")?;
        let mut request = vec![200, 0, 0, 0, 1];
        for value in ["posix-rename@openssh.com", source, target] {
            let bytes = value.as_bytes();
            request.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
            request.extend_from_slice(bytes);
        }
        write_packet(&mut channel, &request)?;
        let status = read_packet(&mut channel)?;
        if status.len() < 9
            || status[0] != 101
            || status[1..5] != [0, 0, 0, 1]
            || status[5..9] != [0, 0, 0, 0]
        {
            return Err("服务器拒绝原子替换，原文件未主动截断".into());
        }
        let _ = channel.send_eof();
        let _ = channel.close();
        Ok(())
    }

    /// 功能：重命名但不覆盖已存在路径；时间：2026-11-10；作者：lq。
    pub fn rename(&self, from: &Path, to: &Path) -> Result<(), String> {
        self.sftp
            .rename(from, to, Some(RenameFlags::empty()))
            .map_err(|_| "重命名失败；目标可能已存在".into())
    }

    /// 功能：删除所选文件或空目录，不跟随链接、不递归；时间：2026-11-10；作者：lq。
    pub fn remove(&self, path: &Path) -> Result<(), String> {
        let stat = self.sftp.lstat(path).map_err(|_| "读取目标属性失败")?;
        if stat.is_dir() {
            self.sftp.rmdir(path)
        } else {
            self.sftp.unlink(path)
        }
        .map_err(|_| "删除失败，目录必须为空".into())
    }

    /// 功能：只修改常规文件或目录的八进制权限；时间：2026-11-10；作者：lq。
    pub fn chmod(&self, path: &Path, permissions: u32) -> Result<(), String> {
        if permissions > 0o7777 {
            return Err("权限范围应为 0000–7777".into());
        }
        let stat = self.sftp.lstat(path).map_err(|_| "读取目标属性失败")?;
        if !stat.is_file() && !stat.is_dir() {
            return Err("拒绝修改符号链接权限".into());
        }
        self.sftp
            .setstat(
                path,
                FileStat {
                    perm: Some(permissions),
                    size: None,
                    uid: None,
                    gid: None,
                    atime: None,
                    mtime: None,
                },
            )
            .map_err(|_| "权限修改失败".into())
    }

    /// 功能：上传至排他临时文件并在成功后无覆盖重命名；时间：2026-11-10；作者：lq。
    pub fn upload(&self, local: &Path, remote: &Path, progress: &Transfer) -> Result<(), String> {
        self.upload_file(local, remote, progress, true)
    }

    /// 功能：单文件与文件夹传输共用排他上传，目录任务累计字节；时间：2026-11-11；作者：lq。
    fn upload_file(
        &self,
        local: &Path,
        remote: &Path,
        progress: &Transfer,
        reset: bool,
    ) -> Result<(), String> {
        progress.check()?;
        if std::fs::symlink_metadata(local)
            .map_err(|_| "无法读取本地源文件")?
            .file_type()
            .is_symlink()
        {
            return Err("不上传符号链接".into());
        }
        let mut input = std::fs::File::open(local).map_err(|_| "无法打开本地源文件")?;
        let metadata = input.metadata().map_err(|_| "无法读取本地属性")?;
        if !metadata.is_file() {
            return Err("仅支持普通文件上传".into());
        }
        if reset {
            progress.total.store(metadata.len(), Ordering::Relaxed);
            progress.completed.store(0, Ordering::Relaxed);
        }
        let temporary = temporary_path(remote)?;
        let result = (|| {
            let mut output = self
                .sftp
                .open_mode(
                    &temporary,
                    OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::EXCLUSIVE,
                    0o600,
                    OpenType::File,
                )
                .map_err(|_| "无法创建远程临时文件")?;
            copy(&mut input, &mut output, progress)?;
            output.close().map_err(|_| "远程关闭失败")?;
            progress.check()?;
            self.sftp
                .rename(&temporary, remote, Some(RenameFlags::empty()))
                .map_err(|_| "上传提交失败；不会覆盖现有目标".into())
        })();
        if result.is_err() {
            let _ = self.sftp.unlink(&temporary);
        }
        result
    }

    /// 功能：下载到 0600 临时文件，成功后无覆盖发布；取消清理部分文件；时间：2026-11-10；作者：lq。
    pub fn download(&self, remote: &Path, local: &Path, progress: &Transfer) -> Result<(), String> {
        self.download_file(remote, local, progress, true)
    }

    /// 功能：单文件与文件夹共用安全下载发布，取消清理临时文件；时间：2026-11-11；作者：lq。
    fn download_file(
        &self,
        remote: &Path,
        local: &Path,
        progress: &Transfer,
        reset: bool,
    ) -> Result<(), String> {
        progress.check()?;
        let stat = self.regular(remote)?;
        if reset {
            progress
                .total
                .store(stat.size.unwrap_or(0), Ordering::Relaxed);
            progress.completed.store(0, Ordering::Relaxed);
        }
        let temporary = temporary_path(local)?;
        let result = (|| {
            let mut input = self.sftp.open(remote).map_err(|_| "无法打开远程源文件")?;
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut output = options
                .open(&temporary)
                .map_err(|_| "无法创建本地临时文件")?;
            copy(&mut input, &mut output, progress)?;
            output.sync_all().map_err(|_| "本地刷盘失败")?;
            progress.check()?;
            // 功能：hard_link 排他发布，避免 exists+rename 的覆盖竞争；时间：2026-11-10；作者：lq。
            std::fs::hard_link(&temporary, local)
                .map_err(|_| "下载提交失败；目标已存在或文件系统不支持安全发布")?;
            Ok(())
        })();
        let _ = std::fs::remove_file(&temporary);
        result
    }
}

/// 功能：临时文件始终位于目标目录，不接受归档路径；时间：2026-11-10；作者：lq。
fn temporary_path(target: &Path) -> Result<PathBuf, String> {
    let parent = target.parent().ok_or("目标缺少父目录")?;
    Ok(parent.join(format!(".qds6-{}.tmp", uuid::Uuid::new_v4())))
}

/// 功能：有界 SFTP 扩展响应读取，禁止不可信服务端分配超大包；时间：2026-11-10；作者：lq。
fn read_packet(channel: &mut ssh2::Channel) -> Result<Vec<u8>, String> {
    let mut length = [0; 4];
    channel
        .read_exact(&mut length)
        .map_err(|_| "SFTP 扩展读取失败")?;
    let length = u32::from_be_bytes(length) as usize;
    if !(1..=65536).contains(&length) {
        return Err("SFTP 扩展包超过限制".into());
    }
    let mut packet = vec![0; length];
    channel
        .read_exact(&mut packet)
        .map_err(|_| "SFTP 扩展响应不完整")?;
    Ok(packet)
}

/// 功能：长度前缀封装扩展请求，不拼接命令或 shell；时间：2026-11-10；作者：lq。
fn write_packet(channel: &mut ssh2::Channel, packet: &[u8]) -> Result<(), String> {
    channel
        .write_all(&(packet.len() as u32).to_be_bytes())
        .and_then(|_| channel.write_all(packet))
        .map_err(|_| "SFTP 扩展请求失败".into())
}

/// 功能：解析有界 SFTP 字符串字段，拒绝截断/越界；时间：2026-11-10；作者：lq。
fn read_string<'a>(bytes: &mut &'a [u8]) -> Result<&'a [u8], String> {
    if bytes.len() < 4 {
        return Err("SFTP 扩展字段不完整".into());
    }
    let length = u32::from_be_bytes(bytes[..4].try_into().expect("four bytes")) as usize;
    if length > bytes.len() - 4 {
        return Err("SFTP 扩展字段越界".into());
    }
    let value = &bytes[4..4 + length];
    *bytes = &bytes[4 + length..];
    Ok(value)
}

/// 功能：64 KiB 流式传输，取消和写入失败不报告成功；时间：2026-11-10；作者：lq。
fn copy(input: &mut impl Read, output: &mut impl Write, progress: &Transfer) -> Result<(), String> {
    let mut buffer = [0; 65536];
    loop {
        progress.check()?;
        let n = input.read(&mut buffer).map_err(|_| "传输读取失败")?;
        if n == 0 {
            break;
        }
        output
            .write_all(&buffer[..n])
            .map_err(|_| "传输写入失败（空间或权限不足）")?;
        progress.completed.fetch_add(n as u64, Ordering::Relaxed);
    }
    output.flush().map_err(|_| "传输刷新失败".into())
}
