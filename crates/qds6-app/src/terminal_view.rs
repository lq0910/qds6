//! 功能：原生终端网格、键盘及 IME 组合输入，使用 Alacritty 状态机；时间：2026-11-10；作者：lq。

use gpui_kit::{
    component::{
        Disableable, Sizable,
        button::{Button, ButtonVariants},
        h_flex,
        input::{Input, InputEvent as SearchInputEvent, InputState},
        v_flex,
    },
    *,
};
// 功能：按需展开搜索条的原生构建扩展；时间：2026-11-11；作者：lq。
use gpui_kit::prelude::FluentBuilder;
use qds6_core::TerminalTheme;
use qds6_services::terminal::TerminalSession;
use std::{ops::Range, time::Duration};

// 功能：仅验收构建编译终端原生鼠标与剪贴板回归；时间：2026-11-16；作者：lq。
#[cfg(feature = "ui-tests")]
pub(crate) mod smoke;

/// 功能：单终端视图与原生输入焦点独立管理；时间：2026-11-10；作者：lq。
pub struct TerminalView {
    session: TerminalSession,
    focus: FocusHandle,
    preedit: String,
    selection: Range<usize>,
    // 功能：输出选区与 IME 组合文本选区独立，命中检测使用实际画布边界；时间：2026-11-16；作者：lq。
    grid_bounds: Option<Bounds<Pixels>>,
    selecting_output: bool,
    columns: u16,
    rows: u16,
    font_size: f32,
    // 功能：软件外观与终端配色独立，光标和搜索使用同一主题；时间：2026-11-11；作者：lq。
    theme: TerminalTheme,
    status: String,
    _refresh: Task<()>,
    search: Entity<InputState>,
    // 功能：终端搜索按需显示，默认把空间留给命令输出；时间：2026-11-11；作者：lq。
    show_search: bool,
    // 功能：搜索任务独立于 SSH 输入与渲染，状态只显示在搜索条；时间：2026-11-27；作者：lq。
    search_task: Option<Task<()>>,
    search_status: &'static str,
    _search_subscription: Subscription,
}

