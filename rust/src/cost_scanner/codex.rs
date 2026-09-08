use super::*;

mod logical_target;
mod pending_range;
mod reconciliation;
use logical_target::*;
use pending_range::{CodexPendingScanContext, codex_cache_has_validated_state};
use reconciliation::*;

fn rebuild_cache_days(cache: &mut CostUsageCache) {
    cache.days.clear();
    for usage in cache.files.values() {
        for (day, models) in &usage.days {
            let day_entry = cache.days.entry(day.clone()).or_default();
            for (model, packed) in models {
                let dest = day_entry
                    .entry(model.clone())
                    .or_insert_with(|| vec![0, 0, 0]);
                if dest.len() < 3 {
                    dest.resize(3, 0);
                }

                let had_core_tokens = dest[0] != 0 || dest[1] != 0 || dest[2] != 0;
                let source_input = packed.first().copied().unwrap_or(0);
                let source_cached = packed.get(1).copied().unwrap_or(0);
                let source_output = packed.get(2).copied().unwrap_or(0);
                let source_has_tokens =
                    source_input != 0 || source_cached != 0 || source_output != 0;
                let source_reasoning = packed
                    .get(3)
                    .copied()
                    .map(|reasoning| reasoning.max(0).min(source_output.max(0)));

                dest[0] = dest[0].saturating_add(source_input);
                dest[1] = dest[1].saturating_add(source_cached);
                dest[2] = dest[2].saturating_add(source_output);

                if !source_has_tokens {
                    continue;
                }

                if !had_core_tokens {
                    match source_reasoning {
                        Some(reasoning) => {
                            if dest.len() >= 4 {
                                dest[3] = reasoning.min(dest[2].max(0));
                            } else {
                                dest.push(reasoning.min(dest[2].max(0)));
                            }
                        }
                        None => dest.truncate(3),
                    }
                    continue;
                }

                match (dest.get(3).copied(), source_reasoning) {
                    (Some(previous), Some(reasoning)) => {
                        let merged = previous.saturating_add(reasoning).min(dest[2].max(0));
                        dest[3] = merged;
                    }
                    _ => dest.truncate(3),
                }
            }
        }
    }
}

fn summary_from_cached_report(
    report: &CachedCostReport,
    period_start: NaiveDate,
    period_end: NaiveDate,
) -> CostSummary {
    CostSummary {
        total_cost_usd: report.total_cost_usd,
        input_tokens: u64::try_from(report.input_tokens.max(0)).unwrap_or(0),
        cached_tokens: u64::try_from(report.cached_tokens.max(0)).unwrap_or(0),
        output_tokens: u64::try_from(report.output_tokens.max(0)).unwrap_or(0),
        reasoning_tokens: report
            .reasoning_tokens
            .map(|reasoning| u64::try_from(reasoning.max(0)).unwrap_or(0)),
        sessions_count: u32::try_from(report.sessions_count.max(0)).unwrap_or(0),
        // The persisted report has no model-level breakdown. A catch-up
        // summary must not claim that its newly rebuilt partial breakdown is
        // complete, even when the validated report itself was fully priced.
        model_pricing_completeness: ModelPricingCompleteness::Partial {
            unpriced_models: Vec::new(),
        },
        history_coverage_established: false,
        known_zero: false,
        period_start: Some(period_start),
        period_end: Some(period_end),
        ..CostSummary::default()
    }
}

fn codex_fork_parent_is_safe(cache: &CostUsageCache, usage: &CostUsageFileUsage) -> bool {
    usage.codex_forked_from_id.as_deref().is_none()
        || codex_parent_baseline(
            cache,
            usage.codex_forked_from_id.as_deref().unwrap_or_default(),
            usage.codex_fork_timestamp.as_deref(),
        )
        .is_some()
}

