//! 功能：快速打开、远程目录书签和本地加密 SQL 资料；时间：2026-11-27；作者：lq。

use super::*;
use gpui_kit::component::{
    WindowExt,
    menu::{DropdownMenu, PopupMenuItem},
};
use qds6_services::local_tools::{self, Bookmark, LocalTools, SqlEntry};
use std::time::Duration;
use uuid::Uuid;

/// 功能：本机数据串行写入，SQL 草稿使用独立延时任务；时间：2026-11-27；作者：lq。
#[derive(Default)]
pub(super) struct ToolsUi {
    data: LocalTools,
    ready: bool,
    pending: bool,
    warning: Option<String>,
    write: Option<Task<()>>,
    draft_timer: Option<Task<()>>,
    pub(super) pending_directory: Option<Bookmark>,
}

impl ToolsUi {
    /// 功能：关闭窗口前等待已经发起的本地写入；时间：2026-11-27；作者：lq。
    pub(super) fn saving(&self) -> bool {
        self.write.is_some()
    }
}

/// 功能：快捷结果绑定稳定连接身份，也支持待打开的路径和 SQL；时间：2026-11-27；作者：lq。
#[derive(Clone)]
enum Target {
    Connection(Uuid),
    Directory(Bookmark),
    Sql(Uuid),
}

/// 功能：快捷窗口负责输入、结果过滤和键盘确认；时间：2026-11-27；作者：lq。
struct QuickDialog {
    input: Entity<InputState>,
    workbench: WeakEntity<Workbench>,
    results: Vec<(String, String, Target)>,
    _subscription: Subscription,
}

impl QuickDialog {
    /// 功能：连接名、主机路径及 SQL 标题用同一查询匹配；时间：2026-11-27；作者：lq。
    fn matches(&self, cx: &App) -> impl Iterator<Item = &(String, String, Target)> {
        let query = self.input.read(cx).value().trim().to_lowercase();
        self.results.iter().filter(move |(title, detail, _)| {
            title.to_lowercase().contains(&query) || detail.to_lowercase().contains(&query)
        })
    }

    /// 功能：选择前关闭列表，把焦点交给对应工作区；时间：2026-11-27；作者：lq。
    fn activate(&self, target: Target, window: &mut Window, cx: &mut App) {
        window.close_dialog(cx);
        let _ = self
            .workbench
            .update(cx, |this, cx| this.open_quick_target(target, window, cx));
    }
}

impl Render for QuickDialog {
    /// 功能：显示有限数量的匹配项，回车默认打开首条；时间：2026-11-27；作者：lq。
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rows = self.matches(cx).take(30).cloned().collect::<Vec<_>>();
        v_flex()
            .gap_2()
            .child(
                Input::new(&self.input)
                    .id("quick-search")
                    .aria_label("搜索连接、目录书签、SQL 收藏"),
            )
            .child(
                div()
                    .id("quick-results")
                    .max_h(px(328.))
                    .overflow_y_scroll()
                    .when(rows.is_empty(), |v| v.child(div().p_3().child("暂无匹配")))
                    .children(rows.into_iter().enumerate().map(
                        |(index, (title, detail, target))| {
                            v_flex()
                                .id(("quick-result", index))
                                .px_2()
                                .py_1()
                                .rounded_md()
                                .cursor_pointer()
                                .hover(|v| v.bg(cx.theme().secondary))
                                .child(div().truncate().child(title))
                                .child(
                                    div()
                                        .truncate()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(detail),
                                )
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.activate(target.clone(), window, cx)
                                }))
                        },
                    )),
            )
    }
}

/// 功能：收藏及历史行使用首个非空 SQL 语句行，限制菜单宽度；时间：2026-11-27；作者：lq。
fn sql_title(sql: &str) -> String {
    sql.lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with("--"))
        .unwrap_or("SQL")
        .chars()
        .take(64)
        .collect()
}

impl Workbench {
    /// 功能：快速工具文件与连接配置同目录，凭据仍在系统凭据库；时间：2026-11-27；作者：lq。
    fn tools_path(storage: Option<PathBuf>) -> Result<PathBuf, String> {
        Ok(storage
            .map(Ok)
            .unwrap_or_else(Self::store_path)?
            .with_file_name("local-tools.qds6"))
    }

