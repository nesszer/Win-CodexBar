//! Year of per-day token totals for the Usage & Spend token activity grid
//! (upstream `SpendDashboardModel.tokenActivity`).
//!
//! Each day keeps two facts apart: whether the scan reached it at all
//! (`is_scanned`) and whether its total is known (`total_tokens`). A scanned
//! day without a total is a real gap, never an implied zero.

use chrono::{Duration, NaiveDate, Utc};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Instant;

use super::claude_incomplete::{ClaudeIncompleteReport, ClaudeIncompleteTracker};
use super::claude_pricing::ClaudeScanPricingResolver;
use super::today::day_key;
use super::{
    ClaudeFileScanResult, CostScanner, CostSummary, add_claude_record_to_daily_tokens,
    add_codex_days_map_to_summary, scan_claude_file_with_pricing,
};
use crate::core::{CostScanOptions, CostUsageDayRange, JsonlScanner};
use crate::cost_reporting_period::cost_bucket_zone;

/// Directory under the cache root that holds the activity-only Codex cache.
const CODEX_ACTIVITY_CACHE_DIR: &str = "token-activity";

/// Days the annual grid covers, today included.
pub const TOKEN_ACTIVITY_DAYS: u32 = 365;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenActivityDay {
    pub day: String,
    /// `None` is unknown: the day was not scanned, or it was scanned and
    /// still cannot be totalled.
    pub total_tokens: Option<u64>,
    pub is_scanned: bool,
}

/// Per-day token activity for one local-log provider, oldest day first.
/// Providers without a supported local history get no days.
pub fn daily_token_activity(provider: &str, days: u32) -> Vec<TokenActivityDay> {
    let today = cost_bucket_zone().date(Utc::now());
    let keys = window_keys(today, days);
    match provider {
        "codex" => codex_token_activity(codex_activity_scanner(days, None), &keys),
        "claude" => claude_token_activity(days, &keys),
        _ => Vec::new(),
    }
}

fn window_keys(today: NaiveDate, days: u32) -> Vec<String> {
    (0..i64::from(days))
        .rev()
        .map(|days_ago| day_key(today - Duration::days(days_ago)))
        .collect()
}

/// Scanner for the year-long Codex activity walk.
///
/// Activity reads only per-day totals, so it skips the Pi/OMP mirror pass
/// (its summary was never read). It also keeps a Codex cache of its own under
/// `{root}/token-activity/`: the shared `codex-v1.json` records the coverage of
/// the 7/30-day cost scans, and a shorter scan there shrinks that coverage and
/// forces every file in a year to be re-parsed on the next refresh. Activity
/// writing into the same file would make both sides thrash and race.
fn codex_activity_scanner(days: u32, shared_cache_root: Option<&Path>) -> CostScanner {
    let scanner = CostScanner::new(days).with_options(CostScanOptions {
        include_pi_sessions: false,
        ..CostScanOptions::default()
    });
    let cache_root = shared_cache_root
        .map(Path::to_path_buf)
        .or_else(JsonlScanner::default_cache_root);
    match cache_root {
        Some(root) => scanner.with_cache_root(root.join(CODEX_ACTIVITY_CACHE_DIR)),
        None => scanner,
    }
}

fn codex_token_activity(scanner: CostScanner, keys: &[String]) -> Vec<TokenActivityDay> {
    let (_summary, _stats, cache) = scanner.scan_codex_detailed_with_cache(None);
    let mut totals = HashMap::new();
    for (key, models) in &cache.days {
        let Some(day) = CostUsageDayRange::parse_day_key(key) else {
            continue;
        };
        let mut one_day = HashMap::new();
        one_day.insert(key.clone(), models.clone());
        let mut scratch = CostSummary::default();
        add_codex_days_map_to_summary(&mut scratch, &one_day, &CostUsageDayRange::new(day, day));
        totals.insert(key.clone(), scratch.total_tokens_for_provider("codex"));
    }
    // A stale or still-catching-up cache proves nothing about the days it
    // has not reached, so it covers no day at all.
    let established = cache.previous_report.is_none() && !cache.codex_scan_incomplete;
    let coverage = established
        .then(|| {
            cache
                .scan_since_key
                .clone()
                .zip(cache.scan_until_key.clone())
        })
        .flatten();
    codex_days(keys, &totals, coverage.as_ref())
}

fn codex_days(
    keys: &[String],
    totals: &HashMap<String, u64>,
    coverage: Option<&(String, String)>,
) -> Vec<TokenActivityDay> {
    keys.iter()
        .map(|key| {
            let scanned = coverage.is_some_and(|(since, until)| key >= since && key <= until);
            TokenActivityDay {
                day: key.clone(),
                total_tokens: scanned.then(|| totals.get(key).copied().unwrap_or(0)),
                is_scanned: scanned,
            }
        })
        .collect()
}

