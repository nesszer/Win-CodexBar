use super::*;

fn write_subagent(
    sessions_root: &Path,
    name: &str,
    session_id: &str,
    parent_id: &str,
    timestamp: DateTime<Utc>,
) -> PathBuf {
    let day = timestamp.with_timezone(&Local).date_naive();
    let day_dir = sessions_root
        .join(day.format("%Y").to_string())
        .join(day.format("%m").to_string())
        .join(day.format("%d").to_string());
    std::fs::create_dir_all(&day_dir).unwrap();
    let path = day_dir.join(name);
    let rows = [
        serde_json::json!({
            "type": "session_meta", "ordinal": 0, "timestamp": timestamp.to_rfc3339(),
            "payload": {
                "id": session_id,
                "forked_from_id": parent_id,
                "subagent_history_start_ordinal": 10,
                "thread_source": "subagent",
                "source": {"subagent": {"thread_spawn": {"parent_thread_id": parent_id}}}
            }
        }),
        lineage_token_row(timestamp, 2, 1_000, 0),
        serde_json::json!({
            "type": "turn_context", "ordinal": 10, "timestamp": timestamp.to_rfc3339(),
            "payload": {"model": "gpt-5.6-sol"}
        }),
        lineage_token_row(timestamp, 12, 1_000, 1_000),
        lineage_token_row(timestamp + Duration::seconds(1), 20, 1_050, 50),
    ];
    let body = rows
        .into_iter()
        .map(|row| row.to_string())
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    std::fs::write(&path, body).unwrap();
    path
}

fn lineage_token_row(
    timestamp: DateTime<Utc>,
    ordinal: i64,
    total_input: i64,
    last_input: i64,
) -> serde_json::Value {
    serde_json::json!({
        "type": "event_msg", "ordinal": ordinal, "timestamp": timestamp.to_rfc3339(),
        "payload": {"type": "token_count", "info": {
            "model": "gpt-5.6-sol",
            "total_token_usage": {
                "input_tokens": total_input, "cached_input_tokens": 0, "output_tokens": 5
            },
            "last_token_usage": {
                "input_tokens": last_input, "cached_input_tokens": 0, "output_tokens": 5
            }
        }}
    })
}

fn write_missing_ordinal_subagent(
    sessions_root: &Path,
    name: &str,
    base: DateTime<Utc>,
    include_owned_usage: bool,
) -> PathBuf {
    let day = base.with_timezone(&Local).date_naive();
    let day_dir = sessions_root
        .join(day.format("%Y").to_string())
        .join(day.format("%m").to_string())
        .join(day.format("%d").to_string());
    std::fs::create_dir_all(&day_dir).unwrap();
    let path = day_dir.join(name);
    let mut missing_ordinal = lineage_token_row(base, 11, 100, 0);
    missing_ordinal.as_object_mut().unwrap().remove("ordinal");
    let tail = if include_owned_usage {
        lineage_token_row(base, 12, 120, 10)
    } else {
        lineage_token_row(base, 12, 100, 0)
    };
    let rows = [
        serde_json::json!({
            "type": "session_meta", "ordinal": 0, "timestamp": base.to_rfc3339(),
            "payload": {
                "id": "child-id",
                "forked_from_id": "absent-parent-id",
                "subagent_history_start_ordinal": 10,
                "thread_source": "subagent",
                "source": {"subagent": {"thread_spawn": {"parent_thread_id": "absent-parent-id"}}}
            }
        }),
        lineage_token_row(base, 9, 100, 0),
        lineage_token_row(base, 10, 100, 0),
        missing_ordinal,
        tail,
    ];
    let body = rows
        .into_iter()
        .map(|row| row.to_string())
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    std::fs::write(&path, body).unwrap();
    path
}

fn bounded_scanner(sessions: &Path, cache_root: &Path) -> CostScanner {
    let mut options = CostScanOptions::app_driven();
    options.codex_candidate_limit = 1;
    options.prefer_newest_codex_sessions_first = false;
    CostScanner::new(7)
        .with_options(options)
        .with_cache_root(cache_root)
        .with_sessions_dirs(vec![sessions.to_path_buf()])
}

