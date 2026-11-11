//! 功能：原生鼠标、键盘与 Metal 帧验证数据库工作区，只用隔离测试库；时间：2026-11-11；作者：lq。
use super::*;
use gpui_kit::test::TestWindowExt;
use std::time::{Duration, Instant};

/// 功能：等待真实后台任务并推动原生帧，不以虚拟时钟替代网络任务；时间：2026-11-11；作者：lq。
fn settle(cx: &mut HeadlessAppContext, handle: AnyWindowHandle, view: &Entity<Workbench>) {
    let start = Instant::now();
    loop {
        cx.run_until_parked();
        cx.update_window(handle, |_, window, cx| {
            window.simulate_next_frame(cx);
            window.render_frame(cx);
        })
        .unwrap();
        if cx.update(|cx| {
            !view.read(cx).services.busy && !view.read(cx).loading && view.read(cx).settings.ready
        }) {
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(30),
            "database UI task timed out"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    for _ in 0..4 {
        cx.advance_clock(Duration::from_millis(100));
        cx.run_until_parked();
        cx.update_window(handle, |_, w, cx| {
            w.simulate_next_frame(cx);
            w.render_frame(cx);
        })
        .unwrap();
    }
}

/// 功能：实际连接后自动对象、双击表、独立草稿、行内保存、布局拖动和关闭确认验收；时间：2026-11-11；作者：lq。
pub fn run_database_grid_smoke() {
    let mut profile = ConnectionProfile::new(Protocol::PostgreSql);
    profile.name = "隔离 PostgreSQL · 表操作验收".into();
    profile.host = "127.0.0.1".into();
    profile.port = 25433;
    profile.username = "qds6".into();
    profile.database = "qds6".into();
    profile.db.tls = TlsMode::Disabled;
    let db = DatabaseSession::connect(&profile, "qds6-fixture-only", None).unwrap();
    db.query("DROP TABLE IF EXISTS public.ui_grid_a; DROP TABLE IF EXISTS public.ui_grid_b; CREATE TABLE public.ui_grid_a(id INT PRIMARY KEY, name TEXT, quantity INT DEFAULT 1); INSERT INTO public.ui_grid_a VALUES (1,'北京',10),(2,'上海',20); CREATE TABLE public.ui_grid_b(id INT PRIMARY KEY, name TEXT); INSERT INTO public.ui_grid_b VALUES(1,'第二张表')").unwrap();
    let root = std::env::temp_dir().join(format!("qds6-grid-ui-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all("output/database-grid").unwrap();
    let mut cx = HeadlessAppContext::with_platform(
        gpui_kit::platform::current_platform(true).text_system(),
        Arc::new(AppAssets),
        gpui_kit::platform::current_headless_renderer,
    );
    cx.update(gpui_kit::init);
    // 功能：Headless 使用可点击的同一 PromptResponse 协议，验收取消和确认分支；时间：2026-11-11；作者：lq。
    cx.update(|cx| {
        cx.set_prompt_builder(|_, _, _, _, handle, window, cx| {
            let prompt = cx.new(|cx| SmokePrompt(cx.focus_handle()));
            handle.with_view(prompt, window, cx)
        })
    });
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
        view.update(cx, |this, cx| {
            this.replace_form(profile, window, cx);
            this.form.secret.update(cx, |input, cx| {
                input.set_value("qds6-fixture-only", window, cx)
            });
            this.connect_service(window, cx);
        })
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    let (a, b) = cx.update(|cx| {
        let s = &view.read(cx).services;
        assert!(!view.read(cx).error, "{}", view.read(cx).status);
        assert!(s.database.is_some());
        assert!(s.table_tabs.is_empty());
        (
            s.tables
                .iter()
                .position(|t| t.name == "ui_grid_a")
                .expect("auto-loaded table a"),
            s.tables
                .iter()
                .position(|t| t.name == "ui_grid_b")
                .expect("auto-loaded table b"),
        )
    });
    cx.update_window(handle, |_, window, cx| {
        assert!(
            window.try_find("run-sql").is_none(),
            "SQL editor must be opt-in"
        );
        let before = window.find("database-object-pane").bounds();
        let from = point(before.right(), before.center().y);
        window.drag(from, from + point(px(70.), px(0.)), cx);
        let after = window.find("database-object-pane").bounds();
        assert!(after.size.width > before.size.width + px(30.));
        window.double_click(("database-object", a), cx);
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.update_window(handle, |_, window, cx| {
        assert_eq!(view.read(cx).services.table_tabs.len(), 1);
        window.double_click("grid-cell-0-1", cx);
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.update_window(handle, |_, window, cx| {
        window.press("cmd-a", cx);
        window.input("北京 · 网格修改", cx);
        window.press("enter", cx);
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.update(|cx| {
        let d = view.read(cx).services.table_tabs[0]
            .grid
            .read(cx)
            .delegate();
        assert_eq!(d.changes().len(), 1);
        assert_eq!(d.value(0, 1), Some(Cell::Text("北京 · 网格修改".into())));
    });
    cx.update_window(handle, |_, window, cx| {
        window.double_click(("database-object", b), cx)
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.update_window(handle, |_, window, cx| {
        assert_eq!(view.read(cx).services.table_tabs.len(), 2);
        window.within("database-table-tabs").click(1usize, cx);
        window.click("grid-details", cx);
        window.click("grid-fields", cx);
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    // 功能：关闭脏标签必须可取消，未保存输入继续保留；时间：2026-11-11；作者：lq。
    cx.update_window(handle, |_, window, cx| {
        window.click(("close-table", 0usize), cx)
    })
    .unwrap();
    cx.update_window(handle, |_, window, cx| {
        window.click("grid-prompt-cancel", cx)
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.update(|cx| {
        assert_eq!(
            view.read(cx).services.table_tabs[0]
                .grid
                .read(cx)
                .delegate()
                .changes()
                .len(),
            1
        )
    });
    // 功能：字段隐藏后表头与单元格保持映射，恢复字段不会丢失草稿；时间：2026-11-11；作者：lq。
    cx.update_window(handle, |_, window, cx| {
        window.click(("visible-field", 2usize), cx)
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.update(|cx| {
        assert_eq!(
            view.read(cx).services.table_tabs[0]
                .grid
                .read(cx)
                .delegate()
                .columns_count(cx),
            2
        )
    });
    cx.update_window(handle, |_, window, cx| {
        window.click(("visible-field", 2usize), cx)
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.capture_screenshot(handle)
        .unwrap()
        .save("output/database-grid/table-draft-1440.png")
        .unwrap();
    cx.update_window(handle, |_, window, cx| window.click("grid-save", cx))
        .unwrap();
    settle(&mut cx, handle, &view);
    assert_eq!(
        db.query("SELECT name FROM public.ui_grid_a WHERE id=1")
            .unwrap()
            .sets[0]
            .rows[0][0],
        Cell::Text("北京 · 网格修改".into())
    );
    cx.update_window(handle, |_, window, cx| {
        // 功能：新增记录直接在网格输入，未输入 quantity 使用服务端默认值；时间：2026-11-11；作者：lq。
        window.click("grid-add", cx);
        window.double_click("grid-cell-2-0", cx);
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.update_window(handle, |_, window, cx| {
        window.press("cmd-a", cx);
        window.input("1", cx);
        window.press("enter", cx);
        window.click("grid-save", cx);
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.update(|cx| {
        let data = view.read(cx).services.table_tabs[0]
            .grid
            .read(cx)
            .delegate();
        assert!(view.read(cx).error);
        assert!(!data.locked);
        assert_eq!(data.changes().len(), 1);
    });
    cx.update_window(handle, |_, window, cx| {
        window.double_click("grid-cell-2-0", cx)
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.update_window(handle, |_, window, cx| {
        window.press("cmd-a", cx);
        window.input("3", cx);
        window.press("enter", cx);
        window.double_click("grid-cell-2-1", cx);
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.update_window(handle, |_, window, cx| {
        window.input("新增行", cx);
        window.press("enter", cx);
        window.click("grid-save", cx);
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    assert_eq!(
        db.query("SELECT quantity FROM public.ui_grid_a WHERE id=3")
            .unwrap()
            .sets[0]
            .rows[0][0],
        Cell::Text("1".into())
    );
    cx.update_window(handle, |_, window, cx| {
        window.click("grid-cell-2-1", cx);
        window.click("grid-delete", cx);
        window.click("grid-save", cx);
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    assert_eq!(
        db.query("SELECT count(*) FROM public.ui_grid_a WHERE id=3")
            .unwrap()
            .sets[0]
            .rows[0][0],
        Cell::Text("0".into())
    );
    cx.update_window(handle, |_, window, cx| {
        window.click("database-new-query", cx);
        assert!(window.find("run-sql").visible());
        view.read(cx).sql.clone().update(cx, |input, cx| {
            input.set_value("SELECT '查询页' AS text, 42 AS value", window, cx)
        });
        window.click("run-sql", cx);
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.update_window(handle, |_, window, cx| {
        let before = window.find("database-query-editor").bounds();
        let from = point(before.center().x, before.bottom());
        window.drag(from, from + point(px(0.), px(-50.)), cx);
        assert!(
            window.find("database-query-editor").bounds().size.height
                < before.size.height - px(20.)
        );
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.capture_screenshot(handle)
        .unwrap()
        .save("output/database-grid/query-1440.png")
        .unwrap();
    // 功能：生成表操作 SQL 不能静默覆盖查询草稿，也不能自动执行；时间：2026-11-11；作者：lq。
    cx.update_window(handle, |_, window, cx| {
        view.update(cx, |this, cx| {
            this.sql_dirty = true;
            this.show_generated_sql("SELECT 99 AS generated_preview".into(), window, cx);
        })
    })
    .unwrap();
    cx.update_window(handle, |_, window, cx| {
        window.click("grid-prompt-cancel", cx)
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.update(|cx| assert!(view.read(cx).sql.read(cx).value().contains("查询页")));
    cx.update_window(handle, |_, window, cx| {
        view.update(cx, |this, cx| {
            this.show_generated_sql("SELECT 99 AS generated_preview".into(), window, cx)
        })
    })
    .unwrap();
    cx.update_window(handle, |_, window, cx| {
        window.click("grid-prompt-confirm", cx)
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.update(|cx| {
        assert!(
            view.read(cx)
                .sql
                .read(cx)
                .value()
                .contains("generated_preview")
        );
        assert_eq!(
            view.read(cx).services.results.as_ref().unwrap().sets[0].rows[0][0],
            Cell::Text("查询页".into())
        );
    });
    cx.update_window(handle, |_, window, cx| {
        window.resize(size(px(960.), px(640.)));
        window.bounds_changed(cx);
        window.within("database-table-tabs").click(1usize, cx);
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.update_window(handle, |_, window, _| {
        let bottom = window.find("grid-bottom-toolbar").bounds();
        assert!(bottom.bottom() <= window.find("workspace-status").bounds().top());
    })
    .unwrap();
    cx.capture_screenshot(handle)
        .unwrap()
        .save("output/database-grid/table-960.png")
        .unwrap();
    cx.update_window(handle, |_, window, cx| {
        window.click(("close-table", 1usize), cx);
        assert_eq!(view.read(cx).services.table_tabs.len(), 1);
        window.click("workspace-close-database", cx);
    })
    .unwrap();
    cx.update_window(handle, |_, window, cx| {
        window.click("grid-prompt-cancel", cx)
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.update(|cx| assert!(view.read(cx).services.database.is_some()));
    cx.update_window(handle, |_, window, cx| {
        window.click("workspace-close-database", cx)
    })
    .unwrap();
    cx.update_window(handle, |_, window, cx| {
        window.click("grid-prompt-confirm", cx)
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.update(|cx| {
        assert!(view.read(cx).services.database.is_none());
        assert!(view.read(cx).services.table_tabs.is_empty());
        assert!(view.read(cx).services.tables.is_empty());
    });
    cx.update_window(handle, |_, window, _| window.remove_window())
        .unwrap();
    drop(cx);
    db.query("DROP TABLE public.ui_grid_a; DROP TABLE public.ui_grid_b")
        .unwrap();
    std::fs::remove_dir_all(root).unwrap();
    println!(
        "PASS native database UI: automatic objects, opt-in SQL, object divider drag, double-click tables/cells, multiple table tabs and drafts, real insert/update/delete/default/save, query divider, 1440/960 layouts, close tab and disconnect cancel/confirm"
    );
}

/// 功能：仅验收环境的原生确认按钮，向 GPUI 发送标准响应；时间：2026-11-11；作者：lq。
struct SmokePrompt(FocusHandle);
impl EventEmitter<PromptResponse> for SmokePrompt {}
impl Focusable for SmokePrompt {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.0.clone()
    }
}
impl Render for SmokePrompt {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .size_full()
            .items_center()
            .justify_center()
            .track_focus(&self.0)
            .child(
                Button::new("grid-prompt-cancel")
                    .label("取消")
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(PromptResponse(0)))),
            )
            .child(
                Button::new("grid-prompt-confirm")
                    .label("确认")
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(PromptResponse(1)))),
            )
    }
}
