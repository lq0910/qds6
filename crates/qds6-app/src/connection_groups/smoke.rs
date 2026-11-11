//! 功能：合成连接下验证分组增删改、键盘、拖动、搜索和磁盘重读；时间：2026-11-11；作者：lq。

use super::*;
use gpui_kit::InputEvent as _;
use gpui_kit::test::{TestQueryExt, TestWindowExt};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

/// 功能：等待真实后台 SQLite 任务，然后绘制稳定帧；时间：2026-11-11；作者：lq。
fn settle(cx: &mut HeadlessAppContext, handle: AnyWindowHandle, view: &Entity<Workbench>) {
    let start = Instant::now();
    loop {
        cx.run_until_parked();
        if cx.update(|cx| {
            let state = view.read(cx);
            !state.loading
                && !state.saving
                && !state.groups.busy
                && state.settings.ready
                && !state.settings.saving
        }) {
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(15),
            "group task did not settle"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    for _ in 0..4 {
        cx.advance_clock(Duration::from_millis(100));
        cx.run_until_parked();
        cx.update_window(handle, |_, window, cx| {
            window.simulate_next_frame(cx);
            window.render_frame(cx);
        })
        .unwrap();
    }
}

/// 功能：按无障碍名称点击弹出项，保留原生菜单命中测试；时间：2026-11-11；作者：lq。
fn click_label(window: &mut Window, label: &str, cx: &mut App) {
    let position = window.find_by_label(label).bounds().center();
    window.dispatch_event(
        MouseDownEvent {
            position,
            button: MouseButton::Left,
            click_count: 1,
            modifiers: Default::default(),
            ..Default::default()
        }
        .to_platform_input(),
        cx,
    );
    window.render_frame(cx);
    window.dispatch_event(
        MouseUpEvent {
            position,
            button: MouseButton::Left,
            click_count: 1,
            modifiers: Default::default(),
        }
        .to_platform_input(),
        cx,
    );
    window.render_frame(cx);
}

/// 功能：覆盖完整操作而非仅设置 UI 状态，不使用网络或系统凭据；时间：2026-11-11；作者：lq。
pub fn run_group_smoke() {
    let output = PathBuf::from("output/connection-groups");
    std::fs::create_dir_all(&output).unwrap();
    let temporary = std::env::temp_dir().join(format!("qds6-group-ui-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&temporary).unwrap();
    let path = temporary.join("connections.sqlite3");
    let mut store = ProfileStore::open(&path).unwrap();
    let mut profiles = Vec::new();
    for (name, group, protocol) in [
        ("开发 API / 中文", "开发环境", Protocol::Ssh),
        ("生产服务器", "生产环境", Protocol::Ssh),
        ("待整理服务器", "", Protocol::Sftp),
        ("同名组数据库", "开发环境", Protocol::PostgreSql),
    ] {
        let mut profile = ConnectionProfile::new(protocol);
        profile.name = name.into();
        profile.group = group.into();
        profile.host = "127.0.0.1".into();
        profile.favorite = name.starts_with("开发");
        store.save(&profile).unwrap();
        profiles.push(profile);
    }
    drop(store);
    let id = profiles[0].id.to_string();
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
        .unwrap()
    });
    settle(&mut cx, handle, &view);

    // 功能：真实按钮创建空分组，空名校验与 Enter 提交；时间：2026-11-11；作者：lq。
    cx.update_window(handle, |_, window, cx| {
        window.click("new-connection-group", cx);
        window.click("save-group-edit", cx);
        assert!(window.find("group-name-error").visible());
        window.click("group-name-input", cx);
        window.input("预发布", cx);
        window.press("enter", cx);
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    assert!(
        ProfileStore::open(&path)
            .unwrap()
            .groups()
            .unwrap()
            .iter()
            .any(|g| g.name == "预发布")
    );
    cx.update_window(handle, |_, window, cx| {
        assert!(window.find("group:servers:预发布").visible());
        window.click("new-connection-group", cx);
        window.input("取消的新组", cx);
        window.press("escape", cx);
        assert!(
            window.try_find("group-editor").is_none(),
            "Escape must cancel group editing"
        );
        window.click("group:servers:开发环境", cx);
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.update_window(handle, |_, window, cx| {
        assert!(
            window.try_find(id.clone()).is_none(),
            "collapsed group must hide connection"
        );
        window.click("connection-search", cx);
        window.input("开发 API", cx);
        assert!(
            window.find(id.clone()).visible(),
            "search must reveal collapsed matches"
        );
    })
    .unwrap();
    cx.capture_screenshot(handle)
        .unwrap()
        .save(output.join("search-collapsed.png"))
        .unwrap();
    cx.update_window(handle, |_, window, cx| {
        window.click("connection-search", cx);
        window.press("cmd-a", cx);
        window.press("backspace", cx);
        assert!(
            window.try_find(id.clone()).is_none(),
            "clearing search restores collapse"
        );
        window.click("group:servers:开发环境", cx);
    })
    .unwrap();
    settle(&mut cx, handle, &view);

    // 功能：拖动归组只改变分组，保留活动表单的未保存名称；时间：2026-11-11；作者：lq。
    cx.update_window(handle, |_, window, cx| {
        window.click(id.clone(), cx);
        window.click("连接名称", cx);
        window.press("cmd-a", cx);
        window.input("未保存的连接名称", cx);
        let from = window.find(id.clone()).bounds().center();
        let target = window.find("folder:group:servers:预发布").bounds().center();
        window.drag(from, target, cx);
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.update(|cx| {
        let state = view.read(cx);
        assert_eq!(state.profiles[0].group, "预发布");
        assert_eq!(
            state.form.name.read(cx).value().as_str(),
            "未保存的连接名称"
        );
        assert!(state.form_dirty);
        assert_eq!(state.form.group.read(cx).value().as_str(), "预发布");
    });

    // 功能：菜单改名、重复名称阻止、中文修改及同名数据库隔离；时间：2026-11-11；作者：lq。
    cx.update_window(handle, |_, window, cx| {
        window.click("group-menu:group:servers:预发布", cx);
        click_label(window, "重命名分组…", cx);
        window.click("group-name-input", cx);
        window.press("cmd-a", cx);
        window.input("生产环境", cx);
        window.press("enter", cx);
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.update_window(handle, |_, window, cx| {
        assert!(window.find("group-name-error").visible());
        window.click("group-name-input", cx);
        window.press("cmd-a", cx);
        window.input("联调环境", cx);
        window.click("save-group-edit", cx);
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.update(|cx| {
        let state = view.read(cx);
        assert_eq!(state.profiles[0].group, "联调环境");
        assert_eq!(state.form.group.read(cx).value().as_str(), "联调环境");
        assert_eq!(
            state.form.name.read(cx).value().as_str(),
            "未保存的连接名称"
        );
        assert_eq!(state.profiles[3], profiles[3]);
    });
    cx.capture_screenshot(handle)
        .unwrap()
        .save(output.join("folders-light-1440x900.png"))
        .unwrap();

    // 功能：连接更多菜单触发归组，不误触打开/丢弃表单确认；时间：2026-11-11；作者：lq。
    cx.update_window(handle, |_, window, cx| {
        window.click(SharedString::from(format!("connection-menu:{id}")), cx);
        click_label(window, "移动到分组", cx);
        click_label(window, "未分组", cx);
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.update(|cx| assert!(view.read(cx).profiles[0].group.is_empty()));

    // 功能：目录右键菜单删除、取消确认以及非空分组删除后保留全部连接；时间：2026-11-11；作者：lq。
    cx.update_window(handle, |_, window, cx| {
        window.right_click("folder:group:servers:生产环境", cx);
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.update_window(handle, |_, window, cx| {
        click_label(window, "删除分组…", cx);
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.update_window(handle, |_, window, cx| {
        window.click("cancel-delete-group", cx);
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    assert!(
        ProfileStore::open(&path)
            .unwrap()
            .groups()
            .unwrap()
            .iter()
            .any(|g| g.name == "生产环境")
    );
    cx.update_window(handle, |_, window, cx| {
        window.click("group-menu:group:servers:生产环境", cx);
        click_label(window, "删除分组…", cx);
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.capture_screenshot(handle)
        .unwrap()
        .save(output.join("delete-confirmation.png"))
        .unwrap();
    cx.update_window(handle, |_, window, cx| {
        window.click("confirm-delete-group", cx)
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.update(|cx| {
        let state = view.read(cx);
        assert_eq!(state.profiles.len(), 4);
        assert!(state.profiles[1].group.is_empty());
    });

    // 功能：方向键、收藏过滤、表单分组选择以及深浅色最小窗口渲染；时间：2026-11-11；作者：lq。
    cx.update_window(handle, |_, window, cx| {
        window.click("group:servers:联调环境", cx);
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.update_window(handle, |_, window, cx| window.press("right", cx))
        .unwrap();
    settle(&mut cx, handle, &view);
    cx.update(|cx| {
        assert!(
            !view
                .read(cx)
                .groups
                .items
                .iter()
                .find(|g| g.name == "联调环境")
                .unwrap()
                .collapsed
        )
    });
    cx.update_window(handle, |_, window, cx| window.press("left", cx))
        .unwrap();
    settle(&mut cx, handle, &view);
    cx.update(|cx| {
        assert!(
            view.read(cx)
                .groups
                .items
                .iter()
                .find(|g| g.name == "联调环境")
                .unwrap()
                .collapsed
        )
    });
    cx.update_window(handle, |_, window, cx| {
        window.click("favorites", cx);
        assert!(window.find(id.clone()).visible());
        assert!(window.try_find(profiles[1].id.to_string()).is_none());
        window.click("favorites", cx);
        window.click("form-group-picker", cx);
        click_label(window, "开发环境", cx);
        assert_eq!(
            view.read(cx).form.group.read(cx).value().as_str(),
            "开发环境"
        );
        window.resize(size(px(960.), px(640.)));
        window.bounds_changed(cx);
        Theme::change(ThemeMode::Dark, Some(window), cx);
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.capture_screenshot(handle)
        .unwrap()
        .save(output.join("folders-dark-960x640.png"))
        .unwrap();
    let disk_groups = ProfileStore::open(&path).unwrap().groups().unwrap();
    cx.update_window(handle, |_, window, cx| {
        view.update(cx, |this, cx| this.load(cx));
        window.render_frame(cx);
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.update(|cx| {
        assert_eq!(view.read(cx).groups.items, disk_groups);
        assert!(view.read(cx).form_dirty);
    });
    cx.update_window(handle, |_, window, _| window.remove_window())
        .unwrap();
    drop(cx);
    std::fs::remove_dir_all(temporary).unwrap();
    println!(
        "QDS6 group smoke PASS: create/rename/delete/cancel, empty folders, duplicate/empty validation, drag/menu moves, preserved dirty form, search reveals collapse, favorite filter, keyboard left/right, scoped DB isolation, form picker, SQLite reload, Metal light 1440x900 and dark 960x640. No live service, Windows, IME composition or installation acceptance."
    );
}
