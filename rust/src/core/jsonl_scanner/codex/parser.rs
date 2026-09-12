use super::helpers::*;
use super::{CodexTotals, CodexUsageRecord, CostUsageDayRange};
use crate::core::CostUsagePricing;
use chrono::DateTime;
use serde_json::Value;

pub(super) struct CodexParserState {
    pub(super) current_model: Option<String>,
    pub(super) previous_totals: Option<CodexTotals>,
    /// High watermark of observed cumulative totals (never lowered). Used for
    /// Ultra interleaved-lineage containment (issue #2037 Phase 1).
    totals_watermark: Option<CodexTotals>,
    /// Latched once any cumulative component drops below the watermark.
    saw_interleaved_totals: bool,
    pub(super) records: Vec<CodexUsageRecord>,
    pub(super) previous_token_timestamp: Option<String>,
    previous_token_timestamp_parsed: Option<DateTime<chrono::FixedOffset>>,
    pub(super) token_timestamps_monotonic: Option<bool>,
    pub(super) token_timestamp_comparisons: u64,
    fork_baseline: Option<CodexTotals>,
    pub(super) fork_baseline_ambiguous: bool,
}

impl CodexParserState {
    pub(super) fn new(initial_model: Option<String>, initial_totals: Option<CodexTotals>) -> Self {
        Self::with_timestamp_state(initial_model, initial_totals, None, None)
    }

    fn with_timestamp_state(
        initial_model: Option<String>,
        initial_totals: Option<CodexTotals>,
        previous_token_timestamp: Option<String>,
        token_timestamps_monotonic: Option<bool>,
    ) -> Self {
        Self::with_timestamp_state_and_fork_mode(
            initial_model,
            initial_totals,
            previous_token_timestamp,
            token_timestamps_monotonic,
            false,
        )
    }

    pub(super) fn with_timestamp_state_and_fork_mode(
        initial_model: Option<String>,
        initial_totals: Option<CodexTotals>,
        previous_token_timestamp: Option<String>,
        token_timestamps_monotonic: Option<bool>,
        fork_baseline_mode: bool,
    ) -> Self {
        let previous_token_timestamp_parsed = previous_token_timestamp
            .as_deref()
            .and_then(parse_rfc3339_timestamp);
        let fork_baseline = fork_baseline_mode.then(|| initial_totals.clone()).flatten();
        Self {
            current_model: initial_model,
            previous_totals: initial_totals.clone(),
            totals_watermark: initial_totals,
            saw_interleaved_totals: false,
            records: Vec::new(),
            previous_token_timestamp,
            previous_token_timestamp_parsed,
            // A parser always validates a fresh prefix.  `None` is only an
            // input marker for the legacy-cache path, not an output state.
            token_timestamps_monotonic: Some(token_timestamps_monotonic.unwrap_or(true)),
            token_timestamp_comparisons: 0,
            fork_baseline,
            fork_baseline_ambiguous: false,
        }
    }

