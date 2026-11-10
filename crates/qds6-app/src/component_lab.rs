//! 功能：隔离的 Kit 技术验证工作台，仅使用合成数据、不读写连接库或联网；时间：2026-11-10；作者：lq。

use super::*;
use gpui_kit::component::{
    dock::{
        BasePanel, DockArea, DockLayout, DockSkin, Panel, PanelControl, PanelEvent, panel_handle,
    },
    list::ListItem,
    menu::{DropdownMenu, PopupMenuItem},
    table::{Column, DataTable, TableDelegate, TableState},
    tree::{TreeItem, TreeState, tree},
};
use std::{cell::Cell, rc::Rc};

/// 功能：固定十万行二十列的数据源，不分配二维全量缓存；时间：2026-11-10；作者：lq。
const ROWS: usize = 100_000;
/// 功能：覆盖横向虚拟化的列数；时间：2026-11-10；作者：lq。
const COLS: usize = 20;

/// 功能：按索引生成可复现的虚拟单元格并统计实际绘制次数；时间：2026-11-10；作者：lq。
struct SampleRows {
    rendered: Rc<Cell<usize>>,
}

impl SampleRows {
    /// 功能：始终返回文本，包含中文和超出 JS 安全整数范围的示例；时间：2026-11-10；作者：lq。
    fn value(row: usize, col: usize) -> SharedString {
        match col {
            0 => (row + 1).to_string(),
            1 => format!("合成记录 {:06}", row + 1),
            2 => (9_007_199_254_740_993_u64 + row as u64).to_string(),
            3 => format!("{}.{:04}", row / 10, row % 10_000),
            _ => format!("r{} / c{}", row + 1, col + 1),
        }
        .into()
    }
}

/// 功能：Kit 原生虚拟表委托，只渲染请求范围；时间：2026-11-10；作者：lq。
impl TableDelegate for SampleRows {
    /// 功能：报告固定合成数据列数；时间：2026-11-10；作者：lq。
    fn columns_count(&self, _: &App) -> usize {
        COLS
    }

    /// 功能：报告合成数据总行数；时间：2026-11-10；作者：lq。
    fn rows_count(&self, _: &App) -> usize {
        ROWS
    }

    /// 功能：提供固定宽度及可调整列定义；时间：2026-11-10；作者：lq。
    fn column(&self, col: usize, _: &App) -> Column {
        let name = match col {
            0 => "序号".into(),
            1 => "中文名称".into(),
            2 => "大整数文本".into(),
            3 => "小数文本".into(),
            _ => format!("字段 {}", col + 1),
        };
        Column::new(format!("column-{col}"), name)
            .width(px(if col == 0 { 80. } else { 180. }))
            .min_width(px(64.))
    }

    /// 功能：使用当前主题前景色绘制表头，修正默认深色表头对比不足；时间：2026-11-10；作者：lq。
    fn render_th(
        &mut self,
        col: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        div()
            .text_color(cx.theme().foreground)
            .child(self.column(col, cx).name)
    }

    /// 功能：按需绘制单元格并累加绘制证据；时间：2026-11-10；作者：lq。
    fn render_td(
        &mut self,
        row: usize,
        col: usize,
        _: &mut Window,
        _: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        self.rendered.set(self.rendered.get() + 1);
        div()
            .id(SharedString::from(format!("cell-{row}-{col}")))
            .test_support()
            .child(Self::value(row, col))
    }

    /// 功能：为单元格提供可访问文本；时间：2026-11-10；作者：lq。
    fn cell_label(&self, row: usize, col: usize, _: &App) -> Option<SharedString> {
        Some(Self::value(row, col))
    }
}

/// 功能：异构面板复用同一停靠外壳，编辑状态由实体持有；时间：2026-11-10；作者：lq。
enum Content {
    Tree(Entity<TreeState>),
    Sql(Entity<EditorState>),
    Results(Entity<TableState<SampleRows>>),
    Notes,
}

