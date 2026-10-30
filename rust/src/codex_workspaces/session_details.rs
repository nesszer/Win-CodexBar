//! Request-level usage of one Codex conversation (upstream #4730).
//!
//! Read on demand when a session is opened in Usage & Spend, never during the
//! usage refresh, and never persisted. The rows come from the same rollout
//! files and the same parser the session summary is built from. The caller
//! passes the summary's own accounting as a checkpoint: a read that does not
//! reproduce it exactly is refused, so newer rows never show up under an
//! older summary.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::core::{CodexParseMode, CostUsagePricing, JsonlScanner};

use super::indexer::{
    CodexWorkspacesIndex, codex_cost_on_day, list_rollout_files, rollout_session_id,
};
use super::types::{CostEstimate, UsageTotals};

/// Requests returned for one conversation. Larger sessions fail instead of
/// being truncated, so totals never silently disagree with the summary.
pub const SESSION_DETAIL_MAX_REQUESTS: usize = 5_000;
/// Rollout bytes read for one conversation. Raw logs also carry tool output.
pub const SESSION_DETAIL_MAX_BYTES: u64 = 256 * 1024 * 1024;

/// Relative tolerance for comparing summed USD values across reads.
const COST_TOLERANCE: f64 = 1e-9;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SessionDetailError {
    #[error("unavailable")]
    Unavailable,
    #[error("sourceChanged")]
    SourceChanged,
    #[error("limitExceeded")]
    LimitExceeded,
}

impl SessionDetailError {
    /// Stable code the desktop bridge returns as the rejection message.
    pub fn code(self) -> &'static str {
        match self {
            Self::Unavailable => "unavailable",
            Self::SourceChanged => "sourceChanged",
            Self::LimitExceeded => "limitExceeded",
        }
    }
}

/// The summary accounting the details must reproduce.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionDetailCheckpoint {
    pub totals: UsageTotals,
    pub cost_estimate: CostEstimate,
}

/// One recorded model request. Input includes cached input; output includes reasoning.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexSessionRequest {
    pub id: String,
    pub model_name: String,
    pub timestamp: Option<DateTime<Utc>>,
    pub input_tokens: u64,
    pub cached_input_tokens: u64,
    pub output_tokens: u64,
    pub reasoning_tokens: Option<u64>,
    pub total_tokens: u64,
    pub cost_usd: Option<f64>,
}

