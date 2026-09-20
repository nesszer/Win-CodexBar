//! Codex account management: discovery, authentication, switching, removal.
//!
//! Port of `windows/.../account_manager.py` (MIT). Manages isolated managed
//! homes under `managed-homes/`, discovers the ambient `~/.codex` identity, and
//! switches the active identity by swapping `auth.json` into the ambient home,
//! rewriting the Codex Desktop `creator_id` global state and backing up/restoring
//! the desktop session.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::{DateTime, Utc};
use thiserror::Error;
use uuid::Uuid;

use super::api::CodexApiError;
use super::credentials::{AuthBackedIdentity, load_identity, parse_credentials_json};
use super::file_locations::{
    ambient_codex_home, auth_backups_directory, codex_desktop_session_root,
    desktop_session_snapshot_path, ensure_directories, managed_homes_directory,
};
use super::login_runner::{CodexLoginOutcome, CodexLoginRunner, ManagedLoginProcess};
use super::models::{CodexAccount, CodexAccountSource, utc_now};

/// Friendly account manager error.
#[derive(Debug, Error)]
pub enum CodexAccountManagerError {
    #[error("{0}")]
    Message(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl From<CodexApiError> for CodexAccountManagerError {
    fn from(value: CodexApiError) -> Self {
        CodexAccountManagerError::Message(value.to_string())
    }
}

/// Result of switching the active account.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexSwitchResult {
    pub switch_id: Uuid,
    pub materialized_account: Option<CodexAccount>,
    pub backup_path: Option<PathBuf>,
    pub ambient_account: Option<CodexAccount>,
    pub desktop_session_backup_path: Option<PathBuf>,
    pub desktop_session_restore_path: Option<PathBuf>,
    pub desktop_session_restore_exists: bool,
}

/// Discovers, authenticates and switches Codex accounts.
#[derive(Debug, Default)]
pub struct CodexAccountManager;

impl CodexAccountManager {
    pub fn new() -> Self {
        Self
    }

    /// Start a `codex login` into a fresh managed home.
    pub fn add_managed_account(
        &self,
        handle: Option<&ManagedLoginProcess>,
    ) -> Result<CodexAccount, CodexAccountManagerError> {
        ensure_directories()?;
        let home_path = managed_homes_directory().join(Uuid::new_v4().to_string());
        fs::create_dir_all(&home_path)?;

        match self.authenticate_account(&home_path, CodexAccountSource::ManagedByApp, None, handle)
        {
            Ok(account) => Ok(account),
            Err(error) => {
                // Best-effort teardown of the fresh managed home on failure;
                // a removal error cannot change the authentication outcome.
                let _removed_home = fs::remove_dir_all(&home_path);
                Err(error)
            }
        }
    }

    /// Re-run `codex login` for an existing account.
    pub fn reauthenticate(
        &self,
        account: &CodexAccount,
        handle: Option<&ManagedLoginProcess>,
    ) -> Result<CodexAccount, CodexAccountManagerError> {
        // Keep credential replacement exclusive with provider reads and
        // refreshes, just like an account switch.
        let _credentials = super::CREDENTIAL_OPERATIONS.blocking_write();
        self.authenticate_account(
            &account.codex_home_path,
            account.source,
            Some(account),
            handle,
        )
    }

    /// Remove app-owned managed homes matching this account.
    pub fn remove_managed_files_if_owned(
        &self,
        account: &CodexAccount,
    ) -> Result<(), CodexAccountManagerError> {
        if !account.source.owns_files() {
            return Ok(());
        }

        let root = fs::canonicalize(managed_homes_directory())
            .unwrap_or_else(|_| managed_homes_directory());
        let targets = self.managed_home_paths_matching(account)?;

        for target in targets {
            let resolved = fs::canonicalize(&target).unwrap_or_else(|_| target.clone());
            let relative = resolved.strip_prefix(&root).map_err(|_| {
                CodexAccountManagerError::Message(
                    "This path is not an app-managed home directory.".to_string(),
                )
            })?;
            if relative.as_os_str().is_empty() {
                return Err(CodexAccountManagerError::Message(
                    "Refusing to remove the managed-homes root.".to_string(),
                ));
            }
            if target.exists() {
                fs::remove_dir_all(&target)?;
            }
        }
        Ok(())
    }

