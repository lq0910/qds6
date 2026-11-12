//! 功能：隐藏本地时提供远程目录树，按需后台读取并与当前目录同步；时间：2026-11-12；作者：lq。

use super::*;
use gpui_kit::component::{
    list::ListItem,
    tree::{TreeEvent, TreeItem, TreeState, tree},
};

/// 功能：目录树独立缓存，列表切换目录不会丢失已展开层级；时间：2026-11-12；作者：lq。
pub(super) struct RemoteDirectoryTree {
    pub(super) state: Entity<TreeState>,
    directories: BTreeMap<PathBuf, Vec<PathBuf>>,
    expanded: BTreeSet<PathBuf>,
    pub(super) loading: BTreeSet<PathBuf>,
    errors: BTreeMap<PathBuf, String>,
}

impl RemoteDirectoryTree {
    /// 功能：只创建原生树状态，显示远程模式前不产生额外网络读取；时间：2026-11-12；作者：lq。
    pub(super) fn new(cx: &mut Context<ServerWorkspace>) -> Self {
        Self {
            state: cx.new(|cx| TreeState::new(cx)),
            directories: BTreeMap::new(),
            expanded: BTreeSet::new(),
            loading: BTreeSet::new(),
            errors: BTreeMap::new(),
        }
    }

    /// 功能：目录缓存仅保存普通文件夹，限制总量且不跟随符号链接；时间：2026-11-12；作者：lq。
    pub(super) fn cache(&mut self, path: PathBuf, entries: &[sftp::Entry]) {
        let directories: Vec<_> = entries
            .iter()
            .filter(|entry| entry.directory && !entry.symlink)
            .map(|entry| entry.path.clone())
            .collect();
        let count = self.directories.values().map(Vec::len).sum::<usize>()
            - self.directories.get(&path).map_or(0, Vec::len)
            + directories.len();
        if count <= 100000 {
            self.errors.remove(&path);
            self.directories.insert(path, directories);
        } else {
            self.errors.insert(path, "目录树缓存已满".into());
        }
    }

    /// 功能：从远程根目录构造文件夹层级，未读取或失败节点保留明确提示；时间：2026-11-12；作者：lq。
    fn node(&self, path: &Path, current: &Path, hidden: bool, depth: usize) -> TreeItem {
        let label = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("/");
        let node = TreeItem::new(path.display().to_string(), label.to_string())
            .expanded(self.expanded.contains(path));
        if depth >= 64 {
            return node;
        }
        let mut children: BTreeSet<_> = self
            .directories
            .get(path)
            .into_iter()
            .flatten()
            .filter(|child| {
                hidden
                    || !child
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .starts_with('.')
            })
            .cloned()
            .collect();
        // 功能：祖先尚未读取或无权列出时，也显示当前已知路径，保持定位上下文；时间：2026-11-12；作者：lq。
        if let Some(child) = current
            .ancestors()
            .find(|ancestor| ancestor.parent() == Some(path))
        {
            children.insert(child.to_path_buf());
        }
        if !children.is_empty() {
            node.children(
                children
                    .iter()
                    .map(|child| self.node(child, current, hidden, depth + 1))
                    .collect::<Vec<_>>(),
            )
        } else if let Some(error) = self.errors.get(path) {
            node.child(
                TreeItem::new(
                    format!("qds6-tree-hint:{}", path.display()),
                    format!("{error} · 收起再展开重试"),
                )
                .disabled(true),
            )
        } else if !self.directories.contains_key(path) || self.loading.contains(path) {
            node.child(
                TreeItem::new(
                    format!("qds6-tree-hint:{}", path.display()),
                    if self.loading.contains(path) {
                        "正在读取…"
                    } else {
                        "展开以读取"
                    },
                )
                .disabled(true),
            )
        } else {
            node
        }
    }
}

impl ServerWorkspace {
    /// 功能：接收原生键盘展开事件，鼠标箭头与键盘使用同一后台加载入口；时间：2026-11-12；作者：lq。
    pub(super) fn subscribe_remote_tree(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let state = self.services.files.remote_tree.state.clone();
        let subscription = cx.subscribe_in(
            &state,
            window,
            |this, _, event: &TreeEvent, window, cx| match event {
                TreeEvent::Expanded(id) => {
                    this.set_remote_tree_expanded(PathBuf::from(id.as_str()), true, window, cx)
                }
                TreeEvent::Collapsed(id) => {
                    this.set_remote_tree_expanded(PathBuf::from(id.as_str()), false, window, cx)
                }
            },
        );
        self.services.files.subscriptions.push(subscription);
    }

    /// 功能：重建树时恢复当前目录高亮，可选择揭示当前目录且不跳动列表滚动；时间：2026-11-12；作者：lq。
    pub(super) fn rebuild_remote_tree(&mut self, reveal: bool, cx: &mut Context<Self>) {
        let files = &self.services.files;
        let current = files.remote.current.clone();
        let item = files
            .remote_tree
            .node(Path::new("/"), &current, self.services.show_hidden, 0);
        let state = files.remote_tree.state.clone();
        state.update(cx, |state, cx| {
            state.set_items(vec![item], cx);
            let selected = state.index_of(&SharedString::from(current.display().to_string()));
            state.set_selected_index(selected, cx);
            if reveal && let Some(index) = selected {
                state.scroll_to_item(index, ScrollStrategy::Center);
            }
        });
    }

