//! 功能：Navicat 式字段列表行内编辑、类型选择与批量 SQL 预览；时间：2026-11-12；作者：lq。
use super::*;
use qds6_services::database::GridColumn;
mod sql;
use sql::{ColumnChange, Definition, FieldValues};

/// 功能：每个字段独立保留可编辑单元格，新增、删除和修改均留在列表中；时间：2026-11-12；作者：lq。
struct DesignRow {
    original: Option<Definition>,
    inputs: Vec<Entity<InputState>>,
    nullable: bool,
    deleted: bool,
    _subscriptions: Vec<Subscription>,
}
impl DesignRow {
    /// 功能：按表元数据初始化行，观察输入变化更新待修改状态；时间：2026-11-12；作者：lq。
    fn new(column: Option<&GridColumn>, window: &mut Window, cx: &mut Context<Workbench>) -> Self {
        let original = column.map(Definition::from);
        let v = original
            .as_ref()
            .map(|o| o.values.clone())
            .unwrap_or_else(|| FieldValues {
                data_type: "text".into(),
                nullable: true,
                ..Default::default()
            });
        let inputs: Vec<_> = [v.name, v.data_type, v.length, v.scale, v.default, v.comment]
            .into_iter()
            .zip(["字段名称", "选择或输入类型", "—", "—", "—", ""])
            .map(|(value, placeholder)| {
                cx.new(|cx| {
                    InputState::new(window, cx)
                        .placeholder(placeholder)
                        .default_value(value)
                })
            })
            .collect();
        let subscriptions = inputs
            .iter()
            .map(|input| cx.observe(input, |_, _, cx| cx.notify()))
            .collect();
        Self {
            original,
            inputs,
            nullable: v.nullable,
            deleted: false,
            _subscriptions: subscriptions,
        }
    }
    /// 功能：读取完整行草稿供脏标记和 SQL 计划使用；时间：2026-11-12；作者：lq。
    fn change(&self, cx: &App) -> ColumnChange {
        let values: Vec<_> = self
            .inputs
            .iter()
            .map(|input| input.read(cx).value().to_string())
            .collect();
        ColumnChange {
            original: self.original.clone(),
            deleted: self.deleted,
            values: FieldValues {
                name: values[0].clone(),
                data_type: values[1].clone(),
                length: values[2].clone(),
                scale: values[3].clone(),
                default: values[4].clone(),
                comment: values[5].clone(),
                nullable: self.nullable,
            },
        }
    }
}

/// 功能：按表隔离的字段行草稿与横纵滚动状态；时间：2026-11-12；作者：lq。
pub(super) struct TableDesign {
    rows: Vec<DesignRow>,
    indexes: bool,
    horizontal: ScrollHandle,
    vertical: ScrollHandle,
}
impl TableDesign {
    /// 功能：从真实字段快照建立行内设计器；时间：2026-11-12；作者：lq。
    pub fn new(columns: &[GridColumn], window: &mut Window, cx: &mut Context<Workbench>) -> Self {
        Self {
            rows: columns
                .iter()
                .map(|c| DesignRow::new(Some(c), window, cx))
                .collect(),
            indexes: false,
            horizontal: ScrollHandle::new(),
            vertical: ScrollHandle::new(),
        }
    }
    /// 功能：结构草稿参与关闭确认、标签脏标记与刷新保护；时间：2026-11-12；作者：lq。
    pub fn dirty(&self, cx: &App) -> bool {
        self.rows.iter().any(|r| r.change(cx).dirty())
    }
}

