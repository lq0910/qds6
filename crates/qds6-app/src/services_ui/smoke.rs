//! 功能：真实 SSH+SFTP 同屏、多服务器隔离和独立数据库工作区回归；时间：2026-11-11；作者：lq。

use super::*;
use crate::workspace_ui::{WorkspaceArea, WorkspaceTarget};
use gpui_kit::test::TestWindowExt;
use std::{
    thread,
    time::{Duration, Instant},
};

/// 功能：等待全局任务和每台服务器自己的文件/草稿任务，限制总等待；时间：2026-11-11；作者：lq。
fn settle(cx: &mut HeadlessAppContext, handle: AnyWindowHandle, view: &Entity<Workbench>) {
    let start = Instant::now();
    loop {
        cx.run_until_parked();
        cx.advance_clock(Duration::from_millis(100));
        let busy = cx.update(|cx| {
            let state = view.read(cx);
            state.services.busy
                || state.loading
                || state
                    .services
                    .servers
                    .iter()
                    .any(|server| server.read(cx).pending())
        });
        if !busy {
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(30),
            "UI service operation timed out"
        );
        thread::sleep(Duration::from_millis(30));
    }
    for _ in 0..8 {
        cx.advance_clock(Duration::from_millis(100));
        cx.run_until_parked();
        cx.update_window(handle, |_, window, cx| {
            window.simulate_next_frame(cx);
            window.render_frame(cx);
        })
        .unwrap();
    }
}

/// 功能：验证上下双区和本地/远程双栏都处于可见工作区，操作栏不覆盖文件；时间：2026-11-11；作者：lq。
fn assert_server_bounds(window: &Window) {
    let terminal = window.find("server-terminal-pane").bounds();
    let files = window.find("server-files-pane").bounds();
    let local = window.find("server-local-pane").bounds();
    let remote = window.find("server-remote-pane").bounds();
    let status = window.find("workspace-status").bounds();
    assert!(terminal.size.height >= px(130.));
    assert!(files.size.height >= px(190.));
    assert!(
        terminal.bottom() <= files.origin.y + px(1.),
        "terminal/files overlap: {terminal:?}/{files:?}"
    );
    assert!(
        local.right() <= remote.origin.x + px(1.),
        "file columns overlap"
    );
    assert!(
        files.bottom() <= status.origin.y,
        "file panel overlaps global status"
    );
    assert!(
        remote.right() <= window.viewport_size().width,
        "remote pane exceeds viewport"
    );
    let upload = window.find("upload").bounds();
    assert!(
        upload.bottom() <= files.bottom(),
        "transfer action exceeds file pane"
    );
}

