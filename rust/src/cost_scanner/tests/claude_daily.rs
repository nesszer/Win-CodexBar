//! Claude daily tokens and cost history, Vertex rows, oversized and non-finite rows.

use super::*;

/// One file's daily token scan, counting each record that cannot be
/// aggregated as the token history path does.
fn scan_claude_file_for_daily_tokens(
    path: &Path,
    cutoff: &DateTime<Utc>,
    seen: &mut HashSet<ClaudeUsageDedupKey>,
    pricing: &mut ClaudeScanPricingResolver,
    daily_tokens: &mut HashMap<String, u64>,
) -> ClaudeFileScanResult {
    let mut aggregation_failures = 0u32;
    // Token history ignores incomplete-request markers.
    let mut incomplete = ClaudeIncompleteTracker::default();
    let mut result = scan_claude_file_with_pricing(
        path,
        cutoff,
        seen,
        None,
        pricing,
        &mut incomplete,
        |record| {
            if record.timestamp.is_none()
                || !add_claude_record_to_daily_tokens(daily_tokens, record)
            {
                aggregation_failures = aggregation_failures.saturating_add(1);
            }
        },
    );
    result.aggregation_failures = result
        .aggregation_failures
        .saturating_add(aggregation_failures);
    result
}

#[test]
fn claude_daily_token_coverage_requires_a_complete_valid_scan() {
    let root = tempfile::tempdir().unwrap();
    let cutoff = Utc::now() - Duration::days(1);
    let valid_path = root.path().join("valid.jsonl");
    let timestamp = Utc::now() - Duration::hours(1);
    let today = timestamp
        .with_timezone(&Local)
        .date_naive()
        .format("%Y-%m-%d")
        .to_string();
    std::fs::write(
        &valid_path,
        format!(
            "{}\n",
            claude_transcript_line(
                &timestamp.to_rfc3339(),
                "requestId",
                "req_valid",
                "msg_valid"
            )
        ),
    )
    .unwrap();

    let mut valid_tokens = HashMap::from([(today.clone(), 0)]);
    let valid_result = scan_claude_file_for_daily_tokens(
        &valid_path,
        &cutoff,
        &mut HashSet::new(),
        &mut ClaudeScanPricingResolver::default(),
        &mut valid_tokens,
    );
    assert!(valid_result.is_complete());
    let mut covered_days = HashSet::new();
    mark_claude_daily_token_coverage(&mut covered_days, &valid_tokens, valid_result);
    assert!(covered_days.contains(&today));

    let assert_uncovered = |path: &Path| {
        let mut daily_tokens = HashMap::from([(today.clone(), 0)]);
        let result = scan_claude_file_for_daily_tokens(
            path,
            &cutoff,
            &mut HashSet::new(),
            &mut ClaudeScanPricingResolver::default(),
            &mut daily_tokens,
        );
        assert!(!result.is_complete());
        let mut covered_days = HashSet::from(["stale-coverage".to_string()]);
        mark_claude_daily_token_coverage(&mut covered_days, &daily_tokens, result);
        assert!(covered_days.is_empty());
        result
    };

    let malformed_path = root.path().join("malformed.jsonl");
    std::fs::write(&malformed_path, b"{malformed\n").unwrap();
    assert_eq!(assert_uncovered(&malformed_path).malformed_lines, 1);

    let incomplete_path = root.path().join("incomplete.jsonl");
    std::fs::write(
        &incomplete_path,
        r#"{"type":"assistant","message":{"id":"msg_preliminary","model":"gpt-5.6-sol","stop_reason":null,"usage":{"input_tokens":1000}}}"#,
    )
    .unwrap();
    assert_eq!(assert_uncovered(&incomplete_path).incomplete_requests, 1);

    let missing_timestamp_path = root.path().join("missing-timestamp.jsonl");
    std::fs::write(
        &missing_timestamp_path,
        r#"{"type":"assistant","requestId":"req_no_timestamp","message":{"id":"msg_no_timestamp","model":"claude-sonnet-4-6","usage":{"input_tokens":1000,"output_tokens":500}}}"#,
    )
    .unwrap();
    assert_eq!(
        assert_uncovered(&missing_timestamp_path).aggregation_failures,
        1
    );

    let unreadable_path = root.path().join("missing.jsonl");
    assert_eq!(assert_uncovered(&unreadable_path).read_failures, 1);

    let scanner = CostScanner::new(1);
    let missing_directory = root.path().join("missing-directory");
    let traversal_read_failures =
        scanner.walk_claude_files(&missing_directory, &cutoff, None, &mut |_| {});
    assert_eq!(traversal_read_failures, 1);
    let mut covered_days = HashSet::from([today]);
    mark_claude_daily_token_coverage(
        &mut covered_days,
        &valid_tokens,
        ClaudeFileScanResult {
            read_failures: traversal_read_failures,
            ..ClaudeFileScanResult::default()
        },
    );
    assert!(covered_days.is_empty());
}

