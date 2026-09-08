use super::*;
use std::io::Write;

#[test]
fn test_unknown_model_falls_back_to_sonnet() {
    // Unknown/retired Claude IDs fall back to Sonnet 4.6 base pricing
    // ($3/1M input, $15/1M output). 100k tokens stay under the 200k tier.
    let cost =
        ClaudePricing::cost_usd_with_cache_ttl("claude-3-5-sonnet", 100_000, 0, 0, 0, 100_000);
    // 100k * $3/M + 100k * $15/M = 0.30 + 1.50 = 1.80
    assert!((cost - 1.80).abs() < 0.001);
}

#[test]
fn records_unknown_claude_model_while_using_fallback_cost() {
    let event: ClaudeEvent = serde_json::from_str(
        r#"{"type":"assistant","timestamp":"2026-01-15T10:00:00Z","requestId":"req_unknown","message":{"id":"msg_unknown","model":"claude-retired-unknown","usage":{"input_tokens":100000,"output_tokens":100000}}}"#,
    )
    .unwrap();
    let record = claude_usage_record_from_event(&event).expect("usage record");
    let mut summary = CostSummary::default();

    add_claude_record_to_summary(&mut summary, &record);

    assert!(summary.total_cost_usd > 0.0);
    assert!(summary.unknown_models.contains("claude-retired-unknown"));
}

#[test]
fn claude_scan_pricing_resolver_reuses_positive_and_negative_resolution() {
    let unknown = format!("claude-scan-unknown-{}", std::process::id());
    let mut resolver = ClaudeScanPricingResolver::default();

    assert!(resolver.is_known("claude-sonnet-4-6"));
    assert!(!resolver.is_known(&unknown));
    assert!(resolver.is_known("claude-sonnet-4-6"));
    assert!(!resolver.is_known(&unknown));
    assert_eq!(resolver.normalization_cache_misses, 2);
    assert_eq!(resolver.resolution_cache_misses, 2);
    assert_eq!(resolver.resolutions.len(), 2);

    let mut cost_resolver = ClaudeScanPricingResolver::default();
    let resolved_unknown = cost_resolver.cost_usd_with_cache_ttl(&unknown, 100, 20, 10, 30, 40);
    let fallback =
        ClaudePricing::cost_usd_with_cache_ttl(FALLBACK_CLAUDE_MODEL, 100, 20, 10, 30, 40);
    assert!((resolved_unknown - fallback).abs() < f64::EPSILON);
}

#[test]
fn claude_scan_pricing_resolver_preserves_tiered_and_cache_ttl_pricing() {
    let mut resolver = ClaudeScanPricingResolver::default();
    let cases = [
        ("claude-sonnet-4-6", 240_000, 0, 0, 0, 0),
        ("claude-fable-5", 100, 30, 20, 20, 5),
    ];

    for (model, input, cache_create, cache_create_1h, cache_read, output) in cases {
        let actual = resolver.cost_usd_with_cache_ttl(
            model,
            input,
            cache_create,
            cache_create_1h,
            cache_read,
            output,
        );
        let expected = ClaudePricing::cost_usd_with_cache_ttl(
            model,
            input,
            cache_create,
            cache_create_1h,
            cache_read,
            output,
        );
        assert!((actual - expected).abs() < f64::EPSILON, "{model}");
    }
}

#[test]
fn claude_scan_pricing_resolver_bounds_normalization_memo() {
    let mut resolver = ClaudeScanPricingResolver::default();
    for index in 0..(ClaudeScanPricingResolver::MEMO_ENTRY_LIMIT + 8) {
        let model = format!("claude-memo-{index}");
        assert_eq!(resolver.normalize(&model), model);
    }
    assert_eq!(
        resolver.normalized_models.len(),
        ClaudeScanPricingResolver::MEMO_ENTRY_LIMIT
    );

    let misses = resolver.normalization_cache_misses;
    assert_eq!(resolver.normalize("claude-memo-0"), "claude-memo-0");
    assert_eq!(resolver.normalization_cache_misses, misses);
    assert_eq!(resolver.normalize("claude-memo-1024"), "claude-memo-1024");
    assert_eq!(resolver.normalization_cache_misses, misses + 1);
}

#[test]
fn test_claude_fable_5_pricing() {
    let cost = ClaudePricing::cost_usd_with_cache_ttl("claude-fable-5", 100, 10, 0, 20, 5);
    let expected = (100.0 / 1_000_000.0) * 10.00
        + (10.0 / 1_000_000.0) * 12.50
        + (20.0 / 1_000_000.0) * 1.00
        + (5.0 / 1_000_000.0) * 50.00;
    assert!((cost - expected).abs() < f64::EPSILON);
}

#[test]
fn test_claude_one_hour_cache_write_pricing() {
    let cost = ClaudePricing::cost_usd_with_cache_ttl("claude-fable-5", 100, 30, 20, 20, 5);
    let expected = (100.0 / 1_000_000.0) * 10.00
        + (10.0 / 1_000_000.0) * 12.50
        + (20.0 / 1_000_000.0) * 20.00
        + (20.0 / 1_000_000.0) * 1.00
        + (5.0 / 1_000_000.0) * 50.00;
    assert!((cost - expected).abs() < f64::EPSILON);
}

#[test]
fn test_claude_sonnet_46_honors_200k_tier() {
    // Delegating to the canonical table means the scanner now honors the
    // 200k long-context tier: 200k @ $3/M + 40k @ $6/M = 0.60 + 0.24 = 0.84
    // (the scanner's old inline table applied a flat $3/M = 0.72).
    let cost = ClaudePricing::cost_usd_with_cache_ttl("claude-sonnet-4-6", 240_000, 0, 0, 0, 0);
    assert!((cost - 0.84).abs() < 0.001);
}

#[test]
fn test_current_gen_opus_uses_5_25_pricing() {
    // Opus 4.5/4.6/4.7/4.8 bill at $5/1M input + $25/1M output = $30 total.
    // Delegation regression guard: opus-4-8 in particular must resolve
    // through the canonical table (it was missing there before this fix).
    for model in [
        "claude-opus-4-5",
        "claude-opus-4-6",
        "claude-opus-4-7",
        "claude-opus-4-8",
    ] {
        let cost = ClaudePricing::cost_usd_with_cache_ttl(model, 1_000_000, 0, 0, 0, 1_000_000);
        assert!(
            (cost - 30.00).abs() < 0.001,
            "{model} should bill $30 ($5 in + $25 out), got {cost}"
        );
    }
}

#[test]
fn test_legacy_opus_keeps_legacy_pricing() {
    // Legacy Opus 4.0 / 4.1 remain at $15/1M input + $75/1M output = $90 in
    // the canonical table. (Retired IDs absent from the table — e.g. Opus 3
    // `claude-3-opus-...` — fall back to Sonnet instead; they are outside
    // any realistic 30-day scan window.)
    for model in ["claude-opus-4-20250514", "claude-opus-4-1"] {
        let cost = ClaudePricing::cost_usd_with_cache_ttl(model, 1_000_000, 0, 0, 0, 1_000_000);
        assert!(
            (cost - 90.00).abs() < 0.001,
            "{model} should bill $90 ($15 in + $75 out), got {cost}"
        );
    }
}

#[test]
fn test_haiku_45_uses_current_pricing() {
    // Haiku 4.5 bills at $1/1M input + $5/1M output = $6 via the canonical
    // table (previously the scanner under-priced it at the Haiku 3 rate).
    let cost =
        ClaudePricing::cost_usd_with_cache_ttl("claude-haiku-4-5", 1_000_000, 0, 0, 0, 1_000_000);
    assert!(
        (cost - 6.00).abs() < 0.001,
        "haiku-4-5 should bill $6 ($1 in + $5 out), got {cost}"
    );
}

#[test]
fn parses_current_codex_payload_token_count_events() {
    let path = std::env::temp_dir().join(format!(
        "codexbar-current-codex-token-count-{}.jsonl",
        std::process::id()
    ));
    // Use a recent timestamp so the event stays inside the scanner's
    // 30-day window no matter when the test runs. A hardcoded date
    // silently ages out of the window and makes this test fail with 0
    // sessions once it is more than 30 days in the past.
    let recent = (Utc::now() - Duration::hours(1))
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string();
    let mut file = File::create(&path).unwrap();
    writeln!(
        file,
        r#"{{"timestamp":"{ts}","type":"event_msg","payload":{{"type":"token_count","info":{{"model":"gpt-5","total_token_usage":{{"input_tokens":125,"cached_input_tokens":30,"output_tokens":15}}}}}}}}"#,
        ts = recent
    )
    .unwrap();
    let scanner = CostScanner::new(30);
    let mut summary = CostSummary::default();
    let today = Local::now().date_naive();
    let range = CostUsageDayRange::new(codex_period_start(today, 30), today);
    let mut cache = CostUsageCache::default();
    let mut stats = CostScanStats::default();
    scanner.parse_codex_file(&path, &range, &mut summary, &mut cache, None, &mut stats);

    assert_eq!(summary.sessions_count, 1);
    assert_eq!(summary.input_tokens, 125);
    assert_eq!(summary.cached_tokens, 30);
    assert_eq!(summary.output_tokens, 15);
    assert_eq!(
        summary
            .by_model_tokens
            .get("gpt-5")
            .map(ModelTokenCounts::total),
        Some(140)
    );
    assert!(scan_codex_file_cost(&path) > 0.0);
    // Best-effort test cleanup; the file may already be gone.
    let _removed = std::fs::remove_file(&path);
}

#[test]
fn scans_gpt6_astra_usage_with_cached_and_reasoning_tokens() {
    let root = tempfile::tempdir().unwrap();
    let sessions = root.path().join("sessions");
    let cache_root = root.path().join("cache");
    let today = Local::now().date_naive();
    let day = today.format("%Y-%m-%d").to_string();
    let day_dir = sessions
        .join(today.format("%Y").to_string())
        .join(today.format("%m").to_string())
        .join(today.format("%d").to_string());
    std::fs::create_dir_all(&day_dir).unwrap();
    let line = serde_json::json!({
        "timestamp": Local::now().to_rfc3339(),
        "type": "event_msg",
        "payload": {
            "type": "token_count",
            "info": {
                "model": "gpt-6-astra",
                "total_token_usage": {
                    "input_tokens": 1000,
                    "cached_input_tokens": 300,
                    "output_tokens": 100,
                    "reasoning_output_tokens": 7
                }
            }
        }
    });
    std::fs::write(day_dir.join("astra.jsonl"), format!("{line}\n")).unwrap();

    let scanner = CostScanner::new(7)
        .with_options(CostScanOptions::app_driven())
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![sessions]);
    let (summary, _, cache) = scanner.scan_codex_detailed_with_cache(None);

    assert_eq!(summary.input_tokens, 1000);
    assert_eq!(summary.cached_tokens, 300);
    assert_eq!(summary.output_tokens, 100);
    assert_eq!(summary.reasoning_tokens, Some(7));
    // Codex token-count rows expose cache reads, not cache writes. The 700
    // non-cached input tokens therefore use Astra's standard input rate.
    assert!((summary.total_cost_usd - 0.0123).abs() < 1e-12);
    assert_eq!(cache.days[&day]["gpt-6-astra"], vec![1000, 300, 100, 7]);
}

