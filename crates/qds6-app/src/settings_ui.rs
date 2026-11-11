//! 功能：原生设置页、后台偏好读写、版本检查与关于信息；时间：2026-11-11；作者：lq。

use super::*;
use qds6_services::updates::{self, UpdateCheck};
use qds6_storage::{AppPreferences, ColorTheme};

// 功能：原生外观专项验收仅进入显式测试构建；时间：2026-11-11；作者：lq。
#[cfg(feature = "ui-tests")]
pub(crate) mod appearance_smoke;

/// 功能：设置读写与更新检查独立管理，避免取消业务任务；时间：2026-11-11；作者：lq。
pub(super) struct SettingsUi {
    pub(super) preferences: AppPreferences,
    pub(super) ready: bool,
    pub(super) saving: bool,
    pub(super) message: SharedString,
    pub(super) checking: bool,
    pub(super) update_message: SharedString,
    pub(super) available: bool,
    qq_copied: bool,
    task: Option<Task<()>>,
    update_task: Option<Task<()>>,
    // 功能：独立保留系统外观订阅，替换连接表单不取消监听；时间：2026-11-11；作者：lq。
    appearance_subscription: Option<Subscription>,
}

impl Default for SettingsUi {
    /// 功能：首次进入展示加载状态，不在启动时联网；时间：2026-11-11；作者：lq。
    fn default() -> Self {
        Self {
            preferences: AppPreferences::default(),
            ready: false,
            saving: false,
            message: "正在读取设置…".into(),
            checking: false,
            update_message: "点击检查 GitHub 上的最新正式版本。".into(),
            available: false,
            qq_copied: false,
            task: None,
            update_task: None,
            appearance_subscription: None,
        }
    }
}

impl Workbench {
    /// 功能：仅自动模式响应系统外观变化，订阅随设置视图销毁；时间：2026-11-11；作者：lq。
    pub(super) fn watch_system_appearance(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings.appearance_subscription =
            Some(cx.observe_window_appearance(window, |this, window, cx| {
                if this.settings.preferences.theme == ColorTheme::System {
                    Theme::sync_system_appearance(Some(window), cx);
                    this.sync_terminal_appearance(cx);
                    cx.notify();
                }
            }));
    }

    /// 功能：当前和新建终端共用亮度解析，自动模式跟随软件的实际外观；时间：2026-11-11；作者：lq。
    pub(super) fn resolved_terminal_theme(&self, cx: &App) -> qds6_core::TerminalTheme {
        let preferences = &self.settings.preferences;
        if preferences.terminal_follow_appearance {
            preferences
                .terminal_theme
                .for_appearance(cx.theme().mode == ThemeMode::Dark)
        } else {
            preferences.terminal_theme
        }
    }

    /// 功能：只更新变化的终端配色，不改临时字号、输出或 SSH 会话；时间：2026-11-11；作者：lq。
    fn sync_terminal_appearance(&self, cx: &mut Context<Self>) {
        let theme = self.resolved_terminal_theme(cx);
        for server in &self.services.servers {
            if let Some(terminal) = server.read(cx).terminal.clone() {
                terminal.update(cx, |terminal, cx| terminal.set_theme(theme, cx));
            }
        }
    }

