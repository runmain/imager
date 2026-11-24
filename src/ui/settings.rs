//! 设置页：偏好设置、话题标题、供应商。系统设置式布局：浅灰侧栏 + 浅灰页面上的白色分组卡片。

use eframe::egui::{self, Color32, RichText};

use super::style;
use crate::api;
use crate::app::{App, Confirm, Fetch, ModelsPurpose, SettingsTab};
use crate::db::ProviderKind;
use crate::db::{DEFAULT_IMAGE_DIR, UI_FONT_SIZE_MAX, UI_FONT_SIZE_MIN};
use crate::storage;
use crate::title::DEFAULT_TITLE_PROMPT;

const AUTO_FONT: &str = "自动（系统字体）";

pub fn draw(app: &mut App, ui: &mut egui::Ui) {
    egui::Panel::left("settings_tabs")
        .exact_size(180.0)
        .resizable(false)
        .frame(
            egui::Frame::new()
                .fill(style::PANEL_BG)
                .stroke(egui::Stroke::new(1.0, style::BORDER))
                .inner_margin(egui::Margin::symmetric(10, 14)),
        )
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            for (tab, label) in [
                (SettingsTab::Preferences, "偏好设置"),
                (SettingsTab::Title, "话题标题"),
                (SettingsTab::Providers, "供应商"),
                (SettingsTab::Assistants, "助手"),
            ] {
                if style::sidebar_item(ui, app.settings_tab == tab, label).clicked() {
                    app.settings_tab = tab;
                }
            }
        });

    match app.settings_tab {
        SettingsTab::Preferences => style::grouped_page(ui, |ui| preferences(app, ui)),
        SettingsTab::Title => style::grouped_page(ui, |ui| title_settings(app, ui)),
        SettingsTab::Providers => providers(app, ui),
        SettingsTab::Assistants => assistants(app, ui),
    }
}

/// 供应商类型下拉；无障碍名称由 `label` 绑定。返回是否改变。
fn kind_combo(ui: &mut egui::Ui, id_salt: &str, kind: &mut ProviderKind, label: egui::Id) -> bool {
    let before = *kind;
    style::combobox(id_salt)
        .selected_text(kind.label())
        .show_ui(ui, |ui| {
            for k in ProviderKind::ALL {
                ui.selectable_value(kind, k, k.label());
            }
        })
        .response
        .labelled_by(label);
    *kind != before
}

fn assistants(app: &mut App, ui: &mut egui::Ui) {
    egui::Panel::left("assistant_list")
        .exact_size(240.0)
        .resizable(false)
        .frame(
            egui::Frame::new()
                .fill(egui::Color32::WHITE)
                .stroke(egui::Stroke::new(1.0, style::BORDER_LIGHT))
                .inner_margin(egui::Margin::same(12)),
        )
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("助手列表").strong().color(style::MUTED));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(style::secondary("+ 添加助手")).clicked() {
                        app.add_assistant();
                    }
                });
            });
            ui.add_space(8.0);
            let rows: Vec<(i64, String)> = app.assistants.iter().map(|a| (a.id, a.name.clone())).collect();
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                for (id, name) in rows {
                    if style::avatar_item(ui, app.settings_assistant == Some(id), &name).clicked() {
                        app.settings_assistant = Some(id);
                    }
                }
            });
        });

    style::grouped_page(ui, |ui| {
        let Some(idx) = app.settings_assistant.and_then(|id| app.assistants.iter().position(|a| a.id == id)) else {
            ui.label(RichText::new("选择或添加一个助手").color(style::MUTED));
            return;
        };
        let id = app.assistants[idx].id;
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.heading(RichText::new(&app.assistants[idx].name).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if !app.assistants[idx].is_default && ui.add(style::danger_button("删除助手")).clicked() {
                    app.confirm = Some(Confirm::DeleteAssistant(id));
                }
            });
        });
        ui.add_space(12.0);
        style::group(ui, "助手", |ui| {
            let a = &mut app.assistants[idx];
            let label = style::section_title(ui, "名称");
            changed |= ui
                .add(egui::TextEdit::singleline(&mut a.name).desired_width(f32::INFINITY))
                .labelled_by(label.id)
                .changed();
            if a.name.trim().is_empty() {
                ui.label(RichText::new("请填写助手名称").small().color(style::ERROR));
            }
            style::row_separator(ui);
            let label = style::section_title(ui, "系统提示词");
            changed |= ui
                .add(
                    egui::TextEdit::multiline(&mut a.system_prompt)
                        .font(egui::TextStyle::Body)
                        .hint_text("留空则不发送系统提示词")
                        .desired_rows(10)
                        .desired_width(f32::INFINITY),
                )
                .labelled_by(label.id)
                .changed();
        });
        if changed {
            app.save_assistant(id);
        }
    });
}

