//! Detached "FloatBar" window: a small always-on-top transparent strip
//! that shows remaining capacity per provider. Runs as an auxiliary
//! Tauri window labeled `floatbar`, independent of the main surface
//! state machine.

use tauri::{LogicalPosition, LogicalSize, Manager, PhysicalPosition, PhysicalSize, WebviewUrl};

use crate::geometry_store;

pub const FLOATBAR_LABEL: &str = "floatbar";
pub const FLOAT_BAR_CONFIG_CHANGED_EVENT: &str = "float-bar-config-changed";
const FLOATBAR_DEFAULT_WIDTH_H: f64 = 360.0;
const FLOATBAR_DEFAULT_HEIGHT_H: f64 = 36.0;
const FLOATBAR_DEFAULT_WIDTH_V: f64 = 80.0;
const FLOATBAR_DEFAULT_HEIGHT_V: f64 = 280.0;
const WINDOWS_MINIMIZED_COORDINATE: i32 = -32_000;

/// Windows parks minimized windows at the exact physical coordinate pair
/// (-32000, -32000). Keep this strict so legitimate negative monitor origins
/// are never mistaken for minimized geometry.
fn is_windows_minimized_position(x: i32, y: i32) -> bool {
    x == WINDOWS_MINIMIZED_COORDINATE && y == WINDOWS_MINIMIZED_COORDINATE
}

fn should_remember_physical_position(is_minimized: bool, x: i32, y: i32) -> bool {
    !is_minimized && !is_windows_minimized_position(x, y)
}

fn physical_rects_intersect(
    first_position: PhysicalPosition<i32>,
    first_size: PhysicalSize<u32>,
    second_position: PhysicalPosition<i32>,
    second_size: PhysicalSize<u32>,
) -> bool {
    let first_left = i64::from(first_position.x);
    let first_top = i64::from(first_position.y);
    let first_right = first_left + i64::from(first_size.width);
    let first_bottom = first_top + i64::from(first_size.height);
    let second_left = i64::from(second_position.x);
    let second_top = i64::from(second_position.y);
    let second_right = second_left + i64::from(second_size.width);
    let second_bottom = second_top + i64::from(second_size.height);

    first_left < second_right
        && first_right > second_left
        && first_top < second_bottom
        && first_bottom > second_top
}

fn fallback_physical_position(
    work_area_position: PhysicalPosition<i32>,
    work_area_size: PhysicalSize<u32>,
    window_size: PhysicalSize<u32>,
    scale_factor: f64,
    style: &str,
) -> PhysicalPosition<i32> {
    let available_width = work_area_size.width.saturating_sub(window_size.width);
    let available_height = work_area_size.height.saturating_sub(window_size.height);
    let scale_factor = if scale_factor.is_finite() && scale_factor > 0.0 {
        scale_factor
    } else {
        1.0
    };
    let edge_padding = (8.0 * scale_factor).round() as u32;
    let x = work_area_position
        .x
        .saturating_add((available_width / 2).min(i32::MAX as u32) as i32);
    let y_offset = if style == "taskbar" {
        available_height.saturating_sub(edge_padding)
    } else {
        edge_padding.min(available_height)
    };
    let y = work_area_position
        .y
        .saturating_add(y_offset.min(i32::MAX as u32) as i32);

    PhysicalPosition::new(x, y)
}