/// Requests grouped by turn. `turn_id: None` holds usage no turn owns.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexSessionTurn {
    pub turn_id: Option<String>,
    pub first_activity: Option<DateTime<Utc>>,
    pub last_activity: Option<DateTime<Utc>>,
    pub input_tokens: u64,
    pub cached_input_tokens: u64,
    pub output_tokens: u64,
    /// `None` when any request in the turn did not report reasoning.
    pub reasoning_tokens: Option<u64>,
    pub total_tokens: u64,
    pub request_count: u32,
    /// `None` when any request in the turn could not be priced.
    pub cost_usd: Option<f64>,
    pub requests: Vec<CodexSessionRequest>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexSessionDetails {
    pub session_id: String,
    pub request_count: u32,
    pub turns: Vec<CodexSessionTurn>,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct SessionDetailLimits {
    pub(super) max_requests: usize,
    pub(super) max_bytes: u64,
}

impl Default for SessionDetailLimits {
    fn default() -> Self {
        Self {
            max_requests: SESSION_DETAIL_MAX_REQUESTS,
            max_bytes: SESSION_DETAIL_MAX_BYTES,
        }
    }
}

struct PricedRow {
    turn_id: Option<String>,
    request: CodexSessionRequest,
}

impl CodexWorkspacesIndex {
    /// Request-level usage of one conversation, validated against its summary.
    pub fn load_session_details(
        &self,
        session_id: &str,
        checkpoint: &SessionDetailCheckpoint,
    ) -> Result<CodexSessionDetails, SessionDetailError> {
        self.load_session_details_at(
            session_id,
            checkpoint,
            Utc::now(),
            SessionDetailLimits::default(),
        )
    }

    pub(super) fn load_session_details_at(
        &self,
        session_id: &str,
        checkpoint: &SessionDetailCheckpoint,
        now: DateTime<Utc>,
        limits: SessionDetailLimits,
    ) -> Result<CodexSessionDetails, SessionDetailError> {
        let session_id = session_id.trim();
        if session_id.is_empty() {
            return Err(SessionDetailError::Unavailable);
        }
        let scope = self.scope().map_err(|_| SessionDetailError::Unavailable)?;
        let zone = crate::cost_reporting_period::cost_bucket_zone();
        let range = self.history_range(zone.date(now));
        // Every rollout the summary could have attributed to this session,
        // decided by the same first-line identity the indexer uses.
        let owned: Vec<PathBuf> = list_rollout_files(&scope, &range)
            .into_iter()
            .filter(|path| rollout_session_id(path).is_some_and(|(id, _)| id == session_id))
            .collect();
        if owned.is_empty() {
            return Err(SessionDetailError::Unavailable);
        }

        // Lengths are captured before any parse and each parse stops there,
        // so an appending rollout cannot grow the read past the budget.
        let mut sized = Vec::with_capacity(owned.len());
        let mut total_bytes = 0u64;
        for path in owned {
            let len = fs::metadata(&path)
                .map_err(|_| SessionDetailError::Unavailable)?
                .len();
            total_bytes = total_bytes.saturating_add(len);
            if total_bytes > limits.max_bytes {
                return Err(SessionDetailError::LimitExceeded);
            }
            sized.push((path, len));
        }

        let mut totals = UsageTotals::default();
        let mut cost = CostEstimate::default();
        let mut rows: Vec<PricedRow> = Vec::new();
        for (path, len) in sized {
            let target = i64::try_from(len).map_err(|_| SessionDetailError::LimitExceeded)?;
            let remaining = limits.max_requests.saturating_sub(rows.len());
            let parsed = JsonlScanner::parse_codex_bounded(
                &path,
                &range,
                CodexParseMode::Standard {
                    start_offset: 0,
                    initial_model: None,
                    initial_totals: None,
                    previous_token_timestamp: None,
                    token_timestamps_monotonic: None,
                },
                None,
                Some(target),
                None,
                Some(remaining),
            )
            .map_err(|_| SessionDetailError::Unavailable)?;
            // A capped parse holds one record past the budget and is never
            // checkpoint-validated: it returns here.
            if parsed.records.len() > remaining {
                return Err(SessionDetailError::LimitExceeded);
            }
            // Summed per file, then per session, in the indexer's order.
            let mut file_totals = UsageTotals::default();
            let mut file_cost = CostEstimate::default();
            for (record, _) in parsed.records {
                let input = record.input.max(0) as u64;
                let cached = (record.cached.max(0) as u64).min(input);
                let output = record.output.max(0) as u64;
                file_totals.add_assign(&UsageTotals::from_parts(input, cached, output));
                let model = CostUsagePricing::normalize_codex_model(&record.model);
                let tokens = input.saturating_add(output);
                let usd = codex_cost_on_day(&record.day_key, &model, input, cached, output)
                    .filter(|usd| usd.is_finite() && *usd >= 0.0);
                match usd {
                    Some(usd) => file_cost.known_usd += usd,
                    None => {
                        file_cost.unknown_tokens = file_cost.unknown_tokens.saturating_add(tokens)
                    }
                }
                rows.push(PricedRow {
                    turn_id: record
                        .turn_id
                        .map(|id| id.trim().to_string())
                        .filter(|id| !id.is_empty()),
                    request: CodexSessionRequest {
                        id: String::new(),
                        model_name: model,
                        timestamp: record.timestamp,
                        input_tokens: input,
                        cached_input_tokens: cached,
                        output_tokens: output,
                        reasoning_tokens: record.reasoning.map(|r| r.max(0) as u64),
                        total_tokens: tokens,
                        cost_usd: usd,
                    },
                });
            }
            totals.add_assign(&file_totals);
            cost.add_assign(&file_cost);
        }

        if !matches_checkpoint(&totals, &cost, checkpoint) {
            return Err(SessionDetailError::SourceChanged);
        }

        #[allow(
            clippy::cast_possible_truncation,
            reason = "request count is capped by the request limit"
        )]
        let request_count = rows.len() as u32;
        Ok(CodexSessionDetails {
            session_id: session_id.to_string(),
            request_count,
            turns: build_turns(rows),
        })
    }
}

