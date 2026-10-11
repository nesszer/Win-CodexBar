//! Claude transcript records: dedup, cutoffs, session ids and incomplete rows.

use super::*;

#[test]
fn derives_claude_dedup_key_from_message_and_request_ids() {
    assert_eq!(
        claude_usage_dedup_key(Some("msg_1"), Some("req_1"), None),
        Some(ClaudeUsageDedupKey::Request {
            message_id: Some("msg_1".to_string()),
            request_id: "req_1".to_string(),
        })
    );
    assert_eq!(
        claude_usage_dedup_key(None, Some(" req_1 "), None),
        Some(ClaudeUsageDedupKey::Request {
            message_id: None,
            request_id: "req_1".to_string(),
        })
    );
    assert_eq!(
        claude_usage_dedup_key(Some("msg_1"), None, Some("session_1")),
        Some(ClaudeUsageDedupKey::Session {
            session_id: "session_1".to_string(),
            message_id: "msg_1".to_string(),
        })
    );
    assert_eq!(claude_usage_dedup_key(Some("msg_1"), None, None), None);
    assert_eq!(
        claude_usage_dedup_key(None, Some("req_1"), Some("session_1")),
        Some(ClaudeUsageDedupKey::Request {
            message_id: None,
            request_id: "req_1".to_string(),
        })
    );
    assert_eq!(
        claude_usage_dedup_key(Some("msg_1"), Some(" "), Some("session_1")),
        Some(ClaudeUsageDedupKey::Session {
            session_id: "session_1".to_string(),
            message_id: "msg_1".to_string(),
        })
    );
    assert_eq!(
        claude_usage_dedup_key(Some(" "), None, Some("session_1")),
        None
    );
    assert_eq!(claude_usage_dedup_key(Some("msg_1"), None, Some(" ")), None);
}

#[test]
fn session_id_falls_back_from_blank_direct_id_to_metadata() {
    let event: ClaudeEvent = serde_json::from_str(
        r#"{"type":"assistant","sessionId":"  ","metadata":{"session_id":" metadata-session "},"message":{"id":"msg_1","model":"claude-sonnet-4-6","usage":{"input_tokens":10}}}"#,
    )
    .unwrap();

    assert_eq!(event.session_id(), Some("metadata-session"));
}

#[test]
fn session_id_falls_back_from_blank_direct_and_metadata_ids_to_nested_metadata() {
    let event: ClaudeEvent = serde_json::from_str(
        r#"{"type":"assistant","sessionId":" ","metadata":{"sessionId":"\t","metadata":{"session_id":" nested-session "}},"message":{"id":"msg_1","model":"claude-sonnet-4-6","usage":{"input_tokens":10}}}"#,
    )
    .unwrap();

    assert_eq!(event.session_id(), Some("nested-session"));
}

#[test]
fn session_aware_claude_dedup_keeps_distinct_sessions_separate() {
    let first: ClaudeEvent = serde_json::from_str(
        r#"{"type":"assistant","sessionId":"session_a","message":{"id":"msg_1","model":"claude-sonnet-4-6","usage":{"input_tokens":10}}}"#,
    )
    .unwrap();
    let second: ClaudeEvent = serde_json::from_str(
        r#"{"type":"assistant","sessionId":"session_b","message":{"id":"msg_1","model":"claude-sonnet-4-6","usage":{"input_tokens":10}}}"#,
    )
    .unwrap();
    let first_record = claude_usage_record_from_event(&first).expect("first usage record");
    let second_record = claude_usage_record_from_event(&second).expect("second usage record");
    let cutoff = DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    let mut seen = HashSet::new();

    assert!(should_count_claude_record(
        &first_record,
        &cutoff,
        &mut seen
    ));
    assert!(should_count_claude_record(
        &second_record,
        &cutoff,
        &mut seen
    ));
    assert!(!should_count_claude_record(
        &first_record,
        &cutoff,
        &mut seen
    ));
}

#[test]
fn counts_claude_usage_once_across_duplicate_records() {
    // The same API response can be replayed into several transcript files
    // (session resume, sidechains); it must only be counted once.
    let event: ClaudeEvent = serde_json::from_str(
        r#"{"type":"assistant","timestamp":"2026-01-15T10:00:00Z","requestId":"req_1","message":{"id":"msg_1","model":"claude-sonnet-4-6","usage":{"input_tokens":100,"output_tokens":50,"cache_creation_input_tokens":10,"cache_read_input_tokens":20}}}"#,
    )
    .unwrap();

    let record = claude_usage_record_from_event(&event).expect("usage record");
    assert_eq!(record.model, "claude-sonnet-4-6");
    assert_eq!(record.input, 100);
    assert_eq!(record.output, 50);
    assert_eq!(record.cache_create, 10);
    assert_eq!(record.cache_read, 20);
    assert!(record.cost.is_some_and(|cost| cost > 0.0));

    let cutoff = DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    let mut seen = HashSet::new();
    assert!(should_count_claude_record(&record, &cutoff, &mut seen));
    assert!(!should_count_claude_record(&record, &cutoff, &mut seen));
}

#[test]
fn rejects_claude_records_before_cutoff() {
    let event: ClaudeEvent = serde_json::from_str(
        r#"{"type":"assistant","timestamp":"2025-12-01T10:00:00Z","requestId":"req_old","message":{"id":"msg_old","model":"claude-sonnet-4-6","usage":{"input_tokens":1,"output_tokens":1}}}"#,
    )
    .unwrap();
    let record = claude_usage_record_from_event(&event).expect("usage record");
    let cutoff = DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    let mut seen = HashSet::new();
    assert!(!should_count_claude_record(&record, &cutoff, &mut seen));
}

