//! Archived and legacy flat Codex rollouts (Issue #616 item 1).
//!
//! Codex archives a thread by moving its rollout from `sessions/YYYY/MM/DD`
//! to the flat `<CODEX_HOME>/archived_sessions`, and unarchiving moves it
//! back. Archived threads stay in cost history, a move keeps the cached usage
//! without a reread, and a copy is never counted twice.

use super::*;
use crate::cost_reporting_period::CostReportingPeriod;
use chrono::Datelike;

fn today() -> NaiveDate {
    Local::now().date_naive()
}

fn session_id(id: u64) -> String {
    format!("0199a213-81c0-7800-8aa1-{id:012}")
}

fn rollout_name(day: NaiveDate, id: u64) -> String {
    format!(
        "rollout-{}T10-00-00-{}.jsonl",
        day.format("%Y-%m-%d"),
        session_id(id)
    )
}

/// A session with one token count at local noon, which keeps it on `day`.
fn rollout_body(day: NaiveDate, session: &str, input: u64) -> String {
    let noon = Local
        .from_local_datetime(&day.and_hms_opt(12, 0, 0).unwrap())
        .earliest()
        .unwrap()
        .with_timezone(&Utc)
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string();
    format!(
        r#"{{"timestamp":"{noon}","type":"session_meta","payload":{{"id":"{session}","timestamp":"{noon}","cwd":"/work"}}}}
{{"timestamp":"{noon}","type":"event_msg","payload":{{"type":"token_count","info":{{"model":"gpt-5","total_token_usage":{{"input_tokens":{input},"cached_input_tokens":0,"output_tokens":5}}}}}}}}
"#
    )
}

fn write_file(dir: &Path, name: &str, body: &str) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    path
}

fn write_rollout(dir: &Path, day: NaiveDate, id: u64, input: u64) -> PathBuf {
    write_file(
        dir,
        &rollout_name(day, id),
        &rollout_body(day, &session_id(id), input),
    )
}

/// Move `path` into `dir` under the same name, as Codex does.
fn move_into(path: &Path, dir: &Path) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let target = dir.join(path.file_name().unwrap());
    std::fs::rename(path, &target).unwrap();
    target
}

fn key(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

fn scanner(sessions_dirs: &[PathBuf], cache_root: &Path) -> CostScanner {
    CostScanner::new(7)
        .with_cache_root(cache_root)
        .with_sessions_dirs(sessions_dirs.to_vec())
}

fn explicit_scan(
    sessions_dirs: &[PathBuf],
    cache_root: &Path,
) -> (CostSummary, CostScanStats, CostUsageCache) {
    scanner(sessions_dirs, cache_root)
        .with_options(CostScanOptions::app_driven())
        .scan_codex_detailed_with_cache(None)
}

/// A later background refresh: default options, outside the debounce.
fn background_scan(
    sessions_dirs: &[PathBuf],
    cache_root: &Path,
) -> (CostSummary, CostScanStats, CostUsageCache) {
    let mut cache = JsonlScanner::load_cache(ProviderId::Codex, Some(cache_root));
    cache.last_scan_unix_ms = 1;
    JsonlScanner::save_cache(ProviderId::Codex, &mut cache, Some(cache_root));
    scanner(sessions_dirs, cache_root).scan_codex_detailed_with_cache(None)
}

struct CodexHome {
    _root: tempfile::TempDir,
    base: PathBuf,
    home: PathBuf,
    sessions: PathBuf,
    archived: PathBuf,
    cache_root: PathBuf,
}

impl CodexHome {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let base = root.path().to_path_buf();
        let home = base.join("codex-home");
        let sessions = home.join("sessions");
        std::fs::create_dir_all(&sessions).unwrap();
        Self {
            archived: home.join("archived_sessions"),
            cache_root: base.join("cache"),
            _root: root,
            base,
            home,
            sessions,
        }
    }

    fn dirs(&self) -> Vec<PathBuf> {
        vec![self.sessions.clone()]
    }
}

