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

    /// 功能：数据库常用入口与对象/多表工作区，SQL 仅由新建查询打开；时间：2026-11-11；作者：lq。
    pub(super) fn database_page(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        use gpui_kit::component::resizable::{h_resizable, resizable_panel};
        let connected = self.services.database.is_some();
        let toolbar = h_flex()
            .px_3()
            .py_2()
            .gap_2()
            .items_center()
            .flex_wrap()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                Button::new("database-new-query")
                    .small()
                    .ghost()
                    .icon(UiIcon::FileCode)
                    .label("新建查询")
                    .disabled(!connected)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.activate_workspace(crate::workspace_ui::WorkspaceTarget::Sql, cx)
                    })),
            )
            .children(
                [(0, "表", UiIcon::Table2), (1, "视图", UiIcon::Columns2)]
                    .into_iter()
                    .map(|(kind, label, icon)| {
                        Button::new(("database-kind", kind))
                            .small()
                            .ghost()
                            .icon(icon)
                            .label(label)
                            .selected(
                                self.services.object_kind == kind
                                    && self.services.active_table_tab.is_none(),
                            )
                            .disabled(!connected)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.services.object_kind = kind;
                                this.services.active_table_tab = None;
                                this.services.query_active = false;
                                cx.notify();
                            }))
                    }),
            )
            .child(
                Button::new("database-design")
                    .small()
                    .ghost()
                    .icon(UiIcon::Settings)
                    .label("表设计")
                    .disabled(!connected)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.services.show_database_tools = !this.services.show_database_tools;
                        cx.notify();
                    })),
            )
            .child(
                Button::new("toggle-csv-export")
                    .small()
                    .ghost()
                    .icon(UiIcon::Download)
                    .label("导出 CSV")
                    .disabled(!connected)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.services.show_csv_export = !this.services.show_csv_export;
                        cx.notify();
                    })),
            )
            .child(div().flex_1())
            .child(
                Button::new("database-disconnect")
                    .small()
                    .ghost()
                    .icon(UiIcon::Unplug)
                    .label("关闭连接")
                    .disabled(self.services.busy || !connected)
                    .on_click(cx.listener(|this, _, window, cx| this.close_database(window, cx))),
            );
        let objects = v_flex()
            .id("database-object-pane")
            .test_support()
            .size_full()
            .min_h_0()
            .child(
                h_flex()
                    .h(px(40.))
                    .px_2()
                    .items_center()
                    .justify_between()
                    .child(div().text_xs().font_semibold().child("数据库对象"))
                    .child(
                        Button::new("load-tables")
                            .small()
                            .ghost()
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
                    .px_2()
                    .pb_2()
                    .child(Input::new(&self.services.object_search).small()),
            )
            .child(
                v_flex()
                    .id("database-objects")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px_2()
                    .children(
                        self.services
                            .tables
                            .iter()
                            .enumerate()
                            .filter(|(_, table)| self.object_matches(table, cx))
                            .map(|(index, table)| {
                                h_flex()
                                    .id(("database-object", index))
                                    .test_support()
                                    .p_2()
                                    .gap_2()
                                    .items_center()
                                    .rounded_md()
                                    .cursor_pointer()
                                    .when(self.services.selected_table == Some(index), |v| {
                                        v.bg(cx.theme().secondary)
                                    })
                                    .hover(|v| v.bg(cx.theme().secondary))
                                    .child(Icon::new(UiIcon::Table2).xsmall())
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .text_xs()
                                            .text_ellipsis()
                                            .child(format!("{}.{}", table.schema, table.name)),
                                    )
                                    .on_click(cx.listener(
                                        move |this, event: &ClickEvent, window, cx| {
                                            this.services.selected_table = Some(index);
                                            if event.click_count() == 2 {
                                                this.table_operation(false, window, cx);
                                            }
                                            cx.notify();
                                        },
                                    ))
                            }),
                    ),
            )
            .child(
                div()
                    .p_2()
                    .text_xs()
                    .child(format!("{} 个对象 · 双击打开", self.services.tables.len())),
            );
        let mut tabs = TabBar::new("database-table-tabs")
            .small()
            .min_w_0()
            .menu(true)
            .selected_index(if self.services.query_active {
                self.services.table_tabs.len() + 1
            } else {
                self.services.active_table_tab.map(|i| i + 1).unwrap_or(0)
            })
            .child(
                Tab::new()
                    .label("对象")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.services.active_table_tab = None;
                        this.services.query_active = false;
                        cx.notify();
                    })),
            );
        for (index, tab) in self.services.table_tabs.iter().enumerate() {
            let dirty = !tab.grid.read(cx).delegate().changes().is_empty();
            tabs = tabs.child(
                Tab::new()
                    .label(format!(
                        "{}{}",
                        tab.table.name,
                        if dirty { " •" } else { "" }
                    ))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.activate_table_tab(index, cx);
                    }))
                    .suffix(
                        Button::new(("close-table", index))
                            .xsmall()
                            .ghost()
                            .icon(UiIcon::X)
                            .disabled(self.services.busy)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.close_table_tab(index, window, cx);
                            })),
                    ),
            );
        }
        if self.services.query_open {
            tabs = tabs.child(
                Tab::new()
                    .label(format!("查询{}", if self.sql_dirty { " •" } else { "" }))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.services.query_active = true;
                        cx.notify();
                    }))
                    .suffix(
                        Button::new("close-database-query")
                            .xsmall()
                            .ghost()
                            .icon(UiIcon::X)
                            .on_click(cx.listener(|this, _, _, cx| {
                                cx.stop_propagation();
                                this.services.query_open = false;
                                this.services.query_active = false;
                                cx.notify();
                            })),
                    ),
            );
        }
        let center = v_flex()
            .id("database-table-workspace")
            .test_support()
            .size_full()
            .min_h_0()
            .min_w_0()
            .child(tabs)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .child(if self.services.query_active {
                        self.database_query_panel(cx).into_any_element()
                    } else if let Some(index) = self.services.active_table_tab {
                        self.table_data_panel(index, cx).into_any_element()
                    } else {
                        self.database_catalog(cx).into_any_element()
                    }),
            );
        v_flex()
            .size_full()
            .min_h_0()
            .child(toolbar)
            .when(self.services.show_database_tools, |v| {
                v.child(self.database_write_tools(cx))
            })
            .when(self.services.show_csv_export, |v| {
                v.child(
                    h_flex()
                        .p_2()
                        .gap_2()
                        .items_center()
                        .child(
                            div().flex_1().child(
                                Input::new(&self.services.csv_path)
                                    .small()
                                    .aria_label("CSV 导出路径"),
                            ),
                        )
                        .child(
                            Button::new("export-csv")
                                .small()
                                .label("导出当前表 / 结果")
                                .disabled(self.services.busy)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.export_database_csv(window, cx)
                                })),
                        ),
                )
            })
            .child(div().flex_1().min_h_0().child(if connected {
                h_resizable("database-object-split")
                    .child(
                        resizable_panel()
                            .size(px(220.))
                            .size_range(px(150.)..px(500.))
                            .child(objects),
                    )
                    .child(
                        resizable_panel()
                            .size(px(900.))
                            .size_range(px(300.)..Pixels::MAX)
                            .child(center),
                    )
                    .into_any_element()
            } else {
                self.service_empty(
                    Protocol::PostgreSql,
                    "尚未连接数据库",
                    "连接后自动显示对象列表，双击表可打开数据。",
                    cx,
                )
                .into_any_element()
            }))
    }

    /// 功能：对象树和对象列表共享类型及文字筛选；时间：2026-11-11；作者：lq。
    fn object_matches(&self, table: &Table, cx: &App) -> bool {
        let kind = if self.services.object_kind == 0 {
            table.kind == "BASE TABLE"
        } else {
            table.kind != "BASE TABLE"
        };
        kind && format!("{}.{}", table.schema, table.name)
            .to_lowercase()
            .contains(&self.services.object_search.read(cx).value().to_lowercase())
    }

    /// 功能：数据库默认页承载可双击对象列表及可收起表详情；时间：2026-11-11；作者：lq。
    fn database_catalog(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let selected = self
            .services
            .selected_table
            .and_then(|i| self.services.tables.get(i));
        v_flex().size_full().min_h_0()
            .child(h_flex().p_2().gap_2().items_center().border_b_1().border_color(cx.theme().border)
                .child(Button::new("browse-table").small().ghost().icon(UiIcon::FolderOpen).label("打开表").disabled(self.services.busy || selected.is_none()).on_click(cx.listener(|this, _, window, cx| this.table_operation(false, window, cx))))
                .child(Button::new("table-structure").small().ghost().icon(UiIcon::Settings).label("设计表").disabled(selected.is_none()).on_click(cx.listener(|this, _, window, cx| this.table_operation(true, window, cx))))
                .child(div().flex_1())
                .child(Button::new("catalog-details").small().ghost().label("表详情").selected(self.services.catalog_details).on_click(cx.listener(|this, _, _, cx| { this.services.catalog_details = !this.services.catalog_details; cx.notify(); }))))
            .child(h_flex().flex_1().min_h_0()
                .child(v_flex().id("database-catalog").test_support().flex_1().min_w_0().min_h_0().overflow_y_scroll()
                    .child(h_flex().p_2().gap_3().border_b_1().border_color(cx.theme().border).child(div().flex_1().text_xs().font_semibold().child("名称")).child(div().w(px(150.)).text_xs().child("模式")).child(div().w(px(100.)).text_xs().child("类型")))
                    .children(self.services.tables.iter().enumerate().filter(|(_, t)| self.object_matches(t, cx)).map(|(index, table)| {
                        h_flex().id(("catalog-object", index)).test_support().p_2().gap_3().cursor_pointer().border_b_1().border_color(cx.theme().border)
                            .when(self.services.selected_table == Some(index), |v| v.bg(cx.theme().secondary))
                            .hover(|v| v.bg(cx.theme().secondary)).child(Icon::new(UiIcon::Table2).xsmall())
                            .child(div().flex_1().min_w_0().text_xs().text_ellipsis().child(table.name.clone()))
                            .child(div().w(px(150.)).text_xs().text_ellipsis().child(table.schema.clone()))
                            .child(div().w(px(100.)).text_xs().child(if table.kind == "BASE TABLE" { "表" } else { "视图" }))
                            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| { this.services.selected_table = Some(index); if event.click_count() == 2 { this.table_operation(false, window, cx); } cx.notify(); }))
                    })))
                .when(self.services.catalog_details, |v| v.child(v_flex().w(px(220.)).p_3().gap_2().border_l_1().border_color(cx.theme().border)
                    .child(div().text_sm().font_semibold().child(selected.map(|t| t.name.clone()).unwrap_or_else(|| "选择数据库对象".into())))
                    .child(div().text_xs().child(selected.map(|t| format!("连接：{}\n模式：{}\n类型：{}\n双击打开表后查看字段、主键和默认值", self.services.database_identity, t.schema, t.kind)).unwrap_or_default())))))
    }

    /// 功能：常用表设计生成可审阅 SQL，执行入口统一在查询页；时间：2026-11-11；作者：lq。
    fn database_write_tools(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
        let view = cx.entity();
        h_flex()
            .p_3()
            .gap_2()
            .items_center()
            .flex_wrap()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                div().w(px(160.)).child(
                    Input::new(&self.services.edit_value)
                        .small()
                        .aria_label("新表名称"),
                ),
            )
            .child(
                Button::new("create-table-ddl")
                    .small()
                    .label("新建表 SQL")
                    .disabled(self.services.busy)
                    .on_click(
                        cx.listener(|this, _, window, cx| this.preview_ddl(true, window, cx)),
                    ),
            )
            .child(
                div().w(px(160.)).child(
                    Input::new(&self.services.edit_column)
                        .small()
                        .aria_label("新增字段名称"),
                ),
            )
            .child(
                Button::new("alter-table-ddl")
                    .small()
                    .label("新增字段 SQL")
                    .disabled(self.services.busy || self.services.selected_table.is_none())
                    .on_click(
                        cx.listener(|this, _, window, cx| this.preview_ddl(false, window, cx)),
                    ),
            )
            .child(
                Button::new("database-table-actions")
                    .small()
                    .ghost()
                    .label("更多表操作")
                    .disabled(self.services.busy || self.services.selected_table.is_none())
                    .dropdown_menu(move |mut menu, _, _| {
                        for (action, label) in [
                            ("rename", "重命名表 SQL"),
                            ("truncate", "清空表 SQL"),
                            ("drop", "删除表 SQL"),
                            ("drop-column", "删除字段 SQL"),
                        ] {
                            let view = view.clone();
                            menu = menu.item(PopupMenuItem::new(label).on_click(
                                move |_, window, cx| {
                                    view.update(cx, |this, cx| {
                                        this.preview_table_action(action, window, cx)
                                    });
                                },
                            ));
                        }
                        menu
                    }),
            )
    }

    /// 功能：当前表导出原快照，未保存草稿先明确保存；时间：2026-11-11；作者：lq。
    fn export_database_csv(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let set = if !self.services.query_active
            && let Some(index) = self.services.active_table_tab
        {
            let data = self.services.table_tabs[index].grid.read(cx).delegate();
            if !data.changes().is_empty() {
                self.status = "请先保存或取消修改，再导出当前页".into();
                cx.notify();
                return;
            }
            Some(data.page.data.clone())
        } else {
            self.services
                .results
                .as_ref()
                .and_then(|r| r.sets.get(self.services.selected_result))
                .cloned()
        };
        let Some(set) = set else {
            self.status = "请先打开表或执行查询".into();
            cx.notify();
            return;
        };
        let path = PathBuf::from(self.services.csv_path.read(cx).value().as_str());
        self.service_job(
            "正在导出 CSV…",
            move || backup::write_new(&path, &qds6_services::database::csv(&set)),
            |this, _, _, _| this.status = "当前页已导出 CSV".into(),
            window,
            cx,
        );
    }

    /// 功能：关闭真实数据库会话与其表标签，事务和未保存修改明确提示；时间：2026-11-11；作者：lq。
    pub(crate) fn close_database(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.services.busy || self.services.database.is_none() {
            return;
        }
        let dirty = self
            .services
            .table_tabs
            .iter()
            .any(|t| !t.grid.read(cx).delegate().changes().is_empty());
        let answer = window.prompt(
            PromptLevel::Warning,
            "关闭数据库连接？",
            Some(if dirty {
                "表中有未保存修改，关闭将丢弃这些修改；未提交事务将回滚。"
            } else {
                "此连接的表标签将关闭，未提交事务将回滚。SQL 草稿保留在本次会话。"
            }),
            &["取消", "关闭连接"],
            cx,
        );
        let session = self.services.database.clone();
        cx.spawn_in(window, async move |this, cx| {
            if answer.await == Ok(1) {
                let _ = this.update_in(cx, |this, _, cx| {
                    if this.services.busy
                        || !this
                            .services
                            .database
                            .as_ref()
                            .zip(session.as_ref())
                            .is_some_and(|(a, b)| Arc::ptr_eq(a, b))
                    {
                        return;
                    }
                    this.services.table_tabs.clear();
                    this.services.active_table_tab = None;
                    this.services.query_active = false;
                    let database = this.services.database.take();
                    this.services.database_profile = None;
                    this.services.database_identity.clear();
                    this.services.database_context = (String::new(), String::new());
                    this.services.tables.clear();
                    this.services.selected_table = None;
                    this.services.grid = None;
                    this.services.results = None;
                    if this.services.page == 3 && this.tab == 3 {
                        this.tab = 0;
                    }
                    this.status = "数据库连接已关闭".into();
                    // 功能：连接销毁和事务回滚留在后台，不阻塞 GPUI；时间：2026-11-11；作者：lq。
                    cx.background_spawn(async move {
                        drop(database);
                        drop(session);
                    })
                    .detach();
                    cx.notify();
                });
            }
        })
        .detach();
    }
    /// 功能：查询标签拥有连接上下文、运行/停止与可拖动结果区；时间：2026-11-11；作者：lq。
    fn database_query_panel(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        use gpui_kit::component::resizable::{resizable_panel, v_resizable};
        let editor = div()
            .id("database-query-editor")
            .test_support()
            .size_full()
            .min_h_0()
            .child(
                Editor::new(&self.sql)
                    .size_full()
                    .bordered(false)
                    .aria_label("SQL 查询编辑器"),
            );
        v_flex()
            .size_full()
            .min_h_0()
            .child(
                h_flex()
                    .p_2()
                    .gap_2()
                    .items_center()
                    .flex_wrap()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        Button::new("query-copy")
                            .small()
                            .ghost()
                            .icon(UiIcon::Copy)
                            .tooltip("复制 SQL")
                            .on_click(cx.listener(|this, _, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(
                                    this.sql.read(cx).value().to_string(),
                                ));
                            })),
                    )
                    .child(div().text_xs().child(format!(
                        "{}  /  {}  /  {}",
                        self.services.database_identity,
                        self.services.database_context.0,
                        self.services.database_context.1
                    )))
                    .child(div().flex_1())
                    .child(
                        Button::new("run-sql")
                            .small()
                            .primary()
                            .icon(UiIcon::Play)
                            .label("运行")
                            .disabled(self.services.busy || self.services.database.is_none())
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.execute_sql(None, window, cx)
                            })),
                    )
                    .child(
                        Button::new("cancel-query")
                            .small()
                            .ghost()
                            .icon(UiIcon::X)
                            .label("停止")
                            .disabled(!self.services.busy)
                            .on_click(cx.listener(|this, _, _, cx| this.cancel_query(cx))),
                    ),
            )
            .child(
                h_flex()
                    .px_2()
                    .py_1()
                    .gap_2()
                    .items_center()
                    .flex_wrap()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(div().text_xs().child("选区优先执行"))
                    .child(div().flex_1())
                    .children(
                        [
                            ("query-begin", "BEGIN"),
                            ("query-commit", "COMMIT"),
                            ("query-rollback", "ROLLBACK"),
                        ]
                        .into_iter()
                        .map(|(id, sql)| {
                            Button::new(id)
                                .small()
                                .ghost()
                                .label(sql)
                                .disabled(self.services.busy)
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.execute_sql(Some(sql.into()), window, cx)
                                }))
                        }),
                    )
                    .child(
                        Button::new("query-export")
                            .small()
                            .ghost()
                            .icon(UiIcon::Download)
                            .tooltip("导出查询结果")
                            .disabled(self.services.results.is_none())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.services.show_csv_export = !this.services.show_csv_export;
                                cx.notify();
                            })),
                    ),
            )
            .child(div().flex_1().min_h_0().child(if self.has_query_results() {
                v_resizable("database-query-results-split")
                    .child(
                        resizable_panel()
                            .size(px(350.))
                            .size_range(px(100.)..Pixels::MAX)
                            .child(editor),
                    )
                    .child(
                        resizable_panel()
                            .size(px(250.))
                            .size_range(px(100.)..Pixels::MAX)
                            .child(self.query_results_panel(cx)),
                    )
                    .into_any_element()
            } else {
                editor.into_any_element()
            }))
    }
    /// 功能：基础表操作生成准确方言 SQL，保留查询草稿与未保存数据；时间：2026-11-11；作者：lq。
    fn preview_table_action(&mut self, action: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(table) = self
            .services
            .selected_table
            .and_then(|i| self.services.tables.get(i))
        else {
            return;
        };
        if self.services.table_tabs.iter().any(|t| {
            t.table.name == table.name
                && t.table.schema == table.schema
                && !t.grid.read(cx).delegate().changes().is_empty()
        }) {
            self.status = "请先保存或取消该表的数据修改".into();
            cx.notify();
            return;
        }
        let Some(profile) = &self.services.database_profile else {
            return;
        };
        let quote = |s: &str| qds6_services::database::identifier(profile.protocol, s);
        let result = (|| {
            let target = format!("{}.{}", quote(&table.schema)?, quote(&table.name)?);
            Ok::<_, String>(match action {
                "rename" => format!(
                    "ALTER TABLE {target} RENAME TO {};",
                    quote(self.services.edit_value.read(cx).value().as_str())?
                ),
                "truncate" => format!("TRUNCATE TABLE {target};"),
                "drop" => format!(
                    "DROP {} {target};",
                    if table.kind == "BASE TABLE" {
                        "TABLE"
                    } else {
                        "VIEW"
                    }
                ),
                "drop-column" => format!(
                    "ALTER TABLE {target} DROP COLUMN {};",
                    quote(self.services.edit_column.read(cx).value().as_str())?
                ),
                _ => return Err("不支持此表操作".into()),
            })
        })();
        match result {
            Ok(sql) => self.show_generated_sql(sql, window, cx),
            Err(e) => {
                self.status = e.into();
                self.error = true;
            }
        }
        cx.notify();
    }
    /// 功能：结构 SQL 进入查询页，替换修改过的草稿前确认并校验代次；时间：2026-11-11；作者：lq。
    pub(super) fn show_generated_sql(
        &mut self,
        sql: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.services.busy {
            return;
        }
        if !self.sql_dirty {
            self.apply_generated_sql(sql, window, cx);
            return;
        }
        let expected = self.sql.read(cx).value().to_string();
        let answer = window.prompt(PromptLevel::Warning, "替换当前查询草稿？", Some("当前查询有修改。需要保留时请先取消并复制；确认后将替换为表操作 SQL，不会自动执行。"), &["取消", "替换草稿"], cx);
        cx.spawn_in(window, async move |this, cx| {
            if answer.await == Ok(1) {
                let _ = this.update_in(cx, |this, window, cx| {
                    if this.services.busy || this.sql.read(cx).value().as_str() != expected {
                        this.status = "查询内容或任务状态已变化，未替换草稿".into();
                        cx.notify();
                        return;
                    }
                    this.apply_generated_sql(sql, window, cx);
                });
            }
        })
        .detach();
    }

    /// 功能：生成的结构 SQL 仅供审阅，仍需明确点击运行；时间：2026-11-11；作者：lq。
    fn apply_generated_sql(&mut self, sql: String, window: &mut Window, cx: &mut Context<Self>) {
        self.sql.update(cx, |s, cx| s.set_value(sql, window, cx));
        self.sql_dirty = true;
        self.activate_workspace(crate::workspace_ui::WorkspaceTarget::Sql, cx);
        self.status = "表操作 SQL 已生成，请审阅目标后点击运行".into();
        cx.notify();
    }
}
