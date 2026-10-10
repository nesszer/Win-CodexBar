//! In-process history store plus the last-full-session estimate cache.

use super::*;

/// Identity that forecast history is scoped to.
///
/// History must never be shared across accounts. Plan sizes differ, so blending
/// observations from two accounts yields a silently wrong median — see the account
/// isolation regression test below.
///
/// ponytail: when this is persisted to disk, hash `account_key` rather than writing the
/// raw address; in-process it stays plain because the email is already resident in
/// `UsageSnapshot`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ForecastScope {
    pub provider_id: String,
    pub account_key: Option<String>,
}

impl ForecastScope {
    pub fn new(provider_id: &str, account_key: Option<&str>) -> Self {
        Self {
            provider_id: provider_id.to_string(),
            account_key: account_key.map(str::to_string),
        }
    }
}

/// Append-only ring of session/weekly observations for burn estimation.
///
/// ponytail: history is process-local and lost on restart; upgrade path = disk cache
/// keyed by provider+account once forecast quality justifies persistence.
#[derive(Debug, Default)]
pub struct SessionEquivalentHistoryStore {
    by_scope: HashMap<ForecastScope, ProviderHistory>,
}

/// The tracked series, in output order: (name, persisted key, window minutes).
const SERIES: [(PlanUtilizationSeriesName, &str, u32); 2] = [
    (
        PlanUtilizationSeriesName::Session,
        "session",
        SESSION_WINDOW_MINUTES,
    ),
    (
        PlanUtilizationSeriesName::Weekly,
        "weekly",
        WEEKLY_WINDOW_MINUTES,
    ),
];

/// One ring per `SERIES` entry, same index.
type ProviderHistory = [Vec<PlanUtilizationHistoryEntry>; SERIES.len()];

static HISTORY_STORE: LazyLock<Mutex<SessionEquivalentHistoryStore>> =
    LazyLock::new(|| Mutex::new(SessionEquivalentHistoryStore::default()));

impl SessionEquivalentHistoryStore {
    /// Record one observation pair for a provider+account (session + optional weekly).
    pub fn record(
        &mut self,
        scope: &ForecastScope,
        session: Option<PlanUtilizationHistoryEntry>,
        weekly: Option<PlanUtilizationHistoryEntry>,
        sample_limit: usize,
    ) {
        let limit = sample_limit.max(1);
        // Keep more raw points than completed groups so grouping still has density.
        let ring = limit.saturating_mul(8).max(24);
        let hist = self.by_scope.entry(scope.clone()).or_default();
        for (buf, entry) in hist.iter_mut().zip([session, weekly]) {
            if let Some(entry) = entry {
                push_ring(buf, entry, ring);
            }
        }
    }

    pub fn histories(&self, scope: &ForecastScope) -> Vec<PlanUtilizationSeriesHistory> {
        let Some(hist) = self.by_scope.get(scope) else {
            return Vec::new();
        };
        SERIES
            .iter()
            .zip(hist)
            .filter(|(_, entries)| !entries.is_empty())
            .map(
                |(&(name, _, window_minutes), entries)| PlanUtilizationSeriesHistory {
                    name,
                    window_minutes,
                    entries: entries.clone(),
                },
            )
            .collect()
    }

    /// Merge persisted series into the in-process store for `scope`. Older
    /// in-process observations win ties; the result stays sorted so the
    /// estimator's chronological check keeps passing.
    pub fn adopt_persisted(
        &mut self,
        scope: &ForecastScope,
        persisted: &[quota_burndown::PersistedPlanSeries],
    ) {
        for series in persisted {
            let entries: Vec<PlanUtilizationHistoryEntry> = series
                .entries
                .iter()
                .filter_map(PersistedEntry::to_history)
                .collect();
            if entries.is_empty() {
                continue;
            }
            let Some(index) = SERIES.iter().position(|(_, key, _)| *key == series.name) else {
                continue;
            };
            let hist = self.by_scope.entry(scope.clone()).or_default();
            merge_ring(
                &mut hist[index],
                entries,
                quota_burndown::MAX_SERIES_SAMPLES,
            );
        }
    }
}

