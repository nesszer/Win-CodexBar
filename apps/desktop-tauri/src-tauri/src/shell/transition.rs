//! Surface transition orchestration: applying transitions, resolving requests,
//! recovery, and snapshot bookkeeping.

use std::sync::Mutex;

use tauri::{AppHandle, Manager, WebviewWindow};

use crate::events;
use crate::proof_harness;
use crate::state::AppState;
use crate::surface::{SurfaceMode, SurfaceTransition, WindowProperties};
use crate::surface_target::SurfaceTarget;
use crate::window_positioner::{self, PanelSize, Rect};

use super::geometry::surface_panel_size;
use super::position::default_surface_position;
use super::window::{apply_window_layout, apply_window_properties, show_window};
use super::{SHELL_TRANSITION_SERIAL, ShellTransitionRequest};

/// Positions from the positioner pipeline are in Tauri's physical coordinate
/// space (e.g. 5120×2880 at 200 % DPI on a 2560×1440 logical desktop).
/// `set_position(PhysicalPosition)` is converted to OS logical coordinates by
/// tao internally (divides by the window's scale factor), so we pass the
/// physical coordinates through without manual division.
fn os_position(_window: &WebviewWindow, x: i32, y: i32) -> tauri::PhysicalPosition<i32> {
    tauri::PhysicalPosition::new(x, y)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SurfaceSnapshot {
    pub mode: SurfaceMode,
    pub target: SurfaceTarget,
}

pub(super) enum TransitionResolution {
    ModeChange {
        transition: SurfaceTransition,
        target: SurfaceTarget,
    },
    SameModeRetarget {
        mode: SurfaceMode,
        target: SurfaceTarget,
    },
    SameModeReopen {
        mode: SurfaceMode,
        target: SurfaceTarget,
    },
    Noop {
        mode: SurfaceMode,
    },
}

pub fn transition_to_target(
    app: &AppHandle,
    mode: SurfaceMode,
    target: SurfaceTarget,
    position: Option<(i32, i32)>,
) -> Result<SurfaceMode, String> {
    apply_transition_request_with_strategy(
        app,
        ShellTransitionRequest {
            mode,
            target,
            position,
        },
        false,
    )
}

pub fn reopen_to_target(
    app: &AppHandle,
    mode: SurfaceMode,
    target: SurfaceTarget,
    position: Option<(i32, i32)>,
) -> Result<SurfaceMode, String> {
    apply_transition_request_with_strategy(
        app,
        ShellTransitionRequest {
            mode,
            target,
            position,
        },
        true,
    )
}

fn apply_transition_request_with_strategy(
    app: &AppHandle,
    request: ShellTransitionRequest,
    force_same_mode_apply: bool,
) -> Result<SurfaceMode, String> {
    apply_transition_request(app, request, force_same_mode_apply)
}

fn apply_transition_request(
    app: &AppHandle,
    request: ShellTransitionRequest,
    force_same_mode_apply: bool,
) -> Result<SurfaceMode, String> {
    let _transition_guard = SHELL_TRANSITION_SERIAL.lock().unwrap();
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "main window unavailable".to_string())?;
    let st = app
        .try_state::<Mutex<AppState>>()
        .ok_or_else(|| "app state unavailable".to_string())?;

    let (previous, resolution) = {
        let guard = st.lock().unwrap();
        let previous = current_surface_snapshot(&guard);
        let resolution = resolve_transition_request(&guard, &request, force_same_mode_apply);
        (previous, resolution)
    };

    let synth = should_synthesize_default_position(&resolution);

    let position = resolve_transition_position(request.position, &resolution, synth, || {
        default_surface_position(app, request.mode)
    })
    .or_else(|| preserved_visible_mode_change_position(&window, &resolution));

    match resolution {
        TransitionResolution::ModeChange { transition, target } => {
            apply_transition(app, &window, &transition, &previous, target, position)
        }
        TransitionResolution::SameModeRetarget { mode, target } => {
            apply_same_mode_target_update(app, &window, mode, target, position)
        }
        TransitionResolution::SameModeReopen { mode, target } => {
            let transition = SurfaceTransition {
                from: mode,
                to: mode,
                properties: mode.window_properties(),
            };
            apply_transition(app, &window, &transition, &previous, target, position)
        }
        TransitionResolution::Noop { mode } => Ok(mode),
    }
}

