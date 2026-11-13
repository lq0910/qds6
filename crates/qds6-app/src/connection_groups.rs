//! 功能：可折叠连接文件夹、内联编辑、菜单及拖动归组；时间：2026-11-11；作者：lq。

use super::*;
use gpui_kit::component::{
    WindowExt,
    dialog::{DialogAction, DialogClose, DialogFooter},
    menu::{ContextMenuExt, DropdownMenu, PopupMenu, PopupMenuItem},
};
use qds6_storage::{ConnectionGroup, GroupScope, validate_group_name};

// 功能：分组真实原生事件回归，隔离生产配置；时间：2026-11-11；作者：lq。
#[cfg(feature = "ui-tests")]
pub(super) mod smoke;

/// 功能：独立持有目录和后台任务，不中断服务或表单任务；时间：2026-11-11；作者：lq。
#[derive(Default)]
pub(super) struct GroupUi {
    pub(super) items: Vec<ConnectionGroup>,
    pub(super) busy: bool,
    task: Option<Task<()>>,
    editor: Option<GroupEditor>,
    focus: std::collections::BTreeMap<String, FocusHandle>,
}

impl GroupUi {
    /// 功能：窗口关闭时保护未提交的分组输入；时间：2026-11-11；作者：lq。
    pub(super) fn editing(&self) -> bool {
        self.editor.is_some()
    }

    /// 功能：目录刷新保留键盘焦点，鼠标点击后方向键继续控制同一文件夹；时间：2026-11-11；作者：lq。
    pub(super) fn set_items(&mut self, items: Vec<ConnectionGroup>, cx: &mut App) {
        let mut keys: Vec<_> = items
            .iter()
            .map(|group| format!("group:{}:{}", group.scope.key(), group.name))
            .collect();
        keys.extend(["group:servers:".into(), "group:databases:".into()]);
        self.focus.retain(|key, _| keys.contains(key));
        for key in keys {
            self.focus.entry(key).or_insert_with(|| cx.focus_handle());
        }
        self.items = items;
    }
}

/// 功能：内联输入独立于连接表单，失败保留名称与错误；时间：2026-11-11；作者：lq。
struct GroupEditor {
    scope: GroupScope,
    original: Option<String>,
    input: Entity<InputState>,
    error: Option<String>,
    _subscription: Subscription,
}

/// 功能：所有命令捕获明确目标，异步回调不使用变化后的选择；时间：2026-11-11；作者：lq。
#[derive(Clone)]
enum GroupCommand {
    Create(GroupScope, String),
    Rename(GroupScope, String, String),
    Delete(GroupScope, String),
    Move(String, String),
    Collapse(GroupScope, String, bool),
}

/// 功能：拖动只携带配置 ID 和工作区，禁止跨工作区或跨窗口误归组；时间：2026-11-11；作者：lq。
#[derive(Clone)]
struct ConnectionDrag {
    owner: EntityId,
    id: String,
    name: String,
    scope: GroupScope,
    group: String,
}
impl Render for ConnectionDrag {
    /// 功能：拖动预览明确当前连接；时间：2026-11-11；作者：lq。
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .gap_2()
            .px_3()
            .py_2()
            .rounded_md()
            .bg(cx.theme().background)
            .border_1()
            .border_color(cx.theme().primary)
            .child(Icon::new(UiIcon::Server).small())
            .child(self.name.clone())
    }
}

impl Workbench {
    /// 功能：工作区对应独立目录；时间：2026-11-11；作者：lq。
    fn group_scope(&self) -> GroupScope {
        GroupScope::for_protocol(self.area.default_protocol())
    }

    /// 功能：阻止并发写入和加载竞态，现有会话仍可交互；时间：2026-11-11；作者：lq。
    fn group_disabled(&self) -> bool {
        !self.ready || self.loading || self.saving || self.groups.busy || self.services.busy
    }

