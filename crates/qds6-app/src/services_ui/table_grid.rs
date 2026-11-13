//! 功能：多表独立草稿、原生单元格编辑与分页工具栏；时间：2026-11-11；作者：lq。
use super::*;
use gpui_kit::component::scroll::{Scrollbar, ScrollbarMode};
use gpui_kit::component::{
    menu::{DropdownMenu, PopupMenuItem},
    table::TableEvent,
};
use qds6_services::database::{BrowseRequest, Cell, PAGE_SIZE, RowChange, TablePage};
use std::collections::BTreeSet;
// 功能：筛选、设计器按表标签独立保存；时间：2026-11-12；作者：lq。
mod design;
mod filters;

/// 功能：原记录和变更分离，未填写的新行字段保留数据库默认值；时间：2026-11-11；作者：lq。
#[derive(Clone)]
struct DraftRow {
    original: Option<Vec<Cell>>,
    values: Vec<Option<Cell>>,
    deleted: bool,
}

/// 功能：每个标签拥有独立网格、筛选输入、字段可见性与详情开关；时间：2026-11-11；作者：lq。
pub(super) struct TableTab {
    pub table: Table,
    pub grid: Entity<TableState<TableRows>>,
    pub request: BrowseRequest,
    filters: Vec<filters::FilterDraft>,
    order: Vec<(usize, bool)>,
    match_any: bool,
    filter_expanded: bool,
    design: design::TableDesign,
    pub designing: bool,
    pub details: bool,
    pub fields: bool,
    _subscriptions: Vec<Subscription>,
}

impl TableTab {
    /// 功能：数据与结构草稿统一参与关闭确认和标签提示；时间：2026-11-12；作者：lq。
    pub(super) fn has_changes(&self, cx: &App) -> bool {
        !self.grid.read(cx).delegate().changes().is_empty() || self.design.dirty(cx)
    }
}

/// 功能：只对真实基础表启用写入，虚拟网格维持稳定行索引；时间：2026-11-11；作者：lq。
pub(super) struct TableRows {
    pub page: TablePage,
    rows: Vec<DraftRow>,
    hidden: BTreeSet<usize>,
    editing: Option<(usize, usize, Entity<InputState>, Option<Cell>)>,
    input_subscription: Option<Subscription>,
    pub locked: bool,
}

impl TableRows {
    /// 功能：从服务端快照建立可撤销草稿；时间：2026-11-11；作者：lq。
    fn new(page: TablePage) -> Self {
        let rows = page
            .data
            .rows
            .iter()
            .map(|row| DraftRow {
                original: Some(row.clone()),
                values: vec![None; row.len()],
                deleted: false,
            })
            .collect();
        Self {
            page,
            rows,
            hidden: BTreeSet::new(),
            editing: None,
            input_subscription: None,
            locked: false,
        }
    }
    /// 功能：导出有变化的行作为单次原子保存计划；时间：2026-11-11；作者：lq。
    pub fn changes(&self) -> Vec<RowChange> {
        self.rows
            .iter()
            .filter(|r| r.deleted || r.original.is_none() || r.values.iter().any(Option::is_some))
            .map(|r| RowChange {
                original: r.original.clone(),
                values: if r.deleted {
                    vec![None; r.values.len()]
                } else {
                    r.values.clone()
                },
                delete: r.deleted,
            })
            .collect()
    }
    /// 功能：映射隐藏字段后的可见列到真实字段；时间：2026-11-11；作者：lq。
    fn column_index(&self, index: usize) -> usize {
        (0..self.page.columns.len())
            .filter(|i| !self.hidden.contains(i))
            .nth(index)
            .unwrap_or(0)
    }
    /// 功能：区分 DEFAULT、NULL 和普通文本；时间：2026-11-11；作者：lq。
    fn value(&self, row: usize, col: usize) -> Option<Cell> {
        self.rows[row].values[col]
            .clone()
            .or_else(|| self.rows[row].original.as_ref().map(|r| r[col].clone()))
    }
    /// 功能：恢复原值时清除脏标记，新增行保留显式赋值；时间：2026-11-11；作者：lq。
    fn set_value(&mut self, row: usize, col: usize, value: Option<Cell>) {
        self.rows[row].values[col] = if self.rows[row]
            .original
            .as_ref()
            .is_some_and(|r| value.as_ref() == Some(&r[col]))
        {
            None
        } else {
            value
        };
    }
}