    /// Discover managed homes and merge them against the stored accounts.
    pub fn discover_managed_accounts(
        &self,
        existing: &[CodexAccount],
    ) -> Result<Vec<CodexAccount>, CodexAccountManagerError> {
        ensure_directories()?;
        let mut discovered = Vec::new();
        let mut entries: Vec<PathBuf> = fs::read_dir(managed_homes_directory())?
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| path.is_dir())
            .collect();
        entries.sort_by_key(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().to_lowercase())
                .unwrap_or_default()
        });
        for home_path in entries {
            if let Some(account) = self.discovered_managed_account(&home_path, existing) {
                discovered.push(account);
            }
        }
        Ok(discovered)
    }

    /// Discover the ambient `~/.codex` account.
    pub fn discover_ambient_account(&self, existing: &[CodexAccount]) -> Option<CodexAccount> {
        let home_path = ambient_codex_home();
        let auth_path = home_path.join("auth.json");
        if !home_path.is_dir() || !auth_path.exists() {
            return None;
        }
        let identity = load_identity(&home_path).ok()?;
        if identity.email.is_none() && identity.provider_account_id.is_none() {
            return None;
        }

        let candidate =
            candidate_account(identity.clone(), &home_path, CodexAccountSource::Ambient);
        let matched = existing.iter().find(|account| candidate.matches(account));
        let discovered_at = directory_timestamp(&home_path);
        Some(build_discovered_account(
            matched,
            identity,
            home_path,
            CodexAccountSource::Ambient,
            discovered_at,
        ))
    }

    /// Identity of the currently active (ambient) account, if any.
    pub fn load_active_identity(&self) -> Option<AuthBackedIdentity> {
        let auth_path = ambient_codex_home().join("auth.json");
        if !auth_path.exists() {
            return None;
        }
        load_identity(&ambient_codex_home()).ok()
    }

    /// Switch the ambient identity to `target`, materializing the previous
    /// ambient account as managed and preserving the desktop session.
    pub fn switch_active_account(
        &self,
        target: &CodexAccount,
        existing: &[CodexAccount],
    ) -> Result<CodexSwitchResult, CodexAccountManagerError> {
        // Called by the shell's blocking worker: keep the guard here so
        // cancelling its async caller cannot release it before the copy ends.
        let _credentials = super::CREDENTIAL_OPERATIONS.blocking_write();
        ensure_directories()?;

        let target_auth_path = target.codex_home_path.join("auth.json");
        if !target_auth_path.exists() {
            return Err(CodexAccountManagerError::Message(
                "The selected account does not contain `auth.json`.".to_string(),
            ));
        }

        let ambient_account = self.discover_ambient_account(existing);
        if ambient_account
            .as_ref()
            .is_some_and(|ambient| ambient.matches(target))
        {
            // A no-op must not replace auth or restore/clear the live session.
            return Ok(CodexSwitchResult {
                switch_id: Uuid::new_v4(),
                materialized_account: None,
                backup_path: None,
                ambient_account,
                desktop_session_backup_path: None,
                desktop_session_restore_path: None,
                desktop_session_restore_exists: false,
            });
        }
        let session_root = codex_desktop_session_root();
        let mut materialized_account: Option<CodexAccount> = None;
        if let Some(ambient) = &ambient_account {
            let is_ambient = ambient.source == CodexAccountSource::Ambient;
            if is_ambient && !ambient.matches(target) {
                materialized_account = Some(self.materialize_as_managed(ambient)?);
            }
        }

        let mut desktop_session_backup_path: Option<PathBuf> = None;
        let mut desktop_session_restore_path: Option<PathBuf> = None;
        let mut desktop_session_restore_exists = false;
        if session_root.is_some() {
            if let Some(materialized) = &materialized_account {
                desktop_session_backup_path =
                    Some(desktop_session_snapshot_path(&materialized.codex_home_path));
            }
            let snapshot_path = desktop_session_snapshot_path(&target.codex_home_path);
            desktop_session_restore_path = Some(snapshot_path.clone());
            desktop_session_restore_exists = path_has_children(&snapshot_path);
        }

        fs::create_dir_all(ambient_codex_home())?;
        let backup_path = self.backup_ambient_auth()?;
        fs::copy(&target_auth_path, ambient_codex_home().join("auth.json"))?;
        self.sync_ambient_global_state(
            ambient_account
                .as_ref()
                .and_then(CodexAccount::effective_workspace_account_id),
            self.target_account_id(target)?,
        );

        Ok(CodexSwitchResult {
            switch_id: Uuid::new_v4(),
            materialized_account,
            backup_path,
            ambient_account: self.discover_ambient_account(existing),
            desktop_session_backup_path,
            desktop_session_restore_path,
            desktop_session_restore_exists,
        })
    }

    /// Copy the ambient account into an app-managed home.
    ///
    /// Reuses the managed home already holding this account's credentials when
    /// one exists. Minting a fresh home on every call let repeated switches
    /// accumulate one duplicate directory per switch, each holding a copy of
    /// whatever `auth.json` happened to be ambient at the time.
    pub fn materialize_as_managed(
        &self,
        account: &CodexAccount,
    ) -> Result<CodexAccount, CodexAccountManagerError> {
        ensure_directories()?;

        let source_auth_path = account.codex_home_path.join("auth.json");
        if !source_auth_path.exists() {
            return Err(CodexAccountManagerError::Message(
                "The current active account does not contain `auth.json`.".to_string(),
            ));
        }

        let destination_home = match self.existing_managed_home_matching(account) {
            Some(existing) => {
                let existing_auth_path = existing.join("auth.json");
                // Never let a stale ambient file overwrite newer managed
                // credentials; that turns a working account into a dead one.
                if credentials_are_at_least_as_fresh(&source_auth_path, &existing_auth_path) {
                    fs::copy(&source_auth_path, &existing_auth_path)?;
                }
                existing
            }
            None => {
                let fresh_home = managed_homes_directory().join(Uuid::new_v4().to_string());
                fs::create_dir_all(&fresh_home)?;
                fs::copy(&source_auth_path, fresh_home.join("auth.json"))?;
                fresh_home
            }
        };

        let now = utc_now();
        let mut materialized = CodexAccount::new(
            account.id,
            account.nickname.clone(),
            account.email_hint.clone(),
            account.auth_subject.clone(),
            account.provider_account_id.clone(),
            destination_home,
            CodexAccountSource::ManagedByApp,
            account.created_at,
            now,
            Some(account.last_authenticated_at.unwrap_or(now)),
        );
        materialized.workspace_account_id = account.workspace_account_id.clone();
        Ok(materialized)
    }

    fn backup_ambient_auth(&self) -> Result<Option<PathBuf>, CodexAccountManagerError> {
        ensure_directories()?;
        let auth_path = ambient_codex_home().join("auth.json");
        if !auth_path.exists() {
            return Ok(None);
        }
        let backup_path =
            auth_backups_directory().join(format!("ambient-auth-{}.json", timestamp_slug()));
        fs::copy(&auth_path, &backup_path)?;
        Ok(Some(backup_path))
    }

    fn target_account_id(&self, target: &CodexAccount) -> Result<Option<String>, CodexApiError> {
        if let Some(account_id) = target.effective_workspace_account_id() {
            return Ok(Some(account_id));
        }
        let identity = load_identity(&target.codex_home_path)?;
        Ok(identity.provider_account_id)
    }

    fn sync_ambient_global_state(
        &self,
        previous_account_id: Option<String>,
        target_account_id: Option<String>,
    ) {
        let Some(target_account_id) = target_account_id else {
            return;
        };
        for file_name in [".codex-global-state.json", ".codex-global-state.json.bak"] {
            self.rewrite_creator_id(
                &ambient_codex_home().join(file_name),
                previous_account_id.as_deref(),
                &target_account_id,
            );
        }
    }

    fn rewrite_creator_id(
        &self,
        path: &Path,
        previous_account_id: Option<&str>,
        target_account_id: &str,
    ) {
        if !path.exists() {
            return;
        }
        let Ok(content) = fs::read_to_string(path) else {
            return;
        };
        let Ok(mut payload) = serde_json::from_str::<serde_json::Value>(&content) else {
            return;
        };
        if !payload.is_object() {
            return;
        }
        let Some(atom_state) = payload
            .get_mut("electron-persisted-atom-state")
            .and_then(|value| value.as_object_mut())
        else {
            return;
        };
        let Some(environment) = atom_state
            .get_mut("environment")
            .and_then(|value| value.as_object_mut())
        else {
            return;
        };
        let Some(creator_id) = environment.get("creator_id") else {
            return;
        };
        let Some(updated) =
            updated_creator_id(creator_id.as_str(), previous_account_id, target_account_id)
        else {
            return;
        };
        if updated == creator_id.as_str().unwrap_or_default() {
            return;
        }
        environment.insert("creator_id".to_string(), serde_json::Value::String(updated));
        let Ok(encoded) = serde_json::to_string_pretty(&payload) else {
            return;
        };
        // Best-effort teardown: persisting the rewritten payload is advisory
        // and a write error cannot change the in-memory rewrite result.
        let _written_payload = fs::write(path, format!("{encoded}\n"));
    }

    /// Find app-managed homes holding credentials for `account`.
    ///
    /// Results are sorted by `managed_home_key` and deduplicated, so repeated
    /// calls return the same order and the same first element regardless of
    /// filesystem iteration order. The first entry is the canonical reuse
    /// target for `materialize_as_managed`; all entries are removal targets
    /// for `remove_managed_files_if_owned`. Returns `Err` when the
    /// managed-homes directory is unreadable; `remove` surfaces the error and
    /// `materialize` falls back to creating a fresh home.
    fn managed_homes_matching(
        &self,
        account: &CodexAccount,
    ) -> Result<Vec<PathBuf>, CodexAccountManagerError> {
        let mut matches: Vec<(String, PathBuf)> = fs::read_dir(managed_homes_directory())?
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|home_path| home_path.is_dir())
            .filter(|home_path| {
                self.discovered_managed_account(home_path, std::slice::from_ref(account))
                    .is_some_and(|candidate| candidate.matches(account))
            })
            .map(|home_path| {
                let resolved =
                    std::path::absolute(&home_path).unwrap_or_else(|_| home_path.clone());
                (managed_home_key(resolved.as_path()), resolved)
            })
            .collect();
        matches.sort_by(|a, b| a.0.cmp(&b.0));
        matches.dedup_by(|a, b| a.0 == b.0);
        Ok(matches.into_iter().map(|(_, path)| path).collect())
    }

    /// Find an app-managed home that already holds credentials for `account`.
    ///
    /// Returns `None` when no managed home matches or the directory is
    /// unreadable — callers then create a fresh home, matching the previous
    /// behaviour.
    fn existing_managed_home_matching(&self, account: &CodexAccount) -> Option<PathBuf> {
        self.managed_homes_matching(account)
            .ok()?
            .into_iter()
            .next()
    }

    fn managed_home_paths_matching(
        &self,
        account: &CodexAccount,
    ) -> Result<Vec<PathBuf>, CodexAccountManagerError> {
        ensure_directories()?;
        if self
            .discovered_managed_account(&account.codex_home_path, &[])
            .is_some_and(|fresh| !fresh.matches(account))
        {
            return Err(CodexAccountManagerError::Message(
                "This managed home now belongs to a different account. Refresh the account list before removing it.".into(),
            ));
        }
        let mut targets: Vec<PathBuf> = vec![
            std::path::absolute(&account.codex_home_path)
                .unwrap_or_else(|_| account.codex_home_path.clone()),
        ];
        // The account's own home is always first; skip the walk's duplicate
        // of it so removal never processes the same home twice.
        let head_key = managed_home_key(targets[0].as_path());
        targets.extend(
            self.managed_homes_matching(account)?
                .into_iter()
                .filter(|home| managed_home_key(home.as_path()) != head_key),
        );
        Ok(targets)
    }

    fn authenticate_account(
        &self,
        home_path: &Path,
        source: CodexAccountSource,
        existing: Option<&CodexAccount>,
        handle: Option<&ManagedLoginProcess>,
    ) -> Result<CodexAccount, CodexAccountManagerError> {
        let result = CodexLoginRunner::run(home_path, Duration::from_secs(180), handle);

        match &result.outcome {
            CodexLoginOutcome::Cancelled => {
                return Err(CodexAccountManagerError::Message(
                    "Account setup cancelled.".to_string(),
                ));
            }
            CodexLoginOutcome::MissingBinary => {
                return Err(CodexAccountManagerError::Message(
                    "Codex CLI could not be found. Install Codex Desktop or the Codex CLI, then restart CodexBar.".to_string(),
                ));
            }
            CodexLoginOutcome::TimedOut(_) => {
                return Err(CodexAccountManagerError::Message(
                    "The Codex sign-in flow timed out.".to_string(),
                ));
            }
            CodexLoginOutcome::LaunchFailed(output) => {
                return Err(CodexAccountManagerError::Message(format!(
                    "Failed to start the Codex sign-in flow: {output}"
                )));
            }
            CodexLoginOutcome::Failed(output) => {
                return Err(CodexAccountManagerError::Message(format!(
                    "The Codex sign-in flow did not complete.\n{output}"
                )));
            }
            CodexLoginOutcome::Success(_) => {}
        }

        let identity = load_identity(home_path)?;
        if identity.email.is_none() && identity.provider_account_id.is_none() {
            return Err(CodexAccountManagerError::Message(
                "Sign-in completed, but the account identity could not be read.".to_string(),
            ));
        }

        let now = utc_now();
        let mut authenticated = CodexAccount::new(
            existing
                .map(|account| account.id)
                .unwrap_or_else(Uuid::new_v4),
            existing.and_then(|account| account.nickname.clone()),
            identity
                .email
                .clone()
                .or_else(|| existing.and_then(|account| account.email_hint.clone())),
            identity
                .auth_subject
                .clone()
                .or_else(|| existing.and_then(|account| account.auth_subject.clone())),
            provider_account_id_after_auth(&identity, existing),
            home_path.to_path_buf(),
            source,
            existing.map(|account| account.created_at).unwrap_or(now),
            now,
            Some(now),
        );
        authenticated.workspace_account_id =
            existing.and_then(|account| account.workspace_account_id.clone());
        Ok(authenticated)
    }

    fn discovered_managed_account(
        &self,
        home_path: &Path,
        existing: &[CodexAccount],
    ) -> Option<CodexAccount> {
        if !home_path.is_dir() {
            return None;
        }
        let auth_path = home_path.join("auth.json");
        if !auth_path.exists() {
            return None;
        }
        let identity = load_identity(home_path).ok()?;
        if identity.email.is_none() && identity.provider_account_id.is_none() {
            return None;
        }

        let discovered_at = directory_timestamp(home_path);
        let candidate = candidate_account(
            identity.clone(),
            home_path,
            CodexAccountSource::ManagedByApp,
        );
        let matched = existing.iter().find(|account| candidate.matches(account));
        Some(build_discovered_account(
            matched,
            identity,
            home_path.to_path_buf(),
            CodexAccountSource::ManagedByApp,
            discovered_at,
        ))
    }
}