impl TerminalView {
    /// 功能：更新已有网格的主题，保留终端字号、输入焦点和屏幕内容；时间：2026-11-11；作者：lq。
    pub(super) fn set_theme(&mut self, theme: TerminalTheme, cx: &mut Context<Self>) {
        // 功能：联动同步重复调用时不触发无意义的网格重绘；时间：2026-11-11；作者：lq。
        if self.theme == theme {
            return;
        }
        self.theme = theme;
        self.session
            .model
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .set_theme(theme);
        cx.notify();
    }
    /// 功能：全局字号偏好应用到现有和新建终端，下一帧同步 PTY 尺寸；时间：2026-11-11；作者：lq。
    pub(super) fn set_font_size(&mut self, size: u8, cx: &mut Context<Self>) {
        self.font_size = f32::from(size.clamp(10, 24));
        cx.notify();
    }
    /// 功能：工具栏和查找快捷键共用终端搜索开关，关闭时恢复终端焦点；时间：2026-11-11；作者：lq。
    pub(super) fn toggle_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_search = !self.show_search;
        if self.show_search {
            self.search.update(cx, |input, cx| input.focus(window, cx));
        } else {
            self.search
                .update(cx, |input, cx| input.set_value("", window, cx));
            window.focus(&self.focus, cx);
        }
        cx.notify();
    }

    /// 功能：后台查找滚屏文字，避免较长历史的扫描阻塞界面；时间：2026-11-27；作者：lq。
    fn find_output(&mut self, previous: bool, cx: &mut Context<Self>) {
        let query = self.search.read(cx).value().to_string();
        if self.search_task.is_some() || query.is_empty() {
            return;
        }
        self.search_status = "正在搜索…";
        let model = self.session.model.clone();
        self.search_task = Some(cx.spawn(async move |view, cx| {
            let requested = query.clone();
            let found = cx
                .background_spawn(async move {
                    model
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .find_output(&requested, previous)
                })
                .await;
            let _ = view.update(cx, |this, cx| {
                this.search_task = None;
                if this.show_search && this.search.read(cx).value().as_str() == query {
                    this.search_status = if found { "已定位" } else { "无匹配" };
                } else {
                    this.session
                        .model
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .clear_search();
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// 功能：让文件管理器读取同一终端的当前目录或将焦点交还给终端；时间：2026-11-27；作者：lq。
    pub(super) fn reported_directory(&self) -> Option<String> {
        self.session
            .model
            .try_lock()
            .ok()?
            .reported_directory()
            .map(str::to_owned)
    }
    /// 功能：验收读取可见终端文本，不进入发行 API；时间：2026-11-10；作者：lq。
    #[cfg(feature = "ui-tests")]
    pub(crate) fn screen_text(&self) -> String {
        self.session
            .model
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .text()
    }
    /// 功能：原生交互验收读取最终配色及字号，发行构建不暴露；时间：2026-11-11；作者：lq。
    #[cfg(feature = "ui-tests")]
    pub(crate) fn appearance(&self) -> (TerminalTheme, f32) {
        (self.theme, self.font_size)
    }
    /// 功能：批次刷新终端，UI 不执行网络操作；时间：2026-11-10；作者：lq。
    pub fn new(session: TerminalSession, window: &mut Window, cx: &mut Context<Self>) -> Self {
        // 功能：仅在收到输出或后台状态变化后请求重绘；时间：2026-11-27；作者：lq。
        let model = session.model.clone();
        let refresh = cx.spawn(async move |this, cx| {
            let mut revision = 0;
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(33))
                    .await;
                let Ok(terminal) = model.try_lock() else {
                    continue;
                };
                let changed = terminal.revision();
                drop(terminal);
                if this
                    .update(cx, |_, cx| {
                        if changed != revision {
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    break;
                }
                revision = changed;
            }
        });
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("搜索终端输出和历史"));
        let subscription = cx.subscribe(
            &search,
            |this, _, event: &SearchInputEvent, cx| match event {
                SearchInputEvent::Change => {
                    this.search_status = "";
                    if let Ok(mut model) = this.session.model.try_lock() {
                        model.clear_search();
                    }
                    cx.notify();
                }
                SearchInputEvent::PressEnter { secondary, shift } => {
                    this.find_output(*shift || *secondary, cx);
                }
                _ => {}
            },
        );
        Self {
            session,
            focus: cx.focus_handle(),
            preedit: String::new(),
            selection: 0..0,
            grid_bounds: None,
            selecting_output: false,
            columns: 100,
            rows: 30,
            font_size: 14.,
            theme: TerminalTheme::default(),
            status: String::new(),
            _refresh: refresh,
            search,
            show_search: false,
            search_task: None,
            search_status: "",
            _search_subscription: subscription,
        }
    }

    /// 功能：输入失败可见，不在重连后重放；时间：2026-11-10；作者：lq。
    fn send(&mut self, bytes: Vec<u8>, cx: &mut Context<Self>) {
        self.clear_output_selection();
        if let Err(error) = self.session.input(bytes) {
            self.status = error;
        }
        cx.notify();
    }

    /// 功能：按与绘制相同的网格尺寸命中字符及半格，越界拖动钳制到可见边缘；时间：2026-11-16；作者：lq。
    fn output_position(&self, position: Point<Pixels>) -> Option<(usize, usize, bool)> {
        let bounds = self.grid_bounds?;
        let column = (f32::from(position.x - bounds.origin.x) / (self.font_size * 0.6))
            .clamp(0., f32::from(self.columns) - 0.001);
        let row = (f32::from(position.y - bounds.origin.y) / (self.font_size * 1.4))
            .clamp(0., f32::from(self.rows) - 1.);
        Some((row as usize, column as usize, column.fract() >= 0.5))
    }

    /// 功能：单击清除旧选区并开始拖选，双击选词、三击选行；时间：2026-11-16；作者：lq。
    fn begin_output_selection(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus, cx);
        if let Some((row, column, right)) = self.output_position(event.position) {
            self.session
                .model
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .begin_selection(row, column, right, event.click_count);
            self.selecting_output = true;
            cx.notify();
        }
    }

    /// 功能：窗口级拖动和释放共用终点更新，避免拖出画布后选区卡住；时间：2026-11-16；作者：lq。
    fn update_output_selection(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        if self.selecting_output
            && let Some((row, column, right)) = self.output_position(position)
        {
            self.session
                .model
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .update_selection(row, column, right);
            cx.notify();
        }
    }

    /// 功能：发送输入或粘贴时退出输出选择，不影响 IME 组合文本；时间：2026-11-16；作者：lq。
    fn clear_output_selection(&mut self) {
        self.selecting_output = false;
        self.session
            .model
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear_selection();
    }

    /// 功能：快捷键与原生编辑菜单共用选区复制，无选区时保留剪贴板；时间：2026-11-16；作者：lq。
    fn copy_output(&mut self, whole_screen: bool, cx: &mut Context<Self>) {
        let model = self.session.model.lock().unwrap_or_else(|e| e.into_inner());
        let text = if whole_screen {
            Some(model.text())
        } else {
            model.selected_text()
        };
        if let Some(text) = text {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    /// 功能：处理 PTY 控制键、应用光标键及复制粘贴；普通字符交由原生 IME；时间：2026-11-10；作者：lq。
    fn key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let key = &event.keystroke;
        if !self.preedit.is_empty() {
            return;
        }
        if key.modifiers.platform {
            match key.key.as_str() {
                "f" => self.toggle_search(window, cx),
                "c" => self.copy_output(key.modifiers.shift, cx),
                "v" => {
                    self.clear_output_selection();
                    if let Some(text) = cx.read_from_clipboard().and_then(|v| v.text())
                        && let Err(error) = self.session.paste(&text)
                    {
                        self.status = error;
                    }
                }
                "+" | "=" => self.font_size = (self.font_size + 1.).min(24.),
                "-" => self.font_size = (self.font_size - 1.).max(10.),
                _ => return,
            }
            cx.stop_propagation();
            cx.notify();
            return;
        }
        let bytes = if key.modifiers.control && key.key.len() == 1 {
            let byte = key.key.as_bytes()[0].to_ascii_lowercase();
            if (b'@'..=b'_').contains(&byte.to_ascii_uppercase()) {
                vec![byte.to_ascii_uppercase() & 0x1f]
            } else {
                return;
            }
        } else {
            let application_cursor = self
                .session
                .model
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .application_cursor();
            let sequence = match key.key.as_str() {
                "enter" => "\r",
                "backspace" => "\x7f",
                "tab" => "\t",
                "escape" => "\x1b",
                "up" if application_cursor => "\x1bOA",
                "down" if application_cursor => "\x1bOB",
                "right" if application_cursor => "\x1bOC",
                "left" if application_cursor => "\x1bOD",
                "home" if application_cursor => "\x1bOH",
                "end" if application_cursor => "\x1bOF",
                "up" => "\x1b[A",
                "down" => "\x1b[B",
                "right" => "\x1b[C",
                "left" => "\x1b[D",
                "home" => "\x1b[H",
                "end" => "\x1b[F",
                "delete" => "\x1b[3~",
                "pageup" => "\x1b[5~",
                "pagedown" => "\x1b[6~",
                "f1" => "\x1bOP",
                "f2" => "\x1bOQ",
                "f3" => "\x1bOR",
                "f4" => "\x1bOS",
                "f5" => "\x1b[15~",
                "f6" => "\x1b[17~",
                "f7" => "\x1b[18~",
                "f8" => "\x1b[19~",
                "f9" => "\x1b[20~",
                "f10" => "\x1b[21~",
                "f11" => "\x1b[23~",
                "f12" => "\x1b[24~",
                _ => return,
            };
            sequence.as_bytes().to_vec()
        };
        self.send(bytes, cx);
        cx.stop_propagation();
    }
}

impl Focusable for TerminalView {
    /// 功能：将终端键盘输入绑定独立焦点；时间：2026-11-10；作者：lq。
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for TerminalView {
    /// 功能：原生网格逐单元绘制，宽字符占两格，画布 bounds 驱动 PTY resize；时间：2026-11-10；作者：lq。
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let frame = self
            .session
            .model
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .frame();
        let status = if self.status.is_empty() {
            frame.status.clone()
        } else {
            self.status.clone()
        };
        let view = cx.entity();
        let font_size = self.font_size;
        // 功能：组合文字和系统候选窗锚定真实终端游标，不固定在左上角；时间：2026-11-11；作者：lq。
        let cursor = frame
            .rows
            .iter()
            .enumerate()
            .find_map(|(row, cells)| {
                cells
                    .iter()
                    .position(|cell| cell.cursor)
                    .map(|column| (column, row))
            })
            .unwrap_or((0, 0));
        let preedit = self.preedit.clone();
        let show_search = self.show_search;
        let palette = self.theme.palette();
        v_flex()
            .size_full()
            .bg(rgb(palette.background))
            .text_color(rgb(palette.foreground))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    // 功能：网格四周留白，PTY 尺寸、绘制和 IME 共用内层画布边界；时间：2026-11-11；作者：lq。
                    .px_3()
                    .py_2()
                    .overflow_hidden()
                    .id("terminal-grid")
                    .test_support()
                    .track_focus(&self.focus)
                    .key_context("Terminal")
                    .on_key_down(cx.listener(Self::key))
                    // 功能：系统“编辑 → 复制”与终端快捷键行为一致；时间：2026-11-16；作者：lq。
                    .on_action(cx.listener(|this, _: &component::input::Copy, _, cx| {
                        this.copy_output(false, cx);
                        cx.stop_propagation();
                    }))
                    .on_scroll(cx.listener(|this, event: &ScrollEvent, _, cx| {
                        let delta = event.delta.pixel_delta(px(20.)).y;
                        this.session
                            .model
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .scroll((f32::from(delta) / 10.) as i32);
                        cx.notify();
                    }))
                    .child(
                        div()
                            .id("terminal-content")
                            .test_support()
                            .size_full()
                            .cursor_text()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(Self::begin_output_selection),
                            )
                            .child(
                                canvas(
                                    move |bounds, _, cx| {
                                        view.update(cx, |this, _| {
                                            this.grid_bounds = Some(bounds);
                                            let columns = (f32::from(bounds.size.width)
                                                / (font_size * 0.6))
                                                .floor()
                                                .clamp(20., 300.)
                                                as u16;
                                            let rows = (f32::from(bounds.size.height)
                                                / (font_size * 1.4))
                                                .floor()
                                                .clamp(5., 120.)
                                                as u16;
                                            if this.columns != columns || this.rows != rows {
                                                this.columns = columns;
                                                this.rows = rows;
                                                this.session.resize(columns, rows);
                                            }
                                        });
                                        (view.clone(), frame, preedit)
                                    },
                                    move |bounds, (view, frame, preedit), window, cx| {
                                        let focus = view.read(cx).focus.clone();
                                        window.handle_input(
                                            &focus,
                                            ElementInputHandler::new(bounds, view.clone()),
                                            cx,
                                        );
                                        // 功能：捕获窗口级移动及释放，拖出网格后也能结束选区；时间：2026-11-16；作者：lq。
                                        let drag_view = view.clone();
                                        window.on_mouse_event(
                                            move |event: &MouseMoveEvent, phase, _, cx| {
                                                if phase.capture() {
                                                    drag_view.update(cx, |this, cx| {
                                                        if event.pressed_button
                                                            == Some(MouseButton::Left)
                                                        {
                                                            this.update_output_selection(
                                                                event.position,
                                                                cx,
                                                            );
                                                        } else {
                                                            this.selecting_output = false;
                                                        }
                                                    });
                                                }
                                            },
                                        );
                                        let release_view = view.clone();
                                        window.on_mouse_event(
                                            move |event: &MouseUpEvent, phase, _, cx| {
                                                if phase.capture()
                                                    && event.button == MouseButton::Left
                                                {
                                                    release_view.update(cx, |this, cx| {
                                                        this.update_output_selection(
                                                            event.position,
                                                            cx,
                                                        );
                                                        this.selecting_output = false;
                                                    });
                                                }
                                            },
                                        );
                                        let cell_width = font_size * 0.6;
                                        let cell_height = font_size * 1.4;
                                        for (row, cells) in frame.rows.iter().enumerate() {
                                            for (column, cell) in cells.iter().enumerate() {
                                                if cell.spacer {
                                                    continue;
                                                }
                                                let origin = bounds.origin
                                                    + point(
                                                        px(column as f32 * cell_width),
                                                        px(row as f32 * cell_height),
                                                    );
                                                let width =
                                                    px(cell_width
                                                        * if cell.wide { 2. } else { 1. });
                                                // 功能：搜索高亮和光标同步适配浅深色，显式真彩色仍由网格保留；时间：2026-11-11；作者：lq。
                                                let matched = show_search && cell.search_match;
                                                // 功能：沿用面板底色，只有不同背景、光标或高亮才发出绘制单元；时间：2026-11-27；作者：lq。
                                                let background = if cell.selected {
                                                    palette.cursor
                                                } else if matched {
                                                    palette.search_background
                                                } else if cell.cursor {
                                                    palette.cursor
                                                } else {
                                                    cell.background
                                                };
                                                if background != palette.background {
                                                    window.paint_quad(fill(
                                                        Bounds {
                                                            origin,
                                                            size: size(width, px(cell_height)),
                                                        },
                                                        rgb(background),
                                                    ));
                                                }
                                                // 功能：空白单元格仅画背景，跳过无可见字形的布局；时间：2026-11-27；作者：lq。
                                                if cell.text == " " {
                                                    continue;
                                                }
                                                let font = font("Menlo");
                                                let run = TextRun {
                                                    len: cell.text.len(),
                                                    font,
                                                    color: rgb(if cell.selected {
                                                        palette.cursor_text
                                                    } else if matched {
                                                        palette.search_foreground
                                                    } else if cell.cursor {
                                                        palette.cursor_text
                                                    } else {
                                                        cell.foreground
                                                    })
                                                    .into(),
                                                    background_color: None,
                                                    underline: None,
                                                    strikethrough: None,
                                                };
                                                let line = window.text_system().shape_line(
                                                    cell.text.clone().into(),
                                                    px(font_size),
                                                    &[run],
                                                    None,
                                                );
                                                let _ = line.paint(
                                                    origin,
                                                    px(cell_height),
                                                    TextAlign::Left,
                                                    None,
                                                    window,
                                                    cx,
                                                );
                                            }
                                        }
                                        if !preedit.is_empty() {
                                            let run = TextRun {
                                                len: preedit.len(),
                                                font: font("Menlo"),
                                                color: rgb(palette.cursor_text).into(),
                                                background_color: Some(rgb(palette.cursor).into()),
                                                underline: None,
                                                strikethrough: None,
                                            };
                                            let line = window.text_system().shape_line(
                                                preedit.into(),
                                                px(font_size),
                                                &[run],
                                                None,
                                            );
                                            let _ = line.paint(
                                                bounds.origin
                                                    + point(
                                                        px(cursor.0 as f32 * cell_width),
                                                        px(cursor.1 as f32 * cell_height),
                                                    ),
                                                px(cell_height),
                                                TextAlign::Left,
                                                None,
                                                window,
                                                cx,
                                            );
                                        }
                                    },
                                )
                                .size_full(),
                            ),
                    ),
            )
            .when(self.show_search, |v| {
                v.child(
                    h_flex()
                        .px_2()
                        .gap_1()
                        .child(
                            div()
                                .flex_1()
                                .child(Input::new(&self.search).aria_label("终端搜索")),
                        )
                        .child(div().text_xs().child(self.search_status))
                        .children(
                            [(true, "上一个"), (false, "下一个")].map(|(previous, label)| {
                                Button::new(label)
                                    .ghost()
                                    .xsmall()
                                    .label(label)
                                    .disabled(
                                        self.search_task.is_some()
                                            || self.search.read(cx).value().is_empty(),
                                    )
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.find_output(previous, cx)
                                    }))
                            }),
                        )
                        .child(
                            Button::new("close-terminal-search")
                                .ghost()
                                .xsmall()
                                .label("关闭")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.toggle_search(window, cx)
                                })),
                        ),
                )
            })
            .child(h_flex().px_3().py_1().text_xs().child(format!(
                "{status} · {}×{} · 拖选文字后 ⌘C 复制 / ⌘⇧C 复制屏幕 / ⌘V 粘贴 / ⌘± 缩放",
                self.columns, self.rows
            )))
    }
}