/// Identity of one Claude year walk. The walk depends on the window length,
/// the local day it ends on, and the discovered project roots.
#[derive(Debug, PartialEq, Eq)]
struct ClaudeActivityKey {
    days: u32,
    today: String,
    roots: Vec<PathBuf>,
}

/// One-entry memo that reuses a value while its key matches and it is younger
/// than `ttl`.
struct TimedMemo<K, V> {
    entry: Option<(K, Instant, V)>,
}

impl<K: PartialEq, V: Clone> TimedMemo<K, V> {
    const fn new() -> Self {
        Self { entry: None }
    }

    fn get(&self, key: &K, now: Instant, ttl: std::time::Duration) -> Option<V> {
        let (stored_key, stored_at, value) = self.entry.as_ref()?;
        (stored_key == key && now.saturating_duration_since(*stored_at) <= ttl)
            .then(|| value.clone())
    }

    fn put(&mut self, key: K, stored_at: Instant, value: V) {
        self.entry = Some((key, stored_at, value));
    }
}

/// The Claude walk reads every transcript in the year and has no incremental
/// cache, so a complete result is reused for the cost scan debounce interval
/// (`CostScanOptions::default().refresh_min_interval_secs`, 60 s).
static CLAUDE_ACTIVITY_MEMO: Mutex<TimedMemo<ClaudeActivityKey, Vec<TokenActivityDay>>> =
    Mutex::new(TimedMemo::new());

fn claude_memo() -> MutexGuard<'static, TimedMemo<ClaudeActivityKey, Vec<TokenActivityDay>>> {
    CLAUDE_ACTIVITY_MEMO
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

fn claude_token_activity(days: u32, keys: &[String]) -> Vec<TokenActivityDay> {
    let scanner = CostScanner::new(days);
    let roots = scanner.claude_projects_roots();
    if roots.paths.is_empty() {
        return claude_days(keys, &HashMap::new(), &HashSet::new(), false);
    }
    let memo_key = ClaudeActivityKey {
        days,
        today: keys.last().cloned().unwrap_or_default(),
        roots: roots.paths.clone(),
    };
    let ttl = std::time::Duration::from_secs(CostScanOptions::default().refresh_min_interval_secs);
    let cached = claude_memo().get(&memo_key, Instant::now(), ttl);
    if let Some(cached) = cached {
        return cached;
    }
    let cutoff = Utc::now() - Duration::days(i64::from(days));
    let mut totals: HashMap<String, u64> = keys.iter().map(|key| (key.clone(), 0)).collect();
    let mut seen = HashSet::new();
    let mut pricing = ClaudeScanPricingResolver::default();
    let mut incomplete = ClaudeIncompleteTracker::default();
    let mut scan = ClaudeFileScanResult::default();
    let traversal_read_failures = {
        let mut handle_file = |path: &Path| {
            let mut aggregation_failures = 0u32;
            let mut result = scan_claude_file_with_pricing(
                path,
                &cutoff,
                &mut seen,
                None,
                &mut pricing,
                &mut incomplete,
                |record| {
                    if record.timestamp.is_none()
                        || !add_claude_record_to_daily_tokens(&mut totals, record)
                    {
                        aggregation_failures = aggregation_failures.saturating_add(1);
                    }
                },
            );
            result.aggregation_failures = result
                .aggregation_failures
                .saturating_add(aggregation_failures);
            scan.absorb(result);
        };
        scanner.walk_claude_roots(&roots.paths, &cutoff, None, &mut handle_file)
    };
    scan.read_failures = scan
        .read_failures
        .saturating_add(roots.read_failures)
        .saturating_add(traversal_read_failures);
    // Dated preliminary rows only invalidate the days they fall on, so they
    // are judged per day below. An undated one cannot be placed on a day and
    // still fails the whole walk.
    let incomplete_report = incomplete.resolve(&seen);
    scan.incomplete_requests = undated_incomplete_requests(&incomplete_report);
    let complete = scan.is_complete();
    let incomplete_days: HashSet<String> = incomplete_report.by_day.into_keys().collect();
    let result = claude_days(keys, &totals, &incomplete_days, complete);
    // A failed or partial walk is never memoized, so the next refresh retries.
    if complete {
        claude_memo().put(memo_key, Instant::now(), result.clone());
    }
    result
}

