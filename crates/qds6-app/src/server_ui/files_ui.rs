//! 功能：原生目录导航、当前目录详情、多选拖放和串行传输；时间：2026-11-11；作者：lq。

use super::*;
// 功能：目录树与当前目录详情列表分离；时间：2026-11-11；作者：lq。
mod explorer;
use explorer::{local_owner, local_permissions};
// 功能：真实 SFTP 隐藏目录和原生侧栏布局专项回归；时间：2026-11-11；作者：lq。
#[cfg(feature = "ui-tests")]
pub(super) mod smoke;
use gpui_kit::component::{
    list::ListItem,
    menu::{DropdownMenu, PopupMenuItem},
    resizable::h_resizable,
    tree::{TreeEvent, TreeItem, TreeState, tree},
};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    path::Path,
    time::Duration,
};

/// 功能：传输方向及树状态分离，避免两侧路径互相覆盖；时间：2026-11-11；作者：lq。
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum FileSide {
    Local,
    Remote,
}
impl FileSide {
    /// 功能：提供稳定的原生控件标识；时间：2026-11-11；作者：lq。
    fn id(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Remote => "remote",
        }
    }
}

/// 功能：按绝对路径缓存子目录，目录只在展开时后台读取；时间：2026-11-11；作者：lq。
pub(super) struct FileBrowser {
    pub(super) root: PathBuf,
    // 功能：树根与当前目录独立，多选范围始终限定当前可见层级；时间：2026-11-11；作者：lq。
    current: PathBuf,
    selection: BTreeSet<PathBuf>,
    anchor: Option<PathBuf>,
    focus: FocusHandle,
    scroll: UniformListScrollHandle,
    // 功能：每侧独立控制目录导航与详情横向位置；时间：2026-11-11；作者：lq。
    horizontal: ScrollHandle,
    show_directories: bool,
    pub(super) selected: Option<sftp::Entry>,
    tree: Entity<TreeState>,
    entries: BTreeMap<PathBuf, Vec<sftp::Entry>>,
    expanded: BTreeSet<PathBuf>,
    loading: BTreeSet<PathBuf>,
    errors: BTreeMap<PathBuf, String>,
    generation: u64,
}
impl FileBrowser {
    /// 功能：统计隐藏项，避免把 /root 等主目录因过滤而误报为空；时间：2026-11-11；作者：lq。
    fn hidden_count(&self, path: &Path) -> usize {
        self.entries
            .get(path)
            .into_iter()
            .flatten()
            .filter(|entry| entry.name.starts_with('.'))
            .count()
    }

    /// 功能：未加载、失败、隐藏过滤与真正空目录使用不同提示；时间：2026-11-11；作者：lq。
    fn empty_hint(&self, path: &Path, show_hidden: bool) -> String {
        if self.loading.contains(path) {
            "正在读取…".into()
        } else if self.errors.contains_key(path) {
            "读取失败 · 收起再展开重试".into()
        } else if self.entries.contains_key(path) {
            let hidden = self.hidden_count(path);
            if !show_hidden && hidden > 0 {
                format!("仅含 {hidden} 个隐藏项 · 勾选显示隐藏文件")
            } else {
                "空文件夹".into()
            }
        } else {
            "展开以读取".into()
        }
    }

    /// 功能：按钮和空白区拖放始终传入对侧当前目录；时间：2026-11-11；作者：lq。
    pub(super) fn destination(&self) -> PathBuf {
        self.current.clone()
    }
    /// 功能：统一定位根节点和缓存文件，不解析占位提示行；时间：2026-11-11；作者：lq。
    fn entry(&self, path: &Path) -> Option<sftp::Entry> {
        if path == self.root {
            Some(sftp::Entry {
                path: path.into(),
                name: path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into(),
                directory: true,
                symlink: false,
                size: 0,
                permissions: 0,
                modified: None,
                uid: None,
                gid: None,
            })
        } else {
            self.entries
                .values()
                .flatten()
                .find(|entry| entry.path == path)
                .cloned()
        }
    }
}

/// 功能：拖放负载携带所属会话和真实路径，跨会话或同侧拖动不产生操作；时间：2026-11-11；作者：lq。
#[derive(Clone)]
struct FileDrag {
    owner: EntityId,
    side: FileSide,
    entries: std::rc::Rc<std::cell::RefCell<Vec<sftp::Entry>>>,
}
impl Render for FileDrag {
    /// 功能：拖动预览明确展示方向和文件名；时间：2026-11-11；作者：lq。
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .px_3()
            .py_2()
            .gap_2()
            .bg(cx.theme().background)
            .border_1()
            .border_color(cx.theme().primary)
            .rounded_md()
            .child(
                Icon::new(if self.side == FileSide::Local {
                    UiIcon::Upload
                } else {
                    UiIcon::Download
                })
                .small(),
            )
            .child(if self.entries.borrow().len() == 1 {
                self.entries.borrow()[0].name.clone()
            } else {
                format!("{} 个项目", self.entries.borrow().len())
            })
    }
}

