//! Codex model attribution, bare usage rows and lineage watermarks.

use super::*;

#[test]
fn codex_turn_context_wins_over_conflicting_event_model() {
    let day = NaiveDate::from_ymd_opt(2026, 5, 31).unwrap();
    let range = CostUsageDayRange::new(day, day);
    let mut parser = CodexParserState::new(None, None);
    parser.process_line(
        r#"{"timestamp":"2026-05-31T10:00:00Z","type":"turn_context","payload":{"model":"gpt-5.5"}}"#,
        &range,
    );
    parser.process_line(
        r#"{"timestamp":"2026-05-31T10:00:01Z","type":"event_msg","payload":{"type":"token_count","model":"gpt-5.6-sol","info":{"last_token_usage":{"input_tokens":5,"cached_input_tokens":1,"output_tokens":2}}}}"#,
        &range,
    );

    assert_eq!(parser.records[0].0.model, "gpt-5.5");
}

#[test]
fn codex_blank_context_clears_stale_model_and_emits_unattributed_usage() {
    let day = NaiveDate::from_ymd_opt(2026, 5, 31).unwrap();
    let range = CostUsageDayRange::new(day, day);
    let mut parser = CodexParserState::new(Some("gpt-5.5".to_string()), None);
    parser.process_line(
        r#"{"timestamp":"2026-05-31T10:00:00Z","type":"turn_context","payload":{"model":" "}}"#,
        &range,
    );
    parser.process_line(
        r#"{"timestamp":"2026-05-31T10:00:01Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":5,"cached_input_tokens":1,"output_tokens":2}}}}"#,
        &range,
    );

    assert_eq!(
        parser.records[0].0.model,
        CostUsagePricing::CODEX_UNATTRIBUTED_MODEL
    );
}

#[test]
fn codex_model_less_token_event_uses_unpriced_sentinel() {
    let day = NaiveDate::from_ymd_opt(2026, 5, 31).unwrap();
    let range = CostUsageDayRange::new(day, day);
    let mut parser = CodexParserState::new(None, None);
    parser.process_line(
        r#"{"timestamp":"2026-05-31T10:00:01Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":10,"cached_input_tokens":0,"output_tokens":2}}}}"#,
        &range,
    );

    assert_eq!(parser.records.len(), 1);
    assert_eq!(
        parser.records[0].0.model,
        CostUsagePricing::CODEX_UNATTRIBUTED_MODEL
    );
}

#[test]
fn cached_tokens_use_larger_cached_or_cache_read_field() {
    let value = serde_json::json!({
        "input_tokens": 100,
        "cached_input_tokens": 20,
        "cache_read_input_tokens": 35,
        "output_tokens": 10
    });
    let totals = read_token_totals(&value);
    assert_eq!(totals.cached, 35);
}

#[test]
fn parses_bare_usage_rows_outside_token_count_envelope() {
    let value = serde_json::json!({
        "model": "gpt-5.6-sol",
        "usage": {
            "prompt_tokens": 120,
            "completion_tokens": 30,
            "cached_input_tokens": 40,
            "cache_read_input_tokens": 55
        }
    });
    let (totals, model) = bare_usage_totals(&value).expect("bare usage");
    assert_eq!(totals.input, 120);
    assert_eq!(totals.output, 30);
    assert_eq!(totals.cached, 55);
    assert_eq!(model.as_deref(), Some("gpt-5.6-sol"));
}

#[test]
fn process_line_accepts_type_less_bare_usage_row() {
    let day = NaiveDate::from_ymd_opt(2026, 5, 31).unwrap();
    let range = CostUsageDayRange::new(day, day);
    let mut parser = CodexParserState::new(None, None);

    parser.process_line(
        r#"{"timestamp":"2026-05-31T10:00:01Z","model":"gpt-5.6-sol","usage":{"prompt_tokens":120,"completion_tokens":30,"cache_read_input_tokens":55}}"#,
        &range,
    );

    assert_eq!(parser.records.len(), 1);
    assert_eq!(parser.records[0].0.model, "gpt-5.6-sol");
    assert_eq!(
        (
            parser.records[0].0.input,
            parser.records[0].0.cached,
            parser.records[0].0.output
        ),
        (120, 55, 30)
    );
}

