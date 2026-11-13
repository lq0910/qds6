//! 功能：真实原生滚动条拖动、右键表/库、复制/重命名和文件向导交互验收；时间：2026-11-12；作者：lq。
use super::*;
use gpui_kit::component::scroll::ScrollbarHandle;
use std::path::Path;

/// 功能：原生事件输入路径和名称，避免直接改写向导内部状态；时间：2026-11-12；作者：lq。
fn input(window: &mut Window, id: &'static str, value: &str, cx: &mut App) {
    window.click(id, cx);
    window.press("cmd-a", cx);
    window.input(value, cx);
}
/// 功能：推动菜单延迟及渲染；时间：2026-11-12；作者：lq。
fn next(cx: &mut HeadlessAppContext, h: AnyWindowHandle, v: &Entity<Workbench>) {
    settle(cx, h, v);
}
/// 功能：纵向和横向拇指拖动后检查滚动偏移和固定表头，并走完真实文件向导；时间：2026-11-12；作者：lq。
pub(super) fn check(
    cx: &mut HeadlessAppContext,
    h: AnyWindowHandle,
    v: &Entity<Workbench>,
    db: &DatabaseSession,
    root: &Path,
) {
    // 功能：同时检查两处目录有实际溢出及可见滚动轨道，不能用“滚轮没报错”代替可滚动；时间：2026-11-12；作者：lq。
    cx.update_window(h, |_, w, cx| {
        let s = &v.read(cx).services;
        let header = w.find("database-catalog-header").bounds();
        let bar = w.find("database-catalog-vertical-scrollbar").bounds();
        let horizontal = w.find("database-catalog-horizontal-scrollbar").bounds();
        assert!(
            bar.size.height > px(200.) && bar.bottom() <= horizontal.top() + px(2.),
            "vertical track {bar:?}, horizontal {horizontal:?}"
        );
        assert!(
            horizontal.bottom() < w.viewport_size().height,
            "catalog horizontal bar outside viewport {horizontal:?}"
        );
        assert!(
            s.catalog_vertical.max_offset().y > px(100.),
            "catalog no overflow {:?}",
            s.catalog_vertical.max_offset()
        );
        s.catalog_vertical.set_offset(point(px(0.), px(0.)));
        s.catalog_horizontal.set_offset(point(px(0.), px(0.)));
        s.object_scroll.set_offset(point(px(0.), px(0.)));
        w.render_frame(cx);
        // 功能：普通纵向滚轮及滚到底部后的继续滚动均不能改变横向偏移；时间：2026-11-13；作者：lq。
        for _ in 0..3 {
            w.scroll("database-catalog", ScrollDelta::Lines(point(0., -100.)), cx);
            w.render_frame(cx);
            assert_eq!(v.read(cx).services.catalog_horizontal.offset().x, px(0.));
        }
        assert!(v.read(cx).services.catalog_vertical.offset().y < px(-500.));
        v.read(cx)
            .services
            .catalog_vertical
            .set_offset(point(px(0.), px(0.)));
        w.render_frame(cx);
        let from = point(bar.center().x, bar.top() + px(10.));
        w.drag(from, point(bar.center().x, bar.bottom() - px(10.)), cx);
        w.render_frame(cx);
        assert!(
            v.read(cx).services.catalog_vertical.offset().y < px(-500.),
            "vertical drag {:?}",
            v.read(cx).services.catalog_vertical.offset()
        );
        assert_eq!(
            w.find("database-catalog-header").bounds().top(),
            header.top(),
            "header moved during vertical scroll"
        );
        let from = point(horizontal.left() + px(15.), horizontal.center().y);
        w.drag(
            from,
            point(horizontal.right() - px(15.), horizontal.center().y),
            cx,
        );
        w.render_frame(cx);
        assert!(
            v.read(cx).services.catalog_horizontal.offset().x < px(-500.),
            "catalog horizontal drag did not move"
        );
        let bar = w.find("database-object-scrollbar").bounds();
        let from = point(bar.center().x, bar.top() + px(10.));
        w.drag(from, point(bar.center().x, bar.bottom() - px(10.)), cx);
        w.render_frame(cx);
        assert!(
            v.read(cx).services.object_scroll.offset().y < px(-500.),
            "object scrollbar drag did not move"
        );
    })
    .unwrap();
    next(cx, h, v);
    cx.capture_screenshot(h)
        .unwrap()
        .save("output/database-grid/catalog-scrollbars-verified.png")
        .unwrap();
    db.query("CREATE TABLE public.aa_transfer_fixture(id INT PRIMARY KEY,name TEXT); INSERT INTO public.aa_transfer_fixture VALUES(1,'向导原始值');").unwrap();
    cx.update_window(h, |_, w, cx| {
        v.update(cx, |this, cx| {
            this.services
                .catalog_vertical
                .set_offset(point(px(0.), px(0.)));
            this.services
                .catalog_horizontal
                .set_offset(point(px(0.), px(0.)));
            this.services
                .object_scroll
                .set_offset(point(px(0.), px(0.)));
            this.load_tables(w, cx);
        })
    })
    .unwrap();
    next(cx, h, v);
    let index = cx.update(|cx| {
        v.read(cx)
            .services
            .tables
            .iter()
            .position(|t| t.name == "aa_transfer_fixture")
            .unwrap()
    });
    cx.update_window(h, |_, w, cx| w.right_click(("catalog-object", index), cx))
        .unwrap();
    next(cx, h, v);
    cx.capture_screenshot(h)
        .unwrap()
        .save("output/database-grid/table-context-menu.png")
        .unwrap();
    cx.update_window(h, |_, w, cx| {
        w.within("popup-menu").hover(5usize, cx);
    })
    .unwrap();
    next(cx, h, v);
    cx.update_window(h, |_, w, cx| {
        click_menu_label(w, "结构和数据 · 生成副本", cx)
    })
    .unwrap();
    next(cx, h, v);
    cx.update_window(h, |_, w, cx| w.click("transfer-next", cx))
        .unwrap();
    next(cx, h, v);
    cx.update_window(h, |_, w, cx| {
        assert!(w.find("transfer-preview").visible());
        w.click("transfer-start", cx);
    })
    .unwrap();
    next(cx, h, v);
    assert!(
        db.tables()
            .unwrap()
            .iter()
            .any(|t| t.name == "aa_transfer_fixture_copy")
    );
    cx.update_window(h, |_, w, cx| {
        assert!(!v.read(cx).error, "{}", v.read(cx).status);
        w.click("transfer-close", cx);
    })
    .unwrap();
    next(cx, h, v);
    // 功能：对象树右键也命中同一表，重命名在预览后直接执行并刷新目录；时间：2026-11-12；作者：lq。
    cx.update_window(h, |_, w, cx| w.right_click(("database-object", index), cx))
        .unwrap();
    next(cx, h, v);
    cx.update_window(h, |_, w, cx| click_menu_label(w, "重命名…", cx))
        .unwrap();
    next(cx, h, v);
    cx.update_window(h, |_, w, cx| {
        input(w, "transfer-name-input", "aa_transfer_renamed", cx);
        w.click("transfer-next", cx);
    })
    .unwrap();
    next(cx, h, v);
    cx.update_window(h, |_, w, cx| w.click("transfer-start", cx))
        .unwrap();
    next(cx, h, v);
    assert!(
        db.tables()
            .unwrap()
            .iter()
            .any(|t| t.name == "aa_transfer_renamed")
    );
    cx.update_window(h, |_, w, cx| w.click("transfer-close", cx))
        .unwrap();
    next(cx, h, v);
    let renamed = cx.update(|cx| {
        v.read(cx)
            .services
            .tables
            .iter()
            .position(|t| t.name == "aa_transfer_renamed")
            .unwrap()
    });
    let file = root.join("native.csv");
    std::fs::write(&file, "id,name\n2,中文导入\n3,\"带,逗号\"\n").unwrap();
    cx.update_window(h, |_, w, cx| w.right_click(("catalog-object", renamed), cx))
        .unwrap();
    next(cx, h, v);
    cx.update_window(h, |_, w, cx| w.within("popup-menu").hover(6usize, cx))
        .unwrap();
    next(cx, h, v);
    cx.update_window(h, |_, w, cx| click_menu_label(w, "CSV / Excel 文件…", cx))
        .unwrap();
    next(cx, h, v);
    cx.update_window(h, |_, w, cx| {
        input(w, "transfer-path-input", file.to_str().unwrap(), cx)
    })
    .unwrap();
    next(cx, h, v);
    cx.update_window(h, |_, w, cx| w.click("transfer-next", cx))
        .unwrap();
    next(cx, h, v);
    cx.capture_screenshot(h)
        .unwrap()
        .save("output/database-grid/import-preview.png")
        .unwrap();
    cx.update_window(h, |_, w, cx| w.click("transfer-start", cx))
        .unwrap();
    next(cx, h, v);
    cx.update_window(h, |_, w, cx| {
        assert!(!v.read(cx).error, "{}", v.read(cx).status);
        w.click("transfer-close", cx);
    })
    .unwrap();
    next(cx, h, v);
    assert_eq!(
        db.query("SELECT count(*)::text FROM public.aa_transfer_renamed")
            .unwrap()
            .sets[0]
            .rows[0][0],
        Cell::Text("3".into())
    );
    // 功能：导出向导真正生成 Excel，运行 SQL 文件真正更新隔离表；时间：2026-11-12；作者：lq。
    cx.update_window(h, |_, w, cx| w.right_click(("catalog-object", renamed), cx))
        .unwrap();
    next(cx, h, v);
    cx.update_window(h, |_, w, cx| w.within("popup-menu").hover(7usize, cx))
        .unwrap();
    next(cx, h, v);
    cx.update_window(h, |_, w, cx| {
        click_menu_label(w, "Excel 文件（.xlsx）…", cx)
    })
    .unwrap();
    next(cx, h, v);
    let exported = root.join("native-export.xlsx");
    cx.update_window(h, |_, w, cx| {
        input(w, "transfer-path-input", exported.to_str().unwrap(), cx);
        w.click("transfer-next", cx);
    })
    .unwrap();
    next(cx, h, v);
    cx.update_window(h, |_, w, cx| w.click("transfer-start", cx))
        .unwrap();
    next(cx, h, v);
    assert_eq!(
        qds6_services::database::read_import(&exported)
            .unwrap()
            .rows
            .len(),
        3
    );
    cx.update_window(h, |_, w, cx| w.click("transfer-close", cx))
        .unwrap();
    next(cx, h, v);
    cx.update_window(h, |_, w, cx| w.right_click(("catalog-object", renamed), cx))
        .unwrap();
    next(cx, h, v);
    cx.update_window(h, |_, w, cx| click_menu_label(w, "运行 SQL 文件…", cx))
        .unwrap();
    next(cx, h, v);
    let script = root.join("native.sql");
    std::fs::write(
        &script,
        "UPDATE public.aa_transfer_renamed SET name='SQL 文件已执行' WHERE id=1;",
    )
    .unwrap();
    cx.update_window(h, |_, w, cx| {
        input(w, "transfer-path-input", script.to_str().unwrap(), cx);
        w.click("transfer-next", cx);
    })
    .unwrap();
    next(cx, h, v);
    cx.update_window(h, |_, w, cx| w.click("transfer-start", cx))
        .unwrap();
    next(cx, h, v);
    assert_eq!(
        db.query("SELECT name FROM public.aa_transfer_renamed WHERE id=1")
            .unwrap()
            .sets[0]
            .rows[0][0],
        Cell::Text("SQL 文件已执行".into())
    );
    cx.update_window(h, |_, w, cx| w.click("transfer-close", cx))
        .unwrap();
    next(cx, h, v);
    // 功能：库对象右键提供整库菜单，进入向导时明确当前目标库；时间：2026-11-12；作者：lq。
    cx.update_window(h, |_, w, cx| w.right_click("database-root-object", cx))
        .unwrap();
    next(cx, h, v);
    cx.capture_screenshot(h)
        .unwrap()
        .save("output/database-grid/database-context-menu.png")
        .unwrap();
    cx.update_window(h, |_, w, cx| {
        click_menu_label(w, "备份整个数据库（SQL）…", cx)
    })
    .unwrap();
    next(cx, h, v);
    cx.capture_screenshot(h)
        .unwrap()
        .save("output/database-grid/database-backup-wizard.png")
        .unwrap();
    cx.update_window(h, |_, w, cx| w.click("transfer-close", cx))
        .unwrap();
    next(cx, h, v);
    // 功能：宽结果集包括 24 列，底部条拖动后最终列出现在视口内；时间：2026-11-12；作者：lq。
    let query = format!(
        "SELECT {}",
        (0..24)
            .map(|i| format!("'字段值{i}'::text AS query_field_{i}"))
            .collect::<Vec<_>>()
            .join(",")
    );
    cx.update_window(h, |_, w, cx| {
        w.click("database-new-query", cx);
        v.update(cx, |this, cx| this.execute_sql(Some(query), w, cx));
    })
    .unwrap();
    next(cx, h, v);
    cx.update_window(h, |_, w, cx| {
        let bar = w.find("query-horizontal-scrollbar").bounds();
        assert!(bar.size.width > px(200.) && bar.bottom() < w.viewport_size().height);
        let from = point(bar.left() + px(10.), bar.center().y);
        w.drag(from, point(bar.right() - px(10.), bar.center().y), cx);
        w.render_frame(cx);
        let grid = v.read(cx).services.grid.as_ref().unwrap().read(cx);
        assert!(
            grid.horizontal_scroll_handle.offset().x < px(-2000.),
            "query drag {:?}",
            grid.horizontal_scroll_handle.offset()
        );
        assert!(
            w.find_by_label("query_field_23").visible(),
            "final query column is not visible"
        );
    })
    .unwrap();
    next(cx, h, v);
    cx.capture_screenshot(h)
        .unwrap()
        .save("output/database-grid/query-scrollbar-final-column.png")
        .unwrap();
    // 功能：小窗口的横向条仍留在结果底部可用；时间：2026-11-12；作者：lq。
    cx.update_window(h, |_, w, cx| {
        w.resize(size(px(960.), px(640.)));
        w.bounds_changed(cx);
    })
    .unwrap();
    next(cx, h, v);
    cx.capture_screenshot(h)
        .unwrap()
        .save("output/database-grid/query-scrollbar-960.png")
        .unwrap();
    cx.update_window(h, |_, w, _| {
        let bar = w.find("query-horizontal-scrollbar").bounds();
        assert!(
            bar.size.width > px(200.) && bar.bottom() < px(640.),
            "narrow query bar {bar:?}; workspace {:?}; editor {:?}",
            w.find("database-table-workspace").bounds(),
            w.find("database-query-editor").bounds()
        );
    })
    .unwrap();
    cx.capture_screenshot(h)
        .unwrap()
        .save("output/database-grid/query-scrollbar-960.png")
        .unwrap();
    cx.update_window(h, |_, w, cx| {
        w.resize(size(px(1440.), px(900.)));
        w.bounds_changed(cx);
    })
    .unwrap();
    next(cx, h, v);
    db.query("DROP TABLE public.aa_transfer_renamed; DROP TABLE public.aa_transfer_fixture_copy")
        .unwrap();
    cx.update_window(h, |_, w, cx| {
        v.update(cx, |this, cx| {
            this.services.query_active = false;
            this.services
                .catalog_horizontal
                .set_offset(point(px(0.), px(0.)));
            this.load_tables(w, cx);
        })
    })
    .unwrap();
    next(cx, h, v);
    println!(
        "PASS native transfer UI: both directory scrollbar thumbs drag, fixed catalog header, wide query final column, table context copy/rename, CSV path/preview/transactional import, database context and backup wizard"
    );
}
