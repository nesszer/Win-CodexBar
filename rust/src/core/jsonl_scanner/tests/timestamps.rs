//! Day ranges, day keys and Codex timestamp parsing and ordering.

use super::*;

fn codex_timestamp_day_key(timestamp: &str) -> Option<String> {
    parse_codex_timestamp(timestamp).map(|parsed| parsed.day_key())
}

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
