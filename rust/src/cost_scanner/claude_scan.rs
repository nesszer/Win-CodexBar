//! Reading one Claude transcript file into usage records, and folding those
//! records into summaries, daily buckets and quota history.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::sync::atomic::AtomicBool;

use chrono::{DateTime, Utc};

use super::claude_incomplete::ClaudeIncompleteTracker;
use super::claude_pricing::ClaudeScanPricingResolver;
use super::claude_usage::{
    ClaudeUsageDedupKey, claude_usage_dedup_key, should_count_claude_record,
};
use super::{
    ClaudeEvent, ClaudeFileScanResult, ClaudeUsageRecord, CostSummary, is_cancelled,
    is_preliminary_claude_usage, today,
};
use crate::cost_reporting_period::cost_bucket_zone;
use crate::providers::claude::quota_history::{
    ClaudeHistoryAttribution, ClaudeQuotaDedupKey, ClaudeQuotaHistoryRecord,
};

pub(super) fn scan_claude_file_with_pricing<F>(
    path: &Path,
    cutoff: &DateTime<Utc>,
    seen: &mut HashSet<ClaudeUsageDedupKey>,
    cancel: Option<&AtomicBool>,
    pricing: &mut ClaudeScanPricingResolver,
    incomplete: &mut ClaudeIncompleteTracker,
    mut on_record: F,
) -> ClaudeFileScanResult
where
    F: FnMut(&ClaudeUsageRecord),
{
    let Ok(file) = File::open(path) else {
        return ClaudeFileScanResult {
            read_failures: 1,
            ..ClaudeFileScanResult::default()
        };
    };

    let mut result = ClaudeFileScanResult::default();
    // Use read_until so a final incomplete line (no trailing newline) is still
    // processed when it is valid UTF-8 JSON, and so a single bad line does not
    // stop the walk the way `lines().map_while(Result::ok)` would.
    let line_result = for_each_jsonl_text_line(BufReader::new(file), |line| {
        if is_cancelled(cancel) {
            return false;
        }
        if line.trim().is_empty() {
            return true;
        }
        let Ok(event) = serde_json::from_str::<ClaudeEvent>(line) else {
            result.malformed_lines = result.malformed_lines.saturating_add(1);
            return true;
        };
        if event.is_vertex_ai_usage_entry() {
            return true;
        }
        if is_preliminary_claude_usage(&event) {
            result.incomplete_requests = result.incomplete_requests.saturating_add(1);
            if let Some(message) = event.message.as_ref() {
                incomplete.record(
                    claude_usage_dedup_key(
                        message.id.as_deref(),
                        event.request_id.as_deref(),
                        event.session_id(),
                    ),
                    message.model.as_deref().unwrap_or("claude-3-5-sonnet"),
                    event.parsed_timestamp(),
                    cutoff,
                );
            }
            return true;
        }
        if let Some(record) = claude_usage_record_from_event_with_pricing(&event, pricing)
            && should_count_claude_record(&record, cutoff, seen)
        {
            result.counted += 1;
            on_record(&record);
        }
        true
    });
    result.malformed_lines = result
        .malformed_lines
        .saturating_add(line_result.malformed_lines);
    result.read_failures = line_result.read_failures;
    result
}

