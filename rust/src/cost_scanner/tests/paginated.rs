use super::*;
use chrono::{DateTime, Duration, Local, Utc};
use std::io::Write;
use std::path::{Path, PathBuf};

fn write_codex_paginated_subagent_fixture(
    sessions_root: &Path,
    name: &str,
    session_id: &str,
    base: DateTime<Utc>,
) -> PathBuf {
    let day = base.with_timezone(&Local).date_naive();
    let day_dir = sessions_root
        .join(day.format("%Y").to_string())
        .join(day.format("%m").to_string())
        .join(day.format("%d").to_string());
    std::fs::create_dir_all(&day_dir).unwrap();
    let path = day_dir.join(name);
    // Desktop v2 aliases session_id to the parent, while id identifies the
    // child. Its paginated token counters start at the child's own usage.
    let metadata = serde_json::json!({
        "type": "session_meta",
        "timestamp": base.to_rfc3339(),
        "payload": {
            "id": session_id,
            "session_id": "parent-id",
            "forked_from_id": "parent-id",
            "parent_thread_id": "parent-id",
            "source": {"subagent": {"thread_spawn": {
                "parent_thread_id": "parent-id", "depth": 1
            }}},
            "thread_source": "subagent",
            "history_mode": "paginated",
            "subagent_history_start_ordinal": 42,
            "multi_agent_version": "v2"
        }
    });
    let mut body = format!("{metadata}\n");
    let mut previous = [0, 0, 0];
    for (index, totals) in [[41_505, 19_328, 132], [42_005, 19_528, 142]]
        .into_iter()
        .enumerate()
    {
        let row = serde_json::json!({
            "type": "event_msg",
            "ordinal": 53 + index,
            "timestamp": (base + Duration::seconds(
                i64::try_from(index).expect("fixture index fits i64") + 1
            )).to_rfc3339(),
            "payload": {
                "type": "token_count",
                "info": {
                    "model": "gpt-5",
                    "total_token_usage": {
                        "input_tokens": totals[0],
                        "cached_input_tokens": totals[1],
                        "output_tokens": totals[2]
                    },
                    "last_token_usage": {
                        "input_tokens": totals[0] - previous[0],
                        "cached_input_tokens": totals[1] - previous[1],
                        "output_tokens": totals[2] - previous[2]
                    }
                }
            }
        });
        body.push_str(&format!("{row}\n"));
        previous = totals;
    }
    std::fs::write(&path, body).unwrap();
    path
}

#[test]
fn paginated_v2_subagent_counts_own_usage_without_parent() {
    let root = tempfile::tempdir().unwrap();
    let sessions = root.path().join("sessions");
    let cache_root = root.path().join("cache");
    let child = write_codex_paginated_subagent_fixture(
        &sessions,
        "child.jsonl",
        "child-id",
        Utc::now() - Duration::hours(1),
    );
    let scanner = CostScanner::new(7)
        .with_options(CostScanOptions::app_driven())
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![sessions]);
    let (summary, _, cache) = scanner.scan_codex_detailed_with_cache(None);

    assert_eq!(summary.input_tokens, 42_005);
    assert_eq!(summary.cached_tokens, 19_528);
    assert_eq!(summary.output_tokens, 142);
    assert_eq!(summary.sessions_count, 1);
    assert!(summary.history_coverage_established);
    assert!(!cache.codex_scan_incomplete);
    assert!(cache.codex_pending_paths.is_empty());
    let usage = &cache.files[&child.to_string_lossy().to_string()];
    assert_eq!(usage.codex_session_id.as_deref(), Some("child-id"));
    assert!(!usage.codex_unresolved_fork_parent);
    assert_eq!(cached_input_total(usage), 42_005);

    let (cached_summary, stats, _) = scanner.scan_codex_detailed_with_cache(None);
    assert_eq!(cached_summary.input_tokens, summary.input_tokens);
    assert!(cached_summary.history_coverage_established);
    assert!(stats.codex_history_read_paths.is_empty());
}

