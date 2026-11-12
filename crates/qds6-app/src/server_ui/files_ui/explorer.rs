//! 功能：双列表目录导航、详情表格和当前层级批量传输；时间：2026-11-12；作者：lq。
use super::*;
use gpui_kit::component::scroll::{ScrollableMask, Scrollbar, ScrollbarMode};

impl FileBrowser {
    /// 功能：详情列表仅列出当前目录直接子项，隐藏过滤同时约束多选范围；时间：2026-11-11；作者：lq。
    pub(super) fn visible_entries(&self, hidden: bool) -> Vec<&sftp::Entry> {
        self.entries
            .get(&self.current)
            .into_iter()
            .flatten()
            .filter(|entry| hidden || !entry.name.starts_with('.'))
            .collect()
    }

    /// 功能：刷新或隐藏过滤后移除失效选择，文件操作沿用最后一个有效选中项；时间：2026-11-11；作者：lq。
    pub(super) fn prune_selection(&mut self, hidden: bool) {
        let visible: BTreeSet<_> = self
            .visible_entries(hidden)
            .iter()
            .map(|e| e.path.clone())
            .collect();
        self.selection.retain(|path| visible.contains(path));
        if self
            .anchor
            .as_ref()
            .is_some_and(|path| !visible.contains(path))
        {
            self.anchor = None;
        }
        self.selected = self
            .selected
            .as_ref()
            .filter(|e| self.selection.contains(&e.path))
            .and_then(|e| self.entry(&e.path))
            .or_else(|| {
                self.selection
                    .iter()
                    .next()
                    .and_then(|path| self.entry(path))
            });
    }

    /// 功能：以可见行顺序冻结传输源，防止展开子目录后重复上传父子项目；时间：2026-11-11；作者：lq。
    pub(super) fn selected_entries(&self, hidden: bool) -> Vec<sftp::Entry> {
        self.visible_entries(hidden)
            .into_iter()
            .filter(|e| self.selection.contains(&e.path))
            .cloned()
            .collect()
    }

    /// 功能：普通单选、平台组合键增减选、Shift 按当前目录顺序连续选择；时间：2026-11-11；作者：lq。
    fn select(&mut self, entry: &sftp::Entry, modifiers: Modifiers, hidden: bool) {
        let additive = modifiers.platform || modifiers.control;
        if modifiers.shift {
            let paths: Vec<_> = self
                .visible_entries(hidden)
                .iter()
                .map(|e| e.path.clone())
                .collect();
            let end = paths.iter().position(|p| p == &entry.path);
            let start = self
                .anchor
                .as_ref()
                .and_then(|a| paths.iter().position(|p| p == a));
            if !additive {
                self.selection.clear();
            }
            if let (Some(start), Some(end)) = (start, end) {
                self.selection
                    .extend(paths[start.min(end)..=start.max(end)].iter().cloned());
            } else {
                self.selection.insert(entry.path.clone());
                self.anchor = Some(entry.path.clone());
            }
        } else {
            if !additive {
                self.selection.clear();
            }
            if !additive || !self.selection.remove(&entry.path) {
                self.selection.insert(entry.path.clone());
            }
            self.anchor = Some(entry.path.clone());
        }
        self.selected = self.selection.contains(&entry.path).then(|| entry.clone());
        self.prune_selection(hidden);
    }
}

impl ServerWorkspace {
    /// 功能：进入目录时更新地址并清空跨层级选择；时间：2026-11-12；作者：lq。
    pub(super) fn navigate_directory(
        &mut self,
        side: FileSide,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let browser = self.services.files.browser_mut(side);
        // 功能：切换目录使旧后台结果失效，并释放上一层缓存；时间：2026-11-12；作者：lq。
        browser.generation += 1;
        browser.entries.clear();
        browser.errors.clear();
        browser.loading.clear();
        browser.current = path.clone();
        browser.selection.clear();
        browser.selected = None;
        browser.anchor = None;
        browser.scroll.scroll_to_item(0, ScrollStrategy::Top);
        let input = if side == FileSide::Local {
            &self.services.local_directory
        } else {
            &self.services.remote_path
        };
        input.update(cx, |input, cx| {
            input.set_value(path.display().to_string(), window, cx)
        });
        self.load_directory(side, path, window, cx);
        cx.notify();
    }

