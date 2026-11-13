//! 功能：QDS6 原生工作台、非秘密连接管理与会话内 SQL 草稿；时间：2026-11-10；作者：lq。

use directories::BaseDirs;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    component::{
        ActiveTheme, Disableable, Icon, Selectable, Sizable, StyledExt, Theme, ThemeMode, TitleBar,
        button::{Button, ButtonVariants},
        checkbox::Checkbox,
        h_flex,
        input::{Editor, EditorState, Input, InputEvent, InputState},
        tab::{Tab, TabBar},
        v_flex,
    },
    *,
};
use services_ui::database_ui::sql_completion::{SqlCompletionProvider, TableNames};
use qds6_core::{ConnectionProfile, Protocol, TextLimits};
use qds6_storage::ProfileStore;
use std::path::PathBuf;

// 功能：接入统一品牌与 Lucide 图标资源；时间：2026-11-10；作者：lq。
mod assets;
// 功能：真实服务工作区与终端原生绘制；时间：2026-11-10；作者：lq。
mod services_ui;
mod terminal_view;
// 功能：服务器终端和双栏文件同屏，状态按服务器标签隔离；时间：2026-11-11；作者：lq。
mod server_ui;
// 功能：统一工作标签、页面层级与任务入口；时间：2026-11-11；作者：lq。
mod workspace_ui;
// 功能：软件偏好、版本检查和关于介绍；时间：2026-11-11；作者：lq。
mod settings_ui;
// 功能：连接文件夹分组与归组交互；时间：2026-11-11；作者：lq。
mod connection_groups;
use assets::{AppAssets, BRAND_IMAGE, BRAND_MARK, UiIcon};

/// 功能：协议统一使用语义图标，不依赖平台字体中的特殊字符；时间：2026-11-10；作者：lq。
fn protocol_icon(protocol: Protocol) -> UiIcon {
    match protocol {
        Protocol::Ssh => UiIcon::Terminal,
        Protocol::Sftp => UiIcon::Folder,
        Protocol::MySql | Protocol::PostgreSql => UiIcon::Database,
    }
}

// 功能：原生菜单与平台快捷键动作；时间：2026-11-10；作者：lq。
gpui_kit::actions!(
    qds6,
    [
        /// 功能：关闭前保护未保存修改；时间：2026-11-10；作者：lq。
        CloseWorkspace,
        /// 功能：新建连接配置；时间：2026-11-10；作者：lq。
        NewConnection,
        /// 功能：保存连接配置；时间：2026-11-10；作者：lq。
        SaveConnection
    ]
);

/// 功能：会话内初始 SQL，不连接或执行数据库；时间：2026-11-10；作者：lq。
const INITIAL_SQL: &str = "-- SQL 草稿 · 仅保留在本次会话\n-- 功能：查询当前时间示例；时间：2026-11-10；作者：lq。\n-- 先连接数据库，再点击执行；选区存在时仅执行选区。\n\nSELECT\n    CURRENT_TIMESTAMP AS current_time;\n";

/// 功能：连接表单状态，与磁盘持久化结果分离；时间：2026-11-10；作者：lq。
struct ProfileForm {
    profile: ConnectionProfile,
    name: Entity<InputState>,
    group: Entity<InputState>,
    host: Entity<InputState>,
    port: Entity<InputState>,
    username: Entity<InputState>,
    database: Entity<InputState>,
    secret: Entity<InputState>,
    private_key: Entity<InputState>,
    ca_file: Entity<InputState>,
    tunnel: Entity<InputState>,
}

impl ProfileForm {
    /// 功能：创建表单，保留已存连接的稳定 ID；时间：2026-11-10；作者：lq。
    fn new(profile: ConnectionProfile, window: &mut Window, cx: &mut App) -> Self {
        Self {
            name: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("例如：开发服务器")
                    .default_value(profile.name.clone())
            }),
            group: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("未分组")
                    .default_value(profile.group.clone())
            }),
            host: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("主机名或 IP 地址")
                    .default_value(profile.host.clone())
            }),
            port: cx.new(|cx| InputState::new(window, cx).default_value(profile.port.to_string())),
            username: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("用户名")
                    .default_value(profile.username.clone())
            }),
            database: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("可选")
                    .default_value(profile.database.clone())
            }),
            secret: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("密码 / 私钥口令；留空使用已保存凭据")
                    .masked(true)
            }),
            private_key: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("私钥文件绝对路径；留空使用密码")
                    .default_value(profile.ssh.private_key.clone())
            }),
            ca_file: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("可选 CA PEM 路径")
                    .default_value(profile.db.ca_file.clone())
            }),
            tunnel: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("已保存 SSH 连接名；留空直连")
                    .default_value(
                        profile
                            .db
                            .tunnel_profile
                            .or(profile.ssh.jump_profile)
                            .map(|id| id.to_string())
                            .unwrap_or_default(),
                    )
            }),
            profile,
        }
    }

    /// 功能：读取表单并在写入前完成校验；时间：2026-11-10；作者：lq。
    fn collect(&self, cx: &App) -> Result<ConnectionProfile, String> {
        let mut profile = self.profile.clone();
        profile.name = self.name.read(cx).value().trim().into();
        profile.group = self.group.read(cx).value().trim().into();
        // 功能：手工输入系统分组名与选择“未分组”保持一致；时间：2026-11-11；作者：lq。
        if profile.group == "未分组" {
            profile.group.clear();
        }
        profile.host = self.host.read(cx).value().trim().into();
        profile.username = self.username.read(cx).value().trim().into();
        profile.database = self.database.read(cx).value().trim().into();
        profile.ssh.private_key = self.private_key.read(cx).value().trim().into();
        profile.db.ca_file = self.ca_file.read(cx).value().trim().into();
        profile.port = self
            .port
            .read(cx)
            .value()
            .trim()
            .parse::<u16>()
            .map_err(|_| "端口必须在 1–65535 之间".to_string())?;
        profile.validate().map_err(|error| error.to_string())?;
        Ok(profile)
    }
}

