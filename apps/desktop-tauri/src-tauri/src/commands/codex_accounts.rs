use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use uuid::Uuid;

use codexbar::codex_accounts::{
    AccountStore, CodexAccount, CodexAccountApi, CodexAccountManager, CodexAccountManagerError,
    CodexAccountRuntime, CodexAccountSource, CodexApiError, CodexSwitchResult, SnapshotStore,
    display_names_by_id, ordinals_by_id, restart_codex_desktop,
};
use codexbar::notifications::CredentialAlertPolicy;
use codexbar::settings::Settings;

use crate::state::AppState;

use super::credential_alerts::FetchAttempt;
use super::providers::quota_notification_account_identity_for;
use super::*;

// ── Codex multi-account (ADR 0003, milestone 2) ──────────────────────

const DEFAULT_FETCH_TIMEOUT_SECONDS: u64 = 60;

/// All stored + discovered Codex accounts, with the stored list preferred.
pub(crate) fn load_codex_accounts() -> Result<Vec<CodexAccount>, String> {
    let store = AccountStore::new();
    let existing = store.load_accounts().map_err(|e| e.to_string())?;

    let manager = CodexAccountManager::new();
    let managed = manager
        .discover_managed_accounts(&existing)
        .map_err(|e| e.to_string())?;
    let ambient = manager.discover_ambient_account(&existing);

    Ok(reconcile_codex_accounts(&existing, &managed, ambient))
}

fn reconcile_codex_accounts(
    existing: &[CodexAccount],
    managed: &[CodexAccount],
    ambient: Option<CodexAccount>,
) -> Vec<CodexAccount> {
    let mut merged: Vec<CodexAccount> = managed.to_vec();
    if let Some(ambient) = ambient {
        if let Some(entry) = merged.iter_mut().find(|account| account.matches(&ambient)) {
            entry.merge_from(&ambient);
        } else {
            merged.push(ambient);
        }
    }

    // Reconcile persisted metadata (nickname, stored timestamps) for managed homes.
    let mut reconciled: Vec<CodexAccount> = existing
        .iter()
        // The ambient home can change identity outside this app. Always use
        // its fresh discovery instead of retaining an old record for that path.
        .filter(|account| account.source.owns_files())
        // A managed home can also be reauthenticated outside the app. Do not
        // retain its former identity alongside the account now owning its auth.
        .filter(|account| {
            !managed.iter().any(|fresh| {
                fresh.standardized_home_path() == account.standardized_home_path()
                    && !fresh.matches(account)
            })
        })
        .map(|account| {
            let mut account = account.clone();
            if let Some(fresh) = managed.iter().find(|fresh| fresh.matches(&account)) {
                account.merge_from(fresh);
            }
            account
        })
        .collect();

    // Add any newly discovered accounts that are not yet persisted.
    for candidate in &merged {
        if !reconciled.iter().any(|account| account.matches(candidate)) {
            reconciled.push(candidate.clone());
        }
    }

    reconciled
}

/// Persist the given accounts to the account store.
pub(crate) fn persist_codex_accounts(accounts: &[CodexAccount]) -> Result<(), String> {
    let store = AccountStore::new();
    let (_existing, removed) = store.load().map_err(|e| e.to_string())?;
    store
        .save(accounts, Some(&removed))
        .map_err(|e| e.to_string())
}

