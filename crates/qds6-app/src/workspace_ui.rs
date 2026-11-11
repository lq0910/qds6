//! 功能：服务器和数据库分区导航，服务器标签承载终端及双栏文件；时间：2026-11-11；作者：lq。

use super::*;

/// 功能：按工作对象划分导航，SSH 和 SFTP 属于同一服务器区域；时间：2026-11-11；作者：lq。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum WorkspaceArea {
    Servers,
    Databases,
}
impl WorkspaceArea {
    /// 功能：兼容已有 SFTP 配置，同时将它归入服务器；时间：2026-11-11；作者：lq。
    pub(super) fn for_protocol(protocol: Protocol) -> Self {
        match protocol {
            Protocol::Ssh | Protocol::Sftp => Self::Servers,
            _ => Self::Databases,
        }
    }
    /// 功能：侧栏标题反映当前功能区；时间：2026-11-11；作者：lq。
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Servers => "服务器连接",
            Self::Databases => "数据库连接",
        }
    }
    /// 功能：新建入口遵循当前工作区；时间：2026-11-11；作者：lq。
    pub(super) fn default_protocol(self) -> Protocol {
        match self {
            Self::Servers => Protocol::Ssh,
            Self::Databases => Protocol::PostgreSql,
        }
    }
}

/// 功能：标签代表服务器、数据库查询或工具文档，不再把 SFTP 拆成会话外标签；时间：2026-11-11；作者：lq。
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum WorkspaceTarget {
    Overview,
    Connection,
    Sql,
    Server(usize),
    Database,
    Backup,
    Settings,
}

impl Workbench {
    /// 功能：切换功能区时保留服务器实体、数据库结果及输入内容；时间：2026-11-11；作者：lq。
    pub(super) fn switch_area(&mut self, area: WorkspaceArea, cx: &mut Context<Self>) {
        if self.area == area && self.tab != 5 {
            // 功能：侧栏收起后可点当前区域入口找回连接列表；时间：2026-11-11；作者：lq。
            self.sidebar_visible = true;
            cx.notify();
            return;
        }
        self.area = area;
        self.sidebar_visible = true;
        let target = match area {
            WorkspaceArea::Servers if !self.services.servers.is_empty() => {
                WorkspaceTarget::Server(self.services.active_server)
            }
            WorkspaceArea::Databases if self.services.database.is_some() => {
                WorkspaceTarget::Database
            }
            _ => WorkspaceTarget::Overview,
        };
        self.activate_workspace(target, cx);
    }

    /// 功能：获取当前文档标签身份，供滚动揭示和点击状态使用；时间：2026-11-11；作者：lq。
    fn workspace_target(&self) -> WorkspaceTarget {
        match self.tab {
            1 => WorkspaceTarget::Connection,
            2 => WorkspaceTarget::Sql,
            4 => WorkspaceTarget::Backup,
            5 => WorkspaceTarget::Settings,
            3 if self.services.page == 3 => WorkspaceTarget::Database,
            3 => WorkspaceTarget::Server(self.services.active_server),
            _ => WorkspaceTarget::Overview,
        }
    }