/// 功能：顶层工作区；任务和订阅跟随视图销毁，不自动联网；时间：2026-11-10；作者：lq。
struct Workbench {
    profiles: Vec<ConnectionProfile>,
    // 功能：分组任务独立管理，避免取消保存或服务任务；时间：2026-11-11；作者：lq。
    groups: connection_groups::GroupUi,
    form: ProfileForm,
    search: Entity<InputState>,
    sql: Entity<EditorState>,
    // 功能：SQL 编辑器补全提供者共享表名列表，连接后原地更新，断连后清空；时间：2026-11-13；作者：lq。
    sql_tables: TableNames,
    tab: usize,
    // 功能：服务器和数据库有独立工作区入口；时间：2026-11-11；作者：lq。
    area: workspace_ui::WorkspaceArea,
    // 功能：连接侧栏可收起，切换布局保留搜索、分组和已连接会话；时间：2026-11-11；作者：lq。
    sidebar_visible: bool,
    // 功能：关闭工具标签保留会话内草稿，按需打开以减少常驻导航；时间：2026-11-11；作者：lq。
    form_open: bool,
    sql_open: bool,
    backup_open: bool,
    advanced_connection: bool,
    // 功能：打开、关闭标签或窗口缩放时把当前标签滚动到可见位置；时间：2026-11-11；作者：lq。
    workspace_scroll: ScrollHandle,
    last_workspace_tab: Option<(workspace_ui::WorkspaceTarget, usize, Pixels)>,
    favorites_only: bool,
    loading: bool,
    saving: bool,
    ready: bool,
    form_dirty: bool,
    sql_dirty: bool,
    status: SharedString,
    error: bool,
    subscriptions: Vec<Subscription>,
    task: Option<Task<()>>,
    storage_path: Option<PathBuf>,
    services: services_ui::ServiceUi,
    // 功能：设置任务独立于连接、传输和查询任务；时间：2026-11-11；作者：lq。
    settings: settings_ui::SettingsUi,
}