#[test]
fn paginated_v2_subagent_does_not_subtract_or_double_count_continued_parent() {
    let root = tempfile::tempdir().unwrap();
    let sessions = root.path().join("sessions");
    let cache_root = root.path().join("cache");
    let base = Utc::now() - Duration::hours(1);
    let parent = write_codex_fork_session_fixture(
        &sessions,
        "parent.jsonl",
        "parent-id",
        None,
        base,
        base + Duration::seconds(10),
        &[1_000_000],
    );
    let child = write_codex_paginated_subagent_fixture(
        &sessions,
        "child.jsonl",
        "child-id",
        base + Duration::seconds(1),
    );
    let scanner = CostScanner::new(7)
        .with_options(CostScanOptions::app_driven())
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![sessions]);
    let (summary, _, cache) = scanner.scan_codex_detailed_with_cache(None);

    assert_eq!(summary.input_tokens, 1_042_005);
    assert_eq!(summary.cached_tokens, 19_528);
    assert_eq!(summary.output_tokens, 147);
    assert_eq!(summary.sessions_count, 2);
    assert!(summary.history_coverage_established);
    assert!(cache.codex_pending_paths.is_empty());
    assert_eq!(
        cached_input_total(&cache.files[&child.to_string_lossy().to_string()]),
        42_005
    );

    let continued = serde_json::json!({
        "type": "event_msg",
        "timestamp": (base + Duration::seconds(20)).to_rfc3339(),
        "payload": {"type": "token_count", "info": {
            "model": "gpt-5",
            "total_token_usage": {
                "input_tokens": 1_001_000, "cached_input_tokens": 0, "output_tokens": 10
            }
        }}
    });
    writeln!(
        File::options().append(true).open(&parent).unwrap(),
        "{continued}"
    )
    .unwrap();
    let (grown, _, grown_cache) = scanner.scan_codex_detailed_with_cache(None);
    assert_eq!(grown.input_tokens, 1_043_005);
    assert_eq!(grown.cached_tokens, 19_528);
    assert_eq!(grown.output_tokens, 152);
    assert_eq!(grown.sessions_count, 2);
    assert!(grown.history_coverage_established);
    assert!(!grown_cache.codex_scan_incomplete);
}

#[test]
fn paginated_v2_subagent_recovers_unchanged_previously_unresolved_cache() {
    let root = tempfile::tempdir().unwrap();
    let sessions = root.path().join("sessions");
    let cache_root = root.path().join("cache");
    let base = Utc::now() - Duration::hours(1);
    let child = write_codex_paginated_subagent_fixture(&sessions, "child.jsonl", "child-id", base);
    let scanner = CostScanner::new(7)
        .with_options(CostScanOptions::app_driven())
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![sessions]);
    let (_, _, mut old_cache) = scanner.scan_codex_detailed_with_cache(None);
    let child_key = child.to_string_lossy().to_string();
    let old_usage = old_cache.files.get_mut(&child_key).unwrap();
    // Recreate the previous parser's cache entry without touching the JSONL's
    // bytes, size, mtime or file identity. Fresh metadata must clear this parent.
    old_usage.days.clear();
    old_usage.parsed_bytes = Some(0);
    old_usage.codex_scan_target_size = None;
    old_usage.last_model = None;
    old_usage.last_totals = None;
    old_usage.codex_token_timestamps_monotonic = None;
    old_usage.codex_last_token_timestamp = None;
    old_usage.codex_forked_from_id = Some("parent-id".to_string());
    old_usage.codex_fork_timestamp = Some(base.to_rfc3339());
    old_usage.codex_unresolved_fork_parent = true;
    old_cache.days.clear();
    old_cache.codex_pending_paths = vec![child_key.clone()];
    old_cache.codex_scan_incomplete = true;
    old_cache.codex_scan_pause_reason = Some(CodexScanPauseReason::NoProgress);
    old_cache.previous_report = Some(CachedCostReport {
        total_cost_usd: 0.1,
        input_tokens: 11,
        cached_tokens: 0,
        output_tokens: 3,
        reasoning_tokens: None,
        sessions_count: 1,
        updated_at: Some((base - Duration::days(1)).to_rfc3339()),
        partial: false,
    });
    JsonlScanner::save_cache(ProviderId::Codex, &mut old_cache, Some(&cache_root));

    let (summary, stats, recovered) = scanner.scan_codex_detailed_with_cache(None);
    assert_eq!(summary.input_tokens, 42_005);
    assert_eq!(summary.cached_tokens, 19_528);
    assert_eq!(summary.output_tokens, 142);
    assert!(summary.history_coverage_established);
    assert!(stats.codex_metadata_read_paths.contains(&child_key));
    assert!(stats.codex_history_read_paths.contains(&child_key));
    assert!(!recovered.files[&child_key].codex_unresolved_fork_parent);
    assert!(recovered.files[&child_key].codex_forked_from_id.is_none());
    assert!(!recovered.codex_scan_incomplete);
    assert!(recovered.codex_pending_paths.is_empty());
    assert!(recovered.codex_scan_pause_reason.is_none());
    assert!(recovered.previous_report.is_none());
}