/// 功能：队列捕获不可变源与目标，切换选中项或目录不改变排队任务；时间：2026-11-11；作者：lq。
#[derive(Clone)]
struct TransferRequest {
    side: FileSide,
    source: PathBuf,
    target: PathBuf,
}
impl TransferRequest {
    /// 功能：短状态文本保留方向与文件名；时间：2026-11-11；作者：lq。
    fn label(&self) -> String {
        format!(
            "{} {}",
            if self.side == FileSide::Local {
                "上传"
            } else {
                "下载"
            },
            self.source
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
        )
    }
}

/// 功能：每台服务器独立保存双树、订阅、当前任务、排队项目及最近结果；时间：2026-11-11；作者：lq。
pub(super) struct FilesUi {
    pub(super) local: FileBrowser,
    pub(super) remote: FileBrowser,
    queue: VecDeque<TransferRequest>,
    active: Option<TransferRequest>,
    history: VecDeque<String>,
    progress_task: Option<Task<()>>,
    subscriptions: Vec<Subscription>,
}
impl FilesUi {
    /// 功能：关闭保护覆盖目录读取和传输队列；时间：2026-11-11；作者：lq。
    pub(super) fn pending(&self) -> bool {
        self.active.is_some()
            || !self.queue.is_empty()
            || !self.local.loading.is_empty()
            || !self.remote.loading.is_empty()
    }
    /// 功能：借用所需一侧的状态；时间：2026-11-11；作者：lq。
    fn browser(&self, side: FileSide) -> &FileBrowser {
        if side == FileSide::Local {
            &self.local
        } else {
            &self.remote
        }
    }
    /// 功能：只修改目标侧的树，不重建对侧；时间：2026-11-11；作者：lq。
    fn browser_mut(&mut self, side: FileSide) -> &mut FileBrowser {
        if side == FileSide::Local {
            &mut self.local
        } else {
            &mut self.remote
        }
    }
}

impl ServerWorkspace {
    /// 功能：安装两侧目录树的原生展开和收起订阅；时间：2026-11-11；作者：lq。
    pub(super) fn new_files_ui(window: &mut Window, cx: &mut Context<Self>) -> FilesUi {
        let local = cx.new(|cx| TreeState::new(cx));
        let remote = cx.new(|cx| TreeState::new(cx));
        let mut subscriptions = Vec::new();
        for (side, state) in [(FileSide::Local, &local), (FileSide::Remote, &remote)] {
            subscriptions.push(cx.subscribe_in(
                state,
                window,
                move |this, _, event: &TreeEvent, window, cx| {
                    match event {
                        TreeEvent::Expanded(id) => {
                            let path = PathBuf::from(id.as_str());
                            let browser = this.services.files.browser_mut(side);
                            browser.expanded.insert(path.clone());
                            if !browser.entries.contains_key(&path) {
                                this.load_directory(side, path, window, cx);
                            }
                        }
                        TreeEvent::Collapsed(id) => {
                            this.services
                                .files
                                .browser_mut(side)
                                .expanded
                                .remove(Path::new(id.as_str()));
                        }
                    }
                    cx.notify();
                },
            ));
        }
        // 功能：树根初始由现有地址栏提供，首次读取成功再转成真实绝对路径；时间：2026-11-11；作者：lq。
        let browser = |tree| FileBrowser {
            root: PathBuf::new(),
            current: PathBuf::new(),
            selection: BTreeSet::new(),
            anchor: None,
            focus: cx.focus_handle(),
            scroll: UniformListScrollHandle::new(),
            horizontal: ScrollHandle::new(),
            show_directories: true,
            tree,
            selected: None,
            entries: BTreeMap::new(),
            expanded: BTreeSet::new(),
            loading: BTreeSet::new(),
            errors: BTreeMap::new(),
            generation: 0,
        };
        FilesUi {
            local: browser(local),
            remote: browser(remote),
            queue: VecDeque::new(),
            active: None,
            history: VecDeque::new(),
            progress_task: None,
            subscriptions,
        }
    }