/// Return a parent cumulative baseline only when exactly one cached session
/// identity is current, complete, timestamp-ordered, and safe to trust.
fn codex_parent_baseline(
    cache: &CostUsageCache,
    parent_session_id: &str,
    child_fork_timestamp: Option<&str>,
) -> Option<crate::core::CodexTotals> {
    let mut baseline = None;
    for (path_key, usage) in &cache.files {
        if usage.codex_session_id.as_deref() != Some(parent_session_id) {
            continue;
        }
        if usage.codex_unresolved_fork_parent
            || usage.codex_token_timestamps_monotonic != Some(true)
        {
            return None;
        }
        let metadata = fs::metadata(path_key).ok()?;
        #[allow(clippy::cast_possible_wrap, reason = "session file sizes fit i64")]
        let size = metadata.len().min(i64::MAX as u64) as i64;
        if usage.mtime_unix_ms != system_time_to_unix_ms(metadata.modified().ok())
            || usage.size != size
            || usage.parsed_bytes.unwrap_or(0) < size
        {
            return None;
        }
        let last_totals = usage.last_totals.clone()?;
        let last_token_timestamp = usage.codex_last_token_timestamp.as_deref()?;
        let child_fork_timestamp = child_fork_timestamp?;
        if !JsonlScanner::codex_timestamp_at_or_before(last_token_timestamp, child_fork_timestamp) {
            return None;
        }
        if baseline.replace(last_totals).is_some() {
            // Duplicate identities make the dependency ambiguous.
            return None;
        }
    }
    baseline
}

fn is_codex_path_in_scan_window(
    path: &Path,
    sessions_dirs: &[PathBuf],
    range: &CostUsageDayRange,
) -> bool {
    sessions_dirs.iter().any(|sessions_dir| {
        codex_scan_dates(range).into_iter().any(|date| {
            let date_dir = sessions_dir
                .join(date.format("%Y").to_string())
                .join(date.format("%m").to_string())
                .join(date.format("%d").to_string());
            path.starts_with(date_dir)
        })
    })
}

/// Claude cost calculation for the usage scanner.
///
/// Per-token rates come from the canonical `CostUsagePricing::claude_cost_usd`
/// table (the single source of truth for Claude pricing). The only
/// scanner-specific piece is the one-hour cache-write premium, which the
/// canonical cost function doesn't model: one-hour cache writes bill at 2x the
struct CodexScanCandidate {
    path: PathBuf,
    mtime_unix_ms: i64,
}

#[derive(Debug, Clone, Copy, Default)]
struct CodexFileScanOutcome {
    bytes_read: i64,
    is_complete: bool,
}

/// Cost usage scanner
impl CostScanner {
    pub fn scan_codex(&self) -> CostSummary {
        self.scan_codex_with_cancel(None)
    }

    /// Scan Codex local logs, stopping early when the caller cancels the scan.
    pub fn scan_codex_with_cancel(&self, cancel: Option<&AtomicBool>) -> CostSummary {
        self.scan_codex_detailed(cancel).0
    }

    /// Scan Codex and return cache/resume stats alongside the summary.
    pub fn scan_codex_detailed(&self, cancel: Option<&AtomicBool>) -> (CostSummary, CostScanStats) {
        let (summary, stats, _cache) = self.scan_codex_detailed_with_cache(cancel);
        (summary, stats)
    }

    /// Scan Codex and retain the decoded cache baseline for same-cycle readers.
    ///
    /// The returned cache is the exact in-memory value used for publication,
    /// including any persistence-budget pruning.  Callers that only need the
    /// summary should use [`Self::scan_codex_detailed`]; daily history readers
    /// use this seam to avoid decoding the same native cache a second time.
    pub(crate) fn scan_codex_detailed_with_cache(
        &self,
        cancel: Option<&AtomicBool>,
    ) -> (CostSummary, CostScanStats, CostUsageCache) {
        let mut summary = CostSummary::default();
        let mut stats = CostScanStats::default();
        let today = Local::now().date_naive();
        let start_date = codex_period_start(today, self.days);
        let range = CostUsageDayRange::new(start_date, today);
        let now_ms = unix_now_ms();

        summary.period_start = Some(start_date);
        summary.period_end = Some(today);

        let cache_root = self.cache_root.as_deref();
        let mut cache = JsonlScanner::load_cache(ProviderId::Codex, cache_root);
        let sessions_dirs = self.get_codex_sessions_dirs();
        let pending_scan = CodexPendingScanContext::new(
            &cache,
            &range,
            &sessions_dirs,
            self.options.is_app_driven(),
        );

        // A no-progress or source-error catch-up is terminal for background
        // synchronization. Keep the resumable queue and last validated report
        // intact until the user explicitly requests an app-driven refresh.
        if pending_scan.should_preserve_pause(&cache, self.options.is_app_driven()) {
            return (
                paused_codex_summary(&cache, start_date, today),
                stats,
                cache,
            );
        }
        if self.options.is_app_driven() || pending_scan.is_incompatible {
            cache.codex_scan_pause_reason = None;
        }

        let scan_range = &pending_scan.scan_range;

        // Debounce: rebuild from disk cache without re-walking session files.
        if JsonlScanner::should_skip_cached_scan(&cache, self.options, now_ms)
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
            if self.sessions_dirs_override.is_none() {
                let mut seen_pi = HashSet::new();
                crate::pi_session_cost::scan_pi_compatible_into(
                    &mut summary,
                    crate::pi_session_cost::PiMappedProvider::Codex,
                    self.days,
                    cancel,
                    &mut seen_pi,
                );
            }
            summary.history_coverage_established =
                cached_history_coverage_established && !is_cancelled(cancel);
            // Upstream 0.50.1 #2932: debounce cache hit with coverage
            // established but zero sessions in-range is a known-zero.
            summary.known_zero =
                summary.history_coverage_established && summary.sessions_count == 0;
            return (summary, stats, cache);
        }

