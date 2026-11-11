//! 功能：原生服务工作区、后台任务反馈、SFTP 编辑和数据库查询；时间：2026-11-10；作者：lq。

use super::*;
use gpui_kit::component::table::{Column, ColumnSort, DataTable, TableDelegate, TableState};
use qds6_core::TlsMode;
use qds6_services::{
    backup::{self, Configuration},
    credentials::CredentialStore,
    database::{DatabaseSession, QueryResult, ResultSet, Table},
    sftp,
    ssh::{self, SshError},
    terminal::TerminalSession,
};
use std::sync::Arc;
use zeroize::Zeroizing;

// 功能：平台选择器的路径和取消行为验收；时间：2026-11-11；作者：lq。
#[cfg(all(test, feature = "ui-tests"))]
mod private_key_tests;

/// 功能：结果网格真实数据源，Kit 仅绘制可见范围；时间：2026-11-10；作者：lq。
pub(super) struct QueryRows(ResultSet);
impl TableDelegate for QueryRows {
    fn columns_count(&self, _: &App) -> usize {
        self.0.columns.len()
    }
    fn rows_count(&self, _: &App) -> usize {
        self.0.rows.len()
    }
    fn column(&self, col: usize, _: &App) -> Column {
        Column::new(format!("result-{col}"), self.0.columns[col].clone())
            .width(px(180.))
            .min_width(px(64.))
            .sortable()
    }

    /// 功能：仅排序有界本地结果缓存，不重写或重放 SQL；时间：2026-11-10；作者：lq。
    fn perform_sort(
        &mut self,
        col: usize,
        sort: ColumnSort,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) {
        self.0.rows.sort_by(|a, b| {
            let order = a[col].display().cmp(&b[col].display());
            if sort == ColumnSort::Descending {
                order.reverse()
            } else {
                order
            }
        });
        cx.notify();
    }
    fn render_th(
        &mut self,
        col: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        div()
            .text_color(cx.theme().foreground)
            .child(self.0.columns[col].clone())
    }
    fn render_td(
        &mut self,
        row: usize,
        col: usize,
        _: &mut Window,
        _: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        div().child(self.0.rows[row][col].display())
    }
    fn cell_label(&self, row: usize, col: usize, _: &App) -> Option<SharedString> {
        Some(self.0.rows[row][col].display().into())
    }
}

/// 功能：会话实体和草稿不依赖页面切换生命周期；时间：2026-11-10；作者：lq。
pub(super) struct ServiceUi {
    pub busy: bool,
    task: Option<Task<()>>,
    // 功能：每台服务器独立实体，数据库不复用文件或终端状态；时间：2026-11-11；作者：lq。
    pub servers: Vec<Entity<crate::server_ui::ServerWorkspace>>,
    // 功能：服务器脏草稿和任务变化会更新外层标签提示；时间：2026-11-11；作者：lq。
    pub(super) server_subscriptions: Vec<Subscription>,
    pub(super) active_server: usize,
    pub(super) page: usize,
    pub(super) database: Option<Arc<DatabaseSession>>,
    pub(super) database_identity: String,
    // 功能：查询栏展示服务端实际数据库及当前模式；时间：2026-11-11；作者：lq。
    database_context: (String, String),
    // 功能：数据库上下文独立于服务器连接表单；时间：2026-11-11；作者：lq。
    database_profile: Option<ConnectionProfile>,
    results: Option<QueryResult>,
    grid: Option<Entity<TableState<QueryRows>>>,
    selected_result: usize,
    tables: Vec<Table>,
    selected_table: Option<usize>,
    // 功能：数据库对象页与多表标签独立于 SQL 草稿；时间：2026-11-11；作者：lq。
    table_tabs: Vec<table_grid::TableTab>,
    active_table_tab: Option<usize>,
    object_kind: usize,
    object_search: Entity<InputState>,
    catalog_details: bool,
    // 功能：查询只在用户打开后加入数据库内部标签；时间：2026-11-11；作者：lq。
    pub(super) query_open: bool,
    pub(super) query_active: bool,
    edit_column: Entity<InputState>,
    edit_value: Entity<InputState>,
    // 功能：默认收起写入工具，为小窗口结果网格保留空间；时间：2026-11-11；作者：lq。
    show_database_tools: bool,
    // 功能：CSV 导出路径独立于 SFTP，本地路径不再跨任务复用；时间：2026-11-11；作者：lq。
    show_csv_export: bool,
    csv_path: Entity<InputState>,
    backup_path: Entity<InputState>,
    backup_password: Entity<InputState>,
    preview: Option<Configuration>,
    import_report: Vec<String>,
    replace: bool,
    include_credentials: bool,
    // 功能：按导出、外部导入、恢复区分操作，避免危险操作与常用动作混排；时间：2026-11-11；作者：lq。
    backup_mode: usize,
    import_source: usize,
    // 功能：路径和口令变化后旧预览失效，防止恢复错文件；时间：2026-11-11；作者：lq。
    _backup_subscriptions: Vec<Subscription>,
}

