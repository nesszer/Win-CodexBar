//! Bounded fork parses continue at their saved cursor.
//!
//! Upstream resumes an unfinished fork at its cached offset while its parent
//! dependency is unchanged (`canResumeCodexForkAccounting`), and parses every
//! other fork from byte zero with fresh accounting state. Either way a fork
//! bills exactly what one uninterrupted cold parse bills.
use super::direct_fork::{
    billed_input, file_len, scan_to_completion_with_passes, session_meta, timestamp, token_count,
    write_rows,
};
use super::*;
use serde_json::{Value, json};

/// A token row without cumulative counters. A fork's first rows replay usage
/// it inherited, which the parent baseline absorbs before the fork bills.
fn last_only(at: DateTime<Utc>, input: i64) -> Value {
    json!({
        "type": "event_msg",
        "timestamp": timestamp(at),
        "payload": {"type": "token_count", "info": {
            "model": "gpt-5.4",
            "last_token_usage": {"input_tokens": input, "output_tokens": 0},
        }},
    })
}

struct Sessions {
    root: tempfile::TempDir,
    sessions: PathBuf,
    day_dir: PathBuf,
    base: DateTime<Utc>,
}

impl Sessions {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let sessions = root.path().join("sessions");
        let base = Utc::now() - Duration::hours(1);
        let day = base.with_timezone(&Local).date_naive();
        let day_dir = partition_dir(&sessions, day);
        Self {
            root,
            sessions,
            day_dir,
            base,
        }
    }

    fn at(&self, seconds: i64) -> DateTime<Utc> {
        self.base + Duration::seconds(seconds)
    }

    fn write(&self, name: &str, rows: &[Value]) -> PathBuf {
        write_rows(&self.day_dir, name, rows)
    }

    /// Root session holding one cumulative snapshot of `total_input` at t=1.
    fn write_root(&self, total_input: i64) -> PathBuf {
        self.write(
            "root.jsonl",
            &[
                session_meta("root", None, self.at(0)),
                token_count(self.at(1), total_input, total_input),
            ],
        )
    }

    /// Scanner with its own cache directory. A one-byte refresh budget reads
    /// one JSONL line per refresh.
    fn scanner(&self, cache: &str, bytes_per_refresh: Option<i64>) -> CostScanner {
        let mut options = CostScanOptions::app_driven();
        options.prefer_newest_codex_sessions_first = false;
        if let Some(limit) = bytes_per_refresh {
            options.codex_max_scan_bytes_per_refresh = limit;
        }
        CostScanner::new(7)
            .with_options(options)
            .with_cache_root(self.root.path().join(cache))
            .with_sessions_dirs(vec![self.sessions.clone()])
    }
}

#[test]
fn bounded_fork_keeps_used_up_inherited_counters() {
    let sessions = Sessions::new();
    let root = sessions.write_root(1_000);
    let child = sessions.write(
        "child.jsonl",
        &[
            session_meta("child", Some("root"), sessions.at(2)),
            last_only(sessions.at(3), 1_000),
            last_only(sessions.at(4), 20),
            last_only(sessions.at(5), 20),
        ],
    );

    for (cache, bytes_per_refresh) in [("cache", None), ("cache-bounded", Some(1))] {
        let scanner = sessions.scanner(cache, bytes_per_refresh);
        let (summary, cache, progress) = scan_to_completion_with_passes(&scanner);
        // The first row replays the inherited 1000; a resumed pass must not
        // refill the used-up counters and absorb the child's own 20 + 20.
        assert_eq!(billed_input(&cache, &child), 40);
        assert_eq!(summary.input_tokens, 1_040);
        assert_eq!(progress.bytes_read, file_len(&root) + file_len(&child));
    }
}

#[test]
fn partial_fork_restarts_when_its_parent_baseline_changes() {
    let sessions = Sessions::new();
    sessions.write_root(1_000);
    let child = sessions.write(
        "child.jsonl",
        &[
            session_meta("child", Some("root"), sessions.at(2)),
            token_count(sessions.at(3), 1_000, 1_000),
            token_count(sessions.at(4), 1_020, 20),
            token_count(sessions.at(5), 1_040, 20),
        ],
    );
    let child_key = child.to_string_lossy().to_string();

    // Read one line per refresh until the child holds a saved cursor.
    let bounded = sessions.scanner("cache", Some(1));
    let saved_cursor = (0..20).any(|_| {
        let (_, _, cache) = bounded.scan_codex_detailed_with_cache(None);
        cache
            .files
            .get(&child_key)
            .and_then(|usage| usage.codex_fork_accounting_state.as_ref())
            .is_some_and(|state| state.resume.is_some())
    });
    assert!(saved_cursor, "the child never paused mid-parse");

    // The root now supplies another baseline. The saved cursor was accounted
    // against the old one, so the child is parsed again from byte zero.
    sessions.write_root(990);
    let (summary, cache, _) = scan_to_completion_with_passes(&sessions.scanner("cache", None));
    let (cold_summary, cold_cache, _) =
        scan_to_completion_with_passes(&sessions.scanner("cache-cold", None));
    assert_eq!(billed_input(&cold_cache, &child), 50);
    assert_eq!(billed_input(&cache, &child), 50);
    assert_eq!(summary.input_tokens, cold_summary.input_tokens);
}

#[test]
fn grown_fork_reparse_starts_from_the_full_inherited_baseline() {
    let sessions = Sessions::new();
    sessions.write_root(1_000);
    let mut rows = vec![
        session_meta("child", Some("root"), sessions.at(2)),
        last_only(sessions.at(3), 600),
        last_only(sessions.at(4), 20),
    ];
    let child = sessions.write("child.jsonl", &rows);
    let scanner = sessions.scanner("cache", None);
    let (_, cache, _) = scan_to_completion_with_passes(&scanner);
    // Both rows still replay the 1000 the child inherited.
    assert_eq!(billed_input(&cache, &child), 0);

    // A finished fork that grows is parsed again from byte zero, so it must
    // start from the whole inherited baseline, not the part left over.
    rows.push(last_only(sessions.at(5), 500));
    sessions.write("child.jsonl", &rows);
    let (summary, cache, _) = scan_to_completion_with_passes(&scanner);
    let (cold_summary, cold_cache, _) =
        scan_to_completion_with_passes(&sessions.scanner("cache-cold", None));
    assert_eq!(billed_input(&cold_cache, &child), 120);
    assert_eq!(billed_input(&cache, &child), 120);
    assert_eq!(summary.input_tokens, cold_summary.input_tokens);
}
