//! 矢量线条图标按钮：用 painter 绘制，不依赖图标字体；悬停显示文字提示，无障碍名称即提示文字。

use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Vec2, pos2, vec2};

use super::style;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Copy,
    Retry,
    Expand,
    Folder,
    Code,
    Image,
    FitWindow,
    ActualSize,
    Close,
    Sparkle,
    Gear,
    Chat,
}
const SIZE: f32 = 28.0;
const GLYPH: f32 = 16.0;

/// 左侧导航项：图标在上、文字在下，选中为主色浅底 + 主色图标。
pub fn nav_item(ui: &mut egui::Ui, icon: Icon, label: &str, selected: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(52.0, 50.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), selected, label));
    if ui.is_rect_visible(rect) {
        let bg = if selected {
            style::ACCENT_BG
        } else if resp.hovered() {
            Color32::from_black_alpha(12)
        } else {
            Color32::TRANSPARENT
        };
        ui.painter().rect_filled(rect, egui::CornerRadius::same(10), bg);
        let color = if selected { style::ACCENT } else { style::MUTED };
        let glyph = Rect::from_center_size(pos2(rect.center().x, rect.top() + 17.0), Vec2::splat(18.0));
        paint(ui.painter(), glyph, icon, Stroke::new(1.6, color));
        ui.painter().text(
            pos2(rect.center().x, rect.bottom() - 11.0),
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::proportional(11.0),
            color,
        );
    }
    resp
}

/// 图标按钮。`active` 为开关类按钮的选中态（如“原始结果”）。
pub fn button(ui: &mut egui::Ui, icon: Icon, label: &str) -> egui::Response {
    toggle(ui, icon, label, None)
}

pub fn toggle(ui: &mut egui::Ui, icon: Icon, label: &str, active: Option<bool>) -> egui::Response {
    let enabled = ui.is_enabled();
    let (rect, resp) = ui.allocate_exact_size(vec2(SIZE, SIZE), Sense::click());
    resp.widget_info(|| match active {
        Some(on) => egui::WidgetInfo::selected(egui::Role::Button, enabled, on, label),
        None => egui::WidgetInfo::labeled(egui::Role::Button, enabled, label),
    });
    if ui.is_rect_visible(rect) {
        let on = active.unwrap_or(false);
        let bg = if on {
            style::ACCENT_BG
        } else if resp.hovered() && enabled {
            Color32::from_rgb(0xf0, 0xf1, 0xf3)
        } else {
            Color32::TRANSPARENT
        };
        ui.painter().rect_filled(rect, egui::CornerRadius::same(6), bg);
        let color = if !enabled {
            Color32::from_rgb(0xc4, 0xc8, 0xce)
        } else if on || resp.hovered() {
            style::ACCENT
        } else {
            style::MUTED
        };
        let glyph = Rect::from_center_size(rect.center(), Vec2::splat(GLYPH));
        paint(ui.painter(), glyph, icon, Stroke::new(1.5, color));
    }
    resp.on_hover_text(label)
}

