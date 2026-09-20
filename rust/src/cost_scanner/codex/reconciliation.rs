use super::*;

pub(super) fn paused_codex_summary(
    cache: &CostUsageCache,
    start_date: NaiveDate,
    today: NaiveDate,
    range: &CostUsageDayRange,
) -> CostSummary {
    if let Some(report) = codex_current_window_report(cache, range) {
        let mut summary = summary_from_cached_report(&report, start_date, today);
        summary.history_coverage_established = true;
        return summary;
    }

    let report = cache
        .previous_report
        .clone()
        .unwrap_or_else(|| JsonlScanner::cached_cost_report_from_days(cache));
    summary_from_cached_report(&report, start_date, today)
}

/// Whether incomplete Codex work is confined to history older than the
/// requested reporting window. This is deliberately conservative: an unknown
/// queue context, source error, malformed path, or unconsumed file tail keeps
/// the current window incomplete until a later scan proves it.
pub(super) fn codex_current_window_is_established(
    cache: &CostUsageCache,
    range: &CostUsageDayRange,
) -> bool {
    if !cache.codex_scan_incomplete {
        return true;
    }
    if cache.codex_pending_paths.is_empty()
        || matches!(
            cache.codex_scan_pause_reason,
            Some(CodexScanPauseReason::Error(_))
        )
    {
        return false;
    }

    let Some(pending_since) = cache.codex_pending_scan_since_key.as_deref() else {
        return false;
    };
    let Some(pending_until) = cache.codex_pending_scan_until_key.as_deref() else {
        return false;
    };
    if pending_since > range.scan_since_key.as_str()
        || pending_until < range.scan_until_key.as_str()
        || cache.codex_pending_scan_root_paths.is_empty()
        || cache.codex_pending_scan_timezone.is_none()
    {
        return false;
    }

    cache
        .codex_pending_paths
        .iter()
        .all(|path| !codex_pending_path_affects_current_window(cache, path, range))
}

/// Current-window publication requires positive decoded evidence. A missing
/// day cannot be treated as a validated zero while historical discovery is
/// still pending.
pub(super) fn codex_current_window_has_evidence(
    cache: &CostUsageCache,
    range: &CostUsageDayRange,
) -> bool {
    cache
        .days
        .keys()
        .any(|day| CostUsageDayRange::is_in_range(day, &range.since_key, &range.until_key))
}

pub(super) fn codex_current_window_report(
    cache: &CostUsageCache,
    range: &CostUsageDayRange,
) -> Option<CachedCostReport> {
    (codex_current_window_is_established(cache, range)
        && codex_current_window_has_evidence(cache, range))
    .then(|| JsonlScanner::cached_cost_report_for_range(cache, range))
}

fn codex_pending_path_affects_current_window(
    cache: &CostUsageCache,
    path_key: &str,
    range: &CostUsageDayRange,
) -> bool {
    let Some(usage) = cache.files.get(path_key) else {
        return true;
    };
    if usage.codex_unresolved_fork_parent || usage.days.is_empty() {
        return true;
    }
    if usage.days.keys().any(|day| {
        CostUsageDayRange::parse_day_key(day).is_none()
            || day >= &range.scan_since_key
            || CostUsageDayRange::is_in_range(day, &range.since_key, &range.until_key)
    }) {
        return true;
    }

    let Ok(metadata) = fs::metadata(path_key) else {
        return true;
    };
    #[allow(clippy::cast_possible_wrap, reason = "file sizes are clamped to i64")]
    let observed_size = metadata.len().min(i64::MAX as u64) as i64;
    if codex_logical_target_has_unconsumed_tail(observed_size, usage) {
        return true;
    }
    let identity_matches = match (
        usage.codex_file_identity.as_ref(),
        JsonlScanner::codex_file_identity(Path::new(path_key), &metadata).as_ref(),
    ) {
        (Some(expected), Some(actual)) => expected == actual,
        (Some(_), None) => false,
        (None, _) => true,
    };
    if !identity_matches
        || usage.mtime_unix_ms != system_time_to_unix_ms(metadata.modified().ok())
        || usage.size != observed_size
    {
        return true;
    }

    false
}

/// Return cached Codex files that are provably gone from the portion of the
/// sessions tree covered by this scan. Entries outside the current roots or
/// date directories are intentionally retained for a later scan.
pub(super) fn missing_codex_cache_paths(
    cache: &CostUsageCache,
    sessions_dirs: &[PathBuf],
    range: &CostUsageDayRange,
) -> Vec<String> {
    let scanned_date_dirs: Vec<PathBuf> = sessions_dirs
        .iter()
        .flat_map(|sessions_dir| {
            codex_scan_dates(range).into_iter().map(|date| {
                sessions_dir
                    .join(date.format("%Y").to_string())
                    .join(date.format("%m").to_string())
                    .join(date.format("%d").to_string())
            })
        })
        .collect();

    cache
        .files
        .keys()
        .filter(|path_key| {
            let path = Path::new(path_key.as_str());
            let in_scanned_root = sessions_dirs
                .iter()
                .any(|sessions_dir| path.starts_with(sessions_dir));
            let in_scanned_date = scanned_date_dirs
                .iter()
                .any(|date_dir| path.starts_with(date_dir));
            in_scanned_root && in_scanned_date && !path.exists()
        })
        .cloned()
        .collect()
}