#[test]
fn derives_claude_dedup_key_from_message_and_request_ids() {
    assert_eq!(
        claude_usage_dedup_key(Some("msg_1"), Some("req_1")).as_deref(),
        Some("msg_1:req_1")
    );
    assert_eq!(
        claude_usage_dedup_key(Some("msg_1"), None).as_deref(),
        Some("message:msg_1")
    );
    assert_eq!(
        claude_usage_dedup_key(None, Some("req_1")).as_deref(),
        Some("request:req_1")
    );
    assert_eq!(claude_usage_dedup_key(None, None), None);
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
    assert!(record.cost > 0.0);

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
    let vertex = format!(
        r#"{{"type":"assistant","timestamp":"{timestamp}","requestId":"req_vrtx_123","message":{{"id":"msg_vrtx_123","model":"claude-sonnet-4-6","usage":{{"input_tokens":1000,"output_tokens":500}}}}}}"#
    );
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
    daily_costs.insert(day_key(&day_one), Some(0.0));
    daily_costs.insert(day_key(&day_two), Some(0.0));

    let cutoff = Utc::now() - Duration::days(30);
    let mut seen = HashSet::new();
    for path in [&file_a, &file_b] {
        for_each_claude_usage_record(path, &cutoff, &mut seen, None, |record| {
            add_claude_record_to_daily_costs(&mut daily_costs, record);
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

fn write_codex_session_fixture(sessions_root: &Path, name: &str, input_tokens: u64) -> PathBuf {
    let today = Local::now().date_naive();
    let day_dir = sessions_root
        .join(today.format("%Y").to_string())
        .join(today.format("%m").to_string())
        .join(today.format("%d").to_string());
    std::fs::create_dir_all(&day_dir).unwrap();
    let path = day_dir.join(name);
    let ts = (Utc::now() - Duration::hours(1))
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string();
    let body = format!(
        r#"{{"timestamp":"{ts}","type":"event_msg","payload":{{"type":"token_count","info":{{"model":"gpt-5","total_token_usage":{{"input_tokens":{input_tokens},"cached_input_tokens":0,"output_tokens":5}}}}}}}}
"#
    );
    std::fs::write(&path, body).unwrap();
    path
}

fn write_codex_session_fixture_with_inputs(
    sessions_root: &Path,
    name: &str,
    input_tokens: &[u64],
) -> PathBuf {
    let today = Local::now().date_naive();
    let day_dir = sessions_root
        .join(today.format("%Y").to_string())
        .join(today.format("%m").to_string())
        .join(today.format("%d").to_string());
    std::fs::create_dir_all(&day_dir).unwrap();
    let base = Utc::now() - Duration::hours(1);
    let mut body = String::new();
    for (index, input) in input_tokens.iter().enumerate() {
        let timestamp = (base
            + Duration::seconds(i64::try_from(index).expect("fixture index fits i64")))
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string();
        body.push_str(&format!(
            r#"{{"timestamp":"{timestamp}","type":"event_msg","payload":{{"type":"token_count","info":{{"model":"gpt-5","total_token_usage":{{"input_tokens":{input},"cached_input_tokens":0,"output_tokens":5}}}}}}}}
"#
        ));
    }
    let path = day_dir.join(name);
    std::fs::write(&path, body).unwrap();
    path
}

fn cached_usage_with_packed(day: &str, model: &str, packed: Vec<i32>) -> CostUsageFileUsage {
    CostUsageFileUsage {
        mtime_unix_ms: 0,
        size: 1,
        codex_file_identity: None,
        days: HashMap::from([(
            day.to_string(),
            HashMap::from([(model.to_string(), packed)]),
        )]),
        parsed_bytes: Some(1),
        codex_scan_target_size: None,
        last_model: None,
        last_totals: None,
        codex_token_timestamps_monotonic: None,
        codex_last_token_timestamp: None,
        codex_session_id: None,
        codex_forked_from_id: None,
        codex_fork_timestamp: None,
        codex_unresolved_fork_parent: false,
    }
}

#[test]
fn rebuild_cache_days_preserves_known_reasoning() {
    let day = Local::now().format("%Y-%m-%d").to_string();
    let mut cache = CostUsageCache {
        files: HashMap::from([
            (
                "a".to_string(),
                cached_usage_with_packed(&day, "gpt-5", vec![10, 0, 4, 3]),
            ),
            (
                "b".to_string(),
                cached_usage_with_packed(&day, "gpt-5", vec![5, 0, 2, 1]),
            ),
        ]),
        ..CostUsageCache::default()
    };

    rebuild_cache_days(&mut cache);

    assert_eq!(cache.days[&day]["gpt-5"], vec![15, 0, 6, 4]);
}

#[test]
fn rebuild_cache_days_reasoning_unknown_is_order_independent() {
    let run = |first: Vec<i32>, second: Vec<i32>| {
        let day = Local::now().format("%Y-%m-%d").to_string();
        let mut cache = CostUsageCache {
            files: HashMap::from([
                (
                    "a".to_string(),
                    cached_usage_with_packed(&day, "gpt-5", first),
                ),
                (
                    "b".to_string(),
                    cached_usage_with_packed(&day, "gpt-5", second),
                ),
            ]),
            ..CostUsageCache::default()
        };

        rebuild_cache_days(&mut cache);
        cache.days[&day]["gpt-5"].clone()
    };

    assert_eq!(run(vec![10, 0, 4, 3], vec![5, 0, 2]), vec![15, 0, 6]);
    assert_eq!(run(vec![5, 0, 2], vec![10, 0, 4, 3]), vec![15, 0, 6]);
}

#[test]
fn rebuild_cache_days_zero_row_does_not_poison_reasoning() {
    let day = Local::now().format("%Y-%m-%d").to_string();
    let mut cache = CostUsageCache {
        files: HashMap::from([
            (
                "a".to_string(),
                cached_usage_with_packed(&day, "gpt-5", vec![10, 0, 4, 3]),
            ),
            (
                "b".to_string(),
                cached_usage_with_packed(&day, "gpt-5", vec![0, 0, 0]),
            ),
        ]),
        ..CostUsageCache::default()
    };

    rebuild_cache_days(&mut cache);

    assert_eq!(cache.days[&day]["gpt-5"], vec![10, 0, 4, 3]);
}

#[test]
fn reasoning_survives_scan_rebuild_and_cache_reload() {
    let root = tempfile::tempdir().unwrap();
    let sessions = root.path().join("sessions");
    let cache_root = root.path().join("cache");
    let today = Local::now().date_naive();
    let day = today.format("%Y-%m-%d").to_string();
    let day_dir = sessions
        .join(today.format("%Y").to_string())
        .join(today.format("%m").to_string())
        .join(today.format("%d").to_string());
    std::fs::create_dir_all(&day_dir).unwrap();
    let timestamp = (Utc::now() - Duration::hours(1))
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string();
    let reasoning_line = serde_json::json!({
        "timestamp": timestamp,
        "type": "event_msg",
        "payload": {
            "type": "token_count",
            "info": {
                "model": "gpt-5",
                "total_token_usage": {
                    "input_tokens": 100,
                    "cached_input_tokens": 0,
                    "output_tokens": 20,
                    "reasoning_output_tokens": 7
                }
            }
        }
    });
    std::fs::write(
        day_dir.join("reasoning.jsonl"),
        format!("{reasoning_line}\n"),
    )
    .unwrap();

    let scanner = CostScanner::new(7)
        .with_options(CostScanOptions::app_driven())
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![sessions]);
    let (summary, _, cache) = scanner.scan_codex_detailed_with_cache(None);
    assert_eq!(summary.output_tokens, 20);
    assert_eq!(summary.reasoning_tokens, Some(7));
    let row = &cache.days[&day]["gpt-5"];
    assert!(row.len() >= 4);
    assert_eq!(row[3], 7);

    let loaded = JsonlScanner::load_cache(ProviderId::Codex, Some(&cache_root));
    let loaded_row = &loaded.days[&day]["gpt-5"];
    assert!(loaded_row.len() >= 4);
    assert_eq!(loaded_row[3], 7);
    assert_eq!(
        JsonlScanner::cached_cost_report_from_days(&loaded).reasoning_tokens,
        Some(7)
    );

    let legacy_root = tempfile::tempdir().unwrap();
    let legacy_sessions = legacy_root.path().join("sessions");
    let legacy_cache_root = legacy_root.path().join("cache");
    let legacy_day_dir = legacy_sessions
        .join(today.format("%Y").to_string())
        .join(today.format("%m").to_string())
        .join(today.format("%d").to_string());
    std::fs::create_dir_all(&legacy_day_dir).unwrap();
    let legacy_line = serde_json::json!({
        "timestamp": timestamp,
        "type": "event_msg",
        "payload": {
            "type": "token_count",
            "info": {
                "model": "gpt-5",
                "total_token_usage": {
                    "input_tokens": 100,
                    "cached_input_tokens": 0,
                    "output_tokens": 20
                }
            }
        }
    });
    std::fs::write(
        legacy_day_dir.join("legacy.jsonl"),
        format!("{legacy_line}\n"),
    )
    .unwrap();

    let legacy_scanner = CostScanner::new(7)
        .with_options(CostScanOptions::app_driven())
        .with_cache_root(&legacy_cache_root)
        .with_sessions_dirs(vec![legacy_sessions]);
    let (legacy_summary, _, legacy_cache) = legacy_scanner.scan_codex_detailed_with_cache(None);
    assert_eq!(legacy_summary.output_tokens, 20);
    assert_eq!(legacy_summary.reasoning_tokens, None);
    assert_eq!(legacy_cache.days[&day]["gpt-5"], vec![100, 0, 20]);
    assert!(
        (summary.total_cost_usd - legacy_summary.total_cost_usd).abs() < 1e-12,
        "reasoning metadata must not change cost"
    );
}

fn write_codex_fork_session_fixture(
    sessions_root: &Path,
    name: &str,
    session_id: &str,
    parent_id: Option<&str>,
    fork_timestamp: DateTime<Utc>,
    token_start: DateTime<Utc>,
    totals: &[i64],
) -> PathBuf {
    let today = Local::now().date_naive();
    let day_dir = sessions_root
        .join(today.format("%Y").to_string())
        .join(today.format("%m").to_string())
        .join(today.format("%d").to_string());
    std::fs::create_dir_all(&day_dir).unwrap();

    let mut body = format!(
        "{{\"type\":\"session_meta\",\"timestamp\":\"{}\",\"payload\":{{\"session_id\":\"{}\"",
        fork_timestamp.to_rfc3339(),
        session_id
    );
    if let Some(parent_id) = parent_id {
        body.push_str(&format!(",\"forked_from_id\":\"{parent_id}\""));
    }
    body.push_str("}}\n");

    for (index, total) in totals.iter().enumerate() {
        let timestamp = (token_start
            + Duration::seconds(i64::try_from(index).expect("fixture index fits i64")))
        .to_rfc3339();
        let line = serde_json::json!({
            "timestamp": timestamp,
            "type": "event_msg",
            "payload": {
                "type": "token_count",
                "info": {
                    "model": "gpt-5",
                    "total_token_usage": {
                        "input_tokens": total,
                        "cached_input_tokens": 0,
                        "output_tokens": 5
                    }
                }
            }
        });
        body.push_str(&line.to_string());
        body.push('\n');
    }

    let path = day_dir.join(name);
    std::fs::write(&path, body).unwrap();
    path
}

fn cached_input_total(usage: &CostUsageFileUsage) -> i32 {
    usage
        .days
        .values()
        .flat_map(|models| models.values())
        .map(|tokens| tokens.first().copied().unwrap_or_default())
        .sum()
}

#[test]
fn ordinary_non_fork_session_keeps_cumulative_accounting() {
    let root = tempfile::tempdir().unwrap();
    let sessions = root.path().join("sessions");
    let cache_root = root.path().join("cache");
    let path = write_codex_session_fixture_with_inputs(&sessions, "ordinary.jsonl", &[100, 140]);

    let mut options = CostScanOptions::app_driven();
    options.prefer_newest_codex_sessions_first = false;
    let scanner = CostScanner::new(7)
        .with_options(options)
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![sessions]);
    let (summary, _, cache) = scanner.scan_codex_detailed_with_cache(None);

    assert_eq!(summary.input_tokens, 140);
    let usage = cache
        .files
        .get(&path.to_string_lossy().to_string())
        .unwrap();
    assert_eq!(cached_input_total(usage), 140);
    assert_eq!(usage.codex_forked_from_id, None);
    assert!(!usage.codex_unresolved_fork_parent);
}

#[test]
fn fork_child_counts_only_growth_above_parent_baseline() {
    let root = tempfile::tempdir().unwrap();
    let sessions = root.path().join("sessions");
    let cache_root = root.path().join("cache");
    let base = Utc::now() - Duration::hours(1);
    let fork = base + Duration::seconds(2);
    let parent = write_codex_fork_session_fixture(
        &sessions,
        "parent.jsonl",
        "parent-id",
        None,
        base,
        base,
        &[1_000_000],
    );
    let child = write_codex_fork_session_fixture(
        &sessions,
        "child.jsonl",
        "child-id",
        Some("parent-id"),
        fork,
        base + Duration::seconds(3),
        &[1_000_000, 1_000_140],
    );

    let now = std::time::SystemTime::now();
    File::options()
        .write(true)
        .open(&parent)
        .unwrap()
        .set_modified(now - std::time::Duration::from_secs(10))
        .unwrap();
    File::options()
        .write(true)
        .open(&child)
        .unwrap()
        .set_modified(now - std::time::Duration::from_secs(5))
        .unwrap();

    let mut options = CostScanOptions::app_driven();
    options.prefer_newest_codex_sessions_first = false;
    let scanner = CostScanner::new(7)
        .with_options(options)
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![sessions]);
    let (summary, _, cache) = scanner.scan_codex_detailed_with_cache(None);

    assert_eq!(summary.input_tokens, 1_000_140);
    assert_eq!(summary.sessions_count, 2);
    let child_usage = cache
        .files
        .get(&child.to_string_lossy().to_string())
        .unwrap();
    assert_eq!(cached_input_total(child_usage), 140);
    assert!(!child_usage.codex_unresolved_fork_parent);
    assert!(cache.codex_pending_paths.is_empty());
    assert!(
        cache
            .files
            .contains_key(&parent.to_string_lossy().to_string())
    );
}

#[test]
fn replaced_fork_child_does_not_reuse_cached_identity() {
    let root = tempfile::tempdir().unwrap();
    let sessions = root.path().join("sessions");
    let cache_root = root.path().join("cache");
    let base = Utc::now() - Duration::hours(1);
    let fork = base + Duration::seconds(2);
    let _parent = write_codex_fork_session_fixture(
        &sessions,
        "parent.jsonl",
        "parent-id",
        None,
        base,
        base,
        &[1_000_000],
    );
    let child = write_codex_fork_session_fixture(
        &sessions,
        "child.jsonl",
        "child-id",
        Some("parent-id"),
        fork,
        base + Duration::seconds(3),
        &[1_000_000, 1_000_140],
    );

    let scanner = CostScanner::new(7)
        .with_options({
            let mut options = CostScanOptions::app_driven();
            options.prefer_newest_codex_sessions_first = false;
            options
        })
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![sessions.clone()]);
    let child_key = child.to_string_lossy().to_string();
    let (_, _, first_cache) = scanner.scan_codex_detailed_with_cache(None);
    let first_child = first_cache.files.get(&child_key).unwrap();
    assert_eq!(first_child.codex_session_id.as_deref(), Some("child-id"));
    assert_eq!(
        first_child.codex_forked_from_id.as_deref(),
        Some("parent-id")
    );

    let old_size = std::fs::metadata(&child).unwrap().len();
    write_codex_fork_session_fixture(
        &sessions,
        "child.jsonl",
        "replacement-id",
        None,
        base + Duration::seconds(4),
        base + Duration::seconds(5),
        &[77],
    );
    let new_size = std::fs::metadata(&child).unwrap().len();
    assert_ne!(old_size, new_size);

    let (summary, _, cache) = scanner.scan_codex_detailed_with_cache(None);
    let child_usage = cache.files.get(&child_key).unwrap();
    assert_eq!(
        child_usage.codex_session_id.as_deref(),
        Some("replacement-id")
    );
    assert_eq!(child_usage.codex_forked_from_id, None);
    assert!(!child_usage.codex_unresolved_fork_parent);
    assert_eq!(cached_input_total(child_usage), 77);
    assert_eq!(summary.input_tokens, 1_000_077);
}

