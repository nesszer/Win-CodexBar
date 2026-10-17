//! Native Codex turn performance per conversation (upstream #4304).
//!
//! A completed turn becomes a sample only when its evidence agrees: a
//! successful `task_complete` with a positive duration, owned
//! `token_usage_record` requests whose output sums to the turn's reported
//! total, and no contradictory or unparseable usage for that turn. Turn output
//! includes reasoning, tools and waits, so these figures describe observed
//! turns. They are not a measure of model generation speed.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::codex_costs::identified_requests::{
    JsonlLine, Usage, non_empty_str, read_line, request_usage, timestamp_unix_ms, totals_at_least,
    turn_id,
};
use crate::core::{CostUsageDayRange, CostUsagePricing};

/// Nearest-rank P95 needs this many observations (upstream).
const P95_MIN_SAMPLES: usize = 20;
/// Nearest-rank quartiles need this many observations (upstream).
const QUARTILE_MIN_SAMPLES: usize = 4;
/// A line can change turn state only if it mentions one of these event types.
const RELEVANT_MARKERS: [&str; 4] = [
    "task_started",
    "task_complete",
    "turn_context",
    "token_usage_record",
];

/// One completed turn whose timing and token evidence agree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TurnSample {
    pub(crate) turn_id: String,
    pub(crate) output_tokens: u64,
    pub(crate) duration_ms: u64,
    pub(crate) first_token_ms: Option<u64>,
    pub(crate) model: Option<String>,
    pub(crate) reasoning_effort: Option<String>,
    pub(crate) input_tokens: Option<u64>,
    pub(crate) cached_input_tokens: Option<u64>,
}

/// Throughput is weighted by elapsed time. Duration and first-token time are medians.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexTurnPerformanceSummary {
    pub sample_count: usize,
    pub output_tokens_per_second: f64,
    pub median_duration_milliseconds: f64,
    pub median_first_token_milliseconds: Option<f64>,
    pub first_token_sample_count: usize,
    pub details: CodexTurnPerformanceDetails,
}

/// Spread and grouping for the performance details disclosure.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexTurnPerformanceDetails {
    pub p95_duration_milliseconds: Option<f64>,
    pub p95_first_token_milliseconds: Option<f64>,
    pub output_rate_lower_quartile: Option<f64>,
    pub output_rate_upper_quartile: Option<f64>,
    pub cached_input_fraction: Option<f64>,
    pub cache_sample_count: usize,
    pub groups: Vec<CodexTurnPerformanceGroup>,
}

/// Turns that share one model and reasoning effort.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexTurnPerformanceGroup {
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub sample_count: usize,
    pub first_token_sample_count: usize,
    pub median_first_token_milliseconds: Option<f64>,
    pub median_duration_milliseconds: f64,
    pub output_tokens_per_second: f64,
}

impl CodexTurnPerformanceSummary {
    /// `None` when there are no samples or the timed duration sums to zero.
    pub(crate) fn from_samples(samples: &[TurnSample]) -> Option<Self> {
        if samples.is_empty() {
            return None;
        }
        let output = checked_sum(samples.iter().map(|sample| sample.output_tokens))?;
        let duration = checked_sum(samples.iter().map(|sample| sample.duration_ms))?;
        if duration == 0 {
            return None;
        }
        let durations = samples
            .iter()
            .map(|sample| sample.duration_ms as f64)
            .collect::<Vec<_>>();
        let first_tokens = first_token_values(samples);
        Some(Self {
            sample_count: samples.len(),
            output_tokens_per_second: rate(output, duration),
            median_duration_milliseconds: median(&durations).unwrap_or(0.0),
            median_first_token_milliseconds: median(&first_tokens),
            first_token_sample_count: first_tokens.len(),
            details: CodexTurnPerformanceDetails::from_samples(samples),
        })
    }
}

