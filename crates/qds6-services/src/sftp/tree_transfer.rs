//! 功能：有界目录传输，不跟随链接、拒绝覆盖，失败清理本次创建的目标；时间：2026-11-11；作者：lq。
use super::*;

/// 功能：先验证整棵源树，再开始创建目标，限制层级和节点总数；时间：2026-11-11；作者：lq。
#[derive(Default)]
struct Plan {
    directories: Vec<PathBuf>,
    files: Vec<PathBuf>,
    bytes: u64,
}
impl Plan {
    /// 功能：规划上限避免超大树耗尽内存或递归栈；时间：2026-11-11；作者：lq。
    fn check(&self, depth: usize, progress: &Transfer) -> Result<(), String> {
        progress.check()?;
        if depth > 64 || self.directories.len() + self.files.len() >= 10000 {
            return Err("文件夹超过 64 层或 10000 项，请分批传输".into());
        }
        Ok(())
    }
    /// 功能：累计总大小时检测溢出；时间：2026-11-11；作者：lq。
    fn file(&mut self, relative: &Path, bytes: u64) -> Result<(), String> {
        self.bytes = self.bytes.checked_add(bytes).ok_or("传输总大小超出范围")?;
        self.files.push(relative.into());
        Ok(())
    }
    /// 功能：只允许路径分量形式的 UTF-8 文件名，防止服务端目录项越界；时间：2026-11-11；作者：lq。
    fn child(relative: &Path, name: &str) -> Result<PathBuf, String> {
        if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\', '\0']) {
            return Err("文件名不能安全传输".into());
        }
        Ok(relative.join(name))
    }
    /// 功能：本地预检拒绝链接和设备等非常规类型；时间：2026-11-11；作者：lq。
    fn local(
        &mut self,
        root: &Path,
        relative: &Path,
        depth: usize,
        progress: &Transfer,
    ) -> Result<(), String> {
        self.check(depth, progress)?;
        let path = if relative.as_os_str().is_empty() {
            root.to_path_buf()
        } else {
            root.join(relative)
        };
        let metadata = std::fs::symlink_metadata(&path).map_err(|_| "无法读取本地源属性")?;
        if metadata.is_file() {
            self.file(relative, metadata.len())
        } else if metadata.is_dir() {
            self.directories.push(relative.into());
            for entry in std::fs::read_dir(&path).map_err(|_| "无法读取本地文件夹")? {
                let entry = entry.map_err(|_| "读取本地文件夹失败")?;
                let name = entry.file_name();
                let name = name.to_str().ok_or("源含非 UTF-8 文件名")?;
                self.local(root, &Self::child(relative, name)?, depth + 1, progress)?;
            }
            Ok(())
        } else {
            Err("源文件夹含符号链接或非常规文件，请单独处理".into())
        }
    }
    /// 功能：远程规划使用 lstat，不进入链接目录；时间：2026-11-11；作者：lq。
    fn remote(
        &mut self,
        client: &Client,
        root: &Path,
        relative: &Path,
        depth: usize,
        progress: &Transfer,
    ) -> Result<(), String> {
        self.check(depth, progress)?;
        let path = if relative.as_os_str().is_empty() {
            root.to_path_buf()
        } else {
            root.join(relative)
        };
        let metadata = client.sftp.lstat(&path).map_err(|_| "无法读取远程源属性")?;
        if metadata.is_file() {
            self.file(relative, metadata.size.unwrap_or(0))
        } else if metadata.is_dir() {
            self.directories.push(relative.into());
            for entry in client.list(&path)? {
                self.remote(
                    client,
                    root,
                    &Self::child(relative, &entry.name)?,
                    depth + 1,
                    progress,
                )?;
            }
            Ok(())
        } else {
            Err("源文件夹含符号链接或非常规文件，请单独处理".into())
        }
    }
}