        let established_report_before_scan = (!cache.codex_scan_incomplete
            && cache.previous_report.is_none()
            && (cache.scan_since_key.is_some()
                || !cache.days.is_empty()
                || !cache.files.is_empty()))
        .then(|| JsonlScanner::cached_cost_report_from_days(&cache));

        // Persist the source-bound work range before doing bounded work.
        cache.codex_pending_scan_since_key = Some(scan_range.scan_since_key.clone());
        cache.codex_pending_scan_until_key = Some(scan_range.scan_until_key.clone());
        cache.codex_pending_scan_root_paths = pending_scan.root_paths.clone();
        cache.codex_pending_scan_timezone = Some(pending_scan.timezone.clone());

        let (mut candidates, discovery_complete) =
            self.collect_codex_candidates(&sessions_dirs, scan_range, &cache, cancel, &mut stats);
        let candidate_limit = if self.options.codex_candidate_limit == 0 {
            usize::MAX
        } else {
            self.options.codex_candidate_limit
        };
        let refresh_byte_limit = if self.options.codex_max_scan_bytes_per_refresh <= 0 {
            i64::MAX
        } else {
            self.options.codex_max_scan_bytes_per_refresh
        };
        let per_file_limit = if self.options.codex_max_session_file_bytes <= 0 {
            i64::MAX
        } else {
            self.options.codex_max_session_file_bytes
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

            let outcome = self.parse_codex_file_bounded(
                &candidate.path,
                scan_range,
                &mut summary,
                &mut cache,
                cancel,
                &mut stats,
                Some(allowance),
            );
            bytes_read_this_refresh =
                bytes_read_this_refresh.saturating_add(outcome.bytes_read.max(0));
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
            let has_unconsumed_tail = cache.files.get(&key).is_some_and(|usage| {
                codex_logical_target_has_unconsumed_tail(observed_size, usage)
            });
            if !outcome.is_complete || has_unconsumed_tail {
                incomplete_processed.push(key);
                stats.files_deferred = stats.files_deferred.saturating_add(1);
            }
        }
        pending_next.extend(incomplete_processed);

