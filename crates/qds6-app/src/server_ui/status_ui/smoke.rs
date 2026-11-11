//! 功能：专用回环 SSH 服务的真实状态采集、终端并行和原生 Metal 布局验收；时间：2026-11-11；作者：lq。

use super::*;
use gpui_kit::test::TestWindowExt;
use qds6_services::ssh::{self, SshError};
use std::time::Instant;

/// 功能：推进 UI 虚拟时钟同时等待真实网络 worker，限制整个步骤等待时间；时间：2026-11-11；作者：lq。
fn wait_for(cx: &mut HeadlessAppContext, mut ready: impl FnMut(&mut HeadlessAppContext) -> bool) {
    let start = Instant::now();
    loop {
        cx.run_until_parked();
        cx.advance_clock(Duration::from_millis(100));
        if ready(cx) {
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(25),
            "status UI timed out"
        );
        std::thread::sleep(Duration::from_millis(30));
    }
}

/// 功能：真实绘制数帧，避免仅构造元素树而未验证布局；时间：2026-11-11；作者：lq。
fn frames(cx: &mut HeadlessAppContext, handle: AnyWindowHandle) {
    for _ in 0..6 {
        cx.run_until_parked();
        cx.advance_clock(Duration::from_millis(100));
        cx.update_window(handle, |_, window, cx| {
            window.simulate_next_frame(cx);
            window.render_frame(cx);
        })
        .unwrap();
    }
}

/// 功能：不读取用户配置；隔离指纹文件、SQLite、草稿密钥与固定回环端口；时间：2026-11-11；作者：lq。
pub fn run_server_status_smoke() {
    assert_eq!(std::env::var("QDS6_FIXTURES").as_deref(), Ok("1"));
    qds6_services::credentials::use_fixture_draft_key([42; 32]);
    let root = std::env::temp_dir().join(format!("qds6-status-ui-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    std::fs::create_dir_all("output/server-status").unwrap();
    let mut profile = ConnectionProfile::new(Protocol::Ssh);
    profile.name = "状态采集验证".into();
    profile.host = "127.0.0.1".into();
    profile.username = "qds6".into();
    profile.port = 22222;
    let known = root.join("known-hosts.json");
    let fingerprint = match ssh::connect(&profile, Some("qds6-fixture-only"), &known) {
        Err(SshError::UnknownHost { fingerprint }) => fingerprint,
        _ => panic!("fixture must ask for trust"),
    };
    ssh::trust_host(&known, &profile, &fingerprint).unwrap();
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
            |window, cx| {
                cx.new(|cx| {
                    Workbench::new_with_store(Some(root.join("connections.sqlite3")), window, cx)
                })
            },
        )
        .unwrap()
    });
    wait_for(&mut cx, |cx| {
        cx.update(|cx| !view.read(cx).loading && view.read(cx).settings.ready)
    });
    cx.update_window(handle, |_, window, cx| {
        view.update(cx, |state, cx| {
            state.replace_form(profile.clone(), window, cx);
            state.form.secret.update(cx, |input, cx| {
                input.set_value("qds6-fixture-only", window, cx)
            });
            state.connect_service(window, cx);
        })
    })
    .unwrap();
    wait_for(&mut cx, |cx| cx.update(|cx| !view.read(cx).services.busy));
    let server = cx.update(|cx| {
        let state = view.read(cx);
        assert_eq!(state.services.servers.len(), 1, "{}", state.status);
        state.services.servers[0].clone()
    });
    wait_for(&mut cx, |cx| {
        cx.update(|cx| server.read(cx).monitoring.state.revision >= 2)
    });
    cx.update(|cx| {
        let state = &server.read(cx).monitoring.state;
        assert!(state.error.is_none(), "{:?}", state.error);
        let sample = state.snapshot.as_ref().unwrap();
        assert!(sample.memory_total > 0);
        assert!(
            sample
                .cpu_percent
                .is_some_and(|n| (0. ..=100.).contains(&n))
        );
        assert!(!sample.processes.is_empty());
        assert!(sample.networks.values().any(|n| n.receive_rate.is_some()));
        assert!(!server.read(cx).monitoring.history.is_empty());
    });
    frames(&mut cx, handle);
    cx.update_window(handle, |_, window, cx| {
        window.click("terminal-grid", cx);
        window.input("printf 'QDS6_STATUS_PARALLEL_OK\\n'", cx);
        window.press("enter", cx);
    })
    .unwrap();
    wait_for(&mut cx, |cx| {
        cx.update(|cx| {
            server
                .read(cx)
                .terminal
                .as_ref()
                .unwrap()
                .read(cx)
                .screen_text()
                .contains("QDS6_STATUS_PARALLEL_OK")
        })
    });
    for (width, height) in [(1440., 900.), (960., 640.)] {
        for (theme, name) in [(ThemeMode::Light, "light"), (ThemeMode::Dark, "dark")] {
            cx.update_window(handle, |_, window, cx| {
                window.resize(size(px(width), px(height)));
                window.bounds_changed(cx);
                Theme::change(theme, Some(window), cx);
            })
            .unwrap();
            frames(&mut cx, handle);
            cx.update_window(handle, |_, window, _| {
                let panel = window.find("server-status-panel").bounds();
                let terminal = window.find("server-terminal-pane").bounds();
                let files = window.find("server-files-pane").bounds();
                assert!(panel.right() <= terminal.origin.x + px(1.));
                assert!(terminal.size.width > px(200.));
                assert!(terminal.bottom() <= files.origin.y + px(1.));
                assert!(files.bottom() <= window.find("workspace-status").bounds().origin.y);
                assert!(panel.bottom() <= window.find("workspace-status").bounds().origin.y);
            })
            .unwrap();
            cx.capture_screenshot(handle)
                .unwrap()
                .save(format!(
                    "output/server-status/{width:.0}x{height:.0}-{name}.png"
                ))
                .unwrap();
        }
    }
    cx.update_window(handle, |_, window, cx| {
        window.click("toggle-server-status", cx)
    })
    .unwrap();
    frames(&mut cx, handle);
    cx.update_window(handle, |_, window, _| {
        assert!(window.try_find("server-status-panel").is_none())
    })
    .unwrap();
    cx.update_window(handle, |_, window, cx| {
        window.click("toggle-server-status", cx)
    })
    .unwrap();
    frames(&mut cx, handle);
    cx.update_window(handle, |_, window, cx| {
        window.click("copy-server-address", cx);
        assert_eq!(
            cx.read_from_clipboard().unwrap().text().as_deref(),
            Some("127.0.0.1")
        );
    })
    .unwrap();
    // 功能：错误保留旧数据并展示采集停止，终端仍持有原实体；时间：2026-11-11；作者：lq。
    cx.update(|cx| {
        server.update(cx, |server, cx| {
            server.monitoring.task = None;
            server.monitoring.monitor = None;
            server.monitoring.state.error = Some("测试：状态采集连接已断开".into());
            cx.notify();
        })
    });
    frames(&mut cx, handle);
    cx.capture_screenshot(handle)
        .unwrap()
        .save("output/server-status/stale-error.png")
        .unwrap();
    println!(
        "PASS: real SSH status delta samples, CPU/memory/swap/process/network, concurrent terminal, light/dark 1440/960 Metal layout, collapse/reopen, copy, stale error rendering"
    );
    drop(server);
    drop(cx);
    std::fs::remove_dir_all(root).unwrap();
}
