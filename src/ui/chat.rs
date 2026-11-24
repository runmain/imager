//! 聊天页：话题列表、消息流、输入区；消息操作均为图标按钮。

use eframe::egui::{self, RichText};

use super::icons::{self, Icon};
use super::style;
use crate::app::{App, Confirm, SIZES, Texture, TitleFor};
use crate::db::{Message, Role, Status};
use crate::title::TitleSource;

/// 解析失败时明文展示的最大字符数，超出部分在数据库中完整保留。
const PLAINTEXT_MAX_CHARS: usize = 20_000;
const THUMB_WIDTH: f32 = 512.0;

pub fn draw(app: &mut App, ui: &mut egui::Ui) {
    egui::Panel::left("topics")
        .default_size(240.0)
        .resizable(true)
        .frame(
            egui::Frame::new()
                .fill(style::PANEL_BG)
                .stroke(egui::Stroke::new(1.0, style::BORDER_LIGHT))
                .inner_margin(egui::Margin::symmetric(12, 16)),
        )
        .show(ui, |ui| topic_list(app, ui));

    // 模型 / 尺寸固定在消息区顶部，输入框与发送按钮固定在底部
    egui::Panel::top("generation_options")
        .resizable(false)
        .frame(
            egui::Frame::new()
                .fill(style::CHAT_BG)
                .stroke(egui::Stroke::new(1.0, style::BORDER_LIGHT))
                .inner_margin(egui::Margin { left: 16, right: 16, top: 10, bottom: 10 }),
        )
        .show(ui, |ui| {
            egui::ScrollArea::horizontal()
                .auto_shrink([false, false])
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .show(ui, |ui| options_bar(app, ui));
        });

    egui::Panel::bottom("composer")
        .resizable(false)
        .frame(egui::Frame::new().fill(style::CHAT_BG).inner_margin(egui::Margin { left: 24, right: 24, top: 6, bottom: 16 }))
        .show(ui, |ui| composer(app, ui));

    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(style::CHAT_BG).inner_margin(egui::Margin::symmetric(24, 12)))
        .show(ui, |ui| messages(app, ui));
}

fn topic_list(app: &mut App, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("话题").strong().color(style::MUTED));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.add(style::secondary("+ 新建")).clicked() {
                app.new_topic();
            }
        });
    });
    ui.add_space(8.0);
    let rows: Vec<_> = app
        .topics
        .iter()
        .map(|t| (t.id, t.title.clone(), t.title_source, t.title_error.clone()))
        .collect();
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 3.0;
        for (id, title, source, error) in rows {
            let selected = app.current_topic == Some(id);
            let label = if app.generating.contains(&id) { format!("{title} · 生成中") } else { title.clone() };
            let mut resp = style::sidebar_item(ui, selected, &label);
            let tip = match (&error, source) {
                (Some(err), _) => format!("{title}\n标题生成失败：{err}"),
                (None, TitleSource::Generated) => format!("{title}\n标题由模型生成"),
                _ => title.clone(),
            };
            resp = resp.on_hover_text(tip);
            if resp.clicked() {
                app.select_topic(Some(id));
            }
            resp.context_menu(|ui| {
                if ui.button("重命名").clicked() {
                    app.rename = Some((TitleFor::Topic(id), title.clone()));
                    ui.close();
                }
                if ui.button("重新生成标题").clicked() {
                    app.regenerate_title(TitleFor::Topic(id));
                    ui.close();
                }
                if ui.button(RichText::new("删除话题").color(style::ERROR)).clicked() {
                    app.confirm = Some(Confirm::DeleteTopic(id));
                    ui.close();
                }
            });
        }
    });
}

