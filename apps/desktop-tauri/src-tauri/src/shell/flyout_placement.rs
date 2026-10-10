//! Where the user put the tray-panel flyout.
//!
//! The flyout opens anchored to the tray icon. Once the user drags it
//! somewhere else, that spot is remembered, and every later open and every
//! content-driven resize keeps the panel there instead of snapping it back to
//! the tray. Double-clicking the panel's move handle forgets the spot.
//!
//! A spot is recorded only when a Win32 move/size loop ends
//! (`WM_EXITSIZEMOVE`). Windows runs that loop for user drags of the window
//! or its frame, never for `SetWindowPos`, so the flyout's own re-anchoring
//! can't be mistaken for a user move.

use crate::geometry_store::{self, StoredGeometry};
use crate::window_positioner::{self, PanelSize, Rect};

/// Geometry-store key for the remembered flyout position. Lives in the
/// position `entries` map, separate from the size-only `"flyout"` entry.
const FLYOUT_POSITION_KEY: &str = "flyout";

/// The position the user dragged the flyout to, if any (physical px).
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

/// Forget the user's spot so the flyout anchors to the tray again.
pub fn clear_position() {
    geometry_store::remove_entry(FLYOUT_POSITION_KEY);
}

/// The remembered position, clamped into the work area the panel overlaps
/// most (or the nearest one when a monitor layout change left it on none), so
/// the panel can't end up off-screen. `None` when the user never moved the
/// flyout.
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

/// Whether a mouse button is held on the flyout itself.
///
/// Windows moves focus off the WebView the moment a move or resize starts on
/// the window frame, so the flyout sees a blur before the gesture even
/// begins. A blur while the user is pressing on the panel itself is that
/// gesture, not a click somewhere else. The window under the cursor is
/// checked, not the panel's bounds, so a click on another topmost window
/// covering the panel still dismisses it.
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

/// Outer window bounds in physical px, as `GetWindowRect` reports them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WindowBounds {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

/// One user move/size loop in progress.
#[derive(Debug, Clone, Copy)]
struct SizeMoveLoop {
    start: WindowBounds,
    /// The user dragged an edge (`WM_SIZING`) rather than the whole window.
    /// Compared bounds can't tell: a move onto a monitor with another DPI
    /// rescales the window too.
    resizing: bool,
}

