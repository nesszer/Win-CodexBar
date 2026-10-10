//! Direct fork chains: `root -> parent (fork) -> child (fork)`.
//!
//! Port of upstream `CodexDirectForkBaselineTests`. The child of a fork must
//! inherit the cumulative-counter origin of the whole chain, including when
//! the intermediate parent has no token snapshot before the child forks.
use super::*;
use serde_json::{Value, json};

const CACHE_SCHEMA_BEFORE_DIRECT_FORK_BASELINES: u64 = 4;

pub(super) fn timestamp(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// Upstream fixture shape: the session (and fork) time lives in the payload.
pub(super) fn session_meta(id: &str, parent: Option<&str>, at: DateTime<Utc>) -> Value {
    let mut payload = json!({
        "id": id,
        "source": "vscode",
        "thread_source": "user",
        "timestamp": timestamp(at),
    });
    if let Some(parent) = parent {
        payload["forked_from_id"] = json!(parent);
    }
    json!({"type": "session_meta", "payload": payload})
}

/// Upstream fixture shape: a full `event_msg` token_count with a model and
/// both cumulative and last-turn counters.
pub(super) fn token_count(at: DateTime<Utc>, total_input: i64, last_input: i64) -> Value {
    json!({
        "type": "event_msg",
        "timestamp": timestamp(at),
        "payload": {"type": "token_count", "info": {
            "model": "gpt-5.4",
            "total_token_usage": {"input_tokens": total_input, "output_tokens": 0},
            "last_token_usage": {"input_tokens": last_input, "output_tokens": 0},
        }},
    })
}

pub(super) fn write_rows(dir: &Path, name: &str, rows: &[Value]) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join(name);
    let body = rows
        .iter()
        .map(|row| format!("{row}\n"))
        .collect::<String>();
    std::fs::write(&path, body).unwrap();
    path
}

struct Chain {
    _root: tempfile::TempDir,
    sessions: PathBuf,
    cache_root: PathBuf,
    root_file: PathBuf,
    parent_file: PathBuf,
    child_file: PathBuf,
    base: DateTime<Utc>,
}

impl Chain {
    fn at(&self, seconds: i64) -> DateTime<Utc> {
        self.base + Duration::seconds(seconds)
    }

    fn root_rows(&self, total_input: i64) -> Vec<Value> {
        vec![
            session_meta("root", None, self.at(0)),
            token_count(self.at(1), total_input, total_input),
        ]
    }
}

/// Upstream timeline: `root` (1000 at t=1) -> `parent` forked at t=2, with an
/// optional own token event at `parent_event_time` (total 1040 / last 40) ->
/// `child` forked at t=4. Upstream covers no parent event and events at t=3
/// and t=8; t=2 additionally puts the parent's event at its own fork instant.
fn write_chain(parent_event_time: Option<i64>) -> Chain {
    let (root, sessions, cache_root) = codex_scan_dirs();
    let base = Utc::now() - Duration::hours(1);
    let day = base.with_timezone(&Local).date_naive();
    let day_dir = partition_dir(&sessions, day);
    let mut chain = Chain {
        sessions,
        cache_root,
        root_file: PathBuf::new(),
        parent_file: PathBuf::new(),
        child_file: PathBuf::new(),
        base,
        _root: root,
    };

    chain.root_file = write_rows(&day_dir, "root.jsonl", &chain.root_rows(1_000));
    let mut parent_rows = vec![session_meta("parent", Some("root"), chain.at(2))];
    if let Some(parent_event_time) = parent_event_time {
        parent_rows.push(token_count(chain.at(parent_event_time), 1_040, 40));
    }
    chain.parent_file = write_rows(&day_dir, "parent.jsonl", &parent_rows);

    let (inherited, copied_last) = match parent_event_time {
        Some(2 | 3) => (1_040, 40),
        None | Some(8) => (1_000, 1_000),
        Some(_) => unreachable!("the fixture uses parent events at t=2, 3, or 8"),
    };
    chain.child_file = write_rows(
        &day_dir,
        "child.jsonl",
        &[
            session_meta("child", Some("parent"), chain.at(4)),
            token_count(chain.at(5), inherited, copied_last),
            token_count(chain.at(6), inherited + 20, 20),
            token_count(chain.at(7), inherited + 20, 20),
        ],
    );
    chain
}

fn scanner(sessions: &Path, cache_root: &Path, bounded: bool) -> CostScanner {
    let mut options = CostScanOptions::app_driven();
    if bounded {
        // Upstream bounds only the per-refresh byte budget.
        options.codex_max_scan_bytes_per_refresh = 512;
    }
    CostScanner::new(7)
        .with_options(options)
        .with_cache_root(cache_root)
        .with_sessions_dirs(vec![sessions.to_path_buf()])
}

