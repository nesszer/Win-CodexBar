use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use uuid::Uuid;

use codexbar::codex_accounts::{
    AccountStore, CodexAccount, CodexAccountApi, CodexAccountManager, CodexAccountManagerError,
    CodexAccountRuntime, CodexApiError, CodexSwitchResult, SnapshotStore, display_names_by_id,
    restart_codex_desktop,
};

use crate::state::AppState;

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

    let mut handles = Vec::with_capacity(accounts.len());
    for account in accounts {
        let permits = Arc::clone(&fetch_permits);
        handles.push(tokio::spawn(async move {
            let Ok(_permit) = permits.acquire_owned().await else {
                return None;
            };
            let api = CodexAccountApi::new();
            let home_path = account.codex_home_path.clone();
            let email_hint = account.email_hint.clone();
            let workspace_account_id = account.effective_workspace_account_id();
            match tokio::time::timeout(
                std::time::Duration::from_secs(DEFAULT_FETCH_TIMEOUT_SECONDS),
                api.fetch_snapshot_for_workspace(
                    &home_path,
                    email_hint.as_deref(),
                    workspace_account_id.as_deref(),
                    true,
                ),
            )
            .await
            {
                Ok(Ok(snapshot)) => Some((account, snapshot)),
                Ok(Err(e)) => {
                    tracing::debug!(
                        "codex account lane {} failed: {}",
                        account.id,
                        into_api_message(e)
                    );
                    None
                }
                Err(_) => {
                    tracing::debug!("codex account lane {} timed out", account.id);
                    None
                }
            }
        }));
    }

    let mut updates = Vec::new();
    for handle in handles {
        if let Ok(Some((fetched_account, snapshot))) = handle.await {
            updates.push((fetched_account, snapshot));
        }
    }
    // Hold the generation owner through the read/merge/write so an invalidated
    // batch cannot overwrite a replacement batch's account snapshots.
    let state = app.state::<Mutex<AppState>>();
    let Ok(state) = state.lock() else { return };
    match save_codex_lane_results(&state, generation, updates) {
        Ok(false) => return,
        Err(e) => tracing::warn!("codex account lanes: failed to persist snapshots: {e}"),
        Ok(true) => {}
    }
    events::emit_codex_accounts_updated(&app);
}

fn save_codex_lane_results(
    state: &AppState,
    generation: u64,
    updates: Vec<(CodexAccount, codexbar::codex_accounts::AccountUsageSnapshot)>,
) -> Result<bool, std::io::Error> {
    if !is_current_provider_refresh_generation(state, generation) {
        return Ok(false);
    }
    let current_accounts = load_codex_accounts().map_err(std::io::Error::other)?;
    let current_by_id: HashMap<Uuid, &CodexAccount> = current_accounts
        .iter()
        .map(|account| (account.id, account))
        .collect();
    let mut snapshots = snapshots_for_accounts(&current_accounts, SnapshotStore::new().load()?);
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

    if let Err(e) = refresh_persisted_accounts(app) {
        tracing::error!("failed to persist accounts after add: {e}");
    }
    Ok(account)
}

