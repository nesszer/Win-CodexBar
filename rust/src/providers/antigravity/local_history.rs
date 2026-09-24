use super::{local_sessions_reader as local_sessions, local_sqlite};
use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};

use crate::spend_contract::LocalTokenHistorySummary;

fn clean_env_path(value: Option<&str>) -> Option<PathBuf> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn configured_database_roots(home: &Path) -> [PathBuf; 3] {
    let gemini_cli_home = std::env::var("GEMINI_CLI_HOME").ok();
    configured_database_roots_from_values(home, gemini_cli_home.as_deref())
}

fn configured_database_roots_from_values(
    home: &Path,
    gemini_cli_home: Option<&str>,
) -> [PathBuf; 3] {
    let gemini_base = clean_env_path(gemini_cli_home).unwrap_or_else(|| home.join(".gemini"));
    local_sqlite::database_roots(&gemini_base)
}

fn summarize_local_usage_from(
    roots: &[PathBuf],
    now: DateTime<Utc>,
    days: u32,
    jsonl_fallback: impl FnOnce() -> LocalTokenHistorySummary,
) -> LocalTokenHistorySummary {
    match local_sqlite::summarize(roots, now, days) {
        local_sqlite::SQLiteScan::Summary(summary) => summary,
        local_sqlite::SQLiteScan::NoDatabases | local_sqlite::SQLiteScan::Unsupported => {
            jsonl_fallback()
        }
    }
}

pub(super) fn summarize_local_usage_from_explicit_roots(
    database_roots: &[PathBuf],
    jsonl_sessions_root: &Path,
    now: DateTime<Utc>,
    days: u32,
) -> LocalTokenHistorySummary {
    summarize_local_usage_from(database_roots, now, days, || {
        local_sessions::summarize_jsonl_at(jsonl_sessions_root, now, days)
    })
}

pub fn summarize_local_usage(days: u32) -> LocalTokenHistorySummary {
    let now = Utc::now();
    let Some(home) = dirs::home_dir() else {
        return LocalTokenHistorySummary::default();
    };
    let roots = configured_database_roots(&home);
    let tokscale_sessions = local_sessions::configured_tokscale_sessions(&home);
    summarize_local_usage_from_explicit_roots(&roots, &tokscale_sessions, now, days)
}

/// Count local Antigravity conversation artifacts for the quota provider's
/// offline fallback. Mirrors upstream #3119 without opening SQLite files.
pub fn offline_conversation_count() -> usize {
    let Some(home) = dirs::home_dir() else {
        return 0;
    };
    let roots = configured_database_roots(&home);
    let tokscale_sessions = local_sessions::configured_tokscale_sessions(&home);
    offline_conversation_count_with_roots(&roots, &tokscale_sessions)
}

#[cfg(test)]
fn offline_conversation_count_in(home: &Path) -> usize {
    let roots = local_sqlite::database_roots(&home.join(".gemini"));
    let tokscale_sessions = local_sessions::tokscale_sessions_from_values(home, None);
    offline_conversation_count_with_roots(&roots, &tokscale_sessions)
}

fn offline_conversation_count_with_roots(
    database_roots: &[PathBuf],
    tokscale_sessions: &Path,
) -> usize {
    let db_count = database_roots
        .iter()
        .map(|root| count_extension(root, "db"))
        .sum::<usize>();
    if db_count > 0 {
        return db_count;
    }
    local_sessions::count_jsonl_sessions_at(tokscale_sessions)
}

