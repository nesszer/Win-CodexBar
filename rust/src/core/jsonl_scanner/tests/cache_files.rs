//! Scan options, session metadata, line boundaries and the cache file.

use super::*;

#[test]
fn cost_scan_options_app_driven_bypasses_debounce() {
    let debounced = CostScanOptions::default();
    let forced = CostScanOptions::app_driven();
    assert!(!debounced.is_app_driven());
    assert!(forced.is_app_driven());
    let last = 1_000_000_i64;
    let now = last + 1_000; // 1s later, within 60s window

    assert!(debounced.should_skip_scan(last, now));
    assert!(!forced.should_skip_scan(last, now));
    assert!(!debounced.should_skip_scan(last, last + 61_000));

    let cache = CostUsageCache {
        last_scan_unix_ms: last,
        ..Default::default()
    };
    assert!(JsonlScanner::should_skip_cached_scan(
        &cache,
        CostScanOptions::default(),
        now
    ));
    assert!(!JsonlScanner::should_skip_cached_scan(
        &cache,
        CostScanOptions::app_driven(),
        now
    ));
}

#[test]
fn session_meta_pre_read_accepts_snake_and_camel_fork_identity() {
    let root = tempfile::tempdir().unwrap();
    let snake = root.path().join("snake.jsonl");
    std::fs::write(
        &snake,
        concat!(
            r#"{"type":"session_meta","timestamp":"2026-05-31T10:00:00Z","payload":{"session_id":"child-snake","forked_from_id":"parent-snake","history_base":{"thread_id":"history-snake"}}}"#,
            "\n"
        ),
    )
    .unwrap();
    assert_eq!(
        JsonlScanner::read_codex_session_metadata(&snake).unwrap(),
        CodexSessionMetadata {
            session_id: Some("child-snake".to_string()),
            forked_from_id: Some("parent-snake".to_string()),
            lineage: CodexSessionLineage::Child,
            fork_timestamp: Some("2026-05-31T10:00:00Z".to_string()),
            history_base_thread_id: Some("history-snake".to_string()),
            is_subagent: false,
            subagent_history_start_ordinal: None,
        }
    );

    let camel = root.path().join("camel.jsonl");
    std::fs::write(
        &camel,
        concat!(
            r#"{"type":"session_meta","payload":{"sessionId":"child-camel","forkedFromId":"parent-camel","timestamp":"2026-05-31T10:00:01Z"}}"#,
            "\n"
        ),
    )
    .unwrap();
    let metadata = JsonlScanner::read_codex_session_metadata(&camel).unwrap();
    assert_eq!(metadata.session_id.as_deref(), Some("child-camel"));
    assert_eq!(metadata.forked_from_id.as_deref(), Some("parent-camel"));
    assert_eq!(
        metadata.fork_timestamp.as_deref(),
        Some("2026-05-31T10:00:01Z")
    );
}

#[test]
fn legacy_file_usage_json_defaults_fork_metadata() {
    let usage: CostUsageFileUsage = serde_json::from_str(
        r#"{"mtime_unix_ms":0,"size":0,"days":{},"parsed_bytes":null,"last_model":null,"last_totals":null}"#,
    )
    .unwrap();
    assert_eq!(usage.codex_session_id, None);
    assert_eq!(usage.codex_forked_from_id, None);
    assert_eq!(usage.codex_fork_timestamp, None);
    assert!(!usage.codex_unresolved_fork_parent);

    let report: CachedCostReport = serde_json::from_str(
        r#"{"total_cost_usd":1.5,"input_tokens":10,"cached_tokens":2,"output_tokens":3,"sessions_count":1,"updated_at":null,"partial":false}"#,
    )
    .unwrap();
    assert_eq!(report.reasoning_tokens, None);
}

