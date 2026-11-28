//! 功能：原生 GPUI 外键关系图，支持字段映射、约束规则和打开表结构；时间：2026-11-28；作者：lq。

use super::*;
use gpui_kit::component::WindowExt;
use qds6_services::database::{ForeignKey, TableName, TableRelationships};
use std::{collections::BTreeMap, sync::Weak};

const NODE_WIDTH: f32 = 214.;
const NODE_HEIGHT: f32 = 82.;
const SIDE_INSET: f32 = 16.;
const ROW_HEIGHT: f32 = 104.;
const NODES_PER_SIDE: usize = 5;

/// 功能：关联按外键方向分列，同表多条外键合并为一个节点；时间：2026-11-28；作者：lq。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Direction {
    Incoming,
    Outgoing,
    SelfReference,
}

struct RelationNode {
    direction: Direction,
    table: TableName,
    keys: Vec<usize>,
}

/// 功能：关系图状态独立于数据库标签，表草稿不随关系查看切换；时间：2026-11-28；作者：lq。
struct RelationshipGraph {
    data: TableRelationships,
    nodes: Vec<RelationNode>,
    workbench: WeakEntity<Workbench>,
    database: Weak<DatabaseSession>,
    page: usize,
    selected: Option<usize>,
    graph_scroll: ScrollHandle,
    details_scroll: ScrollHandle,
}

impl Workbench {
    /// 功能：后台查询一跳关系，使用现有会话任务和错误提示；时间：2026-11-28；作者：lq。
    pub(super) fn open_relationship_graph(
        &mut self,
        table: Table,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(database) = self.services.database.clone() else {
            return;
        };
        if table.kind != "BASE TABLE" || window.has_active_dialog(cx) {
            return;
        }
        let session = Arc::downgrade(&database);
        self.service_job(
            "正在读取外键关系…",
            move || database.table_relationships(&table),
            move |this, data, window, cx| {
                this.status = format!("{} · {} 条外键", data.table, data.keys.len()).into();
                if this.area == crate::workspace_ui::WorkspaceArea::Databases && this.tab == 3 {
                    show_graph(data, session, window, cx);
                }
            },
            window,
            cx,
        );
    }
}

/// 功能：以当前窗口可用宽度承载关系图，关闭后回到原数据库视图；时间：2026-11-28；作者：lq。
fn show_graph(
    data: TableRelationships,
    database: Weak<DatabaseSession>,
    window: &mut Window,
    cx: &mut Context<Workbench>,
) {
    let workbench = cx.entity().downgrade();
    let graph = cx.new(|_| RelationshipGraph::new(data, workbench, database));
    window.open_dialog(cx, move |dialog, window, _| {
        let width = (window.viewport_size().width - px(56.)).min(px(950.));
        dialog.title("关系模型图").w(width).child(graph.clone())
    });
}

impl RelationshipGraph {
    /// 功能：按方向和表身份归组，保留完整约束和字段详单；时间：2026-11-28；作者：lq。
    fn new(
        data: TableRelationships,
        workbench: WeakEntity<Workbench>,
        database: Weak<DatabaseSession>,
    ) -> Self {
        let mut groups = BTreeMap::<_, Vec<_>>::new();
        for (index, key) in data.keys.iter().enumerate() {
            let (direction, table) = if key.source == data.table && key.target == data.table {
                (Direction::SelfReference, &data.table)
            } else if key.target == data.table {
                (Direction::Incoming, &key.source)
            } else {
                (Direction::Outgoing, &key.target)
            };
            groups
                .entry((direction, table.clone()))
                .or_default()
                .push(index);
        }
        let nodes: Vec<_> = groups
            .into_iter()
            .map(|((direction, table), keys)| RelationNode {
                direction,
                table,
                keys,
            })
            .collect();
        let selected = (!nodes.is_empty()).then_some(0);
        Self {
            data,
            nodes,
            workbench,
            database,
            page: 0,
            selected,
            graph_scroll: ScrollHandle::new(),
            details_scroll: ScrollHandle::new(),
        }
    }

