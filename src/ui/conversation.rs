//! 聊天页：左栏“助手 | 话题”切换，右侧对话消息（助手回复按 Markdown 渲染）与输入区。

use eframe::egui::{self, RichText};

use super::icons::{self, Icon};
use super::markdown;
use super::style;
use crate::app::{App, Confirm, TalkTab, TitleFor};
use crate::db::{ChatMessage, Role, Status};

pub fn draw(app: &mut App, ui: &mut egui::Ui) {
    egui::Panel::left("talk_sidebar")
        .default_size(260.0)
        .resizable(true)
        .frame(
            egui::Frame::new()
                .fill(style::PANEL_BG)
                .stroke(egui::Stroke::new(1.0, style::BORDER_LIGHT))
                .inner_margin(egui::Margin::symmetric(12, 14)),
        )
        .show(ui, |ui| sidebar(app, ui));

    egui::Panel::top("talk_header")
        .resizable(false)
        .frame(
            egui::Frame::new()
                .fill(style::CHAT_BG)
                .stroke(egui::Stroke::new(1.0, style::BORDER_LIGHT))
                .inner_margin(egui::Margin { left: 20, right: 16, top: 10, bottom: 10 }),
        )
        .show(ui, |ui| header(app, ui));

    egui::Panel::bottom("talk_composer")
        .resizable(false)
        .frame(egui::Frame::new().fill(style::CHAT_BG).inner_margin(egui::Margin { left: 24, right: 24, top: 6, bottom: 16 }))
        .show(ui, |ui| composer(app, ui));

    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(style::CHAT_BG).inner_margin(egui::Margin::symmetric(24, 12)))
        .show(ui, |ui| messages(app, ui));
}

fn sidebar(app: &mut App, ui: &mut egui::Ui) {
    let selected = match app.talk_tab {
        TalkTab::Assistants => 0,
        TalkTab::Conversations => 1,
    };
    if let Some(i) = style::tab_strip(ui, &["助手", "话题"], selected) {
        app.talk_tab = if i == 0 { TalkTab::Assistants } else { TalkTab::Conversations };
    }
    ui.add_space(10.0);
    match app.talk_tab {
        TalkTab::Assistants => assistant_list(app, ui),
        TalkTab::Conversations => conversation_list(app, ui),
    }
}

fn assistant_list(app: &mut App, ui: &mut egui::Ui) {
    let rows: Vec<(i64, String)> = app.assistants.iter().map(|a| (a.id, a.name.clone())).collect();
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        for (id, name) in rows {
            if style::avatar_item(ui, app.talk_assistant == Some(id), &name).clicked() {
                app.select_assistant(Some(id));
            }
        }
    });
}

fn conversation_list(app: &mut App, ui: &mut egui::Ui) {
    let assistant = app.talk_assistant.and_then(|id| app.assistants.iter().find(|a| a.id == id)).map(|a| a.name.clone());
    ui.horizontal(|ui| {
        ui.add(egui::Label::new(RichText::new(assistant.unwrap_or_default()).strong().color(style::MUTED)).truncate());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.add_enabled(app.talk_assistant.is_some(), style::secondary("+ 新建")).clicked() {
                app.new_conversation();
            }
        });
    });
    ui.add_space(6.0);
    let rows: Vec<(i64, String)> = app.conversations.iter().map(|c| (c.id, c.title.clone())).collect();
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 3.0;
        if rows.is_empty() {
            ui.label(RichText::new("还没有对话，直接在右侧输入即可开始").small().color(style::MUTED));
        }
        for (id, title) in rows {
            let label = if app.chatting.contains(&id) { format!("{title} · 生成中") } else { title.clone() };
            let resp = style::sidebar_item(ui, app.current_conversation == Some(id), &label).on_hover_text(&title);
            if resp.clicked() {
                app.select_conversation(Some(id));
            }
            resp.context_menu(|ui| {
                if ui.button("重命名").clicked() {
                    app.rename = Some((TitleFor::Conversation(id), title.clone()));
                    ui.close();
                }
                if ui.button("重新生成标题").clicked() {
                    app.regenerate_title(TitleFor::Conversation(id));
                    ui.close();
                }
                if ui.button(RichText::new("删除对话").color(style::ERROR)).clicked() {
                    app.confirm = Some(Confirm::DeleteConversation(id));
                    ui.close();
                }
            });
        }
    });
}

