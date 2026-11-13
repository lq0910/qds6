//! 功能：原生鼠标、键盘与 Metal 帧验证数据库工作区，只用隔离测试库；时间：2026-11-11；作者：lq。
use super::*;
use gpui_kit::InputEvent as _;
use gpui_kit::test::{TestQueryExt, TestWindowExt};
use std::time::{Duration, Instant};
// 功能：目录、查询滚动和文件向导专项验收；时间：2026-11-12；作者：lq。
mod transfer;

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
    db.query("DROP TABLE IF EXISTS public.aa_transfer_fixture; DROP TABLE IF EXISTS public.aa_transfer_fixture_copy; DROP TABLE IF EXISTS public.aa_transfer_renamed; DROP TABLE IF EXISTS public.ui_grid_a; DROP TABLE IF EXISTS public.ui_grid_b; CREATE TABLE public.ui_grid_a(id INT PRIMARY KEY, name TEXT, quantity INT DEFAULT 1); INSERT INTO public.ui_grid_a VALUES (1,'北京',10),(2,'上海',20); CREATE TABLE public.ui_grid_b(id INT PRIMARY KEY, name TEXT); INSERT INTO public.ui_grid_b VALUES(1,'第二张表')").unwrap();
    // 功能：宽表、中文备注与长对象列表复现用户截图问题；时间：2026-11-12；作者：lq。
    db.query("COMMENT ON COLUMN public.ui_grid_a.name IS '客户名称备注'; COMMENT ON COLUMN public.ui_grid_b.name IS '宽表名称';").unwrap();
    for i in 0..16 {
        db.query(&format!(
            "ALTER TABLE public.ui_grid_b ADD COLUMN field_{i} TEXT DEFAULT '字段{i}';"
        ))
        .unwrap();
    }
    for i in 0..50 {
        db.query(&format!(
            "CREATE TABLE IF NOT EXISTS public.zz_grid_catalog_{i:02}(id INT);"
        ))
        .unwrap();
    }
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
    // 功能：回车筛选、可见备注、宽表触控板滚动与底部滚动条拖动验收；时间：2026-11-12；作者：lq。
    cx.update_window(handle, |_, window, cx| {
        window.resize(size(px(1440.), px(900.)));
        window.bounds_changed(cx);
        window.click("toggle-connection-sidebar", cx);
        window.click("grid-details", cx);
        window.click("grid-fields", cx);
        view.update(cx, |this, cx| {
            this.services.table_tabs[0].filters[0].column = 1;
            this.services.table_tabs[0].filters[0]
                .value
                .update(cx, |input, cx| {
                    input.set_value("上海", window, cx);
                    input.focus(window, cx);
                });
        });
        window.press("enter", cx);
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.update(|cx| {
        let tab = &view.read(cx).services.table_tabs[0];
        assert_eq!(tab.grid.read(cx).delegate().rows.len(), 1);
        assert_eq!(
            tab.grid.read(cx).delegate().page.columns[1].comment,
            "客户名称备注"
        );
    });
    cx.update_window(handle, |_, window, cx| {
        window.click("grid-clear-filter", cx);
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.update_window(handle, |_, window, cx| {
        window.within("database-table-tabs").click(2usize, cx);
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.update_window(handle, |_, window, cx| {
        let before = view.read(cx).services.table_tabs[1]
            .grid
            .read(cx)
            .horizontal_scroll_handle
            .offset()
            .x;
        window.scroll(
            "grid-viewport",
            ScrollDelta::Pixels(point(px(-1200.), px(0.))),
            cx,
        );
        window.render_frame(cx);
        let after = view.read(cx).services.table_tabs[1]
            .grid
            .read(cx)
            .horizontal_scroll_handle
            .offset()
            .x;
        assert!(
            after < before,
            "horizontal wheel did not move: {before:?}/{after:?}"
        );
        let bar = window.find("grid-horizontal-scrollbar").bounds();
        let viewport = window.find("grid-viewport").bounds();
        assert!(bar.top() >= viewport.bottom() - px(1.));
        assert!(bar.bottom() <= window.find("grid-bottom-toolbar").bounds().top());
        window.scroll(
            "grid-viewport",
            ScrollDelta::Pixels(point(px(10000.), px(0.))),
            cx,
        );
        window.render_frame(cx);
        let from = point(bar.left() + px(20.), bar.center().y);
        window.drag(from, point(bar.right() - px(15.), bar.center().y), cx);
        window.render_frame(cx);
        assert!(
            view.read(cx).services.table_tabs[1]
                .grid
                .read(cx)
                .horizontal_scroll_handle
                .offset()
                .x
                < px(-1000.),
            "scrollbar drag offset: {:?}; bar {bar:?}",
            view.read(cx).services.table_tabs[1]
                .grid
                .read(cx)
                .horizontal_scroll_handle
                .offset()
        );
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.capture_screenshot(handle)
        .unwrap()
        .save("output/database-grid/wide-table-scroll-1440.png")
        .unwrap();
    // 功能：空宽表仍可横向查看最后一列，不依赖已加载数据行；时间：2026-11-12；作者：lq。
    db.query("DELETE FROM public.ui_grid_b").unwrap();
    cx.update_window(handle, |_, window, cx| {
        window.click("grid-refresh", cx);
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.update_window(handle, |_, window, cx| {
        window.scroll(
            "grid-viewport",
            ScrollDelta::Pixels(point(px(-10000.), px(0.))),
            cx,
        );
        window.render_frame(cx);
        assert!(
            view.read(cx).services.table_tabs[1]
                .grid
                .read(cx)
                .delegate()
                .rows
                .is_empty()
        );
        assert!(
            view.read(cx).services.table_tabs[1]
                .grid
                .read(cx)
                .horizontal_scroll_handle
                .offset()
                .x
                < px(-1000.)
        );
    })
    .unwrap();
    cx.capture_screenshot(handle)
        .unwrap()
        .save("output/database-grid/empty-wide-table.png")
        .unwrap();
    cx.update_window(handle, |_, window, cx| {
        window.click("grid-toggle-filter", cx);
        window.click("grid-add-filter", cx);
        window.click("grid-add-sort", cx);
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.capture_screenshot(handle)
        .unwrap()
        .save("output/database-grid/filter-builder-1440.png")
        .unwrap();
    // 功能：验证直接保存的数据库失败保留草稿、重试成功及不切换查询页；时间：2026-11-13；作者：lq。
    cx.update_window(handle, |_, window, cx| {
        window.click("grid-design", cx);
        window.click(("design-column", 1usize), cx);
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.update_window(handle, |_, window, cx| {
        window.scroll(
            "design-horizontal-viewport",
            ScrollDelta::Pixels(point(px(-2000.), px(0.))),
            cx,
        );
        window.render_frame(cx);
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.capture_screenshot(handle)
        .unwrap()
        .save("output/database-grid/inline-design-before-edit.png")
        .unwrap();
    cx.update_window(handle, |_, window, cx| {
        window.click("design-input-1-5", cx);
        window.press("cmd-a", cx);
        window.input("修改后的字段备注", cx);
        window.click("design-input-1-4", cx);
        window.press("cmd-a", cx);
        window.input("qds6_missing_function()", cx);
        window.click("design-save", cx);
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.update(|cx| {
        let this = view.read(cx);
        assert!(this.error && this.services.table_tabs[1].has_changes(cx));
        assert!(!this.services.query_active);
    });
    let target = Table {
        schema: "public".into(),
        name: "ui_grid_b".into(),
        kind: "BASE TABLE".into(),
    };
    assert_eq!(db.grid_columns(&target).unwrap()[1].comment, "宽表名称");
    cx.update_window(handle, |_, window, cx| {
        window.click("design-input-1-4", cx);
        window.press("cmd-a", cx);
        window.press("backspace", cx);
        window.click("design-save", cx);
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.update(|cx| {
        let this = view.read(cx);
        assert!(!this.error, "{}", this.status);
        assert!(!this.services.query_active);
        assert!(this.services.table_tabs[1].designing);
        assert!(!this.services.table_tabs[1].has_changes(cx));
    });
    assert_eq!(
        db.grid_columns(&target).unwrap()[1].comment,
        "修改后的字段备注"
    );
    // 功能：真实事件验证插入、类型参数、非空和批量结构直接保存；时间：2026-11-13；作者：lq。
    cx.update_window(handle, |_, window, cx| {
        window.click("design-new-column", cx);
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.update_window(handle, |_, window, cx| {
        assert!(window.find("design-input-18-0").visible());
        window.input("amount", cx);
        window.click(("design-type-menu", 18usize), cx);
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.capture_screenshot(handle)
        .unwrap()
        .save("output/database-grid/design-type-presets.png")
        .unwrap();
    cx.update_window(handle, |_, window, cx| {
        click_menu_label(window, "numeric", cx);
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.update_window(handle, |_, window, cx| {
        for (field, value) in [(2, "12"), (3, "2")] {
            window.click(format!("design-input-18-{field}"), cx);
            window.press("cmd-a", cx);
            window.input(value, cx);
        }
        window.click(("design-not-null", 18usize), cx);
        window.scroll(
            "design-horizontal-viewport",
            ScrollDelta::Pixels(point(px(-2000.), px(0.))),
            cx,
        );
        window.render_frame(cx);
        window.click("design-input-18-4", cx);
        window.input("0", cx);
        window.click("design-input-18-5", cx);
        window.input("金额", cx);
        window.click(("design-delete-row", 17usize), cx);
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.update_window(handle, |_, window, cx| {
        view.update(cx, |this, _| this.sql_dirty = false);
        window.click("design-save", cx);
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.update(|cx| {
        let this = view.read(cx);
        assert!(!this.error, "{}", this.status);
        assert!(!this.services.query_active);
        assert!(!this.services.table_tabs[1].has_changes(cx));
    });
    let columns = db.grid_columns(&target).unwrap();
    assert!(!columns.iter().any(|c| c.name == "field_15"));
    let amount = columns.iter().find(|c| c.name == "amount").unwrap();
    assert!(amount.data_type.contains("12,2"));
    assert!(!amount.nullable);
    assert_eq!(amount.comment, "金额");
    cx.update_window(handle, |_, window, cx| {
        // 功能：保存刷新重置滚动位置后，先滚到操作列再验证关闭保护；时间：2026-11-13；作者：lq。
        window.scroll(
            "design-column-list",
            ScrollDelta::Pixels(point(px(0.), px(-2000.))),
            cx,
        );
        window.scroll(
            "design-horizontal-viewport",
            ScrollDelta::Pixels(point(px(-2000.), px(0.))),
            cx,
        );
        window.render_frame(cx);
        window.click(("design-delete-row", 17usize), cx);
        window.click(("close-table", 1usize), cx);
    })
    .unwrap();
    cx.update_window(handle, |_, window, cx| {
        window.click("grid-prompt-cancel", cx)
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.update_window(handle, |_, window, cx| {
        assert_eq!(view.read(cx).services.table_tabs.len(), 2);
        assert!(view.read(cx).services.table_tabs[1].has_changes(cx));
        window.click(("design-delete-row", 17usize), cx);
        window.scroll(
            "design-horizontal-viewport",
            ScrollDelta::Pixels(point(px(2000.), px(0.))),
            cx,
        );
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.capture_screenshot(handle)
        .unwrap()
        .save("output/database-grid/table-design-1440.png")
        .unwrap();
    // 功能：小窗口仍保留字段横滚、添加入口和底部状态区；时间：2026-11-12；作者：lq。
    cx.update_window(handle, |_, window, cx| {
        window.resize(size(px(960.), px(640.)));
        window.bounds_changed(cx);
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.update_window(handle, |_, window, cx| {
        assert!(window.find("design-new-column").visible());
        assert!(
            window.find("design-horizontal-scrollbar").bounds().bottom()
                <= window.find("workspace-status").bounds().top()
        );
        window.scroll(
            "design-horizontal-viewport",
            ScrollDelta::Pixels(point(px(-2000.), px(0.))),
            cx,
        );
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.capture_screenshot(handle)
        .unwrap()
        .save("output/database-grid/table-design-960.png")
        .unwrap();
    cx.update_window(handle, |_, window, cx| {
        window.resize(size(px(1440.), px(900.)));
        window.bounds_changed(cx);
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.update_window(handle, |_, window, cx| {
        window.click("design-indexes", cx);
        window.click("design-fields", cx);
        window.click("design-discard", cx);
        window.click("design-back-data", cx);
    })
    .unwrap();
    // 功能：真实右键菜单四种关闭操作，含批量取消时草稿保留；时间：2026-11-12；作者：lq。
    cx.update_window(handle, |_, window, cx| {
        window.right_click(("table-tab-label", 0usize), cx);
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.capture_screenshot(handle)
        .unwrap()
        .save("output/database-grid/tab-context-menu.png")
        .unwrap();
    cx.update_window(handle, |_, window, cx| {
        click_menu_label(window, "关闭右侧选项卡", cx);
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.update(|cx| assert_eq!(view.read(cx).services.table_tabs.len(), 1));
    cx.update_window(handle, |_, window, cx| {
        window.double_click(("database-object", b), cx);
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.update_window(handle, |_, window, cx| {
        view.update(cx, |this, cx| {
            this.services.table_tabs[0].grid.update(cx, |s, cx| {
                s.delegate_mut()
                    .set_value(0, 1, Some(Cell::Text("未保存".into())));
                cx.notify();
            });
        });
        window.right_click(("table-tab-label", 1usize), cx);
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.update_window(handle, |_, window, cx| {
        click_menu_label(window, "关闭其他选项卡", cx);
    })
    .unwrap();
    cx.update_window(handle, |_, window, cx| {
        window.click("grid-prompt-cancel", cx);
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.update(|cx| assert_eq!(view.read(cx).services.table_tabs.len(), 2));
    cx.update_window(handle, |_, window, cx| {
        window.right_click(("table-tab-label", 1usize), cx);
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.update_window(handle, |_, window, cx| {
        click_menu_label(window, "关闭其他选项卡", cx);
    })
    .unwrap();
    cx.update_window(handle, |_, window, cx| {
        window.click("grid-prompt-confirm", cx);
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.update(|cx| assert_eq!(view.read(cx).services.table_tabs.len(), 1));
    cx.update_window(handle, |_, window, cx| {
        window.right_click(("table-tab-label", 0usize), cx);
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.update_window(handle, |_, window, cx| {
        click_menu_label(window, "全部关闭", cx);
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.update_window(handle, |_, window, cx| {
        assert!(view.read(cx).services.table_tabs.is_empty());
        window.within("database-table-tabs").click(0usize, cx);
        window.scroll(
            "database-catalog",
            ScrollDelta::Pixels(point(px(0.), px(-900.))),
            cx,
        );
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.capture_screenshot(handle)
        .unwrap()
        .save("output/database-grid/catalog-scrolled.png")
        .unwrap();
    transfer::check(&mut cx, handle, &view, &db, &root);
    cx.update_window(handle, |_, window, cx| {
        window.click("database-new-query", cx);
        assert!(window.find("run-sql").visible());
        view.update(cx, |this, cx| {
            this.services.selected_table = Some(a);
            this.table_operation(false, window, cx);
        });
    })
    .unwrap();
    settle(&mut cx, handle, &view);
    cx.update_window(handle, |_, window, cx| {
        window.double_click(("database-object", b), cx);
    })
    .unwrap();
    settle(&mut cx, handle, &view);
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
    for i in 0..50 {
        db.query(&format!("DROP TABLE public.zz_grid_catalog_{i:02};"))
            .unwrap();
    }
    db.query("DROP TABLE public.ui_grid_a; DROP TABLE public.ui_grid_b")
        .unwrap();
    std::fs::remove_dir_all(root).unwrap();
    println!(
        "PASS native database UI: automatic objects and clipped catalog, wide/empty table horizontal wheel and scrollbar drag to final column, field comments, Enter filter/reset, filter builder/sort, inline field edit/add/type dropdown/length-scale/NULL/direct structure save, failure retry and structure dirty-close protection, context close-right/others/all with dirty cancel/confirm, grid insert/update/delete/default/save, 1440/960 layouts and disconnect protection"
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

/// 功能：按原生无障碍菜单标签命中，覆盖真实右键菜单而非直接调用业务函数；时间：2026-11-12；作者：lq。
fn click_menu_label(window: &mut Window, label: &str, cx: &mut App) {
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