impl Client {
    /// 功能：文件直接上传，文件夹先传排他暂存目录再无覆盖重命名；时间：2026-11-11；作者：lq。
    pub fn upload_path(
        &self,
        local: &Path,
        remote: &Path,
        progress: &Transfer,
    ) -> Result<(), String> {
        let mut plan = Plan::default();
        plan.local(local, Path::new(""), 0, progress)?;
        if plan.directories.is_empty() {
            return self.upload(local, remote, progress);
        }
        if self.sftp.lstat(remote).is_ok() {
            return Err("目标已存在，请重命名源项或选择其他目录".into());
        }
        let temporary = temporary_path(remote)?;
        progress.total.store(plan.bytes, Ordering::Relaxed);
        progress.completed.store(0, Ordering::Relaxed);
        self.mkdir(&temporary)?;
        let result = (|| {
            for directory in plan.directories.iter().skip(1) {
                progress.check()?;
                self.mkdir(&temporary.join(directory))?;
            }
            for file in &plan.files {
                self.upload_file(&local.join(file), &temporary.join(file), progress, false)?;
            }
            progress.check()?;
            self.rename(&temporary, remote)
        })();
        if let Err(error) = result {
            // 功能：仅清理本次 UUID 暂存树，绝不删除原有同名目标；时间：2026-11-11；作者：lq。
            for file in plan.files.iter().rev() {
                let _ = self.sftp.unlink(&temporary.join(file));
            }
            for directory in plan.directories.iter().rev() {
                let _ = self.sftp.rmdir(&temporary.join(directory));
            }
            return Err(if self.sftp.lstat(&temporary).is_ok() {
                format!("{error}；暂存目录清理失败：{}", temporary.display())
            } else {
                error
            });
        }
        Ok(())
    }

    /// 功能：目录下载排他创建根目录，失败清理本次目标，保留已有同名内容；时间：2026-11-11；作者：lq。
    pub fn download_path(
        &self,
        remote: &Path,
        local: &Path,
        progress: &Transfer,
    ) -> Result<(), String> {
        let mut plan = Plan::default();
        plan.remote(self, remote, Path::new(""), 0, progress)?;
        if plan.directories.is_empty() {
            return self.download(remote, local, progress);
        }
        progress.total.store(plan.bytes, Ordering::Relaxed);
        progress.completed.store(0, Ordering::Relaxed);
        std::fs::create_dir(local).map_err(|_| "无法创建本地目标；目标可能已存在")?;
        // 功能：只记录和清理本次实际创建的项，保留并发写入的其他内容；时间：2026-11-11；作者：lq。
        let mut created_directories = vec![local.to_path_buf()];
        let mut created_files = Vec::new();
        let result = (|| {
            for directory in plan.directories.iter().skip(1) {
                progress.check()?;
                let directory = local.join(directory);
                std::fs::create_dir(&directory).map_err(|_| "无法创建本地子目录")?;
                created_directories.push(directory);
            }
            for file in &plan.files {
                self.download_file(&remote.join(file), &local.join(file), progress, false)?;
                created_files.push(local.join(file));
            }
            progress.check()
        })();
        if let Err(error) = result {
            for file in created_files.iter().rev() {
                let _ = std::fs::remove_file(file);
            }
            for directory in created_directories.iter().rev() {
                let _ = std::fs::remove_dir(directory);
            }
            return Err(if local.try_exists().unwrap_or(true) {
                format!("{error}；未完成目录清理失败：{}", local.display())
            } else {
                error
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    /// 功能：验证层级、空目录、总大小、取消和不跟随符号链接的真实规划；时间：2026-11-11；作者：lq。
    #[test]
    fn local_tree_plan_is_bounded_and_cancellable() {
        let root = std::env::temp_dir().join(format!("qds6-plan-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("nested/empty")).unwrap();
        std::fs::write(root.join("nested/中文.txt"), b"1234").unwrap();
        let progress = Transfer::default();
        let mut plan = Plan::default();
        plan.local(&root, Path::new(""), 0, &progress).unwrap();
        assert_eq!(plan.bytes, 4);
        assert_eq!(plan.directories.len(), 3);
        assert_eq!(plan.files.len(), 1);
        // 功能：单文件根不能追加空分量变成带尾斜杠的目录路径；时间：2026-11-11；作者：lq。
        let mut single = Plan::default();
        single
            .local(&root.join("nested/中文.txt"), Path::new(""), 0, &progress)
            .unwrap();
        assert_eq!(single.bytes, 4);
        assert!(single.directories.is_empty());
        assert!(plan.check(65, &progress).is_err());
        assert!(Plan::child(Path::new(""), "../outside").is_err());
        progress.cancel.store(true, Ordering::Relaxed);
        assert!(
            Plan::default()
                .local(&root, Path::new(""), 0, &progress)
                .is_err()
        );
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(root.join("nested"), root.join("link")).unwrap();
            assert!(
                Plan::default()
                    .local(&root, Path::new(""), 0, &Transfer::default())
                    .is_err()
            );
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
