//! Session-equivalent weekly forecast.
//!
//! Estimates how many full 5-hour session quotas remain in the weekly window
//! from recent plan-utilization burn history (upstream SessionEquivalentForecast).

use chrono::{DateTime, Datelike, Duration, Utc, Weekday};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::{LazyLock, Mutex};

use super::quota_burndown::{self, PersistedPlanEntry as PersistedEntry};

/// Canonical session window length (5 hours).
pub const SESSION_WINDOW_MINUTES: u32 = 300;
/// Canonical weekly window length (7 days).
pub const WEEKLY_WINDOW_MINUTES: u32 = 10_080;
/// Canonical monthly window length (30 days). Upstream 0.48.0 F5 adds a
/// 30-day ("monthly") rate lane alongside session (5h) and weekly (7d).
pub const MONTHLY_WINDOW_MINUTES: u32 = 43_200;
/// Reset-boundary grouping tolerance.
pub const RESET_TOLERANCE_SECS: i64 = 120;

/// Median weekly-percent burn observed per completed session window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SessionEquivalentBurnEstimate {
    pub median_weekly_percent_per_window: f64,
    pub sample_count: usize,
}

/// Forecast of remaining session-equivalent windows inside the weekly quota.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionEquivalentForecast {
    pub estimated_windows_to_exhaust_weekly: f64,
    pub windows_until_reset: i64,
    pub available_windows_until_reset: f64,
    pub sample_count: usize,
    pub weekly_resets_at: DateTime<Utc>,
    pub weekly_used_percent: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weekly_window_id: Option<String>,
}

/// One captured plan-utilization observation.
#[derive(Debug, Clone, PartialEq)]
pub struct PlanUtilizationHistoryEntry {
    pub captured_at: DateTime<Utc>,
    pub used_percent: f64,
    pub resets_at: Option<DateTime<Utc>>,
}

/// Named plan-utilization series used by the burn estimator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlanUtilizationSeriesName {
    Session,
    Weekly,
}

impl PlanUtilizationSeriesName {}

/// Chronological history for one series.
#[derive(Debug, Clone, PartialEq)]
pub struct PlanUtilizationSeriesHistory {
    pub name: PlanUtilizationSeriesName,
    pub window_minutes: u32,
    pub entries: Vec<PlanUtilizationHistoryEntry>,
}

/// Burn-estimator knobs matching upstream SessionEquivalentBurnEstimator.
pub struct SessionEquivalentBurnEstimator;

impl SessionEquivalentBurnEstimator {
    pub const DEFAULT_SAMPLE_LIMIT: usize = 7;
    pub const MINIMUM_SAMPLE_COUNT: usize = 3;

