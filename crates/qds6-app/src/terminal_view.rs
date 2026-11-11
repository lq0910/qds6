//! 功能：原生终端网格、键盘及 IME 组合输入，使用 Alacritty 状态机；时间：2026-11-10；作者：lq。

use gpui_kit::{
    component::{
        h_flex,
        input::{Input, InputState},
        v_flex,
    },
    *,
};
// 功能：按需展开搜索条的原生构建扩展；时间：2026-11-11；作者：lq。
use gpui_kit::prelude::FluentBuilder;
use qds6_core::TerminalTheme;
use qds6_services::terminal::TerminalSession;
use std::{ops::Range, time::Duration};

/// 功能：单终端视图与原生输入焦点独立管理；时间：2026-11-10；作者：lq。
pub struct TerminalView {
    session: TerminalSession,
    focus: FocusHandle,
    preedit: String,
    selection: Range<usize>,
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
        let refresh = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(33))
                    .await;
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    break;
                }
            }
        });
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("搜索当前屏幕"));
        let subscription = cx.observe(&search, |_, _, cx| cx.notify());
        Self {
            session,
            focus: cx.focus_handle(),
            preedit: String::new(),
            selection: 0..0,
            columns: 100,
            rows: 30,
            font_size: 14.,
            theme: TerminalTheme::default(),
            status: String::new(),
            _refresh: refresh,
            search,
            show_search: false,
            _search_subscription: subscription,
        }
    }

    /// 功能：输入失败可见，不在重连后重放；时间：2026-11-10；作者：lq。
    fn send(&mut self, bytes: Vec<u8>, cx: &mut Context<Self>) {
        if let Err(error) = self.session.input(bytes) {
            self.status = error;
        }
        cx.notify();
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
                "c" => {
                    let text = self
                        .session
                        .model
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .text();
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                }
                "v" => {
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
        let search = self.search.read(cx).value().to_lowercase();
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
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| window.focus(&this.focus, cx)),
                    )
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
                            .child(
                                canvas(
                                    move |bounds, _, cx| {
                                        view.update(cx, |this, _| {
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
                                        let cell_width = font_size * 0.6;
                                        let cell_height = font_size * 1.4;
                                        for (row, cells) in frame.rows.iter().enumerate() {
                                            let line_text = cells
                                                .iter()
                                                .filter(|c| !c.spacer)
                                                .map(|c| c.text.as_str())
                                                .collect::<String>()
                                                .to_lowercase();
                                            let matches: Vec<_> = if search.is_empty() {
                                                Vec::new()
                                            } else {
                                                line_text
                                                    .match_indices(search.as_str())
                                                    .map(|(start, text)| start..start + text.len())
                                                    .collect()
                                            };
                                            let mut text_offset = 0;
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
                                                let matched = matches
                                                    .iter()
                                                    .any(|range| range.contains(&text_offset));
                                                window.paint_quad(fill(
                                                    Bounds {
                                                        origin,
                                                        size: size(width, px(cell_height)),
                                                    },
                                                    rgb(if matched {
                                                        palette.search_background
                                                    } else if cell.cursor {
                                                        palette.cursor
                                                    } else {
                                                        cell.background
                                                    }),
                                                ));
                                                let font = font("Menlo");
                                                let run = TextRun {
                                                    len: cell.text.len(),
                                                    font,
                                                    color: rgb(if matched {
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
                                                text_offset += cell.text.to_lowercase().len();
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
                v.child(Input::new(&self.search).aria_label("终端搜索"))
            })
            .child(h_flex().px_3().py_1().text_xs().child(format!(
                "{status} · {}×{} · ⌘C 复制屏幕 / ⌘V 粘贴 / ⌘± 缩放",
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
            if let Err(error) = self.session.paste(&text) {
                self.status = error;
            }
            cx.notify();
        }
    }
}