impl ServiceUi {
    /// 功能：活动数据库和 SFTP 同样需要断连确认，未提交事务不会静默关闭；时间：2026-11-11；作者：lq。
    pub(super) fn has_live_sessions(&self) -> bool {
        !self.servers.is_empty() || self.database.is_some()
    }
    /// 功能：建立独立 Kit 输入和远程编辑器；时间：2026-11-10；作者：lq。
    pub fn new(window: &mut Window, cx: &mut Context<Workbench>) -> Self {
        // 功能：预览只对当前路径和解密口令有效；时间：2026-11-11；作者：lq。
        let backup_path =
            cx.new(|cx| InputState::new(window, cx).placeholder("配置 / 备份文件绝对路径"));
        let backup_password = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("备份口令（至少 8 字符）")
                .masked(true)
        });
        // 功能：搜索框修改实时更新对象列表，不在绘制中请求数据库；时间：2026-11-11；作者：lq。
        let object_search = cx.new(|cx| InputState::new(window, cx).placeholder("搜索表或视图"));
        let backup_subscriptions = [&backup_path, &backup_password]
            .into_iter()
            .map(|input| {
                cx.subscribe(input, |this, _, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.services.preview = None;
                        this.services.import_report.clear();
                        cx.notify();
                    }
                })
            })
            .collect();
        let mut backup_subscriptions: Vec<_> = backup_subscriptions;
        backup_subscriptions.push(cx.observe(&object_search, |_, _, cx| cx.notify()));
        Self {
            busy: false,
            task: None,
            servers: Vec::new(),
            server_subscriptions: Vec::new(),
            active_server: 0,
            page: 0,
            database: None,
            database_identity: String::new(),
            database_context: (String::new(), String::new()),
            database_profile: None,
            results: None,
            grid: None,
            selected_result: 0,
            tables: Vec::new(),
            selected_table: None,
            edit_column: cx
                .new(|cx| InputState::new(window, cx).placeholder("新增 / 删除字段名称")),
            edit_value: cx.new(|cx| InputState::new(window, cx).placeholder("新建 / 重命名表名称")),
            table_tabs: Vec::new(),
            active_table_tab: None,
            object_kind: 0,
            object_search,
            catalog_details: false,
            query_open: false,
            query_active: false,
            show_database_tools: false,
            show_csv_export: false,
            csv_path: cx.new(|cx| InputState::new(window, cx).placeholder("CSV 保存文件绝对路径")),
            backup_path,
            backup_password,
            preview: None,
            import_report: Vec::new(),
            replace: false,
            include_credentials: false,
            backup_mode: 0,
            import_source: 0,
            _backup_subscriptions: backup_subscriptions,
        }
    }
}

/// 功能：后台连接的成功结果或待核对指纹，不自动接受主机密钥；时间：2026-11-10；作者：lq。
enum Connected {
    Server {
        terminal: Option<TerminalSession>,
        client: Option<sftp::Client>,
        profile: ConnectionProfile,
        draft: Option<qds6_services::drafts::Draft>,
        error: Option<String>,
        // 功能：监控失败不影响终端和文件连接；时间：2026-11-11；作者：lq。
        monitor: Result<qds6_services::server_status::Monitor, String>,
    },
    Database(DatabaseSession, ConnectionProfile),
    Trust(ConnectionProfile, String, PathBuf),
}

