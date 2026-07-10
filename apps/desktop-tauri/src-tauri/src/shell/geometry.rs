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
    PanelSize {
        width: props.width as u32,
        height: props.height as u32,
    }
}

pub(super) fn tray_panel_size() -> PanelSize {
    surface_panel_size(SurfaceMode::TrayPanel)
}

pub(super) fn monitor_work_area_rect(monitor: &tauri::Monitor) -> Rect {
    let position = monitor.position();
    let size = monitor.size();
    if let Some(area) = codexbar::host::session::primary_work_area_pixels()
        && area.width > 0
        && area.height > 0
        && area.x >= position.x
        && area.y >= position.y
        && area.x + area.width <= position.x + size.width as i32
        && area.y + area.height <= position.y + size.height as i32
    {
        return Rect {
            x: area.x,
            y: area.y,
            width: area.width as u32,
            height: area.height as u32,
        };
    }

    let work_area = monitor.work_area();
    Rect {
        x: work_area.position.x,
        y: work_area.position.y,
        width: work_area.size.width,
        height: work_area.size.height,
    }
}

pub(super) fn monitor_placement(monitor: &tauri::Monitor) -> MonitorPlacement {
    let position = monitor.position();
    let size = monitor.size();

    MonitorPlacement {
        bounds: Rect {
            x: position.x,
            y: position.y,
            width: size.width,
            height: size.height,
        },
        work_area: monitor_work_area_rect(monitor),
        scale_factor: monitor.scale_factor(),
    }
}

pub(super) fn popout_position(
    anchor_rect: Option<&Rect>,
    monitor: &MonitorPlacement,
    panel_size: &PanelSize,
) -> (i32, i32) {
    window_positioner::calculate_popout_position(
        anchor_rect,
        &monitor.work_area,
        panel_size,
        monitor.scale_factor,
    )
}

/// Center a panel on the monitor's work area (used for Settings windows).
pub(super) fn centered_position(monitor: &MonitorPlacement, panel_size: &PanelSize) -> (i32, i32) {
    let scale = monitor.scale_factor;
    let pw = (panel_size.width as f64 * scale) as i32;
    let ph = (panel_size.height as f64 * scale) as i32;
    let wa = &monitor.work_area;
    let x = wa.x + (wa.width as i32 - pw) / 2;
    let y = wa.y + (wa.height as i32 - ph) / 2;
    (x, y)
}

pub(super) fn inferred_tray_anchor_rect(monitor: &MonitorPlacement) -> Rect {
    const SYNTHETIC_TRAY_ICON_SIZE: u32 = 24;
    const SYNTHETIC_TRAY_EDGE_PADDING: i32 = 8;

    let work_right = monitor.work_area.x + monitor.work_area.width as i32;
    let work_bottom = monitor.work_area.y + monitor.work_area.height as i32;
    let bounds_left = monitor.bounds.x;
    let bounds_right = monitor.bounds.x + monitor.bounds.width as i32;
    let bounds_top = monitor.bounds.y;
    let bounds_bottom = monitor.bounds.y + monitor.bounds.height as i32;
    let left_gap = monitor.work_area.x - bounds_left;
    let right_gap = bounds_right - work_right;
    let top_gap = monitor.work_area.y - bounds_top;
    let bottom_gap = bounds_bottom - work_bottom;

    let x = if left_gap > right_gap {
        monitor.work_area.x - SYNTHETIC_TRAY_ICON_SIZE as i32 - SYNTHETIC_TRAY_EDGE_PADDING
    } else if right_gap > left_gap {
        work_right + SYNTHETIC_TRAY_EDGE_PADDING
    } else {
        work_right - SYNTHETIC_TRAY_ICON_SIZE as i32 - SYNTHETIC_TRAY_EDGE_PADDING
    };
    let y = if top_gap > bottom_gap {
        monitor.work_area.y - SYNTHETIC_TRAY_ICON_SIZE as i32 - SYNTHETIC_TRAY_EDGE_PADDING
    } else if bottom_gap > top_gap {
        work_bottom + SYNTHETIC_TRAY_EDGE_PADDING
    } else {
        bounds_bottom - SYNTHETIC_TRAY_ICON_SIZE as i32 - SYNTHETIC_TRAY_EDGE_PADDING
    };

    Rect {
        x,
        y,
        width: SYNTHETIC_TRAY_ICON_SIZE,
        height: SYNTHETIC_TRAY_ICON_SIZE,
    }
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
    Rect {
        x: anchor.x,
        y: anchor.y,
        width: anchor.width,
        height: anchor.height,
    }
}

pub(super) fn monitor_placement_for_anchor(
    monitors: &[MonitorPlacement],
    anchor: crate::state::TrayAnchor,
) -> Option<MonitorPlacement> {
    let anchor_cx = anchor.x + anchor.width as i32 / 2;
    let anchor_cy = anchor.y + anchor.height as i32 / 2;

    monitor_placement_containing_point(monitors, anchor_cx, anchor_cy)
}

pub(super) fn monitor_placement_containing_point(
    monitors: &[MonitorPlacement],
    x: i32,
    y: i32,
) -> Option<MonitorPlacement> {
    monitors
        .iter()
        .find(|monitor| point_in_rect(&monitor.bounds, x, y))
        .copied()
}

pub(super) fn monitor_for_anchor(
    monitors: &[tauri::Monitor],
    anchor: crate::state::TrayAnchor,
) -> Option<&tauri::Monitor> {
    let anchor_cx = anchor.x + anchor.width as i32 / 2;
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
        point_in_rect(
            &Rect {
                x: pos.x,
                y: pos.y,
                width: size.width,
                height: size.height,
            },
            x,
            y,
        )
    })
}

pub(super) fn point_in_rect(rect: &Rect, x: i32, y: i32) -> bool {
    x >= rect.x && x < rect.x + rect.width as i32 && y >= rect.y && y < rect.y + rect.height as i32
}
