//! 功能：隔离配置下验证原生标签、任务分组和默认/最小窗口尺寸；时间：2026-11-11；作者：lq。

use super::*;
use crate::workspace_ui::{WorkspaceArea, WorkspaceTarget};
use gpui_kit::test::TestWindowExt;
use std::sync::Arc;

/// 功能：完成布局和标签动画后捕获真实 Metal 帧；时间：2026-11-11；作者：lq。
fn capture(cx: &mut HeadlessAppContext, handle: AnyWindowHandle, name: &str) {
    for _ in 0..5 {
        cx.advance_clock(std::time::Duration::from_millis(100));
        cx.run_until_parked();
        cx.update_window(handle, |_, window, cx| {
            window.simulate_next_frame(cx);
            window.render_frame(cx);
        })
        .unwrap();
    }
    cx.capture_screenshot(handle)
        .unwrap()
        .save(format!("output/ui-layout/{name}.png"))
        .unwrap();
}

/// 功能：固定操作栏必须位于状态栏上方，标签栏和按钮不越出窗口；时间：2026-11-11；作者：lq。
fn assert_form_bounds(window: &Window) {
    let status = window.find("workspace-status").bounds();
    let connect = window.find("connect-profile").bounds();
    let save = window.find("save-profile").bounds();
    let tabs = window.find("workspace-tab-strip").bounds();
    assert!(
        connect.bottom() <= status.origin.y,
        "connect overlaps status: {connect:?} / {status:?}"
    );
    assert!(save.bottom() <= status.origin.y, "save overlaps status");
    assert!(
        tabs.size.height <= px(44.),
        "workspace tabs must remain one row"
    );
    assert!(
        save.right() <= status.right(),
        "save button must fit window"
    );
}

/// 功能：标签背景填满栏高，关闭按钮具有留白且垂直居中；时间：2026-11-11；作者：lq。
fn assert_workspace_tab_bounds(window: &mut Window, index: usize, close_id: &'static str) {
    let strip = window.find("workspace-tab-strip").bounds();
    let tab = window.within("workspace-tabs").find(index).bounds();
    let close = window.find(close_id).bounds();
    assert_eq!(strip.size.height, px(40.));
    assert_eq!(tab.origin.y, strip.origin.y, "tab must meet the bar top");
    assert_eq!(tab.bottom(), strip.bottom(), "tab must meet the content");
    assert!(
        close.right() <= tab.right() - px(8.),
        "close needs right padding"
    );
    assert_eq!(close.center().y, tab.center().y, "close must be centered");
}

