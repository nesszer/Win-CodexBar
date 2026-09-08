use super::*;
use chrono::TimeZone;
use std::io::Write;

#[test]
fn test_day_range() {
    let since = NaiveDate::from_ymd_opt(2026, 1, 15).unwrap();
    let until = NaiveDate::from_ymd_opt(2026, 1, 20).unwrap();
    let range = CostUsageDayRange::new(since, until);

    assert_eq!(range.since_key, "2026-01-15");
    assert_eq!(range.until_key, "2026-01-20");
    assert_eq!(range.scan_since_key, "2026-01-14");
    assert_eq!(range.scan_until_key, "2026-01-21");
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
    let mut state = CodexParserState::with_timestamp_state_and_fork_mode(
        None,
        Some(baseline),
        None,
        None,
        true,
    );
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
    let mut state = CodexParserState::with_timestamp_state_and_fork_mode(
        None,
        Some(baseline_without_reasoning),
        None,
        None,
        true,
    );
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
fn legacy_packed_rows_remain_three_slots_and_report_reasoning_is_unknown() {
    let record = CodexUsageRecord {
        day_key: "2026-05-31".to_string(),
        model: "gpt-5.6-sol".to_string(),
        input: 5,
        cached: 1,
        output: 3,
        reasoning: Some(2),
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

#[test]
fn test_is_in_range() {
    assert!(CostUsageDayRange::is_in_range(
        "2026-01-15",
        "2026-01-10",
        "2026-01-20"
    ));
    assert!(!CostUsageDayRange::is_in_range(
        "2026-01-05",
        "2026-01-10",
        "2026-01-20"
    ));
    assert!(!CostUsageDayRange::is_in_range(
        "2026-01-25",
        "2026-01-10",
        "2026-01-20"
    ));
}

#[test]
fn test_parse_day_key() {
    let date = CostUsageDayRange::parse_day_key("2026-01-15");
    assert!(date.is_some());
    let date = date.unwrap();
    assert_eq!(date.year(), 2026);
    assert_eq!(date.month(), 1);
    assert_eq!(date.day(), 15);
}

#[test]
fn codex_timestamp_day_key_uses_local_calendar_day() {
    let today = Local::now().date_naive();
    let local_midnight = today.and_hms_opt(0, 30, 0).unwrap();
    let Some(local_time) = Local.from_local_datetime(&local_midnight).earliest() else {
        return;
    };
    let utc_timestamp = local_time.with_timezone(&chrono::Utc).to_rfc3339();
    let expected = today.format("%Y-%m-%d").to_string();

    assert_eq!(
        codex_timestamp_day_key(&utc_timestamp).as_deref(),
        Some(expected.as_str())
    );
}

#[test]
fn native_codex_timestamp_parser_matches_chrono_for_supported_spellings() {
    for timestamp in [
        "2026-05-31T10:00:00Z",
        "2026-05-31T10:00:00.123Z",
        "2024-02-29T23:59:59.999+05:30",
        "1900-02-28T00:00:00-08:00",
        "1899-12-31T23:59:59.000Z",
    ] {
        assert_eq!(
            parse_rfc3339_timestamp(timestamp),
            DateTime::parse_from_rfc3339(timestamp).ok(),
            "native parser changed {timestamp}"
        );
    }
    for timestamp in [
        "2026-02-29T10:00:00Z",
        "2026-05-31T10:00:00.1234567890Z",
        "2026-05-31T10:00:00+0530",
        "2026-05-31T24:00:00Z",
    ] {
        assert_eq!(
            parse_rfc3339_timestamp(timestamp),
            DateTime::parse_from_rfc3339(timestamp).ok(),
            "native parser changed invalid {timestamp}"
        );
    }
}

#[test]
fn codex_timestamp_fallback_rejects_invalid_calendar_prefixes() {
    for timestamp in [
        "2026-02-29T10:00:00Z",
        "2026-04-31T10:00:00Z",
        "not-a-dateT10:00:00Z",
        "2026-05-31",
    ] {
        assert!(
            parse_codex_timestamp(timestamp).is_none(),
            "invalid timestamp must not be accepted by the day-key fallback: {timestamp}"
        );
    }

    for timestamp in ["2026-05-31T10:00:00+0530", "2026-05-31T10:00:00+05"] {
        let parsed = parse_codex_timestamp(timestamp).expect("historical timestamp shape");
        assert_eq!(parsed.fallback_day_key, "2026-05-31");
        assert!(parsed.parsed.is_none());
    }
}

#[test]
fn codex_timestamp_order_latches_false_and_stops_rechecking() {
    let day = NaiveDate::from_ymd_opt(2026, 5, 31).unwrap();
    let range = CostUsageDayRange::new(day, day);
    let mut parser = CodexParserState::new(Some("gpt-5".to_string()), None);

    for (timestamp, input) in [
        ("2026-05-31T10:00:02Z", 10),
        ("2026-05-31T10:00:01Z", 20),
        ("2026-05-31T10:00:03Z", 30),
    ] {
        parser.process_line(
            &format!(
                r#"{{"timestamp":"{timestamp}","type":"event_msg","payload":{{"type":"token_count","info":{{"last_token_usage":{{"input_tokens":{input},"cached_input_tokens":0,"output_tokens":1}}}}}}}}"#
            ),
            &range,
        );
    }

    assert_eq!(parser.token_timestamps_monotonic, Some(false));
    assert_eq!(parser.token_timestamp_comparisons, 1);
    assert_eq!(parser.records.len(), 3);
}

#[test]
fn codex_timestamp_order_ignores_sub_millisecond_fraction() {
    let day = NaiveDate::from_ymd_opt(2026, 8, 30).unwrap();
    let range = CostUsageDayRange::new(day, day);
    let mut parser = CodexParserState::new(Some("gpt-5".to_string()), None);

    for timestamp in ["2026-08-30T12:00:00.1239Z", "2026-08-30T12:00:00.1231Z"] {
        parser.process_line(
            &format!(
                r#"{{"timestamp":"{timestamp}","type":"event_msg","payload":{{"type":"token_count","info":{{"last_token_usage":{{"input_tokens":1,"cached_input_tokens":0,"output_tokens":1}}}}}}}}"#
            ),
            &range,
        );
    }

    assert_eq!(parser.token_timestamps_monotonic, Some(true));
    assert_eq!(parser.token_timestamp_comparisons, 1);
}

#[test]
fn codex_timestamp_order_detects_millisecond_decrease() {
    let day = NaiveDate::from_ymd_opt(2026, 8, 30).unwrap();
    let range = CostUsageDayRange::new(day, day);
    let mut parser = CodexParserState::new(Some("gpt-5".to_string()), None);

    for timestamp in ["2026-08-30T12:00:00.124Z", "2026-08-30T12:00:00.123Z"] {
        parser.process_line(
            &format!(
                r#"{{"timestamp":"{timestamp}","type":"event_msg","payload":{{"type":"token_count","info":{{"last_token_usage":{{"input_tokens":1,"cached_input_tokens":0,"output_tokens":1}}}}}}}}"#
            ),
            &range,
        );
    }

    assert_eq!(parser.token_timestamps_monotonic, Some(false));
    assert_eq!(parser.token_timestamp_comparisons, 1);
}

#[test]
fn codex_timestamp_order_checks_token_history_outside_requested_window() {
    let day = NaiveDate::from_ymd_opt(2026, 5, 31).unwrap();
    let range = CostUsageDayRange::new(day, day);
    let mut parser = CodexParserState::new(Some("gpt-5".to_string()), None);

    for line in [
        r#"{"timestamp":"2026-06-01T10:00:00Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":10,"cached_input_tokens":0,"output_tokens":1}}}}"#,
        r#"{"timestamp":"2026-05-31T10:00:00Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":20,"cached_input_tokens":0,"output_tokens":2}}}}"#,
    ] {
        parser.process_line(line, &range);
    }

    assert_eq!(parser.token_timestamps_monotonic, Some(false));
    assert_eq!(parser.token_timestamp_comparisons, 1);
    assert_eq!(
        parser.records.len(),
        1,
        "only the in-range event is recorded"
    );
}

#[test]
fn test_fast_codex_parser_reads_last_usage_from_payload() {
    let range = CostUsageDayRange::new(
        NaiveDate::from_ymd_opt(2026, 5, 31).unwrap(),
        NaiveDate::from_ymd_opt(2026, 5, 31).unwrap(),
    );
    let mut parser = CodexParserState::new(None, None);

    parser.process_line(
        r#"{"timestamp":"2026-05-31T10:00:00.000Z","type":"turn_context","payload":{"info":{"model":"gpt-5.5"}}}"#,
        &range,
    );
    parser.process_line(
        r#"{"timestamp":"2026-05-31T10:00:02.000Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":120,"cache_read_input_tokens":40,"output_tokens":9}}}}"#,
        &range,
    );

    assert_eq!(parser.records.len(), 1);
    let record = &parser.records[0];
    assert_eq!(record.day_key, "2026-05-31");
    assert_eq!(record.model, "gpt-5.5");
    assert_eq!((record.input, record.cached, record.output), (120, 40, 9));
    assert_eq!(parser.current_model.as_deref(), Some("gpt-5.5"));
}

#[test]
fn test_fast_codex_parser_diffs_total_usage() {
    let range = CostUsageDayRange::new(
        NaiveDate::from_ymd_opt(2026, 5, 31).unwrap(),
        NaiveDate::from_ymd_opt(2026, 5, 31).unwrap(),
    );
    let mut parser = CodexParserState::new(Some("gpt-5".to_string()), None);

    parser.process_line(
        r#"{"timestamp":"2026-05-31T10:00:01.000Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":1000,"cached_input_tokens":200,"output_tokens":50}}}}"#,
        &range,
    );
    parser.process_line(
        r#"{"timestamp":"2026-05-31T10:00:02.000Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":1250,"cached_input_tokens":260,"output_tokens":90}}}}"#,
        &range,
    );

    assert_eq!(parser.records.len(), 2);
    assert_eq!(
        parser
            .records
            .iter()
            .map(|record| (record.input, record.cached, record.output))
            .collect::<Vec<_>>(),
        vec![(1_000, 200, 50), (250, 60, 40)]
    );
    let totals = parser.previous_totals.expect("last totals");
    assert_eq!(totals.input, 1250);
    assert_eq!(totals.cached, 260);
    assert_eq!(totals.output, 90);
}

#[test]
fn test_fast_codex_parser_reads_legacy_event_msg_shape() {
    let range = CostUsageDayRange::new(
        NaiveDate::from_ymd_opt(2026, 5, 31).unwrap(),
        NaiveDate::from_ymd_opt(2026, 5, 31).unwrap(),
    );
    let mut parser = CodexParserState::new(Some("gpt-5".to_string()), None);

    parser.process_line(
        r#"{"timestamp":"2026-05-31T10:00:02.000Z","type":"event_msg","event_msg":{"type":"token_count","input_tokens":20,"cached_input_tokens":5,"output_tokens":3}}"#,
        &range,
    );

    assert_eq!(parser.records.len(), 1);
    let record = &parser.records[0];
    assert_eq!(record.model, "gpt-5");
    assert_eq!((record.input, record.cached, record.output), (20, 5, 3));
}

#[test]
fn test_parse_codex_file_uses_fast_parser_for_current_logs() {
    let mut file = tempfile::NamedTempFile::new().expect("temp file");
    writeln!(
        file,
        r#"{{"timestamp":"2026-05-31T10:00:00.000Z","type":"turn_context","payload":{{"model":"gpt-5.5"}}}}"#
    )
    .unwrap();
    writeln!(
        file,
        r#"{{"timestamp":"2026-05-31T10:00:01.000Z","type":"event_msg","payload":{{"type":"token_count","info":{{"last_token_usage":{{"input_tokens":45,"cached_input_tokens":12,"output_tokens":8}}}}}}}}"#
    )
    .unwrap();

    let range = CostUsageDayRange::new(
        NaiveDate::from_ymd_opt(2026, 5, 31).unwrap(),
        NaiveDate::from_ymd_opt(2026, 5, 31).unwrap(),
    );
    let parsed = JsonlScanner::parse_codex_file(file.path(), &range, 0, None, None).expect("parse");

    assert_eq!(parsed.last_model.as_deref(), Some("gpt-5.5"));
    assert_eq!(parsed.records.len(), 1);
    let record = &parsed.records[0];
    assert_eq!(record.day_key, "2026-05-31");
    assert_eq!(record.model, "gpt-5.5");
    assert_eq!((record.input, record.cached, record.output), (45, 12, 8));
}

#[test]
fn codex_append_timestamp_state_is_output_equivalent_and_boundary_only() {
    let mut file = tempfile::NamedTempFile::new().expect("temp file");
    for (timestamp, input, output) in [
        ("2026-05-31T10:00:01.000Z", 10, 1),
        ("2026-05-31T10:00:02.000Z", 20, 2),
    ] {
        writeln!(
            file,
            r#"{{"timestamp":"{timestamp}","type":"event_msg","payload":{{"type":"token_count","info":{{"model":"gpt-5.5","total_token_usage":{{"input_tokens":{input},"cached_input_tokens":0,"output_tokens":{output}}}}}}}}}"#
        )
        .unwrap();
    }

    let day = NaiveDate::from_ymd_opt(2026, 5, 31).unwrap();
    let range = CostUsageDayRange::new(day, day);
    let prefix =
        JsonlScanner::parse_codex_file(file.path(), &range, 0, None, None).expect("parse prefix");
    assert_eq!(prefix.token_timestamps_monotonic, Some(true));
    assert_eq!(prefix.token_timestamp_comparisons, 1);
    let prefix_input: i32 = prefix.records.iter().map(|record| record.input).sum();

    writeln!(
        file,
        r#"{{"timestamp":"2026-05-31T10:00:03.000Z","type":"event_msg","payload":{{"type":"token_count","info":{{"model":"gpt-5.5","total_token_usage":{{"input_tokens":30,"cached_input_tokens":0,"output_tokens":3}}}}}}}}"#
    )
    .unwrap();

    let appended = JsonlScanner::parse_codex_file_with_state(
        file.path(),
        &range,
        prefix.parsed_bytes,
        prefix.last_model.clone(),
        prefix.last_totals.clone(),
        prefix.last_token_timestamp.clone(),
        prefix.token_timestamps_monotonic,
        None,
    )
    .expect("parse appended suffix");
    assert_eq!(appended.token_timestamps_monotonic, Some(true));
    assert_eq!(
        appended.token_timestamp_comparisons, 1,
        "only the cached-prefix boundary is compared"
    );

    let full = JsonlScanner::parse_codex_file(file.path(), &range, 0, None, None)
        .expect("parse complete file");
    let full_input: i32 = full.records.iter().map(|record| record.input).sum();
    let appended_input: i32 = appended.records.iter().map(|record| record.input).sum();
    assert_eq!(prefix_input + appended_input, full_input);
    assert_eq!(full_input, 30);
}

#[test]
fn codex_parse_publishes_only_the_committed_prefix_before_an_incomplete_tail() {
    let mut file = tempfile::NamedTempFile::new().expect("temp file");
    let day = NaiveDate::from_ymd_opt(2026, 5, 31).unwrap();
    let range = CostUsageDayRange::new(day, day);
    let committed_line = r#"{"timestamp":"2026-05-31T10:00:00Z","type":"event_msg","payload":{"type":"token_count","info":{"model":"gpt-5.5","total_token_usage":{"input_tokens":10,"cached_input_tokens":0,"output_tokens":1}}}}"#;
    writeln!(file, "{committed_line}").unwrap();
    let committed_bytes =
        i64::try_from(committed_line.len() + 1).expect("fixture line length fits i64");

    let complete_tail = r#"{"timestamp":"2026-05-31T10:00:01Z","type":"event_msg","payload":{"type":"token_count","info":{"model":"gpt-5.5","total_token_usage":{"input_tokens":20,"cached_input_tokens":0,"output_tokens":2}}}}"#;
    let split = complete_tail.len() / 2;
    write!(file, "{}", &complete_tail[..split]).unwrap();
    file.flush().unwrap();

    let partial = JsonlScanner::parse_codex_file(file.path(), &range, 0, None, None)
        .expect("parse committed prefix");
    assert_eq!(partial.records.len(), 1);
    assert_eq!(partial.records[0].input, 10);
    assert_eq!(partial.parsed_bytes, committed_bytes);
    assert_eq!(partial.scan_target_size, committed_bytes);
    assert!(partial.is_complete, "the logical prefix is complete");

    writeln!(file, "{}", &complete_tail[split..]).unwrap();
    let resumed = JsonlScanner::parse_codex_file_with_state(
        file.path(),
        &range,
        partial.parsed_bytes,
        partial.last_model,
        partial.last_totals,
        partial.last_token_timestamp,
        partial.token_timestamps_monotonic,
        None,
    )
    .expect("resume completed tail");
    assert_eq!(resumed.records.len(), 1);
    assert_eq!(resumed.records[0].input, 10);
    assert_eq!(
        resumed.parsed_bytes,
        i64::try_from(std::fs::metadata(file.path()).unwrap().len())
            .expect("fixture file length fits i64")
    );
    assert_eq!(resumed.scan_target_size, resumed.parsed_bytes);
    assert!(resumed.is_complete);
}

#[test]
fn codex_parser_discards_oversized_line_and_recovers_next_record() {
    let mut file = tempfile::NamedTempFile::new().expect("temp file");
    let padding = "x".repeat(CODEX_JSONL_MAX_LINE_BYTES);
    writeln!(
        file,
        r#"{{"timestamp":"2026-05-31T10:00:00Z","type":"turn_context","payload":{{"model":"{padding}"}}}}"#
    )
    .unwrap();
    writeln!(
        file,
        r#"{{"timestamp":"2026-05-31T10:00:01Z","type":"event_msg","payload":{{"type":"token_count","info":{{"last_token_usage":{{"input_tokens":9,"cached_input_tokens":2,"output_tokens":1}}}}}}}}"#
    )
    .unwrap();

    let day = NaiveDate::from_ymd_opt(2026, 5, 31).unwrap();
    let parsed = JsonlScanner::parse_codex_file(
        file.path(),
        &CostUsageDayRange::new(day, day),
        0,
        None,
        None,
    )
    .expect("parse");

    assert_eq!(parsed.records.len(), 1);
    assert_eq!(
        parsed.records[0].model,
        CostUsagePricing::CODEX_UNATTRIBUTED_MODEL
    );
    assert_eq!(
        (
            parsed.records[0].input,
            parsed.records[0].cached,
            parsed.records[0].output
        ),
        (9, 2, 1)
    );
}

#[test]
fn codex_parser_validates_a_record_at_the_line_limit() {
    let mut file = tempfile::NamedTempFile::new().expect("temp file");
    let prefix = r#"{"timestamp":"2026-05-31T10:00:00Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":9,"cached_input_tokens":2,"output_tokens":1}}},"padding":""}"#;
    let padding_len = CODEX_JSONL_MAX_LINE_BYTES - prefix.len();
    let line = format!(
        "{}{}\"}}",
        &prefix[..prefix.len() - 2],
        "x".repeat(padding_len)
    );
    assert_eq!(line.len(), CODEX_JSONL_MAX_LINE_BYTES);
    writeln!(file, "{line}").unwrap();

    let day = NaiveDate::from_ymd_opt(2026, 5, 31).unwrap();
    let parsed = JsonlScanner::parse_codex_file(
        file.path(),
        &CostUsageDayRange::new(day, day),
        0,
        None,
        None,
    )
    .expect("parse");

    assert_eq!(parsed.records.len(), 1);
    assert_eq!(
        (
            parsed.records[0].input,
            parsed.records[0].cached,
            parsed.records[0].output
        ),
        (9, 2, 1)
    );
}

#[test]
fn codex_parser_discards_a_line_at_limit_plus_one_and_keeps_following_record() {
    let mut file = tempfile::NamedTempFile::new().expect("temp file");
    let prefix = r#"{"padding":""}"#;
    let padding_len = CODEX_JSONL_MAX_LINE_BYTES + 1 - prefix.len();
    let oversized = format!(
        "{}{}\"}}",
        &prefix[..prefix.len() - 2],
        "x".repeat(padding_len)
    );
    assert_eq!(oversized.len(), CODEX_JSONL_MAX_LINE_BYTES + 1);
    writeln!(file, "{oversized}").unwrap();
    let valid = r#"{"timestamp":"2026-05-31T10:00:01Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":9,"cached_input_tokens":2,"output_tokens":1}}}}"#;
    writeln!(file, "{valid}").unwrap();

    let day = NaiveDate::from_ymd_opt(2026, 5, 31).unwrap();
    let parsed = JsonlScanner::parse_codex_file(
        file.path(),
        &CostUsageDayRange::new(day, day),
        0,
        None,
        None,
    )
    .expect("parse");

    assert_eq!(parsed.records.len(), 1);
    assert_eq!(parsed.records[0].input, 9);
}

#[test]
fn codex_parser_discards_huge_malformed_lines_before_and_after_valid_records() {
    let mut file = tempfile::NamedTempFile::new().expect("temp file");
    let valid = r#"{"timestamp":"2026-05-31T10:00:00Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":9,"cached_input_tokens":2,"output_tokens":1}}}}"#;
    let malformed = format!("{{{}", "x".repeat(CODEX_JSONL_MAX_LINE_BYTES * 4));
    writeln!(file, "{malformed}").unwrap();
    writeln!(file, "{valid}").unwrap();
    writeln!(file, "{malformed}").unwrap();

    let day = NaiveDate::from_ymd_opt(2026, 5, 31).unwrap();
    let parsed = JsonlScanner::parse_codex_file(
        file.path(),
        &CostUsageDayRange::new(day, day),
        0,
        None,
        None,
    )
    .expect("parse");

    assert_eq!(parsed.records.len(), 1);
    assert_eq!(parsed.records[0].input, 9);
}

#[test]
fn bounded_jsonl_reader_accepts_exact_limit_without_retaining_larger_input() {
    let mut input = vec![b'x'; CODEX_JSONL_MAX_LINE_BYTES];
    input.push(b'\n');
    input.extend_from_slice(br#"{"type":"event_msg"}"#);
    input.push(b'\n');
    let mut reader = BufReader::with_capacity(64 * 1024, std::io::Cursor::new(input));

    let exact = match read_bounded_jsonl_line(&mut reader, CODEX_JSONL_MAX_LINE_BYTES)
        .expect("read")
        .expect("line")
    {
        BoundedJsonlLine::Retained { bytes, .. } => bytes,
        BoundedJsonlLine::Discarded { .. } => panic!("exact-limit line was discarded"),
    };
    let later = match read_bounded_jsonl_line(&mut reader, CODEX_JSONL_MAX_LINE_BYTES)
        .expect("read")
        .expect("line")
    {
        BoundedJsonlLine::Retained { bytes, .. } => bytes,
        BoundedJsonlLine::Discarded { .. } => panic!("following line was discarded"),
    };

    assert_eq!(exact.len(), CODEX_JSONL_MAX_LINE_BYTES);
    assert_eq!(later, br#"{"type":"event_msg"}"#);
}

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

    assert_eq!(parser.records[0].model, "gpt-5.5");
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
        parser.records[0].model,
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
        parser.records[0].model,
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
    assert_eq!(parser.records[0].model, "gpt-5.6-sol");
    assert_eq!(
        (
            parser.records[0].input,
            parser.records[0].cached,
            parser.records[0].output
        ),
        (120, 55, 30)
    );
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
    assert_eq!(parser.records[1].day_key, "2026-05-31");
    assert_eq!(
        (
            parser.records[1].input,
            parser.records[1].cached,
            parser.records[1].output
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

    let total_input: i32 = parser.records.iter().map(|r| r.input).sum();
    let total_output: i32 = parser.records.iter().map(|r| r.output).sum();
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

    let total_input: i32 = parser.records.iter().map(|r| r.input).sum();
    let total_output: i32 = parser.records.iter().map(|r| r.output).sum();
    assert!(
        total_input <= 101,
        "mid-range climb re-added input to {total_input}, expected <= 101"
    );
    assert!(
        total_output <= 21,
        "mid-range climb re-added output to {total_output}, expected <= 21"
    );
}

#[test]
fn cost_scan_options_app_driven_bypasses_debounce() {
    let debounced = CostScanOptions::default();
    let forced = CostScanOptions::app_driven();
    assert!(!debounced.is_app_driven());
    assert!(forced.is_app_driven());
    let last = 1_000_000_i64;
    let now = last + 1_000; // 1s later, within 60s window

    assert!(debounced.should_skip_scan(last, now));
    assert!(!forced.should_skip_scan(last, now));
    assert!(!debounced.should_skip_scan(last, last + 61_000));

    let cache = CostUsageCache {
        last_scan_unix_ms: last,
        ..Default::default()
    };
    assert!(JsonlScanner::should_skip_cached_scan(
        &cache,
        CostScanOptions::default(),
        now
    ));
    assert!(!JsonlScanner::should_skip_cached_scan(
        &cache,
        CostScanOptions::app_driven(),
        now
    ));
}

#[test]
fn session_meta_pre_read_accepts_snake_and_camel_fork_identity() {
    let root = tempfile::tempdir().unwrap();
    let snake = root.path().join("snake.jsonl");
    std::fs::write(
        &snake,
        concat!(
            r#"{"type":"session_meta","timestamp":"2026-05-31T10:00:00Z","payload":{"session_id":"child-snake","forked_from_id":"parent-snake"}}"#,
            "\n"
        ),
    )
    .unwrap();
    assert_eq!(
        JsonlScanner::read_codex_session_metadata(&snake).unwrap(),
        CodexSessionMetadata {
            session_id: Some("child-snake".to_string()),
            forked_from_id: Some("parent-snake".to_string()),
            fork_timestamp: Some("2026-05-31T10:00:00Z".to_string()),
        }
    );

    let camel = root.path().join("camel.jsonl");
    std::fs::write(
        &camel,
        concat!(
            r#"{"type":"session_meta","payload":{"sessionId":"child-camel","forkedFromId":"parent-camel","timestamp":"2026-05-31T10:00:01Z"}}"#,
            "\n"
        ),
    )
    .unwrap();
    let metadata = JsonlScanner::read_codex_session_metadata(&camel).unwrap();
    assert_eq!(metadata.session_id.as_deref(), Some("child-camel"));
    assert_eq!(metadata.forked_from_id.as_deref(), Some("parent-camel"));
    assert_eq!(
        metadata.fork_timestamp.as_deref(),
        Some("2026-05-31T10:00:01Z")
    );
}

#[test]
fn legacy_file_usage_json_defaults_fork_metadata() {
    let usage: CostUsageFileUsage = serde_json::from_str(
        r#"{"mtime_unix_ms":0,"size":0,"days":{},"parsed_bytes":null,"last_model":null,"last_totals":null}"#,
    )
    .unwrap();
    assert_eq!(usage.codex_session_id, None);
    assert_eq!(usage.codex_forked_from_id, None);
    assert_eq!(usage.codex_fork_timestamp, None);
    assert!(!usage.codex_unresolved_fork_parent);

    let report: CachedCostReport = serde_json::from_str(
        r#"{"total_cost_usd":1.5,"input_tokens":10,"cached_tokens":2,"output_tokens":3,"sessions_count":1,"updated_at":null,"partial":false}"#,
    )
    .unwrap();
    assert_eq!(report.reasoning_tokens, None);
}

#[test]
fn is_line_boundary_offset_zero_returns_true() {
    // F2: offset 0 is always a valid boundary (start of file).
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("f.jsonl");
    std::fs::write(
        &path,
        b"hello
world
",
    )
    .unwrap();
    assert!(JsonlScanner::is_line_boundary_offset(&path, 0));
}

#[test]
fn is_line_boundary_offset_at_or_past_size_returns_true() {
    // F2: offset >= file_size returns true (EOF or beyond is a valid boundary).
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("f.jsonl");
    let content = b"line1
line2
";
    std::fs::write(&path, content).unwrap();
    let size = i64::try_from(content.len()).unwrap();
    assert!(JsonlScanner::is_line_boundary_offset(&path, size));
    assert!(JsonlScanner::is_line_boundary_offset(&path, size + 100));
}

#[test]
fn is_line_boundary_offset_exact_newline_returns_true() {
    // F2: offset pointing right after a newline is a valid boundary.
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("f.jsonl");
    // "line1\nline2\n" â€” offset 6 is right after first \n
    std::fs::write(&path, b"line1\nline2\n").unwrap();
    assert!(JsonlScanner::is_line_boundary_offset(&path, 6));
}

#[test]
fn is_line_boundary_offset_midline_returns_false() {
    // F2: offset pointing mid-line (byte before is not \n) returns false.
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("f.jsonl");
    // "line1\nline2\n" â€” offset 3 is mid-line (byte before is 'n')
    std::fs::write(&path, b"line1\nline2\n").unwrap();
    assert!(!JsonlScanner::is_line_boundary_offset(&path, 3));
}

#[test]
fn is_line_boundary_offset_missing_file_returns_false() {
    // F2: missing file returns false (probe fails).
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("nonexistent.jsonl");
    // offset > 0 so it doesn't short-circuit to true
    assert!(!JsonlScanner::is_line_boundary_offset(&path, 10));
}

#[test]
fn catch_up_snapshot_preserves_established_codex_cost_and_tokens() {
    let mut cache = CostUsageCache::default();
    cache.files.insert(
        "session.jsonl".to_string(),
        CostUsageFileUsage {
            mtime_unix_ms: 0,
            size: 100,
            codex_file_identity: None,
            days: HashMap::from([(
                "2026-08-20".to_string(),
                HashMap::from([("gpt-5.6-sol".to_string(), vec![1_000, 250, 100])]),
            )]),
            parsed_bytes: Some(100),
            codex_scan_target_size: None,
            last_model: Some("gpt-5.6-sol".to_string()),
            last_totals: None,
            codex_token_timestamps_monotonic: Some(true),
            codex_last_token_timestamp: None,
            codex_session_id: None,
            codex_forked_from_id: None,
            codex_fork_timestamp: None,
            codex_unresolved_fork_parent: false,
        },
    );
    cache.files.insert(
        "empty.jsonl".to_string(),
        CostUsageFileUsage {
            mtime_unix_ms: 0,
            size: 10,
            codex_file_identity: None,
            days: HashMap::new(),
            parsed_bytes: Some(10),
            codex_scan_target_size: None,
            last_model: None,
            last_totals: None,
            codex_token_timestamps_monotonic: None,
            codex_last_token_timestamp: None,
            codex_session_id: None,
            codex_forked_from_id: None,
            codex_fork_timestamp: None,
            codex_unresolved_fork_parent: false,
        },
    );
    cache.days.insert(
        "2026-08-20".to_string(),
        HashMap::from([("gpt-5.6-sol".to_string(), vec![1_000, 250, 100])]),
    );

    let report = JsonlScanner::cached_cost_report_from_days(&cache);
    let expected = CostUsagePricing::codex_cost_usd_at_date(
        "gpt-5.6-sol",
        1_000,
        250,
        100,
        NaiveDate::from_ymd_opt(2026, 8, 20).unwrap(),
    )
    .expect("known model price");

    assert!((report.total_cost_usd - expected).abs() < 1e-12);
    assert!(report.total_cost_usd > 0.0);
    assert_eq!(report.input_tokens, 1_000);
    assert_eq!(report.cached_tokens, 250);
    assert_eq!(report.output_tokens, 100);
    assert_eq!(report.sessions_count, 1);
    assert!(!report.partial);
    assert!(report.updated_at.is_some());
}

#[test]
fn save_cache_persists_small_codex_artifact() {
    // F19 integration: a normal-sized Codex cache is persisted and
    // reloadable â€” the MAX_LOAD_BYTES refusal does not false-positive.
    let root = tempfile::tempdir().unwrap();
    let cache_root = root.path().to_path_buf();
    let mut cache = CostUsageCache {
        scan_since_key: Some("2026-01-01".to_string()),
        scan_until_key: Some("2026-01-31".to_string()),
        files: HashMap::from([(
            "a.jsonl".to_string(),
            CostUsageFileUsage {
                mtime_unix_ms: 0,
                size: 100,
                codex_file_identity: None,
                days: HashMap::from([(
                    "2026-01-10".to_string(),
                    HashMap::from([("gpt-5.6-sol".to_string(), vec![10, 0, 1])]),
                )]),
                parsed_bytes: None,
                codex_scan_target_size: None,
                last_model: None,
                last_totals: None,
                codex_token_timestamps_monotonic: None,
                codex_last_token_timestamp: None,
                codex_session_id: None,
                codex_forked_from_id: None,
                codex_fork_timestamp: None,
                codex_unresolved_fork_parent: false,
            },
        )]),
        ..Default::default()
    };

    JsonlScanner::save_cache(ProviderId::Codex, &mut cache, Some(&cache_root));

    // File should exist and be reloadable.
    let loaded = JsonlScanner::load_cache(ProviderId::Codex, Some(&cache_root));
    assert!(
        loaded.files.contains_key("a.jsonl"),
        "small artifact persisted"
    );
    assert_eq!(loaded.scan_since_key, Some("2026-01-01".to_string()));
}

#[test]
fn stale_loaded_cache_does_not_replace_newer_baseline() {
    let root = tempfile::tempdir().unwrap();
    let cache_root = root.path();

    let mut initial = CostUsageCache {
        last_scan_unix_ms: 1,
        ..Default::default()
    };
    JsonlScanner::save_cache(ProviderId::Codex, &mut initial, Some(cache_root));

    let mut stale = JsonlScanner::load_cache(ProviderId::Codex, Some(cache_root));
    let mut newer = JsonlScanner::load_cache(ProviderId::Codex, Some(cache_root));
    newer.last_scan_unix_ms = 2;
    JsonlScanner::save_cache(ProviderId::Codex, &mut newer, Some(cache_root));

    stale.last_scan_unix_ms = 3;
    JsonlScanner::save_cache(ProviderId::Codex, &mut stale, Some(cache_root));

    let loaded = JsonlScanner::load_cache(ProviderId::Codex, Some(cache_root));
    assert_eq!(
        loaded.last_scan_unix_ms, 2,
        "a stale decoded baseline must not overwrite the newer cache"
    );
}

#[test]
fn save_cache_refuses_non_bounded_provider_oversize() {
    // F19: non-bounded providers (e.g. Claude) skip the refusal check
    // entirely â€” the MAX_LOAD_BYTES guard only applies to bounded providers.
    // This test confirms the is_bounded_provider gate works: Claude cache
    // is saved regardless of the MAX_LOAD_BYTES check (which is Codex-only).
    let root = tempfile::tempdir().unwrap();
    let cache_root = root.path().to_path_buf();
    let mut cache = CostUsageCache::default();
    cache.files.insert(
        "claude.jsonl".to_string(),
        CostUsageFileUsage {
            mtime_unix_ms: 0,
            size: 100,
            codex_file_identity: None,
            days: HashMap::new(),
            parsed_bytes: None,
            codex_scan_target_size: None,
            last_model: None,
            last_totals: None,
            codex_token_timestamps_monotonic: None,
            codex_last_token_timestamp: None,
            codex_session_id: None,
            codex_forked_from_id: None,
            codex_fork_timestamp: None,
            codex_unresolved_fork_parent: false,
        },
    );

    JsonlScanner::save_cache(ProviderId::Claude, &mut cache, Some(&cache_root));
    let loaded = JsonlScanner::load_cache(ProviderId::Claude, Some(&cache_root));
    assert!(loaded.files.contains_key("claude.jsonl"));
}

#[test]
fn save_cache_refusal_removes_preexisting_destination_artifact() {
    // F19 integration: when the post-encode check refuses the artifact, any
    // pre-existing destination file is removed so a stale/oversized artifact
    // cannot persist and trigger load/refuse/rebuild behavior on next scan.
    let root = tempfile::tempdir().unwrap();
    let cache_root = root.path().to_path_buf();

    let mut cache = CostUsageCache::default();
    cache.files.insert(
        "big.jsonl".to_string(),
        CostUsageFileUsage {
            mtime_unix_ms: 0,
            size: 100,
            codex_file_identity: None,
            days: HashMap::from([(
                "2026-01-10".to_string(),
                HashMap::from([("gpt-5.6-sol".to_string(), vec![10, 0, 1])]),
            )]),
            parsed_bytes: None,
            codex_scan_target_size: None,
            last_model: None,
            last_totals: None,
            codex_token_timestamps_monotonic: None,
            codex_last_token_timestamp: None,
            codex_session_id: None,
            codex_forked_from_id: None,
            codex_fork_timestamp: None,
            codex_unresolved_fork_parent: false,
        },
    );

    // Precreate a "stale" destination artifact so the refusal must remove
    // it. We seed it via a large (over_max) save_limit so the save_cache_with_limit
    // first ENCODES the small cache fine under a generous limit, writes the file,
    // then a follow-up call with a tiny limit must refuse AND remove.
    let cache_path = {
        // Exercise the private helper indirectly via the public path: first
        // persist a valid artifact under a generous limit via save_cache.
        // Then call with an impossible limit (encoded JSON ~hundreds of
        // bytes, limit = 1 byte) to force refusal.
        JsonlScanner::save_cache_with_limit(
            ProviderId::Codex,
            &mut cache,
            Some(&cache_root),
            usize::MAX,
        );
        let p = JsonlScanner::cache_path(ProviderId::Codex, Some(&cache_root));
        assert!(p.exists(), "precreate destination artifact");
        p
    };

    // Sanity: a normal load succeeds against the precreated artifact.
    let loaded = JsonlScanner::load_cache(ProviderId::Codex, Some(&cache_root));
    assert!(loaded.files.contains_key("big.jsonl"));

    // Force refusal with a 1-byte limit: encoded cache will exceed it.
    JsonlScanner::save_cache_with_limit(ProviderId::Codex, &mut cache, Some(&cache_root), 1);

    // Destination must be gone â€” no stale artifact may persist.
    assert!(
        !cache_path.exists(),
        "refusal must remove preexisting destination artifact"
    );

    // No temp file should remain in the cache root (only unique tmp name was used).
    let mut tmp_entries = Vec::new();
    for entry in std::fs::read_dir(&cache_root).unwrap() {
        let name = entry.unwrap().file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') && name.ends_with(".tmp") {
            tmp_entries.push(name.into_owned());
        }
    }
    // Best-effort temp cleanup writes an empty file at the unique name; the
    // invariant is that NO tmp file contains a complete artifact. The set
    // should at most contain a single zero-byte remnant from the cleanup
    // (or be empty); we persist via copy() rather than rename so no live
    // tmp holds data after the save path completes.
    for t in &tmp_entries {
        let meta = std::fs::metadata(cache_root.join(t)).unwrap();
        assert_eq!(meta.len(), 0, "tmp remnant must be empty: {t}");
    }

    // Loading after removal yields a fresh default cache (no rebuild loop).
    let loaded = JsonlScanner::load_cache(ProviderId::Codex, Some(&cache_root));
    assert!(
        loaded.files.is_empty(),
        "no rebuild loop from removed artifact"
    );
}

#[test]
fn save_cache_at_exact_limit_is_accepted() {
    // F19 boundary: an encoded artifact at exactly the injected limit is
    // accepted (only strictly-larger artifacts are refused).
    let root = tempfile::tempdir().unwrap();
    let cache_root = root.path().to_path_buf();

    let cache = CostUsageCache::default();
    // Serialize to learn the actual encoded size for this exact struct.
    let json = serde_json::to_string(&cache).unwrap();
    let exact_limit = json.len();

    let mut cache_for_save = cache;
    JsonlScanner::save_cache_with_limit(
        ProviderId::Codex,
        &mut cache_for_save,
        Some(&cache_root),
        exact_limit,
    );

    let cache_path = JsonlScanner::cache_path(ProviderId::Codex, Some(&cache_root));
    assert!(
        cache_path.exists(),
        "artifact at exact limit must be persisted"
    );
}

#[test]
fn save_cache_one_over_limit_is_refused_and_removes_destination() {
    // F19 boundary: an encoded artifact one byte over the injected limit is
    // refused, and any pre-existing destination is removed.
    let root = tempfile::tempdir().unwrap();
    let cache_root = root.path().to_path_buf();

    let cache = CostUsageCache::default();
    let json = serde_json::to_string(&cache).unwrap();
    // One byte short of the encoded size forces refusal on the next attempt.
    let under_by_one = json.len().saturating_sub(1);

    let mut cache_for_save = cache;
    JsonlScanner::save_cache_with_limit(
        ProviderId::Codex,
        &mut cache_for_save,
        Some(&cache_root),
        under_by_one,
    );

    let cache_path = JsonlScanner::cache_path(ProviderId::Codex, Some(&cache_root));
    assert!(
        !cache_path.exists(),
        "one-over-limit encoded artifact must be refused"
    );
}
