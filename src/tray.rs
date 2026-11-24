//! macOS 菜单栏图标：关闭窗口时隐藏到菜单栏并隐藏 Dock 图标，单击图标重新显示窗口与 Dock 图标；菜单提供“显示 Imager”“退出”。

use std::sync::atomic::{AtomicBool, Ordering};

use eframe::egui;
use tray_icon::menu::{Menu, MenuEvent, MenuItem};
use tray_icon::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

static QUIT: AtomicBool = AtomicBool::new(false);

/// 持有菜单栏图标；丢弃即移除图标。
pub struct Tray {
    _icon: TrayIcon,
}

/// 必须在主线程、事件循环启动后创建（eframe 的 app 创建回调中）。
pub fn install(ctx: &egui::Context) -> Result<Tray, String> {
    let png = image::load_from_memory(include_bytes!("../assets/tray.png")).map_err(|e| e.to_string())?.to_rgba8();
    let (w, h) = png.dimensions();
    let icon = tray_icon::Icon::from_rgba(png.into_raw(), w, h).map_err(|e| e.to_string())?;

    let show = MenuItem::new("显示 Imager", true, None);
    let quit = MenuItem::new("退出", true, None);
    let menu = Menu::new();
    menu.append(&show).map_err(|e| e.to_string())?;
    menu.append(&quit).map_err(|e| e.to_string())?;

    let tray = TrayIconBuilder::new()
        .with_icon(icon)
        .with_tooltip("Imager")
        .with_menu(Box::new(menu))
        // 左键单击显示窗口，右键弹出菜单
        .with_menu_on_left_click(false)
        .build()
        .map_err(|e| e.to_string())?;

    let click_ctx = ctx.clone();
    TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
        if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
            show_window(&click_ctx);
        }
    }));
    let menu_ctx = ctx.clone();
    let (show_id, quit_id) = (show.id().clone(), quit.id().clone());
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        if event.id == quit_id {
            QUIT.store(true, Ordering::SeqCst);
        }
        if event.id == show_id || event.id == quit_id {
            // 退出也先唤醒窗口：隐藏状态下 eframe 不跑帧，无法处理关闭命令
            show_window(&menu_ctx);
        }
    }));
    Ok(Tray { _icon: tray })
}

/// 来自菜单“退出”的真正退出请求。
pub fn quit_requested() -> bool {
    QUIT.load(Ordering::SeqCst)
}

/// 切换 Dock 图标：隐藏窗口时设为 `Accessory`（只留菜单栏图标），显示窗口时恢复 `Regular`。
pub fn set_dock_visible(visible: bool) {
    use objc2_app_kit::NSApplicationActivationPolicy as Policy;
    if let Some(mtm) = objc2::MainThreadMarker::new() {
        let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
        app.setActivationPolicy(if visible { Policy::Regular } else { Policy::Accessory });
    }
}

/// 隐藏的窗口不会收到重绘，eframe 的视口命令得不到处理，因此直接用 AppKit 显示并激活。
fn show_window(ctx: &egui::Context) {
    set_dock_visible(true);
    if let Some(mtm) = objc2::MainThreadMarker::new() {
        let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
        for window in app.windows().iter() {
            window.makeKeyAndOrderFront(None);
        }
        #[allow(deprecated)]
        app.activateIgnoringOtherApps(true);
    }
    ctx.request_repaint();
}
