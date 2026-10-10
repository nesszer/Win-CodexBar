//! JSONL Scanner with Caching
//!
//! Incremental log file parsing for Codex and Claude session logs.
//! Supports file-level caching to avoid re-parsing unchanged files.

use crate::core::{CostUsagePricing, ProviderId};
use chrono::{DateTime, NaiveDate, Utc};

#[cfg(test)]
use chrono::Local;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs::{self, File};
use std::hash::{Hash, Hasher};
use std::io::{BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Clone, Default)]
pub struct CachedCostReadStatus {
    pub has_days: bool,
    pub previous_report: Option<CachedCostReport>,
    pub codex_scan_pause_reason: Option<CodexScanPauseReason>,
}

/// Default scanner-side refresh debounce (upstream CostUsageScanner).
pub const DEFAULT_COST_SCAN_REFRESH_MIN_INTERVAL_SECS: u64 = 60;
/// Default number of dirty Codex rollouts inspected in one refresh.
pub const DEFAULT_CODEX_CANDIDATE_LIMIT: usize = 512;
/// Default maximum newly-read bytes from one Codex rollout in one refresh.
pub const DEFAULT_CODEX_MAX_SESSION_FILE_BYTES: i64 = 256 * 1024 * 1024;
/// Default maximum newly-read Codex bytes across one refresh.
pub const DEFAULT_CODEX_MAX_SCAN_BYTES_PER_REFRESH: i64 = 512 * 1024 * 1024;

/// Options for a cost scan pass (disk-cache-backed full inspections).
///
/// Default debounce is 60s between full disk inspections when a
/// [`CostUsageCache`] is present. Pass [`CostScanOptions::app_driven`] (interval 0)
/// for explicit/CLI refreshes. Production [`crate::cost_scanner::CostScanner`]
/// honors these options and persists cache under `{cache}/CodexBar/cost-usage/`.
#[derive(Debug, Clone, Copy)]
pub struct CostScanOptions {
    /// Minimum seconds between disk-cache-backed full inspections.
    /// Set to 0 to force a fresh scan (app-driven / forceRefresh).
    pub refresh_min_interval_secs: u64,
    /// A16 (upstream 0.48.0 --provider-native-only): when false, exclude
    /// pi/OMP-compatible agent session mirrors from Codex/Claude cost history.
    /// Defaults to true (include mirrors) for backward compatibility.
    pub include_pi_sessions: bool,
    /// Maximum bytes newly read from one Codex rollout during a refresh.
    pub codex_max_session_file_bytes: i64,
    /// Maximum Codex JSONL bytes newly read across one refresh.
    pub codex_max_scan_bytes_per_refresh: i64,
    /// Maximum dirty/new Codex rollout candidates processed per refresh.
    pub codex_candidate_limit: usize,
    /// Prefer recent Codex rollouts while historical catch-up is pending.
    pub prefer_newest_codex_sessions_first: bool,
}

impl Default for CostScanOptions {
    fn default() -> Self {
        Self {
            refresh_min_interval_secs: DEFAULT_COST_SCAN_REFRESH_MIN_INTERVAL_SECS,
            include_pi_sessions: true,
            codex_max_session_file_bytes: DEFAULT_CODEX_MAX_SESSION_FILE_BYTES,
            codex_max_scan_bytes_per_refresh: DEFAULT_CODEX_MAX_SCAN_BYTES_PER_REFRESH,
            codex_candidate_limit: DEFAULT_CODEX_CANDIDATE_LIMIT,
            prefer_newest_codex_sessions_first: true,
        }
    }
}

impl CostScanOptions {
    /// App-driven or forced refresh: skip the scanner debounce entirely.
    pub fn app_driven() -> Self {
        Self {
            refresh_min_interval_secs: 0,
            ..Self::default()
        }
    }

