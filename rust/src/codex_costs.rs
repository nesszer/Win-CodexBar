//! Codex local-log cost aggregation helpers.
//!
//! The SSH wire contract lives in [`summary_contract`]; this module owns the
//! local-scan aggregation and pricing logic.

mod host_costs;
mod quota_windows;
mod summary_contract;

pub(crate) use host_costs::{CodexHostCostsArgs, HostOutputFormat, run_codex_host_costs};
pub use quota_windows::{CodexQuotaWindow, codex_quota_windows_from_cache};
pub(crate) use summary_contract::{
    CodexCostSummary, CodexHostCostReport, CodexHostCostWindow, CodexHostOutcome,
    MAX_REMOTE_CODEX_COST_BYTES, REMOTE_CODEX_COST_INVALID, REMOTE_CODEX_COST_UNAVAILABLE,
    decode_remote_codex_summary,
};

use chrono::{Duration, NaiveDate, Utc};
use std::collections::HashSet;

use crate::core::{
    CodexUsageRecord, CostUsageCache, CostUsageDayRange, CostUsagePricing, JsonlScanner,
    is_unpriced_codex_routing_model,
};
use crate::cost_reporting_period::cost_bucket_zone;
use crate::cost_scanner::{CostSummary, ModelPricingCompleteness, ModelTokenCounts};
use crate::spend_contract::CostCoverageCounts;

/// Build the host summary from one native Codex scan. `today_cache` is the
/// decoded cache the scan itself used, so today's bucket folds without a
/// second filesystem walk or a second cache decode.
pub(crate) fn build_codex_cost_summary(
    history: CostSummary,
    today_cache: &CostUsageCache,
    history_days: u32,
) -> CodexCostSummary {
    let today = codex_today_summary(&history, today_cache);
    CodexCostSummary::from_summaries_at(
        &history,
        &today,
        history_days,
        Utc::now(),
        cost_bucket_zone().identifier(),
    )
}

fn codex_today_summary(history: &CostSummary, cache: &CostUsageCache) -> CostSummary {
    let today = cost_bucket_zone().date(Utc::now());
    let range = CostUsageDayRange::new(today, today);
    let mut summary = CostSummary {
        period_start: Some(today),
        period_end: Some(today),
        history_coverage_established: history.history_coverage_established,
        ..CostSummary::default()
    };
    let (cost, _) = add_codex_days_map_to_summary(&mut summary, &cache.days, &range);
    summary.total_cost_usd = cost;
    summary.sessions_count = cache
        .files
        .values()
        .filter(|usage| usage.days.contains_key(&range.until_key))
        .count()
        .try_into()
        .unwrap_or(u32::MAX);
    summary.known_zero = summary.history_coverage_established && summary.sessions_count == 0;
    summary
}

fn coverage_from_summary(summary: &CostSummary) -> CostCoverageCounts {
    let mut model_names = HashSet::new();
    model_names.extend(summary.by_model.keys().cloned());
    model_names.extend(summary.by_model_tokens.keys().cloned());
    model_names.extend(summary.unknown_models.iter().cloned());

    let mut unpriced_models = summary.unknown_models.clone();
    if let ModelPricingCompleteness::Partial {
        unpriced_models: partial_models,
    } = &summary.model_pricing_completeness
    {
        unpriced_models.extend(partial_models.iter().cloned());
    }
    let unpriced = model_names
        .iter()
        .filter(|model| unpriced_models.contains(*model))
        .count();
    let estimated = model_names.len().saturating_sub(unpriced);

    CostCoverageCounts {
        priced: 0,
        unpriced: unpriced.try_into().unwrap_or(u32::MAX),
        unmetered: 0,
        estimated: estimated.try_into().unwrap_or(u32::MAX),
    }
}

pub(crate) fn codex_period_start(today: NaiveDate, days: u32) -> NaiveDate {
    today - Duration::days(days.saturating_sub(1) as i64)
}

pub(crate) fn codex_scan_dates(range: &CostUsageDayRange) -> Vec<NaiveDate> {
    let Some(mut date) = CostUsageDayRange::parse_day_key(&range.scan_since_key) else {
        return Vec::new();
    };
    let Some(until) = CostUsageDayRange::parse_day_key(&range.scan_until_key) else {
        return Vec::new();
    };
    let mut dates = Vec::new();
    while date <= until {
        dates.push(date);
        date += Duration::days(1);
    }
    dates
}

pub(crate) fn add_codex_records_to_summary(
    summary: &mut CostSummary,
    records: &[(CodexUsageRecord, i64)],
    range: &CostUsageDayRange,
) -> (f64, bool) {
    let mut total_cost = 0.0;
    let mut has_tokens = false;

    for (record, _) in records.iter().filter(|(record, _)| {
        CostUsageDayRange::is_in_range(&record.day_key, &range.since_key, &range.until_key)
    }) {
        let tokens = CodexTokenCounts::from_values(record.input, record.cached, record.output)
            .with_reasoning(
                record
                    .reasoning
                    .map(|reasoning| u64::try_from(reasoning.max(0)).unwrap_or(0)),
            );
        let pricing_day = CostUsageDayRange::parse_day_key(&record.day_key);
        if let Some(cost) = add_codex_tokens_to_summary(summary, &record.model, tokens, pricing_day)
        {
            total_cost += cost;
            has_tokens = true;
        }
    }

    (total_cost, has_tokens)
}