/// 功能：仅操作回环专用服务，使用独立配置、已知主机文件及临时草稿密钥；时间：2026-11-11；作者：lq。
pub fn run_service_ui_smoke() {
    assert_eq!(std::env::var("QDS6_FIXTURES").as_deref(), Ok("1"));
    qds6_services::credentials::use_fixture_draft_key(
        *uuid::Uuid::new_v4()
            .as_bytes()
            .repeat(2)
            .first_chunk::<32>()
            .unwrap(),
    );
    let root = std::env::temp_dir().join(format!("qds6-server-ui-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let output = PathBuf::from("output/service-ui");
    std::fs::create_dir_all(&output).unwrap();
    let mut ssh_profile = ConnectionProfile::new(Protocol::Ssh);
    ssh_profile.name = "测试服务器 / 中文".into();
    ssh_profile.host = "127.0.0.1".into();
    ssh_profile.port = 22222;
    ssh_profile.username = "qds6".into();
    let fingerprint = match ssh::connect(
        &ssh_profile,
        Some("qds6-fixture-only"),
        &root.join("known-hosts.json"),
    ) {
        Err(SshError::UnknownHost { fingerprint }) => fingerprint,
        _ => panic!("first host must require trust"),
    };
    ssh::trust_host(&root.join("known-hosts.json"), &ssh_profile, &fingerprint).unwrap();
    // 功能：测试连接仅保存到专用数据库，截图中的连接列表与会话真实对应；时间：2026-11-11；作者：lq。
    ProfileStore::open(root.join("connections.sqlite3"))
        .unwrap()
        .save(&ssh_profile)
        .unwrap();
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
            state.replace_form(ssh_profile.clone(), window, cx);
            state.form.secret.update(cx, |input, cx| {
                input.set_value("qds6-fixture-only", window, cx)
            });
            state.connect_service(window, cx);
        })
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    let first = cx.update(|cx| {
        let state = view.read(cx);
        assert_eq!(state.services.servers.len(), 1);
        let server = state.services.servers[0].clone();
        assert!(
            server.read(cx).terminal.is_some(),
            "SSH must open a terminal"
        );
        assert!(
            server.read(cx).services.sftp.is_some(),
            "SSH must also open SFTP automatically"
        );
        server
    });
    cx.update_window(handle, |_, window, cx| {
        window.click("terminal-grid", cx);
        window.input("printf '同屏终端中文验证\\n'", cx);
        window.press("enter", cx);
    })
    .unwrap();
    thread::sleep(Duration::from_millis(400));
    settle(&mut cx, handle, &view);
    cx.update(|cx| {
        assert!(
            first
                .read(cx)
                .terminal
                .as_ref()
                .unwrap()
                .read(cx)
                .screen_text()
                .contains("同屏终端中文验证")
        )
    });
    // 功能：真实目录、嵌套文件与空目录验证树形浏览、双向拖放和按钮自动路径；时间：2026-11-11；作者：lq。
    let client = sftp::Client::new(
        ssh::connect(
            &ssh_profile,
            Some("qds6-fixture-only"),
            &root.join("known-hosts.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let files_root = root.join("本地文件");
    std::fs::create_dir_all(files_root.join("项目资源/config")).unwrap();
    std::fs::create_dir_all(files_root.join("下载内容")).unwrap();
    std::fs::create_dir_all(files_root.join("项目资源/empty")).unwrap();
    std::fs::write(
        files_root.join("项目资源/config/app.json"),
        "{\"name\":\"QDS6\"}\n",
    )
    .unwrap();
    std::fs::write(files_root.join("上传说明.txt"), "双向传输中文验证\n").unwrap();
    std::fs::write(files_root.join("按钮上传.txt"), "selected transfer\n").unwrap();
    let remote_root = PathBuf::from(format!("/home/qds6/qds6-files-{}", uuid::Uuid::new_v4()));
    client.mkdir(&remote_root).unwrap();
    client.mkdir(&remote_root.join("服务器目录")).unwrap();
    let seed = root.join("server-note.txt");
    std::fs::write(&seed, "来自服务器\n").unwrap();
    client
        .upload(
            &seed,
            &remote_root.join("服务器说明.txt"),
            &sftp::Transfer::default(),
        )
        .unwrap();
    cx.update_window(handle, |_, window, cx| {
        first.update(cx, |server, cx| {
            server.services.local_directory.update(cx, |input, cx| {
                input.set_value(files_root.display().to_string(), window, cx)
            });
            server.services.remote_path.update(cx, |input, cx| {
                input.set_value(remote_root.display().to_string(), window, cx)
            });
            server.refresh_local(window, cx);
            server.refresh_remote(window, cx);
        })
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.update_window(handle, |_, window, cx| {
        let local = first
            .read(cx)
            .file_row(false, &files_root.join("项目资源"), cx);
        window.double_click(local, cx);
        let remote = first
            .read(cx)
            .file_row(true, &remote_root.join("服务器目录"), cx);
        window.double_click(remote, cx);
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.update_window(handle, |_, window, cx| {
        let source = first
            .read(cx)
            .file_row(false, &files_root.join("上传说明.txt"), cx);
        let target = first
            .read(cx)
            .file_row(true, &remote_root.join("服务器目录"), cx);
        window.drag_to(source, target, cx);
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.capture_screenshot(handle)
        .unwrap()
        .save(output.join("file-tree-debug.png"))
        .unwrap();
    assert_eq!(
        client
            .open_document(&remote_root.join("服务器目录/上传说明.txt"))
            .unwrap()
            .text,
        "双向传输中文验证\n"
    );
    cx.update_window(handle, |_, window, cx| {
        let source = first
            .read(cx)
            .file_row(true, &remote_root.join("服务器说明.txt"), cx);
        let target = first
            .read(cx)
            .file_row(false, &files_root.join("下载内容"), cx);
        window.drag_to(source, target, cx);
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    assert_eq!(
        std::fs::read_to_string(files_root.join("下载内容/服务器说明.txt")).unwrap(),
        "来自服务器\n"
    );
    // 功能：文件夹拖放递归传输，下载至同级目录以验证空目录与层级保留；时间：2026-11-11；作者：lq。
    cx.update_window(handle, |_, window, cx| {
        let source = first
            .read(cx)
            .file_row(false, &files_root.join("项目资源"), cx);
        let target = first
            .read(cx)
            .file_row(true, &remote_root.join("服务器目录"), cx);
        window.drag_to(source, target, cx);
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    assert_eq!(
        client
            .open_document(&remote_root.join("服务器目录/项目资源/config/app.json"))
            .unwrap()
            .text,
        "{\"name\":\"QDS6\"}\n"
    );
    cx.update_window(handle, |_, window, cx| {
        let source = first
            .read(cx)
            .file_row(true, &remote_root.join("服务器目录/项目资源"), cx);
        let target = first
            .read(cx)
            .file_row(false, &files_root.join("下载内容"), cx);
        window.drag_to(source, target, cx);
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    assert!(
        files_root
            .join("下载内容/项目资源/config/app.json")
            .exists()
    );
    cx.update_window(handle, |_, window, cx| {
        let source = first
            .read(cx)
            .file_row(false, &files_root.join("按钮上传.txt"), cx);
        let target = first
            .read(cx)
            .file_row(true, &remote_root.join("服务器目录"), cx);
        window.click(target, cx);
        window.click(source, cx);
        window.click("upload", cx);
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    assert_eq!(
        client
            .open_document(&remote_root.join("服务器目录/按钮上传.txt"))
            .unwrap()
            .text,
        "selected transfer\n"
    );
    // 功能：重复上传拒绝覆盖，原始远程内容保持一致；时间：2026-11-11；作者：lq。
    std::fs::write(files_root.join("按钮上传.txt"), "must not overwrite").unwrap();
    cx.update_window(handle, |_, window, cx| window.click("upload", cx))
        .unwrap();
    settle(&mut cx, handle, &view);
    assert_eq!(
        client
            .open_document(&remote_root.join("服务器目录/按钮上传.txt"))
            .unwrap()
            .text,
        "selected transfer\n"
    );
    assert!(cx.update(|cx| first.read(cx).status.to_string().contains("失败")));
    cx.update_window(handle, |_, window, cx| {
        let source =
            first
                .read(cx)
                .file_row(true, &remote_root.join("服务器目录/按钮上传.txt"), cx);
        let target = first
            .read(cx)
            .file_row(false, &files_root.join("下载内容"), cx);
        window.click(target, cx);
        window.click(source, cx);
        window.click("download", cx);
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    assert_eq!(
        std::fs::read_to_string(files_root.join("下载内容/按钮上传.txt")).unwrap(),
        "selected transfer\n"
    );
    // 功能：模拟其他远程操作占用任务槽后，以原生拖动验证排队和清空；时间：2026-11-11；作者：lq。
    cx.update_window(handle, |_, window, cx| {
        let root_row = first.read(cx).file_row(true, &remote_root, cx);
        window.double_click(root_row, cx);
        first.update(cx, |server, cx| {
            server.services.busy = true;
            cx.notify();
        });
        let target = window.find("remote-files-drop").bounds();
        let destination = point(target.center().x, target.bottom() - px(10.));
        for path in [
            files_root.join("上传说明.txt"),
            files_root.join("按钮上传.txt"),
        ] {
            let source = first.read(cx).file_row(false, &path, cx);
            window.render_frame(cx);
            let source = window.find(source).bounds().center();
            window.drag(source, destination, cx);
        }
        assert!(window.find("clear-transfer-queue").visible());
        window.click("clear-transfer-queue", cx);
        first.update(cx, |server, cx| {
            server.services.busy = false;
            cx.notify();
        });
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    assert!(
        !client
            .list(&remote_root)
            .unwrap()
            .iter()
            .any(|entry| entry.name == "上传说明.txt" || entry.name == "按钮上传.txt")
    );
    // 功能：原生拖动启动较大文件，取消当前任务后不发布最终文件；时间：2026-11-11；作者：lq。
    let cancel_source = files_root.join("取消传输.bin");
    std::fs::write(&cancel_source, vec![42u8; 32 * 1024 * 1024]).unwrap();
    cx.update_window(handle, |_, window, cx| {
        first.update(cx, |server, cx| server.refresh_local(window, cx))
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.update_window(handle, |_, window, cx| {
        let source = first.read(cx).file_row(false, &cancel_source, cx);
        window.render_frame(cx);
        let source = window.find(source).bounds().center();
        let target = window.find("remote-files-drop").bounds();
        window.drag(
            source,
            point(target.center().x, target.bottom() - px(10.)),
            cx,
        );
        window.click("cancel-transfer", cx);
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    assert!(
        !client
            .list(&remote_root)
            .unwrap()
            .iter()
            .any(|entry| entry.name == "取消传输.bin")
    );
    assert!(cx.update(|cx| first.read(cx).status.to_string().contains("取消")));
    std::fs::remove_file(cancel_source).unwrap();
    // 功能：空白区拖放使用该栏目录，不要求手工输入文件路径；时间：2026-11-11；作者：lq。
    cx.update_window(handle, |_, window, cx| {
        let source = first
            .read(cx)
            .file_row(false, &files_root.join("上传说明.txt"), cx);
        window.render_frame(cx);
        let source = window.find(source).bounds().center();
        let target = window.find("remote-files-drop").bounds();
        window.drag(
            source,
            point(target.center().x, target.bottom() - px(10.)),
            cx,
        );
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    assert_eq!(
        client
            .open_document(&remote_root.join("上传说明.txt"))
            .unwrap()
            .text,
        "双向传输中文验证\n"
    );
    cx.update_window(handle, |_, window, cx| {
        let root_row = first.read(cx).file_row(true, &remote_root, cx);
        window.double_click(root_row, cx);
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    println!(
        "PASS file trees: native expand, bidirectional file/folder drag, selection upload/download, nested bytes, empty folders, collision keeps target, queue/clear/cancel, blank-pane drop"
    );
    // 功能：首个 SSH 连接直接展示三面板，两种逻辑视口都重新布局；时间：2026-11-11；作者：lq。
    cx.update_window(handle, |_, window, cx| {
        first.update(cx, |server, cx| {
            server.services.local_directory.update(cx, |input, cx| {
                input.set_value(files_root.display().to_string(), window, cx)
            });
            server.refresh_local(window, cx);
        })
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    // 功能：截图前恢复树根可见，展示实际层级而非验收滚动停留位置；时间：2026-11-11；作者：lq。
    cx.update(|cx| {
        first.read(cx).file_row(false, &files_root, cx);
        first.read(cx).file_row(true, &remote_root, cx);
    });
    for (width, height) in [(1440., 900.), (960., 640.)] {
        cx.update_window(handle, |_, window, cx| {
            window.resize(size(px(width), px(height)));
            window.bounds_changed(cx);
        })
        .unwrap();
        settle(&mut cx, handle, &view);
        cx.update_window(handle, |_, window, _| assert_server_bounds(window))
            .unwrap();
        cx.capture_screenshot(handle)
            .unwrap()
            .save(output.join(format!(
                "server-combined-{}x{}.png",
                width as u32, height as u32
            )))
            .unwrap();
    }
    cx.update_window(handle, |_, window, cx| {
        window.resize(size(px(1440.), px(900.)));
        window.bounds_changed(cx);
        window.render_frame(cx);
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    // 功能：通过原生鼠标拖动验证上下分隔条和左右文件分隔条，而非只检查状态构造；时间：2026-11-11；作者：lq。
    cx.update_window(handle, |_, window, cx| {
        let before = window.find("server-terminal-pane").bounds();
        let from = point(before.center().x, before.bottom());
        window.drag(from, from + point(px(0.), px(-60.)), cx);
        let after = window.find("server-terminal-pane").bounds();
        assert!(
            after.size.height < before.size.height - px(20.),
            "vertical divider did not resize"
        );
        let before = window.find("server-local-pane").bounds();
        let from = point(before.right(), before.center().y);
        window.drag(from, from + point(px(60.), px(0.)), cx);
        let after = window.find("server-local-pane").bounds();
        assert!(
            after.size.width > before.size.width + px(20.),
            "file divider did not resize"
        );
        window.click("toggle-server-files", cx);
        assert!(window.try_find("server-files-pane").is_none());
        window.click("toggle-server-files", cx);
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    // 功能：保留 Kit 压力验收：真实缓冲、撤销和 Metal 绘制，不代替真人 IME；时间：2026-11-11；作者：lq。
    let large = format!("{}\n", "x".repeat(127)).repeat(40960);
    let started = Instant::now();
    cx.update_window(handle, |_, window, cx| {
        first.update(cx, |server, cx| {
            server.services.page = 2;
            server.services.remote_editor.update(cx, |editor, cx| {
                editor.set_value(large.clone(), window, cx);
                editor.focus(window, cx);
            });
            cx.notify();
        });
        window.render_frame(cx);
        window.press("cmd-a", cx);
        window.input("替换测试", cx);
        for _ in 0..8 {
            if first.read(cx).services.remote_editor.read(cx).value().len() == large.len() {
                break;
            }
            window.press("cmd-z", cx);
        }
        assert_eq!(
            first.read(cx).services.remote_editor.read(cx).value().len(),
            large.len()
        );
        first
            .read(cx)
            .services
            .remote_editor
            .clone()
            .update(cx, |editor, cx| {
                editor.set_value("x".repeat(65536), window, cx)
            });
        window.render_frame(cx);
        first.update(cx, |server, cx| {
            server
                .services
                .remote_editor
                .update(cx, |editor, cx| editor.set_value("", window, cx));
            server.services.page = 1;
            cx.notify();
        });
    })
    .unwrap();
    println!(
        "PASS Kit editor: 5 MiB Metal render, replacement+undo, 64 KiB single-line in {} ms",
        started.elapsed().as_millis()
    );
    // 功能：同名不同配置服务器独立拥有本地路径、远程路径和编辑器；时间：2026-11-11；作者：lq。
    let mut second_profile = ssh_profile.clone();
    second_profile.id = uuid::Uuid::new_v4();
    cx.update_window(handle, |_, window, cx| {
        view.update(cx, |state, cx| {
            state.replace_form(second_profile, window, cx);
            state.form.secret.update(cx, |input, cx| {
                input.set_value("qds6-fixture-only", window, cx)
            });
            state.connect_service(window, cx);
        })
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    let second = cx.update(|cx| {
        assert_eq!(view.read(cx).services.servers.len(), 2);
        view.read(cx).services.servers[1].clone()
    });
    cx.update_window(handle, |_, window, cx| {
        second.update(cx, |server, cx| {
            server
                .services
                .remote_path
                .update(cx, |input, cx| input.set_value("/tmp", window, cx))
        });
        window.within("workspace-tabs").click(2usize, cx);
        assert_eq!(view.read(cx).services.active_server, 0);
        assert_eq!(
            first
                .read(cx)
                .services
                .remote_path
                .read(cx)
                .value()
                .as_str(),
            remote_root.display().to_string()
        );
        window.within("workspace-tabs").click(3usize, cx);
        assert_eq!(
            second
                .read(cx)
                .services
                .remote_path
                .read(cx)
                .value()
                .as_str(),
            "/tmp"
        );
        window.within("workspace-tabs").click(2usize, cx);
    })
    .unwrap();
    let remote = PathBuf::from(format!("/home/qds6/qds6-ui-{}.txt", uuid::Uuid::new_v4()));
    let local = root.join("source.txt");
    std::fs::write(&local, "初始内容\r\n").unwrap();
    client
        .upload(&local, &remote, &sftp::Transfer::default())
        .unwrap();
    cx.update_window(handle, |_, window, cx| {
        first.update(cx, |server, cx| {
            server.open_remote(remote.clone(), window, cx)
        })
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.update_window(handle, |_, window, cx| {
        first
            .read(cx)
            .services
            .remote_editor
            .clone()
            .update(cx, |editor, cx| editor.focus(window, cx));
        window.press("cmd-a", cx);
        window.input("同屏文件中文修改\n", cx);
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.update(|cx| {
        assert!(first.read(cx).services.remote_dirty);
        assert!(!second.read(cx).services.remote_dirty);
        assert!(
            qds6_services::drafts::load(&root.join("drafts"), ssh_profile.id)
                .unwrap()
                .is_some()
        );
    });
    // 功能：文件保存过程中切到另一服务器，完成结果仍回到原实体，不会覆盖其他草稿；时间：2026-11-11；作者：lq。
    cx.update_window(handle, |_, window, cx| {
        window.click("save-remote", cx);
        window.within("workspace-tabs").click(3usize, cx);
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    assert_eq!(
        client.open_document(&remote).unwrap().text,
        "同屏文件中文修改\r\n"
    );
    cx.update(|cx| {
        assert!(!first.read(cx).services.remote_dirty);
        assert!(second.read(cx).services.remote_document.is_none());
    });
    cx.update_window(handle, |_, window, cx| {
        window.within("workspace-tabs").click(2usize, cx)
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.capture_screenshot(handle)
        .unwrap()
        .save(output.join("server-remote-editor.png"))
        .unwrap();
    cx.update_window(handle, |_, window, cx| {
        window.click("server-show-files", cx);
        Theme::change(ThemeMode::Dark, Some(window), cx);
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.capture_screenshot(handle)
        .unwrap()
        .save(output.join("server-combined-dark.png"))
        .unwrap();
    // 功能：数据库连接和标签属于独立功能区，切回服务器保留原有连接；时间：2026-11-11；作者：lq。
    let mut pg = ConnectionProfile::new(Protocol::PostgreSql);
    pg.name = "测试 PostgreSQL 17".into();
    pg.host = "localhost".into();
    pg.port = 25432;
    pg.username = "qds6".into();
    pg.database = "qds6".into();
    pg.db.ca_file = PathBuf::from("output/fixtures/ca.crt")
        .canonicalize()
        .unwrap()
        .display()
        .to_string();
    cx.update_window(handle, |_, window, cx| {
        Theme::change(ThemeMode::Light, Some(window), cx);
        view.update(cx, |state, cx| {
            state.replace_form(pg, window, cx);
            state.form.secret.update(cx, |input, cx| {
                input.set_value("qds6-fixture-only", window, cx)
            });
            state.connect_service(window, cx);
        });
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.update_window(handle, |_, window, cx| {
        assert_eq!(view.read(cx).area, WorkspaceArea::Databases);
        assert!(
            view.read(cx).services.database.is_some(),
            "{}",
            view.read(cx).status
        );
        assert!(window.try_find("server-workspace").is_none());
        view.read(cx).sql.clone().update(cx, |editor, cx| {
            editor.set_value(
                "SELECT '数据库中文' AS text, 9007199254740993::bigint AS exact, NULL AS nullable;",
                window,
                cx,
            )
        });
        window.render_frame(cx);
        window.click("run-sql", cx);
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.update(|cx| {
        assert!(!view.read(cx).error, "{}", view.read(cx).status);
        assert!(view.read(cx).services.grid.is_some());
    });
    cx.capture_screenshot(handle)
        .unwrap()
        .save(output.join("database-separate.png"))
        .unwrap();
    cx.update_window(handle, |_, window, cx| {
        window.resize(size(px(960.), px(640.)));
        window.bounds_changed(cx);
        window.render_frame(cx);
        window.click("toggle-csv-export", cx);
        view.read(cx)
            .services
            .csv_path
            .clone()
            .update(cx, |input, cx| {
                input.set_value(root.join("result.csv").display().to_string(), window, cx)
            });
        window.render_frame(cx);
        window.click("export-csv", cx);
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    assert!(root.join("result.csv").exists());
    cx.capture_screenshot(handle)
        .unwrap()
        .save(output.join("database-separate-960x640.png"))
        .unwrap();
    cx.update_window(handle, |_, window, cx| {
        window.click("open-sql-sidebar", cx);
        window.click("run-sql", cx);
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.update(|cx| assert_eq!(view.read(cx).tab, 2));
    cx.update_window(handle, |_, window, cx| {
        window.click("area-servers", cx);
        assert_eq!(view.read(cx).services.active_server, 0);
        assert!(window.find("terminal-grid").visible());
        window.click("area-databases", cx);
        assert!(view.read(cx).services.grid.is_some());
        window.resize(size(px(1440.), px(900.)));
        window.bounds_changed(cx);
        view.update(cx, |state, cx| {
            state.activate_workspace(WorkspaceTarget::Backup, cx);
            state.services.backup_path.update(cx, |input, cx| {
                input.set_value(root.join("backup.qds6").display().to_string(), window, cx)
            });
            state.services.backup_password.update(cx, |input, cx| {
                input.set_value("fixture-password", window, cx)
            });
        });
        window.render_frame(cx);
        window.click("export-encrypted", cx);
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.update_window(handle, |_, window, cx| {
        window.within("backup-modes").click(2usize, cx);
        window.click("preview-config", cx);
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.update(|cx| assert!(view.read(cx).services.preview.is_some()));
    client.remove(&remote).unwrap();
    cx.update_window(handle, |_, window, _| window.remove_window())
        .unwrap();
    drop(cx);
    std::fs::remove_dir_all(root).unwrap();
    println!(
        "PASS server UI: SSH opens terminal+SFTP, three panes at 1440/960, native divider drags, same-name server isolation, background save across tabs, CRLF/encrypted draft, separate PostgreSQL TLS SQL/CSV workspace, backup preview. Real IME/installer/Windows acceptance separate."
    );
}