fn push_ring(
    buf: &mut Vec<PlanUtilizationHistoryEntry>,
    entry: PlanUtilizationHistoryEntry,
    ring: usize,
) {
    buf.push(entry);
    if buf.len() > ring {
        let drop = buf.len() - ring;
        buf.drain(0..drop);
    }
}

/// Merge loaded entries into a ring buffer: append, sort, dedup, cap.
fn merge_ring(
    buf: &mut Vec<PlanUtilizationHistoryEntry>,
    entries: Vec<PlanUtilizationHistoryEntry>,
    ring: usize,
) {
    buf.extend(entries);
    buf.sort_by(|left, right| {
        left.captured_at
            .cmp(&right.captured_at)
            .then_with(|| left.used_percent.total_cmp(&right.used_percent))
    });
    buf.dedup();
    if buf.len() > ring {
        let drop = buf.len() - ring;
        buf.drain(0..drop);
    }
}

/// Load the persisted burndown history for one provider+account into the
/// process-local store, so history survives a restart. Runs once per scope;
/// later calls are cheap no-ops (the loaded marker is kept in the store).
pub fn load_persisted_history(
    provider_id: &str,
    account_key: Option<&str>,
) -> Vec<PlanUtilizationSeriesHistory> {
    static LOADED: LazyLock<Mutex<HashSet<ForecastScope>>> =
        LazyLock::new(|| Mutex::new(HashSet::new()));
    let scope = ForecastScope::new(provider_id, account_key);
    if let Ok(guard) = LOADED.lock()
        && guard.contains(&scope)
    {
        return global_history_store()
            .lock()
            .ok()
            .map(|store| store.histories(&scope))
            .unwrap_or_default();
    }
    let series = quota_burndown::load_persisted_series(provider_id, account_key);
    if let Ok(mut loaded) = LOADED.lock() {
        loaded.insert(scope.clone());
    }
    if let Ok(mut store) = global_history_store().lock() {
        store.adopt_persisted(&scope, &series);
    }
    global_history_store()
        .lock()
        .ok()
        .map(|store| store.histories(&scope))
        .unwrap_or_default()
}

/// Global in-process history used by Claude/Codex snapshot refresh.
pub fn global_history_store() -> &'static Mutex<SessionEquivalentHistoryStore> {
    &HISTORY_STORE
}

/// Record session/weekly windows from a live provider usage snapshot.
///
/// `account_key` scopes the history so switching accounts on one provider does not
/// blend burn observations across plans.
pub fn record_provider_windows(
    provider_id: &str,
    account_key: Option<&str>,
    session: &crate::core::RateWindow,
    weekly: Option<&crate::core::RateWindow>,
    now: DateTime<Utc>,
) {
    let Some(session_entry) = history_entry(session, SESSION_WINDOW_MINUTES, now) else {
        return;
    };
    let weekly_entry = weekly.and_then(|w| history_entry(w, WEEKLY_WINDOW_MINUTES, now));

    if let Ok(mut guard) = global_history_store().lock() {
        guard.record(
            &ForecastScope::new(provider_id, account_key),
            Some(session_entry.clone()),
            weekly_entry.clone(),
            SessionEquivalentBurnEstimator::DEFAULT_SAMPLE_LIMIT,
        );
    }

    // Best-effort persistence for the burndown chart (upstream #4085 keeps a
    // durable store; a failed write only costs the chart, never the forecast).
    if let Err(error) = quota_burndown::persist_recorded_windows(
        provider_id,
        account_key,
        Some(&session_entry),
        weekly_entry.as_ref(),
        now,
    ) {
        tracing::debug!(%error, "quota burndown persistence failed");
    }
}