impl Workbench {
    /// 功能：建立 Kit 编辑与输入组件，然后后台读取连接；时间：2026-11-10；作者：lq。
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self::new_with_store(None, window, cx)
    }

    /// 功能：允许验收使用隔离数据库，避免接触用户连接；时间：2026-11-10；作者：lq。
    fn new_with_store(
        storage_path: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("搜索连接、分组或主机"));
        // 功能：补全提供者和表名列表在 sql 创建前初始化，避免循环引用；时间：2026-11-13；作者：lq。
        let sql_tables = TableNames::default();
        let sql = cx.new(|cx| {
            let mut state = EditorState::new(window, cx)
                .language("sql")
                .line_number(true)
                .searchable(true)
                .soft_wrap(false)
                .default_value(INITIAL_SQL);
            // 功能：注册 SQL 补全提供者，启用关键字和表名联想；时间：2026-11-13；作者：lq。
            state.lsp_mut().completion_provider =
                Some(std::rc::Rc::new(SqlCompletionProvider::new(sql_tables.clone())));
            state
        });
        let subscriptions = vec![
            cx.subscribe(&search, |_, _, _: &InputEvent, cx| cx.notify()),
            cx.subscribe(&sql, |this, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    this.sql_dirty = true;
                    cx.notify();
                }
            }),
        ];
        let mut this = Self {
            profiles: Vec::new(),
            groups: connection_groups::GroupUi::default(),
            form: ProfileForm::new(ConnectionProfile::new(Protocol::Ssh), window, cx),
            search,
            sql,
            sql_tables,
            tab: 0,
            area: workspace_ui::WorkspaceArea::Servers,
            sidebar_visible: true,
            form_open: false,
            sql_open: false,
            backup_open: false,
            advanced_connection: false,
            workspace_scroll: ScrollHandle::new(),
            last_workspace_tab: None,
            favorites_only: false,
            loading: false,
            saving: false,
            ready: false,
            form_dirty: false,
            sql_dirty: false,
            status: "正在读取本地连接…".into(),
            error: false,
            subscriptions,
            task: None,
            storage_path,
            services: services_ui::ServiceUi::new(window, cx),
            settings: settings_ui::SettingsUi::default(),
        };
        this.watch_form(cx);
        this.watch_system_appearance(window, cx);
        this.load(cx);
        this.load_preferences(window, cx);
        this
    }

    /// 功能：标记表单修改并释放旧表单订阅；时间：2026-11-10；作者：lq。
    fn watch_form(&mut self, cx: &mut Context<Self>) {
        self.subscriptions.truncate(2);
        for input in [
            &self.form.name,
            &self.form.group,
            &self.form.host,
            &self.form.port,
            &self.form.username,
            &self.form.database,
            &self.form.secret,
            &self.form.private_key,
            &self.form.ca_file,
            &self.form.tunnel,
        ] {
            self.subscriptions
                .push(cx.subscribe(input, |this, _, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.form_dirty = true;
                        cx.notify();
                    }
                }));
        }
    }

    /// 功能：定位独立应用数据文件，不虚构反向域名；时间：2026-11-10；作者：lq。
    fn store_path() -> Result<PathBuf, String> {
        let directory = match std::env::var_os("QDS6_DATA_DIR") {
            Some(path) => PathBuf::from(path),
            None => BaseDirs::new()
                .ok_or("无法定位应用数据目录")?
                .data_local_dir()
                .join("QDS6"),
        };
        std::fs::create_dir_all(&directory).map_err(|_| "无法创建 QDS6 数据目录")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))
                .map_err(|_| "无法设置 QDS6 数据目录权限")?;
        }
        Ok(directory.join("connections.sqlite3"))
    }

    /// 功能：SQLite 和文件 I/O 在后台进行，失败不伪装成空配置；时间：2026-11-10；作者：lq。
    fn load(&mut self, cx: &mut Context<Self>) {
        if self.loading || self.saving || self.services.busy || self.groups.busy {
            return;
        }
        self.loading = true;
        let storage_path = self.storage_path.clone();
        self.task = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    let path = storage_path.map(Ok).unwrap_or_else(Self::store_path)?;
                    ProfileStore::open(path)
                        .and_then(|store| Ok((store.list()?, store.groups()?)))
                        .map_err(|error| error.to_string())
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.loading = false;
                match result {
                    Ok((profiles, groups)) => {
                        this.profiles = profiles;
                        this.groups.set_items(groups, cx);
                        this.ready = true;
                        this.error = false;
                        this.status = "本地配置已就绪 · 所有连接均未连接".into();
                    }
                    Err(error) => {
                        this.status = error.into();
                        this.error = true;
                        this.ready = false;
                    }
                }
                cx.notify();
            });
        }));
    }

    /// 功能：切换前确认丢弃当前表单，避免无提示丢失输入；时间：2026-11-10；作者：lq。
    fn open_profile(
        &mut self,
        profile: ConnectionProfile,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_profile_action(profile, false, window, cx);
    }

    /// 功能：双击已保存连接时先处理未保存表单，再启动当前连接；时间：2026-11-11；作者：lq。
    fn open_profile_action(
        &mut self,
        profile: ConnectionProfile,
        connect: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.saving || self.services.busy {
            return;
        }
        if self.form_dirty {
            let answer = window.prompt(
                PromptLevel::Warning,
                "放弃尚未保存的连接修改？",
                None,
                &["取消", "放弃修改"],
                cx,
            );
            cx.spawn_in(window, async move |this, window| {
                if answer.await == Ok(1) {
                    let _ = this.update_in(window, |this, window, cx| {
                        this.replace_form(profile, window, cx);
                        if connect {
                            this.connect_service(window, cx);
                        }
                    });
                }
            })
            .detach();
        } else {
            self.replace_form(profile, window, cx);
            if connect {
                self.connect_service(window, cx);
            }
        }
    }

    /// 功能：替换连接表单并保留 SQL 草稿；时间：2026-11-10；作者：lq。
    fn replace_form(
        &mut self,
        profile: ConnectionProfile,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.area = workspace_ui::WorkspaceArea::for_protocol(profile.protocol);
        self.form = ProfileForm::new(profile, window, cx);
        self.watch_form(cx);
        self.form_dirty = false;
        self.tab = 1;
        self.form_open = true;
        self.advanced_connection = false;
        self.error = false;
        self.status = "编辑连接配置 · 保存不会自动连接".into();
        cx.notify();
    }

    /// 功能：事务提交后再刷新列表，失败保留表单；时间：2026-11-10；作者：lq。
    fn save(&mut self, cx: &mut Context<Self>) {
        if !self.ready || self.saving || self.services.busy || self.groups.busy {
            return;
        }
        let mut profile = match self.collect_profile(cx) {
            Ok(profile) => profile,
            Err(error) => {
                self.status = error.into();
                self.error = true;
                cx.notify();
                return;
            }
        };
        // 功能：新凭据先写 Keychain，配置失败时清理；普通 SQLite 从不接收秘密；时间：2026-11-10；作者：lq。
        let secret = zeroize::Zeroizing::new(self.form.secret.read(cx).value().to_string());
        let previous_credential = profile.credential.clone();
        if !secret.is_empty() {
            profile.credential = Some(qds6_core::CredentialRef(uuid::Uuid::new_v4()));
        }
        let storage_path = self.storage_path.clone();
        self.saving = true;
        self.status = "正在保存连接配置…".into();
        self.error = false;
        self.task = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    let path = storage_path.map(Ok).unwrap_or_else(Self::store_path)?;
                    let mut store = ProfileStore::open(path).map_err(|error| error.to_string())?;
                    if !secret.is_empty() {
                        qds6_services::credentials::CredentialStore::set(
                            profile.credential.as_ref().expect("new credential"),
                            &secret,
                        )?;
                    }
                    if let Err(error) = store.save(&profile) {
                        if !secret.is_empty() {
                            let _ = qds6_services::credentials::CredentialStore::delete(
                                profile.credential.as_ref().expect("new credential"),
                            );
                        }
                        return Err(error.to_string());
                    }
                    if !secret.is_empty()
                        && let Some(old) = previous_credential
                    {
                        let _ = qds6_services::credentials::CredentialStore::delete(&old);
                    }
                    let groups = store.groups().map_err(|error| error.to_string())?;
                    Ok::<_, String>((profile, groups))
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.saving = false;
                match result {
                    Ok((profile, groups)) => {
                        this.groups.set_items(groups, cx);
                        if let Some(index) =
                            this.profiles.iter().position(|item| item.id == profile.id)
                        {
                            this.profiles[index] = profile.clone();
                        } else {
                            this.profiles.push(profile.clone());
                        }
                        this.form.profile = profile;
                        this.form_dirty = false;
                        this.error = false;
                        this.status = "连接已保存到本机 · 尚未连接".into();
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

    /// 功能：删除前检查依赖并确认，后台操作 SQLite；凭据保留供本机快照回滚；时间：2026-11-11；作者：lq。
    fn delete_profile(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving || self.loading || self.services.busy || self.groups.busy {
            return;
        }
        let id = self.form.profile.id;
        if self
            .profiles
            .iter()
            .any(|p| p.ssh.jump_profile == Some(id) || p.db.tunnel_profile == Some(id))
        {
            self.status = "此连接仍被跳板或数据库隧道引用，请先移除依赖".into();
            cx.notify();
            return;
        }
        let answer = window.prompt(PromptLevel::Warning, "删除此连接？", Some("活动会话保持连接；该连接的未保存表单修改会丢弃。Keychain 引用保留供已有本机快照回滚。"), &["取消", "删除"], cx);
        cx.spawn_in(window, async move |this, cx| {
            if answer.await != Ok(1) {
                return;
            }
            let _ = this.update_in(cx, |this, window, cx| {
                let storage = this.storage_path.clone();
                this.service_job(
                    "正在删除连接…",
                    move || {
                        let path = storage.map(Ok).unwrap_or_else(Self::store_path)?;
                        ProfileStore::open(path)
                            .and_then(|mut store| store.delete(&id.to_string()))
                            .map_err(|e| e.to_string())
                    },
                    move |this, _, window, cx| {
                        this.profiles.retain(|p| p.id != id);
                        // 功能：仅清理被删连接的表单，保留当前功能区、其他表单和活动文档；时间：2026-11-11；作者：lq。
                        if this.form.profile.id == id {
                            this.form = ProfileForm::new(
                                ConnectionProfile::new(this.area.default_protocol()),
                                window,
                                cx,
                            );
                            this.watch_form(cx);
                            this.form_dirty = false;
                            this.form_open = false;
                            this.advanced_connection = false;
                            if this.tab == 1 {
                                this.tab = 0;
                            }
                        }
                        this.status = "连接已删除，活动会话未关闭".into();
                    },
                    window,
                    cx,
                );
            });
        })
        .detach();
    }

    /// 功能：菜单、快捷键和窗口关闭共用同一丢弃确认；时间：2026-11-10；作者：lq。
    fn allow_close(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.saving
            || self.groups.busy
            || self.services.busy
            || self
                .services
                .servers
                .iter()
                .any(|server| server.read(cx).pending())
        {
            self.status = "正在保存连接或加密草稿，请等待完成再关闭".into();
            cx.notify();
            return false;
        }
        if !self.form_dirty
            && !self.groups.editing()
            && !self.sql_dirty
            && !self
                .services
                .servers
                .iter()
                .any(|server| server.read(cx).services.remote_dirty)
            && !self.services.has_live_sessions()
            && !self.services.busy
        {
            return true;
        }
        let answer = window.prompt(
            PromptLevel::Warning,
            "关闭 QDS6？",
            Some("活动会话将断开；未保存的连接、分组名称、SQL 和远程文件草稿将被丢弃。请先保存或复制需要保留的内容。"),
            &["取消", "丢弃并关闭"],
            cx,
        );
        window
            .spawn(cx, async move |cx| {
                if answer.await == Ok(1) {
                    let _ = cx.update(|window, _| window.remove_window());
                }
            })
            .detach();
        false
    }

    /// 功能：统一紧凑字段、可访问名称与保存中禁用状态；时间：2026-11-11；作者：lq。
    fn field(&self, label: &'static str, input: &Entity<InputState>) -> impl IntoElement + use<> {
        v_flex()
            .flex_1()
            .min_w_0()
            .gap_1()
            .child(div().text_xs().child(label))
            .child(
                Input::new(input)
                    .id(label)
                    .aria_label(label)
                    .disabled(self.saving || self.services.busy),
            )
    }

    /// 功能：本地搜索、收藏筛选和文件夹分组侧栏；时间：2026-11-11；作者：lq。
    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let list = self.connection_group_list(cx);
        v_flex()
            .id("connection-sidebar")
            .test_support()
            .w(px(224.))
            .h_full()
            .flex_shrink_0()
            .border_r_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().sidebar)
            .child(
                v_flex()
                    .p_4()
                    .gap_3()
                    .child(
                        h_flex()
                            .justify_between()
                            .items_center()
                            .child(div().font_semibold().child(self.area.label()))
                            .child(
                                Button::new("new-sidebar")
                                    .ghost()
                                    .small()
                                    .icon(UiIcon::Plus)
                                    .label("新建")
                                    .disabled(self.saving || self.groups.busy)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.open_profile(
                                            ConnectionProfile::new(this.area.default_protocol()),
                                            window,
                                            cx,
                                        )
                                    })),
                            ),
                    )
                    .child(
                        Input::new(&self.search)
                            .id("connection-search")
                            .small()
                            .prefix(Icon::new(UiIcon::Search).small())
                            .aria_label("搜索连接"),
                    )
                    .child(
                        Button::new("favorites")
                            .ghost()
                            .small()
                            .icon(UiIcon::Star)
                            .label(if self.favorites_only {
                                "仅收藏"
                            } else {
                                "全部连接"
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.favorites_only = !this.favorites_only;
                                cx.notify();
                            })),
                    ),
            )
            .child(self.group_toolbar(cx))
            .child(
                div()
                    .px_4()
                    .pb_2()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child("双击连接 · 拖动连接可归组"),
            )
            .child(
                div()
                    .id("connections-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px_2()
                    .child(list),
            )
            .child(
                v_flex()
                    .p_3()
                    .gap_1()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .when(self.area == workspace_ui::WorkspaceArea::Databases, |v| {
                        v.child(
                            h_flex().child(
                                Button::new("open-sql-sidebar")
                                    .ghost()
                                    .small()
                                    .justify_start()
                                    .icon(UiIcon::FileCode)
                                    .label("SQL 草稿")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.activate_workspace(
                                            workspace_ui::WorkspaceTarget::Sql,
                                            cx,
                                        )
                                    })),
                            ),
                        )
                    })
                    .child(
                        h_flex().child(
                            Button::new("open-backup-sidebar")
                                .ghost()
                                .small()
                                .justify_start()
                                .icon(UiIcon::Download)
                                .label("导入与备份")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.activate_workspace(
                                        workspace_ui::WorkspaceTarget::Backup,
                                        cx,
                                    )
                                })),
                        ),
                    )
                    .child(
                        div()
                            .px_2()
                            .pt_2()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("{} 个连接 · 本机存储", self.profiles.len())),
                    ),
            )
    }

    /// 功能：工作区起始页围绕三种连接任务组织入口；时间：2026-11-11；作者：lq。
    fn welcome(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let mut actions = v_flex().gap_1();
        let entries = match self.area {
            workspace_ui::WorkspaceArea::Servers => vec![(
                Protocol::Ssh,
                "SSH + SFTP 服务器",
                "一个连接，同时使用终端、本地与远程文件",
            )],
            workspace_ui::WorkspaceArea::Databases => vec![
                (
                    Protocol::MySql,
                    "MySQL",
                    "浏览数据库对象、编写 SQL 与查看结果",
                ),
                (
                    Protocol::PostgreSql,
                    "PostgreSQL",
                    "浏览数据库对象、编写 SQL 与查看结果",
                ),
            ],
        };
        for (protocol, title, description) in entries {
            actions = actions.child(
                h_flex()
                    .id(protocol.label())
                    .test_support()
                    .w_full()
                    .p_3()
                    .gap_3()
                    .items_center()
                    .rounded_md()
                    .cursor_pointer()
                    .hover(|style| style.bg(cx.theme().secondary))
                    .child(Icon::new(protocol_icon(protocol)).small())
                    .child(
                        v_flex()
                            .flex_1()
                            .gap_1()
                            .child(div().text_sm().font_semibold().child(title))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(description),
                            ),
                    )
                    .child(
                        Icon::new(UiIcon::ChevronRight)
                            .small()
                            .text_color(cx.theme().muted_foreground),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_profile(ConnectionProfile::new(protocol), window, cx)
                    })),
            );
        }
        div()
            .id("welcome-scroll")
            .size_full()
            .overflow_y_scroll()
            .child(
                v_flex()
                    .w_full()
                    .min_h_full()
                    .items_center()
                    .justify_center()
                    .p_8()
                    .child(
                        v_flex()
                            .w_full()
                            .max_w(px(520.))
                            .gap_5()
                            .child(
                                h_flex()
                                    .gap_3()
                                    .items_center()
                                    .child(img(BRAND_IMAGE).size(px(44.)))
                                    .child(
                                        v_flex()
                                            .gap_1()
                                            .child(div().text_2xl().font_semibold().child("QDS6"))
                                            .child(
                                                div()
                                                    .text_sm()
                                                    .text_color(cx.theme().muted_foreground)
                                                    .child(
                                                        if self.area
                                                            == workspace_ui::WorkspaceArea::Servers
                                                        {
                                                            "服务器工作区 · 终端与文件同屏"
                                                        } else {
                                                            "数据库工作区 · 对象、查询与结果"
                                                        },
                                                    ),
                                            ),
                                    ),
                            )
                            .child(Self::section_heading(
                                "开始连接",
                                "选择连接类型，或从左侧打开已保存的连接。",
                                cx,
                            ))
                            .child(actions)
                            .child(
                                h_flex()
                                    .pt_4()
                                    .gap_3()
                                    .flex_wrap()
                                    .border_t_1()
                                    .border_color(cx.theme().border)
                                    .child(
                                        Button::new("new-welcome")
                                            .primary()
                                            .small()
                                            .icon(UiIcon::Plus)
                                            .label("新建连接")
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.open_profile(
                                                    ConnectionProfile::new(
                                                        this.area.default_protocol(),
                                                    ),
                                                    window,
                                                    cx,
                                                )
                                            })),
                                    )
                                    .when(
                                        self.area == workspace_ui::WorkspaceArea::Databases,
                                        |v| {
                                            v.child(
                                                Button::new("open-sql")
                                                    .ghost()
                                                    .small()
                                                    .icon(UiIcon::FileCode)
                                                    .label("SQL 草稿")
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.activate_workspace(
                                                            workspace_ui::WorkspaceTarget::Sql,
                                                            cx,
                                                        )
                                                    })),
                                            )
                                        },
                                    ),
                            ),
                    ),
            )
    }

    /// 功能：按基础、地址、认证分节，固定操作栏始终可达；时间：2026-11-11；作者：lq。
    fn connection_page(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let protocol = self.form.profile.protocol;
        let database = matches!(protocol, Protocol::MySql | Protocol::PostgreSql);
        let existing = self.profiles.iter().any(|p| p.id == self.form.profile.id);
        // 功能：服务器配置只显示 SSH/SFTP，数据库只显示数据库协议；时间：2026-11-11；作者：lq。
        let kinds = if database {
            [Protocol::MySql, Protocol::PostgreSql]
        } else {
            [Protocol::Ssh, Protocol::Sftp]
        };
        let protocols = TabBar::new("connection-protocols")
            .segmented()
            .selected_index(kinds.iter().position(|p| *p == protocol).unwrap_or(0))
            .children(kinds.into_iter().map(|kind| {
                Tab::new()
                    .prefix(Icon::new(protocol_icon(kind)).small())
                    .label(match kind {
                        Protocol::Ssh => "SSH + SFTP",
                        Protocol::Sftp => "仅 SFTP",
                        _ => kind.label(),
                    })
                    .disabled(self.saving || self.services.busy)
            }))
            .on_click(cx.listener(move |this, index: &usize, window, cx| {
                let kind = kinds[*index];
                if this.form.profile.protocol != kind {
                    this.form.profile.protocol = kind;
                    this.form.port.update(cx, |state, cx| {
                        state.set_value(kind.default_port().to_string(), window, cx)
                    });
                    this.form_dirty = true;
                    cx.notify();
                }
            }));
        v_flex()
            .size_full()
            .child(
                div()
                    .id("form-scroll")
                    .test_support()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(
                        v_flex()
                            .w_full()
                            .max_w(px(820.))
                            .p_6()
                            .gap_3()
                            .child(
                                v_flex()
                                    .gap_2()
                                    .child(div().text_xl().font_semibold().child(if existing {
                                        "编辑连接"
                                    } else {
                                        "新建连接"
                                    }))
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(cx.theme().muted_foreground)
                                            .child("配置连接信息，保存后可从左侧随时打开。"),
                                    ),
                            )
                            .child(h_flex().child(protocols))
                            .child(
                                v_flex()
                                    .gap_3()
                                    .child(Self::section_heading("基本信息", "", cx))
                                    .child(
                                        h_flex()
                                            .gap_3()
                                            .child(self.field("连接名称", &self.form.name))
                                            .child(self.group_form_field(cx)),
                                    )
                                    .child(
                                        h_flex()
                                            .gap_3()
                                            .items_center()
                                            .flex_wrap()
                                            .child(
                                                Checkbox::new("favorite")
                                                    .label("收藏连接")
                                                    .checked(self.form.profile.favorite)
                                                    .disabled(self.saving || self.services.busy)
                                                    .on_click(cx.listener(
                                                        |this, checked: &bool, _, cx| {
                                                            this.form.profile.favorite = *checked;
                                                            this.form_dirty = true;
                                                            cx.notify();
                                                        },
                                                    )),
                                            )
                                            .child(
                                                Checkbox::new("production")
                                                    .label("生产环境")
                                                    .checked(self.form.profile.production)
                                                    .disabled(self.saving || self.services.busy)
                                                    .on_click(cx.listener(
                                                        |this, checked: &bool, _, cx| {
                                                            this.form.profile.production = *checked;
                                                            this.form_dirty = true;
                                                            cx.notify();
                                                        },
                                                    )),
                                            ),
                                    ),
                            )
                            .child(
                                v_flex()
                                    .gap_3()
                                    .pt_4()
                                    .border_t_1()
                                    .border_color(cx.theme().border)
                                    .child(Self::section_heading("连接地址", "", cx))
                                    .child(
                                        h_flex()
                                            .gap_3()
                                            .child(self.field("主机", &self.form.host))
                                            .child(
                                                div()
                                                    .w(px(120.))
                                                    .child(self.field("端口", &self.form.port)),
                                            ),
                                    )
                                    .child(
                                        h_flex()
                                            .gap_3()
                                            .child(self.field("用户名", &self.form.username))
                                            .when(database, |row| {
                                                row.child(self.field("数据库", &self.form.database))
                                            }),
                                    ),
                            )
                            .child(
                                v_flex()
                                    .gap_3()
                                    .pt_4()
                                    .border_t_1()
                                    .border_color(cx.theme().border)
                                    .child(Self::section_heading(
                                        "身份验证",
                                        "密码保存在 macOS Keychain；留空使用已保存凭据。",
                                        cx,
                                    ))
                                    .child(self.authentication_fields(cx)),
                            )
                            .child(
                                v_flex()
                                    .gap_3()
                                    .pt_4()
                                    .border_t_1()
                                    .border_color(cx.theme().border)
                                    .child(
                                        Button::new("toggle-connection-advanced")
                                            .ghost()
                                            .small()
                                            .justify_start()
                                            .icon(if self.advanced_connection {
                                                UiIcon::ChevronDown
                                            } else {
                                                UiIcon::ChevronRight
                                            })
                                            .label(if database {
                                                "高级选项 · SSH 隧道与 TLS"
                                            } else {
                                                "高级选项 · 跳板连接"
                                            })
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.advanced_connection =
                                                    !this.advanced_connection;
                                                cx.notify();
                                            })),
                                    )
                                    .when(self.advanced_connection, |v| {
                                        v.child(self.advanced_authentication_fields(cx))
                                    }),
                            ),
                    ),
            )
            .child(
                h_flex()
                    .id("connection-actions")
                    .test_support()
                    .flex_shrink_0()
                    .min_h(px(64.))
                    .px_6()
                    .py_3()
                    .gap_3()
                    .items_center()
                    .flex_wrap()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().background)
                    .child(
                        Button::new("connect-profile")
                            .primary()
                            .icon(UiIcon::Plug)
                            .label(if self.services.busy {
                                "连接中…"
                            } else {
                                "连接"
                            })
                            .disabled(self.services.busy || self.saving)
                            .on_click(
                                cx.listener(|this, _, window, cx| this.connect_service(window, cx)),
                            ),
                    )
                    .child(
                        Button::new("save-profile")
                            .icon(UiIcon::Save)
                            .label(if self.saving {
                                "保存中…"
                            } else {
                                "保存连接"
                            })
                            .disabled(
                                !self.ready || self.loading || self.saving || self.services.busy,
                            )
                            .on_click(cx.listener(|this, _, _, cx| this.save(cx))),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(if self.form_dirty {
                                "有未保存的修改"
                            } else {
                                "保存不会自动连接"
                            }),
                    )
                    .when(existing, |v| {
                        v.child(
                            Button::new("delete-profile")
                                .ghost()
                                .small()
                                .icon(UiIcon::Trash)
                                .label("删除…")
                                .disabled(self.services.busy || self.saving)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.delete_profile(window, cx)
                                })),
                        )
                    }),
            )
    }

    /// 功能：SQL 操作集中于右侧，编辑器与查询结果留在同一页面；时间：2026-11-11；作者：lq。
    fn sql_page(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let connected = self.services.database.is_some();
        v_flex()
            .size_full()
            .min_h_0()
            .child(
                h_flex()
                    .min_h(px(46.))
                    .flex_shrink_0()
                    .px_4()
                    .py_2()
                    .gap_2()
                    .items_center()
                    .flex_wrap()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .text_ellipsis()
                            .child(if connected {
                                format!("{} · 选区优先执行", self.services.database_identity)
                            } else {
                                "SQL 草稿 · 尚未连接数据库".into()
                            }),
                    )
                    .when(!connected, |v| {
                        v.child(
                            Button::new("sql-connect-database")
                                .small()
                                .ghost()
                                .label("连接数据库…")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.open_profile(
                                        ConnectionProfile::new(Protocol::PostgreSql),
                                        window,
                                        cx,
                                    )
                                })),
                        )
                    })
                    .child(
                        Button::new("copy-sql")
                            .ghost()
                            .small()
                            .icon(UiIcon::Copy)
                            .label("复制 SQL")
                            .on_click(cx.listener(|this, _, _, cx| {
                                let text = this.sql.read(cx).value();
                                match TextLimits::default().validate(text.as_bytes()) {
                                    Ok(_) => {
                                        cx.write_to_clipboard(ClipboardItem::new_string(
                                            text.to_string(),
                                        ));
                                        this.status = "SQL 已复制 · 草稿仍仅保留在本次会话".into();
                                        this.error = false;
                                    }
                                    Err(error) => {
                                        this.status = error.to_string().into();
                                        this.error = true;
                                    }
                                }
                                cx.notify();
                            })),
                    )
                    .when(self.services.busy && connected, |v| {
                        v.child(
                            Button::new("cancel-query")
                                .small()
                                .label("取消查询")
                                .on_click(cx.listener(|this, _, _, cx| this.cancel_query(cx))),
                        )
                    })
                    .child(
                        Button::new("run-sql")
                            .small()
                            .primary()
                            .icon(UiIcon::Play)
                            .label("执行 SQL")
                            .disabled(self.services.busy || !connected)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.execute_sql(None, window, cx)
                            })),
                    ),
            )
            .child(
                div().flex_1().min_h_0().child(
                    Editor::new(&self.sql)
                        .size_full()
                        .bordered(false)
                        .aria_label("SQL 草稿编辑器"),
                ),
            )
            .when(self.has_query_results(), |v| {
                v.child(
                    div()
                        .id("sql-results")
                        .flex_1()
                        .min_h_0()
                        .border_t_1()
                        .border_color(cx.theme().border)
                        .child(self.query_results_panel(cx)),
                )
            })
    }
}

