//! 功能：带固定表头、横纵常显滚动条的数据库目录与完整属性详情；时间：2026-11-12；作者：lq。
use super::*;

/// 功能：属性列固定宽度，窄窗口横向滚动查看而不挤压字段；时间：2026-11-12；作者：lq。
const COLUMNS: [(&str, f32); 10] = [
    ("名称", 300.),
    ("模式", 150.),
    ("OID", 100.),
    ("所有者", 150.),
    ("类型 / 引擎", 140.),
    ("行数（估计）", 120.),
    ("大小（字节）", 140.),
    ("主键", 180.),
    ("备注", 360.),
    ("对象类型", 120.),
];

impl Workbench {
    /// 功能：目录和对象树共用筛选、选择、菜单；滚动内容不越过工具栏和标签；时间：2026-11-12；作者：lq。
    pub(super) fn database_catalog(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let selected = self
            .services
            .selected_table
            .and_then(|i| self.services.tables.get(i));
        let header = h_flex()
            .id("database-catalog-header")
            .test_support()
            .h(px(30.))
            .flex_shrink_0()
            .border_b_1()
            .border_color(cx.theme().border)
            .children(COLUMNS.iter().map(|(label, width)| {
                div()
                    .w(px(*width))
                    .flex_shrink_0()
                    .px_2()
                    .text_xs()
                    .font_semibold()
                    .child(*label)
            }));
        let rows = self
            .services
            .tables
            .iter()
            .enumerate()
            .filter(|(_, t)| self.object_matches(t, cx))
            .map(|(index, table)| {
                let props = self
                    .services
                    .table_properties
                    .get(&(table.schema.clone(), table.name.clone()))
                    .cloned()
                    .unwrap_or_default();
                let values = [
                    table.name.clone(),
                    table.schema.clone(),
                    props.oid,
                    props.owner,
                    props.engine,
                    props.rows,
                    props.bytes,
                    props.primary_key,
                    props.comment,
                    if table.kind == "BASE TABLE" {
                        "表".into()
                    } else {
                        "视图".into()
                    },
                ];
                let menu_view = cx.entity();
                let menu_table = table.clone();
                h_flex()
                    .id(("catalog-object", index))
                    .test_support()
                    .h(px(32.))
                    .flex_shrink_0()
                    .items_center()
                    .cursor_pointer()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .when(self.services.selected_table == Some(index), |v| {
                        v.bg(cx.theme().secondary)
                    })
                    .hover(|v| v.bg(cx.theme().secondary))
                    .children(values.into_iter().zip(COLUMNS).map(|(value, (_, width))| {
                        div()
                            .w(px(width))
                            .flex_shrink_0()
                            .px_2()
                            .text_xs()
                            .text_ellipsis()
                            .child(value)
                    }))
                    .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                        this.services.selected_table = Some(index);
                        if event.click_count() == 2 {
                            this.table_operation(false, window, cx);
                        }
                        cx.notify();
                    }))
                    .context_menu(move |menu, window, cx| {
                        Self::table_context_menu(
                            menu,
                            menu_view.clone(),
                            menu_table.clone(),
                            window,
                            cx,
                        )
                    })
            });
        let catalog = div()
            .relative()
            .h_full()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .overflow_hidden()
            .child(
                div()
                    .id("database-catalog-horizontal")
                    .size_full()
                    .overflow_x_scroll()
                    // 功能：禁止纵向滚轮转为横移，横向查看仍使用底部滚动条；时间：2026-11-13；作者：lq。
                    .restrict_scroll_to_axis()
                    .track_scroll(&self.services.catalog_horizontal)
                    .pb(px(14.))
                    .child(
                        v_flex()
                            .w(px(COLUMNS.iter().map(|(_, w)| *w).sum()))
                            .h_full()
                            .min_h_0()
                            .overflow_hidden()
                            .child(header)
                            .child(
                                v_flex()
                                    .id("database-catalog")
                                    .test_support()
                                    .flex_1()
                                    .min_h_0()
                                    .overflow_y_scroll()
                                    .restrict_scroll_to_axis()
                                    .track_scroll(&self.services.catalog_vertical)
                                    .children(rows),
                            ),
                    ),
            )
            .child(
                div()
                    .id("database-catalog-vertical-scrollbar")
                    .test_support()
                    .absolute()
                    .top(px(30.))
                    .bottom(px(14.))
                    .right_0()
                    .w(px(12.))
                    .child(
                        Scrollbar::vertical(&self.services.catalog_vertical)
                            .viewport_from_layout()
                            .mode(ScrollbarMode::Always),
                    ),
            )
            .child(
                div()
                    .id("database-catalog-horizontal-scrollbar")
                    .test_support()
                    .absolute()
                    .left_0()
                    .right(px(12.))
                    .bottom_0()
                    .h(px(12.))
                    .child(
                        Scrollbar::horizontal(&self.services.catalog_horizontal)
                            .viewport_from_layout()
                            .mode(ScrollbarMode::Always),
                    ),
            );
        v_flex()
            .size_full()
            .min_w_0()
            .min_h_0()
            .overflow_hidden()
            .child(
                h_flex()
                    .flex_shrink_0()
                    .p_2()
                    .gap_2()
                    .items_center()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        Button::new("browse-table")
                            .small()
                            .ghost()
                            .icon(UiIcon::FolderOpen)
                            .label("打开表")
                            .disabled(self.services.busy || selected.is_none())
                            .on_click(
                                cx.listener(|this, _, w, cx| this.table_operation(false, w, cx)),
                            ),
                    )
                    .child(
                        Button::new("table-structure")
                            .small()
                            .ghost()
                            .icon(UiIcon::Settings)
                            .label("设计表")
                            .disabled(self.services.busy || selected.is_none())
                            .on_click(
                                cx.listener(|this, _, w, cx| this.table_operation(true, w, cx)),
                            ),
                    )
                    .child(
                        Button::new("catalog-refresh")
                            .small()
                            .ghost()
                            .icon(UiIcon::RefreshCw)
                            .label("刷新")
                            .disabled(self.services.busy)
                            .on_click(cx.listener(|this, _, w, cx| this.load_tables(w, cx))),
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new("catalog-details")
                            .small()
                            .ghost()
                            .label("表详情")
                            .selected(self.services.catalog_details)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.services.catalog_details = !this.services.catalog_details;
                                cx.notify();
                            })),
                    ),
            )
            .child(
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .overflow_hidden()
                    .child(catalog)
                    .when(self.services.catalog_details, |v| {
                        v.child(
                            v_flex()
                                .id("catalog-details-panel")
                                .w(px(260.))
                                .flex_shrink_0()
                                .h_full()
                                .overflow_y_scroll()
                                .p_3()
                                .gap_2()
                                .border_l_1()
                                .border_color(cx.theme().border)
                                .child(
                                    div().text_sm().font_semibold().child(
                                        selected
                                            .map(|t| t.name.clone())
                                            .unwrap_or_else(|| "选择数据库对象".into()),
                                    ),
                                )
                                .when_some(selected, |v, t| {
                                    let p = self
                                        .services
                                        .table_properties
                                        .get(&(t.schema.clone(), t.name.clone()))
                                        .cloned()
                                        .unwrap_or_default();
                                    v.children(
                                        [
                                            ("连接", self.services.database_identity.clone()),
                                            ("模式", t.schema.clone()),
                                            ("对象类型", t.kind.clone()),
                                            ("OID", p.oid),
                                            ("所有者", p.owner),
                                            ("类型 / 引擎", p.engine),
                                            ("行数（估计）", p.rows),
                                            ("大小（字节）", p.bytes),
                                            ("主键", p.primary_key),
                                            ("备注", p.comment),
                                        ]
                                        .into_iter()
                                        .map(
                                            |(k, value)| {
                                                v_flex()
                                                    .gap_1()
                                                    .child(
                                                        div()
                                                            .text_xs()
                                                            .text_color(cx.theme().muted_foreground)
                                                            .child(k),
                                                    )
                                                    .child(
                                                        div().text_xs().whitespace_normal().child(
                                                            if value.is_empty() {
                                                                "—".into()
                                                            } else {
                                                                value
                                                            },
                                                        ),
                                                    )
                                            },
                                        ),
                                    )
                                }),
                        )
                    }),
            )
    }
}