    /// Estimate median full-session weekly burn from plan-utilization histories.
    pub fn estimate(
        histories: &[PlanUtilizationSeriesHistory],
        current_session_resets_at: Option<DateTime<Utc>>,
        now: DateTime<Utc>,
        sample_limit: usize,
    ) -> Option<SessionEquivalentBurnEstimate> {
        if sample_limit == 0 {
            return None;
        }

        let session_history = histories.iter().find(|h| {
            h.name == PlanUtilizationSeriesName::Session
                && h.window_minutes == SESSION_WINDOW_MINUTES
        })?;
        let weekly_history = histories.iter().find(|h| {
            h.name == PlanUtilizationSeriesName::Weekly && h.window_minutes == WEEKLY_WINDOW_MINUTES
        })?;

        let session_duration = Duration::seconds(i64::from(SESSION_WINDOW_MINUTES) * 60);
        let weekly_duration = Duration::seconds(i64::from(WEEKLY_WINDOW_MINUTES) * 60);

        if !is_chronologically_ordered(&session_history.entries)
            || !is_chronologically_ordered(&weekly_history.entries)
        {
            return None;
        }

        // Expired/invalid-remaining current session = idle: don't gate the burn
        // estimate on it. Implausible far-future resets still abort.
        let effective_current = match current_session_resets_at {
            None => None,
            Some(current) => {
                let remaining = (current - now).num_seconds() as f64;
                let max_remaining =
                    session_duration.num_seconds() as f64 + RESET_TOLERANCE_SECS as f64;
                if !remaining.is_finite() || remaining > max_remaining {
                    return None;
                }
                if remaining <= 0.0 {
                    None
                } else {
                    Some(current)
                }
            }
        };

        let mut groups: Vec<SessionGroup> = Vec::new();
        for entry in &session_history.entries {
            if !entry.used_percent.is_finite() || !(0.0..=100.0).contains(&entry.used_percent) {
                continue;
            }
            let Some(resets_at) = entry.resets_at else {
                continue;
            };
            if !is_plausible_reset(resets_at, entry.captured_at, session_duration) {
                continue;
            }

            if let Some(last) = groups.last_mut() {
                let delta = (last.resets_at - resets_at).num_seconds().abs();
                if delta <= RESET_TOLERANCE_SECS {
                    last.entries.push(entry.clone());
                    last.maximum_used_percent = last.maximum_used_percent.max(entry.used_percent);
                    continue;
                }
                if last.resets_at > resets_at {
                    return None;
                }
            }

            groups.push(SessionGroup {
                resets_at,
                entries: vec![entry.clone()],
                maximum_used_percent: entry.used_percent,
            });
        }

        let completed_active_groups: Vec<&SessionGroup> = groups
            .iter()
            .rev()
            .filter(|group| {
                let precedes_current = effective_current
                    .map(|cur| group.resets_at < cur - Duration::seconds(RESET_TOLERANCE_SECS))
                    .unwrap_or(true);
                precedes_current && group.resets_at <= now && group.maximum_used_percent > 0.0
            })
            .collect();

        let weekly_entries: Vec<&PlanUtilizationHistoryEntry> = weekly_history
            .entries
            .iter()
            .filter(|entry| {
                entry.used_percent.is_finite()
                    && (0.0..=100.0).contains(&entry.used_percent)
                    && entry
                        .resets_at
                        .is_some_and(|r| is_plausible_reset(r, entry.captured_at, weekly_duration))
            })
            .collect();
        if weekly_entries.is_empty() {
            return None;
        }

        let mut burns = Vec::new();
        for group in completed_active_groups.into_iter().take(sample_limit) {
            if let Some(burn) = normalized_burn(group, &weekly_entries, session_duration) {
                burns.push(burn);
            }
        }

        if burns.len() < Self::MINIMUM_SAMPLE_COUNT {
            return None;
        }
        burns.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let middle = burns.len() / 2;
        let median = if burns.len().is_multiple_of(2) {
            (burns[middle - 1] + burns[middle]) / 2.0
        } else {
            burns[middle]
        };
        if !median.is_finite() || median <= 0.0 {
            return None;
        }

        Some(SessionEquivalentBurnEstimate {
            median_weekly_percent_per_window: median,
            sample_count: burns.len(),
        })
    }
}