#[test]
fn process_line_keeps_bare_usage_when_model_contains_turn_context() {
    let day = NaiveDate::from_ymd_opt(2026, 5, 31).unwrap();
    let range = CostUsageDayRange::new(day, day);
    let mut parser = CodexParserState::new(None, None);

    parser.process_line(
        r#"{"timestamp":"2026-05-31T10:00:01Z","model":"turn_context","usage":{"prompt_tokens":120,"completion_tokens":30}}"#,
        &range,
    );

    assert_eq!(parser.records.len(), 1);
    assert_eq!(parser.records[0].0.input, 120);
    assert_eq!(parser.records[0].0.output, 30);
}

#[test]
fn timestamp_less_bare_usage_uses_last_accepted_usage_day() {
    let day = NaiveDate::from_ymd_opt(2026, 5, 31).unwrap();
    let range = CostUsageDayRange::new(day, day);
    let mut parser = CodexParserState::new(Some("gpt-5.6-sol".to_string()), None);

    parser.process_line(
        r#"{"timestamp":"2026-05-31T10:00:01Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":10,"cached_input_tokens":2,"output_tokens":1}}}}"#,
        &range,
    );
    parser.process_line(
        r#"{"usage":{"prompt_tokens":20,"completion_tokens":4,"cache_read_input_tokens":3}}"#,
        &range,
    );

    assert_eq!(parser.records.len(), 2);
    assert_eq!(parser.records[1].0.day_key, "2026-05-31");
    assert_eq!(
        (
            parser.records[1].0.input,
            parser.records[1].0.cached,
            parser.records[1].0.output
        ),
        (20, 3, 4)
    );
}

#[test]
fn interleaved_lineage_totals_never_exceed_high_watermark_growth() {
    let day = NaiveDate::from_ymd_opt(2026, 5, 31).unwrap();
    let range = CostUsageDayRange::new(day, day);
    let mut parser = CodexParserState::new(Some("gpt-5.6-sol".to_string()), None);

    parser.process_line(
        r#"{"timestamp":"2026-05-31T10:00:01Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":100,"cached_input_tokens":0,"output_tokens":20}}}}"#,
        &range,
    );
    parser.process_line(
        r#"{"timestamp":"2026-05-31T10:00:02Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":5,"cached_input_tokens":0,"output_tokens":1}}}}"#,
        &range,
    );
    parser.process_line(
        r#"{"timestamp":"2026-05-31T10:00:03Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":101,"cached_input_tokens":0,"output_tokens":21}}}}"#,
        &range,
    );

    let total_input: i64 = parser.records.iter().map(|(r, _)| r.input).sum();
    let total_output: i64 = parser.records.iter().map(|(r, _)| r.output).sum();
    assert!(
        total_input <= 101,
        "input inflated to {total_input}, expected <= 101"
    );
    assert!(
        total_output <= 21,
        "output inflated to {total_output}, expected <= 21"
    );
}

#[test]
fn interleaved_lineage_mid_range_climb_below_watermark_does_not_readd() {
    // 100 â†’ 5 (rewind) â†’ 80 (mid-range below water) â†’ 101 (above water).
    // Phase-1 containment: do not re-add the 5â†’80 climb; only growth above
    // the historical high watermark counts.
    let day = NaiveDate::from_ymd_opt(2026, 5, 31).unwrap();
    let range = CostUsageDayRange::new(day, day);
    let mut parser = CodexParserState::new(Some("gpt-5.6-sol".to_string()), None);

    for (input, output) in [(100, 20), (5, 1), (80, 10), (101, 21)] {
        parser.process_line(
            &format!(
                r#"{{"timestamp":"2026-05-31T10:00:0{input}Z","type":"event_msg","payload":{{"type":"token_count","info":{{"total_token_usage":{{"input_tokens":{input},"cached_input_tokens":0,"output_tokens":{output}}}}}}}"#
            ),
            &range,
        );
    }

    let total_input: i64 = parser.records.iter().map(|(r, _)| r.input).sum();
    let total_output: i64 = parser.records.iter().map(|(r, _)| r.output).sum();
    assert!(
        total_input <= 101,
        "mid-range climb re-added input to {total_input}, expected <= 101"
    );
    assert!(
        total_output <= 21,
        "mid-range climb re-added output to {total_output}, expected <= 21"
    );
}
