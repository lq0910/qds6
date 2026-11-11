//! 功能：删除连接只刷新列表，取消及后台完成不抢走当前工作区；时间：2026-11-11；作者：lq。

use super::{ConnectionProfile, ProfileStore, Protocol, Workbench};
use crate::workspace_ui::{WorkspaceArea, WorkspaceTarget};
use gpui_kit::{AppContext, Entity, TestAppContext, WindowOptions};

/// 功能：等待真实后台 SQLite 操作结束，避免用虚拟时间冒充持久化完成；时间：2026-11-11；作者：lq。
fn settle(cx: &mut TestAppContext, view: &Entity<Workbench>) {
    let start = std::time::Instant::now();
    loop {
        cx.run_until_parked();
        if cx.read(|cx| {
            let state = view.read(cx);
            state.ready && !state.loading && !state.services.busy
        }) {
            break;
        }
        assert!(start.elapsed() < std::time::Duration::from_secs(10));
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

/// 功能：覆盖两种数据库、服务器、最后一条删除、取消及确认前切换文档；时间：2026-11-11；作者：lq。
#[test]
fn deleting_connections_preserves_area_and_other_documents() {
    for protocol in [Protocol::PostgreSql, Protocol::MySql, Protocol::Ssh] {
        let temporary = std::env::temp_dir().join(format!("qds6-delete-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&temporary).unwrap();
        let path = temporary.join("connections.sqlite3");
        let mut profile = ConnectionProfile::new(protocol);
        profile.name = "delete fixture".into();
        profile.host = "localhost".into();
        let mut other = ConnectionProfile::new(Protocol::Ssh);
        other.name = "unrelated server".into();
        other.host = "localhost".into();
        let mut store = ProfileStore::open(&path).unwrap();
        store.save(&profile).unwrap();
        store.save(&other).unwrap();
        let mut cx = TestAppContext::single();
        cx.skip_drawing();
        cx.update(gpui_kit::init);
        let (handle, view) = cx.update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
                cx.new(|cx| Workbench::new_with_store(Some(path.clone()), window, cx))
            })
            .unwrap()
        });
        settle(&mut cx, &view);
        cx.update_window(handle, |_, window, cx| {
            view.update(cx, |state, cx| {
                state.replace_form(profile.clone(), window, cx);
                state.delete_profile(window, cx);
            });
        })
        .unwrap();
        cx.run_until_parked();
        cx.simulate_prompt_answer("取消");
        settle(&mut cx, &view);
        assert_eq!(store.list().unwrap().len(), 2);
        cx.read(|cx| assert_eq!(view.read(cx).form.profile.id, profile.id));

        cx.update_window(handle, |_, window, cx| {
            view.update(cx, |state, cx| state.delete_profile(window, cx));
        })
        .unwrap();
        cx.run_until_parked();
        cx.simulate_prompt_answer("删除");
        settle(&mut cx, &view);
        cx.read(|cx| {
            let state = view.read(cx);
            assert_eq!(state.area, WorkspaceArea::for_protocol(protocol));
            assert_eq!(state.tab, 0);
            assert!(!state.form_open && !state.form_dirty);
            assert_eq!(state.profiles.len(), 1);
            assert_eq!(state.profiles[0].id, other.id);
        });
        assert_eq!(store.list().unwrap().len(), 1);

        // 功能：确认框期间切到 SQL 草稿，删除成功不得切回表单或服务器；时间：2026-11-11；作者：lq。
        cx.update_window(handle, |_, window, cx| {
            view.update(cx, |state, cx| {
                state.replace_form(other.clone(), window, cx);
                state.delete_profile(window, cx);
                state.activate_workspace(WorkspaceTarget::Sql, cx);
            });
        })
        .unwrap();
        cx.run_until_parked();
        cx.simulate_prompt_answer("删除");
        settle(&mut cx, &view);
        cx.read(|cx| {
            let state = view.read(cx);
            assert_eq!(state.area, WorkspaceArea::Databases);
            assert_eq!(state.tab, 2);
            assert!(state.profiles.is_empty());
            assert!(state.sql_open);
        });
        assert!(store.list().unwrap().is_empty());
        drop(cx);
        drop(store);
        std::fs::remove_dir_all(temporary).unwrap();
    }
}