impl Workbench {
    /// 功能：原生单文件浏览，捕获当前输入实体避免切换表单后误填其他连接；时间：2026-11-11；作者：lq。
    fn choose_private_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving || self.services.busy {
            return;
        }
        let input = self.form.private_key.clone();
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("选择 SSH 私钥文件".into()),
        });
        cx.spawn_in(window, async move |this, cx| match paths.await {
            Ok(Ok(Some(paths))) => {
                if let Some(path) = paths.first() {
                    let _ = this.update_in(cx, |this, window, cx| {
                        if this.form.private_key != input || this.saving || this.services.busy {
                            return;
                        }
                        if let Some(path) = path.to_str() {
                            input.update(cx, |input, cx| {
                                input.set_value(path.to_string(), window, cx)
                            });
                            this.form_dirty = true;
                            this.error = false;
                            this.status = "已选择私钥；如有加密请填写私钥口令".into();
                        } else {
                            this.error = true;
                            this.status = "私钥路径不是有效 UTF-8，请更换文件路径".into();
                        }
                        cx.notify();
                    });
                }
            }
            Ok(Ok(None)) => {}
            _ => {
                let _ = this.update(cx, |this, cx| {
                    this.status = "无法打开文件选择器，请重试或粘贴私钥路径".into();
                    this.error = true;
                    cx.notify();
                });
            }
        })
        .detach();
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
        if self.services.busy || self.groups.busy {
            return;
        }
        self.services.busy = true;
        self.error = false;
        self.status = label.to_string().into();
        self.services.task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = cx.background_spawn(async move { job() }).await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.services.busy = false;
                // 功能：后台失败也解除网格锁定，保留草稿供用户修正重试；时间：2026-11-11；作者：lq。
                for tab in &this.services.table_tabs {
                    tab.grid.update(cx, |grid, cx| {
                        grid.delegate_mut().locked = false;
                        cx.notify();
                    });
                }
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
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// 功能：隧道按已保存连接名或 UUID 解析，拒绝缺失、自引用及多跳；时间：2026-11-10；作者：lq。
    pub fn collect_profile(&self, cx: &App) -> Result<ConnectionProfile, String> {
        let mut profile = self.form.collect(cx)?;
        let tunnel = self.form.tunnel.read(cx).value();
        let tunnel = tunnel.trim();
        let reference = if tunnel.is_empty() {
            None
        } else {
            let matches: Vec<_> = self
                .profiles
                .iter()
                .filter(|p| {
                    p.protocol == Protocol::Ssh && (p.name == tunnel || p.id.to_string() == tunnel)
                })
                .collect();
            if matches.len() != 1 {
                return Err(
                    "跳板 / 隧道连接名不存在或不唯一，请输入已保存 SSH 连接名或 UUID".into(),
                );
            }
            let target = matches[0];
            if target.id == profile.id || target.ssh.jump_profile.is_some() {
                return Err("只支持单跳 SSH，不能引用自身或另一个跳板".into());
            }
            Some(target.id)
        };
        if matches!(profile.protocol, Protocol::Ssh | Protocol::Sftp) {
            profile.ssh.jump_profile = reference;
            profile.db.tunnel_profile = None;
        } else {
            profile.db.tunnel_profile = reference;
            profile.ssh.jump_profile = None;
        }
        profile.validate().map_err(|e| e.to_string())?;
        Ok(profile)
    }

    /// 功能：常用认证方式直接展示，认证状态使用真实复选框；时间：2026-11-11；作者：lq。
    pub fn authentication_fields(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let ssh = matches!(self.form.profile.protocol, Protocol::Ssh | Protocol::Sftp);
        v_flex()
            .gap_4()
            .child(self.field("密码 / 私钥口令", &self.form.secret))
            .when(ssh, |v| {
                v.child(
                    v_flex()
                        .gap_2()
                        .child(div().text_sm().child("私钥路径"))
                        .child(
                            h_flex()
                                .gap_2()
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .child(Input::new(&self.form.private_key)),
                                )
                                .child(
                                    Button::new("browse-private-key")
                                        .icon(UiIcon::FolderOpen)
                                        .label("浏览…")
                                        .disabled(self.saving || self.services.busy)
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.choose_private_key(window, cx)
                                        })),
                                ),
                        ),
                )
                .child(
                    Checkbox::new("use-agent")
                        .label("使用 SSH Agent")
                        .checked(self.form.profile.ssh.use_agent)
                        .disabled(self.saving || self.services.busy)
                        .on_click(cx.listener(|this, checked: &bool, _, cx| {
                            this.form.profile.ssh.use_agent = *checked;
                            this.form_dirty = true;
                            cx.notify();
                        })),
                )
            })
            .when(!ssh, |v| {
                v.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(if self.form.profile.db.tls == TlsMode::VerifyFull {
                            "TLS 已开启 · 验证证书与主机名"
                        } else {
                            "TLS 已关闭 · 请确认网络环境可信"
                        }),
                )
            })
    }

    /// 功能：低频跳板和 TLS 证书设置按需展开，协议切换不清空输入；时间：2026-11-11；作者：lq。
    pub fn advanced_authentication_fields(
        &self,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let ssh = matches!(self.form.profile.protocol, Protocol::Ssh | Protocol::Sftp);
        v_flex()
            .gap_4()
            .child(self.field(
                if ssh {
                    "跳板 SSH 连接"
                } else {
                    "SSH 隧道连接"
                },
                &self.form.tunnel,
            ))
            .when(!ssh, |v| {
                v.child(self.field("CA 证书路径", &self.form.ca_file))
                    .child(
                        Checkbox::new("database-tls")
                            .label("验证 TLS 证书与主机名")
                            .checked(self.form.profile.db.tls == TlsMode::VerifyFull)
                            .disabled(self.saving || self.services.busy)
                            .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                this.form.profile.db.tls = if *checked {
                                    TlsMode::VerifyFull
                                } else {
                                    TlsMode::Disabled
                                };
                                this.form_dirty = true;
                                cx.notify();
                            })),
                    )
            })
    }

    /// 功能：用户点击连接后后台握手和认证，最多十个终端；时间：2026-11-10；作者：lq。
    pub fn connect_service(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let profile = match self.collect_profile(cx) {
            Ok(p) => p,
            Err(e) => {
                self.status = e.into();
                self.error = true;
                cx.notify();
                return;
            }
        };
        // 功能：同数据库复用会话，切换数据库前要求关闭旧连接以保护事务和草稿；时间：2026-11-11；作者：lq。
        if matches!(profile.protocol, Protocol::MySql | Protocol::PostgreSql)
            && self.services.database.is_some()
        {
            if self
                .services
                .database_profile
                .as_ref()
                .is_some_and(|p| p == &profile)
            {
                self.activate_workspace(workspace_ui::WorkspaceTarget::Database, cx);
            } else {
                self.status = "请先关闭当前数据库连接，再连接其他数据库".into();
                cx.notify();
            }
            return;
        }
        // 功能：同一连接配置只打开一个服务器工作区，防止多个窗口竞争同一加密草稿；时间：2026-11-11；作者：lq。
        if matches!(profile.protocol, Protocol::Ssh | Protocol::Sftp)
            && let Some(index) = self
                .services
                .servers
                .iter()
                .position(|server| server.read(cx).profile.id == profile.id)
        {
            self.activate_workspace(workspace_ui::WorkspaceTarget::Server(index), cx);
            return;
        }
        if matches!(profile.protocol, Protocol::Ssh | Protocol::Sftp)
            && self.services.servers.len() >= 10
        {
            self.status = "服务器会话上限为 10 个".into();
            cx.notify();
            return;
        }
        let secret = Zeroizing::new(self.form.secret.read(cx).value().to_string());
        let storage = self.storage_path.clone();
        let tunnel_profile = profile
            .ssh
            .jump_profile
            .or(profile.db.tunnel_profile)
            .and_then(|id| self.profiles.iter().find(|p| p.id == id))
            .cloned();
        let name = format!(
            "{}{}",
            if profile.production {
                "● 生产 · "
            } else {
                ""
            },
            profile.name
        );
        self.service_job(
            "正在连接…",
            move || {
                let secret = if secret.is_empty() {
                    if let Some(reference) = &profile.credential {
                        CredentialStore::get(reference)?
                    } else {
                        secret
                    }
                } else {
                    secret
                };
                let path = storage.map(Ok).unwrap_or_else(Self::store_path)?;
                let known = path.with_file_name("known-hosts.json");
                let tunnel = if let Some(jump) = tunnel_profile {
                    let jump_secret = if let Some(reference) = &jump.credential {
                        CredentialStore::get(reference)?
                    } else {
                        Zeroizing::new(String::new())
                    };
                    let session = match ssh::connect(&jump, Some(&jump_secret), &known) {
                        Ok(session) => session,
                        Err(SshError::UnknownHost { fingerprint }) => {
                            return Ok(Connected::Trust(jump, fingerprint, known));
                        }
                        Err(error) => return Err(error.to_string()),
                    };
                    Some(Arc::new(ssh::TunnelLease::start(
                        session,
                        profile.host.clone(),
                        profile.port,
                    )?))
                } else {
                    None
                };
                match profile.protocol {
                    Protocol::Ssh | Protocol::Sftp => {
                        // 功能：终端和文件使用独立 SSH 传输，文件 IO 不阻塞终端读写；时间：2026-11-11；作者：lq。
                        let connect = || {
                            if let Some(tunnel) = &tunnel {
                                ssh::tcp_connect("127.0.0.1", tunnel.port).and_then(|socket| {
                                    ssh::connect_socket(&profile, Some(&secret), &known, socket)
                                })
                            } else {
                                ssh::connect(&profile, Some(&secret), &known)
                            }
                        };
                        let session = match connect() {
                            Ok(session) => session,
                            Err(SshError::UnknownHost { fingerprint }) => {
                                return Ok(Connected::Trust(profile, fingerprint, known));
                            }
                            Err(error) => return Err(error.to_string()),
                        };
                        let (terminal, client, mut error) = if profile.protocol == Protocol::Ssh {
                            let terminal = TerminalSession::start(session, 100, 30)?
                                .with_tunnel(tunnel.clone());
                            match connect()
                                .map_err(|e| e.to_string())
                                .and_then(sftp::Client::new)
                            {
                                Ok(client) => (
                                    Some(terminal),
                                    Some(client.with_tunnel(tunnel.clone())),
                                    None,
                                ),
                                Err(error) => (
                                    Some(terminal),
                                    None,
                                    Some(format!("终端已连接，SFTP 不可用：{error}")),
                                ),
                            }
                        } else {
                            (
                                None,
                                Some(sftp::Client::new(session)?.with_tunnel(tunnel.clone())),
                                None,
                            )
                        };
                        // 功能：采集使用独立认证连接，失败只展示状态错误；时间：2026-11-11；作者：lq。
                        let monitor = connect().map_err(|e| e.to_string()).map(|session| {
                            qds6_services::server_status::Monitor::start(session, tunnel.clone())
                        });
                        let draft = match qds6_services::drafts::load(
                            &path.with_file_name("drafts"),
                            profile.id,
                        ) {
                            Ok(draft) => draft,
                            Err(draft_error) => {
                                error = Some(draft_error);
                                None
                            }
                        };
                        Ok(Connected::Server {
                            terminal,
                            client,
                            profile,
                            draft,
                            error,
                            monitor,
                        })
                    }

                    _ => DatabaseSession::connect(&profile, &secret, tunnel)
                        .map(|database| Connected::Database(database, profile)),
                }
            },
            move |this, connected, window, cx| match connected {
                Connected::Server {
                    terminal,
                    client,
                    profile,
                    draft,
                    error,
                    monitor,
                } => {
                    let storage = this.storage_path.clone();
                    let server = cx.new(|cx| {
                        crate::server_ui::ServerWorkspace::new(
                            profile, terminal, client, storage, window, cx,
                        )
                    });
                    server.update(cx, |server, cx| {
                        server.attach_monitor(monitor, cx);
                        server.initialize(draft, error, window, cx);
                    });
                    // 功能：新连接继承保存的终端字号；时间：2026-11-11；作者：lq。
                    if let Some(terminal) = server.read(cx).terminal.clone() {
                        terminal.update(cx, |terminal, cx| {
                            terminal
                                .set_font_size(this.settings.preferences.terminal_font_size, cx);
                            terminal.set_theme(this.resolved_terminal_theme(cx), cx);
                        });
                    }
                    if let Some(terminal) = server.read(cx).terminal.as_ref() {
                        window.focus(&terminal.focus_handle(cx), cx);
                    }
                    this.services
                        .server_subscriptions
                        .push(cx.observe(&server, |_, _, cx| cx.notify()));
                    this.services.servers.push(server);
                    this.services.active_server = this.services.servers.len() - 1;
                    this.area = workspace_ui::WorkspaceArea::Servers;
                    this.services.page = 0;
                    this.tab = 3;
                    this.status = "服务器已连接 · 上方终端，下方本地与远程文件".into();
                }
                Connected::Database(database, profile) => {
                    this.services.database_profile = Some(profile);
                    this.area = workspace_ui::WorkspaceArea::Databases;
                    this.services.database = Some(Arc::new(database));
                    this.services.database_identity = name.clone();
                    this.services.table_tabs.clear();
                    this.services.active_table_tab = None;
                    this.services.query_active = false;
                    this.services.tables.clear();
                    this.services.selected_table = None;
                    this.services.results = None;
                    this.services.grid = None;
                    this.services.page = 3;
                    this.tab = 3;
                    this.status = format!("{name} · 数据库已连接").into();
                    // 功能：连接完成后自动读取对象目录，不执行用户草稿；时间：2026-11-11；作者：lq。
                    this.load_tables(window, cx);
                }
                Connected::Trust(profile, fingerprint, path) => {
                    let text = format!(
                        "{}:{}\n{}\n请通过可信渠道核对。密钥变化时将阻止连接。",
                        profile.host, profile.port, fingerprint
                    );
                    let answer = window.prompt(
                        PromptLevel::Warning,
                        "首次连接：信任此 SSH 主机？",
                        Some(&text),
                        &["取消", "信任并连接"],
                        cx,
                    );
                    cx.spawn_in(window, async move |this, cx| {
                        if answer.await == Ok(1) {
                            let _ = this.update_in(cx, |this, window, cx| {
                                this.service_job(
                                    "正在保存已确认的主机指纹…",
                                    move || {
                                        ssh::trust_host(&path, &profile, &fingerprint)
                                            .map_err(|e| e.to_string())
                                    },
                                    |this, _, window, cx| this.connect_service(window, cx),
                                    window,
                                    cx,
                                );
                            });
                        }
                    })
                    .detach();
                }
            },
            window,
            cx,
        );
    }

    /// 功能：执行选区 SQL 或完整编辑器内容，任意 SQL 不静默改写；时间：2026-11-10；作者：lq。
    pub fn execute_sql(
        &mut self,
        override_sql: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(database) = self.services.database.clone() else {
            self.status = "请先连接 MySQL 或 PostgreSQL".into();
            cx.notify();
            return;
        };
        let text = self.sql.read(cx).value();
        let range = self.sql.read(cx).selected_range();
        let sql = override_sql.unwrap_or_else(|| {
            if range.is_empty() {
                text.to_string()
            } else {
                text.get(range).unwrap_or(&text).to_string()
            }
        });
        // 功能：结构操作完成后刷新对象列表，查询文本仍保持原样执行；时间：2026-11-11；作者：lq。
        let refresh_objects = sql.split(|c: char| !c.is_ascii_alphabetic()).any(|word| {
            ["CREATE", "ALTER", "DROP", "TRUNCATE"]
                .iter()
                .any(|w| word.eq_ignore_ascii_case(w))
        });
        self.service_job(
            "正在执行 SQL…",
            move || {
                let result = database.query(&sql)?;
                let context = database.connection_context().ok();
                Ok((result, context))
            },
            move |this, (result, context), window, cx| {
                if let Some(context) = context {
                    this.services.database_context = context;
                }
                this.status = format!(
                    "{} 个结果集 · {} ms{}",
                    result.sets.len(),
                    result.elapsed_ms,
                    if result.truncated {
                        " · 达到结果上限，已取消服务端查询"
                    } else {
                        ""
                    }
                )
                .into();
                this.services.grid = result
                    .sets
                    .first()
                    .filter(|s| !s.columns.is_empty())
                    .map(|set| cx.new(|cx| TableState::new(QueryRows(set.clone()), window, cx)));
                this.services.results = Some(result);
                this.services.selected_result = 0;
                // 功能：查询结果留在当前 SQL 编辑区，后台完成不抢走其他标签；时间：2026-11-11；作者：lq。
                if refresh_objects {
                    this.load_tables(window, cx);
                }
            },
            window,
            cx,
        );
    }

    /// 功能：查询取消在独立后台任务中联系服务端，不等待查询锁；时间：2026-11-10；作者：lq。
    pub(super) fn cancel_query(&mut self, cx: &mut Context<Self>) {
        let Some(database) = self.services.database.clone() else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let result = cx.background_spawn(async move { database.cancel() }).await;
            let _ = this.update(cx, |this, cx| {
                this.status = match result {
                    Ok(()) => "服务端已确认取消请求".into(),
                    Err(e) => e.into(),
                };
                cx.notify();
            });
        })
        .detach();
    }

    /// 功能：服务器实体自持终端与文件，数据库使用独立查询布局；时间：2026-11-11；作者：lq。
    pub fn services_page(
        &self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        if self.services.page == 3 {
            return self.database_page(cx).into_any_element();
        }
        if let Some(server) = self.services.servers.get(self.services.active_server) {
            return div().size_full().child(server.clone()).into_any_element();
        }
        self.service_empty(
            Protocol::Ssh,
            "尚未连接服务器",
            "连接后，终端与本地 / 远程文件会同时显示。",
            cx,
        )
        .into_any_element()
    }

    /// 功能：连接后自动加载及手动刷新数据库对象目录；时间：2026-11-11；作者：lq。
    fn load_tables(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(database) = self.services.database.clone() else {
            return;
        };
        self.service_job(
            "正在读取数据库对象…",
            move || {
                let tables = database.tables()?;
                let context = database.connection_context()?;
                Ok((tables, context))
            },
            |this, (tables, context), _, _| {
                this.services.database_context = context;
                let selected = this
                    .services
                    .selected_table
                    .and_then(|i| this.services.tables.get(i))
                    .cloned();
                this.services.tables = tables;
                this.services.selected_table = selected.and_then(|old| {
                    this.services
                        .tables
                        .iter()
                        .position(|t| t.schema == old.schema && t.name == old.name)
                });
                this.status = format!("{} 个数据库对象", this.services.tables.len()).into();
            },
            window,
            cx,
        );
    }

    /// 功能：选中对象打开独立表标签，字段信息在该表详情内显示；时间：2026-11-11；作者：lq。
    fn table_operation(&mut self, structure: bool, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(table) = self
            .services
            .selected_table
            .and_then(|i| self.services.tables.get(i))
            .cloned()
        {
            self.open_table(table, structure, window, cx);
        }
    }

    /// 功能：生成基础 DDL 预览到 Kit 编辑器，用户审阅后显式执行；时间：2026-11-10；作者：lq。
    fn preview_ddl(&mut self, create: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(profile) = self.services.database_profile.as_ref() else {
            return;
        };
        let protocol = profile.protocol;
        let result = (|| {
            let quote = |name: &str| qds6_services::database::identifier(protocol, name);
            if create {
                let schema = if protocol == Protocol::MySql {
                    profile.database.clone()
                } else {
                    "public".into()
                };
                Ok::<_, String>(format!(
                    "CREATE TABLE {}.{} (id BIGINT PRIMARY KEY, value TEXT);",
                    quote(&schema)?,
                    quote(self.services.edit_value.read(cx).value().as_str())?
                ))
            } else {
                let table = self
                    .services
                    .selected_table
                    .and_then(|i| self.services.tables.get(i))
                    .ok_or("请先选择表")?;
                Ok(format!(
                    "ALTER TABLE {}.{} ADD COLUMN {} TEXT;",
                    quote(&table.schema)?,
                    quote(&table.name)?,
                    quote(self.services.edit_column.read(cx).value().as_str())?
                ))
            }
        })();
        match result {
            Ok(sql) => {
                self.show_generated_sql(sql, window, cx);
            }
            Err(error) => {
                self.status = error.into();
                self.error = true;
            }
        }
        cx.notify();
    }
}

// 功能：真实 Metal 与专用服务的联合验收，仅显式 feature 编译；时间：2026-11-10；作者：lq。
#[cfg(feature = "ui-tests")]
mod smoke;
#[cfg(feature = "ui-tests")]
pub use smoke::run_service_ui_smoke;

// 功能：迁移与备份页面独立布局，复用原有后台服务边界；时间：2026-11-11；作者：lq。
mod backup_ui;

// 功能：数据库对象和查询结果的统一布局；时间：2026-11-11；作者：lq。
mod database_ui;

// 功能：布局样例只在显式验收构建中可用，不进入发行程序；时间：2026-11-11；作者：lq。
#[cfg(feature = "ui-tests")]
mod layout_smoke;
#[cfg(feature = "ui-tests")]
pub use layout_smoke::run_layout_smoke;

// 功能：原生多表数据网格与独立草稿；时间：2026-11-11；作者：lq。
mod table_grid;
// 功能：隔离数据库原生验收入口；时间：2026-11-11；作者：lq。
#[cfg(feature = "ui-tests")]
pub use table_grid::smoke::run_database_grid_smoke;