/// Re-run the official Codex login flow for the ambient account without
/// changing account ownership or copying credentials into a managed home.
#[tauri::command]
pub async fn codex_account_reauthenticate(app: tauri::AppHandle) -> Result<CodexAccount, String> {
    let runtime = CodexAccountRuntime::new();
    let _mutation = runtime.try_begin_mutation().map_err(into_user_message)?;
    let target = ambient_account(&load_codex_accounts()?)?;
    let manager = CodexAccountManager::new();
    let authenticated =
        tauri::async_runtime::spawn_blocking(move || manager.reauthenticate(&target, None))
            .await
            .map_err(|e| e.to_string())?
            .map_err(into_user_message)?;

    // The login flow replaced the ambient auth file. Reconcile the identity
    // before refreshing usage so every surface observes the new session. A
    // login can change the ambient identity, so report the reconciled ambient
    // record instead of the pre-reconciliation login result.
    let account = match refresh_persisted_accounts(app.clone()) {
        Ok(accounts) => canonical_reauthenticated_account(&accounts, authenticated),
        Err(e) => {
            // Credential replacement is already committed; persisting optional
            // account metadata is best-effort here (as in `codex_account_add`),
            // so a successful login is not reported as a command failure. The
            // authenticated identity is the best available record when the
            // store cannot be reconciled.
            tracing::warn!("Codex login succeeded but account metadata could not be saved: {e}");
            authenticated
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

    Ok(account)
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
    let snapshot = tokio::time::timeout(
        std::time::Duration::from_secs(DEFAULT_FETCH_TIMEOUT_SECONDS),
        api.fetch_snapshot_for_workspace(
            &home_path,
            email_hint.as_deref(),
            workspace_account_id.as_deref(),
            true,
        ),
    )
    .await
    .map_err(|_| "Timed out waiting for the Codex usage API.".to_string())?
    .map_err(into_api_message)?;

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

/// The account a reauthentication command should report.
///
/// A login that changes the ambient identity produces a fresh persisted record
/// with a new id, while the login helper reuses the pre-login id. Prefer the
/// reconciled ambient account so the command result agrees with the persisted
/// store and provider-update events. Fall back to the login result when the
/// reconciled set cannot identify an ambient account.
fn canonical_reauthenticated_account(
    accounts: &[CodexAccount],
    authenticated: CodexAccount,
) -> CodexAccount {
    ambient_account(accounts).unwrap_or(authenticated)
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
        CodexApiError::Message(msg) => msg,
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
    pub snapshots: HashMap<Uuid, codexbar::codex_accounts::AccountUsageSnapshot>,
}

#[tauri::command]
pub fn get_codex_accounts_state(
    state: tauri::State<'_, Mutex<AppState>>,
) -> Result<CodexAccountsStateBridge, String> {
    let _guard = state.lock().map_err(|e| e.to_string())?;
    let accounts = load_codex_accounts()?;
    let display_names = display_names_by_id(&accounts);
    let snapshots = snapshots_for_accounts(&accounts, codex_account_snapshots()?);
    Ok(CodexAccountsStateBridge {
        accounts,
        display_names,
        snapshots,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn committed_switch_tolerates_unreadable_account_metadata() {
        use codexbar::codex_accounts::file_locations;
        let root = std::env::temp_dir().join(format!("codex-switch-metadata-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        file_locations::with_app_support_directory(root.clone());
        let account_file = file_locations::accounts_file();
        std::fs::write(&account_file, "invalid account metadata").unwrap();
        // This post-commit operation cannot propagate an error to the switch
        // command and skip the refresh/events that follow it.
        persist_materialized_account(Some(&sample_account()));
        assert_eq!(
            std::fs::read_to_string(&account_file).unwrap(),
            "invalid account metadata"
        );
        file_locations::clear_app_support_directory_override();
        assert!(root.starts_with(std::env::temp_dir()));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn superseded_lanes_cannot_overwrite_newer_snapshots() {
        use codexbar::codex_accounts::{AccountUsageSnapshot, file_locations};
        let root = std::env::temp_dir().join(format!("codex-lane-generation-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        file_locations::with_app_support_directory(root.clone());
        let mut state = AppState::new();
        let old_generation = state.provider_refresh_generation;
        invalidate_account_usage(&mut state, ProviderId::Codex);
        let id = Uuid::new_v4();
        let mut account = sample_account();
        account.id = id;
        account.provider_account_id = Some("new".into());
        persist_codex_accounts(&[account.clone()]).unwrap();
        let snapshot = AccountUsageSnapshot {
            email: Some("new@example.com".into()),
            provider_account_id: Some("new".into()),
            plan: None,
            allowed: None,
            limit_reached: None,
            primary_window: None,
            secondary_window: None,
            credits: None,
            cost: None,
            subscription: None,
            updated_at: codexbar::codex_accounts::utc_now(),
        };
        assert!(
            save_codex_lane_results(
                &state,
                state.provider_refresh_generation,
                vec![(account.clone(), snapshot.clone())]
            )
            .unwrap()
        );
        let before = std::fs::read(file_locations::snapshots_file()).unwrap();
        let stale = AccountUsageSnapshot {
            email: Some("old@example.com".into()),
            ..snapshot
        };
        assert!(!save_codex_lane_results(&state, old_generation, vec![(account, stale)]).unwrap());
        assert_eq!(
            std::fs::read(file_locations::snapshots_file()).unwrap(),
            before
        );
        file_locations::clear_app_support_directory_override();
        assert!(root.starts_with(std::env::temp_dir()));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn codex_switch_supersedes_inflight_usage_and_keeps_other_providers() {
        let mut state = AppState::new();
        let other = invalidate_account_usage(&mut state, ProviderId::Claude);
        let mut old = invalidate_account_usage(&mut state, ProviderId::Codex);
        old.account_email = Some("old@example.com".into());
        old.primary.used_percent = 80.0;
        old.error = None;
        state.provider_cache = vec![other, old];
        state.is_refreshing = true;
        let generation = state.provider_refresh_generation;
        let pending = invalidate_account_usage(&mut state, ProviderId::Codex);
        assert!(!is_current_provider_refresh_generation(&state, generation));
        assert!(!state.is_refreshing);
        assert_eq!(state.provider_cache.len(), 2);
        assert!(
            state
                .provider_cache
                .iter()
                .any(|s| s.provider_id == "claude")
        );
        assert!(pending.account_email.is_none() && pending.error.is_some());
        assert_eq!(pending.primary.used_percent, 0.0);
    }

    #[test]
    fn reconciliation_replaces_changed_managed_identity_without_inheriting_metadata() {
        let mut stale = sample_account();
        stale.nickname = Some("Former account".into());
        let mut fresh = sample_account();
        fresh.provider_account_id = Some("replacement".into());
        fresh.email_hint = Some("replacement@example.com".into());
        let accounts = reconcile_codex_accounts(&[stale.clone()], &[fresh.clone()], None);
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].id, fresh.id);
        assert_ne!(accounts[0].id, stale.id);
        assert_eq!(accounts[0].nickname, None);
        assert_eq!(accounts[0].email_hint, fresh.email_hint);
    }

    #[test]
    fn reconciliation_preserves_metadata_for_unchanged_managed_identity() {
        let mut stored = sample_account();
        stored.nickname = Some("Work".into());
        let fresh = sample_account();
        let accounts = reconcile_codex_accounts(&[stored.clone()], &[fresh], None);
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].id, stored.id);
        assert_eq!(accounts[0].nickname, stored.nickname);
    }

    fn sample_account() -> CodexAccount {
        CodexAccount::new(
            Uuid::new_v4(),
            None,
            Some("user@example.com".to_string()),
            Some("auth0|acct".to_string()),
            Some("acct".to_string()),
            std::path::PathBuf::from("/tmp/fake-home"),
            codexbar::codex_accounts::CodexAccountSource::ManagedByApp,
            codexbar::codex_accounts::utc_now(),
            codexbar::codex_accounts::utc_now(),
            Some(codexbar::codex_accounts::utc_now()),
        )
    }

    #[test]
    fn into_user_message_preserves_friendly_text() {
        assert_eq!(
            into_user_message(CodexAccountManagerError::Message(
                "The `codex` command could not be found.".to_string()
            )),
            "The `codex` command could not be found."
        );
    }

    #[test]
    fn ambient_account_selects_only_the_ambient_identity() {
        let managed = sample_account();
        let mut ambient = managed.clone();
        ambient.source = codexbar::codex_accounts::CodexAccountSource::Ambient;

        let selected = ambient_account(&[managed, ambient.clone()]).unwrap();

        assert_eq!(selected.id, ambient.id);
        assert_eq!(
            selected.source,
            codexbar::codex_accounts::CodexAccountSource::Ambient
        );
    }

    #[test]
    fn ambient_account_reports_when_no_ambient_identity_exists() {
        assert_eq!(
            ambient_account(&[sample_account()]).unwrap_err(),
            "No ambient Codex account found."
        );
    }

    #[test]
    fn reconciled_ambient_identity_change_replaces_the_login_result() {
        let mut stored = sample_account();
        stored.source = codexbar::codex_accounts::CodexAccountSource::Ambient;
        stored.provider_account_id = Some("old-workspace".into());
        stored.email_hint = Some("old@example.com".into());

        // Logging in as a different identity at the same ambient home.
        let mut fresh = sample_account();
        fresh.source = codexbar::codex_accounts::CodexAccountSource::Ambient;
        fresh.provider_account_id = Some("new-workspace".into());
        fresh.email_hint = Some("new@example.com".into());

        let reconciled = reconcile_codex_accounts(&[stored.clone()], &[], Some(fresh));
        assert_eq!(reconciled.len(), 1);
        assert_ne!(reconciled[0].id, stored.id);

        // `reauthenticate` reuses the pre-login id; the command must report the
        // reconciled record so it agrees with the persisted store and events.
        let mut authenticated = stored.clone();
        authenticated.email_hint = Some("new@example.com".into());
        let account = canonical_reauthenticated_account(&reconciled, authenticated);
        assert_eq!(account.id, reconciled[0].id);
        assert_eq!(
            account.source,
            codexbar::codex_accounts::CodexAccountSource::Ambient
        );
        assert_eq!(
            account.provider_account_id.as_deref(),
            Some("new-workspace")
        );
    }

    #[test]
    fn canonical_reauthenticated_account_falls_back_without_a_reconciled_ambient() {
        let authenticated = sample_account();
        let account = canonical_reauthenticated_account(&[], authenticated.clone());
        assert_eq!(account.id, authenticated.id);
    }

    #[test]
    fn sample_account_serializes_camel_case() {
        let json = serde_json::to_value(sample_account()).unwrap();
        assert!(json.get("codexHomePath").is_some());
        assert!(json.get("providerAccountId").is_some());
    }
}
