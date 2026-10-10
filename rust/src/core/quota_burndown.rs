//! Recorded remaining-quota burndown for Codex and Claude (upstream 0.70.0
//! #4085, `QuotaBurndownModel` / `PlanUtilizationHistoryStore`).
//!
//! Two halves:
//!
//! 1. [`plan_history_store`] persists the per-account plan-utilization history
//!    that today lives only in the process-local `HISTORY_STORE`
//!    ([`super::session_equivalent_forecast`]). One JSON document per
//!    provider, partitioned by hashed account key — no identifiers beyond the
//!    existing account-key rules, mirroring upstream's hashed account keys.
//! 2. [`QuotaBurndownModel`] turns a persisted series plus the live
//!    `RateWindow` into the chart inputs: observed remaining samples (a usage
//!    drop starts a new segment), the ideal 100%→0% guide, and the capture
//!    age. Expired windows produce `None`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use super::rate_window::RateWindow;
use super::session_equivalent_forecast::PlanUtilizationHistoryEntry;
use crate::atomic_file;
use crate::secure_file;

const STORE_VERSION: u32 = 1;
const STORE_RELATIVE_PATH: &str = "history/quota-burndown-v1.json";

/// Upstream's `resetEquivalenceTolerance`: resets within 2 minutes are the
/// same window.
pub const RESET_EQUIVALENCE_TOLERANCE_SECS: i64 = 120;

/// Upstream caps each account series at 17,520 samples (~730 days hourly).
pub const MAX_SERIES_SAMPLES: usize = 17_520;