/// 顶部选项栏：模型下拉 + 尺寸分段控件。
fn options_bar(app: &mut App, ui: &mut egui::Ui) {
    let models = app.available_models();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let (selected, tooltip) = match app
            .chat_model
            .as_ref()
            .and_then(|(pid, m)| models.iter().find(|(p, _, am)| p == pid && am == m))
        {
            Some((_, name, m)) => (m.clone(), format!("供应商：{name}\n模型：{m}")),
            None => ("未选择模型".into(), "未选择模型".into()),
        };
        let mut picked = app.chat_model.clone();
        let model_label = ui.label(RichText::new("模型").strong().color(style::MUTED));
        let combo_w = (ui.available_width() * 0.28).clamp(140.0, 200.0);
        let _resp = style::combobox("chat_model")
            .width(combo_w)
            .selected_text(selected)
            .show_ui(ui, |ui| {
                for (pid, name, m) in &models {
                    ui.selectable_value(&mut picked, Some((*pid, m.clone())), format!("{name} / {m}"));
                }
            })
            .response
            .labelled_by(model_label.id)
            .on_hover_text(tooltip);
        if picked != app.chat_model {
            app.chat_model = picked;
        }
        ui.add_space(8.0);
        ui.label(RichText::new("尺寸").strong().color(style::MUTED));
        let seg_changed = style::segmented(ui, &mut app.chat_size, SIZES, |s| s.replace('x', " × "));
        if seg_changed {
            app.sync_custom_size_from_chat_size();
        }
        ui.add_space(2.0);
        custom_size_picker(app, ui);
        if models.is_empty() {
            ui.add_space(8.0);
            ui.label(RichText::new("请先在设置中添加供应商并保留模型").small().color(style::WARN));
        }
    });
}

/// 自定义宽高输入框：`[ 宽 ] × [ 高 ]`
fn custom_size_picker(app: &mut App, ui: &mut egui::Ui) {
    let mut changed = false;
    egui::Frame::new()
        .fill(egui::Color32::WHITE)
        .stroke(egui::Stroke::new(1.0, style::BORDER_LIGHT))
        .corner_radius(egui::CornerRadius::same(style::CONTROL_RADIUS))
        .inner_margin(egui::Margin::symmetric(6, 2))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.horizontal(|ui| {
                let w_edit = ui.add(
                    egui::TextEdit::singleline(&mut app.custom_width)
                        .hint_text("宽")
                        .desired_width(46.0)
                        .margin(egui::Margin::symmetric(4, 2))
                        .clip_text(false),
                );
                let w_lost = w_edit.lost_focus();
                let enter_pressed = ui.input(|i| i.key_pressed(egui::Key::Enter));
                if w_lost || (w_edit.has_focus() && enter_pressed) {
                    changed = true;
                }

                ui.label(RichText::new("×").color(style::MUTED).strong());

                let h_edit = ui.add(
                    egui::TextEdit::singleline(&mut app.custom_height)
                        .hint_text("高")
                        .desired_width(46.0)
                        .margin(egui::Margin::symmetric(4, 2))
                        .clip_text(false),
                );
                let h_lost = h_edit.lost_focus();
                if h_lost || (h_edit.has_focus() && enter_pressed) {
                    changed = true;
                }
            });
        });
    if changed {
        app.apply_custom_size();
    }
}

fn composer(app: &mut App, ui: &mut egui::Ui) {
    let blocker = app.send_blocker();
    let mut send = false;
    let edit_id = egui::Id::unique("composer_input");
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
                    send |=
                        ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter) && !i.modifiers.shift);
                }
                ui.add(
                    egui::TextEdit::multiline(&mut app.input)
                        .id(edit_id)
                        .font(egui::TextStyle::Body)
                        .frame(egui::Frame::NONE)
                        .hint_text("描述你想要的图片，Enter 发送，Shift+Enter 换行")
                        .desired_rows(3)
                        .desired_width(w),
                );
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
        app.send();
    }
}

fn messages(app: &mut App, ui: &mut egui::Ui) {
    if app.current_topic.is_none() {
        ui.centered_and_justified(|ui| {
            ui.label(RichText::new("新建一个话题，开始生成图片").color(style::MUTED).heading());
        });
        return;
    }
    let msgs = app.messages.clone();
    if msgs.is_empty() {
        ui.centered_and_justified(|ui| {
            ui.label(RichText::new("输入描述，生成第一张图片").color(style::MUTED).heading());
        });
        return;
    }
    let mut retry = None;
    egui::ScrollArea::vertical().stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
        // 右侧留出滚动条宽度，内容不贴边、不被浮动滚动条压住
        egui::Frame::new().inner_margin(egui::Margin { right: 20, ..Default::default() }).show(ui, |ui| {
            for m in &msgs {
                match m.role {
                    Role::User => {
                        if user_message(app, ui, m) {
                            retry = Some(m.id);
                        }
                    }
                    Role::Assistant => assistant_message(app, ui, m),
                }
                ui.add_space(16.0);
            }
        });
    });
    if let Some(id) = retry {
        app.retry(id);
    }
}

