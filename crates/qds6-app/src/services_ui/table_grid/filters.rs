//! 功能：按表保存多条件筛选与多列排序，回车应用，修改条件不自动发起网络请求；时间：2026-11-12；作者：lq。
use super::*;
use qds6_services::database::{FilterOp, GridFilter};

/// 功能：筛选输入与已应用请求分离，NULL 与空字符串互不混淆；时间：2026-11-12；作者：lq。
pub(super) struct FilterDraft {
    pub column: usize,
    pub op: FilterOp,
    pub value: Entity<InputState>,
    _subscription: Subscription,
}
impl FilterDraft {
    /// 功能：创建支持回车应用的原生筛选输入；时间：2026-11-12；作者：lq。
    pub fn new(window: &mut Window, cx: &mut Context<Workbench>) -> Self {
        let value = cx.new(|cx| InputState::new(window, cx).placeholder("筛选值（支持空字符串）"));
        let subscription = cx.subscribe_in(&value, window, |this, _, event, window, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                this.apply_table_filters(window, cx);
            }
        });
        Self {
            column: 0,
            op: FilterOp::Contains,
            value,
            _subscription: subscription,
        }
    }
}
impl Workbench {
    /// 功能：应用草稿条件时重置页码，服务端执行原生类型比较；时间：2026-11-12；作者：lq。
    fn apply_table_filters(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.services.busy {
            return;
        }
        let Some(index) = self.services.active_table_tab else {
            return;
        };
        let tab = &self.services.table_tabs[index];
        let columns = &tab.grid.read(cx).delegate().page.columns;
        let request = BrowseRequest {
            page: 0,
            filters: tab
                .filters
                .iter()
                .filter(|f| f.op != FilterOp::Contains || !f.value.read(cx).value().is_empty())
                .map(|f| GridFilter {
                    column: columns[f.column].name.clone(),
                    op: f.op,
                    value: f.value.read(cx).value().to_string(),
                })
                .collect(),
            match_any: tab.match_any,
            order: tab
                .order
                .iter()
                .map(|(col, desc)| (columns[*col].name.clone(), *desc))
                .collect(),
        };
        self.refresh_table_tab(request, window, cx);
    }