/// Refresh quota snapshots for every Codex account (ADR 0003 multi-account
/// lanes).
///
/// Runs on the same refresh cycle as the ambient Codex provider lane: each
/// account (ambient + managed) is fetched concurrently, bounded by the shared
/// provider fetch semaphore, and persisted to the account snapshot store. A
/// `codex-accounts-updated` event lets surfaces (Settings accounts panel)
/// re-read the store without manual fetch.
///
/// Failures are per-account and non-fatal: the ambient provider snapshot and
/// the on-demand `codex_account_fetch` command remain authoritative, and the
/// store keeps the last good snapshot per account.
pub(crate) async fn refresh_codex_account_lanes(
    app: tauri::AppHandle,
    fetch_permits: Arc<tokio::sync::Semaphore>,
    generation: u64,
) {
    let accounts = match load_codex_accounts() {
        Ok(accounts) => accounts,
        Err(e) => {
            tracing::warn!("codex account lanes: failed to load accounts: {e}");
            return;
        }
    };
    if accounts.is_empty() {
        return;
    }

    let mut handles = tokio::task::JoinSet::new();
    for account in accounts {
        let permits = Arc::clone(&fetch_permits);
        handles.spawn(async move {
            let Ok(_permit) = permits.acquire_owned().await else {
                return None;
            };
            let api = CodexAccountApi::new();
            let home_path = account.codex_home_path.clone();
            let email_hint = account.email_hint.clone();
            let workspace_account_id = account.effective_workspace_account_id();
            let result = tokio::time::timeout(
                std::time::Duration::from_secs(DEFAULT_FETCH_TIMEOUT_SECONDS),
                api.fetch_snapshot_for_workspace(
                    &home_path,
                    email_hint.as_deref(),
                    workspace_account_id.as_deref(),
                    true,
                ),
            )
            .await;
            match result {
                Ok(result) => {
                    if let Err(error) = &result {
                        tracing::debug!("codex account lane {} failed: {}", account.id, error);
                    }
                    Some((account, result))
                }
                Err(_) => {
                    tracing::debug!("codex account lane {} timed out", account.id);
                    None
                }
            }
        });
    }

    let mut outcomes = Vec::new();
    while let Some(result) = handles.join_next().await {
        if let Ok(Some(outcome)) = result {
            outcomes.push(outcome);
        }
    }

    let current_accounts = match load_codex_accounts() {
        Ok(accounts) => accounts,
        Err(e) => {
            tracing::warn!("codex account lanes: failed to reload accounts: {e}");
            return;
        }
    };
    // Hold the generation owner through the read/merge/write so an invalidated
    // batch cannot overwrite a replacement batch's account snapshots.
    let state = app.state::<Mutex<AppState>>();
    let Ok(mut state) = state.lock() else { return };
    if !is_current_provider_refresh_generation(&state, generation) {
        return;
    }
    let current_by_id: HashMap<Uuid, &CodexAccount> = current_accounts
        .iter()
        .map(|account| (account.id, account))
        .collect();
    let mut updates = Vec::new();
    let mut authentication = Vec::new();
    for (fetched_account, result) in outcomes {
        let Some(current) = current_by_id.get(&fetched_account.id).copied() else {
            continue;
        };
        if !account_lane_is_current(&fetched_account, current) {
            continue;
        }
        match result {
            Ok(snapshot) if account_snapshot_belongs_to(current, &snapshot) => {
                observe_codex_account_lane_outcome(
                    &mut state.notification_manager,
                    &fetched_account,
                    FetchAttempt::Succeeded,
                );
                authentication.push((fetched_account.clone(), false));
                updates.push((fetched_account, snapshot));
            }
            Ok(_) => {}
            Err(error) => {
                authentication.push((fetched_account.clone(), error.state_kind().needs_sign_in()));
                observe_codex_account_lane_outcome(
                    &mut state.notification_manager,
                    &fetched_account,
                    FetchAttempt::Failed(error.state_kind()),
                );
            }
        }
    }
    match save_codex_lane_results(&state, generation, updates, &current_accounts) {
        Ok(false) => return,
        Err(e) => tracing::warn!("codex account lanes: failed to persist snapshots: {e}"),
        Ok(true) => {}
    }
    publish_codex_authentication(&mut state, generation, &current_accounts, authentication);
    events::emit_codex_accounts_updated(&app);
}

fn publish_codex_authentication(
    state: &mut AppState,
    generation: u64,
    live_accounts: &[CodexAccount],
    results: Vec<(CodexAccount, bool)>,
) {
    if !is_current_provider_refresh_generation(state, generation) {
        return;
    }
    state
        .codex_account_needs_authentication
        .retain(|id, _| live_accounts.iter().any(|account| account.id == *id));
    for (fetched, needs_authentication) in results {
        if live_accounts
            .iter()
            .any(|live| live.id == fetched.id && account_lane_is_current(&fetched, live))
        {
            state
                .codex_account_needs_authentication
                .insert(fetched.id, needs_authentication);
        }
    }
}

