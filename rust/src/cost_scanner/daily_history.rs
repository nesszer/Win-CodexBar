//! Per-day cost and token series for the charts, plus the day-slot helpers they share.

use std::collections::{HashMap, HashSet};

use chrono::{Duration, NaiveDate, Utc};

use super::{
    ClaudeFileScanResult, ClaudeUsageRecord, CostScanner, CostSummary,
    add_claude_record_to_daily_costs, checked_add_assign, today,
    zero_fill_uninitialized_claude_daily_costs,
};
use crate::codex_costs::add_codex_days_map_to_summary;
use crate::core::CostUsageDayRange;
use crate::cost_reporting_period::cost_bucket_zone;
use crate::providers::opencodego::local as opencodego_local;

/// Check if any cost usage sources are available
#[allow(
    dead_code,
    reason = "utility probe for cost-usage availability; not yet wired into all call sites"
)]
pub fn has_cost_usage_sources() -> bool {
    let scanner = CostScanner::new(1);
    scanner
        .get_codex_sessions_dirs()
        .iter()
        .any(|dir| dir.exists())
        || scanner.claude_projects_roots().has_possible_roots()
        || crate::pi_session_cost::pi_compatible_session_roots(crate::pi_session_cost::scan_home())
            .iter()
            .any(|dir| dir.exists())
}

/// One `init` slot per day key, from `end` back over `days` days.
pub(super) fn day_slots<T: Clone>(end: NaiveDate, days: u32, init: T) -> HashMap<String, T> {
    (0..days)
        .map(|days_ago| {
            let date = end - Duration::days(i64::from(days_ago));
            (today::day_key(date), init.clone())
        })
        .collect()
}

pub(super) fn sorted_days<T>(days: HashMap<String, T>) -> Vec<(String, T)> {
    let mut days: Vec<_> = days.into_iter().collect();
    days.sort_by(|left, right| left.0.cmp(&right.0));
    days
}

/// One packed Codex cache day summarized on its own, with its cost.
pub(super) fn codex_day_summary(
    day_key: &str,
    models: &HashMap<String, Vec<i64>>,
) -> Option<(CostSummary, f64)> {
    let day = CostUsageDayRange::parse_day_key(day_key)?;
    let one_day = HashMap::from([(day_key.to_string(), models.clone())]);
    let mut summary = CostSummary::default();
    let (cost, _) =
        add_codex_days_map_to_summary(&mut summary, &one_day, &CostUsageDayRange::new(day, day));
    Some((summary, cost))
}

/// Get daily cost history for the last N days
/// Returns calendar-preserving daily costs sorted by date. `None` means the day
/// is unscanned or contains unpriced Codex usage; `Some(0)` is a known zero.
pub fn get_daily_cost_history(provider: &str, days: u32) -> Vec<(String, Option<f64>)> {
    get_daily_cost_and_incomplete_history(provider, days).0
}

/// Daily cost series plus per-day incomplete request counts.
pub type DailyCostAndIncomplete = (Vec<(String, Option<f64>)>, Vec<(String, u32)>);

/// Daily cost history plus, for Claude, the per-day count of incomplete proxy
/// requests (upstream 0.60.5 #3688). Days with no incomplete request are
/// absent from the second vector; it is empty for every other provider.
pub fn get_daily_cost_and_incomplete_history(provider: &str, days: u32) -> DailyCostAndIncomplete {
    let mut daily_incomplete = Vec::new();
    let scanner = CostScanner::new(days);
    let today = cost_bucket_zone().date(Utc::now());
    let mut daily_costs = day_slots(
        today,
        days,
        (provider != "codex" && provider != "claude" && provider != "pi").then_some(0.0),
    );

    match provider {
        "codex" => {
            // Warm/refresh the disk cache, then price from packed days. v0.56.1
            // preserves every calendar slot and distinguishes covered zero from
            // unscanned/unpriced history.
            let (_summary, _stats, cache) = scanner.scan_codex_detailed_with_cache(None);
            if cache.previous_report.is_none() && !cache.codex_scan_incomplete {
                for (day_key, slot) in &mut daily_costs {
                    if cache
                        .scan_since_key
                        .as_deref()
                        .is_some_and(|since| day_key.as_str() >= since)
                        && cache
                            .scan_until_key
                            .as_deref()
                            .is_some_and(|until| day_key.as_str() <= until)
                    {
                        *slot = Some(0.0);
                    }
                }
            }
            for (day_key, models) in &cache.days {
                let Some(slot) = daily_costs.get_mut(day_key) else {
                    continue;
                };
                let Some((day, cost)) = codex_day_summary(day_key, models) else {
                    continue;
                };
                *slot = (!day.model_pricing_completeness.is_partial()).then_some(cost);
            }
        }
        "claude" => {
            // Real per-day breakdown: walk the project logs once,
            // de-duplicating records across files.
            let cutoff = Utc::now() - Duration::days(days as i64);
            let mut unknown_cost_dates = HashSet::new();
            let walk = scanner.walk_claude_records(&cutoff, None, |record| {
                add_claude_record_to_daily_costs(&mut daily_costs, &mut unknown_cost_dates, record)
            });
            daily_incomplete = walk.incomplete.resolve(&walk.completed_keys).daily_sorted();
            if walk.roots_present && walk.scan.is_complete() {
                zero_fill_uninitialized_claude_daily_costs(&mut daily_costs, &unknown_cost_dates);
            }
        }
        "opencodego" => {
            // Per-day cost from the local OpenCode SQLite reader (upstream #2649).
            // Rows are grouped by local calendar day to match Codex/Claude keying.
            for (day_key, cost) in opencodego_local::daily_cost_series(Utc::now(), days) {
                if let Some(slot) = daily_costs.get_mut(&day_key) {
                    *slot = Some(slot.unwrap_or(0.0) + cost);
                }
            }
        }
        "pi" => {
            let scan = crate::pi_session_cost::scan_pi_daily(days, None);
            for (day_key, cost) in &scan.costs {
                if let Some(slot) = daily_costs.get_mut(day_key) {
                    *slot = (!scan.unpriced_days.contains(day_key)).then_some(*cost);
                }
            }
            if scan.history_coverage_established {
                for (day_key, slot) in &mut daily_costs {
                    if slot.is_none() && !scan.unpriced_days.contains(day_key) {
                        *slot = Some(0.0);
                    }
                }
            }
        }
        _ => {}
    }

    (sorted_days(daily_costs), daily_incomplete)
}