    /// 功能：按需重建可见树并恢复选中路径，保留键盘导航与滚动状态；时间：2026-11-11；作者：lq。
    fn rebuild_tree(&mut self, side: FileSide, cx: &mut Context<Self>) {
        self.services
            .files
            .browser_mut(side)
            .prune_selection(self.services.show_hidden);
        let browser = self.services.files.browser(side);
        let items = if browser.root.as_os_str().is_empty() {
            vec![]
        } else {
            vec![Self::tree_node(
                &browser.root,
                browser,
                self.services.show_hidden,
                0,
            )]
        };
        let selected = Some(SharedString::from(browser.current.display().to_string()));
        let tree = browser.tree.clone();
        tree.update(cx, |state, cx| {
            state.set_items(items, cx);
            let ix = selected.as_ref().and_then(|id| state.index_of(id));
            state.set_selected_index(ix, cx);
        });
    }

    /// 功能：有界构造层级，未加载、空目录和读取失败使用禁用提示节点；时间：2026-11-11；作者：lq。
    fn tree_node(path: &Path, browser: &FileBrowser, hidden: bool, depth: usize) -> TreeItem {
        let id = path.display().to_string();
        let label = path
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .unwrap_or("/")
            .to_string();
        let node = TreeItem::new(id.clone(), label).expanded(browser.expanded.contains(path));
        let children: Vec<_> = browser
            .entries
            .get(path)
            .into_iter()
            .flatten()
            .filter(|entry| {
                entry.directory && !entry.symlink && (hidden || !entry.name.starts_with('.'))
            })
            .map(|entry| {
                if entry.directory && !entry.symlink && depth < 64 {
                    Self::tree_node(&entry.path, browser, hidden, depth + 1)
                } else {
                    TreeItem::new(entry.path.display().to_string(), entry.name.clone())
                }
            })
            .collect();
        if !children.is_empty() {
            node.children(children)
        } else {
            let hint = if browser.entries.contains_key(path)
                && !browser.loading.contains(path)
                && !browser.errors.contains_key(path)
            {
                "无子文件夹".into()
            } else {
                browser.empty_hint(path, hidden)
            };
            node.child(TreeItem::new(format!("qds6-placeholder:{id}"), hint).disabled(true))
        }
    }