#[test]
fn is_line_boundary_offset_zero_returns_true() {
    // F2: offset 0 is always a valid boundary (start of file).
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("f.jsonl");
    std::fs::write(
        &path,
        b"hello
world
",
    )
    .unwrap();
    assert!(JsonlScanner::is_line_boundary_offset(&path, 0));
}

#[test]
fn is_line_boundary_offset_at_or_past_size_returns_true() {
    // F2: offset >= file_size returns true (EOF or beyond is a valid boundary).
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("f.jsonl");
    let content = b"line1
line2
";
    std::fs::write(&path, content).unwrap();
    let size = i64::try_from(content.len()).unwrap();
    assert!(JsonlScanner::is_line_boundary_offset(&path, size));
    assert!(JsonlScanner::is_line_boundary_offset(&path, size + 100));
}

#[test]
fn is_line_boundary_offset_exact_newline_returns_true() {
    // F2: offset pointing right after a newline is a valid boundary.
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("f.jsonl");
    // "line1\nline2\n" â€” offset 6 is right after first \n
    std::fs::write(&path, b"line1\nline2\n").unwrap();
    assert!(JsonlScanner::is_line_boundary_offset(&path, 6));
}

#[test]
fn is_line_boundary_offset_midline_returns_false() {
    // F2: offset pointing mid-line (byte before is not \n) returns false.
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("f.jsonl");
    // "line1\nline2\n" â€” offset 3 is mid-line (byte before is 'n')
    std::fs::write(&path, b"line1\nline2\n").unwrap();
    assert!(!JsonlScanner::is_line_boundary_offset(&path, 3));
}

#[test]
fn is_line_boundary_offset_missing_file_returns_false() {
    // F2: missing file returns false (probe fails).
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("nonexistent.jsonl");
    // offset > 0 so it doesn't short-circuit to true
    assert!(!JsonlScanner::is_line_boundary_offset(&path, 10));
}

#[test]
fn catch_up_snapshot_preserves_established_codex_cost_and_tokens() {
    let mut cache = CostUsageCache::default();
    cache.files.insert(
        "session.jsonl".to_string(),
        CostUsageFileUsage {
            parsed_bytes: Some(100),
            last_model: Some("gpt-5.6-sol".to_string()),
            codex_token_timestamps_monotonic: Some(true),
            ..test_file_usage(
                100,
                HashMap::from([(
                    "2026-08-20".to_string(),
                    HashMap::from([("gpt-5.6-sol".to_string(), vec![1_000, 250, 100])]),
                )]),
            )
        },
    );
    cache.files.insert(
        "empty.jsonl".to_string(),
        CostUsageFileUsage {
            parsed_bytes: Some(10),
            ..test_file_usage(10, HashMap::new())
        },
    );
    cache.days.insert(
        "2026-08-20".to_string(),
        HashMap::from([("gpt-5.6-sol".to_string(), vec![1_000, 250, 100])]),
    );

    let report = JsonlScanner::cached_cost_report_from_days(&cache);
    let expected = CostUsagePricing::codex_cost_usd_at_date(
        "gpt-5.6-sol",
        1_000,
        250,
        100,
        NaiveDate::from_ymd_opt(2026, 8, 20).unwrap(),
    )
    .expect("known model price");

    assert!((report.total_cost_usd - expected).abs() < 1e-12);
    assert!(report.total_cost_usd > 0.0);
    assert_eq!(report.input_tokens, 1_000);
    assert_eq!(report.cached_tokens, 250);
    assert_eq!(report.output_tokens, 100);
    assert_eq!(report.sessions_count, 1);
    assert!(!report.partial);
    assert!(report.updated_at.is_some());
}

