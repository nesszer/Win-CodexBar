//! Codex local Workspaces snapshot bridge.

use codexbar::codex_workspaces::{
    CodexLocalProjectUsageSnapshot, CodexSessionDetails, CodexSessionToolActivity,
    CodexWorkspacesIndex, SessionDetailCheckpoint,
};
use codexbar::settings::Settings;
use tauri::State;

use crate::state::AppState;
use std::sync::Mutex;

/// Return the local Codex workspaces usage snapshot.
///
/// Privacy-redacts paths/titles when `settings.hide_personal_info` is set.
#[tauri::command]
pub async fn get_codex_workspaces_snapshot(
    _state: State<'_, Mutex<AppState>>,
    force_refresh: Option<bool>,
    history_days: Option<u32>,
) -> Result<CodexLocalProjectUsageSnapshot, String> {
    let force = force_refresh.unwrap_or(false);
    let days = history_days.unwrap_or(30);
    let hide = Settings::load().hide_personal_info;

    tauri::async_runtime::spawn_blocking(move || {
        let index = CodexWorkspacesIndex::new(days);
        let mut snapshot = index
            .load_snapshot(force, |_| {})
            .map_err(|e| e.to_string())?;
        if hide {
            snapshot.redact_for_privacy();
        }
        Ok(snapshot)
    })
    .await
    .map_err(|e| format!("workspaces worker failed: {e}"))?
}

/// Read one Codex conversation's native tool operations on demand.
///
/// Names and command previews are removed when `settings.hide_personal_info` is set.
#[tauri::command]
pub async fn get_codex_session_tool_activity(
    session_id: String,
    history_days: Option<u32>,
) -> Result<CodexSessionToolActivity, String> {
    let days = history_days.unwrap_or(30);
    let hide = Settings::load().hide_personal_info;

    tauri::async_runtime::spawn_blocking(move || {
        let mut activity = CodexWorkspacesIndex::new(days)
            .load_session_tool_activity(&session_id)
            .map_err(|e| e.to_string())?;
        if hide {
            activity.redact_for_privacy();
        }
        Ok(activity)
    })
    .await
    .map_err(|e| format!("tool activity worker failed: {e}"))?
}

/// Read one Codex conversation's request-level usage on demand.
///
/// `checkpoint` is the summary the UI shows; a read that no longer matches it
/// is refused with `sourceChanged`. Errors are the stable codes
/// `unavailable`, `sourceChanged` and `limitExceeded`.
#[tauri::command]
pub async fn get_codex_session_details(
    session_id: String,
    history_days: Option<u32>,
    checkpoint: SessionDetailCheckpoint,
) -> Result<CodexSessionDetails, String> {
    let days = history_days.unwrap_or(30);

    tauri::async_runtime::spawn_blocking(move || {
        CodexWorkspacesIndex::new(days)
            .load_session_details(&session_id, &checkpoint)
            .map_err(|e| e.code().to_string())
    })
    .await
    .map_err(|_| "unavailable".to_string())?
}