pub(super) fn candidate_account(
    identity: AuthBackedIdentity,
    home_path: &Path,
    source: CodexAccountSource,
) -> CodexAccount {
    CodexAccount::new(
        Uuid::new_v4(),
        None,
        identity.email.clone(),
        identity.auth_subject.clone(),
        identity.provider_account_id.clone(),
        home_path.to_path_buf(),
        source,
        utc_now(),
        utc_now(),
        None,
    )
}

/// Keep a v0.56.3 provider id when it is the legacy selected workspace. A
/// fresh auth read may report the auth-file default instead; that value must
/// not silently replace the app-owned selection.
fn provider_account_id_after_auth(
    identity: &AuthBackedIdentity,
    existing: Option<&CodexAccount>,
) -> Option<String> {
    if let Some(existing) = existing
        && existing.workspace_account_id.is_none()
        && existing.provider_account_id.is_some()
        && identity
            .provider_account_id
            .as_deref()
            .map(str::trim)
            .is_none_or(|auth_id| {
                existing
                    .normalized_provider_account_id()
                    .is_some_and(|selected_id| selected_id != auth_id.to_lowercase())
            })
    {
        return existing.provider_account_id.clone();
    }
    identity
        .provider_account_id
        .clone()
        .or_else(|| existing.and_then(|account| account.provider_account_id.clone()))
}