/// 功能：统一主题的标题栏、侧栏、Kit 标签和状态栏；时间：2026-11-10；作者：lq。
impl Render for Workbench {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let page = match self.tab {
            1 => self.connection_page(cx).into_any_element(),
            2 => self.sql_page(cx).into_any_element(),
            3 => self.services_page(window, cx).into_any_element(),
            4 => self.backup_page(cx).into_any_element(),
            5 => self.settings_page(cx).into_any_element(),
            _ => self.welcome(cx).into_any_element(),
        };
        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_sm()
            .text_color(cx.theme().foreground)
            .child(
                TitleBar::new().child(
                    h_flex()
                        .w_full()
                        .px_3()
                        .items_center()
                        .justify_between()
                        .child(
                            h_flex()
                                .gap_3()
                                .items_center()
                                .child(Icon::default().path(BRAND_MARK).small())
                                .child(div().font_semibold().child("QDS6"))
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child("本地工作区"),
                                ),
                        )
                        .child(
                            Button::new("theme")
                                .ghost()
                                .small()
                                // 功能：标题栏快捷入口循环浅色、深色和自动，沿用后台持久化路径；时间：2026-11-11；作者：lq。
                                .icon(match self.settings.preferences.theme {
                                    qds6_storage::ColorTheme::Light => UiIcon::Moon,
                                    qds6_storage::ColorTheme::Dark => UiIcon::RefreshCw,
                                    qds6_storage::ColorTheme::System => UiIcon::Sun,
                                })
                                .label(match self.settings.preferences.theme {
                                    qds6_storage::ColorTheme::Light => "深色",
                                    qds6_storage::ColorTheme::Dark => "自动",
                                    qds6_storage::ColorTheme::System => "浅色",
                                })
                                .tooltip(match self.settings.preferences.theme {
                                    qds6_storage::ColorTheme::Light => "当前：浅色；切换为深色",
                                    qds6_storage::ColorTheme::Dark => {
                                        "当前：深色；切换为自动（跟随系统）"
                                    }
                                    qds6_storage::ColorTheme::System => {
                                        "当前：自动（跟随系统）；切换为浅色"
                                    }
                                })
                                .disabled(!self.settings.ready || self.settings.saving)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    let mut preferences = this.settings.preferences.clone();
                                    preferences.theme = match preferences.theme {
                                        qds6_storage::ColorTheme::Light => {
                                            qds6_storage::ColorTheme::Dark
                                        }
                                        qds6_storage::ColorTheme::Dark => {
                                            qds6_storage::ColorTheme::System
                                        }
                                        qds6_storage::ColorTheme::System => {
                                            qds6_storage::ColorTheme::Light
                                        }
                                    };
                                    this.save_preferences(preferences, window, cx);
                                })),
                        ),
                ),
            )
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .child(self.navigation_rail(cx))
                    .when(self.tab != 5 && self.sidebar_visible, |row| {
                        row.child(self.sidebar(cx))
                    })
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .child(self.workspace_tabs(window, cx))
                            .child(div().flex_1().min_h_0().overflow_hidden().child(page)),
                    ),
            )
            .child(
                h_flex()
                    .id("workspace-status")
                    .test_support()
                    .h(px(28.))
                    .flex_shrink_0()
                    .px_3()
                    .gap_3()
                    .items_center()
                    .justify_between()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .text_xs()
                    .text_color(if self.error {
                        cx.theme().danger
                    } else {
                        cx.theme().muted_foreground
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_ellipsis()
                            .child(self.status.clone()),
                    )
                    .child(
                        Button::new("reload")
                            .ghost()
                            .xsmall()
                            .icon(UiIcon::RefreshCw)
                            .tooltip("重新读取连接配置")
                            .disabled(self.loading || self.saving)
                            .on_click(cx.listener(|this, _, _, cx| this.load(cx))),
                    ),
            )
    }
}