fn preferences(app: &mut App, ui: &mut egui::Ui) {
    app.request_font_list();

    style::group(ui, "图片保存目录", |ui| {
        ui.horizontal(|ui| {
            let w = (ui.available_width() - 150.0).max(160.0);
            let edit = ui.add(egui::TextEdit::singleline(&mut app.settings.image_dir).desired_width(w));
            let edit = edit.on_hover_text("图片保存目录");
            if edit.lost_focus() {
                if app.settings.image_dir.trim().is_empty() {
                    app.settings.image_dir = DEFAULT_IMAGE_DIR.into();
                }
                app.save_settings();
            }
            if ui.button("选择…").clicked() {
                let start = storage::expand_tilde(&app.settings.image_dir);
                if let Some(dir) = rfd::FileDialog::new().set_directory(&start).pick_folder() {
                    app.settings.image_dir = dir.display().to_string();
                    app.save_settings();
                }
            }
            if ui.button("恢复默认").clicked() {
                app.settings.image_dir = DEFAULT_IMAGE_DIR.into();
                app.save_settings();
            }
        });
        let resolved = storage::expand_tilde(&app.settings.image_dir);
        ui.label(
            RichText::new(format!("实际路径：{}（不存在时会在首次保存图片时创建）", resolved.display()))
                .small()
                .color(style::MUTED),
        );
    });

    style::group(ui, "外观", |ui| {
        style::form_row(ui, "界面字体", |ui, label| font_picker(app, ui, label));
        style::row_separator(ui);
        style::form_row(ui, "界面字号", |ui, label| {
            let mut size = app.settings.ui_font_size;
            ui.label(RichText::new(format!("{size} pt")).color(style::MUTED));
            let resp = ui
                .add(egui::Slider::new(&mut size, UI_FONT_SIZE_MIN..=UI_FONT_SIZE_MAX).step_by(1.0).show_value(false))
                .labelled_by(label);
            if resp.changed() {
                app.set_font_size(size);
            }
        });
    });
}

/// 字体下拉：顶部搜索框 + “自动（系统字体）” + 可显示中文的系统字体。
fn font_picker(app: &mut App, ui: &mut egui::Ui, label: egui::Id) {
    let current = app.settings.ui_font.clone();
    let mut picked: Option<Option<String>> = None;
    let query = &mut app.font_query;
    let families = app.font_families.as_deref();
    let resp = style::combobox("ui_font")
        .width(260.0)
        .height(360.0)
        .selected_text(current.as_deref().unwrap_or(AUTO_FONT))
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show_ui(ui, |ui| {
            ui.add(egui::TextEdit::singleline(query).hint_text("搜索字体").desired_width(f32::INFINITY));
            ui.separator();
            if ui.selectable_label(current.is_none(), AUTO_FONT).clicked() {
                picked = Some(None);
            }
            match families {
                None => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(RichText::new("加载中…").color(style::MUTED));
                    });
                }
                Some(list) => {
                    let needle = query.trim().to_lowercase();
                    let mut shown = 0;
                    for name in list.iter().filter(|n| needle.is_empty() || n.to_lowercase().contains(&needle)) {
                        shown += 1;
                        if ui.selectable_label(current.as_deref() == Some(name), name).clicked() {
                            picked = Some(Some(name.clone()));
                        }
                    }
                    if shown == 0 {
                        ui.label(RichText::new("没有匹配的字体").color(style::MUTED));
                    }
                }
            }
            if picked.is_some() {
                ui.close();
            }
        });
    resp.response.labelled_by(label);
    if let Some(font) = picked {
        app.set_ui_font(font);
    }
}

