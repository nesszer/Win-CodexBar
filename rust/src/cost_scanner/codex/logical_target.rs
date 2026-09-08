use super::*;

pub(super) fn cached_codex_file_is_complete_for_range(
    cache: &CostUsageCache,
    path_key: &str,
    range: &CostUsageDayRange,
) -> bool {
    JsonlScanner::cache_covers_range(cache, range)
        && cache.files.get(path_key).is_some_and(|usage| {
            let Ok(metadata) = fs::metadata(path_key) else {
                return false;
            };
            #[allow(clippy::cast_possible_wrap, reason = "session file sizes fit i64")]
            let size = metadata.len().min(i64::MAX as u64) as i64;
            usage.mtime_unix_ms == system_time_to_unix_ms(metadata.modified().ok())
                && usage.size == size
                && codex_scan_target_size(usage) == size
                && usage.parsed_bytes.unwrap_or(0) >= size
                && !usage.codex_unresolved_fork_parent
                && super::codex_fork_parent_is_safe(cache, usage)
        })
}

/// Give paths already in the durable queue their saved turn before newly
/// discovered dirty paths. The scanner appends unfinished paths after this
/// pass, making the queue a round-robin cursor instead of a newest-first loop.
pub(super) fn prioritize_codex_pending_candidates(
    candidates: &mut Vec<CodexScanCandidate>,
    pending_paths: &[String],
) {
    if pending_paths.is_empty() || candidates.len() < 2 {
        return;
    }

    let mut pending = Vec::with_capacity(candidates.len());
    let mut fresh = Vec::with_capacity(candidates.len());
    for candidate in candidates.drain(..) {
        let key = candidate.path.to_string_lossy();
        if pending_paths
            .iter()
            .any(|path| path.as_str() == key.as_ref())
        {
            pending.push(candidate);
        } else {
            fresh.push(candidate);
        }
    }
    pending.extend(fresh);
    candidates.extend(pending);
}

/// Return the persisted logical end of a Codex parse. Older cache entries did
/// not have a frozen target, so their physical size remains the safe fallback.
pub(super) fn codex_scan_target_size(usage: &CostUsageFileUsage) -> i64 {
    usage.codex_scan_target_size.unwrap_or(usage.size).max(0)
}

/// A cached prefix is resumable toward its original target when the target is
/// still present in the current file. The caller separately validates the
/// byte-boundary/parser-state invariants before using the cursor.
pub(super) fn codex_resumable_scan_target_size(
    metadata_size: i64,
    usage: &CostUsageFileUsage,
) -> Option<i64> {
    let parsed_bytes = usage.parsed_bytes.unwrap_or(usage.size).max(0);
    let target_size = codex_scan_target_size(usage);
    (parsed_bytes < target_size && target_size <= metadata_size).then_some(target_size)
}

/// Whether a logically complete prefix still has physical bytes that must be
/// revisited. This is the catch-up cursor for a growing rollout or a retained
/// incomplete tail.
pub(super) fn codex_logical_target_has_unconsumed_tail(
    metadata_size: i64,
    usage: &CostUsageFileUsage,
) -> bool {
    let parsed_bytes = usage.parsed_bytes.unwrap_or(usage.size).max(0);
    let target_size = codex_scan_target_size(usage);
    parsed_bytes < metadata_size || target_size < metadata_size
}

/// Whether a completed empty fragment has no parser state worth resuming.
pub(super) fn codex_cached_entry_is_complete_empty_fragment(usage: &CostUsageFileUsage) -> bool {
    usage.days.is_empty()
        && usage.parsed_bytes == Some(usage.size)
        && usage.codex_scan_target_size == Some(usage.size)
        && usage.last_model.is_none()
        && usage.last_totals.is_none()
        && usage.codex_last_token_timestamp.is_none()
        && usage.codex_token_timestamps_monotonic != Some(false)
}