    /// Whether this pass was requested by an explicit/app-driven refresh.
    /// The zero debounce used by app-driven scans is the existing refresh
    /// state, so no second force/resume flag is needed.
    pub fn is_app_driven(&self) -> bool {
        self.refresh_min_interval_secs == 0
    }

    /// Whether a prior scan at `last_scan_unix_ms` is still within the debounce window.
    pub fn should_skip_scan(&self, last_scan_unix_ms: i64, now_unix_ms: i64) -> bool {
        // Debounce intervals are seconds-scale config values, far below i64::MAX.
        #[allow(
            clippy::cast_possible_wrap,
            reason = "debounce interval in seconds is a small config value that cannot exceed i64::MAX"
        )]
        let refresh_ms = (self.refresh_min_interval_secs as i64).saturating_mul(1000);
        refresh_ms > 0
            && last_scan_unix_ms > 0
            && now_unix_ms.saturating_sub(last_scan_unix_ms) <= refresh_ms
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CacheStamp {
    byte_len: usize,
    content_hash: u64,
}

impl CacheStamp {
    fn from_bytes(bytes: &[u8]) -> Self {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        bytes.hash(&mut hasher);
        Self {
            byte_len: bytes.len(),
            content_hash: hasher.finish(),
        }
    }

    /// Stamp the serialized cache while ignoring its scan timestamp. This
    /// avoids allocating a second full-size JSON buffer just to normalize one
    /// scalar before comparing cache payloads.
    fn from_cache_payload(bytes: &[u8]) -> Option<Self> {
        const FIELD: &[u8] = b"\"last_scan_unix_ms\":";
        let value_start = bytes
            .windows(FIELD.len())
            .position(|window| window == FIELD)?
            + FIELD.len();
        let mut value_end = value_start;
        if bytes.get(value_end) == Some(&b'-') {
            value_end += 1;
        }
        let digits_start = value_end;
        while bytes.get(value_end).is_some_and(u8::is_ascii_digit) {
            value_end += 1;
        }
        if value_end == digits_start {
            return None;
        }

        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        hasher.write(&bytes[..value_start]);
        hasher.write(&bytes[value_end..]);
        Some(Self {
            byte_len: bytes.len() - (value_end - value_start),
            content_hash: hasher.finish(),
        })
    }
}

/// Terminal reason for a bounded Codex catch-up pause.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodexScanPauseReason {
    /// A bounded pass left work queued without consuming any new source data.
    NoProgress,
    /// The source could not be inspected reliably; keep the validated report until retry.
    Error(String),
}