fn undated_incomplete_requests(report: &ClaudeIncompleteReport) -> u32 {
    let dated = report
        .by_day
        .values()
        .fold(0u32, |sum, count| sum.saturating_add(*count));
    report.total.saturating_sub(dated)
}

fn claude_days(
    keys: &[String],
    totals: &HashMap<String, u64>,
    incomplete_days: &HashSet<String>,
    complete: bool,
) -> Vec<TokenActivityDay> {
    keys.iter()
        .map(|key| TokenActivityDay {
            day: key.clone(),
            total_tokens: (complete && !incomplete_days.contains(key))
                .then(|| totals.get(key).copied().unwrap_or(0)),
            is_scanned: complete,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::ProviderId;

    fn date(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).unwrap()
    }

    #[test]
    fn window_keys_run_oldest_first_and_end_today() {
        let keys = window_keys(date(2024, 3, 1), 365);
        assert_eq!(keys.len(), 365);
        assert_eq!(keys.last().map(String::as_str), Some("2024-03-01"));
        // 2024 is a leap year: 364 days before 1 March is 3 March 2023.
        assert_eq!(keys.first().map(String::as_str), Some("2023-03-03"));
        assert!(keys.contains(&"2024-02-29".to_string()));
    }

    #[test]
    fn codex_days_outside_the_scan_window_are_unscanned_and_unknown() {
        let keys = window_keys(date(2026, 1, 5), 5);
        let totals = HashMap::from([("2026-01-04".to_string(), 42u64)]);
        let coverage = ("2026-01-03".to_string(), "2026-01-05".to_string());
        let days = codex_days(&keys, &totals, Some(&coverage));
        let summary: Vec<_> = days
            .iter()
            .map(|day| (day.day.as_str(), day.total_tokens, day.is_scanned))
            .collect();
        assert_eq!(
            summary,
            vec![
                ("2026-01-01", None, false),
                ("2026-01-02", None, false),
                ("2026-01-03", Some(0), true),
                ("2026-01-04", Some(42), true),
                ("2026-01-05", Some(0), true),
            ]
        );
    }

    #[test]
    fn codex_without_established_coverage_reports_no_known_day() {
        let keys = window_keys(date(2026, 1, 5), 3);
        let totals = HashMap::from([("2026-01-04".to_string(), 42u64)]);
        let days = codex_days(&keys, &totals, None);
        assert!(
            days.iter()
                .all(|day| day.total_tokens.is_none() && !day.is_scanned)
        );
    }

    #[test]
    fn claude_incomplete_days_stay_scanned_but_unknown() {
        let keys = window_keys(date(2026, 1, 3), 3);
        let totals = HashMap::from([
            ("2026-01-01".to_string(), 10u64),
            ("2026-01-02".to_string(), 20u64),
        ]);
        let incomplete = HashSet::from(["2026-01-02".to_string()]);
        let days = claude_days(&keys, &totals, &incomplete, true);
        let summary: Vec<_> = days
            .iter()
            .map(|day| (day.total_tokens, day.is_scanned))
            .collect();
        assert_eq!(
            summary,
            vec![(Some(10), true), (None, true), (Some(0), true)]
        );
    }

    #[test]
    fn claude_failed_walk_scans_no_day() {
        let keys = window_keys(date(2026, 1, 3), 3);
        let days = claude_days(&keys, &HashMap::new(), &HashSet::new(), false);
        assert!(
            days.iter()
                .all(|day| day.total_tokens.is_none() && !day.is_scanned)
        );
    }

    #[test]
    fn only_undated_incomplete_requests_fail_the_claude_walk() {
        let dated_only = ClaudeIncompleteReport {
            total: 3,
            by_day: HashMap::from([("2026-01-02".to_string(), 2), ("2026-01-03".to_string(), 1)]),
            ..ClaudeIncompleteReport::default()
        };
        assert_eq!(undated_incomplete_requests(&dated_only), 0);
        let with_undated = ClaudeIncompleteReport {
            total: 4,
            ..dated_only
        };
        assert_eq!(undated_incomplete_requests(&with_undated), 1);
    }

    #[test]
    fn serializes_camel_case_with_explicit_null() {
        let json = serde_json::to_value(TokenActivityDay {
            day: "2026-01-01".to_string(),
            total_tokens: None,
            is_scanned: true,
        })
        .unwrap();
        assert_eq!(
            json,
            serde_json::json!({"day": "2026-01-01", "totalTokens": null, "isScanned": true})
        );
    }

    /// One recent Codex token_count row in today's partition (synthetic).
    fn write_codex_activity_fixture(sessions_root: &Path, input_tokens: u64) {
        let at = chrono::Local::now() - Duration::minutes(5);
        let day = at.date_naive();
        let day_dir = sessions_root
            .join(day.format("%Y").to_string())
            .join(day.format("%m").to_string())
            .join(day.format("%d").to_string());
        std::fs::create_dir_all(&day_dir).unwrap();
        let ts = at.with_timezone(&Utc).format("%Y-%m-%dT%H:%M:%S%.3fZ");
        let body = format!(
            r#"{{"timestamp":"{ts}","type":"event_msg","payload":{{"type":"token_count","info":{{"model":"gpt-5","total_token_usage":{{"input_tokens":{input_tokens},"cached_input_tokens":0,"output_tokens":5}}}}}}}}
"#
        );
        std::fs::write(day_dir.join("rollout-activity.jsonl"), body).unwrap();
    }

    #[test]
    fn codex_activity_scanner_skips_pi_and_gets_its_own_cache_root() {
        let shared = Path::new("shared-cache");
        let scanner = codex_activity_scanner(365, Some(shared));
        assert!(!scanner.options.include_pi_sessions);
        assert_eq!(scanner.cache_root, Some(shared.join("token-activity")));
    }

    #[test]
    fn codex_activity_leaves_the_shared_cost_coverage_alone() {
        let root = tempfile::tempdir().unwrap();
        let sessions = root.path().join("sessions");
        let shared = root.path().join("cache");
        write_codex_activity_fixture(&sessions, 100);

        let cost = CostScanner::new(7)
            .with_options(CostScanOptions::app_driven())
            .with_cache_root(&shared)
            .with_sessions_dirs(vec![sessions.clone()]);
        cost.scan_codex_detailed(None);
        let before = JsonlScanner::load_cache(ProviderId::Codex, Some(&shared));
        assert!(before.scan_since_key.is_some());

        let keys = window_keys(cost_bucket_zone().date(Utc::now()), TOKEN_ACTIVITY_DAYS);
        let activity = codex_activity_scanner(TOKEN_ACTIVITY_DAYS, Some(&shared))
            .with_sessions_dirs(vec![sessions]);
        let days = codex_token_activity(activity, &keys);

        let after = JsonlScanner::load_cache(ProviderId::Codex, Some(&shared));
        assert_eq!(after.scan_since_key, before.scan_since_key);
        assert_eq!(after.scan_until_key, before.scan_until_key);
        let own = JsonlScanner::load_cache(
            ProviderId::Codex,
            Some(&shared.join(CODEX_ACTIVITY_CACHE_DIR)),
        );
        assert!(own.scan_since_key < before.scan_since_key);
        assert!(days.iter().any(|day| day.is_scanned));
        assert!(days.iter().any(|day| day.total_tokens.unwrap_or(0) > 0));
    }

    #[test]
    fn codex_activity_second_refresh_hits_its_own_debounce() {
        let root = tempfile::tempdir().unwrap();
        let sessions = root.path().join("sessions");
        let shared = root.path().join("cache");
        write_codex_activity_fixture(&sessions, 100);

        let activity = codex_activity_scanner(TOKEN_ACTIVITY_DAYS, Some(&shared))
            .with_sessions_dirs(vec![sessions]);
        let (_, first, _) = activity.clone().scan_codex_detailed_with_cache(None);
        assert!(!first.used_cache_debounce);
        let (_, second, _) = activity.scan_codex_detailed_with_cache(None);
        assert!(second.used_cache_debounce);
    }

    #[test]
    fn timed_memo_reuses_a_value_only_within_ttl_for_the_same_key() {
        let ttl = std::time::Duration::from_secs(60);
        let start = Instant::now();
        let mut memo = TimedMemo::new();
        assert_eq!(memo.get(&"a", start, ttl), None::<u32>);

        memo.put("a", start, 7u32);
        assert_eq!(memo.get(&"a", start + ttl, ttl), Some(7));
        assert_eq!(
            memo.get(&"a", start + ttl + std::time::Duration::from_millis(1), ttl),
            None
        );
        assert_eq!(memo.get(&"b", start, ttl), None);
    }

    #[test]
    fn claude_memo_key_changes_with_window_day_and_roots() {
        let roots = [PathBuf::from("projects")];
        let key = |days, today: &str, roots: &[PathBuf]| ClaudeActivityKey {
            days,
            today: today.to_string(),
            roots: roots.to_vec(),
        };
        let base = key(365, "2026-01-05", &roots);
        assert_eq!(base, key(365, "2026-01-05", &roots));
        assert_ne!(base, key(30, "2026-01-05", &roots));
        assert_ne!(base, key(365, "2026-01-06", &roots));
        assert_ne!(base, key(365, "2026-01-05", &[]));
    }
}