/// 功能：初始化 Kit 默认后端与原生窗口，关闭前保护会话内修改；时间：2026-11-10；作者：lq。
pub fn run() {
    gpui_kit::application().with_assets(AppAssets).run(|cx| {
        gpui_kit::init(cx);
        let [quit, close, new, save] = if cfg!(target_os = "macos") {
            ["cmd-q", "cmd-w", "cmd-n", "cmd-s"]
        } else {
            ["ctrl-q", "ctrl-w", "ctrl-n", "ctrl-s"]
        };
        cx.bind_keys([
            KeyBinding::new(quit, CloseWorkspace, None),
            KeyBinding::new(close, CloseWorkspace, None),
            KeyBinding::new(new, NewConnection, None),
            KeyBinding::new(save, SaveConnection, None),
        ]);
        cx.set_menus([
            Menu::new("QDS6").items([MenuItem::action("退出 QDS6", CloseWorkspace)]),
            Menu::new("连接").items([
                MenuItem::action("新建连接", NewConnection),
                MenuItem::action("保存连接", SaveConnection),
            ]),
            Menu::new("编辑").items([
                MenuItem::action("撤销", component::input::Undo),
                MenuItem::action("重做", component::input::Redo),
                MenuItem::Separator,
                MenuItem::action("剪切", component::input::Cut),
                MenuItem::action("复制", component::input::Copy),
                MenuItem::action("粘贴", component::input::Paste),
                MenuItem::action("全选", component::input::SelectAll),
            ]),
        ]);
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        let mut options = TitleBar::window_options();
        // 功能：启动使用更紧凑的居中窗口，保留可调整尺寸和原最小窗口限制；时间：2026-11-11；作者：lq。
        options.window_bounds = Some(WindowBounds::centered(size(px(1200.), px(780.)), cx));
        options.window_min_size = Some(size(px(960.), px(640.)));
        if let Err(error) = gpui_kit::open_window(options, cx, |window, cx| {
            window.set_window_title("QDS6");
            let view = cx.new(|cx| Workbench::new(window, cx));
            let weak = view.downgrade();
            window.on_window_should_close(cx, move |window, cx| {
                weak.update(cx, |state, cx| state.allow_close(window, cx))
                    .unwrap_or(true)
            });
            let weak = view.downgrade();
            cx.on_action(move |_: &CloseWorkspace, cx| {
                if let Some(window) = cx.active_window() {
                    let _ = window.update(cx, |_, window, cx| {
                        let _ = weak.update(cx, |state, cx| {
                            if state.allow_close(window, cx) {
                                window.remove_window();
                            }
                        });
                    });
                }
            });
            let weak = view.downgrade();
            cx.on_action(move |_: &NewConnection, cx| {
                if let Some(window) = cx.active_window() {
                    let _ = window.update(cx, |_, window, cx| {
                        let _ = weak.update(cx, |state, cx| {
                            state.open_profile(
                                ConnectionProfile::new(state.area.default_protocol()),
                                window,
                                cx,
                            )
                        });
                    });
                }
            });
            let weak = view.downgrade();
            // 功能：保存快捷键按当前工作区路由，远程编辑器不会误保存连接表单；时间：2026-11-11；作者：lq。
            cx.on_action(move |_: &SaveConnection, cx| {
                if let Some(window) = cx.active_window() {
                    let _ = window.update(cx, |_, window, cx| {
                        let _ = weak.update(cx, |state, cx| {
                            if state.tab == 1 {
                                state.save(cx);
                            } else if state.tab == 3 && state.services.page != 3 {
                                if let Some(server) = state
                                    .services
                                    .servers
                                    .get(state.services.active_server)
                                    .cloned()
                                {
                                    server.update(cx, |server, cx| server.save_remote(window, cx));
                                }
                            } else {
                                state.status =
                                    "SQL 草稿仅保留本次会话，请使用「复制 SQL」另行保存".into();
                                cx.notify();
                            }
                        });
                    });
                }
            });
            view
        }) {
            eprintln!("QDS6 窗口创建失败：{error}");
            cx.quit();
        }
        cx.activate(true);
    });
}

