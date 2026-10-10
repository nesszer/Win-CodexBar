//! Monitor geometry helpers: panel sizing, monitor placement, anchor rectangles,
//! and inferred tray-panel positioning.

use crate::surface::SurfaceMode;
use crate::window_positioner::{self, PanelSize, Rect};

#[derive(Clone, Copy)]
pub(super) struct MonitorPlacement {
    pub bounds: Rect,
    pub work_area: Rect,
    pub scale_factor: f64,
}

/// Panel dimensions derived from the tray-panel surface mode properties.
pub(super) fn surface_panel_size(mode: SurfaceMode) -> PanelSize {
    let props = mode.window_properties();
    // Surface window property dimensions are whole-pixel constants.
    #[expect(clippy::cast_possible_truncation, reason = "whole units by design")]
    let width = props.width as u32;
    #[expect(clippy::cast_possible_truncation, reason = "whole units by design")]
    let height = props.height as u32;
    PanelSize { width, height }
}

pub(super) fn tray_panel_size() -> PanelSize {
    surface_panel_size(SurfaceMode::TrayPanel)
}

pub(super) fn monitor_work_area_rect(monitor: &tauri::Monitor) -> Rect {
    let position = monitor.position();
    let size = monitor.size();
    // Monitor dimensions are physical pixels, bounded well below i32::MAX.
    #[expect(
        clippy::cast_possible_wrap,
        reason = "monitor pixel dimensions fit in i32"
    )]
    let size_width = size.width as i32;
    #[expect(
        clippy::cast_possible_wrap,
        reason = "monitor pixel dimensions fit in i32"
    )]
    let size_height = size.height as i32;
    if let Some(area) = codexbar::host::session::primary_work_area_pixels()
        && area.width > 0
        && area.height > 0
        && area.x >= position.x
        && area.y >= position.y
        && area.x + area.width <= position.x + size_width
        && area.y + area.height <= position.y + size_height
    {
        return Rect::new(area.x, area.y, area.width as u32, area.height as u32);
    }

    let work_area = monitor.work_area();
    Rect::new(
        work_area.position.x,
        work_area.position.y,
        work_area.size.width,
        work_area.size.height,
    )
}

pub(super) fn monitor_placement(monitor: &tauri::Monitor) -> MonitorPlacement {
    let position = monitor.position();
    let size = monitor.size();

    MonitorPlacement {
        bounds: Rect::new(position.x, position.y, size.width, size.height),
        work_area: monitor_work_area_rect(monitor),
        scale_factor: monitor.scale_factor(),
    }
}

/// Center a panel on the monitor's work area (used for Settings windows).
pub(super) fn centered_position(monitor: &MonitorPlacement, panel_size: &PanelSize) -> (i32, i32) {
    let scale = monitor.scale_factor;
    // Centering math truncates to whole pixels; panel sizes fit comfortably in i32.
    #[expect(clippy::cast_possible_truncation, reason = "whole units by design")]
    let pw = (panel_size.width as f64 * scale) as i32;
    #[expect(clippy::cast_possible_truncation, reason = "whole units by design")]
    let ph = (panel_size.height as f64 * scale) as i32;
    let wa = &monitor.work_area;
    let x = wa.x + (wa.signed_width() - pw) / 2;
    let y = wa.y + (wa.signed_height() - ph) / 2;
    (x, y)
}

pub(super) fn inferred_tray_anchor_rect(monitor: &MonitorPlacement) -> Rect {
    const SYNTHETIC_TRAY_ICON_SIZE: u32 = 24;
    const SYNTHETIC_TRAY_EDGE_PADDING: i32 = 8;

    let work_right = monitor.work_area.right();
    let work_bottom = monitor.work_area.bottom();
    let bounds_left = monitor.bounds.x;
    let bounds_right = monitor.bounds.right();
    let bounds_top = monitor.bounds.y;
    let bounds_bottom = monitor.bounds.bottom();
    let left_gap = monitor.work_area.x - bounds_left;
    let right_gap = bounds_right - work_right;
    let top_gap = monitor.work_area.y - bounds_top;
    let bottom_gap = bounds_bottom - work_bottom;

    // Synthetic tray icon is 24 px, trivially within i32 range.
    #[expect(
        clippy::cast_possible_wrap,
        reason = "synthetic icon size constant fits i32"
    )]
    let icon_size = SYNTHETIC_TRAY_ICON_SIZE as i32;
    let x = if left_gap > right_gap {
        monitor.work_area.x - icon_size - SYNTHETIC_TRAY_EDGE_PADDING
    } else if right_gap > left_gap {
        work_right + SYNTHETIC_TRAY_EDGE_PADDING
    } else {
        work_right - icon_size - SYNTHETIC_TRAY_EDGE_PADDING
    };
    let y = if top_gap > bottom_gap {
        monitor.work_area.y - icon_size - SYNTHETIC_TRAY_EDGE_PADDING
    } else if bottom_gap > top_gap {
        work_bottom + SYNTHETIC_TRAY_EDGE_PADDING
    } else {
        bounds_bottom - icon_size - SYNTHETIC_TRAY_EDGE_PADDING
    };

    Rect::new(x, y, SYNTHETIC_TRAY_ICON_SIZE, SYNTHETIC_TRAY_ICON_SIZE)
}

pub(super) fn inferred_tray_panel_position_for_monitor(monitor: &MonitorPlacement) -> (i32, i32) {
    inferred_tray_panel_position_for_monitor_size(monitor, &tray_panel_size())
}

pub(super) fn inferred_tray_panel_position_for_monitor_size(
    monitor: &MonitorPlacement,
    panel_size: &PanelSize,
) -> (i32, i32) {
    window_positioner::calculate_panel_position(
        &inferred_tray_anchor_rect(monitor),
        &monitor.bounds,
        &monitor.work_area,
        panel_size,
        monitor.scale_factor,
    )
}

pub(super) fn tray_anchor_rect(anchor: crate::state::TrayAnchor) -> Rect {
    Rect::new(anchor.x, anchor.y, anchor.width, anchor.height)
}

pub(super) fn monitor_for_anchor(
    monitors: &[tauri::Monitor],
    anchor: crate::state::TrayAnchor,
) -> Option<&tauri::Monitor> {
    // Tray icon dimensions are small pixel counts, far below i32::MAX.
    #[expect(
        clippy::cast_possible_wrap,
        reason = "tray icon pixel dimensions fit in i32"
    )]
    let anchor_cx = anchor.x + anchor.width as i32 / 2;
    #[expect(
        clippy::cast_possible_wrap,
        reason = "tray icon pixel dimensions fit in i32"
    )]
    let anchor_cy = anchor.y + anchor.height as i32 / 2;

    monitor_containing_point(monitors, anchor_cx, anchor_cy)
}

pub(super) fn monitor_containing_point(
    monitors: &[tauri::Monitor],
    x: i32,
    y: i32,
) -> Option<&tauri::Monitor> {
    monitors.iter().find(|monitor| {
        let pos = monitor.position();
        let size = monitor.size();
        point_in_rect(&Rect::new(pos.x, pos.y, size.width, size.height), x, y)
    })
}

pub(super) fn point_in_rect(rect: &Rect, x: i32, y: i32) -> bool {
    x >= rect.x && x < rect.right() && y >= rect.y && y < rect.bottom()
}