    /// 功能：每侧独立显示当页邻接表，保持图面绘制大小有界；时间：2026-11-28；作者：lq。
    fn lane(&self, direction: Direction) -> impl Iterator<Item = usize> + '_ {
        self.nodes
            .iter()
            .enumerate()
            .filter_map(move |(index, node)| (node.direction == direction).then_some(index))
    }

    /// 功能：翻页时同步选择第一个可见关系和滚动位置；时间：2026-11-28；作者：lq。
    fn set_page(&mut self, page: usize, cx: &mut Context<Self>) {
        self.page = page;
        let skip = page * NODES_PER_SIDE;
        let selected = self
            .lane(Direction::Incoming)
            .nth(skip)
            .or_else(|| self.lane(Direction::Outgoing).nth(skip))
            .or_else(|| self.lane(Direction::SelfReference).next());
        self.graph_scroll.set_offset(point(px(0.), px(0.)));
        self.select(selected, cx);
    }

    /// 功能：点选节点后高亮对应关系并从头显示外键详情；时间：2026-11-28；作者：lq。
    fn select(&mut self, node: Option<usize>, cx: &mut Context<Self>) {
        self.selected = node;
        self.details_scroll.set_offset(point(px(0.), px(0.)));
        cx.notify();
    }

    /// 功能：跳到同一数据库的表结构标签，并保留已打开表的原有草稿；时间：2026-11-28；作者：lq。
    fn open_structure(&self, table: TableName, window: &mut Window, cx: &mut App) {
        window.close_dialog(cx);
        let Some(database) = self.database.upgrade() else {
            return;
        };
        let _ = self.workbench.update(cx, |this, cx| {
            if this
                .services
                .database
                .as_ref()
                .is_some_and(|live| Arc::ptr_eq(live, &database))
            {
                this.open_table(
                    Table {
                        schema: table.schema,
                        name: table.name,
                        kind: "BASE TABLE".into(),
                    },
                    true,
                    window,
                    cx,
                );
            }
        });
    }

    /// 功能：实体卡显示全限定表身份和首组字段映射，结构按钮直接打开对应表；时间：2026-11-28；作者：lq。
    fn table_card(
        &self,
        node: Option<usize>,
        side: Option<Direction>,
        y: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let relation = node.map(|index| &self.nodes[index]);
        let is_center = side.is_none();
        let table = if is_center {
            &self.data.table
        } else {
            &relation.expect("neighbor node").table
        };
        let target = table.clone();
        let selected = node.is_some() && self.selected == node;
        let description = if is_center {
            relation.map_or_else(
                || "当前表".into(),
                |relation| format!("当前表 · {} 条自关联", relation.keys.len()),
            )
        } else {
            let relation = relation.expect("neighbor node");
            let first = &self.data.keys[relation.keys[0]];
            let column = &first.columns[0];
            let extra = if relation.keys.len() > 1 {
                format!(" · {} 条外键", relation.keys.len())
            } else if first.columns.len() > 1 {
                format!(" · {} 列", first.columns.len())
            } else {
                String::new()
            };
            format!("{} → {}{extra}", column.source, column.target)
        };
        let card = v_flex()
            .id((
                "relationship-node",
                if is_center {
                    usize::MAX
                } else {
                    node.expect("neighbor index")
                },
            ))
            .test_support()
            .absolute()
            .w(px(NODE_WIDTH))
            .h(px(NODE_HEIGHT))
            .top(px(y - NODE_HEIGHT / 2.))
            .justify_center()
            .px_2()
            .gap_1()
            .rounded_md()
            .border_1()
            .border_color(if selected || is_center {
                cx.theme().primary
            } else {
                cx.theme().border
            })
            .bg(if is_center {
                cx.theme().secondary
            } else {
                cx.theme().background
            })
            .when(node.is_some(), |v| {
                v.cursor_pointer()
                    .hover(|v| v.bg(cx.theme().secondary))
                    .on_click(cx.listener(move |this, _, _, cx| this.select(node, cx)))
            })
            .child(
                h_flex()
                    .gap_1()
                    .items_center()
                    .child(
                        Icon::new(UiIcon::Table2)
                            .xsmall()
                            .text_color(cx.theme().primary),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .font_semibold()
                            .child(table.name.clone()),
                    )
                    .child(
                        Button::new((
                            "relationship-structure",
                            if is_center {
                                usize::MAX
                            } else {
                                node.expect("neighbor index")
                            },
                        ))
                        .ghost()
                        .xsmall()
                        .label("结构")
                        .tooltip(format!("打开 {target} 的表结构"))
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.open_structure(target.clone(), window, cx);
                            },
                        )),
                    ),
            )
            .child(
                div()
                    .text_xs()
                    .truncate()
                    .text_color(cx.theme().muted_foreground)
                    .child(table.schema.clone()),
            )
            .child(
                div()
                    .text_xs()
                    .truncate()
                    .text_color(if selected {
                        cx.theme().primary
                    } else {
                        cx.theme().muted_foreground
                    })
                    .child(description),
            );
        match side {
            Some(Direction::Incoming) => card.left(px(SIDE_INSET)),
            Some(Direction::Outgoing) => card.right(px(SIDE_INSET)),
            _ => card.left(relative(0.5)).ml(px(-NODE_WIDTH / 2.)),
        }
        .into_any_element()
    }

    /// 功能：完整显示所选节点的全部约束、复合字段顺序和级联规则；时间：2026-11-28；作者：lq。
    fn details(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(node) = self.selected.map(|index| &self.nodes[index]) else {
            return div().into_any_element();
        };
        v_flex()
            .id("relationship-details")
            .test_support()
            .max_h(px(166.))
            .overflow_y_scroll()
            .track_scroll(&self.details_scroll)
            .px_3()
            .py_2()
            .gap_2()
            .rounded_md()
            .border_1()
            .border_color(cx.theme().border)
            .children(node.keys.iter().map(|&index| {
                let key = &self.data.keys[index];
                v_flex()
                    .gap_1()
                    .flex_shrink_0()
                    .child(
                        h_flex()
                            .flex_wrap()
                            .gap_x_3()
                            .items_center()
                            .child(div().font_semibold().child(key.name.clone()))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(format!(
                                        "更新 {}  ·  删除 {}",
                                        key.update_rule, key.delete_rule
                                    )),
                            ),
                    )
                    .child(div().text_xs().child(mapping(key)))
            }))
            .into_any_element()
    }
}