/// Move a FloatBar that no longer intersects an active monitor onto the
/// primary monitor. This covers both stale geometry from a disconnected
/// display and Windows parking the live window at (-32000, -32000) when a
/// monitor is powered off.
pub(super) fn ensure_visible_on_active_monitor<R: tauri::Runtime, M: WindowGeometry<R>>(
    window: &M,
    style: &str,
) -> bool {
    let Ok(position) = window.outer_position() else {
        return false;
    };
    let Ok(size) = window.outer_size() else {
        return false;
    };
    let Ok(monitors) = window.available_monitors() else {
        return false;
    };
    if monitors.iter().any(|monitor| {
        let work_area = monitor.work_area();
        physical_rects_intersect(position, size, work_area.position, work_area.size)
    }) {
        return false;
    }

    let target_monitor = window
        .primary_monitor()
        .ok()
        .flatten()
        .or_else(|| monitors.into_iter().next());
    let Some(target_monitor) = target_monitor else {
        return false;
    };
    let work_area = target_monitor.work_area();
    let target = fallback_physical_position(
        work_area.position,
        work_area.size,
        size,
        target_monitor.scale_factor(),
        style,
    );

    if window.is_minimized().unwrap_or(false)
        || is_windows_minimized_position(position.x, position.y)
    {
        let _ = window.unminimize();
    }
    window.set_physical_position(target).is_ok()
}

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
#[cfg_attr(not(windows), allow(dead_code))]
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
    style: &str,
    click_through: bool,
) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(FLOATBAR_LABEL) {
        apply_no_activate(&window);
        apply_opacity(&window, opacity);
        apply_click_through(&window, click_through);
        apply_always_on_top(&window);
        ensure_visible_on_active_monitor(&window, style);
        window.show().map_err(|e| e.to_string())?;
        apply_always_on_top(&window);
        return Ok(());
    }

    let (w, h) = initial_size(orientation);
    let url =
        WebviewUrl::App(format!("index.html?window=floatbar&orientation={orientation}").into());

    let builder = tauri::WebviewWindowBuilder::new(app, FLOATBAR_LABEL, url)
        .title("CodexBar Float Bar")
        .inner_size(w, h)
        .decorations(false)
        .shadow(false)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true);

    // WebView2 only honors an alpha (transparent) background when the native
    // window is itself created transparent. Tauri cfg-gates this builder API
    // off on macOS unless `macos-private-api` is enabled, so keep the Windows
    // fix out of the macOS validation path.
    #[cfg(windows)]
    let builder = builder.transparent(true);

    let win = builder
        .background_color(tauri::utils::config::Color(0, 0, 0, 0))
        .visible(false)
        .build()
        .map_err(|e| e.to_string())?;

    // Restore prior geometry if we have one. Otherwise, taskbar style opens
    // near the bottom while the original floating style keeps its top-center
    // placement.
    if let Some(g) = geometry_store::load_entry(FLOATBAR_LABEL)
        .filter(|g| !is_windows_minimized_position(g.x, g.y))
    {
        let _ = win.set_position(LogicalPosition::new(g.x as f64, g.y as f64));
        if let (Some(w), Some(h)) = (g.width, g.height) {
            let _ = win.set_size(LogicalSize::new(w as f64, h as f64));
        }
    } else if let Ok(Some(monitor)) = win.primary_monitor() {
        let scale = win.scale_factor().unwrap_or(1.0);
        let mon_x = monitor.position().x as f64 / scale;
        let mon_y = monitor.position().y as f64 / scale;
        let mon_w = monitor.size().width as f64 / scale;
        let mon_h = monitor.size().height as f64 / scale;
        let x = mon_x + (mon_w - w) / 2.0;
        let y = if style == "taskbar" {
            mon_y + mon_h - h - 8.0
        } else {
            mon_y + 8.0
        };
        let _ = win.set_position(LogicalPosition::new(x.max(mon_x), y.max(mon_y)));
    }

    ensure_visible_on_active_monitor(&win, style);

    apply_no_activate(&win);
    apply_opacity(&win, opacity);
    apply_click_through(&win, click_through);
    apply_always_on_top(&win);
    win.show().map_err(|e| e.to_string())?;
    apply_always_on_top(&win);
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
    let is_minimized = window.is_minimized().unwrap_or(false);
    if !should_remember_physical_position(is_minimized, pos.x, pos.y) {
        return;
    }
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
    fn is_minimized(&self) -> tauri::Result<bool>;
    fn primary_monitor(&self) -> tauri::Result<Option<tauri::Monitor>>;
    fn available_monitors(&self) -> tauri::Result<Vec<tauri::Monitor>>;
    fn unminimize(&self) -> tauri::Result<()>;
    fn set_physical_position(&self, position: PhysicalPosition<i32>) -> tauri::Result<()>;
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
    fn is_minimized(&self) -> tauri::Result<bool> {
        tauri::WebviewWindow::is_minimized(self)
    }
    fn primary_monitor(&self) -> tauri::Result<Option<tauri::Monitor>> {
        tauri::WebviewWindow::primary_monitor(self)
    }
    fn available_monitors(&self) -> tauri::Result<Vec<tauri::Monitor>> {
        tauri::WebviewWindow::available_monitors(self)
    }
    fn unminimize(&self) -> tauri::Result<()> {
        tauri::WebviewWindow::unminimize(self)
    }
    fn set_physical_position(&self, position: PhysicalPosition<i32>) -> tauri::Result<()> {
        tauri::WebviewWindow::set_position(self, position)
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
    fn is_minimized(&self) -> tauri::Result<bool> {
        tauri::Window::is_minimized(self)
    }
    fn primary_monitor(&self) -> tauri::Result<Option<tauri::Monitor>> {
        tauri::Window::primary_monitor(self)
    }
    fn available_monitors(&self) -> tauri::Result<Vec<tauri::Monitor>> {
        tauri::Window::available_monitors(self)
    }
    fn unminimize(&self) -> tauri::Result<()> {
        tauri::Window::unminimize(self)
    }
    fn set_physical_position(&self, position: PhysicalPosition<i32>) -> tauri::Result<()> {
        tauri::Window::set_position(self, position)
    }
}