    pub(super) fn process_line(&mut self, line: &str, range: &CostUsageDayRange) {
        let event_candidate = is_candidate_codex_line(line);
        let bare_candidate = !event_candidate && line.contains("\"usage\"");
        if !event_candidate && !bare_candidate {
            return;
        }

        if event_candidate && let Some(event) = parse_codex_fast_event(line) {
            self.process_fast_event(event, range);
            return;
        }

        let Ok(obj) = serde_json::from_str::<Value>(line) else {
            return;
        };

        if bare_candidate {
            if obj.get("type").is_some() {
                return;
            }
            let parsed_timestamp = obj
                .get("timestamp")
                .and_then(Value::as_str)
                .and_then(parse_codex_timestamp);
            if let Some(timestamp) = obj.get("timestamp").and_then(Value::as_str) {
                // Timestamp order is a property of the whole native file,
                // including usage records outside the requested day window.
                self.observe_token_timestamp(timestamp, parsed_timestamp.as_ref());
            }
            let day_key = parsed_timestamp
                .as_ref()
                .map(ParsedCodexTimestamp::day_key)
                .filter(|day_key| {
                    CostUsageDayRange::is_in_range(day_key, &range.since_key, &range.until_key)
                })
                .or_else(|| self.records.last().map(|record| record.day_key.clone()));
            let Some(day_key) = day_key else {
                return;
            };
            if let Some((totals, model)) = bare_usage_totals(&obj) {
                let model = self
                    .current_model
                    .as_deref()
                    .and_then(model_evidence)
                    .or(model.as_deref().and_then(model_evidence))
                    .unwrap_or(CostUsagePricing::CODEX_UNATTRIBUTED_MODEL)
                    .to_string();
                self.record_usage(
                    range,
                    day_key,
                    &model,
                    totals.input,
                    totals.cached,
                    totals.output,
                    totals.reasoning,
                );
            }
            return;
        }

        let is_token_count = token_count_payload(&obj).is_some();
        let parsed_timestamp = obj
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(parse_codex_timestamp);
        if is_token_count && let Some(timestamp) = obj.get("timestamp").and_then(Value::as_str) {
            // Timestamp order is a property of the whole native file, not
            // only the requested display window. Validate it before the
            // range filter so a cached prefix remains safe to extend.
            self.observe_token_timestamp(timestamp, parsed_timestamp.as_ref());
        }
        let Some(day_key) = parsed_timestamp
            .as_ref()
            .map(ParsedCodexTimestamp::day_key)
            .filter(|day_key| {
                CostUsageDayRange::is_in_range(day_key, &range.since_key, &range.until_key)
            })
        else {
            return;
        };
        if obj.get("type").and_then(|v| v.as_str()) == Some("turn_context") {
            self.update_current_model(&obj);
        }

        if is_token_count {
            self.record_token_count(&obj, day_key, range);
        }
    }

    fn process_fast_event(&mut self, event: CodexFastEvent<'_>, range: &CostUsageDayRange) {
        match event {
            CodexFastEvent::TurnContext { model } => {
                // Explicit blank model evidence clears stale turn context.
                if let Some(raw) = model {
                    self.current_model = model_evidence(raw).map(str::to_string);
                }
            }
            CodexFastEvent::TokenCount { timestamp, payload } => {
                let parsed_timestamp = parse_codex_timestamp(timestamp);
                self.observe_token_timestamp(timestamp, parsed_timestamp.as_ref());
                let Some(parsed_timestamp) = parsed_timestamp else {
                    return;
                };
                let day_key = parsed_timestamp.day_key();
                if !CostUsageDayRange::is_in_range(&day_key, &range.since_key, &range.until_key) {
                    return;
                }
                self.record_fast_token_count(payload, day_key, range);
            }
        }
    }

    fn update_current_model(&mut self, obj: &Value) {
        let candidates = [
            obj.get("model").and_then(|v| v.as_str()),
            obj.get("payload")
                .and_then(|payload| payload.get("model"))
                .and_then(|v| v.as_str()),
            obj.get("payload")
                .and_then(|payload| payload.get("model_name"))
                .and_then(|v| v.as_str()),
            obj.get("payload")
                .and_then(|payload| payload.get("info"))
                .and_then(|info| info.get("model"))
                .and_then(|v| v.as_str()),
            obj.get("payload")
                .and_then(|payload| payload.get("info"))
                .and_then(|info| info.get("model_name"))
                .and_then(|v| v.as_str()),
        ];
        // Only rewrite current_model when the turn_context actually carries a
        // model field (including blank, which clears stale attribution).
        let has_key = candidates.iter().any(|c| c.is_some());
        if !has_key {
            return;
        }
        self.current_model = candidates
            .into_iter()
            .flatten()
            .find_map(model_evidence)
            .map(str::to_string);
    }