/// Cache for scanned file data
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CostUsageCache {
    /// Codex cache schema. Version 0 is any pre-64-bit cache and must be rebuilt.
    #[serde(default)]
    pub codex_cache_schema_version: u32,
    /// Last scan timestamp in milliseconds
    pub last_scan_unix_ms: i64,
    /// Per-file usage data
    #[serde(serialize_with = "save_skip::sorted_map")]
    pub files: HashMap<String, CostUsageFileUsage>,
    /// Aggregated daily data: day_key -> model -> [input, cached, output, reasoning?]
    #[serde(serialize_with = "save_skip::sorted_days")]
    pub days: HashMap<String, HashMap<String, Vec<i64>>>,
    /// Inclusive range covered by the last successful full inspection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scan_since_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scan_until_key: Option<String>,
    /// Last validated cost report retained when the persisted cache needs future
    /// catch-up after trimming or expiry. A completed in-memory scan may still
    /// leave this populated when persistence-budget pruning follows; current
    /// publication completeness is carried separately on `CostSummary`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_report: Option<CachedCostReport>,
    /// Dirty/incomplete Codex rollouts deferred by the foreground work budget.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub codex_pending_paths: Vec<String>,
    /// True while bounded Codex catch-up has not completed for this window.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub codex_scan_incomplete: bool,
    /// Earliest scan start retained for the active Codex catch-up cycle.
    ///
    /// This is deliberately separate from `scan_since_key`: that field is the
    /// last successfully completed scan and must not change merely because a
    /// narrower report was requested while catch-up is pending.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex_pending_scan_since_key: Option<String>,
    /// Scan end and source identity for the active Codex catch-up cycle.
    /// Requests may retain the pending start only when all of these remain
    /// compatible with the persisted work.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex_pending_scan_until_key: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub codex_pending_scan_root_paths: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex_pending_scan_timezone: Option<String>,
    /// Terminal catch-up pause attached to the existing incomplete state. A
    /// background scan must not clear or retry this state; an app-driven
    /// refresh clears it before starting the next pass.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex_scan_pause_reason: Option<CodexScanPauseReason>,
    /// Cached request rows retained as source evidence for Codex recovery.
    ///
    /// This is separate from `files` because the Windows cache currently
    /// persists aggregate day/model totals rather than the native request-row
    /// representation used by upstream.  The map is optional on disk so old
    /// caches remain valid and can be upgraded lazily.
    #[serde(
        default,
        skip_serializing_if = "HashMap::is_empty",
        serialize_with = "save_skip::sorted_map"
    )]
    pub codex_source_rows: HashMap<String, CodexSourceRowCache>,
    /// Request rows of fork-shaped Codex files (a `forked_from_id` or a
    /// parent-baseline lineage), built from the same parsed records as the
    /// file's day totals. Source-row evidence skips these files, so this map
    /// is what lets Priority trace evidence reach forked sessions.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub codex_fork_rows: HashMap<String, Vec<CodexSourceUsageRow>>,
    /// Priority (Fast) turn evidence read from the Codex trace database.
    /// Applied as a pricing overlay whenever day totals are rebuilt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex_priority_turns_cursor: Option<CodexPriorityTurnsCursor>,
    /// Trace-database path and presence seen by the last full scan
    /// (`sqlite:<path>` or `missing:<path>`, upstream
    /// `codexPriorityMetadataKey`). A database that appears later bypasses
    /// the scan debounce once so its evidence is applied promptly.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex_priority_metadata_key: Option<String>,
    /// Zone the day keys were bucketed in (upstream `timeZoneIdentifier`).
    /// A cache from another zone is rebuilt; caches written before the stamp
    /// existed are kept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bucket_time_zone: Option<String>,
    /// Content stamp of the decoded on-disk baseline. This is process-local
    /// and omitted from JSON so a stale reader cannot replace a newer cache.
    #[serde(skip)]
    pub(crate) loaded_stamp: Option<Option<CacheStamp>>,
    /// Stamp of the loaded cache payload with `last_scan_unix_ms` omitted.
    /// This is separate from `loaded_stamp`, which still protects stale writes.
    #[serde(skip)]
    pub(crate) loaded_payload_stamp: Option<CacheStamp>,
}

/// Pricing evidence attached to one cached Codex request row.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodexSourcePricingEvidence {
    pub pricing_model: Option<String>,
    pub pricing_mode: Option<String>,
}

/// A request row recovered from a complete Codex JSONL source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodexSourceUsageRow {
    pub day_key: String,
    /// Exact event time when the source exposed one. Legacy rows omit it and
    /// remain valid for daily history but cannot be split at a quota reset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<DateTime<Utc>>,
    pub model: String,
    pub input: i64,
    pub cached: i64,
    pub output: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<i64>,
    /// End offset of the source JSONL line that produced this row.
    /// Zero means the row came from a legacy cache and cannot be replayed
    /// safely across an append boundary.
    #[serde(default)]
    pub source_end_offset: i64,
    #[serde(default)]
    pub pricing: CodexSourcePricingEvidence,
    /// Codex turn (`task_started`) the request belongs to; matches Priority
    /// trace evidence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
}

/// Source identity and rows retained for a cached Codex file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodexSourceRowCache {
    /// Platform file identity of the source at cache time. A cache entry is
    /// only built when identity succeeds, so the field is always usable.
    pub file_identity: String,
    pub size: i64,
    pub mtime_unix_ms: i64,
    pub prefix_hash: u64,
    pub rows: Vec<CodexSourceUsageRow>,
}