fn scan_to_completion(scanner: &CostScanner) -> (CostSummary, CostUsageCache) {
    let (summary, cache, _) = scan_to_completion_with_passes(scanner);
    (summary, cache)
}

/// Passes and Codex history bytes read until the scan stops deferring work.
pub(super) struct Progress {
    pub(super) passes: usize,
    pub(super) bytes_read: u64,
}

pub(super) fn scan_to_completion_with_passes(
    scanner: &CostScanner,
) -> (CostSummary, CostUsageCache, Progress) {
    let mut bytes_read = 0;
    for passes in 1..=80 {
        let (summary, stats, cache) = scanner.scan_codex_detailed_with_cache(None);
        bytes_read += stats.codex_bytes_read;
        if !cache.codex_scan_incomplete {
            return (summary, cache, Progress { passes, bytes_read });
        }
    }
    panic!("bounded Codex scan never completed");
}

pub(super) fn file_len(path: &Path) -> u64 {
    std::fs::metadata(path).unwrap().len()
}

pub(super) fn billed_input(cache: &CostUsageCache, path: &Path) -> i64 {
    let usage = &cache.files[&path.to_string_lossy().to_string()];
    assert!(
        !usage.codex_unresolved_fork_parent,
        "{path:?} is unresolved"
    );
    usage
        .days
        .values()
        .flat_map(HashMap::values)
        .map(|row| row[0])
        .sum()
}

fn downgrade_cache_schema(cache_root: &Path) {
    let path = cache_root.join("cost-usage").join("codex-v1.json");
    let mut cache: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    cache["codex_cache_schema_version"] = json!(CACHE_SCHEMA_BEFORE_DIRECT_FORK_BASELINES);
    std::fs::write(&path, serde_json::to_vec(&cache).unwrap()).unwrap();
}

fn assert_chain_billing(
    cache: &CostUsageCache,
    summary: &CostSummary,
    chain: &Chain,
    parent_event_time: Option<i64>,
) {
    assert_eq!(billed_input(cache, &chain.child_file), 20);
    let expected = 1_020 + if parent_event_time.is_some() { 40 } else { 0 };
    assert_eq!(summary.input_tokens, expected);
}

#[test]
fn direct_fork_chain_preserves_cumulative_inheritance() {
    for parent_event_time in [None, Some(2_i64), Some(3), Some(8)] {
        for bounded in [false, true] {
            let chain = write_chain(parent_event_time);
            let scanner = scanner(&chain.sessions, &chain.cache_root, bounded);

            // Cold scan, then an unchanged warm scan from the persisted cache.
            let (summary, cache, progress) = scan_to_completion_with_passes(&scanner);
            if bounded {
                assert!(
                    progress.passes > 1,
                    "512-byte budget should defer part of the chain"
                );
                // Deferred forks continue where they stopped: every history
                // byte is read once, as in one unbounded pass.
                let chain_bytes: u64 = [&chain.root_file, &chain.parent_file, &chain.child_file]
                    .into_iter()
                    .map(|path| file_len(path))
                    .sum();
                assert_eq!(progress.bytes_read, chain_bytes);
            }
            assert_chain_billing(&cache, &summary, &chain, parent_event_time);
            let (summary, cache) = scan_to_completion(&scanner);
            assert_chain_billing(&cache, &summary, &chain, parent_event_time);

            // A cache written before this correction is rebuilt, not trusted.
            downgrade_cache_schema(&chain.cache_root);
            let (summary, cache) = scan_to_completion(&scanner);
            assert_chain_billing(&cache, &summary, &chain, parent_event_time);

            // A forced rescan is a cold scan against an empty cache.
            let forced_cache_root = chain.cache_root.with_file_name("cache-forced");
            let forced = self::scanner(&chain.sessions, &forced_cache_root, bounded);
            let (summary, cache) = scan_to_completion(&forced);
            assert_chain_billing(&cache, &summary, &chain, parent_event_time);
        }
    }
}

