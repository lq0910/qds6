//! 功能：仅连接回环 fixture，复现隐藏目录误报为空并验证原生收放与终端留白；时间：2026-11-11；作者：lq。

use super::*;
// 功能：原生鼠标拖动事件转换，避免与组件输入事件同名；时间：2026-11-12；作者：lq。
use crate::workspace_ui::WorkspaceTarget;
use gpui_kit::InputEvent as _;
use gpui_kit::{component::scroll::ScrollbarHandle, test::TestWindowExt};
use qds6_services::ssh::{self, SshError};
use std::time::Instant;

/// 功能：等待后台目录和偏好任务后绘制真实 Metal 帧；时间：2026-11-11；作者：lq。
fn settle(cx: &mut HeadlessAppContext, handle: AnyWindowHandle, view: &Entity<Workbench>) {
    let start = Instant::now();
    loop {
        cx.run_until_parked();
        cx.advance_clock(Duration::from_millis(100));
        if cx.update(|cx| {
            let state = view.read(cx);
            !state.loading
                && state.settings.ready
                && !state.settings.saving
                && !state.services.busy
                && state
                    .services
                    .servers
                    .iter()
                    .all(|server| !server.read(cx).pending())
        }) {
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(30),
            "fixture did not settle"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    for _ in 0..6 {
        cx.advance_clock(Duration::from_millis(100));
        cx.run_until_parked();
        cx.update_window(handle, |_, window, cx| {
            window.simulate_next_frame(cx);
            window.render_frame(cx);
        })
        .unwrap();
    }
}

/// 功能：独立数据、指纹及草稿密钥，不接触用户配置和生产服务；时间：2026-11-11；作者：lq。
pub fn run_sidebar_files_smoke() {
    assert_eq!(std::env::var("QDS6_FIXTURES").as_deref(), Ok("1"));
    qds6_services::credentials::use_fixture_draft_key([41; 32]);
    let root = std::env::temp_dir().join(format!("qds6-sidebar-files-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let root = std::fs::canonicalize(root).unwrap();
    std::fs::create_dir_all("output/sidebar-files").unwrap();
    let mut profile = ConnectionProfile::new(Protocol::Ssh);
    profile.name = "文件与侧栏验证".into();
    profile.host = "127.0.0.1".into();
    profile.port = 22222;
    profile.username = "qds6".into();
    let known = root.join("known-hosts.json");
    let fingerprint = match ssh::connect(&profile, Some("qds6-fixture-only"), &known) {
        Err(SshError::UnknownHost { fingerprint }) => fingerprint,
        _ => panic!("fixture must ask for trust"),
    };
    ssh::trust_host(&known, &profile, &fingerprint).unwrap();
    let client =
        sftp::Client::new(ssh::connect(&profile, Some("qds6-fixture-only"), &known).unwrap())
            .unwrap();
    let remote_root = PathBuf::from(format!("/home/qds6/visibility-{}", uuid::Uuid::new_v4()));
    client.mkdir(&remote_root).unwrap();
    let hidden = remote_root.join("hidden-only");
    let empty = remote_root.join("empty");
    let mixed = remote_root.join("mixed");
    for path in [&hidden, &empty, &mixed] {
        client.mkdir(path).unwrap();
    }
    client.mkdir(&hidden.join(".ssh")).unwrap();
    let seed = root.join("seed.txt");
    std::fs::write(&seed, "fixture visibility content\n").unwrap();
    for path in [
        hidden.join(".bashrc"),
        mixed.join(".profile"),
        mixed.join("readme.txt"),
    ] {
        client
            .upload(&seed, &path, &sftp::Transfer::default())
            .unwrap();
    }
    assert_eq!(client.list(&hidden).unwrap().len(), 2);
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
    settle(&mut cx, handle, &view);
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
    settle(&mut cx, handle, &view);
    let server = cx.update(|cx| {
        assert_eq!(view.read(cx).services.servers.len(), 1);
        view.read(cx).services.servers[0].clone()
    });
    verify_explorer(
        &mut cx,
        handle,
        &view,
        &server,
        &client,
        &root,
        &remote_root,
    );
    verify_file_scrolling(
        &mut cx,
        handle,
        &view,
        &server,
        &client,
        &root,
        &remote_root,
    );
    for (path, expected) in [
        (&hidden, "仅含 2 个隐藏项"),
        (&empty, "空文件夹"),
        (&mixed, ""),
    ] {
        cx.update_window(handle, |_, window, cx| {
            server.update(cx, |state, cx| {
                state.services.remote_path.update(cx, |input, cx| {
                    input.set_value(path.display().to_string(), window, cx)
                });
                state.refresh_remote(window, cx);
            })
        })
        .unwrap();
        settle(&mut cx, handle, &view);
        cx.update(|cx| {
            let browser = &server.read(cx).services.files.remote;
            assert!(browser.errors.is_empty());
            if expected.is_empty() {
                assert_eq!(browser.entries[path].len(), 2);
                assert_eq!(browser.hidden_count(path), 1);
                server.read(cx).file_row(true, &path.join("readme.txt"), cx);
                assert!(
                    browser
                        .visible_entries(false)
                        .iter()
                        .all(|e| e.name != ".profile")
                );
            } else {
                assert!(browser.empty_hint(path, false).starts_with(expected));
            }
        });
        if path == &hidden {
            cx.capture_screenshot(handle)
                .unwrap()
                .save("output/sidebar-files/hidden-filtered.png")
                .unwrap();
            cx.update_window(handle, |_, window, cx| {
                window.click("show-hidden-remote", cx)
            })
            .unwrap();
            settle(&mut cx, handle, &view);
            cx.update(|cx| {
                let state = server.read(cx);
                assert!(state.services.show_hidden);
                state.file_row(true, &hidden.join(".bashrc"), cx);
                state.file_row(true, &hidden.join(".ssh"), cx);
            });
            assert_eq!(
                client.open_document(&hidden.join(".bashrc")).unwrap().text,
                "fixture visibility content\n"
            );
            cx.capture_screenshot(handle)
                .unwrap()
                .save("output/sidebar-files/hidden-shown.png")
                .unwrap();
            cx.update_window(handle, |_, window, cx| window.click("show-hidden", cx))
                .unwrap();
            settle(&mut cx, handle, &view);
        }
    }
    // 功能：真实缺失路径必须显示读取失败，而非空目录；时间：2026-11-11；作者：lq。
    let missing = remote_root.join("does-not-exist");
    cx.update_window(handle, |_, window, cx| {
        server.update(cx, |state, cx| {
            state.services.remote_path.update(cx, |input, cx| {
                input.set_value(missing.display().to_string(), window, cx)
            });
            state.refresh_remote(window, cx);
        })
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.update(|cx| {
        let browser = &server.read(cx).services.files.remote;
        assert!(browser.errors.contains_key(&missing));
        assert!(browser.empty_hint(&missing, false).starts_with("读取失败"));
    });
    // 功能：同一路径恢复后刷新，目录和旧错误提示同时恢复正常；时间：2026-11-11；作者：lq。
    client.mkdir(&missing).unwrap();
    cx.update_window(handle, |_, window, cx| {
        server.update(cx, |state, cx| state.refresh_remote(window, cx))
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.update(|cx| {
        assert!(!server.read(cx).error);
        assert!(server.read(cx).services.files.remote.errors.is_empty());
    });
    cx.update_window(handle, |_, window, cx| {
        server.update(cx, |state, cx| {
            state.services.remote_path.update(cx, |input, cx| {
                input.set_value(mixed.display().to_string(), window, cx)
            });
            state.refresh_remote(window, cx);
        });
        view.update(cx, |state, cx| {
            state
                .search
                .update(cx, |input, cx| input.set_value("保留搜索", window, cx));
            state.activate_workspace(WorkspaceTarget::Server(0), cx);
        });
        window.click("terminal-grid", cx);
        window.input("printf 'QDS6_SIDEBAR_SESSION_OK\\n'", cx);
        window.press("enter", cx);
    })
    .unwrap();
    std::thread::sleep(Duration::from_millis(300));
    settle(&mut cx, handle, &view);
    // 功能：两种窗口尺寸下真实点击收放，核对终端留白及会话路径保留；时间：2026-11-11；作者：lq。
    for (width, height) in [(1440., 900.), (960., 640.)] {
        cx.update_window(handle, |_, window, cx| {
            window.resize(size(px(width), px(height)));
            window.bounds_changed(cx);
        })
        .unwrap();
        settle(&mut cx, handle, &view);
        let before = cx
            .update_window(handle, |_, window, cx| {
                let grid = window.find("terminal-grid").bounds();
                let content = window.find("terminal-content").bounds();
                assert!(content.origin.x >= grid.origin.x + px(12.));
                assert!(content.origin.y >= grid.origin.y + px(8.));
                assert!(content.right() <= grid.right() - px(12.));
                // 功能：窄窗口下上传下载完整落在各自栏内，不能被溢出裁切；时间：2026-11-12；作者：lq。
                let local = window.find("server-local-pane").bounds();
                let remote = window.find("server-remote-pane").bounds();
                let upload = window.find("upload").bounds();
                let download = window.find("download").bounds();
                assert!(upload.origin.x >= local.origin.x && upload.right() <= local.right());
                assert!(download.origin.x >= remote.origin.x && download.right() <= remote.right());
                let toggle = window.find("toggle-connection-sidebar").bounds();
                let strip = window.find("workspace-tab-strip").bounds();
                assert_eq!(toggle.center().y, strip.center().y);
                assert!(toggle.origin.x >= strip.origin.x && toggle.right() <= strip.right());
                window.click("toggle-connection-sidebar", cx);
                grid
            })
            .unwrap();
        settle(&mut cx, handle, &view);
        cx.update_window(handle, |_, window, cx| {
            assert!(window.try_find("connection-sidebar").is_none());
            assert!(
                window.find("terminal-grid").bounds().size.width >= before.size.width + px(220.)
            );
            assert_eq!(view.read(cx).search.read(cx).value().as_str(), "保留搜索");
            assert_eq!(
                view.read(cx).services.servers[0].entity_id(),
                server.entity_id()
            );
            assert_eq!(server.read(cx).services.files.remote.current, mixed);
            assert!(
                server
                    .read(cx)
                    .terminal
                    .as_ref()
                    .unwrap()
                    .read(cx)
                    .screen_text()
                    .contains("QDS6_SIDEBAR_SESSION_OK")
            );
        })
        .unwrap();
        cx.capture_screenshot(handle)
            .unwrap()
            .save(format!("output/sidebar-files/collapsed-{width:.0}.png"))
            .unwrap();
        cx.update_window(handle, |_, window, cx| {
            window.click("toggle-connection-sidebar", cx)
        })
        .unwrap();
        settle(&mut cx, handle, &view);
        cx.update_window(handle, |_, window, _| {
            assert!(window.find("connection-sidebar").visible())
        })
        .unwrap();
        cx.capture_screenshot(handle)
            .unwrap()
            .save(format!("output/sidebar-files/expanded-{width:.0}.png"))
            .unwrap();
        // 功能：默认与最小窗口都可切换远程全宽，工具栏保持可见；时间：2026-11-12；作者：lq。
        cx.update_window(handle, |_, window, cx| {
            window.click("toggle-local-files", cx)
        })
        .unwrap();
        settle(&mut cx, handle, &view);
        cx.update_window(handle, |_, window, _| {
            assert!(window.try_find("server-local-pane").is_none());
            let remote = window.find("server-remote-pane").bounds();
            let terminal = window.find("server-terminal-pane").bounds();
            assert_eq!(remote.size.width, terminal.size.width);
            assert!(window.find("toggle-local-files").visible());
            // 功能：隐藏本地后显示可调宽的远程树，路径栏与列表仍保持可见；时间：2026-11-12；作者：lq。
            let tree = window.find("remote-directory-tree").bounds();
            let list = window.find("file-details-scroll-remote").bounds();
            assert!(tree.size.width >= px(110.) && tree.size.width <= px(320.));
            assert!(tree.right() <= list.origin.x);
        })
        .unwrap();
        cx.capture_screenshot(handle)
            .unwrap()
            .save(format!("output/sidebar-files/remote-only-{width:.0}.png"))
            .unwrap();
        cx.update_window(handle, |_, window, cx| {
            window.click("toggle-local-files", cx)
        })
        .unwrap();
        settle(&mut cx, handle, &view);
    }
    // 功能：设置页原生点击联动开关，保存后返回真实终端核对深浅色及固定覆盖；时间：2026-11-11；作者：lq。
    for (mode, name) in [
        (qds6_storage::ColorTheme::Dark, "dark"),
        (qds6_storage::ColorTheme::Light, "light"),
    ] {
        cx.update_window(handle, |_, window, cx| {
            view.update(cx, |state, cx| {
                let mut preferences = state.settings.preferences.clone();
                preferences.theme = mode;
                state.save_preferences(preferences, window, cx);
            })
        })
        .unwrap();
        settle(&mut cx, handle, &view);
        cx.update(|cx| {
            assert_eq!(
                server
                    .read(cx)
                    .terminal
                    .as_ref()
                    .unwrap()
                    .read(cx)
                    .appearance()
                    .0
                    .is_dark(),
                mode == qds6_storage::ColorTheme::Dark
            )
        });
        cx.capture_screenshot(handle)
            .unwrap()
            .save(format!("output/sidebar-files/linked-{name}.png"))
            .unwrap();
    }
    cx.update_window(handle, |_, window, cx| window.click("open-settings", cx))
        .unwrap();
    settle(&mut cx, handle, &view);
    cx.update_window(handle, |_, window, cx| {
        window.scroll(
            "settings-scroll",
            ScrollDelta::Pixels(point(px(0.), px(-180.))),
            cx,
        );
        window.render_frame(cx);
        assert!(window.find("terminal-follow-appearance").visible());
        window.click("terminal-follow-appearance", cx);
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.update(|cx| {
        assert!(
            !view
                .read(cx)
                .settings
                .preferences
                .terminal_follow_appearance
        )
    });
    cx.update_window(handle, |_, window, cx| {
        window.click("terminal-follow-appearance", cx)
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    assert!(
        ProfileStore::open(root.join("connections.sqlite3"))
            .unwrap()
            .preferences()
            .unwrap()
            .terminal_follow_appearance
    );
    println!(
        "PASS real SSH/SFTP: hidden-only, empty, mixed, missing directory; native reveal; 1440/960 sidebar; terminal padding; session/search/path retained"
    );
    cx.update_window(handle, |_, window, _| window.remove_window())
        .unwrap();
    drop(cx);
    std::fs::remove_dir_all(root).unwrap();
}

/// 功能：真实原生点击、多选、目录层级和批量双向拖放，通过内容校验确认落地；时间：2026-11-11；作者：lq。
fn verify_explorer(
    cx: &mut HeadlessAppContext,
    handle: AnyWindowHandle,
    view: &Entity<Workbench>,
    server: &Entity<ServerWorkspace>,
    client: &sftp::Client,
    root: &Path,
    remote: &Path,
) {
    let local = root.join("explorer");
    let download = root.join("downloads");
    std::fs::create_dir_all(local.join("folder/empty")).unwrap();
    std::fs::create_dir_all(&download).unwrap();
    for (name, text) in [
        ("a.txt", "alpha"),
        ("b.txt", "beta"),
        (".hidden", "hidden"),
        ("folder/nested.txt", "nested"),
    ] {
        std::fs::write(local.join(name), text).unwrap();
    }
    let uploaded = remote.join("uploaded");
    client.mkdir(&uploaded).unwrap();
    cx.update_window(handle, |_, window, cx| {
        view.update(cx, |state, cx| {
            state.activate_workspace(WorkspaceTarget::Server(0), cx)
        });
        server.update(cx, |state, cx| {
            state.navigate_directory(FileSide::Local, local.clone(), window, cx);
            state.navigate_directory(FileSide::Remote, uploaded.clone(), window, cx);
        });
    })
    .unwrap();
    settle(cx, handle, view);
    // 功能：双击进入只显示下一层，地址同步，返回上级不残留选择；时间：2026-11-12；作者：lq。
    cx.update_window(handle, |_, window, cx| {
        let row = server.read(cx).file_row(false, &local.join("folder"), cx);
        window.double_click(row, cx);
    })
    .unwrap();
    settle(cx, handle, view);
    cx.update(|cx| {
        let state = server.read(cx);
        assert_eq!(state.services.files.local.current, local.join("folder"));
        assert_eq!(state.services.files.local.visible_entries(false).len(), 2);
        assert!(state.services.files.local.selection.is_empty());
        assert_eq!(
            state.services.local_directory.read(cx).value().as_str(),
            local.join("folder").to_str().unwrap()
        );
    });
    cx.update_window(handle, |_, window, cx| {
        window.click("parent-directory-local", cx)
    })
    .unwrap();
    settle(cx, handle, view);
    // 功能：平台多选、Shift 连选、Escape 清空与键盘全选都通过真实事件完成；时间：2026-11-11；作者：lq。
    cx.update_window(handle, |_, window, cx| {
        let a = server.read(cx).file_row(false, &local.join("a.txt"), cx);
        let b = server.read(cx).file_row(false, &local.join("b.txt"), cx);
        window.click(a, cx);
        window.click_with_modifiers(
            b,
            Modifiers {
                platform: true,
                ..Default::default()
            },
            cx,
        );
    })
    .unwrap();
    settle(cx, handle, view);
    cx.update(|cx| assert_eq!(server.read(cx).services.files.local.selection.len(), 2));
    cx.update_window(handle, |_, window, cx| {
        window.press("escape", cx);
        let folder = server.read(cx).file_row(false, &local.join("folder"), cx);
        let b = server.read(cx).file_row(false, &local.join("b.txt"), cx);
        window.click(folder, cx);
        window.click_with_modifiers(
            b,
            Modifiers {
                shift: true,
                ..Default::default()
            },
            cx,
        );
    })
    .unwrap();
    settle(cx, handle, view);
    cx.update(|cx| assert_eq!(server.read(cx).services.files.local.selection.len(), 3));
    cx.update_window(handle, |_, window, cx| {
        window.press("escape", cx);
        window.press("cmd-a", cx);
    })
    .unwrap();
    settle(cx, handle, view);
    cx.update(|cx| {
        let browser = &server.read(cx).services.files.local;
        assert_eq!(browser.selection.len(), 3);
        assert!(!browser.selection.contains(&local.join(".hidden")));
    });
    // 功能：从已全选的一行开始拖动，三个顶层项目均应上传且保留子目录；时间：2026-11-11；作者：lq。
    cx.update_window(handle, |_, window, cx| {
        let source = server.read(cx).file_row(false, &local.join("a.txt"), cx);
        window.drag_to(source, "remote-files-drop", cx);
    })
    .unwrap();
    settle(cx, handle, view);
    for (name, text) in [
        ("a.txt", "alpha"),
        ("b.txt", "beta"),
        ("folder/nested.txt", "nested"),
    ] {
        assert_eq!(
            client.open_document(&uploaded.join(name)).unwrap().text,
            text
        );
    }
    assert!(
        client
            .list(&uploaded.join("folder/empty"))
            .unwrap()
            .is_empty()
    );
    assert_eq!(client.list(&uploaded).unwrap().len(), 3);
    cx.capture_screenshot(handle)
        .unwrap()
        .save("output/sidebar-files/explorer-details.png")
        .unwrap();
    cx.update(|cx| {
        let state = server.read(cx);
        let entry = state.services.files.remote.entries[&uploaded]
            .iter()
            .find(|e| e.name == "a.txt")
            .unwrap();
        assert!(entry.modified.is_some());
        assert!(entry.uid.is_some());
        assert!(entry.gid.is_some());
        assert_ne!(entry.permissions, 0);
    });
    // 功能：双列表横向滚动可查看远端全部属性列，不改变当前目录；时间：2026-11-12；作者：lq。
    cx.update_window(handle, |_, window, cx| {
        drag_file_scrollbar(window, FileSide::Remote, Axis::Horizontal, true, cx);
    })
    .unwrap();
    settle(cx, handle, view);
    cx.update(|cx| {
        let browser = &server.read(cx).services.files.remote;
        assert!(browser.horizontal.offset().x < px(0.));
        assert_eq!(browser.current, uploaded);
    });
    cx.capture_screenshot(handle)
        .unwrap()
        .save("output/sidebar-files/explorer-attributes.png")
        .unwrap();
    cx.update_window(handle, |_, window, cx| {
        drag_file_scrollbar(window, FileSide::Remote, Axis::Horizontal, false, cx);
    })
    .unwrap();
    settle(cx, handle, view);
    // 功能：远程全选后拖入本地列表空白区，核对所有文件与空目录真实下载；时间：2026-11-12；作者：lq。
    cx.update_window(handle, |_, window, cx| {
        server.update(cx, |state, cx| {
            state.navigate_directory(FileSide::Local, download.clone(), window, cx)
        });
    })
    .unwrap();
    settle(cx, handle, view);
    cx.update_window(handle, |_, window, cx| {
        window.click("select-all-remote", cx)
    })
    .unwrap();
    settle(cx, handle, view);
    cx.update_window(handle, |_, window, cx| {
        let source = server.read(cx).file_row(true, &uploaded.join("b.txt"), cx);
        window.drag_to(source, "local-files-drop", cx);
    })
    .unwrap();
    settle(cx, handle, view);
    for (name, text) in [
        ("a.txt", "alpha"),
        ("b.txt", "beta"),
        ("folder/nested.txt", "nested"),
    ] {
        assert_eq!(std::fs::read_to_string(download.join(name)).unwrap(), text);
    }
    assert!(download.join("folder/empty").is_dir());
    // 功能：隐藏本地后远程占满区域，下载仍使用原本地目录，重新展开保留选择；时间：2026-11-12；作者：lq。
    cx.update_window(handle, |_, window, cx| {
        let local_row = server.read(cx).file_row(false, &download.join("a.txt"), cx);
        window.click(local_row, cx);
        window.click("toggle-local-files", cx);
    })
    .unwrap();
    settle(cx, handle, view);
    cx.update_window(handle, |_, window, cx| {
        assert!(window.try_find("server-local-pane").is_none());
        let remote = window.find("server-remote-pane").bounds();
        let terminal = window.find("server-terminal-pane").bounds();
        assert_eq!(remote.size.width, terminal.size.width);
        assert_eq!(server.read(cx).services.files.local.destination(), download);
        assert_eq!(server.read(cx).services.files.local.selection.len(), 1);
    })
    .unwrap();
    cx.capture_screenshot(handle)
        .unwrap()
        .save("output/sidebar-files/remote-only.png")
        .unwrap();
    verify_remote_tree_navigation(cx, handle, view, server, &uploaded);
    // 功能：本地隐藏时通过下载按钮完成真实写入，核对目标仍为原目录；时间：2026-11-12；作者：lq。
    let extra = uploaded.join("remote-only.txt");
    let seed = root.join("remote-only-seed.txt");
    std::fs::write(&seed, "remote-only download").unwrap();
    client
        .upload(&seed, &extra, &sftp::Transfer::default())
        .unwrap();
    cx.update_window(handle, |_, window, cx| {
        server.update(cx, |state, cx| state.refresh_remote(window, cx));
    })
    .unwrap();
    settle(cx, handle, view);
    cx.update_window(handle, |_, window, cx| {
        let row = server.read(cx).file_row(true, &extra, cx);
        window.click(row, cx);
    })
    .unwrap();
    settle(cx, handle, view);
    cx.update_window(handle, |_, window, cx| window.click("download", cx))
        .unwrap();
    settle(cx, handle, view);
    assert_eq!(
        std::fs::read_to_string(download.join("remote-only.txt")).unwrap(),
        "remote-only download"
    );
    cx.update_window(handle, |_, window, cx| {
        window.click("toggle-local-files", cx)
    })
    .unwrap();
    settle(cx, handle, view);
    cx.update_window(handle, |_, window, cx| {
        assert!(window.find("server-local-pane").visible());
        assert!(window.try_find("remote-directory-tree").is_none());
        let state = server.read(cx);
        assert_eq!(state.services.files.local.current, download);
        assert!(
            state
                .services
                .files
                .local
                .selection
                .contains(&download.join("a.txt"))
        );
        assert!(state.services.files.remote.selection.contains(&extra));
    })
    .unwrap();
    cx.capture_screenshot(handle)
        .unwrap()
        .save("output/sidebar-files/explorer-batch-download.png")
        .unwrap();
    println!(
        "PASS explorer: enter/parent, dual lists, cmd/shift selection, keyboard/reveal select-all, batch drag upload/download, nested and empty folders, remote attributes"
    );
}

/// 功能：原生点击远程树验证展开不导航、单击导航、地址栏定位和模式恢复；时间：2026-11-12；作者：lq。
fn verify_remote_tree_navigation(
    cx: &mut HeadlessAppContext,
    handle: AnyWindowHandle,
    view: &Entity<Workbench>,
    server: &Entity<ServerWorkspace>,
    uploaded: &Path,
) {
    let folder = uploaded.join("folder");
    let empty = folder.join("empty");
    let row = reveal_remote_tree_row(cx, handle, view, server, &folder);
    cx.update(|cx| {
        let state = server.read(cx);
        let tree = state.services.files.remote_tree.state.read(cx);
        assert!(
            tree.index_of(&SharedString::from(
                uploaded.join("a.txt").display().to_string()
            ))
            .is_none()
        );
        assert_eq!(state.services.files.remote.current, uploaded);
    });
    cx.update_window(handle, |_, window, cx| {
        window.click(("remote-directory-toggle", row), cx)
    })
    .unwrap();
    settle(cx, handle, view);
    cx.update(|cx| {
        let state = server.read(cx);
        assert_eq!(
            state.services.files.remote.current, uploaded,
            "expansion must not navigate"
        );
        assert!(
            state
                .services
                .files
                .remote_tree
                .state
                .read(cx)
                .index_of(&SharedString::from(empty.display().to_string()))
                .is_some()
        );
    });
    let row = reveal_remote_tree_row(cx, handle, view, server, &folder);
    cx.update_window(handle, |_, window, cx| {
        window.click(("remote-directory", row), cx)
    })
    .unwrap();
    settle(cx, handle, view);
    cx.update(|cx| {
        let state = server.read(cx);
        assert_eq!(state.services.files.remote.current, folder);
        assert_eq!(
            state.services.remote_path.read(cx).value().as_str(),
            folder.to_str().unwrap()
        );
        state.file_row(true, &folder.join("nested.txt"), cx);
    });
    // 功能：列表的上级与双击进入同步树中高亮，地址栏回车可重新定位祖先目录；时间：2026-11-12；作者：lq。
    cx.update_window(handle, |_, window, cx| {
        window.click("parent-directory-remote", cx)
    })
    .unwrap();
    settle(cx, handle, view);
    cx.update_window(handle, |_, window, cx| {
        let row = server.read(cx).file_row(true, &folder, cx);
        window.double_click(row, cx);
    })
    .unwrap();
    settle(cx, handle, view);
    cx.update(|cx| {
        let state = server.read(cx);
        let selected = state
            .services
            .files
            .remote_tree
            .state
            .read(cx)
            .selected_item()
            .unwrap();
        assert_eq!(selected.id.as_str(), folder.to_str().unwrap());
    });
    cx.update_window(handle, |_, window, cx| {
        server.update(cx, |state, cx| {
            state.services.remote_path.update(cx, |input, cx| {
                input.set_value(uploaded.display().to_string(), window, cx)
            });
        });
        // 功能：真实聚焦地址栏并按回车，覆盖输入订阅而非直接调用刷新；时间：2026-11-12；作者：lq。
        let input = server.read(cx).services.remote_path.clone();
        input.update(cx, |input, cx| input.focus(window, cx));
        window.press("enter", cx);
    })
    .unwrap();
    settle(cx, handle, view);
    cx.update(|cx| {
        let state = server.read(cx);
        assert_eq!(state.services.files.remote.current, uploaded);
        assert_eq!(
            state
                .services
                .files
                .remote_tree
                .state
                .read(cx)
                .selected_item()
                .unwrap()
                .id
                .as_str(),
            uploaded.to_str().unwrap()
        );
    });
    cx.capture_screenshot(handle)
        .unwrap()
        .save("output/sidebar-files/remote-tree-navigation.png")
        .unwrap();
    println!(
        "PASS remote tree: directories only, lazy expand without navigation, click/parent/double-click/address synchronization"
    );
}

/// 功能：测试先揭示目录行再发送鼠标事件，避免根目录兄弟节点较多导致目标不可见；时间：2026-11-12；作者：lq。
fn reveal_remote_tree_row(
    cx: &mut HeadlessAppContext,
    handle: AnyWindowHandle,
    view: &Entity<Workbench>,
    server: &Entity<ServerWorkspace>,
    path: &Path,
) -> usize {
    let index = cx.update(|cx| {
        let state = server.read(cx).services.files.remote_tree.state.clone();
        state.update(cx, |state, _| {
            let index = state
                .index_of(&SharedString::from(path.display().to_string()))
                .expect("directory must be in remote tree");
            state.scroll_to_item(index, ScrollStrategy::Center);
            index
        })
    });
    settle(cx, handle, view);
    index
}

/// 功能：真实拖动固定视口滚动条滑块，验证手动交互而非直接修改偏移；时间：2026-11-12；作者：lq。
fn drag_file_scrollbar(
    window: &mut Window,
    side: FileSide,
    axis: Axis,
    to_end: bool,
    cx: &mut App,
) {
    let vertical = axis == Axis::Vertical;
    let bounds = window
        .find(SharedString::from(format!(
            "file-{}-track-{}",
            if vertical { "vertical" } else { "horizontal" },
            side.id()
        )))
        .bounds();
    let start = if vertical {
        point(bounds.right() - px(8.), bounds.origin.y + px(8.))
    } else {
        point(bounds.origin.x + px(8.), bounds.bottom() - px(8.))
    };
    let end = if vertical {
        point(start.x, bounds.bottom() - px(8.))
    } else {
        point(bounds.right() - px(8.), start.y)
    };
    let (from, to) = if to_end { (start, end) } else { (end, start) };
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
    for step in 1..=5 {
        let position = from + (to - from) * (step as f32 / 5.);
        window.dispatch_event(
            MouseMoveEvent {
                position,
                pressed_button: Some(MouseButton::Left),
                modifiers: Modifiers::default(),
            }
            .to_platform_input(),
            cx,
        );
        window.render_frame(cx);
    }
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

/// 功能：长目录回归滚轮纵向与双轴拖动，覆盖两侧列表、窄窗口及远程树模式；时间：2026-11-12；作者：lq。
fn verify_file_scrolling(
    cx: &mut HeadlessAppContext,
    handle: AnyWindowHandle,
    view: &Entity<Workbench>,
    server: &Entity<ServerWorkspace>,
    client: &sftp::Client,
    root: &Path,
    remote_root: &Path,
) {
    let local = root.join("scrolling");
    let remote = remote_root.join("scrolling");
    std::fs::create_dir_all(&local).unwrap();
    for index in 0..80 {
        std::fs::write(
            local.join(format!("file-{index:03}.txt")),
            format!("row {index}"),
        )
        .unwrap();
    }
    client
        .upload_path(&local, &remote, &sftp::Transfer::default())
        .unwrap();
    cx.update_window(handle, |_, window, cx| {
        server.update(cx, |state, cx| {
            state.navigate_directory(FileSide::Local, local.clone(), window, cx);
            state.navigate_directory(FileSide::Remote, remote.clone(), window, cx);
        });
    })
    .unwrap();
    settle(cx, handle, view);
    for (width, height) in [(1440., 900.), (960., 640.)] {
        cx.update_window(handle, |_, window, cx| {
            window.resize(size(px(width), px(height)));
            window.bounds_changed(cx);
        })
        .unwrap();
        settle(cx, handle, view);
        for side in [FileSide::Local, FileSide::Remote] {
            cx.update_window(handle, |_, window, cx| {
                // 功能：从顶部发普通鼠标滚轮，必须改变纵向而非横向偏移；时间：2026-11-12；作者：lq。
                server.update(cx, |state, _| {
                    let browser = state.services.files.browser(side);
                    browser.scroll.set_offset(point(px(0.), px(0.)));
                    browser.horizontal.set_offset(point(px(0.), px(0.)));
                });
                window.render_frame(cx);
                window.scroll(
                    SharedString::from(format!("file-details-scroll-{}", side.id())),
                    ScrollDelta::Lines(point(0., -3.)),
                    cx,
                );
            })
            .unwrap();
            settle(cx, handle, view);
            cx.update(|cx| {
                let browser = server.read(cx).services.files.browser(side);
                assert!(browser.scroll.offset().y < px(0.), "wheel must scroll down");
                assert_eq!(
                    browser.horizontal.offset().x,
                    px(0.),
                    "vertical wheel must not move columns"
                );
            });
            cx.update_window(handle, |_, window, cx| {
                // 功能：滚到底部后再滚轮也不得触发横向，之后真实拖动横条；时间：2026-11-12；作者：lq。
                window.scroll(
                    SharedString::from(format!("file-details-scroll-{}", side.id())),
                    ScrollDelta::Pixels(point(px(0.), px(-9000.))),
                    cx,
                );
            })
            .unwrap();
            settle(cx, handle, view);
            cx.update_window(handle, |_, window, cx| {
                let row = (
                    if side == FileSide::Local {
                        "local-entry"
                    } else {
                        "remote-entry"
                    },
                    79usize,
                );
                assert!(window.find(row).visible(), "last file must be reachable");
                window.scroll(
                    SharedString::from(format!("file-details-scroll-{}", side.id())),
                    ScrollDelta::Lines(point(0., -3.)),
                    cx,
                );
                assert_eq!(
                    server
                        .read(cx)
                        .services
                        .files
                        .browser(side)
                        .horizontal
                        .offset()
                        .x,
                    px(0.)
                );
                drag_file_scrollbar(window, side, Axis::Horizontal, true, cx);
            })
            .unwrap();
            settle(cx, handle, view);
            cx.update(|cx| {
                assert!(
                    server
                        .read(cx)
                        .services
                        .files
                        .browser(side)
                        .horizontal
                        .offset()
                        .x
                        < px(0.)
                )
            });
            cx.update_window(handle, |_, window, cx| {
                drag_file_scrollbar(window, side, Axis::Horizontal, false, cx);
                drag_file_scrollbar(window, side, Axis::Vertical, false, cx);
            })
            .unwrap();
            settle(cx, handle, view);
            cx.update_window(handle, |_, window, cx| {
                assert!(
                    window
                        .find((
                            if side == FileSide::Local {
                                "local-entry"
                            } else {
                                "remote-entry"
                            },
                            0usize
                        ))
                        .visible()
                );
                drag_file_scrollbar(window, side, Axis::Vertical, true, cx);
            })
            .unwrap();
            settle(cx, handle, view);
            cx.update_window(handle, |_, window, _| {
                assert!(
                    window
                        .find((
                            if side == FileSide::Local {
                                "local-entry"
                            } else {
                                "remote-entry"
                            },
                            79usize
                        ))
                        .visible(),
                    "vertical thumb drag must reach last file"
                );
            })
            .unwrap();
        }
        cx.capture_screenshot(handle)
            .unwrap()
            .save(format!("output/sidebar-files/scrollbars-{width:.0}.png"))
            .unwrap();
        cx.update_window(handle, |_, window, cx| {
            window.click("toggle-local-files", cx)
        })
        .unwrap();
        settle(cx, handle, view);
        cx.update_window(handle, |_, window, cx| {
            drag_file_scrollbar(window, FileSide::Remote, Axis::Vertical, false, cx);
            window.scroll(
                "file-details-scroll-remote",
                ScrollDelta::Lines(point(0., -5.)),
                cx,
            );
        })
        .unwrap();
        settle(cx, handle, view);
        cx.update(|cx| {
            let browser = &server.read(cx).services.files.remote;
            assert!(browser.scroll.offset().y < px(0.));
            assert_eq!(browser.horizontal.offset().x, px(0.));
        });
        cx.capture_screenshot(handle)
            .unwrap()
            .save(format!(
                "output/sidebar-files/scrollbars-tree-{width:.0}.png"
            ))
            .unwrap();
        cx.update_window(handle, |_, window, cx| {
            window.click("toggle-local-files", cx)
        })
        .unwrap();
        settle(cx, handle, view);
    }
    cx.update_window(handle, |_, window, cx| {
        window.resize(size(px(1440.), px(900.)));
        window.bounds_changed(cx);
    })
    .unwrap();
    settle(cx, handle, view);
    println!(
        "PASS file scrolling: 80 real local/remote files, vertical line/pixel wheel, end-of-list, both scrollbar thumb drags, 1440/960 dual-list and remote-tree modes"
    );
}
