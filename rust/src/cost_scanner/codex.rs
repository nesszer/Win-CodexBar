use super::*;
use crate::core::{CodexForkAccountingState, CodexSessionLineage};

mod cache_days;
mod logical_target;
mod pending_range;
mod reconciliation;
mod scan;
use cache_days::rebuild_cache_days;
use logical_target::*;
use pending_range::{
    CodexPendingScanContext, CodexPendingScanDisposition, codex_cache_has_validated_state,
};
use reconciliation::*;

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
    let uses_parent_baseline = usage.codex_lineage.uses_parent_baseline()
        || (matches!(usage.codex_lineage, CodexSessionLineage::Root)
            && usage.codex_forked_from_id.is_some());
    !uses_parent_baseline
        || usage
            .codex_forked_from_id
            .as_deref()
            .is_some_and(|parent_id| {
                codex_parent_baseline(cache, parent_id, usage.codex_fork_timestamp.as_deref())
                    .is_some()
            })
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
        if let (Some(expected), Some(actual)) = (
            usage.codex_file_identity.as_ref(),
            JsonlScanner::codex_file_identity(Path::new(path_key), &metadata),
        ) && expected != &actual
        {
            return None;
        }
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
        scan::scan_codex_detailed_with_cache(self, cancel)
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
        let file_identity = JsonlScanner::codex_file_identity(path, &metadata);
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
        let cache_entry_is_fresh = |entry: &CostUsageFileUsage| {
            cached_codex_file_is_fresh(cache, entry, cache_covers_range, mtime_ms, size)
        };
        let identity_matches_cached = |entry: &CostUsageFileUsage| match (
            entry.codex_file_identity.as_ref(),
            file_identity.as_ref(),
        ) {
            (Some(expected), Some(actual)) => expected == actual,
            _ => false,
        };

        // The compact cache is authoritative for an unchanged file. Do this
        // before reading even the bounded metadata prefix; raw token history
        // is only needed after freshness fails or a fork needs reconciliation.
        if let Some(entry) = cached.as_ref()
            && cache_entry_is_fresh(entry)
            && identity_matches_cached(entry)
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

        stats.codex_metadata_read_paths.push(path_key.clone());
        stats.codex_read_receipt.metadata_reads =
            stats.codex_read_receipt.metadata_reads.saturating_add(1);
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
            // A parsed identity makes the metadata authoritative: an absent
            // billing parent must clear any dependency cached by older parsers.
            (cached_identity_matches && session_metadata.session_id.is_none())
                .then(|| cached.as_ref()?.codex_forked_from_id.clone())
                .flatten()
        });
        let cached_fork_accounting_state = cached
            .as_ref()
            .and_then(|entry| entry.codex_fork_accounting_state.clone());
        let codex_fork_timestamp = session_metadata.fork_timestamp.clone().or_else(|| {
            cached_identity_matches
                .then(|| cached.as_ref()?.codex_fork_timestamp.clone())
                .flatten()
        });
        let history_base_thread_id =
            session_metadata.history_base_thread_id.clone().or_else(|| {
                cached_identity_matches
                    .then(|| {
                        cached_fork_accounting_state
                            .as_ref()?
                            .history_base_thread_id
                            .clone()
                    })
                    .flatten()
            });
        let codex_lineage = if session_metadata.session_id.is_some() {
            session_metadata.lineage
        } else if cached_identity_matches {
            cached
                .as_ref()
                .map(|entry| entry.codex_lineage)
                .unwrap_or_default()
        } else {
            CodexSessionLineage::Root
        };
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
                || (session_metadata.session_id.is_some()
                    && session_metadata.forked_from_id.is_none()
                    && entry.codex_forked_from_id.is_some())
                || (session_metadata.session_id.is_some()
                    && session_metadata.lineage != entry.codex_lineage)
                || (session_metadata.session_id.is_some()
                    && cached_fork_accounting_state.as_ref().is_some_and(|state| {
                        state.history_base_thread_id != session_metadata.history_base_thread_id
                    }))
        });
        let is_fork = codex_lineage.uses_parent_baseline();
        let cached_fork_state_matches =
            cached_fork_accounting_state.as_ref().is_some_and(|state| {
                state.session_id == codex_session_id
                    && state.forked_from_id == codex_forked_from_id
                    && state.history_base_thread_id == history_base_thread_id
                    && state.fork_timestamp == codex_fork_timestamp
            });
        let fork_baseline = cached_fork_accounting_state
            .as_ref()
            .filter(|_| cached_fork_state_matches)
            .and_then(|state| state.inherited_totals.clone())
            .or_else(|| {
                is_fork
                    .then_some(codex_forked_from_id.as_deref())
                    .flatten()
                    .and_then(|parent_id| {
                        codex_parent_baseline(cache, parent_id, codex_fork_timestamp.as_deref())
                    })
            });
        let remaining_inherited_totals = cached_fork_accounting_state
            .as_ref()
            .filter(|_| cached_fork_state_matches)
            .and_then(|state| state.remaining_inherited_totals.clone());
        let paginated_continuation = is_fork
            && codex_forked_from_id.is_some()
            && history_base_thread_id
                .as_deref()
                .is_some_and(|history_base| Some(history_base) != codex_forked_from_id.as_deref());

        if is_fork && fork_baseline.is_none() {
            cache.files.insert(
                path_key,
                CostUsageFileUsage {
                    mtime_unix_ms: mtime_ms,
                    size,
                    codex_file_identity: file_identity.clone(),
                    days: HashMap::new(),
                    parsed_bytes: Some(0),
                    codex_scan_target_size: None,
                    last_model: None,
                    last_totals: None,
                    codex_token_timestamps_monotonic: None,
                    codex_last_token_timestamp: None,
                    codex_session_id,
                    codex_forked_from_id,
                    codex_fork_accounting_state: None,
                    codex_lineage,
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
            && cached_codex_file_is_fresh(cache, entry, cache_covers_range, mtime_ms, size)
            && (entry.codex_file_identity.is_none() || identity_matches_cached(entry))
            && !cached_identity_changed
        {
            let (session_cost, has_tokens) =
                add_codex_days_map_to_summary(summary, &entry.days, range);
            if has_tokens {
                summary.total_cost_usd += session_cost;
                summary.sessions_count += 1;
            }
            if entry.codex_file_identity != file_identity {
                let mut refreshed = entry.clone();
                refreshed.codex_file_identity = file_identity.clone();
                cache.files.insert(path_key.clone(), refreshed);
            }
            stats.files_skipped = stats.files_skipped.saturating_add(1);
            return CodexFileScanOutcome {
                bytes_read: 0,
                is_complete: true,
            };
        }

        stats.codex_history_read_paths.push(path_key.clone());
        stats.codex_read_receipt.history_reads =
            stats.codex_read_receipt.history_reads.saturating_add(1);

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
                        codex_file_identity: file_identity
                            .clone()
                            .or(entry.codex_file_identity.clone()),
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
                        codex_fork_accounting_state: None,
                        codex_lineage,
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
            JsonlScanner::parse_codex_file_with_state_bounded_fork_target_with_accounting(
                path,
                range,
                baseline,
                paginated_continuation,
                remaining_inherited_totals.clone(),
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
                    codex_file_identity: file_identity.clone(),
                    days: HashMap::new(),
                    parsed_bytes: Some(0),
                    codex_scan_target_size: None,
                    last_model: None,
                    last_totals: None,
                    codex_token_timestamps_monotonic: None,
                    codex_last_token_timestamp: None,
                    codex_session_id,
                    codex_forked_from_id,
                    codex_fork_accounting_state: None,
                    codex_lineage,
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
        let codex_fork_accounting_state = if is_fork {
            parse_result
                .fork_baseline
                .clone()
                .map(|inherited_totals| CodexForkAccountingState {
                    session_id: codex_session_id.clone(),
                    forked_from_id: codex_forked_from_id.clone(),
                    history_base_thread_id: history_base_thread_id.clone(),
                    fork_timestamp: codex_fork_timestamp.clone(),
                    inherited_totals: Some(inherited_totals),
                    remaining_inherited_totals: parse_result.remaining_inherited_totals.clone(),
                })
        } else {
            None
        };
        cache.files.insert(
            path_key,
            CostUsageFileUsage {
                mtime_unix_ms: mtime_ms,
                size,
                codex_file_identity: file_identity,
                days,
                parsed_bytes: Some(parse_result.parsed_bytes),
                codex_scan_target_size: Some(parse_result.scan_target_size),
                last_model: parse_result.last_model,
                last_totals: parse_result.last_totals,
                codex_token_timestamps_monotonic: parse_result.token_timestamps_monotonic,
                codex_last_token_timestamp: parse_result.last_token_timestamp,
                codex_session_id,
                codex_forked_from_id,
                codex_fork_accounting_state,
                codex_lineage,
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