    /// 功能：展开、收起与数据操作共用后台边界，提交成功后刷新精确磁盘快照；时间：2026-11-11；作者：lq。
    fn run_group_command(
        &mut self,
        command: GroupCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.group_disabled() {
            return;
        }
        self.groups.busy = true;
        self.error = false;
        self.status = "正在保存分组…".into();
        let storage = self.storage_path.clone();
        let operation = command.clone();
        self.groups.task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    let path = storage.map(Ok).unwrap_or_else(Self::store_path)?;
                    let mut store = ProfileStore::open(path).map_err(|e| e.to_string())?;
                    match &operation {
                        GroupCommand::Create(scope, name) => store.create_group(*scope, name),
                        GroupCommand::Rename(scope, old, new) => {
                            store.change_group(*scope, old, Some(new))
                        }
                        GroupCommand::Delete(scope, name) => store.change_group(*scope, name, None),
                        GroupCommand::Move(id, target) => store.move_to_group(id, target),
                        GroupCommand::Collapse(scope, name, collapsed) => {
                            store.set_group_collapsed(*scope, name, *collapsed)
                        }
                    }
                    .map_err(|e| e.to_string())?;
                    Ok::<_, String>((
                        store.list().map_err(|e| e.to_string())?,
                        store.groups().map_err(|e| e.to_string())?,
                    ))
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.groups.busy = false;
                match result {
                    Ok((profiles, groups)) => {
                        // 功能：只同步受影响的分组值，保留未保存的其他字段以及用户正在修改的归组；时间：2026-11-11；作者：lq。
                        let old_group = this.form.profile.group.clone();
                        let scope = GroupScope::for_protocol(this.form.profile.protocol);
                        let new_group = match &command {
                            GroupCommand::Rename(s, old, new)
                                if *s == scope && *old == old_group =>
                            {
                                Some(new.clone())
                            }
                            GroupCommand::Delete(s, name) if *s == scope && *name == old_group => {
                                Some(String::new())
                            }
                            GroupCommand::Move(id, target)
                                if *id == this.form.profile.id.to_string() =>
                            {
                                Some(target.clone())
                            }
                            _ => None,
                        };
                        if let Some(new_group) = new_group {
                            this.form.profile.group = new_group.clone();
                            if this.form.group.read(cx).value().as_str() == old_group {
                                let was_dirty = this.form_dirty;
                                this.form.group.update(cx, |input, cx| {
                                    input.set_value(new_group.clone(), window, cx)
                                });
                                this.form_dirty = was_dirty;
                            }
                        }
                        this.profiles = profiles;
                        this.groups.set_items(groups, cx);
                        // 功能：新文件夹创建后立即可见，不被之前的搜索或收藏筛选隐藏；时间：2026-11-11；作者：lq。
                        if matches!(command, GroupCommand::Create(scope, _) if scope == this.group_scope()) {
                            this.search.update(cx, |input, cx| input.set_value("", window, cx));
                            this.favorites_only = false;
                        }
                        if matches!(command, GroupCommand::Create(..) | GroupCommand::Rename(..)) {
                            this.groups.editor = None;
                            // 功能：输入提交后焦点返回新目录，键盘可继续展开/收起；时间：2026-11-11；作者：lq。
                            let target = match &command {
                                GroupCommand::Create(scope, name) | GroupCommand::Rename(scope, _, name) => Some(format!("group:{}:{}", scope.key(), name)),
                                _ => None,
                            };
                            if let Some(focus) = target.and_then(|key| this.groups.focus.get(&key)) { focus.focus(window, cx); }
                        }
                        this.status = match command {
                            GroupCommand::Create(..) => "分组已创建 · 可拖入连接或在组内新建",
                            GroupCommand::Rename(..) => "分组已重命名，连接和活动会话已保留",
                            GroupCommand::Delete(..) => "分组已删除，其中连接已移到未分组",
                            GroupCommand::Move(..) => "连接已移动到目标分组",
                            GroupCommand::Collapse(_, _, true) => "分组已收起，展开状态已保存",
                            GroupCommand::Collapse(_, _, false) => "分组已展开，展开状态已保存",
                        }
                        .into();
                    }
                    Err(error) => {
                        if let Some(editor) = &mut this.groups.editor {
                            editor.error = Some(error.clone());
                        }
                        this.status = format!("分组操作失败：{error}").into();
                        this.error = true;
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// 功能：新建/重命名自动聚焦输入，Enter 提交，Escape 取消，错误就近显示；时间：2026-11-11；作者：lq。
    fn edit_group(
        &mut self,
        scope: GroupScope,
        original: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.group_disabled() {
            return;
        }
        // 功能：重复触发编辑入口不会无提示覆盖已输入的名称；时间：2026-11-11；作者：lq。
        if let Some(editor) = &self.groups.editor {
            editor.input.update(cx, |state, cx| state.focus(window, cx));
            return;
        }
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("例如：生产环境")
                .default_value(original.clone().unwrap_or_default())
        });
        let subscription = cx.subscribe_in(
            &input,
            window,
            |this, _, event: &InputEvent, window, cx| match event {
                InputEvent::PressEnter { .. } => this.submit_group(window, cx),
                InputEvent::Change => {
                    if let Some(editor) = &mut this.groups.editor {
                        editor.error = None;
                    }
                    cx.notify();
                }
                _ => {}
            },
        );
        input.update(cx, |state, cx| state.focus(window, cx));
        self.groups.editor = Some(GroupEditor {
            scope,
            original,
            input,
            error: None,
            _subscription: subscription,
        });
        cx.notify();
    }

    /// 功能：提交前校验名称及重名，失败不关闭输入或改变旧目录；时间：2026-11-11；作者：lq。
    fn submit_group(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.group_disabled() {
            return;
        }
        let Some(editor) = &mut self.groups.editor else {
            return;
        };
        let result = validate_group_name(editor.input.read(cx).value().as_str()).and_then(|name| {
            if editor.original.as_ref() != Some(&name)
                && self
                    .groups
                    .items
                    .iter()
                    .any(|group| group.scope == editor.scope && group.name == name)
            {
                Err(qds6_storage::StoreError::DuplicateGroup)
            } else {
                Ok(name)
            }
        });
        match result {
            Ok(name) => {
                let command = if let Some(old) = &editor.original {
                    GroupCommand::Rename(editor.scope, old.clone(), name)
                } else {
                    GroupCommand::Create(editor.scope, name)
                };
                self.run_group_command(command, window, cx);
            }
            Err(error) => {
                editor.error = Some(error.to_string());
                cx.notify();
            }
        }
    }

    /// 功能：删除确认说明影响数量，并绑定原分组，删除不触碰连接和活动会话；时间：2026-11-11；作者：lq。
    fn confirm_delete_group(
        &mut self,
        scope: GroupScope,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.group_disabled() {
            return;
        }
        let count = self
            .profiles
            .iter()
            .filter(|p| GroupScope::for_protocol(p.protocol) == scope && p.group == name)
            .count();
        let view = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, _, _| {
            let view = view.clone();
            let name = name.clone();
            dialog
                .title("删除分组？")
                .w(px(420.))
                .child(format!(
                    "“{name}”中的 {count} 个连接将移到“未分组”。连接配置和已打开的会话都会保留。"
                ))
                .footer(
                    DialogFooter::new()
                        .child(
                            DialogClose::new()
                                .child(Button::new("cancel-delete-group").label("取消").outline()),
                        )
                        .child(
                            DialogAction::new().child(
                                Button::new("confirm-delete-group")
                                    .label("删除分组")
                                    .danger(),
                            ),
                        ),
                )
                .on_ok(move |_, window, cx| {
                    view.update(cx, |this, cx| {
                        if this.group_disabled() {
                            return false;
                        }
                        this.run_group_command(
                            GroupCommand::Delete(scope, name.clone()),
                            window,
                            cx,
                        );
                        true
                    })
                    .unwrap_or(true)
                })
        });
    }