#[test]
fn public_claude_daily_token_dispatch_reports_incomplete_fixture_scans() {
    const CHILD_MARKER: &str = "CODEXBAR_CLAUDE_DAILY_TOKEN_TEST_CHILD";
    const CHILD_DONE: &str = "isolated Claude daily-history fixture verified";
    if std::env::var_os(CHILD_MARKER).is_some() {
        let config_dir = std::env::var_os("CLAUDE_CONFIG_DIR")
            .map(PathBuf::from)
            .expect("child receives isolated Claude config directory");
        let projects_dir = config_dir.join("projects");
        let project_dir = projects_dir.join("fixture-project");
        let (complete_history, incomplete) = get_daily_token_history("claude", 1);
        assert!(!incomplete, "valid fixture scan should establish coverage");
        assert!(complete_history.iter().any(|(_, tokens)| *tokens > 0));

        std::fs::write(project_dir.join("malformed.jsonl"), b"{malformed\n").unwrap();
        let (partial_history, incomplete) = get_daily_token_history("claude", 1);
        assert!(
            incomplete,
            "malformed fixture should leave coverage incomplete"
        );
        assert_eq!(partial_history, complete_history);
        println!("{CHILD_DONE}");
        return;
    }

    let config_dir = tempfile::tempdir().unwrap();
    let project_dir = config_dir.path().join("projects").join("fixture-project");
    std::fs::create_dir_all(&project_dir).unwrap();
    let timestamp = Utc::now().to_rfc3339();
    std::fs::write(
        project_dir.join("valid.jsonl"),
        format!(
            "{}\n",
            claude_transcript_line(&timestamp, "requestId", "req_public", "msg_public")
        ),
    )
    .unwrap();

    let test_thread = std::thread::current();
    let test_name = test_thread.name().expect("test harness names this thread");
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", test_name, "--nocapture", "--test-threads=1"])
        .env(CHILD_MARKER, "1")
        .env("CLAUDE_CONFIG_DIR", config_dir.path())
        .output()
        .expect("spawn isolated exact-test child");
    assert!(
        output.status.success() && String::from_utf8_lossy(&output.stdout).contains(CHILD_DONE),
        "fixture child failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn classifies_vertex_ai_claude_metadata_without_changing_anthropic_rows() {
    let cases = [
        (
            r#"{"type":"assistant","requestId":"req_vrtx_123","message":{"id":"msg_1","model":"claude-sonnet-4-6","usage":{"input_tokens":1}}}"#,
            true,
        ),
        (
            r#"{"type":"assistant","requestId":"req_1","message":{"id":"msg_vrtx_123","model":"claude-sonnet-4-6","usage":{"input_tokens":1}}}"#,
            true,
        ),
        (
            r#"{"type":"assistant","requestId":"req_1","message":{"id":"msg_1","model":"claude-sonnet-4-6@20260217","usage":{"input_tokens":1}}}"#,
            true,
        ),
        (
            r#"{"type":"assistant","requestId":"req_1","message":{"id":"msg_1","model":"claude-sonnet-4-6","metadata":{"provider":"Google-Vertex-AI"},"usage":{"input_tokens":1}}}"#,
            true,
        ),
        (
            r#"{"type":"assistant","requestId":"req_1","message":{"id":"msg_1","model":"claude-sonnet-4-6","content":[{"context":{"gcp_project":false}}],"usage":{"input_tokens":1}}}"#,
            true,
        ),
        (
            r#"{"type":"assistant","requestId":"req_1","message":{"id":"msg_1","model":"claude-sonnet-4-6","usage":{"input_tokens":1}} ,"metadata":{"provider":"anthropic"}}"#,
            false,
        ),
        (
            r#"{"type":"assistant","requestId":"req_1","message":{"id":"msg_1","model":"claude-sonnet-4-6","usage":{"input_tokens":1}} ,"metadata":{"provider":"gcp"}}"#,
            false,
        ),
        (
            r#"{"type":"assistant","requestId":"req_1","message":{"id":"msg_1","model":"claude-sonnet-4-6","content":[{"text":"vertex"}],"usage":{"input_tokens":1}}}"#,
            false,
        ),
        (
            r#"{"type":"assistant","requestId":"req_1","message":{"id":"msg_1","model":"Claude-sonnet-4-6@20260217","usage":{"input_tokens":1}}}"#,
            false,
        ),
    ];

    for (json, expected) in cases {
        let event: ClaudeEvent = serde_json::from_str(json).unwrap();
        assert_eq!(event.is_vertex_ai_usage_entry(), expected, "{json}");
    }
}

#[test]
fn shared_claude_reader_excludes_vertex_rows_but_keeps_anthropic_usage() {
    let path = std::env::temp_dir().join(format!(
        "codexbar-claude-vertex-filter-{}.jsonl",
        std::process::id()
    ));
    let timestamp = (Utc::now() - Duration::hours(1)).to_rfc3339();
    let anthropic = format!(
        r#"{{"type":"assistant","timestamp":"{timestamp}","requestId":"req_anthropic","message":{{"id":"msg_anthropic","model":"claude-sonnet-4-6","usage":{{"input_tokens":10,"output_tokens":5}}}}}}"#
    );
    let vertex = serde_json::json!({
        "type": "assistant",
        "timestamp": timestamp,
        "requestId": "req_vrtx_123",
        "message": {
            "id": "msg_vrtx_123",
            "model": "claude-sonnet-4-6",
            "usage": {"input_tokens": u64::MAX, "output_tokens": u64::MAX}
        }
    })
    .to_string();
    std::fs::write(&path, format!("{anthropic}\n{vertex}\n")).unwrap();

    let cutoff = Utc::now() - Duration::days(30);
    let mut seen = HashSet::new();
    let mut records = Vec::new();
    let counted = for_each_claude_usage_record(&path, &cutoff, &mut seen, None, |record| {
        records.push((record.input, record.output))
    });

    assert_eq!(counted, 1);
    assert_eq!(records, vec![(10, 5)]);
    let _removed = std::fs::remove_file(&path);
}

#[test]
fn oversized_claude_history_preserves_independent_components_and_fails_closed() {
    let first: ClaudeEvent = serde_json::from_str(&format!(
        r#"{{"type":"assistant","timestamp":"2026-09-20T12:00:00Z","requestId":"req_overflow_1","message":{{"id":"msg_overflow_1","model":"claude-sonnet-4-6","usage":{{"input_tokens":{},"output_tokens":2}}}}}}"#,
        u64::MAX
    ))
    .unwrap();
    let second: ClaudeEvent = serde_json::from_str(
        r#"{"type":"assistant","timestamp":"2026-09-20T12:01:00Z","requestId":"req_overflow_2","message":{"id":"msg_overflow_2","model":"claude-sonnet-4-6","usage":{"input_tokens":1,"output_tokens":3}}}"#,
    )
    .unwrap();
    let first = claude_usage_record_from_event(&first).expect("first usage row");
    let second = claude_usage_record_from_event(&second).expect("second usage row");
    let mut summary = CostSummary::default();

    assert!(add_claude_record_to_summary(&mut summary, &first));
    assert!(!add_claude_record_to_summary(&mut summary, &second));
    assert_eq!(summary.input_tokens, u64::MAX);
    assert_eq!(summary.output_tokens, 5);
    assert!(summary.total_cost_usd.is_finite());

    finalize_claude_summary(
        &mut summary,
        true,
        ClaudeFileScanResult {
            counted: 2,
            aggregation_failures: 1,
            ..ClaudeFileScanResult::default()
        },
        false,
    );
    assert!(!summary.history_coverage_established);
    assert!(!summary.known_zero);
}

#[test]
fn oversized_single_claude_row_keeps_cost_but_marks_combined_quota_tokens_unknown() {
    let event: ClaudeEvent = serde_json::from_str(&format!(
        r#"{{"type":"assistant","timestamp":"2026-09-20T12:00:00Z","requestId":"req_combined_overflow","message":{{"id":"msg_combined_overflow","model":"claude-sonnet-4-6","usage":{{"input_tokens":{},"output_tokens":1}}}}}}"#,
        u64::MAX
    ))
    .unwrap();
    let record = claude_usage_record_from_event(&event).expect("usage row");
    let quota = quota_history_record_from_usage(&record).expect("timestamped quota row");

    assert!(record.cost.is_some_and(f64::is_finite));
    assert_eq!(quota.tokens, None);
    assert!(!quota.tokens_are_complete);
    assert!(quota.cost_usd.is_some_and(f64::is_finite));
    assert!(quota.cost_is_complete);
}

#[test]
fn nonfinite_claude_price_is_unknown_instead_of_zero() {
    let snapshot = crate::core::ModelsDevPricingSnapshot::from_catalog_json_for_tests(
        r#"{
            "anthropic": {"models": {"claude-test-extreme-price": {
                "id": "claude-test-extreme-price", "cost": {"input": 1e308, "output": 1}
            }}}
        }"#,
    )
    .expect("pricing fixture");
    let mut pricing = ClaudeScanPricingResolver::with_snapshot(snapshot);
    let event: ClaudeEvent = serde_json::from_str(&format!(
        r#"{{"type":"assistant","timestamp":"2026-09-20T12:00:00Z","requestId":"req_nonfinite","message":{{"id":"msg_nonfinite","model":"claude-test-extreme-price","usage":{{"input_tokens":{},"output_tokens":1}}}}}}"#,
        u64::MAX
    ))
    .unwrap();
    let record =
        claude_usage_record_from_event_with_pricing(&event, &mut pricing).expect("usage row");
    let mut summary = CostSummary::default();

    assert_eq!(record.cost, None);
    assert!(!add_claude_record_to_summary(&mut summary, &record));
    assert_eq!(summary.input_tokens, u64::MAX);
    assert_eq!(summary.output_tokens, 1);
    assert_eq!(summary.total_cost_usd, 0.0);
    assert!(!summary.known_zero);
}