/// Merge billable records into a day→model→`[input,cached,output]` map.
pub(crate) fn merge_codex_records_into_days(
    days: &mut std::collections::HashMap<String, std::collections::HashMap<String, Vec<i64>>>,
    records: &[(CodexUsageRecord, i64)],
) {
    for (record, _) in records {
        if !CostUsagePricing::counts_toward_codex_subscription(&record.model) {
            continue;
        }
        let models = days.entry(record.day_key.clone()).or_default();
        let packed = models.entry(record.model.clone()).or_default();
        JsonlScanner::merge_codex_record_into_packed(packed, record);
    }
}

/// Apply one packed `[input, cached, output]` triple to a summary.
pub(crate) fn add_codex_packed_tokens_to_summary(
    summary: &mut CostSummary,
    model: &str,
    packed: &[i64],
    pricing_day: Option<NaiveDate>,
) -> Option<f64> {
    let input = packed.first().copied().unwrap_or(0);
    let cached = packed.get(1).copied().unwrap_or(0);
    let output = packed.get(2).copied().unwrap_or(0);
    let reasoning = packed
        .get(3)
        .copied()
        .map(|reasoning| u64::try_from(reasoning.max(0)).unwrap_or(0));
    add_codex_tokens_to_summary(
        summary,
        model,
        CodexTokenCounts::from_values(input, cached, output).with_reasoning(reasoning),
        pricing_day,
    )
}

/// Fold day→model→packed token maps into a cost summary (range-filtered).
/// Returns `(session_cost, has_tokens)` — caller adds cost to `total_cost_usd`.
pub(crate) fn add_codex_days_map_to_summary(
    summary: &mut CostSummary,
    days: &std::collections::HashMap<String, std::collections::HashMap<String, Vec<i64>>>,
    range: &CostUsageDayRange,
) -> (f64, bool) {
    let mut total_cost = 0.0;
    let mut has_tokens = false;
    for (day_key, models) in days {
        if !CostUsageDayRange::is_in_range(day_key, &range.since_key, &range.until_key) {
            continue;
        }
        let pricing_day = CostUsageDayRange::parse_day_key(day_key);
        for (model, packed) in models {
            if let Some(cost) =
                add_codex_packed_tokens_to_summary(summary, model, packed, pricing_day)
            {
                total_cost += cost;
                has_tokens = true;
            }
        }
    }
    (total_cost, has_tokens)
}

#[derive(Clone, Copy)]
struct CodexTokenCounts {
    input: u64,
    cached: u64,
    output: u64,
    reasoning: Option<u64>,
}

impl CodexTokenCounts {
    fn from_values(input: i64, cached: i64, output: i64) -> Self {
        let input = u64::try_from(input.max(0)).unwrap_or(0);
        Self {
            input,
            cached: u64::try_from(cached.max(0)).unwrap_or(0).min(input),
            output: u64::try_from(output.max(0)).unwrap_or(0),
            reasoning: None,
        }
    }

    fn with_reasoning(mut self, reasoning: Option<u64>) -> Self {
        self.reasoning = reasoning;
        self
    }

    fn is_empty(self) -> bool {
        self.input == 0 && self.cached == 0 && self.output == 0
    }
}

fn add_tokens(summary: &mut ModelTokenCounts, tokens: CodexTokenCounts) {
    let had_core_tokens = has_core_tokens(summary);
    merge_reasoning_tokens(
        &mut summary.reasoning_tokens,
        had_core_tokens,
        tokens.reasoning,
    );
    summary.input_tokens = summary.input_tokens.saturating_add(tokens.input);
    summary.output_tokens = summary.output_tokens.saturating_add(tokens.output);
    summary.cached_tokens = summary.cached_tokens.saturating_add(tokens.cached);
}

fn add_summary_tokens(summary: &mut CostSummary, tokens: CodexTokenCounts) {
    let had_core_tokens =
        summary.input_tokens != 0 || summary.output_tokens != 0 || summary.cached_tokens != 0;
    merge_reasoning_tokens(
        &mut summary.reasoning_tokens,
        had_core_tokens,
        tokens.reasoning,
    );
    summary.input_tokens = summary.input_tokens.saturating_add(tokens.input);
    summary.cached_tokens = summary.cached_tokens.saturating_add(tokens.cached);
    summary.output_tokens = summary.output_tokens.saturating_add(tokens.output);
}

fn has_core_tokens(counts: &ModelTokenCounts) -> bool {
    counts.input_tokens != 0 || counts.output_tokens != 0 || counts.cached_tokens != 0
}