#[test]
fn missing_fork_parent_fails_closed_and_persists_pending() {
    let root = tempfile::tempdir().unwrap();
    let sessions = root.path().join("sessions");
    let cache_root = root.path().join("cache");
    let base = Utc::now() - Duration::hours(1);
    let child = write_codex_fork_session_fixture(
        &sessions,
        "child.jsonl",
        "child-id",
        Some("missing-parent"),
        base + Duration::seconds(1),
        base + Duration::seconds(2),
        &[1_000_000, 1_000_140],
    );

    let scanner = CostScanner::new(7)
        .with_options(CostScanOptions::app_driven())
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![sessions]);
    let (summary, _, cache) = scanner.scan_codex_detailed_with_cache(None);

    let child_key = child.to_string_lossy().to_string();
    let child_usage = cache.files.get(&child_key).unwrap();
    assert_eq!(summary.input_tokens, 0);
    assert_eq!(summary.sessions_count, 0);
    assert!(child_usage.days.is_empty());
    assert!(child_usage.codex_unresolved_fork_parent);
    assert!(cache.codex_pending_paths.contains(&child_key));
    assert!(!summary.history_coverage_established);
    assert!(!summary.known_zero);
}

#[test]
fn fork_child_resolves_after_parent_is_cached() {
    let root = tempfile::tempdir().unwrap();
    let sessions = root.path().join("sessions");
    let cache_root = root.path().join("cache");
    let base = Utc::now() - Duration::hours(1);
    let fork = base + Duration::seconds(2);
    let child = write_codex_fork_session_fixture(
        &sessions,
        "child.jsonl",
        "child-id",
        Some("late-parent"),
        fork,
        base + Duration::seconds(3),
        &[1_000_000, 1_000_140],
    );
    let scanner = CostScanner::new(7)
        .with_options({
            let mut options = CostScanOptions::app_driven();
            options.prefer_newest_codex_sessions_first = false;
            options
        })
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![sessions.clone()]);

    let (first, _, first_cache) = scanner.scan_codex_detailed_with_cache(None);
    assert_eq!(first.input_tokens, 0);
    assert!(
        first_cache
            .codex_pending_paths
            .contains(&child.to_string_lossy().to_string())
    );

    let _parent = write_codex_fork_session_fixture(
        &sessions,
        "parent.jsonl",
        "late-parent",
        None,
        base,
        base,
        &[1_000_000],
    );

    let mut resolved = None;
    for _ in 0..3 {
        let (summary, _, cache) = scanner.scan_codex_detailed_with_cache(None);
        if !cache.codex_scan_incomplete {
            resolved = Some((summary, cache));
            break;
        }
    }
    let (summary, cache) = resolved.expect("later bounded pass resolves the child");
    let child_usage = cache
        .files
        .get(&child.to_string_lossy().to_string())
        .unwrap();
    assert_eq!(summary.input_tokens, 1_000_140);
    assert_eq!(cached_input_total(child_usage), 140);
    assert!(!child_usage.codex_unresolved_fork_parent);
    assert!(cache.codex_pending_paths.is_empty());
}

#[test]
fn parent_last_token_after_fork_keeps_child_unresolved() {
    let root = tempfile::tempdir().unwrap();
    let sessions = root.path().join("sessions");
    let cache_root = root.path().join("cache");
    let base = Utc::now() - Duration::hours(1);
    let parent = write_codex_fork_session_fixture(
        &sessions,
        "parent.jsonl",
        "parent-id",
        None,
        base + Duration::seconds(10),
        base + Duration::seconds(10),
        &[1_000_000],
    );
    let child = write_codex_fork_session_fixture(
        &sessions,
        "child.jsonl",
        "child-id",
        Some("parent-id"),
        base + Duration::seconds(1),
        base + Duration::seconds(2),
        &[1_000_000, 1_000_140],
    );

    let scanner = CostScanner::new(7)
        .with_options(CostScanOptions::app_driven())
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![sessions]);
    let (summary, _, cache) = scanner.scan_codex_detailed_with_cache(None);

    let child_usage = cache
        .files
        .get(&child.to_string_lossy().to_string())
        .unwrap();
    assert_eq!(summary.input_tokens, 1_000_000);
    assert_eq!(cached_input_total(child_usage), 0);
    assert!(child_usage.codex_unresolved_fork_parent);
    assert!(
        cache
            .codex_pending_paths
            .contains(&child.to_string_lossy().to_string())
    );
    assert!(
        cache
            .files
            .contains_key(&parent.to_string_lossy().to_string())
    );
}

#[test]
fn deleted_unresolved_child_is_pruned_without_resurrection() {
    let root = tempfile::tempdir().unwrap();
    let sessions = root.path().join("sessions");
    let cache_root = root.path().join("cache");
    let base = Utc::now() - Duration::hours(1);
    let child = write_codex_fork_session_fixture(
        &sessions,
        "child.jsonl",
        "child-id",
        Some("missing-parent"),
        base + Duration::seconds(1),
        base + Duration::seconds(2),
        &[1_000_000, 1_000_140],
    );
    let scanner = CostScanner::new(7)
        .with_options(CostScanOptions::app_driven())
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![sessions]);
    let (_, _, first_cache) = scanner.scan_codex_detailed_with_cache(None);
    assert!(
        first_cache
            .codex_pending_paths
            .contains(&child.to_string_lossy().to_string())
    );

    std::fs::remove_file(&child).unwrap();
    let (summary, _, cache) = scanner.scan_codex_detailed_with_cache(None);
    assert!(summary.history_coverage_established);
    assert!(summary.known_zero);
    assert!(cache.codex_pending_paths.is_empty());
    assert!(
        !cache
            .files
            .contains_key(&child.to_string_lossy().to_string())
    );
}

#[test]
fn fork_baseline_reset_fails_closed_instead_of_billing_fresh_usage() {
    let root = tempfile::tempdir().unwrap();
    let sessions = root.path().join("sessions");
    let cache_root = root.path().join("cache");
    let base = Utc::now() - Duration::hours(1);
    let _parent = write_codex_fork_session_fixture(
        &sessions,
        "parent.jsonl",
        "parent-id",
        None,
        base,
        base,
        &[1_000_000],
    );
    let child = write_codex_fork_session_fixture(
        &sessions,
        "child.jsonl",
        "child-id",
        Some("parent-id"),
        base + Duration::seconds(1),
        base + Duration::seconds(2),
        &[1_000_000, 999_900, 1_000_140],
    );

    let scanner = CostScanner::new(7)
        .with_options(CostScanOptions::app_driven())
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![sessions]);
    let (summary, _, cache) = scanner.scan_codex_detailed_with_cache(None);
    let child_usage = cache
        .files
        .get(&child.to_string_lossy().to_string())
        .unwrap();

    assert_eq!(summary.input_tokens, 1_000_000);
    assert_eq!(cached_input_total(child_usage), 0);
    assert!(child_usage.codex_unresolved_fork_parent);
    assert!(!summary.history_coverage_established);
}

