//! Public position API: default placement, tray/shortcut/inferred panel
//! positions, and remembered detached-surface geometry.

use std::sync::Mutex;

use tauri::{AppHandle, Manager};

use crate::state::AppState;
use crate::surface::SurfaceMode;
use crate::window_positioner;

use super::geometry::{
    MonitorPlacement, inferred_tray_anchor_rect, monitor_for_anchor, monitor_placement,
    monitor_placement_containing_point, monitor_placement_for_anchor, monitor_work_area_rect,
    point_in_rect, popout_position, surface_panel_size, tray_anchor_rect, tray_panel_size,
};

pub fn inferred_tray_panel_position(app: &AppHandle) -> Option<(i32, i32)> {
    let window = app.get_webview_window("main")?;
    let monitor = window
        .primary_monitor()
        .ok()
        .flatten()
        .or_else(|| window.current_monitor().ok().flatten())
        .map(|m| monitor_placement(&m))?;

    Some(super::geometry::inferred_tray_panel_position_for_monitor(
        &monitor,
    ))
}

pub(crate) fn inferred_tray_panel_position_for_monitor_size(
    monitor: &tauri::Monitor,
    panel_size: &window_positioner::PanelSize,
) -> (i32, i32) {
    super::geometry::inferred_tray_panel_position_for_monitor_size(
        &monitor_placement(monitor),
        panel_size,
    )
}

fn current_tray_anchor(app: &AppHandle) -> Option<crate::state::TrayAnchor> {
    let st = app.try_state::<Mutex<AppState>>()?;
    st.lock().ok()?.tray_anchor
}

fn visible_surface_position_for_mode(app: &AppHandle, mode: SurfaceMode) -> Option<(i32, i32)> {
    let window = app.get_webview_window("main")?;
    let monitor_placements = window
        .available_monitors()
        .ok()
        .map(|monitors| monitors.iter().map(monitor_placement).collect::<Vec<_>>());
    let current_monitor = window
        .current_monitor()
        .ok()
        .flatten()
        .map(|monitor| monitor_placement(&monitor));
    let current_window_bounds = match (window.outer_position(), window.outer_size()) {
        (Ok(position), Ok(size)) => Some(((position.x, position.y), (size.width, size.height))),
        _ => None,
    };
    let primary_monitor = window
        .primary_monitor()
        .ok()
        .flatten()
        .map(|monitor| monitor_placement(&monitor));

    visible_surface_position_for_mode_with_fallbacks(
        mode,
        monitor_placements.as_deref(),
        current_tray_anchor(app),
        current_monitor,
        current_window_bounds,
        primary_monitor,
    )
}

pub(super) fn visible_surface_position_for_mode_with_fallbacks(
    mode: SurfaceMode,
    monitor_placements: Option<&[MonitorPlacement]>,
    tray_anchor: Option<crate::state::TrayAnchor>,
    current_monitor: Option<MonitorPlacement>,
    current_window_bounds: Option<((i32, i32), (u32, u32))>,
    primary_monitor: Option<MonitorPlacement>,
) -> Option<(i32, i32)> {
    let panel_size = surface_panel_size(mode);

    if let Some(anchor) = tray_anchor
        && let Some(monitors) = monitor_placements
        && let Some(monitor) = monitor_placement_for_anchor(monitors, anchor)
    {
        return Some(popout_position(
            Some(&tray_anchor_rect(anchor)),
            &monitor,
            &panel_size,
        ));
    }

    // No usable tray anchor (e.g. a right-click menu "Pop Out Dashboard" with no
    // prior left-click, or a proof/automation launch). The tray icon lives on
    // the primary (taskbar) monitor, so anchor the surface there. Crucially, do
    // NOT fall through to the hidden main window's `current_monitor`: after a
    // previous session left the surface on a now-off-view monitor, that monitor
    // becomes `current_monitor` and the surface would reopen where the user
    // can't see it — the multi-monitor "nothing happens" bug.
    if tray_anchor.is_none()
        && let Some(monitor) = primary_monitor
    {
        return Some(popout_position(
            Some(&inferred_tray_anchor_rect(&monitor)),
            &monitor,
            &panel_size,
        ));
    }

    if let Some(monitor) = current_monitor {
        return Some(popout_position(None, &monitor, &panel_size));
    }

    if let Some(monitors) = monitor_placements
        && let Some((current_top_left, current_size)) = current_window_bounds
        && let Some(monitor) = monitor_placement_containing_point(
            monitors,
            current_top_left.0 + current_size.0 as i32 / 2,
            current_top_left.1 + current_size.1 as i32 / 2,
        )
    {
        return Some(popout_position(None, &monitor, &panel_size));
    }

    let monitor = primary_monitor?;
    Some(popout_position(None, &monitor, &panel_size))
}

pub fn default_surface_position(app: &AppHandle, mode: SurfaceMode) -> Option<(i32, i32)> {
    match mode {
        SurfaceMode::Hidden => None,
        SurfaceMode::TrayPanel => tray_panel_position(app)
            .or_else(|| inferred_tray_panel_position(app))
            .or_else(|| shortcut_panel_position(app)),
        SurfaceMode::PopOut => remembered_surface_position(app, mode)
            .or_else(|| visible_surface_position_for_mode(app, mode)),
        SurfaceMode::Settings => {
            remembered_surface_position(app, mode).or_else(|| centered_settings_position(app))
        }
    }
}

