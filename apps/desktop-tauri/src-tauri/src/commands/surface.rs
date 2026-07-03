use super::*;

// ── Surface-mode commands ────────────────────────────────────────────

#[tauri::command]
pub fn set_surface_mode(
    mode: String,
    target: SurfaceTarget,
    window: tauri::WebviewWindow,
) -> Result<String, String> {
    let mode = SurfaceMode::parse(&mode).ok_or_else(|| format!("unknown surface mode: {mode}"))?;
    let target = validate_surface_target(mode, target)?;

    crate::shell::transition_to_target(window.app_handle(), mode, target, None)
        .map(|mode| mode.as_str().to_string())
}

#[tauri::command]
pub fn dismiss_tray_panel(app: tauri::AppHandle) -> Result<(), String> {
    crate::shell::hide_to_tray_if_current(&app, |mode| mode == SurfaceMode::TrayPanel).map(|_| ())
}

/// Arm the gesture blur guard before a resize-grip drag or drag-reorder
/// gesture starts its Win32/OLE modal loop, so the transient
/// `Focused(false)` that loop produces doesn't auto-hide the flyout.
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

#[tauri::command]
pub fn reveal_tray_panel_window(
    app: tauri::AppHandle,
    state: tauri::State<'_, Mutex<AppState>>,
) -> Result<(), String> {
    use tauri::Manager;

    if state
        .lock()
        .map_err(|e| e.to_string())?
        .surface_machine
        .current()
        != SurfaceMode::TrayPanel
    {
        return Ok(());
    }

    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "main window unavailable".to_string())?;
    window.show().map_err(|e| e.to_string())?;
    state
        .lock()
        .map_err(|e| e.to_string())?
        .mark_tray_panel_shown(std::time::Instant::now());
    window.set_focus().map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn close_settings_window(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
) -> Result<(), String> {
    crate::shell::settings_window::dismiss(&app, &window)
}

/// Persist a user-chosen size for the "Pop Out Dashboard" flyout (TrayPanel).
/// Only the size is stored; the flyout is always re-anchored above the tray.
/// The frontend calls this on genuine user drag-resizes, not on its own
/// auto-fit resizes, so auto-fit sizes never freeze the panel.
#[tauri::command]
pub fn set_flyout_size(width: f64, height: f64) -> Result<(), String> {
    let width = (width.round() as i64).clamp(1, i64::from(u32::MAX)) as u32;
    let height = (height.round() as i64).clamp(1, i64::from(u32::MAX)) as u32;
    crate::geometry_store::save(
        SurfaceMode::TrayPanel,
        crate::geometry_store::StoredGeometry {
            x: 0,
            y: 0,
            width: Some(width),
            height: Some(height),
        },
    );
    Ok(())
}

/// Return the remembered flyout size, if the user has manually resized it.
/// The frontend uses this to decide whether to auto-fit (no stored size) or
/// honor the user's size (stored) on open.
#[tauri::command]
pub fn flyout_stored_size() -> Result<Option<(u32, u32)>, String> {
    Ok(crate::geometry_store::load(SurfaceMode::TrayPanel)
        .and_then(|geometry| geometry.width.zip(geometry.height)))
}

#[tauri::command]
pub fn get_current_surface_mode(
    state: tauri::State<'_, Mutex<AppState>>,
) -> Result<String, String> {
    Ok(state
        .lock()
        .map_err(|e| e.to_string())?
        .surface_machine
        .current()
        .as_str()
        .to_string())
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

#[tauri::command]
pub fn get_proof_state(app: tauri::AppHandle) -> Result<ProofStatePayload, String> {
    proof_harness::ensure_proof_mode(&app)?;
    proof_harness::capture_state(&app)
}

#[tauri::command]
pub fn run_proof_command(
    app: tauri::AppHandle,
    command: String,
) -> Result<ProofStatePayload, String> {
    let command =
        ProofCommand::parse(&command).ok_or_else(|| format!("unknown proof command: {command}"))?;
    proof_harness::run_command(&app, command)
}

pub(crate) fn validate_surface_target(
    mode: SurfaceMode,
    target: SurfaceTarget,
) -> Result<SurfaceTarget, String> {
    if mode == SurfaceMode::Hidden {
        return Err("set_surface_mode only supports visible surfaces".into());
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
