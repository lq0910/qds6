//! 功能：只改变测试进程自身的原生外观，验证自动响应及手动优先级；时间：2026-11-11；作者：lq。

use super::*;
use std::time::Duration;

/// 功能：用隔离数据库和真实 macOS 外观通知验收，不修改系统全局主题；时间：2026-11-11；作者：lq。
pub fn run_appearance_smoke() {
    let temporary = std::env::temp_dir().join(format!("qds6-appearance-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&temporary).unwrap();
    let path = temporary.join("connections.sqlite3");
    ProfileStore::open(&path)
        .unwrap()
        .save_preferences(&AppPreferences {
            theme: ColorTheme::System,
            ..Default::default()
        })
        .unwrap();
    gpui_kit::application().with_assets(AppAssets).run(move |cx| {
        gpui_kit::init(cx);
        let (handle, view) = gpui_kit::open_window(TitleBar::window_options(), cx, |window, cx| {
            cx.new(|cx| Workbench::new_with_store(Some(path.clone()), window, cx))
        })
        .unwrap();
        cx.spawn(async move |cx| {
            for attempt in 0..100 {
                let ready = cx.update(|cx| view.read(cx).settings.ready);
                if ready {
                    break;
                }
                assert!(attempt < 99, "isolated preferences failed to load: {}", cx.update(|cx| view.read(cx).settings.message.clone()));
                cx.background_executor().timer(Duration::from_millis(100)).await;
            }
            for (theme, appearance, expected) in [
                (ColorTheme::System, WindowAppearance::Dark, ThemeMode::Dark),
                (ColorTheme::System, WindowAppearance::Light, ThemeMode::Light),
                (ColorTheme::Light, WindowAppearance::Dark, ThemeMode::Light),
                (ColorTheme::Dark, WindowAppearance::Light, ThemeMode::Dark),
                (ColorTheme::System, WindowAppearance::Dark, ThemeMode::Dark),
            ] {
                cx.update_window(handle, |_, window, cx| {
                    view.update(cx, |state, cx| {
                        let mut preferences = state.settings.preferences.clone();
                        preferences.theme = theme;
                        state.apply_preferences(preferences, window, cx);
                        // 功能：重建表单后监听仍须有效，防止表单订阅清理误删外观订阅；时间：2026-11-11；作者：lq。
                        state.replace_form(ConnectionProfile::new(Protocol::Ssh), window, cx);
                    });
                    cx.set_window_appearance(Some(appearance));
                })
                .unwrap();
                // 功能：等待原生通知进入 GPUI 事件循环后再断言，最多等待两秒；时间：2026-11-11；作者：lq。
                for attempt in 0..20 {
                    cx.background_executor().timer(Duration::from_millis(100)).await;
                    let settled = cx.update_window(handle, |_, window, cx| {
                        window.appearance() == appearance && cx.theme().mode == expected
                    }).unwrap();
                    if settled {
                        break;
                    }
                    assert!(attempt < 19, "native appearance failed: {theme:?} / {appearance:?}");
                }
                cx.update(|cx| assert_eq!(view.read(cx).settings.preferences.theme, theme));
                println!("PASS native appearance: preference={theme:?}, appearance={appearance:?}, theme={expected:?}");
            }
            cx.update(|cx| {
                cx.set_window_appearance(None);
                cx.quit();
            });
        }).detach();
        cx.activate(true);
    });
    std::fs::remove_dir_all(temporary).unwrap();
}
