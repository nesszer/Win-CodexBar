//! OpenAI-backed OpenCode local history for the opt-in Codex cost view
//! (upstream #4786).
//!
//! Reads the same local OpenCode database as OpenCode Go but selects only
//! `openai` assistant turns. Costs are the recorded `cost` field, so a
//! recorded $0 stays $0 and is never replaced by a list-price estimate.
//! Tokens are converted to the Codex convention, where `input_tokens`
//! already includes cached input and `output_tokens` includes reasoning.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use chrono::{DateTime, NaiveDate};

use super::scan_cache::ScanCache;
use super::{RowSource, TokenSums, UsageRow, candidate_paths, model_name, scan_rows_from};
use crate::cost_reporting_period::CostTimeZone;
use crate::cost_scanner::{CostSummary, ModelTokenCounts};

/// Separate from the OpenCode Go cache: each holds one entry, and sharing one
/// would make the two row sets evict each other on every refresh.
fn shared_cache() -> &'static ScanCache {
    static CACHE: ScanCache = ScanCache::new();
    &CACHE
}

/// A set `CODEX_HOME` points the Codex ledger at an alternate home. Device-wide
/// OpenCode history belongs only to the ambient ledger, so it stays excluded.
pub(crate) fn codex_home_is_ambient(codex_home: Option<&str>) -> bool {
    codex_home.is_none_or(|value| value.trim().is_empty())
}

fn scan_available_rows() -> Result<Option<Arc<Vec<UsageRow>>>, ()> {
    scan_rows_from(candidate_paths(), shared_cache(), RowSource::OpenAi)
}

#[derive(Debug, Default)]
struct Bucket {
    cost: f64,
    rows: u32,
    requests: u32,
    tokens: TokenSums,
}

impl Bucket {
    fn add(&mut self, row: &UsageRow) {
        self.cost += row.cost;
        self.rows = self.rows.saturating_add(1);
        self.requests = self.requests.saturating_add(row.request_count);
        self.tokens.add(row.tokens.as_ref());
    }
}

/// Codex-convention counts for recorded OpenCode tokens. OpenCode keeps cache
/// counts apart from `input` and reasoning apart from `output`; Codex folds
/// both in. `None` when no row had usable tokens or a sum overflows.
fn codex_token_counts(tokens: &TokenSums) -> Option<ModelTokenCounts> {
    if !tokens.has_usable_rows() {
        return None;
    }
    let input = tokens
        .input
        .checked_add(tokens.cache_read)?
        .checked_add(tokens.cache_write)?;
    Some(ModelTokenCounts {
        input_tokens: input,
        output_tokens: tokens.output.checked_add(tokens.reasoning)?,
        cached_tokens: tokens.cache_read,
        reasoning_tokens: tokens.complete_reasoning(),
    })
}

fn add_counts(target: &mut ModelTokenCounts, add: &ModelTokenCounts) {
    target.input_tokens = target.input_tokens.saturating_add(add.input_tokens);
    target.output_tokens = target.output_tokens.saturating_add(add.output_tokens);
    target.cached_tokens = target.cached_tokens.saturating_add(add.cached_tokens);
    target.reasoning_tokens = match (target.reasoning_tokens, add.reasoning_tokens) {
        (Some(left), Some(right)) => Some(left.saturating_add(right)),
        _ => None,
    };
}

fn row_day(row: &UsageRow, zone: CostTimeZone) -> Option<NaiveDate> {
    DateTime::from_timestamp_millis(row.created_ms).map(|at| zone.date(at))
}

