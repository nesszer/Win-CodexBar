use super::invalidate_account_usage;
use crate::state::AppState;
use codexbar::core::ProviderId;
use codexbar::providers::claude::accounts::{self, AccountManager, ClaudeAccount};
use codexbar::providers::claude::claude_swap::{self, ClaudeSwapAccount};
use serde::Serialize;
use std::sync::Mutex;
use tauri::Emitter;
use tauri::Manager;

static MUTATION: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[tauri::command]
pub fn claude_accounts_list() -> Result<Vec<ClaudeAccount>, String> {
    AccountManager::new()
        .and_then(|m| m.list())
        .map_err(|e| e.to_string())
}

/// External claude-swap accounts plus adapter status for the settings UI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeSwapAccountsState {
    pub enabled: bool,
    pub executable_configured: bool,
    pub accounts: Vec<ClaudeSwapAccount>,
    pub error: Option<String>,
}

fn claude_swap_accounts_state() -> ClaudeSwapAccountsState {
    let settings = codexbar::settings::Settings::load();
    let enabled = settings.claude_swap_enabled();
    let executable_path = settings.claude_swap_executable_path().to_string();
    let executable_configured = !executable_path.trim().is_empty();
    if !enabled || !executable_configured {
        return ClaudeSwapAccountsState {
            enabled,
            executable_configured,
            accounts: Vec::new(),
            error: None,
        };
    }
    match claude_swap::read_account_list(&executable_path) {
        Ok(list) => ClaudeSwapAccountsState {
            enabled,
            executable_configured,
            accounts: claude_swap::project_accounts(&list, settings.hide_personal_info),
            error: None,
        },
        // Adapter failures are isolated from ambient Claude usage: the last
        // built-in account list still renders and the error is surfaced inline.
        Err(error) => ClaudeSwapAccountsState {
            enabled,
            executable_configured,
            accounts: Vec::new(),
            error: Some(error.to_string()),
        },
    }
}

#[tauri::command]
pub async fn claude_swap_accounts_list() -> Result<ClaudeSwapAccountsState, String> {
    tauri::async_runtime::spawn_blocking(claude_swap_accounts_state)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn claude_swap_account_switch(app: tauri::AppHandle, slot: u32) -> Result<(), String> {
    let _mutation = MUTATION
        .try_lock()
        .map_err(|_| "A Claude account operation is already in progress.")?;
    let settings = codexbar::settings::Settings::load();
    if !settings.claude_swap_enabled() {
        return Err("claude-swap integration is disabled.".to_string());
    }
    let executable_path = settings.claude_swap_executable_path().to_string();
    if executable_path.trim().is_empty() {
        return Err("No claude-swap executable path is configured.".to_string());
    }
    // Serialize with our own Claude OAuth/account mutations: cswap owns the
    // credential transaction, so the two paths must never overlap.
    let _credentials = accounts::CREDENTIAL_OPERATION.lock().await;
    tauri::async_runtime::spawn_blocking(move || claude_swap::switch_account(&executable_path, slot))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    let pending = {
        let state = app.state::<Mutex<AppState>>();
        let mut state = state.lock().map_err(|e| e.to_string())?;
        invalidate_account_usage(&mut state, ProviderId::Claude)
    };
    crate::events::emit_provider_updated(&app, &pending);
    drop(_credentials);
    changed(&app);
    tauri::async_runtime::spawn(async move {
        let _refresh = super::refresh_providers(app).await;
    });
    Ok(())
}

fn changed(app: &tauri::AppHandle) {
    let _emit = app.emit("claude-accounts-updated", ());
    let handle = app.clone();
    let _dispatch = app.run_on_main_thread(move || crate::tray_bridge::rebuild_tray_menu(&handle));
}

#[tauri::command]
pub async fn claude_account_add(app: tauri::AppHandle) -> Result<(), String> {
    let _mutation = MUTATION
        .try_lock()
        .map_err(|_| "A Claude account operation is already in progress.")?;
    accounts::begin_login();
    let login = tauri::async_runtime::spawn_blocking(accounts::login)
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    let _credentials = accounts::CREDENTIAL_OPERATION.lock().await;
    AccountManager::new()
        .and_then(|m| m.import(login))
        .map_err(|e| e.to_string())?;
    changed(&app);
    Ok(())
}

#[tauri::command]
pub fn claude_account_cancel_login() {
    accounts::cancel_login();
}

#[tauri::command]
pub async fn claude_account_save_current(app: tauri::AppHandle) -> Result<(), String> {
    let _mutation = MUTATION
        .try_lock()
        .map_err(|_| "A Claude account operation is already in progress.")?;
    let _credentials = accounts::CREDENTIAL_OPERATION.lock().await;
    AccountManager::new()
        .and_then(|m| m.save_current())
        .map_err(|e| e.to_string())?;
    changed(&app);
    Ok(())
}

#[tauri::command]
pub async fn claude_account_remove(app: tauri::AppHandle, id: String) -> Result<(), String> {
    let _mutation = MUTATION
        .try_lock()
        .map_err(|_| "A Claude account operation is already in progress.")?;
    let _credentials = accounts::CREDENTIAL_OPERATION.lock().await;
    AccountManager::new()
        .and_then(|m| m.remove(&id))
        .map_err(|e| e.to_string())?;
    changed(&app);
    Ok(())
}

#[tauri::command]
pub async fn claude_account_switch(app: tauri::AppHandle, id: String) -> Result<(), String> {
    let _mutation = MUTATION
        .try_lock()
        .map_err(|_| "A Claude account operation is already in progress.")?;
    let _credentials = accounts::CREDENTIAL_OPERATION.lock().await;
    tauri::async_runtime::spawn_blocking(move || {
        accounts::require_cli_closed()?;
        AccountManager::new()?.switch(&id)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;
    let pending = {
        let state = app.state::<Mutex<AppState>>();
        let mut state = state.lock().map_err(|e| e.to_string())?;
        invalidate_account_usage(&mut state, ProviderId::Claude)
    };
    crate::events::emit_provider_updated(&app, &pending);
    drop(_credentials);
    changed(&app);
    tauri::async_runtime::spawn(async move {
        let _refresh = super::refresh_providers(app).await;
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switching_invalidates_old_identity_usage_and_inflight_results() {
        let mut state = AppState::new();
        let mut old = invalidate_account_usage(&mut state, ProviderId::Claude);
        old.account_email = Some("old@example.com".into());
        old.plan_name = Some("old-plan".into());
        old.error = None;
        old.primary.used_percent = 80.0;
        state.provider_cache = vec![old];
        state.is_refreshing = true;
        state
            .transient_provider_failure_counts
            .insert(ProviderId::Claude, 1);
        let generation = state.provider_refresh_generation;
        let pending = invalidate_account_usage(&mut state, ProviderId::Claude);
        assert!(pending.account_email.is_none());
        assert!(pending.plan_name.is_none());
        assert!(pending.error.is_some());
        assert_eq!(pending.primary.used_percent, 0.0);
        assert_eq!(state.provider_cache.len(), 1);
        assert!(state.provider_cache[0].error.is_some());
        assert_ne!(state.provider_refresh_generation, generation);
        assert!(!state.is_refreshing);
        assert!(
            !state
                .transient_provider_failure_counts
                .contains_key(&ProviderId::Claude)
        );
    }
}
