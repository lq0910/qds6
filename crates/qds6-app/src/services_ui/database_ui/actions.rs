//! 功能：Navicat 式对象菜单、文件选择/预览/确认/消息日志向导；时间：2026-11-12；作者：lq。
use super::*;
use gpui_kit::component::menu::PopupMenu;
use qds6_services::database::{TransferFormat, read_import};

/// 功能：同一向导承载表交换、复制、重命名和数据库级 SQL 备份恢复；时间：2026-11-12；作者：lq。
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum TransferMode {
    Import,
    Export,
    RunSql,
    Backup,
    Restore,
    Copy(bool),
    Rename,
}
impl TransferMode {
    /// 功能：向导标题明确操作范围，避免把整库恢复误当单表导入；时间：2026-11-12；作者：lq。
    fn title(self) -> &'static str {
        match self {
            Self::Import => "导入向导",
            Self::Export => "导出向导",
            Self::RunSql => "运行 SQL 文件",
            Self::Backup => "整库 SQL 备份",
            Self::Restore => "整库 SQL 恢复",
            Self::Copy(_) => "复制表",
            Self::Rename => "重命名表",
        }
    }
    /// 功能：只有数据库级文件操作需要可配置客户端目录；时间：2026-11-12；作者：lq。
    fn native_client(self) -> bool {
        matches!(self, Self::Backup | Self::Restore | Self::RunSql)
    }
}
/// 功能：目标与连接在打开向导时固定，异步文件选择按代次更新；时间：2026-11-12；作者：lq。
pub(crate) struct TransferWizard {
    id: uuid::Uuid,
    mode: TransferMode,
    database: Arc<DatabaseSession>,
    table: Option<Table>,
    path: Entity<InputState>,
    client_dir: Entity<InputState>,
    name: Entity<InputState>,
    format: TransferFormat,
    step: usize,
    data: Option<ResultSet>,
    script: Option<String>,
    preview: String,
    log: String,
    // 功能：预览后输入变化必须重新预览，不能确认旧目标写入新路径；时间：2026-11-12；作者：lq。
    expected: (String, String, String),
}