#[test]
fn archived_and_legacy_flat_rollouts_count_toward_cost_history() {
    let codex = CodexHome::new();
    let recent = today() - Duration::days(2);
    let older = today() - Duration::days(3);
    write_rollout(&partition_dir(&codex.sessions, recent), recent, 1, 100);
    let archived = write_rollout(&codex.archived, recent, 2, 20);
    let legacy_flat = write_rollout(&codex.sessions, older, 3, 3);
    // A flat name without a date is kept; its events decide.
    let undated = write_file(
        &codex.archived,
        "imported-session.jsonl",
        &rollout_body(today() - Duration::days(1), &session_id(4), 4000),
    );
    // Hidden files, and rollouts whose name dates them outside the window,
    // are never opened.
    let hidden = write_file(
        &codex.archived,
        &format!(".{}", rollout_name(recent, 5)),
        &rollout_body(recent, &session_id(5), 50_000),
    );
    let stale_day = today() - Duration::days(40);
    let stale = write_rollout(&codex.archived, stale_day, 6, 600_000);

    let (summary, stats, cache) = explicit_scan(&codex.dirs(), &codex.cache_root);

    assert_eq!(summary.input_tokens, 4123);
    assert_eq!(summary.sessions_count, 4);
    assert!(summary.history_coverage_established);
    for counted in [&archived, &legacy_flat, &undated] {
        assert!(
            cache.files.contains_key(&key(counted)),
            "{} is cached",
            key(counted)
        );
    }
    for ignored in [&hidden, &stale] {
        assert!(
            !stats.codex_metadata_read_paths.contains(&key(ignored)),
            "{} is not opened",
            key(ignored)
        );
    }
    assert!(!cache.codex_scan_incomplete);
    assert!(cache.codex_scan_pause_reason.is_none());
}

#[test]
fn archiving_a_counted_rollout_keeps_its_usage_without_a_reread() {
    let codex = CodexHome::new();
    let day = today() - Duration::days(2);
    write_rollout(&partition_dir(&codex.sessions, day), day, 1, 100);
    let dated = write_rollout(&partition_dir(&codex.sessions, day), day, 2, 20);
    let (summary, _, _) = explicit_scan(&codex.dirs(), &codex.cache_root);
    assert_eq!(summary.input_tokens, 120);

    let archived = move_into(&dated, &codex.archived);
    let (summary, stats, cache) = background_scan(&codex.dirs(), &codex.cache_root);

    assert_eq!(summary.input_tokens, 120);
    assert!(stats.codex_metadata_read_paths.is_empty());
    assert!(stats.codex_history_read_paths.is_empty());
    assert_eq!(stats.files_parsed, 0);
    assert_eq!(stats.files_skipped, 2);
    assert!(cache.codex_scan_pause_reason.is_none());
    assert!(!cache.codex_scan_incomplete);
    assert!(cache.files.contains_key(&key(&archived)));
    assert!(!cache.files.contains_key(&key(&dated)));
}

#[test]
fn unarchiving_moves_the_cached_usage_back_into_its_partition() {
    let codex = CodexHome::new();
    let day = today() - Duration::days(3);
    let archived = write_rollout(&codex.archived, day, 7, 40);
    let (summary, _, _) = explicit_scan(&codex.dirs(), &codex.cache_root);
    assert_eq!(summary.input_tokens, 40);

    let restored = move_into(&archived, &partition_dir(&codex.sessions, day));
    let (summary, stats, cache) = background_scan(&codex.dirs(), &codex.cache_root);

    assert_eq!(summary.input_tokens, 40);
    assert!(stats.codex_metadata_read_paths.is_empty());
    assert!(stats.codex_history_read_paths.is_empty());
    assert!(cache.codex_scan_pause_reason.is_none());
    assert!(!cache.codex_scan_incomplete);
    assert!(cache.files.contains_key(&key(&restored)));
    assert!(!cache.files.contains_key(&key(&archived)));
}