impl TableDelegate for TableRows {
    fn columns_count(&self, _: &App) -> usize {
        self.page.columns.len() - self.hidden.len()
    }
    fn rows_count(&self, _: &App) -> usize {
        self.rows.len()
    }
    fn column(&self, col: usize, _: &App) -> Column {
        let c = &self.page.columns[self.column_index(col)];
        Column::new(format!("table-col-{col}"), c.name.clone())
            .width(px(180.))
            .min_width(px(70.))
    }
    /// 功能：三行表头展示名称、类型及真实备注，截断文本可悬停查看；时间：2026-11-12；作者：lq。
    fn render_th(
        &mut self,
        col: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let c = &self.page.columns[self.column_index(col)];
        let tooltip = format!("{} · {}\n{}", c.name, c.data_type, c.comment);
        v_flex()
            .id(SharedString::from(format!(
                "grid-header-{}",
                self.column_index(col)
            )))
            .test_support()
            .w_full()
            .min_w_0()
            .overflow_hidden()
            .text_xs()
            .child(div().text_ellipsis().font_semibold().child(format!(
                "{}{}",
                if c.primary { "PK · " } else { "" },
                c.name
            )))
            .child(
                div()
                    .text_ellipsis()
                    .text_color(cx.theme().muted_foreground)
                    .child(c.data_type.clone()),
            )
            .child(
                div()
                    .text_ellipsis()
                    .text_color(cx.theme().muted_foreground)
                    .child(if c.comment.is_empty() {
                        "—".into()
                    } else {
                        c.comment.clone()
                    }),
            )
            .tooltip(move |window, cx| {
                gpui_kit::component::tooltip::Tooltip::new(tooltip.clone()).build(window, cx)
            })
    }
    /// 功能：编辑器直接嵌入结果单元格，修改和删除行保留可视标记；时间：2026-11-11；作者：lq。
    fn render_td(
        &mut self,
        row: usize,
        col: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let col = self.column_index(col);
        let edited = self.rows[row].values[col].is_some() || self.rows[row].original.is_none();
        let deleted = self.rows[row].deleted;
        // 功能：长文本与 JSON 可悬停查看完整内容，编辑入口仍为双击；时间：2026-11-12；作者：lq。
        let tooltip = self
            .value(row, col)
            .map(|v| v.display())
            .unwrap_or_else(|| "DEFAULT".into());
        div()
            .id(SharedString::from(format!("grid-cell-{row}-{col}")))
            .test_support()
            .size_full()
            .overflow_hidden()
            .tooltip(move |window, cx| {
                gpui_kit::component::tooltip::Tooltip::new(tooltip.clone()).build(window, cx)
            })
            .when(deleted, |v| v.opacity(0.4))
            .when(edited, |v| v.bg(cx.theme().secondary))
            .on_key_down(cx.listener(|state, event: &KeyDownEvent, window, cx| {
                if matches!(event.keystroke.key.as_str(), "escape" | "enter" | "tab") {
                    let data = state.delegate_mut();
                    if let Some((row, col, _, previous)) = data.editing.take() {
                        if event.keystroke.key == "escape" {
                            data.rows[row].values[col] = previous;
                        }
                        cx.stop_propagation();
                        state.focus_handle(cx).focus(window, cx);
                        cx.notify();
                    }
                }
            }))
            .child(
                if let Some((r, c, input, _)) = &self.editing
                    && *r == row
                    && *c == col
                {
                    Input::new(input)
                        .small()
                        .disabled(self.locked)
                        .into_any_element()
                } else {
                    div()
                        .text_xs()
                        .text_ellipsis()
                        .child(format!(
                            "{}{}",
                            if deleted {
                                "删除 · "
                            } else if edited {
                                "• "
                            } else {
                                ""
                            },
                            self.value(row, col)
                                .map(|v| v.display())
                                .unwrap_or_else(|| "DEFAULT".into())
                        ))
                        .into_any_element()
                },
            )
    }
    fn cell_label(&self, row: usize, col: usize, _: &App) -> Option<SharedString> {
        self.value(row, self.column_index(col))
            .map(|v| v.display().into())
    }
}