impl SessionEquivalentForecast {
    /// Build a forecast from current windows + burn estimate.
    pub fn make(
        session_window: &crate::core::RateWindow,
        weekly_window: &crate::core::RateWindow,
        burn_estimate: &SessionEquivalentBurnEstimate,
        weekly_window_id: Option<String>,
        now: DateTime<Utc>,
        work_days: Option<u8>,
    ) -> Option<Self> {
        if session_window.is_informational {
            return None;
        }
        if session_window.window_minutes != Some(SESSION_WINDOW_MINUTES) {
            return None;
        }
        if weekly_window.window_minutes != Some(WEEKLY_WINDOW_MINUTES) {
            return None;
        }
        let weekly_resets_at = weekly_window.resets_at?;
        if !weekly_window.used_percent.is_finite()
            || !(0.0..=100.0).contains(&weekly_window.used_percent)
        {
            return None;
        }
        if !burn_estimate.median_weekly_percent_per_window.is_finite()
            || burn_estimate.median_weekly_percent_per_window <= 0.0
            || burn_estimate.sample_count < SessionEquivalentBurnEstimator::MINIMUM_SAMPLE_COUNT
        {
            return None;
        }

        let session_seconds = f64::from(SESSION_WINDOW_MINUTES) * 60.0;
        let weekly_seconds = f64::from(WEEKLY_WINDOW_MINUTES) * 60.0;

        // Expired session resets mean the window is idle — still show the learned
        // estimate against the weekly lane. Only reject implausible far-future resets.
        if let Some(session_resets_at) = session_window.resets_at {
            let session_remaining = (session_resets_at - now).num_seconds() as f64;
            if !session_remaining.is_finite()
                || session_remaining > session_seconds + RESET_TOLERANCE_SECS as f64
            {
                return None;
            }
        }

        let weekly_remaining = (weekly_resets_at - now).num_seconds() as f64;
        if !weekly_remaining.is_finite()
            || weekly_remaining <= 0.0
            || weekly_remaining > weekly_seconds + RESET_TOLERANCE_SECS as f64
        {
            return None;
        }

        let remaining_weekly_percent = (100.0 - weekly_window.used_percent).clamp(0.0, 100.0);
        if remaining_weekly_percent <= 0.0 {
            return None;
        }
        let estimated_windows =
            remaining_weekly_percent / burn_estimate.median_weekly_percent_per_window;
        if !estimated_windows.is_finite() || estimated_windows < 0.0 {
            return None;
        }

        let remaining_seconds = effective_remaining_seconds(now, weekly_resets_at, work_days);
        if remaining_seconds < 0.0 {
            return None;
        }
        let available_windows_until_reset = remaining_seconds / session_seconds;
        // Whole-window count by design; the fractional window is dropped.
        #[allow(
            clippy::cast_possible_truncation,
            reason = "window count is a small whole number; fractional part dropped by design"
        )]
        let windows_until_reset = available_windows_until_reset.floor() as i64;

        Some(Self {
            estimated_windows_to_exhaust_weekly: estimated_windows,
            windows_until_reset,
            available_windows_until_reset,
            sample_count: burn_estimate.sample_count,
            weekly_resets_at,
            weekly_used_percent: weekly_window.used_percent,
            weekly_window_id,
        })
    }
}

#[derive(Debug, Clone)]
struct SessionGroup {
    resets_at: DateTime<Utc>,
    entries: Vec<PlanUtilizationHistoryEntry>,
    maximum_used_percent: f64,
}

#[derive(Debug, Clone)]
struct BurnObservation {
    session_used_percent: f64,
    weekly_entry: PlanUtilizationHistoryEntry,
}

fn normalized_burn(
    group: &SessionGroup,
    weekly_entries: &[&PlanUtilizationHistoryEntry],
    session_duration: Duration,
) -> Option<f64> {
    let first_session = group.entries.first()?;
    let last_session = group.entries.last()?;

    let mut observations: Vec<BurnObservation> = Vec::new();
    let window_start = group.resets_at - session_duration;

    if let Some(weekly_start) = nearest_entry(
        window_start,
        weekly_entries,
        Duration::seconds(RESET_TOLERANCE_SECS),
        true,
    ) && weekly_start.captured_at <= window_start
        && weekly_start.captured_at < first_session.captured_at
    {
        observations.push(BurnObservation {
            session_used_percent: 0.0,
            weekly_entry: weekly_start.clone(),
        });
    }

    for session_entry in &group.entries {
        // Upstream observationAlignmentTolerance is 0 → exact capture match only.
        if let Some(weekly_entry) = nearest_entry(
            session_entry.captured_at,
            weekly_entries,
            Duration::zero(),
            false,
        ) {
            observations.push(BurnObservation {
                session_used_percent: session_entry.used_percent,
                weekly_entry: weekly_entry.clone(),
            });
        }
    }

    if group.maximum_used_percent >= 100.0
        && let Some(weekly_end) = nearest_entry(
            group.resets_at,
            weekly_entries,
            Duration::seconds(RESET_TOLERANCE_SECS),
            true,
        )
        && weekly_end.captured_at <= group.resets_at
        && last_session.captured_at < weekly_end.captured_at
    {
        observations.push(BurnObservation {
            session_used_percent: 100.0,
            weekly_entry: weekly_end.clone(),
        });
    }

    observations.sort_by(|lhs, rhs| {
        match lhs
            .weekly_entry
            .captured_at
            .cmp(&rhs.weekly_entry.captured_at)
        {
            std::cmp::Ordering::Equal => lhs
                .session_used_percent
                .partial_cmp(&rhs.session_used_percent)
                .unwrap_or(std::cmp::Ordering::Equal),
            other => other,
        }
    });

    let start = observations.first()?;
    let end = observations.last()?;
    if start.weekly_entry.captured_at >= end.weekly_entry.captured_at {
        return None;
    }
    let start_reset = start.weekly_entry.resets_at?;
    let end_reset = end.weekly_entry.resets_at?;
    if (start_reset - end_reset).num_seconds().abs() > RESET_TOLERANCE_SECS {
        return None;
    }

    let session_consumption = end.session_used_percent - start.session_used_percent;
    let weekly_burn = end.weekly_entry.used_percent - start.weekly_entry.used_percent;
    if !session_consumption.is_finite()
        || session_consumption <= 0.0
        || !weekly_burn.is_finite()
        || weekly_burn <= 0.0
    {
        return None;
    }
    let full_allowance_burn = 100.0 * weekly_burn / session_consumption;
    if !full_allowance_burn.is_finite() || full_allowance_burn <= 0.0 {
        return None;
    }
    Some(full_allowance_burn)
}

