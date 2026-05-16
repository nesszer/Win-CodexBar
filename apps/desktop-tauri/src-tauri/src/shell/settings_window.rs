//! Detached Settings window: opens Settings/About in a separate window
//! so the tray panel stays open.

use tauri::{Emitter, Manager, PhysicalPosition, WebviewUrl};

const SETTINGS_LABEL: &str = "settings";
const SETTINGS_WIDTH: f64 = 720.0;
const SETTINGS_HEIGHT: f64 = 580.0;

/// Open the detached Settings window, or focus it if already open.
///
/// When the window already exists, emits `settings-change-tab` so the
/// frontend can switch to the requested tab without a full reload.
pub fn open_or_focus(app: &tauri::AppHandle, tab: &str) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(SETTINGS_LABEL) {
        window.show().map_err(|e| e.to_string())?;
        window.set_focus().map_err(|e| e.to_string())?;
        app.emit_to(SETTINGS_LABEL, "settings-change-tab", tab)
            .map_err(|e| e.to_string())?;
        return Ok(());
    }

    let url = WebviewUrl::App(format!("index.html?window=settings&tab={tab}").into());

    let win = tauri::WebviewWindowBuilder::new(app, SETTINGS_LABEL, url)
        .title("CodexBar Settings")
        .inner_size(SETTINGS_WIDTH, SETTINGS_HEIGHT)
        .decorations(false)
        .shadow(false)
        .theme(Some(tauri::Theme::Dark))
        .resizable(true)
        .build()
        .map_err(|e| e.to_string())?;

    // Force DWM caption to dark; keep WS_THICKFRAME since window is resizable
    super::dwm::force_dark_caption_resizable(&win);

    // Manually center: Tauri's .center() is unreliable on Windows when
    // called from async commands. Compute position from the primary monitor.
    if let Ok(Some(monitor)) = win.primary_monitor() {
        let pos = monitor.position();
        let size = monitor.size();
        let scale = win.scale_factor().unwrap_or(1.0);
        let win_w = (SETTINGS_WIDTH * scale) as i32;
        let win_h = (SETTINGS_HEIGHT * scale) as i32;
        let x = pos.x + (size.width as i32 - win_w) / 2;
        let y = pos.y + (size.height as i32 - win_h) / 2;
        let _ = win.set_position(PhysicalPosition::new(x, y));
    }

    Ok(())
}

/// Dismiss Settings without exiting CodexBar.
///
/// The detached Settings window is hidden instead of closed so Tauri's
/// process/window lifecycle cannot interpret this as an app quit. If Settings
/// is rendered in the main shell surface, hide that surface back to tray.
pub fn dismiss(app: &tauri::AppHandle, window: &tauri::WebviewWindow) -> Result<(), String> {
    if window.label() == SETTINGS_LABEL {
        return window.hide().map_err(|e| e.to_string());
    }

    crate::shell::hide_to_tray_if_current(app, |mode| {
        mode == crate::surface::SurfaceMode::Settings
    })?;
    Ok(())
}