/// 功能：详情按字段顺序显示精确映射，保留原表名和字段名；时间：2026-11-28；作者：lq。
fn mapping(key: &ForeignKey) -> String {
    let source = key
        .columns
        .iter()
        .map(|column| column.source.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let target = key
        .columns
        .iter()
        .map(|column| column.target.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    format!("{} ({source}) → {} ({target})", key.source, key.target)
}

impl Render for RelationshipGraph {
    /// 功能：通过原生画布绘制引用箭头，节点和详情仍使用原生交互组件；时间：2026-11-28；作者：lq。
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let incoming = self.lane(Direction::Incoming).count();
        let outgoing = self.lane(Direction::Outgoing).count();
        let self_node = self.lane(Direction::SelfReference).next();
        let pages = incoming.max(outgoing).div_ceil(NODES_PER_SIDE).max(1);
        let skip = self.page * NODES_PER_SIDE;
        let left: Vec<_> = self
            .lane(Direction::Incoming)
            .skip(skip)
            .take(NODES_PER_SIDE)
            .collect();
        let right: Vec<_> = self
            .lane(Direction::Outgoing)
            .skip(skip)
            .take(NODES_PER_SIDE)
            .collect();
        let height = (left.len().max(right.len()) as f32 * ROW_HEIGHT + 24.).max(176.);
        let center = height * 0.5;
        let mut edges = Vec::new();
        let mut cards = Vec::new();
        for (direction, indices) in [(Direction::Incoming, &left), (Direction::Outgoing, &right)] {
            for (row, &index) in indices.iter().enumerate() {
                let y = center + (row as f32 - (indices.len() - 1) as f32 * 0.5) * ROW_HEIGHT;
                cards.push(self.table_card(Some(index), Some(direction), y, cx));
                edges.push((direction, y, self.selected == Some(index)));
            }
        }
        if let Some(index) = self_node {
            edges.push((
                Direction::SelfReference,
                center,
                self.selected == Some(index),
            ));
        }
        let primary = cx.theme().primary;
        let muted = cx.theme().muted_foreground.opacity(0.65);
        let viewport = (window.viewport_size().height - px(340.))
            .clamp(px(180.), px(440.))
            .min(px(height));
        v_flex()
            .gap_2()
            .child(
                h_flex()
                    .gap_3()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .font_semibold()
                            .child(self.data.table.to_string()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("{} 条外键", self.data.keys.len())),
                    ),
            )
            .when(self.nodes.is_empty(), |v| {
                v.child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child("在当前连接可见的元数据中未找到此表的外键。"),
                )
            })
            .when(incoming + outgoing > 0, |v| {
                v.child(
                    h_flex()
                        .px_3()
                        .justify_between()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(format!("引用当前表 · {incoming}"))
                        .child(format!("当前表引用 · {outgoing}")),
                )
            })
            .child(
                div()
                    .id("relationship-canvas")
                    .test_support()
                    .w_full()
                    .h(viewport)
                    .overflow_y_scroll()
                    .track_scroll(&self.graph_scroll)
                    .rounded_md()
                    .border_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .relative()
                            .w_full()
                            .h(px(height))
                            .flex_shrink_0()
                            .child(
                                canvas(
                                    move |bounds, _, _| bounds,
                                    move |_, bounds, window, _| {
                                        draw_edges(bounds, center, &edges, primary, muted, window);
                                    },
                                )
                                .absolute()
                                .size_full(),
                            )
                            .children(cards)
                            .child(self.table_card(self_node, None, center, cx)),
                    ),
            )
            .when(pages > 1, |v| {
                v.child(
                    h_flex()
                        .justify_end()
                        .items_center()
                        .gap_2()
                        .child(
                            Button::new("relationship-previous")
                                .ghost()
                                .xsmall()
                                .label("上一页")
                                .disabled(self.page == 0)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.set_page(this.page.saturating_sub(1), cx)
                                })),
                        )
                        .child(
                            div()
                                .text_xs()
                                .child(format!("{} / {pages}", self.page + 1)),
                        )
                        .child(
                            Button::new("relationship-next")
                                .ghost()
                                .xsmall()
                                .label("下一页")
                                .disabled(self.page + 1 == pages)
                                .on_click(
                                    cx.listener(|this, _, _, cx| this.set_page(this.page + 1, cx)),
                                ),
                        ),
                )
            })
            .child(self.details(cx))
    }
}