/// Resize the floatbar to the given logical dimensions and re-assert the
/// native interaction invariants in the same step.
///
/// A resize goes through `SetWindowPos`/frame changes, which can drop the
/// extended window styles, so the no-activate and click-through flags must be
/// re-applied afterwards. Keeping both halves here gives callers (including the
/// webview) a single canonical "the bar changed size" entry point instead of
/// pairing a JS `setSize` with a separate native repair command.
pub fn resize(
    window: &tauri::WebviewWindow,
    width: f64,
    height: f64,
    click_through: bool,
) -> Result<(), String> {
    window
        .set_size(LogicalSize::new(width, height))
        .map_err(|e| e.to_string())?;
    apply_no_activate(window);
    apply_click_through(window, click_through);
    apply_always_on_top(window);
    Ok(())
}

/// Re-assert native topmost ordering without activating the window.
///
/// Tauri's `always_on_top(true)` sets the initial intent, but on Windows
/// resize/style changes and competing topmost windows can still disturb z-order.
/// This Win32 pass keeps the floatbar visually above normal app windows while
/// preserving the current foreground app's input focus.
pub fn apply_always_on_top(window: &tauri::WebviewWindow) {
    let _ = window;
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
            const HWND_TOPMOST: isize = -1;
            const SWP_NOSIZE: u32 = 0x0001;
            const SWP_NOMOVE: u32 = 0x0002;
            const SWP_NOACTIVATE: u32 = 0x0010;
            let flags = SWP_NOSIZE | SWP_NOMOVE | SWP_NOACTIVATE;
            SetWindowPos(h.hwnd.get(), HWND_TOPMOST, 0, 0, 0, 0, flags);
        }
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
            const WS_EX_LAYERED: isize = 0x00080000;
            let ex = GetWindowLongPtrW(h.hwnd.get(), GWL_EXSTYLE);
            if ex & WS_EX_LAYERED == 0 {
                set_extended_style(h.hwnd.get(), ex | WS_EX_LAYERED);
            }
            const LWA_ALPHA: u32 = 0x00000002;
            SetLayeredWindowAttributes(h.hwnd.get(), 0, alpha, LWA_ALPHA);
        }
    }
}

/// Keep the floatbar from activating when it is shown or clicked. This makes
/// it behave like a desktop widget that visually sits above the taskbar without
/// stealing focus from the active app.
pub fn apply_no_activate(window: &tauri::WebviewWindow) {
    let _ = window;
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
            const WS_EX_NOACTIVATE: isize = 0x08000000;
            let ex = GetWindowLongPtrW(h.hwnd.get(), GWL_EXSTYLE);
            if ex & WS_EX_NOACTIVATE == 0 {
                set_extended_style(h.hwnd.get(), ex | WS_EX_NOACTIVATE);
            }
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
                set_extended_style(h.hwnd.get(), new_ex);
            }
        }
    }
}

#[cfg(windows)]
const GWL_EXSTYLE: i32 = -20;

