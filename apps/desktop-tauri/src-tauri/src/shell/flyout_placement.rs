//! Where the user put the tray panel, for the opt-in **Movable panel**
//! setting.
//!
//! The panel opens anchored to the tray icon. Once the user drags it
//! somewhere else, that spot is remembered, and every later open and every
//! content-driven resize keeps the panel there instead of snapping it back to
//! the tray. Double-clicking the panel's move strip forgets the spot.
//!
//! A spot is recorded only when a Win32 move loop ends (`WM_EXITSIZEMOVE`).
//! Windows runs that loop for user drags of the window, never for
//! `SetWindowPos`, so the panel's own re-anchoring can't be mistaken for a
//! user move.

use crate::geometry_store::{self, StoredGeometry};
use crate::window_positioner::{self, PanelSize, Rect};

/// Geometry-store key for the remembered panel position.
const FLYOUT_POSITION_KEY: &str = "flyout";

/// The position the user dragged the panel to, if any (physical px).
pub fn stored_position() -> Option<(i32, i32)> {
    geometry_store::load_entry(FLYOUT_POSITION_KEY).map(|geometry| (geometry.x, geometry.y))
}

fn save_position(x: i32, y: i32) {
    geometry_store::save_entry(
        FLYOUT_POSITION_KEY,
        StoredGeometry {
            x,
            y,
            width: None,
            height: None,
        },
    );
}

/// Forget the user's spot so the panel anchors to the tray again.
pub fn clear_position() {
    geometry_store::remove_entry(FLYOUT_POSITION_KEY);
}

/// The remembered position, clamped into the work area the panel overlaps
/// most (or the nearest one when a monitor layout change left it on none), so
/// the panel can't end up off-screen. `None` when the user never moved it.
pub fn placed_position(window: &tauri::WebviewWindow) -> Option<(i32, i32)> {
    let (x, y) = stored_position()?;
    let outer = window.outer_size().ok()?;
    let panel = Rect::new(x, y, outer.width, outer.height);
    let work_areas: Vec<Rect> = window
        .available_monitors()
        .ok()?
        .iter()
        .map(super::geometry::monitor_work_area_rect)
        .collect();
    let work_area = best_work_area(&work_areas, &panel)?;
    // The size is already physical, so clamp with a 1.0 scale.
    Some(window_positioner::clamp_position_to_work_area(
        x,
        y,
        &work_area,
        &PanelSize {
            width: outer.width,
            height: outer.height,
        },
        1.0,
    ))
}

/// The work area `panel` overlaps most; with no overlap, the one whose center
/// is nearest.
fn best_work_area(work_areas: &[Rect], panel: &Rect) -> Option<Rect> {
    work_areas.iter().copied().max_by_key(|area| {
        (
            overlap_area(area, panel),
            std::cmp::Reverse(center_distance(area, panel)),
        )
    })
}

fn overlap_area(a: &Rect, b: &Rect) -> i64 {
    let width = (right(a).min(right(b)) - i64::from(a.x).max(i64::from(b.x))).max(0);
    let height = (bottom(a).min(bottom(b)) - i64::from(a.y).max(i64::from(b.y))).max(0);
    width * height
}

fn center_distance(a: &Rect, b: &Rect) -> i64 {
    // Doubled centers keep the math in integers.
    let dx = (i64::from(a.x) + right(a)) - (i64::from(b.x) + right(b));
    let dy = (i64::from(a.y) + bottom(a)) - (i64::from(b.y) + bottom(b));
    dx * dx + dy * dy
}

fn right(rect: &Rect) -> i64 {
    i64::from(rect.x) + i64::from(rect.width)
}

fn bottom(rect: &Rect) -> i64 {
    i64::from(rect.y) + i64::from(rect.height)
}

/// Whether a mouse button is held on the panel itself.
///
/// Windows moves focus off the WebView the moment a native move starts, so
/// the panel sees a blur before the drag even begins. A blur while the user
/// is pressing on the panel is that drag, not a click somewhere else. The
/// window under the cursor is checked, not the panel's bounds, so a click on
/// another topmost window covering the panel still dismisses it.
#[cfg(windows)]
pub fn pointer_pressed_inside(window: &tauri::Window) -> bool {
    const GA_ROOT: u32 = 2;
    if !mouse_button_down() {
        return false;
    }
    let Some(flyout) = super::activation::root_hwnd(window) else {
        return false;
    };
    let mut cursor = Win32Point::default();
    // SAFETY: GetCursorPos writes the caller-owned point; WindowFromPoint and
    // GetAncestor only read the window tree.
    unsafe {
        if GetCursorPos(&mut cursor) == 0 {
            return false;
        }
        let under_cursor = WindowFromPoint(cursor);
        under_cursor != 0 && GetAncestor(under_cursor, GA_ROOT) == flyout
    }
}