    /// 功能：可点击的四套终端配色预览，浅深色固定各两套且独立保存；时间：2026-11-11；作者：lq。
    fn terminal_theme_picker(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let mut choices = h_flex().w_full().gap_3();
        for theme in qds6_core::TerminalTheme::ALL {
            let palette = theme.palette();
            let selected = self.resolved_terminal_theme(cx) == theme;
            choices = choices.child(
                Button::new(theme.id())
                    .flex_1()
                    .min_w_0()
                    .h(px(142.))
                    .p_2()
                    .selected(selected)
                    .disabled(!self.settings.ready || self.settings.saving)
                    .accessibility_label(format!(
                        "{}终端主题，{}{}",
                        theme.label(),
                        if theme.is_dark() { "深色" } else { "浅色" },
                        if selected { "，已选择" } else { "" }
                    ))
                    .child(
                        v_flex()
                            .w_full()
                            .gap_2()
                            .child(
                                h_flex()
                                    .w_full()
                                    .justify_between()
                                    .items_center()
                                    .child(div().text_sm().child(format!(
                                        "{} · {}",
                                        theme.label(),
                                        if theme.is_dark() { "深色" } else { "浅色" }
                                    )))
                                    .child(
                                        Icon::new(if selected {
                                            UiIcon::Check
                                        } else {
                                            UiIcon::Circle
                                        })
                                        .xsmall(),
                                    ),
                            )
                            .child(
                                v_flex()
                                    .w_full()
                                    .h(px(92.))
                                    .rounded_sm()
                                    .p_2()
                                    .gap_1()
                                    .bg(rgb(palette.background))
                                    .text_color(rgb(palette.foreground))
                                    .font_family("Menlo")
                                    .text_size(px(11.))
                                    .child(
                                        h_flex()
                                            .gap_1()
                                            .child(
                                                div().text_color(rgb(palette.ansi[2])).child("~ ❯"),
                                            )
                                            .child("cargo build"),
                                    )
                                    .child(
                                        div()
                                            .text_color(rgb(palette.ansi[4]))
                                            .child("src/  Cargo.toml"),
                                    )
                                    .child(
                                        div()
                                            .text_color(rgb(palette.ansi[2]))
                                            .child("✓ Finished  0.42s"),
                                    )
                                    .child(h_flex().gap_1().children((1..7).map(move |index| {
                                        div()
                                            .flex_1()
                                            .h(px(5.))
                                            .rounded_sm()
                                            .bg(rgb(palette.ansi[index]))
                                    }))),
                            ),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        let mut preferences = this.settings.preferences.clone();
                        preferences.terminal_theme = theme;
                        // 功能：点击具体配色即固定，重新勾选联动即可恢复自动切换；时间：2026-11-11；作者：lq。
                        preferences.terminal_follow_appearance = false;
                        this.save_preferences(preferences, window, cx);
                    })),
            );
        }
        v_flex()
            .gap_3()
            .pt_4()
            .border_t_1()
            .border_color(cx.theme().border)
            .child(Self::section_heading(
                "终端配色",
                "默认随软件切换深浅色；选择下方配色可固定使用。",
                cx,
            ))
            // 功能：显式联动开关，兼容偏好固定终端背景的使用方式；时间：2026-11-11；作者：lq。
            .child(
                Checkbox::new("terminal-follow-appearance")
                    .label("跟随软件深浅色")
                    .checked(self.settings.preferences.terminal_follow_appearance)
                    .disabled(!self.settings.ready || self.settings.saving)
                    .on_click(cx.listener(|this, checked: &bool, window, cx| {
                        let mut preferences = this.settings.preferences.clone();
                        // 功能：关闭联动时固定当前可见颜色，避免开关本身造成跳色；时间：2026-11-11；作者：lq。
                        preferences.terminal_theme = this.resolved_terminal_theme(cx);
                        preferences.terminal_follow_appearance = *checked;
                        this.save_preferences(preferences, window, cx);
                    })),
            )
            .child(choices)
    }

    /// 功能：后台读取偏好并应用主题与编辑器设置，不接触系统凭据；时间：2026-11-11；作者：lq。
    pub(super) fn load_preferences(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let storage = self.storage_path.clone();
        self.settings.task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    let path = storage.map(Ok).unwrap_or_else(Self::store_path)?;
                    ProfileStore::open(path)
                        .and_then(|store| store.preferences())
                        .map_err(|e| e.to_string())
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                match result {
                    Ok(preferences) => {
                        this.apply_preferences(preferences, window, cx);
                        this.settings.ready = true;
                        this.settings.message = "更改后自动保存到本机。".into();
                    }
                    Err(error) => this.settings.message = format!("读取设置失败：{error}").into(),
                }
                cx.notify();
            });
        }));
    }

    /// 功能：主题、SQL 换行与所有现有终端字号即时生效，保留草稿及连接；时间：2026-11-11；作者：lq。
    fn apply_preferences(
        &mut self,
        preferences: AppPreferences,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // 功能：自动模式读取当前原生窗口外观，手动模式不受系统变化覆盖；时间：2026-11-11；作者：lq。
        match preferences.theme {
            ColorTheme::Light => Theme::change(ThemeMode::Light, Some(window), cx),
            ColorTheme::Dark => Theme::change(ThemeMode::Dark, Some(window), cx),
            ColorTheme::System => Theme::sync_system_appearance(Some(window), cx),
        }
        // 功能：只应用实际变化的编辑偏好，切换主题不重置终端临时缩放；时间：2026-11-11；作者：lq。
        if !self.settings.ready
            || preferences.sql_soft_wrap != self.settings.preferences.sql_soft_wrap
        {
            self.sql.update(cx, |sql, cx| {
                sql.set_soft_wrap(preferences.sql_soft_wrap, window, cx)
            });
        }
        if !self.settings.ready
            || preferences.terminal_font_size != self.settings.preferences.terminal_font_size
        {
            for server in &self.services.servers {
                if let Some(terminal) = server.read(cx).terminal.clone() {
                    terminal.update(cx, |terminal, cx| {
                        terminal.set_font_size(preferences.terminal_font_size, cx)
                    });
                }
            }
        }
        self.settings.preferences = preferences;
        self.sync_terminal_appearance(cx);
    }

    /// 功能：后台原子保存，成功后应用，失败保留原偏好并明确反馈；时间：2026-11-11；作者：lq。
    pub(super) fn save_preferences(
        &mut self,
        preferences: AppPreferences,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.settings.ready || self.settings.saving {
            return;
        }
        self.settings.saving = true;
        self.settings.message = "正在保存设置…".into();
        let storage = self.storage_path.clone();
        self.settings.task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    let path = storage.map(Ok).unwrap_or_else(Self::store_path)?;
                    ProfileStore::open(path)
                        .and_then(|store| store.save_preferences(&preferences))
                        .map_err(|e| e.to_string())?;
                    Ok::<_, String>(preferences)
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.settings.saving = false;
                match result {
                    Ok(preferences) => {
                        this.apply_preferences(preferences, window, cx);
                        this.settings.message = "设置已保存。".into();
                    }
                    Err(error) => {
                        this.settings.message = format!("保存失败：{error}，请重试。").into()
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// 功能：用户主动检查，禁止重复请求，结果仅更新设置区域；时间：2026-11-11；作者：lq。
    fn check_for_updates(&mut self, cx: &mut Context<Self>) {
        if self.settings.checking {
            return;
        }
        self.settings.checking = true;
        self.settings.available = false;
        self.settings.update_message = "正在检查 GitHub Releases…".into();
        self.settings.update_task = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async { updates::check_latest(env!("CARGO_PKG_VERSION")) })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.settings.checking = false;
                this.settings.update_message = match result {
                    Ok(UpdateCheck::Available(version)) => {
                        this.settings.available = true;
                        format!("发现新版本 {version}，可前往发布页下载。")
                    }
                    Ok(UpdateCheck::Current(version)) => {
                        format!("当前版本无需更新（最新正式发布 {version}）。")
                    }
                    Ok(UpdateCheck::Unpublished) => {
                        "未找到公开的正式发布，暂时无法比较更新。".into()
                    }
                    Err(error) => format!("{error}。也可直接打开发布页。"),
                }
                .into();
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// 功能：设置行统一左右布局，文字可收缩而操作按钮保持可点击；时间：2026-11-11；作者：lq。
    fn preference_row<C: IntoElement>(
        title: &'static str,
        description: &'static str,
        control: C,
        cx: &App,
    ) -> impl IntoElement + use<C> {
        h_flex()
            .w_full()
            .gap_4()
            .py_3()
            .items_center()
            .justify_between()
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_1()
                    .child(div().font_medium().child(title))
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(description),
                    ),
            )
            .child(div().flex_shrink_0().child(control))
    }

    /// 功能：原生可滚动设置页，在最小窗口展示常用偏好、版本与关于信息；时间：2026-11-11；作者：lq。
    pub(super) fn settings_page(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let disabled = !self.settings.ready || self.settings.saving;
        let preferences = &self.settings.preferences;
        let theme = TabBar::new("settings-theme")
            .segmented()
            .small()
            // 功能：选中保存的模式，自动模式不会因系统浅深色而跳到手动选项；时间：2026-11-11；作者：lq。
            .selected_index(match preferences.theme {
                ColorTheme::Light => 0,
                ColorTheme::Dark => 1,
                ColorTheme::System => 2,
            })
            .child(Tab::new().label("浅色").disabled(disabled))
            .child(Tab::new().label("深色").disabled(disabled))
            .child(Tab::new().label("自动").disabled(disabled))
            .on_click(cx.listener(|this, index: &usize, window, cx| {
                let mut preferences = this.settings.preferences.clone();
                preferences.theme = match *index {
                    0 => ColorTheme::Light,
                    1 => ColorTheme::Dark,
                    _ => ColorTheme::System,
                };
                this.save_preferences(preferences, window, cx);
            }));
        let font = h_flex()
            .gap_2()
            .items_center()
            .child(
                Button::new("terminal-font-decrease")
                    .small()
                    .label("−")
                    .disabled(disabled || preferences.terminal_font_size <= 10)
                    .on_click(cx.listener(|this, _, window, cx| {
                        let mut preferences = this.settings.preferences.clone();
                        preferences.terminal_font_size =
                            preferences.terminal_font_size.saturating_sub(1).max(10);
                        this.save_preferences(preferences, window, cx);
                    })),
            )
            .child(
                div()
                    .w(px(44.))
                    .text_center()
                    .child(format!("{} px", preferences.terminal_font_size)),
            )
            .child(
                Button::new("terminal-font-increase")
                    .small()
                    .label("+")
                    .disabled(disabled || preferences.terminal_font_size >= 24)
                    .on_click(cx.listener(|this, _, window, cx| {
                        let mut preferences = this.settings.preferences.clone();
                        preferences.terminal_font_size =
                            (preferences.terminal_font_size + 1).min(24);
                        this.save_preferences(preferences, window, cx);
                    })),
            );
        let wrap = Checkbox::new("settings-sql-wrap")
            .label("自动换行")
            .checked(preferences.sql_soft_wrap)
            .disabled(disabled)
            .on_click(cx.listener(|this, checked: &bool, window, cx| {
                let mut preferences = this.settings.preferences.clone();
                preferences.sql_soft_wrap = *checked;
                this.save_preferences(preferences, window, cx);
            }));
        v_flex()
            .id("settings-scroll")
            .test_support()
            .size_full()
            .overflow_y_scroll()
            .child(
                v_flex()
                    .w_full()
                    .max_w(px(900.))
                    .p_6()
                    .gap_5()
                    .child(
                        v_flex()
                            .gap_1()
                            .child(div().text_xl().font_semibold().child("设置"))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(self.settings.message.clone()),
                            )
                            .when(!self.settings.ready, |v| {
                                v.child(
                                    Button::new("reload-preferences")
                                        .small()
                                        .label("重新读取设置")
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.load_preferences(window, cx)
                                        })),
                                )
                            }),
                    )
                    .child(
                        v_flex()
                            .gap_1()
                            .child(Self::section_heading("常用设置", "", cx))
                            .child(Self::preference_row(
                                "界面主题",
                                "选择浅色、深色，或自动跟随系统外观。",
                                theme,
                                cx,
                            ))
                            .child(Self::preference_row(
                                "终端字号",
                                "应用到当前终端和新建终端，支持 10–24 px。",
                                font,
                                cx,
                            ))
                            .child(Self::preference_row(
                                "SQL 编辑器",
                                "长语句按编辑区宽度自动换行。",
                                wrap,
                                cx,
                            )),
                    )
                    .child(self.terminal_theme_picker(cx))
                    .child(
                        v_flex()
                            .gap_3()
                            .pt_4()
                            .border_t_1()
                            .border_color(cx.theme().border)
                            .child(Self::section_heading("版本与更新", "", cx))
                            .child(
                                h_flex()
                                    .gap_4()
                                    .items_center()
                                    .justify_between()
                                    .child(
                                        div().child(format!(
                                            "当前版本  {}",
                                            env!("CARGO_PKG_VERSION")
                                        )),
                                    )
                                    .child(
                                        Button::new("check-for-updates")
                                            .small()
                                            .icon(UiIcon::RefreshCw)
                                            .label("检查更新")
                                            .loading(self.settings.checking)
                                            .disabled(self.settings.checking)
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.check_for_updates(cx)
                                            })),
                                    ),
                            )
                            .child(
                                div()
                                    .id("update-message")
                                    .test_support()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(self.settings.update_message.clone()),
                            )
                            .child(
                                h_flex().child(
                                    Button::new("open-releases")
                                        .small()
                                        .ghost()
                                        .label(if self.settings.available {
                                            "下载新版本 ↗"
                                        } else {
                                            "查看发布页 ↗"
                                        })
                                        .on_click(|_, _, cx| cx.open_url(updates::RELEASES_URL)),
                                ),
                            ),
                    )
                    .child(
                        v_flex()
                            .gap_4()
                            .pt_4()
                            .border_t_1()
                            .border_color(cx.theme().border)
                            .child(Self::section_heading("关于 QDS6", "", cx))
                            .child(
                                h_flex()
                                    .items_start()
                                    .gap_6()
                                    .child(
                                        v_flex()
                                            .flex_1()
                                            .min_w_0()
                                            .gap_3()
                                            .child(
                                                h_flex()
                                                    .gap_3()
                                                    .items_center()
                                                    .child(img(BRAND_IMAGE).size(px(40.)))
                                                    .child(
                                                        div()
                                                            .text_lg()
                                                            .font_semibold()
                                                            .child("QDS6"),
                                                    ),
                                            )
                                            .child(div().child(
                                                "面向开发者的 SSH、SFTP 与数据库桌面工作区。",
                                            ))
                                            .child(div().font_medium().child("非人类程序员出品"))
                                            .child(
                                                h_flex()
                                                    .gap_2()
                                                    .items_center()
                                                    .child(div().child("QQ：960661824"))
                                                    .child(
                                                        Button::new("copy-about-qq")
                                                            .ghost()
                                                            .xsmall()
                                                            .icon(UiIcon::Copy)
                                                            .label(if self.settings.qq_copied {
                                                                "已复制"
                                                            } else {
                                                                "复制"
                                                            })
                                                            .tooltip("复制 QQ 号")
                                                            .on_click(cx.listener(
                                                                |this, _, _, cx| {
                                                                    cx.write_to_clipboard(
                                                                        ClipboardItem::new_string(
                                                                            "960661824".into(),
                                                                        ),
                                                                    );
                                                                    this.settings.qq_copied = true;
                                                                    cx.notify();
                                                                },
                                                            )),
                                                    ),
                                            )
                                            .child(
                                                h_flex().child(
                                                    Button::new("open-source-repository")
                                                        .ghost()
                                                        .small()
                                                        .label("开源仓库  github.com/lq0910/qds6 ↗")
                                                        .on_click(|_, _, cx| {
                                                            cx.open_url(updates::REPOSITORY_URL)
                                                        }),
                                                ),
                                            ),
                                    )
                                    .child(
                                        v_flex()
                                            .flex_shrink_0()
                                            .items_center()
                                            .gap_2()
                                            // 功能：保留原图白底和静区，深色模式也可扫码；时间：2026-11-11；作者：lq。
                                            .child(
                                                div()
                                                    .id("about-wechat-qr")
                                                    .test_support()
                                                    .size(px(144.))
                                                    .bg(rgb(0xffffff))
                                                    .child(img(assets::WECHAT_QR).size(px(144.))),
                                            )
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(cx.theme().muted_foreground)
                                                    .child("微信扫码关注公众号"),
                                            ),
                                    ),
                            ),
                    ),
            )
    }
}
