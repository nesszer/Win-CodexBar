//! Per-UTC-day bucketing of the OpenAI Admin API costs and completions listings.
//!
//! Mirrors upstream v0.66.0 `Resources/Plugins/openai.js` (`daily` map, `bucket()`) and the
//! card bounds in `OpenAIAPIProviderDescriptor.mapPluginCard`. Buckets are keyed by
//! `start_time`; the first bucket seen for a day fixes its `end_time`.

use chrono::{DateTime, Utc};
use std::collections::{BTreeMap, HashMap};

use super::{CompletionsUsageBucket, CostBucket, cost_amount};
use crate::core::{
    OpenAiApiDailyUsage, OpenAiApiLineItemCost, OpenAiApiModelUsage, OpenAiApiUsageHistory,
    ProviderError,
};

/// Upstream card bound: line items plus models across every day.
const MAX_CARD_ENTRIES: usize = 10_000;
/// Largest integer a JavaScript number holds exactly; upstream rejects anything above it.
const MAX_SAFE_COUNT: u64 = 9_007_199_254_740_991;
const DEFAULT_LINE_ITEM: &str = "API";
const DEFAULT_MODEL: &str = "Responses and Chat Completions";

/// Running per-model totals for one day. `input` and `output` include audio tokens and
/// `cached` is a subset of `input`.
#[derive(Default)]
struct ModelTotals {
    requests: u64,
    input: u64,
    cached: u64,
    output: u64,
}

struct DayAccumulator {
    start: i64,
    end: i64,
    cost: f64,
    lines: HashMap<String, f64>,
    models: HashMap<String, ModelTotals>,
}

impl DayAccumulator {
    fn new(start: i64, end: i64) -> Self {
        Self {
            start,
            end,
            cost: 0.0,
            lines: HashMap::new(),
            models: HashMap::new(),
        }
    }

    fn finish(self) -> Result<OpenAiApiDailyUsage, ProviderError> {
        let mut line_items: Vec<_> = self
            .lines
            .into_iter()
            .map(|(name, cost_usd)| OpenAiApiLineItemCost { name, cost_usd })
            .collect();
        line_items.sort_by(|a, b| {
            b.cost_usd
                .total_cmp(&a.cost_usd)
                .then_with(|| a.name.cmp(&b.name))
        });
        let mut models: Vec<_> = self
            .models
            .into_iter()
            .map(|(name, totals)| {
                Ok(OpenAiApiModelUsage {
                    name,
                    requests: totals.requests,
                    input_tokens: totals.input,
                    cached_input_tokens: totals.cached,
                    output_tokens: totals.output,
                    total_tokens: checked_count_sum(
                        totals.input,
                        totals.output,
                        "model total_tokens",
                    )?,
                })
            })
            .collect::<Result<Vec<_>, ProviderError>>()?;
        models.sort_by(|a, b| {
            b.total_tokens
                .cmp(&a.total_tokens)
                .then_with(|| a.name.cmp(&b.name))
        });
        let sum = |field: fn(&OpenAiApiModelUsage) -> u64, name: &str| {
            models.iter().try_fold(0_u64, |sum, model| {
                checked_count_sum(sum, field(model), name)
            })
        };
        Ok(OpenAiApiDailyUsage {
            start_time: self.start,
            end_time: self.end,
            cost_usd: self.cost,
            requests: sum(|model| model.requests, "daily requests")?,
            input_tokens: sum(|model| model.input_tokens, "daily input_tokens")?,
            cached_input_tokens: sum(
                |model| model.cached_input_tokens,
                "daily cached_input_tokens",
            )?,
            output_tokens: sum(|model| model.output_tokens, "daily output_tokens")?,
            total_tokens: sum(|model| model.total_tokens, "daily total_tokens")?,
            line_items,
            models,
        })
    }
}