#[test]
fn cost_scan_second_pass_skips_unchanged_files_via_cache() {
    let root = tempfile::tempdir().unwrap();
    let sessions = root.path().join("sessions");
    let cache_root = root.path().join("cache");
    write_codex_session_fixture(&sessions, "a.jsonl", 100);
    write_codex_session_fixture(&sessions, "b.jsonl", 200);

    let scanner = CostScanner::new(7)
        .with_options(CostScanOptions::app_driven())
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![sessions.clone()]);

    let (summary1, stats1) = scanner.scan_codex_detailed(None);
    assert_eq!(stats1.files_parsed, 2, "first pass parses both files");
    assert_eq!(stats1.files_skipped, 0);
    assert_eq!(stats1.codex_metadata_read_paths.len(), 2);
    assert_eq!(stats1.codex_history_read_paths.len(), 2);
    assert_eq!(stats1.codex_read_receipt.metadata_reads, 2);
    assert_eq!(stats1.codex_read_receipt.history_reads, 2);
    assert!(summary1.total_cost_usd > 0.0);
    assert_eq!(summary1.sessions_count, 2);

    // Second pass with default debounce still inspects files but skips re-parse.
    // Use app_driven so we exercise per-file mtime skip rather than whole-scan debounce.
    let (summary2, stats2) = scanner.scan_codex_detailed(None);
    assert_eq!(stats2.files_seen, 2);
    assert_eq!(stats2.files_skipped, 2, "cache hit skips re-parse");
    assert_eq!(stats2.files_parsed, 0);
    assert!(stats2.codex_metadata_read_paths.is_empty());
    assert!(stats2.codex_history_read_paths.is_empty());
    assert_eq!(stats2.codex_read_receipt, Default::default());
    assert_eq!(summary2.input_tokens, summary1.input_tokens);
    assert!((summary2.total_cost_usd - summary1.total_cost_usd).abs() < 1e-9);

    // Force path already used above; confirm debounce short-circuit with default options.
    let debounced = CostScanner::new(7)
        .with_options(CostScanOptions::default())
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![sessions.clone()]);
    let (summary3, stats3) = debounced.scan_codex_detailed(None);
    assert!(
        stats3.used_cache_debounce,
        "default options debounce within 60s"
    );
    assert_eq!(stats3.files_seen, 0);
    assert_eq!(summary3.input_tokens, summary1.input_tokens);

    // app_driven after debounce still re-reads (skip via mtime, not full re-parse).
    let forced = CostScanner::new(7)
        .with_options(CostScanOptions::app_driven())
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![sessions]);
    let (_, stats4) = forced.scan_codex_detailed(None);
    assert!(!stats4.used_cache_debounce);
    assert_eq!(stats4.files_skipped, 2);
    assert_eq!(stats4.files_parsed, 0);
    assert!(stats4.codex_history_read_paths.is_empty());
}

#[test]
fn codex_lazy_history_receipt_reads_only_changed_file_and_matches_fresh_parse() {
    let root = tempfile::tempdir().unwrap();
    let sessions = root.path().join("sessions");
    let cache_root = root.path().join("cache");
    let first_path = write_codex_session_fixture_with_inputs(&sessions, "first.jsonl", &[100]);
    let second_path = write_codex_session_fixture_with_inputs(&sessions, "second.jsonl", &[200]);
    let scanner = CostScanner::new(7)
        .with_options(CostScanOptions::app_driven())
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![sessions.clone()]);

    let (initial, _, _) = scanner.scan_codex_detailed_with_cache(None);
    let (unchanged, unchanged_stats, _) = scanner.scan_codex_detailed_with_cache(None);
    assert_eq!(unchanged.input_tokens, initial.input_tokens);
    assert!(unchanged_stats.codex_metadata_read_paths.is_empty());
    assert!(unchanged_stats.codex_history_read_paths.is_empty());
    assert_eq!(unchanged_stats.codex_read_receipt, Default::default());

    use std::io::Write as _;
    let timestamp = Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
    let extra = format!(
        r#"{{"timestamp":"{timestamp}","type":"event_msg","payload":{{"type":"token_count","info":{{"model":"gpt-5","total_token_usage":{{"input_tokens":150,"cached_input_tokens":0,"output_tokens":5}}}}}}}}
"#
    );
    std::fs::OpenOptions::new()
        .append(true)
        .open(&first_path)
        .unwrap()
        .write_all(extra.as_bytes())
        .unwrap();

    let (incremental, incremental_stats, _) = scanner.scan_codex_detailed_with_cache(None);
    assert_eq!(
        incremental_stats.codex_metadata_read_paths,
        vec![first_path.to_string_lossy().to_string()]
    );
    assert_eq!(
        incremental_stats.codex_history_read_paths,
        vec![first_path.to_string_lossy().to_string()]
    );
    assert_eq!(incremental_stats.codex_read_receipt.metadata_reads, 1);
    assert_eq!(incremental_stats.codex_read_receipt.history_reads, 1);
    assert_eq!(incremental.input_tokens, 350);

    let fresh = CostScanner::new(7)
        .with_options(CostScanOptions::app_driven())
        .with_cache_root(root.path().join("fresh-cache"))
        .with_sessions_dirs(vec![sessions]);
    let (full, full_stats) = fresh.scan_codex_detailed(None);
    assert_eq!(full_stats.codex_history_read_paths.len(), 2);
    assert_eq!(incremental.input_tokens, full.input_tokens);
    assert_eq!(incremental.output_tokens, full.output_tokens);
    assert_eq!(incremental.cached_tokens, full.cached_tokens);
    assert_eq!(incremental.by_model_tokens, full.by_model_tokens);
    assert!((incremental.total_cost_usd - full.total_cost_usd).abs() < 1e-12);
    assert!(second_path.exists());
}

#[test]
fn codex_file_identity_invalidates_same_path_cache_without_eager_history_read() {
    let root = tempfile::tempdir().unwrap();
    let sessions = root.path().join("sessions");
    let cache_root = root.path().join("cache");
    let path = write_codex_session_fixture(&sessions, "replacement.jsonl", 100);
    let scanner = CostScanner::new(7)
        .with_options(CostScanOptions::app_driven())
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![sessions.clone()]);
    let (_, _, _) = scanner.scan_codex_detailed_with_cache(None);
    let old_mtime = std::fs::metadata(&path).unwrap().modified().unwrap();
    let rotated = path.with_extension("old");
    std::fs::rename(&path, &rotated).unwrap();
    let replacement = write_codex_session_fixture(&sessions, "replacement.jsonl", 200);
    // Windows requires a handle with write-attribute access for set_modified;
    // keep the replacement's mtime equal to the original without opening it
    // read-only. The file contents have the same length, so path/mtime/size
    // remain unchanged while the file identity changes.
    std::fs::OpenOptions::new()
        .write(true)
        .open(&replacement)
        .unwrap()
        .set_modified(old_mtime)
        .unwrap();

    let (summary, stats) = scanner.scan_codex_detailed(None);
    assert_eq!(summary.input_tokens, 200);
    assert_eq!(
        stats.codex_history_read_paths,
        vec![replacement.to_string_lossy().to_string()]
    );
    assert_eq!(stats.codex_read_receipt.history_reads, 1);
}

#[test]
fn cancelled_fresh_cache_hit_is_not_authoritative() {
    let root = tempfile::tempdir().unwrap();
    let cache_root = root.path().join("cache");
    let today = Local::now().date_naive().format("%Y-%m-%d").to_string();
    let usage = HashMap::from([(
        today.clone(),
        HashMap::from([("gpt-5.6-sol".to_string(), vec![100, 0, 10])]),
    )]);
    let mut cache = CostUsageCache {
        last_scan_unix_ms: unix_now_ms(),
        files: HashMap::from([(
            "cached.jsonl".to_string(),
            CostUsageFileUsage {
                mtime_unix_ms: 0,
                size: 100,
                codex_file_identity: None,
                days: usage.clone(),
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
        )]),
        days: usage,
        ..Default::default()
    };
    JsonlScanner::save_cache(ProviderId::Codex, &mut cache, Some(&cache_root));

    let cancel = AtomicBool::new(true);
    let scanner = CostScanner::new(7)
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![root.path().join("sessions")]);
    let (summary, stats) = scanner.scan_codex_detailed(Some(&cancel));

    assert!(
        stats.used_cache_debounce,
        "fresh cache should use debounce path"
    );
    assert_eq!(summary.sessions_count, 1, "cached usage is still visible");
    assert!(
        !summary.history_coverage_established,
        "cancelled cache publication must not claim complete history"
    );
    assert!(!summary.known_zero);
}

#[test]
fn cost_scan_cancel_stops_between_files() {
    let root = tempfile::tempdir().unwrap();
    let sessions = root.path().join("sessions");
    let cache_root = root.path().join("cache");
    write_codex_session_fixture(&sessions, "a.jsonl", 100);
    write_codex_session_fixture(&sessions, "b.jsonl", 200);
    write_codex_session_fixture(&sessions, "c.jsonl", 300);

    let cancel = AtomicBool::new(true);
    let scanner = CostScanner::new(7)
        .with_options(CostScanOptions::app_driven())
        .with_cache_root(cache_root)
        .with_sessions_dirs(vec![sessions]);
    let (summary, stats) = scanner.scan_codex_detailed(Some(&cancel));
    assert_eq!(stats.files_seen, 0, "cancel before first file stops walk");
    assert_eq!(summary.sessions_count, 0);
}

#[test]
fn cost_scan_reconciles_deleted_file_to_known_zero() {
    let root = tempfile::tempdir().unwrap();
    let sessions = root.path().join("sessions");
    let cache_root = root.path().join("cache");
    let path = write_codex_session_fixture(&sessions, "deleted.jsonl", 100);

    let scanner = CostScanner::new(7)
        .with_options(CostScanOptions::app_driven())
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![sessions]);
    let (first, _) = scanner.scan_codex_detailed(None);
    assert_eq!(first.sessions_count, 1);
    assert!(first.total_cost_usd > 0.0);

    std::fs::remove_file(&path).unwrap();
    let (second, _) = scanner.scan_codex_detailed(None);

    assert_eq!(second.sessions_count, 0);
    assert_eq!(second.total_cost_usd, 0.0);
    assert!(second.history_coverage_established);
    assert!(second.known_zero);
    let cache = JsonlScanner::load_cache(ProviderId::Codex, Some(&cache_root));
    assert!(cache.files.is_empty(), "deleted JSONL row must be removed");
    assert!(cache.days.is_empty(), "stale daily totals must disappear");
}

#[test]
fn cost_scan_reconciliation_preserves_sibling_totals_once() {
    let root = tempfile::tempdir().unwrap();
    let sessions = root.path().join("sessions");
    let cache_root = root.path().join("cache");
    let deleted = write_codex_session_fixture(&sessions, "deleted.jsonl", 100);
    let sibling = write_codex_session_fixture(&sessions, "sibling.jsonl", 200);

    let scanner = CostScanner::new(7)
        .with_options(CostScanOptions::app_driven())
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![sessions]);
    let (first, _) = scanner.scan_codex_detailed(None);
    assert_eq!(first.sessions_count, 2);

    std::fs::remove_file(&deleted).unwrap();
    let (second, _) = scanner.scan_codex_detailed(None);

    assert_eq!(second.sessions_count, 1);
    assert_eq!(second.input_tokens, 200);
    assert_eq!(second.output_tokens, 5);
    let cache = JsonlScanner::load_cache(ProviderId::Codex, Some(&cache_root));
    assert_eq!(cache.files.len(), 1);
    assert!(
        !cache
            .files
            .contains_key(&deleted.to_string_lossy().to_string())
    );
    assert!(
        cache
            .files
            .contains_key(&sibling.to_string_lossy().to_string())
    );
}