fn paint(p: &egui::Painter, r: Rect, icon: Icon, s: Stroke) {
    let at = |x: f32, y: f32| pos2(r.left() + r.width() * x, r.top() + r.height() * y);
    let radius = egui::CornerRadius::same(2);
    match icon {
        Icon::Copy => {
            p.rect_stroke(Rect::from_min_max(at(0.35, 0.35), at(0.95, 0.95)), radius, s, egui::StrokeKind::Middle);
            p.line(vec![at(0.65, 0.2), at(0.65, 0.05), at(0.05, 0.05), at(0.05, 0.65), at(0.2, 0.65)], s);
        }
        Icon::Retry => {
            let c = r.center();
            let rad = r.width() * 0.4;
            let pts: Vec<Pos2> = (0..=24)
                .map(|i| {
                    let a = -0.35 * std::f32::consts::PI + i as f32 / 24.0 * 1.6 * std::f32::consts::PI;
                    c + vec2(a.cos(), a.sin()) * rad
                })
                .collect();
            let tip = pts[0];
            p.line(pts, s);
            p.line(vec![tip + vec2(-4.5, -1.0), tip, tip + vec2(-0.5, -4.5)], s);
        }
        Icon::Expand => {
            p.line(vec![at(0.05, 0.4), at(0.05, 0.05), at(0.4, 0.05)], s);
            p.line(vec![at(0.6, 0.95), at(0.95, 0.95), at(0.95, 0.6)], s);
            p.line_segment([at(0.05, 0.05), at(0.42, 0.42)], s);
            p.line_segment([at(0.95, 0.95), at(0.58, 0.58)], s);
        }
        Icon::Folder => {
            p.line(
                vec![
                    at(0.05, 0.85),
                    at(0.05, 0.15),
                    at(0.38, 0.15),
                    at(0.48, 0.3),
                    at(0.95, 0.3),
                    at(0.95, 0.85),
                    at(0.05, 0.85),
                ],
                s,
            );
        }
        Icon::Code => {
            p.line(vec![at(0.32, 0.2), at(0.05, 0.5), at(0.32, 0.8)], s);
            p.line(vec![at(0.68, 0.2), at(0.95, 0.5), at(0.68, 0.8)], s);
            p.line_segment([at(0.58, 0.12), at(0.42, 0.88)], s);
        }
        Icon::Image => {
            p.rect_stroke(Rect::from_min_max(at(0.05, 0.12), at(0.95, 0.88)), radius, s, egui::StrokeKind::Middle);
            p.circle_stroke(at(0.33, 0.37), r.width() * 0.08, s);
            p.line(vec![at(0.1, 0.82), at(0.4, 0.55), at(0.58, 0.7), at(0.72, 0.58), at(0.92, 0.78)], s);
        }
        Icon::FitWindow => {
            p.line(vec![at(0.05, 0.35), at(0.05, 0.05), at(0.35, 0.05)], s);
            p.line(vec![at(0.65, 0.05), at(0.95, 0.05), at(0.95, 0.35)], s);
            p.line(vec![at(0.95, 0.65), at(0.95, 0.95), at(0.65, 0.95)], s);
            p.line(vec![at(0.35, 0.95), at(0.05, 0.95), at(0.05, 0.65)], s);
        }
        Icon::ActualSize => {
            p.line(vec![at(0.12, 0.3), at(0.25, 0.2), at(0.25, 0.8)], s);
            p.circle_stroke(at(0.5, 0.38), 0.6, s);
            p.circle_stroke(at(0.5, 0.65), 0.6, s);
            p.line(vec![at(0.68, 0.3), at(0.8, 0.2), at(0.8, 0.8)], s);
        }
        Icon::Close => {
            p.line_segment([at(0.15, 0.15), at(0.85, 0.85)], s);
            p.line_segment([at(0.85, 0.15), at(0.15, 0.85)], s);
        }
        Icon::Sparkle => {
            // 大四角星 + 右上小星
            let (c, a, b) = (at(0.42, 0.58), r.width() * 0.4, r.width() * 0.1);
            let star = |c: Pos2, a: f32, b: f32| {
                vec![
                    c + vec2(0.0, -a),
                    c + vec2(b, -b),
                    c + vec2(a, 0.0),
                    c + vec2(b, b),
                    c + vec2(0.0, a),
                    c + vec2(-b, b),
                    c + vec2(-a, 0.0),
                    c + vec2(-b, -b),
                    c + vec2(0.0, -a),
                ]
            };
            p.line(star(c, a, b), s);
            p.line(star(at(0.84, 0.16), r.width() * 0.16, r.width() * 0.045), s);
        }
        Icon::Gear => {
            let c = r.center();
            p.circle_stroke(c, r.width() * 0.16, s);
            p.circle_stroke(c, r.width() * 0.33, s);
            for i in 0..8 {
                let ang = i as f32 * std::f32::consts::FRAC_PI_4;
                let d = vec2(ang.cos(), ang.sin());
                p.line_segment([c + d * r.width() * 0.33, c + d * r.width() * 0.48], s);
            }
        }
        Icon::Chat => {
            // 圆角对话框 + 左下尾巴 + 两行文字
            p.rect_stroke(Rect::from_min_max(at(0.05, 0.1), at(0.95, 0.72)), egui::CornerRadius::same(4), s, egui::StrokeKind::Middle);
            p.line(vec![at(0.25, 0.72), at(0.18, 0.93), at(0.45, 0.72)], s);
            p.line_segment([at(0.28, 0.33), at(0.72, 0.33)], s);
            p.line_segment([at(0.28, 0.5), at(0.58, 0.5)], s);
        }
    }
}