fn claude_transcript_line(
    timestamp: &str,
    request_key: &str,
    request_id: &str,
    message_id: &str,
) -> String {
    format!(
        r#"{{"type":"assistant","timestamp":"{timestamp}","{request_key}":"{request_id}","message":{{"id":"{message_id}","model":"claude-sonnet-4-6","usage":{{"input_tokens":1000,"output_tokens":500}}}}}}"#
    )
}

#[test]
fn daily_history_dedups_across_files_and_buckets_by_local_day() {
    // End-to-end regression for the daily buckets: two transcript files,
    // two different days, plus a replay of the day-one record in the
    // second file (snake_case request_id, as another writer would emit).
    let dir = std::env::temp_dir();
    let file_a = dir.join(format!(
        "codexbar-claude-daily-a-{}.jsonl",
        std::process::id()
    ));
    let file_b = dir.join(format!(
        "codexbar-claude-daily-b-{}.jsonl",
        std::process::id()
    ));

    // >24h apart guarantees two distinct local calendar days.
    let day_one = Utc::now() - Duration::hours(30);
    let day_two = Utc::now() - Duration::hours(2);
    let ts_one = day_one.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
    let ts_two = day_two.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();

    std::fs::write(
        &file_a,
        format!(
            "{}\n{}\n",
            claude_transcript_line(&ts_one, "requestId", "req_1", "msg_1"),
            claude_transcript_line(&ts_two, "requestId", "req_2", "msg_2"),
        ),
    )
    .unwrap();
    std::fs::write(
        &file_b,
        format!(
            "{}\n",
            claude_transcript_line(&ts_one, "request_id", "req_1", "msg_1"),
        ),
    )
    .unwrap();

    let day_key = |ts: &DateTime<Utc>| {
        ts.with_timezone(&Local)
            .date_naive()
            .format("%Y-%m-%d")
            .to_string()
    };
    let mut daily_costs = HashMap::new();
    daily_costs.insert(day_key(&day_one), None);
    daily_costs.insert(day_key(&day_two), None);
    let mut unknown_cost_dates = HashSet::new();

    let cutoff = Utc::now() - Duration::days(30);
    let mut seen = HashSet::new();
    for path in [&file_a, &file_b] {
        for_each_claude_usage_record(path, &cutoff, &mut seen, None, |record| {
            add_claude_record_to_daily_costs(&mut daily_costs, &mut unknown_cost_dates, record);
        });
    }

    let day_one_cost = daily_costs[&day_key(&day_one)].expect("day one cost");
    let day_two_cost = daily_costs[&day_key(&day_two)].expect("day two cost");
    assert!(day_one_cost > 0.0, "day one should carry real cost");
    // Identical usage on both days: equal buckets proves the file-b
    // replay was de-duplicated (a leak would double day one).
    assert!(
        (day_one_cost - day_two_cost).abs() < f64::EPSILON,
        "each day should hold exactly one record's cost, got {day_one_cost} vs {day_two_cost}"
    );

    // Best-effort test cleanup; the files may already be gone.
    let _removed_a = std::fs::remove_file(&file_a);
    let _removed_b = std::fs::remove_file(&file_b);
}