/// Per-file usage tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostUsageFileUsage {
    /// File modification time in milliseconds
    pub mtime_unix_ms: i64,
    /// File size in bytes
    pub size: i64,
    /// Stable source identity used to detect same-path replacement without
    /// opening the raw token history. Legacy entries may omit this field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex_file_identity: Option<String>,
    /// Daily usage data extracted from this file
    #[serde(serialize_with = "save_skip::sorted_days")]
    pub days: HashMap<String, HashMap<String, Vec<i64>>>,
    /// Bytes parsed so far (for incremental parsing)
    pub parsed_bytes: Option<i64>,
    /// Frozen logical end of the scan target. A growing rollout may have a
    /// physical tail beyond this boundary; that tail remains queued until a
    /// later pass can consume complete records from it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex_scan_target_size: Option<i64>,
    /// Last model seen (for delta calculations)
    pub last_model: Option<String>,
    /// Last token totals (for delta calculations)
    pub last_totals: Option<CodexTotals>,
    /// Whether the parsed Codex token timestamps were non-decreasing.
    ///
    /// `None` is an old cache entry that has never had its timestamp order
    /// validated.  Such an entry must not use the append-only fast path until
    /// a full parse establishes this state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex_token_timestamps_monotonic: Option<bool>,
    /// The last parsed Codex token timestamp, used to validate an appended
    /// suffix without replaying the cached prefix.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex_last_token_timestamp: Option<String>,
    /// Native Codex session identity from the first authoritative session_meta row.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex_session_id: Option<String>,
    /// Native Codex parent session identity for forked rollouts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex_forked_from_id: Option<String>,
    /// Native Codex fork accounting state. This preserves the normalized
    /// inherited baseline across bounded scans and process restarts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex_fork_accounting_state: Option<CodexForkAccountingState>,
    /// Native Codex session lineage. This distinguishes a root session from a
    /// paginated subagent whose ancestry is independent for billing purposes.
    #[serde(default, skip_serializing_if = "CodexSessionLineage::is_root")]
    pub codex_lineage: CodexSessionLineage,
    /// Native Codex fork timestamp used for safe parent-baseline validation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex_fork_timestamp: Option<String>,
    /// True when a fork cannot be billed safely until its parent is available.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub codex_unresolved_fork_parent: bool,
}

/// Billing-relevant Codex session lineage.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodexSessionLineage {
    #[default]
    Root,
    Independent,
    Child,
}

impl CodexSessionLineage {
    fn is_root(&self) -> bool {
        matches!(self, Self::Root)
    }

    pub(crate) fn uses_parent_baseline(self) -> bool {
        matches!(self, Self::Child)
    }
}

/// Lightweight identity metadata read from the first authoritative Codex
/// `session_meta` row.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct CodexSessionMetadata {
    pub session_id: Option<String>,
    pub forked_from_id: Option<String>,
    pub lineage: CodexSessionLineage,
    pub fork_timestamp: Option<String>,
    pub history_base_thread_id: Option<String>,
    pub is_subagent: bool,
    pub subagent_history_start_ordinal: Option<i64>,
}

/// Running totals for Codex token counting
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodexTotals {
    pub input: i64,
    pub cached: i64,
    pub output: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<i64>,
}

/// Persisted accounting state for a Codex fork whose cumulative counters may
/// include a paginated continuation of an earlier thread.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexForkAccountingState {
    pub session_id: Option<String>,
    pub forked_from_id: Option<String>,
    pub history_base_thread_id: Option<String>,
    pub fork_timestamp: Option<String>,
    pub inherited_totals: Option<CodexTotals>,
    /// Timestamp of the first token event in this fork's log. A descendant
    /// forked before it uses this fork's inherited counter origin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_token_timestamp: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remaining_inherited_totals: Option<CodexTotals>,
    /// True when the child log itself supplied enough copied-prefix history to
    /// establish the inherited baseline without consulting a parent cache row.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub locally_resolved: bool,
    /// Parser state of an unfinished bounded parse, so the next pass continues
    /// at `parsed_bytes` instead of rereading the fork from byte zero. Absent
    /// once the parse reaches its target, and in caches written before it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume: Option<CodexForkResumeState>,
}