#[cfg(not(windows))]
pub fn pointer_pressed_inside(_window: &tauri::Window) -> bool {
    false
}

/// Top-left corner in physical px, as `GetWindowRect` reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WindowOrigin {
    left: i32,
    top: i32,
}

/// The spot to remember after one user move loop: where the window landed,
/// if it moved at all.
fn placement_after_move(start: WindowOrigin, end: WindowOrigin) -> Option<(i32, i32)> {
    (start != end).then_some((end.left, end.top))
}

/// Record user drags of `window` from now on. The subclass must be installed
/// on the thread that owns the window, so this hops to the main thread.
#[cfg(windows)]
pub fn track_user_moves(window: &tauri::WebviewWindow) {
    let Some(hwnd) = super::activation::root_hwnd(window) else {
        tracing::warn!("flyout_placement: no window handle; user moves won't be remembered");
        return;
    };
    let scheduled = window.run_on_main_thread(move || {
        // SAFETY: `hwnd` is the live top-level flyout window and this runs on
        // its owning thread. The subclass proc forwards every message.
        let installed = unsafe { SetWindowSubclass(hwnd, move_subclass_proc, MOVE_SUBCLASS_ID, 0) };
        if installed == 0 {
            tracing::warn!("flyout_placement: SetWindowSubclass failed");
        }
    });
    if let Err(error) = scheduled {
        tracing::warn!(%error, "flyout_placement: couldn't schedule move tracking");
    }
}

#[cfg(not(windows))]
pub fn track_user_moves(_window: &tauri::WebviewWindow) {}

/// Whether the user is dragging the panel right now.
#[cfg(windows)]
pub fn user_move_in_progress() -> bool {
    MOVE_START.lock().is_ok_and(|start| start.is_some())
}

#[cfg(not(windows))]
pub fn user_move_in_progress() -> bool {
    false
}

#[cfg(windows)]
const MOVE_SUBCLASS_ID: usize = 0xC0DE_F1E0;
#[cfg(windows)]
const WM_NCDESTROY: u32 = 0x0082;
#[cfg(windows)]
const WM_ENTERSIZEMOVE: u32 = 0x0231;
#[cfg(windows)]
const WM_EXITSIZEMOVE: u32 = 0x0232;

/// Where the window was when the current move loop started. There is one
/// flyout window, so one slot is enough.
#[cfg(windows)]
static MOVE_START: std::sync::Mutex<Option<WindowOrigin>> = std::sync::Mutex::new(None);

#[cfg(windows)]
unsafe extern "system" fn move_subclass_proc(
    hwnd: isize,
    msg: u32,
    wparam: usize,
    lparam: isize,
    _id: usize,
    _data: usize,
) -> isize {
    match msg {
        WM_ENTERSIZEMOVE => {
            if let Ok(mut start) = MOVE_START.lock() {
                *start = window_origin(hwnd);
            }
        }
        WM_EXITSIZEMOVE => {
            let start = MOVE_START.lock().ok().and_then(|mut start| start.take());
            if let (Some(start), Some(end)) = (start, window_origin(hwnd))
                && let Some((x, y)) = placement_after_move(start, end)
            {
                save_position(x, y);
            }
        }
        WM_NCDESTROY => {
            if let Ok(mut start) = MOVE_START.lock() {
                *start = None;
            }
            // SAFETY: removes this subclass from the window being destroyed,
            // on its owning thread.
            unsafe {
                RemoveWindowSubclass(hwnd, move_subclass_proc, MOVE_SUBCLASS_ID);
            }
        }
        _ => {}
    }
    // SAFETY: forwards the message this subclass received, unchanged.
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}

#[cfg(windows)]
fn window_origin(hwnd: isize) -> Option<WindowOrigin> {
    let mut rect = Win32Rect::default();
    // SAFETY: `hwnd` is the live window the subclass is attached to and
    // `rect` is a caller-owned out-parameter.
    if unsafe { GetWindowRect(hwnd, &mut rect) } == 0 {
        return None;
    }
    Some(WindowOrigin {
        left: rect.left,
        top: rect.top,
    })
}