    /// 功能：切换远程模式或导航后展开当前目录祖先，只加载路径层级而不扫描整棵树；时间：2026-11-12；作者：lq。
    pub(super) fn sync_remote_tree(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.services.files.show_local {
            return;
        }
        let current = self.services.files.remote.current.clone();
        if !current.is_absolute() {
            return;
        }
        let ancestors: Vec<_> = current.ancestors().map(Path::to_path_buf).collect();
        for path in ancestors.into_iter().rev().take(65) {
            self.services
                .files
                .remote_tree
                .expanded
                .insert(path.clone());
            if !self
                .services
                .files
                .remote_tree
                .directories
                .contains_key(&path)
                && !self.services.files.remote_tree.errors.contains_key(&path)
            {
                self.load_remote_tree_directory(path, window, cx);
            }
        }
        self.rebuild_remote_tree(true, cx);
    }

    /// 功能：展开只改变树结构，目录行单击才导航列表；失败可收起再展开重试；时间：2026-11-12；作者：lq。
    fn set_remote_tree_expanded(
        &mut self,
        path: PathBuf,
        expanded: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let tree = &mut self.services.files.remote_tree;
        if expanded {
            tree.expanded.insert(path.clone());
            if !tree.directories.contains_key(&path) || tree.errors.contains_key(&path) {
                self.load_remote_tree_directory(path, window, cx);
            }
        } else {
            tree.expanded.remove(&path);
        }
        self.rebuild_remote_tree(false, cx);
        cx.notify();
    }

    /// 功能：目录树网络工作放后台，失败仅标注对应节点，不覆盖右侧列表或传输反馈；时间：2026-11-12；作者：lq。
    fn load_remote_tree_directory(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(client) = self.services.sftp.clone() else {
            return;
        };
        if !self.services.files.remote_tree.loading.insert(path.clone()) {
            return;
        }
        cx.spawn_in(window, async move |this, cx| {
            let requested = path.clone();
            let result = cx
                .background_spawn(async move {
                    client
                        .lock()
                        .map_err(|_| "SFTP 状态异常".to_string())?
                        .list(&path)
                })
                .await;
            let _ = this.update_in(cx, |this, _, cx| {
                let reveal = this.services.files.remote.current.starts_with(&requested);
                let tree = &mut this.services.files.remote_tree;
                tree.loading.remove(&requested);
                match result {
                    Ok(entries) => tree.cache(requested, &entries),
                    Err(error) => {
                        tree.errors.insert(requested, error);
                    }
                }
                this.rebuild_remote_tree(reveal, cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// 功能：远程模式左侧仅显示文件夹，当前路径高亮，箭头独立展开；时间：2026-11-12；作者：lq。
    pub(super) fn remote_directory_tree(&self, cx: &mut Context<Self>) -> AnyElement {
        let state = self.services.files.remote_tree.state.clone();
        let view = cx.entity();
        div()
            .id("remote-directory-tree")
            .test_support()
            .size_full()
            .min_h_0()
            .overflow_hidden()
            .child(tree(&state, move |index, node, _, _, _| {
                let path = PathBuf::from(node.item().id.as_str());
                let valid = !node.item().id.starts_with("qds6-tree-hint:");
                let expanded = node.is_expanded();
                let has_children = !node.item().children.is_empty();
                let toggle_view = view.clone();
                let toggle_path = path.clone();
                let click_view = view.clone();
                ListItem::new(("remote-directory", index))
                    .h(px(28.))
                    .py_0()
                    .pl(px(4. + node.depth() as f32 * 12.))
                    .pr_1()
                    .child(
                        h_flex()
                            .gap_1()
                            .min_w_0()
                            .w_full()
                            .child(
                                div()
                                    .id(("remote-directory-toggle", index))
                                    .test_support()
                                    .w(px(18.))
                                    .flex_shrink_0()
                                    .when(valid && has_children, |v| {
                                        v.child(
                                            Icon::new(if expanded {
                                                UiIcon::ChevronDown
                                            } else {
                                                UiIcon::ChevronRight
                                            })
                                            .xsmall(),
                                        )
                                        .on_click(
                                            move |_, window, cx| {
                                                cx.stop_propagation();
                                                toggle_view.update(cx, |this, cx| {
                                                    this.set_remote_tree_expanded(
                                                        toggle_path.clone(),
                                                        !expanded,
                                                        window,
                                                        cx,
                                                    )
                                                });
                                            },
                                        )
                                    }),
                            )
                            .when(valid, |v| {
                                v.child(
                                    Icon::new(if expanded {
                                        UiIcon::FolderOpen
                                    } else {
                                        UiIcon::Folder
                                    })
                                    .small(),
                                )
                            })
                            .child(
                                div()
                                    .min_w_0()
                                    .text_xs()
                                    .text_ellipsis()
                                    .child(node.item().label.clone()),
                            ),
                    )
                    .when(valid, |v| {
                        v.on_click(move |_, window, cx| {
                            cx.stop_propagation();
                            click_view.update(cx, |this, cx| {
                                if this.services.files.remote.current != path {
                                    this.navigate_directory(
                                        FileSide::Remote,
                                        path.clone(),
                                        window,
                                        cx,
                                    );
                                }
                            });
                        })
                    })
            }))
            .into_any_element()
    }
}
