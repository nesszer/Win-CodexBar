//! Codex token deltas: reasoning, fork baselines, clamping and packed rows.

use super::*;

fn last_usage_delta(last: &Value) -> (i64, i64, i64, Option<i64>) {
    let totals = read_token_totals(last);
    (
        totals.input.max(0),
        totals.cached.max(0),
        totals.output.max(0),
        totals.reasoning,
    )
}

#[test]
fn reasoning_output_is_clamped_and_preserved_for_value_and_fast_shapes() {
    let value = serde_json::json!({
        "output_tokens": 20,
        "reasoning_output_tokens": 7,
    });
    let totals = read_token_totals(&value);
    assert_eq!(totals.output, 20);
    assert_eq!(totals.reasoning, Some(7));
    assert_eq!(last_usage_delta(&value), (0, 0, 20, Some(7)));

    let fast: CodexFastTotals = serde_json::from_value(serde_json::json!({
        "output_tokens": 20,
        "reasoning_output_tokens": 99,
    }))
    .unwrap();
    let fast_totals = codex_totals_from_fast(fast);
    assert_eq!(fast_totals.output, 20);
    assert_eq!(fast_totals.reasoning, Some(20));
}

#[test]
fn missing_reasoning_stays_unknown_for_cumulative_and_event_usage() {
    let value = serde_json::json!({ "output_tokens": 20 });
    assert_eq!(read_token_totals(&value).reasoning, None);
    assert_eq!(last_usage_delta(&value), (0, 0, 20, None));

    let mut state = CodexParserState::new(None, None);
    assert_eq!(
        state.total_usage_delta(&serde_json::json!({
            "output_tokens": 10,
        })),
        (0, 0, 10, None)
    );
}

#[test]
fn cumulative_reasoning_uses_the_comparable_previous_total() {
    let mut state = CodexParserState::new(None, None);
    assert_eq!(
        state.total_usage_delta(&serde_json::json!({
            "output_tokens": 10,
            "reasoning_output_tokens": 4,
        })),
        (0, 0, 10, Some(4))
    );
    assert_eq!(
        state.total_usage_delta(&serde_json::json!({
            "output_tokens": 20,
            "reasoning_output_tokens": 9,
        })),
        (0, 0, 10, Some(5))
    );
}

#[test]
fn fork_baseline_subtracts_known_reasoning_without_affecting_core_tokens() {
    let baseline = CodexTotals {
        input: 10,
        cached: 2,
        output: 10,
        reasoning: Some(4),
    };
    let mut state = CodexParserState::from_mode(CodexParseMode::ParentBaseline {
        baseline,
        paginated_continuation: false,
        remaining_inherited_totals: None,
    });
    assert_eq!(
        state.apply_totals_delta(CodexTotals {
            input: 20,
            cached: 5,
            output: 20,
            reasoning: Some(9),
        }),
        (10, 3, 10, Some(5))
    );

    let baseline_without_reasoning = CodexTotals {
        input: 10,
        cached: 2,
        output: 10,
        reasoning: None,
    };
    let mut state = CodexParserState::from_mode(CodexParseMode::ParentBaseline {
        baseline: baseline_without_reasoning,
        paginated_continuation: false,
        remaining_inherited_totals: None,
    });
    assert_eq!(
        state.apply_totals_delta(CodexTotals {
            input: 20,
            cached: 5,
            output: 20,
            reasoning: Some(9),
        }),
        (10, 3, 10, None)
    );
    assert!(!state.fork_baseline_ambiguous);
}

#[test]
fn inferred_fork_waits_for_present_explicit_start_ordinal() {
    let range = CostUsageDayRange::new(
        NaiveDate::from_ymd_opt(2026, 9, 22).unwrap(),
        NaiveDate::from_ymd_opt(2026, 9, 22).unwrap(),
    );
    let mut state = CodexParserState::from_mode(CodexParseMode::InferSubagent {
        start_ordinal: Some(10),
    });

    state.process_line(
        r#"{"timestamp":"2026-09-22T10:00:00Z","type":"event_msg","payload":{"type":"token_count","info":{"model":"gpt-5.6-sol","total_token_usage":{"input_tokens":100,"cached_input_tokens":20,"output_tokens":10},"last_token_usage":{"input_tokens":0,"cached_input_tokens":0,"output_tokens":0}}}}"#,
        &range,
    );

    assert!(state.records.is_empty());
    assert!(state.fork_baseline.is_none());
    assert!(!state.fork_baseline_locally_resolved());

    state.process_line(
        r#"{"ordinal":10,"timestamp":"2026-09-22T10:00:01Z","type":"event_msg","payload":{"type":"token_count","info":{"model":"gpt-5.6-sol","total_token_usage":{"input_tokens":110,"cached_input_tokens":22,"output_tokens":11},"last_token_usage":{"input_tokens":10,"cached_input_tokens":2,"output_tokens":1}}}}"#,
        &range,
    );

    assert_eq!(state.records.len(), 1);
    assert_eq!(state.records[0].0.input, 10);
    assert_eq!(state.records[0].0.cached, 2);
    assert_eq!(state.records[0].0.output, 1);
    assert!(!state.fork_baseline_locally_resolved());
}

