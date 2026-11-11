//! 功能：按任务组织配置导出、外部导入、恢复和本机回滚；时间：2026-11-11；作者：lq。

use super::*;

// 功能：通过平台对话框响应验证选择、取消和预览失效，不读取用户文件；时间：2026-11-11；作者：lq。
#[cfg(all(test, feature = "ui-tests"))]
mod tests {
    use super::Workbench;
    use gpui_kit::{AppContext, TestAppContext, WindowOptions};

    /// 功能：选择新路径清空旧预览，取消保留预览，忙碌时禁止再打开对话框；时间：2026-11-11；作者：lq。
    #[test]
    fn backup_picker_selection_and_cancellation() {
        let temporary = std::env::temp_dir().join(format!("qds6-picker-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&temporary).unwrap();
        let mut cx = TestAppContext::single();
        cx.skip_drawing();
        cx.update(gpui_kit::init);
        let (handle, view) = cx.update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
                cx.new(|cx| {
                    Workbench::new_with_store(
                        Some(temporary.join("connections.sqlite3")),
                        window,
                        cx,
                    )
                })
            })
            .unwrap()
        });
        let selected = temporary.join("中文备份.json");
        cx.update_window(handle, |_, window, cx| {
            view.update(cx, |state, cx| {
                state.services.backup_mode = 1;
                state.services.preview = Some(qds6_services::backup::Configuration::new(&[]));
                state.services.import_report = vec!["旧报告".into()];
                state.choose_backup_file(window, cx);
            });
        })
        .unwrap();
        assert!(cx.did_prompt_for_paths());
        cx.simulate_path_prompt_response(|options| {
            assert!(options.files && !options.directories && !options.multiple);
            Some(vec![selected.clone()])
        });
        cx.run_until_parked();
        cx.read(|cx| {
            let state = view.read(cx);
            assert_eq!(
                state.services.backup_path.read(cx).value().as_str(),
                selected.to_str().unwrap()
            );
            assert!(state.services.preview.is_none());
            assert!(state.services.import_report.is_empty());
        });
        cx.update_window(handle, |_, window, cx| {
            view.update(cx, |state, cx| {
                state.services.backup_mode = 2;
                state.services.preview = Some(qds6_services::backup::Configuration::new(&[]));
                state.choose_backup_file(window, cx);
            });
        })
        .unwrap();
        assert!(cx.did_prompt_for_paths());
        cx.simulate_path_prompt_response(|_| None);
        cx.run_until_parked();
        cx.read(|cx| {
            let state = view.read(cx);
            assert_eq!(
                state.services.backup_path.read(cx).value().as_str(),
                selected.to_str().unwrap()
            );
            assert!(state.services.preview.is_some());
        });
        cx.update_window(handle, |_, window, cx| {
            view.update(cx, |state, cx| {
                state.services.busy = true;
                state.choose_backup_file(window, cx);
            });
        })
        .unwrap();
        assert!(!cx.did_prompt_for_paths());
        drop(cx);
        std::fs::remove_dir_all(temporary).unwrap();
    }

    /// 功能：含密码预览的最终确认只显示数量，取消不写连接、Keychain 或恢复点；时间：2026-11-11；作者：lq。
    #[test]
    fn password_import_confirmation_cancel_keeps_preview_without_writing() {
        let temporary =
            std::env::temp_dir().join(format!("qds6-import-cancel-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&temporary).unwrap();
        let mut cx = TestAppContext::single();
        cx.skip_drawing();
        cx.update(gpui_kit::init);
        let (handle, view) = cx.update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
                cx.new(|cx| {
                    Workbench::new_with_store(
                        Some(temporary.join("connections.sqlite3")),
                        window,
                        cx,
                    )
                })
            })
            .unwrap()
        });
        cx.update_window(handle, |_, window, cx| {
            view.update(cx, |state, cx| {
                let mut profile = qds6_core::ConnectionProfile::new(qds6_core::Protocol::MySql);
                profile.name = "fixture import".into();
                profile.host = "localhost".into();
                let mut config = qds6_services::backup::Configuration::new(&[profile.clone()]);
                config
                    .secrets
                    .insert(profile.id, "public-test-secret".into());
                state.services.preview = Some(config);
                state.services.backup_mode = 1;
                state.confirm_configuration_restore(window, cx);
            });
        })
        .unwrap();
        cx.run_until_parked();
        assert!(cx.has_pending_prompt());
        let (title, detail) = cx.pending_prompt().unwrap();
        assert!(title.contains("导入"));
        assert!(detail.contains("1 个密码"));
        assert!(!detail.contains("public-test-secret"));
        cx.simulate_prompt_answer("取消");
        cx.run_until_parked();
        cx.read(|cx| {
            let state = view.read(cx);
            assert!(state.profiles.is_empty());
            assert_eq!(state.services.preview.as_ref().unwrap().secrets.len(), 1);
            assert!(!state.services.busy);
        });
        assert!(!std::fs::read_dir(&temporary).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("before-restore")
        }));
        drop(cx);
        std::fs::remove_dir_all(temporary).unwrap();
    }
}

