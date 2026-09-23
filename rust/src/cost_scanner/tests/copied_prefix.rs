use super::*;

fn write_copied_prefix_subagent_fixture(
    sessions_root: &Path,
    name: &str,
    session_id: &str,
    parent_id: &str,
    base: DateTime<Utc>,
    owned: bool,
) -> PathBuf {
    let day = base.with_timezone(&Local).date_naive();
    let day_dir = sessions_root
        .join(day.format("%Y").to_string())
        .join(day.format("%m").to_string())
        .join(day.format("%d").to_string());
    std::fs::create_dir_all(&day_dir).unwrap();
    let path = day_dir.join(name);
    let mut lines = vec![
        serde_json::json!({
            "type": "session_meta", "ordinal": 0, "timestamp": base.to_rfc3339(),
            "payload": {
                "id": session_id, "forked_from_id": parent_id,
                "subagent_history_start_ordinal": 10,
                "thread_source": "subagent",
                "source": {"subagent": {"thread_spawn": {"parent_thread_id": parent_id}}}
            }
        }),
        token_row(base, 2, [1_000, 900, 100], [0, 0, 0], "gpt-5.6-sol"),
        serde_json::json!({
            "type": "turn_context", "ordinal": 10, "timestamp": base.to_rfc3339(),
            "payload": {"model": "gpt-5.6-sol"}
        }),
        token_row(
            base,
            12,
            [1_000, 900, 100],
            [1_000, 900, 100],
            "gpt-5.6-sol",
        ),
        token_row(
            base,
            13,
            [5_000, 3_900, 500],
            [5_000, 3_900, 500],
            "gpt-5.6-sol",
        ),
    ];
    if owned {
        lines.extend([
            token_row(base, 19, [5_050, 3_910, 505], [50, 10, 5], "gpt-5.6-sol"),
            token_row(
                base + Duration::seconds(1),
                20,
                [5_070, 3_915, 510],
                [20, 5, 5],
                "gpt-5.6-sol",
            ),
            token_row(
                base + Duration::seconds(2),
                21,
                [5_070, 3_915, 510],
                [20, 5, 5],
                "gpt-5.6-sol",
            ),
        ]);
    }
    let body = lines
        .into_iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    std::fs::write(&path, body).unwrap();
    path
}

fn token_row(
    timestamp: DateTime<Utc>,
    ordinal: i64,
    total: [i64; 3],
    last: [i64; 3],
    model: &str,
) -> serde_json::Value {
    serde_json::json!({
        "type": "event_msg", "ordinal": ordinal, "timestamp": timestamp.to_rfc3339(),
        "payload": {"type": "token_count", "info": {
            "model": model,
            "total_token_usage": {
                "input_tokens": total[0], "cached_input_tokens": total[1], "output_tokens": total[2]
            },
            "last_token_usage": {
                "input_tokens": last[0], "cached_input_tokens": last[1], "output_tokens": last[2]
            }
        }}
    })
}

#[test]
fn copied_prefix_subagent_infers_advancing_baseline_without_parent() {
    let root = tempfile::tempdir().unwrap();
    let sessions = root.path().join("sessions");
    let cache_root = root.path().join("cache");
    let child = write_copied_prefix_subagent_fixture(
        &sessions,
        "child.jsonl",
        "child-id",
        "missing-parent",
        Utc::now() - Duration::hours(1),
        true,
    );
    let scanner = CostScanner::new(7)
        .with_options(CostScanOptions::app_driven())
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![sessions]);

    let (summary, _, cache) = scanner.scan_codex_detailed_with_cache(None);
    assert_eq!(summary.input_tokens, 70);
    assert_eq!(summary.cached_tokens, 15);
    assert_eq!(summary.output_tokens, 10);
    assert_eq!(summary.sessions_count, 1);
    let usage = &cache.files[&child.to_string_lossy().to_string()];
    assert!(!usage.codex_unresolved_fork_parent);
    assert!(
        usage
            .codex_fork_accounting_state
            .as_ref()
            .is_some_and(|state| state.locally_resolved)
    );
    assert_eq!(
        usage.days.values().next().unwrap()["gpt-5.6-sol"],
        vec![70, 15, 10]
    );

    let (cached, stats, _) = scanner.scan_codex_detailed_with_cache(None);
    assert_eq!(cached.input_tokens, 70);
    assert!(stats.codex_history_read_paths.is_empty());
}