#[cfg(windows)]
unsafe fn set_extended_style(hwnd: isize, ex_style: isize) {
    unsafe {
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex_style);
        const SWP_NOSIZE: u32 = 0x0001;
        const SWP_NOMOVE: u32 = 0x0002;
        const SWP_NOZORDER: u32 = 0x0004;
        const SWP_NOACTIVATE: u32 = 0x0010;
        const SWP_FRAMECHANGED: u32 = 0x0020;
        let flags = SWP_NOSIZE | SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED;
        SetWindowPos(hwnd, 0, 0, 0, 0, 0, flags);
    }
}

#[cfg(windows)]
#[link(name = "user32")]
unsafe extern "system" {
    fn GetWindowLongPtrW(hwnd: isize, index: i32) -> isize;
    fn SetWindowLongPtrW(hwnd: isize, index: i32, new: isize) -> isize;
    fn SetLayeredWindowAttributes(hwnd: isize, color_key: u32, alpha: u8, flags: u32) -> i32;
    fn SetWindowPos(
        hwnd: isize,
        hwnd_insert_after: isize,
        x: i32,
        y: i32,
        cx: i32,
        cy: i32,
        flags: u32,
    ) -> i32;
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
        assert_eq!(
            initial_size("horizontal"),
            (FLOATBAR_DEFAULT_WIDTH_H, FLOATBAR_DEFAULT_HEIGHT_H)
        );
        assert_eq!(
            initial_size("vertical"),
            (FLOATBAR_DEFAULT_WIDTH_V, FLOATBAR_DEFAULT_HEIGHT_V)
        );
        // Unknown values fall through to horizontal so a corrupted setting
        // can't yield an unreadable strip.
        assert_eq!(
            initial_size("diagonal"),
            (FLOATBAR_DEFAULT_WIDTH_H, FLOATBAR_DEFAULT_HEIGHT_H)
        );
    }

    #[test]
    fn windows_minimized_position_is_not_restored() {
        assert!(is_windows_minimized_position(-32_000, -32_000));
    }

    #[test]
    fn legitimate_negative_monitor_positions_are_preserved() {
        assert!(!is_windows_minimized_position(-3_840, 0));
        assert!(!is_windows_minimized_position(-8_000, -8_000));
        assert!(!is_windows_minimized_position(-16_000, -16_000));
    }

    #[test]
    fn minimized_or_parked_physical_positions_are_not_remembered() {
        assert!(!should_remember_physical_position(true, 100, 100));
        assert!(!should_remember_physical_position(false, -32_000, -32_000));
        assert!(should_remember_physical_position(false, -3_840, 100));
    }

    #[test]
    fn physical_multi_monitor_origin_is_remembered() {
        assert!(should_remember_physical_position(false, -8_000, -8_000));
    }

    #[test]
    fn window_must_intersect_an_active_monitor_to_be_visible() {
        let window_size = PhysicalSize::new(211, 40);
        let monitor_size = PhysicalSize::new(1_920, 1_032);

        assert!(physical_rects_intersect(
            PhysicalPosition::new(780, 8),
            window_size,
            PhysicalPosition::new(0, 0),
            monitor_size,
        ));
        assert!(!physical_rects_intersect(
            PhysicalPosition::new(-32_000, -32_000),
            window_size,
            PhysicalPosition::new(0, 0),
            monitor_size,
        ));
        assert!(physical_rects_intersect(
            PhysicalPosition::new(-3_840, 8),
            window_size,
            PhysicalPosition::new(-3_840, 0),
            monitor_size,
        ));
    }

    #[test]
    fn disconnected_monitor_position_falls_back_to_primary_work_area() {
        let work_area_position = PhysicalPosition::new(0, 0);
        let work_area_size = PhysicalSize::new(1_920, 1_032);
        let window_size = PhysicalSize::new(211, 40);

        assert_eq!(
            fallback_physical_position(
                work_area_position,
                work_area_size,
                window_size,
                1.0,
                "floating",
            ),
            PhysicalPosition::new(854, 8),
        );
        assert_eq!(
            fallback_physical_position(
                work_area_position,
                work_area_size,
                window_size,
                1.0,
                "taskbar",
            ),
            PhysicalPosition::new(854, 984),
        );
    }
}