/// Merge rows recorded on `[start, end]` reporting days into a Codex summary.
/// Rows whose tokens are unusable count as unknown, so the summary's token
/// totals stop claiming to be exact; their costs still count.
pub(crate) fn merge_rows_into_summary(
    rows: &[UsageRow],
    zone: CostTimeZone,
    start: NaiveDate,
    end: NaiveDate,
    summary: &mut CostSummary,
) {
    let mut total = Bucket::default();
    let mut by_model: HashMap<String, Bucket> = HashMap::new();
    for row in rows {
        let Some(day) = row_day(row, zone) else {
            continue;
        };
        if day < start || day > end {
            continue;
        }
        total.add(row);
        by_model
            .entry(model_name(&row.model).to_string())
            .or_default()
            .add(row);
    }
    if total.rows == 0 {
        return;
    }

    summary.total_cost_usd += total.cost;
    summary.sessions_count = summary.sessions_count.saturating_add(total.requests);
    summary.recorded_cost_rows = summary.recorded_cost_rows.saturating_add(total.rows);
    if let Some(counts) = codex_token_counts(&total.tokens) {
        summary.input_tokens = summary.input_tokens.saturating_add(counts.input_tokens);
        summary.output_tokens = summary.output_tokens.saturating_add(counts.output_tokens);
        summary.cached_tokens = summary.cached_tokens.saturating_add(counts.cached_tokens);
    }
    // Native Codex rows leave reasoning unknown, so the merged window cannot
    // claim an exact reasoning total either.
    summary.reasoning_tokens = None;
    summary.unknown_token_rows = summary
        .unknown_token_rows
        .saturating_add(total.tokens.unknown_token_rows());

    for (model, bucket) in by_model {
        *summary.by_model.entry(model.clone()).or_insert(0.0) += bucket.cost;
        if let Some(counts) = codex_token_counts(&bucket.tokens) {
            match summary.by_model_tokens.get_mut(&model) {
                Some(existing) => add_counts(existing, &counts),
                None => {
                    summary.by_model_tokens.insert(model.clone(), counts);
                }
            }
        }
        let unknown = bucket.tokens.unknown_token_rows();
        if unknown > 0 {
            let slot = summary
                .unknown_token_rows_by_model
                .entry(model)
                .or_insert(0);
            *slot = slot.saturating_add(unknown);
        }
    }
}

/// Add rows to calendar-preserving Codex daily slots. Only days already in the
/// slices are touched. A day Codex marks unknown (`None`) stays unknown, because
/// its native share is not established. Returns true when some touched day had
/// rows without exact tokens, so the token series is incomplete.
pub(crate) fn merge_rows_into_daily(
    rows: &[UsageRow],
    zone: CostTimeZone,
    costs: &mut [(String, Option<f64>)],
    tokens: &mut [(String, u64)],
) -> bool {
    let mut by_day: BTreeMap<String, Bucket> = BTreeMap::new();
    for row in rows {
        let Some(day) = row_day(row, zone) else {
            continue;
        };
        by_day
            .entry(day.format("%Y-%m-%d").to_string())
            .or_default()
            .add(row);
    }

    let mut tokens_incomplete = false;
    for (day_key, slot) in costs.iter_mut() {
        if let (Some(bucket), Some(cost)) = (by_day.get(day_key.as_str()), slot.as_mut()) {
            *cost += bucket.cost;
        }
    }
    for (day_key, slot) in tokens.iter_mut() {
        let Some(bucket) = by_day.get(day_key.as_str()) else {
            continue;
        };
        if bucket.tokens.unknown_token_rows() > 0 {
            tokens_incomplete = true;
        }
        if let Some(counts) = codex_token_counts(&bucket.tokens) {
            *slot = slot.saturating_add(counts.total_for_provider("codex"));
        }
    }
    tokens_incomplete
}

/// Add this device's OpenAI-backed OpenCode usage to a Codex summary for the
/// `[start, end]` reporting days. Returns false when a database exists but
/// could not be read, so the caller keeps coverage partial.
pub(crate) fn append_to_codex_summary(
    summary: &mut CostSummary,
    zone: CostTimeZone,
    start: NaiveDate,
    end: NaiveDate,
) -> bool {
    match scan_available_rows() {
        Ok(Some(rows)) => {
            merge_rows_into_summary(&rows, zone, start, end, summary);
            true
        }
        Ok(None) => true,
        Err(()) => {
            tracing::debug!("OpenCode local history for Codex is unreadable; skipped");
            false
        }
    }
}

/// [`merge_rows_into_daily`] over the local OpenCode install. An unreadable
/// database leaves the slots alone and reports the token series incomplete.
pub(crate) fn append_to_codex_daily(
    zone: CostTimeZone,
    costs: &mut [(String, Option<f64>)],
    tokens: &mut [(String, u64)],
) -> bool {
    match scan_available_rows() {
        Ok(Some(rows)) => merge_rows_into_daily(&rows, zone, costs, tokens),
        Ok(None) => false,
        Err(()) => {
            tracing::debug!("OpenCode local history for Codex is unreadable; skipped");
            true
        }
    }
}