fn claude_daily_cost_record(timestamp: DateTime<Utc>, cost: Option<f64>) -> ClaudeUsageRecord {
    ClaudeUsageRecord {
        model: "claude-test".to_string(),
        pricing_known: cost.is_some(),
        timestamp: Some(timestamp),
        dedup_key: None,
        input: 1,
        output: 1,
        cache_create: 0,
        cache_read: 0,
        cost,
    }
}

#[test]
fn unknown_claude_cost_date_cannot_be_restored_by_later_priced_record() {
    let timestamp = Utc::now();
    let day = timestamp
        .with_timezone(&Local)
        .date_naive()
        .format("%Y-%m-%d")
        .to_string();
    let mut daily_costs = HashMap::from([(day.clone(), None)]);
    let mut unknown_cost_dates = HashSet::new();

    assert!(add_claude_record_to_daily_costs(
        &mut daily_costs,
        &mut unknown_cost_dates,
        &claude_daily_cost_record(timestamp, Some(0.75)),
    ));
    assert!(!add_claude_record_to_daily_costs(
        &mut daily_costs,
        &mut unknown_cost_dates,
        &claude_daily_cost_record(timestamp, None),
    ));
    assert!(!add_claude_record_to_daily_costs(
        &mut daily_costs,
        &mut unknown_cost_dates,
        &claude_daily_cost_record(timestamp, Some(1.25)),
    ));

    assert_eq!(daily_costs[&day], None);
    assert!(unknown_cost_dates.contains(&day));
}