fn matches_checkpoint(
    totals: &UsageTotals,
    cost: &CostEstimate,
    checkpoint: &SessionDetailCheckpoint,
) -> bool {
    let expected = &checkpoint.cost_estimate;
    let tolerance = COST_TOLERANCE * expected.known_usd.abs().max(1.0);
    *totals == checkpoint.totals
        && cost.unknown_tokens == expected.unknown_tokens
        && (cost.known_usd - expected.known_usd).abs() <= tolerance
}

/// Turns ordered by first activity, each with its requests in time order;
/// unattributed usage is a final group, never an invented turn.
fn build_turns(rows: Vec<PricedRow>) -> Vec<CodexSessionTurn> {
    let mut by_turn: HashMap<String, Vec<CodexSessionRequest>> = HashMap::new();
    let mut unattributed = Vec::new();
    for row in rows {
        match row.turn_id {
            Some(id) => by_turn.entry(id).or_default().push(row.request),
            None => unattributed.push(row.request),
        }
    }
    let mut turns: Vec<CodexSessionTurn> = by_turn
        .into_iter()
        .map(|(id, requests)| build_turn(Some(id), requests))
        .collect();
    turns.sort_by(|a, b| {
        cmp_missing_last(a.first_activity, b.first_activity).then_with(|| a.turn_id.cmp(&b.turn_id))
    });
    if !unattributed.is_empty() {
        let mut turn = build_turn(None, unattributed);
        turn.first_activity = None;
        turn.last_activity = None;
        turns.push(turn);
    }
    turns
}

fn build_turn(turn_id: Option<String>, mut requests: Vec<CodexSessionRequest>) -> CodexSessionTurn {
    requests.sort_by(|a, b| cmp_missing_last(a.timestamp, b.timestamp));
    for (index, request) in requests.iter_mut().enumerate() {
        request.id = format!("request-{index}");
    }
    let mut turn = CodexSessionTurn {
        turn_id,
        first_activity: None,
        last_activity: None,
        input_tokens: 0,
        cached_input_tokens: 0,
        output_tokens: 0,
        reasoning_tokens: Some(0),
        total_tokens: 0,
        request_count: 0,
        cost_usd: Some(0.0),
        requests: Vec::new(),
    };
    for request in &requests {
        if let Some(at) = request.timestamp {
            turn.first_activity = Some(turn.first_activity.map_or(at, |t| t.min(at)));
            turn.last_activity = Some(turn.last_activity.map_or(at, |t| t.max(at)));
        }
        turn.input_tokens = turn.input_tokens.saturating_add(request.input_tokens);
        turn.cached_input_tokens = turn
            .cached_input_tokens
            .saturating_add(request.cached_input_tokens);
        turn.output_tokens = turn.output_tokens.saturating_add(request.output_tokens);
        turn.total_tokens = turn.total_tokens.saturating_add(request.total_tokens);
        turn.reasoning_tokens = turn
            .reasoning_tokens
            .zip(request.reasoning_tokens)
            .map(|(sum, value)| sum.saturating_add(value));
        turn.cost_usd = turn
            .cost_usd
            .zip(request.cost_usd)
            .map(|(sum, value)| sum + value);
        turn.request_count = turn.request_count.saturating_add(1);
    }
    turn.requests = requests;
    turn
}

fn cmp_missing_last<T: Ord>(a: Option<T>, b: Option<T>) -> std::cmp::Ordering {
    match (a, b) {
        (Some(a), Some(b)) => a.cmp(&b),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    }
}

#[cfg(test)]
mod tests;