#[test]
fn changed_root_revalidates_empty_parent_descendants() {
    let chain = write_chain(None);
    let scanner = scanner(&chain.sessions, &chain.cache_root, false);
    let (summary, cache) = scan_to_completion(&scanner);
    assert_chain_billing(&cache, &summary, &chain, None);
    let root_dir = chain.root_file.parent().unwrap();

    // Root rewritten with the same counters: the grandchild is revalidated
    // through the empty parent and keeps its own 20 tokens.
    write_rows(root_dir, "root.jsonl", &chain.root_rows(1_000));
    let (summary, cache) = scan_to_completion(&scanner);
    assert_chain_billing(&cache, &summary, &chain, None);

    // Root history rewritten to a counter above what the child replays. The
    // child must not keep the baseline it inherited through the empty
    // parent; it fails closed instead of billing against a stale origin.
    let before = std::fs::metadata(&chain.root_file)
        .unwrap()
        .modified()
        .unwrap();
    write_rows(root_dir, "root.jsonl", &chain.root_rows(1_010));
    // The rewrite has the same length, and on a fast runner it can land in the
    // same mtime tick, so the scanner would see an unchanged file. Move the
    // mtime forward so the change is always detected.
    std::fs::OpenOptions::new()
        .write(true)
        .open(&chain.root_file)
        .unwrap()
        .set_modified(before + std::time::Duration::from_secs(2))
        .unwrap();
    let (summary, _, cache) = scanner.scan_codex_detailed_with_cache(None);
    let child = &cache.files[&chain.child_file.to_string_lossy().to_string()];
    assert!(child.codex_unresolved_fork_parent);
    assert!(child.days.is_empty());
    assert_eq!(summary.input_tokens, 1_010);
}

#[test]
fn cyclic_empty_fork_ancestry_stays_unresolved() {
    let (_root, sessions, cache_root) = codex_scan_dirs();
    let base = Utc::now() - Duration::hours(1);
    let day = base.with_timezone(&Local).date_naive();
    let day_dir = partition_dir(&sessions, day);
    let files = [("a", "b"), ("b", "a")].map(|(id, parent)| {
        write_rows(
            &day_dir,
            &format!("{id}.jsonl"),
            &[session_meta(id, Some(parent), base)],
        )
    });
    let scanner = scanner(&sessions, &cache_root, false);

    for _ in 0..2 {
        let (summary, _, cache) = scanner.scan_codex_detailed_with_cache(None);
        assert_eq!(summary.input_tokens, 0);
        for file in &files {
            let usage = &cache.files[&file.to_string_lossy().to_string()];
            assert!(usage.codex_unresolved_fork_parent);
            assert!(usage.days.is_empty());
        }
    }
}

/// `root(1000)` -> `depth` empty forks -> `leaf` (20 own tokens). Returns the
/// scan result for the leaf.
fn scan_empty_fork_ladder(depth: usize) -> (CostSummary, CostUsageCache, PathBuf) {
    let root = tempfile::tempdir().unwrap();
    let sessions = root.path().join("sessions");
    let base = Utc::now() - Duration::hours(1);
    let day = base.with_timezone(&Local).date_naive();
    let day_dir = partition_dir(&sessions, day);
    write_rows(
        &day_dir,
        "s000.jsonl",
        &[
            session_meta("s000", None, base),
            token_count(base + Duration::seconds(1), 1_000, 1_000),
        ],
    );
    for level in 1..=depth {
        write_rows(
            &day_dir,
            &format!("s{level:03}.jsonl"),
            &[session_meta(
                &format!("s{level:03}"),
                Some(&format!("s{:03}", level - 1)),
                base + Duration::seconds(2),
            )],
        );
    }
    let leaf = write_rows(
        &day_dir,
        "leaf.jsonl",
        &[
            session_meta(
                "leaf",
                Some(&format!("s{depth:03}")),
                base + Duration::seconds(3),
            ),
            token_count(base + Duration::seconds(4), 1_000, 1_000),
            token_count(base + Duration::seconds(5), 1_020, 20),
        ],
    );
    let scanner = scanner(&sessions, &root.path().join("cache"), false);
    let (summary, _, cache) = scanner.scan_codex_detailed_with_cache(None);
    (summary, cache, leaf)
}

#[test]
fn empty_fork_ladder_resolves_within_depth_guard() {
    let (summary, cache, leaf) = scan_empty_fork_ladder(8);
    assert_eq!(billed_input(&cache, &leaf), 20);
    assert_eq!(summary.input_tokens, 1_020);
}

#[test]
fn empty_fork_ladder_beyond_depth_guard_stays_unresolved() {
    let (summary, cache, leaf) = scan_empty_fork_ladder(70);
    let usage = &cache.files[&leaf.to_string_lossy().to_string()];
    assert!(usage.codex_unresolved_fork_parent);
    assert!(usage.days.is_empty());
    assert_eq!(summary.input_tokens, 1_000);
}
