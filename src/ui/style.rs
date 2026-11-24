//! 视觉风格：macOS 原生风格（浅灰侧栏、白色内容区、分组卡片、分段控件），主色翠绿。
//! 文字大小统一由 [`set_font_size`] 派生，界面代码只用 `small()` / `strong()` / `heading()` 等相对样式。

use eframe::egui::{self, Color32, CornerRadius, Rect, RichText, Sense, Shadow, Stroke, TextStyle, Vec2, Visuals, pos2, vec2};

pub const ACCENT: Color32 = Color32::from_rgb(0x05, 0x96, 0x69);
pub const ACCENT_HOVER: Color32 = Color32::from_rgb(0x04, 0x78, 0x57);
pub const ACCENT_BG: Color32 = Color32::from_rgb(0xec, 0xfd, 0xf5);
/// 侧栏底色（macOS 浅色侧栏）。
pub const PANEL_BG: Color32 = Color32::from_rgb(0xf6, 0xf7, 0xf9);
/// 系统设置式分组页面底色，卡片为白色。
pub const GROUPED_BG: Color32 = Color32::from_rgb(0xf8, 0xf9, 0xfa);
pub const BORDER: Color32 = Color32::from_rgb(0xe2, 0xe4, 0xe9);
pub const BORDER_LIGHT: Color32 = Color32::from_rgb(0xeb, 0xed, 0xf0);
pub const MUTED: Color32 = Color32::from_rgb(0x64, 0x74, 0x8b);
pub const TEXT: Color32 = Color32::from_rgb(0x1e, 0x29, 0x3b);
pub const ERROR: Color32 = Color32::from_rgb(0xdc, 0x26, 0x26);
pub const ERROR_BG: Color32 = Color32::from_rgb(0xfe, 0xf2, 0xf2);
pub const ERROR_BORDER: Color32 = Color32::from_rgb(0xfe, 0xca, 0xca);
pub const WARN: Color32 = Color32::from_rgb(0xd9, 0x77, 0x06);
pub const WARN_BG: Color32 = Color32::from_rgb(0xff, 0xfb, 0xeb);
pub const USER_BUBBLE: Color32 = Color32::from_rgb(0xec, 0xfd, 0xf5);
pub const ASSISTANT_BUBBLE: Color32 = Color32::from_rgb(0xf8, 0xfa, 0xfc);
pub const ASSISTANT_BORDER: Color32 = Color32::from_rgb(0xe2, 0xe8, 0xf0);
pub const CHAT_BG: Color32 = Color32::WHITE;
pub const CODE_BG: Color32 = Color32::from_rgb(0xf8, 0xf9, 0xfa);
pub const VIEWER_BG: Color32 = Color32::from_rgb(0xf1, 0xf3, 0xf5);
pub const FOCUS_BORDER: Color32 = Color32::from_rgb(0x05, 0x96, 0x69);
/// 分段控件槽底色。
const TRACK_BG: Color32 = Color32::from_rgb(0xeb, 0xed, 0xf0);
const HOVER_BG: Color32 = Color32::from_rgba_premultiplied(0, 0, 0, 10);

/// 控件圆角 7px，卡片 / 弹窗 10px。
pub const CONTROL_RADIUS: u8 = 7;
pub const CARD_RADIUS: u8 = 10;