fn assert_locally_inferred(cache: &CostUsageCache, path: &Path) {
    let usage = &cache.files[&path.to_string_lossy().to_string()];
    assert!(!usage.codex_unresolved_fork_parent);
    assert!(
        usage
            .codex_fork_accounting_state
            .as_ref()
            .is_some_and(|state| state.locally_resolved)
    );
}

fn assert_unresolved(cache: &CostUsageCache, path: &Path) {
    let usage = &cache.files[&path.to_string_lossy().to_string()];
    assert!(usage.codex_unresolved_fork_parent);
    assert!(usage.days.is_empty());
    assert!(usage.codex_fork_accounting_state.is_none());
}

#[test]
fn appended_owned_token_row_reinfers_locally_resolved_subagent_from_start() {
    use std::io::Write as _;

    let root = tempfile::tempdir().unwrap();
    let sessions = root.path().join("sessions");
    let cache_root = root.path().join("cache");
    let base = Utc::now() - Duration::hours(1);
    let child = write_subagent(&sessions, "child.jsonl", "child-id", "missing-parent", base);
    let scanner = bounded_scanner(&sessions, &cache_root);

    let (initial, _, initial_cache) = scanner.scan_codex_detailed_with_cache(None);
    assert_eq!(initial.input_tokens, 50);
    assert_locally_inferred(&initial_cache, &child);

    let appended_owned_row = lineage_token_row(base + Duration::seconds(2), 22, 1_100, 50);
    std::fs::OpenOptions::new()
        .append(true)
        .open(&child)
        .unwrap()
        .write_all(format!("{appended_owned_row}\n").as_bytes())
        .unwrap();

    let (grown, _, grown_cache) = scanner.scan_codex_detailed_with_cache(None);
    assert_eq!(grown.input_tokens, 100);
    assert_locally_inferred(&grown_cache, &child);
}

#[test]
fn replaced_parent_with_same_path_size_and_mtime_cannot_author_lineage() {
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
    let child = write_subagent(
        &sessions,
        "child.jsonl",
        "child-id",
        "parent-id",
        base + Duration::seconds(10),
    );
    let scanner = CostScanner::new(7)
        .with_options(CostScanOptions::app_driven())
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![sessions.clone()]);
    let (_, _, cache) = scanner.scan_codex_detailed_with_cache(None);
    let parent_key = parent.to_string_lossy().to_string();
    let child_usage = &cache.files[&child.to_string_lossy().to_string()];
    assert!(matches!(
        CodexLineagePlanner::new(&cache).decision_for_usage(&cache, child_usage),
        CodexLineageDecision::ParentReady(_)
    ));

    let old_identity = cache.files[&parent_key]
        .codex_file_identity
        .clone()
        .expect("parent identity persisted");
    let old_metadata = std::fs::metadata(&parent).unwrap();
    let old_mtime = old_metadata.modified().unwrap();
    let old_size = old_metadata.len();
    let rotated = parent.with_extension("old");
    std::fs::rename(&parent, &rotated).unwrap();
    let replacement = write_codex_fork_session_fixture(
        &sessions,
        "parent.jsonl",
        "parent-id",
        None,
        base,
        base,
        &[2_000],
    );
    std::fs::OpenOptions::new()
        .write(true)
        .open(&replacement)
        .unwrap()
        .set_modified(old_mtime)
        .unwrap();
    let replacement_metadata = std::fs::metadata(&replacement).unwrap();
    assert_eq!(replacement_metadata.len(), old_size);
    let replacement_identity =
        JsonlScanner::codex_file_identity(&replacement, &replacement_metadata)
            .expect("replacement identity available");
    assert_ne!(replacement_identity, old_identity);

    assert_eq!(
        CodexLineagePlanner::new(&cache).decision_for_usage(&cache, child_usage),
        CodexLineageDecision::Unsafe
    );
}