#[test]
fn copied_prefix_subagent_inherited_only_suffix_is_not_billed() {
    let root = tempfile::tempdir().unwrap();
    let sessions = root.path().join("sessions");
    let cache_root = root.path().join("cache");
    let child = write_copied_prefix_subagent_fixture(
        &sessions,
        "child.jsonl",
        "child-id",
        "missing-parent",
        Utc::now() - Duration::hours(1),
        false,
    );
    let scanner = CostScanner::new(7)
        .with_options(CostScanOptions::app_driven())
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![sessions]);

    let (summary, _, cache) = scanner.scan_codex_detailed_with_cache(None);
    assert_eq!(summary.input_tokens, 0);
    assert_eq!(summary.output_tokens, 0);
    assert_eq!(summary.sessions_count, 0);
    let usage = &cache.files[&child.to_string_lossy().to_string()];
    assert!(usage.days.is_empty());
    assert!(!usage.codex_unresolved_fork_parent);
    let state = usage.codex_fork_accounting_state.as_ref().unwrap();
    assert!(state.locally_resolved);
    assert!(state.inherited_totals.is_none());

    let (cached, stats, _) = scanner.scan_codex_detailed_with_cache(None);
    assert_eq!(cached.input_tokens, 0);
    assert_eq!(cached.output_tokens, 0);
    assert_eq!(cached.sessions_count, 0);
    assert!(stats.codex_history_read_paths.is_empty());
}

#[test]
fn copied_prefix_subagent_prefers_validated_parent_baseline() {
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
        base,
        &[1_000],
    );
    let child = write_copied_prefix_subagent_fixture(
        &sessions,
        "child.jsonl",
        "child-id",
        "parent-id",
        base + Duration::seconds(10),
        true,
    );
    let now = std::time::SystemTime::now();
    std::fs::OpenOptions::new()
        .write(true)
        .open(&child)
        .unwrap()
        .set_modified(now - std::time::Duration::from_secs(20))
        .unwrap();
    let mut options = CostScanOptions::app_driven();
    options.prefer_newest_codex_sessions_first = false;
    let parent_size = std::fs::metadata(&parent).unwrap().len();
    let child_size = std::fs::metadata(&child).unwrap().len();
    options.codex_max_session_file_bytes =
        i64::try_from(parent_size.max(child_size)).expect("fixture size fits i64");
    let scanner = CostScanner::new(7)
        .with_options(options)
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![sessions]);

    let (_, stats, cache) = scanner.scan_codex_detailed_with_cache(None);
    let state = cache.files[&child.to_string_lossy().to_string()]
        .codex_fork_accounting_state
        .as_ref()
        .unwrap();

    assert_eq!(state.inherited_totals.as_ref().unwrap().input, 1_000);
    assert!(!state.locally_resolved);
    assert_eq!(stats.files_seen, 2);
    assert_eq!(stats.codex_read_receipt.metadata_reads, 2);
    assert_eq!(stats.codex_read_receipt.history_reads, 2);
    assert_eq!(
        stats.codex_bytes_read,
        parent_size.saturating_add(child_size),
        "one bounded parse per candidate must enforce the per-file allowance"
    );
    assert_eq!(
        stats.codex_history_read_paths,
        vec![
            parent.to_string_lossy().to_string(),
            child.to_string_lossy().to_string(),
        ]
    );
}

#[test]
fn candidate_limit_counts_each_child_parent_candidate_once() {
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
        base,
        &[1_000],
    );
    let child = write_copied_prefix_subagent_fixture(
        &sessions,
        "child.jsonl",
        "child-id",
        "parent-id",
        base + Duration::seconds(10),
        true,
    );
    let now = std::time::SystemTime::now();
    std::fs::OpenOptions::new()
        .write(true)
        .open(&child)
        .unwrap()
        .set_modified(now - std::time::Duration::from_secs(20))
        .unwrap();
    std::fs::OpenOptions::new()
        .write(true)
        .open(&parent)
        .unwrap()
        .set_modified(now - std::time::Duration::from_secs(10))
        .unwrap();
    let mut options = CostScanOptions::app_driven();
    options.prefer_newest_codex_sessions_first = false;
    options.codex_candidate_limit = 1;
    let scanner = CostScanner::new(7)
        .with_options(options)
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![sessions]);

    let (_, stats, cache) = scanner.scan_codex_detailed_with_cache(None);

    assert_eq!(stats.files_seen, 1);
    assert_eq!(stats.codex_read_receipt.metadata_reads, 1);
    assert_eq!(stats.codex_read_receipt.history_reads, 1);
    assert_eq!(
        stats.codex_metadata_read_paths,
        vec![child.to_string_lossy().to_string()]
    );
    assert_eq!(
        cache.codex_pending_paths,
        vec![parent.to_string_lossy().to_string()]
    );
}

