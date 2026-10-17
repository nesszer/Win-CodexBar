use std::fs;

use chrono::DateTime;
use serde_json::{Value, json};

use super::{CodexTurnPerformanceSummary, TurnSample, TurnScan, scan_session_turns};
use crate::core::{CostUsageDayRange, CostUsagePricing};

const SESSION: &str = "sess-1";
const T0: &str = "2026-06-01T10:00:00.000Z";
const T_FIRST_RECORD: &str = "2026-06-01T10:00:05.000Z";
const T_SECOND_RECORD: &str = "2026-06-01T10:00:20.000Z";
const T_COMPLETE: &str = "2026-06-01T10:00:21.000Z";

fn wide_range() -> CostUsageDayRange {
    CostUsageDayRange {
        since_key: "2000-01-01".to_string(),
        until_key: "2100-12-31".to_string(),
        scan_since_key: "1999-12-31".to_string(),
        scan_until_key: "2101-01-01".to_string(),
    }
}

fn range_from(since: &str) -> CostUsageDayRange {
    CostUsageDayRange {
        since_key: since.to_string(),
        until_key: "2100-12-31".to_string(),
        scan_since_key: since.to_string(),
        scan_until_key: "2101-01-01".to_string(),
    }
}

fn epoch_seconds(timestamp: &str) -> i64 {
    DateTime::parse_from_rfc3339(timestamp)
        .expect("fixture timestamp")
        .timestamp()
}

fn scan_lines(lines: &[Value], range: &CostUsageDayRange) -> Vec<TurnSample> {
    let mut scan = TurnScan::new(SESSION);
    for line in lines {
        scan.observe_line(line.to_string().as_bytes());
    }
    scan.finish(range)
}

fn turn_context(turn: &str, model: Option<&str>, effort: Option<&str>) -> Value {
    let mut payload = json!({ "turn_id": turn });
    if let Some(model) = model {
        payload["model"] = json!(model);
    }
    if let Some(effort) = effort {
        payload["effort"] = json!(effort);
    }
    json!({ "type": "turn_context", "timestamp": T0, "payload": payload })
}

fn task_started(turn: &str) -> Value {
    json!({
        "type": "event_msg",
        "timestamp": T0,
        "payload": { "type": "task_started", "turn_id": turn }
    })
}

fn task_complete(
    turn: &str,
    duration_ms: u64,
    first_token_ms: Option<u64>,
    started_at: &str,
) -> Value {
    json!({
        "type": "event_msg",
        "timestamp": T_COMPLETE,
        "payload": {
            "type": "task_complete",
            "turn_id": turn,
            "duration_ms": duration_ms,
            "time_to_first_token_ms": first_token_ms,
            "started_at": epoch_seconds(started_at),
            "error": null
        }
    })
}

/// One `token_usage_record`. `turn_output` is the turn's reported running total.
fn usage_record(
    response: &str,
    turn: &str,
    at: &str,
    model: &str,
    usage: (u64, u64, u64),
    turn_output: u64,
) -> Value {
    let (input, cached, output) = usage;
    json!({
        "type": "token_usage_record",
        "timestamp": at,
        "payload": {
            "thread_id": SESSION,
            "response_id": response,
            "turn_id": turn,
            "model": model,
            "usage": {
                "input_tokens": input,
                "cached_input_tokens": cached,
                "output_tokens": output,
                "reasoning_output_tokens": 0
            },
            "thread_token_usage": {
                "input_tokens": 1_000_000,
                "cached_input_tokens": 1_000_000,
                "output_tokens": 1_000_000,
                "reasoning_output_tokens": 0
            },
            "turn_token_usage": {
                "input_tokens": input,
                "cached_input_tokens": cached,
                "output_tokens": turn_output,
                "reasoning_output_tokens": 0
            }
        }
    })
}