#[test]
fn missing_explicit_ordinal_keeps_subagent_cache_unresolved() {
    let root = tempfile::tempdir().unwrap();
    let sessions = root.path().join("sessions");
    let cache_root = root.path().join("cache");
    let base = Utc::now() - Duration::hours(1);
    let child = write_missing_ordinal_subagent(&sessions, "child.jsonl", base, true);
    let scanner = CostScanner::new(7)
        .with_options(CostScanOptions::app_driven())
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![sessions]);

    let (summary, _, cache) = scanner.scan_codex_detailed_with_cache(None);

    assert_eq!(summary.input_tokens, 0);
    assert_eq!(summary.sessions_count, 0);
    assert_unresolved(&cache, &child);
}

#[test]
fn missing_ordinal_cannot_complete_zero_usage_subagent_cache() {
    let root = tempfile::tempdir().unwrap();
    let sessions = root.path().join("sessions");
    let cache_root = root.path().join("cache");
    let base = Utc::now() - Duration::hours(1);
    let child = write_missing_ordinal_subagent(&sessions, "child.jsonl", base, false);
    let scanner = CostScanner::new(7)
        .with_options(CostScanOptions::app_driven())
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![sessions]);

    let (summary, _, cache) = scanner.scan_codex_detailed_with_cache(None);

    assert_eq!(summary.input_tokens, 0);
    assert_eq!(summary.sessions_count, 0);
    assert_unresolved(&cache, &child);
}

#[test]
fn missing_ordinal_after_local_resolution_keeps_subagent_cache_unresolved() {
    use std::io::Write as _;

    let root = tempfile::tempdir().unwrap();
    let sessions = root.path().join("sessions");
    let cache_root = root.path().join("cache");
    let base = Utc::now() - Duration::hours(1);
    let child = write_subagent(
        &sessions,
        "child.jsonl",
        "child-id",
        "missing-parent-id",
        base,
    );
    let mut missing_ordinal = lineage_token_row(base + Duration::seconds(2), 21, 1_060, 10);
    missing_ordinal.as_object_mut().unwrap().remove("ordinal");
    std::fs::OpenOptions::new()
        .append(true)
        .open(&child)
        .unwrap()
        .write_all(format!("{missing_ordinal}\n").as_bytes())
        .unwrap();
    let scanner = bounded_scanner(&sessions, &cache_root);

    let (summary, _, cache) = scanner.scan_codex_detailed_with_cache(None);

    assert_eq!(summary.input_tokens, 0);
    assert_eq!(summary.sessions_count, 0);
    assert_unresolved(&cache, &child);
}

#[test]
fn legacy_cache_without_file_identity_is_reparsed() {
    let root = tempfile::tempdir().unwrap();
    let sessions = root.path().join("sessions");
    let cache_root = root.path().join("cache");
    let base = Utc::now() - Duration::hours(1);
    let session = write_codex_fork_session_fixture(
        &sessions,
        "session.jsonl",
        "root-session-id",
        None,
        base,
        base,
        &[1_000],
    );
    let scanner = CostScanner::new(7)
        .with_options(CostScanOptions::app_driven())
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![sessions]);

    let (_, _, _) = scanner.scan_codex_detailed_with_cache(None);
    let session_key = session.to_string_lossy().to_string();
    let mut legacy_cache = JsonlScanner::load_cache(ProviderId::Codex, Some(&cache_root));
    legacy_cache
        .files
        .get_mut(&session_key)
        .unwrap()
        .codex_file_identity = None;
    JsonlScanner::save_cache(ProviderId::Codex, &mut legacy_cache, Some(&cache_root));

    let (_, stats, refreshed_cache) = scanner.scan_codex_detailed_with_cache(None);

    assert!(stats.codex_history_read_paths.contains(&session_key));
    assert!(
        refreshed_cache.files[&session_key]
            .codex_file_identity
            .is_some()
    );
}