fn title_settings(app: &mut App, ui: &mut egui::Ui) {
    ui.label(
        RichText::new("话题首次收到返回时，调用所选供应商的 /responses 接口生成不超过 10 个字的标题。")
            .color(style::MUTED),
    );
    ui.add_space(12.0);

    style::group(ui, "供应商与模型", |ui| {
        let current = app.settings.title_provider_id;
        let current_name = current
            .and_then(|id| app.provider(id))
            .map(|p| if p.enabled { p.name.clone() } else { format!("{}（已禁用）", p.name) })
            .unwrap_or_else(|| "不使用（截取消息文字）".into());
        let mut picked = current;
        style::form_row(ui, "标题供应商", |ui, label| {
            style::combobox("title_provider")
                .width(280.0)
                .selected_text(current_name)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut picked, None, "不使用（截取消息文字）");
                    for p in &app.providers {
                        let label = if p.enabled { p.name.clone() } else { format!("{}（已禁用）", p.name) };
                        ui.selectable_value(&mut picked, Some(p.id), label);
                    }
                })
                .response
                .labelled_by(label);
        });
        if picked != current {
            app.settings.title_provider_id = picked;
            app.settings.title_model_id.clear();
            app.save_settings();
            if let Some(id) = picked {
                app.fetch_models(id, ModelsPurpose::Title);
            }
        }

        let Some(pid) = app.settings.title_provider_id else { return };
        if !app.title_fetch.contains_key(&pid) {
            app.fetch_models(pid, ModelsPurpose::Title);
        }
        style::row_separator(ui);
        style::form_row(ui, "标题模型", |ui, label| {
            let loading = matches!(app.title_fetch.get(&pid), Some(Fetch::Loading));
            if loading {
                ui.spinner();
            } else if ui.add(style::secondary("获取")).clicked() {
                app.fetch_models(pid, ModelsPurpose::Title);
            }
            let fetched = match app.title_fetch.get(&pid) {
                Some(Fetch::Done(list)) => list.clone(),
                _ => Vec::new(),
            };
            let saved = app.settings.title_model_id.clone();
            let shown = if saved.is_empty() {
                "请选择".to_string()
            } else if matches!(app.title_fetch.get(&pid), Some(Fetch::Done(_))) && !fetched.contains(&saved) {
                format!("{saved}（不在最新列表中）")
            } else {
                saved.clone()
            };
            let mut chosen = saved.clone();
            style::combobox("title_model")
                .width(240.0)
                .selected_text(shown)
                .show_ui(ui, |ui| {
                    for m in &fetched {
                        ui.selectable_value(&mut chosen, m.clone(), m);
                    }
                })
                .response
                .labelled_by(label);
            if chosen != saved {
                app.settings.title_model_id = chosen;
                app.save_settings();
            }
        });
        if let Some(Fetch::Failed(err)) = app.title_fetch.get(&pid) {
            ui.label(RichText::new(format!("获取模型失败：{err}")).small().color(style::ERROR));
        }
        if app.settings.title_model_id.is_empty() {
            ui.label(RichText::new("请选择标题模型").small().color(style::WARN));
        }
    });

    style::group(ui, "标题提示词", |ui| {
        ui.label(RichText::new("{prompt} 会被替换为话题的首条消息。").small().color(style::MUTED));
        let edit = ui
            .add(
                egui::TextEdit::multiline(&mut app.settings.title_prompt)
                    .font(egui::TextStyle::Body)
                    .desired_rows(7)
                    .desired_width(f32::INFINITY),
            )
            .on_hover_text("标题提示词");
        if edit.changed() {
            app.save_settings();
        }
        if !app.settings.title_prompt.contains("{prompt}") {
            ui.label(RichText::new("提示词中没有 {prompt}，调用时会把首条消息追加到末尾。").small().color(style::WARN));
        }
        if ui.add(style::secondary("恢复默认提示词")).clicked() {
            app.settings.title_prompt = DEFAULT_TITLE_PROMPT.into();
            app.save_settings();
        }
    });
}

