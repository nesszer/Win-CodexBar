//! Reading one Claude transcript file into usage records, and folding those
//! records into summaries, daily buckets and quota history.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::path::Path;
use std::sync::atomic::AtomicBool;

use chrono::{DateTime, Utc};

use super::claude_file_cache::{ClaudeParsedLine, claude_file_lines};
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

    // Parsing is cached per file (only appended bytes are read again); the
    // cutoff, de-duplication, incomplete tracking and pricing below run on
    // every scan, so the result matches a cold read of the file.
    let snapshot = claude_file_lines(path, file, cancel, pricing);
    let mut result = ClaudeFileScanResult {
        read_failures: snapshot.read_failures,
        ..ClaudeFileScanResult::default()
    };
    for line in snapshot.lines.lines() {
        if is_cancelled(cancel) {
            break;
        }
        match line {
            ClaudeParsedLine::Malformed => {
                result.malformed_lines = result.malformed_lines.saturating_add(1);
            }
            ClaudeParsedLine::Preliminary { row } => {
                result.incomplete_requests = result.incomplete_requests.saturating_add(1);
                if let Some((key, model, timestamp)) = row {
                    incomplete.record(key.clone(), model, *timestamp, cutoff);
                }
            }
            ClaudeParsedLine::Usage {
                record,
                cache_create_1h,
            } => {
                let record = repriced_claude_record(record, *cache_create_1h, pricing);
                if should_count_claude_record(&record, cutoff, seen) {
                    result.counted += 1;
                    on_record(&record);
                }
            }
            ClaudeParsedLine::Repeat { of, timestamp } => {
                let Some(ClaudeParsedLine::Usage {
                    record: anchor,
                    cache_create_1h,
                }) = snapshot.lines.get(*of)
                else {
                    continue;
                };
                // Same checks as `should_count_claude_record`, made before the
                // record is materialized: almost always the anchor was counted
                // and this repeat is a duplicate.
                if timestamp.is_some_and(|timestamp| timestamp < *cutoff)
                    || anchor
                        .dedup_key
                        .as_ref()
                        .is_some_and(|key| seen.contains(key))
                {
                    continue;
                }
                let repeat = ClaudeUsageRecord {
                    timestamp: *timestamp,
                    ..anchor.clone()
                };
                let record = repriced_claude_record(&repeat, *cache_create_1h, pricing);
                if should_count_claude_record(&record, cutoff, seen) {
                    result.counted += 1;
                    on_record(&record);
                }
            }
        }
    }
    result
}

/// Classify one transcript line for cost scanning. Lines that never affect a
/// scan (blank lines, Vertex AI rows, non-usage events) yield `None`.
pub(super) fn claude_parsed_line(
    line: &str,
    pricing: &mut ClaudeScanPricingResolver,
) -> Option<ClaudeParsedLine> {
    if line.trim().is_empty() {
        return None;
    }
    let Ok(event) = serde_json::from_str::<ClaudeEvent>(line) else {
        return Some(ClaudeParsedLine::Malformed);
    };
    if event.is_vertex_ai_usage_entry() {
        return None;
    }
    if is_preliminary_claude_usage(&event) {
        let row = event.message.as_ref().map(|message| {
            (
                claude_usage_dedup_key(
                    message.id.as_deref(),
                    event.request_id.as_deref(),
                    event.session_id(),
                ),
                message
                    .model
                    .as_deref()
                    .unwrap_or("claude-3-5-sonnet")
                    .to_string(),
                event.parsed_timestamp(),
            )
        });
        return Some(ClaudeParsedLine::Preliminary { row });
    }
    let cache_create_1h = event
        .message
        .as_ref()
        .and_then(|message| message.usage.as_ref())
        .map(|usage| {
            usage.one_hour_cache_creation_tokens(usage.cache_creation_input_tokens.unwrap_or(0))
        })
        .unwrap_or(0);
    claude_usage_record_from_event_with_pricing(&event, pricing).map(|record| {
        ClaudeParsedLine::Usage {
            record,
            cache_create_1h,
        }
    })
}

/// `record` with pricing recomputed by `pricing`, borrowed when unchanged.
fn repriced_claude_record<'a>(
    record: &'a ClaudeUsageRecord,
    cache_create_1h: u64,
    pricing: &mut ClaudeScanPricingResolver,
) -> Cow<'a, ClaudeUsageRecord> {
    let pricing_known = pricing.is_known(&record.model);
    let computed = pricing.cost_usd_with_cache_ttl(
        &record.model,
        record.timestamp,
        record.input,
        record.cache_create,
        cache_create_1h,
        record.cache_read,
        record.output,
    );
    let cost = computed.is_finite().then_some(computed);
    let same_cost = cost.map(f64::to_bits) == record.cost.map(f64::to_bits);
    if pricing_known == record.pricing_known && same_cost {
        return Cow::Borrowed(record);
    }
    Cow::Owned(ClaudeUsageRecord {
        pricing_known,
        cost,
        ..record.clone()
    })
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
    let timestamp = event.parsed_timestamp();
    let computed_cost = pricing.cost_usd_with_cache_ttl(
        model,
        timestamp,
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
        timestamp,
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
