use super::*;
use crate::core::{
    CodexForkAccountingState, CodexParseMode, CodexSessionLineage, CodexSessionMetadata,
};

mod cache_days;
mod flat_sources;
mod fork_resume;
mod logical_target;
mod pending_range;
mod priority_trace;
mod reconciliation;
mod scan;
use cache_days::rebuild_cache_days;
use flat_sources::{
    CodexFlatListing, earliest_flat_codex_day, is_flat_codex_path_in_scan_window,
    relocate_moved_codex_rollouts,
};
use fork_resume::CodexForkResume;
use logical_target::*;
use pending_range::{
    CodexPendingScanContext, CodexPendingScanDisposition, codex_cache_has_validated_state,
};
use reconciliation::*;

#[derive(Debug)]
enum CodexAccountingMode {
    Standard,
    Baseline {
        baseline: crate::core::CodexTotals,
        paginated_continuation: bool,
        provenance: CodexBaselineProvenance,
    },
    InferSubagent {
        start_ordinal: Option<i64>,
    },
    Unresolved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CodexBaselineProvenance {
    ValidatedParent { replaces_cached_state: bool },
    CachedValidatedParent,
}

impl CodexAccountingMode {
    fn is_unresolved(&self) -> bool {
        matches!(self, Self::Unresolved)
    }

    fn infers_subagent_baseline(&self) -> bool {
        matches!(self, Self::InferSubagent { .. })
    }

