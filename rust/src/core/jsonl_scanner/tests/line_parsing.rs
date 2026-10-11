//! The fast Codex line parser, committed prefixes and line-length limits.

use super::*;

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
    let (record, _) = &parser.records[0];
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
            .map(|(record, _)| (record.input, record.cached, record.output))
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
    let (record, _) = &parser.records[0];
    assert_eq!(record.model, "gpt-5");
    assert_eq!((record.input, record.cached, record.output), (20, 5, 3));
}

#[test]
fn codex_fast_parser_accepts_compact_and_spaced_event_records_equally() {
    let range = CostUsageDayRange::new(
        NaiveDate::from_ymd_opt(2026, 5, 31).unwrap(),
        NaiveDate::from_ymd_opt(2026, 5, 31).unwrap(),
    );
    let parsed = |line: &str| {
        let mut parser = CodexParserState::new(Some("gpt-5".to_string()), None);
        parser.process_line(line, &range);
        (
            parser.current_model,
            parser
                .records
                .into_iter()
                .map(|(record, _)| {
                    (
                        record.day_key,
                        record.model,
                        record.input,
                        record.cached,
                        record.output,
                        record.reasoning,
                    )
                })
                .collect::<Vec<_>>(),
        )
    };

    let compact_event = r#"{"timestamp":"2026-05-31T10:00:01Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":20,"cached_input_tokens":5,"output_tokens":3}}}}"#;
    let spaced_event = r#"{ "timestamp": "2026-05-31T10:00:01Z", "type": "event_msg", "payload": { "type": "token_count", "info": { "last_token_usage": { "input_tokens": 20, "cached_input_tokens": 5, "output_tokens": 3 } } } }"#;
    assert!(is_candidate_codex_line(spaced_event));
    assert_eq!(parsed(compact_event), parsed(spaced_event));

    let compact_context = r#"{"timestamp":"2026-05-31T10:00:00Z","type":"turn_context","payload":{"model":"gpt-5.5"}}"#;
    let spaced_context = "{\t\"timestamp\": \"2026-05-31T10:00:00Z\",\t\"type\":\t\"turn_context\",\t\"payload\": {\t\"model\": \"gpt-5.5\"\t}\t}";
    assert!(is_candidate_codex_line(spaced_context));
    assert_eq!(parsed(compact_context), parsed(spaced_context));

    let unrelated = r#"{ "timestamp": "2026-05-31T10:00:00Z", "type": "response", "payload": { "model": "gpt-5.5" } }"#;
    assert!(!is_candidate_codex_line(unrelated));
    assert_eq!(parsed(unrelated), (Some("gpt-5".to_string()), Vec::new()));
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
    let (record, _) = &parsed.records[0];
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
    let prefix_input: i64 = prefix.records.iter().map(|(record, _)| record.input).sum();

    writeln!(
        file,
        r#"{{"timestamp":"2026-05-31T10:00:03.000Z","type":"event_msg","payload":{{"type":"token_count","info":{{"model":"gpt-5.5","total_token_usage":{{"input_tokens":30,"cached_input_tokens":0,"output_tokens":3}}}}}}}}"#
    )
    .unwrap();

    let appended = JsonlScanner::parse_codex(
        file.path(),
        &range,
        CodexParseMode::Standard {
            start_offset: prefix.parsed_bytes,
            initial_model: prefix.last_model.clone(),
            initial_totals: prefix.last_totals.clone(),
            previous_token_timestamp: prefix.last_token_timestamp.clone(),
            token_timestamps_monotonic: prefix.token_timestamps_monotonic,
        },
        None,
        None,
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
    let full_input: i64 = full.records.iter().map(|(record, _)| record.input).sum();
    let appended_input: i64 = appended
        .records
        .iter()
        .map(|(record, _)| record.input)
        .sum();
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
    assert_eq!(partial.records[0].0.input, 10);
    assert_eq!(partial.parsed_bytes, committed_bytes);
    assert_eq!(partial.scan_target_size, committed_bytes);
    assert!(partial.is_complete, "the logical prefix is complete");

    writeln!(file, "{}", &complete_tail[split..]).unwrap();
    let resumed = JsonlScanner::parse_codex(
        file.path(),
        &range,
        CodexParseMode::Standard {
            start_offset: partial.parsed_bytes,
            initial_model: partial.last_model,
            initial_totals: partial.last_totals,
            previous_token_timestamp: partial.last_token_timestamp,
            token_timestamps_monotonic: partial.token_timestamps_monotonic,
        },
        None,
        None,
        None,
    )
    .expect("resume completed tail");
    assert_eq!(resumed.records.len(), 1);
    assert_eq!(resumed.records[0].0.input, 10);
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
        parsed.records[0].0.model,
        CostUsagePricing::CODEX_UNATTRIBUTED_MODEL
    );
    assert_eq!(
        (
            parsed.records[0].0.input,
            parsed.records[0].0.cached,
            parsed.records[0].0.output
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
            parsed.records[0].0.input,
            parsed.records[0].0.cached,
            parsed.records[0].0.output
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
    assert_eq!(parsed.records[0].0.input, 9);
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
    assert_eq!(parsed.records[0].0.input, 9);
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