pub fn apply(ctx: &egui::Context, font_size: u8) {
    ctx.set_theme(egui::Theme::Light);
    let mut v = Visuals::light();
    v.panel_fill = Color32::WHITE;
    v.window_fill = Color32::WHITE;
    v.extreme_bg_color = Color32::WHITE;
    v.text_edit_bg_color = Some(Color32::WHITE);
    v.faint_bg_color = GROUPED_BG;
    v.code_bg_color = CODE_BG;
    v.selection.bg_fill = Color32::from_rgb(0xc8, 0xef, 0xdc);
    v.selection.stroke = Stroke::new(1.0, ACCENT);
    v.hyperlink_color = ACCENT;
    v.override_text_color = Some(TEXT);
    v.text_cursor.stroke = Stroke::new(2.0, ACCENT);
    v.slider_trailing_fill = true;

    let r = CornerRadius::same(CONTROL_RADIUS);
    let w = &mut v.widgets;
    w.noninteractive.corner_radius = r;
    w.noninteractive.bg_fill = Color32::WHITE;
    w.noninteractive.weak_bg_fill = Color32::WHITE;
    w.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
    w.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
    for (state, fill, stroke) in [
        (&mut w.inactive, Color32::WHITE, Color32::from_rgb(0xdb, 0xde, 0xe4)),
        (&mut w.hovered, Color32::from_rgb(0xf8, 0xfa, 0xfc), Color32::from_rgb(0xc4, 0xcb, 0xd5)),
        (&mut w.active, Color32::from_rgb(0xf1, 0xf5, 0xf9), ACCENT),
        (&mut w.open, Color32::from_rgb(0xf8, 0xfa, 0xfc), Color32::from_rgb(0xc4, 0xcb, 0xd5)),
    ] {
        state.corner_radius = r;
        state.weak_bg_fill = fill;
        state.bg_fill = TRACK_BG;
        state.bg_stroke = Stroke::new(1.0, stroke);
        state.fg_stroke = Stroke::new(1.0, TEXT);
        state.expansion = 0.0;
    }

    v.window_corner_radius = CornerRadius::same(12);
    v.window_stroke = Stroke::new(1.0, BORDER);
    v.window_shadow = Shadow { offset: [0, 8], blur: 24, spread: 0, color: Color32::from_black_alpha(20) };
    v.menu_corner_radius = CornerRadius::same(8);
    v.popup_shadow = Shadow { offset: [0, 4], blur: 16, spread: 0, color: Color32::from_black_alpha(18) };
    ctx.set_visuals_of(egui::Theme::Light, v);

    ctx.all_styles_mut(|s| {
        s.spacing.item_spacing = vec2(8.0, 7.0);
        s.spacing.button_padding = vec2(12.0, 6.0);
        s.spacing.interact_size.y = 30.0;
        s.spacing.window_margin = egui::Margin::same(16);
        s.spacing.menu_margin = egui::Margin::same(6);
        s.spacing.icon_width = 14.0;
        s.spacing.icon_spacing = 8.0;
        s.spacing.scroll = egui::style::ScrollStyle::floating();
    });
    set_font_size(ctx, font_size);
}

/// 按正文字号派生全部文字样式：小字 −2、标题 +5、按钮 / 等宽同正文。
pub fn set_font_size(ctx: &egui::Context, size: u8) {
    let body = f32::from(crate::db::clamp_font_size(size));
    ctx.all_styles_mut(|s| {
        use egui::FontFamily::{Monospace, Proportional};
        s.text_styles = [
            (TextStyle::Small, egui::FontId::new(body - 2.0, Proportional)),
            (TextStyle::Body, egui::FontId::new(body, Proportional)),
            (TextStyle::Button, egui::FontId::new(body, Proportional)),
            (TextStyle::Heading, egui::FontId::new(body + 5.0, Proportional)),
            (TextStyle::Monospace, egui::FontId::new(body, Monospace)),
        ]
        .into();
    });
}

/// 绿色主按钮。
pub fn primary(text: &str) -> egui::Button<'_> {
    egui::Button::new(RichText::new(text).color(Color32::WHITE).strong())
        .fill(ACCENT)
        .corner_radius(CornerRadius::same(CONTROL_RADIUS))
        .stroke(Stroke::NONE)
}

/// 次级线框按钮（白色背景 + 细边框）。
pub fn secondary(text: &str) -> egui::Button<'_> {
    egui::Button::new(RichText::new(text).color(TEXT))
        .fill(Color32::WHITE)
        .corner_radius(CornerRadius::same(CONTROL_RADIUS))
        .stroke(Stroke::new(1.0, BORDER))
}

/// 危险操作按钮（淡红底 + 红字）。
pub fn danger_button(text: &str) -> egui::Button<'_> {
    egui::Button::new(RichText::new(text).color(ERROR).strong())
        .fill(ERROR_BG)
        .corner_radius(CornerRadius::same(CONTROL_RADIUS))
        .stroke(Stroke::new(1.0, ERROR_BORDER))
}

/// 幽灵小按钮（无背景，悬停时浅灰）。
pub fn ghost_button(text: &str) -> egui::Button<'_> {
    egui::Button::new(RichText::new(text).color(MUTED))
        .fill(Color32::TRANSPARENT)
        .corner_radius(CornerRadius::same(CONTROL_RADIUS))
        .stroke(Stroke::NONE)
}

/// 危险小按钮（如删除列表项）。
pub fn danger_ghost_button(text: &str) -> egui::Button<'_> {
    egui::Button::new(RichText::new(text).color(ERROR))
        .fill(Color32::TRANSPARENT)
        .corner_radius(CornerRadius::same(CONTROL_RADIUS))
        .stroke(Stroke::NONE)
}


/// 绘制精致现代的下拉箭头（Chevron Down）
pub fn paint_chevron_down(painter: &egui::Painter, rect: Rect, stroke: Stroke) {
    let c = rect.center();
    let w = 4.0;
    let h = 2.5;
    painter.line(
        vec![
            pos2(c.x - w, c.y - h),
            pos2(c.x, c.y + h),
            pos2(c.x + w, c.y - h),
        ],
        stroke,
    );
}

