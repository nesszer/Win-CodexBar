//! Pending Codex scan starts, trace validation and empty fragments.

use super::*;

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
    let (_root, sessions, cache_root) = codex_scan_dirs();
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
    let (_root, sessions, cache_root) = codex_scan_dirs();
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
    let (_root, sessions, cache_root) = codex_scan_dirs();
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
    let (_root, sessions, cache_root) = codex_scan_dirs();
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
    let (_root, sessions, cache_root) = codex_scan_dirs();
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
    let (_root, sessions, cache_root) = codex_scan_dirs();
    let path = write_codex_session_fixture(&sessions, "disappeared.jsonl", 100);

    let initial = app_scanner(7, &cache_root, &sessions);
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

    let explicit = app_scanner(7, &cache_root, &sessions);
    let (resumed_summary, _, resumed_cache) = explicit.scan_codex_detailed_with_cache(None);
    assert_eq!(resumed_summary.sessions_count, 0);
    assert!(resumed_summary.history_coverage_established);
    assert!(resumed_cache.codex_scan_pause_reason.is_none());
    assert!(resumed_cache.codex_pending_paths.is_empty());
    assert!(resumed_cache.previous_report.is_none());
}

#[test]
fn paused_catch_up_round_trips_without_retrying_in_background() {
    let (_root, sessions, cache_root) = codex_scan_dirs();
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
    let (_root, sessions, cache_root) = codex_scan_dirs();
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
    let (_root, sessions, cache_root) = codex_scan_dirs();
    let path = write_codex_session_fixture(&sessions, "empty.jsonl", 100);
    std::fs::write(&path, b"\n").unwrap();
    let key = path.to_string_lossy().to_string();

    let scanner = app_scanner(7, &cache_root, &sessions);
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
    let (_root, sessions, cache_root) = codex_scan_dirs();
    let path = write_codex_session_fixture(&sessions, "empty.jsonl", 100);
    std::fs::write(&path, b"\n").unwrap();
    let key = path.to_string_lossy().to_string();

    let scanner = app_scanner(7, &cache_root, &sessions);
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
    let (_root, sessions, cache_root) = codex_scan_dirs();
    let path = write_codex_session_fixture(&sessions, "incomplete.jsonl", 100);
    std::fs::write(&path, br#"{"timestamp":"2026-09-07T00:00:00Z""#).unwrap();
    let key = path.to_string_lossy().to_string();

    let scanner = app_scanner(7, &cache_root, &sessions);
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