#[test]
fn paginated_v2_subagent_repairs_stale_complete_cache_metadata() {
    let root = tempfile::tempdir().unwrap();
    let sessions = root.path().join("sessions");
    let cache_root = root.path().join("cache");
    let base = Utc::now() - Duration::hours(1);
    let child = write_codex_paginated_subagent_fixture(&sessions, "child.jsonl", "child-id", base);
    let scanner = CostScanner::new(7)
        .with_options(CostScanOptions::app_driven())
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![sessions]);
    let (_, _, mut cache) = scanner.scan_codex_detailed_with_cache(None);
    let child_key = child.to_string_lossy().to_string();
    let usage = cache.files.get_mut(&child_key).unwrap();
    usage.codex_forked_from_id = Some("parent-id".to_string());
    usage.codex_lineage = CodexSessionLineage::Root;
    usage.codex_unresolved_fork_parent = false;
    JsonlScanner::save_cache(ProviderId::Codex, &mut cache, Some(&cache_root));

    let (_, stats, recovered) = scanner.scan_codex_detailed_with_cache(None);
    let usage = &recovered.files[&child_key];
    assert!(stats.codex_history_read_paths.contains(&child_key));
    assert_eq!(usage.codex_forked_from_id, None);
    assert_eq!(usage.codex_lineage, CodexSessionLineage::Independent);
    assert!(!usage.codex_unresolved_fork_parent);
}

#[test]
fn paginated_v2_subagent_background_refresh_adds_new_local_day_without_repair() {
    let root = tempfile::tempdir().unwrap();
    let sessions = root.path().join("sessions");
    let cache_root = root.path().join("cache");
    let today = Local::now().date_naive();
    let yesterday = today.pred_opt().unwrap();
    let local_noon = |day: chrono::NaiveDate| {
        day.and_hms_opt(12, 0, 0)
            .unwrap()
            .and_local_timezone(Local)
            .single()
            .expect("fixture local noon exists")
            .with_timezone(&Utc)
    };
    let prior_child = write_codex_paginated_subagent_fixture(
        &sessions,
        "yesterday-child.jsonl",
        "yesterday-child-id",
        local_noon(yesterday),
    );
    let background = CostScanner::new(7)
        .with_options(CostScanOptions::default())
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![sessions.clone()]);
    let (initial, _, mut established) = background.scan_codex_detailed_with_cache(None);
    let yesterday_key = yesterday.format("%Y-%m-%d").to_string();
    let today_key = today.format("%Y-%m-%d").to_string();
    assert!(initial.history_coverage_established);
    assert_eq!(initial.input_tokens, 42_005);
    assert_eq!(
        established.days[&yesterday_key]["gpt-5"],
        vec![42_005, 19_528, 142]
    );
    assert!(!established.days.contains_key(&today_key));

    let new_child = write_codex_paginated_subagent_fixture(
        &sessions,
        "today-child.jsonl",
        "today-child-id",
        local_noon(today),
    );
    // Simulate the next scheduled interval without waiting out the debounce.
    established.last_scan_unix_ms = 0;
    JsonlScanner::save_cache(ProviderId::Codex, &mut established, Some(&cache_root));

    let (refreshed, stats, cache) = background.scan_codex_detailed_with_cache(None);
    assert!(!stats.used_cache_debounce);
    assert_eq!(refreshed.input_tokens, 84_010);
    assert_eq!(refreshed.cached_tokens, 39_056);
    assert_eq!(refreshed.output_tokens, 284);
    assert_eq!(refreshed.sessions_count, 2);
    assert!(refreshed.history_coverage_established);
    assert_eq!(
        cache.days[&yesterday_key]["gpt-5"],
        vec![42_005, 19_528, 142]
    );
    assert_eq!(cache.days[&today_key]["gpt-5"], vec![42_005, 19_528, 142]);
    for (path, session_id) in [
        (prior_child, "yesterday-child-id"),
        (new_child, "today-child-id"),
    ] {
        let usage = &cache.files[&path.to_string_lossy().to_string()];
        assert_eq!(usage.codex_session_id.as_deref(), Some(session_id));
        assert!(!usage.codex_unresolved_fork_parent);
    }
    assert!(!cache.codex_scan_incomplete);
    assert!(cache.codex_pending_paths.is_empty());
    assert!(cache.codex_scan_pause_reason.is_none());
    assert!(cache.previous_report.is_none());
}