#[cfg(windows)]
fn mouse_button_down() -> bool {
    // Physical buttons: with swapped buttons the primary one is VK_RBUTTON,
    // so check both.
    const VK_LBUTTON: i32 = 0x01;
    const VK_RBUTTON: i32 = 0x02;
    // SAFETY: GetAsyncKeyState only reads global input state. A negative
    // result means the key is down.
    [VK_LBUTTON, VK_RBUTTON]
        .into_iter()
        .any(|key| unsafe { GetAsyncKeyState(key) } < 0)
}

#[cfg(windows)]
#[repr(C)]
#[derive(Default)]
struct Win32Point {
    x: i32,
    y: i32,
}

#[cfg(windows)]
#[repr(C)]
#[derive(Default)]
struct Win32Rect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

#[cfg(windows)]
#[link(name = "user32")]
// SAFETY: FFI declarations for user32 calls; every call site passes the live
// flyout window handle or caller-owned out-parameters.
unsafe extern "system" {
    fn GetAsyncKeyState(key: i32) -> i16;
    fn GetWindowRect(hwnd: isize, rect: *mut Win32Rect) -> i32;
    fn GetCursorPos(point: *mut Win32Point) -> i32;
    fn WindowFromPoint(point: Win32Point) -> isize;
    fn GetAncestor(hwnd: isize, flags: u32) -> isize;
}

#[cfg(windows)]
#[link(name = "comctl32")]
// SAFETY: FFI declarations for the comctl32 subclass API; called with the
// live flyout window handle and a `'static` subclass procedure.
unsafe extern "system" {
    fn SetWindowSubclass(
        hwnd: isize,
        proc: unsafe extern "system" fn(isize, u32, usize, isize, usize, usize) -> isize,
        id: usize,
        data: usize,
    ) -> i32;
    fn RemoveWindowSubclass(
        hwnd: isize,
        proc: unsafe extern "system" fn(isize, u32, usize, isize, usize, usize) -> isize,
        id: usize,
    ) -> i32;
    fn DefSubclassProc(hwnd: isize, msg: u32, wparam: usize, lparam: isize) -> isize;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn origin(left: i32, top: i32) -> WindowOrigin {
        WindowOrigin { left, top }
    }

    // A stacked layout: primary monitor below, a taller one above.
    fn stacked_work_areas() -> Vec<Rect> {
        vec![Rect::new(0, 0, 2560, 1392), Rect::new(0, -2560, 3840, 2452)]
    }

    #[test]
    fn a_drag_records_where_the_panel_landed() {
        assert_eq!(
            placement_after_move(origin(1936, 511), origin(400, -2000)),
            Some((400, -2000))
        );
    }

    #[test]
    fn a_drag_that_ends_where_it_started_records_nothing() {
        assert_eq!(
            placement_after_move(origin(400, 200), origin(400, 200)),
            None
        );
    }

    #[test]
    fn a_panel_straddling_two_monitors_belongs_to_the_one_it_overlaps_most() {
        let panel = Rect::new(800, -900, 400, 1000);
        let chosen = best_work_area(&stacked_work_areas(), &panel).expect("work area");
        assert_eq!((chosen.x, chosen.y), (0, -2560));
    }

    #[test]
    fn a_panel_whose_corner_hangs_off_screen_stays_on_its_monitor() {
        // The top-left corner is left of every monitor.
        let panel = Rect::new(-40, 300, 400, 900);
        let chosen = best_work_area(&stacked_work_areas(), &panel).expect("work area");
        assert_eq!((chosen.x, chosen.y), (0, 0));
    }

    #[test]
    fn a_panel_on_a_monitor_that_is_gone_moves_to_the_nearest_one() {
        // It was on a monitor right of the primary that has been unplugged.
        let panel = Rect::new(3000, 200, 400, 900);
        let chosen = best_work_area(&stacked_work_areas(), &panel).expect("work area");
        assert_eq!((chosen.x, chosen.y), (0, 0));
    }

    #[test]
    fn no_monitors_means_no_placement() {
        assert!(best_work_area(&[], &Rect::new(0, 0, 400, 900)).is_none());
    }
}