impl Workbench {
    /// 功能：左右表目录共用菜单且以右键命中的表为准，不依赖旧选中项；时间：2026-11-12；作者：lq。
    pub(super) fn table_context_menu(
        mut menu: PopupMenu,
        view: Entity<Self>,
        table: Table,
        window: &mut Window,
        cx: &mut Context<PopupMenu>,
    ) -> PopupMenu {
        let busy = view.read(cx).services.busy;
        view.update(cx, |this, cx| {
            this.services.selected_table = this
                .services
                .tables
                .iter()
                .position(|t| t.schema == table.schema && t.name == table.name);
            cx.notify();
        });
        for (action, label) in [
            ("open", "打开表"),
            ("design", "设计表"),
            ("query", "新建查询"),
            ("rename", "重命名…"),
        ] {
            let v = view.clone();
            let t = table.clone();
            menu = menu.item(
                PopupMenuItem::new(label)
                    .disabled(busy)
                    .on_click(move |_, w, cx| {
                        v.update(cx, |this, cx| this.table_menu_action(&t, action, w, cx))
                    }),
            );
        }
        let v = view.clone();
        let t = table.clone();
        menu = menu
            .separator()
            .submenu("复制表", window, cx, move |mut m, _, _| {
                for (with_data, label) in [
                    (true, "结构和数据 · 生成副本"),
                    (false, "仅结构 · 生成副本"),
                ] {
                    let v = v.clone();
                    let t = t.clone();
                    m = m.item(
                        PopupMenuItem::new(label)
                            .disabled(busy || t.kind != "BASE TABLE")
                            .on_click(move |_, w, cx| {
                                v.update(cx, |this, cx| {
                                    this.open_transfer(
                                        TransferMode::Copy(with_data),
                                        Some(t.clone()),
                                        TransferFormat::SqlData,
                                        w,
                                        cx,
                                    )
                                })
                            }),
                    );
                }
                m
            });
        let v = view.clone();
        let t = table.clone();
        menu = menu.submenu("导入…", window, cx, move |mut m, _, _| {
            for (mode, label) in [
                (TransferMode::RunSql, "SQL 文件…"),
                (TransferMode::Import, "CSV / Excel 文件…"),
            ] {
                let v = v.clone();
                let t = t.clone();
                m = m.item(
                    PopupMenuItem::new(label)
                        .disabled(busy || mode == TransferMode::Import && t.kind != "BASE TABLE")
                        .on_click(move |_, w, cx| {
                            v.update(cx, |this, cx| {
                                this.open_transfer(
                                    mode,
                                    Some(t.clone()),
                                    TransferFormat::Csv,
                                    w,
                                    cx,
                                )
                            })
                        }),
                );
            }
            m
        });
        let v = view.clone();
        let t = table.clone();
        menu =
            menu.submenu("导出…", window, cx, move |mut m, _, _| {
                for (format, label) in [
                    (TransferFormat::Csv, "CSV 文件…"),
                    (TransferFormat::Excel, "Excel 文件（.xlsx）…"),
                    (TransferFormat::SqlData, "SQL 文件 · 结构和数据…"),
                ] {
                    let v = v.clone();
                    let t = t.clone();
                    m = m.item(PopupMenuItem::new(label).disabled(busy).on_click(
                        move |_, w, cx| {
                            v.update(cx, |this, cx| {
                                this.open_transfer(
                                    TransferMode::Export,
                                    Some(t.clone()),
                                    format,
                                    w,
                                    cx,
                                )
                            })
                        },
                    ));
                }
                m
            });
        let v = view.clone();
        let t = table.clone();
        menu =
            menu.submenu("转储 SQL 文件", window, cx, move |mut m, _, _| {
                for (format, label) in [
                    (TransferFormat::SqlData, "结构和数据…"),
                    (TransferFormat::SqlStructure, "仅结构…"),
                ] {
                    let v = v.clone();
                    let t = t.clone();
                    m = m.item(PopupMenuItem::new(label).disabled(busy).on_click(
                        move |_, w, cx| {
                            v.update(cx, |this, cx| {
                                this.open_transfer(
                                    TransferMode::Export,
                                    Some(t.clone()),
                                    format,
                                    w,
                                    cx,
                                )
                            })
                        },
                    ));
                }
                m
            });
        let v = view.clone();
        let t = table.clone();
        menu = menu.item(
            PopupMenuItem::new("运行 SQL 文件…")
                .disabled(busy)
                .on_click(move |_, w, cx| {
                    v.update(cx, |this, cx| {
                        this.open_transfer(
                            TransferMode::RunSql,
                            Some(t.clone()),
                            TransferFormat::SqlData,
                            w,
                            cx,
                        )
                    })
                }),
        );
        menu.separator().item(
            PopupMenuItem::new("刷新")
                .disabled(busy)
                .on_click(move |_, w, cx| view.update(cx, |this, cx| this.load_tables(w, cx))),
        )
    }
    /// 功能：活动数据库连接的侧栏菜单也提供整库入口，禁止对未连接或其他连接误执行；时间：2026-11-12；作者：lq。
    pub(crate) fn connected_database_menu(
        mut menu: PopupMenu,
        view: Entity<Self>,
        profile: &ConnectionProfile,
        cx: &mut Context<PopupMenu>,
    ) -> PopupMenu {
        if !matches!(profile.protocol, Protocol::MySql | Protocol::PostgreSql) {
            return menu;
        }
        let active = view
            .read(cx)
            .services
            .database_profile
            .as_ref()
            .is_some_and(|p| p.id == profile.id);
        let busy = view.read(cx).services.busy;
        menu = menu.separator();
        for (mode, label) in [
            (TransferMode::Backup, "备份整个数据库（SQL）…"),
            (TransferMode::Restore, "导入 / 恢复整个数据库（SQL）…"),
        ] {
            let v = view.clone();
            menu = menu.item(
                PopupMenuItem::new(label)
                    .disabled(!active || busy)
                    .on_click(move |_, w, cx| {
                        v.update(cx, |this, cx| {
                            this.open_transfer(mode, None, TransferFormat::SqlData, w, cx)
                        })
                    }),
            );
        }
        menu
    }
    /// 功能：库对象右键提供整个当前数据库的官方 SQL 备份和恢复；时间：2026-11-12；作者：lq。
    pub(super) fn database_context_menu(
        mut menu: PopupMenu,
        view: Entity<Self>,
        _: &mut Window,
        cx: &mut Context<PopupMenu>,
    ) -> PopupMenu {
        let busy = view.read(cx).services.busy;
        for (mode, label) in [
            (TransferMode::RunSql, "运行 SQL 文件…"),
            (TransferMode::Backup, "备份整个数据库（SQL）…"),
            (TransferMode::Restore, "导入 / 恢复整个数据库（SQL）…"),
        ] {
            let v = view.clone();
            menu = menu.item(
                PopupMenuItem::new(label)
                    .disabled(busy)
                    .on_click(move |_, w, cx| {
                        v.update(cx, |this, cx| {
                            this.open_transfer(mode, None, TransferFormat::SqlData, w, cx)
                        })
                    }),
            );
        }
        menu.separator().item(
            PopupMenuItem::new("刷新")
                .disabled(busy)
                .on_click(move |_, w, cx| view.update(cx, |this, cx| this.load_tables(w, cx))),
        )
    }
    /// 功能：基础打开和设计沿用现有多表工作区，表查询只生成 SQL 不自动执行；时间：2026-11-12；作者：lq。
    fn table_menu_action(
        &mut self,
        t: &Table,
        action: &str,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.services.busy {
            return;
        }
        match action {
            "open" | "design" => self.open_table(t.clone(), action == "design", w, cx),
            "rename" => self.open_transfer(
                TransferMode::Rename,
                Some(t.clone()),
                TransferFormat::SqlData,
                w,
                cx,
            ),
            _ => {
                if let Some(p) = &self.services.database_profile {
                    let q = |n: &str| qds6_services::database::identifier(p.protocol, n);
                    if let (Ok(schema), Ok(name)) = (q(&t.schema), q(&t.name)) {
                        self.show_generated_sql(
                            format!("SELECT * FROM {schema}.{name} LIMIT 1000;"),
                            w,
                            cx,
                        );
                    }
                }
            }
        }
    }
    /// 功能：打开向导时保留原查询和表草稿，写操作前阻止丢失未保存数据；时间：2026-11-12；作者：lq。
    pub(super) fn open_transfer(
        &mut self,
        mode: TransferMode,
        table: Option<Table>,
        format: TransferFormat,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.services.busy {
            return;
        }
        let Some(database) = self.services.database.clone() else {
            return;
        };
        if !matches!(mode, TransferMode::Export | TransferMode::Backup)
            && self
                .services
                .table_tabs
                .iter()
                .any(|tab| tab.has_changes(cx))
        {
            self.status = "请先保存或取消表中的数据及结构修改，再执行此操作".into();
            cx.notify();
            return;
        }
        self.area = crate::workspace_ui::WorkspaceArea::Databases;
        self.services.page = 3;
        self.tab = 3;
        self.services.transfer = Some(TransferWizard {
            id: uuid::Uuid::new_v4(),
            mode,
            database,
            table: table.clone(),
            path: cx.new(|cx| InputState::new(w, cx).placeholder("选择文件或填写绝对路径")),
            client_dir: cx.new(|cx| {
                InputState::new(w, cx)
                    .placeholder("自动查找；也可填写 PostgreSQL / MySQL 的 bin 目录")
            }),
            name: cx.new(|cx| {
                let mut i = InputState::new(w, cx);
                i.set_value(table.map(|t| t.name).unwrap_or_default(), w, cx);
                i
            }),
            format,
            step: 0,
            data: None,
            script: None,
            preview: String::new(),
            log: String::new(),
            expected: Default::default(),
        });
        cx.notify();
    }
    /// 功能：原生文件选择按模式区分打开和保存，取消不修改当前状态；时间：2026-11-12；作者：lq。
    fn choose_transfer_path(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        let Some(s) = self.services.transfer.as_ref() else {
            return;
        };
        let id = s.id;
        if matches!(s.mode, TransferMode::Export | TransferMode::Backup) {
            let filename = format!(
                "{}.{}",
                s.table
                    .as_ref()
                    .map(|t| t.name.as_str())
                    .unwrap_or(&self.services.database_context.0),
                s.format.extension()
            );
            let dir = directories::UserDirs::new()
                .map(|d| d.download_dir().unwrap_or(d.home_dir()).to_path_buf())
                .unwrap_or_else(std::env::temp_dir);
            let result = cx.prompt_for_new_path(&dir, Some(&filename));
            cx.spawn_in(w, async move |this, cx| {
                if let Ok(Ok(Some(path))) = result.await {
                    let _ =
                        this.update_in(cx, |this, w, cx| this.set_transfer_path(id, path, w, cx));
                }
            })
            .detach();
        } else {
            let result = cx.prompt_for_paths(PathPromptOptions {
                files: true,
                directories: false,
                multiple: false,
                prompt: Some("选择 SQL、CSV 或 Excel 文件".into()),
            });
            cx.spawn_in(w, async move |this, cx| {
                if let Ok(Ok(Some(paths))) = result.await
                    && let Some(path) = paths.into_iter().next()
                {
                    let _ =
                        this.update_in(cx, |this, w, cx| this.set_transfer_path(id, path, w, cx));
                }
            })
            .detach();
        }
    }
    /// 功能：异步选择结果只回填原向导，避免切换连接或重开向导后串路径；时间：2026-11-12；作者：lq。
    fn set_transfer_path(
        &mut self,
        id: uuid::Uuid,
        path: PathBuf,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(s) = self.services.transfer.as_mut().filter(|s| s.id == id) {
            s.path
                .update(cx, |i, cx| i.set_value(path.display().to_string(), w, cx));
            s.step = 0;
            cx.notify();
        }
    }
    /// 功能：第一步解析文件、显示目标和前 100 行，不提前执行写操作；时间：2026-11-12；作者：lq。
    fn preview_transfer(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        let Some(s) = self.services.transfer.as_ref() else {
            return;
        };
        let id = s.id;
        let mode = s.mode;
        let table = s.table.clone();
        let database = s.database.clone();
        let format = s.format;
        let expected = (
            s.path.read(cx).value().to_string(),
            s.client_dir.read(cx).value().to_string(),
            s.name.read(cx).value().to_string(),
        );
        let inputs = expected.clone();
        let protocol = self
            .services
            .database_profile
            .as_ref()
            .map(|p| p.protocol)
            .unwrap_or(Protocol::PostgreSql);
        self.service_job("正在生成导入导出预览…",move || {
            let result=(|| {
                let path=PathBuf::from(&inputs.0);
                match mode {
                    TransferMode::Import=>{
                        let data=read_import(&path)?;
                        let cols=database.grid_columns(table.as_ref().ok_or("未选择表")?)?;
                        for name in &data.columns {
                            let c=cols.iter().find(|c|c.name==*name).ok_or_else(||format!("目标表不存在字段：{name}"))?;
                            if c.generated||!c.writable{return Err(format!("{name} 是自动生成或暂不支持导入的字段，请从源文件移除该列"));}
                        }
                        let preview=format!("{} 行 · {} 个字段\n按首行字段名匹配；只追加，不覆盖已有记录。\nExcel 读取首个工作表；CSV 使用 UTF-8。\\N 表示 NULL，文本开头的反斜线需加倍。\n\n{}\n{}",data.rows.len(),data.columns.len(),data.columns.join("  |  "),data.rows.iter().take(100).map(|r|r.iter().map(|c|c.display()).collect::<Vec<_>>().join("  |  ")).collect::<Vec<_>>().join("\n"));
                        Ok((Some(data),None,preview))
                    }
                    TransferMode::RunSql|TransferMode::Restore=>{
                        let size=std::fs::metadata(&path).map_err(|e|e.to_string())?.len();
                        if size==0{return Err("SQL 文件为空".into());}
                        if path.extension().and_then(|s|s.to_str()).is_none_or(|s|!s.eq_ignore_ascii_case("sql")){return Err("请选择 UTF-8 的 .sql 文件".into());}
                        let script=if mode==TransferMode::RunSql&&size<=5*1024*1024 {Some(std::fs::read_to_string(&path).map_err(|_|"SQL 文件必须为 UTF-8")?.trim_start_matches('\u{feff}').to_string())}else{None};
                        Ok((None,script,format!("SQL 文件：{}\n大小：{size} 字节\n在当前数据库执行。遇到错误停止。\nPostgreSQL 整库恢复使用事务；MySQL DDL 会隐式提交，失败时可能已有部分语句生效。\n整库恢复建议选择空数据库；已有同名对象不会自动删除。",path.display())))
                    }
                    TransferMode::Export|TransferMode::Backup=>{
                        if !path.is_absolute(){return Err("请填写绝对保存路径".into());}
                        if path.exists(){return Err("该文件已存在，请选择新文件名".into());}
                        if path.extension().and_then(|e|e.to_str()).is_none_or(|e|!e.eq_ignore_ascii_case(format.extension())) {return Err(format!("请选择 .{} 文件名",format.extension()));}
                        let message=if mode==TransferMode::Backup {"备份当前整个数据库的结构和数据，包含视图、函数与触发器；不包含服务器级用户/角色。使用官方客户端生成 SQL。"}else if format==TransferFormat::SqlStructure {"仅转储所选表结构，包含主键、索引、默认值和备注，不含记录。"}else {"导出所选整张表的已提交数据，不受当前页、筛选或查询结果行数限制。CSV/Excel 首行为字段名；NULL 为 \\N，文本开头反斜线加倍。"};
                        Ok((None,None,format!("保存到：{}\n{message}",path.display())))
                    }
                    TransferMode::Copy(data)=>Ok((None,None,format!("在同一模式生成不重名的 _copy 副本。\n复制字段、主键、索引、默认值和外键{}。\n不复制触发器和访问权限。",if data{"，并复制全部记录"}else{"，不复制记录"}))),
                    TransferMode::Rename=>{
                        let t=table.as_ref().ok_or("未选择表")?;
                        let sql=format!("ALTER {} {}.{} RENAME TO {};",if t.kind=="BASE TABLE"{"TABLE"}else{"VIEW"},qds6_services::database::identifier(protocol,&t.schema)?,qds6_services::database::identifier(protocol,&t.name)?,qds6_services::database::identifier(protocol,inputs.2.trim())?);
                        Ok((None,Some(sql.clone()),sql))
                    }
                }
            })();Ok((result,expected))
        },move |this,(result,expected),_,cx| {
            if let Some(s)=this.services.transfer.as_mut().filter(|s|s.id==id) {
                match result {Ok((data,script,preview))=>{s.data=data;s.script=script;s.preview=preview;s.expected=expected;s.step=1;this.status="预览已生成，请核对目标和内容后点击开始".into();},Err(e)=>{s.log=e.clone();s.step=2;this.status=e.into();this.error=true;}}
            }cx.notify();
        },w,cx);
    }
    /// 功能：确认前再次核对输入和连接，后台完成后保留成功/错误日志；时间：2026-11-12；作者：lq。
    fn start_transfer(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        let Some(s) = self.services.transfer.as_ref() else {
            return;
        };
        if s.expected
            != (
                s.path.read(cx).value().to_string(),
                s.client_dir.read(cx).value().to_string(),
                s.name.read(cx).value().to_string(),
            )
        {
            self.status = "选项已改变，请返回上一步重新预览".into();
            cx.notify();
            return;
        }
        if self
            .services
            .database
            .as_ref()
            .is_none_or(|d| !Arc::ptr_eq(d, &s.database))
        {
            return;
        }
        let mode = s.mode;
        let id = s.id;
        let database = s.database.clone();
        let table = s.table.clone();
        let data = s.data.clone();
        let script = s.script.clone();
        let format = s.format;
        let path = PathBuf::from(&s.expected.0);
        let dir = PathBuf::from(&s.expected.1);
        self.service_job("正在执行文件任务…",move || {
            let result=(||match mode {
                TransferMode::Import=>database.import_table(table.as_ref().ok_or("未选择表")?,data.as_ref().ok_or("未解析数据")?).map(|n|format!("导入完成：追加 {n} 行，整批事务已提交")),
                TransferMode::Export=>database.export_table(table.as_ref().ok_or("未选择表")?,format,&path).map(|n|format!("导出完成：{n} 行\n{}",path.display())),
                TransferMode::Copy(with_data)=>database.copy_table(table.as_ref().ok_or("未选择表")?,with_data).map(|t|format!("复制完成：{}.{}",t.schema,t.name)),
                TransferMode::Rename=>database.query(script.as_deref().ok_or("未生成重命名语句")?).map(|_|"重命名完成；对象列表已刷新".into()),
                TransferMode::Backup|TransferMode::Restore=>database.database_sql_file(&path,mode==TransferMode::Restore,Some(&dir)),
                TransferMode::RunSql=>{
                    if let Some(sql)=script.filter(|s|!s.lines().any(|line|line.trim_start().starts_with('\\')||line.trim_start().to_ascii_uppercase().starts_with("DELIMITER "))) {
                        database.query(&sql).and_then(|r|if r.truncated {Err("SQL 结果达到上限，已取消剩余执行；请核对执行状态，较大脚本请使用整库恢复入口".into())}else{Ok(format!("SQL 执行完成：{} 个结果集 · {} ms",r.sets.len(),r.elapsed_ms))})
                    }else{database.database_sql_file(&path,true,Some(&dir))}
                }
            })();Ok(result)
        },move |this,result,w,cx|{
            let success=result.is_ok();
            let message=result.unwrap_or_else(|e|e);
            if let Some(s)=this.services.transfer.as_mut().filter(|s|s.id==id) {s.step=2;s.log=message.clone();}
            this.status=message.into();this.error= !success;
            if success&&!matches!(mode,TransferMode::Export|TransferMode::Backup){
                // 功能：变更后关闭旧的无草稿表快照，避免继续对旧表名或旧结构编辑；时间：2026-11-12；作者：lq。
                this.services.table_tabs.clear();this.services.active_table_tab=None;this.load_tables(w,cx);
            }cx.notify();
        },w,cx);
    }
    /// 功能：常规/消息日志布局呈现文件、连接和预览，开始按钮仅在确认步骤启用；时间：2026-11-12；作者：lq。
    pub(super) fn database_transfer_panel(
        &self,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let s = self.services.transfer.as_ref().expect("transfer wizard");
        let busy = self.services.busy;
        let mode = s.mode;
        v_flex().id("database-transfer-wizard").test_support().size_full().min_w_0().min_h_0().overflow_hidden()
            .child(h_flex().flex_shrink_0().p_3().gap_4().items_center().border_b_1().border_color(cx.theme().border)
                .child(div().text_sm().font_semibold().child(mode.title()))
                .child(div().text_xs().text_color(cx.theme().muted_foreground).child(if s.step==2{"消息日志"}else if s.step==1{"确认与预览"}else{"常规"})))
            .child(v_flex().id("transfer-content").flex_1().min_h_0().overflow_y_scroll().p_4().gap_3()
                .child(div().text_sm().child(format!("连接：{}\n数据库：{}\n{}",self.services.database_identity,self.services.database_context.0,s.table.as_ref().map(|t|format!("目标对象：{}.{}",t.schema,t.name)).unwrap_or_else(||"范围：整个当前数据库".into()))))
                .when(s.step==0,|v|v
                    .when(!matches!(mode,TransferMode::Copy(_)|TransferMode::Rename),|v|v.child(h_flex().gap_2().items_center()
                        .child(div().w(px(70.)).text_sm().child("文件"))
                        .child(div().flex_1().min_w_0().child(Input::new(&s.path).id("transfer-path-input").small()))
                        .child(Button::new("transfer-choose-file").small().label("选择…").disabled(busy).on_click(cx.listener(|this,_,w,cx|this.choose_transfer_path(w,cx))))))
                    .when(mode==TransferMode::Export,|v|v.child(h_flex().gap_2().items_center().flex_wrap().child(div().w(px(70.)).text_sm().child("格式")).children([(TransferFormat::Csv,"CSV"),(TransferFormat::Excel,"Excel (.xlsx)"),(TransferFormat::SqlData,"SQL · 结构和数据"),(TransferFormat::SqlStructure,"SQL · 仅结构")].into_iter().enumerate().map(|(index,(format,label))|Button::new(("transfer-format",index)).small().label(label).selected(s.format==format).disabled(busy).on_click(cx.listener(move |this,_,_,cx|{if let Some(s)=this.services.transfer.as_mut(){s.format=format;}cx.notify();}))))))
                    .when(mode==TransferMode::Rename,|v|v.child(h_flex().gap_2().items_center().child(div().text_sm().child("新名称")).child(Input::new(&s.name).id("transfer-name-input").small())))
                    .when(mode.native_client(),|v|v.child(h_flex().gap_2().items_center().child(div().w(px(100.)).text_sm().child("客户端目录")).child(div().flex_1().min_w_0().child(Input::new(&s.client_dir).small()))))
                    .child(div().text_xs().text_color(cx.theme().muted_foreground).whitespace_normal().child(match mode {TransferMode::Import=>"支持 UTF-8 CSV、Excel .xlsx / .xls，首行字段名；在下一步核对数据，只追加记录。",TransferMode::Export=>"导出整张表；Excel 按文本保存，保留大整数和前导零。已有文件不覆盖。",TransferMode::Backup|TransferMode::Restore=>"整库使用对应版本的 pg_dump / psql 或 mysqldump / mysql。恢复会修改当前数据库，请核对连接及目标库。",TransferMode::RunSql=>"UTF-8 编码，遇到错误停止。普通 SQL 可直接运行；大文件及客户端指令使用官方客户端。",TransferMode::Copy(_)=>"副本会在同一模式生成，自动使用 _copy、_copy_2 等不重名名称。",TransferMode::Rename=>"下一步预览准确的重命名语句，点击开始后执行。"})))
                .when(s.step>0,|v|v.child(div().id("transfer-preview").test_support().text_xs().whitespace_normal().child(if s.step==2{s.log.clone()}else{s.preview.clone()}))))
            .child(h_flex().flex_shrink_0().p_3().gap_2().justify_end().border_t_1().border_color(cx.theme().border)
                .child(Button::new("transfer-close").small().label("关闭").disabled(busy).on_click(cx.listener(|this,_,_,cx|{this.services.transfer=None;cx.notify();})))
                .when(s.step>0,|v|v.child(Button::new("transfer-back").small().label("上一步").disabled(busy).on_click(cx.listener(|this,_,_,cx|{if let Some(s)=this.services.transfer.as_mut(){s.step=0;}cx.notify();}))))
                .when(s.step==0,|v|v.child(Button::new("transfer-next").small().primary().label("下一步").disabled(busy).on_click(cx.listener(|this,_,w,cx|this.preview_transfer(w,cx)))))
                .when(s.step==1,|v|v.child(Button::new("transfer-start").small().primary().label("开始").disabled(busy).on_click(cx.listener(|this,_,w,cx|this.start_transfer(w,cx))))))
    }
}
