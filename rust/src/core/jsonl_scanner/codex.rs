use super::*;

mod helpers;
mod parser;
pub(crate) mod priority;
pub(crate) mod source_rows;

use helpers::{
    BoundedJsonlLine, CODEX_JSONL_MAX_LINE_BYTES, nonempty_json_string, parse_rfc3339_timestamp,
    read_bounded_jsonl_line, read_bounded_jsonl_line_until, session_meta_field,
};
pub(crate) use parser::CodexParseMode;
use parser::CodexParserState;

/// Saved cursor and parser state of an unfinished parent-baseline fork parse.
/// Restoring all of it accounts the remaining bytes exactly as one
/// uninterrupted parse would.
#[derive(Debug, Clone)]
pub(crate) struct CodexForkParseResume {
    pub start_offset: i64,
    pub paginated_continuation: bool,
    /// Effective inherited baseline, after any paginated-continuation raise.
    pub inherited_totals: CodexTotals,
    pub remaining_inherited_totals: Option<CodexTotals>,
    pub last_model: Option<String>,
    pub last_totals: CodexTotals,
    pub last_token_timestamp: Option<String>,
    pub first_token_timestamp: Option<String>,
    pub token_timestamps_monotonic: Option<bool>,
    pub state: CodexForkResumeState,
}

use crate::cost_reporting_period::cost_bucket_zone;

/// Persisted Codex cache schema version. Version 0 predates 64-bit totals;
/// version 1 can retain a terminal pause after treating a paginated v2
/// subagent's independent counters as an inherited fork. Version 3 adds
/// persisted paginated-fork accounting state. Version 4 reparses copied-prefix
/// subagents with locally inferred component baselines. Version 5 records a
/// fork's first token timestamp so direct-fork chains resolve through parents
/// with no token snapshot at the descendant's fork time. Version 6 records the
/// Codex turn id on source rows so Priority trace evidence can be matched.
/// Rebuild older artifacts.
pub(crate) const CODEX_CACHE_SCHEMA_VERSION: u32 = 6;

/// Whether a persisted Codex cache artifact matches the current schema.
/// A mismatched artifact (e.g. a pre-64-bit cache from an older release) is
/// invalid and must be rebuilt rather than deserialized into wider fields.
pub(crate) fn codex_cache_schema_is_current(schema_version: u32) -> bool {
    schema_version == CODEX_CACHE_SCHEMA_VERSION
}

/// Whether a persisted Codex cache bucketed its days in the zone now in
/// effect. Artifacts written before the zone was stamped are kept.
pub(crate) fn codex_cache_zone_is_current(bucket_time_zone: Option<&str>) -> bool {
    bucket_time_zone.is_none_or(|zone| zone == cost_bucket_zone().identifier())
}

/// Apply the Codex cache schema version and bucket zone policy to a freshly
/// decoded artifact.
///
/// A mismatched artifact is invalidated: a fresh, current-version cache is
/// returned with the decoded baseline stamp retained so the caller stays
/// authoritative over the artifact it just read. A matching artifact keeps its
/// contents and receives the same stamp.
pub(crate) fn codex_cache_apply_load_policy(
    mut cache: CostUsageCache,
    stamp: CacheStamp,
) -> CostUsageCache {
    if !codex_cache_schema_is_current(cache.codex_cache_schema_version)
        || !codex_cache_zone_is_current(cache.bucket_time_zone.as_deref())
    {
        return CostUsageCache {
            codex_cache_schema_version: CODEX_CACHE_SCHEMA_VERSION,
            loaded_stamp: Some(Some(stamp)),
            ..CostUsageCache::default()
        };
    }
    cache.loaded_stamp = Some(Some(stamp));
    cache
}

/// Stamp the current schema version and bucket zone before a Codex cache is
/// persisted.
pub(crate) fn codex_cache_stamp_schema_version(cache: &mut CostUsageCache) {
    cache.codex_cache_schema_version = CODEX_CACHE_SCHEMA_VERSION;
    cache.bucket_time_zone = Some(cost_bucket_zone().identifier());
}

#[cfg(test)]
use helpers::{
    CodexFastPayload, CodexFastTotals, bare_usage_totals, codex_totals_from_fast,
    fast_totals_from_payload, is_candidate_codex_line, parse_codex_timestamp, read_token_totals,
};