fn build_discovered_account(
    matched: Option<&CodexAccount>,
    identity: AuthBackedIdentity,
    home_path: PathBuf,
    source: CodexAccountSource,
    discovered_at: Option<DateTime<Utc>>,
) -> CodexAccount {
    // No readable timestamp on a home we just found is a filesystem oddity;
    // "now" is the only honest fallback for discovery ordering.
    let discovered_at = discovered_at.unwrap_or_else(utc_now);
    let mut discovered = CodexAccount::new(
        matched
            .map(|account| account.id)
            .unwrap_or_else(Uuid::new_v4),
        matched.and_then(|account| account.nickname.clone()),
        identity
            .email
            .clone()
            .or_else(|| matched.and_then(|account| account.email_hint.clone())),
        identity
            .auth_subject
            .clone()
            .or_else(|| matched.and_then(|account| account.auth_subject.clone())),
        provider_account_id_after_auth(&identity, matched),
        home_path,
        source,
        matched
            .map(|account| account.created_at)
            .unwrap_or(discovered_at),
        matched
            .map(|account| account.updated_at.max(discovered_at))
            .unwrap_or(discovered_at),
        matched
            .and_then(|account| account.last_authenticated_at)
            .or(Some(discovered_at)),
    );
    discovered.workspace_account_id =
        matched.and_then(|account| account.workspace_account_id.clone());
    discovered
}