fn providers(app: &mut App, ui: &mut egui::Ui) {
    egui::Panel::left("provider_list")
        .exact_size(260.0)
        .resizable(false)
        .frame(
            egui::Frame::new()
                .fill(egui::Color32::WHITE)
                .stroke(egui::Stroke::new(1.0, style::BORDER_LIGHT))
                .inner_margin(egui::Margin::same(12)),
        )
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("供应商列表").strong().color(style::MUTED));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(style::secondary("+ 添加供应商")).clicked() {
                        app.begin_add_provider();
                    }
                });
            });
            ui.add_space(8.0);
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                let rows: Vec<(i64, String, bool)> =
                    app.providers.iter().map(|p| (p.id, p.name.clone(), p.enabled)).collect();
                for (id, name, enabled) in rows {
                    if provider_row(ui, id, &name, enabled, app.selected_provider == Some(id)).clicked() {
                        app.selected_provider = Some(id);
                        app.show_key = false;
                    }
                }
                if app.providers.is_empty() {
                    ui.label(RichText::new("还没有供应商").color(style::MUTED));
                }
            });
        });

    style::grouped_page(ui, |ui| {
        if let Some(id) = app.selected_provider.filter(|id| app.provider(*id).is_some()) {
            provider_detail(app, ui, id);
        } else {
            ui.label(RichText::new("选择或添加一个供应商").color(style::MUTED));
        }
    });
}

fn provider_row(ui: &mut egui::Ui, id: i64, name: &str, enabled: bool, selected: bool) -> egui::Response {
    let (bg, border) = if selected {
        (Color32::from_rgb(0xf1, 0xf5, 0xf9), Color32::from_rgb(0xcb, 0xd5, 0xe1))
    } else {
        (Color32::WHITE, style::BORDER_LIGHT)
    };
    let inner = egui::Frame::new()
        .fill(bg)
        .stroke(egui::Stroke::new(1.0, border))
        .corner_radius(egui::CornerRadius::same(style::CONTROL_RADIUS))
        .inner_margin(egui::Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let initial: String = name.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default();
                style::pill(ui, &initial, Color32::from_rgb(0x47, 0x55, 0x69), Color32::from_rgb(0xf1, 0xf5, 0xf9));
                let name_color = if selected { Color32::from_rgb(0x0f, 0x17, 0x2a) } else { Color32::from_rgb(0x33, 0x41, 0x55) };
                ui.add(egui::Label::new(RichText::new(name).color(name_color).strong()).truncate());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if enabled {
                        style::pill(ui, "启用", style::ACCENT, style::ACCENT_BG);
                    } else {
                        style::pill(ui, "已停用", style::MUTED, Color32::from_rgb(0xf1, 0xf5, 0xf9));
                    }
                });
            });
        });
    ui.interact(inner.response.rect, ui.scope_id().with(("provider_row", id)), egui::Sense::click())
}