/// 顶栏：对话标题 + 模型下拉（按钮只显示模型名，下拉项带供应商）。
fn header(app: &mut App, ui: &mut egui::Ui) {
    let models = app.talk_models();
    let title = app
        .current_conversation
        .and_then(|id| app.conversations.iter().find(|c| c.id == id))
        .map(|c| c.title.clone())
        .unwrap_or_else(|| crate::title::DEFAULT_CONVERSATION_TITLE.into());
    ui.horizontal(|ui| {
        ui.add(egui::Label::new(RichText::new(title).strong().size(ui.text_style_height(&egui::TextStyle::Body) + 1.0)).truncate());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if models.is_empty() {
                ui.label(RichText::new("请先在设置中添加供应商并保留模型").small().color(style::WARN));
                return;
            }
            let (shown, tip) = match app
                .talk_model
                .as_ref()
                .and_then(|(pid, m)| models.iter().find(|(p, _, am)| p == pid && am == m))
            {
                Some((_, name, m)) => (m.clone(), format!("供应商：{name}\n模型：{m}")),
                None => ("未选择模型".into(), "未选择模型".into()),
            };
            let mut picked = app.talk_model.clone();
            style::combobox("talk_model")
                .width((ui.available_width() * 0.4).clamp(140.0, 220.0))
                .selected_text(shown)
                .show_ui(ui, |ui| {
                    for (pid, name, m) in &models {
                        ui.selectable_value(&mut picked, Some((*pid, m.clone())), format!("{name} / {m}"));
                    }
                })
                .response
                .on_hover_text(tip);
            if picked != app.talk_model {
                app.talk_model = picked;
            }
        });
    });
}

fn composer(app: &mut App, ui: &mut egui::Ui) {
    let blocker = app.talk_send_blocker();
    let mut send = false;
    let edit_id = egui::Id::unique("talk_input");
    let focused = ui.memory(|m| m.has_focus(edit_id));
    let stroke = if focused { style::ACCENT } else { style::BORDER };
    style::card()
        .stroke(egui::Stroke::new(1.0, stroke))
        .inner_margin(egui::Margin { left: 16, right: 10, top: 12, bottom: 10 })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let w = ui.available_width() - 76.0;
                // Enter 发送、Shift+Enter 换行：在 TextEdit 处理前截获不带 Shift 的 Enter
                if focused && blocker.is_none() {
                    send |= ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
                }
                let edit = ui.add(
                    egui::TextEdit::multiline(&mut app.talk_input)
                        .id(edit_id)
                        .font(egui::TextStyle::Body)
                        .frame(egui::Frame::NONE)
                        .hint_text("输入消息，Enter 发送，Shift+Enter 换行")
                        .desired_rows(3)
                        .desired_width(w),
                );
                let value = app.talk_input.clone();
                edit.widget_info(|| {
                    let mut info = egui::WidgetInfo::text_edit(true, "", &value, "");
                    info.label = Some("聊天输入".into());
                    info
                });
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Max), |ui| {
                    let btn = ui.add_enabled(blocker.is_none(), style::primary("发送").min_size(egui::vec2(64.0, 30.0)));
                    send |= btn.clicked();
                    if let Some(reason) = blocker {
                        btn.on_disabled_hover_text(reason);
                    }
                });
            });
        });
    if send {
        app.send_chat();
    }
}

