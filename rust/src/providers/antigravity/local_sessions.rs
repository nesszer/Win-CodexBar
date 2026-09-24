pub use super::local_history::{offline_conversation_count, summarize_local_usage as summarize};
pub use crate::spend_contract::{
    LocalHistoryCoverage, LocalTokenHistorySummary as LocalSessionSummary,
};

use chrono::{DateTime, Utc};
use std::path::{Path, PathBuf};

pub fn summarize_from_roots(
    database_roots: &[PathBuf],
    jsonl_sessions_root: &Path,
    now: DateTime<Utc>,
    days: u32,
) -> LocalSessionSummary {
    super::local_history::summarize_local_usage_from_explicit_roots(
        database_roots,
        jsonl_sessions_root,
        now,
        days,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use std::fs;

    #[test]
    fn explicit_roots_use_only_caller_paths_and_fixed_window() {
        let dir = tempfile::tempdir().unwrap();
        let caller_database_root = dir.path().join("caller-databases");
        let caller_root = dir.path().join("caller-sessions");
        let unrelated_root = dir.path().join("unrelated-sessions");
        fs::create_dir_all(&caller_database_root).unwrap();
        fs::create_dir_all(&caller_root).unwrap();
        fs::create_dir_all(&unrelated_root).unwrap();
        let connection =
            rusqlite::Connection::open(caller_database_root.join("foreign.db")).unwrap();
        connection
            .execute("CREATE TABLE unrelated(id INTEGER PRIMARY KEY)", [])
            .unwrap();

        fs::write(
            caller_root.join("caller.jsonl"),
            concat!(
                "{\"type\":\"usage\",\"responseId\":\"in-window\",\"timestamp\":1787572800000,\"input\":100,\"output\":20}\n",
                "{\"type\":\"usage\",\"responseId\":\"old\",\"timestamp\":1784894400000,\"input\":900,\"output\":90}\n"
            ),
        )
        .unwrap();
        fs::write(
            unrelated_root.join("unrelated.jsonl"),
            b"{\"type\":\"usage\",\"responseId\":\"unrelated\",\"timestamp\":1787572800000,\"input\":9000,\"output\":900}\n",
        )
        .unwrap();

        let now = Utc.timestamp_millis_opt(1787576400000).single().unwrap();
        let summary = summarize_from_roots(
            std::slice::from_ref(&caller_database_root),
            &caller_root,
            now,
            7,
        );

        assert_eq!(summary.total_tokens, 120);
        assert_eq!(summary.session_count, 1);
        assert_eq!(summary.coverage, LocalHistoryCoverage::Complete);
    }
}