        let mut pruned_paths_pending = Vec::new();
        if discovery_complete && !is_cancelled(cancel) {
            pruned_paths_pending = missing_codex_cache_paths(&cache, &sessions_dirs, scan_range);
            if self.options.is_app_driven() {
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
                || (!self.options.is_app_driven()
                    && pruned_paths_pending.iter().any(|pending| pending == path))
            {
                return true;
            }
            Path::new(path).exists()
                && is_codex_path_in_scan_window(Path::new(path), &sessions_dirs, scan_range)
        });
        if discovery_complete
            && !is_cancelled(cancel)
            && (pruned_paths_pending.is_empty() || self.options.is_app_driven())
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
        rebuild_cache_days(&mut cache);
        cache.last_scan_unix_ms = now_ms;
        if cache.codex_scan_incomplete {
            if cache.previous_report.is_none() {
                cache.previous_report = established_report_before_scan;
            }
            if !is_cancelled(cancel) {
                cache.codex_scan_pause_reason = if !discovery_complete {
                    Some(CodexScanPauseReason::Error(
                        "Codex session source unavailable".to_string(),
                    ))
                } else if !pruned_paths_pending.is_empty()
                    || (bytes_read_this_refresh == 0 && !cache.codex_pending_paths.is_empty())
                {
                    Some(CodexScanPauseReason::NoProgress)
                } else {
                    None
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
        let (native_cost, _) =
            add_codex_days_map_to_summary(&mut rebuilt, &summary_cache.days, &range);
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
        summary = if preserving_previous_report {
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
        if !preserving_previous_report
            && self.sessions_dirs_override.is_none()
            && self.options.include_pi_sessions
        {
            let mut seen_pi = HashSet::new();
            crate::pi_session_cost::scan_pi_compatible_into(
                &mut summary,
                crate::pi_session_cost::PiMappedProvider::Codex,
                self.days,
                cancel,
                &mut seen_pi,
            );
        }

        // v0.56.1 #3279: only publish authoritative coverage after all
        // cancellable scan work, including Pi/OMP, has completed. Persistence
        // pruning may retain `previous_report`, but that must not make a
        // completed in-memory scan stale or make a cancelled partial scan look
        // complete.
        summary.history_coverage_established =
            !is_cancelled(cancel) && !cache.codex_scan_incomplete;
        // Upstream 0.50.1 #2932: a completed scan with zero results is a
        // *known* zero. Only set when coverage is established; an incomplete
        // scan must NOT fabricate a zero.
        summary.known_zero = summary.history_coverage_established && summary.sessions_count == 0;

        (summary, stats, cache)
    }

    /// Scan Claude local logs
    pub(super) fn get_codex_sessions_dirs(&self) -> Vec<PathBuf> {
        if let Some(dirs) = &self.sessions_dirs_override {
            return dirs.clone();
        }
        let settings = Settings::load();
        let codex_home = std::env::var("CODEX_HOME").ok();
        codex_sessions_dir_candidates(
            dirs::home_dir(),
            codex_home,
            &settings.codex_custom_sessions_dirs,
            &default_wsl_roots(),
        )
    }

    fn collect_codex_candidates(
        &self,
        sessions_dirs: &[PathBuf],
        range: &CostUsageDayRange,
        cache: &CostUsageCache,
        cancel: Option<&AtomicBool>,
        stats: &mut CostScanStats,
    ) -> (Vec<CodexScanCandidate>, bool) {
        let mut candidates = Vec::new();
        let mut seen = HashSet::new();
        let mut discovery_complete = true;
        let cache_has_validated_state = codex_cache_has_validated_state(cache);
        let mut dates = codex_scan_dates(range);
        if self.options.prefer_newest_codex_sessions_first {
            dates.reverse();
        }

        for sessions_dir in sessions_dirs {
            if !sessions_dir.is_dir() {
                if cache_has_codex_path_under(cache, sessions_dir)
                    || (sessions_dirs.len() == 1 && cache_has_validated_state)
                {
                    discovery_complete = false;
                }
                continue;
            }
            for date in &dates {
                if is_cancelled(cancel) {
                    return (candidates, false);
                }
                let day_dir = sessions_dir
                    .join(date.format("%Y").to_string())
                    .join(date.format("%m").to_string())
                    .join(date.format("%d").to_string());
                if !day_dir.exists() {
                    if cache_has_codex_path_under(cache, &day_dir) {
                        discovery_complete = false;
                    }
                    continue;
                }
                let Ok(entries) = fs::read_dir(&day_dir) else {
                    if cache_has_codex_path_under(cache, &day_dir) {
                        discovery_complete = false;
                    }
                    continue;
                };
                for entry in entries.flatten() {
                    if is_cancelled(cancel) {
                        return (candidates, false);
                    }
                    let path = entry.path();
                    if !path
                        .extension()
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("jsonl"))
                    {
                        continue;
                    }
                    let path_key = path.to_string_lossy().to_string();
                    if !seen.insert(path_key.clone()) {
                        continue;
                    }
                    let Ok(metadata) = entry.metadata() else {
                        continue;
                    };
                    let mtime_unix_ms = system_time_to_unix_ms(metadata.modified().ok());
                    let unchanged_complete =
                        cached_codex_file_is_complete_for_range(cache, &path_key, range);
                    if unchanged_complete {
                        stats.files_seen = stats.files_seen.saturating_add(1);
                        stats.files_skipped = stats.files_skipped.saturating_add(1);
                        continue;
                    }
                    candidates.push(CodexScanCandidate {
                        path,
                        mtime_unix_ms,
                    });
                }
            }
        }

        // Persisted paths are retried even if their directory partition was not
        // rediscovered this pass, as long as they remain in the requested scan
        // window. Missing paths are pruned after a complete discovery pass.
        for path_key in &cache.codex_pending_paths {
            if seen.contains(path_key) {
                continue;
            }
            let path = PathBuf::from(path_key);
            if !is_codex_path_in_scan_window(&path, sessions_dirs, range) {
                continue;
            }
            let Ok(metadata) = fs::metadata(&path) else {
                continue;
            };
            candidates.push(CodexScanCandidate {
                path,
                mtime_unix_ms: system_time_to_unix_ms(metadata.modified().ok()),
            });
        }

        if self.options.prefer_newest_codex_sessions_first {
            candidates.sort_by(|lhs, rhs| {
                rhs.mtime_unix_ms
                    .cmp(&lhs.mtime_unix_ms)
                    .then_with(|| rhs.path.cmp(&lhs.path))
            });
        } else {
            candidates.sort_by(|lhs, rhs| {
                lhs.mtime_unix_ms
                    .cmp(&rhs.mtime_unix_ms)
                    .then_with(|| lhs.path.cmp(&rhs.path))
            });
        }
        (candidates, discovery_complete)
    }

    #[cfg(test)]
    fn parse_codex_file(
        &self,
        path: &Path,
        range: &CostUsageDayRange,
        summary: &mut CostSummary,
        cache: &mut CostUsageCache,
        cancel: Option<&AtomicBool>,
        stats: &mut CostScanStats,
    ) {
        let _ = self.parse_codex_file_bounded(path, range, summary, cache, cancel, stats, None);
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "bounded file scan carries shared scan state"
    )]
    fn parse_codex_file_bounded(
        &self,
        path: &Path,
        range: &CostUsageDayRange,
        summary: &mut CostSummary,
        cache: &mut CostUsageCache,
        cancel: Option<&AtomicBool>,
        stats: &mut CostScanStats,
        max_bytes_to_read: Option<i64>,
    ) -> CodexFileScanOutcome {
        if is_cancelled(cancel) {
            return CodexFileScanOutcome::default();
        }
        stats.files_seen = stats.files_seen.saturating_add(1);

        let metadata = match fs::metadata(path) {
            Ok(metadata) => metadata,
            Err(_) => return CodexFileScanOutcome::default(),
        };
        #[allow(
            clippy::cast_possible_wrap,
            reason = "file sizes are clamped to i64::MAX"
        )]
        let size = metadata.len().min(i64::MAX as u64) as i64;
        let mtime_ms = system_time_to_unix_ms(metadata.modified().ok());
        let path_key = path.to_string_lossy().to_string();
        let cached = cache.files.get(&path_key).cloned();
        let cache_covers_range = JsonlScanner::cache_covers_range(cache, range);
        let trace_was_pruned = cached.as_ref().is_some_and(|entry| {
            entry.size > size
                && (entry.parsed_bytes.unwrap_or(0) > size || codex_scan_target_size(entry) > size)
        });
        if trace_was_pruned && !self.options.is_app_driven() {
            // A shrinking trace invalidates the append cursor. Preserve the
            // validated cache and queue the path for an explicit cold refresh
            // instead of silently replacing history during synchronization.
            return CodexFileScanOutcome {
                bytes_read: 0,
                is_complete: false,
            };
        }
        let session_metadata = JsonlScanner::read_codex_session_metadata(path).unwrap_or_default();
        let cached_identity_matches = cached
            .as_ref()
            .is_some_and(|entry| entry.mtime_unix_ms == mtime_ms && entry.size == size);
        let codex_session_id = session_metadata.session_id.clone().or_else(|| {
            cached_identity_matches
                .then(|| cached.as_ref()?.codex_session_id.clone())
                .flatten()
        });
        let codex_forked_from_id = session_metadata.forked_from_id.clone().or_else(|| {
            cached_identity_matches
                .then(|| cached.as_ref()?.codex_forked_from_id.clone())
                .flatten()
        });
        let codex_fork_timestamp = session_metadata.fork_timestamp.clone().or_else(|| {
            cached_identity_matches
                .then(|| cached.as_ref()?.codex_fork_timestamp.clone())
                .flatten()
        });
        let cached_identity_changed = cached.as_ref().is_some_and(|entry| {
            session_metadata
                .session_id
                .as_ref()
                .zip(entry.codex_session_id.as_ref())
                .is_some_and(|(current, previous)| current != previous)
                || session_metadata
                    .forked_from_id
                    .as_ref()
                    .zip(entry.codex_forked_from_id.as_ref())
                    .is_some_and(|(current, previous)| current != previous)
        });
        let is_fork = codex_forked_from_id.is_some();
        let fork_baseline = codex_forked_from_id.as_deref().and_then(|parent_id| {
            codex_parent_baseline(cache, parent_id, codex_fork_timestamp.as_deref())
        });