fn nearest_entry<'a>(
    target: DateTime<Utc>,
    entries: &[&'a PlanUtilizationHistoryEntry],
    tolerance: Duration,
    require_not_after_target: bool,
) -> Option<&'a PlanUtilizationHistoryEntry> {
    if entries.is_empty() {
        return None;
    }
    let mut lower = 0usize;
    let mut upper = entries.len();
    while lower < upper {
        let middle = (lower + upper) / 2;
        if entries[middle].captured_at < target {
            lower = middle + 1;
        } else {
            upper = middle;
        }
    }

    let mut candidates = Vec::new();
    if lower < entries.len() {
        candidates.push(entries[lower]);
    }
    if lower > 0 {
        candidates.push(entries[lower - 1]);
    }

    let tol_secs = tolerance.num_seconds().abs();
    candidates
        .into_iter()
        .filter(|e| !require_not_after_target || e.captured_at <= target)
        .filter(|e| (e.captured_at - target).num_seconds().abs() <= tol_secs)
        .min_by_key(|e| (e.captured_at - target).num_seconds().abs())
}

fn is_chronologically_ordered(entries: &[PlanUtilizationHistoryEntry]) -> bool {
    entries
        .windows(2)
        .all(|w| w[0].captured_at <= w[1].captured_at)
}

fn is_plausible_reset(
    resets_at: DateTime<Utc>,
    captured_at: DateTime<Utc>,
    duration: Duration,
) -> bool {
    let remaining = (resets_at - captured_at).num_seconds();
    remaining >= -RESET_TOLERANCE_SECS && remaining <= duration.num_seconds() + RESET_TOLERANCE_SECS
}

/// Remaining seconds until weekly reset, optionally counting only work days.
///
/// `work_days` in `[2, 6]` keeps ISO weekdays `1..=work_days` (Mon..).
/// Anything else falls back to wall-clock remaining time.
pub fn effective_remaining_seconds(
    now: DateTime<Utc>,
    resets_at: DateTime<Utc>,
    work_days: Option<u8>,
) -> f64 {
    let wall = (resets_at - now).num_seconds().max(0) as f64;
    let Some(work_days) = work_days else {
        return wall;
    };
    if !(2..=6).contains(&work_days) {
        return wall;
    }

    let mut work_seconds = 0.0_f64;
    let mut cursor = now;
    while cursor < resets_at {
        let next_day = match start_of_next_utc_day(cursor) {
            Some(d) if d > cursor => d,
            _ => return wall,
        };
        let slice_end = if next_day < resets_at {
            next_day
        } else {
            resets_at
        };
        if is_workday(cursor, work_days) {
            work_seconds += (slice_end - cursor).num_seconds() as f64;
        }
        cursor = slice_end;
    }
    work_seconds
}

fn start_of_next_utc_day(dt: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let date = dt.date_naive() + Duration::days(1);
    date.and_hms_opt(0, 0, 0)
        .map(|naive| DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc))
}

fn is_workday(date: DateTime<Utc>, work_days: u8) -> bool {
    let iso = match date.weekday() {
        Weekday::Mon => 1,
        Weekday::Tue => 2,
        Weekday::Wed => 3,
        Weekday::Thu => 4,
        Weekday::Fri => 5,
        Weekday::Sat => 6,
        Weekday::Sun => 7,
    };
    iso <= work_days
}

// ── In-process history store ─────────────────────────────────────────

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

#[cfg(test)]
mod tests;
