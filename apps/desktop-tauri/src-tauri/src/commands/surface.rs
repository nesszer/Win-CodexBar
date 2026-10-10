use super::*;
use crate::shell::activation::Activation;

// ── Surface-mode commands ────────────────────────────────────────────

#[tauri::command]
pub fn set_surface_mode(
    mode: String,
    target: SurfaceTarget,
    window: tauri::WebviewWindow,
) -> Result<String, String> {
    let mode = SurfaceMode::parse(&mode).ok_or_else(|| format!("unknown surface mode: {mode}"))?;
    let target = validate_surface_target(mode, target)?;

    // The frontend only calls this from a click (e.g. a Settings tab).
    crate::shell::transition_to_target(
        window.app_handle(),
        mode,
        target,
        None,
        Activation::UserAction,
    )
    .map(|mode| mode.as_str().to_string())
}

#[tauri::command]
pub fn dismiss_tray_panel(app: tauri::AppHandle) -> Result<(), String> {
    crate::shell::flyout_window::hide(&app)
}

#[tauri::command]
pub fn begin_flyout_gesture(app: tauri::AppHandle) -> Result<(), String> {
    let state = app
        .try_state::<Mutex<AppState>>()
        .ok_or_else(|| "app state unavailable".to_string())?;
    state
        .lock()
        .map_err(|e| e.to_string())?
        .begin_gesture_blur_guard(std::time::Instant::now());
    Ok(())
}

/// Disarm the gesture blur guard when a gesture ends (mouseup / dragend),
/// so a genuine outside click can dismiss the flyout again immediately.
#[tauri::command]
pub fn end_flyout_gesture(app: tauri::AppHandle) -> Result<(), String> {
    let state = app
        .try_state::<Mutex<AppState>>()
        .ok_or_else(|| "app state unavailable".to_string())?;
    state
        .lock()
        .map_err(|e| e.to_string())?
        .end_gesture_blur_guard();
    Ok(())
}

/// Open (or focus) a detached Settings/About window.
///
/// Unlike `set_surface_mode`, this spawns a *separate* window so the tray
/// panel stays open.  On Windows, `WebviewWindowBuilder::build` deadlocks
/// inside synchronous Tauri commands, so this must be `async`.
#[tauri::command]
pub async fn open_settings_window(app: tauri::AppHandle, tab: String) -> Result<(), String> {
    crate::shell::settings_window::open_or_focus(&app, &tab)
}

/// Open (or focus) the detached flyout ("Pop Out Dashboard") window, the
/// only dashboard layout. Used by the frontend global-shortcut fallback.
/// Same `async` requirement as `open_settings_window`:
/// `WebviewWindowBuilder::build` deadlocks inside synchronous Tauri commands
/// on Windows.
#[tauri::command]
pub async fn open_flyout_window(app: tauri::AppHandle) -> Result<(), String> {
    crate::shell::flyout_window::open_or_focus(&app, None, Activation::UserAction)
}

/// Reveal the flyout window after the frontend's first layout pass. Called by
/// `useTrayPanelLayout` once content has been measured and auto-fit, so
/// Windows never shows a pre-measure blank/backing frame.
///
/// No-ops when the flyout window doesn't exist or no one-shot reveal is pending.
/// The window takes focus only as far as the pending reveal's activation
/// allows (see `shell::activation`).
#[tauri::command]
pub fn reveal_tray_panel_window(
    app: tauri::AppHandle,
    state: tauri::State<'_, Mutex<AppState>>,
) -> Result<(), String> {
    use tauri::Manager;

    let Some(window) = app.get_webview_window(crate::shell::flyout_window::FLYOUT_LABEL) else {
        return Ok(());
    };
    let mut guard = state.lock().map_err(|e| e.to_string())?;
    let Some(activation) = guard.take_pending_flyout_reveal() else {
        return Ok(());
    };
    drop(guard);
    window.show().map_err(|e| e.to_string())?;
    state
        .lock()
        .map_err(|e| e.to_string())?
        .mark_tray_panel_shown(std::time::Instant::now());
    crate::shell::activation::apply(&window, activation)
}

#[tauri::command]
pub fn close_settings_window(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
) -> Result<(), String> {
    crate::shell::settings_window::dismiss(&app, &window)
}

#[tauri::command]
pub fn get_current_surface_state(
    state: tauri::State<'_, Mutex<AppState>>,
) -> Result<CurrentSurfaceState, String> {
    let guard = state.lock().map_err(|e| e.to_string())?;
    Ok(CurrentSurfaceState {
        mode: guard.surface_machine.current().as_str().to_string(),
        target: guard.current_target.clone(),
    })
}

pub(crate) fn validate_surface_target(
    mode: SurfaceMode,
    target: SurfaceTarget,
) -> Result<SurfaceTarget, String> {
    if mode == SurfaceMode::Hidden {
        return Err("set_surface_mode only supports visible surfaces".into());
    }

    // The legacy PopOut layout on `main` is retired; the dashboard is the
    // tray-panel flyout, opened with `open_flyout_window`.
    if mode == SurfaceMode::PopOut {
        return Err("the popOut surface is retired; use open_flyout_window".into());
    }

    if target.mode() != mode {
        return Err(format!(
            "surface target '{}' is not valid for mode '{}'",
            target_label(&target),
            mode.as_str()
        ));
    }

    Ok(target)
}

fn target_label(target: &SurfaceTarget) -> String {
    match target {
        SurfaceTarget::Summary => "summary".into(),
        SurfaceTarget::Dashboard => "dashboard".into(),
        SurfaceTarget::Provider { provider_id } => format!("provider:{provider_id}"),
        SurfaceTarget::Settings { tab } => format!("settings:{tab}"),
    }
}