    /// 功能：全选只覆盖当前层级可见项，焦点限定于对应文件列表；时间：2026-11-11；作者：lq。
    pub(super) fn select_all_files(
        &mut self,
        side: FileSide,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let browser = self.services.files.browser_mut(side);
        browser.selection = browser
            .visible_entries(self.services.show_hidden)
            .iter()
            .map(|e| e.path.clone())
            .collect();
        browser.selected = browser
            .visible_entries(self.services.show_hidden)
            .first()
            .map(|e| (*e).clone());
        browser.anchor = browser.selected.as_ref().map(|e| e.path.clone());
        browser.focus.focus(window, cx);
        cx.notify();
    }

    /// 功能：批量预检容量和链接后统一排队，防止只传输全选中的前若干项；时间：2026-11-11；作者：lq。
    pub(super) fn enqueue_batch(
        &mut self,
        side: FileSide,
        entries: Vec<sftp::Entry>,
        directory: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if entries.is_empty() {
            return;
        }
        if entries
            .iter()
            .any(|e| e.symlink || e.path.file_name().is_none())
        {
            self.status = "所选项目含符号链接，请取消选择链接后再传输".into();
            self.error = true;
        } else if self.services.files.queue.len() + entries.len() > 20000 {
            self.status = "批量传输超过队列容量（20000 项），请分批选择".into();
            self.error = true;
        } else if self.services.sftp.is_none() {
            self.status = "SFTP 未连接".into();
            self.error = true;
        } else {
            for entry in entries {
                self.enqueue_transfer(side, entry, directory.clone(), window, cx);
            }
        }
        cx.notify();
    }