impl Workbench {
    /// 功能：双击进入当前网格单元格，Enter/Tab 收起编辑器，Escape 撤回本次单元格输入；时间：2026-11-11；作者：lq。
    fn edit_grid_cell(
        grid: &Entity<TableState<TableRows>>,
        row: usize,
        visible_col: usize,
        window: &mut Window,
        cx: &mut App,
    ) {
        grid.update(cx, |state, cx| {
            let data = state.delegate_mut();
            let col = data.column_index(visible_col);
            if data.locked
                || !data.page.editable
                || !data.page.columns[col].writable
                || data.rows[row].deleted
            {
                return;
            }
            let previous = data.rows[row].values[col].clone();
            let value = data
                .value(row, col)
                .map(|v| match v {
                    Cell::Null => String::new(),
                    _ => v.display(),
                })
                .unwrap_or_default();
            let input = cx.new(|cx| InputState::new(window, cx).default_value(value));
            data.input_subscription = Some(cx.subscribe_in(
                &input,
                window,
                move |state, input, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::Change) {
                        let value = input.read(cx).value().to_string();
                        state
                            .delegate_mut()
                            .set_value(row, col, Some(Cell::Text(value)));
                        cx.notify();
                    } else if matches!(event, InputEvent::PressEnter { .. }) {
                        state.delegate_mut().editing = None;
                        state.focus_handle(cx).focus(window, cx);
                        cx.notify();
                    }
                },
            ));
            data.editing = Some((row, col, input.clone(), previous));
            input.update(cx, |input, cx| input.focus(window, cx));
            cx.notify();
        });
    }

    /// 功能：表标签与对象列表选中同一对象，避免详情操作指向其他表；时间：2026-11-11；作者：lq。
    pub(super) fn activate_table_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        let table = &self.services.table_tabs[index].table;
        self.services.selected_table = self
            .services
            .tables
            .iter()
            .position(|t| t.schema == table.schema && t.name == table.name);
        self.services.active_table_tab = Some(index);
        self.services.query_active = false;
        cx.notify();
    }

    /// 功能：已打开表再次双击切换原标签，保留各表草稿；时间：2026-11-11；作者：lq。
    pub(super) fn open_table(
        &mut self,
        table: Table,
        structure: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.services.busy {
            return;
        }
        if let Some(index) = self
            .services
            .table_tabs
            .iter()
            .position(|tab| tab.table.schema == table.schema && tab.table.name == table.name)
        {
            self.services.active_table_tab = Some(index);
            if structure {
                self.services.table_tabs[index].designing = true;
            }
            self.services.query_active = false;
            cx.notify();
            return;
        }
        let Some(database) = self.services.database.clone() else {
            return;
        };
        let target = table.clone();
        self.service_job(
            "正在打开表…",
            move || database.browse_page(&target, &BrowseRequest::default()),
            move |this, page, window, cx| {
                let design = design::TableDesign::new(&page.columns, window, cx);
                let grid = cx.new(|cx| {
                    TableState::new(TableRows::new(page), window, cx).cell_selectable(true)
                });
                let events = cx.subscribe_in(
                    &grid,
                    window,
                    |this, grid, event: &TableEvent, window, cx| {
                        if let TableEvent::DoubleClickedCell(row, col) = event {
                            // 功能：结构草稿尚未执行时禁止混入数据编辑，避免保存后刷新覆盖结构草稿；时间：2026-11-12；作者：lq。
                            if this
                                .services
                                .table_tabs
                                .iter()
                                .any(|tab| &tab.grid == grid && tab.design.dirty(cx))
                            {
                                this.status = "请先保存或撤销结构修改，再编辑数据".into();
                                cx.notify();
                                return;
                            }
                            Self::edit_grid_cell(grid, *row, *col, window, cx);
                        }
                    },
                );
                let observe = cx.observe(&grid, |_, _, cx| cx.notify());
                let filter = filters::FilterDraft::new(window, cx);
                this.services.table_tabs.push(TableTab {
                    table,
                    grid,
                    request: BrowseRequest::default(),
                    filters: vec![filter],
                    order: Vec::new(),
                    match_any: false,
                    filter_expanded: false,
                    design,
                    designing: structure,
                    details: false,
                    fields: false,
                    _subscriptions: vec![events, observe],
                });

                this.services.active_table_tab = Some(this.services.table_tabs.len() - 1);
                this.services.query_active = false;
                this.status = "双击单元格编辑 · 底部保存或取消修改".into();
            },
            window,
            cx,
        );
    }

    /// 功能：分页和筛选刷新原网格，禁止覆盖未保存变更；时间：2026-11-11；作者：lq。
    fn refresh_table_tab(
        &mut self,
        request: BrowseRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.services.active_table_tab else {
            return;
        };
        let tab = &self.services.table_tabs[index];
        if tab.has_changes(cx) {
            self.status = "请先保存或撤销当前表的数据与结构修改，再刷新、筛选或翻页".into();
            cx.notify();
            return;
        }
        let Some(database) = self.services.database.clone() else {
            return;
        };
        let table = tab.table.clone();
        let query = request.clone();
        self.service_job(
            "正在读取表数据…",
            move || database.browse_page(&table, &query),
            move |this, page, window, cx| {
                let design = design::TableDesign::new(&page.columns, window, cx);
                let tab = &mut this.services.table_tabs[index];
                tab.design = design;
                tab.request = request;
                // 功能：结构变更后按字段名重新映射筛选、排序及隐藏列，避免错位或越界；时间：2026-11-12；作者：lq。
                let previous = &tab.grid.read(cx).delegate().page.columns;
                for filter in &mut tab.filters {
                    filter.column = previous
                        .get(filter.column)
                        .and_then(|old| page.columns.iter().position(|c| c.name == old.name))
                        .unwrap_or(0);
                }
                tab.order = tab
                    .order
                    .iter()
                    .filter_map(|(i, desc)| {
                        previous
                            .get(*i)
                            .and_then(|old| page.columns.iter().position(|c| c.name == old.name))
                            .map(|i| (i, *desc))
                    })
                    .collect();
                tab.grid.update(cx, |state, cx| {
                    let hidden: Vec<_> = state
                        .delegate()
                        .hidden
                        .iter()
                        .filter_map(|i| state.delegate().page.columns.get(*i))
                        .map(|c| c.name.clone())
                        .collect();
                    *state.delegate_mut() = TableRows::new(page);
                    state.delegate_mut().hidden = state
                        .delegate()
                        .page
                        .columns
                        .iter()
                        .enumerate()
                        .filter_map(|(i, c)| hidden.contains(&c.name).then_some(i))
                        .collect();
                    if state.delegate().hidden.len() == state.delegate().page.columns.len() {
                        state.delegate_mut().hidden.clear();
                    }
                    state.clear_selection(cx);
                    state.refresh(cx);
                });
                this.status = "表数据已刷新".into();
            },
            window,
            cx,
        );
    }

    /// 功能：保存固定的表实体和变更快照，失败保留全部草稿；时间：2026-11-11；作者：lq。
    fn save_table_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.services.busy {
            return;
        }
        let Some(index) = self.services.active_table_tab else {
            return;
        };
        let tab = &self.services.table_tabs[index];
        let grid = tab.grid.clone();
        let changes = grid.read(cx).delegate().changes();
        if changes.is_empty() {
            return;
        }
        let Some(database) = self.services.database.clone() else {
            return;
        };
        let table = tab.table.clone();
        let columns = grid.read(cx).delegate().page.columns.clone();
        grid.update(cx, |s, cx| {
            s.delegate_mut().locked = true;
            s.delegate_mut().editing = None;
            cx.notify();
        });
        // 功能：保存成功先更新本地快照，再单独刷新，避免刷新失败导致重复插入；时间：2026-11-11；作者：lq。
        self.service_job(
            "正在保存表数据…",
            move || database.save_rows(&table, &columns, &changes),
            move |this, _, window, cx| {
                grid.update(cx, |s, cx| {
                    let data = s.delegate_mut();
                    let columns = data.page.columns.clone();
                    data.rows.retain(|r| !r.deleted);
                    for row in &mut data.rows {
                        if let Some(values) = &mut row.original {
                            for (i, value) in row.values.iter_mut().enumerate() {
                                if let Some(v) = value.take() {
                                    values[i] = v;
                                }
                            }
                        } else {
                            // 新增记录的默认值由刷新取得，清除草稿防止重试写入。
                            row.original = Some(
                                columns
                                    .iter()
                                    .enumerate()
                                    .map(|(i, _)| row.values[i].take().unwrap_or(Cell::Null))
                                    .collect(),
                            );
                        }
                    }
                    data.page.data.rows = data
                        .rows
                        .iter()
                        .filter_map(|r| r.original.clone())
                        .collect();
                    data.locked = false;
                    cx.notify();
                });
                let active = this.services.active_table_tab;
                this.services.active_table_tab = Some(index);
                this.refresh_table_tab(this.services.table_tabs[index].request.clone(), window, cx);
                this.services.active_table_tab = active;
                this.status = "数据已保存，正在刷新服务端默认值".into();
            },
            window,
            cx,
        );
    }

    /// 功能：只在有未保存修改时确认关闭表标签；时间：2026-11-11；作者：lq。
    pub(super) fn close_table_tab(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.services.busy {
            return;
        }
        let grid = self.services.table_tabs[index].grid.clone();
        if !self.services.table_tabs[index].has_changes(cx) {
            self.remove_table_tab(index, cx);
            return;
        }
        let answer = window.prompt(
            PromptLevel::Warning,
            "关闭表并丢弃未保存修改？",
            Some(&self.services.table_tabs[index].table.name),
            &["取消", "丢弃并关闭"],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            if answer.await == Ok(1) {
                let _ = this.update_in(cx, |this, _, cx| {
                    if !this.services.busy
                        && let Some(index) =
                            this.services.table_tabs.iter().position(|t| t.grid == grid)
                    {
                        this.remove_table_tab(index, cx);
                    }
                });
            }
        })
        .detach();
    }
    /// 功能：按点击时标签身份批量关闭，统一确认脏草稿，取消时全部保留；时间：2026-11-12；作者：lq。
    pub(super) fn close_table_tabs(
        &mut self,
        anchor: &Entity<TableState<TableRows>>,
        mode: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.services.busy {
            return;
        }
        let Some(index) = self
            .services
            .table_tabs
            .iter()
            .position(|t| &t.grid == anchor)
        else {
            return;
        };
        let targets: Vec<_> = self
            .services
            .table_tabs
            .iter()
            .enumerate()
            .filter(|(i, _)| match mode {
                0 => *i == index,
                1 => *i != index,
                2 => *i > index,
                _ => true,
            })
            .map(|(_, t)| t.grid.clone())
            .collect();
        let dirty = self
            .services
            .table_tabs
            .iter()
            .filter(|t| targets.contains(&t.grid) && t.has_changes(cx))
            .count();
        if dirty == 0 {
            self.remove_table_tabs(&targets, cx);
            return;
        }
        let answer = window.prompt(
            PromptLevel::Warning,
            "关闭选项卡并丢弃未保存修改？",
            Some(&format!(
                "将关闭 {} 个表标签，其中 {} 个有未保存修改。",
                targets.len(),
                dirty
            )),
            &["取消", "丢弃并关闭"],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            if answer.await == Ok(1) {
                let _ = this.update_in(cx, |this, _, cx| {
                    if !this.services.busy {
                        this.remove_table_tabs(&targets, cx);
                    }
                });
            }
        })
        .detach();
    }
    /// 功能：倒序移除原标签实体，避免关闭过程中索引偏移；时间：2026-11-12；作者：lq。
    fn remove_table_tabs(
        &mut self,
        targets: &[Entity<TableState<TableRows>>],
        cx: &mut Context<Self>,
    ) {
        for i in (0..self.services.table_tabs.len()).rev() {
            if targets.contains(&self.services.table_tabs[i].grid) {
                self.remove_table_tab(i, cx);
            }
        }
        if let Some(active) = self.services.active_table_tab {
            let query_active = self.services.query_active;
            self.activate_table_tab(active, cx);
            self.services.query_active = query_active;
        }
    }

    /// 功能：关闭后保持相邻标签及对象页稳定；时间：2026-11-11；作者：lq。
    fn remove_table_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        self.services.table_tabs.remove(index);
        self.services.active_table_tab = self.services.active_table_tab.and_then(|active| {
            if self.services.table_tabs.is_empty() {
                None
            } else {
                Some(if active > index {
                    active - 1
                } else {
                    active.min(self.services.table_tabs.len() - 1)
                })
            }
        });
        if let Some(active) = self.services.active_table_tab {
            let table = &self.services.table_tabs[active].table;
            self.services.selected_table = self
                .services
                .tables
                .iter()
                .position(|t| t.schema == table.schema && t.name == table.name);
        }
        cx.notify();
    }

    /// 功能：数据页工具栏、字段选择、详情及底部写入操作同屏；时间：2026-11-11；作者：lq。
    pub(super) fn table_data_panel(
        &self,
        index: usize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        if self.services.table_tabs[index].designing {
            return self.table_design_panel(index, cx);
        }
        let toolbar = self.table_filter_toolbar(index, cx);
        let tab = &self.services.table_tabs[index];
        let grid = tab.grid.clone();
        let data = grid.read(cx).delegate();
        let dirty = !data.changes().is_empty();
        let selected = grid
            .read(cx)
            .selected_cell()
            .map(|(r, _)| r)
            .or_else(|| grid.read(cx).selected_row());
        let writable = data.page.editable;
        let locked = self.services.busy || tab.design.dirty(cx);
        let request = tab.request.clone();

        // 功能：网格受可用宽高约束，单独预留常显横向滚动条，表头与行共享滚动句柄；时间：2026-11-12；作者：lq。
        let content = h_flex()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .overflow_hidden()
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .overflow_hidden()
                    .child(
                        div()
                            .id("grid-viewport")
                            .test_support()
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            .overflow_hidden()
                            .child(
                                DataTable::new(&grid)
                                    .small()
                                    .header_row_height(px(66.))
                                    .scrollbar_visible(true, false),
                            ),
                    )
                    .child(
                        div()
                            .id("grid-horizontal-scrollbar")
                            .test_support()
                            .relative()
                            .h(px(14.))
                            .flex_shrink_0()
                            .w_full()
                            .child(
                                Scrollbar::horizontal(&grid.read(cx).horizontal_scroll_handle)
                                    .viewport_from_layout()
                                    .mode(ScrollbarMode::Always),
                            ),
                    ),
            );
        let content = content.when(tab.details, |v| {
            v.child(
                v_flex()
                    .id("table-details")
                    .test_support()
                    .w(px(220.))
                    .h_full()
                    .min_h_0()
                    .p_3()
                    .gap_2()
                    .border_l_1()
                    .border_color(cx.theme().border)
                    .overflow_y_scroll()
                    .child(div().font_semibold().child(tab.table.name.clone()))
                    .child(div().text_xs().child(format!(
                        "{}\n{}.{}\n{}\n本页 {} 行\n{}",
                        self.services.database_identity,
                        tab.table.schema,
                        tab.table.name,
                        tab.table.kind,
                        data.rows.len(),
                        if writable {
                            "可编辑 · 主键自动识别"
                        } else {
                            "只读 · 视图或无可写主键"
                        }
                    )))
                    .child(div().text_xs().font_semibold().pt_2().child("索引"))
                    .children(data.page.indexes.rows.iter().map(|row| {
                        div().text_xs().py_1().child(
                            row.iter()
                                .map(Cell::display)
                                .collect::<Vec<_>>()
                                .join(" · "),
                        )
                    }))
                    .when(data.page.indexes.rows.is_empty(), |v| {
                        v.child(div().text_xs().child("无可见索引"))
                    })
                    .children(data.page.columns.iter().map(|c| {
                        v_flex()
                            .gap_1()
                            .py_2()
                            .border_b_1()
                            .border_color(cx.theme().border)
                            .child(div().text_xs().font_semibold().child(format!(
                                "{}{}",
                                if c.primary { "主键 · " } else { "" },
                                c.name
                            )))
                            .child(div().text_xs().child(format!(
                                "{} · {}{}\n默认：{}",
                                c.data_type,
                                if c.nullable { "可空" } else { "非空" },
                                if c.generated { " · 生成列" } else { "" },
                                c.default.as_deref().unwrap_or("无")
                            )))
                            .child(div().text_xs().child(c.comment.clone()))
                    })),
            )
        });
        let bottom = h_flex()
            .id("grid-bottom-toolbar")
            .test_support()
            .p_2()
            .gap_1()
            .items_center()
            .flex_wrap()
            .border_t_1()
            .border_color(cx.theme().border)
            .child(
                Button::new("grid-add")
                    .small()
                    .ghost()
                    .icon(UiIcon::Plus)
                    .tooltip("新增记录")
                    .disabled(locked || !writable)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.services.table_tabs[index].grid.update(cx, |s, cx| {
                            let d = s.delegate_mut();
                            d.rows.push(DraftRow {
                                original: None,
                                values: vec![None; d.page.columns.len()],
                                deleted: false,
                            });
                            let row = d.rows.len() - 1;
                            s.set_selected_row(row, cx);
                            cx.notify();
                        });
                    })),
            )
            .child(
                Button::new("grid-delete")
                    .small()
                    .ghost()
                    .icon(UiIcon::Trash)
                    .tooltip("删除 / 恢复选中行")
                    .disabled(locked || !writable || selected.is_none())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.services.table_tabs[index].grid.update(cx, |s, cx| {
                            let row = s
                                .selected_cell()
                                .map(|(r, _)| r)
                                .or_else(|| s.selected_row());
                            if let Some(row) = row {
                                let d = s.delegate_mut();
                                d.editing = None;
                                if d.rows[row].original.is_none() {
                                    d.rows.remove(row);
                                    s.clear_selection(cx);
                                } else {
                                    d.rows[row].deleted = !d.rows[row].deleted;
                                }
                            }
                            cx.notify();
                        });
                    })),
            )
            .child(
                Button::new("grid-save")
                    .small()
                    .ghost()
                    .icon(UiIcon::Check)
                    .label("保存")
                    .disabled(locked || !dirty)
                    .on_click(cx.listener(|this, _, window, cx| this.save_table_tab(window, cx))),
            )
            .child(
                Button::new("grid-revert")
                    .small()
                    .ghost()
                    .icon(UiIcon::X)
                    .tooltip("取消本表全部未保存修改")
                    .disabled(locked || !dirty)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.services.table_tabs[index].grid.update(cx, |s, cx| {
                            let d = s.delegate_mut();
                            let mut restored = TableRows::new(d.page.clone());
                            restored.hidden = d.hidden.clone();
                            *d = restored;
                            s.clear_selection(cx);
                            s.refresh(cx);
                        });
                    })),
            )
            .child(
                Button::new("grid-null")
                    .small()
                    .ghost()
                    .label("NULL")
                    .disabled(locked || !writable)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.set_grid_special(index, false, cx);
                    })),
            )
            .child(
                Button::new("grid-default")
                    .small()
                    .ghost()
                    .label("DEFAULT")
                    .disabled(locked || !writable)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.set_grid_special(index, true, cx);
                    })),
            )
            .child(
                Button::new("grid-refresh")
                    .small()
                    .ghost()
                    .icon(UiIcon::RefreshCw)
                    .disabled(locked || dirty)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.refresh_table_tab(request.clone(), window, cx)
                    })),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_xs()
                    .text_ellipsis()
                    .child(format!(
                        "{} 行 · {} 项待保存{}",
                        data.rows.len(),
                        data.changes().len(),
                        if writable { "" } else { " · 只读" }
                    )),
            )
            .child(
                Button::new("grid-prev")
                    .small()
                    .ghost()
                    .label("上一页")
                    .disabled(locked || dirty || tab.request.page == 0)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        let mut r = this.services.table_tabs[index].request.clone();
                        r.page = r.page.saturating_sub(1);
                        this.refresh_table_tab(r, window, cx);
                    })),
            )
            .child(div().text_xs().child(format!(
                "第 {} 页 · 每页 {}",
                tab.request.page + 1,
                PAGE_SIZE
            )))
            .child(
                Button::new("grid-next")
                    .small()
                    .ghost()
                    .label("下一页")
                    .disabled(locked || dirty || !data.page.has_next)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        let mut r = this.services.table_tabs[index].request.clone();
                        r.page += 1;
                        this.refresh_table_tab(r, window, cx);
                    })),
            );
        v_flex()
            .size_full()
            .min_w_0()
            .min_h_0()
            .overflow_hidden()
            .child(toolbar)
            .when(tab.fields, |v| {
                v.child(h_flex().p_2().gap_3().flex_wrap().children(
                    data.page.columns.iter().enumerate().map(|(col, c)| {
                        Checkbox::new(("visible-field", col))
                            .label(c.name.clone())
                            .checked(!data.hidden.contains(&col))
                            .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                                this.services.table_tabs[index].grid.update(cx, |s, cx| {
                                    let d = s.delegate_mut();
                                    d.editing = None;
                                    if *checked {
                                        d.hidden.remove(&col);
                                    } else if d.hidden.len() + 1 < d.page.columns.len() {
                                        d.hidden.insert(col);
                                    }
                                    s.clear_selection(cx);
                                    s.refresh(cx);
                                });
                            }))
                    }),
                ))
            })
            .child(content)
            .child(bottom)
            .into_any_element()
    }

    /// 功能：选中单元格显式设置 NULL，新行可恢复 DEFAULT；时间：2026-11-11；作者：lq。
    fn set_grid_special(&mut self, index: usize, default: bool, cx: &mut Context<Self>) {
        self.services.table_tabs[index].grid.update(cx, |s, cx| {
            if let Some((row, visible)) = s.selected_cell() {
                let d = s.delegate_mut();
                let col = d.column_index(visible);
                if d.locked
                    || !d.page.editable
                    || !d.page.columns[col].writable
                    || d.rows[row].deleted
                {
                    return;
                }
                if default && d.rows[row].original.is_some() {
                    return;
                }
                if !default && !d.page.columns[col].nullable {
                    return;
                }
                d.editing = None;
                d.set_value(row, col, if default { None } else { Some(Cell::Null) });
                cx.notify();
            }
        });
    }
}

// 功能：原生数据库工作区验收仅在显式测试构建启用；时间：2026-11-11；作者：lq。
#[cfg(feature = "ui-tests")]
pub(super) mod smoke;
