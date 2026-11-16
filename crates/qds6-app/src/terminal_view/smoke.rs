//! 功能：通过原生窗口事件验收输出拖选、高亮与系统剪贴板；时间：2026-11-16；作者：lq。

use super::*;
use gpui_kit::test::TestWindowExt;

/// 功能：模拟真实按下、移动和释放，覆盖反向拖动及画布外松开；时间：2026-11-16；作者：lq。
fn drag(window: &mut Window, from: Point<Pixels>, to: Point<Pixels>, cx: &mut App) {
    window.dispatch_event(
        MouseMoveEvent {
            position: from,
            pressed_button: None,
            modifiers: Modifiers::default(),
        }
        .to_platform_input(),
        cx,
    );
    window.render_frame(cx);
    window.dispatch_event(
        MouseDownEvent {
            position: from,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
            click_count: 1,
            first_mouse: false,
        }
        .to_platform_input(),
        cx,
    );
    window.render_frame(cx);
    window.dispatch_event(
        MouseMoveEvent {
            position: to,
            pressed_button: Some(MouseButton::Left),
            modifiers: Modifiers::default(),
        }
        .to_platform_input(),
        cx,
    );
    window.render_frame(cx);
    window.dispatch_event(
        MouseUpEvent {
            position: to,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
            click_count: 1,
        }
        .to_platform_input(),
        cx,
    );
    window.render_frame(cx);
}

/// 功能：在已有离线终端演示上验证实际事件路由，不连接用户服务器；时间：2026-11-16；作者：lq。
pub(crate) fn verify(
    cx: &mut HeadlessAppContext,
    handle: AnyWindowHandle,
    terminal: &Entity<TerminalView>,
) {
    cx.update_window(handle, |_, window, cx| {
        // 功能：窗口尺寸稳定后写入专用样本，避免旧配色样本因缩行进入历史区；时间：2026-11-16；作者：lq。
        terminal
            .read(cx)
            .session
            .model
            .lock()
            .unwrap()
            .feed("\x1bcQDS6 · 终端选区 / Selection\r\n\r\nlq@qds6 $ test".as_bytes());
        window.refresh();
        window.render_frame(cx);
        let bounds = window.find("terminal-content").bounds();
        let font_size = terminal.read(cx).font_size;
        let position = |column: f32, row: f32| {
            bounds.origin + point(px(column * font_size * 0.6), px(row * font_size * 1.4))
        };
        for (from, to) in [
            (position(0.1, 0.5), position(3.9, 0.5)),
            (position(3.9, 0.5), position(0.1, 0.5)),
        ] {
            drag(window, from, to, cx);
            assert!(!terminal.read(cx).selecting_output);
            assert_eq!(
                terminal
                    .read(cx)
                    .session
                    .model
                    .lock()
                    .unwrap()
                    .selected_text()
                    .as_deref(),
                Some("QDS6"),
                "mouse selection: bounds={bounds:?}, from={from:?}, to={to:?}"
            );
            window.press("cmd-c", cx);
            assert_eq!(
                cx.read_from_clipboard().unwrap().text().as_deref(),
                Some("QDS6")
            );
            let frame = terminal.read(cx).session.model.lock().unwrap().frame();
            assert!(frame.rows[0][..4].iter().all(|cell| cell.selected));
            assert!(!frame.rows[0][4].selected);
        }
        // 功能：跨行选区包含中文，剪贴板不得带入其余命令输出；时间：2026-11-16；作者：lq。
        drag(window, position(0.1, 0.5), position(1.9, 2.5), cx);
        window.press("cmd-c", cx);
        assert_eq!(
            cx.read_from_clipboard().unwrap().text().as_deref(),
            Some("QDS6 · 终端选区 / Selection\n\nlq")
        );
        // 功能：单击清空选区后复制保留已有剪贴板，整屏复制需显式按组合键；时间：2026-11-16；作者：lq。
        drag(window, position(0.1, 0.5), position(0.1, 0.5), cx);
        cx.write_to_clipboard(ClipboardItem::new_string("keep clipboard".into()));
        window.press("cmd-c", cx);
        assert_eq!(
            cx.read_from_clipboard().unwrap().text().as_deref(),
            Some("keep clipboard")
        );
        window.press("cmd-shift-c", cx);
        assert_eq!(
            cx.read_from_clipboard().unwrap().text().unwrap(),
            terminal.read(cx).screen_text()
        );
        // 功能：画布外释放后不再跟随无按键移动，并可立即开始下一次拖选；时间：2026-11-16；作者：lq。
        drag(
            window,
            position(0.1, 0.5),
            point(bounds.right() + px(20.), bounds.bottom() + px(20.)),
            cx,
        );
        assert!(!terminal.read(cx).selecting_output);
        drag(window, position(0.1, 0.5), position(3.9, 0.5), cx);
        window.dispatch_action(Box::new(component::input::Copy), cx);
    })
    .unwrap();
    // 功能：原生菜单 action 经 GPUI defer 分发，等待后再核对系统剪贴板；时间：2026-11-16；作者：lq。
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        assert_eq!(
            cx.read_from_clipboard().unwrap().text().as_deref(),
            Some("QDS6")
        );
        window.render_frame(cx);
    })
    .unwrap();
    cx.capture_screenshot(handle)
        .unwrap()
        .save("output/ui-layout/terminal-selection.png")
        .unwrap();
    println!(
        "PASS terminal selection: forward/reverse drag, partial/Unicode multiline clipboard, highlight, empty copy, explicit whole-screen copy, outside release, native Copy action"
    );
}
