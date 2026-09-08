use super::read_receipt::CodexScanReadReceipt;

/// Per-pass counters for cache/resume behavior (tests + diagnostics).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CostScanStats {
    pub files_seen: u32,
    pub files_parsed: u32,
    pub files_skipped: u32,
    pub files_resumed: u32,
    /// Files deferred to a later bounded Codex catch-up pass.
    pub files_deferred: u32,
    /// Newly consumed Codex JSONL bytes in this refresh.
    pub codex_bytes_read: u64,
    /// Timestamp comparisons performed while validating Codex append history.
    pub token_timestamp_comparisons: u64,
    /// Source-read receipt: JSONL paths whose identity prefix was inspected.
    pub codex_metadata_read_paths: Vec<String>,
    /// Source-read receipt: JSONL paths whose token history was parsed.
    pub codex_history_read_paths: Vec<String>,
    /// Lazy-read state kept separate so callers can prove cache-only refreshes.
    pub codex_read_receipt: CodexScanReadReceipt,
    pub used_cache_debounce: bool,
}