/// Walk JSONL text lines from `reader`, including a final incomplete line at EOF.
/// Continues past invalid UTF-8 segments. `on_line` returns `false` to stop early.
pub(super) fn for_each_jsonl_text_line<R, F>(mut reader: R, mut on_line: F) -> ClaudeJsonlLineResult
where
    R: BufRead,
    F: FnMut(&str) -> bool,
{
    let mut result = ClaudeJsonlLineResult::default();
    let mut buf = Vec::new();
    loop {
        buf.clear();
        match reader.read_until(b'\n', &mut buf) {
            Ok(0) => break,
            Ok(_) => {}
            Err(_) => {
                result.read_failures = result.read_failures.saturating_add(1);
                break;
            }
        }
        while matches!(buf.last(), Some(b'\n' | b'\r')) {
            buf.pop();
        }
        let Ok(line) = std::str::from_utf8(&buf) else {
            result.malformed_lines = result.malformed_lines.saturating_add(1);
            continue;
        };
        if !on_line(line) {
            break;
        }
    }
    result
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct ClaudeJsonlLineResult {
    pub(super) malformed_lines: u32,
    pub(super) read_failures: u32,
}

pub(super) fn finalize_claude_summary(
    summary: &mut CostSummary,
    projects_dir_exists: bool,
    scan_result: ClaudeFileScanResult,
    cancelled: bool,
) {
    let complete = projects_dir_exists && !cancelled && scan_result.is_complete();
    summary.history_coverage_established = complete;
    summary.known_zero = complete
        && summary.sessions_count == 0
        && summary.input_tokens == 0
        && summary.output_tokens == 0
        && summary.cached_tokens == 0
        && summary.total_cost_usd == 0.0;
}

pub(super) fn claude_usage_record_from_event_with_pricing(
    event: &ClaudeEvent,
    pricing: &mut ClaudeScanPricingResolver,
) -> Option<ClaudeUsageRecord> {
    if event.event_type.as_deref() != Some("assistant") || is_preliminary_claude_usage(event) {
        return None;
    }

    let message = event.message.as_ref()?;
    let usage = message.usage.as_ref()?;
    let model = message.model.as_deref().unwrap_or("claude-3-5-sonnet");

    let input = usage.input_tokens.unwrap_or(0);
    let output = usage.output_tokens.unwrap_or(0);
    let cache_create = usage.cache_creation_input_tokens.unwrap_or(0);
    let cache_read = usage.cache_read_input_tokens.unwrap_or(0);

    if input == 0 && output == 0 && cache_create == 0 && cache_read == 0 {
        return None;
    }

    let cache_create_1h = usage.one_hour_cache_creation_tokens(cache_create);
    let pricing_known = pricing.is_known(model);
    let computed_cost = pricing.cost_usd_with_cache_ttl(
        model,
        input,
        cache_create,
        cache_create_1h,
        cache_read,
        output,
    );
    let cost = computed_cost.is_finite().then_some(computed_cost);

    Some(ClaudeUsageRecord {
        model: model.to_string(),
        pricing_known,
        timestamp: event.parsed_timestamp(),
        dedup_key: claude_usage_dedup_key(
            message.id.as_deref(),
            event.request_id.as_deref(),
            event.session_id(),
        ),
        input,
        output,
        cache_create,
        cache_read,
        cost,
    })
}

pub(super) fn add_claude_record_to_summary(
    summary: &mut CostSummary,
    record: &ClaudeUsageRecord,
) -> bool {
    if !record.pricing_known {
        summary.unknown_models.insert(record.model.clone());
    }

    let mut complete = checked_add_assign(&mut summary.input_tokens, record.input);
    complete &= checked_add_assign(&mut summary.output_tokens, record.output);
    let cached = record.cache_create.checked_add(record.cache_read);
    complete &= cached.is_some_and(|value| checked_add_assign(&mut summary.cached_tokens, value));

    if let Some(cost) = record.cost {
        complete &= checked_add_finite(&mut summary.total_cost_usd, cost);
        complete &= checked_add_finite(
            summary.by_model.entry(record.model.clone()).or_insert(0.0),
            cost,
        );
    } else {
        complete = false;
    }

    let model_tokens = summary
        .by_model_tokens
        .entry(record.model.clone())
        .or_default();
    complete &= checked_add_assign(&mut model_tokens.input_tokens, record.input);
    complete &= checked_add_assign(&mut model_tokens.output_tokens, record.output);
    complete &=
        cached.is_some_and(|value| checked_add_assign(&mut model_tokens.cached_tokens, value));
    complete
}

pub(super) fn checked_add_assign(total: &mut u64, value: u64) -> bool {
    let Some(sum) = total.checked_add(value) else {
        return false;
    };
    *total = sum;
    true
}

pub(super) fn checked_add_finite(total: &mut f64, value: f64) -> bool {
    let sum = *total + value;
    if !value.is_finite() || !sum.is_finite() {
        return false;
    }
    *total = sum;
    true
}

pub(super) fn quota_history_record_from_usage(
    record: &ClaudeUsageRecord,
) -> Option<ClaudeQuotaHistoryRecord> {
    let timestamp = record.timestamp?;
    let tokens = record
        .input
        .checked_add(record.output)
        .and_then(|value| value.checked_add(record.cache_create))
        .and_then(|value| value.checked_add(record.cache_read));
    let dedup_key = record.dedup_key.as_ref().map(|key| match key {
        ClaudeUsageDedupKey::Request {
            message_id,
            request_id,
        } => ClaudeQuotaDedupKey::Request {
            message_id: message_id.clone(),
            request_id: request_id.clone(),
        },
        ClaudeUsageDedupKey::Session {
            session_id,
            message_id,
        } => ClaudeQuotaDedupKey::Session {
            session_id: session_id.clone(),
            message_id: message_id.clone(),
        },
    });
    Some(ClaudeQuotaHistoryRecord {
        timestamp,
        tokens,
        cost_usd: record.cost,
        tokens_are_complete: tokens.is_some(),
        cost_is_complete: record.pricing_known && record.cost.is_some_and(|cost| cost >= 0.0),
        dedup_key,
        attribution: ClaudeHistoryAttribution::Unavailable,
    })
}

/// Add one usage record to the per-day cost buckets, keyed by the record's
/// own timestamp in the pinned bucket zone. Records outside the initialized
/// date range (or without a timestamp) are ignored.
pub(super) fn add_claude_record_to_daily_costs(
    daily_costs: &mut HashMap<String, Option<f64>>,
    unknown_cost_dates: &mut HashSet<String>,
    record: &ClaudeUsageRecord,
) -> bool {
    let Some(timestamp) = record.timestamp else {
        return true;
    };
    let date_str = today::day_key(cost_bucket_zone().date(timestamp));
    if let Some(cost) = daily_costs.get_mut(&date_str) {
        if unknown_cost_dates.contains(&date_str) {
            return false;
        }
        let Some(record_cost) = record.cost else {
            *cost = None;
            unknown_cost_dates.insert(date_str);
            return false;
        };
        let sum = cost.unwrap_or(0.0) + record_cost;
        if !sum.is_finite() {
            *cost = None;
            unknown_cost_dates.insert(date_str);
            return false;
        }
        *cost = Some(sum);
    }
    true
}

pub(super) fn zero_fill_uninitialized_claude_daily_costs(
    daily_costs: &mut HashMap<String, Option<f64>>,
    unknown_cost_dates: &HashSet<String>,
) {
    for (day, cost) in daily_costs {
        if cost.is_none() && !unknown_cost_dates.contains(day) {
            *cost = Some(0.0);
        }
    }
}
