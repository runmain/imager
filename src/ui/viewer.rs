//! 应用内图片预览：窗口中央的浮层，按窗口大小等比缩小显示（不放大）。
//! 预览区内滚轮缩放、按住拖动平移、双击复原；左上角关闭（macOS 习惯），点遮罩 / Esc 也可关闭。

use eframe::egui::{self, RichText};

use super::icons::{self, Icon};
use super::style;
use crate::app::{App, Texture};

/// 预览图最多占窗口宽高的比例。
const MAX_FRACTION: f32 = 0.8;

pub fn draw(app: &mut App, ctx: &egui::Context) {
    let Some(path) = app.viewer.as_ref().map(|v| v.path.clone()) else { return };
    let max = ctx.content_rect().size() * MAX_FRACTION;
    let mut close = false;
    let resp = egui::Modal::new(egui::Id::unique("image_preview"))
        .backdrop_color(egui::Color32::from_black_alpha(120))
        .frame(
            egui::Frame::new()
                .fill(egui::Color32::WHITE)
                .stroke(egui::Stroke::new(1.0, style::BORDER_LIGHT))
                .corner_radius(egui::CornerRadius::same(style::CARD_RADIUS))
                .shadow(egui::Shadow {
                    offset: [0, 8],
                    blur: 24,
                    spread: 0,
                    color: egui::Color32::from_black_alpha(24),
                })
                .inner_margin(egui::Margin::same(12)),
        )
        .show(ctx, |ui| {
            close = toolbar(app, ui, &path);
            ui.add_space(8.0);
            preview(app, ui, &path, max);
        });
    if close || resp.should_close() {
        app.viewer = None;
    }
}

/// 左侧：关闭、文件名、尺寸；右侧：复制图片。返回是否点击了关闭。
fn toolbar(app: &mut App, ui: &mut egui::Ui, path: &str) -> bool {
    let mut close = false;
    ui.horizontal(|ui| {
        close = icons::button(ui, Icon::Close, "关闭").clicked();
        let name = std::path::Path::new(path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string());
        ui.label(RichText::new(name).strong());
        if let Texture::Ready(tex) = app.full_texture(path) {
            let s = tex.size_vec2();
            ui.label(RichText::new(format!("{}x{}", s.x as u32, s.y as u32)).small().color(style::MUTED));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if icons::button(ui, Icon::Copy, "复制图片").clicked() {
                app.copy_image(path);
            }
            ui.label(RichText::new("滚轮缩放 · 拖动平移 · 双击复原").small().color(style::MUTED));
        });
    });
    close
}

fn preview(app: &mut App, ui: &mut egui::Ui, path: &str, max: egui::Vec2) {
    let tex = match app.full_texture(path) {
        Texture::Loading => {
            ui.allocate_ui(egui::vec2(240.0, 160.0), |ui| ui.centered_and_justified(|ui| ui.spinner()));
            return;
        }
        Texture::Missing(reason) => {
            ui.label(RichText::new(reason.clone()).color(style::ERROR));
            return;
        }
        Texture::Ready(tex) => tex.clone(),
    };
    let image = tex.size_vec2();
    let frame = fit_size(image, max);
    let Some(viewer) = app.viewer.as_mut() else { return };
    // 可视区域（场景坐标）；初始为整张图，即按 frame 大小完整显示
    let full = egui::Rect::from_min_size(egui::Pos2::ZERO, image);
    if viewer.scene_rect == egui::Rect::NOTHING {
        viewer.scene_rect = full;
    }
    let mut scene_rect = viewer.scene_rect;
    ui.vertical_centered(|ui| {
        ui.allocate_ui(frame, |ui| {
            ui.set_min_size(frame);
            // 滚轮缩放降速：每格约 ±5%，单帧最多 ±20%。Scene 按“1 + 滚动量×速度”累加，
            // 触控板 / 惯性滚动单帧量很大时会直接缩到最小，因此先限幅
            const ZOOM_SPEED: f32 = 1.0 / 1000.0;
            const MAX_FRAME_SCROLL: f32 = 200.0;
            ui.ctx().options_mut(|o| o.input_options.scroll_zoom_speed = ZOOM_SPEED);
            ui.ctx().input_mut(|i| {
                let d = i.smooth_scroll_delta;
                let total = (d.x + d.y).clamp(-MAX_FRAME_SCROLL, MAX_FRAME_SCROLL);
                i.smooth_scroll_delta = egui::vec2(0.0, total);
            });
            let resp = egui::Scene::new()
                .zoom_range(0.1..=16.0)
                // 滚轮直接缩放（默认是平移，需按 Ctrl 才缩放）；平移靠按住拖动
                .scroll_zooms(true)
                .show(ui, &mut scene_rect, |ui| {
                    // 图片固定放在场景原点，与初始可视区（整张图）重合。不能交给布局摆放：
                    // 场景内继承外层的居中布局，图片会被摆在场景默认内区（1000 宽）的中线上而偏移、被裁切
                    ui.put(full, egui::Image::from_texture(&tex).fit_to_exact_size(image))
                        .widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, "预览图"));
                })
                .response;
            if resp.double_clicked() {
                scene_rect = full;
            }
        });
    });
    if let Some(viewer) = app.viewer.as_mut() {
        viewer.scene_rect = scene_rect;
    }
}

/// 等比缩小到 `max` 以内；小图保持原尺寸。
pub fn fit_size(image: egui::Vec2, max: egui::Vec2) -> egui::Vec2 {
    let scale = (max.x / image.x).min(max.y / image.y).min(1.0);
    image * scale
}