#[test]
fn cancelled_scan_after_deletion_preserves_stale_cache_row() {
    let root = tempfile::tempdir().unwrap();
    let sessions = root.path().join("sessions");
    let cache_root = root.path().join("cache");
    let path = write_codex_session_fixture(&sessions, "deleted.jsonl", 100);

    let scanner = CostScanner::new(7)
        .with_options(CostScanOptions::app_driven())
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![sessions]);
    let (first, _) = scanner.scan_codex_detailed(None);
    assert_eq!(first.sessions_count, 1);
    std::fs::remove_file(&path).unwrap();

    let cancel = AtomicBool::new(true);
    let (cancelled, stats) = scanner.scan_codex_detailed(Some(&cancel));

    assert_eq!(stats.files_seen, 0);
    assert_eq!(cancelled.sessions_count, 0);
    assert!(!cancelled.history_coverage_established);
    assert!(!cancelled.known_zero);
    let cache = JsonlScanner::load_cache(ProviderId::Codex, Some(&cache_root));
    assert!(
        cache
            .files
            .contains_key(&path.to_string_lossy().to_string())
    );
}

#[test]
fn cost_scan_resumes_appended_bytes() {
    let root = tempfile::tempdir().unwrap();
    let sessions = root.path().join("sessions");
    let cache_root = root.path().join("cache");
    let path = write_codex_session_fixture(&sessions, "grow.jsonl", 50);

    let scanner = CostScanner::new(7)
        .with_options(CostScanOptions::app_driven())
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![sessions.clone()]);
    let (s1, st1) = scanner.scan_codex_detailed(None);
    assert_eq!(st1.files_parsed, 1);
    assert_eq!(st1.token_timestamp_comparisons, 0);
    assert_eq!(s1.input_tokens, 50);

    // Append another cumulative token_count event (100 total => +50 delta).
    let ts = Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
    let extra = format!(
        r#"{{"timestamp":"{ts}","type":"event_msg","payload":{{"type":"token_count","info":{{"model":"gpt-5","total_token_usage":{{"input_tokens":100,"cached_input_tokens":0,"output_tokens":10}}}}}}}}
"#
    );
    use std::io::Write as _;
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap();
    f.write_all(extra.as_bytes()).unwrap();
    drop(f);

    // Bump mtime/size visibly on some FS by rewriting metadata via reopen.
    let (s2, st2) = scanner.scan_codex_detailed(None);
    assert_eq!(st2.files_resumed, 1, "grown file resumes from offset");
    assert_eq!(st2.files_parsed, 0);
    assert_eq!(
        st2.token_timestamp_comparisons, 1,
        "resume validates only the cached-prefix boundary and appended event"
    );
    assert_eq!(s2.input_tokens, 100);

    // The append-only path must publish the same aggregate as a fresh
    // full parse; the optimization is allowed to change work, not data.
    let full_scanner = CostScanner::new(7)
        .with_options(CostScanOptions::app_driven())
        .with_cache_root(root.path().join("fresh-cache"))
        .with_sessions_dirs(vec![sessions]);
    let (full, full_stats) = full_scanner.scan_codex_detailed(None);
    assert_eq!(full_stats.files_parsed, 1);
    assert_eq!(s2.input_tokens, full.input_tokens);
    assert_eq!(s2.cached_tokens, full.cached_tokens);
    assert_eq!(s2.output_tokens, full.output_tokens);
    assert_eq!(s2.sessions_count, full.sessions_count);
    assert_eq!(s2.by_model_tokens, full.by_model_tokens);
    assert_eq!(s2.by_model.len(), full.by_model.len());
    for (model, resumed_cost) in &s2.by_model {
        let full_cost = full.by_model.get(model).copied().expect("full model row");
        assert!((resumed_cost - full_cost).abs() < 1e-12);
    }
    assert!((s2.total_cost_usd - full.total_cost_usd).abs() < 1e-12);

    let cached = JsonlScanner::load_cache(ProviderId::Codex, Some(&cache_root));
    let cached_file = cached
        .files
        .get(&path.to_string_lossy().to_string())
        .expect("resumed file cache entry");
    assert_eq!(cached_file.codex_token_timestamps_monotonic, Some(true));
    assert!(cached_file.codex_last_token_timestamp.is_some());
}

#[test]
fn bounded_growing_rollout_freezes_target_and_resumes_a_retained_tail() {
    let root = tempfile::tempdir().unwrap();
    let sessions = root.path().join("sessions");
    let cache_root = root.path().join("cache");
    let path =
        write_codex_session_fixture_with_inputs(&sessions, "growing.jsonl", &[100, 200, 300]);
    let initial_size = i64::try_from(std::fs::metadata(&path).unwrap().len())
        .expect("fixture file length fits i64");
    let first_line_bytes = i64::try_from(
        std::fs::read(&path)
            .unwrap()
            .split(|byte| *byte == b'\n')
            .next()
            .unwrap()
            .len(),
    )
    .expect("fixture line length fits i64")
        + 1;

    let mut options = CostScanOptions::app_driven();
    options.codex_max_session_file_bytes = first_line_bytes;
    options.codex_max_scan_bytes_per_refresh = first_line_bytes;
    let scanner = CostScanner::new(7)
        .with_options(options)
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![sessions.clone()]);

    let (first, _, first_cache) = scanner.scan_codex_detailed_with_cache(None);
    assert_eq!(first.input_tokens, 100);
    let first_usage = first_cache
        .files
        .get(&path.to_string_lossy().to_string())
        .unwrap();
    assert_eq!(first_usage.codex_scan_target_size, Some(initial_size));
    assert_eq!(first_usage.parsed_bytes, Some(first_line_bytes));
    assert!(first_cache.codex_scan_incomplete);

    let timestamp = (Utc::now() - Duration::minutes(10))
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string();
    let append_line = |total: u64| {
        format!(
            r#"{{"timestamp":"{timestamp}","type":"event_msg","payload":{{"type":"token_count","info":{{"model":"gpt-5","total_token_usage":{{"input_tokens":{total},"cached_input_tokens":0,"output_tokens":{}}}}}}}}}
"#,
            total / 10
        )
    };
    let mut next_total = 400_u64;
    let mut bounded_summary = first;
    let mut bounded_cache = first_cache;
    for _ in 0..8 {
        let line = append_line(next_total);
        next_total += 100;
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        file.write_all(line.as_bytes()).unwrap();
        drop(file);

        (bounded_summary, _, bounded_cache) = scanner.scan_codex_detailed_with_cache(None);
        let usage = bounded_cache
            .files
            .get(&path.to_string_lossy().to_string())
            .unwrap();
        assert_eq!(usage.codex_scan_target_size, Some(initial_size));
        assert!(usage.parsed_bytes.unwrap_or_default() <= initial_size);
        if usage.parsed_bytes == Some(initial_size) {
            break;
        }
    }

    assert_eq!(bounded_summary.input_tokens, 300);
    let bounded_usage = bounded_cache
        .files
        .get(&path.to_string_lossy().to_string())
        .unwrap();
    assert_eq!(bounded_usage.parsed_bytes, Some(initial_size));
    assert_eq!(bounded_usage.codex_scan_target_size, Some(initial_size));
    assert!(
        bounded_cache.codex_scan_incomplete,
        "the appended tail stays queued"
    );

    for _ in 0..32 {
        if !bounded_cache.codex_scan_incomplete {
            break;
        }
        (bounded_summary, _, bounded_cache) = scanner.scan_codex_detailed_with_cache(None);
    }
    assert!(!bounded_cache.codex_scan_incomplete);
    let stable_summary = bounded_summary.clone();
    let stable_size = i64::try_from(std::fs::metadata(&path).unwrap().len())
        .expect("fixture file length fits i64");

    let partial_line = append_line(next_total);
    let split = partial_line.len() / 2;
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap();
    file.write_all(&partial_line.as_bytes()[..split]).unwrap();
    drop(file);
    let (partial_summary, _, partial_cache) = scanner.scan_codex_detailed_with_cache(None);
    let partial_usage = partial_cache
        .files
        .get(&path.to_string_lossy().to_string())
        .unwrap();
    assert_eq!(partial_summary.input_tokens, stable_summary.input_tokens);
    assert_eq!(partial_usage.parsed_bytes, Some(stable_size));
    assert_eq!(partial_usage.codex_scan_target_size, Some(stable_size));
    assert!(partial_cache.codex_scan_incomplete);

    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap();
    file.write_all(&partial_line.as_bytes()[split..]).unwrap();
    drop(file);
    next_total += 100;
    let (resumed_summary, _, resumed_cache) = scanner.scan_codex_detailed_with_cache(None);
    assert!(!resumed_cache.codex_scan_incomplete);
    assert_eq!(resumed_summary.input_tokens, next_total - 100);
    let resumed_usage = resumed_cache
        .files
        .get(&path.to_string_lossy().to_string())
        .unwrap();
    assert_eq!(
        resumed_usage.parsed_bytes,
        Some(
            i64::try_from(std::fs::metadata(&path).unwrap().len())
                .expect("fixture file length fits i64"),
        )
    );
    assert_eq!(
        resumed_usage.codex_scan_target_size,
        resumed_usage.parsed_bytes
    );
}

#[test]
fn cost_scan_midline_rewrite_forces_full_parse_not_resume() {
    // F2 (upstream 0.48.0 #2648): when a file is rewritten/truncated so the
    // cached resume offset is now mid-line (byte before offset is not \n),
    // the scanner must fall through to a full re-parse from offset 0 rather
    // than resuming from the stale mid-line offset.
    let root = tempfile::tempdir().unwrap();
    let sessions = root.path().join("sessions");
    let cache_root = root.path().join("cache");
    let _path = write_codex_session_fixture(&sessions, "a.jsonl", 100);

    let scanner = CostScanner::new(7)
        .with_options(CostScanOptions::app_driven())
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![sessions.clone()]);
    let (s1, st1) = scanner.scan_codex_detailed(None);
    assert_eq!(st1.files_parsed, 1);
    assert_eq!(s1.input_tokens, 100);

    // Rewrite the file with a shorter body at the same path so the cached
    // parsed_bytes offset now points mid-line in the new content.
    let today = Local::now().date_naive();
    let day_dir = sessions
        .join(today.format("%Y").to_string())
        .join(today.format("%m").to_string())
        .join(today.format("%d").to_string());
    let ts = (Utc::now() - Duration::minutes(30))
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string();
    // Shorter content with different token count — the cached offset will
    // be past EOF or mid-line in this new content.
    let body = format!(
        r#"{{"timestamp":"{ts}","type":"event_msg","payload":{{"type":"token_count","info":{{"model":"gpt-5","total_token_usage":{{"input_tokens":50,"cached_input_tokens":0,"output_tokens":5}}}}}}}}
"#
    );
    std::fs::write(day_dir.join("a.jsonl"), body).unwrap();

    let (s2, st2) = scanner.scan_codex_detailed(None);
    // The scanner must full-parse (not resume) because the cached offset
    // no longer sits on a line boundary in the rewritten content.
    assert!(
        st2.files_parsed >= 1 || st2.files_resumed == 0,
        "midline rewrite forces full parse, not resume (parsed={}, resumed={})",
        st2.files_parsed,
        st2.files_resumed
    );
    assert_eq!(s2.input_tokens, 50, "full parse picks up new token count");
}