    /// 功能：后台恢复 SQL 和快捷入口，启动期间已录入的内容优先；时间：2026-11-27；作者：lq。
    pub(super) fn load_local_tools(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let storage = self.storage_path.clone();
        cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_spawn(async move { local_tools::load(&Self::tools_path(storage)?) })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                match result {
                    Ok(data) => {
                        if !this.sql_dirty
                            && let Some(draft) = &data.draft
                        {
                            this.sql.update(cx, |editor, cx| {
                                editor.set_value(draft.clone(), window, cx)
                            });
                            this.sql_dirty = false;
                        }
                        this.local_tools.data = data;
                        this.local_tools.ready = true;
                        if this.sql_dirty {
                            this.queue_sql_draft(cx);
                        }
                    }
                    Err(error) => this.local_tools.warning = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// 功能：序列化本地写入，任务运行时的新变更在本次完成后继续保存；时间：2026-11-27；作者：lq。
    fn save_tools(&mut self, cx: &mut Context<Self>) {
        self.local_tools.pending = true;
        if !self.local_tools.ready || self.local_tools.write.is_some() {
            return;
        }
        self.local_tools.pending = false;
        let storage = self.storage_path.clone();
        let data = self.local_tools.data.clone();
        let draft = data.draft.clone();
        self.local_tools.write = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(
                    async move { local_tools::save(&Self::tools_path(storage)?, &data) },
                )
                .await;
            let _ = this.update(cx, |this, cx| {
                this.local_tools.write = None;
                this.local_tools.warning = result.err();
                if this.local_tools.warning.is_none() {
                    if draft.as_deref() == Some(this.sql.read(cx).value().as_str()) {
                        this.sql_dirty = false;
                    }
                    if this.local_tools.pending {
                        this.save_tools(cx);
                    }
                } else {
                    this.status = "本机书签、SQL 或历史保存失败；当前内容仍可使用".into();
                    this.error = true;
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// 功能：等输入停顿后写入 SQL 草稿，连续输入期间不重复加密；时间：2026-11-27；作者：lq。
    pub(super) fn queue_sql_draft(&mut self, cx: &mut Context<Self>) {
        if !self.local_tools.ready {
            return;
        }
        self.local_tools.draft_timer = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(500))
                .await;
            let _ = this.update(cx, |this, cx| this.flush_sql_draft(cx));
        }));
    }

    /// 功能：窗口关闭前提交还在延迟队列里的草稿；时间：2026-11-27；作者：lq。
    pub(super) fn flush_sql_draft(&mut self, cx: &mut Context<Self>) {
        if self.local_tools.draft_timer.take().is_none() {
            return;
        }
        let sql = self.sql.read(cx).value().to_string();
        if sql.len() > local_tools::MAX_DRAFT {
            self.status = "SQL 超过加密草稿的 512 KiB 上限，请复制另存".into();
            self.error = true;
            cx.notify();
        } else if self.local_tools.data.draft.as_ref() != Some(&sql) {
            self.local_tools.data.draft = Some(sql);
            self.save_tools(cx);
        }
    }

    /// 功能：SQL 编辑区的保存快捷键立即写入仍在等待的草稿；时间：2026-11-27；作者：lq。
    pub(super) fn save_sql_now(&mut self, cx: &mut Context<Self>) {
        if !self.local_tools.ready {
            self.status = self
                .local_tools
                .warning
                .clone()
                .unwrap_or_else(|| "正在读取本机 SQL 草稿".into())
                .into();
        } else if self.local_tools.draft_timer.is_some() {
            self.flush_sql_draft(cx);
        } else if self.local_tools.warning.is_some() {
            self.save_tools(cx);
        } else {
            self.status = "SQL 草稿会自动保存".into();
        }
        cx.notify();
    }

    /// 功能：只记录用于快速定位的已保存连接，最近项去重并有界；时间：2026-11-27；作者：lq。
    pub(super) fn remember_connection(&mut self, id: Uuid, cx: &mut Context<Self>) {
        if !self.local_tools.ready
            || !self.profiles.iter().any(|profile| profile.id == id)
            || self.local_tools.data.recent.first() == Some(&id)
        {
            return;
        }
        self.local_tools.data.recent.retain(|recent| *recent != id);
        self.local_tools.data.recent.insert(0, id);
        self.local_tools.data.recent.truncate(20);
        self.save_tools(cx);
    }

    /// 功能：同一服务器下按目录保留多个书签；时间：2026-11-27；作者：lq。
    fn active_directory_bookmark(&self, cx: &App) -> Option<Bookmark> {
        if self.area != workspace_ui::WorkspaceArea::Servers || self.tab != 3 {
            return None;
        }
        let server = self
            .services
            .servers
            .get(self.services.active_server)?
            .read(cx);
        self.profiles.iter().find(|p| p.id == server.profile.id)?;
        Some(Bookmark {
            connection: server.profile.id,
            path: server.remote_bookmark_path()?,
        })
    }

    /// 功能：标题栏提供快捷搜索与当前目录收藏入口；时间：2026-11-27；作者：lq。
    pub(super) fn quick_open_button(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let bookmark = self.active_directory_bookmark(cx);
        h_flex()
            .gap_1()
            .child(
                Button::new("quick-open")
                    .ghost()
                    .small()
                    .icon(UiIcon::Search)
                    .label("快速打开")
                    .tooltip(if cfg!(target_os = "macos") {
                        "快速打开 ⌘K"
                    } else {
                        "快速打开 Ctrl+K"
                    })
                    .on_click(
                        cx.listener(|this, _, window, cx| this.open_quick_dialog(window, cx)),
                    ),
            )
            .when_some(bookmark, |v, bookmark| {
                let saved = self.local_tools.data.bookmarks.contains(&bookmark);
                v.child(
                    Button::new("directory-bookmark")
                        .ghost()
                        .small()
                        .label(if saved {
                            "取消目录书签"
                        } else {
                            "收藏当前目录"
                        })
                        .disabled(!self.local_tools.ready)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let bookmarks = &mut this.local_tools.data.bookmarks;
                            if bookmarks.contains(&bookmark) {
                                bookmarks.retain(|entry| entry != &bookmark);
                            } else {
                                bookmarks.insert(0, bookmark.clone());
                                bookmarks.truncate(100);
                            }
                            this.save_tools(cx);
                        })),
                )
            })
    }

    /// 功能：按最近连接、目录书签、SQL 收藏和其余连接建立搜索候选；时间：2026-11-27；作者：lq。
    pub(super) fn open_quick_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if window.has_active_dialog(cx) {
            return;
        }
        let describe = |p: &ConnectionProfile| {
            format!(
                "{} · {}:{} · {}",
                if p.production {
                    "生产"
                } else {
                    p.protocol.label()
                },
                p.host,
                p.port,
                p.group
            )
        };
        let connection =
            |p: &ConnectionProfile| (p.name.clone(), describe(p), Target::Connection(p.id));
        let mut results = self
            .local_tools
            .data
            .recent
            .iter()
            .filter_map(|id| self.profiles.iter().find(|p| p.id == *id))
            .map(connection)
            .collect::<Vec<_>>();
        for bookmark in &self.local_tools.data.bookmarks {
            if let Some(p) = self.profiles.iter().find(|p| p.id == bookmark.connection) {
                results.push((
                    bookmark.path.clone(),
                    format!("目录 · {} · {}", p.name, describe(p)),
                    Target::Directory(bookmark.clone()),
                ));
            }
        }
        for entry in &self.local_tools.data.favorites {
            results.push((
                sql_title(&entry.sql),
                "SQL 收藏".into(),
                Target::Sql(entry.id),
            ));
        }
        results.extend(
            self.profiles
                .iter()
                .filter(|p| !self.local_tools.data.recent.contains(&p.id))
                .map(connection),
        );
        let input =
            cx.new(|cx| InputState::new(window, cx).placeholder("连接、远程目录、SQL 收藏"));
        let workbench = cx.entity().downgrade();
        let dialog_input = input.clone();
        let view = cx.new(|cx: &mut Context<QuickDialog>| {
            let subscription = cx.subscribe_in(
                &dialog_input,
                window,
                |this, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::PressEnter { .. }) {
                        let target = this.matches(cx).next().map(|(_, _, target)| target.clone());
                        if let Some(target) = target {
                            this.activate(target, window, cx);
                        }
                    } else if matches!(event, InputEvent::Change) {
                        cx.notify();
                    }
                },
            );
            QuickDialog {
                input: dialog_input,
                workbench,
                results,
                _subscription: subscription,
            }
        });
        window.open_dialog(cx, move |dialog, _, _| {
            dialog.title("快速打开").w(px(560.)).child(view.clone())
        });
        input.update(cx, |input, cx| input.focus(window, cx));
    }

    /// 功能：调用已有连接确认、会话切换和 SQL 编辑入口；时间：2026-11-27；作者：lq。
    fn open_quick_target(&mut self, target: Target, window: &mut Window, cx: &mut Context<Self>) {
        match target {
            Target::Connection(id) => {
                self.local_tools.pending_directory = None;
                if let Some(profile) = self.profiles.iter().find(|p| p.id == id).cloned() {
                    self.open_profile_action(profile, true, window, cx);
                }
            }
            Target::Directory(bookmark) => {
                if let Some(index) = self
                    .services
                    .servers
                    .iter()
                    .position(|server| server.read(cx).profile.id == bookmark.connection)
                {
                    self.activate_workspace(workspace_ui::WorkspaceTarget::Server(index), cx);
                    self.services.servers[index].update(cx, |server, cx| {
                        server.open_bookmark(bookmark.path.into(), window, cx)
                    });
                } else if let Some(profile) = self
                    .profiles
                    .iter()
                    .find(|p| p.id == bookmark.connection)
                    .cloned()
                {
                    self.open_profile_bookmark(profile, bookmark, window, cx);
                }
            }
            Target::Sql(id) => {
                if let Some(sql) = self
                    .local_tools
                    .data
                    .favorites
                    .iter()
                    .find(|entry| entry.id == id)
                    .map(|entry| entry.sql.clone())
                {
                    self.load_saved_sql(sql, window, cx);
                }
            }
        }
    }

    /// 功能：执行历史按连接记载尝试运行的 SQL，生产连接默认关闭；时间：2026-11-27；作者：lq。
    pub(super) fn remember_sql(&mut self, sql: &str, cx: &mut Context<Self>) {
        let Some(profile) = self.services.database_profile.as_ref() else {
            return;
        };
        if !self.local_tools.ready
            || !self.history_active(profile)
            || sql.trim().is_empty()
            || sql.len() > local_tools::MAX_SNIPPET
        {
            return;
        }
        let id = profile.id;
        let entries = &mut self.local_tools.data.history;
        if entries
            .first()
            .is_some_and(|entry| entry.connection == Some(id) && entry.sql == sql)
        {
            return;
        }
        entries.insert(
            0,
            SqlEntry {
                id: Uuid::new_v4(),
                connection: Some(id),
                sql: sql.into(),
            },
        );
        entries.truncate(50);
        self.save_tools(cx);
    }

    /// 功能：生产连接缺少显式偏好时关闭 SQL 历史；时间：2026-11-27；作者：lq。
    fn history_active(&self, profile: &ConnectionProfile) -> bool {
        self.local_tools
            .data
            .history_enabled
            .get(&profile.id)
            .copied()
            .unwrap_or(!profile.production)
    }

    /// 功能：从历史或收藏打开 SQL，确认替换现有编辑内容且不自动运行；时间：2026-11-27；作者：lq。
    fn load_saved_sql(&mut self, sql: String, window: &mut Window, cx: &mut Context<Self>) {
        let current = self.sql.read(cx).value().to_string();
        if current != sql && !current.trim().is_empty() && current != INITIAL_SQL {
            let answer = window.prompt(
                PromptLevel::Warning,
                "替换当前 SQL？",
                Some("可先将当前编辑内容加入收藏。"),
                &["取消", "替换"],
                cx,
            );
            cx.spawn_in(window, async move |this, cx| {
                if answer.await == Ok(1) {
                    let _ = this.update_in(cx, |this, window, cx| {
                        if this.sql.read(cx).value().as_str() == current {
                            this.apply_saved_sql(sql, window, cx);
                        }
                    });
                }
            })
            .detach();
        } else {
            self.apply_saved_sql(sql, window, cx);
        }
    }

    /// 功能：恢复文本到当前查询编辑器并排队保存；时间：2026-11-27；作者：lq。
    fn apply_saved_sql(&mut self, sql: String, window: &mut Window, cx: &mut Context<Self>) {
        self.sql
            .update(cx, |editor, cx| editor.set_value(sql, window, cx));
        if self.services.database.is_some() {
            self.services.query_active = true;
        }
        self.activate_workspace(workspace_ui::WorkspaceTarget::Sql, cx);
        self.sql_dirty = true;
        self.queue_sql_draft(cx);
    }

    /// 功能：两处 SQL 编辑入口共用收藏、按库历史及清理控制；时间：2026-11-27；作者：lq。
    pub(super) fn sql_library_button(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let view = cx.entity();
        let profile = self
            .services
            .database_profile
            .as_ref()
            .map(|profile| (profile.id, self.history_active(profile)));
        let favorites = self
            .local_tools
            .data
            .favorites
            .iter()
            .map(|entry| (sql_title(&entry.sql), entry.id))
            .collect::<Vec<_>>();
        let history = self
            .local_tools
            .data
            .history
            .iter()
            .filter(|entry| profile.is_some_and(|(id, _)| entry.connection == Some(id)))
            .map(|entry| (sql_title(&entry.sql), entry.id))
            .collect::<Vec<_>>();
        Button::new("sql-library")
            .ghost()
            .small()
            .label("SQL 收藏 / 历史")
            .tooltip(
                self.local_tools
                    .warning
                    .clone()
                    .unwrap_or_else(|| "保存 SQL 收藏和按连接查看历史".into()),
            )
            .disabled(!self.local_tools.ready)
            .dropdown_menu(move |mut menu, _, _| {
                menu = menu.item(PopupMenuItem::new("收藏当前 SQL").on_click({
                    let view = view.clone();
                    move |_, _, cx| view.update(cx, |this, cx| this.favorite_current_sql(cx))
                }));
                for (title, id) in &favorites {
                    let (title, id) = (title.clone(), *id);
                    let open = view.clone();
                    let remove = view.clone();
                    menu = menu
                        .item(PopupMenuItem::new(format!("收藏 · {title}")).on_click(
                            move |_, window, cx| {
                                open.update(cx, |this, cx| {
                                    this.open_quick_target(Target::Sql(id), window, cx)
                                });
                            },
                        ))
                        .item(PopupMenuItem::new(format!("取消收藏 · {title}")).on_click(
                            move |_, _, cx| {
                                remove.update(cx, |this, cx| {
                                    this.local_tools
                                        .data
                                        .favorites
                                        .retain(|entry| entry.id != id);
                                    this.save_tools(cx);
                                });
                            },
                        ));
                }
                if let Some((connection, enabled)) = profile {
                    menu = menu.item(
                        PopupMenuItem::new(if enabled {
                            "关闭此连接的历史"
                        } else {
                            "开启此连接的历史"
                        })
                        .on_click({
                            let view = view.clone();
                            move |_, _, cx| {
                                view.update(cx, |this, cx| {
                                    this.local_tools
                                        .data
                                        .history_enabled
                                        .insert(connection, !enabled);
                                    this.save_tools(cx);
                                })
                            }
                        }),
                    );
                    if !history.is_empty() {
                        menu = menu.item(PopupMenuItem::new("清空此连接历史").on_click({
                            let view = view.clone();
                            move |_, _, cx| {
                                view.update(cx, |this, cx| {
                                    this.local_tools
                                        .data
                                        .history
                                        .retain(|entry| entry.connection != Some(connection));
                                    this.save_tools(cx);
                                })
                            }
                        }));
                    }
                    for (title, id) in &history {
                        let id = *id;
                        let view = view.clone();
                        menu = menu.item(PopupMenuItem::new(format!("历史 · {title}")).on_click(
                            move |_, window, cx| {
                                view.update(cx, |this, cx| {
                                    if let Some(sql) = this
                                        .local_tools
                                        .data
                                        .history
                                        .iter()
                                        .find(|entry| entry.id == id)
                                        .map(|entry| entry.sql.clone())
                                    {
                                        this.load_saved_sql(sql, window, cx);
                                    }
                                });
                            },
                        ));
                    }
                }
                menu
            })
    }

    /// 功能：显式收藏当前编辑内容，有界保存并避免同文重复；时间：2026-11-27；作者：lq。
    pub(super) fn favorite_current_sql(&mut self, cx: &mut Context<Self>) {
        let sql = self.sql.read(cx).value().to_string();
        let favorites = &mut self.local_tools.data.favorites;
        if favorites.iter().any(|entry| entry.sql == sql) {
            return;
        }
        if sql.trim().is_empty() || sql.len() > local_tools::MAX_SNIPPET || favorites.len() >= 50 {
            self.status = "收藏需为非空 SQL，单条上限 64 KiB，最多 50 条".into();
            self.error = true;
            cx.notify();
            return;
        }
        favorites.insert(
            0,
            SqlEntry {
                id: Uuid::new_v4(),
                connection: self.services.database_profile.as_ref().map(|p| p.id),
                sql,
            },
        );
        self.save_tools(cx);
    }
}