        if is_fork && fork_baseline.is_none() {
            cache.files.insert(
                path_key,
                CostUsageFileUsage {
                    mtime_unix_ms: mtime_ms,
                    size,
                    days: HashMap::new(),
                    parsed_bytes: Some(0),
                    codex_scan_target_size: None,
                    last_model: None,
                    last_totals: None,
                    codex_token_timestamps_monotonic: None,
                    codex_last_token_timestamp: None,
                    codex_session_id,
                    codex_forked_from_id,
                    codex_fork_timestamp,
                    codex_unresolved_fork_parent: true,
                },
            );
            stats.files_parsed = stats.files_parsed.saturating_add(1);
            return CodexFileScanOutcome {
                bytes_read: 0,
                is_complete: false,
            };
        }

        if let Some(entry) = &cached
            && cache_covers_range
            && !entry.codex_unresolved_fork_parent
            && entry.mtime_unix_ms == mtime_ms
            && entry.size == size
            && codex_scan_target_size(entry) == size
            && entry.parsed_bytes.unwrap_or(0) >= size
        {
            let (session_cost, has_tokens) =
                add_codex_days_map_to_summary(summary, &entry.days, range);
            if has_tokens {
                summary.total_cost_usd += session_cost;
                summary.sessions_count += 1;
            }
            stats.files_skipped = stats.files_skipped.saturating_add(1);
            return CodexFileScanOutcome {
                bytes_read: 0,
                is_complete: true,
            };
        }