pub(super) fn resolve_transition_request(
    state: &AppState,
    request: &ShellTransitionRequest,
    force_same_mode_apply: bool,
) -> TransitionResolution {
    let mode = state.surface_machine.current();
    let target = AppState::resolved_target_for_mode(request.mode, Some(request.target.clone()));

    if mode != request.mode {
        TransitionResolution::ModeChange {
            transition: SurfaceTransition {
                from: mode,
                to: request.mode,
                properties: request.mode.window_properties(),
            },
            target,
        }
    } else if state.current_target != target {
        TransitionResolution::SameModeRetarget { mode, target }
    } else if force_same_mode_apply {
        TransitionResolution::SameModeReopen { mode, target }
    } else {
        TransitionResolution::Noop { mode }
    }
}

pub(super) fn current_surface_snapshot(state: &AppState) -> SurfaceSnapshot {
    SurfaceSnapshot {
        mode: state.surface_machine.current(),
        target: state.current_target.clone(),
    }
}

pub(super) fn resolve_transition_position<F>(
    requested: Option<(i32, i32)>,
    resolution: &TransitionResolution,
    synthesize_default: bool,
    fallback: F,
) -> Option<(i32, i32)>
where
    F: FnOnce() -> Option<(i32, i32)>,
{
    if requested.is_some() {
        return requested;
    }

    match resolution {
        TransitionResolution::ModeChange { .. } | TransitionResolution::SameModeReopen { .. }
            if synthesize_default =>
        {
            fallback()
        }
        TransitionResolution::ModeChange { .. }
        | TransitionResolution::SameModeReopen { .. }
        | TransitionResolution::SameModeRetarget { .. }
        | TransitionResolution::Noop { .. } => None,
    }
}

pub(super) fn reclamp_preserved_visible_position(
    current_top_left: (i32, i32),
    monitor_rect: &Rect,
    destination_mode: SurfaceMode,
    scale_factor: f64,
) -> (i32, i32) {
    window_positioner::clamp_position_to_work_area(
        current_top_left.0,
        current_top_left.1,
        monitor_rect,
        &surface_panel_size(destination_mode),
        scale_factor,
    )
}

pub(super) fn monitor_for_preserved_visible_position(
    monitors: &[(Rect, f64)],
    current_top_left: (i32, i32),
    current_size: Option<(u32, u32)>,
) -> Option<(Rect, f64)> {
    if let Some((rect, scale_factor)) = monitors.iter().find(|(rect, _)| {
        super::geometry::point_in_rect(rect, current_top_left.0, current_top_left.1)
    }) {
        return Some((*rect, *scale_factor));
    }

    if let Some((width, height)) = current_size {
        let center_x = current_top_left.0 + width as i32 / 2;
        let center_y = current_top_left.1 + height as i32 / 2;
        if let Some((rect, scale_factor)) = monitors
            .iter()
            .find(|(rect, _)| super::geometry::point_in_rect(rect, center_x, center_y))
        {
            return Some((*rect, *scale_factor));
        }
    }

    None
}

fn current_monitor_work_area(
    window: &WebviewWindow,
    current_top_left: (i32, i32),
) -> Option<(Rect, f64)> {
    let current_size = window
        .outer_size()
        .ok()
        .map(|size| (size.width, size.height));

    if let Ok(monitors) = window.available_monitors() {
        let monitor_work_areas = monitors
            .iter()
            .map(|monitor| {
                (
                    super::geometry::monitor_work_area_rect(monitor),
                    monitor.scale_factor(),
                )
            })
            .collect::<Vec<_>>();
        if let Some(monitor) = monitor_for_preserved_visible_position(
            &monitor_work_areas,
            current_top_left,
            current_size,
        ) {
            return Some(monitor);
        }
    }

    if let Ok(Some(monitor)) = window.current_monitor() {
        return Some((
            super::geometry::monitor_work_area_rect(&monitor),
            monitor.scale_factor(),
        ));
    }

    let monitor = window.primary_monitor().ok()??;
    Some((
        super::geometry::monitor_work_area_rect(&monitor),
        monitor.scale_factor(),
    ))
}

fn clamp_current_window_to_work_area(window: &WebviewWindow) {
    let Ok(position) = window.outer_position() else {
        return;
    };
    let Ok(size) = window.outer_size() else {
        return;
    };
    let Some((monitor_rect, _scale_factor)) =
        current_monitor_work_area(window, (position.x, position.y))
    else {
        return;
    };
    let panel_size = PanelSize {
        width: size.width,
        height: size.height,
    };
    let (x, y) = window_positioner::clamp_position_to_work_area(
        position.x,
        position.y,
        &monitor_rect,
        &panel_size,
        1.0,
    );
    if x != position.x || y != position.y {
        let _ = window.set_position(os_position(window, x, y));
    }
}