#[test]
fn ignores_claude_events_without_countable_usage() {
    // Non-assistant events carry no billable usage.
    let event: ClaudeEvent =
        serde_json::from_str(r#"{"type":"user","message":{"usage":{"input_tokens":5}}}"#).unwrap();
    assert!(claude_usage_record_from_event(&event).is_none());

    // Zero-token usage blocks (e.g. synthetic messages) are not sessions.
    let event: ClaudeEvent = serde_json::from_str(
        r#"{"type":"assistant","message":{"id":"msg_zero","model":"claude-sonnet-4-6","usage":{"input_tokens":0,"output_tokens":0}}}"#,
    )
    .unwrap();
    assert!(claude_usage_record_from_event(&event).is_none());
}

#[test]
fn excludes_preliminary_proxy_estimates_but_keeps_cache_aware_rows() {
    let preliminary: ClaudeEvent = serde_json::from_str(
        r#"{"type":"assistant","message":{"id":"msg_preliminary","model":"gpt-5.6-sol","stop_reason":null,"usage":{"input_tokens":1000}}}"#,
    )
    .unwrap();
    assert!(claude_usage_record_from_event(&preliminary).is_none());

    let completed: ClaudeEvent = serde_json::from_str(
        r#"{"type":"assistant","message":{"id":"msg_completed","model":"gpt-5.6-sol","stop_reason":"end_turn","usage":{"input_tokens":1000}}}"#,
    )
    .unwrap();
    assert!(claude_usage_record_from_event(&completed).is_some());

    let cache_aware: ClaudeEvent = serde_json::from_str(
        r#"{"type":"assistant","message":{"id":"msg_cache_aware","model":"gpt-5.6-sol","stop_reason":null,"usage":{"input_tokens":1000,"cache_read_input_tokens":1}}}"#,
    )
    .unwrap();
    assert!(claude_usage_record_from_event(&cache_aware).is_some());
}

#[test]
fn claude_scan_counts_unreconciled_incomplete_requests_per_day_and_model() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("transcript.jsonl");
    let now = Utc::now() - Duration::hours(1);
    let ts = now.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
    let day = cost_bucket_zone().date(now).format("%Y-%m-%d").to_string();
    let row = |request: &str, message: &str, stop: &str, usage: &str| {
        format!(
            r#"{{"type":"assistant","timestamp":"{ts}","requestId":"{request}","message":{{"id":"{message}","model":"claude-sonnet-4-6","stop_reason":{stop},"usage":{usage}}}}}"#
        )
    };
    let body = [
        // Superseded: a completed row with the same key exists.
        row("req_done", "msg_done", "null", r#"{"input_tokens":1000}"#),
        row(
            "req_done",
            "msg_done",
            r#""end_turn""#,
            r#"{"input_tokens":1000,"output_tokens":50}"#,
        ),
        // Never completed, duplicated preliminary rows count once.
        row("req_open", "msg_open", "null", r#"{"input_tokens":500}"#),
        row("req_open", "msg_open", "null", r#"{"input_tokens":500}"#),
    ]
    .join("\n");
    std::fs::write(&path, body).unwrap();

    let cutoff = Utc::now() - Duration::days(1);
    let mut seen = HashSet::new();
    let mut pricing = ClaudeScanPricingResolver::default();
    let mut tracker = ClaudeIncompleteTracker::default();
    let mut summary = CostSummary::default();
    let result = scan_claude_file_with_pricing(
        &path,
        &cutoff,
        &mut seen,
        None,
        &mut pricing,
        &mut tracker,
        |record| assert!(add_claude_record_to_summary(&mut summary, record)),
    );
    assert_eq!(result.counted, 1);
    assert!(
        !result.is_complete(),
        "preliminary rows keep coverage unknown"
    );

    let report = tracker.resolve(&seen);
    report.apply_to(&mut summary);
    assert_eq!(summary.incomplete_request_count, 1);
    assert_eq!(
        summary.incomplete_by_model.get("claude-sonnet-4-6"),
        Some(&1)
    );
    assert_eq!(report.by_day.get(&day), Some(&1));
    // Only the completed row contributes tokens.
    assert_eq!(summary.input_tokens, 1000);
    assert_eq!(summary.output_tokens, 50);
}

#[test]
fn malformed_claude_history_stays_unknown_while_valid_empty_history_is_known_zero() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("transcript.jsonl");
    let cutoff = Utc::now() - Duration::days(1);

    std::fs::write(&path, b"\n").unwrap();
    let mut empty_seen = HashSet::new();
    let mut empty_pricing = ClaudeScanPricingResolver::default();
    let empty_result = scan_claude_file_with_pricing(
        &path,
        &cutoff,
        &mut empty_seen,
        None,
        &mut empty_pricing,
        &mut ClaudeIncompleteTracker::default(),
        |_| {},
    );
    let mut empty_summary = CostSummary::default();
    finalize_claude_summary(&mut empty_summary, true, empty_result, false);
    assert!(empty_summary.history_coverage_established);
    assert!(empty_summary.known_zero);

    std::fs::write(&path, b"{malformed\n").unwrap();
    let mut malformed_seen = HashSet::new();
    let mut malformed_pricing = ClaudeScanPricingResolver::default();
    let malformed_result = scan_claude_file_with_pricing(
        &path,
        &cutoff,
        &mut malformed_seen,
        None,
        &mut malformed_pricing,
        &mut ClaudeIncompleteTracker::default(),
        |_| {},
    );
    assert_eq!(malformed_result.malformed_lines, 1);
    let mut malformed_summary = CostSummary::default();
    finalize_claude_summary(&mut malformed_summary, true, malformed_result, false);
    assert!(!malformed_summary.history_coverage_established);
    assert!(!malformed_summary.known_zero);
}
