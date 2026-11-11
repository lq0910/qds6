//! 功能：Alacritty 状态机与 SSH PTY，有限回滚、异步输入和断线状态；时间：2026-11-10；作者：lq。

use alacritty_terminal::{
    Term,
    event::{Event, EventListener},
    grid::{Dimensions, Scroll},
    term::{Config, TermMode, cell::Flags},
    vte::ansi::{Color, NamedColor, Processor},
};
use qds6_core::{TerminalPalette, TerminalTheme};
use std::{
    collections::VecDeque,
    io::{Read, Write},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, SyncSender},
    },
    thread,
    time::{Duration, Instant},
};

/// 功能：传给 SSH worker 的有界命令；时间：2026-11-10；作者：lq。
enum Command {
    Input(Vec<u8>),
    Resize(u16, u16),
}

/// 功能：终端协议响应回队列，拒绝远程 OSC52 剪贴板访问；时间：2026-11-10；作者：lq。
#[derive(Clone)]
pub struct Listener(Arc<Mutex<VecDeque<u8>>>);
impl EventListener for Listener {
    /// 功能：保留终端查询响应，不接受服务端访问系统剪贴板；时间：2026-11-10；作者：lq。
    fn send_event(&self, event: Event) {
        if let Event::PtyWrite(text) = event {
            let mut queue = self.0.lock().unwrap_or_else(|e| e.into_inner());
            if queue.len() + text.len() <= 65536 {
                queue.extend(text.bytes());
            }
        }
    }
}

/// 功能：可绘制单元，保存宽字符、组合字符与色彩语义；时间：2026-11-10；作者：lq。
#[derive(Clone)]
pub struct Cell {
    pub text: String,
    pub foreground: u32,
    pub background: u32,
    pub wide: bool,
    pub spacer: bool,
    pub bold: bool,
    pub underline: bool,
    pub cursor: bool,
}

/// 功能：UI 快照在锁外绘制，网络 worker 不进入 GPUI；时间：2026-11-10；作者：lq。
pub struct Frame {
    pub rows: Vec<Vec<Cell>>,
    pub status: String,
    pub bracketed_paste: bool,
}

/// 功能：终端模型独立于会话以支持离线状态机验收；时间：2026-11-10；作者：lq。
pub struct Model {
    term: Term<Listener>,
    parser: Processor,
    responses: Arc<Mutex<VecDeque<u8>>>,
    pub status: String,
    // 功能：配色只影响快照映射，保留 VT 网格、历史和真彩色；时间：2026-11-11；作者：lq。
    theme: TerminalTheme,
}

/// 功能：与 UI 无关的有效终端尺寸；时间：2026-11-10；作者：lq。
struct Size {
    columns: usize,
    rows: usize,
}
impl Dimensions for Size {
    fn total_lines(&self) -> usize {
        self.rows
    }
    fn screen_lines(&self) -> usize {
        self.rows
    }
    fn columns(&self) -> usize {
        self.columns
    }
}

impl Model {
    /// 功能：限制终端维度和回滚行数，禁用 OSC52；时间：2026-11-10；作者：lq。
    pub fn new(columns: u16, rows: u16) -> Self {
        let responses = Arc::new(Mutex::new(VecDeque::new()));
        let config = Config {
            scrolling_history: 5000,
            osc52: alacritty_terminal::term::Osc52::Disabled,
            ..Default::default()
        };
        Self {
            term: Term::new(
                config,
                &Size {
                    columns: columns.clamp(20, 300) as usize,
                    rows: rows.clamp(5, 120) as usize,
                },
                Listener(responses.clone()),
            ),
            parser: Processor::new(),
            responses,
            status: "已连接".into(),
            theme: TerminalTheme::default(),
        }
    }

    /// 功能：按字节批次推进 VT 状态机；时间：2026-11-10；作者：lq。
    pub fn feed(&mut self, bytes: &[u8]) {
        self.parser.advance(&mut self.term, bytes);
    }

    /// 功能：切换主题重新映射现有网格，不清屏、不重连、不改服务端颜色；时间：2026-11-11；作者：lq。
    pub fn set_theme(&mut self, theme: TerminalTheme) {
        self.theme = theme;
    }

    /// 功能：限制 resize 尺寸并保留状态机内容；时间：2026-11-10；作者：lq。
    pub fn resize(&mut self, columns: u16, rows: u16) {
        self.term.resize(Size {
            columns: columns.clamp(20, 300) as usize,
            rows: rows.clamp(5, 120) as usize,
        });
    }