impl CodexTurnPerformanceDetails {
    fn from_samples(samples: &[TurnSample]) -> Self {
        let durations = samples
            .iter()
            .map(|sample| sample.duration_ms as f64)
            .collect::<Vec<_>>();
        let first_tokens = first_token_values(samples);
        let rates = samples
            .iter()
            .map(|sample| rate(sample.output_tokens, sample.duration_ms))
            .collect::<Vec<_>>();
        let cache = samples
            .iter()
            .filter(|sample| sample.input_tokens.is_some() && sample.cached_input_tokens.is_some())
            .collect::<Vec<_>>();
        let cached_input_fraction = match (
            checked_sum(cache.iter().filter_map(|sample| sample.input_tokens)),
            checked_sum(cache.iter().filter_map(|sample| sample.cached_input_tokens)),
        ) {
            (Some(input), Some(cached)) if input > 0 => Some(cached as f64 / input as f64),
            _ => None,
        };
        Self {
            p95_duration_milliseconds: nearest_rank(&durations, 0.95, P95_MIN_SAMPLES),
            p95_first_token_milliseconds: nearest_rank(&first_tokens, 0.95, P95_MIN_SAMPLES),
            output_rate_lower_quartile: nearest_rank(&rates, 0.25, QUARTILE_MIN_SAMPLES),
            output_rate_upper_quartile: nearest_rank(&rates, 0.75, QUARTILE_MIN_SAMPLES),
            cached_input_fraction,
            cache_sample_count: cache.len(),
            groups: group_samples(samples),
        }
    }
}

fn group_samples(samples: &[TurnSample]) -> Vec<CodexTurnPerformanceGroup> {
    // BTreeMap order on `Option` puts unknown model and effort first, as the upstream sort does.
    let mut grouped: BTreeMap<(Option<String>, Option<String>), Vec<&TurnSample>> = BTreeMap::new();
    for sample in samples {
        grouped
            .entry((sample.model.clone(), sample.reasoning_effort.clone()))
            .or_default()
            .push(sample);
    }
    grouped
        .into_iter()
        .filter_map(|((model, reasoning_effort), observations)| {
            let output = checked_sum(observations.iter().map(|sample| sample.output_tokens))?;
            let duration = checked_sum(observations.iter().map(|sample| sample.duration_ms))?;
            if duration == 0 {
                return None;
            }
            let durations = observations
                .iter()
                .map(|sample| sample.duration_ms as f64)
                .collect::<Vec<_>>();
            let first_tokens = observations
                .iter()
                .filter_map(|sample| sample.first_token_ms.map(|ms| ms as f64))
                .collect::<Vec<_>>();
            Some(CodexTurnPerformanceGroup {
                model,
                reasoning_effort,
                sample_count: observations.len(),
                first_token_sample_count: first_tokens.len(),
                median_first_token_milliseconds: median(&first_tokens),
                median_duration_milliseconds: median(&durations).unwrap_or(0.0),
                output_tokens_per_second: rate(output, duration),
            })
        })
        .collect()
}

fn first_token_values(samples: &[TurnSample]) -> Vec<f64> {
    samples
        .iter()
        .filter_map(|sample| sample.first_token_ms.map(|ms| ms as f64))
        .collect()
}

fn checked_sum(mut values: impl Iterator<Item = u64>) -> Option<u64> {
    values.try_fold(0_u64, |total, value| total.checked_add(value))
}

fn rate(output: u64, duration_ms: u64) -> f64 {
    output as f64 / duration_ms as f64 * 1000.0
}

fn sorted(values: &[f64]) -> Vec<f64> {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    sorted
}

/// Even counts average the two middle values, matching the upstream halving.
fn median(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let sorted = sorted(values);
    let middle = sorted.len() / 2;
    Some(if sorted.len().is_multiple_of(2) {
        sorted[middle - 1] / 2.0 + sorted[middle] / 2.0
    } else {
        sorted[middle]
    })
}

/// Nearest-rank percentile; `None` below the minimum observation count.
fn nearest_rank(values: &[f64], fraction: f64, minimum: usize) -> Option<f64> {
    if values.len() < minimum {
        return None;
    }
    let sorted = sorted(values);
    #[expect(
        clippy::cast_possible_truncation,
        reason = "ceil(len * fraction) with 0 < fraction <= 1 is at most len"
    )]
    let rank = (sorted.len() as f64 * fraction).ceil() as usize;
    sorted.get(rank.checked_sub(1)?).copied()
}

/// Completed-turn samples from one rollout file that belongs to `session_id`.
///
/// Returns nothing when the file cannot be read or when a turn-relevant line is
/// too large to inspect: a partial record could change the outcome of any turn.
pub(crate) fn scan_session_turns(
    path: &Path,
    session_id: &str,
    range: &CostUsageDayRange,
) -> Vec<TurnSample> {
    let Ok(file) = File::open(path) else {
        return Vec::new();
    };
    let mut reader = BufReader::new(file);
    let mut consumed = 0_u64;
    let mut scan = TurnScan::new(session_id);
    loop {
        match read_line(&mut reader, &mut consumed) {
            Ok(Some(JsonlLine::Complete(bytes))) => scan.observe_line(&bytes),
            Ok(Some(JsonlLine::Oversized(prefix))) => {
                let prefix = String::from_utf8_lossy(&prefix);
                if RELEVANT_MARKERS
                    .iter()
                    .any(|marker| prefix.contains(marker))
                {
                    return Vec::new();
                }
            }
            Ok(None) => break,
            Err(_) => return Vec::new(),
        }
    }
    scan.finish(range)
}