    /// 功能：后台读取本地目录，不跟随符号链接并限制单目录数量；时间：2026-11-11；作者：lq。
    fn read_local(path: &Path) -> Result<Vec<sftp::Entry>, String> {
        let mut entries = Vec::new();
        for entry in std::fs::read_dir(path)
            .map_err(|_| "无法打开本地目录")?
            .take(20001)
        {
            let entry = entry.map_err(|_| "读取本地目录失败")?;
            let metadata = entry
                .path()
                .symlink_metadata()
                .map_err(|_| "无法读取本地属性")?;
            let name = entry
                .file_name()
                .to_str()
                .ok_or("本地目录含非 UTF-8 文件名")?
                .to_string();
            entries.push(sftp::Entry {
                path: entry.path(),
                name,
                size: metadata.len(),
                permissions: local_permissions(&metadata),
                modified: metadata
                    .modified()
                    .ok()
                    .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|time| time.as_secs()),
                uid: local_owner(&metadata).0,
                gid: local_owner(&metadata).1,
                directory: metadata.is_dir(),
                symlink: metadata.is_symlink(),
            });
        }
        if entries.len() > 20000 {
            return Err("本地目录超过 20000 项".into());
        }
        entries.sort_by(|a, b| b.directory.cmp(&a.directory).then(a.name.cmp(&b.name)));
        Ok(entries)
    }

    /// 功能：目录任务与文件操作分槽，代次校验避免旧导航结果覆盖新路径；时间：2026-11-11；作者：lq。
    fn load_directory(
        &mut self,
        side: FileSide,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let client = self.services.sftp.clone();
        if side == FileSide::Remote && client.is_none() {
            return;
        }
        let browser = self.services.files.browser_mut(side);
        if !browser.loading.insert(path.clone()) {
            return;
        }
        let generation = browser.generation;
        let is_root = path == browser.current;
        let requested = path.clone();
        self.rebuild_tree(side, cx);
        cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    if side == FileSide::Local {
                        let canonical = if is_root {
                            std::fs::canonicalize(&path).map_err(|_| "本地目录不可用")?
                        } else {
                            path
                        };
                        Self::read_local(&canonical).map(|entries| (canonical, entries))
                    } else {
                        let client = client
                            .as_ref()
                            .ok_or("SFTP 未连接")?
                            .lock()
                            .map_err(|_| "SFTP 状态异常")?;
                        let canonical = if is_root {
                            client.canonical_directory(&path)?
                        } else {
                            path
                        };
                        client.list(&canonical).map(|entries| (canonical, entries))
                    }
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                let browser = this.services.files.browser_mut(side);
                if generation != browser.generation {
                    return;
                }
                browser.loading.remove(&requested);
                let is_current = is_root && browser.current == requested;
                let mut recovered_error = None;
                match result {
                    Ok((path, entries)) => {
                        if browser.entries.values().map(Vec::len).sum::<usize>()
                            - browser.entries.get(&path).map_or(0, Vec::len)
                            + entries.len()
                            > 100000
                        {
                            browser.errors.insert(
                                requested.clone(),
                                "展开的目录超过 100000 项，请切换树根".into(),
                            );
                        } else {
                            if is_current && browser.current != path {
                                if browser.root == requested {
                                    browser.root = path.clone();
                                }
                                browser.current = path.clone();
                                browser.expanded.insert(path.clone());
                            }
                            recovered_error = browser.errors.remove(&requested);
                            browser.entries.insert(path.clone(), entries);
                            browser.prune_selection(this.services.show_hidden);
                            if is_current {
                                let input = if side == FileSide::Local {
                                    &this.services.local_directory
                                } else {
                                    &this.services.remote_path
                                };
                                input.update(cx, |input, cx| {
                                    input.set_value(path.display().to_string(), window, cx)
                                });
                            }
                        }
                    }
                    Err(error) => {
                        browser.errors.insert(requested, error.clone());
                        this.status = error.into();
                        this.error = true;
                    }
                }
                // 功能：同一路径重试成功后清理对应旧错误，不覆盖其他文件操作的反馈；时间：2026-11-11；作者：lq。
                if recovered_error.as_deref() == Some(this.status.as_ref()) && this.error {
                    this.status = "服务器已连接 · 终端与文件互不阻塞".into();
                    this.error = false;
                }
                this.rebuild_tree(side, cx);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// 功能：地址变化时切换树根，刷新时保留已展开目录；时间：2026-11-11；作者：lq。
    fn refresh_browser(&mut self, side: FileSide, window: &mut Window, cx: &mut Context<Self>) {
        let input = if side == FileSide::Local {
            &self.services.local_directory
        } else {
            &self.services.remote_path
        };
        let path = PathBuf::from(input.read(cx).value().as_str());
        if path.as_os_str().is_empty() {
            return;
        }
        if self.services.files.browser(side).current != path {
            self.navigate_directory(side, path, window, cx);
        } else {
            self.load_directory(side, path, window, cx);
        }
    }
    /// 功能：兼容编辑器和现有调用的远程刷新入口；时间：2026-11-11；作者：lq。
    pub(crate) fn refresh_remote(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.refresh_browser(FileSide::Remote, window, cx);
    }
    /// 功能：本地刷新不占用远程文件任务槽；时间：2026-11-11；作者：lq。
    pub(crate) fn refresh_local(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.refresh_browser(FileSide::Local, window, cx);
    }

    /// 功能：箭头和双击切换目录，单击只选择，保持目标选择稳定；时间：2026-11-11；作者：lq。
    fn toggle_directory(
        &mut self,
        side: FileSide,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let browser = self.services.files.browser_mut(side);
        if !browser.expanded.remove(&path) {
            browser.expanded.insert(path.clone());
            if !browser.entries.contains_key(&path) {
                self.load_directory(side, path, window, cx);
                return;
            }
        }
        self.rebuild_tree(side, cx);
        cx.notify();
    }

    /// 功能：当前层级选中项目批量传向对侧当前目录；时间：2026-11-11；作者：lq。
    fn transfer_selected(&mut self, side: FileSide, window: &mut Window, cx: &mut Context<Self>) {
        let entries = self
            .services
            .files
            .browser(side)
            .selected_entries(self.services.show_hidden);
        let target_side = if side == FileSide::Local {
            FileSide::Remote
        } else {
            FileSide::Local
        };
        let destination = self.services.files.browser(target_side).destination();
        self.enqueue_batch(side, entries, destination, window, cx);
    }

    /// 功能：排队时冻结路径，不覆盖同名目标，拒绝符号链接及重复任务；时间：2026-11-11；作者：lq。
    fn enqueue_transfer(
        &mut self,
        side: FileSide,
        entry: sftp::Entry,
        directory: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if entry.symlink || entry.path.file_name().is_none() {
            self.status = "符号链接不参与传输，请选择普通文件或文件夹".into();
            cx.notify();
            return;
        }
        let target = directory.join(entry.path.file_name().unwrap());
        let request = TransferRequest {
            side,
            source: entry.path,
            target,
        };
        let files = &mut self.services.files;
        if files.queue.len() >= 20000 {
            self.status = "传输队列已满（20000 项）".into();
        } else if files.active.iter().chain(files.queue.iter()).any(|item| {
            item.side == side && item.source == request.source && item.target == request.target
        }) {
            self.status = "该传输已在队列中".into();
        } else {
            files.queue.push_back(request);
            self.start_next_transfer(window, cx);
        }
        cx.notify();
    }

    /// 功能：串行执行传输，100ms 刷新真实字节进度；完成自动刷新目标树；时间：2026-11-11；作者：lq。
    pub(super) fn start_next_transfer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.services.busy || self.services.files.active.is_some() {
            return;
        }
        let Some(request) = self.services.files.queue.pop_front() else {
            return;
        };
        let Some(client) = self.services.sftp.clone() else {
            return;
        };
        self.services.busy = true;
        self.services.transfer = sftp::Transfer::default();
        let progress = self.services.transfer.clone();
        self.status = format!("{} → {}", request.label(), request.target.display()).into();
        self.error = false;
        self.services.files.active = Some(request.clone());
        self.services.files.progress_task = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
                let running = this
                    .update(cx, |this, cx| {
                        cx.notify();
                        this.services.files.active.is_some()
                    })
                    .unwrap_or(false);
                if !running {
                    break;
                }
            }
        }));
        self.services.task = Some(cx.spawn_in(window, async move |this, cx| {
            let operation = request.clone();
            let result = cx
                .background_spawn(async move {
                    let client = client.lock().map_err(|_| "SFTP 状态异常")?;
                    if operation.side == FileSide::Local {
                        client.upload_path(&operation.source, &operation.target, &progress)
                    } else {
                        client.download_path(&operation.source, &operation.target, &progress)
                    }
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.services.busy = false;
                this.services.files.active = None;
                this.services.files.progress_task = None;
                let message = match result {
                    Ok(()) => format!(
                        "已完成 · {} → {}",
                        request.label(),
                        request.target.display()
                    ),
                    Err(error) => {
                        this.error = true;
                        format!("{}失败 · {error}", request.label())
                    }
                };
                this.status = message.clone().into();
                this.services.files.history.push_front(message);
                this.services.files.history.truncate(20);
                let side = if request.side == FileSide::Local {
                    FileSide::Remote
                } else {
                    FileSide::Local
                };
                if let Some(parent) = request.target.parent() {
                    this.load_directory(side, parent.into(), window, cx);
                }
                this.start_next_transfer(window, cx);
                cx.notify();
            });
        }));
    }

    /// 功能：本地与远程各自提供目录导航、详情列表和批量传输工具栏；时间：2026-11-11；作者：lq。
    fn browser_panel(&self, side: FileSide, cx: &mut Context<Self>) -> AnyElement {
        let browser = self.services.files.browser(side);
        let owner = cx.entity().entity_id();
        let input = if side == FileSide::Local {
            &self.services.local_directory
        } else {
            &self.services.remote_path
        };
        let root = browser.current.clone();
        // 功能：底部展示真实总数和隐藏项，明确目录已成功读取；时间：2026-11-11；作者：lq。
        let hidden_count = browser.hidden_count(&root);
        let hidden_only = !self.services.show_hidden
            && hidden_count > 0
            && browser
                .entries
                .get(&root)
                .is_some_and(|entries| entries.len() == hidden_count);
        let selected = format!(
            "{} 项 · 已选 {} 项 · 双击进入 / ⌘或Ctrl多选 / Shift连选",
            browser.visible_entries(self.services.show_hidden).len(),
            browser.selection.len()
        );
        let target = self
            .services
            .files
            .browser(if side == FileSide::Local {
                FileSide::Remote
            } else {
                FileSide::Local
            })
            .destination();
        let directories = self.directory_tree(side, cx);
        let files = self.directory_list(side, cx);
        let drop_view = cx.entity();
        let directory = browser.destination();
        let error = browser.errors.get(&browser.current).cloned();
        v_flex()
            .id(if side == FileSide::Local {
                "server-local-pane"
            } else {
                "server-remote-pane"
            })
            .test_support()
            .size_full()
            .min_w_0()
            .min_h_0()
            .overflow_hidden()
            .child(
                h_flex()
                    .px_2()
                    .h(px(32.))
                    .flex_shrink_0()
                    .gap_2()
                    .child(
                        Button::new(SharedString::from(format!(
                            "toggle-directories-{}",
                            side.id()
                        )))
                        .ghost()
                        .xsmall()
                        .icon(UiIcon::Folder)
                        .selected(browser.show_directories)
                        .tooltip("显示 / 收起目录树")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let browser = this.services.files.browser_mut(side);
                            browser.show_directories = !browser.show_directories;
                            cx.notify();
                        })),
                    )
                    .child(
                        div()
                            .text_xs()
                            .font_semibold()
                            .child(if side == FileSide::Local {
                                "本地文件"
                            } else {
                                "远程文件"
                            }),
                    )
                    .when(side == FileSide::Local, |v| {
                        v.child(
                            Button::new("choose-local-directory")
                                .ghost()
                                .xsmall()
                                .label("选择目录")
                                .tooltip("选择本地文件树的起始文件夹")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.choose_local_directory(window, cx)
                                })),
                        )
                    })
                    // 功能：仅含隐藏项时一键展示，无需猜测连接是否失败；时间：2026-11-11；作者：lq。
                    .when(hidden_only, |row| {
                        row.child(
                            Button::new(SharedString::from(format!("show-hidden-{}", side.id())))
                                .ghost()
                                .xsmall()
                                .label("显示隐藏文件")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.services.show_hidden = true;
                                    this.rebuild_tree(FileSide::Local, cx);
                                    this.rebuild_tree(FileSide::Remote, cx);
                                    cx.notify();
                                })),
                        )
                    })
                    .child(div().flex_1())
                    .child(
                        Button::new(SharedString::from(format!("select-all-{}", side.id())))
                            .ghost()
                            .xsmall()
                            .label("全选")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.select_all_files(side, window, cx);
                            })),
                    )
                    .child(
                        Button::new(if side == FileSide::Local {
                            "upload"
                        } else {
                            "download"
                        })
                        .xsmall()
                        .ghost()
                        .icon(if side == FileSide::Local {
                            UiIcon::Upload
                        } else {
                            UiIcon::Download
                        })
                        .label(if side == FileSide::Local {
                            "上传 →"
                        } else {
                            "← 下载"
                        })
                        .tooltip(format!("传入 {}", target.display()))
                        .disabled(browser.selection.is_empty())
                        .on_click(cx.listener(
                            move |this, _, window, cx| this.transfer_selected(side, window, cx),
                        )),
                    ),
            )
            .child(
                h_flex()
                    .px_2()
                    .pb_1()
                    .gap_1()
                    .flex_shrink_0()
                    .child(
                        Button::new(SharedString::from(format!(
                            "parent-directory-{}",
                            side.id()
                        )))
                        .xsmall()
                        .ghost()
                        .label("上级")
                        .disabled(browser.current.parent().is_none())
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                let parent = this
                                    .services
                                    .files
                                    .browser(side)
                                    .current
                                    .parent()
                                    .map(Path::to_path_buf);
                                if let Some(parent) = parent {
                                    let input = if side == FileSide::Local {
                                        &this.services.local_directory
                                    } else {
                                        &this.services.remote_path
                                    };
                                    input.update(cx, |input, cx| {
                                        input.set_value(parent.display().to_string(), window, cx)
                                    });
                                    this.refresh_browser(side, window, cx);
                                }
                            },
                        )),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(Input::new(input).small().aria_label(
                                if side == FileSide::Local {
                                    "本地目录"
                                } else {
                                    "远程目录"
                                },
                            )),
                    )
                    .child(
                        Button::new(if side == FileSide::Local {
                            "local-refresh"
                        } else {
                            "remote-refresh"
                        })
                        .ghost()
                        .xsmall()
                        .icon(UiIcon::RefreshCw)
                        .tooltip("刷新目录 / 跳转路径")
                        .on_click(cx.listener(
                            move |this, _, window, cx| this.refresh_browser(side, window, cx),
                        )),
                    ),
            )
            .child(
                div()
                    .id(if side == FileSide::Local {
                        "local-files-drop"
                    } else {
                        "remote-files-drop"
                    })
                    .test_support()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .can_drop(move |data, _, _| {
                        data.downcast_ref::<FileDrag>()
                            .is_some_and(|drag| drag.owner == owner && drag.side != side)
                    })
                    .drag_over::<FileDrag>(|style, _, _, cx| style.bg(cx.theme().accent))
                    .on_drop(move |drag: &FileDrag, window, cx| {
                        cx.stop_propagation();
                        drop_view.update(cx, |this, cx| {
                            this.enqueue_batch(
                                drag.side,
                                drag.entries.borrow().clone(),
                                directory.clone(),
                                window,
                                cx,
                            );
                        });
                    })
                    .child(if browser.show_directories {
                        h_resizable(SharedString::from(format!("file-navigation-{}", side.id())))
                            .child(
                                resizable_panel()
                                    .size(px(145.))
                                    .size_range(px(85.)..px(320.))
                                    .child(
                                        div()
                                            .size_full()
                                            .min_h_0()
                                            .overflow_hidden()
                                            .border_r_1()
                                            .border_color(cx.theme().border)
                                            .child(directories),
                                    ),
                            )
                            .child(
                                resizable_panel()
                                    .size_range(px(140.)..Pixels::MAX)
                                    .child(files),
                            )
                            .into_any_element()
                    } else {
                        files
                    }),
            )
            .child(
                div()
                    .h(px(23.))
                    .px_2()
                    .flex_shrink_0()
                    .text_xs()
                    .text_color(if error.is_some() {
                        cx.theme().danger
                    } else {
                        cx.theme().muted_foreground
                    })
                    .text_ellipsis()
                    .child(error.unwrap_or(selected)),
            )
            .into_any_element()
    }

    /// 功能：双侧文件管理器共享紧凑工具栏与底部传输状态；时间：2026-11-11；作者：lq。
    pub(super) fn files_page(&self, cx: &mut Context<Self>) -> AnyElement {
        if self.services.sftp.is_none() {
            return div()
                .p_4()
                .text_sm()
                .child("SFTP 不可用；终端仍可继续使用。")
                .into_any_element();
        }
        let files = &self.services.files;
        let completed = self.services.transfer.completed.load(Ordering::Relaxed);
        let total = self.services.transfer.total.load(Ordering::Relaxed);
        let status = if let Some(active) = &files.active {
            format!(
                "{} · {} / {} · 等待 {}",
                active.label(),
                format_bytes(completed),
                format_bytes(total),
                files.queue.len()
            )
        } else if self.status.as_ref() == "服务器已连接 · 终端与文件互不阻塞" {
            "拖到对侧文件夹或空白处传输 · 同名目标会提示冲突".into()
        } else {
            self.status.to_string()
        };
        v_flex()
            .size_full()
            .min_h_0()
            .child(
                h_flex()
                    .px_2()
                    .h(px(30.))
                    .flex_shrink_0()
                    .gap_3()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(div().text_xs().font_semibold().child("SFTP 文件"))
                    .child(div().flex_1())
                    .child(
                        Checkbox::new("show-hidden")
                            .label("显示隐藏文件")
                            .checked(self.services.show_hidden)
                            .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                this.services.show_hidden = *checked;
                                this.rebuild_tree(FileSide::Local, cx);
                                this.rebuild_tree(FileSide::Remote, cx);
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("toggle-file-tools")
                            .ghost()
                            .xsmall()
                            .icon(UiIcon::Ellipsis)
                            .tooltip("远程文件操作")
                            .selected(self.services.show_file_tools)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.services.show_file_tools = !this.services.show_file_tools;
                                cx.notify();
                            })),
                    ),
            )
            .when(self.services.show_file_tools, |v| {
                v.child(
                    h_flex()
                        .px_2()
                        .py_1()
                        .gap_1()
                        .flex_wrap()
                        .child(
                            div().w(px(180.)).child(
                                Input::new(&self.services.target)
                                    .small()
                                    .aria_label("远程目标名称或权限"),
                            ),
                        )
                        .children(
                            [
                                ("mkdir", "新建目录"),
                                ("rename", "重命名"),
                                ("chmod", "权限"),
                                ("remove", "删除…"),
                            ]
                            .into_iter()
                            .map(|(action, label)| {
                                Button::new(action)
                                    .small()
                                    .ghost()
                                    .label(label)
                                    .disabled(
                                        self.services.busy
                                            || (action != "mkdir"
                                                && self.services.files.remote.selection.len() != 1),
                                    )
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.file_operation(action, window, cx)
                                    }))
                            }),
                        ),
                )
            })
            .child(
                div().flex_1().min_h_0().child(
                    h_resizable("server-file-split")
                        .child(
                            resizable_panel()
                                .size_range(px(180.)..Pixels::MAX)
                                .child(self.browser_panel(FileSide::Local, cx)),
                        )
                        .child(
                            resizable_panel()
                                .size_range(px(180.)..Pixels::MAX)
                                .child(self.browser_panel(FileSide::Remote, cx)),
                        ),
                ),
            )
            .child(
                h_flex()
                    .id("transfer-status")
                    .test_support()
                    .h(px(29.))
                    .flex_shrink_0()
                    .px_2()
                    .gap_2()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(
                        Icon::new(if files.active.is_some() {
                            UiIcon::RefreshCw
                        } else {
                            UiIcon::Columns2
                        })
                        .xsmall(),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_xs()
                            .text_ellipsis()
                            .text_color(if self.error {
                                cx.theme().danger
                            } else {
                                cx.theme().muted_foreground
                            })
                            .child(status),
                    )
                    // 功能：队列和最近结果收进菜单，避免挤压文件树；时间：2026-11-11；作者：lq。
                    .child(
                        Button::new("transfer-history")
                            .ghost()
                            .xsmall()
                            .label("传输记录")
                            .dropdown_menu({
                                let queue: Vec<_> = files
                                    .queue
                                    .iter()
                                    .map(|item| {
                                        format!(
                                            "等待 · {} → {}",
                                            item.label(),
                                            item.target.display()
                                        )
                                    })
                                    .collect();
                                let history: Vec<_> = files.history.iter().cloned().collect();
                                move |mut menu, _, _| {
                                    if queue.is_empty() && history.is_empty() {
                                        menu = menu.item(PopupMenuItem::label("暂无传输记录"));
                                    }
                                    for item in queue.iter().chain(history.iter()) {
                                        menu = menu.item(PopupMenuItem::label(item.clone()));
                                    }
                                    menu
                                }
                            }),
                    )
                    .when(files.active.is_some(), |v| {
                        v.child(
                            Button::new("cancel-transfer")
                                .ghost()
                                .xsmall()
                                .label("取消当前")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.services.transfer.cancel.store(true, Ordering::Relaxed);
                                    cx.notify();
                                })),
                        )
                    })
                    .when(!files.queue.is_empty(), |v| {
                        v.child(
                            Button::new("clear-transfer-queue")
                                .ghost()
                                .xsmall()
                                .label("清空等待")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.services.files.queue.clear();
                                    cx.notify();
                                })),
                        )
                    }),
            )
            .into_any_element()
    }

    /// 功能：验收定位当前目录真实文件行后发送原生鼠标事件；时间：2026-11-11；作者：lq。
    #[cfg(feature = "ui-tests")]
    pub(crate) fn file_row(&self, remote: bool, path: &Path, cx: &App) -> (&'static str, usize) {
        let side = if remote {
            FileSide::Remote
        } else {
            FileSide::Local
        };
        let _ = cx;
        let browser = self.services.files.browser(side);
        let index = browser
            .visible_entries(self.services.show_hidden)
            .iter()
            .position(|entry| entry.path == path)
            .expect("file path must be in the current directory");
        browser.scroll.scroll_to_item(index, ScrollStrategy::Top);
        (
            if remote {
                "remote-entry"
            } else {
                "local-entry"
            },
            index,
        )
    }

    /// 功能：地址栏回车与刷新采用同一跳转逻辑；时间：2026-11-11；作者：lq。
    pub(super) fn subscribe_file_paths(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for (side, input) in [
            (FileSide::Local, &self.services.local_directory),
            (FileSide::Remote, &self.services.remote_path),
        ] {
            self.services.files.subscriptions.push(cx.subscribe_in(
                input,
                window,
                move |this, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::PressEnter { .. }) {
                        this.refresh_browser(side, window, cx);
                    }
                },
            ));
        }
    }

    /// 功能：使用系统文件夹选择器切换本地树根，取消时保留当前目录；时间：2026-11-11；作者：lq。
    fn choose_local_directory(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("选择本地文件夹".into()),
        });
        cx.spawn_in(window, async move |this, cx| match paths.await {
            Ok(Ok(Some(paths))) => {
                if let Some(path) = paths.first() {
                    let _ = this.update_in(cx, |this, window, cx| {
                        this.services.local_directory.update(cx, |input, cx| {
                            input.set_value(path.display().to_string(), window, cx)
                        });
                        this.refresh_local(window, cx);
                    });
                }
            }
            Ok(Ok(None)) => {}
            _ => {
                let _ = this.update(cx, |this, cx| {
                    this.status = "无法打开本地文件夹选择器，请在地址栏输入路径".into();
                    this.error = true;
                    cx.notify();
                });
            }
        })
        .detach();
    }
}

/// 功能：文件大小按二进制单位显示，避免长字节数挤占文件名；时间：2026-11-11；作者：lq。
fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KiB", bytes as f64 / 1024.)
    } else {
        format!("{:.1} MiB", bytes as f64 / (1024. * 1024.))
    }
}