/// Daily token totals (provider-aware, see [`cache_is_separate_from_input`])
/// for the Tokens chart mode, plus
/// whether local history looks incomplete at the old edge of the window
/// (Codex backfill still in progress → the chart shows a "Refreshing"
/// marker; upstream 0.50.0 #2930).
pub fn get_daily_token_history(provider: &str, days: u32) -> (Vec<(String, u64)>, bool) {
    let scanner = CostScanner::new(days);
    let today = cost_bucket_zone().date(Utc::now());
    let mut daily_tokens = day_slots(today, days, 0u64);
    let mut covered_days: HashSet<String> = HashSet::new();

    match provider {
        "codex" => {
            // Warm/refresh the disk cache, then read exact local token totals
            // from packed days through the same summary path the cost chart
            // uses.
            let (_summary, _stats, cache) = scanner.scan_codex_detailed_with_cache(None);
            for (day_key, models) in &cache.days {
                let Some(slot) = daily_tokens.get_mut(day_key) else {
                    continue;
                };
                let Some((day, _)) = codex_day_summary(day_key, models) else {
                    continue;
                };
                *slot = day.total_tokens_for_provider("codex");
                covered_days.insert(day_key.clone());
            }
        }
        "claude" => {
            // Per-day token breakdown from the same de-duplicated record walk
            // as the cost chart. Only a complete valid walk establishes
            // authoritative coverage of the requested history window.
            let cutoff = Utc::now() - Duration::days(days as i64);
            let walk = scanner.walk_claude_records(&cutoff, None, |record| {
                add_claude_record_to_daily_tokens(&mut daily_tokens, record)
            });
            if walk.roots_present {
                mark_claude_daily_token_coverage(&mut covered_days, &daily_tokens, walk.scan);
            }
        }
        "pi" => {
            let scan = crate::pi_session_cost::scan_pi_daily(days, None);
            for (day_key, tokens) in scan.tokens {
                if let Some(slot) = daily_tokens.get_mut(&day_key) {
                    *slot = tokens;
                }
            }
            if scan.history_coverage_established {
                covered_days.extend(daily_tokens.keys().cloned());
            }
        }
        _ => {}
    }

    let result = sorted_days(daily_tokens);

    let incomplete = if matches!(provider, "claude" | "pi") {
        // A complete filesystem scan covers the requested window even when
        // the roots contain no sessions; failed scans leave coverage empty.
        covered_days.is_empty()
    } else {
        // Codex catch-up may not have reached the oldest quarter of the window.
        provider == "codex"
            && !covered_days.is_empty()
            && covered_days.len() < days as usize
            && result[..(result.len() / 4).max(1)]
                .iter()
                .any(|(date, _)| !covered_days.contains(date))
    };

    (result, incomplete)
}

pub(super) fn add_claude_record_to_daily_tokens(
    daily_tokens: &mut HashMap<String, u64>,
    record: &ClaudeUsageRecord,
) -> bool {
    let Some(timestamp) = record.timestamp else {
        return true;
    };
    let date_str = today::day_key(cost_bucket_zone().date(timestamp));
    if let Some(slot) = daily_tokens.get_mut(&date_str) {
        // Claude reports cache reads and writes separately from input, so the
        // day total includes them (same rule as the window and model totals).
        let Some(tokens) = record
            .input
            .checked_add(record.output)
            .and_then(|tokens| tokens.checked_add(record.cache_read))
            .and_then(|tokens| tokens.checked_add(record.cache_create))
        else {
            return false;
        };
        return checked_add_assign(slot, tokens);
    }
    true
}

pub(super) fn mark_claude_daily_token_coverage(
    covered_days: &mut HashSet<String>,
    daily_tokens: &HashMap<String, u64>,
    scan_result: ClaudeFileScanResult,
) {
    if scan_result.is_complete() {
        covered_days.extend(daily_tokens.keys().cloned());
    } else {
        covered_days.clear();
    }
}