/// 创建带有精致 Chevron 箭头的 ComboBox
pub fn combobox<'a>(id_salt: impl std::hash::Hash + std::fmt::Debug) -> egui::ComboBox<'a> {
    egui::ComboBox::from_id_salt(id_salt).icon(|ui, rect, visuals, _is_open| {
        let color = visuals.fg_stroke.color;
        paint_chevron_down(ui.painter(), rect, Stroke::new(1.4, color));
    })
}

/// 现代化 Switch 开关（替代生硬的默认 Checkbox）
pub fn toggle_switch(ui: &mut egui::Ui, on: &mut bool, label: &str) -> egui::Response {
    let height = 20.0;
    let width = 36.0;
    let radius = height / 2.0;
    let (rect, mut resp) = ui.allocate_exact_size(vec2(width, height), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Switch, ui.is_enabled(), *on, label));
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    if ui.is_rect_visible(rect) {
        let track_fill = if *on {
            ACCENT
        } else if resp.hovered() {
            Color32::from_rgb(0xd8, 0xdc, 0xe2)
        } else {
            Color32::from_rgb(0xe2, 0xe4, 0xe9)
        };
        ui.painter().rect_filled(rect, CornerRadius::same(radius as u8), track_fill);
        let knob_radius = radius - 2.0;
        let knob_x = if *on {
            rect.right() - radius
        } else {
            rect.left() + radius
        };
        let knob_center = pos2(knob_x, rect.center().y);
        ui.painter().circle_filled(
            pos2(knob_center.x, knob_center.y + 0.5),
            knob_radius,
            Color32::from_black_alpha(20),
        );
        ui.painter().circle_filled(knob_center, knob_radius, Color32::WHITE);
    }
    resp
}

/// 现代美化复选框（带平滑选中高亮与白色对勾）
pub fn custom_checkbox(ui: &mut egui::Ui, checked: &mut bool, label: &str) -> egui::Response {
    let size = 16.0;
    let label_galley = egui::WidgetText::from(label).into_galley(
        ui,
        None,
        ui.available_width() - size - 8.0,
        TextStyle::Body,
    );
    let total_w = size + 8.0 + label_galley.size().x;
    let total_h = size.max(label_galley.size().y);
    let (rect, mut resp) = ui.allocate_exact_size(vec2(total_w, total_h), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, ui.is_enabled(), *checked, label));
    if resp.clicked() {
        *checked = !*checked;
        resp.mark_changed();
    }
    if ui.is_rect_visible(rect) {
        let box_rect = Rect::from_min_size(
            pos2(rect.left(), rect.center().y - size / 2.0),
            Vec2::splat(size),
        );
        let r = CornerRadius::same(4);
        if *checked {
            ui.painter().rect_filled(box_rect, r, ACCENT);
            let p1 = pos2(box_rect.left() + 4.0, box_rect.center().y);
            let p2 = pos2(box_rect.left() + 7.0, box_rect.bottom() - 4.5);
            let p3 = pos2(box_rect.right() - 3.5, box_rect.top() + 4.5);
            ui.painter().line(vec![p1, p2, p3], Stroke::new(1.8, Color32::WHITE));
        } else {
            let bg = if resp.hovered() { Color32::from_rgb(0xf8, 0xfa, 0xfc) } else { Color32::WHITE };
            let stroke_color = if resp.hovered() { Color32::from_rgb(0xa1, 0xa1, 0xaa) } else { BORDER };
            ui.painter().rect_filled(box_rect, r, bg);
            ui.painter().rect_stroke(box_rect, r, Stroke::new(1.2, stroke_color), egui::StrokeKind::Inside);
        }
        let text_pos = pos2(box_rect.right() + 8.0, rect.center().y - label_galley.size().y / 2.0);
        ui.painter().galley(text_pos, label_galley, TEXT);
    }
    resp
}
/// 胶囊标签（启用/禁用等）：微细边框 + 低饱和底色，精致小徽标。
pub fn pill(ui: &mut egui::Ui, text: &str, fg: Color32, bg: Color32) {
    egui::Frame::new()
        .fill(bg)
        .corner_radius(CornerRadius::same(4))
        .inner_margin(egui::Margin::symmetric(6, 2))
        .show(ui, |ui| ui.label(RichText::new(text).small().color(fg)));
}