#[test]
fn previous_report_clears_after_successful_full_scan() {
    // F8 (upstream 0.48.0): a completed full scan clears previous_report so
    // the refreshing indicator does not stay permanently on.
    let root = tempfile::tempdir().unwrap();
    let sessions = root.path().join("sessions");
    let cache_root = root.path().join("cache");
    write_codex_session_fixture(&sessions, "a.jsonl", 100);

    let scanner = CostScanner::new(7)
        .with_options(CostScanOptions::app_driven())
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![sessions.clone()]);

    // First scan: builds cache fresh; no previous_report expected.
    let (summary1, _) = scanner.scan_codex_detailed(None);
    assert!(summary1.history_coverage_established);
    let cache = JsonlScanner::load_cache(ProviderId::Codex, Some(&cache_root));
    assert!(
        cache.previous_report.is_none(),
        "first scan clears previous_report"
    );

    // Inject a previous_report to simulate trim-set catch-up.
    let mut cache = JsonlScanner::load_cache(ProviderId::Codex, Some(&cache_root));
    cache.previous_report = Some(crate::core::CachedCostReport {
        total_cost_usd: 0.0,
        input_tokens: 0,
        cached_tokens: 0,
        output_tokens: 0,
        reasoning_tokens: None,
        sessions_count: 0,
        updated_at: None,
        partial: false,
    });
    JsonlScanner::save_cache(ProviderId::Codex, &mut cache, Some(&cache_root));

    // Verify the cache now has previous_report set.
    let cache = JsonlScanner::load_cache(ProviderId::Codex, Some(&cache_root));
    assert!(
        cache.previous_report.is_some(),
        "injected previous_report persists"
    );

    // Full scan with app_driven clears previous_report on success.
    let (summary2, _) = scanner.scan_codex_detailed(None);
    assert!(
        summary2.history_coverage_established,
        "after full scan coverage is established"
    );

    let cache = JsonlScanner::load_cache(ProviderId::Codex, Some(&cache_root));
    assert!(
        cache.previous_report.is_none(),
        "full scan clears previous_report"
    );
}

// ── Upstream 0.50.1 #2932: known-zero history ────────────────────────────

#[test]
fn known_zero_is_set_when_scan_completes_with_no_sessions() {
    let root = tempfile::tempdir().unwrap();
    let sessions = root.path().join("sessions");
    let cache_root = root.path().join("cache");
    std::fs::create_dir_all(&sessions).unwrap();

    let scanner = CostScanner::new(7)
        .with_options(CostScanOptions::app_driven())
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![sessions.clone()]);

    let (summary, _) = scanner.scan_codex_detailed(None);
    assert!(summary.history_coverage_established, "scan completed");
    assert_eq!(summary.sessions_count, 0, "no sessions");
    assert!(summary.known_zero, "completed scan with zero = known-zero");
}

#[test]
fn known_zero_is_not_set_when_scan_has_results() {
    let root = tempfile::tempdir().unwrap();
    let sessions = root.path().join("sessions");
    let cache_root = root.path().join("cache");
    write_codex_session_fixture(&sessions, "a.jsonl", 100);

    let scanner = CostScanner::new(7)
        .with_options(CostScanOptions::app_driven())
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![sessions.clone()]);

    let (summary, _) = scanner.scan_codex_detailed(None);
    assert!(summary.history_coverage_established);
    assert_eq!(summary.sessions_count, 1);
    assert!(!summary.known_zero, "scan with results is not known-zero");
}

#[test]
fn tiny_candidate_limit_prefers_newest_dirty_file_and_persists_older_pending() {
    let root = tempfile::tempdir().unwrap();
    let sessions = root.path().join("sessions");
    let cache_root = root.path().join("cache");
    let older = write_codex_session_fixture(&sessions, "a-older.jsonl", 100);
    let newer = write_codex_session_fixture(&sessions, "z-newer.jsonl", 200);

    let mut options = CostScanOptions::app_driven();
    options.codex_candidate_limit = 1;
    let scanner = CostScanner::new(7)
        .with_options(options)
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![sessions]);

    let (first, first_stats, first_cache) = scanner.scan_codex_detailed_with_cache(None);
    assert_eq!(first_stats.files_parsed, 1);
    assert_eq!(
        first.input_tokens, 200,
        "newest dirty file is processed first"
    );
    assert!(first_cache.codex_scan_incomplete);
    assert_eq!(
        first_cache.codex_pending_paths,
        vec![older.to_string_lossy().to_string()]
    );
    assert!(
        !first_cache
            .codex_pending_paths
            .contains(&newer.to_string_lossy().to_string())
    );

    let (second, _, second_cache) = scanner.scan_codex_detailed_with_cache(None);
    assert_eq!(second.input_tokens, 300);
    assert!(!second_cache.codex_scan_incomplete);
    assert!(second_cache.codex_pending_paths.is_empty());
}

#[test]
fn tiny_byte_limit_resumes_and_drains_to_unbounded_totals() {
    let root = tempfile::tempdir().unwrap();
    let sessions = root.path().join("sessions");
    let cache_root = root.path().join("bounded-cache");
    let path = write_codex_session_fixture_with_inputs(&sessions, "multi.jsonl", &[100, 200, 300]);
    let first_line_bytes = i64::try_from(
        std::fs::read(&path)
            .unwrap()
            .split(|byte| *byte == b'\n')
            .next()
            .unwrap()
            .len(),
    )
    .expect("fixture line length fits i64")
        + 1;

    let mut bounded_options = CostScanOptions::app_driven();
    bounded_options.codex_max_session_file_bytes = first_line_bytes;
    bounded_options.codex_max_scan_bytes_per_refresh = first_line_bytes;
    let bounded = CostScanner::new(7)
        .with_options(bounded_options)
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![sessions.clone()]);

    let (first, first_stats, first_cache) = bounded.scan_codex_detailed_with_cache(None);
    assert_eq!(first_stats.codex_bytes_read, first_line_bytes as u64);
    assert!(first_stats.files_deferred > 0);
    assert!(first_cache.codex_scan_incomplete);
    assert!(
        first_cache
            .files
            .get(&path.to_string_lossy().to_string())
            .expect("partial cache entry")
            .parsed_bytes
            .unwrap_or(0)
            < i64::try_from(std::fs::metadata(&path).unwrap().len())
                .expect("fixture file length fits i64")
    );
    assert!(!first.history_coverage_established);

    let mut final_bounded = None;
    let mut saw_resume = false;
    for _ in 0..8 {
        let (summary, stats, cache) = bounded.scan_codex_detailed_with_cache(None);
        saw_resume |= stats.files_resumed > 0;
        if !cache.codex_scan_incomplete {
            final_bounded = Some(summary);
            break;
        }
    }
    let final_bounded = final_bounded.expect("bounded passes drain");
    assert!(saw_resume, "later passes resume the cached prefix");

    let full = CostScanner::new(7)
        .with_options(CostScanOptions::app_driven())
        .with_cache_root(root.path().join("full-cache"))
        .with_sessions_dirs(vec![sessions]);
    let (full_summary, _, full_cache) = full.scan_codex_detailed_with_cache(None);
    assert!(!full_cache.codex_scan_incomplete);
    assert_eq!(final_bounded.input_tokens, full_summary.input_tokens);
    assert_eq!(final_bounded.cached_tokens, full_summary.cached_tokens);
    assert_eq!(final_bounded.output_tokens, full_summary.output_tokens);
    assert_eq!(final_bounded.sessions_count, full_summary.sessions_count);
    assert_eq!(final_bounded.by_model_tokens, full_summary.by_model_tokens);
    assert!((final_bounded.total_cost_usd - full_summary.total_cost_usd).abs() < 1e-12);
}

#[test]
fn incomplete_summary_preserves_previous_report_and_marks_it_non_authoritative() {
    let root = tempfile::tempdir().unwrap();
    let sessions = root.path().join("sessions");
    let cache_root = root.path().join("cache");
    write_codex_session_fixture_with_inputs(&sessions, "multi.jsonl", &[100, 200]);

    let report = CachedCostReport {
        total_cost_usd: 42.5,
        input_tokens: 11,
        cached_tokens: 2,
        output_tokens: 3,
        reasoning_tokens: Some(7),
        sessions_count: 7,
        updated_at: Some("2026-09-06T00:00:00Z".to_string()),
        partial: false,
    };
    let mut cache = CostUsageCache {
        previous_report: Some(report.clone()),
        codex_scan_incomplete: true,
        ..Default::default()
    };
    JsonlScanner::save_cache(ProviderId::Codex, &mut cache, Some(&cache_root));

    let scanner = CostScanner::new(7)
        .with_options(CostScanOptions::app_driven())
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![sessions]);
    let cancel = AtomicBool::new(true);
    let (summary, _, saved) = scanner.scan_codex_detailed_with_cache(Some(&cancel));

    assert_eq!(summary.total_cost_usd, report.total_cost_usd);
    assert_eq!(summary.input_tokens, report.input_tokens as u64);
    assert_eq!(summary.cached_tokens, report.cached_tokens as u64);
    assert_eq!(summary.output_tokens, report.output_tokens as u64);
    assert_eq!(summary.reasoning_tokens, Some(7));
    assert_eq!(summary.sessions_count, report.sessions_count as u32);
    assert!(!summary.history_coverage_established);
    assert!(!summary.known_zero);
    assert!(summary.model_pricing_completeness.is_partial());
    assert_eq!(
        saved.previous_report.map(|saved| saved.total_cost_usd),
        Some(42.5)
    );
    assert!(saved.codex_scan_incomplete);
}

#[test]
fn failed_catch_up_pause_preserves_cursor_and_report_until_explicit_refresh() {
    let root = tempfile::tempdir().unwrap();
    let sessions = root.path().join("sessions");
    let cache_root = root.path().join("cache");
    let pending = write_codex_session_fixture(&sessions, "pending.jsonl", 100);
    let report = CachedCostReport {
        total_cost_usd: 42.5,
        input_tokens: 11,
        cached_tokens: 2,
        output_tokens: 3,
        reasoning_tokens: Some(7),
        sessions_count: 7,
        updated_at: Some("2026-09-06T00:00:00Z".to_string()),
        partial: false,
    };
    let mut cache = CostUsageCache {
        previous_report: Some(report.clone()),
        codex_pending_paths: vec![pending.to_string_lossy().to_string()],
        codex_scan_incomplete: true,
        ..Default::default()
    };
    JsonlScanner::save_cache(ProviderId::Codex, &mut cache, Some(&cache_root));

    let failed = CostScanner::new(7)
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![root.path().join("temporarily-unavailable")]);
    let (failed_summary, _, failed_cache) = failed.scan_codex_detailed_with_cache(None);
    assert_eq!(
        failed_cache.codex_scan_pause_reason,
        Some(CodexScanPauseReason::Error(
            "Codex session source unavailable".to_string()
        ))
    );
    assert_eq!(failed_cache.codex_pending_paths, cache.codex_pending_paths);
    assert_eq!(
        failed_cache
            .previous_report
            .as_ref()
            .map(|saved| saved.total_cost_usd),
        Some(report.total_cost_usd)
    );
    assert_eq!(failed_summary.total_cost_usd, report.total_cost_usd);

    let background = CostScanner::new(7)
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![sessions.clone()]);
    let (background_summary, background_stats, background_cache) =
        background.scan_codex_detailed_with_cache(None);
    assert_eq!(background_stats.files_parsed, 0);
    assert_eq!(background_summary.total_cost_usd, report.total_cost_usd);
    assert_eq!(
        background_cache.codex_pending_paths,
        failed_cache.codex_pending_paths
    );
    assert_eq!(
        background_cache.codex_scan_pause_reason,
        failed_cache.codex_scan_pause_reason
    );

    let explicit = CostScanner::new(7)
        .with_options(CostScanOptions::app_driven())
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![sessions]);
    let (resumed_summary, _, resumed_cache) = explicit.scan_codex_detailed_with_cache(None);
    assert!(resumed_cache.codex_scan_pause_reason.is_none());
    assert!(!resumed_cache.codex_scan_incomplete);
    assert!(resumed_cache.codex_pending_paths.is_empty());
    assert!(resumed_summary.history_coverage_established);
    assert_eq!(resumed_summary.input_tokens, 100);
    assert!(resumed_cache.previous_report.is_none());
}