/// 功能：原生渲染与交互验收入口，仅在显式功能开关下编译；时间：2026-11-10；作者：lq。
#[cfg(feature = "ui-tests")]
mod smoke;
#[cfg(feature = "ui-tests")]
pub use services_ui::run_service_ui_smoke;
// 功能：导出独立服务器状态验收，不进入发行构建；时间：2026-11-11；作者：lq。
#[cfg(feature = "ui-tests")]
pub use server_ui::status_ui::smoke::run_server_status_smoke;
// 功能：导出隔离 SSH/SFTP 的终端留白、侧栏及文件可见性验收；时间：2026-11-11；作者：lq。
#[cfg(feature = "ui-tests")]
pub use server_ui::run_sidebar_files_smoke;
#[cfg(feature = "ui-tests")]
pub use smoke::run_ui_smoke;
// 功能：独立布局回归覆盖最小窗口、标签和任务入口；时间：2026-11-11；作者：lq。
#[cfg(feature = "ui-tests")]
pub use services_ui::run_layout_smoke;
// 功能：公开隔离的原生自动主题验收入口；时间：2026-11-11；作者：lq。
#[cfg(feature = "ui-tests")]
pub use settings_ui::appearance_smoke::run_appearance_smoke;
// 功能：文件夹分组验收只在测试构建开放；时间：2026-11-11；作者：lq。
#[cfg(feature = "ui-tests")]
pub use connection_groups::smoke::run_group_smoke;

// 功能：独立验证 Kit 树、虚拟表和停靠组件，不接触正式配置；时间：2026-11-10；作者：lq。
#[cfg(any(feature = "component-lab", feature = "ui-tests"))]
mod component_lab;
// 功能：隔离 SQLite 下回归删除连接后的原生导航与确认行为；时间：2026-11-11；作者：lq。
#[cfg(all(test, feature = "ui-tests"))]
mod deletion_tests;
#[cfg(feature = "component-lab")]
pub use component_lab::run_component_lab;
#[cfg(feature = "ui-tests")]
pub use component_lab::run_component_smoke;
// 功能：公开多表网格及查询原生验收入口；时间：2026-11-11；作者：lq。
#[cfg(feature = "ui-tests")]
pub use services_ui::run_database_grid_smoke;
