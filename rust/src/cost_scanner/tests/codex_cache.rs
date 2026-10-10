//! Codex scan cache reuse, resume, cancel and reconciliation.

use super::*;

#[test]
fn cost_scan_second_pass_skips_unchanged_files_via_cache() {
    let (_root, sessions, cache_root) = codex_scan_dirs();
    write_codex_session_fixture(&sessions, "a.jsonl", 100);
    write_codex_session_fixture(&sessions, "b.jsonl", 200);

    let scanner = app_scanner(7, &cache_root, &sessions);

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
    CodexLineagePlanner::reset_graph_build_count();
    let (summary2, stats2) = scanner.scan_codex_detailed(None);
    assert_eq!(stats2.files_seen, 2);
    assert_eq!(stats2.files_skipped, 2, "cache hit skips re-parse");
    assert_eq!(stats2.files_parsed, 0);
    assert_eq!(
        CodexLineagePlanner::graph_build_count(),
        0,
        "warm root-only scan must bypass lineage graph construction"
    );
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
    let forced = app_scanner(7, &cache_root, &sessions);
    let (_, stats4) = forced.scan_codex_detailed(None);
    assert!(!stats4.used_cache_debounce);
    assert_eq!(stats4.files_skipped, 2);
    assert_eq!(stats4.files_parsed, 0);
    assert!(stats4.codex_history_read_paths.is_empty());
}