fn merge_reasoning_tokens(
    reasoning_tokens: &mut Option<u64>,
    had_core_tokens: bool,
    incoming: Option<u64>,
) {
    match (had_core_tokens, *reasoning_tokens, incoming) {
        (false, _, incoming) => *reasoning_tokens = incoming,
        (true, None, _) => {}
        (true, Some(_), None) => *reasoning_tokens = None,
        (true, Some(previous), Some(incoming)) => {
            *reasoning_tokens = Some(previous.saturating_add(incoming));
        }
    }
}

fn add_codex_tokens_to_summary(
    summary: &mut CostSummary,
    model: &str,
    tokens: CodexTokenCounts,
    pricing_day: Option<NaiveDate>,
) -> Option<f64> {
    if tokens.is_empty() {
        return None;
    }
    if !CostUsagePricing::counts_toward_codex_subscription(model) {
        return None;
    }

    let is_routing_unpriced = is_unpriced_codex_routing_model(model);
    let model_key = if CostUsagePricing::is_codex_unattributed_model(model) {
        CostUsagePricing::CODEX_UNATTRIBUTED_MODEL.to_string()
    } else if is_routing_unpriced {
        // Preserve the original routing model name (e.g. "codex-auto-review") so
        // the breakdown shows it as a deliberately-unpriced row, distinct from the
        // model-less "unknown" sentinel.
        model.to_string()
    } else {
        model.to_string()
    };

    // Unattributed and routing-unpriced usage is visible but never priced and
    // must not trigger a models.dev catalog refresh (it is deliberately unpriced,
    // not "unknown yet"). Upstream 0.48.0 F18: codex-auto-review rows are retained
    // with cost-nil so priced rows in the same history stay ranked.
    if CostUsagePricing::is_codex_unattributed_model(&model_key) || is_routing_unpriced {
        add_summary_tokens(summary, tokens);
        summary.by_model.entry(model_key.clone()).or_insert(0.0);
        add_tokens(
            summary
                .by_model_tokens
                .entry(model_key.clone())
                .or_default(),
            tokens,
        );
        // Mark the breakdown as partial so the dashboard labels it (F18).
        summary.model_pricing_completeness.mark_unpriced(&model_key);
        return Some(0.0);
    }

    let priced = CostUsagePricing::codex_day_aggregate_cost_usd(
        &model_key,
        tokens.input,
        tokens.cached,
        tokens.output,
        pricing_day,
    );
    let uses_fallback_pricing = priced.is_none();
    let cost = priced.unwrap_or_else(|| {
        codex_cost_usd_fallback(&model_key, tokens.input, tokens.cached, tokens.output)
    });
    if uses_fallback_pricing {
        summary.unknown_models.insert(model_key.clone());
        summary.model_pricing_completeness.mark_unpriced(&model_key);
    }

    add_summary_tokens(summary, tokens);
    *summary.by_model.entry(model_key.clone()).or_insert(0.0) += cost;

    let speed_bucket = codex_speed_bucket(&model_key);
    *summary
        .by_speed
        .entry(speed_bucket.to_string())
        .or_insert(0.0) += cost;
    add_tokens(
        summary.by_model_tokens.entry(model_key).or_default(),
        tokens,
    );
    add_tokens(
        summary
            .by_speed_tokens
            .entry(speed_bucket.to_string())
            .or_default(),
        tokens,
    );
    Some(cost)
}

fn codex_speed_bucket(model: &str) -> &'static str {
    let normalized = model.to_ascii_lowercase();
    if normalized.contains("fast")
        || normalized.contains("priority")
        || normalized.contains("spark")
        || normalized.contains("smoke")
    {
        "fast"
    } else {
        "standard"
    }
}

fn codex_cost_usd_fallback(model: &str, input: u64, cached: u64, output: u64) -> f64 {
    let (input_price, cached_price, output_price) = match model.to_lowercase().as_str() {
        m if m.contains("gpt-4o-mini") => (0.15, 0.075, 0.60),
        m if m.contains("gpt-4o") => (2.50, 1.25, 10.00),
        m if m.contains("gpt-4-turbo") => (10.00, 5.00, 30.00),
        m if m.contains("gpt-4") => (30.00, 15.00, 60.00),
        m if m.contains("o1-mini") => (3.00, 1.50, 12.00),
        m if m.contains("o1") => (15.00, 7.50, 60.00),
        _ => (2.50, 1.25, 10.00),
    };

    let cached = cached.min(input);
    let non_cached = input.saturating_sub(cached);
    let input_cost = (non_cached as f64 / 1_000_000.0) * input_price;
    let cached_cost = (cached as f64 / 1_000_000.0) * cached_price;
    let output_cost = (output as f64 / 1_000_000.0) * output_price;

    input_cost + cached_cost + output_cost
}

#[cfg(test)]
pub(crate) mod tests;
