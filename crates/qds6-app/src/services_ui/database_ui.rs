//! 功能：数据库对象、SQL 与结果同屏组织，低频写入和导出按需展开；时间：2026-11-11；作者：lq。

use super::*;

impl Workbench {
    /// 功能：SQL 页面只在查询后分配结果区，空闲时保留完整编辑空间；时间：2026-11-11；作者：lq。
    pub(crate) fn has_query_results(&self) -> bool {
        self.services.results.is_some()
    }

    /// 功能：共享原生结果标签和虚拟网格，多结果集不再跳转到独立页面；时间：2026-11-11；作者：lq。
    pub(crate) fn query_results_panel(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let mut tabs = TabBar::new("query-results")
            .small()
            .underline()
            .min_w_0()
            .flex_1()
            .menu(true)
            .selected_index(self.services.selected_result);
        if let Some(results) = &self.services.results {
            for (index, set) in results.sets.iter().enumerate() {
                tabs = tabs.child(Tab::new().label(format!(
                    "结果 {} · {} 行",
                    index + 1,
                    set.rows.len()
                )));
            }
        }
        let tabs = tabs.on_click(cx.listener(|this, index: &usize, window, cx| {
            if let Some(set) = this
                .services
                .results
                .as_ref()
                .and_then(|r| r.sets.get(*index))
            {
                this.services.grid = if set.columns.is_empty() {
                    None
                } else {
                    Some(cx.new(|cx| TableState::new(QueryRows(set.clone()), window, cx)))
                };
                this.services.selected_result = *index;
                cx.notify();
            }
        }));
        v_flex()
            .size_full()
            .min_h_0()
            .child(
                h_flex()
                    .h(px(36.))
                    .flex_shrink_0()
                    .px_3()
                    .gap_3()
                    .items_center()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(tabs)
                    .when_some(self.services.results.as_ref(), |row, result| {
                        row.child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!(
                                    "{} ms{}",
                                    result.elapsed_ms,
                                    if result.truncated {
                                        " · 已达上限"
                                    } else {
                                        ""
                                    }
                                )),
                        )
                    }),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .when_some(self.services.grid.as_ref(), |v, grid| {
                        v.child(DataTable::new(grid).small())
                    })
                    .when(self.services.grid.is_none(), |v| {
                        v.p_5()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(
                                self.services
                                    .results
                                    .as_ref()
                                    .and_then(|r| r.sets.get(self.services.selected_result))
                                    .map(|set| format!("执行完成 · 影响 {} 行", set.affected))
                                    .unwrap_or_else(|| "暂无结果".into()),
                            )
                    }),
            )
    }

    /// 功能：数据库上下文固定在对象树，查询编辑和结果共享中央工作区；时间：2026-11-11；作者：lq。
    pub(super) fn database_page(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let connected = self.services.database.is_some();
        let toolbar = h_flex()
            .px_3()
            .py_2()
            .gap_2()
            .flex_wrap()
            .items_center()
            .border_b_1()
            .border_color(cx.theme().border)
            .children(
                [
                    ("begin-transaction", "BEGIN", "BEGIN"),
                    ("commit-transaction", "COMMIT", "COMMIT"),
                    ("rollback-transaction", "ROLLBACK", "ROLLBACK"),
                ]
                .into_iter()
                .map(|(id, label, sql)| {
                    Button::new(id)
                        .ghost()
                        .small()
                        .label(label)
                        .disabled(self.services.busy || !connected)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.execute_sql(Some(sql.into()), window, cx)
                        }))
                }),
            )
            .child(div().flex_1())
            .child(
                Button::new("toggle-database-tools")
                    .ghost()
                    .small()
                    .icon(UiIcon::Settings)
                    .label("数据修改 / DDL")
                    .selected(self.services.show_database_tools)
                    .disabled(!connected)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.services.show_database_tools = !this.services.show_database_tools;
                        cx.notify();
                    })),
            )
            .child(
                Button::new("toggle-csv-export")
                    .ghost()
                    .small()
                    .icon(UiIcon::Download)
                    .label("CSV 导出")
                    .selected(self.services.show_csv_export)
                    .disabled(self.services.results.is_none())
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.services.show_csv_export = !this.services.show_csv_export;
                        cx.notify();
                    })),
            );
        let objects = v_flex()
            .w(px(180.))
            .flex_shrink_0()
            .h_full()
            .min_h_0()
            .border_r_1()
            .border_color(cx.theme().border)
            .child(
                h_flex()
                    .h(px(46.))
                    .px_3()
                    .items_center()
                    .justify_between()
                    .child(div().text_xs().font_semibold().child("数据库对象"))
                    .child(
                        Button::new("load-tables")
                            .ghost()
                            .small()
                            .icon(UiIcon::RefreshCw)
                            .tooltip("刷新对象")
                            .disabled(self.services.busy || !connected)
                            .on_click(
                                cx.listener(|this, _, window, cx| this.load_tables(window, cx)),
                            ),
                    ),
            )
            .child(
                div()
                    .id("database-objects")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px_2()
                    .child(
                        v_flex()
                            .gap_1()
                            .children(self.services.tables.iter().enumerate().map(
                                |(index, table)| {
                                    h_flex()
                                        .id(("database-object", index))
                                        .test_support()
                                        .p_2()
                                        .gap_2()
                                        .items_center()
                                        .rounded_md()
                                        .cursor_pointer()
                                        .bg(if self.services.selected_table == Some(index) {
                                            cx.theme().secondary
                                        } else {
                                            cx.theme().background
                                        })
                                        .hover(|style| style.bg(cx.theme().secondary))
                                        .child(Icon::new(UiIcon::Table2).xsmall())
                                        .child(
                                            div()
                                                .min_w_0()
                                                .text_xs()
                                                .text_ellipsis()
                                                .child(format!("{}.{}", table.schema, table.name)),
                                        )
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.services.selected_table = Some(index);
                                            cx.notify();
                                        }))
                                },
                            )),
                    )
                    .when(self.services.tables.is_empty(), |v| {
                        v.child(
                            div()
                                .p_2()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child("点击刷新读取表与视图"),
                        )
                    }),
            )
            .child(
                v_flex()
                    .p_2()
                    .gap_1()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(
                        Button::new("browse-table")
                            .small()
                            .label("浏览所选表")
                            .disabled(self.services.busy || self.services.selected_table.is_none())
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.table_operation(false, window, cx)
                            })),
                    )
                    .child(
                        Button::new("table-structure")
                            .ghost()
                            .small()
                            .label("字段与索引")
                            .disabled(self.services.busy || self.services.selected_table.is_none())
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.table_operation(true, window, cx)
                            })),
                    ),
            );
        v_flex()
            .size_full()
            .min_h_0()
            .child(toolbar)
            .when(self.services.show_csv_export, |v| {
                v.child(
                    h_flex()
                        .px_4()
                        .py_3()
                        .gap_3()
                        .items_center()
                        .border_b_1()
                        .border_color(cx.theme().border)
                        .child(div().text_xs().child("CSV 保存路径"))
                        .child(
                            div().flex_1().min_w_0().child(
                                Input::new(&self.services.csv_path).aria_label("CSV 导出路径"),
                            ),
                        )
                        .child(
                            Button::new("export-csv")
                                .small()
                                .label("导出当前结果")
                                .disabled(self.services.busy || self.services.results.is_none())
                                .on_click(cx.listener(|this, _, window, cx| {
                                    let Some(set) = this
                                        .services
                                        .results
                                        .as_ref()
                                        .and_then(|r| r.sets.get(this.services.selected_result))
                                        .cloned()
                                    else {
                                        return;
                                    };
                                    let path = PathBuf::from(
                                        this.services.csv_path.read(cx).value().as_str(),
                                    );
                                    this.service_job(
                                        "正在导出 CSV…",
                                        move || {
                                            backup::write_new(
                                                &path,
                                                &qds6_services::database::csv(&set),
                                            )
                                        },
                                        |this, _, _, _| {
                                            this.status =
                                                "CSV 已导出（空字段为 NULL，引号空字符串为空文本）"
                                                    .into()
                                        },
                                        window,
                                        cx,
                                    );
                                })),
                        ),
                )
            })
            .when(self.services.show_database_tools, |v| {
                v.child(self.database_write_tools(cx))
            })
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .when(connected, |row| row.child(objects))
                    .child(div().flex_1().min_w_0().h_full().child(self.sql_page(cx))),
            )
    }

    /// 功能：写入工具集中在有界展开区，确认和参数化服务逻辑保持原样；时间：2026-11-11；作者：lq。
    fn database_write_tools(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        div()
            .id("database-write-tools")
            .max_h(px(210.))
            .overflow_y_scroll()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                v_flex()
                    .p_4()
                    .gap_3()
                    .child(
                        h_flex()
                            .gap_3()
                            .child(self.field("主键列", &self.services.edit_key_column))
                            .child(self.field("主键值", &self.services.edit_key)),
                    )
                    .child(
                        h_flex()
                            .gap_3()
                            .child(self.field("修改列 / DDL 字段", &self.services.edit_column))
                            .child(self.field("新值 / 新表名称", &self.services.edit_value)),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .flex_wrap()
                            .child(
                                Checkbox::new("edit-null")
                                    .label("NULL")
                                    .checked(self.services.edit_null)
                                    .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                        this.services.edit_null = *checked;
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("update-row")
                                    .small()
                                    .label("预览更新一行")
                                    .disabled(self.services.busy)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.confirm_edit(false, window, cx)
                                    })),
                            )
                            .child(
                                Button::new("delete-row")
                                    .small()
                                    .label("预览删除一行")
                                    .disabled(self.services.busy)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.confirm_edit(true, window, cx)
                                    })),
                            )
                            .child(
                                Button::new("create-table-ddl")
                                    .ghost()
                                    .small()
                                    .label("新表 DDL")
                                    .disabled(self.services.busy)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.preview_ddl(true, window, cx)
                                    })),
                            )
                            .child(
                                Button::new("alter-table-ddl")
                                    .ghost()
                                    .small()
                                    .label("新增字段 DDL")
                                    .disabled(self.services.busy)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.preview_ddl(false, window, cx)
                                    })),
                            ),
                    ),
            )
    }
}