#[derive(Debug, Error)]
pub enum QuotaBurndownStoreError {
    #[error("quota burndown account key is empty")]
    EmptyAccountKey,
    #[error("failed to read quota burndown history: {0}")]
    Read(#[source] std::io::Error),
    #[error("failed to decode quota burndown history: {0}")]
    Deserialize(#[source] serde_json::Error),
    #[error("unsupported quota burndown store version {0}")]
    UnsupportedVersion(u32),
    #[error("failed to encode quota burndown history: {0}")]
    Serialize(#[source] serde_json::Error),
    #[error("failed to persist quota burndown history: {0}")]
    Persist(#[source] std::io::Error),
}

/// One persisted plan-utilization entry (serde form of the in-process
/// [`PlanUtilizationHistoryEntry`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PersistedPlanEntry {
    /// RFC 3339 capture time.
    pub captured_at: String,
    pub used_percent: f64,
    /// RFC 3339 reset time; absent when the provider did not advertise one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resets_at: Option<String>,
}

/// One named series: `session` (5h) or `weekly` (7d), per account.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PersistedPlanSeries {
    pub name: String,
    pub window_minutes: u32,
    pub entries: Vec<PersistedPlanEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProviderBurndownDocument {
    version: u32,
    /// Hashed account key -> series list. Hashing keeps persisted keys
    /// anonymous for email/org identities, matching upstream's key rules.
    accounts: BTreeMap<String, Vec<PersistedPlanSeries>>,
}

impl Default for ProviderBurndownDocument {
    fn default() -> Self {
        Self {
            version: STORE_VERSION,
            accounts: BTreeMap::new(),
        }
    }
}

/// Hash an in-process account key for persistence. Token-account ids are
/// already opaque; email/org keys are lowercased and hashed so the file
/// carries no plain addresses and case variants collapse (the `ponytail`
/// note in `session_equivalent_forecast`).
pub fn persisted_account_key(account_key: Option<&str>) -> String {
    match account_key.map(str::trim).filter(|k| !k.is_empty()) {
        Some(key) if key.starts_with("token-account:") => key.to_ascii_lowercase(),
        Some(key) => {
            format!(
                "sha256:{}",
                super::aws_signing::sha256_hex(key.to_ascii_lowercase().as_bytes())
            )
        }
        None => String::new(),
    }
}

pub fn store_path(config_root: &Path) -> PathBuf {
    config_root.join(STORE_RELATIVE_PATH)
}

fn validate_key(account_key: &str) -> Result<(), QuotaBurndownStoreError> {
    if account_key.trim().is_empty() {
        Err(QuotaBurndownStoreError::EmptyAccountKey)
    } else {
        Ok(())
    }
}

fn validate_document(document: &ProviderBurndownDocument) -> Result<(), QuotaBurndownStoreError> {
    if document.version != STORE_VERSION {
        return Err(QuotaBurndownStoreError::UnsupportedVersion(
            document.version,
        ));
    }
    Ok(())
}

fn load_document(path: &Path) -> Result<ProviderBurndownDocument, QuotaBurndownStoreError> {
    let raw = match secure_file::read_string(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ProviderBurndownDocument::default());
        }
        Err(error) => return Err(QuotaBurndownStoreError::Read(error)),
    };
    let document: ProviderBurndownDocument =
        serde_json::from_str(&raw).map_err(QuotaBurndownStoreError::Deserialize)?;
    validate_document(&document)?;
    Ok(document)
}

/// The persisted series list for one hashed account key, or empty.
pub fn load_series(
    config_root: &Path,
    provider_id: &str,
    account_key: &str,
) -> Result<Vec<PersistedPlanSeries>, QuotaBurndownStoreError> {
    validate_key(account_key)?;
    let path = store_path(config_root).with_file_name(provider_file_name(provider_id));
    let document = load_document(&path)?;
    Ok(document
        .accounts
        .get(account_key)
        .cloned()
        .unwrap_or_default())
}

/// Merge `incoming` entries into the persisted series for one account and
/// write the document back atomically. Entries are sorted by capture time and
/// deduplicated; the per-series sample cap is enforced.
pub fn merge_and_persist_series(
    config_root: &Path,
    provider_id: &str,
    account_key: &str,
    incoming: Vec<PersistedPlanSeries>,
) -> Result<Vec<PersistedPlanSeries>, QuotaBurndownStoreError> {
    validate_key(account_key)?;
    let path = store_path(config_root).with_file_name(provider_file_name(provider_id));
    let mut document = load_document(&path)?;
    let series = document
        .accounts
        .entry(account_key.to_string())
        .or_default();
    merge_series(series, incoming);
    if series.is_empty() {
        document.accounts.remove(account_key);
    }
    persist_document(&path, &document)?;
    Ok(document
        .accounts
        .get(account_key)
        .cloned()
        .unwrap_or_default())
}

fn provider_file_name(provider_id: &str) -> String {
    format!("{provider_id}-burndown-v1.json")
}

/// Persist one recorded observation pair (session + optional weekly) for the
/// account. Best-effort: `Err` never blocks the in-process forecast path.
pub fn persist_recorded_windows(
    provider_id: &str,
    account_key: Option<&str>,
    session: Option<&PlanUtilizationHistoryEntry>,
    weekly: Option<&PlanUtilizationHistoryEntry>,
    _now: DateTime<Utc>,
) -> Result<(), QuotaBurndownStoreError> {
    let key = persisted_account_key(account_key);
    if key.is_empty() {
        // Upstream keeps an unscoped bucket; locally history without an
        // account key cannot be told apart on a shared machine, so it stays
        // process-local (no identifier is invented).
        return Ok(());
    }
    let Some(config_root) = dirs::config_dir().map(|root| root.join("CodexBar")) else {
        return Ok(());
    };
    let mut series = Vec::new();
    if let Some(entry) = session {
        series.push(PersistedPlanSeries {
            name: "session".to_string(),
            window_minutes: super::session_equivalent_forecast::SESSION_WINDOW_MINUTES,
            entries: vec![PersistedPlanEntry::from_history(entry)],
        });
    }
    if let Some(entry) = weekly {
        series.push(PersistedPlanSeries {
            name: "weekly".to_string(),
            window_minutes: super::session_equivalent_forecast::WEEKLY_WINDOW_MINUTES,
            entries: vec![PersistedPlanEntry::from_history(entry)],
        });
    }
    if series.is_empty() {
        return Ok(());
    }
    merge_and_persist_series(&config_root, provider_id, &key, series).map(|_| ())
}

/// Load the persisted series for one provider+account (empty when the key is
/// missing or the store is unreadable).
pub fn load_persisted_series(
    provider_id: &str,
    account_key: Option<&str>,
) -> Vec<PersistedPlanSeries> {
    let key = persisted_account_key(account_key);
    if key.is_empty() {
        return Vec::new();
    }
    dirs::config_dir()
        .map(|root| root.join("CodexBar"))
        .and_then(|config_root| {
            load_series(&config_root, provider_id, &key)
                .map_err(|error| tracing::debug!(%error, "quota burndown load failed"))
                .ok()
        })
        .unwrap_or_default()
}

fn merge_series(existing: &mut Vec<PersistedPlanSeries>, incoming: Vec<PersistedPlanSeries>) {
    for series in incoming {
        if series.entries.is_empty() {
            continue;
        }
        if let Some(target) = existing.iter_mut().find(|target| {
            target.name == series.name && target.window_minutes == series.window_minutes
        }) {
            target.entries.extend(series.entries);
            normalize_entries(&mut target.entries);
        } else {
            let mut series = series;
            normalize_entries(&mut series.entries);
            existing.push(series);
        }
    }
    existing.retain(|series| !series.entries.is_empty());
    existing.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then_with(|| left.window_minutes.cmp(&right.window_minutes))
    });
}

