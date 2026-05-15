//! Detached "FloatBar" window: a small always-on-top transparent strip
//! that shows remaining capacity per provider. Runs as an auxiliary
//! Tauri window labeled `floatbar`, independent of the main surface
//! state machine.

use tauri::{LogicalPosition, LogicalSize, Manager, WebviewUrl};

use crate::geometry_store;

pub const FLOATBAR_LABEL: &str = "floatbar";
pub const FLOAT_BAR_CONFIG_CHANGED_EVENT: &str = "float-bar-config-changed";
const FLOATBAR_DEFAULT_WIDTH_H: f64 = 360.0;
const FLOATBAR_DEFAULT_HEIGHT_H: f64 = 36.0;
const FLOATBAR_DEFAULT_WIDTH_V: f64 = 80.0;
const FLOATBAR_DEFAULT_HEIGHT_V: f64 = 280.0;

/// Initial dimensions (logical pixels) for the floating bar given an
/// orientation string. Unknown values fall back to horizontal so callers
/// don't have to pre-validate.
pub fn initial_size(orientation: &str) -> (f64, f64) {
    match orientation {
        "vertical" => (FLOATBAR_DEFAULT_WIDTH_V, FLOATBAR_DEFAULT_HEIGHT_V),
        _ => (FLOATBAR_DEFAULT_WIDTH_H, FLOATBAR_DEFAULT_HEIGHT_H),
    }
}

/// Convert a 0..=100 opacity value to a Win32 SetLayeredWindowAttributes
/// alpha byte (0..=255). Values below 30 are clamped so the bar is never
/// fully invisible — that would be a usability footgun.
pub fn opacity_to_alpha(opacity: u8) -> u8 {
    let clamped = opacity.clamp(30, 100);
    ((clamped as u32) * 255 / 100) as u8
}

/// Open the floating-bar window, or focus + reapply attributes if already
/// open. Position is restored from the geometry store keyed by
/// `floatbar`; on first launch the window is centered horizontally near
/// the top of the primary monitor.
pub fn show(
    app: &tauri::AppHandle,
    opacity: u8,
    orientation: &str,
    click_through: bool,
) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(FLOATBAR_LABEL) {
        apply_opacity(&window, opacity);
        apply_click_through(&window, click_through);
        window.show().map_err(|e| e.to_string())?;
        if !click_through {
            window.set_focus().map_err(|e| e.to_string())?;
        }
        return Ok(());
    }

    let (w, h) = initial_size(orientation);
    let url = WebviewUrl::App(format!("index.html?window=floatbar&orientation={orientation}").into());

    let win = tauri::WebviewWindowBuilder::new(app, FLOATBAR_LABEL, url)
        .title("CodexBar Float Bar")
        .inner_size(w, h)
        .decorations(false)
        .shadow(false)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .transparent(true)
        .visible(false)
        .build()
        .map_err(|e| e.to_string())?;

    // Restore prior geometry if we have one; otherwise center top.
    if let Some(g) = geometry_store::load_entry(FLOATBAR_LABEL) {
        let _ = win.set_position(LogicalPosition::new(g.x as f64, g.y as f64));
        if let (Some(w), Some(h)) = (g.width, g.height) {
            let _ = win.set_size(LogicalSize::new(w as f64, h as f64));
        }
    } else if let Ok(Some(monitor)) = win.primary_monitor() {
        let scale = win.scale_factor().unwrap_or(1.0);
        let mon_w = monitor.size().width as f64 / scale;
        let x = (mon_w - w) / 2.0;
        let _ = win.set_position(LogicalPosition::new(x.max(0.0), 8.0));
    }

    apply_opacity(&win, opacity);
    apply_click_through(&win, click_through);
    win.show().map_err(|e| e.to_string())?;
    if !click_through {
        let _ = win.set_focus();
    }
    Ok(())
}