#[test]
fn bounded_refresh_detects_duplicate_parent_owners_across_cache_and_candidate() {
    let root = tempfile::tempdir().unwrap();
    let sessions = root.path().join("sessions");
    let cache_root = root.path().join("cache");
    let base = Utc::now() - Duration::hours(1);
    let child = write_subagent(&sessions, "child.jsonl", "child-id", "parent-id", base);
    let scanner = bounded_scanner(&sessions, &cache_root);

    let (_, first_stats, first_cache) = scanner.scan_codex_detailed_with_cache(None);
    assert_eq!(first_stats.codex_read_receipt.metadata_reads, 1);
    assert_eq!(first_stats.codex_read_receipt.history_reads, 1);
    assert_locally_inferred(&first_cache, &child);

    let first_parent = write_codex_fork_session_fixture(
        &sessions,
        "parent-a.jsonl",
        "parent-id",
        None,
        base - Duration::seconds(2),
        base - Duration::seconds(2),
        &[1_000],
    );
    let (_, second_stats, second_cache) = scanner.scan_codex_detailed_with_cache(None);
    assert_eq!(second_stats.codex_read_receipt.metadata_reads, 1);
    assert_eq!(second_stats.codex_read_receipt.history_reads, 1);
    assert_locally_inferred(&second_cache, &child);

    let second_parent = write_codex_fork_session_fixture(
        &sessions,
        "parent-b.jsonl",
        "parent-id",
        None,
        base - Duration::seconds(1),
        base - Duration::seconds(1),
        &[2_000],
    );
    let (summary, third_stats, cache) = scanner.scan_codex_detailed_with_cache(None);

    assert_eq!(third_stats.codex_read_receipt.metadata_reads, 1);
    assert_eq!(third_stats.codex_read_receipt.history_reads, 0);
    assert_eq!(summary.sessions_count, 0);
    assert_unresolved(&cache, &first_parent);
    assert_unresolved(&cache, &second_parent);
    assert_unresolved(&cache, &child);
}

#[test]
fn bounded_refresh_detects_equal_timestamp_two_node_cycle() {
    let root = tempfile::tempdir().unwrap();
    let sessions = root.path().join("sessions");
    let cache_root = root.path().join("cache");
    let base = Utc::now() - Duration::hours(1);
    let first = write_subagent(&sessions, "first.jsonl", "first-id", "second-id", base);
    let scanner = bounded_scanner(&sessions, &cache_root);
    let (_, _, first_cache) = scanner.scan_codex_detailed_with_cache(None);
    assert_locally_inferred(&first_cache, &first);

    let second = write_subagent(&sessions, "second.jsonl", "second-id", "first-id", base);
    let (summary, stats, cache) = scanner.scan_codex_detailed_with_cache(None);

    assert_eq!(stats.codex_read_receipt.metadata_reads, 1);
    assert_eq!(stats.codex_read_receipt.history_reads, 0);
    assert_eq!(summary.sessions_count, 0);
    assert_unresolved(&cache, &first);
    assert_unresolved(&cache, &second);
}

#[test]
fn bounded_refresh_rejects_self_cycle_migration() {
    let root = tempfile::tempdir().unwrap();
    let sessions = root.path().join("sessions");
    let cache_root = root.path().join("cache");
    let base = Utc::now() - Duration::hours(1);
    let session = write_subagent(&sessions, "self.jsonl", "self-id", "missing-id", base);
    let scanner = bounded_scanner(&sessions, &cache_root);
    let (_, _, first_cache) = scanner.scan_codex_detailed_with_cache(None);
    assert_locally_inferred(&first_cache, &session);

    write_subagent(
        &sessions,
        "self.jsonl",
        "self-id",
        "self-id",
        base + Duration::seconds(1),
    );
    let (summary, stats, cache) = scanner.scan_codex_detailed_with_cache(None);

    assert_eq!(stats.codex_read_receipt.metadata_reads, 1);
    assert_eq!(stats.codex_read_receipt.history_reads, 0);
    assert_eq!(summary.sessions_count, 0);
    assert_unresolved(&cache, &session);
}