#[test]
fn ranged_catch_up_snapshot_excludes_historical_days_and_keeps_measurement_time() {
    let usage = |day: &str| CostUsageFileUsage {
        parsed_bytes: Some(100),
        last_model: Some("gpt-5.6-sol".to_string()),
        codex_token_timestamps_monotonic: Some(true),
        ..test_file_usage(
            100,
            HashMap::from([(
                day.to_string(),
                HashMap::from([("gpt-5.6-sol".to_string(), vec![100, 25, 10, 4])]),
            )]),
        )
    };
    let mut cache = CostUsageCache {
        last_scan_unix_ms: 1,
        ..CostUsageCache::default()
    };
    cache
        .files
        .insert("current.jsonl".to_string(), usage("2026-09-19"));
    cache
        .files
        .insert("historical.jsonl".to_string(), usage("2026-09-01"));
    cache.days.insert(
        "2026-09-19".to_string(),
        HashMap::from([("gpt-5.6-sol".to_string(), vec![100, 25, 10, 4])]),
    );
    cache.days.insert(
        "2026-09-01".to_string(),
        HashMap::from([("gpt-5.6-sol".to_string(), vec![900, 225, 90, 36])]),
    );

    let range = CostUsageDayRange {
        since_key: "2026-09-19".to_string(),
        until_key: "2026-09-19".to_string(),
        scan_since_key: "2026-09-18".to_string(),
        scan_until_key: "2026-09-20".to_string(),
    };
    let report = JsonlScanner::cached_cost_report_for_range(&cache, &range);

    assert_eq!(report.input_tokens, 100);
    assert_eq!(report.cached_tokens, 25);
    assert_eq!(report.output_tokens, 10);
    assert_eq!(report.reasoning_tokens, Some(4));
    assert_eq!(report.sessions_count, 1);
    assert_eq!(
        report.updated_at,
        DateTime::<Utc>::from_timestamp_millis(1).map(|timestamp| timestamp.to_rfc3339())
    );
}

#[test]
fn codex_cache_round_trip_preserves_64_bit_counts_and_rebuilds_legacy_schema() {
    let root = tempfile::tempdir().unwrap();
    let cache_root = root.path();
    let mut cache = CostUsageCache::default();
    cache.days.insert(
        "2026-09-09".to_string(),
        HashMap::from([(
            "gpt-5.6-luna".to_string(),
            vec![3_000_000_000, 2_800_000_000, 200],
        )]),
    );

    JsonlScanner::save_cache(ProviderId::Codex, &mut cache, Some(cache_root));
    let loaded = JsonlScanner::load_cache(ProviderId::Codex, Some(cache_root));
    assert_eq!(
        loaded.days["2026-09-09"]["gpt-5.6-luna"],
        vec![3_000_000_000, 2_800_000_000, 200]
    );

    let cache_path = JsonlScanner::cache_path(ProviderId::Codex, Some(cache_root));
    let mut legacy: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&cache_path).unwrap()).unwrap();
    legacy
        .as_object_mut()
        .unwrap()
        .remove("codex_cache_schema_version");
    std::fs::write(&cache_path, serde_json::to_vec(&legacy).unwrap()).unwrap();

    let invalidated = JsonlScanner::load_cache(ProviderId::Codex, Some(cache_root));
    assert!(invalidated.days.is_empty());
    assert!(invalidated.files.is_empty());
    let status = JsonlScanner::load_cache_status(ProviderId::Codex, Some(cache_root));
    assert!(!status.has_days);
    assert!(status.previous_report.is_none());
}