/// Hide / destroy the floating bar.
pub fn hide(app: &tauri::AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(FLOATBAR_LABEL) {
        // Persist position before closing so it reopens in place.
        remember_geometry(&window);
        window.close().map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Capture current position into the geometry store under the floatbar key.
///
/// Accepts any Tauri window handle (`Window` from event callbacks or
/// `WebviewWindow` from `get_webview_window`), since `WindowEvent`
/// callbacks deliver a `&Window` while imperative call sites have a
/// `&WebviewWindow`.
pub fn remember_geometry<R: tauri::Runtime, M: WindowGeometry<R>>(window: &M) {
    let Ok(pos) = window.outer_position() else {
        return;
    };
    let Ok(size) = window.outer_size() else {
        return;
    };
    let scale = window.scale_factor().unwrap_or(1.0);
    geometry_store::save_entry(
        FLOATBAR_LABEL,
        geometry_store::StoredGeometry {
            x: (pos.x as f64 / scale).round() as i32,
            y: (pos.y as f64 / scale).round() as i32,
            width: Some((size.width as f64 / scale).round() as u32),
            height: Some((size.height as f64 / scale).round() as u32),
        },
    );
}

/// Subset of `tauri::WebviewWindow` / `tauri::Window` used by
/// [`remember_geometry`]. Both types implement the underlying methods, but
/// they don't share a public trait — this private trait bridges them so we
/// can be called from `WindowEvent` (which delivers `&Window`) and from
/// imperative paths (which hold `&WebviewWindow`).
pub trait WindowGeometry<R: tauri::Runtime> {
    fn outer_position(&self) -> tauri::Result<tauri::PhysicalPosition<i32>>;
    fn outer_size(&self) -> tauri::Result<tauri::PhysicalSize<u32>>;
    fn scale_factor(&self) -> tauri::Result<f64>;
}

impl<R: tauri::Runtime> WindowGeometry<R> for tauri::WebviewWindow<R> {
    fn outer_position(&self) -> tauri::Result<tauri::PhysicalPosition<i32>> {
        tauri::WebviewWindow::outer_position(self)
    }
    fn outer_size(&self) -> tauri::Result<tauri::PhysicalSize<u32>> {
        tauri::WebviewWindow::outer_size(self)
    }
    fn scale_factor(&self) -> tauri::Result<f64> {
        tauri::WebviewWindow::scale_factor(self)
    }
}

impl<R: tauri::Runtime> WindowGeometry<R> for tauri::Window<R> {
    fn outer_position(&self) -> tauri::Result<tauri::PhysicalPosition<i32>> {
        tauri::Window::outer_position(self)
    }
    fn outer_size(&self) -> tauri::Result<tauri::PhysicalSize<u32>> {
        tauri::Window::outer_size(self)
    }
    fn scale_factor(&self) -> tauri::Result<f64> {
        tauri::Window::scale_factor(self)
    }
}

/// Apply the current opacity setting to an existing floatbar window via
/// `SetLayeredWindowAttributes`. No-op on non-Windows platforms.
pub fn apply_opacity(window: &tauri::WebviewWindow, opacity: u8) {
    let _ = (window, opacity);
    #[cfg(windows)]
    {
        use raw_window_handle::HasWindowHandle;
        let alpha = opacity_to_alpha(opacity);
        let Ok(handle) = window.window_handle() else {
            return;
        };
        let raw_window_handle::RawWindowHandle::Win32(h) = handle.as_raw() else {
            return;
        };
        unsafe {
            // Ensure WS_EX_LAYERED is set so SetLayeredWindowAttributes works.
            const GWL_EXSTYLE: i32 = -20;
            const WS_EX_LAYERED: isize = 0x00080000;
            let ex = GetWindowLongPtrW(h.hwnd.get(), GWL_EXSTYLE);
            if ex & WS_EX_LAYERED == 0 {
                SetWindowLongPtrW(h.hwnd.get(), GWL_EXSTYLE, ex | WS_EX_LAYERED);
            }
            const LWA_ALPHA: u32 = 0x00000002;
            SetLayeredWindowAttributes(h.hwnd.get(), 0, alpha, LWA_ALPHA);
        }
    }
}

/// Toggle click-through (`WS_EX_TRANSPARENT`). When enabled, mouse events
/// pass through to the window beneath — true overlay mode.
pub fn apply_click_through(window: &tauri::WebviewWindow, click_through: bool) {
    let _ = (window, click_through);
    #[cfg(windows)]
    {
        use raw_window_handle::HasWindowHandle;
        let Ok(handle) = window.window_handle() else {
            return;
        };
        let raw_window_handle::RawWindowHandle::Win32(h) = handle.as_raw() else {
            return;
        };
        unsafe {
            const GWL_EXSTYLE: i32 = -20;
            const WS_EX_LAYERED: isize = 0x00080000;
            const WS_EX_TRANSPARENT: isize = 0x00000020;
            let ex = GetWindowLongPtrW(h.hwnd.get(), GWL_EXSTYLE);
            let mut new_ex = ex | WS_EX_LAYERED;
            if click_through {
                new_ex |= WS_EX_TRANSPARENT;
            } else {
                new_ex &= !WS_EX_TRANSPARENT;
            }
            if new_ex != ex {
                SetWindowLongPtrW(h.hwnd.get(), GWL_EXSTYLE, new_ex);
            }
        }
    }
}

#[cfg(windows)]
#[link(name = "user32")]
unsafe extern "system" {
    fn GetWindowLongPtrW(hwnd: isize, index: i32) -> isize;
    fn SetWindowLongPtrW(hwnd: isize, index: i32, new: isize) -> isize;
    fn SetLayeredWindowAttributes(hwnd: isize, color_key: u32, alpha: u8, flags: u32) -> i32;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opacity_to_alpha_clamps_low_values() {
        assert_eq!(opacity_to_alpha(0), opacity_to_alpha(30));
        assert_eq!(opacity_to_alpha(10), opacity_to_alpha(30));
    }

    #[test]
    fn opacity_to_alpha_full_is_255() {
        assert_eq!(opacity_to_alpha(100), 255);
    }

    #[test]
    fn opacity_to_alpha_is_monotonic() {
        let a = opacity_to_alpha(30);
        let b = opacity_to_alpha(60);
        let c = opacity_to_alpha(100);
        assert!(a < b);
        assert!(b < c);
    }

    #[test]
    fn opacity_to_alpha_midpoint() {
        // 50% should be roughly half of 255.
        let alpha = opacity_to_alpha(50);
        assert!((125..=130).contains(&alpha), "got {alpha}");
    }

    #[test]
    fn initial_size_picks_orientation() {
        assert_eq!(initial_size("horizontal"), (FLOATBAR_DEFAULT_WIDTH_H, FLOATBAR_DEFAULT_HEIGHT_H));
        assert_eq!(initial_size("vertical"), (FLOATBAR_DEFAULT_WIDTH_V, FLOATBAR_DEFAULT_HEIGHT_V));
        // Unknown values fall through to horizontal so a corrupted setting
        // can't yield an unreadable strip.
        assert_eq!(initial_size("diagonal"), (FLOATBAR_DEFAULT_WIDTH_H, FLOATBAR_DEFAULT_HEIGHT_H));
    }
}
