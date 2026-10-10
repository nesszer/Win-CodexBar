use super::*;
use crate::core::test_fixtures::test_file_usage;

fn file_usage(day: &str, model: &str, counts: Vec<i64>) -> CostUsageFileUsage {
    test_file_usage(
        100,
        HashMap::from([(
            day.to_string(),
            HashMap::from([(model.to_string(), counts)]),
        )]),
    )
}

fn seeded_cache() -> CostUsageCache {
    CostUsageCache {
        last_scan_unix_ms: 1_000,
        scan_since_key: Some("2026-01-01".to_string()),
        scan_until_key: Some("2026-01-31".to_string()),
        files: HashMap::from([
            (
                "a.jsonl".to_string(),
                file_usage("2026-01-10", "gpt-5.6-sol", vec![10, 0, 1]),
            ),
            (
                "b.jsonl".to_string(),
                file_usage("2026-01-11", "gpt-5.6-luna", vec![20, 5, 2]),
            ),
        ]),
        days: HashMap::from([(
            "2026-01-10".to_string(),
            HashMap::from([("gpt-5.6-sol".to_string(), vec![10, 0, 1])]),
        )]),
        ..Default::default()
    }
}

fn artifact_path(root: &Path) -> PathBuf {
    JsonlScanner::cache_path(ProviderId::Codex, Some(root))
}

fn saved_root() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    let mut cache = seeded_cache();
    JsonlScanner::save_cache(ProviderId::Codex, &mut cache, Some(root.path()));
    root
}

#[test]
fn equal_content_encodes_to_equal_bytes_regardless_of_map_order() {
    let build = |reverse: bool| {
        let mut cache = CostUsageCache::default();
        let mut keys: Vec<u32> = (0..64).collect();
        if reverse {
            keys.reverse();
        }
        for key in keys {
            cache.files.insert(
                format!("f{key}.jsonl"),
                file_usage("2026-01-10", "m", vec![1]),
            );
            cache.days.insert(
                format!("2026-01-{key:02}"),
                HashMap::from([("m".to_string(), vec![i64::from(key)])]),
            );
        }
        serde_json::to_string(&cache).unwrap()
    };
    assert_eq!(build(false), build(true));
}

#[test]
fn unchanged_payload_with_new_scan_time_skips_the_rewrite() {
    let root = saved_root();
    let path = artifact_path(root.path());
    let before = std::fs::read(&path).unwrap();
    let modified = std::fs::metadata(&path).unwrap().modified().unwrap();

    let mut cache = JsonlScanner::load_cache(ProviderId::Codex, Some(root.path()));
    assert_eq!(cache.last_scan_unix_ms, 1_000);
    cache.last_scan_unix_ms = 5_000;
    JsonlScanner::save_cache(ProviderId::Codex, &mut cache, Some(root.path()));

    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert_eq!(
        std::fs::metadata(&path).unwrap().modified().unwrap(),
        modified
    );

    // The debounce time stays available in this process, so the next scan can
    // still be debounced without another rewrite.
    let reloaded = JsonlScanner::load_cache(ProviderId::Codex, Some(root.path()));
    assert_eq!(reloaded.last_scan_unix_ms, 5_000);
    assert!(JsonlScanner::should_skip_cached_scan(
        &reloaded,
        CostScanOptions::default(),
        5_000 + 1_000
    ));
}

#[test]
fn repeated_unchanged_saves_keep_the_latest_scan_time_in_memory() {
    let root = saved_root();
    let path = artifact_path(root.path());
    let before = std::fs::read(&path).unwrap();

    let mut cache = JsonlScanner::load_cache(ProviderId::Codex, Some(root.path()));
    for scan_unix_ms in [2_000, 3_000, 4_000] {
        cache.last_scan_unix_ms = scan_unix_ms;
        JsonlScanner::save_cache(ProviderId::Codex, &mut cache, Some(root.path()));
    }
    assert_eq!(std::fs::read(&path).unwrap(), before);
    let reloaded = JsonlScanner::load_cache(ProviderId::Codex, Some(root.path()));
    assert_eq!(reloaded.last_scan_unix_ms, 4_000);
}

#[test]
fn resetting_the_scan_time_is_not_lost_by_a_skipped_save() {
    let root = saved_root();
    let mut cache = JsonlScanner::load_cache(ProviderId::Codex, Some(root.path()));
    cache.last_scan_unix_ms = 0;
    JsonlScanner::save_cache(ProviderId::Codex, &mut cache, Some(root.path()));

    let reloaded = JsonlScanner::load_cache(ProviderId::Codex, Some(root.path()));
    assert_eq!(reloaded.last_scan_unix_ms, 0);
    assert!(!JsonlScanner::should_skip_cached_scan(
        &reloaded,
        CostScanOptions::default(),
        1_000
    ));
}

#[test]
fn changed_file_state_is_written_with_the_new_scan_time() {
    let root = saved_root();
    let mut cache = JsonlScanner::load_cache(ProviderId::Codex, Some(root.path()));
    cache.last_scan_unix_ms = 5_000;
    cache
        .files
        .get_mut("a.jsonl")
        .unwrap()
        .days
        .get_mut("2026-01-10")
        .unwrap()
        .insert("gpt-5.6-sol".to_string(), vec![11, 0, 1]);
    JsonlScanner::save_cache(ProviderId::Codex, &mut cache, Some(root.path()));

    let on_disk: CostUsageCache =
        serde_json::from_slice(&std::fs::read(artifact_path(root.path())).unwrap()).unwrap();
    assert_eq!(on_disk.last_scan_unix_ms, 5_000);
    assert_eq!(
        on_disk.files["a.jsonl"].days["2026-01-10"]["gpt-5.6-sol"],
        vec![11, 0, 1]
    );

    // The rewrite becomes the new baseline for the next skip decision.
    let before = std::fs::read(artifact_path(root.path())).unwrap();
    cache.last_scan_unix_ms = 6_000;
    JsonlScanner::save_cache(ProviderId::Codex, &mut cache, Some(root.path()));
    assert_eq!(std::fs::read(artifact_path(root.path())).unwrap(), before);
}