#[test]
fn unresolved_legacy_fork_keeps_background_daily_usage_live() {
    let root = tempfile::tempdir().unwrap();
    let sessions = root.path().join("sessions");
    let cache_root = root.path().join("cache");
    let today = Local::now().date_naive();
    let yesterday = today.pred_opt().unwrap();
    let local_noon = |day: chrono::NaiveDate| {
        day.and_hms_opt(12, 0, 0)
            .unwrap()
            .and_local_timezone(Local)
            .single()
            .expect("fixture local noon exists")
            .with_timezone(&Utc)
    };
    let yesterday_noon = local_noon(yesterday);
    let today_noon = local_noon(today);
    let healthy_yesterday = write_codex_fork_session_fixture(
        &sessions,
        "healthy-yesterday.jsonl",
        "healthy-yesterday-id",
        None,
        yesterday_noon,
        yesterday_noon,
        &[100],
    );
    let background = CostScanner::new(7)
        .with_options(CostScanOptions::default())
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![sessions.clone()]);
    let (initial, _, mut established) = background.scan_codex_detailed_with_cache(None);
    assert!(initial.history_coverage_established);
    assert_eq!(initial.input_tokens, 100);

    let unresolved = write_codex_fork_session_fixture(
        &sessions,
        "legacy-fork.jsonl",
        "legacy-child-id",
        Some("parent-outside-history-window"),
        today_noon,
        today_noon,
        &[1_000_000, 1_000_140],
    );
    let healthy_today = write_codex_fork_session_fixture(
        &sessions,
        "healthy-today.jsonl",
        "healthy-today-id",
        None,
        today_noon,
        today_noon,
        &[200],
    );
    established.last_scan_unix_ms = 0;
    JsonlScanner::save_cache(ProviderId::Codex, &mut established, Some(&cache_root));

    let (live, _, cache) = background.scan_codex_detailed_with_cache(None);
    let yesterday_key = yesterday.format("%Y-%m-%d").to_string();
    let today_key = today.format("%Y-%m-%d").to_string();
    let unresolved_key = unresolved.to_string_lossy().to_string();
    assert_eq!(live.input_tokens, 300);
    assert_eq!(live.output_tokens, 10);
    assert_eq!(live.sessions_count, 2);
    assert!(!live.history_coverage_established);
    assert!(!live.known_zero);
    assert_eq!(cache.days[&yesterday_key]["gpt-5"], vec![100, 0, 5]);
    assert_eq!(cache.days[&today_key]["gpt-5"], vec![200, 0, 5]);
    assert_eq!(cache.codex_pending_paths, vec![unresolved_key.clone()]);
    assert!(cache.files[&unresolved_key].codex_unresolved_fork_parent);
    assert!(cache.files[&unresolved_key].days.is_empty());
    assert!(cache.codex_scan_incomplete);
    assert!(cache.codex_scan_pause_reason.is_none());
    assert!(cache.previous_report.is_none());

    // A quiet interval must not turn the unresolved fork into a global pause.
    let (quiet, _, quiet_cache) = background.scan_codex_detailed_with_cache(None);
    assert_eq!(quiet.input_tokens, 300);
    assert!(quiet_cache.codex_scan_pause_reason.is_none());
    assert!(quiet_cache.previous_report.is_none());

    let appended = serde_json::json!({
        "type": "event_msg",
        "timestamp": (today_noon + Duration::minutes(1)).to_rfc3339(),
        "payload": {"type": "token_count", "info": {
            "model": "gpt-5",
            "total_token_usage": {
                "input_tokens": 350, "cached_input_tokens": 0, "output_tokens": 10
            }
        }}
    });
    writeln!(
        File::options().append(true).open(&healthy_today).unwrap(),
        "{appended}"
    )
    .unwrap();
    let (grown, _, grown_cache) = background.scan_codex_detailed_with_cache(None);
    assert_eq!(grown.input_tokens, 450);
    assert_eq!(grown.output_tokens, 15);
    assert_eq!(grown.sessions_count, 2);
    assert!(!grown.history_coverage_established);
    assert!(!grown.known_zero);
    assert_eq!(grown_cache.days[&yesterday_key]["gpt-5"], vec![100, 0, 5]);
    assert_eq!(grown_cache.days[&today_key]["gpt-5"], vec![350, 0, 10]);
    assert_eq!(
        grown_cache.codex_pending_paths,
        vec![unresolved_key.clone()]
    );
    assert!(grown_cache.files[&unresolved_key].days.is_empty());
    assert!(grown_cache.codex_scan_pause_reason.is_none());
    assert!(grown_cache.previous_report.is_none());

    let (stable, stable_stats, stable_cache) = background.scan_codex_detailed_with_cache(None);
    assert_eq!(stable.input_tokens, 450);
    for healthy_path in [&healthy_yesterday, &healthy_today] {
        assert!(
            !stable_stats
                .codex_history_read_paths
                .contains(&healthy_path.to_string_lossy().to_string()),
            "unchanged healthy history must remain cached while a legacy fork is pending"
        );
    }
    assert!(stable_cache.codex_scan_pause_reason.is_none());
    assert!(stable_cache.previous_report.is_none());
}