    /// 功能：文档切换不破坏服务器、文件和数据库状态；时间：2026-11-11；作者：lq。
    pub(super) fn activate_workspace(&mut self, target: WorkspaceTarget, cx: &mut Context<Self>) {
        match target {
            WorkspaceTarget::Overview => self.tab = 0,
            WorkspaceTarget::Connection => {
                self.area = WorkspaceArea::for_protocol(self.form.profile.protocol);
                self.form_open = true;
                self.tab = 1;
            }
            WorkspaceTarget::Sql => {
                self.area = WorkspaceArea::Databases;
                if self.services.database.is_some() {
                    self.services.query_open = true;
                    self.services.query_active = true;
                    self.services.page = 3;
                    self.tab = 3;
                } else {
                    self.sql_open = true;
                    self.tab = 2;
                }
            }
            WorkspaceTarget::Backup => {
                self.backup_open = true;
                self.tab = 4;
            }
            WorkspaceTarget::Settings => self.tab = 5,
            WorkspaceTarget::Server(index) => {
                self.area = WorkspaceArea::Servers;
                self.services.active_server = index;
                self.services.page = 0;
                self.tab = 3;
            }
            WorkspaceTarget::Database => {
                self.area = WorkspaceArea::Databases;
                self.services.page = 3;
                self.tab = 3;
            }
        }
        if !self.services.busy && !self.loading && !self.saving && !self.error {
            self.status = match target {
                WorkspaceTarget::Overview => match self.area {
                    WorkspaceArea::Servers => "服务器 · SSH 终端与 SFTP 文件同屏",
                    WorkspaceArea::Databases => "数据库 · 对象、SQL 与查询结果",
                },
                WorkspaceTarget::Connection => "编辑连接配置 · 保存不会自动连接",
                WorkspaceTarget::Sql => "SQL 草稿 · 仅保留在本次会话",
                WorkspaceTarget::Backup => "导入与备份 · 先预览，再确认写入",
                WorkspaceTarget::Settings => "设置 · 常用偏好、软件更新与关于 QDS6",
                WorkspaceTarget::Database => "数据库 · 选择对象或直接编写 SQL",
                WorkspaceTarget::Server(_) => "服务器 · 终端与双栏文件同屏，拖动分隔条调整比例",
            }
            .into();
        }
        cx.notify();
    }

