//! Scanner-level Codex Auto-review included usage (upstream #4452, #4453).
//!
//! A `codex-auto-review` request at or after 2026-10-06T07:00Z whose rollout's
//! leaf `session_meta` names a ChatGPT creator account is priced at exactly
//! $0 and keeps its tokens. Earlier requests and API-key sessions keep the
//! existing unpriced treatment. Fixtures are synthetic.

use super::*;
use crate::core::CostUsageCache;
use serde_json::{Value, json};
use std::path::Path;

/// Timestamps on 2026-10-05 are before the cutoff in every timezone that
/// matters here; 2026-10-06T08:00Z is after it.
const BEFORE_CUTOFF: &str = "2026-10-05T12:00:00Z";
const AFTER_CUTOFF: &str = "2026-10-06T08:00:00Z";

fn session_meta(creator: Option<&str>) -> Value {
    let mut payload = json!({"id": "auto-review-session"});
    if let Some(creator) = creator {
        payload["creator_account_id"] = json!(creator);
    }
    json!({"type": "session_meta", "timestamp": BEFORE_CUTOFF, "payload": payload})
}

fn turn_context(timestamp: &str, model: &str) -> Value {
    json!({"type": "turn_context", "timestamp": timestamp, "payload": {"model": model}})
}

fn token_count(timestamp: &str, input: i64, cached: i64, output: i64) -> Value {
    json!({
        "type": "event_msg", "timestamp": timestamp,
        "payload": {"type": "token_count", "info": {"last_token_usage": {
            "input_tokens": input, "cached_input_tokens": cached, "output_tokens": output
        }}}
    })
}

/// Write one rollout under `sessions/2026/10/05` and scan a 30-day window.
fn scan_rollout(root: &Path, lines: &[Value]) -> (CostSummary, CostUsageCache) {
    let day_dir = root.join("sessions").join("2026").join("10").join("05");
    std::fs::create_dir_all(&day_dir).unwrap();
    let body: String = lines.iter().map(|line| format!("{line}\n")).collect();
    std::fs::write(
        day_dir.join("rollout-2026-10-05T12-00-00-auto-review.jsonl"),
        body,
    )
    .unwrap();
    let (summary, _stats, cache) = CostScanner::new(30)
        .with_cache_root(root.join("cache"))
        .with_sessions_dirs(vec![root.join("sessions")])
        .scan_codex_detailed_with_cache(None);
    (summary, cache)
}

fn auto_review_tokens(summary: &CostSummary) -> Option<&ModelTokenCounts> {
    summary.by_model_tokens.get("codex-auto-review")
}

#[test]
fn auto_review_after_cutoff_from_chatgpt_session_is_free_and_complete() {
    let root = tempfile::tempdir().unwrap();
    let (summary, _cache) = scan_rollout(
        root.path(),
        &[
            session_meta(Some("account-chatgpt")),
            turn_context(AFTER_CUTOFF, "codex-auto-review"),
            token_count(AFTER_CUTOFF, 100, 20, 10),
        ],
    );

    assert_eq!(
        summary.model_pricing_completeness,
        ModelPricingCompleteness::Complete
    );
    assert_eq!(summary.total_cost_usd, 0.0);
    let tokens = auto_review_tokens(&summary).expect("tokens stay in totals");
    assert_eq!(tokens.input_tokens, 100);
    assert_eq!(tokens.cached_tokens, 20);
    assert_eq!(tokens.output_tokens, 10);
}

#[test]
fn auto_review_before_cutoff_from_chatgpt_session_stays_unpriced() {
    let root = tempfile::tempdir().unwrap();
    let (summary, _cache) = scan_rollout(
        root.path(),
        &[
            session_meta(Some("account-chatgpt")),
            turn_context(BEFORE_CUTOFF, "codex-auto-review"),
            token_count(BEFORE_CUTOFF, 100, 20, 10),
        ],
    );

    assert!(summary.model_pricing_completeness.is_partial());
    assert!(matches!(
        &summary.model_pricing_completeness,
        ModelPricingCompleteness::Partial { unpriced_models }
            if unpriced_models.contains(&"codex-auto-review".to_string())
    ));
    assert_eq!(
        auto_review_tokens(&summary).map(|t| t.input_tokens),
        Some(100)
    );
}

#[test]
fn auto_review_after_cutoff_from_api_key_session_stays_unpriced() {
    let root = tempfile::tempdir().unwrap();
    let (summary, _cache) = scan_rollout(
        root.path(),
        &[
            session_meta(None),
            turn_context(AFTER_CUTOFF, "codex-auto-review"),
            token_count(AFTER_CUTOFF, 100, 20, 10),
        ],
    );

    assert!(summary.model_pricing_completeness.is_partial());
    assert_eq!(
        auto_review_tokens(&summary).map(|t| t.input_tokens),
        Some(100)
    );
}

#[test]
fn auto_review_mixed_before_and_after_cutoff_keeps_the_earlier_request_unpriced() {
    let root = tempfile::tempdir().unwrap();
    let (summary, _cache) = scan_rollout(
        root.path(),
        &[
            session_meta(Some("account-chatgpt")),
            turn_context(BEFORE_CUTOFF, "codex-auto-review"),
            token_count(BEFORE_CUTOFF, 100, 20, 10),
            token_count(AFTER_CUTOFF, 50, 0, 5),
        ],
    );

    assert!(summary.model_pricing_completeness.is_partial());
    let tokens = auto_review_tokens(&summary).expect("both requests stay in totals");
    assert_eq!(tokens.input_tokens, 150);
    assert_eq!(tokens.output_tokens, 15);
}