    fn requires_cached_reparse(&self) -> bool {
        matches!(
            self,
            Self::Baseline {
                provenance: CodexBaselineProvenance::ValidatedParent {
                    replaces_cached_state: true
                },
                ..
            }
        )
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

fn codex_usage_uses_parent(usage: &CostUsageFileUsage) -> bool {
    usage.codex_lineage.uses_parent_baseline()
        || (matches!(usage.codex_lineage, CodexSessionLineage::Root)
            && usage.codex_forked_from_id.is_some())
}

fn codex_fork_uses_local_inference(usage: &CostUsageFileUsage) -> bool {
    usage
        .codex_fork_accounting_state
        .as_ref()
        .is_some_and(|state| state.locally_resolved)
}

/// `<CODEX_HOME>/logs_2.sqlite`, else `~/.codex/logs_2.sqlite`.
///
/// Deliberate Windows deviation: upstream pins the trace database to
/// `~/.codex` and ignores `CODEX_HOME`. Here the scanned session roots come
/// from `CODEX_HOME`, and the Priority overlay only prices session files under
/// the database's own home, so both must resolve the same home or a
/// `CODEX_HOME` user would never see Priority pricing.
fn ambient_codex_trace_database_path(
    codex_home: Option<String>,
    home_dir: Option<PathBuf>,
) -> Option<PathBuf> {
    let home = codex_home
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| home_dir.map(|home| home.join(".codex")))?;
    Some(home.join(priority_trace::CODEX_TRACE_DATABASE_FILE))
}

/// First day that can hold a Codex date partition: January 1 of the earliest
/// `YYYY` directory under any sessions root (upstream `firstPartitionDate`).
///
/// An all-available window starts here so the `YYYY/MM/DD` walk never probes
/// empty history. Years before 1970 are stray directories, not sessions.
pub(super) fn first_codex_partition_date(sessions_dirs: &[PathBuf]) -> Option<NaiveDate> {
    sessions_dirs
        .iter()
        .filter_map(|root| fs::read_dir(root).ok())
        .flat_map(|entries| entries.flatten())
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter_map(|entry| {
            let name = entry.file_name();
            let name = name.to_str()?;
            (name.len() == 4 && name.bytes().all(|byte| byte.is_ascii_digit()))
                .then(|| name.parse::<i32>().ok())
                .flatten()
        })
        .filter(|year| *year >= 1970)
        .min()
        .and_then(|year| NaiveDate::from_ymd_opt(year, 1, 1))
}

fn is_codex_path_in_scan_window(
    path: &Path,
    sessions_dirs: &[PathBuf],
    range: &CostUsageDayRange,
) -> bool {
    if is_flat_codex_path_in_scan_window(path, sessions_dirs, range) {
        return true;
    }
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

struct CodexPreparedCandidate {
    path: PathBuf,
    session_metadata: CodexSessionMetadata,
    lineage_gate: CodexLineageGate,
    parent_owner_expected: bool,
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

    /// The Codex trace database that supplies Priority evidence.
    ///
    /// An explicit fixture is authoritative. Like upstream's test isolation,
    /// unit tests and injected sessions roots never fall back to the ambient
    /// user database, so no test reads (or caches a cursor for) real traces.
    pub(super) fn codex_trace_database_path(&self) -> Option<PathBuf> {
        if let Some(path) = &self.codex_trace_database_override {
            return Some(path.clone());
        }
        if self.sessions_dirs_override.is_some() || cfg!(test) {
            return None;
        }
        ambient_codex_trace_database_path(std::env::var("CODEX_HOME").ok(), dirs::home_dir())
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "candidate discovery carries the shared scan state"
    )]
    fn collect_codex_candidates(
        &self,
        sessions_dirs: &[PathBuf],
        range: &CostUsageDayRange,
        cache: &CostUsageCache,
        planner: &CodexLineagePlanner,
        flat_listing: &CodexFlatListing,
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
                        cached_codex_file_is_complete_for_range(cache, planner, &path_key, range);
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

        // Archived and legacy flat rollouts, listed once per pass.
        discovery_complete &= flat_listing.complete;
        for (path, mtime_unix_ms) in &flat_listing.files {
            if is_cancelled(cancel) {
                return (candidates, false);
            }
            let path_key = path.to_string_lossy().to_string();
            if !seen.insert(path_key.clone()) {
                continue;
            }
            if cached_codex_file_is_complete_for_range(cache, planner, &path_key, range) {
                stats.files_seen = stats.files_seen.saturating_add(1);
                stats.files_skipped = stats.files_skipped.saturating_add(1);
                continue;
            }
            candidates.push(CodexScanCandidate {
                path: path.clone(),
                mtime_unix_ms: *mtime_unix_ms,
            });
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
        let planner = CodexLineagePlanner::new(cache);
        let _ = self.parse_codex_file_bounded(
            path, range, summary, cache, cancel, stats, None, None, &planner,
        );
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
        prepared_candidate: Option<&CodexPreparedCandidate>,
        planner: &CodexLineagePlanner,
    ) -> CodexFileScanOutcome {
        if is_cancelled(cancel) {
            return CodexFileScanOutcome::default();
        }
        if prepared_candidate.is_none() {
            stats.files_seen = stats.files_seen.saturating_add(1);
        }

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
            cached_codex_file_is_fresh(cache, planner, entry, cache_covers_range, mtime_ms, size)
        };
        let identity_matches_cached = |entry: &CostUsageFileUsage| {
            codex_file_identity_matches(
                entry.codex_file_identity.as_deref(),
                file_identity.as_deref(),
            )
        };

        // The compact cache is authoritative for an unchanged file. Do this
        // before reading even the bounded metadata prefix; raw token history
        // is only needed after freshness fails or a fork needs reconciliation.
        if let Some(entry) = cached.as_ref()
            && prepared_candidate
                .is_none_or(|candidate| candidate.lineage_gate == CodexLineageGate::Eligible)
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

        let session_metadata = if let Some(prepared) = prepared_candidate {
            prepared.session_metadata.clone()
        } else {
            stats.codex_metadata_read_paths.push(path_key.clone());
            stats.codex_read_receipt.metadata_reads =
                stats.codex_read_receipt.metadata_reads.saturating_add(1);
            JsonlScanner::read_codex_session_metadata(path).unwrap_or_default()
        };
        // Cached lineage metadata belongs to a physical file, not merely a
        // path/size/mtime tuple. Missing identity evidence fails closed.
        let cached_identity_matches = cached.as_ref().is_some_and(identity_matches_cached);
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
        let matching_cached_fork_state = cached_fork_accounting_state
            .as_ref()
            .filter(|_| cached_fork_state_matches);
        let paginated_continuation = is_fork
            && codex_forked_from_id.is_some()
            && history_base_thread_id
                .as_deref()
                .is_some_and(|history_base| Some(history_base) != codex_forked_from_id.as_deref());
        let lineage_gate = prepared_candidate
            .map(|candidate| candidate.lineage_gate)
            .unwrap_or_default();
        let parent_owner_expected =
            prepared_candidate.is_some_and(|candidate| candidate.parent_owner_expected);
        let lineage_decision = planner.decision_for_scan(
            cache,
            is_fork,
            lineage_gate,
            codex_forked_from_id.as_deref(),
            codex_fork_timestamp.as_deref(),
            parent_owner_expected,
        );
        let accounting_mode = lineage_decision.accounting_mode(
            matching_cached_fork_state,
            &session_metadata,
            paginated_continuation,
        );

        if accounting_mode.is_unresolved() {
            // Priority evidence lives in `codex_fork_rows`; a fork that could
            // not resolve its parent baseline must not keep stale rows.
            cache.codex_fork_rows.remove(&path_key);
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
            && cached_codex_file_is_fresh(cache, planner, entry, cache_covers_range, mtime_ms, size)
            && identity_matches_cached(entry)
            && !cached_identity_changed
            && !accounting_mode.requires_cached_reparse()
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
                let parse_result = match JsonlScanner::parse_codex(
                    path,
                    range,
                    CodexParseMode::Standard {
                        start_offset,
                        initial_model: entry.last_model.clone(),
                        initial_totals: entry.last_totals.clone(),
                        previous_token_timestamp: entry.codex_last_token_timestamp.clone(),
                        token_timestamps_monotonic: entry.codex_token_timestamps_monotonic,
                    },
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
                if codex_forked_from_id.is_some() {
                    // Only extend rows that already cover the parsed prefix;
                    // a partial row set could never match the day totals.
                    if let Some(rows) = cache.codex_fork_rows.get_mut(&path_key) {
                        rows.extend(codex_fork_rows_from_records(&parse_result.records));
                    }
                } else {
                    cache.codex_fork_rows.remove(&path_key);
                }
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

        let fork_resume = cached
            .as_ref()
            .filter(|entry| {
                is_fork
                    && cache_covers_range
                    && !cached_identity_changed
                    && identity_matches_cached(entry)
            })
            .zip(matching_cached_fork_state)
            .and_then(|(entry, state)| {
                CodexForkResume::for_entry(path, entry, state, &accounting_mode, size, mtime_ms)
            });
        let resumed = fork_resume.is_some();
        let parse_target_size = (!accounting_mode.requires_cached_reparse())
            .then(|| {
                cached
                    .as_ref()
                    .and_then(|entry| codex_resumable_scan_target_size(size, entry))
            })
            .flatten();
        let parse_result = match match &accounting_mode {
            CodexAccountingMode::Standard => JsonlScanner::parse_codex(
                path,
                range,
                CodexParseMode::Standard {
                    start_offset: 0,
                    initial_model: None,
                    initial_totals: None,
                    previous_token_timestamp: None,
                    token_timestamps_monotonic: None,
                },
                cancel,
                None,
                max_bytes_to_read,
            ),
            CodexAccountingMode::Baseline {
                baseline,
                paginated_continuation,
                ..
            } => match fork_resume {
                Some(resume) => JsonlScanner::parse_codex(
                    path,
                    range,
                    CodexParseMode::ResumeParentBaseline(resume.parse),
                    cancel,
                    Some(resume.target_size),
                    max_bytes_to_read,
                ),
                None => {
                    JsonlScanner::parse_codex(
                        path,
                        range,
                        CodexParseMode::ParentBaseline {
                            baseline: baseline.clone(),
                            paginated_continuation: *paginated_continuation,
                            // A parse from byte zero replays the inherited counters
                            // itself. Only a resumed parse carries their used-up
                            // remainder, as upstream restores fork state only then.
                            remaining_inherited_totals: None,
                        },
                        cancel,
                        parse_target_size,
                        max_bytes_to_read,
                    )
                }
            },
            CodexAccountingMode::InferSubagent { start_ordinal } => JsonlScanner::parse_codex(
                path,
                range,
                CodexParseMode::InferSubagent {
                    start_ordinal: *start_ordinal,
                },
                cancel,
                parse_target_size,
                max_bytes_to_read,
            ),
            CodexAccountingMode::Unresolved => {
                unreachable!("unresolved forks return before parsing")
            }
        } {
            Ok(result) => result,
            Err(_) => return CodexFileScanOutcome::default(),
        };
        stats.token_timestamp_comparisons = stats
            .token_timestamp_comparisons
            .saturating_add(parse_result.token_timestamp_comparisons);
        if parse_result.fork_baseline_ambiguous
            || (accounting_mode.infers_subagent_baseline()
                && !parse_result.fork_baseline_locally_resolved)
        {
            // Priority evidence lives in `codex_fork_rows`; an ambiguous fork
            // baseline must not keep stale rows.
            cache.codex_fork_rows.remove(&path_key);
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
        // A resumed parse returns only the suffix; its prefix is the cached day
        // map, which is billed as the non-fork resume path bills it.
        let mut days = match (resumed, cached.as_ref()) {
            (true, Some(entry)) => entry.days.clone(),
            _ => HashMap::new(),
        };
        merge_codex_records_into_days(&mut days, &parse_result.records);
        if is_fork || codex_forked_from_id.is_some() {
            // Priority evidence for fork-shaped files is tracked separately
            // (source-row evidence skips them), so rebuild the row set from
            // the same parsed records that fed the day totals.
            cache.codex_fork_rows.insert(
                path_key.clone(),
                codex_fork_rows_from_records(&parse_result.records),
            );
        } else {
            cache.codex_fork_rows.remove(&path_key);
        }
        let (session_cost, has_tokens) = if resumed {
            add_codex_days_map_to_summary(summary, &days, range)
        } else {
            add_codex_records_to_summary(summary, &parse_result.records, range)
        };
        if has_tokens {
            summary.total_cost_usd += session_cost;
            summary.sessions_count += 1;
        }
        let outcome = CodexFileScanOutcome {
            bytes_read: parse_result.bytes_read,
            is_complete: parse_result.is_complete,
        };
        let locally_resolved = parse_result.fork_baseline_locally_resolved;
        let codex_fork_accounting_state = if is_fork
            && (parse_result.fork_baseline.is_some() || parse_result.fork_baseline_locally_resolved)
        {
            Some(CodexForkAccountingState {
                session_id: codex_session_id.clone(),
                forked_from_id: codex_forked_from_id.clone(),
                history_base_thread_id: history_base_thread_id.clone(),
                fork_timestamp: codex_fork_timestamp.clone(),
                inherited_totals: parse_result.fork_baseline.clone(),
                first_token_timestamp: parse_result.first_token_timestamp.clone(),
                remaining_inherited_totals: parse_result.remaining_inherited_totals.clone(),
                locally_resolved,
                resume: parse_result.fork_resume_state.clone(),
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
        if resumed {
            stats.files_resumed = stats.files_resumed.saturating_add(1);
        } else {
            stats.files_parsed = stats.files_parsed.saturating_add(1);
        }
        outcome
    }
}

/// Request rows for a fork-shaped file, keeping exactly the records that
/// [`merge_codex_records_into_days`] folds into its day totals.
fn codex_fork_rows_from_records(
    records: &[(crate::core::CodexUsageRecord, i64)],
) -> Vec<crate::core::CodexSourceUsageRow> {
    crate::core::rows_from_records(records)
        .into_iter()
        .filter(|row| crate::core::CostUsagePricing::counts_toward_codex_subscription(&row.model))
        .collect()
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