    fn record_token_count(&mut self, obj: &Value, day_key: String, range: &CostUsageDayRange) {
        let Some(payload) = token_count_payload(obj) else {
            return;
        };
        let Some((delta_input, delta_cached, delta_output, reasoning)) = self.token_deltas(payload)
        else {
            return;
        };
        if delta_input == 0 && delta_cached == 0 && delta_output == 0 {
            return;
        }

        let info = payload.get("info");
        let model = self.resolve_token_model(info, payload, obj);
        self.record_usage(
            range,
            day_key,
            &model,
            delta_input,
            delta_cached,
            delta_output,
            reasoning,
        );
    }

    fn record_fast_token_count(
        &mut self,
        payload: CodexFastPayload<'_>,
        day_key: String,
        range: &CostUsageDayRange,
    ) {
        let Some((delta_input, delta_cached, delta_output, reasoning)) =
            self.fast_token_deltas(&payload)
        else {
            return;
        };
        if delta_input == 0 && delta_cached == 0 && delta_output == 0 {
            return;
        }

        let event_model = payload
            .info
            .as_ref()
            .and_then(|info| info.model.or(info.model_name))
            .or(payload.model)
            .and_then(model_evidence);
        // Prefer current turn_context model over a conflicting event model,
        // matching upstream precedence. Fall back to unattributed (not gpt-5).
        let model = self
            .current_model
            .as_deref()
            .and_then(model_evidence)
            .or(event_model)
            .unwrap_or(CostUsagePricing::CODEX_UNATTRIBUTED_MODEL)
            .to_string();
        self.record_usage(
            range,
            day_key,
            &model,
            delta_input,
            delta_cached,
            delta_output,
            reasoning,
        );
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "record construction mirrors the persisted token fields without changing parser state semantics"
    )]
    fn record_usage(
        &mut self,
        range: &CostUsageDayRange,
        day_key: String,
        model: &str,
        input: i64,
        cached: i64,
        output: i64,
        reasoning: Option<i64>,
    ) {
        if !CostUsageDayRange::is_in_range(&day_key, &range.since_key, &range.until_key) {
            return;
        }
        self.records.push(CodexUsageRecord {
            day_key,
            model: CostUsagePricing::normalize_codex_model(model),
            input,
            cached: cached.min(input),
            output,
            reasoning: clamp_reasoning(reasoning, output),
        });
    }

    fn resolve_token_model(&self, info: Option<&Value>, payload: &Value, obj: &Value) -> String {
        let event_model = info
            .and_then(|i| i.get("model").or(i.get("model_name")))
            .or_else(|| payload.get("model"))
            .or_else(|| obj.get("model"))
            .and_then(|v| v.as_str())
            .and_then(model_evidence);
        self.current_model
            .as_deref()
            .and_then(model_evidence)
            .or(event_model)
            .unwrap_or(CostUsagePricing::CODEX_UNATTRIBUTED_MODEL)
            .to_string()
    }

    fn token_deltas(&mut self, payload: &Value) -> Option<(i64, i64, i64, Option<i64>)> {
        let info = payload.get("info");
        if let Some(total) = info.and_then(|i| i.get("total_token_usage")) {
            return Some(self.total_usage_delta(total));
        }

        if let Some(last) = info.and_then(|i| i.get("last_token_usage")) {
            return Some(last_usage_delta(last));
        }

        let direct = read_token_totals(payload);
        (direct.input != 0 || direct.cached != 0 || direct.output != 0).then_some((
            direct.input.max(0),
            direct.cached.max(0),
            direct.output.max(0),
            direct.reasoning,
        ))
    }

    fn fast_token_deltas(
        &mut self,
        payload: &CodexFastPayload<'_>,
    ) -> Option<(i64, i64, i64, Option<i64>)> {
        if let Some(total) = payload
            .info
            .as_ref()
            .and_then(|info| info.total_token_usage)
        {
            return Some(self.fast_total_usage_delta(total));
        }

        if let Some(last) = payload.info.as_ref().and_then(|info| info.last_token_usage) {
            return Some(fast_last_usage_delta(last));
        }

        let direct = fast_totals_from_payload(payload);
        (direct.input != 0 || direct.cached != 0 || direct.output != 0).then_some((
            direct.input.max(0),
            direct.cached.max(0),
            direct.output.max(0),
            direct.reasoning,
        ))
    }

    pub(super) fn total_usage_delta(&mut self, total: &Value) -> (i64, i64, i64, Option<i64>) {
        let totals = read_token_totals(total);
        self.apply_totals_delta(totals)
    }

    fn fast_total_usage_delta(&mut self, total: CodexFastTotals) -> (i64, i64, i64, Option<i64>) {
        let totals = codex_totals_from_fast(total);
        self.apply_totals_delta(totals)
    }

    pub(super) fn apply_totals_delta(
        &mut self,
        totals: CodexTotals,
    ) -> (i64, i64, i64, Option<i64>) {
        self.latch_if_below_watermark(&totals);

        let delta = if self.saw_interleaved_totals {
            contained_total_delta(
                self.totals_watermark.as_ref(),
                self.previous_totals.as_ref(),
                &totals,
            )
        } else {
            let previous = self.previous_totals.as_ref();
            let input = (totals.input - previous.map_or(0, |t| t.input)).max(0);
            let cached = (totals.cached - previous.map_or(0, |t| t.cached)).max(0);
            let output = (totals.output - previous.map_or(0, |t| t.output)).max(0);
            CodexTotals {
                input,
                cached,
                output,
                reasoning: cumulative_reasoning_delta(previous, totals.reasoning, output),
            }
        };

        self.previous_totals = Some(totals.clone());
        self.raise_watermark(&totals);
        (delta.input, delta.cached, delta.output, delta.reasoning)
    }

    fn observe_token_timestamp(
        &mut self,
        timestamp: &str,
        parsed_timestamp: Option<&ParsedCodexTimestamp>,
    ) {
        let current_parsed = parsed_timestamp
            .map(|parsed| parsed.parsed)
            .unwrap_or_else(|| parse_rfc3339_timestamp(timestamp));
        if let Some(previous) = self.previous_token_timestamp.as_deref()
            && self.token_timestamps_monotonic != Some(false)
        {
            self.token_timestamp_comparisons = self.token_timestamp_comparisons.saturating_add(1);
            let ordered = match (
                self.previous_token_timestamp_parsed.as_ref(),
                current_parsed.as_ref(),
            ) {
                (Some(previous), Some(current)) => previous <= current,
                // A malformed historical timestamp keeps the scanner's
                // existing lexical fallback semantics.  The current parsed
                // value is deliberately not reparsed here.
                _ => previous <= timestamp,
            };
            if !ordered {
                self.token_timestamps_monotonic = Some(false);
            }
        }
        self.previous_token_timestamp = Some(timestamp.to_string());
        self.previous_token_timestamp_parsed = current_parsed;
    }

    fn latch_if_below_watermark(&mut self, totals: &CodexTotals) {
        if let Some(baseline) = self.fork_baseline.as_ref()
            && (totals.input < baseline.input
                || totals.cached < baseline.cached
                || totals.output < baseline.output)
        {
            self.fork_baseline_ambiguous = true;
        }
        let Some(water) = self.totals_watermark.as_ref() else {
            return;
        };
        if totals.input < water.input
            || totals.cached < water.cached
            || totals.output < water.output
        {
            self.saw_interleaved_totals = true;
        }
    }

    fn raise_watermark(&mut self, totals: &CodexTotals) {
        self.totals_watermark = Some(match self.totals_watermark.as_ref() {
            Some(water) => CodexTotals {
                input: water.input.max(totals.input),
                cached: water.cached.max(totals.cached),
                output: water.output.max(totals.output),
                reasoning: match (water.reasoning, totals.reasoning) {
                    (Some(water), Some(current)) => Some(water.max(current)),
                    (Some(water), None) => Some(water),
                    (None, Some(current)) => Some(current),
                    (None, None) => None,
                },
            },
            None => totals.clone(),
        });
    }
}
