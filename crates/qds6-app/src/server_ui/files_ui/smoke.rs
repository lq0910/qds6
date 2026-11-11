//! 功能：仅连接回环 fixture，复现隐藏目录误报为空并验证原生收放与终端留白；时间：2026-11-11；作者：lq。

use super::*;
use crate::workspace_ui::WorkspaceTarget;
use gpui_kit::test::TestWindowExt;
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
                        .tree
                        .read(cx)
                        .index_of(&SharedString::from(
                            path.join(".profile").display().to_string()
                        ))
                        .is_none()
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
    // 功能：双击进入只显示下一层，父目录树与地址同步，返回上级不残留选择；时间：2026-11-11；作者：lq。
    cx.update_window(handle, |_, window, cx| {
        let row = server.read(cx).file_row(false, &local.join("folder"), cx);
        window.double_click(row, cx);
    })
    .unwrap();
    settle(cx, handle, view);
    cx.update(|cx| {
        let state = server.read(cx);
        assert_eq!(state.services.files.local.current, local.join("folder"));
        assert_eq!(state.services.files.local.root, local);
        assert_eq!(state.services.files.local.visible_entries(false).len(), 2);
        assert!(state.services.files.local.selection.is_empty());
        assert_eq!(
            state.services.local_directory.read(cx).value().as_str(),
            local.join("folder").to_str().unwrap()
        );
        assert!(
            state
                .services
                .files
                .local
                .tree
                .read(cx)
                .index_of(&SharedString::from(
                    local.join("folder/nested.txt").display().to_string()
                ))
                .is_none()
        );
    });
    cx.update_window(handle, |_, window, cx| {
        window.click("parent-directory-local", cx)
    })
    .unwrap();
    settle(cx, handle, view);
    // 功能：单击左侧目录树也可定位当前文件夹；时间：2026-11-11；作者：lq。
    cx.update_window(handle, |_, window, cx| {
        let index = server
            .read(cx)
            .services
            .files
            .local
            .tree
            .read(cx)
            .index_of(&SharedString::from(
                local.join("folder").display().to_string(),
            ))
            .unwrap();
        window.click(("local-directory", index), cx);
    })
    .unwrap();
    settle(cx, handle, view);
    cx.update(|cx| {
        assert_eq!(
            server.read(cx).services.files.local.current,
            local.join("folder")
        )
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
    // 功能：目录树收起后保留当前路径，横向滚动能实际查看远端全部属性列；时间：2026-11-11；作者：lq。
    cx.update_window(handle, |_, window, cx| {
        window.click("toggle-directories-remote", cx);
        window.scroll(
            "file-details-scroll-remote",
            ScrollDelta::Pixels(point(px(-900.), px(0.))),
            cx,
        );
    })
    .unwrap();
    settle(cx, handle, view);
    cx.update(|cx| {
        let browser = &server.read(cx).services.files.remote;
        assert!(!browser.show_directories);
        assert!(browser.horizontal.offset().x < px(0.));
        assert_eq!(browser.current, uploaded);
    });
    cx.capture_screenshot(handle)
        .unwrap()
        .save("output/sidebar-files/explorer-attributes.png")
        .unwrap();
    cx.update_window(handle, |_, window, cx| {
        window.scroll(
            "file-details-scroll-remote",
            ScrollDelta::Pixels(point(px(900.), px(0.))),
            cx,
        );
        window.click("toggle-directories-remote", cx);
    })
    .unwrap();
    settle(cx, handle, view);
    // 功能：远程全选后拖入本地目录树，核对所有文件与空目录真实下载；时间：2026-11-11；作者：lq。
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
        let target = server
            .read(cx)
            .services
            .files
            .local
            .tree
            .read(cx)
            .index_of(&SharedString::from(download.display().to_string()))
            .unwrap();
        window.drag_to(source, ("local-directory", target), cx);
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
    cx.capture_screenshot(handle)
        .unwrap()
        .save("output/sidebar-files/explorer-batch-download.png")
        .unwrap();
    println!(
        "PASS explorer: enter/parent, directory-only tree, cmd/shift selection, keyboard/reveal select-all, batch drag upload/download, nested and empty folders, remote attributes"
    );
}