    /// 功能：窄活动栏切换服务器与数据库，当前区域用背景及强调色提示；时间：2026-11-11；作者：lq。
    pub(super) fn navigation_rail(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let mut rail = v_flex()
            .w(px(64.))
            .h_full()
            .flex_shrink_0()
            .py_2()
            .gap_2()
            .border_r_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().sidebar);
        for (area, id, label, icon) in [
            (
                WorkspaceArea::Servers,
                "area-servers",
                "服务器",
                UiIcon::Server,
            ),
            (
                WorkspaceArea::Databases,
                "area-databases",
                "数据库",
                UiIcon::Database,
            ),
        ] {
            rail = rail.child(
                v_flex()
                    .id(id)
                    .test_support()
                    .mx_1()
                    .py_3()
                    .gap_1()
                    .items_center()
                    .rounded_md()
                    .cursor_pointer()
                    .bg(if self.area == area && self.tab != 5 {
                        cx.theme().secondary
                    } else {
                        cx.theme().sidebar
                    })
                    .text_color(if self.area == area && self.tab != 5 {
                        cx.theme().foreground
                    } else {
                        cx.theme().muted_foreground
                    })
                    .hover(|v| v.bg(cx.theme().secondary))
                    .child(Icon::new(icon).size(px(20.)))
                    .child(div().text_xs().child(label))
                    .on_click(cx.listener(move |this, _, _, cx| this.switch_area(area, cx))),
            );
        }
        // 功能：设置常驻活动栏底部，与业务工作区入口分开；时间：2026-11-11；作者：lq。
        rail.child(div().flex_1()).child(
            v_flex()
                .id("open-settings")
                .test_support()
                .mx_1()
                .py_3()
                .gap_1()
                .items_center()
                .rounded_md()
                .cursor_pointer()
                .bg(if self.tab == 5 {
                    cx.theme().secondary
                } else {
                    cx.theme().sidebar
                })
                .text_color(if self.tab == 5 {
                    cx.theme().foreground
                } else {
                    cx.theme().muted_foreground
                })
                .hover(|v| v.bg(cx.theme().secondary))
                .child(Icon::new(UiIcon::Settings).size(px(20.)))
                .child(div().text_xs().child("设置"))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.activate_workspace(WorkspaceTarget::Settings, cx)
                })),
        )
    }

    /// 功能：同一功能区只显示本区会话，标签过多可滚动和用溢出菜单选择；时间：2026-11-11；作者：lq。
    pub(super) fn workspace_tabs(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        use WorkspaceTarget::*;
        let current = self.workspace_target();
        let mut items = vec![(
            Overview,
            if self.area == WorkspaceArea::Servers {
                "服务器"
            } else {
                "数据库"
            }
            .to_string(),
            // 功能：概览图标标识工作对象，与相邻的侧栏收放按钮区分；时间：2026-11-11；作者：lq。
            if self.area == WorkspaceArea::Servers {
                UiIcon::Server
            } else {
                UiIcon::Database
            },
            false,
        )];
        if (self.form_open || current == Connection)
            && WorkspaceArea::for_protocol(self.form.profile.protocol) == self.area
        {
            items.push((
                Connection,
                format!("连接配置{}", if self.form_dirty { " •" } else { "" }),
                UiIcon::Server,
                true,
            ));
        }
        if self.area == WorkspaceArea::Servers {
            for (index, server) in self.services.servers.iter().enumerate() {
                let state = server.read(cx);
                items.push((
                    Server(index),
                    format!(
                        "{}{}{}",
                        if state.profile.production {
                            "生产 · "
                        } else {
                            ""
                        },
                        state.profile.name,
                        if state.services.remote_dirty {
                            " •"
                        } else {
                            ""
                        }
                    ),
                    UiIcon::Terminal,
                    true,
                ));
            }
        } else {
            if self.services.database.is_some() || current == Database {
                items.push((
                    Database,
                    if self.services.database_identity.is_empty() {
                        "数据库会话".into()
                    } else {
                        self.services.database_identity.clone()
                    },
                    UiIcon::Database,
                    true,
                ));
            }
            if self.sql_open || current == Sql {
                items.push((
                    Sql,
                    format!("SQL 草稿{}", if self.sql_dirty { " •" } else { "" }),
                    UiIcon::FileCode,
                    true,
                ));
            }
        }
        if current == Backup {
            items.push((Backup, "导入与备份".into(), UiIcon::Download, true));
        }
        if current == Settings {
            items.push((Settings, "设置".into(), UiIcon::Settings, true));
        }
        let selected = items.iter().position(|item| item.0 == current).unwrap_or(0);
        // 功能：侧栏收放后重新揭示当前标签，避免活动会话被挤出可视区；时间：2026-11-11；作者：lq。
        let identity = (
            current,
            items.len(),
            window.viewport_size().width - px(if self.sidebar_visible { 224. } else { 0. }),
        );
        if self.last_workspace_tab != Some(identity) {
            let scroll = self.workspace_scroll.clone();
            let view = cx.entity_id();
            window.on_next_frame(move |_, cx| {
                scroll.scroll_to_item(selected);
                cx.notify(view);
            });
            self.last_workspace_tab = Some(identity);
        }
        let mut bar = TabBar::new("workspace-tabs")
            .w_full()
            .min_w_0()
            .h(px(40.))
            .max_width(px(210.))
            .menu(items.len() > 3)
            .track_scroll(&self.workspace_scroll)
            .selected_index(selected);
        for (target, label, icon, closable) in items {
            let close_id = match target {
                Connection => "workspace-close-connection".to_string(),
                Sql => "workspace-close-sql".into(),
                Backup => "workspace-close-backup".into(),
                Settings => "workspace-close-settings".into(),
                Server(index) => format!("server-close-{index}"),
                Database => "workspace-close-database".into(),
                _ => String::new(),
            };
            bar = bar.child(
                Tab::new()
                    // 功能：标签填满 40px 栏高，图标及关闭按钮保留左右安全留白；时间：2026-11-11；作者：lq。
                    // Kit 默认标签高 32px，以最小高度约束避免选中背景悬空。
                    .min_h(px(40.))
                    .px_2()
                    .prefix(Icon::new(icon).small())
                    .aria_label(label.clone())
                    .label(label)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.activate_workspace(target, cx);
                        if let Server(index) = target
                            && let Some(server) = this.services.servers.get(index)
                            && let Some(terminal) = server.read(cx).terminal.as_ref()
                        {
                            window.focus(&terminal.focus_handle(cx), cx);
                        }
                    }))
                    .when(closable, |tab| {
                        tab.suffix(
                            Button::new(SharedString::from(close_id))
                                .ghost()
                                .xsmall()
                                .icon(UiIcon::X)
                                .tooltip(if matches!(target, Server(_)) {
                                    "断开此服务器"
                                } else if target == Database {
                                    "关闭数据库连接"
                                } else {
                                    "关闭标签 · 草稿保留在本次会话"
                                })
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    cx.stop_propagation();
                                    if let Server(index) = target {
                                        this.close_server(index, window, cx);
                                        return;
                                    }
                                    if target == Database {
                                        this.close_database(window, cx);
                                        return;
                                    }
                                    match target {
                                        Connection => this.form_open = false,
                                        Sql => this.sql_open = false,
                                        Backup => this.backup_open = false,
                                        _ => {}
                                    }
                                    if current == target {
                                        this.tab = 0;
                                    }
                                    cx.notify();
                                })),
                        )
                    }),
            );
        }
        h_flex()
            .id("workspace-tab-strip")
            .test_support()
            .w_full()
            .min_w_0()
            .flex_shrink_0()
            // 功能：收放按钮固定在标签栏左端，收起后仍能一键展开；时间：2026-11-11；作者：lq。
            .when(self.tab != 5, |row| {
                row.child(
                    Button::new("toggle-connection-sidebar")
                        .ghost()
                        .small()
                        .icon(UiIcon::PanelLeft)
                        .selected(self.sidebar_visible)
                        .tooltip(if self.sidebar_visible {
                            "收起连接列表"
                        } else {
                            "展开连接列表"
                        })
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.sidebar_visible = !this.sidebar_visible;
                            cx.notify();
                        })),
                )
            })
            // 功能：复用 Kit 标签栏底边，避免双重分隔线；时间：2026-11-11；作者：lq。
            .child(div().flex_1().min_w_0().child(bar))
    }

    /// 功能：按稳定实体关闭服务器，文件保存中禁止断开，脏草稿必须明确确认；时间：2026-11-11；作者：lq。
    pub(super) fn close_server(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(server) = self.services.servers.get(index).cloned() else {
            return;
        };
        if server.read(cx).pending() {
            self.status = "此服务器正在执行文件或草稿任务，请完成后再断开".into();
            cx.notify();
            return;
        }
        let dirty = server.read(cx).services.remote_dirty;
        let answer = window.prompt(
            PromptLevel::Warning,
            "断开此服务器？",
            Some(if dirty {
                "此服务器有未保存远程修改。断开会丢弃会话内草稿；请先保存或复制需要的内容。"
            } else {
                "该服务器的终端与文件连接将同时关闭，其他服务器和数据库保持连接。"
            }),
            &["取消", "断开"],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            if answer.await == Ok(1) {
                let _ = this.update_in(cx, |this, _, cx| {
                    if server.read(cx).pending() {
                        this.status = "服务器任务尚未完成，未断开".into();
                        cx.notify();
                        return;
                    }
                    if let Some(index) = this
                        .services
                        .servers
                        .iter()
                        .position(|item| item.entity_id() == server.entity_id())
                    {
                        this.services.servers.remove(index);
                        drop(this.services.server_subscriptions.remove(index));
                        if this.services.active_server > index {
                            this.services.active_server -= 1;
                        }
                        this.services.active_server = this
                            .services
                            .active_server
                            .min(this.services.servers.len().saturating_sub(1));
                        if this.services.servers.is_empty()
                            && this.tab == 3
                            && this.services.page != 3
                        {
                            this.tab = 0;
                        }
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// 功能：页面分节使用文字和细分隔，不堆叠装饰卡片；时间：2026-11-11；作者：lq。
    pub(super) fn section_heading(
        title: &'static str,
        description: &'static str,
        cx: &App,
    ) -> impl IntoElement + use<> {
        v_flex()
            .gap_1()
            .child(div().text_sm().font_semibold().child(title))
            .when(!description.is_empty(), |v| {
                v.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(description),
                )
            })
    }

    /// 功能：无连接时提供明确的后续操作，空状态居中且不显示无效工具栏；时间：2026-11-11；作者：lq。
    pub(super) fn service_empty(
        &self,
        protocol: Protocol,
        title: &'static str,
        description: &'static str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap_3()
            .p_8()
            .child(
                Icon::new(protocol_icon(protocol))
                    .size(px(32.))
                    .text_color(cx.theme().muted_foreground),
            )
            .child(div().text_lg().font_semibold().child(title))
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(description),
            )
            .child(
                Button::new("empty-new-connection")
                    .primary()
                    .small()
                    .icon(UiIcon::Plus)
                    .label("新建连接")
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_profile(ConnectionProfile::new(protocol), window, cx)
                    })),
            )
    }
}