impl Workbench {
    /// 功能：每次只展示一种操作流程，预览与危险确认跟随当前任务；时间：2026-11-11；作者：lq。
    pub fn backup_page(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let mode = self.services.backup_mode;
        let modes = TabBar::new("backup-modes")
            .segmented()
            .selected_index(mode)
            .children(
                ["导出与备份", "从其他工具导入", "恢复与回滚"]
                    .map(|label| Tab::new().label(label).disabled(self.services.busy)),
            )
            .on_click(cx.listener(|this, index: &usize, _, cx| {
                this.services.backup_mode = *index;
                this.services.preview = None;
                this.services.import_report.clear();
                cx.notify();
            }));
        let mut content = v_flex().gap_5();
        if mode == 1 {
            let mut sources = h_flex().gap_2().flex_wrap();
            for (index, (id, label)) in [
                ("import-putty", "PuTTY"),
                ("import-finalshell", "FinalShell"),
                ("import-filezilla", "FileZilla"),
                ("import-navicat", "Navicat"),
            ]
            .into_iter()
            .enumerate()
            {
                sources = sources.child(
                    Button::new(id)
                        .small()
                        .label(label)
                        .selected(index == self.services.import_source)
                        .disabled(self.services.busy)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.services.import_source = index;
                            this.services.preview = None;
                            this.services.import_report.clear();
                            cx.notify();
                        })),
                );
            }
            content = content
                .child(Self::section_heading(
                    "选择来源",
                    // 功能：按来源准确声明密码迁移能力，不将暂不支持说成技术不可迁移；时间：2026-11-11；作者：lq。
                    if matches!(self.services.import_source, 1 | 3) {
                        "连接与账号一并导入；已识别的保存密码确认后写入系统凭据库，私钥需重新选择。"
                    } else {
                        "迁移受支持的连接字段与账号；此来源的密码需补填，私钥需重新选择。"
                    },
                    cx,
                ))
                .child(sources);
        }
        content = content.child(
            v_flex()
                .gap_2()
                .child(div().text_xs().child(if mode == 0 {
                    "保存到文件（绝对路径）"
                } else {
                    "导入 / 备份文件"
                }))
                .child(
                    // 功能：导入和恢复使用系统文件选择器，路径仍可粘贴；时间：2026-11-11；作者：lq。
                    h_flex()
                        .gap_2()
                        .child(
                            div().flex_1().min_w_0().child(
                                Input::new(&self.services.backup_path)
                                    .id("backup-file-path")
                                    .aria_label("配置文件路径")
                                    .disabled(self.services.busy),
                            ),
                        )
                        .when(mode != 0, |row| {
                            row.child(
                                Button::new("choose-backup-file")
                                    .icon(UiIcon::FolderOpen)
                                    .label("选择文件…")
                                    .disabled(self.services.busy)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.choose_backup_file(window, cx)
                                    })),
                            )
                        }),
                ),
        );
        if mode != 1 {
            content = content.child(
                v_flex()
                    .gap_2()
                    .child(div().text_xs().child(if mode == 0 {
                        "加密备份口令"
                    } else {
                        "解密口令（加密备份必填）"
                    }))
                    .child(
                        Input::new(&self.services.backup_password)
                            .aria_label("备份口令")
                            .disabled(self.services.busy),
                    ),
            );
        }
        if mode == 0 {
            content = content
                .child(
                    Checkbox::new("include-credentials")
                        .label("在加密备份中包含已保存密码 / 私钥口令")
                        .checked(self.services.include_credentials)
                        .disabled(self.services.busy)
                        .on_click(cx.listener(|this, checked: &bool, _, cx| {
                            this.services.include_credentials = *checked;
                            cx.notify();
                        })),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child("配置导出始终不含凭据；加密备份不包含私钥文件、查询历史及草稿。"),
                )
                .child(
                    h_flex()
                        .gap_3()
                        .flex_wrap()
                        .child(
                            Button::new("export-encrypted")
                                .primary()
                                .icon(UiIcon::Lock)
                                .label("创建加密备份")
                                .disabled(self.services.busy)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.export_configuration(true, window, cx)
                                })),
                        )
                        .child(
                            Button::new("export-config")
                                .icon(UiIcon::Download)
                                .label("仅导出连接配置")
                                .disabled(self.services.busy)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.export_configuration(false, window, cx)
                                })),
                        ),
                );
        } else if mode == 1 {
            content = content.child(
                Button::new("preview-import")
                    .primary()
                    .label("解析并预览导入")
                    .disabled(self.services.busy)
                    .on_click(cx.listener(|this, _, window, cx| this.preview_import(window, cx))),
            );
        } else {
            content = content
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(
                            "先读取文件并核对预览，确认后才会修改连接。恢复不会自动连接服务器。",
                        ),
                )
                .child(
                    h_flex().child(
                        Button::new("preview-config")
                            .primary()
                            .label("读取并预览恢复")
                            .disabled(self.services.busy)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.preview_configuration(window, cx)
                            })),
                    ),
                );
        }
        if mode != 0 {
            content = content.when_some(self.services.preview.as_ref(), |v, preview| {
                let mut rows = v_flex().gap_1();
                for profile in preview.profiles.iter().take(20) {
                    rows = rows.child(
                        h_flex()
                            .min_w_0()
                            .py_2()
                            .gap_3()
                            .items_center()
                            .child(Icon::new(protocol_icon(profile.protocol)).small())
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .child(div().text_ellipsis().child(profile.name.clone()))
                                    // 功能：逐连接显示凭据状态，界面只查询是否存在，不读取明文；时间：2026-11-11；作者：lq。
                                    .when(
                                        mode == 1 && matches!(self.services.import_source, 1 | 3),
                                        |v| {
                                            v.child(
                                                div()
                                                    .text_xs()
                                                    .text_color(cx.theme().muted_foreground)
                                                    .child(
                                                        if preview.secrets.contains_key(&profile.id)
                                                        {
                                                            "密码可迁移"
                                                        } else {
                                                            "认证信息需补全，详见兼容报告"
                                                        },
                                                    ),
                                            )
                                        },
                                    ),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(format!("{}:{}", profile.host, profile.port)),
                            ),
                    );
                }
                v.child(
                    v_flex()
                        .pt_5()
                        .gap_4()
                        .border_t_1()
                        .border_color(cx.theme().border)
                        .child(div().font_semibold().child(format!(
                            "预览 · {} 个连接 · {} 个密码可迁移",
                            preview.profiles.len(),
                            preview.secrets.len()
                        )))
                        .child(rows)
                        .when(preview.profiles.len() > 20, |v| {
                            v.child(div().text_xs().child("仅展示前 20 个连接。"))
                        })
                        .when(!self.services.import_report.is_empty(), |v| {
                            v.child(
                                v_flex()
                                    .gap_2()
                                    .child(div().text_sm().font_semibold().child("兼容报告"))
                                    .children(self.services.import_report.iter().take(20).map(
                                        |warning| {
                                            div()
                                                .text_xs()
                                                .text_color(cx.theme().muted_foreground)
                                                .child(warning.clone())
                                        },
                                    )),
                            )
                        })
                        .child(
                            Checkbox::new("replace-restore")
                                .label("替换全部现有连接（默认按 ID 合并）")
                                .checked(self.services.replace)
                                .disabled(self.services.busy)
                                .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                    this.services.replace = *checked;
                                    cx.notify();
                                })),
                        )
                        .child(
                            h_flex()
                                .gap_3()
                                .items_center()
                                .flex_wrap()
                                .child(
                                    Button::new("confirm-restore")
                                        .primary()
                                        .label(if mode == 1 {
                                            "确认导入…"
                                        } else {
                                            "确认恢复…"
                                        })
                                        .disabled(self.services.busy)
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.confirm_configuration_restore(window, cx)
                                        })),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child("写入前自动保存本机快照"),
                                ),
                        ),
                )
            });
        }
        if mode == 2 {
            content = content.child(
                v_flex()
                    .mt_3()
                    .pt_5()
                    .gap_3()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(Self::section_heading(
                        "本机快照回滚",
                        "仅适用于同一 Mac；在上方选择快照文件，凭据仍依赖原 Keychain。",
                        cx,
                    ))
                    .child(
                        h_flex().child(
                            Button::new("rollback-local-snapshot")
                                .ghost()
                                .small()
                                .label("回滚所选本机快照…")
                                .disabled(self.services.busy)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.rollback_configuration(window, cx)
                                })),
                        ),
                    ),
            );
        }
        div()
            .id("backup-scroll")
            .size_full()
            .overflow_y_scroll()
            .child(
                v_flex()
                    .w_full()
                    .max_w(px(820.))
                    .p_6()
                    .gap_6()
                    .child(
                        v_flex()
                            .gap_2()
                            .child(div().text_xl().font_semibold().child("导入与备份"))
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("迁移连接配置，或为当前工作区创建可恢复的备份。"),
                            ),
                    )
                    .child(h_flex().child(modes))
                    .child(content),
            )
    }

    /// 功能：原生单文件选择用于外部导入和备份恢复，取消保留路径与预览；时间：2026-11-11；作者：lq。
    fn choose_backup_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.services.busy {
            return;
        }
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("选择导入或备份文件".into()),
        });
        cx.spawn_in(window, async move |this, cx| match paths.await {
            Ok(Ok(Some(paths))) => {
                if let Some(path) = paths.first() {
                    let _ = this.update_in(cx, |this, window, cx| {
                        this.services.backup_path.update(cx, |input, cx| {
                            input.set_value(path.display().to_string(), window, cx)
                        });
                        this.services.preview = None;
                        this.services.import_report.clear();
                        this.error = false;
                        this.status = "文件已选择，请解析并核对预览后再确认写入".into();
                        cx.notify();
                    });
                }
            }
            Ok(Ok(None)) => {}
            _ => {
                let _ = this.update(cx, |this, cx| {
                    this.status = "无法打开文件选择器，请重试或粘贴文件路径".into();
                    this.error = true;
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// 功能：沿用后台加密与新文件写入，明文导出始终排除凭据；时间：2026-11-11；作者：lq。
    fn export_configuration(
        &mut self,
        encrypted: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let profiles = self.profiles.clone();
        let include_credentials = encrypted && self.services.include_credentials;
        let path = PathBuf::from(self.services.backup_path.read(cx).value().as_str());
        let password = Zeroizing::new(self.services.backup_password.read(cx).value().to_string());
        self.service_job(
            "正在导出…",
            move || {
                let config = if include_credentials {
                    Configuration::with_credentials(&profiles)?
                } else {
                    Configuration::new(&profiles)
                };
                backup::write_new(
                    &path,
                    &backup::encode(&config, if encrypted { Some(&password) } else { None })?,
                )
            },
            |this, _, _, _| {
                this.status = "文件已导出；包含范围以所选选项为准，私钥文件未包含".into()
            },
            window,
            cx,
        );
    }

    /// 功能：仅预览所选来源文件，兼容报告和连接清单核对后再写入；时间：2026-11-11；作者：lq。
    fn preview_import(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let source = [
            qds6_services::migration::Source::Putty,
            qds6_services::migration::Source::FinalShell,
            qds6_services::migration::Source::FileZilla,
            qds6_services::migration::Source::Navicat,
        ][self.services.import_source];
        let path = PathBuf::from(self.services.backup_path.read(cx).value().as_str());
        // 功能：只对 Navicat 重新导入匹配旧版遗漏数据库名的连接，不提前修改本机配置；时间：2026-11-11；作者：lq。
        let existing = self.profiles.clone();
        self.services.preview = None;
        self.services.import_report.clear();
        self.service_job(
            "正在解析导入预览…",
            move || {
                let mut preview =
                    qds6_services::migration::preview(source, &backup::read_bounded(&path)?)?;
                if matches!(source, qds6_services::migration::Source::Navicat) {
                    preview.reuse_legacy_navicat_ids(&existing);
                }
                Ok(preview)
            },
            |this, preview, _, _| {
                this.services.import_report = preview.warnings;
                this.services.preview = Some(preview.config);
                this.services.replace = false;
                this.status = "解析完成，请核对预览和兼容报告，再确认导入".into();
            },
            window,
            cx,
        );
    }

    /// 功能：读取和校验备份仅建立预览，不触发恢复；时间：2026-11-11；作者：lq。
    fn preview_configuration(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.services.preview = None;
        self.services.import_report.clear();
        let path = PathBuf::from(self.services.backup_path.read(cx).value().as_str());
        let password = Zeroizing::new(self.services.backup_password.read(cx).value().to_string());
        self.service_job(
            "正在校验文件…",
            move || backup::decode(&backup::read_bounded(&path)?, Some(&password)),
            |this, config, _, _| {
                this.status = format!("预览：{} 个连接；尚未写入", config.profiles.len()).into();
                this.services.preview = Some(config);
            },
            window,
            cx,
        );
    }

    /// 功能：保持本机快照回滚的原生确认和后台事务边界；时间：2026-11-11；作者：lq。
    fn rollback_configuration(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let source = PathBuf::from(self.services.backup_path.read(cx).value().as_str());
        let storage = self.storage_path.clone();
        let answer = window.prompt(
            PromptLevel::Warning,
            "回滚本机快照？",
            Some("将替换全部连接并保留当前状态快照。只适用于同一 Mac，凭据仍依赖原 Keychain。"),
            &["取消", "回滚"],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            if answer.await == Ok(1) {
                let _ = this.update_in(cx, |this, window, cx| {
                    this.service_job(
                        "正在回滚本机快照…",
                        move || {
                            let path = storage.map(Ok).unwrap_or_else(Self::store_path)?;
                            let mut store = ProfileStore::open(&path).map_err(|e| e.to_string())?;
                            backup::restore_local_snapshot(
                                &mut store,
                                &source,
                                &path.with_file_name(backup::snapshot_name()),
                            )?;
                            Ok((
                                store.list().map_err(|e| e.to_string())?,
                                store.groups().map_err(|e| e.to_string())?,
                            ))
                        },
                        |this, (profiles, groups), _, cx| {
                            this.profiles = profiles;
                            // 功能：回滚后同步恢复配置中新出现的分组；时间：2026-11-11；作者：lq。
                            this.groups.set_items(groups, cx);
                            this.services.preview = None;
                            this.status =
                                "本机快照已回滚；活动会话保持原连接，重新连接后生效".into();
                        },
                        window,
                        cx,
                    );
                });
            }
        })
        .detach();
    }

    /// 功能：最终确认后先快照再合并或替换，保留原恢复安全边界；时间：2026-11-11；作者：lq。
    fn confirm_configuration_restore(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(config) = self.services.preview.clone() else {
            return;
        };
        let storage = self.storage_path.clone();
        let replace = self.services.replace;
        // 功能：最终确认明确本次凭据写入数量，取消不会提前写入 Keychain；时间：2026-11-11；作者：lq。
        let secret_count = config.secrets.len();
        let importing = self.services.backup_mode == 1;
        let detail = format!(
            "{}其中 {secret_count} 个密码将在确认后写入系统凭据库。不会自动连接服务器。",
            if replace {
                "现有连接将被替换；会先创建本机快照。"
            } else {
                "同 ID 连接将更新，新 ID 连接将添加；会先创建本机快照。"
            }
        );
        let answer = window.prompt(
            PromptLevel::Warning,
            if importing {
                "确认导入连接配置？"
            } else {
                "确认恢复连接配置？"
            },
            Some(&detail),
            &["取消", if importing { "导入" } else { "恢复" }],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            if answer.await == Ok(1) {
                let _ = this.update_in(cx, |this, window, cx| {
                    this.service_job(
                        if importing {
                            "正在创建快照并导入…"
                        } else {
                            "正在创建快照并恢复…"
                        },
                        move || {
                            let path = storage.map(Ok).unwrap_or_else(Self::store_path)?;
                            let snapshot = path.with_file_name(backup::snapshot_name());
                            let mut store = ProfileStore::open(&path).map_err(|e| e.to_string())?;
                            config.restore(&mut store, &snapshot, replace)?;
                            Ok((
                                store.list().map_err(|e| e.to_string())?,
                                store.groups().map_err(|e| e.to_string())?,
                            ))
                        },
                        move |this, (profiles, groups), _, cx| {
                            this.profiles = profiles;
                            // 功能：导入/恢复后立即显示目录，不需手工重新读取；时间：2026-11-11；作者：lq。
                            this.groups.set_items(groups, cx);
                            this.services.preview = None;
                            this.status = format!(
                                "{}完成，{secret_count} 个密码已写入系统凭据库；操作前快照已保留",
                                if importing { "导入" } else { "恢复" }
                            )
                            .into();
                        },
                        window,
                        cx,
                    );
                });
            }
        })
        .detach();
    }
}