/// Fork parser state that the per-file cache fields do not already carry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodexForkResumeState {
    /// Parent baseline the interrupted parse started from. A validated parent
    /// that now supplies a different baseline restarts the parse.
    pub parse_baseline: CodexTotals,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub totals_watermark: Option<CodexTotals>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub saw_interleaved_totals: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub paginated_baseline_checked: bool,
}

/// Snapshot of the last validated cost report, persisted so spend surfaces keep
/// showing totals while a rescan catches up after the cache is trimmed or the
/// debounce window expires (upstream 0.48.0 #2628). See the cache-budget module
/// for the save/load overshoot contract.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedCostReport {
    /// Total cost in USD for the reported window.
    pub total_cost_usd: f64,
    /// Total input tokens.
    pub input_tokens: i64,
    /// Total cached tokens.
    pub cached_tokens: i64,
    /// Total output tokens.
    pub output_tokens: i64,
    /// Total reasoning output tokens when every contributing packed row knows it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_tokens: Option<i64>,
    /// Number of sessions contributing.
    pub sessions_count: i32,
    /// ISO 8601 timestamp when this report was generated.
    pub updated_at: Option<String>,
    /// Whether the report was marked partial (unpriced routing rows retained).
    #[serde(default)]
    pub partial: bool,
}

/// Result of parsing a Codex file
#[derive(Debug)]
pub struct CodexParseResult {
    /// Individual token-count deltas used for per-request pricing, paired
    /// with the end offset of the source JSONL line that produced each.
    pub records: Vec<(CodexUsageRecord, i64)>,
    /// Bytes parsed
    pub parsed_bytes: i64,
    /// Stable logical target reached by this parse. This may be behind the
    /// physical EOF when the tail ended inside an incomplete JSONL record.
    pub scan_target_size: i64,
    /// Last model seen
    pub last_model: Option<String>,
    /// Last totals seen
    pub last_totals: Option<CodexTotals>,
    /// Timestamp-order state for the parsed token history.
    pub token_timestamps_monotonic: Option<bool>,
    /// Last token timestamp observed by the parser.
    pub last_token_timestamp: Option<String>,
    /// First token timestamp of the parsed history. A resumed fork parse
    /// carries its saved value forward; resumed standard parses see only their
    /// suffix, so fork accounting never reads it from them.
    pub first_token_timestamp: Option<String>,
    /// Number of timestamp comparisons performed while validating this parse.
    pub token_timestamp_comparisons: u64,
    /// Newly consumed bytes in this parse pass.
    pub bytes_read: i64,
    /// Whether this pass reached the file's current EOF without cancellation/budget deferral.
    pub is_complete: bool,
    /// A fork-baseline parse observed a cumulative component below the inherited
    /// parent baseline. The child must be discarded rather than billed as fresh.
    pub fork_baseline_ambiguous: bool,
    /// Effective inherited baseline after normalizing a paginated continuation.
    pub fork_baseline: Option<CodexTotals>,
    /// Remaining inherited counters used when a fork emits last-only rows.
    pub remaining_inherited_totals: Option<CodexTotals>,
    pub fork_baseline_locally_resolved: bool,
    /// State to continue an unfinished parent-baseline fork parse. `None` once
    /// the parse is complete and for every other parse mode.
    pub fork_resume_state: Option<CodexForkResumeState>,
}