#[test]
fn removed_file_state_is_written() {
    let root = saved_root();
    let mut cache = JsonlScanner::load_cache(ProviderId::Codex, Some(root.path()));
    cache.files.remove("b.jsonl");
    JsonlScanner::save_cache(ProviderId::Codex, &mut cache, Some(root.path()));

    let on_disk: CostUsageCache =
        serde_json::from_slice(&std::fs::read(artifact_path(root.path())).unwrap()).unwrap();
    assert!(!on_disk.files.contains_key("b.jsonl"));
}

#[test]
fn budget_pruning_changes_the_payload_and_is_written() {
    let root = tempfile::tempdir().unwrap();
    let path = artifact_path(root.path());
    let mut seeded = CostUsageCache {
        codex_cache_schema_version: CODEX_CACHE_SCHEMA_VERSION,
        last_scan_unix_ms: 1_000,
        scan_since_key: Some("2026-06-01".to_string()),
        scan_until_key: Some("2026-06-30".to_string()),
        ..Default::default()
    };
    // Written directly so the seed artifact keeps the out-of-window entries
    // that `save_cache` would prune.
    for index in 0..=crate::core::CostUsageCacheBudget::MAX_FILE_ENTRIES {
        seeded.files.insert(
            format!("old-{index}.jsonl"),
            file_usage("2026-01-10", "m", vec![1]),
        );
    }
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, serde_json::to_vec(&seeded).unwrap()).unwrap();

    let mut cache = JsonlScanner::load_cache(ProviderId::Codex, Some(root.path()));
    assert!(cache.files.len() > crate::core::CostUsageCacheBudget::MAX_FILE_ENTRIES);
    cache.last_scan_unix_ms = 5_000;
    JsonlScanner::save_cache(ProviderId::Codex, &mut cache, Some(root.path()));

    let on_disk: CostUsageCache = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert!(on_disk.files.is_empty(), "out-of-window entries are pruned");
    assert!(on_disk.previous_report.is_some());
    assert_eq!(on_disk.last_scan_unix_ms, 5_000);
}

#[test]
fn stale_unchanged_writer_does_not_touch_the_newer_baseline() {
    let root = saved_root();
    let path = artifact_path(root.path());

    let mut stale = JsonlScanner::load_cache(ProviderId::Codex, Some(root.path()));
    let mut newer = JsonlScanner::load_cache(ProviderId::Codex, Some(root.path()));
    newer.scan_until_key = Some("2026-02-28".to_string());
    newer.last_scan_unix_ms = 2_000;
    JsonlScanner::save_cache(ProviderId::Codex, &mut newer, Some(root.path()));
    let newer_bytes = std::fs::read(&path).unwrap();

    stale.last_scan_unix_ms = 9_000;
    JsonlScanner::save_cache(ProviderId::Codex, &mut stale, Some(root.path()));

    assert_eq!(std::fs::read(&path).unwrap(), newer_bytes);
    let loaded = JsonlScanner::load_cache(ProviderId::Codex, Some(root.path()));
    assert_eq!(loaded.last_scan_unix_ms, 2_000);
    assert_eq!(loaded.scan_until_key.as_deref(), Some("2026-02-28"));
}

#[test]
fn recorded_scan_time_does_not_outlive_a_replaced_artifact() {
    let root = saved_root();
    let mut cache = JsonlScanner::load_cache(ProviderId::Codex, Some(root.path()));
    cache.last_scan_unix_ms = 5_000;
    JsonlScanner::save_cache(ProviderId::Codex, &mut cache, Some(root.path()));

    // Another process replaces the artifact: the in-memory time must not apply.
    let mut other = seeded_cache();
    other.codex_cache_schema_version = CODEX_CACHE_SCHEMA_VERSION;
    other.last_scan_unix_ms = 700;
    other.scan_until_key = Some("2026-03-31".to_string());
    std::fs::write(
        artifact_path(root.path()),
        serde_json::to_vec(&other).unwrap(),
    )
    .unwrap();

    let reloaded = JsonlScanner::load_cache(ProviderId::Codex, Some(root.path()));
    assert_eq!(reloaded.last_scan_unix_ms, 700);
}

#[test]
fn first_save_without_a_decoded_baseline_always_writes() {
    let root = tempfile::tempdir().unwrap();
    let mut cache = seeded_cache();
    JsonlScanner::save_cache(ProviderId::Codex, &mut cache, Some(root.path()));
    assert!(artifact_path(root.path()).exists());

    // A manually constructed cache has no decoded baseline, so it is never
    // treated as unchanged even when the content is identical.
    let path = artifact_path(root.path());
    std::fs::write(&path, b"{}").unwrap();
    let mut rebuilt = seeded_cache();
    JsonlScanner::save_cache(ProviderId::Codex, &mut rebuilt, Some(root.path()));
    assert!(std::fs::read(&path).unwrap().len() > 2);
}