#[test]
fn an_archived_copy_of_a_dated_rollout_is_counted_once() {
    let codex = CodexHome::new();
    let day = today() - Duration::days(2);
    let dated = write_rollout(&partition_dir(&codex.sessions, day), day, 8, 100);
    std::fs::create_dir_all(&codex.archived).unwrap();
    let copy = codex.archived.join(rollout_name(day, 8));
    std::fs::copy(&dated, &copy).unwrap();

    let (summary, stats, cache) = explicit_scan(&codex.dirs(), &codex.cache_root);

    assert_eq!(summary.input_tokens, 100);
    assert_eq!(summary.sessions_count, 1);
    assert!(!stats.codex_metadata_read_paths.contains(&key(&copy)));
    assert!(!cache.files.contains_key(&key(&copy)));
    assert!(cache.codex_scan_pause_reason.is_none());
}

#[test]
fn a_dated_copy_replaces_the_archived_entry_instead_of_adding_to_it() {
    let codex = CodexHome::new();
    let day = today() - Duration::days(2);
    let archived = write_rollout(&codex.archived, day, 9, 100);
    let (summary, _, _) = explicit_scan(&codex.dirs(), &codex.cache_root);
    assert_eq!(summary.input_tokens, 100);

    let dated_dir = partition_dir(&codex.sessions, day);
    std::fs::create_dir_all(&dated_dir).unwrap();
    let dated = dated_dir.join(rollout_name(day, 9));
    std::fs::copy(&archived, &dated).unwrap();
    let (summary, _, cache) = background_scan(&codex.dirs(), &codex.cache_root);

    assert_eq!(summary.input_tokens, 100);
    assert_eq!(summary.sessions_count, 1);
    assert!(cache.files.contains_key(&key(&dated)));
    assert!(!cache.files.contains_key(&key(&archived)));
    assert!(cache.codex_scan_pause_reason.is_none());
}

#[test]
fn a_removed_archive_pauses_background_scans_until_an_explicit_refresh() {
    let codex = CodexHome::new();
    let day = today() - Duration::days(2);
    write_rollout(&partition_dir(&codex.sessions, day), day, 10, 100);
    write_rollout(&codex.archived, day, 11, 20);
    let (summary, _, _) = explicit_scan(&codex.dirs(), &codex.cache_root);
    assert_eq!(summary.input_tokens, 120);

    std::fs::rename(&codex.archived, codex.home.join("archived_sessions.old")).unwrap();
    let (summary, _, cache) = background_scan(&codex.dirs(), &codex.cache_root);
    assert_eq!(summary.input_tokens, 120);
    assert_eq!(
        cache.codex_scan_pause_reason,
        Some(CodexScanPauseReason::NoProgress)
    );

    let (summary, _, cache) = explicit_scan(&codex.dirs(), &codex.cache_root);
    assert_eq!(summary.input_tokens, 100);
    assert!(cache.codex_scan_pause_reason.is_none());
    assert!(!cache.codex_scan_incomplete);
}

#[test]
fn an_unreachable_home_with_archived_history_is_a_source_failure() {
    let codex = CodexHome::new();
    let other_home = codex.base.join("other-home");
    let other_sessions = other_home.join("sessions");
    std::fs::create_dir_all(&other_sessions).unwrap();
    let day = today() - Duration::days(2);
    write_rollout(&partition_dir(&codex.sessions, day), day, 12, 100);
    write_rollout(&other_home.join("archived_sessions"), day, 13, 1000);
    let dirs = vec![codex.sessions.clone(), other_sessions];
    let (summary, _, _) = explicit_scan(&dirs, &codex.cache_root);
    assert_eq!(summary.input_tokens, 1100);

    let offline = codex.base.join("other-home-offline");
    std::fs::rename(&other_home, &offline).unwrap();
    let (summary, _, cache) = background_scan(&dirs, &codex.cache_root);
    assert_eq!(summary.input_tokens, 1100);
    assert_eq!(
        cache.codex_scan_pause_reason,
        Some(CodexScanPauseReason::Error(
            "Codex session source unavailable".to_string()
        ))
    );

    std::fs::rename(&offline, &other_home).unwrap();
    let (summary, _, cache) = explicit_scan(&dirs, &codex.cache_root);
    assert_eq!(summary.input_tokens, 1100);
    assert!(cache.codex_scan_pause_reason.is_none());
    assert!(!cache.codex_scan_incomplete);
}