fn observe_codex_account_lane_outcome(
    manager: &mut codexbar::notifications::NotificationManager,
    account: &CodexAccount,
    attempt: FetchAttempt,
) {
    if account.source != CodexAccountSource::ManagedByApp {
        return;
    }

    let account_scope = quota_notification_account_identity_for(
        ProviderId::Codex,
        "",
        account.email_hint.as_deref(),
        None,
        Some(account.id),
    );
    match attempt {
        FetchAttempt::Succeeded => {
            manager.observe_credential_recovery(ProviderId::Codex, &account_scope);
        }
        FetchAttempt::Failed(kind) if kind.needs_sign_in() => {
            let policy = CredentialAlertPolicy::from_settings(&Settings::load());
            manager.observe_credential_failure(policy, ProviderId::Codex, &account_scope, kind);
        }
        FetchAttempt::Failed(_) => {}
    }
}

fn save_codex_lane_results(
    state: &AppState,
    generation: u64,
    updates: Vec<(CodexAccount, codexbar::codex_accounts::AccountUsageSnapshot)>,
    current_accounts: &[CodexAccount],
) -> Result<bool, std::io::Error> {
    if !is_current_provider_refresh_generation(state, generation) {
        return Ok(false);
    }
    let current_by_id: HashMap<Uuid, &CodexAccount> = current_accounts
        .iter()
        .map(|account| (account.id, account))
        .collect();
    let mut snapshots = snapshots_for_accounts(current_accounts, SnapshotStore::new().load()?);
    for (fetched_account, snapshot) in updates {
        if current_by_id
            .get(&fetched_account.id)
            .is_some_and(|current| {
                account_lane_is_current(&fetched_account, current)
                    && account_snapshot_belongs_to(current, &snapshot)
            })
        {
            snapshots.insert(fetched_account.id, snapshot);
        }
    }
    SnapshotStore::new().save(&snapshots)?;
    Ok(true)
}

#[tauri::command]
pub fn codex_accounts_list() -> Result<Vec<CodexAccount>, String> {
    load_codex_accounts()
}

#[tauri::command]
pub async fn codex_account_add(app: tauri::AppHandle) -> Result<CodexAccount, String> {
    let runtime = CodexAccountRuntime::new();
    let _mutation = runtime.try_begin_mutation().map_err(into_user_message)?;
    let manager = CodexAccountManager::new();
    let account = tauri::async_runtime::spawn_blocking(move || manager.add_managed_account(None))
        .await
        .map_err(|e| e.to_string())?
        .map_err(into_user_message)?;

    crate::auto_resume::clear(&app, ProviderId::Codex);
    if let Err(e) = refresh_persisted_accounts(app) {
        tracing::error!("failed to persist accounts after add: {e}");
    }
    Ok(account)
}

/// Re-run the official Codex login flow in the selected account's existing
/// home. Omitting the id retains the ambient-account behavior.
#[tauri::command]
pub async fn codex_account_reauthenticate(
    app: tauri::AppHandle,
    id: Option<String>,
) -> Result<CodexAccount, String> {
    let runtime = CodexAccountRuntime::new();
    let _mutation = runtime.try_begin_mutation().map_err(into_user_message)?;
    let target = reauthentication_target(&load_codex_accounts()?, id.as_deref())?;
    let manager = CodexAccountManager::new();
    let authenticated =
        tauri::async_runtime::spawn_blocking(move || manager.reauthenticate(&target, None))
            .await
            .map_err(|e| e.to_string())?
            .map_err(into_user_message)?;

    // The login flow replaced the selected home's auth file. Reconcile the identity
    // before refreshing usage so every surface observes the new session. The
    // logged-in record is transient: reconciliation can drop or replace the
    // ambient identity, so report only a record that was actually persisted.
    let account = match refresh_persisted_accounts(app.clone()) {
        Ok(accounts) => canonical_reauthenticated_account(&accounts, &authenticated),
        Err(e) => {
            // Credential replacement is already committed, but the reconciled
            // account set could not be saved. Reporting the transient login
            // result would expose an account the store never committed, so the
            // persistence error is surfaced instead.
            tracing::warn!("Codex login succeeded but account metadata could not be saved: {e}");
            Err(e)
        }
    };
    let pending = {
        let state = app.state::<Mutex<AppState>>();
        let mut state = state.lock().map_err(|e| e.to_string())?;
        invalidate_account_usage(&mut state, ProviderId::Codex)
    };
    events::emit_provider_updated(&app, &pending);

    let refresh_app = app.clone();
    tauri::async_runtime::spawn(async move {
        let _ = do_refresh_providers(&refresh_app).await;
    });

    account
}