/// A turn with two requests (40 + 60 output) whose reported total is 100.
fn two_request_turn(turn: &str) -> Vec<Value> {
    vec![
        turn_context(turn, Some("gpt-5.5"), Some("high")),
        task_started(turn),
        usage_record(
            &format!("{turn}-a"),
            turn,
            T_FIRST_RECORD,
            "gpt-5.5",
            (1000, 250, 40),
            40,
        ),
        usage_record(
            &format!("{turn}-b"),
            turn,
            T_SECOND_RECORD,
            "gpt-5.5",
            (1000, 250, 60),
            100,
        ),
        task_complete(turn, 20_000, Some(1500), T0),
    ]
}

fn sample(turn: &str, output: u64, duration_ms: u64, first_token_ms: Option<u64>) -> TurnSample {
    TurnSample {
        turn_id: turn.to_string(),
        output_tokens: output,
        duration_ms,
        first_token_ms,
        model: None,
        reasoning_effort: None,
        input_tokens: None,
        cached_input_tokens: None,
    }
}

#[test]
fn timed_turn_with_agreeing_requests_becomes_a_sample() {
    let samples = scan_lines(&two_request_turn("t1"), &wide_range());

    assert_eq!(samples.len(), 1);
    let turn = &samples[0];
    assert_eq!(turn.turn_id, "t1");
    assert_eq!(turn.output_tokens, 100);
    assert_eq!(turn.duration_ms, 20_000);
    assert_eq!(turn.first_token_ms, Some(1500));
    assert_eq!(
        turn.model.as_deref(),
        Some(CostUsagePricing::normalize_codex_model("gpt-5.5").as_str())
    );
    assert_eq!(turn.reasoning_effort.as_deref(), Some("high"));
    assert_eq!(turn.input_tokens, Some(2000));
    assert_eq!(turn.cached_input_tokens, Some(500));
}

#[test]
fn failed_turn_is_rejected() {
    let mut lines = two_request_turn("t1");
    let last = lines.len() - 1;
    lines[last]["payload"]["error"] = json!({ "message": "boom" });

    assert!(scan_lines(&lines, &wide_range()).is_empty());
}

#[test]
fn reported_output_that_disagrees_with_requests_is_rejected() {
    let lines = vec![
        turn_context("t1", Some("gpt-5.5"), Some("high")),
        task_started("t1"),
        usage_record("t1-a", "t1", T_FIRST_RECORD, "gpt-5.5", (1000, 250, 40), 40),
        usage_record(
            "t1-b",
            "t1",
            T_SECOND_RECORD,
            "gpt-5.5",
            (1000, 250, 60),
            90,
        ),
        task_complete("t1", 20_000, Some(1500), T0),
    ];

    assert!(scan_lines(&lines, &wide_range()).is_empty());
}

#[test]
fn replayed_identical_response_is_counted_once() {
    let lines = vec![
        turn_context("t1", Some("gpt-5.5"), Some("high")),
        task_started("t1"),
        usage_record("t1-a", "t1", T_FIRST_RECORD, "gpt-5.5", (1000, 250, 40), 40),
        usage_record("t1-a", "t1", T_FIRST_RECORD, "gpt-5.5", (1000, 250, 40), 40),
        task_complete("t1", 20_000, Some(1500), T0),
    ];

    let samples = scan_lines(&lines, &wide_range());

    assert_eq!(samples.len(), 1);
    assert_eq!(samples[0].output_tokens, 40);
}

#[test]
fn contradictory_copies_of_one_response_reject_both_turns() {
    let lines = vec![
        turn_context("t1", Some("gpt-5.5"), Some("high")),
        task_started("t1"),
        usage_record(
            "shared",
            "t1",
            T_FIRST_RECORD,
            "gpt-5.5",
            (1000, 250, 40),
            40,
        ),
        task_complete("t1", 20_000, Some(1500), T0),
        turn_context("t2", Some("gpt-5.5"), Some("high")),
        task_started("t2"),
        usage_record(
            "shared",
            "t2",
            T_FIRST_RECORD,
            "gpt-5.5",
            (1000, 250, 41),
            41,
        ),
        task_complete("t2", 20_000, Some(1500), T0),
    ];

    assert!(scan_lines(&lines, &wide_range()).is_empty());
}