#[derive(Default)]
struct TurnState {
    reported_output: Option<u64>,
    rejected: bool,
    completion: Option<Completion>,
    effort: Option<String>,
    conflicting_effort: bool,
}

impl TurnState {
    /// Mirrors upstream: a later absent effort is a conflict once an effort was seen.
    fn observe_effort(&mut self, effort: Option<String>) {
        if self.effort.is_some() && self.effort != effort {
            self.conflicting_effort = true;
        }
        self.effort = effort;
    }
}

#[derive(Clone)]
struct Completion {
    completed_at_ms: Option<i64>,
    started_at_ms: Option<i64>,
    duration_ms: Option<u64>,
    first_token_ms: Option<u64>,
}

/// Owned request rows for one turn, summed across unique responses.
struct TurnRows {
    output: u64,
    input: u64,
    cached: u64,
    valid: bool,
    valid_cache: bool,
    first_timestamp_ms: Option<i64>,
    last_timestamp_ms: Option<i64>,
    models: BTreeSet<String>,
}

impl TurnRows {
    fn new() -> Self {
        Self {
            output: 0,
            input: 0,
            cached: 0,
            valid: true,
            valid_cache: true,
            first_timestamp_ms: None,
            last_timestamp_ms: None,
            models: BTreeSet::new(),
        }
    }

    fn add(&mut self, usage: Usage, timestamp_ms: Option<i64>, model: String) {
        let Ok(output) = u64::try_from(usage.2) else {
            self.valid = false;
            return;
        };
        match self.output.checked_add(output) {
            Some(total) => self.output = total,
            None => {
                self.valid = false;
                return;
            }
        }
        match (u64::try_from(usage.0), u64::try_from(usage.1)) {
            (Ok(input), Ok(cached)) if cached <= input => {
                match (
                    self.input.checked_add(input),
                    self.cached.checked_add(cached),
                ) {
                    (Some(total_input), Some(total_cached)) => {
                        self.input = total_input;
                        self.cached = total_cached;
                    }
                    _ => self.valid_cache = false,
                }
            }
            _ => self.valid_cache = false,
        }
        match timestamp_ms {
            Some(timestamp) => {
                self.first_timestamp_ms = Some(
                    self.first_timestamp_ms
                        .map_or(timestamp, |first| first.min(timestamp)),
                );
                self.last_timestamp_ms = Some(
                    self.last_timestamp_ms
                        .map_or(timestamp, |last| last.max(timestamp)),
                );
            }
            None => self.valid = false,
        }
        self.models.insert(model);
    }
}

struct UsageRecord {
    response_id: String,
    timestamp_ms: Option<i64>,
    usage: Usage,
    turn_total: Option<Usage>,
    model: Option<String>,
}

fn parse_usage_record(object: &Value, payload: &Value) -> Option<UsageRecord> {
    let response_id = non_empty_str(payload.get("response_id"))?;
    let timestamp = object.get("timestamp")?.as_str()?;
    let usage = request_usage(payload.get("usage"))?;
    let thread_total = request_usage(payload.get("thread_token_usage"))?;
    if !totals_at_least(thread_total, usage) {
        return None;
    }
    let turn_total = request_usage(payload.get("turn_token_usage"))
        .filter(|total| totals_at_least(*total, usage));
    Some(UsageRecord {
        response_id: response_id.to_string(),
        timestamp_ms: timestamp_unix_ms(timestamp),
        usage,
        turn_total,
        model: non_empty_str(payload.get("model")).and_then(model_evidence),
    })
}

struct TurnScan<'a> {
    session_id: &'a str,
    partial: bool,
    current_turn: Option<String>,
    active_turn: Option<String>,
    current_model: Option<String>,
    turn_models: HashMap<String, String>,
    states: HashMap<String, TurnState>,
    rows: HashMap<String, TurnRows>,
    responses: HashMap<String, (String, Usage)>,
}