/// The spot to remember after one user move/size loop, if any.
///
/// Dragging the whole window places the flyout. A resize from the top or left
/// edge also moves the window; that only updates the spot once the user has
/// placed the flyout, so resizing a tray-anchored panel keeps it tray-anchored.
fn placement_after_size_move(
    size_move: SizeMoveLoop,
    end: WindowBounds,
    already_placed: bool,
) -> Option<(i32, i32)> {
    let start = size_move.start;
    let moved = (start.left, start.top) != (end.left, end.top);
    (moved && (already_placed || !size_move.resizing)).then_some((end.left, end.top))
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
        let installed =
            unsafe { SetWindowSubclass(hwnd, size_move_subclass_proc, SIZE_MOVE_SUBCLASS_ID, 0) };
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

/// Whether the user is dragging the flyout or one of its edges right now.
#[cfg(windows)]
pub fn user_move_in_progress() -> bool {
    SIZE_MOVE.lock().is_ok_and(|size_move| size_move.is_some())
}

#[cfg(not(windows))]
pub fn user_move_in_progress() -> bool {
    false
}

#[cfg(windows)]
const SIZE_MOVE_SUBCLASS_ID: usize = 0xC0DE_F1E0;
#[cfg(windows)]
const WM_ENTERSIZEMOVE: u32 = 0x0231;
#[cfg(windows)]
const WM_EXITSIZEMOVE: u32 = 0x0232;
#[cfg(windows)]
const WM_SIZING: u32 = 0x0214;
#[cfg(windows)]
const WM_NCDESTROY: u32 = 0x0082;

/// The move/size loop in progress. There is one flyout window, so one slot
/// is enough.
#[cfg(windows)]
static SIZE_MOVE: std::sync::Mutex<Option<SizeMoveLoop>> = std::sync::Mutex::new(None);

#[cfg(windows)]
unsafe extern "system" fn size_move_subclass_proc(
    hwnd: isize,
    msg: u32,
    wparam: usize,
    lparam: isize,
    _id: usize,
    _data: usize,
) -> isize {
    match msg {
        WM_ENTERSIZEMOVE => {
            if let Ok(mut size_move) = SIZE_MOVE.lock() {
                *size_move = window_bounds(hwnd).map(|start| SizeMoveLoop {
                    start,
                    resizing: false,
                });
            }
        }
        WM_SIZING => {
            if let Ok(mut size_move) = SIZE_MOVE.lock()
                && let Some(size_move) = size_move.as_mut()
            {
                size_move.resizing = true;
            }
        }
        WM_NCDESTROY => {
            if let Ok(mut size_move) = SIZE_MOVE.lock() {
                *size_move = None;
            }
            // SAFETY: removes this subclass from the window being destroyed,
            // on its owning thread.
            unsafe {
                RemoveWindowSubclass(hwnd, size_move_subclass_proc, SIZE_MOVE_SUBCLASS_ID);
            }
        }
        WM_EXITSIZEMOVE => {
            let size_move = SIZE_MOVE
                .lock()
                .ok()
                .and_then(|mut size_move| size_move.take());
            if let (Some(size_move), Some(end)) = (size_move, window_bounds(hwnd))
                && let Some((x, y)) =
                    placement_after_size_move(size_move, end, stored_position().is_some())
            {
                save_position(x, y);
            }
        }
        _ => {}
    }
    // SAFETY: forwards the message this subclass received, unchanged.
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}

#[cfg(windows)]
fn window_bounds(hwnd: isize) -> Option<WindowBounds> {
    let mut rect = Win32Rect::default();
    // SAFETY: `hwnd` is the live window the subclass is attached to and
    // `rect` is a caller-owned out-parameter.
    if unsafe { GetWindowRect(hwnd, &mut rect) } == 0 {
        return None;
    }
    Some(WindowBounds {
        left: rect.left,
        top: rect.top,
        right: rect.right,
        bottom: rect.bottom,
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
    fn DefSubclassProc(hwnd: isize, msg: u32, wparam: usize, lparam: isize) -> isize;
    fn RemoveWindowSubclass(
        hwnd: isize,
        proc: unsafe extern "system" fn(isize, u32, usize, isize, usize, usize) -> isize,
        id: usize,
    ) -> i32;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bounds(left: i32, top: i32, width: i32, height: i32) -> WindowBounds {
        WindowBounds {
            left,
            top,
            right: left + width,
            bottom: top + height,
        }
    }

    fn moved(start: WindowBounds) -> SizeMoveLoop {
        SizeMoveLoop {
            start,
            resizing: false,
        }
    }

    fn resized(start: WindowBounds) -> SizeMoveLoop {
        SizeMoveLoop {
            start,
            resizing: true,
        }
    }

    #[test]
    fn dragging_the_window_places_the_flyout() {
        let start = bounds(1936, 511, 300, 873);
        let end = bounds(400, 200, 300, 873);
        assert_eq!(
            placement_after_size_move(moved(start), end, false),
            Some((400, 200))
        );
    }

    #[test]
    fn dragging_onto_a_monitor_with_another_dpi_still_places_the_flyout() {
        // Windows rescales the window on the way (100% -> 225%).
        let start = bounds(1808, 611, 340, 773);
        let end = bounds(600, -1500, 765, 1739);
        assert_eq!(
            placement_after_size_move(moved(start), end, false),
            Some((600, -1500))
        );
    }

    #[test]
    fn resizing_a_tray_anchored_flyout_from_the_top_left_keeps_it_tray_anchored() {
        let start = bounds(1936, 511, 300, 873);
        let end = bounds(1836, 411, 400, 973);
        assert_eq!(placement_after_size_move(resized(start), end, false), None);
    }

    #[test]
    fn resizing_a_placed_flyout_from_the_top_left_follows_its_new_corner() {
        let start = bounds(400, 200, 300, 873);
        let end = bounds(300, 100, 400, 973);
        assert_eq!(
            placement_after_size_move(resized(start), end, true),
            Some((300, 100))
        );
    }

    #[test]
    fn resizing_from_the_bottom_right_never_records_a_spot() {
        let start = bounds(400, 200, 300, 873);
        let end = bounds(400, 200, 360, 900);
        assert_eq!(placement_after_size_move(resized(start), end, true), None);
        assert_eq!(placement_after_size_move(resized(start), end, false), None);
    }

    #[test]
    fn a_drag_that_ends_where_it_started_records_nothing() {
        let start = bounds(400, 200, 300, 873);
        assert_eq!(placement_after_size_move(moved(start), start, false), None);
    }

    fn area(x: i32, y: i32, width: u32, height: u32) -> Rect {
        Rect::new(x, y, width, height)
    }

    // A stacked layout: primary monitor below, a taller one above.
    fn stacked_work_areas() -> Vec<Rect> {
        vec![area(0, 0, 2560, 1392), area(0, -2560, 3840, 2452)]
    }

    #[test]
    fn a_panel_straddling_two_monitors_belongs_to_the_one_it_overlaps_most() {
        let panel = area(800, -900, 400, 1000);
        let chosen = best_work_area(&stacked_work_areas(), &panel).expect("work area");
        assert_eq!((chosen.x, chosen.y), (0, -2560));
    }

    #[test]
    fn a_panel_whose_corner_hangs_off_screen_stays_on_its_monitor() {
        // The top-left corner is left of every monitor.
        let panel = area(-40, 300, 400, 900);
        let chosen = best_work_area(&stacked_work_areas(), &panel).expect("work area");
        assert_eq!((chosen.x, chosen.y), (0, 0));
    }

    #[test]
    fn a_panel_on_a_monitor_that_is_gone_moves_to_the_nearest_one() {
        // It was on a monitor right of the primary that has been unplugged.
        let panel = area(3000, 200, 400, 900);
        let chosen = best_work_area(&stacked_work_areas(), &panel).expect("work area");
        assert_eq!((chosen.x, chosen.y), (0, 0));
    }

    #[test]
    fn no_monitors_means_no_placement() {
        assert!(best_work_area(&[], &area(0, 0, 400, 900)).is_none());
    }
}
