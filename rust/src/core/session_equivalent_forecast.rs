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

mod history;
pub use history::*;

#[cfg(test)]
mod tests;
