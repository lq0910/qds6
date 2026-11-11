//! 功能：服务器独立工作区，上方终端、下方本地/远程文件，任务和草稿按会话隔离；时间：2026-11-11；作者：lq。

use super::*;
use gpui_kit::component::resizable::{resizable_panel, v_resizable};
// 功能：原生双栏树与传输队列；时间：2026-11-11；作者：lq。
mod files_ui;
use files_ui::FilesUi;
// 功能：文件可见性和连接栏回归入口仅在验收构建导出；时间：2026-11-11；作者：lq。
#[cfg(feature = "ui-tests")]
pub use files_ui::smoke::run_sidebar_files_smoke;
// 功能：可收起的实时状态侧栏与独立采样生命周期；时间：2026-11-11；作者：lq。
pub(crate) mod status_ui;
use qds6_services::{
    sftp::{self, RemoteDocument},
    terminal::TerminalSession,
};
use status_ui::StatusUi;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};

/// 功能：每个服务器标签拥有独立终端、SFTP、编辑器、传输任务与状态；时间：2026-11-11；作者：lq。
pub(super) struct ServerWorkspace {
    pub(super) profile: ConnectionProfile,
    pub(super) terminal: Option<Entity<crate::terminal_view::TerminalView>>,
    pub(super) services: ServerFiles,
    pub(super) status: SharedString,
    error: bool,
    // 功能：连接或草稿恢复警告独立保留，不被目录加载成功覆盖；时间：2026-11-11；作者：lq。
    connection_notice: Option<String>,
    storage_path: Option<PathBuf>,
    files_visible: bool,
    // 功能：监控按服务器隔离，不参与文件任务锁；时间：2026-11-11；作者：lq。
    monitoring: StatusUi,
}

/// 功能：文件状态只属于当前服务器实体，切换标签不会复用路径或草稿；时间：2026-11-11；作者：lq。
pub(super) struct ServerFiles {
    pub busy: bool,
    pub(super) task: Option<Task<()>>,
    pub(super) page: usize,
    pub(super) sftp: Option<Arc<Mutex<sftp::Client>>>,
    pub(super) sftp_profile: Option<ConnectionProfile>,
    pub(super) draft_task: Option<Task<()>>,
    pub(super) draft_revision: Arc<AtomicU64>,
    pub draft_pending: bool,
    files: FilesUi,
    pub(super) local_directory: Entity<InputState>,
    pub(super) show_hidden: bool,
    pub(super) remote_path: Entity<InputState>,
    pub(super) target: Entity<InputState>,
    pub(super) remote_editor: Entity<EditorState>,
    pub(super) remote_document: Option<RemoteDocument>,
    pub remote_dirty: bool,
    pub(super) transfer: sftp::Transfer,
    pub(super) show_file_tools: bool,
    pub(super) _editor_subscription: Subscription,
}