fn normalize_entries(entries: &mut Vec<PersistedPlanEntry>) {
    entries.sort_by(|left, right| {
        left.captured_at
            .cmp(&right.captured_at)
            .then_with(|| left.used_percent.total_cmp(&right.used_percent))
            .then_with(|| left.resets_at.cmp(&right.resets_at))
    });
    entries.dedup();
    if entries.len() > MAX_SERIES_SAMPLES {
        let drop = entries.len() - MAX_SERIES_SAMPLES;
        entries.drain(0..drop);
    }
}

fn persist_document(
    path: &Path,
    document: &ProviderBurndownDocument,
) -> Result<(), QuotaBurndownStoreError> {
    if document.accounts.is_empty() && !path.exists() {
        return Ok(());
    }
    let raw = serde_json::to_string_pretty(document).map_err(QuotaBurndownStoreError::Serialize)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(QuotaBurndownStoreError::Read)?;
    }
    if document.accounts.is_empty() {
        // Last account for the provider was removed: drop the file.
        match std::fs::remove_file(path) {
            Ok(()) => return Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(QuotaBurndownStoreError::Persist(error)),
        }
    }
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("quota-burndown-v1.json");
    let temp = path.with_file_name(format!(".{file_name}.tmp-{}", Uuid::new_v4()));
    let result = (|| {
        secure_file::write_string(&temp, &raw)?;
        atomic_file::replace_staged(&temp, path)
    })();
    if result.is_err() {
        let _cleanup = std::fs::remove_file(&temp);
    }
    result.map_err(QuotaBurndownStoreError::Persist)?;
    Ok(())
}

impl PersistedPlanEntry {
    pub fn from_history(entry: &PlanUtilizationHistoryEntry) -> Self {
        Self {
            captured_at: entry.captured_at.to_rfc3339(),
            used_percent: entry.used_percent,
            resets_at: entry.resets_at.map(|dt| dt.to_rfc3339()),
        }
    }

    pub fn to_history(&self) -> Option<PlanUtilizationHistoryEntry> {
        Some(PlanUtilizationHistoryEntry {
            captured_at: DateTime::parse_from_rfc3339(&self.captured_at)
                .ok()?
                .with_timezone(&Utc),
            used_percent: self.used_percent,
            resets_at: self
                .resets_at
                .as_ref()
                .and_then(|raw| DateTime::parse_from_rfc3339(raw).ok())
                .map(|dt| dt.with_timezone(&Utc)),
        })
    }
}

/// One chart sample: capture time and remaining percent (0-100).
#[derive(Debug, Clone, PartialEq)]
pub struct QuotaBurndownSample {
    pub captured_at: DateTime<Utc>,
    pub remaining_percent: f64,
}

