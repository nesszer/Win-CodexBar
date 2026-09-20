use super::cache_days::days_from_codex_source_rows;
use super::*;
use crate::core::{
    CodexSourceRowCache, CodexSourceUsageRow, read_source_rows, recover_rows, row_cache,
    row_cache_matches, row_cache_needs_recovery,
};

/// A complete, non-forked file whose source rows may be (re)priced this pass.
struct CodexSourceRowPlan {
    metadata: fs::Metadata,
    source_rows: Vec<CodexSourceUsageRow>,
    recoverable_cache: Option<CodexSourceRowCache>,
}

/// Decide whether a completed file qualifies for source-row evidence, and
/// whether its cached pricing needs recovery. `None` skips the file entirely:
/// the pass was partial, the file keeps an unconsumed tail, or the usage is
/// fork/parent-baseline shaped (whose source rows are not trusted here).
fn codex_source_row_plan(
    cache: &CostUsageCache,
    path: &Path,
    scan_range: &CostUsageDayRange,
) -> Option<CodexSourceRowPlan> {
    let usage_safe = cache
        .files
        .get(&path.to_string_lossy().to_string())
        .is_some_and(|usage| {
            usage.codex_forked_from_id.is_none()
                && !usage.codex_lineage.uses_parent_baseline()
                && !usage.codex_unresolved_fork_parent
        });
    if !usage_safe {
        return None;
    }
    let metadata = fs::metadata(path).ok()?;
    let source_rows = read_source_rows(path, scan_range).ok()?;
    let recoverable_cache = cache
        .codex_source_rows
        .get(&path.to_string_lossy().to_string())
        .filter(|cached| !cached.rows.is_empty() && row_cache_matches(path, &metadata, cached))
        .cloned();
    Some(CodexSourceRowPlan {
        metadata,
        source_rows,
        recoverable_cache,
    })
}

/// Apply a source-row plan: recover cached pricing when required, retain the
/// resulting rows as evidence, and unprice the file's day map when recovery
/// left appended rows unvalidated.
fn apply_codex_source_row_plan(cache: &mut CostUsageCache, key: &str, plan: CodexSourceRowPlan) {
    let CodexSourceRowPlan {
        metadata,
        source_rows,
        recoverable_cache,
    } = plan;
    let requires_recovery = recoverable_cache
        .as_ref()
        .is_some_and(|cached| row_cache_needs_recovery(cached, &source_rows));
    let rows = if requires_recovery {
        let cached = recoverable_cache.expect("recovery cache checked above");
        recover_rows(&cached.rows, &source_rows, cached.size)
    } else {
        source_rows
    };
    let Some(source_cache) = row_cache(&key_path(key), &metadata, rows) else {
        return;
    };
    if requires_recovery && let Some(usage) = cache.files.get_mut(key) {
        // Rebuild from recovered rows even when a new row is intentionally
        // unresolved. Leaving the normal parser's model-priced days in place
        // would silently price an appended row that has no validated
        // historical evidence.
        usage.days = days_from_codex_source_rows(&source_cache.rows);
    }
    cache
        .codex_source_rows
        .insert(key.to_string(), source_cache);
}

fn key_path(key: &str) -> PathBuf {
    PathBuf::from(key)
}