fn preserved_visible_mode_change_position(
    window: &WebviewWindow,
    resolution: &TransitionResolution,
) -> Option<(i32, i32)> {
    let TransitionResolution::ModeChange { transition, .. } = resolution else {
        return None;
    };

    if !transition.from.window_properties().visible || !transition.to.window_properties().visible {
        return None;
    }

    // When opening Settings from TrayPanel, don't preserve the tray's
    // bottom-right position — Settings should appear centered on screen
    // like a standalone window (matching macOS behavior).
    if transition.from == SurfaceMode::TrayPanel && transition.to == SurfaceMode::Settings {
        return None;
    }

    let current = window.outer_position().ok()?;
    let current_top_left = (current.x, current.y);
    let (monitor_rect, scale_factor) = current_monitor_work_area(window, current_top_left)?;

    Some(reclamp_preserved_visible_position(
        current_top_left,
        &monitor_rect,
        transition.to,
        scale_factor,
    ))
}

pub(super) fn should_synthesize_default_position(resolution: &TransitionResolution) -> bool {
    match resolution {
        TransitionResolution::ModeChange { transition, .. } => {
            transition.from == SurfaceMode::Hidden
                || (transition.from == SurfaceMode::TrayPanel
                    && transition.to == SurfaceMode::Settings)
        }
        TransitionResolution::SameModeReopen { .. } => true,
        TransitionResolution::SameModeRetarget { .. } | TransitionResolution::Noop { .. } => false,
    }
}

pub(super) fn restore_surface_snapshot(state: &mut AppState, snapshot: &SurfaceSnapshot) {
    if snapshot.mode == SurfaceMode::Hidden {
        let _ = state.hide_surface();
    } else {
        if snapshot.mode == SurfaceMode::TrayPanel {
            state.last_shown_at = Some(std::time::Instant::now());
        }
        let _ = state.transition_surface(snapshot.mode, snapshot.target.clone());
    }
}

fn commit_surface_snapshot(app: &AppHandle, snapshot: &SurfaceSnapshot) -> Result<(), String> {
    let st = app
        .try_state::<Mutex<AppState>>()
        .ok_or_else(|| "app state unavailable".to_string())?;
    let mut guard = st.lock().unwrap();
    restore_surface_snapshot(&mut guard, snapshot);
    Ok(())
}

pub(super) fn hidden_surface_snapshot() -> SurfaceSnapshot {
    SurfaceSnapshot {
        mode: SurfaceMode::Hidden,
        target: SurfaceTarget::Summary,
    }
}

pub(super) fn restore_recovery_surface<F>(
    recovery: &SurfaceSnapshot,
    mut apply_properties: F,
) -> Result<(), String>
where
    F: FnMut(&WindowProperties) -> Result<(), String>,
{
    apply_properties(&recovery.mode.window_properties())
}

pub(super) fn recovery_snapshot_for_failed_transition(
    transition: &SurfaceTransition,
    previous: &SurfaceSnapshot,
    requested_target: &SurfaceTarget,
) -> SurfaceSnapshot {
    if previous.mode == SurfaceMode::Hidden {
        SurfaceSnapshot {
            mode: transition.to,
            target: requested_target.clone(),
        }
    } else {
        previous.clone()
    }
}

fn apply_same_mode_target_update(
    app: &AppHandle,
    window: &WebviewWindow,
    mode: SurfaceMode,
    target: SurfaceTarget,
    position: Option<(i32, i32)>,
) -> Result<SurfaceMode, String> {
    if let Some((x, y)) = position {
        let _ = window.set_position(os_position(window, x, y));
    }
    clamp_current_window_to_work_area(window);
    // Commit state + emit event before making the window visible so the
    // React frontend renders the correct surface first.
    commit_surface_snapshot(
        app,
        &SurfaceSnapshot {
            mode,
            target: target.clone(),
        },
    )?;
    events::emit_surface_mode_changed(app, mode, mode, target);
    let _ = window.show();
    let _ = window.set_focus();
    proof_harness::sync_after_surface_transition(app);
    Ok(mode)
}