    /// 功能：目录菜单同时用于右键和更多按钮；时间：2026-11-11；作者：lq。
    fn folder_menu(
        menu: PopupMenu,
        view: Entity<Self>,
        scope: GroupScope,
        name: String,
        cx: &App,
    ) -> PopupMenu {
        let disabled = view.read(cx).group_disabled();
        let new_view = view.clone();
        let new_name = name.clone();
        let mut menu = menu.item(
            PopupMenuItem::new("在此分组新建连接")
                .icon(UiIcon::Plus)
                .disabled(disabled)
                .on_click(move |_, window, cx| {
                    new_view.update(cx, |this, cx| {
                        let mut profile = ConnectionProfile::new(match scope {
                            GroupScope::Servers => Protocol::Ssh,
                            GroupScope::Databases => Protocol::PostgreSql,
                        });
                        profile.group = new_name.clone();
                        this.open_profile(profile, window, cx);
                    });
                }),
        );
        if !name.is_empty() {
            let rename_view = view.clone();
            let rename_name = name.clone();
            menu = menu
                .separator()
                .item(
                    PopupMenuItem::new("重命名分组…")
                        .disabled(disabled)
                        .on_click(move |_, window, cx| {
                            rename_view.update(cx, |this, cx| {
                                this.edit_group(scope, Some(rename_name.clone()), window, cx)
                            });
                        }),
                )
                .item(
                    PopupMenuItem::new("删除分组…")
                        .icon(UiIcon::Trash)
                        .disabled(disabled)
                        .on_click(move |_, window, cx| {
                            view.update(cx, |this, cx| {
                                this.confirm_delete_group(scope, name.clone(), window, cx)
                            });
                        }),
                );
        }
        menu
    }