    /// 功能：有界回滚滚动，输出不复制到无界文本缓冲；时间：2026-11-10；作者：lq。
    pub fn scroll(&mut self, lines: i32) {
        self.term.scroll_display(Scroll::Delta(lines));
    }

    /// 功能：按服务端 DEC 应用光标模式选择控制序列，支持 Vim/tmux；时间：2026-11-11；作者：lq。
    pub fn application_cursor(&self) -> bool {
        self.term.mode().contains(TermMode::APP_CURSOR)
    }

    /// 功能：从可视网格复制有界屏幕；时间：2026-11-10；作者：lq。
    pub fn text(&self) -> String {
        self.frame()
            .rows
            .iter()
            .map(|row| {
                row.iter()
                    .filter(|c| !c.spacer)
                    .map(|c| c.text.as_str())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// 功能：将 Alacritty 网格映射为 GPUI 颜色与游标单元；时间：2026-11-10；作者：lq。
    pub fn frame(&self) -> Frame {
        let palette = self.theme.palette();
        let content = self.term.renderable_content();
        let mut rows = vec![Vec::with_capacity(self.term.columns()); self.term.screen_lines()];
        for indexed in content.display_iter {
            let line = indexed.point.line.0 + content.display_offset as i32;
            if line < 0 || line as usize >= rows.len() {
                continue;
            }
            let source = indexed.cell;
            let mut text = source.c.to_string();
            if let Some(extra) = source.zerowidth() {
                text.extend(extra.iter());
            }
            let mut foreground = color(source.fg, true, &palette);
            let mut background = color(source.bg, false, &palette);
            if source.flags.contains(Flags::INVERSE) {
                std::mem::swap(&mut foreground, &mut background);
            }
            if source.flags.contains(Flags::HIDDEN) {
                foreground = background;
            }
            rows[line as usize].push(Cell {
                text,
                foreground,
                background,
                wide: source.flags.contains(Flags::WIDE_CHAR),
                spacer: source
                    .flags
                    .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER),
                bold: source.flags.contains(Flags::BOLD),
                underline: source.flags.contains(Flags::UNDERLINE),
                cursor: content.cursor.point == indexed.point
                    && self.term.mode().contains(TermMode::SHOW_CURSOR),
            });
        }
        Frame {
            rows,
            status: self.status.clone(),
            bracketed_paste: self.term.mode().contains(TermMode::BRACKETED_PASTE),
        }
    }
}

/// 功能：主题接管默认色及 ANSI16，扩展256与显式真彩色保持原值；时间：2026-11-11；作者：lq。
fn color(value: Color, foreground: bool, palette: &TerminalPalette) -> u32 {
    match value {
        Color::Spec(c) => (u32::from(c.r) << 16) | (u32::from(c.g) << 8) | u32::from(c.b),
        Color::Named(NamedColor::Background) => palette.background,
        Color::Named(NamedColor::Foreground | NamedColor::BrightForeground) => palette.foreground,
        Color::Named(NamedColor::Cursor) => palette.cursor,
        Color::Named(c) if (c as usize) < 16 => palette.ansi[c as usize],
        Color::Indexed(i) if i < 16 => palette.ansi[i as usize],
        Color::Indexed(i) if i >= 232 => {
            let n = 8 + u32::from(i - 232) * 10;
            (n << 16) | (n << 8) | n
        }
        Color::Indexed(i) => {
            let i = u32::from(i - 16);
            let component = |n| if n == 0 { 0 } else { 55 + n * 40 };
            (component(i / 36) << 16) | (component(i / 6 % 6) << 8) | component(i % 6)
        }
        _ => {
            if foreground {
                palette.foreground
            } else {
                palette.background
            }
        }
    }
}

/// 功能：终端会话句柄，销毁后停止 worker，不重放任何输入；时间：2026-11-10；作者：lq。
pub struct TerminalSession {
    pub model: Arc<Mutex<Model>>,
    commands: SyncSender<Command>,
    stop: Arc<AtomicBool>,
    _tunnel: Option<Arc<crate::ssh::TunnelLease>>,
}

impl TerminalSession {
    /// 功能：原生配色验收的离线终端，不连接服务，不进入发行构建；时间：2026-11-11；作者：lq。
    #[cfg(feature = "test-support")]
    pub fn theme_fixture(text: &str) -> Self {
        let mut model = Model::new(100, 24);
        model.feed(text.as_bytes());
        model.status = "离线配色演示 · 无网络连接".into();
        let model = Arc::new(Mutex::new(model));
        let (commands, receiver) = mpsc::sync_channel(64);
        let stop = Arc::new(AtomicBool::new(false));
        let worker_model = model.clone();
        let worker_stop = stop.clone();
        thread::spawn(move || {
            while !worker_stop.load(Ordering::Relaxed) {
                match receiver.recv_timeout(Duration::from_millis(20)) {
                    Ok(Command::Resize(columns, rows)) => worker_model
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .resize(columns, rows),
                    Ok(Command::Input(_)) => {}
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(_) => break,
                }
            }
        });
        Self {
            model,
            commands,
            stop,
            _tunnel: None,
        }
    }
    /// 功能：请求真正的 xterm PTY 和 shell，随后后台非阻塞读写；时间：2026-11-10；作者：lq。
    pub fn start(session: ssh2::Session, columns: u16, rows: u16) -> Result<Self, String> {
        let mut channel = session.channel_session().map_err(|_| "无法创建 SSH 通道")?;
        // 功能：请求服务端 UTF-8 locale；服务器可拒绝，界面不假定远端配置已更改；时间：2026-11-10；作者：lq。
        let _ = channel.setenv("LANG", "C.UTF-8");
        channel
            .request_pty(
                "xterm-256color",
                None,
                Some((u32::from(columns), u32::from(rows), 0, 0)),
            )
            .map_err(|_| "服务器拒绝 PTY")?;
        channel
            .handle_extended_data(ssh2::ExtendedData::Merge)
            .map_err(|_| "无法合并终端输出")?;
        channel.shell().map_err(|_| "服务器拒绝交互 Shell")?;
        session.set_blocking(false);
        let model = Arc::new(Mutex::new(Model::new(columns, rows)));
        let worker_model = model.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let (commands, receiver) = mpsc::sync_channel(128);
        thread::spawn(move || {
            let mut buffer = [0; 32768];
            let mut pending = VecDeque::new();
            let mut last_keepalive = Instant::now();
            let mut outcome = "会话已结束";
            // 功能：EAGAIN 的 resize 保留至下一轮重试，避免 UI 与远端尺寸失配；时间：2026-11-11；作者：lq。
            let mut pending_resize = None;
            while !worker_stop.load(Ordering::Relaxed) {
                if pending.len() < 1024 * 1024 {
                    for _ in 0..16 {
                        match receiver.try_recv() {
                            Ok(Command::Input(bytes)) => pending.extend(bytes),
                            Ok(Command::Resize(columns, rows)) => {
                                pending_resize = Some((columns, rows));
                            }
                            Err(_) => break,
                        }
                    }
                }
                if let Some((columns, rows)) = pending_resize {
                    match channel.request_pty_size(u32::from(columns), u32::from(rows), None, None)
                    {
                        Ok(()) => {
                            worker_model
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .resize(columns, rows);
                            pending_resize = None;
                        }
                        Err(error) if error.code() == ssh2::ErrorCode::Session(-37) => {}
                        Err(_) => {
                            worker_model
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .status = "PTY 尺寸更新失败".into();
                            pending_resize = None;
                        }
                    }
                }
                if !pending.is_empty() {
                    match channel.write(pending.make_contiguous()) {
                        Ok(n) => {
                            pending.drain(..n);
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                        Err(_) => {
                            outcome = "连接写入失败 · 请手动重连";
                            break;
                        }
                    }
                }
                match channel.read(&mut buffer) {
                    Ok(0) if channel.eof() => break,
                    Ok(n) if n > 0 => {
                        let mut model = worker_model.lock().unwrap_or_else(|e| e.into_inner());
                        model.feed(&buffer[..n]);
                        let mut replies = model.responses.lock().unwrap_or_else(|e| e.into_inner());
                        pending.extend(replies.drain(..));
                    }
                    Ok(_) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(_) => {
                        outcome = "连接读取失败 · 请手动重连";
                        break;
                    }
                }
                if last_keepalive.elapsed() > Duration::from_secs(20) {
                    if session
                        .keepalive_send()
                        .is_err_and(|e| e.code() != ssh2::ErrorCode::Session(-37))
                    {
                        outcome = "保活失败 · 请手动重连";
                        break;
                    }
                    last_keepalive = Instant::now();
                }
                thread::sleep(Duration::from_millis(8));
            }
            let _ = channel.close();
            worker_model
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .status = outcome.into();
        });
        Ok(Self {
            model,
            commands,
            stop,
            _tunnel: None,
        })
    }

    /// 功能：终端持有跳板隧道，关闭时独立释放，不影响其他资源；时间：2026-11-10；作者：lq。
    pub fn with_tunnel(mut self, tunnel: Option<Arc<crate::ssh::TunnelLease>>) -> Self {
        self._tunnel = tunnel;
        self
    }

    /// 功能：单次输入限制 64 KiB 并在拥塞时反馈，禁止 UI 阻塞发送；时间：2026-11-10；作者：lq。
    pub fn input(&self, bytes: Vec<u8>) -> Result<(), String> {
        if bytes.len() > 65536 {
            return Err("单次终端输入超过 64 KiB".into());
        }
        self.commands
            .try_send(Command::Input(bytes))
            .map_err(|_| "终端已断开或输入队列繁忙".into())
    }

    /// 功能：粘贴尊重服务端 bracketed paste 模式并过滤注入结束标记；时间：2026-11-10；作者：lq。
    pub fn paste(&self, text: &str) -> Result<(), String> {
        let bracketed = self
            .model
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .term
            .mode()
            .contains(TermMode::BRACKETED_PASTE);
        let text = text.replace('\u{1b}', "").replace("\r\n", "\n");
        self.input(if bracketed {
            format!("\x1b[200~{text}\x1b[201~").into_bytes()
        } else {
            text.replace('\n', "\r").into_bytes()
        })
    }

    /// 功能：异步传递 PTY resize；时间：2026-11-10；作者：lq。
    pub fn resize(&self, columns: u16, rows: u16) {
        let _ = self
            .commands
            .try_send(Command::Resize(columns.clamp(20, 300), rows.clamp(5, 120)));
    }
}

impl Drop for TerminalSession {
    /// 功能：会话关闭不等待网络阻塞或重放命令；时间：2026-11-10；作者：lq。
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    /// 功能：验证光标定位、备用屏幕、宽字符与回滚上限；时间：2026-11-10；作者：lq。
    #[test]
    fn vt_state_machine_handles_fullscreen_and_wide_text() {
        let mut model = Model::new(80, 24);
        model.feed("hello\r\n中文\x1b[31m红\x1b[0m".as_bytes());
        assert!(model.text().contains("中文红"));
        model.feed(b"\x1b[?1h");
        assert!(model.application_cursor());
        model.feed(b"\x1b[?1l");
        assert!(!model.application_cursor());
        model.feed(b"\x1b[?1049h\x1b[2J\x1b[HVIM\x1b[10;5Hstatus");
        assert!(model.text().starts_with("VIM"));
        model.feed(b"\x1b[?1049l");
        assert!(model.text().contains("hello"));
        for _ in 0..6000 {
            model.feed(b"line\r\n");
        }
        assert!(model.term.grid().history_size() <= 5000);
        model.resize(100, 30);
        assert_eq!(model.frame().rows.len(), 30);
    }

    /// 功能：四套主题重映射现有 ANSI 网格，保留真彩色、反显、隐藏文字和输出；时间：2026-11-11；作者：lq。
    #[test]
    fn terminal_theme_recolors_existing_output_without_losing_semantics() {
        let mut model = Model::new(80, 24);
        model.feed(b"A\x1b[31mR\x1b[38;2;17;34;51mT\x1b[0;7mI\x1b[0;8mH\x1b[0m");
        let text = model.text();
        for theme in TerminalTheme::ALL {
            model.set_theme(theme);
            let palette = theme.palette();
            let frame = model.frame();
            assert_eq!(frame.rows[0][0].foreground, palette.foreground);
            assert_eq!(frame.rows[0][0].background, palette.background);
            assert_eq!(frame.rows[0][1].foreground, palette.ansi[1]);
            assert_eq!(frame.rows[0][2].foreground, 0x112233);
            assert_eq!(frame.rows[0][3].foreground, palette.background);
            assert_eq!(frame.rows[0][3].background, palette.foreground);
            assert_eq!(frame.rows[0][4].foreground, frame.rows[0][4].background);
            assert_eq!(model.text(), text);
        }
    }
}