/// A billable Codex token-count delta.
#[derive(Debug, Clone)]
pub struct CodexUsageRecord {
    pub day_key: String,
    pub timestamp: Option<DateTime<Utc>>,
    pub model: String,
    pub input: i64,
    pub cached: i64,
    pub output: i64,
    pub reasoning: Option<i64>,
    pub turn_id: Option<String>,
}

/// Day range for scanning
pub struct CostUsageDayRange {
    pub since_key: String,
    pub until_key: String,
    pub scan_since_key: String,
    pub scan_until_key: String,
}

impl CostUsageDayRange {
    pub fn new(since: NaiveDate, until: NaiveDate) -> Self {
        let since_minus_one = since - chrono::Duration::days(1);
        let until_plus_one = until + chrono::Duration::days(1);

        Self {
            since_key: Self::day_key(since),
            until_key: Self::day_key(until),
            scan_since_key: Self::day_key(since_minus_one),
            scan_until_key: Self::day_key(until_plus_one),
        }
    }

    pub fn day_key(date: NaiveDate) -> String {
        date.format("%Y-%m-%d").to_string()
    }

    pub fn is_in_range(day_key: &str, since: &str, until: &str) -> bool {
        day_key >= since && day_key <= until
    }

    pub fn parse_day_key(key: &str) -> Option<NaiveDate> {
        NaiveDate::parse_from_str(key, "%Y-%m-%d").ok()
    }
}

/// JSONL Scanner for cost/usage logs
pub struct JsonlScanner;
pub(crate) mod codex;
pub(crate) use codex::priority::CodexPriorityOverlay;
pub use codex::priority::{
    CODEX_PRIORITY_COMPLETED_MODEL_RETENTION_LIMIT, CodexPriorityCursorAnchor,
    CodexPriorityTurnMetadata, CodexPriorityTurnsCursor,
};
pub(crate) use codex::{CodexForkParseResume, CodexParseMode};
mod persistence;
mod save_skip;
#[cfg(test)]
#[path = "jsonl_scanner/tests/fixtures.rs"]
pub(crate) mod test_fixtures;
pub(crate) use codex::source_rows::{
    read_source_rows, recover_rows, row_cache, row_cache_matches, row_cache_needs_recovery,
    row_priced_model, rows_from_records,
};

impl JsonlScanner {
    /// Whether a cached scan should be reused under `options` (issue #2089).
    pub fn should_skip_cached_scan(
        cache: &CostUsageCache,
        options: CostScanOptions,
        now_unix_ms: i64,
    ) -> bool {
        options.should_skip_scan(cache.last_scan_unix_ms, now_unix_ms)
    }

    pub(crate) fn cached_cost_report_from_days(cache: &CostUsageCache) -> CachedCostReport {
        Self::cached_cost_report_from_days_filtered(cache, None)
    }

    /// Build a retained report for one requested reporting window.
    ///
    /// Codex catch-up can retain days outside the active dashboard window while
    /// it processes historical files. A retained report must therefore use the
    /// requested days rather than summing every day that happens to remain in
    /// the cache. The cache scan timestamp is the measurement time for the
    /// report; this keeps a stale report honest while a later bounded pass is
    /// still pending.
    pub(crate) fn cached_cost_report_for_range(
        cache: &CostUsageCache,
        range: &CostUsageDayRange,
    ) -> CachedCostReport {
        Self::cached_cost_report_from_days_filtered(
            cache,
            Some((&range.since_key, &range.until_key)),
        )
    }

