use super::*;

mod helpers;
mod parser;

use helpers::{
    BoundedJsonlLine, CODEX_JSONL_MAX_LINE_BYTES, nonempty_json_string, parse_rfc3339_timestamp,
    read_bounded_jsonl_line, read_bounded_jsonl_line_until, session_meta_field,
};
use parser::CodexParserState;

#[cfg(test)]
use helpers::{
    CodexFastTotals, bare_usage_totals, codex_timestamp_day_key, codex_totals_from_fast,
    last_usage_delta, parse_codex_timestamp, read_token_totals,
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
            return Ok(CodexSessionMetadata {
                session_id: session_meta_field(&obj, payload, &["id", "session_id", "sessionId"]),
                forked_from_id: session_meta_field(
                    &obj,
                    payload,
                    &[
                        "forked_from_id",
                        "forkedFromId",
                        "parent_session_id",
                        "parentSessionId",
                    ],
                ),
                fork_timestamp: nonempty_json_string(obj.get("timestamp")).or_else(|| {
                    payload.and_then(|value| nonempty_json_string(value.get("timestamp")))
                }),
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

    /// Compare RFC3339 timestamps using parsed instants. Malformed timestamps
    /// are unsafe for fork-baseline reconciliation and therefore fail closed.
    pub(crate) fn codex_timestamp_at_or_before(earlier: &str, later: &str) -> bool {
        match (
            parse_rfc3339_timestamp(earlier),
            parse_rfc3339_timestamp(later),
        ) {
            (Some(earlier), Some(later)) => earlier <= later,
            _ => false,
        }
    }

    /// Parse a Codex JSONL file
    pub fn parse_codex_file(
        file_path: &Path,
        range: &CostUsageDayRange,
        start_offset: i64,
        initial_model: Option<String>,
        initial_totals: Option<CodexTotals>,
    ) -> std::io::Result<CodexParseResult> {
        Self::parse_codex_file_with_state(
            file_path,
            range,
            start_offset,
            initial_model,
            initial_totals,
            None,
            None,
            None,
        )
    }

    /// Parse a Codex file while retaining the timestamp-order state of an
    /// already decoded prefix.  A known prefix only pays for the append
    /// boundary and newly read token events; an unknown legacy prefix is
    /// intentionally rejected by the caller and should be parsed from zero.
    #[allow(
        clippy::too_many_arguments,
        reason = "resume state mirrors the persisted parser cache"
    )]
    pub fn parse_codex_file_with_state(
        file_path: &Path,
        range: &CostUsageDayRange,
        start_offset: i64,
        initial_model: Option<String>,
        initial_totals: Option<CodexTotals>,
        previous_token_timestamp: Option<String>,
        token_timestamps_monotonic: Option<bool>,
        cancel: Option<&AtomicBool>,
    ) -> std::io::Result<CodexParseResult> {
        Self::parse_codex_file_with_state_bounded(
            file_path,
            range,
            start_offset,
            initial_model,
            initial_totals,
            previous_token_timestamp,
            token_timestamps_monotonic,
            cancel,
            None,
        )
    }

    /// Parse a Codex file with an optional cap on bytes newly consumed this pass.
    /// The reader may finish the current bounded JSONL line before yielding.
    #[allow(
        clippy::too_many_arguments,
        reason = "resume state mirrors the persisted parser cache"
    )]
    pub fn parse_codex_file_with_state_bounded(
        file_path: &Path,
        range: &CostUsageDayRange,
        start_offset: i64,
        initial_model: Option<String>,
        initial_totals: Option<CodexTotals>,
        previous_token_timestamp: Option<String>,
        token_timestamps_monotonic: Option<bool>,
        cancel: Option<&AtomicBool>,
        max_bytes_to_read: Option<i64>,
    ) -> std::io::Result<CodexParseResult> {
        Self::parse_codex_file_with_state_bounded_internal(
            file_path,
            range,
            start_offset,
            initial_model,
            initial_totals,
            previous_token_timestamp,
            token_timestamps_monotonic,
            cancel,
            false,
            None,
            max_bytes_to_read,
        )
    }

    /// Parse a Codex file against a caller-owned frozen target. The target is
    /// intentionally separate from the current physical EOF so an active
    /// rollout cannot make a bounded catch-up pass chase its own growth.
    #[allow(
        clippy::too_many_arguments,
        reason = "resume state mirrors the persisted parser cache"
    )]
    pub(crate) fn parse_codex_file_with_state_bounded_target(
        file_path: &Path,
        range: &CostUsageDayRange,
        start_offset: i64,
        initial_model: Option<String>,
        initial_totals: Option<CodexTotals>,
        previous_token_timestamp: Option<String>,
        token_timestamps_monotonic: Option<bool>,
        cancel: Option<&AtomicBool>,
        scan_target_size: Option<i64>,
        max_bytes_to_read: Option<i64>,
    ) -> std::io::Result<CodexParseResult> {
        Self::parse_codex_file_with_state_bounded_internal(
            file_path,
            range,
            start_offset,
            initial_model,
            initial_totals,
            previous_token_timestamp,
            token_timestamps_monotonic,
            cancel,
            false,
            scan_target_size,
            max_bytes_to_read,
        )
    }

    /// Parse a forked Codex child from byte zero with a parent cumulative
    /// baseline. This is intentionally separate from ordinary append-resume
    /// parsing so existing non-fork semantics remain unchanged.
    #[allow(
        clippy::too_many_arguments,
        reason = "fork parse state mirrors the persisted parser cache"
    )]
    pub(crate) fn parse_codex_file_with_state_bounded_fork(
        file_path: &Path,
        range: &CostUsageDayRange,
        initial_totals: CodexTotals,
        cancel: Option<&AtomicBool>,
        max_bytes_to_read: Option<i64>,
    ) -> std::io::Result<CodexParseResult> {
        Self::parse_codex_file_with_state_bounded_internal(
            file_path,
            range,
            0,
            None,
            Some(initial_totals),
            None,
            None,
            cancel,
            true,
            None,
            max_bytes_to_read,
        )
    }

    /// Fork equivalent of [`Self::parse_codex_file_with_state_bounded_target`].
    #[allow(
        clippy::too_many_arguments,
        reason = "fork parse state mirrors the persisted parser cache"
    )]
    pub(crate) fn parse_codex_file_with_state_bounded_fork_target(
        file_path: &Path,
        range: &CostUsageDayRange,
        initial_totals: CodexTotals,
        cancel: Option<&AtomicBool>,
        scan_target_size: Option<i64>,
        max_bytes_to_read: Option<i64>,
    ) -> std::io::Result<CodexParseResult> {
        Self::parse_codex_file_with_state_bounded_internal(
            file_path,
            range,
            0,
            None,
            Some(initial_totals),
            None,
            None,
            cancel,
            true,
            scan_target_size,
            max_bytes_to_read,
        )
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "resume state mirrors the persisted parser cache"
    )]
    fn parse_codex_file_with_state_bounded_internal(
        file_path: &Path,
        range: &CostUsageDayRange,
        start_offset: i64,
        initial_model: Option<String>,
        initial_totals: Option<CodexTotals>,
        previous_token_timestamp: Option<String>,
        token_timestamps_monotonic: Option<bool>,
        cancel: Option<&AtomicBool>,
        fork_baseline_mode: bool,
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

        let safe_start_offset = start_offset.clamp(0, file_size);
        let requested_target_size = scan_target_size
            .unwrap_or(file_size)
            .max(safe_start_offset)
            .min(file_size);

        let mut reader = BufReader::new(file);
        if safe_start_offset > 0 {
            reader.seek(SeekFrom::Start(safe_start_offset as u64))?;
        }

        let mut parser = CodexParserState::with_timestamp_state_and_fork_mode(
            initial_model,
            initial_totals,
            previous_token_timestamp,
            token_timestamps_monotonic,
            fork_baseline_mode,
        );
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
            parser.process_line(line, range);
            committed_bytes = parsed_bytes;
        }

        let effective_target_size = if incomplete_tail && !cancelled && !budget_exhausted {
            committed_bytes
        } else {
            requested_target_size
        };
        let is_complete = !cancelled && !budget_exhausted && parsed_bytes >= effective_target_size;
        let bytes_read = parsed_bytes.saturating_sub(safe_start_offset).max(0);
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
            token_timestamp_comparisons: parser.token_timestamp_comparisons,
            bytes_read,
            is_complete,
            fork_baseline_ambiguous: parser.fork_baseline_ambiguous,
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