/// A clamped observation of `window`, or `None` when it is informational, has
/// another length than `minutes`, or carries a non-finite percent.
fn history_entry(
    window: &crate::core::RateWindow,
    minutes: u32,
    now: DateTime<Utc>,
) -> Option<PlanUtilizationHistoryEntry> {
    if window.is_informational
        || window.window_minutes != Some(minutes)
        || !window.used_percent.is_finite()
    {
        return None;
    }
    Some(PlanUtilizationHistoryEntry {
        captured_at: now,
        used_percent: window.used_percent.clamp(0.0, 100.0),
        resets_at: window.resets_at,
    })
}

/// Last learned full-session burn estimate retained across idle refreshes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RetainedFullSessionEstimate {
    pub estimate: f64,
    pub updated_at: DateTime<Utc>,
}

/// Keep the last positive full-session estimate when a refresh yields none.
///
/// `fresh` wins when present and valid; otherwise `previous` is kept so the UI
/// does not blank while the session window is idle (upstream #2336).
pub fn retain_last_full_session_estimate(
    previous: Option<RetainedFullSessionEstimate>,
    fresh: Option<f64>,
    now: DateTime<Utc>,
) -> Option<RetainedFullSessionEstimate> {
    match fresh {
        Some(estimate) if estimate.is_finite() && estimate > 0.0 => {
            Some(RetainedFullSessionEstimate {
                estimate,
                updated_at: now,
            })
        }
        _ => previous,
    }
}

// ponytail: in-memory only, lost on restart; upgrade = persist to cache file.
#[derive(Debug, Default)]
struct LastFullSessionEstimateStore {
    by_scope: HashMap<ForecastScope, RetainedFullSessionEstimate>,
}

static LAST_FULL_SESSION_ESTIMATE_STORE: LazyLock<Mutex<LastFullSessionEstimateStore>> =
    LazyLock::new(|| Mutex::new(LastFullSessionEstimateStore::default()));

fn last_full_session_estimate_store() -> &'static Mutex<LastFullSessionEstimateStore> {
    &LAST_FULL_SESSION_ESTIMATE_STORE
}

/// Remember/recall the last learned full-session burn estimate for a provider+account.
pub fn remember_full_session_estimate(
    scope: &ForecastScope,
    fresh: Option<f64>,
    now: DateTime<Utc>,
) -> Option<f64> {
    let Ok(mut guard) = last_full_session_estimate_store().lock() else {
        return fresh.filter(|v| v.is_finite() && *v > 0.0);
    };
    let previous = guard.by_scope.get(scope).copied();
    let retained = retain_last_full_session_estimate(previous, fresh, now);
    if let Some(entry) = retained {
        guard.by_scope.insert(scope.clone(), entry);
        Some(entry.estimate)
    } else {
        guard.by_scope.remove(scope);
        None
    }
}

/// Compute forecast for a provider+account using the in-process history ring.
pub fn forecast_for_provider(
    provider_id: &str,
    account_key: Option<&str>,
    session: &crate::core::RateWindow,
    weekly: &crate::core::RateWindow,
    now: DateTime<Utc>,
    work_days: Option<u8>,
) -> Option<SessionEquivalentForecast> {
    let scope = ForecastScope::new(provider_id, account_key);
    let histories = global_history_store()
        .lock()
        .ok()
        .map(|g| g.histories(&scope))
        .unwrap_or_default();
    let fresh_burn = SessionEquivalentBurnEstimator::estimate(
        &histories,
        session.resets_at,
        now,
        SessionEquivalentBurnEstimator::DEFAULT_SAMPLE_LIMIT,
    );
    let fresh_median = fresh_burn
        .as_ref()
        .map(|b| b.median_weekly_percent_per_window);
    let sample_count = fresh_burn
        .as_ref()
        .map(|b| b.sample_count)
        .unwrap_or(SessionEquivalentBurnEstimator::MINIMUM_SAMPLE_COUNT);
    let median = remember_full_session_estimate(&scope, fresh_median, now)?;
    let burn = SessionEquivalentBurnEstimate {
        median_weekly_percent_per_window: median,
        sample_count,
    };
    SessionEquivalentForecast::make(session, weekly, &burn, None, now, work_days)
}
