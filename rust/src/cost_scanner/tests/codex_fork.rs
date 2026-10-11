//! Codex fork children billed above their parent baseline.

use super::*;

#[test]
fn ordinary_non_fork_session_keeps_cumulative_accounting() {
    let (_root, sessions, cache_root) = codex_scan_dirs();
    let path = write_codex_session_fixture_with_inputs(&sessions, "ordinary.jsonl", &[100, 140]);

    let mut options = CostScanOptions::app_driven();
    options.prefer_newest_codex_sessions_first = false;
    let scanner = CostScanner::new(7)
        .with_options(options)
        .with_cache_root(&cache_root)
        .with_sessions_dirs(vec![sessions]);
    let (summary, _, cache) = scanner.scan_codex_detailed_with_cache(None);

    assert_eq!(summary.input_tokens, 140);
    let usage = cached_file(&cache, &path);
    assert_eq!(cached_input_total(usage), 140);
    assert_eq!(usage.codex_forked_from_id, None);
    assert!(!usage.codex_unresolved_fork_parent);
}

#[test]
fn fork_child_counts_only_growth_above_parent_baseline() {
    let (_root, sessions, cache_root) = codex_scan_dirs();
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
    let child_usage = cached_file(&cache, &child);
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
    let (_root, sessions, cache_root) = codex_scan_dirs();
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
    let (_root, sessions, cache_root) = codex_scan_dirs();
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

    let scanner = app_scanner(7, &cache_root, &sessions);
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
    let (_root, sessions, cache_root) = codex_scan_dirs();
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
    let child_usage = cached_file(&cache, &child);
    assert_eq!(summary.input_tokens, 1_000_140);
    assert_eq!(cached_input_total(child_usage), 140);
    assert!(!child_usage.codex_unresolved_fork_parent);
    assert!(cache.codex_pending_paths.is_empty());
}

#[test]
fn parent_last_token_after_fork_keeps_child_unresolved() {
    let (_root, sessions, cache_root) = codex_scan_dirs();
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

    let scanner = app_scanner(7, &cache_root, &sessions);
    let (summary, _, cache) = scanner.scan_codex_detailed_with_cache(None);

    let child_usage = cached_file(&cache, &child);
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
    let (_root, sessions, cache_root) = codex_scan_dirs();
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
    let scanner = app_scanner(7, &cache_root, &sessions);
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
    let (_root, sessions, cache_root) = codex_scan_dirs();
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

    let scanner = app_scanner(7, &cache_root, &sessions);
    let (summary, _, cache) = scanner.scan_codex_detailed_with_cache(None);
    let child_usage = cached_file(&cache, &child);

    assert_eq!(summary.input_tokens, 1_000_000);
    assert_eq!(cached_input_total(child_usage), 0);
    assert!(child_usage.codex_unresolved_fork_parent);
    assert!(!summary.history_coverage_established);
}
