//! Durable Codex quota reset observations (upstream 0.62.0 #3358).
//!
//! Each refresh of the Codex chart observes the live weekly window's
//! `resets_at`. Persisting those observations lets
//! [`crate::codex_costs::quota_windows::codex_quota_windows_from_cache`]
//! recover real window boundaries across restarts instead of estimating every
//! boundary from the live window alone. Modeled on the Claude
//! `reset_observations` store; Codex history is not account-partitioned, so
//! observations live under a single well-known scope.

use crate::{atomic_file, secure_file};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use thiserror::Error;
use uuid::Uuid;

const STORE_VERSION: u32 = 1;
const STORE_RELATIVE_PATH: &str = "codex/quota-reset-observations-v1.json";
const MAX_OBSERVATIONS_PER_ACCOUNT: usize = 512;

/// Every Codex chart observation shares this scope (no account partitioning).
pub const CODEX_ACCOUNT_SCOPE: &str = "codex";

#[derive(Debug, Error)]
pub enum CodexResetObservationError {
    #[error("Codex reset observation account scope is empty")]
    EmptyAccountScope,
    #[error("Codex reset observation account scope does not match the requested partition")]
    AccountScopeMismatch,
    #[error("failed to read Codex reset observations: {0}")]
    Read(#[source] std::io::Error),
    #[error("failed to decode Codex reset observations: {0}")]
    Deserialize(#[source] serde_json::Error),
    #[error("unsupported Codex reset observation store version {0}")]
    UnsupportedVersion(u32),
    #[error("failed to encode Codex reset observations: {0}")]
    Serialize(#[source] serde_json::Error),
    #[error("failed to persist Codex reset observations: {0}")]
    Persist(#[source] std::io::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CodexResetObservationStore {
    version: u32,
    #[serde(default)]
    accounts: BTreeMap<String, Vec<CodexResetObservation>>,
}

impl Default for CodexResetObservationStore {
    fn default() -> Self {
        Self {
            version: STORE_VERSION,
            accounts: BTreeMap::new(),
        }
    }
}

/// One observed weekly reset: when it was seen, and the boundary it pointed at.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodexResetObservation {
    pub captured_at: chrono::DateTime<chrono::Utc>,
    pub resets_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexResetObservationMergeResult {
    pub observations: Vec<CodexResetObservation>,
    pub changed: bool,
}

/// Path of the store relative to an explicit config root (tests, proof homes).
pub fn store_path(config_root: &Path) -> PathBuf {
    config_root.join(STORE_RELATIVE_PATH)
}

fn validate_scope(account_scope: &str) -> Result<(), CodexResetObservationError> {
    if account_scope.trim().is_empty() {
        Err(CodexResetObservationError::EmptyAccountScope)
    } else {
        Ok(())
    }
}

/// A missing store reads as empty. Only the merge path also treats a blank
/// file as empty; `load_reset_observations` reports it as a decode error.
fn read_store(
    path: &Path,
    blank_is_empty: bool,
) -> Result<CodexResetObservationStore, CodexResetObservationError> {
    let raw = match secure_file::read_string(path) {
        Ok(raw) if blank_is_empty && raw.trim().is_empty() => {
            return Ok(CodexResetObservationStore::default());
        }
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(CodexResetObservationStore::default());
        }
        Err(error) => return Err(CodexResetObservationError::Read(error)),
    };
    let store: CodexResetObservationStore =
        serde_json::from_str(&raw).map_err(CodexResetObservationError::Deserialize)?;
    if store.version != STORE_VERSION {
        return Err(CodexResetObservationError::UnsupportedVersion(
            store.version,
        ));
    }
    Ok(store)
}

/// Read the observations recorded for `account_scope`.
pub fn load_reset_observations(
    config_root: &Path,
    account_scope: &str,
) -> Result<Vec<CodexResetObservation>, CodexResetObservationError> {
    validate_scope(account_scope)?;
    let store = read_store(&store_path(config_root), false)?;
    Ok(store
        .accounts
        .get(account_scope)
        .cloned()
        .unwrap_or_default())
}

/// Deduplicate and order observations: unique by `resets_at` (newest
/// `captured_at` wins), then sorted by reset time. Keeps the log bounded.
pub fn merge_reset_observations(
    existing: &mut Vec<CodexResetObservation>,
    incoming: impl IntoIterator<Item = CodexResetObservation>,
) -> bool {
    let before = existing.clone();
    for observation in incoming {
        match existing
            .iter_mut()
            .find(|row| row.resets_at == observation.resets_at)
        {
            Some(row) if row.captured_at < observation.captured_at => {
                row.captured_at = observation.captured_at;
            }
            Some(_) => {}
            None => existing.push(observation),
        }
    }
    existing.sort_by_key(|row| row.resets_at);
    if existing.len() > MAX_OBSERVATIONS_PER_ACCOUNT {
        let excess = existing.len() - MAX_OBSERVATIONS_PER_ACCOUNT;
        existing.drain(..excess);
    }
    *existing != before
}

/// Merge one freshly observed reset and persist the store.
///
/// The write is atomic: bytes are staged with `secure_file::write_string` and
/// published with `atomic_file::replace_staged`, so a failure leaves the
/// previous store intact and never exposes a partly written secret.
pub fn merge_and_persist_reset_observation(
    config_root: &Path,
    account_scope: &str,
    observed_resets_at: chrono::DateTime<chrono::Utc>,
    captured_at: chrono::DateTime<chrono::Utc>,
) -> Result<CodexResetObservationMergeResult, CodexResetObservationError> {
    validate_scope(account_scope)?;
    if account_scope != CODEX_ACCOUNT_SCOPE {
        return Err(CodexResetObservationError::AccountScopeMismatch);
    }

    let path = store_path(config_root);
    let mut store = read_store(&path, true)?;
    let (changed, observations) = {
        let rows = store.accounts.entry(account_scope.to_string()).or_default();
        let changed = merge_reset_observations(
            rows,
            [CodexResetObservation {
                captured_at,
                resets_at: observed_resets_at,
            }],
        );
        (changed, rows.clone())
    };
    if changed || !path.exists() {
        let raw =
            serde_json::to_string_pretty(&store).map_err(CodexResetObservationError::Serialize)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(CodexResetObservationError::Read)?;
        }
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("quota-reset-observations-v1.json");
        let temp = path.with_file_name(format!(".{file_name}.tmp-{}", Uuid::new_v4()));
        let result = (|| {
            secure_file::write_string(&temp, &raw)?;
            atomic_file::replace_staged(&temp, &path)
        })();
        if let Err(error) = result {
            let cleanup_result = std::fs::remove_file(&temp);
            if let Err(cleanup_error) = cleanup_result {
                tracing::warn!(
                    "failed to clean staged codex reset observations file: {cleanup_error}"
                );
            }
            tracing::debug!("publishing reset observations failed: {error}");
            return Err(CodexResetObservationError::Read(error));
        }
    }
    Ok(CodexResetObservationMergeResult {
        observations,
        changed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn temp_root(tag: &str) -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix(&format!("codex-resets-{tag}-"))
            .tempdir()
            .expect("temp dir")
    }

    fn at(secs: i64) -> chrono::DateTime<chrono::Utc> {
        chrono::Utc.timestamp_opt(secs, 0).unwrap()
    }

    #[test]
    fn rejects_empty_scope() {
        let root = temp_root("empty-scope");
        assert!(matches!(
            load_reset_observations(root.path(), "  "),
            Err(CodexResetObservationError::EmptyAccountScope)
        ));
    }

    #[test]
    fn rejects_foreign_scope() {
        let root = temp_root("foreign-scope");
        assert!(matches!(
            merge_and_persist_reset_observation(root.path(), "claude", at(1_000), at(2_000)),
            Err(CodexResetObservationError::AccountScopeMismatch)
        ));
    }

    #[test]
    fn missing_store_loads_empty() {
        let root = temp_root("missing");
        assert!(
            load_reset_observations(root.path(), CODEX_ACCOUNT_SCOPE)
                .expect("load")
                .is_empty()
        );
    }

    #[test]
    fn observation_survives_reload_and_dedupes_by_reset() {
        let root = temp_root("dedupe");
        merge_and_persist_reset_observation(root.path(), CODEX_ACCOUNT_SCOPE, at(5_000), at(1_000))
            .expect("first merge");
        // Same reset observed again later: keep the newer capture, still one row.
        merge_and_persist_reset_observation(root.path(), CODEX_ACCOUNT_SCOPE, at(5_000), at(2_000))
            .expect("second merge");
        let rows = load_reset_observations(root.path(), CODEX_ACCOUNT_SCOPE).expect("load");
        assert_eq!(
            rows,
            vec![CodexResetObservation {
                captured_at: at(2_000),
                resets_at: at(5_000)
            }]
        );
        // An older capture for the same reset must not regress it.
        merge_and_persist_reset_observation(root.path(), CODEX_ACCOUNT_SCOPE, at(5_000), at(500))
            .expect("third merge");
        let rows = load_reset_observations(root.path(), CODEX_ACCOUNT_SCOPE).expect("load");
        assert_eq!(rows[0].captured_at, at(2_000));
    }

    #[test]
    fn observations_are_sorted_by_reset_and_bounded() {
        let root = temp_root("sort");
        merge_and_persist_reset_observation(root.path(), CODEX_ACCOUNT_SCOPE, at(9_000), at(1_000))
            .expect("late");
        merge_and_persist_reset_observation(root.path(), CODEX_ACCOUNT_SCOPE, at(3_000), at(1_100))
            .expect("early");
        let rows = load_reset_observations(root.path(), CODEX_ACCOUNT_SCOPE).expect("load");
        assert_eq!(
            rows.iter().map(|row| row.resets_at).collect::<Vec<_>>(),
            vec![at(3_000), at(9_000)]
        );
    }

    #[test]
    fn unsupported_version_is_rejected() {
        let root = temp_root("version");
        let path = store_path(root.path());
        std::fs::create_dir_all(path.parent().unwrap()).expect("mkdir");
        secure_file::write_string(&path, r#"{"version": 99, "accounts": {}}"#).expect("write");
        assert!(matches!(
            load_reset_observations(root.path(), CODEX_ACCOUNT_SCOPE),
            Err(CodexResetObservationError::UnsupportedVersion(99))
        ));
    }

    #[test]
    fn blank_store_fails_load_but_merges_as_empty() {
        let root = temp_root("blank");
        let path = store_path(root.path());
        std::fs::create_dir_all(path.parent().unwrap()).expect("mkdir");
        secure_file::write_string(
            &path, "  
",
        )
        .expect("write");
        assert!(matches!(
            load_reset_observations(root.path(), CODEX_ACCOUNT_SCOPE),
            Err(CodexResetObservationError::Deserialize(_))
        ));
        let merged = merge_and_persist_reset_observation(
            root.path(),
            CODEX_ACCOUNT_SCOPE,
            at(4_000),
            at(1_000),
        )
        .expect("merge over blank store");
        assert!(merged.changed);
        assert_eq!(
            load_reset_observations(root.path(), CODEX_ACCOUNT_SCOPE).expect("load"),
            merged.observations
        );
    }

    #[test]
    fn unsupported_version_blocks_merge() {
        let root = temp_root("merge-version");
        let path = store_path(root.path());
        std::fs::create_dir_all(path.parent().unwrap()).expect("mkdir");
        secure_file::write_string(&path, r#"{"version": 99, "accounts": {}}"#).expect("write");
        assert!(matches!(
            merge_and_persist_reset_observation(
                root.path(),
                CODEX_ACCOUNT_SCOPE,
                at(4_000),
                at(1_000)
            ),
            Err(CodexResetObservationError::UnsupportedVersion(99))
        ));
    }

    #[test]
    fn persisted_file_is_secure_wrapped() {
        let root = temp_root("secure");
        merge_and_persist_reset_observation(root.path(), CODEX_ACCOUNT_SCOPE, at(7_000), at(1_000))
            .expect("merge");
        let raw = std::fs::read_to_string(store_path(root.path())).expect("read");
        assert!(raw.contains("codexbar.secure-file"), "not DPAPI wrapped");
        assert!(!raw.contains("resets_at"), "plaintext payload leaked");
    }
}