/// 返回是否点击了“重试”。
fn user_message(app: &mut App, ui: &mut egui::Ui, m: &Message) -> bool {
    let mut retry = false;
    ui.with_layout(egui::Layout::top_down(egui::Align::Max), |ui| {
        let meta = format!(
            "{} · {} · {}",
            m.model_id.as_deref().unwrap_or("-"),
            m.size.as_deref().unwrap_or("-"),
            m.created_at
        );
        ui.label(RichText::new(meta).small().color(style::MUTED));
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
        ui.horizontal(|ui| {
            let busy = app.generating.contains(&m.topic_id);
            if icons::button(ui, Icon::Copy, "复制提示词").clicked() {
                ui.ctx().copy_text(m.content.clone());
                app.show_toast("已复制提示词");
            }
            if icons::button(ui, Icon::Retry, "重试").clicked() && !busy {
                retry = true;
            }
        });
    });
    retry
}

fn assistant_message(app: &mut App, ui: &mut egui::Ui, m: &Message) {
    ui.label(RichText::new(format!("画图 · {}", m.created_at)).small().color(style::MUTED));
    match m.status {
        Status::Pending => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("生成中…（通常 30–60 秒，慢时可能数分钟）");
            });
        }
        Status::Interrupted => {
            ui.label(RichText::new("已中断：应用在请求过程中退出，可点击上方重试图标").color(style::WARN));
        }
        Status::Success | Status::Error => {
            // “原始结果”打开时替换图片 / 错误区域展示，关闭后恢复
            if app.raw_open.contains(&m.id) {
                raw_block(app, ui, m);
            } else {
                for img in &m.images {
                    thumbnail(app, ui, img);
                }
                if !m.failures.is_empty() {
                    egui::Frame::new()
                        .fill(style::WARN_BG)
                        .corner_radius(egui::CornerRadius::same(style::CARD_RADIUS))
                        .inner_margin(egui::Margin::same(8))
                        .show(ui, |ui| {
                            ui.label(RichText::new("以下图片来源获取失败：").color(style::WARN));
                            for f in &m.failures {
                                ui.add(egui::Label::new(RichText::new(f).small()).wrap().selectable(true));
                            }
                        });
                }
                if m.images.is_empty() {
                    error_block(app, ui, m);
                }
            }
            actions(app, ui, m);
        }
    }
}

/// 助手消息底部的单行图标：图片操作（打开 / 复制 / 所在目录）+ 原始结果开关。
fn actions(app: &mut App, ui: &mut egui::Ui, m: &Message) {
    ui.horizontal(|ui| {
        for img in &m.images {
            if icons::button(ui, Icon::Expand, "打开图片").clicked() {
                app.open_viewer(&img.local_path);
            }
            if icons::button(ui, Icon::Copy, "复制图片").clicked() {
                app.copy_image(&img.local_path);
            }
            if icons::button(ui, Icon::Folder, "打开所在目录").on_hover_text(&img.local_path).clicked()
                && let Some(dir) = std::path::Path::new(&img.local_path).parent()
                && let Err(err) = super::open_path(dir)
            {
                app.report(err);
            }
        }
        let open = app.raw_open.contains(&m.id);
        if icons::toggle(ui, Icon::Code, "原始结果", Some(open)).clicked() {
            app.toggle_raw(m.id);
        }
    });
}