fn count_extension(root: &Path, extension: &str) -> usize {
    fs::read_dir(root)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some(extension))
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spend_contract::LocalHistoryCoverage;
    use chrono::TimeZone;
    use rusqlite::Connection;

    #[test]
    fn foreign_database_preserves_valid_tokscale_history() {
        let dir = tempfile::tempdir().unwrap();
        let gemini_base = dir.path().join(".gemini");
        let database_root = gemini_base.join("antigravity-cli").join("conversations");
        fs::create_dir_all(&database_root).unwrap();
        let connection = Connection::open(database_root.join("foreign.db")).unwrap();
        connection
            .execute(
                "CREATE TABLE unrelated(id INTEGER PRIMARY KEY, value TEXT)",
                [],
            )
            .unwrap();

        let tokscale_sessions = dir
            .path()
            .join(".config/tokscale/antigravity-cache/sessions");
        fs::create_dir_all(&tokscale_sessions).unwrap();
        let session_path = tokscale_sessions.join("session-a.jsonl");
        fs::write(
            &session_path,
            b"{\"type\":\"usage\",\"responseId\":\"r1\",\"timestamp\":1787572800000,\"input\":100,\"output\":20}\n",
        )
        .unwrap();

        let roots = local_sqlite::database_roots(&gemini_base);
        let now = Utc.timestamp_millis_opt(1787576400000).single().unwrap();
        let summary = summarize_local_usage_from(&roots, now, 7, || {
            local_sessions::summarize_jsonl_paths(
                std::slice::from_ref(&session_path),
                now,
                7,
                false,
            )
        });

        assert_eq!(summary.total_tokens, 120);
        assert_eq!(summary.session_count, 1);
        assert_eq!(summary.coverage, LocalHistoryCoverage::Complete);
    }

    #[test]
    fn foreign_only_input_does_not_fabricate_known_zero_native_usage() {
        let dir = tempfile::tempdir().unwrap();
        let gemini_base = dir.path().join(".gemini");
        let database_root = gemini_base.join("antigravity-cli").join("conversations");
        fs::create_dir_all(&database_root).unwrap();
        let connection = Connection::open(database_root.join("foreign.db")).unwrap();
        connection
            .execute("CREATE TABLE unrelated(id INTEGER PRIMARY KEY)", [])
            .unwrap();

        let roots = local_sqlite::database_roots(&gemini_base);
        let now = Utc.timestamp_millis_opt(1787576400000).single().unwrap();
        let summary = summarize_local_usage_from(&roots, now, 7, LocalTokenHistorySummary::default);

        assert_eq!(summary, LocalTokenHistorySummary::default());
        assert_eq!(summary.coverage, LocalHistoryCoverage::Unavailable);
    }

    #[test]
    fn scan_context_honors_non_empty_root_overrides() {
        let home = Path::new(r"C:\Users\test");
        let tokscale_sessions =
            local_sessions::tokscale_sessions_from_values(home, Some(r"E:\tokscale-root"));
        let roots = configured_database_roots_from_values(home, Some(r"D:\gemini-root"));
        assert_eq!(
            roots[0],
            PathBuf::from(r"D:\gemini-root")
                .join("antigravity-cli")
                .join("conversations")
        );
        assert_eq!(
            tokscale_sessions,
            PathBuf::from(r"E:\tokscale-root")
                .join("antigravity-cache")
                .join("sessions")
        );
        let defaults = local_sessions::tokscale_sessions_from_values(home, Some(""));
        let default_roots = configured_database_roots_from_values(home, Some("  "));
        assert_eq!(default_roots[1], home.join(".gemini").join("antigravity"));
        assert_eq!(
            defaults,
            home.join(".config")
                .join("tokscale")
                .join("antigravity-cache")
                .join("sessions")
        );
    }

    #[test]
    fn offline_count_prefers_cli_and_app_db_artifacts_then_tokscale() {
        let dir = tempfile::tempdir().unwrap();
        let app = dir
            .path()
            .join(".gemini")
            .join("antigravity")
            .join("conversations");
        fs::create_dir_all(&app).unwrap();
        fs::write(app.join("a.db"), b"").unwrap();
        fs::write(app.join("a.db-wal"), b"").unwrap();
        assert_eq!(offline_conversation_count_in(dir.path()), 1);

        fs::remove_file(app.join("a.db")).unwrap();
        let cache = dir
            .path()
            .join(".config")
            .join("tokscale")
            .join("antigravity-cache")
            .join("sessions");
        fs::create_dir_all(&cache).unwrap();
        fs::write(cache.join("one.jsonl"), b"{}\n").unwrap();
        assert_eq!(offline_conversation_count_in(dir.path()), 1);
    }
}
