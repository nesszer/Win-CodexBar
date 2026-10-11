//! Codex token payloads, reasoning tokens and cached day rebuilds.

use super::*;

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
    let (_root, sessions, cache_root) = codex_scan_dirs();
    let today = Local::now().date_naive();
    let day = today.format("%Y-%m-%d").to_string();
    let day_dir = partition_dir(&sessions, today);
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

    let scanner = app_scanner(7, &cache_root, &sessions);
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
fn recent_codex_fixture_time_stays_on_the_local_day() {
    let utc_plus_7 = FixedOffset::east_opt(7 * 3600).unwrap();
    let at = |hour, minute| {
        utc_plus_7
            .with_ymd_and_hms(2026, 10, 1, hour, minute, 0)
            .unwrap()
    };
    assert_eq!(recent_fixture_time_at(at(8, 30)), at(7, 30));
    assert_eq!(recent_fixture_time_at(at(1, 0)), at(0, 0));
    assert_eq!(recent_fixture_time_at(at(0, 40)), at(0, 0));
    assert_eq!(recent_fixture_time_at(at(0, 0)), at(0, 0));

    let ci_run = Utc.with_ymd_and_hms(2026, 10, 1, 0, 45, 0).unwrap();
    assert_eq!(
        recent_fixture_time_at(ci_run),
        Utc.with_ymd_and_hms(2026, 10, 1, 0, 0, 0).unwrap()
    );
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
    let run = |first: Vec<i64>, second: Vec<i64>| {
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
fn rebuild_cache_days_aggregates_multiple_files_above_i32_max() {
    let day = Local::now().format("%Y-%m-%d").to_string();
    let mut cache = CostUsageCache {
        files: HashMap::from([
            (
                "a".to_string(),
                cached_usage_with_packed(&day, "gpt-5", vec![1_500_000_000, 1_400_000_000, 100]),
            ),
            (
                "b".to_string(),
                cached_usage_with_packed(&day, "gpt-5", vec![1_500_000_000, 1_400_000_000, 100]),
            ),
        ]),
        ..CostUsageCache::default()
    };

    rebuild_cache_days(&mut cache);

    assert_eq!(
        cache.days[&day]["gpt-5"],
        vec![3_000_000_000, 2_800_000_000, 200]
    );
}

#[test]
fn retained_report_sums_multiple_days_above_i32_max() {
    let day_a = "2026-09-08";
    let day_b = "2026-09-09";
    let mut cache = CostUsageCache {
        files: HashMap::from([
            (
                "a".to_string(),
                cached_usage_with_packed(
                    day_a,
                    "gpt-5.6-sol",
                    vec![1_500_000_000, 1_400_000_000, 1_000_000],
                ),
            ),
            (
                "b".to_string(),
                cached_usage_with_packed(
                    day_b,
                    "gpt-5.6-sol",
                    vec![1_500_000_000, 1_400_000_000, 1_000_000],
                ),
            ),
        ]),
        ..CostUsageCache::default()
    };
    rebuild_cache_days(&mut cache);

    let report = JsonlScanner::cached_cost_report_from_days(&cache);
    assert_eq!(report.input_tokens, 3_000_000_000);
    assert_eq!(report.cached_tokens, 2_800_000_000);
    assert_eq!(report.output_tokens, 2_000_000);

    let start = chrono::NaiveDate::from_ymd_opt(2026, 9, 8).unwrap();
    let end = chrono::NaiveDate::from_ymd_opt(2026, 9, 9).unwrap();
    let summary = summary_from_cached_report(&report, start, end);
    assert_eq!(summary.input_tokens, 3_000_000_000);
    assert_eq!(summary.cached_tokens, 2_800_000_000);
    assert_eq!(summary.output_tokens, 2_000_000);
    assert_eq!(summary.sessions_count, 2);
}

#[test]
fn reasoning_survives_scan_rebuild_and_cache_reload() {
    let (_root, sessions, cache_root) = codex_scan_dirs();
    // The event timestamp (now − 1h) decides the parsed day key, so derive the
    // fixture day from that same instant: at local 00:00–01:00 now − 1h falls
    // on the previous local day and the row would land there, not on today.
    let event_time = Utc::now() - Duration::hours(1);
    let today = event_time.with_timezone(&Local).date_naive();
    let day = today.format("%Y-%m-%d").to_string();
    let day_dir = partition_dir(&sessions, today);
    std::fs::create_dir_all(&day_dir).unwrap();
    let timestamp = event_time.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
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

    let scanner = app_scanner(7, &cache_root, &sessions);
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
    let legacy_day_dir = partition_dir(&legacy_sessions, today);
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

    let legacy_scanner = app_scanner(7, &legacy_cache_root, &legacy_sessions);
    let (legacy_summary, _, legacy_cache) = legacy_scanner.scan_codex_detailed_with_cache(None);
    assert_eq!(legacy_summary.output_tokens, 20);
    assert_eq!(legacy_summary.reasoning_tokens, None);
    assert_eq!(legacy_cache.days[&day]["gpt-5"], vec![100, 0, 20]);
    assert!(
        (summary.total_cost_usd - legacy_summary.total_cost_usd).abs() < 1e-12,
        "reasoning metadata must not change cost"
    );
}