impl Workbench {
    /// 功能：新增直接插入可编辑行，滚动到新行并聚焦字段名称；时间：2026-11-12；作者：lq。
    fn add_design_row(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let row = DesignRow::new(None, window, cx);
        row.inputs[0].update(cx, |input, cx| input.focus(window, cx));
        let d = &mut self.services.table_tabs[index].design;
        d.rows.push(row);
        d.horizontal.set_offset(point(px(0.), px(0.)));
        d.vertical.scroll_to_bottom();
        cx.notify();
    }
    /// 功能：按预置切换类型，自动填写常用长度并清理不兼容的旧参数；时间：2026-11-12；作者：lq。
    fn choose_design_type(
        &mut self,
        index: usize,
        row: usize,
        value: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let inputs = &self.services.table_tabs[index].design.rows[row].inputs;
        let length = match value {
            "character varying" | "varchar" | "varbinary" => "255",
            "character" | "char" | "binary" | "bit" | "bit varying" => "1",
            "numeric" | "decimal" => "10",
            _ => "",
        };
        for (input, value) in [
            (&inputs[1], value),
            (&inputs[2], length),
            (
                &inputs[3],
                if matches!(value, "numeric" | "decimal") {
                    "0"
                } else {
                    ""
                },
            ),
        ] {
            input.update(cx, |input, cx| input.set_value(value, window, cx));
        }
        cx.notify();
    }
    /// 功能：每一行直接编辑字段属性，固定表头与横滚列表共用偏移；时间：2026-11-12；作者：lq。
    pub(super) fn table_design_panel(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let tab = &self.services.table_tabs[index];
        let data = tab.grid.read(cx).delegate();
        let d = &tab.design;
        let readonly =
            self.services.busy || tab.table.kind != "BASE TABLE" || !data.changes().is_empty();
        let count = d.rows.iter().filter(|r| r.change(cx).dirty()).count();
        let protocol = self
            .services
            .database_profile
            .as_ref()
            .map(|p| p.protocol)
            .unwrap_or(Protocol::PostgreSql);
        let toolbar = h_flex()
            .p_2()
            .gap_2()
            .items_center()
            .flex_wrap()
            .flex_shrink_0()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                Button::new("design-back-data")
                    .small()
                    .label("返回数据")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.services.table_tabs[index].designing = false;
                        cx.notify();
                    })),
            )
            .children(
                [(false, "字段"), (true, "索引")]
                    .into_iter()
                    .map(|(indexes, label)| {
                        Button::new(if indexes {
                            "design-indexes"
                        } else {
                            "design-fields"
                        })
                        .small()
                        .ghost()
                        .label(label)
                        .selected(d.indexes == indexes)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.services.table_tabs[index].design.indexes = indexes;
                            cx.notify();
                        }))
                    }),
            )
            .child(div().flex_1())
            .child(div().text_xs().child(format!(
                "{}.{} · {} 个字段",
                tab.table.schema,
                tab.table.name,
                data.page.columns.len()
            )))
            .child(
                Button::new("design-refresh")
                    .small()
                    .ghost()
                    .icon(UiIcon::RefreshCw)
                    .tooltip("重新读取结构与数据；有草稿时请先撤销修改")
                    .disabled(readonly || count > 0)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.refresh_table_tab(BrowseRequest::default(), window, cx);
                    })),
            );
        // 功能：紧凑行操作留在字段页顶部，避免底部独立表单分散编辑；时间：2026-11-12；作者：lq。
        let actions = h_flex()
            .px_2()
            .py_2()
            .gap_2()
            .items_center()
            .flex_wrap()
            .flex_shrink_0()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                Button::new("design-new-column")
                    .small()
                    .ghost()
                    .icon(UiIcon::Plus)
                    .label("添加字段")
                    .disabled(readonly)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.add_design_row(index, window, cx)
                    })),
            )
            .child(
                Button::new("design-preview")
                    .small()
                    .primary()
                    .label("预览变更 SQL")
                    .disabled(readonly || count == 0)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.preview_design(index, window, cx)
                    })),
            )
            .child(
                Button::new("design-discard")
                    .small()
                    .ghost()
                    .label("撤销修改")
                    .disabled(readonly || count == 0)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        let columns = this.services.table_tabs[index]
                            .grid
                            .read(cx)
                            .delegate()
                            .page
                            .columns
                            .clone();
                        this.services.table_tabs[index].design =
                            TableDesign::new(&columns, window, cx);
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(if count == 0 {
                        "直接点击单元格编辑 · 类型可下拉选择".to_string()
                    } else {
                        format!("{count} 个字段待变更 · 预览后在查询页运行")
                    }),
            );
        let widths = [30., 160., 220., 68., 64., 56., 48., 180., 240., 64.];
        let total: f32 = widths.iter().sum();
        let heading = h_flex()
            .h(px(32.))
            .items_center()
            .flex_shrink_0()
            .bg(cx.theme().secondary)
            .children(
                [
                    "",
                    "名称",
                    "类型",
                    "长度",
                    "小数位",
                    "非空",
                    "主键",
                    "默认值",
                    "备注",
                    "操作",
                ]
                .into_iter()
                .zip(widths)
                .map(|(label, width)| {
                    div()
                        .w(px(width))
                        .flex_shrink_0()
                        .px_2()
                        .text_xs()
                        .font_semibold()
                        .child(label)
                }),
            );
        let rows = d
            .rows
            .iter()
            .enumerate()
            .filter(|(_, r)| !r.deleted || r.original.is_some())
            .map(|(row, r)| {
                let generated = r.original.as_ref().is_some_and(|o| o.generated);
                let locked = readonly || r.deleted || generated;
                let change = r.change(cx);
                let marker = if r.deleted {
                    "−"
                } else if r.original.is_none() {
                    "+"
                } else if change.dirty() {
                    "•"
                } else {
                    ""
                };
                let input_cell = |field: usize, width: f32| {
                    div()
                        .id(SharedString::from(format!("design-input-{row}-{field}")))
                        .test_support()
                        .w(px(width))
                        .h_full()
                        .flex_shrink_0()
                        .border_r_1()
                        .border_color(cx.theme().border)
                        .child(
                            Input::new(&r.inputs[field])
                                .small()
                                .appearance(false)
                                .disabled(locked),
                        )
                };
                let view = cx.entity();
                h_flex()
                    .id(("design-column", row))
                    .test_support()
                    .h(px(36.))
                    .items_center()
                    .flex_shrink_0()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .when(change.dirty(), |v| v.bg(cx.theme().secondary.opacity(0.5)))
                    .child(
                        div()
                            .w(px(widths[0]))
                            .flex_shrink_0()
                            .text_center()
                            .text_xs()
                            .child(marker),
                    )
                    .child(input_cell(0, widths[1]))
                    .child(
                        h_flex()
                            .w(px(widths[2]))
                            .flex_shrink_0()
                            .border_r_1()
                            .border_color(cx.theme().border)
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .id(("design-type-input", row))
                                    .test_support()
                                    .child(
                                        Input::new(&r.inputs[1])
                                            .small()
                                            .appearance(false)
                                            .disabled(locked),
                                    ),
                            )
                            .child(
                                Button::new(("design-type-menu", row))
                                    .small()
                                    .ghost()
                                    .icon(UiIcon::ChevronDown)
                                    .tooltip("选择字段类型")
                                    .disabled(locked)
                                    .dropdown_menu(move |mut menu, _, _| {
                                        for &value in sql::type_presets(protocol) {
                                            let view = view.clone();
                                            menu = menu.item(PopupMenuItem::new(value).on_click(
                                                move |_, window, cx| {
                                                    view.update(cx, |this, cx| {
                                                        this.choose_design_type(
                                                            index, row, value, window, cx,
                                                        )
                                                    });
                                                },
                                            ));
                                        }
                                        menu
                                    }),
                            ),
                    )
                    .child(input_cell(2, widths[3]))
                    .child(input_cell(3, widths[4]))
                    .child(
                        div()
                            .w(px(widths[5]))
                            .flex_shrink_0()
                            .flex()
                            .justify_center()
                            .child(
                                Checkbox::new(("design-not-null", row))
                                    .checked(!r.nullable)
                                    .disabled(locked)
                                    .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                                        this.services.table_tabs[index].design.rows[row].nullable =
                                            !*checked;
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .w(px(widths[6]))
                            .flex_shrink_0()
                            .text_xs()
                            .text_center()
                            .child(if r.original.as_ref().is_some_and(|o| o.primary) {
                                "PK"
                            } else {
                                ""
                            }),
                    )
                    .child(input_cell(4, widths[7]))
                    .child(input_cell(5, widths[8]))
                    .child(
                        div().w(px(widths[9])).flex_shrink_0().child(
                            Button::new(("design-delete-row", row))
                                .small()
                                .ghost()
                                .label(if r.deleted { "恢复" } else { "删除" })
                                .disabled(readonly)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    let row = &mut this.services.table_tabs[index].design.rows[row];
                                    row.deleted = !row.deleted;
                                    cx.notify();
                                })),
                        ),
                    )
            });
        let fields = v_flex().flex_1().min_h_0().min_w_0().overflow_hidden().child(actions)
            .child(div().id("design-horizontal-viewport").test_support().flex_1().min_h_0().min_w_0().overflow_x_scroll().restrict_scroll_to_axis().track_scroll(&d.horizontal)
                .child(v_flex().w(px(total)).flex_shrink_0().h_full().child(heading)
                    .child(v_flex().id("design-column-list").test_support().flex_1().min_h_0().overflow_y_scroll().restrict_scroll_to_axis().track_scroll(&d.vertical)
                        .children(rows)
                        .child(div().p_2().child(Button::new("design-add-tail").small().ghost().icon(UiIcon::Plus).label("添加字段").disabled(readonly)
                            .on_click(cx.listener(move |this, _, window, cx| this.add_design_row(index, window, cx))))))))
            .child(div().id("design-horizontal-scrollbar").test_support().h(px(12.)).flex_shrink_0().relative()
                .child(Scrollbar::horizontal(&d.horizontal).viewport_from_layout().mode(ScrollbarMode::Always)))
            .child(div().px_3().py_2().flex_shrink_0().text_xs().text_color(cx.theme().muted_foreground)
                .child(if readonly { "请先保存或取消数据修改；视图仅支持查看结构" } else {
                    "变更保留在当前表草稿中 · 删除可恢复 · 主键显示现有定义 · 生成列通过 SQL 编辑"
                }));
        let indexes = v_flex()
            .id("design-index-list")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .p_3()
            .gap_3()
            .children(data.page.indexes.rows.iter().map(|r| {
                div()
                    .text_xs()
                    .child(r.iter().map(Cell::display).collect::<Vec<_>>().join("\n"))
            }))
            .when(data.page.indexes.rows.is_empty(), |v| v.child("无可见索引"));
        v_flex()
            .size_full()
            .min_w_0()
            .min_h_0()
            .overflow_hidden()
            .child(toolbar)
            .child(if d.indexes {
                indexes.into_any_element()
            } else {
                fields.into_any_element()
            })
            .into_any_element()
    }
    /// 功能：统一预览全部字段草稿，不直接对连接执行结构写入；时间：2026-11-12；作者：lq。
    fn preview_design(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let tab = &self.services.table_tabs[index];
        if self.services.busy
            || tab.table.kind != "BASE TABLE"
            || !tab.grid.read(cx).delegate().changes().is_empty()
        {
            return;
        }
        let Some(profile) = &self.services.database_profile else {
            return;
        };
        let changes: Vec<_> = tab.design.rows.iter().map(|r| r.change(cx)).collect();
        match sql::generate_sql(profile.protocol, &tab.table, &changes) {
            Ok(sql) => self.show_generated_sql(sql, window, cx),
            Err(error) => {
                self.status = error.into();
                self.error = true;
                cx.notify();
            }
        }
    }
}
