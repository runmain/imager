//! 界面绘制入口：左侧导航、提示条、确认弹窗，分发到生图页、聊天页与设置页。

pub mod chat;
pub mod conversation;
pub mod markdown;
pub mod icons;
pub mod settings;
pub mod style;
pub mod viewer;

use eframe::egui::{self, RichText};

use crate::app::{App, Confirm, Page, TitleFor};
use icons::Icon;

pub fn draw(app: &mut App, ui: &mut egui::Ui) {
    // 根区域先铺浅色底，避免任何未填充的面板透出清屏色
    ui.painter().rect_filled(ui.max_rect(), egui::CornerRadius::ZERO, style::CHAT_BG);
    egui::Panel::left("nav")
        .exact_size(64.0)
        .resizable(false)
        .frame(
            egui::Frame::new()
                .fill(style::PANEL_BG)
                .stroke(egui::Stroke::new(1.0, style::BORDER_LIGHT))
                .inner_margin(egui::Margin::symmetric(6, 20)),
        )
        .show(ui, |ui| nav(app, ui));

    if let Some(msg) = app.banner.clone() {
        egui::Panel::top("banner")
            .frame(egui::Frame::new().fill(style::WARN_BG).inner_margin(egui::Margin::symmetric(16, 8)))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&msg).color(style::WARN));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("关闭").clicked() {
                            app.banner = None;
                        }
                    });
                });
            });
    }

    match app.page {
        Page::Chat => chat::draw(app, ui),
        Page::Talk => conversation::draw(app, ui),
        Page::Settings => settings::draw(app, ui),
    }

    confirm_modal(app, ui.ctx());
    rename_modal(app, ui.ctx());
    settings::add_provider_modal(app, ui.ctx());
    viewer::draw(app, ui.ctx());
    toast(app, ui.ctx());
}

/// 底部居中的短暂提示（复制成功等），到时自动消失。
fn toast(app: &mut App, ctx: &egui::Context) {
    let Some((text, until)) = app.toast.clone() else { return };
    if ctx.input(|i| i.time) >= until {
        app.toast = None;
        return;
    }
    egui::Area::new(egui::Id::unique("toast"))
        .order(egui::Order::Tooltip)
        .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -120.0))
        .interactable(false)
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(style::TEXT)
                .corner_radius(egui::CornerRadius::same(style::CARD_RADIUS))
                .inner_margin(egui::Margin::symmetric(14, 8))
                .show(ui, |ui| ui.label(RichText::new(text).color(egui::Color32::WHITE)));
        });
}

fn nav(app: &mut App, ui: &mut egui::Ui) {
    ui.vertical_centered(|ui| {
        ui.add_space(4.0);
        ui.spacing_mut().item_spacing.y = 8.0;
        for (page, icon, label) in
            [(Page::Chat, Icon::Sparkle, "生图"), (Page::Talk, Icon::Chat, "聊天"), (Page::Settings, Icon::Gear, "设置")]
        {
            if icons::nav_item(ui, icon, label, app.page == page).clicked() {
                app.page = page;
            }
        }
    });
}

fn confirm_modal(app: &mut App, ctx: &egui::Context) {
    let Some(confirm) = app.confirm else { return };
    let (title, detail) = match confirm {
        Confirm::DeleteProvider(id) => (
            "删除供应商",
            format!(
                "确定删除供应商「{}」？其保留的模型会一并删除。",
                app.provider(id).map(|p| p.name.as_str()).unwrap_or("")
            ),
        ),
        Confirm::DeleteTopic(id) => (
            "删除话题",
            format!(
                "确定删除话题「{}」？只删除对话记录，已保存的图片文件不受影响。",
                app.topics.iter().find(|t| t.id == id).map(|t| t.title.as_str()).unwrap_or("")
            ),
        ),
        Confirm::DeleteConversation(id) => (
            "删除对话",
            format!(
                "确定删除对话「{}」？其全部消息会一并删除。",
                app.conversations.iter().find(|c| c.id == id).map(|c| c.title.as_str()).unwrap_or("")
            ),
        ),
        Confirm::DeleteAssistant(id) => (
            "删除助手",
            format!(
                "确定删除助手「{}」？它的全部对话与消息会一并删除。",
                app.assistants.iter().find(|a| a.id == id).map(|a| a.name.as_str()).unwrap_or("")
            ),
        ),
    };
    let mut decided: Option<bool> = None;
    let resp = egui::Modal::new(egui::Id::unique("confirm")).show(ctx, |ui| {
        ui.set_width(380.0);
        ui.spacing_mut().item_spacing.y = 10.0;
        ui.heading(RichText::new(title).strong().color(style::TEXT));
        ui.label(RichText::new(detail).color(style::MUTED));
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(style::danger_button("删除")).clicked() {
                    decided = Some(true);
                }
                if ui.add(style::secondary("取消")).clicked() {
                    decided = Some(false);
                }
            });
        });
    });
    if resp.should_close() && decided.is_none() {
        decided = Some(false);
    }
    match decided {
        Some(true) => {
            app.confirm = None;
            match confirm {
                Confirm::DeleteProvider(id) => app.delete_provider(id),
                Confirm::DeleteTopic(id) => app.delete_topic(id),
                Confirm::DeleteConversation(id) => app.delete_conversation(id),
                Confirm::DeleteAssistant(id) => app.delete_assistant(id),
            }
        }
        Some(false) => app.confirm = None,
        None => {}
    }
}

fn rename_modal(app: &mut App, ctx: &egui::Context) {
    let Some((target, mut name)) = app.rename.take() else { return };
    let heading = match target {
        TitleFor::Topic(_) => "重命名话题",
        TitleFor::Conversation(_) => "重命名对话",
    };
    let mut done: Option<bool> = None;
    let resp = egui::Modal::new(egui::Id::unique("rename")).show(ctx, |ui| {
        ui.set_width(380.0);
        ui.spacing_mut().item_spacing.y = 10.0;
        ui.heading(RichText::new(heading).strong().color(style::TEXT));
        let edit = ui.add(egui::TextEdit::singleline(&mut name).desired_width(f32::INFINITY));
        edit.request_focus();
        if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            done = Some(true);
        }
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(style::primary("保存")).clicked() {
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
        Some(true) => app.rename(target, &name),
        Some(false) => {}
        None => app.rename = Some((target, name)),
    }
}

/// 用系统默认程序打开文件或目录。
pub fn open_path(path: &std::path::Path) -> Result<(), String> {
    let cmd = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
    std::process::Command::new(cmd)
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|err| format!("无法打开 {}：{err}", path.display()))
}