#[tauri::command]
pub fn codex_account_remove(app: tauri::AppHandle, id: String) -> Result<(), String> {
    let runtime = CodexAccountRuntime::new();
    let _mutation = runtime.try_begin_mutation().map_err(into_user_message)?;
    let manager = CodexAccountManager::new();
    let accounts = load_codex_accounts()?;
    let target = accounts
        .iter()
        .find(|account| account.id.to_string() == id)
        .ok_or_else(|| "Codex account not found.".to_string())?;

    manager
        .remove_managed_files_if_owned(target)
        .map_err(into_user_message)?;
    runtime
        .invalidate_restart_for_removed_account(target)
        .map_err(into_user_message)?;

    let remaining: Vec<CodexAccount> = accounts
        .into_iter()
        .filter(|account| account.id.to_string() != id)
        .collect();
    persist_codex_accounts(&remaining)?;
    crate::auto_resume::clear(&app, ProviderId::Codex);
    events::emit_settings_changed(&app);
    accounts_changed(&app);
    Ok(())
}

#[tauri::command]
pub async fn codex_account_switch(
    app: tauri::AppHandle,
    id: String,
) -> Result<CodexSwitchResult, String> {
    let runtime = CodexAccountRuntime::new();
    let _mutation = runtime.try_begin_mutation().map_err(into_user_message)?;
    let manager = CodexAccountManager::new();
    let accounts = load_codex_accounts()?;
    let target = accounts
        .iter()
        .find(|account| account.id.to_string() == id)
        .ok_or_else(|| "Codex account not found.".to_string())?
        .clone();

    let persisted = accounts.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        manager.switch_active_account(&target, &persisted)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(into_user_message)?;

    // Materialized ambient account may need persisting.
    runtime
        .remember_restart(&result)
        .map_err(into_user_message)?;
    let pending = {
        let state = app.state::<Mutex<AppState>>();
        let mut state = state.lock().map_err(|e| e.to_string())?;
        invalidate_account_usage(&mut state, ProviderId::Codex)
    };
    events::emit_provider_updated(&app, &pending);
    persist_materialized_account(result.materialized_account.as_ref());

    // Discovery must see the persisted account ID before a lane fetch starts.
    // Credential replacement is already committed. Even if optional account
    // metadata cannot be saved, refresh and notify all surfaces of that switch.
    let refresh_app = app.clone();
    tauri::async_runtime::spawn(async move {
        let _ = do_refresh_providers(&refresh_app).await;
    });

    events::emit_settings_changed(&app);
    accounts_changed(&app);
    Ok(result)
}

fn persist_materialized_account(materialized: Option<&CodexAccount>) {
    let Some(materialized) = materialized else {
        return;
    };
    let persist = || -> Result<(), String> {
        let mut accounts = load_codex_accounts()?;
        if let Some(entry) = accounts.iter_mut().find(|a| a.matches(materialized)) {
            entry.merge_from(materialized);
        } else {
            accounts.push(materialized.clone());
        }
        persist_codex_accounts(&accounts)
    };
    if let Err(error) = persist() {
        tracing::warn!("Codex account switched but account metadata could not be saved: {error}");
    }
}

#[tauri::command]
pub async fn codex_account_fetch(
    app: tauri::AppHandle,
    id: String,
) -> Result<codexbar::codex_accounts::AccountUsageSnapshot, String> {
    let accounts = load_codex_accounts()?;
    let target = accounts
        .iter()
        .find(|account| account.id.to_string() == id)
        .ok_or_else(|| "Codex account not found.".to_string())?
        .clone();

    let api = CodexAccountApi::new();
    let home_path = target.codex_home_path.clone();
    let email_hint = target.email_hint.clone();
    let workspace_account_id = target.effective_workspace_account_id();
    let state = app.state::<Mutex<AppState>>();
    let generation = state
        .lock()
        .map_err(|e| e.to_string())?
        .provider_refresh_generation;
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(DEFAULT_FETCH_TIMEOUT_SECONDS),
        api.fetch_snapshot_for_workspace(
            &home_path,
            email_hint.as_deref(),
            workspace_account_id.as_deref(),
            true,
        ),
    )
    .await
    .map_err(|_| "Timed out waiting for the Codex usage API.".to_string())?;
    if let Ok(live_accounts) = load_codex_accounts()
        && let Ok(mut state) = state.lock()
    {
        publish_codex_authentication(
            &mut state,
            generation,
            &live_accounts,
            vec![(
                target.clone(),
                result
                    .as_ref()
                    .err()
                    .is_some_and(|error| error.state_kind().needs_sign_in()),
            )],
        );
    }
    events::emit_codex_accounts_updated(&app);
    let snapshot = result.map_err(into_api_message)?;

    // Persist snapshot to the snapshot store, keyed by account id.
    if let Ok(mut snapshots) = SnapshotStore::new().load()
        && load_codex_accounts().ok().is_some_and(|accounts| {
            accounts.iter().any(|account| {
                account.id == target.id
                    && account_lane_is_current(&target, account)
                    && account_snapshot_belongs_to(account, &snapshot)
            })
        })
    {
        snapshots.insert(target.id, snapshot.clone());
        let _ = SnapshotStore::new().save(&snapshots);
    }

    if refresh_persisted_accounts(app).is_err() {
        // Non-fatal: the snapshot was still fetched.
    }
    Ok(snapshot)
}

