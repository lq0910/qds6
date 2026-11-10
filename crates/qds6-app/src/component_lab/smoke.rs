//! 功能：通过 Kit 公开测试 API 验证树、菜单、虚拟表与停靠交互；时间：2026-11-10；作者：lq。

use super::*;
use gpui_kit::test::TestWindowExt;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

/// 功能：等待事件队列及停靠/标签动画稳定，使用真实字体与 Metal 渲染；时间：2026-11-10；作者：lq。
fn settle(cx: &mut HeadlessAppContext, handle: AnyWindowHandle) {
    for _ in 0..4 {
        cx.advance_clock(Duration::from_millis(100));
        cx.run_until_parked();
        cx.update_window(handle, |_, window, cx| window.render_frame(cx))
            .unwrap();
    }
}

/// 功能：确认二维可见范围有界，避免仅以大数据源条数声称虚拟化已生效；时间：2026-11-10；作者：lq。
fn visible_range(view: &Entity<ComponentLab>, cx: &mut HeadlessAppContext) -> String {
    cx.update(|cx| {
        let table = view.read(cx).table.read(cx);
        let range = table.visible_range();
        assert!(!range.rows().is_empty());
        assert!(
            range.rows().len() < 100,
            "only visible rows may be laid out"
        );
        assert!(!range.cols().is_empty());
        assert!(
            range.cols().len() < COLS,
            "offscreen columns must be virtualized"
        );
        format!("rows={:?}, cols={:?}", range.rows(), range.cols())
    })
}

