use super::invalidate_account_usage;
use crate::state::AppState;
use codexbar::core::ProviderId;
use codexbar::providers::claude::accounts::{self, AccountManager, ClaudeAccount};
use codexbar::providers::claude::claude_swap::{
    self, ClaudeSwapAccount, ClaudeSwapAccountAction, ClaudeSwapAccountList, ClaudeSwapAccountRow,
    ClaudeSwapUsageStatus,
};
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

#[derive(Debug, Clone)]
struct ClaudeSwapConfig {
    enabled: bool,
    executable_path: String,
    hide_personal_info: bool,
}

impl ClaudeSwapConfig {
    fn load() -> Self {
        let settings = codexbar::settings::Settings::load();
        Self {
            enabled: settings.claude_swap_enabled(),
            executable_path: settings.claude_swap_executable_path().to_string(),
            hide_personal_info: settings.hide_personal_info,
        }
    }

    fn require_enabled() -> Result<Self, String> {
        let config = Self::load();
        if !config.enabled {
            return Err("claude-swap integration is disabled.".to_string());
        }
        if config.executable_path.trim().is_empty() {
            return Err("No claude-swap executable path is configured.".to_string());
        }
        Ok(config)
    }
}

fn claude_swap_accounts_state() -> ClaudeSwapAccountsState {
    let config = ClaudeSwapConfig::load();
    let enabled = config.enabled;
    let executable_configured = !config.executable_path.trim().is_empty();
    if !enabled || !executable_configured {
        return ClaudeSwapAccountsState {
            enabled,
            executable_configured,
            accounts: Vec::new(),
            error: None,
        };
    }
    match claude_swap::read_account_list(&config.executable_path) {
        Ok(list) => ClaudeSwapAccountsState {
            enabled,
            executable_configured,
            accounts: claude_swap::project_accounts(&list, config.hide_personal_info),
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

fn account_row_for_slot(
    list: &ClaudeSwapAccountList,
    slot: u32,
) -> Result<&ClaudeSwapAccountRow, String> {
    list.accounts
        .iter()
        .find(|account| account.number == slot)
        .ok_or_else(|| "claude-swap did not report that account slot.".to_string())
}

/// Emitted when a Claude account change needs a provider refresh before the
/// settle. Event order is load-bearing for every listener:
///
/// 1. `claude-accounts-reconciling` — listeners show the reconciling phase.
/// 2. the bounded provider refresh runs; superseded batches are detected below.
/// 3. `claude-accounts-reconciled` — the terminal marker; settling must be
///    event-driven, never inferred from a switch promise resolving.
/// 4. `claude-accounts-updated` + tray rebuild — the settled reload.
///
/// Do not reorder these emits. A refresh that never started or was superseded
/// mid-flight keeps the reconciling phase armed instead of settling, so no
/// surface shows "settled" while a refresh is still in flight.
async fn refresh_after_claude_change(app: tauri::AppHandle) -> Result<(), String> {
    let pending = {
        let state = app.state::<Mutex<AppState>>();
        let mut state = state.lock().map_err(|e| e.to_string())?;
        invalidate_account_usage(&mut state, ProviderId::Claude)
    };
    crate::events::emit_provider_updated(&app, &pending);
    let _emit = app.emit("claude-accounts-reconciling", ());
    let refresh_result = super::refresh_providers(app.clone()).await;
    if !refresh_providers_ran(&app) {
        // begin_provider_refresh skipped (another batch owns the refresh) or
        // finish_provider_refresh dropped a superseded generation: a refresh
        // is still in flight, so stay in the reconciling phase and let the
        // owning batch's completion settle listeners.
        return refresh_result;
    }
    let _reconciled = app.emit("claude-accounts-reconciled", ());
    changed(&app);
    refresh_result
}

/// Whether the completed refresh batch owned the generation it published
/// under. A skipped or superseded batch must not settle account listeners.
fn refresh_providers_ran(app: &tauri::AppHandle) -> bool {
    let state = app.state::<Mutex<AppState>>();
    state
        .lock()
        .map(|guard| !guard.is_refreshing)
        .unwrap_or(false)
}

#[derive(Debug, Clone, Copy)]
enum ClaudeSwapAccountOperation {
    Switch,
    Reauthenticate,
}

#[derive(Debug, PartialEq, Eq)]
struct ClaudeSwapMutationOutcome {
    applied: bool,
    error: Option<String>,
}

impl ClaudeSwapMutationOutcome {
    fn confirmed() -> Self {
        Self {
            applied: true,
            error: None,
        }
    }

    fn applied_unconfirmed(error: impl Into<String>) -> Self {
        Self {
            applied: true,
            error: Some(error.into()),
        }
    }
}

fn validate_claude_swap_operation(
    operation: ClaudeSwapAccountOperation,
    account: &ClaudeSwapAccountRow,
) -> Result<(), String> {
    let expected_action = match operation {
        ClaudeSwapAccountOperation::Switch => ClaudeSwapAccountAction::Switch,
        ClaudeSwapAccountOperation::Reauthenticate => ClaudeSwapAccountAction::Reauthenticate,
    };
    if claude_swap::action_for_account(account) == Some(expected_action) {
        return Ok(());
    }
    match operation {
        ClaudeSwapAccountOperation::Switch if account.is_active => {
            Err("That claude-swap account is already active.".to_string())
        }
        ClaudeSwapAccountOperation::Switch => {
            Err("That claude-swap account is not available for switching.".to_string())
        }
        ClaudeSwapAccountOperation::Reauthenticate => Err(
            "That claude-swap account does not currently require re-authentication.".to_string(),
        ),
    }
}

fn reauthentication_is_repaired(account: &ClaudeSwapAccountRow) -> bool {
    account.is_active && account.usage_status == ClaudeSwapUsageStatus::Ok
}

fn run_claude_swap_operation(
    config: &ClaudeSwapConfig,
    slot: u32,
    operation: ClaudeSwapAccountOperation,
) -> Result<ClaudeSwapMutationOutcome, String> {
    let _credentials = accounts::CREDENTIAL_OPERATION.blocking_lock();
    let before = claude_swap::read_account_list(&config.executable_path)
        .map_err(|error| error.to_string())?;
    let account = account_row_for_slot(&before, slot)?;
    validate_claude_swap_operation(operation, account)?;

    let result = claude_swap::switch_account(&config.executable_path, slot)
        .map_err(|error| error.to_string())?;
    if !result.switched {
        return Err(result.reason);
    }
    if matches!(operation, ClaudeSwapAccountOperation::Switch) {
        return Ok(ClaudeSwapMutationOutcome::confirmed());
    }

    let after = match claude_swap::read_account_list(&config.executable_path) {
        Ok(list) => list,
        Err(error) => {
            return Ok(ClaudeSwapMutationOutcome::applied_unconfirmed(format!(
                "claude-swap re-authentication was applied, but confirmation failed: {error}"
            )));
        }
    };
    let account = match account_row_for_slot(&after, slot) {
        Ok(account) => account,
        Err(error) => return Ok(ClaudeSwapMutationOutcome::applied_unconfirmed(error)),
    };
    if reauthentication_is_repaired(account) {
        Ok(ClaudeSwapMutationOutcome::confirmed())
    } else {
        Ok(ClaudeSwapMutationOutcome::applied_unconfirmed(
            "claude-swap re-authentication completed without a confirmed account repair.",
        ))
    }
}

async fn finish_claude_swap_mutation(
    app: tauri::AppHandle,
    outcome: ClaudeSwapMutationOutcome,
) -> Result<(), String> {
    if !outcome.applied {
        return outcome.error.map_or(Ok(()), Err);
    }
    let refresh_error = refresh_after_claude_change(app).await.err();
    match (outcome.error, refresh_error) {
        (None, None) => Ok(()),
        (Some(operation_error), None) => Err(operation_error),
        (None, Some(refresh_error)) => Err(refresh_error),
        (Some(operation_error), Some(refresh_error)) => Err(format!(
            "{operation_error} Refresh also failed: {refresh_error}"
        )),
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
    // MUTATION is held for the whole command body, including
    // `finish_claude_swap_mutation`'s awaited provider refresh below. This is
    // deliberate: it serializes account mutations across the full
    // reconciliation, so other add/save/remove operations fail fast with
    // "already in progress" instead of racing the swap. The refresh is bounded,
    // so the lock window stays finite.
    let _mutation = MUTATION
        .try_lock()
        .map_err(|_| "A Claude account operation is already in progress.")?;
    let config = ClaudeSwapConfig::require_enabled()?;
    let outcome = tauri::async_runtime::spawn_blocking(move || {
        run_claude_swap_operation(&config, slot, ClaudeSwapAccountOperation::Switch)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;
    finish_claude_swap_mutation(app, outcome).await
}

/// Re-authenticate an active slot whose current Claude credential belongs to a
/// different account. The source-owned fixed-slot operation is reused without
/// a force flag, and success is reported only after a fresh list confirms the
/// foreign-credential marker is gone.
#[tauri::command]
pub async fn claude_swap_account_reauthenticate(
    app: tauri::AppHandle,
    slot: u32,
) -> Result<(), String> {
    let _mutation = MUTATION
        .try_lock()
        .map_err(|_| "A Claude account operation is already in progress.")?;
    let config = ClaudeSwapConfig::require_enabled()?;
    let outcome = tauri::async_runtime::spawn_blocking(move || {
        run_claude_swap_operation(&config, slot, ClaudeSwapAccountOperation::Reauthenticate)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;
    finish_claude_swap_mutation(app, outcome).await
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
    crate::auto_resume::clear(&app, ProviderId::Claude);
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
    crate::auto_resume::clear(&app, ProviderId::Claude);
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
    crate::auto_resume::clear(&app, ProviderId::Claude);
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
    drop(_credentials);
    refresh_after_claude_change(app).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account_row(active: bool, status: &str) -> ClaudeSwapAccountRow {
        let raw = serde_json::json!({
            "schemaVersion": 1,
            "activeAccountNumber": if active { serde_json::json!(1) } else { serde_json::Value::Null },
            "accounts": [{
                "number": 1,
                "email": "test@example.com",
                "active": active,
                "usageStatus": status
            }]
        });
        codexbar::providers::claude::claude_swap::parse_account_list(&raw.to_string())
            .expect("fixture should parse")
            .accounts
            .into_iter()
            .next()
            .expect("fixture should contain one account")
    }

    #[test]
    fn operation_validation_uses_the_projected_action_state() {
        let switchable = account_row(false, "ok");
        assert!(
            validate_claude_swap_operation(ClaudeSwapAccountOperation::Switch, &switchable).is_ok()
        );

        let blocked = account_row(false, "no_credentials");
        assert_eq!(
            validate_claude_swap_operation(ClaudeSwapAccountOperation::Switch, &blocked),
            Err("That claude-swap account is not available for switching.".to_string())
        );
    }

    #[test]
    fn reauthentication_confirmation_requires_a_live_ok_status() {
        assert!(reauthentication_is_repaired(&account_row(true, "ok")));
        assert!(!reauthentication_is_repaired(&account_row(
            true,
            "no_credentials"
        )));
        assert!(!reauthentication_is_repaired(&account_row(true, "unknown")));
    }

    #[test]
    fn applied_but_unconfirmed_outcome_remains_refreshable() {
        let outcome = ClaudeSwapMutationOutcome::applied_unconfirmed("confirmation unavailable");
        assert!(outcome.applied);
        assert_eq!(outcome.error.as_deref(), Some("confirmation unavailable"));
        assert_eq!(ClaudeSwapMutationOutcome::confirmed().error, None);
    }

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