pub(super) fn scan_codex_detailed_with_cache(
    scanner: &CostScanner,
    cancel: Option<&AtomicBool>,
) -> (CostSummary, CostScanStats, CostUsageCache) {
    let mut summary = CostSummary::default();
    let mut stats = CostScanStats::default();
    let today = Local::now().date_naive();
    let start_date = codex_period_start(today, scanner.days);
    let range = CostUsageDayRange::new(start_date, today);
    let now_ms = unix_now_ms();

    summary.period_start = Some(start_date);
    summary.period_end = Some(today);

    let cache_root = scanner.cache_root.as_deref();
    let mut cache = JsonlScanner::load_cache(ProviderId::Codex, cache_root);
    let sessions_dirs = scanner.get_codex_sessions_dirs();
    let pending_scan = CodexPendingScanContext::new(
        &cache,
        &range,
        &sessions_dirs,
        scanner.options.is_app_driven(),
    );

    // A no-progress or source-error catch-up is terminal for background
    // synchronization. Keep the resumable queue and last validated report
    // intact until the user explicitly requests an app-driven refresh.
    if matches!(
        CodexPendingScanDisposition::before_scan(&cache, scanner.options.is_app_driven()),
        CodexPendingScanDisposition::PreservePause
    ) {
        let mut summary = paused_codex_summary(&cache, start_date, today, &range);
        append_pi_compatible_costs(scanner, &mut summary, cancel);
        summary.history_coverage_established =
            summary.history_coverage_established && !is_cancelled(cancel);
        summary.known_zero = summary.history_coverage_established && summary.sessions_count == 0;
        return (summary, stats, cache);
    }
    if scanner.options.is_app_driven() || pending_scan.is_incompatible {
        cache.codex_scan_pause_reason = None;
    }

    let scan_range = &pending_scan.scan_range;

    // Debounce: rebuild from disk cache without re-walking session files.
    if JsonlScanner::should_skip_cached_scan(&cache, scanner.options, now_ms)
        && !cache.codex_scan_incomplete
        && JsonlScanner::cache_covers_range(&cache, &range)
        && (!cache.days.is_empty() || !cache.files.is_empty())
    {
        stats.used_cache_debounce = true;
        // A16 (upstream 0.48.0): cache hit within debounce = coverage established
        // when the cache has data and no catch-up is pending. Final publication
        // also waits for the cancellable Pi/OMP scan below.
        let cached_history_coverage_established = !cache.codex_scan_incomplete
            && cache.previous_report.is_none()
            && JsonlScanner::cache_covers_range(&cache, &range);
        let (cost, _) = add_codex_days_map_to_summary(&mut summary, &cache.days, &range);
        summary.total_cost_usd += cost;
        // Session count is a display field; the cache holds far fewer files than u32::MAX.
        #[allow(clippy::cast_possible_truncation, reason = "cache file counts fit u32")]
        let sessions_count = cache
            .files
            .values()
            .filter(|usage| {
                usage.days.keys().any(|day| {
                    CostUsageDayRange::is_in_range(day, &range.since_key, &range.until_key)
                })
            })
            .count() as u32;
        summary.sessions_count = sessions_count;

        // Pi-compatible sessions are outside the Codex JSONL cache.
        // Skip when tests inject sessions roots — avoid scanning the real home tree.
        append_pi_compatible_costs(scanner, &mut summary, cancel);
        summary.history_coverage_established =
            cached_history_coverage_established && !is_cancelled(cancel);
        // Upstream 0.50.1 #2932: debounce cache hit with coverage
        // established but zero sessions in-range is a known-zero.
        summary.known_zero = summary.history_coverage_established && summary.sessions_count == 0;
        return (summary, stats, cache);
    }

    let established_report_before_scan = (!cache.codex_scan_incomplete
        && cache.previous_report.is_none()
        && (cache.scan_since_key.is_some() || !cache.days.is_empty() || !cache.files.is_empty()))
    .then(|| JsonlScanner::cached_cost_report_for_range(&cache, &range));

    // Persist the source-bound work range before doing bounded work.
    cache.codex_pending_scan_since_key = Some(scan_range.scan_since_key.clone());
    cache.codex_pending_scan_until_key = Some(scan_range.scan_until_key.clone());
    cache.codex_pending_scan_root_paths = pending_scan.root_paths.clone();
    cache.codex_pending_scan_timezone = Some(pending_scan.timezone.clone());

    let (mut candidates, discovery_complete) =
        scanner.collect_codex_candidates(&sessions_dirs, scan_range, &cache, cancel, &mut stats);
    let candidate_limit = if scanner.options.codex_candidate_limit == 0 {
        usize::MAX
    } else {
        scanner.options.codex_candidate_limit
    };
    let refresh_byte_limit = if scanner.options.codex_max_scan_bytes_per_refresh <= 0 {
        i64::MAX
    } else {
        scanner.options.codex_max_scan_bytes_per_refresh
    };
    let per_file_limit = if scanner.options.codex_max_session_file_bytes <= 0 {
        i64::MAX
    } else {
        scanner.options.codex_max_session_file_bytes
    };
    let mut bytes_read_this_refresh = 0_i64;
    let mut pending_next = cache.codex_pending_paths.clone();
    let pending_paths_before_pass = cache.codex_pending_paths.clone();
    prioritize_codex_pending_candidates(&mut candidates, &pending_paths_before_pass);
    if discovery_complete && !is_cancelled(cancel) {
        pending_next
            .retain(|path| !cached_codex_file_is_complete_for_range(&cache, path, scan_range));
    }

    let mut incomplete_processed = Vec::new();
    for (index, candidate) in candidates.iter().enumerate() {
        if is_cancelled(cancel)
            || index >= candidate_limit
            || bytes_read_this_refresh >= refresh_byte_limit
        {
            for deferred in &candidates[index..] {
                let key = deferred.path.to_string_lossy().to_string();
                if !pending_next.contains(&key) {
                    pending_next.push(key);
                }
            }
            stats.files_deferred = stats.files_deferred.saturating_add(
                u32::try_from((candidates.len() - index).min(u32::MAX as usize))
                    .unwrap_or(u32::MAX),
            );
            break;
        }

        let refresh_remaining = refresh_byte_limit.saturating_sub(bytes_read_this_refresh);
        let allowance = per_file_limit.min(refresh_remaining);
        if allowance <= 0 {
            for deferred in &candidates[index..] {
                let key = deferred.path.to_string_lossy().to_string();
                if !pending_next.contains(&key) {
                    pending_next.push(key);
                }
            }
            stats.files_deferred = stats.files_deferred.saturating_add(
                u32::try_from((candidates.len() - index).min(u32::MAX as usize))
                    .unwrap_or(u32::MAX),
            );
            break;
        }

        let outcome = scanner.parse_codex_file_bounded(
            &candidate.path,
            scan_range,
            &mut summary,
            &mut cache,
            cancel,
            &mut stats,
            Some(allowance),
        );
        bytes_read_this_refresh = bytes_read_this_refresh.saturating_add(outcome.bytes_read.max(0));
        stats.codex_bytes_read = stats
            .codex_bytes_read
            .saturating_add(u64::try_from(outcome.bytes_read.max(0)).unwrap_or(u64::MAX));
        let key = candidate.path.to_string_lossy().to_string();
        pending_next.retain(|pending| pending != &key);
        let observed_size = fs::metadata(&candidate.path)
            .ok()
            .map(|metadata| {
                #[allow(
                    clippy::cast_possible_wrap,
                    reason = "file sizes are clamped to i64::MAX"
                )]
                let size = metadata.len().min(i64::MAX as u64) as i64;
                size
            })
            .unwrap_or(0);
        let has_unconsumed_tail = cache
            .files
            .get(&key)
            .is_some_and(|usage| codex_logical_target_has_unconsumed_tail(observed_size, usage));
        if !outcome.is_complete || has_unconsumed_tail {
            incomplete_processed.push(key);
            stats.files_deferred = stats.files_deferred.saturating_add(1);
        } else if let Some(plan) = codex_source_row_plan(&cache, &candidate.path, scan_range) {
            apply_codex_source_row_plan(&mut cache, &key, plan);
        }
    }
    pending_next.extend(incomplete_processed);

    let mut pruned_paths_pending = Vec::new();
    if discovery_complete && !is_cancelled(cancel) {
        pruned_paths_pending = missing_codex_cache_paths(&cache, &sessions_dirs, scan_range);
        if scanner.options.is_app_driven() {
            reconcile_missing_codex_cache_files(&mut cache, &sessions_dirs, scan_range);
            for path in &pending_paths_before_pass {
                if !Path::new(path).exists() {
                    cache.files.remove(path);
                }
            }
        } else {
            for path in &pruned_paths_pending {
                if !pending_next.contains(path) {
                    pending_next.push(path.clone());
                }
            }
        }
    }
    pending_next.retain(|path| {
        // An incomplete discovery is a source failure, not proof that a
        // queued path was pruned. Preserve the priority cursor verbatim so
        // the next explicit refresh can validate the source and resume it.
        if !discovery_complete
            || is_cancelled(cancel)
            || (!scanner.options.is_app_driven()
                && pruned_paths_pending.iter().any(|pending| pending == path))
        {
            return true;
        }
        Path::new(path).exists()
            && is_codex_path_in_scan_window(Path::new(path), &sessions_dirs, scan_range)
    });
    if discovery_complete
        && !is_cancelled(cancel)
        && (pruned_paths_pending.is_empty() || scanner.options.is_app_driven())
    {
        pending_next.sort();
        pending_next.dedup();
    } else {
        // Preserve queue order while the source is unavailable or the
        // pass is cancelled; this is the durable priority cursor.
        let mut seen_pending = HashSet::new();
        pending_next.retain(|path| seen_pending.insert(path.clone()));
    }
    cache.codex_pending_paths = pending_next;
    cache.codex_scan_incomplete =
        !discovery_complete || is_cancelled(cancel) || !cache.codex_pending_paths.is_empty();
    let pending_disposition = CodexPendingScanDisposition::after_scan(
        &cache,
        discovery_complete,
        is_cancelled(cancel),
        !pruned_paths_pending.is_empty(),
        bytes_read_this_refresh,
    );
    rebuild_cache_days(&mut cache);
    cache.last_scan_unix_ms = now_ms;
    if cache.codex_scan_incomplete {
        if pending_disposition.keeps_live_rows() {
            // Preserve live rows only when there is no validated report to
            // protect. v0.60.5 keeps the prior report for unresolved current
            // work; the current window is not authoritative until the fork
            // dependency is resolved.
            // The healthy files in this range are now validated. Keep the
            // incomplete flag as the coverage marker while acknowledging
            // the range so unchanged files stay on the cache fast path.
            cache.scan_since_key = Some(scan_range.scan_since_key.clone());
            cache.scan_until_key = Some(scan_range.scan_until_key.clone());
        } else if cache.previous_report.is_none() {
            cache.previous_report = established_report_before_scan;
        }
        if !is_cancelled(cancel) {
            cache.codex_scan_pause_reason = if !discovery_complete {
                Some(CodexScanPauseReason::Error(
                    "Codex session source unavailable".to_string(),
                ))
            } else {
                pending_disposition.pause_reason()
            };
        }
    } else {
        cache.scan_since_key = Some(scan_range.scan_since_key.clone());
        cache.scan_until_key = Some(scan_range.scan_until_key.clone());
        cache.codex_pending_scan_since_key = None;
        cache.codex_pending_scan_until_key = None;
        cache.codex_pending_scan_root_paths.clear();
        cache.codex_pending_scan_timezone = None;
        cache.previous_report = None;
        cache.codex_scan_pause_reason = None;
    }
    JsonlScanner::save_cache(ProviderId::Codex, &mut cache, cache_root);

    // Build the current native summary from the complete decoded cache view,
    // including prior cached files that were not reread in this bounded pass.
    // A cancelled pass retains missing rows on disk for deletion
    // reconciliation, but must not publish those stale rows in its summary.
    let mut summary_cache = cache.clone();
    if is_cancelled(cancel) {
        summary_cache
            .files
            .retain(|path, _| Path::new(path).exists());
        rebuild_cache_days(&mut summary_cache);
    }
    let mut rebuilt = CostSummary {
        period_start: Some(start_date),
        period_end: Some(today),
        ..CostSummary::default()
    };
    let (native_cost, _) = add_codex_days_map_to_summary(&mut rebuilt, &summary_cache.days, &range);
    rebuilt.total_cost_usd += native_cost;
    #[allow(clippy::cast_possible_truncation, reason = "cache file counts fit u32")]
    {
        rebuilt.sessions_count = summary_cache
            .files
            .values()
            .filter(|usage| {
                usage.days.keys().any(|day| {
                    CostUsageDayRange::is_in_range(day, &range.since_key, &range.until_key)
                })
            })
            .count() as u32;
    }
    let cancelled_with_missing_cache_rows =
        is_cancelled(cancel) && cache.files.keys().any(|path| !Path::new(path).exists());
    let preserving_previous_report = cache.codex_scan_incomplete
        && cache.previous_report.is_some()
        && !cancelled_with_missing_cache_rows;
    let current_window_report = (cache.codex_scan_incomplete && !is_cancelled(cancel))
        .then(|| codex_current_window_report(&cache, &range))
        .flatten();
    let published_current_window = current_window_report.is_some();
    summary = if let Some(report) = current_window_report {
        let mut summary = summary_from_cached_report(&report, start_date, today);
        summary.history_coverage_established = true;
        summary
    } else if preserving_previous_report {
        cache
            .previous_report
            .as_ref()
            .map(|report| summary_from_cached_report(report, start_date, today))
            .unwrap_or(rebuilt)
    } else {
        rebuilt
    };

    // OMP / pi-compatible agent sessions (upstream #2269). Dedup by entry id.
    // Skip when tests inject sessions roots — avoid scanning the real home tree.
    // A16 --provider-native-only: skip pi/OMP mirrors when disabled.
    append_pi_compatible_costs(scanner, &mut summary, cancel);

    // v0.56.1 #3279: only publish authoritative coverage after all
    // cancellable scan work, including Pi/OMP, has completed. Persistence
    // pruning may retain `previous_report`, but that must not make a
    // completed in-memory scan stale or make a cancelled partial scan look
    // complete.
    summary.history_coverage_established =
        !is_cancelled(cancel) && (!cache.codex_scan_incomplete || published_current_window);
    // Upstream 0.50.1 #2932: a completed scan with zero results is a
    // *known* zero. An incomplete scan may publish only a non-empty validated
    // current window; it must not infer zero from historical metadata alone.
    summary.known_zero = summary.history_coverage_established
        && summary.sessions_count == 0
        && (!cache.codex_scan_incomplete || published_current_window);

    (summary, stats, cache)
}

fn append_pi_compatible_costs(
    scanner: &CostScanner,
    summary: &mut CostSummary,
    cancel: Option<&AtomicBool>,
) {
    if !scanner.options.include_pi_sessions || scanner.sessions_dirs_override.is_some() {
        return;
    }

    let mut seen_pi = HashSet::new();
    crate::pi_session_cost::scan_pi_compatible_into(
        summary,
        crate::pi_session_cost::PiMappedProvider::Codex,
        scanner.days,
        cancel,
        &mut seen_pi,
    );
}