    /// 功能：虚拟化详情，文件名吸收额外宽度，属性可横向滚动；时间：2026-11-12；作者：lq。
    pub(super) fn directory_list(&self, side: FileSide, cx: &mut Context<Self>) -> AnyElement {
        let browser = self.services.files.browser(side);
        let count = browser.visible_entries(self.services.show_hidden).len();
        let header = h_flex()
            .h(px(26.))
            .flex_shrink_0()
            .border_b_1()
            .border_color(cx.theme().border)
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            // 功能：文件名列优先扩展，表头与数据行保持相同伸缩规则；时间：2026-11-12；作者：lq。
            .child(div().flex_1().min_w(px(240.)).px_2().child("文件名"))
            .children(
                [
                    ("大小", 85.),
                    ("类型", 100.),
                    ("修改时间", 155.),
                    ("权限", 110.),
                    ("用户/用户组", 125.),
                ]
                .into_iter()
                .map(|(label, width)| detail_cell(label.into(), width)),
            );
        let focus = browser.focus.clone();
        let view = cx.entity();
        let list = uniform_list(
            SharedString::from(format!("file-details-{}", side.id())),
            count,
            move |range, _, cx| {
                view.update(cx, |this, cx| {
                    range
                        .map(|index| this.detail_row(side, index, cx))
                        .collect::<Vec<_>>()
                })
            },
        )
        .track_scroll(&browser.scroll)
        .size_full();
        let content = div()
            .id(SharedString::from(format!(
                "file-details-scroll-{}",
                side.id()
            )))
            .test_support()
            // 功能：仅裁切并应用横向手动滚动偏移，禁止将纵向滚轮转换成横向；时间：2026-11-12；作者：lq。
            .absolute()
            .top_0()
            .left_0()
            .right(px(16.))
            .bottom(px(16.))
            .overflow_hidden()
            .track_scroll(&browser.horizontal)
            .track_focus(&focus)
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                let key = &event.keystroke;
                if key.key == "a" && (key.modifiers.platform || key.modifiers.control) {
                    cx.stop_propagation();
                    this.select_all_files(side, window, cx);
                } else if key.key == "escape" {
                    let browser = this.services.files.browser_mut(side);
                    browser.selection.clear();
                    browser.selected = None;
                    browser.anchor = None;
                    cx.stop_propagation();
                    cx.notify();
                } else if key.key == "enter" {
                    let browser = this.services.files.browser(side);
                    if browser.selection.len() == 1
                        && let Some(entry) = browser.selected.clone()
                    {
                        cx.stop_propagation();
                        if entry.directory && !entry.symlink {
                            this.navigate_directory(side, entry.path, window, cx);
                        } else if side == FileSide::Remote && !entry.symlink {
                            this.open_remote(entry.path, window, cx);
                        }
                    }
                }
            }))
            .child(
                v_flex()
                    .w(px(815.))
                    .min_w_full()
                    .h_full()
                    .child(header)
                    .when(count == 0, |v| {
                        v.child(
                            div()
                                .p_3()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(
                                    browser.empty_hint(&browser.current, self.services.show_hidden),
                                )
                                // 功能：仅隐藏项的快捷入口放在空态内，避免挤压窄栏上传下载；时间：2026-11-12；作者：lq。
                                .when(
                                    !self.services.show_hidden
                                        && browser.hidden_count(&browser.current) > 0,
                                    |v| {
                                        v.child(
                                            Button::new(SharedString::from(format!(
                                                "show-hidden-{}",
                                                side.id()
                                            )))
                                            .ghost()
                                            .xsmall()
                                            .label("显示隐藏文件")
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.services.show_hidden = true;
                                                this.refresh_file_selection(FileSide::Local);
                                                this.refresh_file_selection(FileSide::Remote);
                                                this.rebuild_remote_tree(false, cx);
                                                cx.notify();
                                            })),
                                        )
                                    },
                                ),
                        )
                    })
                    .when(count > 0, |v| v.child(div().flex_1().min_h_0().child(list))),
            )
            .into_any_element();
        div()
            .relative()
            .size_full()
            .child(content)
            // 功能：滚轮在整个列表视口（含表头）只控制纵向，横向仅通过底部条手动调整；时间：2026-11-12；作者：lq。
            .child(
                ScrollableMask::new(Axis::Vertical, &browser.scroll).id(SharedString::from(
                    format!("file-vertical-wheel-{}", side.id()),
                )),
            )
            // 功能：纵向滚动条固定在可见区域右侧，不随宽表格横移，排除表头和底部横条；时间：2026-11-12；作者：lq。
            .child(
                div()
                    .id(SharedString::from(format!(
                        "file-vertical-track-{}",
                        side.id()
                    )))
                    .test_support()
                    .absolute()
                    .top(px(26.))
                    .bottom(px(16.))
                    .right_0()
                    .w(px(16.))
                    .child(
                        Scrollbar::vertical(&browser.scroll)
                            .id(SharedString::from(format!(
                                "file-vertical-bar-{}",
                                side.id()
                            )))
                            .viewport_from_layout()
                            .mode(ScrollbarMode::Always),
                    ),
            )
            // 功能：横向条独占底部空间，与纵向条互不遮挡；时间：2026-11-12；作者：lq。
            .child(
                div()
                    .id(SharedString::from(format!(
                        "file-horizontal-track-{}",
                        side.id()
                    )))
                    .test_support()
                    .absolute()
                    .bottom_0()
                    .left_0()
                    .right(px(16.))
                    .h(px(16.))
                    .child(
                        Scrollbar::horizontal(&browser.horizontal)
                            .id(SharedString::from(format!(
                                "file-horizontal-bar-{}",
                                side.id()
                            )))
                            .viewport_from_layout()
                            .mode(ScrollbarMode::Always),
                    ),
            )
            .into_any_element()
    }

    /// 功能：多选行、文件属性、双击进入与双向批量拖放共享当前目录快照；时间：2026-11-11；作者：lq。
    fn detail_row(&self, side: FileSide, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let browser = self.services.files.browser(side);
        let Some(entry) = browser
            .visible_entries(self.services.show_hidden)
            .get(index)
            .map(|e| (*e).clone())
        else {
            return div().into_any_element();
        };
        let view = cx.entity();
        let owner = view.entity_id();
        let selected = browser.selection.contains(&entry.path);
        let down = entry.clone();
        let clicked = entry.clone();
        let drag_view = view.clone();
        let row = h_flex()
            .id((
                SharedString::from(format!("detail-row-{}", side.id())),
                index,
            ))
            .h(px(28.))
            .w_full()
            .text_xs()
            .cursor_pointer()
            .when(selected, |v| v.bg(cx.theme().selection))
            .when(!selected && index % 2 == 1, |v| {
                v.bg(cx.theme().muted.opacity(0.3))
            })
            .child(
                h_flex()
                    .id((
                        if side == FileSide::Local {
                            "local-entry"
                        } else {
                            "remote-entry"
                        },
                        index,
                    ))
                    .test_support()
                    // 功能：单侧全宽时新增空间优先分配给文件名；时间：2026-11-12；作者：lq。
                    .flex_1()
                    .min_w(px(240.))
                    .px_2()
                    .gap_1()
                    .overflow_hidden()
                    .child(
                        Icon::new(if entry.directory {
                            UiIcon::Folder
                        } else {
                            UiIcon::FileCode
                        })
                        .small(),
                    )
                    .child(div().text_ellipsis().min_w_0().child(entry.name.clone())),
            )
            .child(detail_cell(
                if entry.directory {
                    "—".into()
                } else {
                    format_bytes(entry.size)
                },
                85.,
            ))
            .child(detail_cell(file_type(&entry), 100.))
            .child(detail_cell(modified_text(entry.modified), 155.))
            .child(detail_cell(permission_text(&entry), 110.))
            .child(detail_cell(owner_text(&entry), 125.))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    let browser = this.services.files.browser_mut(side);
                    browser.focus.focus(window, cx);
                    // 功能：按下已选行保留整组选中，拖动不会退化为单文件；时间：2026-11-11；作者：lq。
                    if !browser.selection.contains(&down.path)
                        || event.modifiers.shift
                        || event.modifiers.platform
                        || event.modifiers.control
                    {
                        browser.select(&down, event.modifiers, this.services.show_hidden);
                    }
                    cx.notify();
                }),
            )
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                cx.stop_propagation();
                if event.click_count() == 2 {
                    if clicked.directory && !clicked.symlink {
                        this.navigate_directory(side, clicked.path.clone(), window, cx);
                    } else if side == FileSide::Remote && !clicked.symlink {
                        this.open_remote(clicked.path.clone(), window, cx);
                    }
                } else if !event.modifiers().shift
                    && !event.modifiers().platform
                    && !event.modifiers().control
                {
                    this.services.files.browser_mut(side).select(
                        &clicked,
                        Modifiers::default(),
                        this.services.show_hidden,
                    );
                    cx.notify();
                }
            }));
        let row = if !entry.symlink {
            row.on_drag(
                FileDrag {
                    owner,
                    side,
                    entries: std::rc::Rc::new(std::cell::RefCell::new(vec![entry.clone()])),
                },
                move |drag, _, _, cx| {
                    // 功能：在拖动真正开始时冻结最新多选，而非沿用上次渲染的选择；时间：2026-11-11；作者：lq。
                    let state = drag_view.read(cx);
                    let selection = state
                        .services
                        .files
                        .browser(side)
                        .selected_entries(state.services.show_hidden);
                    if selection
                        .iter()
                        .any(|e| e.path == drag.entries.borrow()[0].path)
                    {
                        *drag.entries.borrow_mut() = selection;
                    }
                    cx.new(|_| drag.clone())
                },
            )
        } else {
            row
        };
        if entry.directory && !entry.symlink {
            row.can_drop(move |data, _, _| {
                data.downcast_ref::<FileDrag>()
                    .is_some_and(|d| d.owner == owner && d.side != side)
            })
            .drag_over::<FileDrag>(|style, _, _, cx| {
                style
                    .bg(cx.theme().accent)
                    .border_1()
                    .border_color(cx.theme().primary)
            })
            .on_drop(cx.listener(move |this, drag: &FileDrag, window, cx| {
                cx.stop_propagation();
                this.enqueue_batch(
                    drag.side,
                    drag.entries.borrow().clone(),
                    entry.path.clone(),
                    window,
                    cx,
                );
            }))
            .into_any_element()
        } else {
            row.into_any_element()
        }
    }
}