    /// 功能：默认紧凑筛选，展开后编辑 AND/OR、多条件和多字段排序；时间：2026-11-12；作者：lq。
    pub(super) fn table_filter_toolbar(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let tab = &self.services.table_tabs[index];
        let dirty = !tab.grid.read(cx).delegate().changes().is_empty();
        let locked = self.services.busy;
        let view = cx.entity();
        let columns = tab.grid.read(cx).delegate().page.columns.clone();
        let header = h_flex()
            .flex_shrink_0()
            .p_2()
            .gap_2()
            .items_center()
            .flex_wrap()
            .child(
                Button::new("grid-toggle-filter")
                    .small()
                    .ghost()
                    .label(if tab.filter_expanded {
                        "收起筛选"
                    } else {
                        "筛选与排序"
                    })
                    .selected(tab.filter_expanded)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.services.table_tabs[index].filter_expanded =
                            !this.services.table_tabs[index].filter_expanded;
                        cx.notify();
                    })),
            )
            .child(
                Button::new("grid-apply-filter")
                    .small()
                    .primary()
                    .label("应用")
                    .disabled(locked || dirty)
                    .on_click(
                        cx.listener(|this, _, window, cx| this.apply_table_filters(window, cx)),
                    ),
            )
            .child(
                Button::new("grid-clear-filter")
                    .small()
                    .ghost()
                    .label("重置")
                    .disabled(locked || dirty)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        let filter = FilterDraft::new(window, cx);
                        let tab = &mut this.services.table_tabs[index];
                        tab.filters = vec![filter];
                        tab.order.clear();
                        tab.match_any = false;
                        this.refresh_table_tab(BrowseRequest::default(), window, cx);
                    })),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!(
                        "已应用 {} 条筛选 · {} 项排序",
                        tab.request.filters.len(),
                        tab.request.order.len()
                    )),
            )
            .child(div().flex_1())
            .child(
                Button::new("grid-fields")
                    .small()
                    .ghost()
                    .label("字段显示")
                    .selected(tab.fields)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.services.table_tabs[index].fields =
                            !this.services.table_tabs[index].fields;
                        cx.notify();
                    })),
            )
            .child(
                Button::new("grid-design")
                    .small()
                    .ghost()
                    .label("设计表")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.services.table_tabs[index].designing = true;
                        cx.notify();
                    })),
            )
            .child(
                Button::new("grid-details")
                    .small()
                    .ghost()
                    .label("表详情")
                    .selected(tab.details)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.services.table_tabs[index].details =
                            !this.services.table_tabs[index].details;
                        cx.notify();
                    })),
            );
        let rows =
            tab.filters
                .iter()
                .enumerate()
                .take(if tab.filter_expanded { usize::MAX } else { 1 })
                .map(|(rule, filter)| {
                    let field_view = view.clone();
                    let op_view = view.clone();
                    let fields = columns.clone();
                    h_flex()
                        .px_2()
                        .pb_2()
                        .gap_2()
                        .items_center()
                        .flex_wrap()
                        .child(
                            Button::new(("grid-filter-column", rule))
                                .small()
                                .label(columns[filter.column].name.clone())
                                .disabled(locked)
                                .dropdown_menu(move |mut menu, _, _| {
                                    for (col, c) in fields.iter().enumerate() {
                                        let view = field_view.clone();
                                        menu = menu.item(
                                            PopupMenuItem::new(format!(
                                                "{}{}",
                                                c.name,
                                                if c.comment.is_empty() {
                                                    String::new()
                                                } else {
                                                    format!(" · {}", c.comment)
                                                }
                                            ))
                                            .on_click(move |_, _, cx| {
                                                view.update(cx, |this, cx| {
                                                    this.services.table_tabs[index].filters[rule]
                                                        .column = col;
                                                    cx.notify();
                                                });
                                            }),
                                        );
                                    }
                                    menu
                                }),
                        )
                        .child(
                            Button::new(("grid-filter-op", rule))
                                .small()
                                .label(filter.op.label())
                                .disabled(locked)
                                .dropdown_menu(move |mut menu, _, _| {
                                    for op in FilterOp::ALL {
                                        let view = op_view.clone();
                                        menu = menu.item(PopupMenuItem::new(op.label()).on_click(
                                            move |_, _, cx| {
                                                view.update(cx, |this, cx| {
                                                    this.services.table_tabs[index].filters[rule]
                                                        .op = op;
                                                    cx.notify();
                                                });
                                            },
                                        ));
                                    }
                                    menu
                                }),
                        )
                        .child(
                            div()
                                .id(("grid-filter-input", rule))
                                .test_support()
                                .w(px(220.))
                                .child(
                                    Input::new(&filter.value)
                                        .small()
                                        .disabled(locked || !filter.op.needs_value()),
                                ),
                        )
                        .when(tab.filter_expanded, |row| {
                            row.child(
                                Button::new(("grid-remove-filter", rule))
                                    .small()
                                    .ghost()
                                    .icon(UiIcon::X)
                                    .disabled(locked)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.services.table_tabs[index].filters.remove(rule);
                                        cx.notify();
                                    })),
                            )
                        })
                });
        let extra = v_flex()
            .px_2()
            .pb_2()
            .gap_2()
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        Button::new("grid-add-filter")
                            .small()
                            .label("添加条件")
                            .disabled(locked || tab.filters.len() >= 32)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                let filter = FilterDraft::new(window, cx);
                                this.services.table_tabs[index].filters.push(filter);
                                cx.notify();
                            })),
                    )
                    .child(
                        Checkbox::new("grid-filter-any")
                            .label("满足任一条件（OR）")
                            .checked(tab.match_any)
                            .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                                this.services.table_tabs[index].match_any = *checked;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("grid-add-sort")
                            .small()
                            .label("添加排序")
                            .disabled(locked || tab.order.len() >= columns.len().min(16))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let tab = &mut this.services.table_tabs[index];
                                let count = tab.grid.read(cx).delegate().page.columns.len();
                                if let Some(col) =
                                    (0..count).find(|col| !tab.order.iter().any(|(c, _)| c == col))
                                {
                                    tab.order.push((col, false));
                                }
                                cx.notify();
                            })),
                    ),
            )
            .children(tab.order.iter().enumerate().map(|(rule, (col, desc))| {
                let view = view.clone();
                let fields = columns.clone();
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(div().text_xs().child(format!("排序 {}", rule + 1)))
                    .child(
                        Button::new(("grid-sort-field", rule))
                            .small()
                            .label(columns[*col].name.clone())
                            .disabled(locked)
                            .dropdown_menu(move |mut menu, _, _| {
                                for (col, c) in fields.iter().enumerate() {
                                    let view = view.clone();
                                    menu = menu.item(PopupMenuItem::new(c.name.clone()).on_click(
                                        move |_, _, cx| {
                                            view.update(cx, |this, cx| {
                                                let tab = &mut this.services.table_tabs[index];
                                                if !tab
                                                    .order
                                                    .iter()
                                                    .enumerate()
                                                    .any(|(i, (c, _))| i != rule && *c == col)
                                                {
                                                    tab.order[rule].0 = col;
                                                }
                                                cx.notify();
                                            });
                                        },
                                    ));
                                }
                                menu
                            }),
                    )
                    .child(
                        Button::new(("grid-sort-direction", rule))
                            .small()
                            .label(if *desc { "降序 ↓" } else { "升序 ↑" })
                            .disabled(locked)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.services.table_tabs[index].order[rule].1 =
                                    !this.services.table_tabs[index].order[rule].1;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new(("grid-remove-sort", rule))
                            .small()
                            .ghost()
                            .icon(UiIcon::X)
                            .disabled(locked)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.services.table_tabs[index].order.remove(rule);
                                cx.notify();
                            })),
                    )
            }));
        v_flex()
            .flex_shrink_0()
            .max_h(px(260.))
            .min_w_0()
            .overflow_hidden()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(header)
            .child(
                v_flex()
                    .id("grid-filter-rules")
                    .min_h_0()
                    .overflow_y_scroll()
                    .children(rows)
                    .when(tab.filter_expanded, |v| v.child(extra)),
            )
            .into_any_element()
    }
}
