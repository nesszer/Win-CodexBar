//! JSONL Scanner with Caching
//!
//! Incremental log file parsing for Codex and Claude session logs.
//! Supports file-level caching to avoid re-parsing unchanged files.

#![allow(
    dead_code,
    reason = "scanner types are deserialized from JSONL for parsing but not all are read"
)]

use crate::core::{CostUsagePricing, ProviderId};
use chrono::{NaiveDate, Utc};

#[cfg(test)]
use chrono::{DateTime, Local};
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

#[derive(Deserialize, Default)]
struct CachedCostReadStatusProjection {
    #[serde(
        default,
        rename = "days",
        deserialize_with = "deserialize_nonempty_object"
    )]
    has_days: bool,
    #[serde(default)]
    previous_report: Option<CachedCostReport>,
    #[serde(default)]
    codex_scan_pause_reason: Option<CodexScanPauseReason>,
}

fn deserialize_nonempty_object<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{IgnoredAny, MapAccess, Visitor};

    struct NonemptyObjectVisitor;

    impl<'de> Visitor<'de> for NonemptyObjectVisitor {
        type Value = bool;

        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("a JSON object")
        }

        fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
        where
            A: MapAccess<'de>,
        {
            let mut nonempty = false;
            while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {
                nonempty = true;
            }
            Ok(nonempty)
        }
    }

    deserializer.deserialize_map(NonemptyObjectVisitor)
}
/// Maximum retained Codex JSONL line size (upstream session-metadata bound).
const CODEX_JSONL_MAX_LINE_BYTES: usize = 256 * 1024;

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
    /// Last scan timestamp in milliseconds
    pub last_scan_unix_ms: i64,
    /// Per-file usage data
    pub files: HashMap<String, CostUsageFileUsage>,
    /// Aggregated daily data: day_key -> model -> [input, cached, output, reasoning?]
    pub days: HashMap<String, HashMap<String, Vec<i32>>>,
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
    /// Content stamp of the decoded on-disk baseline. This is process-local
    /// and omitted from JSON so a stale reader cannot replace a newer cache.
    #[serde(skip)]
    pub(crate) loaded_stamp: Option<Option<CacheStamp>>,
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
    pub days: HashMap<String, HashMap<String, Vec<i32>>>,
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
    /// Native Codex fork timestamp used for safe parent-baseline validation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex_fork_timestamp: Option<String>,
    /// True when a fork cannot be billed safely until its parent is available.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub codex_unresolved_fork_parent: bool,
}

/// Lightweight identity metadata read from the first authoritative Codex
/// `session_meta` row.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct CodexSessionMetadata {
    pub session_id: Option<String>,
    pub forked_from_id: Option<String>,
    pub fork_timestamp: Option<String>,
}

/// Running totals for Codex token counting
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexTotals {
    pub input: i32,
    pub cached: i32,
    pub output: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<i32>,
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
    pub input_tokens: i32,
    /// Total cached tokens.
    pub cached_tokens: i32,
    /// Total output tokens.
    pub output_tokens: i32,
    /// Total reasoning output tokens when every contributing packed row knows it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_tokens: Option<i32>,
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
    /// Individual token-count deltas used for per-request pricing.
    pub records: Vec<CodexUsageRecord>,
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
    /// Number of timestamp comparisons performed while validating this parse.
    pub token_timestamp_comparisons: u64,
    /// Newly consumed bytes in this parse pass.
    pub bytes_read: i64,
    /// Whether this pass reached the file's current EOF without cancellation/budget deferral.
    pub is_complete: bool,
    /// A fork-baseline parse observed a cumulative component below the inherited
    /// parent baseline. The child must be discarded rather than billed as fresh.
    pub fork_baseline_ambiguous: bool,
}

