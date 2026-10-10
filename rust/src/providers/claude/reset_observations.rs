//! Durable, account-partitioned Claude quota reset observations.

use super::quota_history::ClaudeQuotaResetObservation;
use crate::{atomic_file, secure_file};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use thiserror::Error;
use uuid::Uuid;

const STORE_VERSION: u32 = 1;
const STORE_RELATIVE_PATH: &str = "claude/quota-reset-observations-v1.json";

#[derive(Debug, Error)]
pub enum ClaudeResetObservationError {
    #[error("Claude reset observation account scope is empty")]
    EmptyAccountScope,
    #[error("Claude reset observation account scope does not match the requested partition")]
    AccountScopeMismatch,
    #[error("failed to read Claude reset observations: {0}")]
    Read(#[source] std::io::Error),
    #[error("failed to decode Claude reset observations: {0}")]
    Deserialize(#[source] serde_json::Error),
    #[error("unsupported Claude reset observation store version {0}")]
    UnsupportedVersion(u32),
    #[error("failed to encode Claude reset observations: {0}")]
    Serialize(#[source] serde_json::Error),
    #[error("failed to persist Claude reset observations: {0}")]
    Persist(#[source] std::io::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ClaudeResetObservationStore {
    version: u32,
    #[serde(default)]
    accounts: BTreeMap<String, Vec<ClaudeQuotaResetObservation>>,
}

impl Default for ClaudeResetObservationStore {
    fn default() -> Self {
        Self {
            version: STORE_VERSION,
            accounts: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeResetObservationMergeResult {
    pub observations: Vec<ClaudeQuotaResetObservation>,
    pub changed: bool,
}

pub fn store_path(config_root: &Path) -> PathBuf {
    config_root.join(STORE_RELATIVE_PATH)
}

pub fn load_reset_observations(
    config_root: &Path,
    account_scope: &str,
) -> Result<Vec<ClaudeQuotaResetObservation>, ClaudeResetObservationError> {
    validate_scope(account_scope)?;
    let path = store_path(config_root);
    let raw = match secure_file::read_string(&path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(ClaudeResetObservationError::Read(error)),
    };
    let store: ClaudeResetObservationStore =
        serde_json::from_str(&raw).map_err(ClaudeResetObservationError::Deserialize)?;
    validate_store(&store)?;
    Ok(store
        .accounts
        .get(account_scope)
        .cloned()
        .unwrap_or_default())
}

pub fn merge_reset_observations(
    existing: &mut Vec<ClaudeQuotaResetObservation>,
    incoming: impl IntoIterator<Item = ClaudeQuotaResetObservation>,
) -> bool {
    let before = existing.clone();
    existing.extend(incoming);
    existing.sort_by(|left, right| {
        left.account_scope
            .cmp(&right.account_scope)
            .then_with(|| left.captured_at.cmp(&right.captured_at))
            .then_with(|| left.resets_at.cmp(&right.resets_at))
    });
    existing.dedup();
    *existing != before
}

pub fn merge_and_persist_reset_observations(
    config_root: &Path,
    account_scope: &str,
    incoming: &[ClaudeQuotaResetObservation],
) -> Result<ClaudeResetObservationMergeResult, ClaudeResetObservationError> {
    validate_scope(account_scope)?;
    if incoming
        .iter()
        .any(|observation| observation.account_scope != account_scope)
    {
        return Err(ClaudeResetObservationError::AccountScopeMismatch);
    }

    let path = store_path(config_root);
    let mut store = load_store(&path)?;
    let (changed, observations) = {
        let observations = store.accounts.entry(account_scope.to_string()).or_default();
        let changed = merge_reset_observations(observations, incoming.iter().cloned());
        (changed, observations.clone())
    };
    if changed || !path.exists() {
        let raw =
            serde_json::to_string_pretty(&store).map_err(ClaudeResetObservationError::Serialize)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(ClaudeResetObservationError::Read)?;
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
        if result.is_err() {
            let _cleanup = std::fs::remove_file(&temp);
        }
        result.map_err(ClaudeResetObservationError::Persist)?;
    }
    Ok(ClaudeResetObservationMergeResult {
        observations,
        changed,
    })
}

fn load_store(path: &Path) -> Result<ClaudeResetObservationStore, ClaudeResetObservationError> {
    let raw = match secure_file::read_string(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ClaudeResetObservationStore::default());
        }
        Err(error) => return Err(ClaudeResetObservationError::Read(error)),
    };
    let store: ClaudeResetObservationStore =
        serde_json::from_str(&raw).map_err(ClaudeResetObservationError::Deserialize)?;
    validate_store(&store)?;
    Ok(store)
}

fn validate_scope(account_scope: &str) -> Result<(), ClaudeResetObservationError> {
    if account_scope.trim().is_empty() {
        Err(ClaudeResetObservationError::EmptyAccountScope)
    } else {
        Ok(())
    }
}

fn validate_store(store: &ClaudeResetObservationStore) -> Result<(), ClaudeResetObservationError> {
    if store.version != STORE_VERSION {
        return Err(ClaudeResetObservationError::UnsupportedVersion(
            store.version,
        ));
    }
    for (scope, observations) in &store.accounts {
        validate_scope(scope)?;
        if observations
            .iter()
            .any(|observation| observation.account_scope != *scope)
        {
            return Err(ClaudeResetObservationError::AccountScopeMismatch);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{DateTime, Utc};
    use tempfile::tempdir;

    fn observation(scope: &str, captured: &str, reset: &str) -> ClaudeQuotaResetObservation {
        ClaudeQuotaResetObservation {
            account_scope: scope.into(),
            captured_at: captured.parse::<DateTime<Utc>>().unwrap(),
            resets_at: reset.parse::<DateTime<Utc>>().unwrap(),
        }
    }

    #[test]
    fn persist_reload_and_account_isolation() {
        let root = tempdir().unwrap();
        let a = observation("a", "2026-09-20T10:00:00Z", "2026-09-21T10:00:00Z");
        let b = observation("b", "2026-09-20T11:00:00Z", "2026-09-21T11:00:00Z");
        merge_and_persist_reset_observations(root.path(), "a", std::slice::from_ref(&a)).unwrap();
        merge_and_persist_reset_observations(root.path(), "b", std::slice::from_ref(&b)).unwrap();
        assert_eq!(load_reset_observations(root.path(), "a").unwrap(), vec![a]);
        assert_eq!(load_reset_observations(root.path(), "b").unwrap(), vec![b]);
    }

    #[test]
    fn duplicate_merge_is_idempotent_and_sorted() {
        let root = tempdir().unwrap();
        let late = observation("a", "2026-09-20T11:00:00Z", "2026-09-21T11:00:00Z");
        let early = observation("a", "2026-09-20T10:00:00Z", "2026-09-21T10:00:00Z");
        let first =
            merge_and_persist_reset_observations(root.path(), "a", &[late.clone(), early.clone()])
                .unwrap();
        let second = merge_and_persist_reset_observations(root.path(), "a", &[late]).unwrap();
        assert!(first.changed);
        assert!(!second.changed);
        assert_eq!(
            second.observations,
            vec![early, first.observations[1].clone()]
        );
    }

    #[test]
    fn malformed_and_future_stores_fail_closed() {
        let root = tempdir().unwrap();
        let path = store_path(root.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"not-json").unwrap();
        assert!(matches!(
            load_reset_observations(root.path(), "a"),
            Err(ClaudeResetObservationError::Deserialize(_))
        ));
        std::fs::write(&path, br#"{"version":99,"accounts":{}}"#).unwrap();
        assert_eq!(
            load_reset_observations(root.path(), "a")
                .unwrap_err()
                .to_string(),
            "unsupported Claude reset observation store version 99"
        );
    }
}