/// 功能：表头与数据行共用固定列宽；时间：2026-11-11；作者：lq。
fn detail_cell(value: String, width: f32) -> impl IntoElement {
    div()
        .w(px(width))
        .flex_shrink_0()
        .px_2()
        .overflow_hidden()
        .text_ellipsis()
        .child(value)
}

/// 功能：文件夹、链接和扩展名具有可辨认的类型文本；时间：2026-11-11；作者：lq。
fn file_type(entry: &sftp::Entry) -> String {
    if entry.symlink {
        "符号链接".into()
    } else if entry.directory {
        "文件夹".into()
    } else {
        Path::new(&entry.name)
            .extension()
            .and_then(|e| e.to_str())
            .filter(|e| !e.is_empty())
            .map(|e| format!("{} 文件", e.to_uppercase()))
            .unwrap_or_else(|| "文件".into())
    }
}

/// 功能：按系统时区格式化服务端 UNIX 修改时间，未知或越界显示占位符；时间：2026-11-11；作者：lq。
fn modified_text(seconds: Option<u64>) -> String {
    seconds
        .and_then(|s| i64::try_from(s).ok())
        .and_then(|s| chrono::DateTime::from_timestamp(s, 0))
        .map(|time| {
            time.with_timezone(&chrono::Local)
                .format("%Y/%m/%d %H:%M")
                .to_string()
        })
        .unwrap_or_else(|| "—".into())
}