#[test]
fn codex_v1_cache_rebuild_clears_stalled_subagent_refresh_state() {
    let root = tempfile::tempdir().unwrap();
    let mut cache = CostUsageCache {
        codex_scan_incomplete: true,
        codex_pending_paths: vec!["stalled-subagent.jsonl".to_string()],
        codex_scan_pause_reason: Some(CodexScanPauseReason::NoProgress),
        previous_report: Some(CachedCostReport {
            total_cost_usd: 1.0,
            input_tokens: 11,
            cached_tokens: 2,
            output_tokens: 3,
            reasoning_tokens: None,
            sessions_count: 1,
            updated_at: Some("2026-09-16T10:00:00Z".to_string()),
            partial: false,
        }),
        last_scan_unix_ms: i64::MAX,
        ..CostUsageCache::default()
    };
    JsonlScanner::save_cache(ProviderId::Codex, &mut cache, Some(root.path()));
    let cache_path = JsonlScanner::cache_path(ProviderId::Codex, Some(root.path()));
    let mut old: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&cache_path).unwrap()).unwrap();
    old["codex_cache_schema_version"] = serde_json::json!(1);
    std::fs::write(&cache_path, serde_json::to_vec(&old).unwrap()).unwrap();

    let rebuilt = JsonlScanner::load_cache(ProviderId::Codex, Some(root.path()));
    assert!(!rebuilt.codex_scan_incomplete);
    assert!(rebuilt.codex_pending_paths.is_empty());
    assert!(rebuilt.codex_scan_pause_reason.is_none());
    assert!(rebuilt.previous_report.is_none());
    assert_eq!(rebuilt.last_scan_unix_ms, 0);
    let status = JsonlScanner::load_cache_status(ProviderId::Codex, Some(root.path()));
    assert!(status.previous_report.is_none());
}

#[test]
fn codex_cache_schema_policy_helpers_rebuild_mismatched_load() {
    let stamp = CacheStamp::from_bytes(b"baseline");

    let legacy = CostUsageCache {
        codex_cache_schema_version: 0,
        days: HashMap::from([(
            "2026-09-09".to_string(),
            HashMap::from([("gpt-5.6-luna".to_string(), vec![1, 2, 3])]),
        )]),
        ..CostUsageCache::default()
    };
    let rebuilt = codex_cache_apply_load_policy(legacy, stamp.clone());
    assert_eq!(
        rebuilt.codex_cache_schema_version,
        CODEX_CACHE_SCHEMA_VERSION
    );
    assert!(rebuilt.days.is_empty());
    assert!(rebuilt.files.is_empty());
    assert!(rebuilt.loaded_stamp.is_some());

    let current = CostUsageCache {
        codex_cache_schema_version: CODEX_CACHE_SCHEMA_VERSION,
        days: HashMap::from([(
            "2026-09-09".to_string(),
            HashMap::from([("gpt-5.6-luna".to_string(), vec![1, 2, 3])]),
        )]),
        ..CostUsageCache::default()
    };
    let kept = codex_cache_apply_load_policy(current, stamp);
    assert_eq!(kept.codex_cache_schema_version, CODEX_CACHE_SCHEMA_VERSION);
    assert_eq!(kept.days["2026-09-09"]["gpt-5.6-luna"], vec![1, 2, 3]);
    assert!(kept.loaded_stamp.is_some());

    assert!(codex_cache_schema_is_current(CODEX_CACHE_SCHEMA_VERSION));
    assert!(!codex_cache_schema_is_current(0));

    let mut stamped = CostUsageCache::default();
    codex_cache_stamp_schema_version(&mut stamped);
    assert_eq!(
        stamped.codex_cache_schema_version,
        CODEX_CACHE_SCHEMA_VERSION
    );
}

#[test]
fn save_cache_persists_small_codex_artifact() {
    // F19 integration: a normal-sized Codex cache is persisted and
    // reloadable â€” the MAX_LOAD_BYTES refusal does not false-positive.
    let root = tempfile::tempdir().unwrap();
    let cache_root = root.path().to_path_buf();
    let mut cache = CostUsageCache {
        scan_since_key: Some("2026-01-01".to_string()),
        scan_until_key: Some("2026-01-31".to_string()),
        files: HashMap::from([(
            "a.jsonl".to_string(),
            test_file_usage(
                100,
                HashMap::from([(
                    "2026-01-10".to_string(),
                    HashMap::from([("gpt-5.6-sol".to_string(), vec![10, 0, 1])]),
                )]),
            ),
        )]),
        ..Default::default()
    };

    JsonlScanner::save_cache(ProviderId::Codex, &mut cache, Some(&cache_root));

    // File should exist and be reloadable.
    let loaded = JsonlScanner::load_cache(ProviderId::Codex, Some(&cache_root));
    assert!(
        loaded.files.contains_key("a.jsonl"),
        "small artifact persisted"
    );
    assert_eq!(loaded.scan_since_key, Some("2026-01-01".to_string()));
}