impl ServerWorkspace {
    /// 功能：接收后台认证结果，建立独立文件视图并在后台读取初始目录；时间：2026-11-11；作者：lq。
    pub(super) fn new(
        profile: ConnectionProfile,
        terminal: Option<TerminalSession>,
        client: Option<sftp::Client>,
        storage_path: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let remote_editor = cx.new(|cx| {
            EditorState::new(window, cx)
                .line_number(true)
                .searchable(true)
                .soft_wrap(false)
        });
        let subscription = cx.subscribe(&remote_editor, |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.services.remote_dirty =
                    this.services.remote_document.as_ref().is_some_and(|d| {
                        d.text.replace("\r\n", "\n")
                            != this.services.remote_editor.read(cx).value().as_str()
                    });
                // 功能：撤销回基线也更新缓存，避免重启恢复已撤销的旧内容；时间：2026-11-11；作者：lq。
                if this.services.remote_document.is_some() {
                    this.queue_draft(cx);
                }
                cx.notify();
            }
        });
        let files = Self::new_files_ui(window, cx);
        Self {
            terminal: terminal.map(|session| {
                cx.new(|cx| crate::terminal_view::TerminalView::new(session, window, cx))
            }),
            services: ServerFiles {
                busy: false,
                task: None,
                page: 1,
                sftp: client.map(|client| Arc::new(Mutex::new(client))),
                sftp_profile: Some(profile.clone()),
                draft_task: None,
                draft_revision: Arc::new(AtomicU64::new(0)),
                draft_pending: false,
                files,
                local_directory: cx.new(|cx| {
                    InputState::new(window, cx)
                        .default_value(std::env::var("HOME").unwrap_or_else(|_| "/tmp".into()))
                }),
                show_hidden: false,
                remote_path: cx.new(|cx| InputState::new(window, cx).default_value(".")),
                target: cx
                    .new(|cx| InputState::new(window, cx).placeholder("目标名称 / 八进制权限")),
                remote_editor,
                remote_document: None,
                remote_dirty: false,
                transfer: Default::default(),
                show_file_tools: false,
                _editor_subscription: subscription,
            },
            profile,
            storage_path,
            status: "服务器已连接 · 终端与文件互不阻塞".into(),
            error: false,
            connection_notice: None,
            files_visible: true,
            monitoring: StatusUi::default(),
        }
    }

    /// 功能：服务器工作区显式初始化目录和加密草稿，不读取其他连接的数据；时间：2026-11-11；作者：lq。
    pub(super) fn initialize(
        &mut self,
        draft: Option<qds6_services::drafts::Draft>,
        error: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(draft) = draft {
            self.services
                .remote_editor
                .update(cx, |editor, cx| editor.set_value(draft.text, window, cx));
            self.services.remote_document = Some(draft.baseline);
            self.services.remote_dirty = true;
            self.services.page = 2;
        }
        self.subscribe_file_paths(window, cx);
        self.connection_notice = error;
        self.refresh_remote(window, cx);
        self.refresh_local(window, cx);
        cx.notify();
    }

    /// 功能：供窗口关闭与标签关闭检查独立后台任务；时间：2026-11-11；作者：lq。
    pub(super) fn pending(&self) -> bool {
        self.services.busy || self.services.draft_pending || self.services.files.pending()
    }

    /// 功能：复用主程序数据路径规则，凭据仍由服务层管理；时间：2026-11-11；作者：lq。
    fn store_path() -> Result<PathBuf, String> {
        Workbench::store_path()
    }

    /// 功能：统一后台任务边界，失败保留输入和草稿；时间：2026-11-10；作者：lq。
    pub(super) fn service_job<T: Send + 'static>(
        &mut self,
        label: &str,
        job: impl FnOnce() -> Result<T, String> + Send + 'static,
        done: impl FnOnce(&mut Self, T, &mut Window, &mut Context<Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.services.busy {
            return;
        }
        self.services.busy = true;
        self.error = false;
        self.status = label.to_string().into();
        self.services.task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = cx.background_spawn(async move { job() }).await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.services.busy = false;
                match result {
                    Ok(value) => {
                        this.error = false;
                        done(this, value, window, cx);
                    }
                    Err(error) => {
                        this.status = error.into();
                        this.error = true;
                    }
                }
                this.start_next_transfer(window, cx);
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// 功能：用户确认丢弃后后台清理加密缓存，失败明确告知可能仍可恢复旧草稿；时间：2026-11-10；作者：lq。
    fn discard_remote(&mut self, cx: &mut Context<Self>) {
        self.services.draft_revision.fetch_add(1, Ordering::SeqCst);
        self.services.draft_task = None;
        self.services.draft_pending = false;
        self.services.remote_document = None;
        self.services.remote_dirty = false;
        self.services.page = 1;
        let storage = self.storage_path.clone();
        let connection = self.services.sftp_profile.as_ref().map(|p| p.id);
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    let path = storage.map(Ok).unwrap_or_else(Self::store_path)?;
                    if let Some(id) = connection {
                        qds6_services::drafts::remove(&path.with_file_name("drafts"), id)?;
                    }
                    Ok::<_, String>(())
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.status = match result {
                    Ok(()) => "草稿已丢弃并清理缓存".into(),
                    Err(error) => format!("草稿已关闭，但缓存清理失败：{error}").into(),
                };
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// 功能：远程草稿防抖加密落盘，Keychain 不可用时明确回退会话草稿；时间：2026-11-10；作者：lq。
    fn queue_draft(&mut self, cx: &mut Context<Self>) {
        let (Some(profile), Some(baseline)) = (
            self.services.sftp_profile.as_ref(),
            self.services.remote_document.clone(),
        ) else {
            return;
        };
        let draft = qds6_services::drafts::Draft {
            schema: 1,
            connection: profile.id,
            baseline,
            text: self.services.remote_editor.read(cx).value().to_string(),
        };
        let storage = self.storage_path.clone();
        self.services.draft_pending = true;
        let revision = self.services.draft_revision.clone();
        let expected = revision.fetch_add(1, Ordering::SeqCst) + 1;
        self.services.draft_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(600))
                .await;
            let result = cx
                .background_spawn(async move {
                    let path = storage.map(Ok).unwrap_or_else(Self::store_path)?;
                    qds6_services::drafts::save_current(
                        &path.with_file_name("drafts"),
                        &draft,
                        &revision,
                        expected,
                    )
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.services.draft_revision.load(Ordering::SeqCst) != expected {
                    return;
                }
                this.services.draft_pending = false;
                this.status = match result {
                    Ok(()) => "远程草稿已加密缓存；未上传至服务器".into(),
                    Err(error) => format!("草稿仅保留本次会话：{error}").into(),
                };
                cx.notify();
            });
        }));
    }

    /// 功能：保留未保存远程草稿，明确保存/复制后才允许打开其他文件；时间：2026-11-10；作者：lq。
    pub(super) fn open_remote(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.services.remote_dirty {
            self.status = "当前远程草稿有修改，请先保存或复制后再打开其他文件".into();
            cx.notify();
            return;
        }
        let Some(client) = self.services.sftp.clone() else {
            return;
        };
        self.service_job(
            "正在读取远程文本…",
            move || {
                client
                    .lock()
                    .map_err(|_| "SFTP 状态异常")?
                    .open_document(&path)
            },
            |this, document, window, cx| {
                let text = document.text.replace("\r\n", "\n");
                // 功能：使用 Kit 内置语法，不执行文件内容；时间：2026-11-11；作者：lq。
                let language = match document
                    .path
                    .extension()
                    .and_then(|v| v.to_str())
                    .unwrap_or("")
                {
                    "json" => "json",
                    "sh" | "bash" | "zsh" => "bash",
                    "sql" => "sql",
                    "toml" => "toml",
                    "yaml" | "yml" => "yaml",
                    _ => "plain",
                };
                this.services.remote_editor.update(cx, |editor, cx| {
                    editor.set_highlighter(language, cx);
                    editor.set_value(text, window, cx);
                });
                this.services.remote_document = Some(document);
                this.services.page = 2;
                this.services.remote_dirty = false;
                this.status = "文本已打开 · 修改将加密缓存，保存后才会上传".into();
            },
            window,
            cx,
        );
    }

    /// 功能：内容冲突/失败时保留编辑器及未保存标记；时间：2026-11-10；作者：lq。
    pub(super) fn save_remote(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(client), Some(baseline)) = (
            self.services.sftp.clone(),
            self.services.remote_document.clone(),
        ) else {
            return;
        };
        let mut text = self.services.remote_editor.read(cx).value().to_string();
        if baseline.crlf {
            text = text.replace("\r\n", "\n").replace('\n', "\r\n");
        }
        let storage = self.storage_path.clone();
        let connection = self.services.sftp_profile.as_ref().map(|p| p.id);
        self.services.draft_revision.fetch_add(1, Ordering::SeqCst);
        self.services.draft_task = None;
        self.services.draft_pending = false;
        self.service_job(
            "正在检查冲突并保存…",
            move || {
                let document = client
                    .lock()
                    .map_err(|_| "SFTP 状态异常")?
                    .save_document(&baseline, &text)?;
                if let (Ok(path), Some(connection)) =
                    (storage.map(Ok).unwrap_or_else(Self::store_path), connection)
                {
                    let _ =
                        qds6_services::drafts::remove(&path.with_file_name("drafts"), connection);
                }
                Ok(document)
            },
            |this, document, _, _| {
                this.services.remote_document = Some(document);
                this.services.remote_dirty = false;
                this.status =
                    "远程文件已保存 · 已请求服务器原子重命名，跨客户端仍存在提交竞争窗口".into();
            },
            window,
            cx,
        );
    }

    /// 功能：上传下载默认拒绝覆盖，删除确认且只支持空目录；时间：2026-11-10；作者：lq。
    fn file_operation(
        &mut self,
        action: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if action == "remove" {
            let Some(path) = self
                .services
                .files
                .remote
                .selected
                .as_ref()
                .map(|entry| entry.path.clone())
            else {
                return;
            };
            let description = path.display().to_string();
            let answer = window.prompt(
                PromptLevel::Warning,
                "删除远程选中项？",
                Some(&description),
                &["取消", "删除"],
                cx,
            );
            cx.spawn_in(window, async move |this, cx| {
                if answer.await == Ok(1) {
                    let _ = this.update_in(cx, |this, window, cx| {
                        this.perform_file_operation("confirmed-remove", Some(path), window, cx)
                    });
                }
            })
            .detach();
        } else {
            self.perform_file_operation(action, None, window, cx);
        }
    }

    /// 功能：后台串行文件操作与明确取消状态，失败不丢弃旧列表；时间：2026-11-10；作者：lq。
    fn perform_file_operation(
        &mut self,
        action: &'static str,
        confirmed_path: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(client) = self.services.sftp.clone() else {
            return;
        };
        // 功能：删除确认冻结原路径；目录重命名发生在父目录，避免误移动进自身；时间：2026-11-11；作者：lq。
        let selected = confirmed_path.or_else(|| {
            self.services
                .files
                .remote
                .selected
                .as_ref()
                .map(|entry| entry.path.clone())
        });
        let directory = if action == "rename" {
            selected
                .as_ref()
                .and_then(|path| path.parent())
                .map(PathBuf::from)
                .unwrap_or_else(|| self.services.files.remote.destination())
        } else {
            self.services.files.remote.destination()
        };
        let target = self.services.target.read(cx).value().to_string();
        self.service_job(
            "正在操作远程文件…",
            move || {
                let client = client.lock().map_err(|_| "SFTP 状态异常")?;
                let target_path = || -> Result<PathBuf, String> {
                    if target.is_empty()
                        || target.contains('/')
                        || target == "."
                        || target == ".."
                        || target.contains('\0')
                    {
                        Err("目标名称应为单个文件名，不能包含路径".into())
                    } else {
                        Ok(directory.join(&target))
                    }
                };
                match action {
                    "mkdir" => client.mkdir(&target_path()?),
                    "rename" => {
                        client.rename(&selected.ok_or("请先选择远程文件")?, &target_path()?)
                    }
                    "chmod" => client.chmod(
                        &selected.ok_or("请先选择远程文件")?,
                        u32::from_str_radix(&target, 8).map_err(|_| "请输入八进制权限，如 0644")?,
                    ),
                    "confirmed-remove" => client.remove(&selected.ok_or("请先选择远程文件")?),
                    _ => Err("未知文件操作".into()),
                }
            },
            |this, _, window, cx| {
                this.status = "文件操作成功".into();
                this.refresh_remote(window, cx);
            },
            window,
            cx,
        );
    }

    /// 功能：远程文本保留在当前服务器下半区，随时返回文件列表；时间：2026-11-11；作者：lq。
    fn remote_page(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        v_flex()
            .size_full()
            .child(
                h_flex()
                    .gap_2()
                    .flex_shrink_0()
                    .min_h(px(46.))
                    .px_3()
                    .py_2()
                    .items_center()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        div().flex_1().min_w_0().text_xs().text_ellipsis().child(
                            self.services
                                .remote_document
                                .as_ref()
                                .map(|d| {
                                    format!(
                                        "{}{} · {}",
                                        d.path.display(),
                                        if self.services.remote_dirty {
                                            " •"
                                        } else {
                                            ""
                                        },
                                        if d.crlf { "CRLF" } else { "LF" }
                                    )
                                })
                                .unwrap_or_else(|| "尚未打开文件".into()),
                        ),
                    )
                    .child(
                        Button::new("save-remote")
                            .primary()
                            .small()
                            .icon(UiIcon::Save)
                            .label("保存远程文件")
                            .disabled(self.services.busy || self.services.remote_document.is_none())
                            .on_click(
                                cx.listener(|this, _, window, cx| this.save_remote(window, cx)),
                            ),
                    )
                    .child(
                        Button::new("copy-remote")
                            .ghost()
                            .small()
                            .label("复制草稿")
                            .on_click(cx.listener(|this, _, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(
                                    this.services.remote_editor.read(cx).value().to_string(),
                                ))
                            })),
                    )
                    .child(
                        Button::new("discard-remote")
                            .ghost()
                            .small()
                            .disabled(self.services.busy)
                            .label("放弃并关闭")
                            .on_click(cx.listener(|_this, _, window, cx| {
                                let answer = window.prompt(
                                    PromptLevel::Warning,
                                    "丢弃当前远程草稿？",
                                    None,
                                    &["取消", "丢弃"],
                                    cx,
                                );
                                cx.spawn_in(window, async move |this, cx| {
                                    if answer.await == Ok(1) {
                                        let _ = this.update_in(cx, |this, _, cx| {
                                            this.discard_remote(cx);
                                        });
                                    }
                                })
                                .detach();
                            })),
                    ),
            )
            .child(
                div().flex_1().min_h_0().child(
                    Editor::new(&self.services.remote_editor)
                        .disabled(self.services.busy)
                        .size_full()
                        .aria_label("远程文件编辑器"),
                ),
            )
            .into_any_element()
    }
}