#[test]
fn bounded_refresh_rejects_dependent_of_locally_inferred_parent() {
    let root = tempfile::tempdir().unwrap();
    let sessions = root.path().join("sessions");
    let cache_root = root.path().join("cache");
    let base = Utc::now() - Duration::hours(1);
    let parent = write_subagent(&sessions, "parent.jsonl", "parent-id", "missing-id", base);
    let scanner = bounded_scanner(&sessions, &cache_root);
    let (_, _, first_cache) = scanner.scan_codex_detailed_with_cache(None);
    assert_locally_inferred(&first_cache, &parent);

    let dependent = write_subagent(
        &sessions,
        "dependent.jsonl",
        "dependent-id",
        "parent-id",
        base + Duration::seconds(1),
    );
    let (_, stats, cache) = scanner.scan_codex_detailed_with_cache(None);

    assert_eq!(stats.codex_read_receipt.metadata_reads, 1);
    assert_eq!(stats.codex_read_receipt.history_reads, 0);
    assert_locally_inferred(&cache, &parent);
    assert_unresolved(&cache, &dependent);
}

#[test]
fn current_refresh_scopes_unsafe_cache_invalidation_to_range_and_dependencies() {
    let root = tempfile::tempdir().unwrap();
    let sessions = root.path().join("sessions");
    let cache_root = root.path().join("cache");
    let active_time = Utc::now() - Duration::hours(1);
    let old_date = Local::now().date_naive() - Duration::days(30);
    let old_day = old_date.format("%Y-%m-%d").to_string();
    let old_dir = sessions
        .join(old_date.format("%Y").to_string())
        .join(old_date.format("%m").to_string())
        .join(old_date.format("%d").to_string());
    let mut cache = CostUsageCache::default();

    {
        let mut add_cached = |name: &str, session_id: &str, parent_id: Option<&str>| {
            let path = old_dir.join(name).to_string_lossy().to_string();
            let mut usage = cached_usage_with_packed(&old_day, "gpt-5.6-sol", vec![100, 0, 5, 0]);
            usage.codex_session_id = Some(session_id.to_string());
            usage.codex_forked_from_id = parent_id.map(str::to_string);
            cache.files.insert(path, usage);
        };
        add_cached("unrelated-a.jsonl", "unrelated-a", Some("unrelated-b"));
        add_cached("unrelated-b.jsonl", "unrelated-b", Some("unrelated-a"));
        add_cached("required-a.jsonl", "required-parent", None);
        add_cached("required-b.jsonl", "required-parent", None);
    }
    JsonlScanner::save_cache(ProviderId::Codex, &mut cache, Some(&cache_root));

    let active_child = write_codex_fork_session_fixture(
        &sessions,
        "active-child.jsonl",
        "active-child",
        Some("required-parent"),
        active_time,
        active_time + Duration::seconds(1),
        &[1_000_000, 1_000_140],
    );
    let scanner = CostScanner::new(7)
        .with_options(CostScanOptions::app_driven())
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![sessions]);

    let (summary, _, refreshed) = scanner.scan_codex_detailed_with_cache(None);
    let cached_path = |name: &str| old_dir.join(name).to_string_lossy().to_string();

    for path in [
        cached_path("unrelated-a.jsonl"),
        cached_path("unrelated-b.jsonl"),
    ] {
        let usage = refreshed
            .files
            .get(&path)
            .expect("unrelated history retained");
        assert_eq!(usage.days[&old_day]["gpt-5.6-sol"], vec![100, 0, 5, 0]);
        assert!(!usage.codex_unresolved_fork_parent);
    }
    for path in [
        cached_path("required-a.jsonl"),
        cached_path("required-b.jsonl"),
    ] {
        assert_unresolved(&refreshed, Path::new(&path));
    }
    assert_eq!(summary.input_tokens, 0);
    assert_eq!(summary.sessions_count, 0);
    assert_unresolved(&refreshed, &active_child);
}
