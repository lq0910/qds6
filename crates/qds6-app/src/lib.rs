//! 功能：QDS6 原生工作台、非秘密连接管理与会话内 SQL 草稿；时间：2026-11-10；作者：lq。

use directories::BaseDirs;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    component::{
        ActiveTheme, Disableable, Icon, Selectable, Sizable, StyledExt, Theme, ThemeMode, TitleBar,
        button::{Button, ButtonVariants},
        h_flex,
        input::{Editor, EditorState, Input, InputEvent, InputState},
        tab::{Tab, TabBar},
        v_flex,
    },
    *,
};
use qds6_core::{ConnectionProfile, Protocol, TextLimits};
use qds6_storage::ProfileStore;
use std::path::PathBuf;

// 功能：接入统一品牌与 Lucide 图标资源；时间：2026-11-10；作者：lq。
mod assets;
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
const INITIAL_SQL: &str = "-- SQL 草稿 · 仅保留在本次会话\n-- 功能：本地编辑示例；时间：2026-11-10；作者：lq。\n-- 数据库尚未连接，此处不会执行查询。\n\nSELECT\n    CURRENT_TIMESTAMP AS current_time;\n";

/// 功能：连接表单状态，与磁盘持久化结果分离；时间：2026-11-10；作者：lq。
struct ProfileForm {
    profile: ConnectionProfile,
    name: Entity<InputState>,
    group: Entity<InputState>,
    host: Entity<InputState>,
    port: Entity<InputState>,
    username: Entity<InputState>,
    database: Entity<InputState>,
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
            profile,
        }
    }

    /// 功能：读取表单并在写入前完成校验；时间：2026-11-10；作者：lq。
    fn collect(&self, cx: &App) -> Result<ConnectionProfile, String> {
        let mut profile = self.profile.clone();
        profile.name = self.name.read(cx).value().trim().into();
        profile.group = self.group.read(cx).value().trim().into();
        profile.host = self.host.read(cx).value().trim().into();
        profile.username = self.username.read(cx).value().trim().into();
        profile.database = self.database.read(cx).value().trim().into();
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
    form: ProfileForm,
    search: Entity<InputState>,
    sql: Entity<EditorState>,
    tab: usize,
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
        let sql = cx.new(|cx| {
            EditorState::new(window, cx)
                .language("sql")
                .line_number(true)
                .searchable(true)
                .soft_wrap(false)
                .default_value(INITIAL_SQL)
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
            form: ProfileForm::new(ConnectionProfile::new(Protocol::Ssh), window, cx),
            search,
            sql,
            tab: 0,
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
        };
        this.watch_form(cx);
        this.load(cx);
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
        if self.loading || self.saving {
            return;
        }
        self.loading = true;
        let storage_path = self.storage_path.clone();
        self.task = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    let path = storage_path.map(Ok).unwrap_or_else(Self::store_path)?;
                    ProfileStore::open(path)
                        .and_then(|store| store.list())
                        .map_err(|error| error.to_string())
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.loading = false;
                match result {
                    Ok(profiles) => {
                        this.profiles = profiles;
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
        if self.saving {
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
                        this.replace_form(profile, window, cx)
                    });
                }
            })
            .detach();
        } else {
            self.replace_form(profile, window, cx);
        }
    }

    /// 功能：替换连接表单并保留 SQL 草稿；时间：2026-11-10；作者：lq。
    fn replace_form(
        &mut self,
        profile: ConnectionProfile,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.form = ProfileForm::new(profile, window, cx);
        self.watch_form(cx);
        self.form_dirty = false;
        self.tab = 1;
        self.error = false;
        self.status = "编辑连接配置 · 保存不会自动连接".into();
        cx.notify();
    }

    /// 功能：事务提交后再刷新列表，失败保留表单；时间：2026-11-10；作者：lq。
    fn save(&mut self, cx: &mut Context<Self>) {
        if !self.ready || self.saving {
            return;
        }
        let profile = match self.form.collect(cx) {
            Ok(profile) => profile,
            Err(error) => {
                self.status = error.into();
                self.error = true;
                cx.notify();
                return;
            }
        };
        let storage_path = self.storage_path.clone();
        self.saving = true;
        self.status = "正在保存连接配置…".into();
        self.error = false;
        self.task = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    let path = storage_path.map(Ok).unwrap_or_else(Self::store_path)?;
                    let mut store = ProfileStore::open(path).map_err(|error| error.to_string())?;
                    store.save(&profile).map_err(|error| error.to_string())?;
                    Ok::<_, String>(profile)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.saving = false;
                match result {
                    Ok(profile) => {
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

    /// 功能：菜单、快捷键和窗口关闭共用同一丢弃确认；时间：2026-11-10；作者：lq。
    fn allow_close(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.saving {
            self.status = "正在保存连接，请等待保存完成再关闭".into();
            cx.notify();
            return false;
        }
        if !self.form_dirty && !self.sql_dirty {
            return true;
        }
        let answer = window.prompt(
            PromptLevel::Warning,
            "关闭 QDS6？",
            Some("未保存的连接修改和会话内 SQL 草稿将被丢弃。可取消关闭并保存连接或复制 SQL。"),
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

    /// 功能：统一字段样式和可访问名称，保存时冻结输入；时间：2026-11-10；作者：lq。
    fn field(&self, label: &'static str, input: &Entity<InputState>) -> impl IntoElement + use<> {
        v_flex()
            .flex_1()
            .min_w_0()
            .gap_2()
            .child(div().text_sm().child(label))
            .child(
                Input::new(input)
                    .id(label)
                    .aria_label(label)
                    .disabled(self.saving),
            )
    }

    /// 功能：本地搜索、收藏筛选和连接分组侧栏；时间：2026-11-10；作者：lq。
    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let query = self.search.read(cx).value();
        let mut profiles: Vec<_> = self
            .profiles
            .iter()
            .filter(|p| p.matches(&query) && (!self.favorites_only || p.favorite))
            .cloned()
            .collect();
        profiles.sort_by(|a, b| a.group.cmp(&b.group).then(a.name.cmp(&b.name)));
        let mut list = v_flex().gap_1();
        let mut group: Option<String> = None;
        for profile in &profiles {
            if group.as_ref() != Some(&profile.group) {
                group = Some(profile.group.clone());
                list = list.child(
                    div()
                        .px_2()
                        .pt_4()
                        .pb_2()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(if profile.group.is_empty() {
                            "未分组".into()
                        } else {
                            profile.group.clone()
                        }),
                );
            }
            let selected = self.form.profile.id == profile.id && self.tab == 1;
            let profile = profile.clone();
            // 功能：连接类型和收藏标记统一绘制为 SVG，保留连接名称原文；时间：2026-11-10；作者：lq。
            let heading = h_flex()
                .gap_2()
                .items_center()
                .child(Icon::new(protocol_icon(profile.protocol)).small())
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_sm()
                        .child(profile.name.clone()),
                )
                .when(profile.favorite, |row| {
                    row.child(Icon::new(UiIcon::Star).xsmall())
                });
            let detail = format!(
                "{} · {}:{}{}",
                profile.protocol.label(),
                profile.host,
                profile.port,
                if profile.production { " · 生产" } else { "" }
            );
            list = list.child(
                div()
                    .id(SharedString::from(profile.id.to_string()))
                    .test_support()
                    .px_2()
                    .py_2()
                    .rounded_md()
                    .cursor_pointer()
                    .bg(if selected {
                        cx.theme().secondary
                    } else {
                        cx.theme().sidebar
                    })
                    .hover(|style| style.bg(cx.theme().secondary))
                    .child(heading)
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(detail),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_profile(profile.clone(), window, cx)
                    })),
            );
        }
        v_flex()
            .w(px(264.))
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
                            .child(div().font_semibold().child("连接"))
                            .child(
                                Button::new("new-sidebar")
                                    .ghost()
                                    .small()
                                    .icon(UiIcon::Plus)
                                    .label("新建")
                                    .disabled(self.saving)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.open_profile(
                                            ConnectionProfile::new(Protocol::Ssh),
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
            .child(
                div()
                    .id("connections-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px_2()
                    .child(list)
                    .when(profiles.is_empty(), |element| {
                        element.child(
                            div()
                                .px_2()
                                .py_6()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(if self.loading {
                                    "正在读取…"
                                } else if !self.ready {
                                    "配置读取失败，请重试"
                                } else if self.profiles.is_empty() {
                                    "还没有连接。新建一个开始。"
                                } else {
                                    "没有匹配的连接"
                                }),
                        )
                    }),
            )
            .child(
                div()
                    .p_4()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!("{} 个已保存连接 · 本机", self.profiles.len())),
            )
    }

    /// 功能：显示当前可用操作及未接入的网络能力；时间：2026-11-10；作者：lq。
    fn welcome(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        v_flex()
            .size_full()
            .justify_center()
            .px_12()
            .gap_5()
            // 功能：彩色品牌使用图像管线，保留原始渐变和透明边距；时间：2026-11-10；作者：lq。
            .child(img(BRAND_IMAGE).size(px(88.)))
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child("本地工作区 / QDS6 0.1.0"),
            )
            .child(div().text_3xl().font_semibold().child("连接，从此简单。"))
            .child(
                div()
                    .text_color(cx.theme().muted_foreground)
                    .child("终端、文件与数据库，一个工作台。"),
            )
            .child(
                h_flex()
                    .gap_3()
                    .child(
                        Button::new("new-welcome")
                            .primary()
                            .icon(UiIcon::Plus)
                            .label("新建连接")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_profile(ConnectionProfile::new(Protocol::Ssh), window, cx)
                            })),
                    )
                    .child(
                        Button::new("open-sql")
                            .icon(UiIcon::FileCode)
                            .label("打开 SQL 草稿")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.tab = 2;
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .mt_6()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(
                        "当前可管理本地连接配置、编辑 SQL 草稿。SSH、SFTP 和数据库连接正在开发。",
                    ),
            )
    }

    /// 功能：非秘密连接编辑页，不接受密码输入；时间：2026-11-10；作者：lq。
    fn connection_page(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let mut protocols = h_flex().gap_2();
        for protocol in Protocol::ALL {
            protocols = protocols.child(
                Button::new(protocol.label())
                    .small()
                    .icon(protocol_icon(protocol))
                    .label(protocol.label())
                    .selected(self.form.profile.protocol == protocol)
                    .disabled(self.saving)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if this.form.profile.protocol == protocol {
                            return;
                        }
                        this.form.profile.protocol = protocol;
                        this.form.port.update(cx, |state, cx| {
                            state.set_value(protocol.default_port().to_string(), window, cx)
                        });
                        this.form_dirty = true;
                        cx.notify();
                    })),
            );
        }
        div()
            .id("form-scroll")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .child(
                v_flex()
                    .p_8()
                    .gap_5()
                    .max_w(px(840.))
                    .child(
                        h_flex()
                            .justify_between()
                            .items_center()
                            .child(div().text_xl().font_semibold().child(
                                if self.profiles.iter().any(|p| p.id == self.form.profile.id) {
                                    "编辑连接"
                                } else {
                                    "新建连接"
                                },
                            ))
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("未连接"),
                            ),
                    )
                    .child(protocols)
                    .child(
                        h_flex()
                            .gap_4()
                            .child(self.field("连接名称", &self.form.name))
                            .child(self.field("分组", &self.form.group)),
                    )
                    .child(
                        h_flex()
                            .gap_4()
                            .child(self.field("主机", &self.form.host))
                            .child(self.field("端口", &self.form.port)),
                    )
                    .child(
                        h_flex()
                            .gap_4()
                            .child(self.field("用户名", &self.form.username))
                            .child(self.field("数据库（MySQL / PostgreSQL）", &self.form.database)),
                    )
                    .child(
                        h_flex()
                            .gap_3()
                            .child(
                                Button::new("favorite")
                                    .ghost()
                                    .small()
                                    .icon(UiIcon::Star)
                                    .label(if self.form.profile.favorite {
                                        "已收藏"
                                    } else {
                                        "收藏"
                                    })
                                    .disabled(self.saving)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.form.profile.favorite = !this.form.profile.favorite;
                                        this.form_dirty = true;
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("production")
                                    .ghost()
                                    .small()
                                    .label(if self.form.profile.production {
                                        "● 生产环境"
                                    } else {
                                        "○ 非生产环境"
                                    })
                                    .disabled(self.saving)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.form.profile.production =
                                            !this.form.profile.production;
                                        this.form_dirty = true;
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .border_t_1()
                            .border_color(cx.theme().border)
                            .pt_5()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child("此处保存地址和用户名。认证与连接功能尚未接入。"),
                    )
                    .child(
                        h_flex()
                            .gap_3()
                            .items_center()
                            .child(
                                Button::new("save-profile")
                                    .primary()
                                    .icon(UiIcon::Save)
                                    .label(if self.saving {
                                        "保存中…"
                                    } else {
                                        "保存连接"
                                    })
                                    .disabled(!self.ready || self.loading || self.saving)
                                    .on_click(cx.listener(|this, _, _, cx| this.save(cx))),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(if self.form_dirty {
                                        "有未保存的修改"
                                    } else {
                                        "保存后仍保持断开"
                                    }),
                            ),
                    ),
            )
    }

    /// 功能：Kit SQL 编辑器与复制出口；不提供虚假的查询执行；时间：2026-11-10；作者：lq。
    fn sql_page(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        v_flex()
            .size_full()
            .child(
                h_flex()
                    .h_12()
                    .px_4()
                    .gap_3()
                    .items_center()
                    .justify_between()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child("SQL 草稿 · 仅本次会话 · 未连接数据库"),
                    )
                    .child(
                        Button::new("copy-sql")
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
    }
}