/// 功能：Kit 停靠面板同时实现行为和外观两个接口；时间：2026-11-10；作者：lq。
struct LabPanel {
    name: &'static str,
    title: &'static str,
    focus: FocusHandle,
    content: Content,
}

/// 功能：稳定面板标识并允许关闭后通过重置布局恢复；时间：2026-11-10；作者：lq。
impl BasePanel for LabPanel {
    /// 功能：返回停靠系统使用的稳定面板名；时间：2026-11-10；作者：lq。
    fn panel_name(&self) -> &'static str {
        self.name
    }
}

/// 功能：绘制中文标题与放大按钮，内容区域不额外留白；时间：2026-11-10；作者：lq。
impl Panel for LabPanel {
    /// 功能：返回面板中文标题；时间：2026-11-10；作者：lq。
    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.title
    }

    /// 功能：由编辑器、表格和树自行管理内边距；时间：2026-11-10；作者：lq。
    fn inner_padding(&self, _: &App) -> bool {
        false
    }

    /// 功能：在工具栏提供面板放大和恢复；时间：2026-11-10；作者：lq。
    fn zoom_control(&self, _: &App) -> Option<PanelControl> {
        Some(PanelControl::Toolbar)
    }
}

/// 功能：让 Kit 管理面板生命周期事件；时间：2026-11-10；作者：lq。
impl EventEmitter<PanelEvent> for LabPanel {}

/// 功能：面板焦点进入实际树、编辑器或表格；时间：2026-11-10；作者：lq。
impl Focusable for LabPanel {
    /// 功能：按内容类型转交键盘焦点；时间：2026-11-10；作者：lq。
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        match &self.content {
            // 功能：当前 Kit 的 TreeState 只提供 focus 方法，不实现 Focusable；面板使用自己的焦点外壳；时间：2026-11-10；作者：lq。
            Content::Tree(_) => self.focus.clone(),
            Content::Sql(state) => state.focus_handle(cx),
            Content::Results(state) => state.focus_handle(cx),
            Content::Notes => self.focus.clone(),
        }
    }
}

/// 功能：验证真实 Kit 树、编辑器和虚拟表，所有测试数据明确标识为合成；时间：2026-11-10；作者：lq。
impl Render for LabPanel {
    /// 功能：绘制当前面板对应的原生组件；时间：2026-11-10；作者：lq。
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = match &self.content {
            Content::Tree(state) => tree(state, |ix, entry, _, _, _| {
                let item = entry.item();
                // 功能：避免与 Tree 自身的整数行 ID 冲突，保持测试及自动化定位唯一；时间：2026-11-10；作者：lq。
                ListItem::new(("lab-tree-item", ix))
                    .accessibility_label(item.label.clone())
                    .child(
                        h_flex()
                            .pl(px(entry.depth() as f32 * 16.))
                            .gap_2()
                            .child(if entry.is_folder() {
                                Icon::new(if entry.is_expanded() {
                                    UiIcon::ChevronDown
                                } else {
                                    UiIcon::ChevronRight
                                })
                                .xsmall()
                                .into_any_element()
                            } else {
                                div().w_3().into_any_element()
                            })
                            .child(
                                Icon::new(if entry.is_folder() {
                                    UiIcon::Folder
                                } else {
                                    UiIcon::Database
                                })
                                .small(),
                            )
                            .child(item.label.clone()),
                    )
            })
            .into_any_element(),
            Content::Sql(state) => Editor::new(state)
                .size_full()
                .bordered(false)
                .aria_label("组件验证 SQL 草稿")
                .into_any_element(),
            Content::Results(state) => DataTable::new(state)
                .small()
                .stripe(true)
                .scroll_bounce(false)
                .accessibility_label("十万行二十列合成数据")
                .into_any_element(),
            Content::Notes => v_flex()
                .p_5()
                .gap_3()
                .child("M0 · Kit 组件验证")
                .child("所有连接、对象和表格内容均为合成数据。")
                .child("拖动标签可调整顺序或移动到其他分组；拖动分隔线可调整尺寸。")
                .child("SQL 草稿只保存在当前窗口中，切换、拖动和重置布局均保留内容。")
                .child("此窗口不读取正式配置、不连接服务，也不执行 SQL。")
                .into_any_element(),
        };
        div()
            .id(self.name)
            .test_support()
            .track_focus(&self.focus)
            .size_full()
            .min_w_0()
            .min_h_0()
            .bg(cx.theme().background)
            .child(content)
    }
}