        if !is_fork
            && !cached_identity_changed
            && let Some(entry) = &cached
        {
            let start_offset = entry.parsed_bytes.unwrap_or(0);
            let same_partial =
                size == entry.size && mtime_ms == entry.mtime_unix_ms && start_offset < size;
            let growing = size > entry.size;
            let parser_state_safe = entry.codex_token_timestamps_monotonic.is_some();
            if cache_covers_range
                && (same_partial || growing)
                && !codex_cached_entry_is_complete_empty_fragment(entry)
                && start_offset > 0
                && start_offset <= size
                && parser_state_safe
                && JsonlScanner::is_line_boundary_offset(path, start_offset)
            {
                let resumable_target_size = codex_resumable_scan_target_size(size, entry);
                let parse_result = match JsonlScanner::parse_codex_file_with_state_bounded_target(
                    path,
                    range,
                    start_offset,
                    entry.last_model.clone(),
                    entry.last_totals.clone(),
                    entry.codex_last_token_timestamp.clone(),
                    entry.codex_token_timestamps_monotonic,
                    cancel,
                    resumable_target_size,
                    max_bytes_to_read,
                ) {
                    Ok(result) => result,
                    Err(_) => return CodexFileScanOutcome::default(),
                };
                stats.token_timestamp_comparisons = stats
                    .token_timestamp_comparisons
                    .saturating_add(parse_result.token_timestamp_comparisons);
                let mut days = entry.days.clone();
                merge_codex_records_into_days(&mut days, &parse_result.records);
                let (session_cost, has_tokens) =
                    add_codex_days_map_to_summary(summary, &days, range);
                if has_tokens {
                    summary.total_cost_usd += session_cost;
                    summary.sessions_count += 1;
                }
                let outcome = CodexFileScanOutcome {
                    bytes_read: parse_result.bytes_read,
                    is_complete: parse_result.is_complete,
                };
                cache.files.insert(
                    path_key,
                    CostUsageFileUsage {
                        mtime_unix_ms: mtime_ms,
                        size,
                        days,
                        parsed_bytes: Some(parse_result.parsed_bytes),
                        codex_scan_target_size: Some(parse_result.scan_target_size),
                        last_model: parse_result.last_model.or_else(|| entry.last_model.clone()),
                        last_totals: parse_result
                            .last_totals
                            .or_else(|| entry.last_totals.clone()),
                        codex_token_timestamps_monotonic: parse_result
                            .token_timestamps_monotonic
                            .or(entry.codex_token_timestamps_monotonic),
                        codex_last_token_timestamp: parse_result
                            .last_token_timestamp
                            .or_else(|| entry.codex_last_token_timestamp.clone()),
                        codex_session_id: codex_session_id.clone(),
                        codex_forked_from_id: codex_forked_from_id.clone(),
                        codex_fork_timestamp: codex_fork_timestamp.clone(),
                        codex_unresolved_fork_parent: false,
                    },
                );
                stats.files_resumed = stats.files_resumed.saturating_add(1);
                return outcome;
            }
        }