#[test]
fn missing_unobserved_sessions_root_does_not_pause_validated_cache() {
    let root = tempfile::tempdir().unwrap();
    let sessions = root.path().join("sessions");
    let missing = root.path().join("optional-sessions");
    let cache_root = root.path().join("cache");
    write_codex_session_fixture(&sessions, "observed.jsonl", 100);

    let initial = CostScanner::new(7)
        .with_options(CostScanOptions::app_driven())
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![sessions.clone()]);
    let (initial_summary, _) = initial.scan_codex_detailed(None);
    assert!(initial_summary.history_coverage_established);

    let mut cache = JsonlScanner::load_cache(ProviderId::Codex, Some(&cache_root));
    cache.last_scan_unix_ms = 1;
    JsonlScanner::save_cache(ProviderId::Codex, &mut cache, Some(&cache_root));

    let background = CostScanner::new(7)
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![sessions, missing]);
    let (summary, _, saved) = background.scan_codex_detailed_with_cache(None);

    assert!(summary.history_coverage_established);
    assert_eq!(summary.input_tokens, 100);
    assert!(!saved.codex_scan_incomplete);
    assert!(saved.codex_scan_pause_reason.is_none());
}

#[test]
fn trace_pruning_preserves_cursor_and_validated_history_until_refresh() {
    let root = tempfile::tempdir().unwrap();
    let sessions = root.path().join("sessions");
    let cache_root = root.path().join("cache");
    let path = write_codex_session_fixture(&sessions, "pruned.jsonl", 100);

    let initial = CostScanner::new(7)
        .with_options(CostScanOptions::app_driven())
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![sessions.clone()]);
    let (initial_summary, _, initial_cache) = initial.scan_codex_detailed_with_cache(None);
    assert_eq!(initial_summary.input_tokens, 100);
    assert!(!initial_cache.codex_scan_incomplete);

    // Keep the next pass outside the scanner debounce while simulating Codex
    // retention pruning the same trace file down to a smaller valid payload.
    let mut initial_cache = JsonlScanner::load_cache(ProviderId::Codex, Some(&cache_root));
    initial_cache.last_scan_unix_ms = 1;
    JsonlScanner::save_cache(ProviderId::Codex, &mut initial_cache, Some(&cache_root));
    let _ = write_codex_session_fixture(&sessions, "pruned.jsonl", 1);

    let background = CostScanner::new(7)
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![sessions.clone()]);
    let (paused_summary, _, paused_cache) = background.scan_codex_detailed_with_cache(None);
    assert_eq!(paused_summary.input_tokens, 100);
    assert_eq!(
        paused_cache.codex_scan_pause_reason,
        Some(CodexScanPauseReason::NoProgress)
    );
    assert_eq!(
        paused_cache.codex_pending_paths,
        vec![path.to_string_lossy().to_string()]
    );
    assert!(paused_cache.previous_report.is_some());

    let explicit = CostScanner::new(7)
        .with_options(CostScanOptions::app_driven())
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![sessions]);
    let (resumed_summary, _, resumed_cache) = explicit.scan_codex_detailed_with_cache(None);
    assert_eq!(resumed_summary.input_tokens, 1);
    assert!(resumed_cache.codex_scan_pause_reason.is_none());
    assert!(resumed_cache.codex_pending_paths.is_empty());
    assert!(resumed_cache.previous_report.is_none());
}

fn expected_codex_scan_start(days: u32) -> String {
    let today = Local::now().date_naive();
    let report_start = today - Duration::days(i64::from(days.saturating_sub(1)));
    CostUsageDayRange::new(report_start, today).scan_since_key
}

fn pending_codex_options(force: bool) -> CostScanOptions {
    let mut options = if force {
        CostScanOptions::app_driven()
    } else {
        CostScanOptions::default()
    };
    options.codex_candidate_limit = 1;
    options
}

#[test]
fn pending_codex_scan_30_to_7_keeps_wide_start_and_narrow_report() {
    let root = tempfile::tempdir().unwrap();
    let sessions = root.path().join("sessions");
    let cache_root = root.path().join("cache");
    for index in 0..3 {
        write_codex_session_fixture(&sessions, &format!("pending-{index}.jsonl"), 100 + index);
    }

    let first = CostScanner::new(30)
        .with_options(pending_codex_options(true))
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![sessions.clone()]);
    let (_, _, first_cache) = first.scan_codex_detailed_with_cache(None);
    assert!(first_cache.codex_scan_incomplete);

    let second = CostScanner::new(7)
        .with_options(pending_codex_options(false))
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![sessions]);
    let (summary, _, second_cache) = second.scan_codex_detailed_with_cache(None);
    let today = Local::now().date_naive();
    assert!(second_cache.codex_scan_incomplete);
    assert_eq!(
        second_cache.codex_pending_scan_since_key.as_deref(),
        Some(expected_codex_scan_start(30).as_str())
    );
    assert_eq!(summary.period_start, Some(today - Duration::days(6)));
    assert_eq!(summary.period_end, Some(today));
}

#[test]
fn pending_codex_scan_7_to_30_expands_to_earliest_start() {
    let root = tempfile::tempdir().unwrap();
    let sessions = root.path().join("sessions");
    let cache_root = root.path().join("cache");
    for index in 0..3 {
        write_codex_session_fixture(&sessions, &format!("pending-{index}.jsonl"), 100 + index);
    }

    let first = CostScanner::new(7)
        .with_options(pending_codex_options(true))
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![sessions.clone()]);
    let (_, _, first_cache) = first.scan_codex_detailed_with_cache(None);
    assert!(first_cache.codex_scan_incomplete);

    let second = CostScanner::new(30)
        .with_options(pending_codex_options(false))
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![sessions]);
    let (_, _, second_cache) = second.scan_codex_detailed_with_cache(None);
    assert!(second_cache.codex_scan_incomplete);
    assert_eq!(
        second_cache.codex_pending_scan_since_key.as_deref(),
        Some(expected_codex_scan_start(30).as_str())
    );
}

#[test]
fn pending_codex_scan_repeated_narrow_wide_alternation_is_monotonic() {
    let root = tempfile::tempdir().unwrap();
    let sessions = root.path().join("sessions");
    let cache_root = root.path().join("cache");
    for index in 0..8 {
        write_codex_session_fixture(&sessions, &format!("pending-{index}.jsonl"), 100 + index);
    }

    let first = CostScanner::new(30)
        .with_options(pending_codex_options(true))
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![sessions.clone()]);
    let (_, _, first_cache) = first.scan_codex_detailed_with_cache(None);
    assert!(first_cache.codex_scan_incomplete);

    for days in [7, 30, 7, 30, 7] {
        let scanner = CostScanner::new(days)
            .with_options(pending_codex_options(false))
            .with_cache_root(&cache_root)
            .with_sessions_dirs(vec![sessions.clone()]);
        let (_, _, cache) = scanner.scan_codex_detailed_with_cache(None);
        assert!(cache.codex_scan_incomplete);
        assert_eq!(
            cache.codex_pending_scan_since_key.as_deref(),
            Some(expected_codex_scan_start(30).as_str())
        );
    }
}

#[test]
fn pending_codex_scan_incompatible_root_timezone_or_end_resets_start() {
    for incompatibility in ["root", "timezone", "end"] {
        let root = tempfile::tempdir().unwrap();
        let sessions = root.path().join("sessions");
        let alternate_sessions = root.path().join("alternate-sessions");
        let cache_root = root.path().join("cache");
        for index in 0..3 {
            write_codex_session_fixture(&sessions, &format!("pending-{index}.jsonl"), 100 + index);
            write_codex_session_fixture(
                &alternate_sessions,
                &format!("alternate-{index}.jsonl"),
                200 + index,
            );
        }

        let first = CostScanner::new(30)
            .with_options(pending_codex_options(true))
            .with_cache_root(&cache_root)
            .with_sessions_dirs(vec![sessions.clone()]);
        let (_, _, first_cache) = first.scan_codex_detailed_with_cache(None);
        assert!(first_cache.codex_scan_incomplete);

        let mut persisted = JsonlScanner::load_cache(ProviderId::Codex, Some(&cache_root));
        match incompatibility {
            "root" => {}
            "timezone" => {
                persisted.codex_pending_scan_timezone = Some("not-the-local-zone".to_string())
            }
            "end" => persisted.codex_pending_scan_until_key = Some("2099-01-01".to_string()),
            _ => unreachable!(),
        }
        JsonlScanner::save_cache(ProviderId::Codex, &mut persisted, Some(&cache_root));

        let roots = if incompatibility == "root" {
            vec![alternate_sessions]
        } else {
            vec![sessions]
        };
        let second = CostScanner::new(7)
            .with_options(pending_codex_options(false))
            .with_cache_root(&cache_root)
            .with_sessions_dirs(roots);
        let (_, _, second_cache) = second.scan_codex_detailed_with_cache(None);
        assert!(second_cache.codex_scan_incomplete);
        assert_eq!(
            second_cache.codex_pending_scan_since_key.as_deref(),
            Some(expected_codex_scan_start(7).as_str()),
            "{incompatibility} context must reset the pending start"
        );
    }
}

#[test]
fn pending_codex_scan_force_rescan_resets_start() {
    let root = tempfile::tempdir().unwrap();
    let sessions = root.path().join("sessions");
    let cache_root = root.path().join("cache");
    for index in 0..3 {
        write_codex_session_fixture(&sessions, &format!("pending-{index}.jsonl"), 100 + index);
    }

    let first = CostScanner::new(30)
        .with_options(pending_codex_options(true))
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![sessions.clone()]);
    let (_, _, first_cache) = first.scan_codex_detailed_with_cache(None);
    assert!(first_cache.codex_scan_incomplete);

    let forced = CostScanner::new(7)
        .with_options(pending_codex_options(true))
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![sessions]);
    let (_, _, forced_cache) = forced.scan_codex_detailed_with_cache(None);
    assert!(forced_cache.codex_scan_incomplete);
    assert_eq!(
        forced_cache.codex_pending_scan_since_key.as_deref(),
        Some(expected_codex_scan_start(7).as_str())
    );
}