fn directory_timestamp(path: &Path) -> Option<DateTime<Utc>> {
    let auth_path = path.join("auth.json");
    if auth_path.exists()
        && let Ok(metadata) = fs::metadata(&auth_path)
        && let Ok(modified) = metadata.modified()
    {
        return Some(modified.into());
    }
    fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .map(Into::into)
        .ok()
}

/// Whether `candidate` holds credentials at least as recently refreshed as
/// `incumbent`, so reusing a managed home cannot downgrade a live account to a
/// stale token. When either side's freshness is unknown — unreadable JSON,
/// missing `last_refresh`, or an unrecognized schema — this defaults to
/// `false`: the incumbent is kept rather than clobbered. Clobbering on
/// unknown freshness is how a stale ambient token kills a working account
/// (the failure mode that motivated home reuse in the first place).
fn credentials_are_at_least_as_fresh(candidate: &Path, incumbent: &Path) -> bool {
    let last_refresh = |path: &Path| {
        fs::read_to_string(path)
            .ok()
            .and_then(|json| parse_credentials_json(&json).ok())
            .and_then(|credentials| credentials.last_refresh)
    };
    match (last_refresh(candidate), last_refresh(incumbent)) {
        (Some(candidate), Some(incumbent)) => candidate >= incumbent,
        // Unknown freshness must not destroy a possibly-live incumbent.
        _ => false,
    }
}

