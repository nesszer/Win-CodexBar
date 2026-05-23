//! Updater lifecycle commands: check, download, apply, dismiss, and release-page linking.
//!
//! State transitions are mirrored through [`events::emit_update_state_changed`]
//! so the frontend can react without polling.

use std::sync::Mutex;

use tauri::Manager;

use super::open_url_in_browser;
use crate::events;
use crate::state::{AppState, UpdateState, UpdateStatePayload};
use codexbar::updater::UpdateInfo;

#[tauri::command]
pub fn get_update_state(state: tauri::State<'_, Mutex<AppState>>) -> UpdateStatePayload {
    state
        .lock()
        .map(|guard| guard.update_payload())
        .unwrap_or_else(|_| UpdateState::default().to_payload())
}

#[tauri::command]
pub async fn check_for_updates(
    app: tauri::AppHandle,
    state: tauri::State<'_, Mutex<AppState>>,
) -> Result<UpdateStatePayload, String> {
    // Guard: skip if already checking or downloading.
    {
        let mut guard = state.lock().map_err(|e| e.to_string())?;
        match guard.update_state {
            UpdateState::Checking | UpdateState::Downloading(_) => {
                return Ok(guard.update_payload());
            }
            _ => {}
        }
        guard.update_state = UpdateState::Checking;
        guard.update_info = None;
        guard.installer_path = None;
    }

    let checking_payload = {
        let guard = state.lock().map_err(|e| e.to_string())?;
        guard.update_payload()
    };
    events::emit_update_state_changed(&app, &checking_payload);

    let settings = codexbar::settings::Settings::load();

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        codexbar::updater::check_for_updates_with_channel(settings.update_channel),
    )
    .await;

    let (new_state, new_info) = match result {
        Ok(Some(info)) => (UpdateState::Available(info.version.clone()), Some(info)),
        Ok(None) => (UpdateState::Idle, None),
        Err(_) => (
            UpdateState::Error("Update check timed out".to_string()),
            None,
        ),
    };

    let payload = {
        let mut guard = state.lock().map_err(|e| e.to_string())?;
        guard.update_state = new_state;
        guard.update_info = new_info;
        guard.last_update_check_ms = Some(chrono::Utc::now().timestamp_millis());
        guard.update_payload()
    };
    events::emit_update_state_changed(&app, &payload);

    Ok(payload)
}

#[tauri::command]
pub async fn download_update(
    app: tauri::AppHandle,
    state: tauri::State<'_, Mutex<AppState>>,
) -> Result<UpdateStatePayload, String> {
    let info = match update_info_for_download(&state)? {
        DownloadStart::Ready(info) => info,
        DownloadStart::AlreadyDownloading(payload) => return Ok(payload),
    };

    if !info.supports_auto_download() {
        return Err(
            "This update does not support automatic download. Open the release page instead."
                .to_string(),
        );
    }

    let initial_payload = set_downloading_state(&state)?;
    events::emit_update_state_changed(&app, &initial_payload);
    spawn_download_task(app.clone(), info);

    Ok(initial_payload)
}

enum DownloadStart {
    Ready(UpdateInfo),
    AlreadyDownloading(UpdateStatePayload),
}

fn update_info_for_download(
    state: &tauri::State<'_, Mutex<AppState>>,
) -> Result<DownloadStart, String> {
    let guard = state.lock().map_err(|e| e.to_string())?;
    match &guard.update_state {
        UpdateState::Available(_) | UpdateState::Error(_) => {}
        UpdateState::Downloading(_) => {
            return Ok(DownloadStart::AlreadyDownloading(guard.update_payload()));
        }
        _ => return Err("No update available to download".to_string()),
    }
    guard
        .update_info
        .clone()
        .map(DownloadStart::Ready)
        .ok_or("No update information available".to_string())
}

fn set_downloading_state(
    state: &tauri::State<'_, Mutex<AppState>>,
) -> Result<UpdateStatePayload, String> {
    let mut guard = state.lock().map_err(|e| e.to_string())?;
    guard.update_state = UpdateState::Downloading(0.0);
    Ok(guard.update_payload())
}