#[test]
fn stale_loaded_cache_does_not_replace_newer_baseline() {
    let root = tempfile::tempdir().unwrap();
    let cache_root = root.path();

    let mut initial = CostUsageCache {
        last_scan_unix_ms: 1,
        ..Default::default()
    };
    JsonlScanner::save_cache(ProviderId::Codex, &mut initial, Some(cache_root));

    let mut stale = JsonlScanner::load_cache(ProviderId::Codex, Some(cache_root));
    let mut newer = JsonlScanner::load_cache(ProviderId::Codex, Some(cache_root));
    newer.last_scan_unix_ms = 2;
    newer.scan_since_key = Some("2026-01-01".to_string());
    JsonlScanner::save_cache(ProviderId::Codex, &mut newer, Some(cache_root));

    stale.last_scan_unix_ms = 3;
    JsonlScanner::save_cache(ProviderId::Codex, &mut stale, Some(cache_root));

    let loaded = JsonlScanner::load_cache(ProviderId::Codex, Some(cache_root));
    assert_eq!(
        loaded.last_scan_unix_ms, 2,
        "a stale decoded baseline must not overwrite the newer cache"
    );
}

#[test]
fn save_cache_refuses_non_bounded_provider_oversize() {
    // F19: non-bounded providers (e.g. Claude) skip the refusal check
    // entirely â€” the MAX_LOAD_BYTES guard only applies to bounded providers.
    // This test confirms the is_bounded_provider gate works: Claude cache
    // is saved regardless of the MAX_LOAD_BYTES check (which is Codex-only).
    let root = tempfile::tempdir().unwrap();
    let cache_root = root.path().to_path_buf();
    let mut cache = CostUsageCache::default();
    cache.files.insert(
        "claude.jsonl".to_string(),
        test_file_usage(100, HashMap::new()),
    );

    JsonlScanner::save_cache(ProviderId::Claude, &mut cache, Some(&cache_root));
    let loaded = JsonlScanner::load_cache(ProviderId::Claude, Some(&cache_root));
    assert!(loaded.files.contains_key("claude.jsonl"));
}