fn managed_home_key(path: &Path) -> String {
    std::path::absolute(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .to_lowercase()
}

fn path_has_children(path: &Path) -> bool {
    if !path.exists() || !path.is_dir() {
        return false;
    }
    fs::read_dir(path)
        .map(|mut entries| entries.next().is_some())
        .unwrap_or(false)
}

fn timestamp_slug() -> String {
    utc_now().format("%Y%m%d-%H%M%S").to_string()
}

/// Compute the replacement `creator_id` for the target account.
fn updated_creator_id(
    creator_id: Option<&str>,
    previous_account_id: Option<&str>,
    target_account_id: &str,
) -> Option<String> {
    let creator_id = creator_id?.trim();
    if creator_id.is_empty() {
        return None;
    }
    if creator_id == target_account_id || creator_id.ends_with(&format!("__{target_account_id}")) {
        return Some(creator_id.to_string());
    }
    if let Some(previous) = previous_account_id
        && creator_id.contains(previous)
    {
        return Some(creator_id.replace(previous, target_account_id));
    }
    if looks_like_uuid(creator_id) {
        return Some(target_account_id.to_string());
    }
    if let Some((prefix, suffix)) = creator_id.rsplit_once("__")
        && looks_like_uuid(suffix)
    {
        return Some(format!("{prefix}__{target_account_id}"));
    }
    None
}

fn looks_like_uuid(value: &str) -> bool {
    Uuid::parse_str(value.trim()).is_ok()
}

include!("account_manager/tests.rs");