/// 功能：执行可复现的 M0 Mac 组件验收，所有输入来自合成数据；时间：2026-11-10；作者：lq。
pub fn run_component_smoke() {
    let output = PathBuf::from("output/m0-components");
    std::fs::create_dir_all(&output).expect("create component evidence directory");
    let mut cx = HeadlessAppContext::with_platform(
        gpui_kit::platform::current_platform(true).text_system(),
        Arc::new(AppAssets),
        gpui_kit::platform::current_headless_renderer,
    );
    cx.update(gpui_kit::init);
    let mut evidence = String::from(
        "QDS6 M0 component smoke; author=lq; date=2026-11-10\nSynthetic data only; no database or network I/O.\n",
    );
    for (width, height) in [(1440., 900.), (960., 640.)] {
        let (handle, view) = cx.update(|cx| {
            Theme::change(ThemeMode::Light, None, cx);
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Default::default(),
                        size: size(px(width), px(height)),
                    })),
                    show: false,
                    focus: false,
                    ..Default::default()
                },
                cx,
                |window, cx| cx.new(|cx| ComponentLab::new(window, cx)),
            )
            .expect("open isolated component window")
        });
        settle(&mut cx, handle);
        evidence.push_str(&format!(
            "\nWindow {width}x{height}; initial {}\n",
            visible_range(&view, &mut cx)
        ));
        cx.capture_screenshot(handle)
            .unwrap()
            .save(output.join(format!("lab-{width}-light.png")))
            .unwrap();

        // 功能：直接点击单元格验证表格选择事件，而非只检查程序设置的选择值；时间：2026-11-10；作者：lq。
        cx.update_window(handle, |_, window, cx| {
            window.click("cell-0-1", cx);
            assert_eq!(view.read(cx).table.read(cx).selected_cell(), Some((0, 1)));
        })
        .unwrap();

        // 功能：用真实点击与方向键验证折叠、展开和选择；时间：2026-11-10；作者：lq。
        cx.update_window(handle, |_, window, cx| {
            window.within("lab-tree").click(0usize, cx);
            assert!(
                view.read(cx)
                    .tree
                    .read(cx)
                    .index_of(&"postgres".into())
                    .is_none()
            );
            window.press("right", cx);
            assert!(
                view.read(cx)
                    .tree
                    .read(cx)
                    .index_of(&"postgres".into())
                    .is_some()
            );
            window.press("down", cx);
            window.press("right", cx);
            assert!(
                view.read(cx)
                    .tree
                    .read(cx)
                    .index_of(&"users".into())
                    .is_some()
            );
            window.press("down", cx);
            assert_eq!(
                view.read(cx)
                    .tree
                    .read(cx)
                    .selected_item()
                    .unwrap()
                    .id
                    .as_ref(),
                "users"
            );
        })
        .unwrap();

        // 功能：编辑中文草稿后用真实标签点击与拖动核对实体状态保留；时间：2026-11-10；作者：lq。
        let draft =
            "-- 功能：停靠草稿保留验收；时间：2026-11-10；作者：lq。\nSELECT '移动后仍保留';\n";
        cx.update_window(handle, |_, window, cx| {
            view.read(cx)
                .sql
                .clone()
                .update(cx, |state, cx| state.focus(window, cx));
            window.press(
                if cfg!(target_os = "macos") {
                    "cmd-a"
                } else {
                    "ctrl-a"
                },
                cx,
            );
            window.input(draft, cx);
            window.within("tab-bar").click(1usize, cx);
            assert!(window.find("lab-notes").visible());
            assert!(window.try_find("lab-sql").is_none());
            window.within("tab-bar").drag_to(1usize, 0usize, cx);
        })
        .unwrap();
        settle(&mut cx, handle);
        cx.update_window(handle, |_, window, cx| {
            window.within("tab-bar").click(1usize, cx);
            assert!(window.find("lab-sql").visible());
            assert_eq!(view.read(cx).sql.read(cx).value().as_ref(), draft);
            // 功能：拖动 SQL 标签到结果分组中央，验证跨组移动；时间：2026-11-10；作者：lq。
            let from = window.within("tab-bar").find(1usize).bounds().center();
            let target = window.find("lab-results").bounds().center();
            window.drag(from, target, cx);
        })
        .unwrap();
        settle(&mut cx, handle);
        cx.update_window(handle, |_, window, cx| {
            assert!(window.find("lab-sql").visible());
            assert!(
                window.try_find("lab-results").is_none(),
                "SQL must become active in target group"
            );
            assert!(
                window.find("lab-notes").bounds().bottom() <= window.find("lab-sql").bounds().top()
            );
            // 功能：实际点击弹出菜单重置布局，不能仅直接调用 reset 冒充菜单验收；时间：2026-11-10；作者：lq。
            window.click("lab-menu", cx);
            window.within("popup-menu").click(0usize, cx);
        })
        .unwrap();
        settle(&mut cx, handle);
        cx.update_window(handle, |_, window, cx| {
            assert!(window.try_find("popup-menu").is_none());
            assert!(window.find("lab-results").visible());
            assert_eq!(view.read(cx).sql.read(cx).value().as_ref(), draft);
            // 功能：使用面板真实工具栏验证放大后可恢复；时间：2026-11-10；作者：lq。
            let snapshot = window.find("lab-sql");
            let group_ix = snapshot
                .path()
                .iter()
                .position(|id| *id == "tab-panel".into())
                .unwrap();
            let group = snapshot.path()[group_ix - 1].clone();
            window.within(group).click("zoom-in", cx);
        })
        .unwrap();
        settle(&mut cx, handle);
        cx.update_window(handle, |_, window, cx| {
            assert!(window.find("lab-sql").bounds().size.width > px(width - 20.));
            assert!(window.try_find("lab-tree").is_none());
            let snapshot = window.find("lab-sql");
            let group_ix = snapshot
                .path()
                .iter()
                .position(|id| *id == "tab-panel".into())
                .unwrap();
            let group = snapshot.path()[group_ix - 1].clone();
            window.within(group).click("zoom-out", cx);
        })
        .unwrap();
        settle(&mut cx, handle);
        cx.update_window(handle, |_, window, cx| {
            assert_eq!(view.read(cx).sql.read(cx).value().as_ref(), draft);
            // 功能：通过布局边界定位真实分隔线，模拟拖动调整侧栏宽度；时间：2026-11-10；作者：lq。
            let left = window.find("lab-tree").bounds();
            let right = window.find("lab-sql").bounds();
            let from = point((left.right() + right.left()) / 2., left.center().y);
            window.drag(from, point(from.x + px(50.), from.y), cx);
        })
        .unwrap();
        settle(&mut cx, handle);
        cx.update_window(handle, |_, window, cx| {
            assert!(
                window.find("lab-tree").bounds().size.width > px(280.),
                "split divider must resize tree panel"
            );
            window.click("lab-menu", cx);
            window.within("popup-menu").click(3usize, cx);
        })
        .unwrap();
        let start = Instant::now();
        settle(&mut cx, handle);
        let last_range = visible_range(&view, &mut cx);
        let elapsed = start.elapsed();
        cx.update(|cx| {
            let table = view.read(cx).table.read(cx);
            assert_eq!(table.selected_cell(), Some((ROWS - 1, 0)));
            assert!(table.visible_range().rows().contains(&(ROWS - 1)));
        });
        cx.update_window(handle, |_, window, cx| {
            assert!(window.find("cell-99999-0").visible());
            window.click("lab-menu", cx);
            window.within("popup-menu").click(5usize, cx);
            assert!(cx.theme().mode.is_dark());
        })
        .unwrap();
        settle(&mut cx, handle);
        cx.capture_screenshot(handle)
            .unwrap()
            .save(output.join(format!("lab-{width}-dark-last.png")))
            .unwrap();
        cx.update_window(handle, |_, window, cx| {
            // 功能：横向定位末列，确认列内容可访问；时间：2026-11-10；作者：lq。
            view.read(cx)
                .table
                .clone()
                .update(cx, |table, cx| table.scroll_to_col(COLS - 1, cx));
            window.render_frame(cx);
        })
        .unwrap();
        settle(&mut cx, handle);
        cx.update_window(handle, |_, window, cx| {
            assert!(window.find("cell-99999-19").visible());
            view.read(cx).table.focus_handle(cx).focus(window, cx);
            window.press("up", cx);
            assert_eq!(
                view.read(cx).table.read(cx).selected_cell(),
                Some((ROWS - 2, 0))
            );
            window.click("lab-menu", cx);
            window.within("popup-menu").click(2usize, cx);
        })
        .unwrap();
        settle(&mut cx, handle);
        cx.update(|cx| {
            let table = view.read(cx).table.read(cx);
            assert_eq!(table.selected_cell(), Some((0, 0)));
            assert!(table.visible_range().rows().contains(&0));
            let rendered = table.delegate().rendered.get();
            assert!(rendered < ROWS * COLS / 10, "drawing must remain bounded across all test frames");
            evidence.push_str(&format!("Tree collapse/expand/keyboard selection: PASS\nTab switch/reorder/cross-group drag/draft retention: PASS\nMenu reset/jump/theme, panel zoom/restore and split resize: PASS\nLast row {last_range}; four settled frames took {elapsed:?} (not input-latency benchmark)\nCell mouse selection, horizontal last column and keyboard row navigation: PASS\nTotal rendered cells across test frames: {rendered} / {} source cells\n", ROWS * COLS));
        });
        cx.update_window(handle, |_, window, _| window.remove_window())
            .unwrap();
    }
    std::fs::write(output.join("results.txt"), evidence).unwrap();
    println!(
        "QDS6 component smoke passed: tree, menus, 100000x20 virtual table, tabs, cross-group drag, divider resize and draft retention at 1440x900 and 960x640. Native IME and Windows not tested."
    );
}