/// 功能：M0 实验窗口的实体与会话内布局，不持久保存；时间：2026-11-10；作者：lq。
struct ComponentLab {
    area: Entity<DockArea>,
    panels: Vec<Entity<LabPanel>>,
    table: Entity<TableState<SampleRows>>,
    tree: Entity<TreeState>,
    sql: Entity<EditorState>,
    focus: FocusHandle,
    _observations: Vec<Subscription>,
}

impl ComponentLab {
    /// 功能：创建可重复的树与十万行表格，构建真实停靠布局；时间：2026-11-10；作者：lq。
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let tree = cx.new(|cx| {
            TreeState::new(cx).items(vec![
                TreeItem::new("development", "开发环境 · 合成")
                    .child(
                        TreeItem::new("postgres", "PostgreSQL 示例")
                            .child(TreeItem::new("users", "public.users"))
                            .child(TreeItem::new("orders", "public.orders")),
                    )
                    .child(TreeItem::new("mysql", "MySQL 示例"))
                    .expanded(true),
                TreeItem::new("production", "生产环境 · 合成")
                    .child(TreeItem::new("readonly", "只读数据库示例")),
            ])
        });
        let sql = cx.new(|cx| EditorState::new(window, cx)
            .language("sql")
            .line_number(true)
            .searchable(true)
            .default_value("-- 功能：组件验证草稿，不执行查询；时间：2026-11-10；作者：lq。\nSELECT '中文' AS name, 9007199254740993 AS id;\n"));
        let table = cx.new(|cx| {
            TableState::new(
                SampleRows {
                    rendered: Rc::new(Cell::new(0)),
                },
                window,
                cx,
            )
            .cell_selectable(true)
            .col_movable(false)
            .sortable(false)
        });
        let panels = [
            ("lab-tree", "对象树 · 合成", Content::Tree(tree.clone())),
            ("lab-sql", "SQL 草稿", Content::Sql(sql.clone())),
            ("lab-notes", "验证说明", Content::Notes),
            (
                "lab-results",
                "100,000 行 × 20 列 · 合成",
                Content::Results(table.clone()),
            ),
        ]
        .into_iter()
        .map(|(name, title, content)| {
            cx.new(|cx| LabPanel {
                name,
                title,
                content,
                focus: cx.focus_handle(),
            })
        })
        .collect();
        let (area, _) = DockSkin::dock_area("qds6-component-lab", Some(1), window, cx);
        let observations = vec![
            cx.observe(&tree, |_, _, cx| cx.notify()),
            cx.observe(&table, |_, _, cx| cx.notify()),
            cx.observe(&sql, |_, _, cx| cx.notify()),
        ];
        let mut this = Self {
            area,
            panels,
            table,
            tree,
            sql,
            focus: cx.focus_handle(),
            _observations: observations,
        };
        this.reset(window, cx);
        this
    }

    /// 功能：重置面板位置并复用内容实体，保护 SQL 草稿和树状态；时间：2026-11-10；作者：lq。
    fn reset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let layout = DockLayout::h_split()
            .child(
                DockLayout::tabs().panel_view(panel_handle(self.panels[0].clone()), cx),
                Some(px(260.)),
            )
            .child(
                DockLayout::v_split()
                    .child(
                        DockLayout::tabs()
                            .panel_view(panel_handle(self.panels[1].clone()), cx)
                            .panel_view(panel_handle(self.panels[2].clone()), cx),
                        Some(px(300.)),
                    )
                    .child(
                        DockLayout::tabs().panel_view(panel_handle(self.panels[3].clone()), cx),
                        None,
                    ),
                None,
            );
        self.area
            .update(cx, |area, cx| area.set_center(layout, window, cx));
        cx.notify();
    }

    /// 功能：通过表格公开 API 定位首尾，测试大范围滚动而不遍历数据源；时间：2026-11-10；作者：lq。
    fn jump(&mut self, row: usize, cx: &mut Context<Self>) {
        self.table.update(cx, |table, cx| {
            table.set_selected_cell(row, 0, cx);
            table.scroll_to_row(row, cx);
        });
    }
}