        let parse_target_size = cached
            .as_ref()
            .and_then(|entry| codex_resumable_scan_target_size(size, entry));
        let parse_result = match if let Some(baseline) = fork_baseline.clone() {
            JsonlScanner::parse_codex_file_with_state_bounded_fork_target(
                path,
                range,
                baseline,
                cancel,
                parse_target_size,
                max_bytes_to_read,
            )
        } else {
            JsonlScanner::parse_codex_file_with_state_bounded(
                path,
                range,
                0,
                None,
                None,
                None,
                None,
                cancel,
                max_bytes_to_read,
            )
        } {
            Ok(result) => result,
            Err(_) => return CodexFileScanOutcome::default(),
        };
        stats.token_timestamp_comparisons = stats
            .token_timestamp_comparisons
            .saturating_add(parse_result.token_timestamp_comparisons);
        if parse_result.fork_baseline_ambiguous {
            cache.files.insert(
                path_key,
                CostUsageFileUsage {
                    mtime_unix_ms: mtime_ms,
                    size,
                    days: HashMap::new(),
                    parsed_bytes: Some(0),
                    codex_scan_target_size: None,
                    last_model: None,
                    last_totals: None,
                    codex_token_timestamps_monotonic: None,
                    codex_last_token_timestamp: None,
                    codex_session_id,
                    codex_forked_from_id,
                    codex_fork_timestamp,
                    codex_unresolved_fork_parent: true,
                },
            );
            stats.files_parsed = stats.files_parsed.saturating_add(1);
            return CodexFileScanOutcome {
                bytes_read: parse_result.bytes_read,
                is_complete: false,
            };
        }
        let mut days = HashMap::new();
        merge_codex_records_into_days(&mut days, &parse_result.records);
        let (session_cost, has_tokens) =
            add_codex_records_to_summary(summary, &parse_result.records, range);
        if has_tokens {
            summary.total_cost_usd += session_cost;
            summary.sessions_count += 1;
        }
        let outcome = CodexFileScanOutcome {
            bytes_read: parse_result.bytes_read,
            is_complete: parse_result.is_complete,
        };
        cache.files.insert(
            path_key,
            CostUsageFileUsage {
                mtime_unix_ms: mtime_ms,
                size,
                days,
                parsed_bytes: Some(parse_result.parsed_bytes),
                codex_scan_target_size: Some(parse_result.scan_target_size),
                last_model: parse_result.last_model,
                last_totals: parse_result.last_totals,
                codex_token_timestamps_monotonic: parse_result.token_timestamps_monotonic,
                codex_last_token_timestamp: parse_result.last_token_timestamp,
                codex_session_id,
                codex_forked_from_id,
                codex_fork_timestamp,
                codex_unresolved_fork_parent: false,
            },
        );
        stats.files_parsed = stats.files_parsed.saturating_add(1);
        outcome
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