/// A billable Codex token-count delta.
#[derive(Debug, Clone)]
pub struct CodexUsageRecord {
    pub day_key: String,
    pub model: String,
    pub input: i32,
    pub cached: i32,
    pub output: i32,
    pub reasoning: Option<i32>,
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
mod codex;

impl JsonlScanner {
    /// Whether a cached scan should be reused under `options` (issue #2089).
    pub fn should_skip_cached_scan(
        cache: &CostUsageCache,
        options: CostScanOptions,
        now_unix_ms: i64,
    ) -> bool {
        options.should_skip_scan(cache.last_scan_unix_ms, now_unix_ms)
    }

    /// Load cache from disk.
    ///
    /// Refuses to decode artifacts larger than the load cap
    /// (`crate::core::CostUsageCacheBudget::MAX_LOAD_BYTES`); an oversized artifact is
    /// cheaper to rebuild bounded than to decode in one shot, so the caller
    /// gets a fresh empty cache instead (upstream 0.48.0 overshoot contract).
    /// Only Codex persistence is bounded; other providers load unbounded.
    pub fn load_cache(provider: ProviderId, cache_root: Option<&Path>) -> CostUsageCache {
        let cache_path = Self::cache_path(provider, cache_root);

        if crate::core::is_bounded_provider(provider) {
            // Artifacts are bounded by MAX_LOAD_BYTES (320 MiB), fitting usize on
            // any supported target even before the budget comparison below.
            #[allow(
                clippy::cast_possible_truncation,
                reason = "bounded artifacts fit usize on any supported target"
            )]
            let file_bytes = crate::core::artifact_file_size(&cache_path) as usize;
            if file_bytes > crate::core::CostUsageCacheBudget::MAX_LOAD_BYTES {
                return CostUsageCache::default();
            }
        }

        if let Ok(contents) = fs::read_to_string(&cache_path)
            && let Ok(mut cache) = serde_json::from_str::<CostUsageCache>(&contents)
        {
            cache.loaded_stamp = Some(Some(CacheStamp::from_bytes(contents.as_bytes())));
            return cache;
        }

