use super::invalidate_account_usage;
use crate::state::AppState;
use codexbar::core::{ProviderError, ProviderId};
use codexbar::providers::grok::GrokProvider;
use codexbar::providers::grok::accounts::{
    self, AccountManager, GrokAccount, GrokAccountUsage, GrokUsageStatus,
};
use std::sync::Mutex;
use tauri::Emitter;
use tauri::Manager;

static MUTATION: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[tauri::command]
pub fn grok_accounts_list() -> Result<Vec<GrokAccount>, String> {
    AccountManager::new()
        .and_then(|m| m.list())
        .map_err(|e| e.to_string())
}

fn changed(app: &tauri::AppHandle) {
    let _emit = app.emit("grok-accounts-updated", ());
    let handle = app.clone();
    let _dispatch = app.run_on_main_thread(move || crate::tray_bridge::rebuild_tray_menu(&handle));
}

fn refresh_after_grok_change(app: tauri::AppHandle) -> Result<(), String> {
    let pending = {
        let state = app.state::<Mutex<AppState>>();
        let mut state = state.lock().map_err(|e| e.to_string())?;
        invalidate_account_usage(&mut state, ProviderId::Grok)
    };
    crate::events::emit_provider_updated(&app, &pending);
    changed(&app);
    tauri::async_runtime::spawn(async move {
        let _refresh = super::refresh_providers(app).await;
    });
    Ok(())
}

#[tauri::command]
pub async fn grok_account_add(app: tauri::AppHandle) -> Result<(), String> {
    let _mutation = MUTATION
        .try_lock()
        .map_err(|_| "A Grok account operation is already in progress.")?;
    let _ = accounts::cleanup_abandoned_logins();
    accounts::begin_login();
    let login = tauri::async_runtime::spawn_blocking(accounts::login)
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    let _credentials = accounts::CREDENTIAL_OPERATION.lock().await;
    AccountManager::new()
        .and_then(|m| m.import(login))
        .map_err(|e| e.to_string())?;
    crate::auto_resume::clear(&app, ProviderId::Grok);
    changed(&app);
    Ok(())
}

#[tauri::command]
pub fn grok_account_cancel_login() {
    accounts::cancel_login();
}

#[tauri::command]
pub async fn grok_account_save_current(app: tauri::AppHandle) -> Result<(), String> {
    let _mutation = MUTATION
        .try_lock()
        .map_err(|_| "A Grok account operation is already in progress.")?;
    let _credentials = accounts::CREDENTIAL_OPERATION.lock().await;
    AccountManager::new()
        .and_then(|m| m.save_current())
        .map_err(|e| e.to_string())?;
    crate::auto_resume::clear(&app, ProviderId::Grok);
    changed(&app);
    Ok(())
}

#[tauri::command]
pub async fn grok_account_remove(app: tauri::AppHandle, id: String) -> Result<(), String> {
    let _mutation = MUTATION
        .try_lock()
        .map_err(|_| "A Grok account operation is already in progress.")?;
    let _credentials = accounts::CREDENTIAL_OPERATION.lock().await;
    AccountManager::new()
        .and_then(|m| m.remove(&id))
        .map_err(|e| e.to_string())?;
    crate::auto_resume::clear(&app, ProviderId::Grok);
    changed(&app);
    Ok(())
}

#[tauri::command]
pub async fn grok_account_fetch(id: String) -> Result<GrokAccountUsage, String> {
    let manager = AccountManager::new().map_err(|error| error.to_string())?;
    let usage = match manager.auth_text_for_usage(&id, false).await {
        Ok(text) => match GrokProvider::new().fetch_usage_from_auth_json(&text).await {
            Err(ProviderError::AuthRequired) => {
                match manager.auth_text_for_usage(&id, true).await {
                    Ok(text) => GrokProvider::new().fetch_usage_from_auth_json(&text).await,
                    Err(error) => Err(error),
                }
            }
            result => result,
        },
        Err(error) => Err(error),
    };
    Ok(match usage {
        Ok(usage) => usage,
        Err(ProviderError::AuthRequired) => {
            GrokAccountUsage::unavailable(GrokUsageStatus::SignInRequired)
        }
        Err(_) => GrokAccountUsage::unavailable(GrokUsageStatus::Failed),
    })
}

#[tauri::command]
pub async fn grok_account_reauthenticate(app: tauri::AppHandle, id: String) -> Result<(), String> {
    let _mutation = MUTATION
        .try_lock()
        .map_err(|_| "A Grok account operation is already in progress.")?;
    let _ = accounts::cleanup_abandoned_logins();
    accounts::begin_login();
    let login = tauri::async_runtime::spawn_blocking(accounts::login)
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    let _credentials = accounts::CREDENTIAL_OPERATION.lock().await;
    let result = AccountManager::new().and_then(|manager| manager.reauthenticate(&id, login));
    drop(_credentials);
    // Even a partial persistence failure can have refreshed the saved login.
    refresh_after_grok_change(app)?;
    result.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn grok_account_switch(app: tauri::AppHandle, id: String) -> Result<(), String> {
    let _mutation = MUTATION
        .try_lock()
        .map_err(|_| "A Grok account operation is already in progress.")?;
    AccountManager::new()
        .map_err(|error| error.to_string())?
        .auth_text_for_usage(&id, false)
        .await
        .map_err(|error| error.to_string())?;
    accounts::select_orca_account(&id)
        .await
        .map_err(|error| error.to_string())?;
    let _credentials = accounts::CREDENTIAL_OPERATION.lock().await;
    tauri::async_runtime::spawn_blocking(move || AccountManager::new()?.switch(&id))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    drop(_credentials);
    refresh_after_grok_change(app)
}