#[test]
fn all_available_history_reaches_archived_rollouts_older_than_any_partition() {
    let codex = CodexHome::new();
    let recent = today() - Duration::days(2);
    let old = NaiveDate::from_ymd_opt(today().year() - 2, 6, 1).unwrap();
    write_rollout(&partition_dir(&codex.sessions, recent), recent, 14, 100);
    write_rollout(&codex.archived, old, 15, 20);

    let all = CostScanner::for_period(CostReportingPeriod::AllAvailable)
        .with_options(CostScanOptions::app_driven())
        .with_cache_root(codex.base.join("cache-all"))
        .with_sessions_dirs(codex.dirs());
    let (summary, _) = all.scan_codex_detailed(None);
    assert_eq!(summary.input_tokens, 120);
    assert_eq!(summary.period_start, Some(old));

    let rolling = CostScanner::new(30)
        .with_options(CostScanOptions::app_driven())
        .with_cache_root(codex.base.join("cache-30"))
        .with_sessions_dirs(codex.dirs());
    let (summary, _) = rolling.scan_codex_detailed(None);
    assert_eq!(summary.input_tokens, 100);
}

#[test]
fn a_look_alike_archived_rollout_is_scanned_as_a_new_file() {
    let codex = CodexHome::new();
    let day = today() - Duration::days(2);
    let original = write_rollout(&partition_dir(&codex.sessions, day), day, 16, 100);
    let (summary, _, _) = explicit_scan(&codex.dirs(), &codex.cache_root);
    assert_eq!(summary.input_tokens, 100);

    // The counted file leaves the tree, and an unrelated file with its name
    // lands in the archive. Its identity differs, so it is not adopted.
    move_into(&original, &codex.base.join("elsewhere"));
    let look_alike = write_file(
        &codex.archived,
        &rollout_name(day, 16),
        &rollout_body(day, &session_id(99), 7),
    );
    let (summary, stats, cache) = background_scan(&codex.dirs(), &codex.cache_root);
    assert!(stats.codex_history_read_paths.contains(&key(&look_alike)));
    assert_eq!(summary.input_tokens, 100, "the last report is kept");
    assert_eq!(
        cache.codex_scan_pause_reason,
        Some(CodexScanPauseReason::NoProgress)
    );
    assert!(cache.files.contains_key(&key(&original)));

    let (summary, _, cache) = explicit_scan(&codex.dirs(), &codex.cache_root);
    assert_eq!(summary.input_tokens, 7);
    assert!(cache.codex_scan_pause_reason.is_none());
    assert!(!cache.files.contains_key(&key(&original)));
}

#[test]
fn flat_rollout_names_carry_their_day() {
    let day = JsonlScanner::codex_filename_day_key;
    assert_eq!(
        day("rollout-2025-10-03T10-00-00-0199a213.jsonl"),
        Some("2025-10-03")
    );
    assert_eq!(day("12025-10-03.jsonl"), Some("2025-10-03"));
    assert_eq!(day("\u{e9}2025-10-03.jsonl"), Some("2025-10-03"));
    assert_eq!(day("notes.jsonl"), None);
    assert_eq!(day("2025-1-03.jsonl"), None);
    assert_eq!(day("2025-10-0"), None);

    let in_range = |name| JsonlScanner::codex_flat_name_in_range(name, "2025-10-01", "2025-10-05");
    assert!(in_range("notes.jsonl"));
    assert!(in_range("rollout-2025-10-05T23-59-59-x.jsonl"));
    assert!(!in_range("rollout-2025-09-30T10-00-00-x.jsonl"));
}