#[test]
fn claude_daily_zero_fill_preserves_unknown_dates_and_fills_untouched_dates() {
    let unknown_day = "2026-09-22".to_string();
    let untouched_day = "2026-09-23".to_string();
    let mut daily_costs =
        HashMap::from([(unknown_day.clone(), None), (untouched_day.clone(), None)]);
    let unknown_cost_dates = HashSet::from([unknown_day.clone()]);

    zero_fill_uninitialized_claude_daily_costs(&mut daily_costs, &unknown_cost_dates);

    assert_eq!(daily_costs[&unknown_day], None);
    assert_eq!(daily_costs[&untouched_day], Some(0.0));
}

#[test]
fn claude_scan_counts_final_incomplete_jsonl_line() {
    let path =
        std::env::temp_dir().join(format!("codexbar-claude-tail-{}.jsonl", std::process::id()));
    let ts = (Utc::now() - Duration::hours(1))
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string();
    // No trailing newline — the last (only) record must still be counted.
    let body = claude_transcript_line(&ts, "requestId", "req_tail", "msg_tail");
    std::fs::write(&path, body.as_bytes()).unwrap();

    let cutoff = Utc::now() - Duration::days(1);
    let mut seen = HashSet::new();
    let counted = for_each_claude_usage_record(&path, &cutoff, &mut seen, None, |_| {});
    assert_eq!(counted, 1, "incomplete final JSONL line must be processed");
    // Best-effort test cleanup; the file may already be gone.
    let _removed = std::fs::remove_file(&path);
}