/// Chart inputs for one recorded quota window (upstream `QuotaBurndownModel`).
///
/// `now` is the live reading's capture time; upstream passes the latest
/// history capture so the line never extends past the last capture.
#[derive(Debug, Clone, PartialEq)]
pub struct QuotaBurndownModel {
    pub start: DateTime<Utc>,
    pub reset: DateTime<Utc>,
    pub samples: Vec<QuotaBurndownSample>,
    pub ideal: [QuotaBurndownSample; 2],
}

impl QuotaBurndownModel {
    /// Build from a history series plus the live window. Returns `None` for
    /// informational placeholders, missing duration/reset, or an expired /
    /// not-yet-started window (`start <= now < reset` must hold).
    pub fn build(
        entries: &[PlanUtilizationHistoryEntry],
        window: &RateWindow,
        now: DateTime<Utc>,
    ) -> Option<Self> {
        if window.is_informational || !window.used_percent.is_finite() {
            return None;
        }
        let window_minutes = window.window_minutes?;
        if window_minutes == 0 {
            return None;
        }
        let reset = window.resets_at?;
        let duration = chrono::Duration::minutes(i64::from(window_minutes));
        if duration <= chrono::Duration::zero() {
            return None;
        }
        let start = reset - duration;
        if start > now || now >= reset {
            return None;
        }

        let tolerance = chrono::Duration::seconds(RESET_EQUIVALENCE_TOLERANCE_SECS);
        let mut historical: Vec<(DateTime<Utc>, f64)> = entries
            .iter()
            .filter(|entry| {
                entry.captured_at >= start
                    && entry.captured_at <= now
                    && entry.used_percent.is_finite()
                    && entry
                        .resets_at
                        .map(|entry_reset| (entry_reset - reset).abs() <= tolerance)
                        .unwrap_or(true)
            })
            .map(|entry| (entry.captured_at, entry.used_percent))
            .collect();

        // Same-timestamp entries collapse to the latest value (the live
        // reading wins a tie), matching upstream's overwrite rule.
        historical.sort_by_key(|(captured_at, _)| *captured_at);
        let mut deduped: Vec<(DateTime<Utc>, f64)> = Vec::with_capacity(historical.len());
        for (date, used) in historical {
            if let Some(last) = deduped.last_mut()
                && last.0 == date
            {
                last.1 = used;
                continue;
            }
            deduped.push((date, used));
        }

        // Walk history + the live reading: a decrease in used percent starts
        // a new segment (upstream's reset-boundary segmentation).
        let mut current: Vec<(DateTime<Utc>, f64)> = Vec::new();
        for (date, used) in deduped
            .into_iter()
            .chain(std::iter::once((now, window.used_percent)))
        {
            if let Some(last) = current.last_mut() {
                if last.0 == date {
                    last.1 = used;
                    continue;
                }
                if used < last.1 {
                    current.clear();
                }
            }
            current.push((date, used));
        }

        Some(Self {
            start,
            reset,
            samples: current
                .into_iter()
                .map(|(captured_at, used)| QuotaBurndownSample {
                    captured_at,
                    remaining_percent: (100.0 - used).clamp(0.0, 100.0),
                })
                .collect(),
            ideal: [
                QuotaBurndownSample {
                    captured_at: start,
                    remaining_percent: 100.0,
                },
                QuotaBurndownSample {
                    captured_at: reset,
                    remaining_percent: 0.0,
                },
            ],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn at(value: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(value)
            .unwrap()
            .with_timezone(&Utc)
    }

    fn entry(captured: &str, used: f64, reset: Option<&str>) -> PlanUtilizationHistoryEntry {
        PlanUtilizationHistoryEntry {
            captured_at: at(captured),
            used_percent: used,
            resets_at: reset.map(at),
        }
    }

    fn window(used: f64, minutes: u32, reset: DateTime<Utc>) -> RateWindow {
        let mut window = RateWindow::new(used);
        window.window_minutes = Some(minutes);
        window.resets_at = Some(reset);
        window
    }

    fn persisted(name: &str, entries: Vec<PersistedPlanEntry>) -> PersistedPlanSeries {
        PersistedPlanSeries {
            name: name.to_string(),
            window_minutes: if name == "session" { 300 } else { 10_080 },
            entries,
        }
    }

    fn persisted_entry(captured: &str, used: f64, reset: Option<&str>) -> PersistedPlanEntry {
        PersistedPlanEntry {
            captured_at: captured.to_string(),
            used_percent: used,
            resets_at: reset.map(str::to_string),
        }
    }

    #[test]
    fn builds_current_window_samples_and_ideal_line() {
        let now = at("2026-09-30T12:00:00Z");
        let reset = now + chrono::Duration::hours(2);
        let entries = vec![
            entry("2026-09-30T10:00:00Z", 20.0, Some("2026-09-30T14:00:00Z")),
            entry("2026-09-30T11:00:00Z", 45.0, Some("2026-09-30T14:00:00Z")),
        ];
        let model = QuotaBurndownModel::build(&entries, &window(60.0, 300, reset), now).unwrap();
        assert_eq!(model.start, reset - chrono::Duration::hours(5));
        assert_eq!(model.reset, reset);
        assert_eq!(
            model.samples,
            vec![
                QuotaBurndownSample {
                    captured_at: at("2026-09-30T10:00:00Z"),
                    remaining_percent: 80.0,
                },
                QuotaBurndownSample {
                    captured_at: at("2026-09-30T11:00:00Z"),
                    remaining_percent: 55.0,
                },
                QuotaBurndownSample {
                    captured_at: now,
                    remaining_percent: 40.0,
                },
            ]
        );
        assert_eq!(
            model.ideal,
            [
                QuotaBurndownSample {
                    captured_at: reset - chrono::Duration::hours(5),
                    remaining_percent: 100.0,
                },
                QuotaBurndownSample {
                    captured_at: reset,
                    remaining_percent: 0.0,
                },
            ]
        );
    }

    #[test]
    fn isolates_current_reset_and_keeps_only_newest_segment_after_usage_drop() {
        let now = at("2026-09-30T12:00:00Z");
        let reset = now + chrono::Duration::hours(2);
        let entries = vec![
            entry("2026-09-30T06:00:00Z", 90.0, Some("2026-09-30T09:00:00Z")),
            entry("2026-09-30T08:00:00Z", 70.0, Some("2026-09-30T14:00:00Z")),
            entry("2026-09-30T10:00:00Z", 80.0, Some("2026-09-30T14:00:00Z")),
            entry("2026-09-30T10:30:00Z", 85.0, Some("2026-09-30T09:00:00Z")),
            entry("2026-09-30T11:00:00Z", 10.0, Some("2026-09-30T14:00:00Z")),
            entry("2026-09-30T11:30:00Z", 25.0, Some("2026-09-30T14:00:00Z")),
        ];
        let model = QuotaBurndownModel::build(&entries, &window(30.0, 300, reset), now).unwrap();
        assert_eq!(
            model.samples,
            vec![
                QuotaBurndownSample {
                    captured_at: at("2026-09-30T11:00:00Z"),
                    remaining_percent: 90.0,
                },
                QuotaBurndownSample {
                    captured_at: at("2026-09-30T11:30:00Z"),
                    remaining_percent: 75.0,
                },
                QuotaBurndownSample {
                    captured_at: now,
                    remaining_percent: 70.0,
                },
            ]
        );
    }

    #[test]
    fn accepts_small_reset_drift_while_excluding_another_cycle() {
        let now = at("2026-09-30T12:00:00Z");
        let reset = now + chrono::Duration::hours(2);
        let entries = vec![
            entry("2026-09-30T10:00:00Z", 20.0, Some("2026-09-30T14:01:30Z")),
            entry("2026-09-30T11:00:00Z", 40.0, Some("2026-09-30T09:00:00Z")),
        ];
        let model = QuotaBurndownModel::build(&entries, &window(50.0, 300, reset), now).unwrap();
        assert_eq!(
            model.samples,
            vec![
                QuotaBurndownSample {
                    captured_at: at("2026-09-30T10:00:00Z"),
                    remaining_percent: 80.0,
                },
                QuotaBurndownSample {
                    captured_at: now,
                    remaining_percent: 50.0,
                },
            ]
        );
    }

    #[test]
    fn requires_valid_current_reset_and_duration() {
        let now = at("2026-09-30T12:00:00Z");
        let entries: Vec<PlanUtilizationHistoryEntry> = Vec::new();
        // No reset.
        let mut no_reset = window(10.0, 300, now + chrono::Duration::hours(1));
        no_reset.resets_at = None;
        assert!(QuotaBurndownModel::build(&entries, &no_reset, now).is_none());
        // Zero duration.
        assert!(
            QuotaBurndownModel::build(
                &entries,
                &window(10.0, 0, now + chrono::Duration::hours(1)),
                now
            )
            .is_none()
        );
        // Informational placeholder.
        let placeholder = RateWindow::informational("no session");
        assert!(QuotaBurndownModel::build(&entries, &placeholder, now).is_none());
        // Expired window (now past reset).
        assert!(
            QuotaBurndownModel::build(
                &entries,
                &window(10.0, 300, now - chrono::Duration::hours(1)),
                now
            )
            .is_none()
        );
    }

    #[test]
    fn uses_live_window_when_history_has_no_current_samples() {
        let now = at("2026-09-30T12:00:00Z");
        let reset = now + chrono::Duration::hours(2);
        let entries = vec![entry(
            "2026-09-30T06:00:00Z",
            80.0,
            Some("2026-09-30T09:00:00Z"),
        )];
        let model = QuotaBurndownModel::build(&entries, &window(125.0, 300, reset), now).unwrap();
        assert_eq!(
            model.samples,
            vec![QuotaBurndownSample {
                captured_at: now,
                remaining_percent: 0.0,
            }]
        );
    }

    #[test]
    fn deduplicates_timestamps_and_skips_nonfinite_history() {
        let now = at("2026-09-30T12:00:00Z");
        let reset = now + chrono::Duration::hours(2);
        let duplicate = "2026-09-30T11:00:00Z";
        let entries = vec![
            PlanUtilizationHistoryEntry {
                captured_at: at("2026-09-30T10:00:00Z"),
                used_percent: f64::NAN,
                resets_at: Some(reset),
            },
            entry(duplicate, 20.0, Some("2026-09-30T14:00:00Z")),
            entry(duplicate, 30.0, Some("2026-09-30T14:00:00Z")),
            entry("2026-09-30T12:00:00Z", 40.0, Some("2026-09-30T14:00:00Z")),
        ];
        let model = QuotaBurndownModel::build(&entries, &window(150.0, 300, reset), now).unwrap();
        assert_eq!(
            model.samples,
            vec![
                QuotaBurndownSample {
                    captured_at: at(duplicate),
                    remaining_percent: 70.0,
                },
                QuotaBurndownSample {
                    captured_at: now,
                    remaining_percent: 0.0,
                },
            ]
        );
        // Nonfinite live usage is unreachable through the public API:
        // `RateWindow::new` normalizes it to 0.0 (see `finite_percent`).
    }

    #[test]
    fn persisted_keys_round_trip_and_hash_personal_identities() {
        assert_eq!(
            persisted_account_key(Some("token-account:0f0e0d0c-0000-0000-0000-000000000000")),
            "token-account:0f0e0d0c-0000-0000-0000-000000000000"
        );
        let hashed = persisted_account_key(Some("Person@Example.com"));
        assert!(hashed.starts_with("sha256:"));
        assert!(!hashed.contains("Person"));
        assert_eq!(hashed, persisted_account_key(Some("person@example.com")));
        assert_eq!(persisted_account_key(None), "");
    }

    #[test]
    fn persist_reload_and_account_isolation() {
        let root = tempdir().unwrap();
        let a = persisted(
            "session",
            vec![persisted_entry(
                "2026-09-30T10:00:00Z",
                20.0,
                Some("2026-09-30T14:00:00Z"),
            )],
        );
        let b = persisted(
            "session",
            vec![persisted_entry(
                "2026-09-30T11:00:00Z",
                40.0,
                Some("2026-09-30T14:00:00Z"),
            )],
        );
        let key_a = persisted_account_key(Some("a@example.com"));
        let key_b = persisted_account_key(Some("b@example.com"));
        merge_and_persist_series(root.path(), "codex", &key_a, vec![a.clone()]).unwrap();
        merge_and_persist_series(root.path(), "codex", &key_b, vec![b.clone()]).unwrap();
        assert_eq!(load_series(root.path(), "codex", &key_a).unwrap(), vec![a]);
        assert_eq!(load_series(root.path(), "codex", &key_b).unwrap(), vec![b]);
    }

    #[test]
    fn duplicate_merge_is_idempotent_and_sorted() {
        let root = tempdir().unwrap();
        let key = persisted_account_key(Some("a@example.com"));
        let early = persisted_entry("2026-09-30T10:00:00Z", 20.0, None);
        let late = persisted_entry("2026-09-30T11:00:00Z", 40.0, None);
        merge_and_persist_series(
            root.path(),
            "codex",
            &key,
            vec![persisted("session", vec![late.clone(), early.clone()])],
        )
        .unwrap();
        let first = load_series(root.path(), "codex", &key).unwrap();
        merge_and_persist_series(
            root.path(),
            "codex",
            &key,
            vec![persisted("session", vec![late])],
        )
        .unwrap();
        let second = load_series(root.path(), "codex", &key).unwrap();
        assert_eq!(first, second);
        assert_eq!(second[0].entries[0], early);
    }

    #[test]
    fn series_merge_extends_and_caps_samples() {
        let mut existing = vec![persisted(
            "session",
            vec![persisted_entry("2026-09-30T10:00:00Z", 20.0, None)],
        )];
        let mut flood = persisted("session", Vec::new());
        flood.entries = (0..MAX_SERIES_SAMPLES + 10)
            .map(|index| {
                persisted_entry(
                    &format!("2026-09-{:02}T10:00:00Z", 1 + index / 24),
                    (index % 100) as f64,
                    None,
                )
            })
            .collect();
        merge_series(&mut existing, vec![flood]);
        assert_eq!(existing.len(), 1);
        assert_eq!(existing[0].entries.len(), MAX_SERIES_SAMPLES);
        // 17531 entries sort into the cap; the first 11 drop (Sept 1's used
        // 0..10), so the oldest kept entry is used 11 on Sept 1.
        assert_eq!(
            existing[0].entries[0],
            persisted_entry("2026-09-01T10:00:00Z", 11.0, None)
        );
    }

    #[test]
    fn empty_incoming_merge_is_a_noop_without_creating_the_file() {
        let root = tempdir().unwrap();
        let key = persisted_account_key(Some("a@example.com"));
        let path = store_path(root.path()).with_file_name(provider_file_name("codex"));
        merge_and_persist_series(root.path(), "codex", &key, Vec::new()).unwrap();
        assert!(!path.exists());
        // Persisting real entries creates it; another empty merge keeps it.
        merge_and_persist_series(
            root.path(),
            "codex",
            &key,
            vec![persisted(
                "session",
                vec![persisted_entry("2026-09-30T10:00:00Z", 20.0, None)],
            )],
        )
        .unwrap();
        assert!(path.exists());
        merge_and_persist_series(root.path(), "codex", &key, Vec::new()).unwrap();
        assert!(path.exists());
    }

    #[test]
    fn history_entry_serde_round_trip() {
        let history = entry("2026-09-30T10:00:00Z", 42.5, Some("2026-09-30T14:00:00Z"));
        let persisted = PersistedPlanEntry::from_history(&history);
        let parsed = persisted.to_history().unwrap();
        assert_eq!(parsed, history);
        let no_reset = entry("2026-09-30T10:00:00Z", 10.0, None);
        let persisted = PersistedPlanEntry::from_history(&no_reset);
        assert_eq!(persisted.to_history().unwrap(), no_reset);
    }
}