#[tauri::command]
pub fn codex_account_snapshots()
-> Result<HashMap<Uuid, codexbar::codex_accounts::AccountUsageSnapshot>, String> {
    SnapshotStore::new().load().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn codex_account_restart_desktop(
    _app: tauri::AppHandle,
    switch_id: String,
) -> Result<(), String> {
    let runtime = CodexAccountRuntime::new();
    let _mutation = runtime.try_begin_mutation().map_err(into_user_message)?;
    let active = CodexAccountManager::new().discover_ambient_account(&[]);
    let pending = runtime
        .pending_restart_for(&switch_id, active.as_ref())
        .map_err(into_user_message)?;
    tauri::async_runtime::spawn_blocking(move || {
        restart_codex_desktop(
            0.8,
            None,
            pending.desktop_session_backup_path.as_deref(),
            pending.desktop_session_restore_path.as_deref(),
        )
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;
    // The outgoing session backup is single-use. Replaying it after relaunch
    // would overwrite that backup with the newly active account's session.
    runtime.clear_pending_restart().map_err(into_user_message)?;
    Ok(())
}

/// Merge discovered accounts back into the persisted list after identity
/// changes (login/switch) so the store reflects reality.
///
/// Returns the post-reconciliation accounts, which is exactly the set that was
/// persisted, so callers can report a canonical account from persisted state.
fn refresh_persisted_accounts(app: tauri::AppHandle) -> Result<Vec<CodexAccount>, String> {
    let accounts = load_codex_accounts()?;
    persist_codex_accounts(&accounts)?;
    events::emit_settings_changed(&app);
    accounts_changed(&app);
    Ok(accounts)
}

/// Select the ambient identity from a persisted account set.
fn ambient_account(accounts: &[CodexAccount]) -> Result<CodexAccount, String> {
    accounts
        .iter()
        .find(|account| account.source == codexbar::codex_accounts::CodexAccountSource::Ambient)
        .cloned()
        .ok_or_else(|| "No ambient Codex account found.".to_string())
}

fn reauthentication_target(
    accounts: &[CodexAccount],
    id: Option<&str>,
) -> Result<CodexAccount, String> {
    match id {
        None => ambient_account(accounts),
        Some(id) => accounts
            .iter()
            .find(|account| account.id.to_string() == id)
            .cloned()
            .ok_or_else(|| "Codex account not found.".to_string()),
    }
}

/// The account a reauthentication command should report.
///
/// The persisted reconciled set is authoritative. A login that changes the
/// ambient identity produces a fresh persisted record with a new id, while the
/// login helper reuses the pre-login id, so the transient login result is used
/// only to locate its persisted counterpart. The persisted ambient record is
/// authoritative for this command; identity matching is a fallback for legacy
/// stores that contain no ambient record.
/// When neither is present the login was never committed, so the command fails
/// instead of exposing a dropped or replaced transient account.
/// Managed logins resolve within their own home, preferring the stable account
/// id when several saved workspace accounts share that home.
fn canonical_reauthenticated_account(
    accounts: &[CodexAccount],
    authenticated: &CodexAccount,
) -> Result<CodexAccount, String> {
    if authenticated.source.owns_files() {
        let same_home = |account: &&CodexAccount| {
            account.source.owns_files()
                && account.standardized_home_path() == authenticated.standardized_home_path()
        };
        return accounts
            .iter()
            .filter(same_home)
            .find(|account| account.id == authenticated.id)
            .or_else(|| accounts.iter().find(same_home))
            .cloned()
            .ok_or_else(|| "Codex account login was not persisted.".to_string());
    }
    if let Some(account) = accounts
        .iter()
        .find(|account| account.source == codexbar::codex_accounts::CodexAccountSource::Ambient)
    {
        return Ok(account.clone());
    }
    if let Some(account) = accounts
        .iter()
        .find(|account| account.matches(authenticated))
    {
        return Ok(account.clone());
    }
    ambient_account(accounts)
}

fn accounts_changed(app: &tauri::AppHandle) {
    events::emit_codex_accounts_updated(app);
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || crate::tray_bridge::rebuild_tray_menu(&handle));
}

fn into_user_message(error: CodexAccountManagerError) -> String {
    match error {
        CodexAccountManagerError::Message(msg) => msg,
        CodexAccountManagerError::Io(e) => e.to_string(),
    }
}

fn into_api_message(error: CodexApiError) -> String {
    match error {
        CodexApiError::AuthenticationRequired(msg)
        | CodexApiError::SessionExpired(msg)
        | CodexApiError::PermissionDenied(msg)
        | CodexApiError::Message(msg) => msg,
        CodexApiError::Network(e) => format!("network error: {e}"),
        CodexApiError::Parse(e) => format!("failed to parse Codex payload: {e}"),
    }
}

fn account_home_key(account: &CodexAccount) -> String {
    std::path::absolute(&account.codex_home_path)
        .unwrap_or_else(|_| account.codex_home_path.clone())
        .to_string_lossy()
        .to_lowercase()
}

/// In-flight results are only authoritative for the selected workspace and
/// managed home that started the request.
fn account_lane_is_current(started: &CodexAccount, current: &CodexAccount) -> bool {
    started.id == current.id
        && account_home_key(started) == account_home_key(current)
        && started.effective_workspace_account_id() == current.effective_workspace_account_id()
}

fn account_snapshot_belongs_to(
    account: &CodexAccount,
    snapshot: &codexbar::codex_accounts::AccountUsageSnapshot,
) -> bool {
    let snapshot_workspace = snapshot
        .provider_account_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_lowercase);
    match (account.effective_workspace_account_id(), snapshot_workspace) {
        (Some(account_workspace), Some(snapshot_workspace)) => {
            account_workspace == snapshot_workspace
        }
        (None, None) => true,
        _ => false,
    }
}

fn snapshots_for_accounts(
    accounts: &[CodexAccount],
    snapshots: HashMap<Uuid, codexbar::codex_accounts::AccountUsageSnapshot>,
) -> HashMap<Uuid, codexbar::codex_accounts::AccountUsageSnapshot> {
    let accounts_by_id: HashMap<Uuid, &CodexAccount> = accounts
        .iter()
        .map(|account| (account.id, account))
        .collect();
    snapshots
        .into_iter()
        .filter(|(id, snapshot)| {
            accounts_by_id
                .get(id)
                .is_some_and(|account| account_snapshot_belongs_to(account, snapshot))
        })
        .collect()
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexAccountsStateBridge {
    pub accounts: Vec<CodexAccount>,
    pub display_names: HashMap<Uuid, String>,
    pub account_ordinals: HashMap<Uuid, usize>,
    pub snapshots: HashMap<Uuid, codexbar::codex_accounts::AccountUsageSnapshot>,
    pub needs_authentication: HashMap<Uuid, bool>,
}

#[tauri::command]
pub fn get_codex_accounts_state(
    state: tauri::State<'_, Mutex<AppState>>,
) -> Result<CodexAccountsStateBridge, String> {
    let guard = state.lock().map_err(|e| e.to_string())?;
    let accounts = load_codex_accounts()?;
    let display_names = display_names_by_id(&accounts);
    let account_ordinals = ordinals_by_id(&accounts);
    let snapshots = snapshots_for_accounts(&accounts, codex_account_snapshots()?);
    let needs_authentication = guard
        .codex_account_needs_authentication
        .iter()
        .filter(|(id, _)| accounts.iter().any(|account| account.id == **id))
        .map(|(id, needs)| (*id, *needs))
        .collect();
    Ok(CodexAccountsStateBridge {
        accounts,
        display_names,
        account_ordinals,
        snapshots,
        needs_authentication,
    })
}

#[cfg(test)]
#[path = "codex_accounts/tests.rs"]
mod tests;