#[test]
fn cold_scan_orders_multi_level_parent_chain_before_children() {
    let root = tempfile::tempdir().unwrap();
    let sessions = root.path().join("sessions");
    let cache_root = root.path().join("cache");
    let base = Utc::now() - Duration::hours(1);
    let ancestor = write_codex_fork_session_fixture(
        &sessions,
        "ancestor.jsonl",
        "ancestor-id",
        None,
        base,
        base,
        &[1_000],
    );
    let parent = write_codex_fork_session_fixture(
        &sessions,
        "parent.jsonl",
        "parent-id",
        Some("ancestor-id"),
        base + Duration::seconds(10),
        base + Duration::seconds(10),
        &[1_500],
    );
    let child = write_codex_fork_session_fixture(
        &sessions,
        "child.jsonl",
        "child-id",
        Some("parent-id"),
        base + Duration::seconds(20),
        base + Duration::seconds(20),
        &[2_000],
    );
    let now = std::time::SystemTime::now();
    for (path, age) in [(&child, 30), (&parent, 20), (&ancestor, 10)] {
        std::fs::OpenOptions::new()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(now - std::time::Duration::from_secs(age))
            .unwrap();
    }
    let mut options = CostScanOptions::app_driven();
    options.prefer_newest_codex_sessions_first = false;
    let scanner = CostScanner::new(7)
        .with_options(options)
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![sessions]);

    let (_, stats, cache) = scanner.scan_codex_detailed_with_cache(None);

    assert_eq!(stats.files_seen, 3);
    assert_eq!(stats.codex_read_receipt.metadata_reads, 3);
    assert_eq!(stats.codex_read_receipt.history_reads, 3);
    assert_eq!(
        stats.codex_history_read_paths,
        vec![
            ancestor.to_string_lossy().to_string(),
            parent.to_string_lossy().to_string(),
            child.to_string_lossy().to_string(),
        ]
    );
    let parent_state = cache.files[&parent.to_string_lossy().to_string()]
        .codex_fork_accounting_state
        .as_ref()
        .unwrap();
    let child_state = cache.files[&child.to_string_lossy().to_string()]
        .codex_fork_accounting_state
        .as_ref()
        .unwrap();
    assert_eq!(parent_state.inherited_totals.as_ref().unwrap().input, 1_000);
    assert_eq!(child_state.inherited_totals.as_ref().unwrap().input, 1_500);
}

fn assert_unsafe_lineage_is_unresolved(
    summary: &CostSummary,
    stats: &CostScanStats,
    cache: &CostUsageCache,
    paths: &[&Path],
) {
    assert_eq!(summary.sessions_count, 0);
    assert_eq!(summary.input_tokens, 0);
    assert_eq!(
        stats.codex_read_receipt.metadata_reads,
        u32::try_from(paths.len()).expect("fixture count fits u32")
    );
    assert_eq!(stats.codex_read_receipt.history_reads, 0);
    assert!(stats.codex_history_read_paths.is_empty());
    for path in paths {
        let usage = &cache.files[&path.to_string_lossy().to_string()];
        assert!(usage.codex_unresolved_fork_parent);
        assert!(usage.codex_fork_accounting_state.is_none());
        assert!(usage.days.is_empty());
    }
}

#[test]
fn duplicate_parent_session_ids_fail_closed_with_their_child() {
    let root = tempfile::tempdir().unwrap();
    let sessions = root.path().join("sessions");
    let cache_root = root.path().join("cache");
    let base = Utc::now() - Duration::hours(1);
    let first_parent = write_codex_fork_session_fixture(
        &sessions,
        "first-parent.jsonl",
        "parent-id",
        None,
        base,
        base,
        &[1_000],
    );
    let second_parent = write_codex_fork_session_fixture(
        &sessions,
        "second-parent.jsonl",
        "parent-id",
        None,
        base + Duration::seconds(1),
        base + Duration::seconds(1),
        &[2_000],
    );
    let child = write_copied_prefix_subagent_fixture(
        &sessions,
        "child.jsonl",
        "child-id",
        "parent-id",
        base + Duration::seconds(2),
        true,
    );
    let mut options = CostScanOptions::app_driven();
    options.prefer_newest_codex_sessions_first = false;
    let scanner = CostScanner::new(7)
        .with_options(options)
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![sessions]);

    let (summary, stats, cache) = scanner.scan_codex_detailed_with_cache(None);

    assert_unsafe_lineage_is_unresolved(
        &summary,
        &stats,
        &cache,
        &[&first_parent, &second_parent, &child],
    );
}

