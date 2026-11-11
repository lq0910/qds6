//! 功能：隔离数据库下验证真实 Kit 交互及 Metal 绘制；时间：2026-11-10；作者：lq。

use super::*;
use gpui_kit::test::TestWindowExt;
use std::sync::Arc;

/// 功能：覆盖中文配置、验证失败、持久化、草稿保留与主题切换；时间：2026-11-10；作者：lq。
pub fn run_ui_smoke() {
    // 功能：验证品牌、业务图标及默认组件资源均可离线加载，避免仅编译通过却显示空图标；时间：2026-11-10；作者：lq。
    for path in AppAssets.list("").expect("list embedded icons") {
        assert!(
            AppAssets.load(&path).expect("load embedded icon").is_some(),
            "missing embedded asset: {path}"
        );
    }
    let output = PathBuf::from("output/m0-ui");
    std::fs::create_dir_all(&output).expect("create screenshot directory");
    let temporary = std::env::temp_dir().join(format!(
        "qds6-ui-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&temporary).expect("create isolated database directory");
    let path = temporary.join("connections.sqlite3");
    let mut cx = HeadlessAppContext::with_platform(
        gpui_kit::platform::current_platform(true).text_system(),
        Arc::new(AppAssets),
        gpui_kit::platform::current_headless_renderer,
    );
    cx.update(gpui_kit::init);
    let (handle, view) = cx.update(|cx| {
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds {
                    origin: Default::default(),
                    size: size(px(1440.), px(900.)),
                })),
                show: false,
                focus: false,
                ..Default::default()
            },
            cx,
            |window, cx| cx.new(|cx| Workbench::new_with_store(Some(path.clone()), window, cx)),
        )
        .expect("open workbench")
    });
    cx.run_until_parked();
    // 功能：保存彩色品牌和浅色工作区的原生 Metal 渲染证据；时间：2026-11-10；作者：lq。
    settle(&mut cx, handle, &view);
    cx.capture_screenshot(handle)
        .expect("brand Metal screenshot")
        .save(output.join("welcome-brand-light.png"))
        .unwrap();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(view.read(cx).ready, "isolated SQLite must load");
        window.click("new-welcome", cx);
        assert!(window.find("连接名称").visible());
        window.click("save-profile", cx);
    })
    .expect("render and navigate");
    cx.update(|cx| assert!(view.read(cx).error, "empty profile must be rejected"));
    cx.update_window(handle, |_, window, cx| {
        for (field, value) in [
            ("连接名称", "开发服务器 / 中文"),
            ("分组", "开发环境"),
            ("主机", "127.0.0.1"),
            ("用户名", "lq"),
        ] {
            window.click(field, cx);
            window.input(value, cx);
        }
        window.click("favorite", cx);
        window.click("save-profile", cx);
    })
    .expect("fill and save connection");
    cx.run_until_parked();
    let id = cx.update(|cx| {
        let state = view.read(cx);
        assert!(!state.error && !state.saving && !state.form_dirty);
        assert_eq!(state.profiles.len(), 1);
        assert_eq!(state.profiles[0].name, "开发服务器 / 中文");
        assert!(state.profiles[0].favorite);
        state.profiles[0].id.to_string()
    });
    assert_eq!(ProfileStore::open(&path).unwrap().list().unwrap().len(), 1);
    settle(&mut cx, handle, &view);
    cx.capture_screenshot(handle)
        .expect("real Metal screenshot")
        .save(output.join("connection-light.png"))
        .unwrap();
    cx.update_window(handle, |_, window, cx| {
        view.update(cx, |state, cx| {
            state.activate_workspace(workspace_ui::WorkspaceTarget::Sql, cx);
        });
        window.render_frame(cx);
        view.read(cx)
            .sql
            .clone()
            .update(cx, |state, cx| state.focus(window, cx));
        let modifier = if cfg!(target_os = "macos") {
            "cmd"
        } else {
            "ctrl"
        };
        window.press(&format!("{modifier}-a"), cx);
        window.input(
            "-- 功能：中文 SQL 输入验收；时间：2026-11-10；作者：lq。\nSELECT '中文', 42;",
            cx,
        );
        window.click("copy-sql", cx);
        assert!(
            cx.read_from_clipboard()
                .unwrap()
                .text()
                .unwrap()
                .contains("SELECT '中文', 42;")
        );
    })
    .unwrap();
    cx.run_until_parked();
    cx.update(|cx| assert!(view.read(cx).sql_dirty));
    settle(&mut cx, handle, &view);
    cx.capture_screenshot(handle)
        .unwrap()
        .save(output.join("sql-light.png"))
        .unwrap();
    cx.update_window(handle, |_, window, cx| window.click("theme", cx))
        .unwrap();
    settle(&mut cx, handle, &view);
    cx.update_window(handle, |_, window, cx| {
        assert!(cx.theme().mode.is_dark());
        view.update(cx, |state, cx| {
            state.activate_workspace(workspace_ui::WorkspaceTarget::Connection, cx);
        });
        window.render_frame(cx);
        window.click(id.clone(), cx);
        assert!(
            view.read(cx)
                .sql
                .read(cx)
                .value()
                .contains("SELECT '中文', 42;")
        );
        window.click("reload", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update(|cx| assert_eq!(view.read(cx).profiles.len(), 1));
    settle(&mut cx, handle, &view);
    cx.capture_screenshot(handle)
        .unwrap()
        .save(output.join("connection-dark.png"))
        .unwrap();
    cx.update_window(handle, |_, window, _| window.remove_window())
        .unwrap();
    drop(cx);
    std::fs::remove_dir_all(temporary).unwrap();
    println!(
        "QDS6 UI smoke passed: Chinese text input, invalid-form rejection, SQLite save/reload, favorites, SQL copy, tab retention, theme, Metal screenshots. IME composition and native window chrome require separate acceptance."
    );
}

/// 功能：推进模拟时钟直至标签动画结束，再捕获稳定帧；时间：2026-11-10；作者：lq。
fn settle(cx: &mut HeadlessAppContext, handle: AnyWindowHandle, view: &Entity<Workbench>) {
    // 功能：偏好持久化已异步化，等待真实保存完成后再断言主题；时间：2026-11-11；作者：lq。
    let start = std::time::Instant::now();
    loop {
        cx.run_until_parked();
        let pending = cx.update(|cx| {
            let state = view.read(cx);
            state.loading || !state.settings.ready || state.settings.saving
        });
        if !pending {
            break;
        }
        assert!(
            start.elapsed() < std::time::Duration::from_secs(10),
            "workbench preferences/load timed out"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    for _ in 0..4 {
        cx.advance_clock(std::time::Duration::from_millis(100));
        cx.run_until_parked();
        cx.update_window(handle, |_, window, cx| {
            window.simulate_next_frame(cx);
            window.render_frame(cx);
        })
        .unwrap();
    }
}