pub(super) fn apply_transition(
    app: &AppHandle,
    window: &WebviewWindow,
    transition: &SurfaceTransition,
    previous: &SurfaceSnapshot,
    current_target: SurfaceTarget,
    position: Option<(i32, i32)>,
) -> Result<SurfaceMode, String> {
    if let Some((x, y)) = position {
        let _ = window.set_position(os_position(window, x, y));
    }

    // Phase 1: apply layout properties (size, decorations, etc.) WITHOUT
    // making the window visible yet.
    match apply_window_layout(window, &transition.properties) {
        Ok(needs_show) => {
            // Phase 2: commit state + emit event so the React frontend can
            // start rendering the correct surface BEFORE the window appears.
            commit_surface_snapshot(
                app,
                &SurfaceSnapshot {
                    mode: transition.to,
                    target: current_target.clone(),
                },
            )?;
            events::emit_surface_mode_changed(app, transition.from, transition.to, current_target);

            // Phase 3: now make the window visible. TrayPanel is revealed by
            // the frontend after its first layout pass so Windows never shows
            // the pre-measure blank/backing frame.
            if needs_show && transition.to != SurfaceMode::TrayPanel {
                let _ = show_window(window);
            }
            clamp_current_window_to_work_area(window);

            proof_harness::sync_after_surface_transition(app);
            Ok(transition.to)
        }
        Err(err) => {
            let recovery =
                recovery_snapshot_for_failed_transition(transition, previous, &current_target);
            if let Err(recovery_err) = restore_recovery_surface(&recovery, |properties| {
                apply_window_properties(window, properties)
            }) {
                let hidden = hidden_surface_snapshot();
                if let Err(hide_err) = window.hide().map_err(|e| e.to_string()) {
                    tracing::warn!(
                        "shell: failed to restore recovery surface during {:?} -> {:?} after reverting to {:?}: apply error: {}; recovery error: {}; hide error: {}",
                        transition.from,
                        transition.to,
                        recovery.mode,
                        err,
                        recovery_err,
                        hide_err
                    );
                    return Err(format!(
                        "failed to recover shell surface after {:?} -> {:?}: apply error: {}; recovery error: {}; hide error: {}",
                        transition.from, transition.to, err, recovery_err, hide_err
                    ));
                }
                commit_surface_snapshot(app, &hidden)?;
                events::emit_surface_mode_changed(
                    app,
                    transition.from,
                    hidden.mode,
                    hidden.target.clone(),
                );
                proof_harness::sync_after_surface_transition(app);
                tracing::warn!(
                    "shell: failed to restore recovery surface during {:?} -> {:?} after reverting to {:?}; forcing hidden surface: apply error: {}; recovery error: {}",
                    transition.from,
                    transition.to,
                    recovery.mode,
                    err,
                    recovery_err
                );
                return Ok(hidden.mode);
            }
            commit_surface_snapshot(app, &recovery)?;
            events::emit_surface_mode_changed(
                app,
                transition.from,
                recovery.mode,
                recovery.target.clone(),
            );
            proof_harness::sync_after_surface_transition(app);
            tracing::warn!(
                "shell: recovered from window-property failure during {:?} -> {:?} by reapplying {:?}: {}",
                transition.from,
                transition.to,
                recovery.mode,
                err
            );
            Ok(recovery.mode)
        }
    }
}

pub fn handle_tray_panel_click(app: &AppHandle, position: Option<(i32, i32)>) {
    const BLUR_DISMISS_CLICK_WINDOW: std::time::Duration = std::time::Duration::from_millis(250);

    let consumed_blur_dismissal = {
        let st = app.state::<Mutex<AppState>>();
        st.lock()
            .unwrap()
            .take_recent_blur_dismissal(std::time::Instant::now(), BLUR_DISMISS_CLICK_WINDOW)
    };

    if consumed_blur_dismissal {
        return;
    }

    toggle_tray_panel(app, position);
}

/// Toggle the tray panel: hide if currently showing, show at `position` otherwise.
pub fn toggle_tray_panel(app: &AppHandle, position: Option<(i32, i32)>) {
    let current = {
        let st = app.state::<Mutex<AppState>>();
        st.lock().unwrap().surface_machine.current()
    };
    let main_window_visible = app
        .get_webview_window("main")
        .and_then(|window| window.is_visible().ok())
        .unwrap_or(false);

    if should_hide_tray_panel_on_toggle(current, main_window_visible) {
        let _ = super::window::hide_to_tray(app);
    } else {
        let _ = reopen_to_target(
            app,
            SurfaceMode::TrayPanel,
            SurfaceTarget::Summary,
            position,
        );
    }
}

pub(super) fn should_hide_tray_panel_on_toggle(
    current: SurfaceMode,
    main_window_visible: bool,
) -> bool {
    current == SurfaceMode::TrayPanel && main_window_visible
}
