//! 功能：验证私钥选择、取消和表单切换竞态，不读取任何私钥内容；时间：2026-11-11；作者：lq。

use super::{ConnectionProfile, Protocol, Workbench};
use gpui_kit::{AppContext, TestAppContext, WindowOptions};

/// 功能：模拟系统选择器返回中文路径，验证不误改后续表单且不发起连接；时间：2026-11-11；作者：lq。
#[test]
fn private_key_picker_preserves_cancel_and_switched_form() {
    let temporary = std::env::temp_dir().join(format!("qds6-key-picker-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&temporary).unwrap();
    let mut cx = TestAppContext::single();
    cx.skip_drawing();
    cx.update(gpui_kit::init);
    let (handle, view) = cx.update(|cx| {
        gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
            cx.new(|cx| {
                Workbench::new_with_store(Some(temporary.join("connections.sqlite3")), window, cx)
            })
        })
        .unwrap()
    });
    let selected = temporary.join("中文 私钥.pem");
    cx.update_window(handle, |_, window, cx| {
        view.update(cx, |state, cx| state.choose_private_key(window, cx))
    })
    .unwrap();
    assert!(cx.did_prompt_for_paths());
    cx.simulate_path_prompt_response(|options| {
        assert!(options.files && !options.directories && !options.multiple);
        Some(vec![selected.clone()])
    });
    cx.run_until_parked();
    cx.read(|cx| {
        let state = view.read(cx);
        assert_eq!(
            state.form.private_key.read(cx).value().as_str(),
            selected.to_str().unwrap()
        );
        assert!(state.form_dirty);
        assert!(!state.services.busy);
        assert!(state.services.servers.is_empty());
    });
    cx.update_window(handle, |_, window, cx| {
        view.update(cx, |state, cx| state.choose_private_key(window, cx))
    })
    .unwrap();
    cx.simulate_path_prompt_response(|_| None);
    cx.run_until_parked();
    cx.read(|cx| {
        assert_eq!(
            view.read(cx).form.private_key.read(cx).value().as_str(),
            selected.to_str().unwrap()
        )
    });
    cx.update_window(handle, |_, window, cx| {
        view.update(cx, |state, cx| {
            state.choose_private_key(window, cx);
            state.replace_form(ConnectionProfile::new(Protocol::Ssh), window, cx);
        })
    })
    .unwrap();
    cx.simulate_path_prompt_response(|_| Some(vec![selected.clone()]));
    cx.run_until_parked();
    cx.read(|cx| assert!(view.read(cx).form.private_key.read(cx).value().is_empty()));
    drop(cx);
    std::fs::remove_dir_all(temporary).unwrap();
}