    /// 功能：连接菜单提供明确归组入口，无需拖动也可操作；时间：2026-11-11；作者：lq。
    fn connection_menu(
        menu: PopupMenu,
        view: Entity<Self>,
        profile: ConnectionProfile,
        window: &mut Window,
        cx: &mut Context<PopupMenu>,
    ) -> PopupMenu {
        let scope = GroupScope::for_protocol(profile.protocol);
        let state = view.read(cx);
        let disabled = state.group_disabled();
        let mut targets: Vec<String> = state
            .groups
            .items
            .iter()
            .filter(|g| g.scope == scope && !g.name.is_empty())
            .map(|g| g.name.clone())
            .collect();
        targets.push(String::new());
        // 功能：已连接库的右键菜单追加整库 SQL 备份恢复；时间：2026-11-12；作者：lq。
        let menu = Self::connected_database_menu(menu, view.clone(), &profile, cx);
        menu.submenu("移动到分组", window, cx, move |mut menu, _, _| {
            for target in &targets {
                let id = profile.id.to_string();
                let target = target.clone();
                let view = view.clone();
                menu = menu.item(
                    PopupMenuItem::new(if target.is_empty() {
                        "未分组".into()
                    } else {
                        target.clone()
                    })
                    .checked(profile.group == target)
                    .disabled(disabled || profile.group == target)
                    .on_click(move |_, window, cx| {
                        view.update(cx, |this, cx| {
                            this.run_group_command(
                                GroupCommand::Move(id.clone(), target.clone()),
                                window,
                                cx,
                            )
                        });
                    }),
                );
            }
            menu
        })
    }

    /// 功能：连接表单可直接选择已有文件夹，兼容手工输入新名称；时间：2026-11-11；作者：lq。
    pub(super) fn group_form_field(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let scope = GroupScope::for_protocol(self.form.profile.protocol);
        let mut targets: Vec<_> = self
            .groups
            .items
            .iter()
            .filter(|group| group.scope == scope && !group.name.is_empty())
            .map(|group| group.name.clone())
            .collect();
        targets.push(String::new());
        let view = cx.entity();
        v_flex()
            .flex_1()
            .min_w_0()
            .gap_1()
            .child(div().text_xs().child("分组"))
            .child(
                h_flex()
                    .gap_1()
                    .child(
                        Input::new(&self.form.group)
                            .id("分组")
                            .aria_label("分组")
                            .flex_1()
                            .min_w_0()
                            .disabled(self.saving || self.services.busy || self.groups.busy),
                    )
                    .child(
                        Button::new("form-group-picker")
                            .ghost()
                            .icon(UiIcon::ChevronDown)
                            .tooltip("选择已有分组")
                            .disabled(self.group_disabled())
                            .dropdown_menu(move |mut menu, _, cx| {
                                let current = view.read(cx).form.group.read(cx).value();
                                for target in &targets {
                                    let target = target.clone();
                                    let view = view.clone();
                                    menu =
                                        menu.item(
                                            PopupMenuItem::new(if target.is_empty() {
                                                "未分组".into()
                                            } else {
                                                target.clone()
                                            })
                                            .checked(current.as_str() == target)
                                            .on_click(move |_, window, cx| {
                                                view.update(cx, |this, cx| {
                                                    if this.form.group.read(cx).value().as_str()
                                                        != target
                                                    {
                                                        this.form.group.update(cx, |input, cx| {
                                                            input.set_value(
                                                                target.clone(),
                                                                window,
                                                                cx,
                                                            )
                                                        });
                                                        this.form_dirty = true;
                                                        cx.notify();
                                                    }
                                                });
                                            }),
                                        );
                                }
                                menu
                            }),
                    ),
            )
    }