#[test]
fn first_token_longer_than_duration_is_dropped_but_turn_is_kept() {
    let mut lines = two_request_turn("t1");
    let last = lines.len() - 1;
    lines[last] = task_complete("t1", 20_000, Some(25_000), T0);

    let samples = scan_lines(&lines, &wide_range());

    assert_eq!(samples.len(), 1);
    assert_eq!(samples[0].first_token_ms, None);
    assert_eq!(samples[0].duration_ms, 20_000);
}

#[test]
fn turn_with_requests_from_two_models_has_no_single_model() {
    let lines = vec![
        turn_context("t1", None, Some("high")),
        task_started("t1"),
        usage_record("t1-a", "t1", T_FIRST_RECORD, "gpt-5.5", (1000, 250, 40), 40),
        usage_record(
            "t1-b",
            "t1",
            T_SECOND_RECORD,
            "gpt-5.4",
            (1000, 250, 60),
            100,
        ),
        task_complete("t1", 20_000, Some(1500), T0),
    ];

    let samples = scan_lines(&lines, &wide_range());

    assert_eq!(samples.len(), 1);
    assert_eq!(samples[0].model, None);
}

#[test]
fn conflicting_effort_for_one_turn_is_dropped() {
    let mut lines = two_request_turn("t1");
    // A later context for the same turn without an effort is a conflict upstream.
    lines.insert(1, turn_context("t1", Some("gpt-5.5"), None));

    let samples = scan_lines(&lines, &wide_range());

    assert_eq!(samples.len(), 1);
    assert_eq!(samples[0].reasoning_effort, None);
}

#[test]
fn usage_for_another_thread_does_not_count_toward_the_turn() {
    let mut other = usage_record("t1-a", "t1", T_FIRST_RECORD, "gpt-5.5", (1000, 250, 40), 40);
    other["payload"]["thread_id"] = json!("sess-2");
    let lines = vec![
        turn_context("t1", Some("gpt-5.5"), Some("high")),
        task_started("t1"),
        other,
        task_complete("t1", 20_000, Some(1500), T0),
    ];

    assert!(scan_lines(&lines, &wide_range()).is_empty());
}

#[test]
fn turn_outside_the_day_range_is_excluded() {
    let lines = two_request_turn("t1");

    assert!(scan_lines(&lines, &range_from("2030-01-01")).is_empty());
    assert_eq!(scan_lines(&lines, &wide_range()).len(), 1);
}

#[test]
fn oversized_turn_relevant_line_drops_the_whole_file() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let file = dir.path().join("rollout.jsonl");
    let padding = "x".repeat(512 * 1024);
    let oversized = format!(
        "{{\"type\":\"event_msg\",\"payload\":{{\"type\":\"task_complete\",\"pad\":\"{padding}\"}}}}"
    );

    let clean = two_request_turn("t1")
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&file, format!("{clean}\n")).expect("write clean rollout");
    assert_eq!(scan_session_turns(&file, SESSION, &wide_range()).len(), 1);

    fs::write(&file, format!("{clean}\n{oversized}\n")).expect("write oversized rollout");
    assert!(scan_session_turns(&file, SESSION, &wide_range()).is_empty());
}

#[test]
fn oversized_line_that_is_not_turn_relevant_keeps_the_file() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let file = dir.path().join("rollout.jsonl");
    let padding = "x".repeat(512 * 1024);
    let unrelated = format!("{{\"type\":\"response_item\",\"pad\":\"{padding}\"}}");
    let clean = two_request_turn("t1")
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&file, format!("{clean}\n{unrelated}\n")).expect("write rollout");

    assert_eq!(scan_session_turns(&file, SESSION, &wide_range()).len(), 1);
}

#[test]
fn p95_needs_twenty_samples() {
    let nineteen = (1..=19)
        .map(|ms| sample(&format!("t{ms}"), 100, ms * 1000, None))
        .collect::<Vec<_>>();
    let twenty = (1..=20)
        .map(|ms| sample(&format!("t{ms}"), 100, ms * 1000, None))
        .collect::<Vec<_>>();

    let below = CodexTurnPerformanceSummary::from_samples(&nineteen).expect("summary");
    assert_eq!(below.details.p95_duration_milliseconds, None);

    let at = CodexTurnPerformanceSummary::from_samples(&twenty).expect("summary");
    // Nearest rank: ceil(20 * 0.95) = 19th of 20 ascending values.
    assert_eq!(at.details.p95_duration_milliseconds, Some(19_000.0));
}