#[test]
fn inferred_fork_keeps_missing_ordinal_unresolved_after_boundary_opens() {
    let range = CostUsageDayRange::new(
        NaiveDate::from_ymd_opt(2026, 9, 22).unwrap(),
        NaiveDate::from_ymd_opt(2026, 9, 22).unwrap(),
    );
    let mut state = CodexParserState::from_mode(CodexParseMode::InferSubagent {
        start_ordinal: Some(10),
    });
    let token_line = |ordinal: Option<i64>, total: i64, last: i64| {
        let mut value = serde_json::json!({
            "timestamp": "2026-09-22T10:00:00Z",
            "type": "event_msg",
            "payload": {"type": "token_count", "info": {
                "model": "gpt-5.6-sol",
                "total_token_usage": {"input_tokens": total, "cached_input_tokens": 0, "output_tokens": 0},
                "last_token_usage": {"input_tokens": last, "cached_input_tokens": 0, "output_tokens": 0}
            }}
        });
        if let Some(ordinal) = ordinal {
            value["ordinal"] = serde_json::json!(ordinal);
        }
        value.to_string()
    };

    state.process_line(&token_line(Some(9), 100, 0), &range);
    state.process_line(&token_line(Some(10), 100, 0), &range);
    state.process_line(&token_line(Some(11), 110, 110), &range);
    assert!(state.fork_baseline_locally_resolved());
    state.process_line(&token_line(None, 120, 10), &range);
    assert!(!state.fork_baseline_locally_resolved());
    state.process_line(&token_line(Some(12), 130, 10), &range);

    assert_eq!(state.records.len(), 1);
    assert_eq!(state.records[0].0.input, 10);
    assert!(!state.fork_baseline_locally_resolved());
}

#[test]
fn inferred_fork_keeps_missing_ordinal_unresolved_after_local_resolution() {
    let range = CostUsageDayRange::new(
        NaiveDate::from_ymd_opt(2026, 9, 22).unwrap(),
        NaiveDate::from_ymd_opt(2026, 9, 22).unwrap(),
    );
    let mut state = CodexParserState::from_mode(CodexParseMode::InferSubagent {
        start_ordinal: Some(10),
    });
    let token_line = |ordinal: Option<i64>, total: i64, last: i64| {
        let mut value = serde_json::json!({
            "timestamp": "2026-09-22T10:00:00Z",
            "type": "event_msg",
            "payload": {"type": "token_count", "info": {
                "model": "gpt-5.6-sol",
                "total_token_usage": {"input_tokens": total, "cached_input_tokens": 0, "output_tokens": 0},
                "last_token_usage": {"input_tokens": last, "cached_input_tokens": 0, "output_tokens": 0}
            }}
        });
        if let Some(ordinal) = ordinal {
            value["ordinal"] = serde_json::json!(ordinal);
        }
        value.to_string()
    };

    state.process_line(&token_line(Some(9), 100, 0), &range);
    state.process_line(&token_line(Some(10), 100, 0), &range);
    state.process_line(&token_line(Some(11), 110, 10), &range);
    assert!(state.fork_baseline_locally_resolved());
    state.process_line(&token_line(None, 120, 10), &range);

    assert!(!state.fork_baseline_locally_resolved());
}