impl JsonlScanner {
    /// Get default Codex sessions root directory
    pub fn default_codex_sessions_root() -> Option<PathBuf> {
        // Check CODEX_HOME environment variable
        if let Ok(home) = std::env::var("CODEX_HOME") {
            let home = home.trim();
            if !home.is_empty() {
                return Some(PathBuf::from(home).join("sessions"));
            }
        }

        // Default to ~/.codex/sessions
        dirs::home_dir().map(|h| h.join(".codex").join("sessions"))
    }

    /// Get default Claude projects roots
    pub fn default_claude_projects_roots() -> Vec<PathBuf> {
        let mut roots = Vec::new();

        // Check CLAUDE_CONFIG_DIR
        if let Ok(config_dir) = std::env::var("CLAUDE_CONFIG_DIR") {
            let path = PathBuf::from(config_dir.trim()).join("projects");
            if path.exists() {
                roots.push(path);
            }
        }

        // Default locations
        if let Some(home) = dirs::home_dir() {
            let default_path = home.join(".claude").join("projects");
            if default_path.exists() && !roots.contains(&default_path) {
                roots.push(default_path);
            }
        }

        roots
    }

    /// List Codex session files in the given date range
    pub fn list_codex_session_files(
        root: &Path,
        scan_since_key: &str,
        scan_until_key: &str,
    ) -> Vec<PathBuf> {
        let mut files = Vec::new();

        let Some(mut date) = CostUsageDayRange::parse_day_key(scan_since_key) else {
            return files;
        };
        let Some(until_date) = CostUsageDayRange::parse_day_key(scan_until_key) else {
            return files;
        };

        while date <= until_date {
            let year = format!("{:04}", date.year());
            let month = format!("{:02}", date.month());
            let day = format!("{:02}", date.day());

            let day_dir = root.join(&year).join(&month).join(&day);

            if let Ok(entries) = fs::read_dir(&day_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path
                        .extension()
                        .is_some_and(|e| e.eq_ignore_ascii_case("jsonl"))
                    {
                        files.push(path);
                    }
                }
            }

            date += chrono::Duration::days(1);
        }