/// 功能：统一主题的标题栏、侧栏、Kit 标签和状态栏；时间：2026-11-10；作者：lq。
impl Render for Workbench {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let page = match self.tab {
            1 => self.connection_page(cx).into_any_element(),
            2 => self.sql_page(cx).into_any_element(),
            _ => self.welcome(cx).into_any_element(),
        };
        v_flex()
            .size_full()
            .bg(cx.theme().background)
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
                                .icon(if cx.theme().mode.is_dark() {
                                    UiIcon::Sun
                                } else {
                                    UiIcon::Moon
                                })
                                .label(if cx.theme().mode.is_dark() {
                                    "浅色"
                                } else {
                                    "深色"
                                })
                                .on_click(|_, window, cx| {
                                    let mode = if cx.theme().mode.is_dark() {
                                        ThemeMode::Light
                                    } else {
                                        ThemeMode::Dark
                                    };
                                    Theme::change(mode, Some(window), cx);
                                }),
                        ),
                ),
            )
            .child(
                h_flex().flex_1().min_h_0().child(self.sidebar(cx)).child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .child(
                            TabBar::new("workspace-tabs")
                                .underline()
                                .selected_index(self.tab)
                                // 功能：Kit 的 icon 槽为纯图标模式，使用 prefix 保留标签文字；时间：2026-11-10；作者：lq。
                                .child(
                                    Tab::new()
                                        .prefix(Icon::new(UiIcon::PanelLeft).small())
                                        .label("工作区"),
                                )
                                .child(Tab::new().prefix(Icon::new(UiIcon::Server).small()).label(
                                    if self.form_dirty {
                                        "连接配置 •"
                                    } else {
                                        "连接配置"
                                    },
                                ))
                                .child(
                                    Tab::new()
                                        .prefix(Icon::new(UiIcon::FileCode).small())
                                        .label(if self.sql_dirty {
                                            "SQL 草稿 •"
                                        } else {
                                            "SQL 草稿"
                                        }),
                                )
                                .on_click(cx.listener(|this, index: &usize, _, cx| {
                                    this.tab = *index;
                                    cx.notify();
                                })),
                        )
                        .child(page),
                ),
            )
            .child(
                h_flex()
                    .min_h(px(30.))
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
                    .child(self.status.clone())
                    .child(
                        Button::new("reload")
                            .ghost()
                            .xsmall()
                            .icon(UiIcon::RefreshCw)
                            .label("重新读取")
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
        options.window_bounds = Some(WindowBounds::centered(size(px(1440.), px(900.)), cx));
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
                            state.open_profile(ConnectionProfile::new(Protocol::Ssh), window, cx)
                        });
                    });
                }
            });
            let weak = view.downgrade();
            cx.on_action(move |_: &SaveConnection, cx| {
                let _ = weak.update(cx, |state, cx| {
                    if state.tab == 1 {
                        state.save(cx);
                    } else {
                        state.status = "SQL 草稿仅保留本次会话，请使用「复制 SQL」另行保存".into();
                        cx.notify();
                    }
                });
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
pub use smoke::run_ui_smoke;

// 功能：独立验证 Kit 树、虚拟表和停靠组件，不接触正式配置；时间：2026-11-10；作者：lq。
#[cfg(any(feature = "component-lab", feature = "ui-tests"))]
mod component_lab;
#[cfg(feature = "component-lab")]
pub use component_lab::run_component_lab;
#[cfg(feature = "ui-tests")]
pub use component_lab::run_component_smoke;