#[test]
fn codex_token_pipeline_preserves_counts_above_i32_max() {
    let parsed = read_token_totals(&serde_json::json!({
        "input_tokens": 3_000_000_000_i64,
        "cached_input_tokens": 2_800_000_000_i64,
        "output_tokens": 200,
    }));
    assert_eq!(parsed.input, 3_000_000_000);
    assert_eq!(parsed.cached, 2_800_000_000);
    assert_eq!(parsed.output, 200);

    let mut packed = Vec::new();
    for _ in 0..2 {
        JsonlScanner::merge_codex_record_into_packed(
            &mut packed,
            &CodexUsageRecord {
                day_key: "2026-09-09".to_string(),
                timestamp: None,
                model: "gpt-5.6-luna".to_string(),
                input: 1_500_000_000,
                cached: 1_400_000_000,
                output: 100,
                reasoning: None,
                turn_id: None,
            },
        );
    }
    assert_eq!(packed, vec![3_000_000_000, 2_800_000_000, 200]);

    let mut cache = CostUsageCache::default();
    cache.days.insert(
        "2026-09-09".to_string(),
        HashMap::from([("gpt-5.6-luna".to_string(), packed)]),
    );
    let report = JsonlScanner::cached_cost_report_from_days(&cache);
    assert_eq!(report.input_tokens, 3_000_000_000);
    assert_eq!(report.cached_tokens, 2_800_000_000);
    assert_eq!(report.output_tokens, 200);
}

#[test]
fn negative_cumulative_components_are_clamped_at_the_source() {
    let value = serde_json::json!({
        "input_tokens": -5,
        "cached_input_tokens": -9,
        "cache_read_input_tokens": -3,
        "output_tokens": -2,
        "reasoning_output_tokens": -1,
    });
    let totals = read_token_totals(&value);
    assert_eq!(totals.input, 0);
    assert_eq!(totals.cached, 0);
    assert_eq!(totals.output, 0);
    assert_eq!(totals.reasoning, Some(0));

    let fast: CodexFastTotals = serde_json::from_value(value.clone()).unwrap();
    let fast_totals = codex_totals_from_fast(fast);
    assert_eq!(fast_totals.input, 0);
    assert_eq!(fast_totals.cached, 0);
    assert_eq!(fast_totals.output, 0);
    assert_eq!(fast_totals.reasoning, Some(0));

    // The payload borrows `&str` fields, so deserialize from a str rather than
    // an owned `Value`.
    let payload_json = value.to_string();
    let payload: CodexFastPayload<'_> = serde_json::from_str(&payload_json).unwrap();
    let payload_totals = fast_totals_from_payload(&payload);
    assert_eq!(payload_totals.input, 0);
    assert_eq!(payload_totals.cached, 0);
    assert_eq!(payload_totals.output, 0);
    assert_eq!(payload_totals.reasoning, Some(0));
}

#[test]
fn negative_cumulative_totals_do_not_inflate_later_deltas() {
    let mut state = CodexParserState::new(None, None);
    // A malformed cumulative record with negative counts must be clamped so it
    // cannot lower the high watermark below zero.
    assert_eq!(
        state.total_usage_delta(&serde_json::json!({
            "input_tokens": -5,
            "output_tokens": -2,
        })),
        (0, 0, 0, None)
    );
    // A later normal climb only counts its true growth above the clamped zero.
    assert_eq!(
        state.total_usage_delta(&serde_json::json!({
            "input_tokens": 3,
            "output_tokens": 1,
        })),
        (3, 0, 1, None)
    );
}

#[test]
fn legacy_packed_rows_remain_three_slots_and_report_reasoning_is_unknown() {
    let record = CodexUsageRecord {
        day_key: "2026-05-31".to_string(),
        timestamp: None,
        model: "gpt-5.6-sol".to_string(),
        input: 5,
        cached: 1,
        output: 3,
        reasoning: Some(2),
        turn_id: None,
    };
    let mut packed = vec![10, 2, 4];
    JsonlScanner::merge_codex_record_into_packed(&mut packed, &record);
    assert_eq!(packed, vec![15, 3, 7]);

    let mut cache = CostUsageCache::default();
    cache.days.insert(
        "2026-05-31".to_string(),
        HashMap::from([("gpt-5.6-sol".to_string(), packed)]),
    );
    let report = JsonlScanner::cached_cost_report_from_days(&cache);
    assert_eq!(report.reasoning_tokens, None);
}

#[test]
fn known_packed_rows_report_reasoning_only_when_all_token_rows_are_known() {
    let mut cache = CostUsageCache::default();
    cache.days.insert(
        "2026-05-31".to_string(),
        HashMap::from([("gpt-5.6-sol".to_string(), vec![10, 2, 4, 3])]),
    );
    assert_eq!(
        JsonlScanner::cached_cost_report_from_days(&cache).reasoning_tokens,
        Some(3)
    );

    cache
        .days
        .get_mut("2026-05-31")
        .unwrap()
        .insert("gpt-5.6-fast".to_string(), vec![1, 0, 1]);
    assert_eq!(
        JsonlScanner::cached_cost_report_from_days(&cache).reasoning_tokens,
        None
    );
}