/// 功能：等待真实后台偏好读写完成，不用虚拟时钟替代 SQLite 落盘；时间：2026-11-11；作者：lq。
fn settle_preferences(cx: &mut HeadlessAppContext, view: &Entity<Workbench>) {
    let start = std::time::Instant::now();
    loop {
        cx.run_until_parked();
        if cx.update(|cx| view.read(cx).settings.ready && !view.read(cx).settings.saving) {
            break;
        }
        assert!(
            start.elapsed() < std::time::Duration::from_secs(10),
            "preferences did not settle"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

/// 功能：验证可操作布局而非只核对构造代码，不连接用户服务；时间：2026-11-11；作者：lq。
pub fn run_layout_smoke() {
    std::fs::create_dir_all("output/ui-layout").unwrap();
    let temporary = std::env::temp_dir().join(format!("qds6-layout-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&temporary).unwrap();
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
                    Workbench::new_with_store(
                        Some(temporary.join("connections.sqlite3")),
                        window,
                        cx,
                    )
                })
            },
        )
        .unwrap()
    });
    cx.run_until_parked();
    settle_preferences(&mut cx, &view);
    for (width, height) in [(1200., 780.), (960., 640.)] {
        let tag = format!("{}x{}", width as u32, height as u32);
        cx.update_window(handle, |_, window, cx| {
            window.resize(size(px(width), px(height)));
            // 功能：Headless 平台 resize 后显式同步视口，避免只裁图而不重新布局；时间：2026-11-11；作者：lq。
            window.bounds_changed(cx);
            assert_eq!(window.viewport_size(), size(px(width), px(height)));
            Theme::change(ThemeMode::Light, Some(window), cx);
            view.update(cx, |state, cx| {
                state.form_dirty = false;
                state.services.results = None;
                state.services.grid = None;
                state.switch_area(WorkspaceArea::Servers, cx);
                state.activate_workspace(WorkspaceTarget::Overview, cx);
            });
        })
        .unwrap();
        capture(&mut cx, handle, &format!("welcome-{tag}"));
        cx.update_window(handle, |_, window, cx| {
            window.click("new-welcome", cx);
            assert_form_bounds(window);
            assert!(
                window.try_find("数据库").is_none(),
                "SSH should not show database field"
            );
        })
        .unwrap();
        capture(&mut cx, handle, &format!("connection-ssh-{tag}"));
        cx.update_window(handle, |_, window, cx| {
            view.update(cx, |state, cx| {
                state.form_dirty = false;
                state.replace_form(ConnectionProfile::new(Protocol::MySql), window, cx);
            });
            window.render_frame(cx);
            window.within("connection-protocols").click(0usize, cx);
            assert_eq!(view.read(cx).form.profile.protocol, Protocol::MySql);
            assert_eq!(view.read(cx).form.port.read(cx).value().as_str(), "3306");
            assert!(window.try_find("数据库").is_some());
            assert!(window.try_find("私钥路径").is_none());
            assert_form_bounds(window);
        })
        .unwrap();
        capture(&mut cx, handle, &format!("connection-mysql-{tag}"));
        // 功能：最小窗口通过滚动访问高级设置，底部主操作始终固定；时间：2026-11-11；作者：lq。
        cx.update_window(handle, |_, window, cx| {
            window.scroll(
                "form-scroll",
                ScrollDelta::Pixels(point(px(0.), px(-1200.))),
                cx,
            );
            window.click("toggle-connection-advanced", cx);
            window.scroll(
                "form-scroll",
                ScrollDelta::Pixels(point(px(0.), px(-1200.))),
                cx,
            );
            assert!(window.find("database-tls").visible());
            assert!(window.find("CA 证书路径").visible());
            assert_form_bounds(window);
        })
        .unwrap();
        capture(&mut cx, handle, &format!("connection-advanced-{tag}"));
        cx.update_window(handle, |_, window, cx| {
            window.click("open-sql-sidebar", cx);
            window.within("workspace-tabs").click(1usize, cx);
            assert_eq!(
                view.read(cx).tab,
                1,
                "native tab must activate connection page"
            );
            window.within("workspace-tabs").click(2usize, cx);
            assert_eq!(view.read(cx).tab, 2);
            assert_eq!(window.find("run-sql").disabled(), Some(true));
            view.update(cx, |state, cx| {
                state.sql.update(cx, |editor, cx| {
                    editor.set_value("SELECT '布局草稿保留';", window, cx)
                })
            });
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(handle, |_, window, cx| {
            window.click("workspace-close-sql", cx);
            assert_eq!(view.read(cx).tab, 0);
            window.click("open-sql-sidebar", cx);
            assert!(view.read(cx).sql.read(cx).value().contains("布局草稿保留"));
        })
        .unwrap();
        capture(&mut cx, handle, &format!("sql-{tag}"));
        cx.update_window(handle, |_, window, cx| {
            view.update(cx, |state, cx| {
                let set = ResultSet {
                    columns: vec!["id".into(), "name".into(), "environment".into()],
                    types: vec!["BIGINT".into(), "TEXT".into(), "TEXT".into()],
                    rows: (0..10_000)
                        .map(|i| {
                            vec![
                                qds6_services::database::Cell::Text(i.to_string()),
                                qds6_services::database::Cell::Text(format!("合成记录 {i}")),
                                qds6_services::database::Cell::Text("layout-fixture".into()),
                            ]
                        })
                        .collect(),
                    affected: 0,
                };
                state.services.grid = Some(cx.new(|cx| {
                    TableState::new(QueryRows::new(set.clone(), window, cx), window, cx)
                }));
                state.services.results = Some(QueryResult {
                    sets: vec![
                        set,
                        ResultSet {
                            affected: 2,
                            ..Default::default()
                        },
                    ],
                    elapsed_ms: 12,
                    truncated: false,
                });
                state.services.selected_result = 0;
                cx.notify();
            });
            window.render_frame(cx);
            assert!(window.within("query-results").find(0usize).visible());
            window.within("query-results").click(1usize, cx);
            assert_eq!(view.read(cx).services.selected_result, 1);
            window.within("query-results").click(0usize, cx);
        })
        .unwrap();
        capture(&mut cx, handle, &format!("sql-results-fixture-{tag}"));
        cx.update_window(handle, |_, window, cx| {
            window.click("open-backup-sidebar", cx);
            assert!(window.find("export-encrypted").visible());
            assert!(window.try_find("preview-config").is_none());
            assert!(window.try_find("rollback-local-snapshot").is_none());
        })
        .unwrap();
        capture(&mut cx, handle, &format!("backup-export-{tag}"));
        cx.update_window(handle, |_, window, cx| {
            window.within("backup-modes").click(1usize, cx);
            window.click("import-navicat", cx);
            assert_eq!(view.read(cx).services.import_source, 3);
            assert!(window.find("preview-import").visible());
            assert!(window.try_find("export-encrypted").is_none());
            assert!(window.try_find("confirm-restore").is_none());
        })
        .unwrap();
        capture(&mut cx, handle, &format!("backup-import-{tag}"));
        cx.update_window(handle, |_, window, cx| {
            window.within("backup-modes").click(2usize, cx);
            assert!(window.find("preview-config").visible());
            assert!(window.find("rollback-local-snapshot").visible());
            assert!(window.try_find("confirm-restore").is_none());
        })
        .unwrap();
        capture(&mut cx, handle, &format!("backup-restore-{tag}"));
        cx.update_window(handle, |_, window, cx| {
            view.update(cx, |state, cx| {
                state.activate_workspace(WorkspaceTarget::Server(0), cx)
            });
            window.render_frame(cx);
            assert!(window.find("empty-new-connection").visible());
            assert!(window.try_find("terminal-split").is_none());
        })
        .unwrap();
        capture(&mut cx, handle, &format!("terminal-empty-{tag}"));
        cx.update_window(handle, |_, window, cx| {
            Theme::change(ThemeMode::Dark, Some(window), cx);
            view.update(cx, |state, cx| {
                state.activate_workspace(WorkspaceTarget::Connection, cx)
            });
            window.render_frame(cx);
            assert_form_bounds(window);
        })
        .unwrap();
        capture(&mut cx, handle, &format!("connection-dark-{tag}"));
        cx.update_window(handle, |_, _window, cx| {
            view.update(cx, |state, cx| {
                state.services.backup_mode = 0;
                state.activate_workspace(WorkspaceTarget::Backup, cx);
            })
        })
        .unwrap();
        capture(&mut cx, handle, &format!("backup-dark-{tag}"));
        // 功能：导入/恢复均可选择文件，小窗口按钮不溢出；SSH 预览不修改现有连接；时间：2026-11-11；作者：lq。
        let import_path = temporary.join("测试_connect_config.json");
        std::fs::write(
            &import_path,
            r#"{"name":"导入测试","host":"localhost","port":2222,"user_name":"test","conection_type":100,"proxy_id":"0","authentication_type":2,"secret_key_id":"test-key"}"#,
        ).unwrap();
        let profile_count = cx.update(|cx| view.read(cx).profiles.len());
        cx.update_window(handle, |_, window, cx| {
            view.update(cx, |state, cx| {
                state.services.backup_mode = 1;
                state.services.import_source = 1;
                state.services.backup_path.update(cx, |input, cx| {
                    input.set_value(import_path.display().to_string(), window, cx);
                });
            });
            window.render_frame(cx);
            let chooser = window.find("choose-backup-file");
            assert!(chooser.visible());
            let chooser = chooser.bounds();
            let input = window.find("backup-file-path").bounds();
            assert!(input.right() <= chooser.origin.x);
            assert!(chooser.right() <= window.viewport_size().width);
            window.click("preview-import", cx);
        })
        .unwrap();
        let started = std::time::Instant::now();
        loop {
            cx.run_until_parked();
            if cx.update(|cx| !view.read(cx).services.busy) {
                break;
            }
            assert!(started.elapsed() < std::time::Duration::from_secs(10));
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        cx.update(|cx| {
            let state = view.read(cx);
            assert!(!state.error, "{}", state.status);
            assert_eq!(state.services.preview.as_ref().unwrap().profiles.len(), 1);
            assert!(
                state
                    .services
                    .import_report
                    .iter()
                    .any(|w| w.contains("私钥需重新选择"))
            );
            assert_eq!(state.profiles.len(), profile_count);
        });
        capture(&mut cx, handle, &format!("finalshell-preview-{tag}"));
        // 功能：FinalShell 与数据库密码预览的原生布局验收，预览始终不修改正式连接；时间：2026-11-11；作者：lq。
        let vectors: Vec<serde_json::Value> = serde_json::from_str(include_str!(
            "../../../qds6-services/src/migration/fixtures/passwords.json"
        ))
        .unwrap();
        let encoded = |format: &str| {
            vectors
                .iter()
                .find(|v| v["format"] == format && v["plaintext"] == "qds6-fixture-only")
                .unwrap()["encoded"]
                .as_str()
                .unwrap()
        };
        let finalshell = serde_json::to_vec(&serde_json::json!({
            "name":"SSH 密码迁移","host":"localhost","user_name":"fixture",
            "authentication_type":1,"conection_type":100,"password":encoded("finalshell")
        }))
        .unwrap();
        let navicat = format!("<Connections><Connection ConnectionName='MySQL 密码迁移' ConnType='MySQL' Host='localhost' UserName='fixture' Password='{}'/><Connection ConnectionName='PostgreSQL 密码迁移' ConnType='PostgreSQL' Host='localhost' UserName='fixture' Password='{}'/></Connections>", encoded("navicat-blowfish"), encoded("navicat-aes")).into_bytes();
        for (source, label, bytes, count) in
            [(1, "finalshell", finalshell, 1), (3, "navicat", navicat, 2)]
        {
            let path = temporary.join(format!("password-{label}.json"));
            std::fs::write(&path, bytes).unwrap();
            cx.update_window(handle, |_, window, cx| {
                view.update(cx, |state, cx| {
                    state.services.import_source = source;
                    state.services.preview = None;
                    state.services.import_report.clear();
                    state.services.backup_path.update(cx, |input, cx| {
                        input.set_value(path.display().to_string(), window, cx)
                    });
                    cx.notify();
                });
                window.render_frame(cx);
                window.click("preview-import", cx);
            })
            .unwrap();
            let started = std::time::Instant::now();
            loop {
                cx.run_until_parked();
                if cx.update(|cx| !view.read(cx).services.busy) {
                    break;
                }
                assert!(started.elapsed() < std::time::Duration::from_secs(10));
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            cx.update(|cx| {
                let state = view.read(cx);
                assert!(!state.error, "{}", state.status);
                let preview = state.services.preview.as_ref().unwrap();
                assert_eq!(preview.profiles.len(), count);
                assert_eq!(preview.secrets.len(), count);
                assert!(preview.profiles.iter().all(|p| p.credential.is_none()));
                assert!(
                    !state
                        .services
                        .import_report
                        .join(" ")
                        .contains("qds6-fixture-only")
                );
                assert_eq!(state.profiles.len(), profile_count);
            });
            capture(&mut cx, handle, &format!("password-import-{label}-{tag}"));
            // 功能：小窗口滚动后逐条密码报告和确认按钮可访问，不只截取上半页；时间：2026-11-11；作者：lq。
            cx.update_window(handle, |_, window, cx| {
                window.scroll(
                    "preview-import",
                    ScrollDelta::Pixels(point(px(0.), px(-1200.))),
                    cx,
                );
                window.render_frame(cx);
                assert!(window.find("confirm-restore").visible());
                let button = window.find("confirm-restore").bounds();
                let status = window.find("workspace-status").bounds();
                assert!(button.bottom() <= status.origin.y);
                assert!(button.right() <= window.viewport_size().width);
            })
            .unwrap();
            capture(
                &mut cx,
                handle,
                &format!("password-import-{label}-report-{tag}"),
            );
            cx.update_window(handle, |_, window, cx| {
                window.scroll(
                    "confirm-restore",
                    ScrollDelta::Pixels(point(px(0.), px(1200.))),
                    cx,
                );
            })
            .unwrap();
        }
        cx.update_window(handle, |_, window, cx| {
            view.update(cx, |state, cx| {
                state.services.backup_mode = 2;
                state.services.preview = None;
                state.services.import_report.clear();
                cx.notify();
            });
            window.render_frame(cx);
            assert!(window.find("choose-backup-file").visible());
        })
        .unwrap();
        capture(&mut cx, handle, &format!("restore-file-{tag}"));
        cx.update(|cx| {
            view.update(cx, |state, cx| {
                state.services.backup_mode = 0;
                cx.notify();
            });
        });
        // 功能：底部设置入口可见，设置页面可滚动访问二维码，返回不销毁草稿；时间：2026-11-11；作者：lq。
        cx.update_window(handle, |_, window, cx| {
            assert!(window.find("open-settings").visible());
            let entry = window.find("open-settings").bounds();
            let status = window.find("workspace-status").bounds();
            assert!(entry.bottom() <= status.origin.y);
            window.click("open-settings", cx);
            assert_eq!(view.read(cx).tab, 5);
        })
        .unwrap();
        capture(&mut cx, handle, &format!("settings-dark-{tag}"));
        cx.update_window(handle, |_, window, cx| {
            assert!(window.try_find("check-for-updates").is_some());
            window.click("terminal-font-increase", cx);
        })
        .unwrap();
        settle_preferences(&mut cx, &view);
        cx.update_window(handle, |_, window, cx| {
            window.click("settings-sql-wrap", cx);
        })
        .unwrap();
        settle_preferences(&mut cx, &view);
        cx.update_window(handle, |_, window, cx| {
            window.within("settings-theme").click(0usize, cx);
        })
        .unwrap();
        settle_preferences(&mut cx, &view);
        capture(&mut cx, handle, &format!("settings-light-{tag}"));
        // 功能：真实点击自动选项和标题栏三态循环，检查即时外观及 SQLite 保存；时间：2026-11-11；作者：lq。
        cx.update_window(handle, |_, window, cx| {
            window.within("settings-theme").click(2usize, cx);
        })
        .unwrap();
        settle_preferences(&mut cx, &view);
        cx.update_window(handle, |_, window, cx| {
            assert_eq!(
                view.read(cx).settings.preferences.theme,
                qds6_storage::ColorTheme::System
            );
            assert_eq!(cx.theme().mode, ThemeMode::from(window.appearance()));
            assert!(window.within("settings-theme").find(2usize).visible());
        })
        .unwrap();
        capture(&mut cx, handle, &format!("settings-auto-{tag}"));
        for theme in [
            qds6_storage::ColorTheme::Light,
            qds6_storage::ColorTheme::Dark,
            qds6_storage::ColorTheme::System,
        ] {
            cx.update_window(handle, |_, window, cx| window.click("theme", cx))
                .unwrap();
            settle_preferences(&mut cx, &view);
            cx.update_window(handle, |_, window, cx| {
                assert_eq!(view.read(cx).settings.preferences.theme, theme);
                let expected = match theme {
                    qds6_storage::ColorTheme::Light => ThemeMode::Light,
                    qds6_storage::ColorTheme::Dark => ThemeMode::Dark,
                    qds6_storage::ColorTheme::System => ThemeMode::from(window.appearance()),
                };
                assert_eq!(cx.theme().mode, expected);
            })
            .unwrap();
        }
        // 功能：仅显式启用时真实点击更新检查，普通布局回归不依赖公网；时间：2026-11-11；作者：lq。
        if width == 1200. && std::env::var("QDS6_LIVE_UPDATE_CHECK").as_deref() == Ok("1") {
            cx.update_window(handle, |_, window, cx| {
                window.click("check-for-updates", cx);
                assert!(view.read(cx).settings.checking);
            })
            .unwrap();
            let start = std::time::Instant::now();
            loop {
                cx.run_until_parked();
                if cx.update(|cx| !view.read(cx).settings.checking) {
                    break;
                }
                assert!(start.elapsed() < std::time::Duration::from_secs(20));
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            println!(
                "PASS update UI request completed: {}",
                cx.update(|cx| view.read(cx).settings.update_message.clone())
            );
            capture(&mut cx, handle, "settings-live-update");
        }
        cx.update_window(handle, |_, window, cx| {
            window.scroll(
                "settings-scroll",
                ScrollDelta::Pixels(point(px(0.), px(-1000.))),
                cx,
            );
        })
        .unwrap();
        capture(&mut cx, handle, &format!("settings-about-{tag}"));
        cx.update_window(handle, |_, window, cx| {
            assert!(window.find("about-wechat-qr").visible());
            assert!(window.find("check-for-updates").visible());
            assert!(window.find("open-source-repository").visible());
            window.click("copy-about-qq", cx);
            assert_eq!(
                cx.read_from_clipboard()
                    .and_then(|item| item.text())
                    .as_deref(),
                Some("960661824")
            );
            window.click("area-databases", cx);
            assert_eq!(
                view.read(cx).tab,
                0,
                "same-area navigation must exit settings"
            );
            assert!(view.read(cx).sql.read(cx).value().contains("布局草稿保留"));
        })
        .unwrap();
        // 功能：重新打开同一个数据库核对持久化值，测试线程执行而非 UI 回调；时间：2026-11-11；作者：lq。
        let expected = cx.update(|cx| view.read(cx).settings.preferences.clone());
        assert_eq!(
            ProfileStore::open(temporary.join("connections.sqlite3"))
                .unwrap()
                .preferences()
                .unwrap(),
            expected
        );
        println!(
            "PASS settings {tag}: saved preferences, QQ clipboard, QR visibility, return to workspace with draft intact"
        );
        println!(
            "PASS layout {tag}: native tabs, draft retention, protocol fields, fixed actions, task-scoped backup, empty state, light/dark Metal frames"
        );
    }
    // 功能：复现截图中的概览、配置和中文服务器标签，隔离验证深浅色及点击路由；时间：2026-11-11；作者：lq。
    cx.update_window(handle, |_, window, cx| {
        view.update(cx, |state, cx| {
            state.replace_form(ConnectionProfile::new(Protocol::Ssh), window, cx);
            state.form_open = true;
            let mut profile = ConnectionProfile::new(Protocol::Ssh);
            profile.name = "测试服务器 / 中文".into();
            state.services.servers = vec![cx.new(|cx| {
                crate::server_ui::ServerWorkspace::new(
                    profile,
                    None,
                    None,
                    Some(temporary.join("connections.sqlite3")),
                    window,
                    cx,
                )
            })];
            state.activate_workspace(WorkspaceTarget::Server(0), cx);
        });
    })
    .unwrap();
    for (mode, name) in [(ThemeMode::Light, "light"), (ThemeMode::Dark, "dark")] {
        cx.update_window(handle, |_, window, cx| {
            Theme::change(mode, Some(window), cx)
        })
        .unwrap();
        capture(&mut cx, handle, &format!("workspace-tabs-{name}"));
        cx.update_window(handle, |_, window, cx| {
            assert_workspace_tab_bounds(window, 1, "workspace-close-connection");
            assert_workspace_tab_bounds(window, 2, "server-close-0");
            window.within("workspace-tabs").click(0usize, cx);
            assert_eq!(view.read(cx).tab, 0);
            window.within("workspace-tabs").click(2usize, cx);
            assert_eq!(view.read(cx).tab, 3);
            assert_eq!(view.read(cx).services.active_server, 0);
        })
        .unwrap();
    }
    cx.update_window(handle, |_, window, cx| {
        window.click("workspace-close-connection", cx);
        assert!(!view.read(cx).form_open);
        assert_eq!(view.read(cx).tab, 3);
    })
    .unwrap();
    println!(
        "PASS workspace tab geometry: full height, padded close, light/dark, switch and close"
    );
    // 功能：只用无网络占位服务器验证长标签新增与窗口缩放后的活动标签可见性；时间：2026-11-11；作者：lq。
    cx.update_window(handle, |_, window, cx| {
        view.update(cx, |state, cx| {
            state.form_open = false;
            state.services.servers = (0..8)
                .map(|index| {
                    let mut profile = ConnectionProfile::new(Protocol::Ssh);
                    profile.name = format!("布局占位服务器 {index} · 长连接名称测试");
                    cx.new(|cx| {
                        crate::server_ui::ServerWorkspace::new(
                            profile,
                            None,
                            None,
                            Some(temporary.join("connections.sqlite3")),
                            window,
                            cx,
                        )
                    })
                })
                .collect();
            state.activate_workspace(WorkspaceTarget::Server(7), cx);
        });
    })
    .unwrap();
    for (width, height) in [(1440., 900.), (960., 640.)] {
        cx.update_window(handle, |_, window, cx| {
            window.resize(size(px(width), px(height)));
            window.bounds_changed(cx);
        })
        .unwrap();
        capture(
            &mut cx,
            handle,
            &format!("server-tabs-overflow-{}", width as u32),
        );
        cx.update_window(handle, |_, window, cx| {
            let selected = window.within("workspace-tabs").find(8usize).bounds();
            let strip = window.find("workspace-tab-strip").bounds();
            let menu = window.within("workspace-tabs").find("more").bounds();
            let scroll = &view.read(cx).workspace_scroll;
            assert!(selected.origin.x >= strip.origin.x && selected.right() <= menu.origin.x,
                "active tab invisible: {selected:?}/{strip:?}/{menu:?}, scroll {:?}/{:?}/{:?}, child {:?}", scroll.bounds(), scroll.offset(), scroll.max_offset(), scroll.bounds_for_item(8));
        }).unwrap();
    }
    println!("PASS tab overflow: active server revealed after open and resize");
    // 功能：使用真实 VT 网格及原生终端画布验收四套配色，不连接远端；时间：2026-11-11；作者：lq。
    let terminal = cx.update_window(handle, |_, window, cx| {
        window.resize(size(px(1440.), px(900.)));
        window.bounds_changed(cx);
        let sample = "QDS6 · 终端配色演示 / Theme preview\r\n\r\n\x1b[32mlq@qds6\x1b[0m \x1b[34m~/workspace\x1b[0m $ cargo build\r\n   \x1b[32mCompiling\x1b[0m qds6 v0.1.0\r\n    \x1b[32mFinished\x1b[0m dev profile in 0.42s\r\n\r\n\x1b[34msrc/\x1b[0m  \x1b[36mdocs/\x1b[0m  Cargo.toml  README.md\r\n\x1b[33mwarning\x1b[0m: 示例警告信息\r\n\x1b[31merror\x1b[0m: 示例错误信息\r\n\x1b[35mANSI accents\x1b[0m · \x1b[38;2;34;153;204mTrue color #2299CC\x1b[0m\r\n\r\n\x1b[32mlq@qds6\x1b[0m $ ";
        let terminal = cx.new(|cx| crate::terminal_view::TerminalView::new(TerminalSession::theme_fixture(sample), window, cx));
        let mut profile = ConnectionProfile::new(Protocol::Ssh);
        profile.name = "终端配色 · 离线演示".into();
        profile.host = "offline-preview".into();
        profile.username = "lq".into();
        let server = cx.new(|cx| crate::server_ui::ServerWorkspace::new(profile, None, None, Some(temporary.join("connections.sqlite3")), window, cx));
        server.update(cx, |server, _| server.terminal = Some(terminal.clone()));
        view.update(cx, |state, cx| {
            state.services.servers = vec![server];
            state.activate_workspace(WorkspaceTarget::Server(0), cx);
        });
        terminal.update(cx, |terminal, cx| terminal.set_font_size(19, cx));
        terminal
    }).unwrap();
    let before = cx.update(|cx| terminal.read(cx).screen_text());
    for theme in qds6_core::TerminalTheme::ALL {
        cx.update_window(handle, |_, window, cx| {
            window.click("open-settings", cx);
            window.scroll(
                "settings-scroll",
                ScrollDelta::Pixels(point(px(0.), px(2000.))),
                cx,
            );
            window.scroll(
                "settings-scroll",
                ScrollDelta::Pixels(point(px(0.), px(-240.))),
                cx,
            );
            assert!(window.find(theme.id()).visible());
            window.click(theme.id(), cx);
        })
        .unwrap();
        settle_preferences(&mut cx, &view);
        cx.update_window(handle, |_, window, cx| {
            assert_eq!(view.read(cx).settings.preferences.terminal_theme, theme);
            assert_eq!(terminal.read(cx).appearance(), (theme, 19.));
            assert_eq!(terminal.read(cx).screen_text(), before);
            window.within("workspace-tabs").click(1usize, cx);
        })
        .unwrap();
        capture(&mut cx, handle, theme.id());
    }
    println!(
        "PASS terminal themes: four native previews, existing output and temporary font size retained, persisted selection"
    );
    // 功能：重建工作台验证启动读取真实保存的偏好，而非仅检查数据库内容；时间：2026-11-11；作者：lq。
    let expected = cx.update(|cx| view.read(cx).settings.preferences.clone());
    let reloaded = cx
        .update_window(handle, |_, window, cx| {
            cx.new(|cx| {
                Workbench::new_with_store(Some(temporary.join("connections.sqlite3")), window, cx)
            })
        })
        .unwrap();
    settle_preferences(&mut cx, &reloaded);
    cx.update_window(handle, |_, window, cx| {
        assert_eq!(reloaded.read(cx).settings.preferences, expected);
        // 功能：重启后自动仍按原生外观解析，不能误当作固定浅色；时间：2026-11-11；作者：lq。
        assert_eq!(
            cx.theme().mode,
            match expected.theme {
                qds6_storage::ColorTheme::Light => ThemeMode::Light,
                qds6_storage::ColorTheme::Dark => ThemeMode::Dark,
                qds6_storage::ColorTheme::System => ThemeMode::from(window.appearance()),
            }
        );
    })
    .unwrap();
    println!(
        "PASS settings restart: reconstructed workspace loads persisted preferences and theme"
    );
    cx.update_window(handle, |_, window, _| window.remove_window())
        .unwrap();
    drop(cx);
    std::fs::remove_dir_all(temporary).unwrap();
}