fn messages(app: &mut App, ui: &mut egui::Ui) {
    if app.chat_messages.is_empty() {
        ui.centered_and_justified(|ui| {
            ui.label(RichText::new("输入消息，开始对话").color(style::MUTED).heading());
        });
        return;
    }
    let msgs = app.chat_messages.clone();
    egui::ScrollArea::vertical().stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
        egui::Frame::new().inner_margin(egui::Margin { right: 20, ..Default::default() }).show(ui, |ui| {
            // 回复记录不存模型，沿用触发它的用户消息所选模型
            let mut model = None;
            for m in &msgs {
                match m.role {
                    Role::User => {
                        model = m.model_id.clone();
                        user_message(app, ui, m);
                    }
                    Role::Assistant => assistant_message(app, ui, m, model.as_deref()),
                }
                ui.add_space(16.0);
            }
        });
    });
}

fn user_message(app: &mut App, ui: &mut egui::Ui, m: &ChatMessage) {
    ui.with_layout(egui::Layout::top_down(egui::Align::Max), |ui| {
        let max = (ui.available_width() * 0.75).max(260.0);
        egui::Frame::new()
            .fill(style::USER_BUBBLE)
            .corner_radius(egui::CornerRadius::same(style::CARD_RADIUS))
            .stroke(egui::Stroke::new(1.0, style::BORDER_LIGHT))
            .inner_margin(egui::Margin::symmetric(14, 10))
            .show(ui, |ui| {
                ui.set_max_width(max);
                ui.add(egui::Label::new(&m.content).wrap().selectable(true));
            });
        if icons::button(ui, Icon::Copy, "复制消息").clicked() {
            ui.ctx().copy_text(m.content.clone());
            app.show_toast("已复制");
        }
    });
}

fn assistant_message(app: &mut App, ui: &mut egui::Ui, m: &ChatMessage, model: Option<&str>) {
    message_header(ui, m.model_id.as_deref().or(model).unwrap_or("助手"), &m.created_at);
    ui.add_space(8.0);
    let text = match m.status {
        Status::Pending => app.streaming.get(&m.id).cloned().unwrap_or_default(),
        _ => m.content.clone(),
    };
    if m.status == Status::Pending && text.is_empty() {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(RichText::new("思考中…").color(style::MUTED));
        });
        return;
    }
    if !text.is_empty() {
        let blocks = app.md_cache.get(m.id, &text);
        // 助手回复底色卡片（与浅绿用户气泡、白色背景明确区分）
        let copied = egui::Frame::new()
            .fill(style::ASSISTANT_BUBBLE)
            .stroke(egui::Stroke::new(1.0, style::ASSISTANT_BORDER))
            .corner_radius(egui::CornerRadius::same(style::CARD_RADIUS))
            .inner_margin(egui::Margin::symmetric(16, 12))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.push_id(m.id, |ui| markdown::show(ui, &blocks)).inner
            })
            .inner;
        if copied {
            app.show_toast("已复制代码");
        }
        ui.add_space(6.0);
    }
    match m.status {
        Status::Pending => {
            ui.spinner();
        }
        Status::Error | Status::Interrupted => {
            let err = m.error.clone().unwrap_or_else(|| "回复失败".into());
            egui::Frame::new()
                .fill(style::ERROR_BG)
                .corner_radius(egui::CornerRadius::same(style::CARD_RADIUS))
                .inner_margin(egui::Margin::same(10))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.add(egui::Label::new(RichText::new(err).color(style::ERROR)).wrap().selectable(true));
                });
        }
        Status::Success => {
            if icons::button(ui, Icon::Copy, "复制回复").clicked() {
                ui.ctx().copy_text(m.content.clone());
                app.show_toast("已复制");
            }
        }
    }
}

/// 回复头部：模型首字母圆形头像 + 模型名 + 时间。
fn message_header(ui: &mut egui::Ui, model: &str, time: &str) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(30.0, 30.0), egui::Sense::hover());
        ui.painter().circle_filled(rect.center(), 15.0, style::ACCENT_BG);
        // 去掉 “google/” 一类的组织前缀后取首字母
        let short = model.rsplit('/').next().unwrap_or(model);
        let initial: String = short.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default();
        ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, initial, egui::FontId::proportional(13.0), style::ACCENT);
        ui.add_space(4.0);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            ui.label(RichText::new(model).color(style::TEXT));
            ui.label(RichText::new(time).small().color(style::MUTED));
        });
    });
}