        files
    }

    /// Session files kept directly in `dir` rather than in a `YYYY/MM/DD`
    /// partition (upstream `listCodexSessionFilesFlat`): Codex's flat
    /// `archived_sessions` folder and legacy rollouts in a sessions root.
    /// A file whose name carries a date outside the scan keys is skipped; a
    /// name without a date is kept, so its events decide. Hidden files and
    /// directories are ignored.
    pub(crate) fn list_codex_flat_session_files(
        dir: &Path,
        scan_since_key: &str,
        scan_until_key: &str,
    ) -> std::io::Result<Vec<PathBuf>> {
        let mut files = Vec::new();
        for entry in fs::read_dir(dir)?.flatten() {
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                continue;
            }
            let path = entry.path();
            if !path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("jsonl"))
            {
                continue;
            }
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if !name.starts_with('.')
                && Self::codex_flat_name_in_range(&name, scan_since_key, scan_until_key)
            {
                files.push(path);
            }
        }
        Ok(files)
    }

    /// Whether a flat session file name stays in the scan keys: true when the
    /// name carries no date.
    pub(crate) fn codex_flat_name_in_range(
        name: &str,
        scan_since_key: &str,
        scan_until_key: &str,
    ) -> bool {
        Self::codex_filename_day_key(name)
            .is_none_or(|day| CostUsageDayRange::is_in_range(day, scan_since_key, scan_until_key))
    }

    /// The first `YYYY-MM-DD` run in a session file name (upstream
    /// `dayKeyFromFilename`), e.g. `2025-10-03` in
    /// `rollout-2025-10-03T10-00-00-<uuid>.jsonl`.
    pub(crate) fn codex_filename_day_key(name: &str) -> Option<&str> {
        let bytes = name.as_bytes();
        (0..bytes.len().saturating_sub(9)).find_map(|start| {
            let shaped = bytes[start..start + 10]
                .iter()
                .enumerate()
                .all(|(offset, byte)| match offset {
                    4 | 7 => *byte == b'-',
                    _ => byte.is_ascii_digit(),
                });
            shaped.then(|| name.get(start..start + 10)).flatten()
        })
    }

    /// Read only a bounded prefix until the first authoritative `session_meta`
    /// row is found. Fork decisions must not require parsing the child usage
    /// stream before a safe parent baseline is selected.
    pub(crate) fn read_codex_session_metadata(
        file_path: &Path,
    ) -> std::io::Result<CodexSessionMetadata> {
        let file = File::open(file_path)?;
        let mut reader = BufReader::new(file);
        let mut bytes_examined = 0_usize;

        while bytes_examined < CODEX_JSONL_MAX_LINE_BYTES {
            let Some(line) = read_bounded_jsonl_line(&mut reader, CODEX_JSONL_MAX_LINE_BYTES)?
            else {
                break;
            };
            let (line_bytes, consumed) = match line {
                BoundedJsonlLine::Retained {
                    bytes, consumed, ..
                } => (bytes, consumed),
                BoundedJsonlLine::Discarded { consumed, .. } => {
                    bytes_examined = bytes_examined.saturating_add(consumed);
                    continue;
                }
            };
            bytes_examined = bytes_examined.saturating_add(consumed);
            if line_bytes.is_empty() {
                continue;
            }
            let Ok(line) = std::str::from_utf8(&line_bytes) else {
                continue;
            };
            let line = line.strip_suffix('\r').unwrap_or(line);
            let Ok(obj) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            if obj.get("type").and_then(Value::as_str) != Some("session_meta") {
                continue;
            }

            let payload = obj.get("payload").filter(|value| value.is_object());
            // Desktop v2 subagents fetch inherited context through pagination,
            // but their token counters start at zero. `forked_from_id` describes
            // conversation ancestry, not an inherited billing baseline. Keep
            // legacy/unknown fork formats conservative by requiring all markers.
            let independent_subagent = payload.is_some_and(|value| {
                value.get("history_mode").and_then(Value::as_str) == Some("paginated")
                    && value.get("multi_agent_version").and_then(Value::as_str) == Some("v2")
                    && (value.get("thread_source").and_then(Value::as_str) == Some("subagent")
                        || value
                            .pointer("/source/subagent/thread_spawn")
                            .is_some_and(Value::is_object))
            });
            let is_subagent = payload.is_some_and(|value| {
                value.get("thread_source").and_then(Value::as_str) == Some("subagent")
                    || value
                        .pointer("/source/subagent/thread_spawn")
                        .is_some_and(Value::is_object)
            });
            let history_base_thread_id = payload
                .and_then(|value| value.get("history_base"))
                .filter(|value| value.is_object())
                .and_then(|value| session_meta_field(value, None, &["thread_id", "threadId"]));
            let forked_from_id = (!independent_subagent)
                .then(|| {
                    session_meta_field(
                        &obj,
                        payload,
                        &[
                            "forked_from_id",
                            "forkedFromId",
                            "parent_session_id",
                            "parentSessionId",
                            "parent_thread_id",
                            "parentThreadId",
                        ],
                    )
                })
                .flatten();
            return Ok(CodexSessionMetadata {
                session_id: session_meta_field(&obj, payload, &["id", "session_id", "sessionId"]),
                lineage: if independent_subagent {
                    CodexSessionLineage::Independent
                } else if forked_from_id.is_some() {
                    CodexSessionLineage::Child
                } else {
                    CodexSessionLineage::Root
                },
                forked_from_id,
                fork_timestamp: nonempty_json_string(obj.get("timestamp")).or_else(|| {
                    payload.and_then(|value| nonempty_json_string(value.get("timestamp")))
                }),
                history_base_thread_id,
                is_subagent,
                subagent_history_start_ordinal: payload
                    .and_then(|value| value.get("subagent_history_start_ordinal"))
                    .and_then(Value::as_i64),
            });
        }

        Ok(CodexSessionMetadata::default())
    }

    /// Return the platform file identity used by the cost-cache freshness
    /// receipt. This is metadata-only; it never reads token history bytes.
    #[cfg(windows)]
    pub(crate) fn codex_file_identity(
        file_path: &Path,
        _metadata: &fs::Metadata,
    ) -> Option<String> {
        use std::os::windows::io::AsRawHandle;

        use windows::Win32::Foundation::HANDLE;
        use windows::Win32::Storage::FileSystem::{
            BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
        };

        let file = File::open(file_path).ok()?;
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        // SAFETY: `file` is an open file handle and `info` is valid for writes
        // for the duration of the call.
        let ok = unsafe { GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &mut info) };
        if ok.is_err() {
            return None;
        }
        let file_index = ((info.nFileIndexHigh as u64) << 32) | info.nFileIndexLow as u64;
        Some(format!("{}:{file_index}", info.dwVolumeSerialNumber))
    }

    #[cfg(unix)]
    pub(crate) fn codex_file_identity(
        _file_path: &Path,
        metadata: &fs::Metadata,
    ) -> Option<String> {
        use std::os::unix::fs::MetadataExt;

        Some(format!("{}:{}", metadata.dev(), metadata.ino()))
    }

    #[cfg(not(any(unix, windows)))]
    pub(crate) fn codex_file_identity(
        _file_path: &Path,
        metadata: &fs::Metadata,
    ) -> Option<String> {
        Some(format!("{:?}:{}", metadata.modified().ok(), metadata.len()))
    }

    /// Order two RFC3339 timestamps by parsed instant. `None` when either is
    /// malformed, so fork-baseline reconciliation fails closed.
    pub(crate) fn codex_timestamp_cmp(earlier: &str, later: &str) -> Option<std::cmp::Ordering> {
        Some(parse_rfc3339_timestamp(earlier)?.cmp(&parse_rfc3339_timestamp(later)?))
    }

    /// Parse a Codex JSONL file
    pub fn parse_codex_file(
        file_path: &Path,
        range: &CostUsageDayRange,
        start_offset: i64,
        initial_model: Option<String>,
        initial_totals: Option<CodexTotals>,
    ) -> std::io::Result<CodexParseResult> {
        Self::parse_codex(
            file_path,
            range,
            CodexParseMode::Standard {
                start_offset,
                initial_model,
                initial_totals,
                previous_token_timestamp: None,
                token_timestamps_monotonic: None,
            },
            None,
            None,
            None,
        )
    }

    /// Parse a Codex file in `mode`. `scan_target_size` freezes the end of the
    /// pass below the physical EOF so an active rollout cannot make a bounded
    /// catch-up pass chase its own growth. `max_bytes_to_read` caps the bytes
    /// newly consumed; the reader may finish the current line before yielding.
    pub(crate) fn parse_codex(
        file_path: &Path,
        range: &CostUsageDayRange,
        mode: CodexParseMode,
        cancel: Option<&AtomicBool>,
        scan_target_size: Option<i64>,
        max_bytes_to_read: Option<i64>,
    ) -> std::io::Result<CodexParseResult> {
        let file = File::open(file_path)?;
        // Session JSONL files are bounded by the cache budget; sizes fit i64.
        #[allow(
            clippy::cast_possible_wrap,
            reason = "session JSONL file sizes fit i64"
        )]
        let file_size = file.metadata()?.len() as i64;

        let safe_start_offset = mode.start_offset().clamp(0, file_size);
        let requested_target_size = scan_target_size
            .unwrap_or(file_size)
            .max(safe_start_offset)
            .min(file_size);

        let mut reader = BufReader::new(file);
        if safe_start_offset > 0 {
            reader.seek(SeekFrom::Start(safe_start_offset as u64))?;
        }

        let mut parser = CodexParserState::from_mode(mode);
        let mut parsed_bytes = safe_start_offset;
        let mut committed_bytes = safe_start_offset;
        let mut cancelled = false;
        let mut budget_exhausted = false;
        let mut incomplete_tail = false;

        loop {
            if max_bytes_to_read.is_some_and(|limit| {
                parsed_bytes.saturating_sub(safe_start_offset) >= limit.max(0)
                    && parsed_bytes < requested_target_size
            }) {
                budget_exhausted = true;
                break;
            }
            if cancel.is_some_and(|flag| flag.load(Ordering::Relaxed)) {
                cancelled = true;
                break;
            }
            let remaining_to_target = requested_target_size.saturating_sub(parsed_bytes);
            if remaining_to_target == 0 {
                break;
            }
            let max_total_bytes = usize::try_from(remaining_to_target).ok();
            let Some(line) = read_bounded_jsonl_line_until(
                &mut reader,
                CODEX_JSONL_MAX_LINE_BYTES,
                max_total_bytes,
            )?
            else {
                break;
            };
            if cancel.is_some_and(|flag| flag.load(Ordering::Relaxed)) {
                cancelled = true;
                break;
            }
            let (line_bytes, consumed, terminated_by_newline) = match line {
                BoundedJsonlLine::Retained {
                    bytes,
                    consumed,
                    terminated_by_newline,
                } => (Some(bytes), consumed, terminated_by_newline),
                BoundedJsonlLine::Discarded {
                    consumed,
                    terminated_by_newline,
                } => (None, consumed, terminated_by_newline),
            };
            let consumed_i64 = i64::try_from(consumed).unwrap_or(i64::MAX);
            parsed_bytes = parsed_bytes.saturating_add(consumed_i64);
            let Some(line_bytes) = line_bytes else {
                if terminated_by_newline {
                    committed_bytes = parsed_bytes;
                } else {
                    incomplete_tail = true;
                    parsed_bytes = committed_bytes;
                    break;
                }
                continue;
            };
            if line_bytes.is_empty() {
                committed_bytes = parsed_bytes;
                continue;
            }
            let Ok(line) = std::str::from_utf8(&line_bytes) else {
                if terminated_by_newline {
                    committed_bytes = parsed_bytes;
                    continue;
                }
                incomplete_tail = true;
                parsed_bytes = committed_bytes;
                break;
            };
            let line = line.strip_suffix('\r').unwrap_or(line);
            if !terminated_by_newline && serde_json::from_str::<Value>(line).is_err() {
                incomplete_tail = true;
                parsed_bytes = committed_bytes;
                break;
            }
            parser.process_line_with_source_offset(line, range, parsed_bytes);
            committed_bytes = parsed_bytes;
        }

        let effective_target_size = if incomplete_tail && !cancelled && !budget_exhausted {
            committed_bytes
        } else {
            requested_target_size
        };
        let is_complete = !cancelled && !budget_exhausted && parsed_bytes >= effective_target_size;
        let bytes_read = parsed_bytes.saturating_sub(safe_start_offset).max(0);
        let fork_baseline_locally_resolved = parser.fork_baseline_locally_resolved();
        let fork_resume_state = if is_complete {
            None
        } else {
            parser.fork_resume_state()
        };
        Ok(CodexParseResult {
            records: parser.records,
            parsed_bytes,
            scan_target_size: if is_complete {
                effective_target_size
            } else {
                requested_target_size
            },
            last_model: parser.current_model,
            last_totals: parser.previous_totals,
            token_timestamps_monotonic: parser.token_timestamps_monotonic,
            last_token_timestamp: parser.previous_token_timestamp,
            first_token_timestamp: parser.first_token_timestamp,
            token_timestamp_comparisons: parser.token_timestamp_comparisons,
            bytes_read,
            is_complete,
            fork_baseline_ambiguous: parser.fork_baseline_ambiguous,
            fork_baseline: parser.fork_baseline,
            remaining_inherited_totals: parser.remaining_inherited_totals,
            fork_baseline_locally_resolved,
            fork_resume_state,
        })
    }

    /// F2 (upstream 0.48.0 #2648): whether a cached resume offset sits on a real
    /// line boundary. A partial trailing-line write leaves the cached offset
    /// mid-line; resuming there re-parses from mid-line and corrupts the first
    /// resumed record. Returns  when the byte just before  is
    /// not a newline (or the probe fails), signalling the caller to fall back
    /// to a full re-parse from zero.
    pub fn is_line_boundary_offset(file_path: &Path, offset: i64) -> bool {
        use std::io::{Read, Seek};
        if offset <= 0 {
            return true;
        }
        // Session JSONL file sizes fit i64; metadata feeds only boundary probes.
        #[allow(
            clippy::cast_possible_wrap,
            reason = "session JSONL file sizes fit i64"
        )]
        let file_size_i64 = fs::metadata(file_path).map(|m| m.len() as i64);
        let Ok(file_size) = file_size_i64 else {
            return false;
        };
        if offset >= file_size {
            return true;
        }
        let Ok(mut probe) = File::open(file_path) else {
            return false;
        };
        if probe.seek(SeekFrom::Start((offset - 1) as u64)).is_err() {
            return false;
        }
        let mut prev_byte = [0u8; 1];
        probe.read_exact(&mut prev_byte).is_ok() && prev_byte[0] == b'\n'
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