/// Remove cached Codex files that are provably gone after an explicit refresh.
pub(super) fn reconcile_missing_codex_cache_files(
    cache: &mut CostUsageCache,
    sessions_dirs: &[PathBuf],
    range: &CostUsageDayRange,
) {
    for path in missing_codex_cache_paths(cache, sessions_dirs, range) {
        cache.files.remove(&path);
    }
    cache
        .codex_pending_paths
        .retain(|path| Path::new(path).exists());
}

/// Whether the cache contains a path that depends on this source partition.
/// Missing optional roots are normal; only a root/date partition that has
/// previously contributed a cached or queued path can make discovery
/// incomplete.
pub(super) fn cache_has_codex_path_under(cache: &CostUsageCache, parent: &Path) -> bool {
    cache
        .files
        .keys()
        .chain(cache.codex_pending_paths.iter())
        .any(|path| Path::new(path).starts_with(parent))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usage(day: &str, input: i64) -> CostUsageFileUsage {
        CostUsageFileUsage {
            mtime_unix_ms: 0,
            size: 100,
            codex_file_identity: None,
            days: HashMap::from([(
                day.to_string(),
                HashMap::from([("gpt-5.6-sol".to_string(), vec![input, 0, 10])]),
            )]),
            parsed_bytes: Some(100),
            codex_scan_target_size: None,
            last_model: Some("gpt-5.6-sol".to_string()),
            last_totals: None,
            codex_token_timestamps_monotonic: Some(true),
            codex_last_token_timestamp: None,
            codex_session_id: None,
            codex_forked_from_id: None,
            codex_lineage: CodexSessionLineage::Root,
            codex_fork_timestamp: None,
            codex_unresolved_fork_parent: false,
        }
    }

    fn historical_pending_cache(old_path: &str, current_path: &str) -> CostUsageCache {
        CostUsageCache {
            last_scan_unix_ms: 1,
            files: HashMap::from([
                (old_path.to_string(), usage("2026-09-01", 900)),
                (current_path.to_string(), usage("2026-09-19", 100)),
            ]),
            days: HashMap::from([
                (
                    "2026-09-01".to_string(),
                    HashMap::from([("gpt-5.6-sol".to_string(), vec![900, 0, 10])]),
                ),
                (
                    "2026-09-19".to_string(),
                    HashMap::from([("gpt-5.6-sol".to_string(), vec![100, 0, 10])]),
                ),
            ]),
            codex_pending_paths: vec![old_path.to_string()],
            codex_scan_incomplete: true,
            codex_pending_scan_since_key: Some("2026-09-01".to_string()),
            codex_pending_scan_until_key: Some("2026-09-20".to_string()),
            codex_pending_scan_root_paths: vec!["C:\\sessions".to_string()],
            codex_pending_scan_timezone: Some("UTC".to_string()),
            ..CostUsageCache::default()
        }
    }

    fn active_range() -> CostUsageDayRange {
        CostUsageDayRange {
            since_key: "2026-09-19".to_string(),
            until_key: "2026-09-19".to_string(),
            scan_since_key: "2026-09-18".to_string(),
            scan_until_key: "2026-09-20".to_string(),
        }
    }

    #[test]
    fn historical_pending_work_keeps_current_window_publishable() {
        let root = tempfile::tempdir().unwrap();
        let old_path = root.path().join("old.jsonl");
        let current_path = root.path().join("current.jsonl");
        std::fs::write(&old_path, vec![0_u8; 100]).unwrap();
        std::fs::write(&current_path, vec![0_u8; 100]).unwrap();
        let old_key = old_path.to_string_lossy().into_owned();
        let current_key = current_path.to_string_lossy().into_owned();
        let mut cache = historical_pending_cache(&old_key, &current_key);
        let metadata = std::fs::metadata(&old_path).unwrap();
        let old_usage = cache.files.get_mut(&old_key).unwrap();
        old_usage.mtime_unix_ms = system_time_to_unix_ms(metadata.modified().ok());
        old_usage.size = i64::try_from(metadata.len()).unwrap();
        let range = active_range();

        assert!(codex_current_window_is_established(&cache, &range));
        assert!(codex_current_window_has_evidence(&cache, &range));
        let report = codex_current_window_report(&cache, &range).unwrap();
        assert_eq!(report.input_tokens, 100);
        assert_eq!(report.sessions_count, 1);
    }

    #[test]
    fn metadata_failure_blocks_historical_pending_publication() {
        let old_path = r"C:\sessions\missing-old.jsonl";
        let current_path = r"C:\sessions\current.jsonl";
        let cache = historical_pending_cache(old_path, current_path);
        let range = active_range();

        assert!(!codex_current_window_is_established(&cache, &range));
        assert!(codex_current_window_report(&cache, &range).is_none());
    }

    #[test]
    fn current_pending_work_and_source_errors_block_publication() {
        let old_path = r"C:\sessions\old.jsonl";
        let current_path = r"C:\sessions\current.jsonl";
        let range = active_range();

        let mut current_pending = historical_pending_cache(old_path, current_path);
        current_pending.codex_pending_paths = vec![current_path.to_string()];
        assert!(!codex_current_window_is_established(
            &current_pending,
            &range
        ));

        let mut source_error = historical_pending_cache(old_path, current_path);
        source_error.codex_scan_pause_reason = Some(CodexScanPauseReason::Error(
            "source unavailable".to_string(),
        ));
        assert!(!codex_current_window_is_established(&source_error, &range));
        assert!(codex_current_window_report(&source_error, &range).is_none());
    }
}