/// 白色圆角卡片（系统设置式分组）。
pub fn card() -> egui::Frame {
    egui::Frame::new()
        .fill(Color32::WHITE)
        .stroke(Stroke::new(1.0, BORDER_LIGHT))
        .corner_radius(CornerRadius::same(CARD_RADIUS))
        .shadow(Shadow {
            offset: [0, 2],
            blur: 8,
            spread: 0,
            color: Color32::from_black_alpha(10),
        })
        .inner_margin(egui::Margin::symmetric(16, 12))
}

/// 分组：卡片上方的小号灰色标题 + 白色卡片。
pub fn group<R>(ui: &mut egui::Ui, caption: &str, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    ui.label(RichText::new(caption).small().strong().color(MUTED));
    let inner = card()
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner;
    ui.add_space(14.0);
    inner
}

/// 返回标签的 Response，用于给输入框 `labelled_by` 绑定无障碍名称。
pub fn section_title(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.label(RichText::new(text).strong())
}

/// 卡片内一行（系统设置式）：左侧标签，右侧控件右对齐。`add` 收到标签 id，用于 `labelled_by`。
pub fn form_row<R>(ui: &mut egui::Ui, label: &str, add: impl FnOnce(&mut egui::Ui, egui::Id) -> R) -> R {
    ui.horizontal(|ui| {
        ui.set_min_height(ui.spacing().interact_size.y + 4.0);
        let l = ui.label(label);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| add(ui, l.id)).inner
    })
    .inner
}

/// 卡片内行间分隔线。
pub fn row_separator(ui: &mut egui::Ui) {
    let r = ui.available_rect_before_wrap();
    let y = r.top() + 3.0;
    ui.painter().hline(r.x_range(), y, Stroke::new(1.0, Color32::from_rgb(0xee, 0xee, 0xf0)));
    ui.add_space(7.0);
}

/// 系统设置式页面背景（浅灰）+ 内边距，内容区可滚动。
pub fn grouped_page(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(GROUPED_BG).inner_margin(egui::Margin::symmetric(28, 20)))
        .show(ui, |ui| {
            egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                ui.set_max_width(720.0);
                add(ui);
            });
        });
}

/// 侧栏行（话题、设置分类）：整行可点，现代 macOS / Linear 灰度层次设计，选中浅冷灰底 + 深色文字，悬停微弱背景。
pub fn sidebar_item(ui: &mut egui::Ui, selected: bool, text: &str) -> egui::Response {
    let height = ui.text_style_height(&TextStyle::Button) + 14.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), selected, text));
    if ui.is_rect_visible(rect) {
        let fill = if selected {
            Color32::from_rgb(0xe8, 0xeb, 0xf0)
        } else if resp.hovered() {
            Color32::from_rgb(0xf1, 0xf3, 0xf6)
        } else {
            Color32::TRANSPARENT
        };
        ui.painter().rect_filled(rect, CornerRadius::same(CONTROL_RADIUS), fill);
        let color = if selected {
            Color32::from_rgb(0x0f, 0x17, 0x2a)
        } else {
            Color32::from_rgb(0x47, 0x55, 0x69)
        };
        let text_weight = if selected { RichText::new(text).color(color).strong() } else { RichText::new(text).color(color) };
        let galley = egui::WidgetText::from(text_weight).into_galley(
            ui,
            Some(egui::TextWrapMode::Truncate),
            rect.width() - 20.0,
            TextStyle::Button,
        );
        let pos = egui::pos2(rect.left() + 10.0, rect.center().y - galley.size().y / 2.0);
        ui.painter().galley(pos, galley, color);
    }
    resp
}

/// 铺满宽度的页签条（如聊天页“助手 | 话题”）：浅灰槽，选中页签为主色浅底 + 主色字。
/// 无障碍名称为页签文字；返回被点击的页签下标。
pub fn tab_strip(ui: &mut egui::Ui, labels: &[&str], selected: usize) -> Option<usize> {
    let h = ui.spacing().interact_size.y + 6.0;
    let (outer, _) = ui.allocate_exact_size(vec2(ui.available_width(), h + 6.0), Sense::hover());
    ui.painter().rect_filled(outer, CornerRadius::same(CARD_RADIUS), TRACK_BG);
    let inner = outer.shrink(3.0);
    let w = inner.width() / labels.len() as f32;
    let mut clicked = None;
    for (i, label) in labels.iter().enumerate() {
        let rect = Rect::from_min_size(pos2(inner.left() + w * i as f32, inner.top()), vec2(w, inner.height()));
        let id = ui.scope_id().with(("tab_strip", label));
        let resp = ui.interact(rect, id, Sense::click());
        let on = i == selected;
        resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Tab, ui.is_enabled(), on, *label));
        if on {
            ui.painter().rect_filled(rect, CornerRadius::same(CONTROL_RADIUS + 1), ACCENT_BG);
        } else if resp.hovered() {
            ui.painter().rect_filled(rect, CornerRadius::same(CONTROL_RADIUS + 1), HOVER_BG);
        }
        let color = if on { ACCENT } else { MUTED };
        let text = if on { RichText::new(*label).strong() } else { RichText::new(*label) };
        let galley = egui::WidgetText::from(text.color(color)).into_galley(ui, None, f32::INFINITY, TextStyle::Button);
        ui.painter().galley(rect.center() - galley.size() / 2.0, galley, color);
        if resp.clicked() {
            clicked = Some(i);
        }
    }
    clicked
}