/// 功能：箭头从引用表指向被引用表，自关联用回环显示；时间：2026-11-28；作者：lq。
fn draw_edges(
    bounds: Bounds<Pixels>,
    center: f32,
    edges: &[(Direction, f32, bool)],
    primary: Hsla,
    muted: Hsla,
    window: &mut Window,
) {
    let width = f32::from(bounds.size.width);
    let p = |x, y| bounds.origin + point(px(x), px(y));
    for &(direction, y, selected) in edges {
        let mut path = PathBuilder::stroke(px(if selected { 2. } else { 1.25 }));
        if direction == Direction::SelfReference {
            let x = width * 0.5;
            let top = center - NODE_HEIGHT * 0.5;
            path.move_to(p(x - 22., top));
            path.cubic_bezier_to(
                p(x + 22., top),
                p(x - 22., top - 38.),
                p(x + 22., top - 38.),
            );
            path.move_to(p(x + 17., top - 8.));
            path.line_to(p(x + 22., top));
            path.line_to(p(x + 27., top - 8.));
        } else {
            let (from_x, from_y, to_x, to_y) = if direction == Direction::Incoming {
                (
                    SIDE_INSET + NODE_WIDTH,
                    y,
                    (width - NODE_WIDTH) * 0.5,
                    center,
                )
            } else {
                (
                    (width + NODE_WIDTH) * 0.5,
                    center,
                    width - SIDE_INSET - NODE_WIDTH,
                    y,
                )
            };
            let mid = (from_x + to_x) * 0.5;
            path.move_to(p(from_x, from_y));
            path.cubic_bezier_to(p(to_x, to_y), p(mid, from_y), p(mid, to_y));
            path.move_to(p(to_x - 8., to_y - 4.));
            path.line_to(p(to_x, to_y));
            path.line_to(p(to_x - 8., to_y + 4.));
        }
        if let Ok(path) = path.build() {
            window.paint_path(path, if selected { primary } else { muted });
        }
    }
}
