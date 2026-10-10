//! Tauri commands that drive the floating-bar window.
//!
//! The resize command applies a new size and the native interaction state
//! together.

use codexbar::settings::Settings;
use tauri::{AppHandle, Manager};

use super::window as floatbar_window;

#[tauri::command]
pub fn resize_float_bar(app: AppHandle, width: f64, height: f64) -> Result<(), String> {
    let settings = Settings::load();
    if let Some(window) = app.get_webview_window(floatbar_window::FLOATBAR_LABEL) {
        // One native operation owns the resize + interaction-state invariant,
        // so the webview never has to repair Win32 window styles itself.
        floatbar_window::resize(&window, width, height, settings.float_bar_click_through)?;
    }
    Ok(())
}