#[test]
fn codex_lazy_history_receipt_reads_only_changed_file_and_matches_fresh_parse() {
    let (root, sessions, cache_root) = codex_scan_dirs();
    let first_path = write_codex_session_fixture_with_inputs(&sessions, "first.jsonl", &[100]);
    let second_path = write_codex_session_fixture_with_inputs(&sessions, "second.jsonl", &[200]);
    let scanner = app_scanner(7, &cache_root, &sessions);

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

    let fresh = app_scanner(7, root.path().join("fresh-cache"), &sessions);
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
fn codex_source_recovery_keeps_appended_duplicate_unpriced_after_cache_reload() {
    let (_root, sessions, cache_root) = codex_scan_dirs();
    let path = write_codex_session_fixture_with_inputs(&sessions, "recovery.jsonl", &[100]);
    let scanner = app_scanner(7, &cache_root, &sessions);

    let (_, _, mut first_cache) = scanner.scan_codex_detailed_with_cache(None);
    let path_key = path.to_string_lossy().to_string();
    let source_rows = first_cache
        .codex_source_rows
        .get_mut(&path_key)
        .expect("source rows persisted");
    assert_eq!(source_rows.rows.len(), 1);
    // Cached evidence that disagrees with the source: a Priority row on a
    // model with a Fast lane, so the recovered row keeps a `-priority` key
    // (a Priority row without a Fast lane would price at its Standard base).
    source_rows.rows[0].pricing.pricing_model = Some("gpt-5.5".to_string());
    source_rows.rows[0].pricing.pricing_mode = Some("priority".to_string());
    first_cache.last_scan_unix_ms = 1;
    JsonlScanner::save_cache(ProviderId::Codex, &mut first_cache, Some(&cache_root));

    // Half an hour after the fixture's row, so both rows share one local day.
    let appended_time = recent_codex_fixture_time() + Duration::minutes(30);
    let timestamp = appended_time.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
    let appended = format!(
        r#"{{"timestamp":"{timestamp}","type":"event_msg","payload":{{"type":"token_count","info":{{"model":"gpt-5","total_token_usage":{{"input_tokens":200,"cached_input_tokens":0,"output_tokens":10}}}}}}}}"#
    ) + "\n";
    std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(appended.as_bytes())
        .unwrap();

    let (_, _, second_cache) = scanner.scan_codex_detailed_with_cache(None);
    let usage = second_cache.files.get(&path_key).expect("file cache");
    let day = appended_time
        .with_timezone(&Local)
        .format("%Y-%m-%d")
        .to_string();
    assert_eq!(
        usage.days[&day]["gpt-5.5-priority"],
        vec![100, 0, 5],
        "gpt-5.5 turns also price at the Priority rate"
    );
    assert_eq!(
        usage.days[&day][CostUsagePricing::CODEX_UNATTRIBUTED_MODEL],
        vec![100, 0, 5]
    );
}

#[test]
fn codex_file_identity_invalidates_same_path_cache_without_eager_history_read() {
    let (_root, sessions, cache_root) = codex_scan_dirs();
    let path = write_codex_session_fixture(&sessions, "replacement.jsonl", 100);
    let scanner = app_scanner(7, &cache_root, &sessions);
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
                parsed_bytes: Some(100),
                last_model: Some("gpt-5.6-sol".to_string()),
                codex_token_timestamps_monotonic: Some(true),
                ..test_file_usage(100, usage.clone())
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
    let (_root, sessions, cache_root) = codex_scan_dirs();
    write_codex_session_fixture(&sessions, "a.jsonl", 100);
    write_codex_session_fixture(&sessions, "b.jsonl", 200);
    write_codex_session_fixture(&sessions, "c.jsonl", 300);

    let cancel = AtomicBool::new(true);
    let scanner = app_scanner(7, cache_root, &sessions);
    let (summary, stats) = scanner.scan_codex_detailed(Some(&cancel));
    assert_eq!(stats.files_seen, 0, "cancel before first file stops walk");
    assert_eq!(summary.sessions_count, 0);
}

#[test]
fn cost_scan_reconciles_deleted_file_to_known_zero() {
    let (_root, sessions, cache_root) = codex_scan_dirs();
    let path = write_codex_session_fixture(&sessions, "deleted.jsonl", 100);

    let scanner = app_scanner(7, &cache_root, &sessions);
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
    let (_root, sessions, cache_root) = codex_scan_dirs();
    let deleted = write_codex_session_fixture(&sessions, "deleted.jsonl", 100);
    let sibling = write_codex_session_fixture(&sessions, "sibling.jsonl", 200);

    let scanner = app_scanner(7, &cache_root, &sessions);
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
    let (_root, sessions, cache_root) = codex_scan_dirs();
    let path = write_codex_session_fixture(&sessions, "deleted.jsonl", 100);

    let scanner = app_scanner(7, &cache_root, &sessions);
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
    let (root, sessions, cache_root) = codex_scan_dirs();
    let path = write_codex_session_fixture(&sessions, "grow.jsonl", 50);

    let scanner = app_scanner(7, &cache_root, &sessions);
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
    let full_scanner = app_scanner(7, root.path().join("fresh-cache"), &sessions);
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
fn codex_partial_rescan_replaces_changed_session_after_cache_reopen() {
    // Windows parity for upstream 0.60.2 cost persistence: a rewritten
    // session must replace the cached file aggregate even when the first
    // refresh only consumes a bounded prefix and the next refresh reloads the
    // cache from disk.
    let (root, sessions, cache_root) = codex_scan_dirs();
    let path = write_codex_session_fixture_with_inputs(&sessions, "changed.jsonl", &[100, 200]);
    let old_metadata = std::fs::metadata(&path).unwrap();
    let old_size = old_metadata.len();
    let old_mtime = old_metadata.modified().unwrap();

    let initial_scanner = app_scanner(7, &cache_root, &sessions);
    let (initial, initial_stats) = initial_scanner.scan_codex_detailed(None);
    assert_eq!(initial_stats.files_parsed, 1);
    assert_eq!(initial.input_tokens, 200);
    assert!(initial.history_coverage_established);

    // Keep the path, identity, and byte length stable while changing both
    // token snapshots. The mtime change proves that the cached aggregate is
    // invalidated before the bounded rescan begins.
    write_codex_session_fixture_with_inputs(&sessions, "changed.jsonl", &[300, 400]);
    std::fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(old_mtime + std::time::Duration::from_secs(2))
        .unwrap();
    let rewritten_metadata = std::fs::metadata(&path).unwrap();
    assert_eq!(rewritten_metadata.len(), old_size);
    assert_ne!(rewritten_metadata.modified().unwrap(), old_mtime);

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

    let bounded_scanner = CostScanner::new(7)
        .with_options(bounded_options)
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![sessions.clone()]);
    let (partial, partial_stats, partial_cache) =
        bounded_scanner.scan_codex_detailed_with_cache(None);
    assert!(partial_stats.files_parsed >= 1);
    assert!(partial_cache.codex_scan_incomplete);
    assert!(!partial.history_coverage_established);
    assert_eq!(
        partial_cache
            .previous_report
            .as_ref()
            .map(|report| report.input_tokens),
        Some(200),
        "the last validated report remains visible during catch-up"
    );
    let partial_file = partial_cache
        .files
        .get(&path.to_string_lossy().to_string())
        .expect("partially rescanned file cache entry");
    assert_eq!(partial_file.parsed_bytes, Some(first_line_bytes));

    // A new scanner instance models a process/cache reopen. The persisted
    // cursor must resume the rewritten file and finish at the new total.
    let reopened_scanner = CostScanner::new(7)
        .with_options(bounded_options)
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![sessions.clone()]);
    let (resumed, resumed_stats, resumed_cache) =
        reopened_scanner.scan_codex_detailed_with_cache(None);
    assert_eq!(resumed_stats.files_resumed, 1);
    assert_eq!(resumed_stats.files_parsed, 0);
    assert!(!resumed_cache.codex_scan_incomplete);
    assert!(resumed.history_coverage_established);
    assert_eq!(resumed.input_tokens, 400);

    // Resumption may change the amount of work, but it must publish the same
    // cost aggregate as a clean full parse of the rewritten session.
    let fresh_scanner = app_scanner(7, root.path().join("fresh-cache"), &sessions);
    let (fresh, fresh_stats) = fresh_scanner.scan_codex_detailed(None);
    assert_eq!(fresh_stats.files_parsed, 1);
    assert_eq!(resumed.input_tokens, fresh.input_tokens);
    assert_eq!(resumed.cached_tokens, fresh.cached_tokens);
    assert_eq!(resumed.output_tokens, fresh.output_tokens);
    assert_eq!(resumed.sessions_count, fresh.sessions_count);
    assert_eq!(resumed.by_model_tokens, fresh.by_model_tokens);
    assert_eq!(resumed.by_model.len(), fresh.by_model.len());
    for (model, resumed_cost) in &resumed.by_model {
        let fresh_cost = fresh.by_model.get(model).copied().expect("fresh model row");
        assert!((resumed_cost - fresh_cost).abs() < 1e-12);
    }
    assert!((resumed.total_cost_usd - fresh.total_cost_usd).abs() < 1e-12);
}