/// Group both listings by UTC day. Days that start after `now` are dropped and only the
/// newest `history_days` remain, ascending by start time.
pub(super) fn daily_usage(
    costs: &[CostBucket],
    completions: &[CompletionsUsageBucket],
    now: DateTime<Utc>,
    history_days: u32,
) -> Result<Vec<OpenAiApiDailyUsage>, ProviderError> {
    let mut days: BTreeMap<i64, DayAccumulator> = BTreeMap::new();

    for bucket in costs {
        let day = days
            .entry(bucket.start_time)
            .or_insert_with(|| DayAccumulator::new(bucket.start_time, bucket.end_time));
        for result in &bucket.results {
            let amount = cost_amount(result)?;
            day.cost += amount;
            *day.lines
                .entry(display_name(result.line_item.as_deref(), DEFAULT_LINE_ITEM))
                .or_default() += amount;
        }
    }

    for bucket in completions {
        let day = days
            .entry(bucket.start_time)
            .or_insert_with(|| DayAccumulator::new(bucket.start_time, bucket.end_time));
        for result in &bucket.results {
            let input = count(result.input_tokens, "input_tokens")?;
            let cached = count(result.input_cached_tokens, "input_cached_tokens")?;
            // Audio breakdowns are already inside `input_tokens`/`output_tokens`; they are
            // validated but never added again (upstream 5a40ac439).
            count(result.input_audio_tokens, "input_audio_tokens")?;
            let output = count(result.output_tokens, "output_tokens")?;
            count(result.output_audio_tokens, "output_audio_tokens")?;
            let requests = count(result.num_model_requests, "num_model_requests")?;
            let model = day
                .models
                .entry(display_name(result.model.as_deref(), DEFAULT_MODEL))
                .or_default();
            model.requests = checked_count_sum(model.requests, requests, "model requests")?;
            model.input = checked_count_sum(model.input, input, "model input_tokens")?;
            model.cached = checked_count_sum(model.cached, cached, "model cached_input_tokens")?;
            model.output = checked_count_sum(model.output, output, "model output_tokens")?;
        }
    }

    let now = now.timestamp();
    let mut daily: Vec<_> = days
        .into_values()
        .filter(|day| day.start <= now)
        .map(DayAccumulator::finish)
        .collect::<Result<_, ProviderError>>()?;
    let excess = daily.len().saturating_sub(history_days as usize);
    daily.drain(..excess);
    Ok(daily)
}

/// The card payload for `daily`, or `None` when it breaks an upstream card bound (a day
/// that does not end after it starts, or more than [`MAX_CARD_ENTRIES`] breakdown rows).
/// The chart is auxiliary, so an out-of-bounds history is dropped and the spend summary
/// still shows, where upstream would fail the whole fetch.
pub(super) fn usage_history(
    daily: Vec<OpenAiApiDailyUsage>,
    history_days: u32,
    project_id: Option<&str>,
) -> Option<OpenAiApiUsageHistory> {
    if daily.iter().any(|day| day.end_time <= day.start_time) {
        tracing::warn!("Dropping OpenAI API daily history: a bucket does not end after it starts");
        return None;
    }
    if daily.iter().any(|day| !counts_fit_js_number(day)) {
        tracing::warn!(
            "Dropping OpenAI API daily history: an aggregate count exceeds the JavaScript safe-integer range"
        );
        return None;
    }
    let entries: usize = daily
        .iter()
        .map(|day| day.line_items.len() + day.models.len())
        .sum();
    if entries > MAX_CARD_ENTRIES {
        tracing::warn!(entries, "Dropping OpenAI API daily history: too many rows");
        return None;
    }
    Some(OpenAiApiUsageHistory {
        history_days,
        project_id: project_id.map(ToOwned::to_owned),
        daily,
    })
}

fn counts_fit_js_number(day: &OpenAiApiDailyUsage) -> bool {
    let safe = |count| count <= MAX_SAFE_COUNT;
    [
        day.requests,
        day.input_tokens,
        day.cached_input_tokens,
        day.output_tokens,
        day.total_tokens,
    ]
    .into_iter()
    .all(safe)
        && day.models.iter().all(|model| {
            [
                model.requests,
                model.input_tokens,
                model.cached_input_tokens,
                model.output_tokens,
                model.total_tokens,
            ]
            .into_iter()
            .all(safe)
        })
}

fn checked_count_sum(left: u64, right: u64, field: &str) -> Result<u64, ProviderError> {
    left.checked_add(right).ok_or_else(|| {
        ProviderError::Parse(format!(
            "OpenAI API completions {field} total exceeds the supported integer range"
        ))
    })
}

/// Upstream `name()`: a trimmed non-empty string, else the fallback.
fn display_name(raw: Option<&str>, fallback: &str) -> String {
    raw.map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or(fallback)
        .to_string()
}

/// Upstream `integer(value, field, optional)`: absent is zero; negative or beyond the
/// JavaScript safe-integer range is a parse failure.
fn count(value: Option<i64>, field: &str) -> Result<u64, ProviderError> {
    let Some(value) = value else { return Ok(0) };
    u64::try_from(value)
        .ok()
        .filter(|value| *value <= MAX_SAFE_COUNT)
        .ok_or_else(|| {
            ProviderError::Parse(format!(
                "OpenAI API completions {field} must be a non-negative integer"
            ))
        })
}