/// 功能：实验窗口菜单与选择状态反馈；时间：2026-11-10；作者：lq。
impl Render for ComponentLab {
    /// 功能：组合实验标题栏、弹出菜单、停靠区与状态栏；时间：2026-11-10；作者：lq。
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let weak = cx.entity().downgrade();
        let selected = self
            .tree
            .read(cx)
            .selected_item()
            .map(|item| item.label.clone())
            .unwrap_or("尚未选择对象".into());
        // 功能：从同一编辑实体读取字节数，用于人工核对拖动/重置后草稿仍在；时间：2026-11-10；作者：lq。
        let draft_bytes = self.sql.read(cx).value().len();
        v_flex()
            .track_focus(&self.focus)
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(
                TitleBar::new().child(
                    h_flex()
                        .w_full()
                        .px_3()
                        .justify_between()
                        .child("QDS6 · M0 组件验证")
                        .child(
                            Button::new("lab-menu")
                                .small()
                                .label("验证操作")
                                .dropdown_menu(move |menu, _, _| {
                                    let reset = weak.clone();
                                    let first = weak.clone();
                                    let last = weak.clone();
                                    menu.item(PopupMenuItem::new("重置布局").on_click(
                                        move |_, window, cx| {
                                            let _ =
                                                reset.update(cx, |this, cx| this.reset(window, cx));
                                        },
                                    ))
                                    .separator()
                                    .item(PopupMenuItem::new("定位首行").on_click(
                                        move |_, _, cx| {
                                            let _ = first.update(cx, |this, cx| this.jump(0, cx));
                                        },
                                    ))
                                    .item(PopupMenuItem::new("定位末行").on_click(
                                        move |_, _, cx| {
                                            let _ =
                                                last.update(cx, |this, cx| this.jump(ROWS - 1, cx));
                                        },
                                    ))
                                    .separator()
                                    .item(
                                        PopupMenuItem::new("切换浅深主题").on_click(
                                            |_, window, cx| {
                                                let mode = if cx.theme().mode.is_dark() {
                                                    ThemeMode::Light
                                                } else {
                                                    ThemeMode::Dark
                                                };
                                                Theme::change(mode, Some(window), cx);
                                            },
                                        ),
                                    )
                                }),
                        ),
                ),
            )
            .child(div().flex_1().min_h_0().child(self.area.clone()))
            .child(
                h_flex()
                    .h_8()
                    .px_3()
                    .items_center()
                    .justify_between()
                    .text_xs()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(selected)
                    .child(format!("合成数据 · 未连接服务 · 草稿 {draft_bytes} 字节")),
            )
    }
}

/// 功能：原生人工验收入口，与正式客户端使用同一资源及后端；时间：2026-11-10；作者：lq。
#[cfg(feature = "component-lab")]
pub fn run_component_lab() {
    gpui_kit::application().with_assets(AppAssets).run(|cx| {
        gpui_kit::init(cx);
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        let mut options = TitleBar::window_options();
        options.window_bounds = Some(WindowBounds::centered(size(px(1440.), px(900.)), cx));
        options.window_min_size = Some(size(px(960.), px(640.)));
        gpui_kit::open_window(options, cx, |window, cx| {
            window.set_window_title("QDS6 · M0 组件验证");
            cx.new(|cx| ComponentLab::new(window, cx))
        })
        .expect("open QDS6 component lab");
        cx.activate(true);
    });
}

// 功能：真实组件输入、拖动与 GPU 截图验收仅显式编译；时间：2026-11-10；作者：lq。
#[cfg(feature = "ui-tests")]
mod smoke;
#[cfg(feature = "ui-tests")]
pub use smoke::run_component_smoke;