    fn cached_cost_report_from_days_filtered(
        cache: &CostUsageCache,
        range: Option<(&str, &str)>,
    ) -> CachedCostReport {
        let mut total_cost_usd = 0.0;
        let mut input_tokens = 0_i64;
        let mut cached_tokens = 0_i64;
        let mut output_tokens = 0_i64;
        let mut reasoning_tokens = 0_i64;
        let mut reasoning_known = true;
        let mut partial = false;

        let day_is_included = |day_key: &str| {
            range.is_none_or(|(since, until)| CostUsageDayRange::is_in_range(day_key, since, until))
        };

        for (day_key, models) in &cache.days {
            if !day_is_included(day_key) {
                continue;
            }
            let pricing_day = NaiveDate::parse_from_str(day_key, "%Y-%m-%d").ok();
            for (model, values) in models {
                let input = values.first().copied().unwrap_or(0).max(0);
                let cached = values.get(1).copied().unwrap_or(0).max(0);
                let output = values.get(2).copied().unwrap_or(0).max(0);
                input_tokens = input_tokens.saturating_add(input);
                cached_tokens = cached_tokens.saturating_add(cached);
                output_tokens = output_tokens.saturating_add(output);
                if input > 0 || cached > 0 || output > 0 {
                    if let Some(reasoning) = values.get(3).copied() {
                        reasoning_tokens =
                            reasoning_tokens.saturating_add(reasoning.max(0).min(output));
                    } else {
                        reasoning_known = false;
                    }
                }

                if CostUsagePricing::is_codex_unattributed_model(model) {
                    partial = true;
                    continue;
                }
                if !CostUsagePricing::counts_toward_codex_subscription(model) {
                    continue;
                }
                let priced = CostUsagePricing::codex_day_aggregate_cost_usd(
                    model,
                    u64::try_from(input).unwrap_or(0),
                    u64::try_from(cached).unwrap_or(0),
                    u64::try_from(output).unwrap_or(0),
                    pricing_day,
                );
                if let Some(cost) = priced {
                    total_cost_usd += cost;
                } else {
                    partial = true;
                }
            }
        }

        let sessions_count = i32::try_from(
            cache
                .files
                .values()
                .filter(|usage| usage.days.keys().any(|day| day_is_included(day)))
                .count(),
        )
        .unwrap_or(i32::MAX);
        let measured_at = if cache.last_scan_unix_ms > 0 {
            DateTime::<Utc>::from_timestamp_millis(cache.last_scan_unix_ms)
                .map(|timestamp| timestamp.to_rfc3339())
        } else {
            None
        };
        CachedCostReport {
            total_cost_usd,
            input_tokens,
            cached_tokens,
            output_tokens,
            reasoning_tokens: reasoning_known.then_some(reasoning_tokens),
            sessions_count,
            updated_at: Some(measured_at.unwrap_or_else(|| Utc::now().to_rfc3339())),
            partial,
        }
    }

    /// Merge one Codex record into a packed day/model row. A three-slot row is
    /// deliberately treated as reasoning-unknown, including when a known row
    /// is merged into an existing legacy row.
    pub(crate) fn merge_codex_record_into_packed(packed: &mut Vec<i64>, record: &CodexUsageRecord) {
        let was_empty = packed.is_empty();
        if packed.len() < 3 {
            packed.resize(3, 0);
        }
        packed[0] = packed[0].saturating_add(record.input.max(0));
        packed[1] = packed[1].saturating_add(record.cached.max(0));
        packed[2] = packed[2].saturating_add(record.output.max(0));

        match record.reasoning {
            Some(reasoning) if was_empty => packed.push(reasoning.max(0).min(record.output.max(0))),
            Some(reasoning) if packed.len() >= 4 => {
                packed[3] = packed[3].saturating_add(reasoning.max(0).min(record.output.max(0)));
            }
            Some(_) => {}
            None => packed.truncate(3),
        }
    }

    /// Whether `cache` covers the requested day window (for debounce short-circuit).
    pub fn cache_covers_range(cache: &CostUsageCache, range: &CostUsageDayRange) -> bool {
        match (&cache.scan_since_key, &cache.scan_until_key) {
            (Some(since), Some(until)) => {
                since.as_str() <= range.since_key.as_str()
                    && until.as_str() >= range.until_key.as_str()
            }
            _ => !cache.days.is_empty() || !cache.files.is_empty(),
        }
    }
}

use chrono::Datelike;