#[test]
fn pending_codex_scan_start_survives_restart() {
    let root = tempfile::tempdir().unwrap();
    let sessions = root.path().join("sessions");
    let cache_root = root.path().join("cache");
    for index in 0..3 {
        write_codex_session_fixture(&sessions, &format!("pending-{index}.jsonl"), 100 + index);
    }

    let first = CostScanner::new(30)
        .with_options(pending_codex_options(true))
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![sessions.clone()]);
    let (_, _, first_cache) = first.scan_codex_detailed_with_cache(None);
    assert_eq!(
        first_cache.codex_pending_scan_since_key.as_deref(),
        Some(expected_codex_scan_start(30).as_str())
    );

    let persisted = JsonlScanner::load_cache(ProviderId::Codex, Some(&cache_root));
    assert_eq!(
        persisted.codex_pending_scan_since_key.as_deref(),
        Some(expected_codex_scan_start(30).as_str())
    );

    let restarted = CostScanner::new(7)
        .with_options(pending_codex_options(false))
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![sessions]);
    let (_, _, restarted_cache) = restarted.scan_codex_detailed_with_cache(None);
    assert_eq!(
        restarted_cache.codex_pending_scan_since_key.as_deref(),
        Some(expected_codex_scan_start(30).as_str())
    );
}

#[test]
fn disappeared_trace_path_waits_for_explicit_validation() {
    let root = tempfile::tempdir().unwrap();
    let sessions = root.path().join("sessions");
    let cache_root = root.path().join("cache");
    let path = write_codex_session_fixture(&sessions, "disappeared.jsonl", 100);

    let initial = CostScanner::new(7)
        .with_options(CostScanOptions::app_driven())
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![sessions.clone()]);
    let (initial_summary, _, _) = initial.scan_codex_detailed_with_cache(None);
    assert_eq!(initial_summary.input_tokens, 100);

    let mut cache = JsonlScanner::load_cache(ProviderId::Codex, Some(&cache_root));
    cache.last_scan_unix_ms = 1;
    JsonlScanner::save_cache(ProviderId::Codex, &mut cache, Some(&cache_root));
    std::fs::remove_file(&path).unwrap();

    let background = CostScanner::new(7)
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![sessions.clone()]);
    let (paused_summary, _, paused_cache) = background.scan_codex_detailed_with_cache(None);
    assert_eq!(paused_summary.input_tokens, 100);
    assert_eq!(
        paused_cache.codex_scan_pause_reason,
        Some(CodexScanPauseReason::NoProgress)
    );
    assert_eq!(
        paused_cache.codex_pending_paths,
        vec![path.to_string_lossy().to_string()]
    );

    let explicit = CostScanner::new(7)
        .with_options(CostScanOptions::app_driven())
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![sessions]);
    let (resumed_summary, _, resumed_cache) = explicit.scan_codex_detailed_with_cache(None);
    assert_eq!(resumed_summary.sessions_count, 0);
    assert!(resumed_summary.history_coverage_established);
    assert!(resumed_cache.codex_scan_pause_reason.is_none());
    assert!(resumed_cache.codex_pending_paths.is_empty());
    assert!(resumed_cache.previous_report.is_none());
}

#[test]
fn paused_catch_up_round_trips_without_retrying_in_background() {
    let root = tempfile::tempdir().unwrap();
    let sessions = root.path().join("sessions");
    let cache_root = root.path().join("cache");
    let pending = write_codex_session_fixture(&sessions, "pending.jsonl", 100);
    let report = CachedCostReport {
        total_cost_usd: 9.25,
        input_tokens: 21,
        cached_tokens: 1,
        output_tokens: 5,
        reasoning_tokens: None,
        sessions_count: 2,
        updated_at: Some("2026-09-06T00:00:00Z".to_string()),
        partial: true,
    };
    let mut cache = CostUsageCache {
        previous_report: Some(report.clone()),
        codex_pending_paths: vec![pending.to_string_lossy().to_string()],
        codex_scan_incomplete: true,
        codex_scan_pause_reason: Some(CodexScanPauseReason::NoProgress),
        ..Default::default()
    };

    let encoded = serde_json::to_string(&cache).unwrap();
    let decoded: CostUsageCache = serde_json::from_str(&encoded).unwrap();
    assert_eq!(
        decoded.codex_scan_pause_reason,
        cache.codex_scan_pause_reason
    );

    JsonlScanner::save_cache(ProviderId::Codex, &mut cache, Some(&cache_root));
    let scanner = CostScanner::new(7)
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![sessions]);
    let (summary, stats, saved) = scanner.scan_codex_detailed_with_cache(None);
    assert_eq!(stats.files_parsed, 0);
    assert_eq!(summary.total_cost_usd, report.total_cost_usd);
    assert_eq!(saved.codex_pending_paths, cache.codex_pending_paths);
    assert_eq!(
        saved
            .previous_report
            .as_ref()
            .map(|saved| saved.total_cost_usd),
        Some(report.total_cost_usd)
    );
    assert_eq!(saved.codex_scan_pause_reason, cache.codex_scan_pause_reason);
}

#[test]
fn pending_and_incomplete_round_trip_through_cache_json() {
    let cache = CostUsageCache {
        codex_pending_paths: vec!["C:\\sessions\\pending.jsonl".to_string()],
        codex_scan_incomplete: true,
        ..Default::default()
    };

    let encoded = serde_json::to_string(&cache).unwrap();
    let decoded: CostUsageCache = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded.codex_pending_paths, cache.codex_pending_paths);
    assert!(decoded.codex_scan_incomplete);
}

#[test]
fn deleted_pending_path_is_pruned_after_complete_discovery() {
    let root = tempfile::tempdir().unwrap();
    let sessions = root.path().join("sessions");
    let cache_root = root.path().join("cache");
    let path = write_codex_session_fixture_with_inputs(&sessions, "partial.jsonl", &[100, 200]);
    let first_line_bytes = i64::try_from(
        std::fs::read(&path)
            .unwrap()
            .split(|byte| *byte == b'\n')
            .next()
            .unwrap()
            .len(),
    )
    .expect("fixture line length fits i64")
        + 1;
    let mut options = CostScanOptions::app_driven();
    options.codex_max_session_file_bytes = first_line_bytes;
    options.codex_max_scan_bytes_per_refresh = first_line_bytes;
    let scanner = CostScanner::new(7)
        .with_options(options)
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![sessions]);
    let (_, _, first_cache) = scanner.scan_codex_detailed_with_cache(None);
    assert!(first_cache.codex_scan_incomplete);
    assert_eq!(first_cache.codex_pending_paths.len(), 1);

    std::fs::remove_file(&path).unwrap();
    let (summary, _, second_cache) = scanner.scan_codex_detailed_with_cache(None);
    assert_eq!(summary.sessions_count, 0);
    assert!(summary.history_coverage_established);
    assert!(second_cache.codex_pending_paths.is_empty());
    assert!(!second_cache.codex_scan_incomplete);
    assert!(
        !second_cache
            .files
            .contains_key(&path.to_string_lossy().to_string())
    );
    assert!(second_cache.days.is_empty());
}

#[test]
fn legacy_cache_json_defaults_bounded_scan_state() {
    let legacy = r#"{"last_scan_unix_ms":0,"files":{},"days":{}}"#;
    let cache: CostUsageCache = serde_json::from_str(legacy).unwrap();
    assert!(cache.codex_pending_paths.is_empty());
    assert!(!cache.codex_scan_incomplete);
}

#[test]
fn complete_empty_codex_fragment_persists_in_cache() {
    let root = tempfile::tempdir().unwrap();
    let sessions = root.path().join("sessions");
    let cache_root = root.path().join("cache");
    let path = write_codex_session_fixture(&sessions, "empty.jsonl", 100);
    std::fs::write(&path, b"\n").unwrap();
    let key = path.to_string_lossy().to_string();

    let scanner = CostScanner::new(7)
        .with_options(CostScanOptions::app_driven())
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![sessions]);
    let (summary, _, cache) = scanner.scan_codex_detailed_with_cache(None);

    assert_eq!(summary.input_tokens, 0);
    let entry = cache.files.get(&key).expect("empty fragment is cached");
    assert!(entry.days.is_empty());
    assert_eq!(entry.parsed_bytes, Some(1));
    assert_eq!(entry.codex_scan_target_size, Some(1));

    let persisted = JsonlScanner::load_cache(ProviderId::Codex, Some(&cache_root));
    assert!(persisted.files.contains_key(&key));
}

#[test]
fn complete_empty_codex_fragment_reparses_from_start_after_growth() {
    let root = tempfile::tempdir().unwrap();
    let sessions = root.path().join("sessions");
    let cache_root = root.path().join("cache");
    let path = write_codex_session_fixture(&sessions, "empty.jsonl", 100);
    std::fs::write(&path, b"\n").unwrap();
    let key = path.to_string_lossy().to_string();

    let scanner = CostScanner::new(7)
        .with_options(CostScanOptions::app_driven())
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![sessions.clone()]);
    let (_, _, first_cache) = scanner.scan_codex_detailed_with_cache(None);
    let first = first_cache.files.get(&key).expect("initial empty fragment");
    assert_eq!(first.parsed_bytes, Some(1));
    assert_eq!(first.codex_scan_target_size, Some(1));

    write_codex_session_fixture(&sessions, "empty.jsonl", 100);
    let (grown_summary, stats, grown_cache) = scanner.scan_codex_detailed_with_cache(None);

    assert_eq!(grown_summary.input_tokens, 100);
    assert_eq!(stats.files_resumed, 0);
    assert!(!grown_cache.files[&key].days.is_empty());
}

#[test]
fn incomplete_or_buffered_empty_codex_fragment_is_not_marked_complete() {
    let root = tempfile::tempdir().unwrap();
    let sessions = root.path().join("sessions");
    let cache_root = root.path().join("cache");
    let path = write_codex_session_fixture(&sessions, "incomplete.jsonl", 100);
    std::fs::write(&path, br#"{"timestamp":"2026-09-07T00:00:00Z""#).unwrap();
    let key = path.to_string_lossy().to_string();

    let scanner = CostScanner::new(7)
        .with_options(CostScanOptions::app_driven())
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![sessions]);
    let (_, _, cache) = scanner.scan_codex_detailed_with_cache(None);

    let entry = cache
        .files
        .get(&key)
        .expect("incomplete fragment is tracked");
    assert!(entry.days.is_empty());
    assert_ne!(entry.parsed_bytes, Some(entry.size));
    assert!(cache.codex_scan_incomplete);
    assert!(cache.codex_pending_paths.contains(&key));

    let buffered_root = tempfile::tempdir().unwrap();
    let buffered_sessions = buffered_root.path().join("sessions");
    let buffered_cache_root = buffered_root.path().join("cache");
    let buffered_path = write_codex_session_fixture(&buffered_sessions, "buffered.jsonl", 100);
    std::fs::write(&buffered_path, b"\nnot-yet-read").unwrap();
    let buffered_key = buffered_path.to_string_lossy().to_string();
    let mut options = CostScanOptions::app_driven();
    options.codex_max_session_file_bytes = 1;
    options.codex_max_scan_bytes_per_refresh = 1;
    let buffered_scanner = CostScanner::new(7)
        .with_options(options)
        .with_cache_root(&buffered_cache_root)
        .with_sessions_dirs(vec![buffered_sessions]);
    let (_, _, buffered_cache) = buffered_scanner.scan_codex_detailed_with_cache(None);

    let buffered_entry = buffered_cache
        .files
        .get(&buffered_key)
        .expect("buffered fragment is tracked");
    assert!(buffered_entry.days.is_empty());
    assert_ne!(buffered_entry.parsed_bytes, Some(buffered_entry.size));
    assert!(buffered_cache.codex_scan_incomplete);
    assert!(buffered_cache.codex_pending_paths.contains(&buffered_key));
}