#[test]
fn quartiles_need_four_samples() {
    let three = [1000, 2000, 3000]
        .iter()
        .enumerate()
        .map(|(index, duration)| sample(&format!("t{index}"), 1000, *duration, None))
        .collect::<Vec<_>>();
    let four = [1000, 2000, 3000, 4000]
        .iter()
        .enumerate()
        .map(|(index, duration)| sample(&format!("t{index}"), 1000, *duration, None))
        .collect::<Vec<_>>();

    let below = CodexTurnPerformanceSummary::from_samples(&three).expect("summary");
    assert_eq!(below.details.output_rate_lower_quartile, None);
    assert_eq!(below.details.output_rate_upper_quartile, None);

    // Rates are 1000, 500, 333.3 and 250 tok/s; nearest ranks pick 250 and 500.
    let at = CodexTurnPerformanceSummary::from_samples(&four).expect("summary");
    assert_eq!(at.details.output_rate_lower_quartile, Some(250.0));
    assert_eq!(at.details.output_rate_upper_quartile, Some(500.0));
}

#[test]
fn even_sample_count_median_averages_the_middle_pair() {
    let samples = vec![
        sample("a", 100, 1000, Some(100)),
        sample("b", 100, 3000, Some(300)),
    ];

    let summary = CodexTurnPerformanceSummary::from_samples(&samples).expect("summary");

    assert_eq!(summary.median_duration_milliseconds, 2000.0);
    assert_eq!(summary.median_first_token_milliseconds, Some(200.0));
}

#[test]
fn output_rate_is_weighted_by_elapsed_time_and_first_token_by_its_own_count() {
    let samples = vec![
        sample("a", 100, 10_000, Some(1000)),
        sample("b", 300, 10_000, None),
    ];

    let summary = CodexTurnPerformanceSummary::from_samples(&samples).expect("summary");

    // 400 output tokens over 20 seconds, not the mean of the two per-turn rates.
    assert_eq!(summary.sample_count, 2);
    assert_eq!(summary.output_tokens_per_second, 20.0);
    assert_eq!(summary.first_token_sample_count, 1);
    assert_eq!(summary.median_first_token_milliseconds, Some(1000.0));
}

#[test]
fn cached_input_fraction_uses_only_samples_with_cache_evidence() {
    let mut with_cache = sample("a", 100, 10_000, None);
    with_cache.input_tokens = Some(1000);
    with_cache.cached_input_tokens = Some(250);
    let mut more_cache = sample("b", 100, 10_000, None);
    more_cache.input_tokens = Some(1000);
    more_cache.cached_input_tokens = Some(750);
    let no_evidence = sample("c", 100, 10_000, None);

    let details = CodexTurnPerformanceSummary::from_samples(&[with_cache, more_cache, no_evidence])
        .expect("summary")
        .details;

    assert_eq!(details.cache_sample_count, 2);
    assert_eq!(details.cached_input_fraction, Some(0.5));
}

#[test]
fn groups_separate_model_and_effort_with_unknown_first() {
    let mut high = sample("a", 100, 10_000, None);
    high.model = Some("gpt-5.5".to_string());
    high.reasoning_effort = Some("high".to_string());
    let unknown = sample("b", 100, 10_000, None);

    let groups = CodexTurnPerformanceSummary::from_samples(&[high, unknown])
        .expect("summary")
        .details
        .groups;

    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].model, None);
    assert_eq!(groups[1].model.as_deref(), Some("gpt-5.5"));
    assert_eq!(groups[1].reasoning_effort.as_deref(), Some("high"));
}

#[test]
fn empty_or_untimed_samples_produce_no_summary() {
    assert_eq!(CodexTurnPerformanceSummary::from_samples(&[]), None);
    assert_eq!(
        CodexTurnPerformanceSummary::from_samples(&[sample("a", 100, 0, None)]),
        None
    );
}