fn provider_detail(app: &mut App, ui: &mut egui::Ui, id: i64) {
    let Some(idx) = app.providers.iter().position(|p| p.id == id) else { return };
    let mut changed = false;

    ui.horizontal(|ui| {
        ui.heading(RichText::new(&app.providers[idx].name).strong());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.add(style::danger_button("删除供应商")).clicked() {
                app.confirm = Some(Confirm::DeleteProvider(id));
            }
            let p = &mut app.providers[idx];
            let label = if p.enabled { "已启用" } else { "已禁用" };
            changed |= ui.checkbox(&mut p.enabled, label).changed();
        });
    });
    ui.add_space(12.0);

    let mut restore_name = false;
    style::group(ui, "连接", |ui| {
        let p = &mut app.providers[idx];
        let label = style::section_title(ui, "名称");
        let name_edit =
            ui.add(egui::TextEdit::singleline(&mut p.name).desired_width(f32::INFINITY)).labelled_by(label.id);
        changed |= name_edit.changed();
        let name_blank = p.name.trim().is_empty();
        if name_blank {
            ui.label(RichText::new("请填写供应商名称").small().color(style::ERROR));
        }
        restore_name = name_blank && name_edit.lost_focus();
        style::row_separator(ui);

        let label = style::section_title(ui, "类型");
        changed |= kind_combo(ui, "provider_kind", &mut p.kind, label.id);
        style::row_separator(ui);

        let label = style::section_title(ui, "API Key");
        ui.horizontal(|ui| {
            let w = ui.available_width() - 60.0;
            changed |= ui
                .add(egui::TextEdit::singleline(&mut p.api_key).password(!app.show_key).desired_width(w))
                .labelled_by(label.id)
                .changed();
            if ui.add(style::secondary(if app.show_key { "隐藏" } else { "显示" })).clicked() {
                app.show_key = !app.show_key;
            }
        });
        style::row_separator(ui);

        let label = style::section_title(ui, "API Base URL");
        changed |= ui
            .add(
                egui::TextEdit::singleline(&mut p.base_url)
                    .hint_text("https://example.com/v1")
                    .desired_width(f32::INFINITY),
            )
            .labelled_by(label.id)
            .changed();
        if !p.base_url.trim().is_empty() {
            let hint = match p.kind {
                ProviderKind::OpenAI => format!(
                    "生图 {}　聊天 {}",
                    api::join_url(&p.base_url, "/images/generations"),
                    api::join_url(&p.base_url, "/responses")
                ),
                ProviderKind::Anthropic => format!("聊天 {}", api::join_url(&p.base_url, "/messages")),
            };
            ui.label(RichText::new(hint).small().color(style::MUTED));
        }
    });
    if restore_name {
        app.restore_provider_name(id);
    }
    if changed {
        app.save_provider(id);
    }

    models_section(app, ui, id);
}