        // Track a missing or unreadable baseline separately from a manually
        // constructed cache so a concurrent first writer can invalidate it.
        CostUsageCache {
            loaded_stamp: Some(Self::cache_stamp(&cache_path)),
            ..CostUsageCache::default()
        }
    }

    /// Read only the cache metadata needed by presentation surfaces.
    ///
    /// v0.56.0 performance parity: skip raw per-file scanner state and day
    /// payloads when callers only need stale/catch-up status.
    pub fn load_cache_status(
        provider: ProviderId,
        cache_root: Option<&Path>,
    ) -> CachedCostReadStatus {
        let cache_path = Self::cache_path(provider, cache_root);
        if crate::core::is_bounded_provider(provider) {
            #[allow(
                clippy::cast_possible_truncation,
                reason = "bounded artifacts fit usize on any supported target"
            )]
            let file_bytes = crate::core::artifact_file_size(&cache_path) as usize;
            if file_bytes > crate::core::CostUsageCacheBudget::MAX_LOAD_BYTES {
                return CachedCostReadStatus::default();
            }
        }

        let Ok(file) = File::open(cache_path) else {
            return CachedCostReadStatus::default();
        };
        let Ok(projection) =
            serde_json::from_reader::<_, CachedCostReadStatusProjection>(BufReader::new(file))
        else {
            return CachedCostReadStatus::default();
        };
        CachedCostReadStatus {
            has_days: projection.has_days,
            previous_report: projection.previous_report,
            codex_scan_pause_reason: projection.codex_scan_pause_reason,
        }
    }
    pub(crate) fn cached_cost_report_from_days(cache: &CostUsageCache) -> CachedCostReport {
        let mut total_cost_usd = 0.0;
        let mut input_tokens = 0_i32;
        let mut cached_tokens = 0_i32;
        let mut output_tokens = 0_i32;
        let mut reasoning_tokens = 0_i32;
        let mut reasoning_known = true;
        let mut partial = false;

        for (day_key, models) in &cache.days {
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
                let priced = pricing_day
                    .and_then(|day| {
                        CostUsagePricing::codex_cost_usd_at_date(
                            model,
                            u64::try_from(input).unwrap_or(0),
                            u64::try_from(cached).unwrap_or(0),
                            u64::try_from(output).unwrap_or(0),
                            day,
                        )
                    })
                    .or_else(|| {
                        CostUsagePricing::codex_cost_usd(
                            model,
                            u64::try_from(input).unwrap_or(0),
                            u64::try_from(cached).unwrap_or(0),
                            u64::try_from(output).unwrap_or(0),
                        )
                    });
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
                .filter(|usage| !usage.days.is_empty())
                .count(),
        )
        .unwrap_or(i32::MAX);
        CachedCostReport {
            total_cost_usd,
            input_tokens,
            cached_tokens,
            output_tokens,
            reasoning_tokens: reasoning_known.then_some(reasoning_tokens),
            sessions_count,
            updated_at: Some(Utc::now().to_rfc3339()),
            partial,
        }
    }

    /// Merge one Codex record into a packed day/model row. A three-slot row is
    /// deliberately treated as reasoning-unknown, including when a known row
    /// is merged into an existing legacy row.
    pub(crate) fn merge_codex_record_into_packed(packed: &mut Vec<i32>, record: &CodexUsageRecord) {
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

    /// Save cache to disk (temp sibling + copy into place).
    ///
    /// Before encoding, prunes the cache to the persistence budget so the
    /// artifact stays small enough to decode in one shot (upstream 0.48.0
    /// #2637). Only Codex persistence is bounded; the overshoot contract lets
    /// the encoded size exceed `MAX_FILE_BYTES` up to `MAX_LOAD_BYTES`
    /// when protected (partially parsed) entries cannot be trimmed further.
    pub fn save_cache(provider: ProviderId, cache: &mut CostUsageCache, cache_root: Option<&Path>) {
        Self::save_cache_with_limit(
            provider,
            cache,
            cache_root,
            crate::core::CostUsageCacheBudget::MAX_LOAD_BYTES,
        );
    }

    /// Save with an explicit post-encode refusal limit, injected by tests.
    ///
    /// Identical to `save_cache` except the post-encode oversize check uses
    /// `max_load_bytes` rather than the production `MAX_LOAD_BYTES` const.
    /// Production callers MUST use `save_cache`; this helper exists so the
    /// refusal / stale-destination removal can be exercised without encoding a
    /// ~320 MiB test artifact.
    fn save_cache_with_limit(
        provider: ProviderId,
        cache: &mut CostUsageCache,
        cache_root: Option<&Path>,
        max_load_bytes: usize,
    ) {
        let cache_path = Self::cache_path(provider, cache_root);

        // A decoded baseline is only valid for the file contents that produced
        // it. Refuse a stale writer before pruning or creating directories so a
        // concurrent scan remains authoritative.
        if let Some(expected) = cache.loaded_stamp.as_ref()
            && Self::cache_stamp(&cache_path).as_ref() != expected.as_ref()
        {
            return;
        }

        let Some(parent) = cache_path.parent() else {
            return;
        };
        // Best-effort cache dir creation; a missing dir surfaces as the write error below.
        let _dir_created = fs::create_dir_all(parent);

        if crate::core::is_bounded_provider(provider) {
            // v0.55.1 #3051: snapshot the fully validated report BEFORE persistence
            // pruning. If budget trimming creates a catch-up cycle, this is the
            // established spend/tokens users should keep seeing until replacement
            // history finishes, not a zero-cost reconstruction of the trimmed cache.
            let established_report = cache
                .previous_report
                .clone()
                .unwrap_or_else(|| Self::cached_cost_report_from_days(cache));
            let pruned = crate::core::prune_out_of_window_for_budget(
                &mut cache.files,
                &mut cache.days,
                cache.scan_since_key.as_deref(),
                cache.scan_until_key.as_deref(),
                false,
            );
            let estimate = crate::core::estimated_cache_bytes(&cache.files, &cache.days);
            let trimmed = if estimate > crate::core::CostUsageCacheBudget::MAX_FILE_BYTES {
                crate::core::trim_in_window_for_budget(
                    &mut cache.files,
                    &mut cache.days,
                    cache.scan_since_key.as_deref(),
                    cache.scan_until_key.as_deref(),
                    crate::core::CostUsageCacheBudget::MAX_FILE_BYTES,
                )
            } else {
                Vec::new()
            };
            // A16 (upstream 0.48.0): when entries were trimmed for budget, the persisted
            // artifact no longer covers the full window — set previous_report so the
            // next refresh can signal catch-up is pending (and spend surfaces can show
            // the last-validated snapshot during the rescan).
            if (!pruned.is_empty() || !trimmed.is_empty()) && cache.previous_report.is_none() {
                cache.previous_report = Some(established_report);
            }
        }

        let Ok(json) = serde_json::to_string(cache) else {
            return;
        };

        // F19 (upstream 0.48.0): after bounded encode, if the artifact still
        // exceeds MAX_LOAD_BYTES, refuse persistence. Also remove any existing
        // destination artifact so a stale/oversized file cannot persist and
        // trip the load-refusal path on the next scan (which would force an
        // unnecessary full rebuild from a poisoned artifact). This is a
        // one-shot refusal (not a persist/refuse/rebuild loop): the budget
        // enforcement above already pruned and trimmed; if the result is still
        // too large (e.g. a single protected entry exceeds the limit), the
        // artifact is dropped and the next scan rebuilds from scratch.
        if crate::core::is_bounded_provider(provider)
            && crate::core::CostUsageCacheBudget::should_refuse_persistence(
                json.len(),
                max_load_bytes,
            )
        {
            // Best-effort removal; ignore errors (file may not exist).
            let _cleared = fs::remove_file(&cache_path);
            return;
        }

        let tmp_name = format!(
            ".{}.{}-{}.tmp",
            provider.cli_name(),
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );
        let tmp_path = parent.join(tmp_name);
        if fs::write(&tmp_path, json.as_bytes()).is_err() {
            return;
        }
        // Recheck after encoding/pruning: another scan may have replaced the
        // destination while this writer was preparing its payload.
        if let Some(expected) = cache.loaded_stamp.as_ref()
            && Self::cache_stamp(&cache_path).as_ref() != expected.as_ref()
        {
            let _removed_tmp = fs::remove_file(&tmp_path);
            return;
        }
        // `copy` replaces an existing target on Windows; prefer it over rename.
        let wrote = if fs::copy(&tmp_path, &cache_path).is_ok() {
            true
        } else {
            // Fallback direct write when copy fails; the copy error already surfaced.
            fs::write(&cache_path, json.as_bytes()).is_ok()
        };
        if wrote {
            cache.loaded_stamp = Some(Some(CacheStamp::from_bytes(json.as_bytes())));
        }
        // Best-effort temp cleanup (ignore errors — unique name avoids clashes).
        let _truncated_tmp = fs::File::create(&tmp_path).and_then(|f| f.set_len(0));
    }

    /// Default on-disk cache root: `%LOCALAPPDATA%\CodexBar` (via `dirs::cache_dir`).
    pub fn default_cache_root() -> Option<PathBuf> {
        dirs::cache_dir().map(|d| d.join("CodexBar"))
    }

    fn cache_path(provider: ProviderId, cache_root: Option<&Path>) -> PathBuf {
        let root = cache_root
            .map(|p| p.to_path_buf())
            .or_else(Self::default_cache_root)
            .unwrap_or_else(|| PathBuf::from("."));

        // Mirror upstream layout: {cacheRoot}/cost-usage/{provider}-v1.json
        root.join("cost-usage")
            .join(format!("{}-v1.json", provider.cli_name()))
    }

    fn cache_stamp(cache_path: &Path) -> Option<CacheStamp> {
        fs::read(cache_path)
            .ok()
            .map(|contents| CacheStamp::from_bytes(&contents))
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