fn spawn_download_task(app_handle: tauri::AppHandle, info: UpdateInfo) {
    tokio::spawn(async move {
        let (tx, rx) = tokio::sync::watch::channel(codexbar::updater::UpdateState::Available);
        let progress_handle = spawn_download_progress_task(app_handle.clone(), rx);
        let final_payload = run_download_task(app_handle.clone(), info, tx).await;

        events::emit_update_state_changed(&app_handle, &final_payload);
        progress_handle.abort();
    });
}

fn spawn_download_progress_task(
    app: tauri::AppHandle,
    mut rx: tokio::sync::watch::Receiver<codexbar::updater::UpdateState>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        while rx.changed().await.is_ok() {
            let backend_state = rx.borrow().clone();
            if let codexbar::updater::UpdateState::Downloading(progress) = backend_state {
                emit_download_progress(&app, progress);
            }
        }
    })
}

fn emit_download_progress(app: &tauri::AppHandle, progress: f32) {
    let st = app.state::<Mutex<AppState>>();
    let payload = {
        let mut guard = st.lock().unwrap();
        guard.update_state = UpdateState::Downloading(progress);
        guard.update_payload()
    };
    events::emit_update_state_changed(app, &payload);
}

async fn run_download_task(
    app: tauri::AppHandle,
    info: UpdateInfo,
    tx: tokio::sync::watch::Sender<codexbar::updater::UpdateState>,
) -> UpdateStatePayload {
    let download_handle =
        tokio::spawn(async move { codexbar::updater::download_update(&info, tx).await });

    match download_handle.await {
        Ok(Ok(path)) => finish_download(&app, UpdateState::Ready, Some(path)),
        Ok(Err(error)) => finish_download(&app, UpdateState::Error(error), None),
        Err(join_err) => finish_download(
            &app,
            UpdateState::Error(format!("Download task failed: {join_err}")),
            None,
        ),
    }
}

fn finish_download(
    app: &tauri::AppHandle,
    update_state: UpdateState,
    installer_path: Option<std::path::PathBuf>,
) -> UpdateStatePayload {
    let st = app.state::<Mutex<AppState>>();
    let mut guard = st.lock().unwrap();
    guard.update_state = update_state;
    guard.installer_path = installer_path;
    guard.update_payload()
}

#[tauri::command]
pub fn apply_update(state: tauri::State<'_, Mutex<AppState>>) -> Result<(), String> {
    let (path, expected_sha256) = {
        let guard = state.lock().map_err(|e| e.to_string())?;
        let path = guard
            .installer_path
            .clone()
            .ok_or("No downloaded update available to apply")?;
        let expected_sha256 = guard
            .update_info
            .as_ref()
            .and_then(|info| info.expected_sha256.clone())
            .ok_or("Missing SHA256 digest for downloaded update")?;
        (path, expected_sha256)
    };
    codexbar::updater::verify_installer_hash(&path, &expected_sha256)?;
    codexbar::updater::apply_update(&path)
}

#[tauri::command]
pub fn dismiss_update(
    app: tauri::AppHandle,
    state: tauri::State<'_, Mutex<AppState>>,
) -> Result<UpdateStatePayload, String> {
    let payload = {
        let mut guard = state.lock().map_err(|e| e.to_string())?;
        guard.update_state = UpdateState::Idle;
        guard.update_info = None;
        guard.installer_path = None;
        guard.update_payload()
    };
    events::emit_update_state_changed(&app, &payload);
    Ok(payload)
}

#[tauri::command]
pub fn open_release_page(state: tauri::State<'_, Mutex<AppState>>) -> Result<(), String> {
    let url = {
        let guard = state.lock().map_err(|e| e.to_string())?;
        guard
            .update_info
            .as_ref()
            .map(|info| info.release_url.clone())
            .ok_or("No update information available")?
    };
    open_url_in_browser(&url)
}