/// 功能：终端与双栏文件默认同屏，拖动分隔条调整比例；时间：2026-11-11；作者：lq。
impl Render for ServerWorkspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let terminal = v_flex()
            .id("server-terminal-pane")
            .test_support()
            .size_full()
            .min_h_0()
            .child(
                h_flex()
                    .h(px(32.))
                    .flex_shrink_0()
                    .px_3()
                    .items_center()
                    .gap_2()
                    .child(Icon::new(UiIcon::Terminal).small())
                    .child(div().text_xs().child(format!(
                        "{}@{}:{}",
                        self.profile.username, self.profile.host, self.profile.port
                    )))
                    .child(div().flex_1())
                    .child(
                        Button::new("toggle-server-status")
                            .ghost()
                            .xsmall()
                            .icon(UiIcon::Server)
                            .label(if self.monitoring.visible {
                                "收起状态"
                            } else {
                                "服务器状态"
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.monitoring.visible = !this.monitoring.visible;
                                cx.notify();
                            })),
                    )
                    .when_some(self.terminal.as_ref(), |v, terminal| {
                        let terminal = terminal.clone();
                        v.child(
                            Button::new("find-in-terminal")
                                .ghost()
                                .xsmall()
                                .icon(UiIcon::Search)
                                .tooltip("查找终端内容 ⌘F")
                                .on_click(move |_, window, cx| {
                                    terminal.update(cx, |terminal, cx| {
                                        terminal.toggle_search(window, cx)
                                    })
                                }),
                        )
                    })
                    .child(
                        Button::new("toggle-server-files")
                            .ghost()
                            .xsmall()
                            .icon(UiIcon::Folder)
                            .label(if self.files_visible {
                                "收起文件"
                            } else {
                                "显示文件"
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.files_visible = !this.files_visible;
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .when_some(self.terminal.as_ref(), |v, terminal| {
                        v.child(terminal.clone())
                    })
                    .when(self.terminal.is_none(), |v| {
                        v.child(div().p_4().text_sm().child("此连接仅启用了 SFTP 文件传输"))
                    }),
            );
        let files =
            v_flex()
                .id("server-files-pane")
                .test_support()
                .size_full()
                .min_h_0()
                .when(self.services.remote_document.is_some(), |v| {
                    v.child(
                        h_flex()
                            .h(px(32.))
                            .flex_shrink_0()
                            .px_2()
                            .items_center()
                            .gap_2()
                            .child(
                                Button::new("server-show-files")
                                    .ghost()
                                    .xsmall()
                                    .label("本地 / 远程文件")
                                    .selected(self.services.page != 2)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.services.page = 1;
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("server-show-editor")
                                    .ghost()
                                    .xsmall()
                                    .label("远程编辑")
                                    .selected(self.services.page == 2)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.services.page = 2;
                                        cx.notify();
                                    })),
                            ),
                    )
                })
                .child(div().flex_1().min_h_0().overflow_hidden().child(
                    if self.services.page == 2 {
                        self.remote_page(cx).into_any_element()
                    } else {
                        self.files_page(cx).into_any_element()
                    },
                ));
        v_flex()
            .id("server-workspace")
            .test_support()
            .size_full()
            .min_h_0()
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .when(self.monitoring.visible, |v| {
                        v.child(self.server_status_panel(cx))
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .child(if self.files_visible {
                                v_resizable("server-vertical-split")
                                    .child(
                                        resizable_panel()
                                            .size(window.viewport_size().height * 0.43)
                                            .size_range(px(130.)..Pixels::MAX)
                                            .child(terminal),
                                    )
                                    .child(
                                        resizable_panel()
                                            .size(window.viewport_size().height * 0.43)
                                            .size_range(px(190.)..Pixels::MAX)
                                            .child(files),
                                    )
                                    .into_any_element()
                            } else {
                                terminal.into_any_element()
                            }),
                    ),
            )
            .when(
                !self.files_visible
                    || self.services.page == 2
                    || self.services.sftp.is_none()
                    || self.connection_notice.is_some(),
                |v| {
                    v.child(
                        div()
                            .h(px(24.))
                            .flex_shrink_0()
                            .px_3()
                            .text_xs()
                            .text_ellipsis()
                            .text_color(if self.error || self.connection_notice.is_some() {
                                cx.theme().danger
                            } else {
                                cx.theme().muted_foreground
                            })
                            .child(
                                self.connection_notice
                                    .as_ref()
                                    .map(|notice| format!("{notice} · {}", self.status))
                                    .unwrap_or_else(|| self.status.to_string()),
                            ),
                    )
                },
            )
    }
}