#[test]
fn two_node_subagent_cycle_fails_closed() {
    let root = tempfile::tempdir().unwrap();
    let sessions = root.path().join("sessions");
    let cache_root = root.path().join("cache");
    let base = Utc::now() - Duration::hours(1);
    let first = write_copied_prefix_subagent_fixture(
        &sessions,
        "first.jsonl",
        "first-id",
        "second-id",
        base,
        true,
    );
    let second = write_copied_prefix_subagent_fixture(
        &sessions,
        "second.jsonl",
        "second-id",
        "first-id",
        base + Duration::seconds(1),
        true,
    );
    let mut options = CostScanOptions::app_driven();
    options.prefer_newest_codex_sessions_first = false;
    let scanner = CostScanner::new(7)
        .with_options(options)
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![sessions]);

    let (summary, stats, cache) = scanner.scan_codex_detailed_with_cache(None);

    assert_unsafe_lineage_is_unresolved(&summary, &stats, &cache, &[&first, &second]);
}

#[test]
fn self_referential_subagent_fails_closed() {
    let root = tempfile::tempdir().unwrap();
    let sessions = root.path().join("sessions");
    let cache_root = root.path().join("cache");
    let session = write_copied_prefix_subagent_fixture(
        &sessions,
        "self-cycle.jsonl",
        "self-id",
        "self-id",
        Utc::now() - Duration::hours(1),
        true,
    );
    let scanner = CostScanner::new(7)
        .with_options(CostScanOptions::app_driven())
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![sessions]);

    let (summary, stats, cache) = scanner.scan_codex_detailed_with_cache(None);

    assert_unsafe_lineage_is_unresolved(&summary, &stats, &cache, &[&session]);
}

fn assert_cached_inference_is_replaced_when_parent_appears(
    prefer_newest_codex_sessions_first: bool,
) {
    let root = tempfile::tempdir().unwrap();
    let sessions = root.path().join("sessions");
    let cache_root = root.path().join("cache");
    let base = Utc::now() - Duration::hours(1);
    let child = write_copied_prefix_subagent_fixture(
        &sessions,
        "child.jsonl",
        "child-id",
        "parent-id",
        base + Duration::seconds(10),
        true,
    );
    let mut options = CostScanOptions::app_driven();
    options.prefer_newest_codex_sessions_first = prefer_newest_codex_sessions_first;
    let scanner = CostScanner::new(7)
        .with_options(options)
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![sessions.clone()]);

    let (_, _, inferred_cache) = scanner.scan_codex_detailed_with_cache(None);
    let inferred_state = inferred_cache.files[&child.to_string_lossy().to_string()]
        .codex_fork_accounting_state
        .as_ref()
        .unwrap();
    assert!(inferred_state.locally_resolved);
    assert_eq!(
        inferred_state.inherited_totals.as_ref().unwrap().input,
        5_000
    );

    let parent = write_codex_fork_session_fixture(
        &sessions,
        "parent.jsonl",
        "parent-id",
        None,
        base,
        base,
        &[1_000],
    );
    let now = std::time::SystemTime::now();
    std::fs::OpenOptions::new()
        .write(true)
        .open(parent)
        .unwrap()
        .set_modified(now - std::time::Duration::from_secs(10))
        .unwrap();

    let (_, stats, validated_cache) = scanner.scan_codex_detailed_with_cache(None);
    let validated_state = validated_cache.files[&child.to_string_lossy().to_string()]
        .codex_fork_accounting_state
        .as_ref()
        .unwrap();
    assert!(!validated_state.locally_resolved);
    assert_eq!(
        validated_state.inherited_totals.as_ref().unwrap().input,
        1_000
    );
    assert!(
        stats
            .codex_history_read_paths
            .contains(&child.to_string_lossy().to_string()),
        "the unchanged child must be reparsed when baseline provenance changes"
    );
}

#[test]
fn copied_prefix_subagent_replaces_cached_inference_when_parent_is_visited_first() {
    assert_cached_inference_is_replaced_when_parent_appears(true);
}

#[test]
fn copied_prefix_subagent_replaces_cached_inference_when_child_would_be_visited_first() {
    assert_cached_inference_is_replaced_when_parent_appears(false);
}
