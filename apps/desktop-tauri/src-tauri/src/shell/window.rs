//! Window-property application and the hide-to-tray flow.

use std::sync::Mutex;

use tauri::{AppHandle, Manager, WebviewWindow};

use crate::state::AppState;
use crate::surface::{SurfaceMode, SurfaceTransition, WindowProperties};
use crate::surface_target::SurfaceTarget;

use super::SHELL_TRANSITION_SERIAL;
use super::transition::{SurfaceSnapshot, apply_transition, current_surface_snapshot};

pub(super) struct HideToTrayPlan {
    pub previous: SurfaceSnapshot,
    pub transition: Option<SurfaceTransition>,
    pub target: SurfaceTarget,
}

/// Apply the window properties dictated by a surface mode.
pub fn apply_window_properties(
    window: &WebviewWindow,
    props: &WindowProperties,
) -> Result<(), String> {
    let needs_show = apply_window_layout(window, props)?;
    if needs_show {
        show_window(window)?;
    }
    Ok(())
}

/// Apply layout properties (decorations, size, always-on-top) WITHOUT making
/// the window visible.  Returns `true` when the caller should subsequently
/// call [`show_window`] to make it visible, or `false` when the mode hides
/// the window (already handled internally).
pub fn apply_window_layout(
    window: &WebviewWindow,
    props: &WindowProperties,
) -> Result<bool, String> {
    let map_err = |e: tauri::Error| e.to_string();

    window.set_decorations(props.decorations).map_err(map_err)?;
    window.set_resizable(props.resizable).map_err(map_err)?;
    window
        .set_always_on_top(props.always_on_top)
        .map_err(map_err)?;
    window
        .set_skip_taskbar(props.skip_taskbar)
        .map_err(map_err)?;
    // Borderless surfaces draw their own chrome via a DWM subclass that zeros
    // the native non-client area. Apply it AFTER `set_resizable` so the
    // resizable variant sees WS_THICKFRAME present and preserves it (keeping
    // the native resize affordance). Native decorations are incompatible with
    // this subclass, so decorated surfaces skip it.
    if !props.decorations {
        if props.resizable {
            super::dwm::force_dark_caption_resizable(window);
        } else {
            super::dwm::force_dark_caption(window);
        }
    }

    if props.visible {
        let (width, height) = remembered_mode_for_properties(props)
            .map(|mode| logical_size_from_geometry(mode, props, crate::geometry_store::load(mode)))
            .unwrap_or((props.width, props.height));
        let (width, height) = capped_logical_size(window, width, height);
        let size = tauri::LogicalSize::new(width, height);
        window.set_size(size).map_err(map_err)?;

        if let (Some(min_w), Some(min_h)) = (props.min_width, props.min_height) {
            window
                .set_min_size(Some(tauri::LogicalSize::new(min_w, min_h)))
                .map_err(map_err)?;
        } else {
            window
                .set_min_size::<tauri::LogicalSize<f64>>(None)
                .map_err(map_err)?;
        }

        Ok(true) // caller should show
    } else {
        window.hide().map_err(map_err)?;
        Ok(false)
    }
}

fn remembered_mode_for_properties(props: &WindowProperties) -> Option<SurfaceMode> {
    [SurfaceMode::PopOut, SurfaceMode::Settings]
        .into_iter()
        .find(|mode| {
            let mode_props = mode.window_properties();
            props.visible == mode_props.visible
                && props.decorations == mode_props.decorations
                && props.resizable == mode_props.resizable
                && props.width == mode_props.width
                && props.height == mode_props.height
                && props.min_width == mode_props.min_width
                && props.min_height == mode_props.min_height
                && props.always_on_top == mode_props.always_on_top
                && props.blur_dismiss == mode_props.blur_dismiss
                && props.skip_taskbar == mode_props.skip_taskbar
        })
}

pub(super) fn logical_size_from_geometry(
    mode: SurfaceMode,
    props: &WindowProperties,
    stored: Option<crate::geometry_store::StoredGeometry>,
) -> (f64, f64) {
    if !crate::geometry_store::should_remember(mode) {
        return (props.width, props.height);
    }

    let width = stored
        .and_then(|geometry| geometry.width)
        .map(|width| width.max(1) as f64)
        .unwrap_or(props.width);
    let height = stored
        .and_then(|geometry| geometry.height)
        .map(|height| height.max(1) as f64)
        .unwrap_or(props.height);

    (
        props.min_width.map_or(width, |min| width.max(min)),
        props.min_height.map_or(height, |min| height.max(min)),
    )
}

fn capped_logical_size(window: &WebviewWindow, width: f64, height: f64) -> (f64, f64) {
    const MARGIN: f64 = 16.0;

    let Some(monitor) = window
        .current_monitor()
        .ok()
        .flatten()
        .or_else(|| window.primary_monitor().ok().flatten())
    else {
        return (width, height);
    };

    let scale = monitor.scale_factor();
    let scale = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    };
    let work_area = monitor.work_area();
    let max_width = (work_area.size.width as f64 / scale - MARGIN).max(320.0);
    let max_height = (work_area.size.height as f64 / scale - MARGIN).max(240.0);

    (width.min(max_width), height.min(max_height))
}

/// Make the window visible and give it input focus.
pub fn show_window(window: &WebviewWindow) -> Result<(), String> {
    let map_err = |e: tauri::Error| e.to_string();
    window.show().map_err(map_err)?;
    window.set_focus().map_err(map_err)?;
    Ok(())
}

pub fn hide_to_tray(app: &AppHandle) -> Result<SurfaceMode, String> {
    hide_to_tray_if_current(app, |_| true).map(|mode| mode.unwrap_or(SurfaceMode::Hidden))
}

pub fn hide_to_tray_if_current<P>(
    app: &AppHandle,
    is_eligible: P,
) -> Result<Option<SurfaceMode>, String>
where
    P: FnOnce(SurfaceMode) -> bool,
{
    let _transition_guard = SHELL_TRANSITION_SERIAL.lock().unwrap();
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "main window unavailable".to_string())?;
    let st = app
        .try_state::<Mutex<AppState>>()
        .ok_or_else(|| "app state unavailable".to_string())?;
    let plan = {
        let mut guard = st.lock().unwrap();
        prepare_hide_to_tray_if_current(&mut guard, is_eligible)
    };

    let Some(plan) = plan else {
        return Ok(None);
    };

    if let Some(transition) = plan.transition {
        apply_transition(app, &window, &transition, &plan.previous, plan.target, None).map(Some)
    } else {
        let _ = window.hide();
        Ok(Some(SurfaceMode::Hidden))
    }
}

#[allow(dead_code)]
pub fn hide_to_tray_state(state: &mut AppState) {
    let _ = prepare_hide_to_tray_if_current(state, |_| true);
}

pub(super) fn prepare_hide_to_tray_if_current<P>(
    state: &mut AppState,
    is_eligible: P,
) -> Option<HideToTrayPlan>
where
    P: FnOnce(SurfaceMode) -> bool,
{
    let current = state.surface_machine.current();
    if !is_eligible(current) {
        return None;
    }

    let previous = current_surface_snapshot(state);
    let transition = state.hide_surface();
    Some(HideToTrayPlan {
        previous,
        transition,
        target: state.current_target.clone(),
    })
}