    /// 功能：顶部显式新建分组和折叠工具，编辑区不挤进连接表单；时间：2026-11-11；作者：lq。
    pub(super) fn group_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        v_flex()
            .px_4()
            .pb_2()
            .gap_2()
            .child(
                Button::new("new-connection-group")
                    .ghost()
                    .small()
                    .justify_start()
                    .icon(UiIcon::FolderPlus)
                    .label("新建分组")
                    .disabled(self.group_disabled())
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.edit_group(this.group_scope(), None, window, cx)
                    })),
            )
            .when_some(self.groups.editor.as_ref(), |column, editor| {
                column.child(
                    v_flex()
                        .id("group-editor")
                        .test_support()
                        .gap_2()
                        .p_2()
                        .rounded_md()
                        .bg(cx.theme().secondary)
                        .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                            if event.keystroke.key == "escape" && !this.groups.busy {
                                this.groups.editor = None;
                                cx.stop_propagation();
                                cx.notify();
                            }
                        }))
                        .child(div().text_xs().child(if editor.original.is_some() {
                            "重命名分组"
                        } else {
                            "新建分组"
                        }))
                        .child(
                            Input::new(&editor.input)
                                .id("group-name-input")
                                .aria_label("分组名称")
                                .small()
                                .disabled(self.groups.busy),
                        )
                        .when_some(editor.error.clone(), |v, error| {
                            v.child(
                                div()
                                    .id("group-name-error")
                                    .test_support()
                                    .text_xs()
                                    .text_color(cx.theme().danger)
                                    .child(error),
                            )
                        })
                        .child(
                            h_flex()
                                .gap_2()
                                .justify_end()
                                .child(
                                    Button::new("cancel-group-edit")
                                        .ghost()
                                        .small()
                                        .label("取消")
                                        .disabled(self.groups.busy)
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.groups.editor = None;
                                            cx.notify();
                                        })),
                                )
                                .child(
                                    Button::new("save-group-edit")
                                        .primary()
                                        .small()
                                        .label("保存")
                                        .loading(self.groups.busy)
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.submit_group(window, cx)
                                        })),
                                ),
                        ),
                )
            })
    }

    /// 功能：文件夹列表保留空组，搜索临时展开命中项，不改动已存折叠状态；时间：2026-11-11；作者：lq。
    pub(super) fn connection_group_list(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let scope = self.group_scope();
        let query = self.search.read(cx).value().trim().to_lowercase();
        let searching = !query.is_empty() || self.favorites_only;
        let mut groups: Vec<_> = self
            .groups
            .items
            .iter()
            .filter(|g| g.scope == scope && !g.name.is_empty())
            .cloned()
            .collect();
        groups.sort_by(|a, b| a.name.cmp(&b.name));
        groups.push(
            self.groups
                .items
                .iter()
                .find(|g| g.scope == scope && g.name.is_empty())
                .cloned()
                .unwrap_or(ConnectionGroup {
                    scope,
                    name: String::new(),
                    collapsed: false,
                }),
        );
        let mut list = v_flex().gap_1();
        let owner = cx.entity_id();
        let mut visible_groups = 0;
        for group in groups {
            let total = self
                .profiles
                .iter()
                .filter(|p| GroupScope::for_protocol(p.protocol) == scope && p.group == group.name)
                .count();
            let mut members: Vec<_> = self
                .profiles
                .iter()
                .filter(|p| {
                    GroupScope::for_protocol(p.protocol) == scope
                        && p.group == group.name
                        && (p.matches(&query) || (p.group.is_empty() && "未分组".contains(&query)))
                        && (!self.favorites_only || p.favorite)
                })
                .cloned()
                .collect();
            members.sort_by(|a, b| a.name.cmp(&b.name));
            if searching
                && members.is_empty()
                && (self.favorites_only
                    || if group.name.is_empty() {
                        !"未分组".contains(&query)
                    } else {
                        !group.name.to_lowercase().contains(&query)
                    })
            {
                continue;
            }
            visible_groups += 1;
            let expanded = !group.collapsed || searching;
            let name = group.name.clone();
            let label = if name.is_empty() {
                "未分组".into()
            } else {
                name.clone()
            };
            let key = format!("group:{}:{}", scope.key(), name);
            let focus = self
                .groups
                .focus
                .get(&key)
                .cloned()
                .unwrap_or_else(|| cx.focus_handle());
            let clicked_focus = focus.clone();
            let toggle_name = name.clone();
            let key_name = name.clone();
            let drop_name = name.clone();
            let accept_name = name.clone();
            let view = cx.entity();
            let menu_view = view.clone();
            let menu_name = name.clone();
            let row = h_flex()
                .id(SharedString::from(format!("folder:{key}")))
                .test_support()
                .w_full()
                .rounded_md()
                .can_drop(move |data, _, _| {
                    data.downcast_ref::<ConnectionDrag>().is_some_and(|drag| {
                        drag.owner == owner && drag.scope == scope && drag.group != accept_name
                    })
                })
                .drag_over::<ConnectionDrag>(|style, _, _, cx| {
                    style
                        .bg(cx.theme().accent)
                        .border_1()
                        .border_color(cx.theme().primary)
                })
                .on_drop(cx.listener(move |this, drag: &ConnectionDrag, window, cx| {
                    cx.stop_propagation();
                    this.run_group_command(
                        GroupCommand::Move(drag.id.clone(), drop_name.clone()),
                        window,
                        cx,
                    );
                }))
                .child(
                    Button::new(SharedString::from(key.clone()))
                        .track_focus(&focus)
                        .accessibility_label(format!(
                            "{label}，{total} 个连接，{}",
                            if expanded { "已展开" } else { "已收起" }
                        ))
                        .ghost()
                        .small()
                        .flex_1()
                        .min_w_0()
                        .justify_start()
                        .px_1()
                        .icon(if expanded {
                            UiIcon::ChevronDown
                        } else {
                            UiIcon::ChevronRight
                        })
                        .tooltip(format!(
                            "{label} · {total} 个连接{}",
                            if searching {
                                " · 筛选时自动展开"
                            } else {
                                ""
                            }
                        ))
                        .disabled(self.group_disabled())
                        .child(
                            h_flex()
                                .gap_2()
                                .flex_1()
                                .min_w_0()
                                .child(
                                    Icon::new(if expanded {
                                        UiIcon::FolderOpen
                                    } else {
                                        UiIcon::Folder
                                    })
                                    .small(),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .text_ellipsis()
                                        .text_sm()
                                        .font_medium()
                                        .child(label),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(if searching {
                                            format!("{}/{}", members.len(), total)
                                        } else {
                                            total.to_string()
                                        }),
                                ),
                        )
                        .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                            if !searching
                                && matches!(event.keystroke.key.as_str(), "left" | "right")
                            {
                                cx.stop_propagation();
                                this.run_group_command(
                                    GroupCommand::Collapse(
                                        scope,
                                        key_name.clone(),
                                        event.keystroke.key == "left",
                                    ),
                                    window,
                                    cx,
                                );
                            }
                        }))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            clicked_focus.focus(window, cx);
                            if !searching {
                                this.run_group_command(
                                    GroupCommand::Collapse(scope, toggle_name.clone(), expanded),
                                    window,
                                    cx,
                                );
                            }
                        })),
                )
                .child(
                    Button::new(SharedString::from(format!("group-menu:{key}")))
                        .ghost()
                        .xsmall()
                        .icon(UiIcon::Ellipsis)
                        .tooltip("分组操作")
                        .disabled(self.group_disabled())
                        .dropdown_menu(move |menu, _, cx| {
                            Self::folder_menu(menu, menu_view.clone(), scope, menu_name.clone(), cx)
                        }),
                )
                .context_menu(move |menu, _, cx| {
                    Self::folder_menu(menu, view.clone(), scope, name.clone(), cx)
                });
            list = list.child(row);
            if expanded {
                if members.is_empty() {
                    list = list.child(
                        div()
                            .pl_8()
                            .py_2()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(if searching {
                                "没有匹配的连接"
                            } else {
                                "暂无连接 · 可拖入或新建"
                            }),
                    );
                }
                for profile in members {
                    list = list.child(self.group_connection_row(profile, cx));
                }
            }
        }
        if self.loading || !self.ready || visible_groups == 0 {
            list = list.child(
                div()
                    .px_2()
                    .py_4()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(if self.loading {
                        "正在读取…"
                    } else if !self.ready {
                        "配置读取失败，请重试"
                    } else {
                        "没有匹配的连接"
                    }),
            );
        }
        list
    }

    /// 功能：连接行缩进对齐文件夹，单击编辑、双击连接、拖动或菜单归组；时间：2026-11-11；作者：lq。
    fn group_connection_row(
        &self,
        profile: ConnectionProfile,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let selected = self.form.profile.id == profile.id && self.tab == 1;
        let view = cx.entity();
        let menu_view = view.clone();
        let menu_profile = profile.clone();
        let context_profile = profile.clone();
        let clicked = profile.clone();
        h_flex()
            .id(SharedString::from(profile.id.to_string()))
            .test_support()
            .w_full()
            .min_w_0()
            .pl_6()
            .pr_1()
            .py_2()
            .rounded_md()
            .gap_1()
            .cursor_pointer()
            .bg(if selected {
                cx.theme().secondary
            } else {
                cx.theme().sidebar
            })
            .hover(|style| style.bg(cx.theme().secondary))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_1()
                    .child(
                        h_flex()
                            .gap_2()
                            .min_w_0()
                            .child(Icon::new(protocol_icon(profile.protocol)).small())
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_ellipsis()
                                    .text_sm()
                                    .child(profile.name.clone()),
                            )
                            .when(profile.favorite, |row| {
                                row.child(Icon::new(UiIcon::Star).xsmall())
                            }),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_ellipsis()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!(
                                "{} · {}:{}{}",
                                profile.protocol.label(),
                                profile.host,
                                profile.port,
                                if profile.production { " · 生产" } else { "" }
                            )),
                    ),
            )
            .child(
                Button::new(SharedString::from(format!(
                    "connection-menu:{}",
                    profile.id
                )))
                .ghost()
                .xsmall()
                .icon(UiIcon::Ellipsis)
                .tooltip("连接归组")
                .disabled(self.group_disabled())
                // 功能：阻止连接行点击冒泡，同时保留外层 Popover 的鼠标按下开菜单；时间：2026-11-11；作者：lq。
                .on_click(|_, _, cx| cx.stop_propagation())
                .dropdown_menu(move |menu, window, cx| {
                    Self::connection_menu(menu, menu_view.clone(), menu_profile.clone(), window, cx)
                }),
            )
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                this.open_profile_action(clicked.clone(), event.click_count() >= 2, window, cx);
            }))
            .on_drag(
                ConnectionDrag {
                    owner: cx.entity_id(),
                    id: profile.id.to_string(),
                    name: profile.name,
                    scope: GroupScope::for_protocol(profile.protocol),
                    group: profile.group,
                },
                |drag, _, _, cx| cx.new(|_| drag.clone()),
            )
            .context_menu(move |menu, window, cx| {
                Self::connection_menu(menu, view.clone(), context_profile.clone(), window, cx)
            })
    }
}