/// 功能：展示真实 UID/GID，SFTP v3 不返回名称时不猜测属主；时间：2026-11-11；作者：lq。
fn owner_text(entry: &sftp::Entry) -> String {
    let text = |id: Option<u32>| id.map(|v| v.to_string()).unwrap_or_else(|| "—".into());
    format!("{} / {}", text(entry.uid), text(entry.gid))
}

/// 功能：展示 POSIX 类型和 rwx 权限，包含 setuid/setgid/sticky 特殊位；时间：2026-11-11；作者：lq。
fn permission_text(entry: &sftp::Entry) -> String {
    if entry.permissions == 0 {
        return "—".into();
    }
    let mut chars = vec![if entry.symlink {
        'l'
    } else if entry.directory {
        'd'
    } else {
        '-'
    }];
    for (bit, letter) in [
        (0o400, 'r'),
        (0o200, 'w'),
        (0o100, 'x'),
        (0o040, 'r'),
        (0o020, 'w'),
        (0o010, 'x'),
        (0o004, 'r'),
        (0o002, 'w'),
        (0o001, 'x'),
    ] {
        chars.push(if entry.permissions & bit != 0 {
            letter
        } else {
            '-'
        });
    }
    for (position, bit, lower, upper) in [
        (3, 0o4000, 's', 'S'),
        (6, 0o2000, 's', 'S'),
        (9, 0o1000, 't', 'T'),
    ] {
        if entry.permissions & bit != 0 {
            chars[position] = if chars[position] == 'x' { lower } else { upper };
        }
    }
    chars.into_iter().collect()
}

/// 功能：在后台目录读取时收集本地权限，非 Unix 平台无 POSIX 属性时显示未知；时间：2026-11-11；作者：lq。
pub(super) fn local_permissions(metadata: &std::fs::Metadata) -> u32 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        metadata.mode()
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        0
    }
}

/// 功能：本地 UID/GID 由系统文件元数据提供，不把远程编号解析成本机用户；时间：2026-11-11；作者：lq。
pub(super) fn local_owner(metadata: &std::fs::Metadata) -> (Option<u32>, Option<u32>) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        (Some(metadata.uid()), Some(metadata.gid()))
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        (None, None)
    }
}

// 功能：文件属性边界回归，覆盖链接、特殊权限与未知/越界时间；时间：2026-11-11；作者：lq。
#[cfg(test)]
mod tests {
    use super::{file_type, modified_text, owner_text, permission_text};
    use qds6_services::sftp;

    /// 功能：以真实 POSIX 模式位构造文件属性，不需要网络服务；时间：2026-11-11；作者：lq。
    fn entry(mode: u32) -> sftp::Entry {
        sftp::Entry {
            path: "/fixture/test.txt".into(),
            name: "test.txt".into(),
            size: 5,
            permissions: mode,
            directory: false,
            symlink: false,
            modified: None,
            uid: Some(1001),
            gid: None,
        }
    }

    /// 功能：权限含目录/链接类型和特殊执行位，0000 权限不误报未知；时间：2026-11-11；作者：lq。
    #[test]
    fn posix_permissions_preserve_special_bits() {
        assert_eq!(permission_text(&entry(0o100644)), "-rw-r--r--");
        assert_eq!(permission_text(&entry(0o104755)), "-rwsr-xr-x");
        assert_eq!(permission_text(&entry(0o102640)), "-rw-r-S---");
        assert_eq!(permission_text(&entry(0o100000)), "----------");
        let mut directory = entry(0o041777);
        directory.directory = true;
        assert_eq!(permission_text(&directory), "drwxrwxrwt");
        directory.symlink = true;
        assert!(permission_text(&directory).starts_with('l'));
        assert_eq!(file_type(&directory), "符号链接");
    }

    /// 功能：缺失属主/时间及超范围时间显示未知，保留有效 UID；时间：2026-11-11；作者：lq。
    #[test]
    fn unknown_metadata_is_not_invented() {
        let item = entry(0);
        assert_eq!(permission_text(&item), "—");
        assert_eq!(owner_text(&item), "1001 / —");
        assert_eq!(modified_text(None), "—");
        assert_eq!(modified_text(Some(u64::MAX)), "—");
        assert_eq!(file_type(&item), "TXT 文件");
        assert_ne!(modified_text(Some(1_800_000_000)), "—");
    }
}
