//! Bounded Codex scans: known-zero history, byte and file limits, paused reports.

use super::*;

// ── Upstream 0.50.1 #2932: known-zero history ────────────────────────────

#[test]
fn known_zero_is_set_when_scan_completes_with_no_sessions() {
    let (_root, sessions, cache_root) = codex_scan_dirs();
    std::fs::create_dir_all(&sessions).unwrap();

    let scanner = app_scanner(7, &cache_root, &sessions);

    let (summary, _) = scanner.scan_codex_detailed(None);
    assert!(summary.history_coverage_established, "scan completed");
    assert_eq!(summary.sessions_count, 0, "no sessions");
    assert!(summary.known_zero, "completed scan with zero = known-zero");
}

#[test]
fn known_zero_is_not_set_when_scan_has_results() {
    let (_root, sessions, cache_root) = codex_scan_dirs();
    write_codex_session_fixture(&sessions, "a.jsonl", 100);

    let scanner = app_scanner(7, &cache_root, &sessions);

    let (summary, _) = scanner.scan_codex_detailed(None);
    assert!(summary.history_coverage_established);
    assert_eq!(summary.sessions_count, 1);
    assert!(!summary.known_zero, "scan with results is not known-zero");
}

#[test]
fn tiny_candidate_limit_prefers_newest_dirty_file_and_persists_older_pending() {
    let (_root, sessions, cache_root) = codex_scan_dirs();
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

    let full = app_scanner(7, root.path().join("full-cache"), &sessions);
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
    let (_root, sessions, cache_root) = codex_scan_dirs();
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

    let scanner = app_scanner(7, &cache_root, &sessions);
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
    let (root, sessions, cache_root) = codex_scan_dirs();
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

    let explicit = app_scanner(7, &cache_root, &sessions);
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

    let initial = app_scanner(7, &cache_root, &sessions);
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
    let (_root, sessions, cache_root) = codex_scan_dirs();
    let path = write_codex_session_fixture(&sessions, "pruned.jsonl", 100);

    let initial = app_scanner(7, &cache_root, &sessions);
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

    let explicit = app_scanner(7, &cache_root, &sessions);
    let (resumed_summary, _, resumed_cache) = explicit.scan_codex_detailed_with_cache(None);
    assert_eq!(resumed_summary.input_tokens, 1);
    assert!(resumed_cache.codex_scan_pause_reason.is_none());
    assert!(resumed_cache.codex_pending_paths.is_empty());
    assert!(resumed_cache.previous_report.is_none());
}

/// #755: a background scan paused with `no_progress` and no retained report
/// used to sum every cached day, so Today and 30d showed the same totals.
#[test]
fn paused_scan_without_previous_report_reports_only_the_requested_days() {
    let root = tempfile::tempdir().unwrap();
    let sessions = root.path().join("sessions");
    std::fs::create_dir_all(&sessions).unwrap();
    let cache_root = root.path().join("cache");
    let today = Local::now().date_naive();
    let older = (today - chrono::Duration::days(10))
        .format("%Y-%m-%d")
        .to_string();
    let today = today.format("%Y-%m-%d").to_string();
    let mut cache = CostUsageCache {
        files: HashMap::from([
            (
                "today".to_string(),
                cached_usage_with_packed(&today, "gpt-5.6-sol", vec![1_000, 400, 10]),
            ),
            (
                "older".to_string(),
                cached_usage_with_packed(&older, "gpt-5.6-sol", vec![5_000, 2_000, 50]),
            ),
        ]),
        // A queued file whose fork parent never resolved keeps the current
        // window unestablished, as in the reported cache.
        codex_pending_paths: vec![root.path().join("gone.jsonl").to_string_lossy().to_string()],
        codex_scan_incomplete: true,
        codex_scan_pause_reason: Some(CodexScanPauseReason::NoProgress),
        ..Default::default()
    };
    rebuild_cache_days(&mut cache);
    JsonlScanner::save_cache(ProviderId::Codex, &mut cache, Some(&cache_root));

    let scan = |days| {
        CostScanner::new(days)
            .with_cache_root(&cache_root)
            .with_sessions_dirs(vec![sessions.clone()])
            .scan_codex_detailed_with_cache(None)
    };
    let (today_summary, today_stats, _) = scan(1);
    let (month_summary, month_stats, _) = scan(30);

    assert_eq!(today_stats.files_parsed, 0);
    assert_eq!(month_stats.files_parsed, 0);
    assert_eq!(today_summary.input_tokens, 1_000);
    assert_eq!(today_summary.output_tokens, 10);
    assert_eq!(month_summary.input_tokens, 6_000);
    assert_eq!(month_summary.output_tokens, 60);
    assert!(today_summary.total_cost_usd < month_summary.total_cost_usd);
}