/// 带首字圆形徽标的侧栏行（助手列表）：选中为主色浅底。
pub fn avatar_item(ui: &mut egui::Ui, selected: bool, text: &str) -> egui::Response {
    let height = ui.text_style_height(&TextStyle::Button) + 22.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), selected, text));
    if ui.is_rect_visible(rect) {
        let fill = if selected {
            ACCENT_BG
        } else if resp.hovered() {
            Color32::from_rgb(0xf1, 0xf3, 0xf6)
        } else {
            Color32::TRANSPARENT
        };
        ui.painter().rect_filled(rect, CornerRadius::same(CARD_RADIUS), fill);
        let r = 13.0;
        let c = pos2(rect.left() + 10.0 + r, rect.center().y);
        ui.painter().circle_filled(c, r, if selected { Color32::from_rgb(0xc8, 0xef, 0xdc) } else { ACCENT_BG });
        let initial: String = text.chars().next().map(|ch| ch.to_lowercase().collect()).unwrap_or_default();
        ui.painter().text(c, egui::Align2::CENTER_CENTER, initial, egui::FontId::proportional(12.0), ACCENT);
        let galley = egui::WidgetText::from(RichText::new(text).color(TEXT)).into_galley(
            ui,
            Some(egui::TextWrapMode::Truncate),
            rect.right() - c.x - r - 22.0,
            TextStyle::Button,
        );
        let pos = pos2(c.x + r + 12.0, rect.center().y - galley.size().y / 2.0);
        ui.painter().galley(pos, galley, TEXT);
    }
    resp
}

/// macOS 分段控件：灰色槽内一组等宽分段，选中段为白底浮起。
/// `display` 把选项值转成显示文字；无障碍名称为选项值本身。返回是否改变。
pub fn segmented(
    ui: &mut egui::Ui,
    current: &mut String,
    options: &[&str],
    display: impl Fn(&str) -> String,
) -> bool {
    let mut changed = false;
    let seg_h = ui.spacing().interact_size.y - 4.0;
    let galleys: Vec<_> = options
        .iter()
        .map(|o| {
            let color = if current == o { TEXT } else { MUTED };
            egui::WidgetText::from(RichText::new(display(o)).color(color)).into_galley(
                ui,
                None,
                f32::INFINITY,
                TextStyle::Button,
            )
        })
        .collect();
    let seg_w = galleys.iter().map(|g| g.size().x).fold(0.0, f32::max) + 16.0;
    egui::Frame::new()
        .fill(TRACK_BG)
        .corner_radius(CornerRadius::same(CONTROL_RADIUS + 1))
        .inner_margin(egui::Margin::same(2))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            ui.horizontal(|ui| {
                for (opt, galley) in options.iter().zip(galleys) {
                    let selected = current == opt;
                    let (rect, resp) = ui.allocate_exact_size(vec2(seg_w, seg_h), Sense::click());
                    resp.widget_info(|| {
                        egui::WidgetInfo::selected(egui::Role::RadioButton, ui.is_enabled(), selected, *opt)
                    });
                    if selected {
                        ui.painter().add(
                            Shadow { offset: [0, 1], blur: 3, spread: 0, color: Color32::from_black_alpha(30) }
                                .as_shape(rect, CornerRadius::same(CONTROL_RADIUS)),
                        );
                        ui.painter().rect_filled(rect, CornerRadius::same(CONTROL_RADIUS), Color32::WHITE);
                    } else if resp.hovered() {
                        ui.painter().rect_filled(rect, CornerRadius::same(CONTROL_RADIUS), HOVER_BG);
                    }
                    let color = if selected { TEXT } else { MUTED };
                    let pos = rect.center() - galley.size() / 2.0;
                    ui.painter().galley(pos, galley, color);
                    if resp.clicked() && !selected {
                        *current = opt.to_string();
                        changed = true;
                    }
                }
            });
        });
    changed
}