/// Center the Settings window on the current/primary monitor.
fn centered_settings_position(app: &AppHandle) -> Option<(i32, i32)> {
    let window = app.get_webview_window("main")?;

    let monitor = window
        .current_monitor()
        .ok()
        .flatten()
        .or_else(|| window.primary_monitor().ok().flatten())
        .or_else(|| {
            window
                .available_monitors()
                .ok()
                .and_then(|v| v.into_iter().next())
        });

    let placement = monitor.map(|m| monitor_placement(&m))?;
    let panel_size = surface_panel_size(SurfaceMode::Settings);
    Some(super::geometry::centered_position(&placement, &panel_size))
}

/// Load persisted geometry and clamp it into the current monitor's work area so
/// a monitor layout change can't leave the window off-screen.
fn remembered_surface_position(app: &AppHandle, mode: SurfaceMode) -> Option<(i32, i32)> {
    let stored = crate::geometry_store::load(mode)?;
    let window = app.get_webview_window("main")?;
    let monitors = window
        .available_monitors()
        .ok()?
        .iter()
        .map(monitor_placement)
        .collect::<Vec<_>>();
    let primary_monitor = window
        .primary_monitor()
        .ok()
        .flatten()
        .map(|monitor| monitor_placement(&monitor));

    remembered_surface_position_with_monitors(mode, stored, &monitors, primary_monitor)
}

pub(super) fn remembered_surface_position_with_monitors(
    mode: SurfaceMode,
    stored: crate::geometry_store::StoredGeometry,
    monitors: &[MonitorPlacement],
    primary_monitor: Option<MonitorPlacement>,
) -> Option<(i32, i32)> {
    let placement = monitors
        .iter()
        .copied()
        .find(|monitor| point_in_rect(&monitor.work_area, stored.x, stored.y))
        .or(primary_monitor)?;
    let panel_size = remembered_panel_size(mode, stored);

    Some(window_positioner::clamp_position_to_work_area(
        stored.x,
        stored.y,
        &placement.work_area,
        &panel_size,
        placement.scale_factor,
    ))
}

pub(super) fn remembered_panel_size(
    mode: SurfaceMode,
    stored: crate::geometry_store::StoredGeometry,
) -> window_positioner::PanelSize {
    let default_size = surface_panel_size(mode);
    window_positioner::PanelSize {
        width: stored.width.unwrap_or(default_size.width).max(1),
        height: stored.height.unwrap_or(default_size.height).max(1),
    }
}

/// Persist the current position (and size, when resizable) of the main window
/// when it is hosting a remembered surface. Called from the Tauri window-event
/// pump so user drags are captured even without an explicit close.
pub fn remember_current_geometry_if_eligible(window: &tauri::Window) {
    let app = window.app_handle();
    let Some(st) = app.try_state::<Mutex<AppState>>() else {
        return;
    };
    let current_mode = {
        let guard = st.lock().unwrap();
        guard.surface_machine.current()
    };
    // The TrayPanel flyout persists its size explicitly from the frontend (only
    // on genuine user drag-resizes, never on its own auto-fit resizes), so skip
    // the automatic capture here — otherwise an auto-fit resize would be saved
    // and freeze the panel at that size.
    if current_mode == SurfaceMode::TrayPanel
        || !crate::geometry_store::should_remember(current_mode)
    {
        return;
    }

    // Never persist maximized/minimized bounds as the remembered geometry —
    // only genuine restored drags/resizes. Otherwise clicking the maximize
    // button would make the surface reopen permanently oversized with no way
    // back to its default size.
    if window.is_maximized().unwrap_or(false) || window.is_minimized().unwrap_or(false) {
        return;
    }

    let Ok(pos) = window.outer_position() else {
        return;
    };
    let scale_factor = window.scale_factor().unwrap_or(1.0).max(1.0);
    let logical_size = window.outer_size().ok().map(|size| {
        (
            (size.width as f64 / scale_factor).round().max(1.0) as u32,
            (size.height as f64 / scale_factor).round().max(1.0) as u32,
        )
    });
    crate::geometry_store::save(
        current_mode,
        crate::geometry_store::StoredGeometry {
            x: pos.x,
            y: pos.y,
            width: logical_size.map(|size| size.0),
            height: logical_size.map(|size| size.1),
        },
    );
}

/// Calculate panel position anchored to the saved tray icon rectangle.
pub fn tray_panel_position(app: &AppHandle) -> Option<(i32, i32)> {
    let anchor = current_tray_anchor(app)?;

    let window = app.get_webview_window("main")?;
    let monitors = window.available_monitors().ok()?;

    let monitor = monitor_for_anchor(&monitors, anchor)?;
    let placement = monitor_placement(monitor);

    Some(window_positioner::calculate_panel_position(
        &tray_anchor_rect(anchor),
        &placement.bounds,
        &placement.work_area,
        &tray_panel_size(),
        placement.scale_factor,
    ))
}

/// Calculate panel position for shortcut/second-instance opens (22 % left, centred).
pub fn shortcut_panel_position(app: &AppHandle) -> Option<(i32, i32)> {
    let window = app.get_webview_window("main")?;
    let monitor = window.primary_monitor().ok()??;
    let scale = monitor.scale_factor();

    Some(window_positioner::calculate_shortcut_position(
        &monitor_work_area_rect(&monitor),
        &tray_panel_size(),
        scale,
    ))
}