#[test]
fn bounded_growing_rollout_freezes_target_and_resumes_a_retained_tail() {
    let (_root, sessions, cache_root) = codex_scan_dirs();
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
    let first_usage = cached_file(&first_cache, &path);
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
        let usage = cached_file(&bounded_cache, &path);
        assert_eq!(usage.codex_scan_target_size, Some(initial_size));
        assert!(usage.parsed_bytes.unwrap_or_default() <= initial_size);
        if usage.parsed_bytes == Some(initial_size) {
            break;
        }
    }

    assert_eq!(bounded_summary.input_tokens, 300);
    let bounded_usage = cached_file(&bounded_cache, &path);
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
    let partial_usage = cached_file(&partial_cache, &path);
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
    let resumed_usage = cached_file(&resumed_cache, &path);
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
    let (_root, sessions, cache_root) = codex_scan_dirs();
    let _path = write_codex_session_fixture(&sessions, "a.jsonl", 100);

    let scanner = app_scanner(7, &cache_root, &sessions);
    let (s1, st1) = scanner.scan_codex_detailed(None);
    assert_eq!(st1.files_parsed, 1);
    assert_eq!(s1.input_tokens, 100);

    // Rewrite the file with a shorter body at the same path so the cached
    // parsed_bytes offset now points mid-line in the new content.
    let today = Local::now().date_naive();
    let day_dir = partition_dir(&sessions, today);
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
    let (_root, sessions, cache_root) = codex_scan_dirs();
    write_codex_session_fixture(&sessions, "a.jsonl", 100);

    let scanner = app_scanner(7, &cache_root, &sessions);

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