static EMPTY_VALUE: Value = Value::Null;

fn payload_of(object: &Value) -> &Value {
    object.get("payload").unwrap_or(&EMPTY_VALUE)
}

impl<'a> TurnScan<'a> {
    fn new(session_id: &'a str) -> Self {
        Self {
            session_id,
            partial: false,
            current_turn: None,
            active_turn: None,
            current_model: None,
            turn_models: HashMap::new(),
            states: HashMap::new(),
            rows: HashMap::new(),
            responses: HashMap::new(),
        }
    }

    fn observe_line(&mut self, bytes: &[u8]) {
        let Ok(text) = std::str::from_utf8(bytes) else {
            return;
        };
        if !RELEVANT_MARKERS.iter().any(|marker| text.contains(marker)) {
            return;
        }
        // A malformed line (for example a rollout still being written) is skipped, not fatal.
        let Ok(object) = serde_json::from_str::<Value>(text) else {
            return;
        };
        match object.get("type").and_then(Value::as_str) {
            Some("turn_context") => self.observe_turn_context(&object),
            Some("token_usage_record") => self.observe_usage_record(&object),
            Some("event_msg") => {
                let payload = payload_of(&object);
                match payload.get("type").and_then(Value::as_str) {
                    Some("task_started") => self.observe_task_started(payload),
                    Some("task_complete") => self.observe_task_complete(&object),
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn observe_turn_context(&mut self, object: &Value) {
        let payload = payload_of(object);
        // An explicitly blank model clears stale evidence; an omitted field keeps it.
        if let Some(raw) = payload.get("model").or_else(|| payload.get("model_name")) {
            self.current_model = raw.as_str().and_then(model_evidence);
        }
        let Some(turn) = turn_id(payload) else {
            return;
        };
        if let Some(model) = &self.current_model {
            self.turn_models.insert(turn.clone(), model.clone());
        }
        let effort = payload
            .get("effort")
            .and_then(Value::as_str)
            .and_then(effort_evidence);
        self.states
            .entry(turn.clone())
            .or_default()
            .observe_effort(effort);
        self.active_turn = Some(turn);
    }

    fn observe_task_started(&mut self, payload: &Value) {
        let turn = turn_id(payload);
        if let Some(turn) = &turn
            && let Some(state) = self.states.get_mut(turn)
        {
            // A restarted turn invalidates any completion seen for its earlier run.
            state.completion = None;
        }
        self.current_turn = turn;
    }

    fn observe_task_complete(&mut self, object: &Value) {
        let payload = payload_of(object);
        let Some(turn) = non_empty_str(payload.get("turn_id")).map(str::to_string) else {
            return;
        };
        let succeeded = payload.get("error").is_none_or(Value::is_null);
        let duration = performance_integer(payload.get("duration_ms"));
        let started_seconds = performance_integer(payload.get("started_at"));
        let started_at_ms = started_seconds
            .and_then(|seconds| i64::try_from(seconds).ok())
            .and_then(|seconds| seconds.checked_mul(1000));
        let valid_start =
            payload.get("started_at").is_none_or(Value::is_null) || started_at_ms.is_some();
        let duration_ms = (succeeded && valid_start)
            .then_some(duration)
            .flatten()
            .filter(|ms| *ms > 0);
        let completion = Completion {
            completed_at_ms: object
                .get("timestamp")
                .and_then(Value::as_str)
                .and_then(timestamp_unix_ms),
            started_at_ms,
            duration_ms,
            first_token_ms: performance_integer(payload.get("time_to_first_token_ms")),
        };
        self.states.entry(turn).or_default().completion = Some(completion);
    }

    fn observe_usage_record(&mut self, object: &Value) {
        let payload = payload_of(object);
        if non_empty_str(payload.get("thread_id")) != Some(self.session_id) {
            return;
        }
        if payload
            .get("session_id")
            .is_some_and(|raw| non_empty_str(Some(raw)) != Some(self.session_id))
        {
            return;
        }
        let turn = turn_id(payload)
            .or_else(|| self.current_turn.clone())
            .or_else(|| self.active_turn.clone());
        let Some(record) = parse_usage_record(object, payload) else {
            // Owned but unusable usage makes its turn unprovable.
            if let Some(turn) = turn {
                self.states.entry(turn).or_default().rejected = true;
            }
            return;
        };
        self.accept(record, turn);
    }

    fn accept(&mut self, record: UsageRecord, turn: Option<String>) {
        let Some(turn) = turn else {
            return;
        };
        if let Some((seen_turn, seen_usage)) = self.responses.get(&record.response_id) {
            if *seen_turn == turn && *seen_usage == record.usage {
                // A replayed copy of the same response is counted once.
                return;
            }
            // Contradictory copies of one response make both turns unprovable.
            let seen_turn = seen_turn.clone();
            self.states.entry(seen_turn).or_default().rejected = true;
            self.states.entry(turn).or_default().rejected = true;
            return;
        }
        self.responses
            .insert(record.response_id.clone(), (turn.clone(), record.usage));

        let model = record
            .model
            .clone()
            .or_else(|| self.turn_models.get(&turn).cloned())
            .unwrap_or_else(|| CostUsagePricing::CODEX_UNATTRIBUTED_MODEL.to_string());
        self.rows
            .entry(turn.clone())
            .or_insert_with(TurnRows::new)
            .add(record.usage, record.timestamp_ms, model);
        // Upstream: the last accepted record's turn total is the reported output.
        let reported = record
            .turn_total
            .and_then(|total| u64::try_from(total.2).ok());
        self.states.entry(turn).or_default().reported_output = reported;
    }

    fn finish(self, range: &CostUsageDayRange) -> Vec<TurnSample> {
        if self.partial {
            return Vec::new();
        }
        let mut turns = self.states.keys().cloned().collect::<Vec<_>>();
        turns.sort();
        turns
            .into_iter()
            .filter_map(|turn| self.finish_turn(&turn, range))
            .collect()
    }

    fn finish_turn(&self, turn: &str, range: &CostUsageDayRange) -> Option<TurnSample> {
        let state = self.states.get(turn)?;
        if state.rejected {
            return None;
        }
        let completion = state.completion.as_ref()?;
        let duration_ms = completion.duration_ms?;
        let rows = self.rows.get(turn)?;
        if !rows.valid || rows.output != state.reported_output? {
            return None;
        }
        let completed_at_ms = completion.completed_at_ms?;
        let last = rows.last_timestamp_ms?;
        let first = rows.first_timestamp_ms?;
        if last > completed_at_ms {
            return None;
        }
        if completion
            .started_at_ms
            .is_some_and(|started| first < started)
        {
            return None;
        }
        let day = bucket_day(completed_at_ms)?;
        if !CostUsageDayRange::is_in_range(&day, &range.since_key, &range.until_key) {
            return None;
        }
        let model = match rows.models.len() {
            1 => rows
                .models
                .iter()
                .next()
                .filter(|model| model.as_str() != CostUsagePricing::CODEX_UNATTRIBUTED_MODEL)
                .cloned(),
            _ => None,
        };
        let (input_tokens, cached_input_tokens) = if rows.valid_cache {
            (Some(rows.input), Some(rows.cached))
        } else {
            (None, None)
        };
        Some(TurnSample {
            turn_id: turn.to_string(),
            output_tokens: rows.output,
            duration_ms,
            first_token_ms: completion.first_token_ms.filter(|ms| *ms <= duration_ms),
            model,
            reasoning_effort: if state.conflicting_effort {
                None
            } else {
                state.effort.clone()
            },
            input_tokens,
            cached_input_tokens,
        })
    }
}

/// Local calendar day of a completion, using the same zone as the cost buckets.
fn bucket_day(milliseconds: i64) -> Option<String> {
    let instant: DateTime<Utc> = DateTime::from_timestamp_millis(milliseconds)?;
    Some(
        crate::cost_reporting_period::cost_bucket_zone()
            .date(instant)
            .format("%Y-%m-%d")
            .to_string(),
    )
}

fn model_evidence(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    (!trimmed.is_empty()).then(|| CostUsagePricing::normalize_codex_model(trimmed))
}

fn effort_evidence(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// Non-negative whole number; booleans and fractions are rejected, whole floats accepted.
fn performance_integer(value: Option<&Value>) -> Option<u64> {
    let number = value?.as_number()?;
    if let Some(whole) = number.as_u64() {
        return Some(whole);
    }
    let float = number.as_f64()?;
    #[expect(
        clippy::cast_possible_truncation,
        reason = "finite, whole, non-negative, and below i64::MAX before the cast"
    )]
    let whole = float as u64;
    (float.is_finite() && float >= 0.0 && float.fract() == 0.0 && float < i64::MAX as f64)
        .then_some(whole)
}

#[cfg(test)]
mod tests;