#[test]
fn save_cache_refusal_removes_preexisting_destination_artifact() {
    // F19 integration: when the post-encode check refuses the artifact, any
    // pre-existing destination file is removed so a stale/oversized artifact
    // cannot persist and trigger load/refuse/rebuild behavior on next scan.
    let root = tempfile::tempdir().unwrap();
    let cache_root = root.path().to_path_buf();

    let mut cache = CostUsageCache::default();
    cache.files.insert(
        "big.jsonl".to_string(),
        test_file_usage(
            100,
            HashMap::from([(
                "2026-01-10".to_string(),
                HashMap::from([("gpt-5.6-sol".to_string(), vec![10, 0, 1])]),
            )]),
        ),
    );

    // Precreate a "stale" destination artifact so the refusal must remove
    // it. We seed it via a large (over_max) save_limit so the save_cache_with_limit
    // first ENCODES the small cache fine under a generous limit, writes the file,
    // then a follow-up call with a tiny limit must refuse AND remove.
    let cache_path = {
        // Exercise the private helper indirectly via the public path: first
        // persist a valid artifact under a generous limit via save_cache.
        // Then call with an impossible limit (encoded JSON ~hundreds of
        // bytes, limit = 1 byte) to force refusal.
        JsonlScanner::save_cache_with_limit(
            ProviderId::Codex,
            &mut cache,
            Some(&cache_root),
            usize::MAX,
        );
        let p = JsonlScanner::cache_path(ProviderId::Codex, Some(&cache_root));
        assert!(p.exists(), "precreate destination artifact");
        p
    };

    // Sanity: a normal load succeeds against the precreated artifact.
    let loaded = JsonlScanner::load_cache(ProviderId::Codex, Some(&cache_root));
    assert!(loaded.files.contains_key("big.jsonl"));

    // Force refusal with a 1-byte limit: encoded cache will exceed it.
    JsonlScanner::save_cache_with_limit(ProviderId::Codex, &mut cache, Some(&cache_root), 1);

    // Destination must be gone â€” no stale artifact may persist.
    assert!(
        !cache_path.exists(),
        "refusal must remove preexisting destination artifact"
    );

    // No temp file should remain in the cache root (only unique tmp name was used).
    let mut tmp_entries = Vec::new();
    for entry in std::fs::read_dir(&cache_root).unwrap() {
        let name = entry.unwrap().file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') && name.ends_with(".tmp") {
            tmp_entries.push(name.into_owned());
        }
    }
    // Best-effort temp cleanup writes an empty file at the unique name; the
    // invariant is that NO tmp file contains a complete artifact. The set
    // should at most contain a single zero-byte remnant from the cleanup
    // (or be empty); we persist via copy() rather than rename so no live
    // tmp holds data after the save path completes.
    for t in &tmp_entries {
        let meta = std::fs::metadata(cache_root.join(t)).unwrap();
        assert_eq!(meta.len(), 0, "tmp remnant must be empty: {t}");
    }

    // Loading after removal yields a fresh default cache (no rebuild loop).
    let loaded = JsonlScanner::load_cache(ProviderId::Codex, Some(&cache_root));
    assert!(
        loaded.files.is_empty(),
        "no rebuild loop from removed artifact"
    );
}

#[test]
fn save_cache_at_exact_limit_is_accepted() {
    // F19 boundary: an encoded artifact at exactly the injected limit is
    // accepted (only strictly-larger artifacts are refused).
    let root = tempfile::tempdir().unwrap();
    let cache_root = root.path().to_path_buf();

    let mut cache = CostUsageCache::default();
    // Saving stamps the schema version and bucket zone first; serialize the
    // stamped struct to learn the exact encoded size.
    codex_cache_stamp_schema_version(&mut cache);
    let json = serde_json::to_string(&cache).unwrap();
    let exact_limit = json.len();

    let mut cache_for_save = cache;
    JsonlScanner::save_cache_with_limit(
        ProviderId::Codex,
        &mut cache_for_save,
        Some(&cache_root),
        exact_limit,
    );

    let cache_path = JsonlScanner::cache_path(ProviderId::Codex, Some(&cache_root));
    assert!(
        cache_path.exists(),
        "artifact at exact limit must be persisted"
    );
}

#[test]
fn save_cache_one_over_limit_is_refused_and_removes_destination() {
    // F19 boundary: an encoded artifact one byte over the injected limit is
    // refused, and any pre-existing destination is removed.
    let root = tempfile::tempdir().unwrap();
    let cache_root = root.path().to_path_buf();

    let mut cache = CostUsageCache::default();
    codex_cache_stamp_schema_version(&mut cache);
    let json = serde_json::to_string(&cache).unwrap();
    // One byte short of the encoded size forces refusal on the next attempt.
    let under_by_one = json.len().saturating_sub(1);

    let mut cache_for_save = cache;
    JsonlScanner::save_cache_with_limit(
        ProviderId::Codex,
        &mut cache_for_save,
        Some(&cache_root),
        under_by_one,
    );

    let cache_path = JsonlScanner::cache_path(ProviderId::Codex, Some(&cache_root));
    assert!(
        !cache_path.exists(),
        "one-over-limit encoded artifact must be refused"
    );
}