impl EntityInputHandler for TerminalView {
    /// 功能：IME 仅访问未提交的组合文本，不能读取远端屏幕秘密；时间：2026-11-10；作者：lq。
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let utf16: Vec<_> = self.preedit.encode_utf16().collect();
        let range = range.start.min(utf16.len())..range.end.min(utf16.len());
        *adjusted = Some(range.clone());
        Some(String::from_utf16_lossy(&utf16[range]))
    }
    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.selection.clone(),
            reversed: false,
        })
    }
    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        if self.preedit.is_empty() {
            None
        } else {
            Some(0..self.preedit.encode_utf16().count())
        }
    }
    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.preedit.clear();
        self.selection = 0..0;
        cx.notify();
    }
    /// 功能：仅把已提交文本发送到 PTY，组合过程不发送重复字符；时间：2026-11-10；作者：lq。
    fn replace_text_in_range(
        &mut self,
        _: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.preedit.clear();
        self.selection = 0..0;
        self.send(text.as_bytes().to_vec(), cx);
    }
    /// 功能：组合输入在本地显示，提交前不写入远端；时间：2026-11-10；作者：lq。
    fn replace_and_mark_text_in_range(
        &mut self,
        _: Option<Range<usize>>,
        text: &str,
        selection: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.preedit = text.into();
        self.selection =
            selection.unwrap_or_else(|| text.encode_utf16().count()..text.encode_utf16().count());
        cx.notify();
    }
    fn bounds_for_range(
        &mut self,
        _: Range<usize>,
        bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let frame = self
            .session
            .model
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .frame();
        let cursor = frame
            .rows
            .iter()
            .enumerate()
            .find_map(|(row, cells)| {
                cells
                    .iter()
                    .position(|cell| cell.cursor)
                    .map(|column| (column, row))
            })
            .unwrap_or((0, 0));
        Some(Bounds {
            origin: bounds.origin
                + point(
                    px(cursor.0 as f32 * self.font_size * 0.6),
                    px(cursor.1 as f32 * self.font_size * 1.4),
                ),
            size: size(px(100.), px(self.font_size * 1.4)),
        })
    }
    fn character_index_for_point(
        &mut self,
        _: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        Some(0)
    }
    /// 功能：粘贴按终端 bracketed paste 协议处理；时间：2026-11-10；作者：lq。
    fn paste(&mut self, item: ClipboardItem, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = item.text() {
            self.clear_output_selection();
            if let Err(error) = self.session.paste(&text) {
                self.status = error;
            }
            cx.notify();
        }
    }
}