/// “原始结果”：HTTP 状态码 + 响应体（格式化 JSON，base64 已替换为占位描述）。
/// 全高展开，不嵌套滚动区，由消息列表统一滚动。
fn raw_block(app: &mut App, ui: &mut egui::Ui, m: &Message) {
    egui::Frame::new().fill(style::CODE_BG).corner_radius(egui::CornerRadius::same(style::CARD_RADIUS)).inner_margin(egui::Margin::same(10)).show(
        ui,
        |ui| {
            ui.set_width(ui.available_width());
            let status = match (m.http_status, m.error.as_deref()) {
                (Some(code), Some(err)) if !err.starts_with("HTTP") => format!("HTTP {code} · {err}"),
                (Some(code), _) => format!("HTTP {code}"),
                (None, Some(err)) => err.to_string(),
                (None, None) => "无 HTTP 状态".to_string(),
            };
            ui.horizontal(|ui| {
                ui.label(RichText::new(status).small().color(style::MUTED).strong());
                if let Some(body) = m.response_body.clone() {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if icons::button(ui, Icon::Copy, "复制返回内容").clicked() {
                            ui.ctx().copy_text(body);
                            app.show_toast("已复制返回内容");
                        }
                    });
                }
            });
            match m.response_body.as_deref() {
                Some(_) => {
                    let body = app.body_cache.entry(m.id).or_insert_with(|| plaintext(m.response_body.as_deref()));
                    code_text(ui, body);
                }
                None => {
                    ui.label(RichText::new("没有响应体（请求未发出或本地即失败）").small().color(style::MUTED));
                }
            }
        },
    );
}

/// 只读、可选中复制的等宽文本，按内容全高展示。
fn code_text(ui: &mut egui::Ui, text: &str) {
    ui.add(egui::TextEdit::multiline(&mut &*text).code_editor().desired_width(f32::INFINITY).desired_rows(1));
}

/// 缩略图：单击在应用内预览；悬停显示格式、尺寸与保存路径。
fn thumbnail(app: &mut App, ui: &mut egui::Ui, img: &crate::db::ImageRecord) {
    let dims = match (img.width, img.height) {
        (Some(w), Some(h)) => format!("{} {w}x{h}", img.format.to_uppercase()),
        _ => img.format.to_uppercase(),
    };
    let tip = format!("{dims}\n{}\n点击预览", img.local_path);
    match app.texture(&img.local_path) {
        Texture::Loading => {
            ui.spinner();
        }
        Texture::Missing(reason) => {
            ui.label(RichText::new(reason.clone()).color(style::ERROR));
        }
        Texture::Ready(tex) => {
            let size = tex.size_vec2();
            let width = size.x.min(THUMB_WIDTH).min(ui.available_width());
            let shown = egui::vec2(width, size.y * width / size.x.max(1.0));
            let resp = ui
                .add(
                    egui::Image::from_texture(tex)
                        .fit_to_exact_size(shown)
                        .corner_radius(egui::CornerRadius::same(10))
                        .sense(egui::Sense::click()),
                )
                .on_hover_text(tip);
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, "缩略图"));
            if resp.clicked() {
                app.open_viewer(&img.local_path);
            }
        }
    }
}

fn error_block(app: &mut App, ui: &mut egui::Ui, m: &Message) {
    egui::Frame::new().fill(style::ERROR_BG).corner_radius(egui::CornerRadius::same(style::CARD_RADIUS)).inner_margin(egui::Margin::same(10)).show(
        ui,
        |ui| {
            ui.set_width(ui.available_width());
            let headline = match (m.error.as_deref(), m.http_status) {
                (Some(e), Some(code)) if !e.starts_with("HTTP") => format!("{e}（HTTP {code}）"),
                (Some(e), _) => e.lines().next().unwrap_or(e).chars().take(200).collect(),
                (None, Some(code)) => format!("HTTP {code}"),
                (None, None) => "生成失败".into(),
            };
            ui.label(RichText::new(headline).color(style::ERROR).strong());
            if let Some(body) = m.response_body.as_deref() {
                ui.label(RichText::new("上游返回内容：").color(style::MUTED));
                let shown = app.body_cache.entry(m.id).or_insert_with(|| plaintext(Some(body)));
                code_text(ui, shown);
            }
        },
    );
}

/// JSON 格式化后返回；过长时截断并提示完整内容在数据库中。
fn plaintext(body: Option<&str>) -> String {
    let raw = body.unwrap_or("");
    let pretty = serde_json::from_str::<serde_json::Value>(raw)
        .ok()
        .and_then(|v| serde_json::to_string_pretty(&v).ok())
        .unwrap_or_else(|| raw.to_string());
    if pretty.chars().count() > PLAINTEXT_MAX_CHARS {
        let mut cut: String = pretty.chars().take(PLAINTEXT_MAX_CHARS).collect();
        cut.push_str("\n…（已截断，完整内容见数据库）");
        cut
    } else {
        pretty
    }
}