#[test]
fn unresolved_legacy_fork_old_no_progress_pause_retains_report_in_background() {
    let root = tempfile::tempdir().unwrap();
    let sessions = root.path().join("sessions");
    let cache_root = root.path().join("cache");
    let base = Utc::now() - Duration::hours(1);
    let unresolved = write_codex_fork_session_fixture(
        &sessions,
        "legacy-fork.jsonl",
        "legacy-child-id",
        Some("missing-parent-id"),
        base,
        base,
        &[1_000_000, 1_000_140],
    );
    write_codex_fork_session_fixture(
        &sessions,
        "healthy.jsonl",
        "healthy-id",
        None,
        base,
        base,
        &[100],
    );
    let initial = CostScanner::new(7)
        .with_options(CostScanOptions::app_driven())
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![sessions.clone()]);
    let (_, _, mut paused) = initial.scan_codex_detailed_with_cache(None);
    let unresolved_key = unresolved.to_string_lossy().to_string();
    let unchanged_fork = paused.files[&unresolved_key].clone();
    assert!(unchanged_fork.codex_unresolved_fork_parent);
    assert_eq!(paused.codex_pending_paths, vec![unresolved_key.clone()]);
    paused.codex_scan_pause_reason = Some(CodexScanPauseReason::NoProgress);
    paused.previous_report = Some(CachedCostReport {
        total_cost_usd: 0.1,
        input_tokens: 11,
        cached_tokens: 0,
        output_tokens: 3,
        reasoning_tokens: None,
        sessions_count: 1,
        updated_at: Some((base - Duration::days(1)).to_rfc3339()),
        partial: false,
    });
    JsonlScanner::save_cache(ProviderId::Codex, &mut paused, Some(&cache_root));
    write_codex_fork_session_fixture(
        &sessions,
        "new-healthy.jsonl",
        "new-healthy-id",
        None,
        base + Duration::minutes(1),
        base + Duration::minutes(1),
        &[200],
    );

    let background = CostScanner::new(7)
        .with_options(CostScanOptions::default())
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![sessions]);
    let (summary, stats, cache) = background.scan_codex_detailed_with_cache(None);
    // v0.60.5 keeps the last validated report while unresolved current work
    // remains queued; newly discovered rows are not authoritative yet.
    assert_eq!(summary.input_tokens, 11);
    assert_eq!(summary.output_tokens, 3);
    assert_eq!(summary.sessions_count, 1);
    assert!(!summary.history_coverage_established);
    assert!(!summary.known_zero);
    assert!(stats.files_seen > 0);
    assert!(cache.codex_scan_incomplete);
    assert_eq!(cache.codex_pending_paths, vec![unresolved_key.clone()]);
    let still_unresolved = &cache.files[&unresolved_key];
    assert!(still_unresolved.codex_unresolved_fork_parent);
    assert!(still_unresolved.days.is_empty());
    assert_eq!(still_unresolved.size, unchanged_fork.size);
    assert_eq!(still_unresolved.mtime_unix_ms, unchanged_fork.mtime_unix_ms);
    assert!(cache.codex_scan_pause_reason.is_none());
    assert_eq!(
        cache
            .previous_report
            .as_ref()
            .map(|report| report.input_tokens),
        Some(11)
    );
}
