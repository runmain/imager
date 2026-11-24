use eframe::egui;
use imageclient::app::{App, default_db_path};
use imageclient::db::Db;

fn main() -> eframe::Result {
    let db_path = std::env::var_os("IMAGECLIENT_DB").map(Into::into).unwrap_or_else(default_db_path);
    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png")).expect("内置图标应为合法 PNG");
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Imager")
            .with_icon(icon)
            .with_inner_size([1280.0, 820.0])
            .with_min_inner_size([900.0, 600.0]),
        ..Default::default()
    };
    match Db::open(&db_path) {
        Ok((db, backup)) => eframe::run_native(
            "Imager",
            options,
            Box::new(move |cc| {
                #[allow(unused_mut)]
                let mut app = App::new(cc.egui_ctx.clone(), db, backup);
                #[cfg(target_os = "macos")]
                app.install_tray();
                Ok(Box::new(app))
            }),
        ),
        Err(err) => {
            let msg = format!("无法打开数据库 {}：{err}", db_path.display());
            eprintln!("{msg}");
            eframe::run_ui_native("Imager", options, move |ui, _| {
                egui::CentralPanel::default().show(ui, |ui| {
                    ui.colored_label(egui::Color32::RED, &msg);
                });
            })
        }
    }
}
