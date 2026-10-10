//! Claude local usage projected onto observed weekly quota windows.
//!
//! This module is deliberately separate from `UsageSnapshot`: quota history is
//! display data and must never become a source for current quota, readiness,
//! notifications, pacing, or account rotation.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const CLAUDE_QUOTA_WEEK_MINUTES: i64 = 7 * 24 * 60;
const RESET_TOLERANCE_SECONDS: i64 = 120;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaudeQuotaDedupKey {
    Request {
        message_id: Option<String>,
        request_id: String,
    },
    Session {
        session_id: String,
        message_id: String,
    },
}

/// One final Claude request row. Token and cost completeness are independent:
/// a known token subtotal may coexist with an unknown cost, and vice versa.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub enum ClaudeHistoryAttribution {
    Account(String),
    #[default]
    Unavailable,
}

impl ClaudeHistoryAttribution {
    pub fn matches_account(&self, account_scope: &str) -> bool {
        matches!(self, Self::Account(scope) if scope == account_scope)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClaudeQuotaHistoryRecord {
    pub timestamp: DateTime<Utc>,
    pub tokens: Option<u64>,
    pub cost_usd: Option<f64>,
    pub tokens_are_complete: bool,
    pub cost_is_complete: bool,
    #[serde(default)]
    pub dedup_key: Option<ClaudeQuotaDedupKey>,
    #[serde(default)]
    pub attribution: ClaudeHistoryAttribution,
}

impl ClaudeQuotaHistoryRecord {
    fn completeness_score(&self) -> (bool, bool, bool, bool) {
        (
            self.tokens_are_complete,
            self.cost_is_complete,
            self.tokens.is_some(),
            self.cost_usd.is_some(),
        )
    }
}

/// A reset forecast observed while reading one Claude account.
///
/// A forecast is not itself a reset event. When a later observation is made
/// after the forecast, the forecasted instant becomes an exact boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaudeQuotaResetObservation {
    pub account_scope: String,
    pub captured_at: DateTime<Utc>,
    pub resets_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClaudeQuotaWindow {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub total_tokens: Option<u64>,
    pub total_cost_usd: Option<f64>,
    pub tokens_are_complete: bool,
    pub cost_is_complete: bool,
    pub entry_count: u32,
    pub boundaries_are_estimated: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClaudeQuotaHistoryReport {
    pub account_scope: String,
    pub windows: Vec<ClaudeQuotaWindow>,
    pub history_coverage_established: bool,
}

/// Inputs that control projection of Claude request history onto weekly
/// windows. Keeping these together prevents the projection API from becoming
/// an untyped list of reset and coverage flags.
#[derive(Debug, Clone, Copy)]
pub struct ClaudeQuotaHistoryOptions<'a> {
    pub live_reset_at: Option<DateTime<Utc>>,
    pub window_minutes: Option<u32>,
    pub observations: &'a [ClaudeQuotaResetObservation],
    pub now: DateTime<Utc>,
    pub max_windows: usize,
    pub history_coverage_established: bool,
}

pub fn deduplicate_claude_records(
    mut records: Vec<ClaudeQuotaHistoryRecord>,
) -> Vec<ClaudeQuotaHistoryRecord> {
    records.sort_by(|left, right| {
        left.timestamp
            .cmp(&right.timestamp)
            .then_with(|| left.dedup_key.cmp(&right.dedup_key))
            .then_with(|| left.completeness_score().cmp(&right.completeness_score()))
    });

    let mut keyed = BTreeMap::<ClaudeQuotaDedupKey, ClaudeQuotaHistoryRecord>::new();
    let mut unkeyed = Vec::new();
    for record in records {
        let Some(key) = record.dedup_key.clone() else {
            unkeyed.push(record);
            continue;
        };
        match keyed.get(&key) {
            Some(existing) if existing.completeness_score() >= record.completeness_score() => {}
            _ => {
                keyed.insert(key, record);
            }
        }
    }

    let mut result = keyed.into_values().chain(unkeyed).collect::<Vec<_>>();
    result.sort_by(|left, right| {
        left.timestamp
            .cmp(&right.timestamp)
            .then_with(|| left.dedup_key.cmp(&right.dedup_key))
    });
    result
}

/// Build recent Claude weekly windows from exact request timestamps.
///
/// The returned order is newest window first. Reset observations are filtered
/// by `account_scope`; no current provider snapshot is accepted or modified.
pub fn aggregate_claude_quota_windows(
    account_scope: impl Into<String>,
    records: &[ClaudeQuotaHistoryRecord],
    options: ClaudeQuotaHistoryOptions<'_>,
) -> ClaudeQuotaHistoryReport {
    let ClaudeQuotaHistoryOptions {
        live_reset_at,
        window_minutes,
        observations,
        now,
        max_windows,
        history_coverage_established,
    } = options;
    let account_scope = account_scope.into();
    let empty = |account_scope| ClaudeQuotaHistoryReport {
        account_scope,
        windows: Vec::new(),
        history_coverage_established,
    };
    let count = max_windows.clamp(1, 8);
    let duration_minutes = normalized_window_minutes(window_minutes);
    let duration = Duration::minutes(duration_minutes);
    let Some(live_reset_at) = live_reset_at.filter(|reset| reset.timestamp_millis() > 0) else {
        return empty(account_scope);
    };

    let evidence = ResetEvidence::new(&account_scope, observations, now);
    if evidence.is_cancelled(live_reset_at) {
        return empty(account_scope);
    }
    let current_end = current_window_end(live_reset_at, now, duration);
    let boundaries = quota_boundaries(current_end, duration, &evidence, count);
    let attributed_records = records
        .iter()
        .filter(|record| record.attribution.matches_account(&account_scope))
        .cloned()
        .collect::<Vec<_>>();
    // A current account identity does not prove ownership of historical
    // transcript rows. Keep the account-scoped surface absent until a source
    // supplies explicit attribution for at least one row.
    if attributed_records.is_empty() {
        return empty(account_scope);
    }
    let deduped = deduplicate_claude_records(attributed_records);

    let mut windows = Vec::with_capacity(count);
    for pair in boundaries.windows(2).rev().take(count) {
        let start = pair[0];
        let end = pair[1];
        let mut totals = Totals::default();
        let mut entry_count = 0_u32;
        for record in deduped
            .iter()
            .filter(|record| record.timestamp >= start && record.timestamp < end)
        {
            entry_count = entry_count.saturating_add(1);
            totals.add(record);
        }
        windows.push(ClaudeQuotaWindow {
            start,
            end,
            total_tokens: totals.tokens,
            total_cost_usd: totals.cost,
            tokens_are_complete: totals.tokens_complete
                && totals.tokens.is_some()
                && history_coverage_established,
            cost_is_complete: totals.cost_complete
                && totals.cost.is_some()
                && history_coverage_established,
            entry_count,
            boundaries_are_estimated: !evidence.confirms(start)
                || !evidence.confirms(end)
                || end == current_end,
        });
    }

    ClaudeQuotaHistoryReport {
        account_scope,
        windows,
        history_coverage_established,
    }
}

pub fn normalized_window_minutes(window_minutes: Option<u32>) -> i64 {
    let week = CLAUDE_QUOTA_WEEK_MINUTES;
    match window_minutes.map(i64::from) {
        Some(minutes) if (week - 24 * 60..=week + 24 * 60).contains(&minutes) => minutes,
        _ => week,
    }
}

#[derive(Debug)]
struct Totals {
    tokens: Option<u64>,
    tokens_valid: bool,
    tokens_complete: bool,
    cost: Option<f64>,
    cost_valid: bool,
    cost_complete: bool,
    saw_tokens: bool,
    saw_cost: bool,
}

impl Default for Totals {
    fn default() -> Self {
        Self {
            tokens: None,
            tokens_valid: true,
            tokens_complete: true,
            cost: None,
            cost_valid: true,
            cost_complete: true,
            saw_tokens: false,
            saw_cost: false,
        }
    }
}

impl Totals {
    fn add(&mut self, record: &ClaudeQuotaHistoryRecord) {
        self.tokens_complete &= record.tokens_are_complete && record.tokens.is_some();
        if let Some(tokens) = record.tokens {
            self.saw_tokens = true;
            if self.tokens_valid {
                self.tokens = match self.tokens {
                    None => Some(tokens),
                    Some(total) => match total.checked_add(tokens) {
                        Some(sum) => Some(sum),
                        None => {
                            self.tokens_valid = false;
                            self.tokens_complete = false;
                            None
                        }
                    },
                };
                if !self.tokens_valid {
                    self.tokens = None;
                }
            }
        }

        self.cost_complete &= record.cost_is_complete && record.cost_usd.is_some();
        if self.cost_valid {
            let Some(cost) = record.cost_usd else {
                return;
            };
            self.saw_cost = true;
            if !cost.is_finite() || cost < 0.0 {
                self.cost = None;
                self.cost_valid = false;
                self.cost_complete = false;
            } else {
                let total = self.cost.unwrap_or(0.0) + cost;
                if total.is_finite() {
                    self.cost = Some(total);
                } else {
                    self.cost = None;
                    self.cost_valid = false;
                    self.cost_complete = false;
                }
            }
        }
    }
}

struct ResetEvidence {
    exact: BTreeSet<DateTime<Utc>>,
    cancelled: Vec<DateTime<Utc>>,
}

impl ResetEvidence {
    fn new(
        account_scope: &str,
        observations: &[ClaudeQuotaResetObservation],
        now: DateTime<Utc>,
    ) -> Self {
        let mut sorted = observations
            .iter()
            .filter(|observation| {
                observation.account_scope == account_scope
                    && observation.captured_at <= now
                    && observation.resets_at > observation.captured_at
            })
            .cloned()
            .collect::<Vec<_>>();
        sorted.sort_by_key(|observation| observation.captured_at);

        let mut exact = BTreeSet::new();
        let mut cancelled = Vec::new();
        for pair in sorted.windows(2) {
            let earlier = &pair[0];
            let later = &pair[1];
            if later.captured_at >= earlier.resets_at {
                exact.insert(earlier.resets_at);
            } else if later
                .resets_at
                .signed_duration_since(earlier.resets_at)
                .abs()
                >= Duration::seconds(RESET_TOLERANCE_SECONDS)
            {
                cancelled.push(earlier.resets_at);
            }
        }
        Self { exact, cancelled }
    }

    fn confirms(&self, instant: DateTime<Utc>) -> bool {
        self.exact.contains(&instant)
    }

    fn is_cancelled(&self, instant: DateTime<Utc>) -> bool {
        self.cancelled.iter().any(|candidate| {
            (candidate.timestamp_millis() - instant.timestamp_millis()).abs()
                < RESET_TOLERANCE_SECONDS * 1000
        })
    }
}

fn current_window_end(
    live_reset_at: DateTime<Utc>,
    now: DateTime<Utc>,
    duration: Duration,
) -> DateTime<Utc> {
    if live_reset_at > now {
        return live_reset_at;
    }
    let elapsed = now.signed_duration_since(live_reset_at).num_seconds();
    let period = duration.num_seconds().max(1);
    let steps = elapsed.div_euclid(period) + 1;
    live_reset_at + Duration::seconds(steps * period)
}

fn quota_boundaries(
    current_end: DateTime<Utc>,
    duration: Duration,
    evidence: &ResetEvidence,
    count: usize,
) -> Vec<DateTime<Utc>> {
    let count_i32 = i32::try_from(count).expect("quota window count is capped");
    let earliest = current_end - duration * (count_i32 + 2);
    let mut boundaries = BTreeSet::new();
    boundaries.insert(current_end);
    boundaries.insert(current_end - duration);
    for &reset in &evidence.exact {
        if reset >= earliest && reset < current_end {
            boundaries.insert(reset);
        }
        let nominal_start = reset - duration;
        if nominal_start >= earliest && nominal_start < current_end {
            boundaries.insert(nominal_start);
        }
    }
    let mut sorted = boundaries.into_iter().collect::<Vec<_>>();
    sorted.sort();
    let mut filled = Vec::with_capacity(sorted.len() + count + 2);
    for boundary in sorted {
        if let Some(previous) = filled.last().copied() {
            let mut cursor = previous;
            while boundary.signed_duration_since(cursor)
                > duration + Duration::seconds(RESET_TOLERANCE_SECONDS)
            {
                cursor += duration;
                filled.push(cursor);
            }
        }
        if filled.last().copied() != Some(boundary) && !evidence.is_cancelled(boundary) {
            filled.push(boundary);
        }
    }
    while filled.len() < count + 1 {
        let oldest = filled.first().copied().unwrap_or(current_end);
        filled.insert(0, oldest - duration);
    }
    filled
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(value: &str) -> DateTime<Utc> {
        value.parse().expect("RFC3339 timestamp")
    }

    /// A row whose tokens and cost are complete exactly when present.
    fn record(
        at: &str,
        key: Option<ClaudeQuotaDedupKey>,
        tokens: Option<u64>,
        cost: Option<f64>,
    ) -> ClaudeQuotaHistoryRecord {
        ClaudeQuotaHistoryRecord {
            timestamp: ts(at),
            tokens,
            cost_usd: cost,
            tokens_are_complete: tokens.is_some(),
            cost_is_complete: cost.is_some(),
            dedup_key: key,
            attribution: ClaudeHistoryAttribution::Account("account-a".into()),
        }
    }

    /// One closed weekly window ending at the 2026-09-22 reset.
    fn options() -> ClaudeQuotaHistoryOptions<'static> {
        ClaudeQuotaHistoryOptions {
            live_reset_at: Some(ts("2026-09-22T12:00:00Z")),
            window_minutes: None,
            observations: &[],
            now: ts("2026-09-21T12:00:00Z"),
            max_windows: 1,
            history_coverage_established: true,
        }
    }

    #[test]
    fn reset_observations_are_account_scoped_and_rollovers_are_exact() {
        let now = ts("2026-09-21T12:00:00Z");
        let live_reset = ts("2026-09-22T12:00:00Z");
        let observations = vec![
            ClaudeQuotaResetObservation {
                account_scope: "account-a".into(),
                captured_at: ts("2026-09-14T10:00:00Z"),
                resets_at: ts("2026-09-15T12:00:00Z"),
            },
            ClaudeQuotaResetObservation {
                account_scope: "account-a".into(),
                captured_at: ts("2026-09-16T12:01:00Z"),
                resets_at: live_reset,
            },
            ClaudeQuotaResetObservation {
                account_scope: "account-b".into(),
                captured_at: ts("2026-09-16T12:01:00Z"),
                resets_at: ts("2026-09-23T12:00:00Z"),
            },
        ];
        let report = aggregate_claude_quota_windows(
            "account-a",
            &[record("2026-09-20T10:00:00Z", None, Some(1), Some(0.1))],
            ClaudeQuotaHistoryOptions {
                live_reset_at: Some(live_reset),
                window_minutes: Some(10_080),
                observations: &observations,
                now,
                max_windows: 3,
                ..options()
            },
        );
        assert_eq!(report.windows.len(), 3);
        assert_eq!(report.windows[1].end, ts("2026-09-15T12:00:00Z"));
        assert_eq!(report.windows[0].start, ts("2026-09-15T12:00:00Z"));
    }

    #[test]
    fn dedup_prefers_complete_row_and_preserves_request_message_session_keys() {
        let key = ClaudeQuotaDedupKey::Request {
            message_id: Some("msg-1".into()),
            request_id: "req-1".into(),
        };
        let records = deduplicate_claude_records(vec![
            record("2026-09-20T10:00:00Z", Some(key.clone()), Some(10), None),
            record("2026-09-20T10:00:00Z", Some(key), Some(20), Some(0.4)),
            record(
                "2026-09-20T10:01:00Z",
                Some(ClaudeQuotaDedupKey::Session {
                    session_id: "session-1".into(),
                    message_id: "message-1".into(),
                }),
                Some(3),
                Some(0.1),
            ),
        ]);
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].tokens, Some(20));
        assert_eq!(records[0].cost_usd, Some(0.4));
    }

    #[test]
    fn tokens_and_cost_keep_independent_partial_totals() {
        let report = aggregate_claude_quota_windows(
            "account-a",
            &[
                record("2026-09-20T10:00:00Z", None, Some(10), None),
                record("2026-09-20T11:00:00Z", None, None, Some(0.5)),
            ],
            options(),
        );
        let window = &report.windows[0];
        assert_eq!(window.total_tokens, Some(10));
        assert!(!window.tokens_are_complete);
        assert_eq!(window.total_cost_usd, Some(0.5));
        assert!(!window.cost_is_complete);
    }

    #[test]
    fn token_overflow_does_not_publish_a_partial_subtotal() {
        let report = aggregate_claude_quota_windows(
            "account-a",
            &[
                record("2026-09-20T10:00:00Z", None, Some(u64::MAX), None),
                record("2026-09-20T11:00:00Z", None, Some(1), None),
            ],
            options(),
        );

        let window = &report.windows[0];
        assert_eq!(window.total_tokens, None);
        assert!(!window.tokens_are_complete);
    }

    #[test]
    fn history_projection_does_not_mutate_any_live_state() {
        let before = ts("2026-09-22T12:00:00Z");
        let report = aggregate_claude_quota_windows(
            "account-a",
            &[],
            ClaudeQuotaHistoryOptions {
                live_reset_at: Some(before),
                window_minutes: Some(10_080),
                ..options()
            },
        );
        assert_eq!(before, ts("2026-09-22T12:00:00Z"));
        assert_eq!(report.account_scope, "account-a");
    }

    #[test]
    fn account_scoped_history_excludes_unavailable_and_mismatched_rows() {
        let mut unavailable = record("2026-09-20T10:00:00Z", None, Some(100), Some(1.0));
        unavailable.attribution = ClaudeHistoryAttribution::Unavailable;
        let mut other_account = unavailable.clone();
        other_account.attribution = ClaudeHistoryAttribution::Account("account-b".into());
        let mut matching = unavailable.clone();
        matching.attribution = ClaudeHistoryAttribution::Account("account-a".into());
        let report = aggregate_claude_quota_windows(
            "account-a",
            &[unavailable, other_account, matching],
            options(),
        );
        let window = &report.windows[0];
        assert_eq!(window.entry_count, 1);
        assert_eq!(window.total_tokens, Some(100));
        assert_eq!(window.total_cost_usd, Some(1.0));
    }

    #[test]
    fn account_scoped_history_is_absent_without_explicit_attribution() {
        let mut record = record("2026-09-20T10:00:00Z", None, Some(100), Some(1.0));
        record.attribution = ClaudeHistoryAttribution::Unavailable;
        let report = aggregate_claude_quota_windows("account-a", &[record], options());
        assert!(report.windows.is_empty());
    }

    #[test]
    fn legacy_rows_without_attribution_deserialize_as_unavailable() {
        let json = r#"{
            "timestamp":"2026-09-20T10:00:00Z",
            "tokens":1,
            "cost_usd":0.1,
            "tokens_are_complete":true,
            "cost_is_complete":true
        }"#;
        let record: ClaudeQuotaHistoryRecord = serde_json::from_str(json).unwrap();
        assert_eq!(record.attribution, ClaudeHistoryAttribution::Unavailable);
    }
}