fn models_section(app: &mut App, ui: &mut egui::Ui, id: i64) {
    let saved = app.models.get(&id).cloned().unwrap_or_default();
    ui.horizontal(|ui| {
        ui.label(RichText::new("模型").small().strong().color(style::MUTED));
        style::pill(ui, &saved.len().to_string(), style::MUTED, style::PANEL_BG);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if matches!(app.provider_fetch.get(&id), Some(Fetch::Loading)) {
                ui.spinner();
            } else if ui.add(style::secondary("获取")).clicked() {
                app.fetch_models(id, ModelsPurpose::Provider);
            }
        });
    });

    style::card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        if saved.is_empty() {
            ui.label(RichText::new("还没有保留的模型，点击“获取”拉取模型列表").color(style::MUTED));
        }
        let mut remove = None;
        for (i, m) in saved.iter().enumerate() {
            if i > 0 {
                style::row_separator(ui);
            }
            ui.horizontal(|ui| {
                ui.label(m);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(style::danger_ghost_button("—")).on_hover_text("移除").clicked() {
                        remove = Some(m.clone());
                    }
                });
            });
        }
        if let Some(m) = remove {
            let rest = saved.iter().filter(|x| **x != m).cloned().collect();
            app.set_models(id, rest);
        }
    });

    match app.provider_fetch.get(&id).cloned() {
        Some(Fetch::Failed(err)) => {
            ui.label(RichText::new(format!("获取模型失败：{err}")).color(style::ERROR));
        }
        Some(Fetch::Done(list)) => {
            ui.add_space(14.0);
            let selection = app.fetch_selection.entry(id).or_default();
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("拉取到 {} 个模型，勾选要保留的模型", list.len()))
                        .small()
                        .strong()
                        .color(style::MUTED),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(style::secondary("清空")).clicked() {
                        selection.clear();
                    }
                    if ui.add(style::secondary("全选")).clicked() {
                        selection.extend(list.iter().cloned());
                    }
                });
            });
            style::card().show(ui, |ui| {
                ui.set_width(ui.available_width());
                egui::ScrollArea::vertical().id_salt("fetched_models").max_height(280.0).show(ui, |ui| {
                    for m in &list {
                        let mut on = selection.contains(m);
                        if ui.checkbox(&mut on, m).changed() {
                            if on {
                                selection.insert(m.clone());
                            } else {
                                selection.remove(m);
                            }
                        }
                    }
                });
            });
            let mut chosen: Vec<String> = list.iter().filter(|m| selection.contains(*m)).cloned().collect();
            // 已保留但不在本次列表中的模型保持原样，避免误删
            chosen.extend(saved.iter().filter(|m| !list.contains(m) && selection.contains(*m)).cloned());
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.add(style::primary("保存所选")).clicked() {
                    app.set_models(id, chosen);
                    app.provider_fetch.remove(&id);
                }
                if ui.add(style::secondary("取消")).clicked() {
                    app.provider_fetch.remove(&id);
                }
            });
        }
        _ => {}
    }
}

/// “添加供应商”窗口：名称必填，Base URL / Key 可稍后在详情页补充。
pub fn add_provider_modal(app: &mut App, ctx: &egui::Context) {
    let Some(mut draft) = app.new_provider.take() else { return };
    let mut done: Option<bool> = None;
    let name_ok = !draft.name.trim().is_empty();
    let resp = egui::Modal::new(egui::Id::unique("add_provider")).show(ctx, |ui| {
        ui.set_width(420.0);
        ui.heading("添加供应商");
        ui.add_space(4.0);
        let label = ui.label(RichText::new("名称 *").strong());
        let name = ui
            .add(egui::TextEdit::singleline(&mut draft.name).hint_text("例如：OpenAI").desired_width(f32::INFINITY))
            .labelled_by(label.id);
        if ui.memory(|m| m.focused().is_none()) {
            name.request_focus();
        }
        if !name_ok {
            ui.label(RichText::new("请填写供应商名称").color(style::ERROR));
        }
        let label = ui.label(RichText::new("类型").strong());
        kind_combo(ui, "new_provider_kind", &mut draft.kind, label.id);
        let label = ui.label(RichText::new("API Base URL").strong());
        ui.add(
            egui::TextEdit::singleline(&mut draft.base_url)
                .hint_text("https://example.com/v1")
                .desired_width(f32::INFINITY),
        )
        .labelled_by(label.id);
        let label = ui.label(RichText::new("API Key").strong());
        ui.add(egui::TextEdit::singleline(&mut draft.api_key).password(true).desired_width(f32::INFINITY))
            .labelled_by(label.id);
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let add = ui.add_enabled(name_ok, style::primary("添加"));
                if add.clicked() || (name_ok && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                    done = Some(true);
                }
                if ui.add(style::secondary("取消")).clicked() {
                    done = Some(false);
                }
            });
        });
    });
    if resp.should_close() && done.is_none() {
        done = Some(false);
    }
    match done {
        Some(true) => {
            if let Err(err) = app.add_provider(&draft) {
                app.report(err);
                app.new_provider = Some(draft);
            }
        }
        Some(false) => {}
        None => app.new_provider = Some(draft),
    }
}
