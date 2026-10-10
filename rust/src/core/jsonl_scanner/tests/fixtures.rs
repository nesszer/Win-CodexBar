//! Cache-entry fixtures shared by the JSONL and cost scanner tests.

use super::{CodexSessionLineage, CostUsageFileUsage};
use std::collections::HashMap;

/// A root-session cache entry with no parse state; tests override the
/// fields they exercise with struct update syntax.
pub(crate) fn test_file_usage(
    size: i64,
    days: HashMap<String, HashMap<String, Vec<i64>>>,
) -> CostUsageFileUsage {
    CostUsageFileUsage {
        mtime_unix_ms: 0,
        size,
        codex_file_identity: None,
        days,
        parsed_bytes: None,
        codex_scan_target_size: None,
        last_model: None,
        last_totals: None,
        codex_token_timestamps_monotonic: None,
        codex_last_token_timestamp: None,
        codex_session_id: None,
        codex_forked_from_id: None,
        codex_fork_accounting_state: None,
        codex_lineage: CodexSessionLineage::Root,
        codex_fork_timestamp: None,
        codex_unresolved_fork_parent: false,
    }
}
