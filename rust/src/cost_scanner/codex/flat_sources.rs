//! Flat Codex rollouts: the `archived_sessions` sibling of each sessions root
//! and legacy `*.jsonl` files kept directly in a root.
//!
//! Codex archives a thread by moving `sessions/YYYY/MM/DD/<rollout>.jsonl`
//! to the flat `<CODEX_HOME>/archived_sessions/<rollout>.jsonl`, and
//! unarchiving moves it back under the partition its name dates. Upstream
//! lists both flat directories next to the date partitions
//! (`codexSessionsRoots`, `listCodexSessionFilesFlat`); without them an
//! archived thread dropped out of cost history.
//!
//! A move keeps the file's identity, so the cached usage follows the file to
//! its new path. It is neither read again nor dropped as missing, which
//! would pause a background scan.

use super::*;

/// Upstream `codexArchivedSessionsRoot`: only a root named `sessions` has an
/// `archived_sessions` sibling.
fn codex_archived_sessions_dir(sessions_dir: &Path) -> Option<PathBuf> {
    let named_sessions = sessions_dir
        .file_name()
        .is_some_and(|name| name.eq_ignore_ascii_case("sessions"));
    named_sessions
        .then(|| sessions_dir.parent())
        .flatten()
        .map(|home| home.join("archived_sessions"))
}

fn is_flat_dir_of(dir: &Path, sessions_dir: &Path) -> bool {
    dir == sessions_dir
        || codex_archived_sessions_dir(sessions_dir).is_some_and(|archive| archive == dir)
}

/// Every flat directory to list, each with the index of the sessions root
/// (Codex home) it belongs to: the root itself, then its archive.
fn codex_flat_dirs(sessions_dirs: &[PathBuf]) -> Vec<(PathBuf, usize)> {
    let mut dirs: Vec<(PathBuf, usize)> = Vec::new();
    for (home, sessions_dir) in sessions_dirs.iter().enumerate() {
        let candidates =
            std::iter::once(sessions_dir.clone()).chain(codex_archived_sessions_dir(sessions_dir));
        for dir in candidates {
            if dirs.iter().all(|(listed, _)| listed != &dir) {
                dirs.push((dir, home));
            }
        }
    }
    dirs
}

/// The Codex home a cached rollout belongs to: anywhere under its sessions
/// root, or directly inside its archive.
fn codex_home_of(path: &Path, sessions_dirs: &[PathBuf]) -> Option<usize> {
    sessions_dirs.iter().position(|root| {
        path.starts_with(root)
            || codex_archived_sessions_dir(root)
                .is_some_and(|archive| path.parent() == Some(archive.as_path()))
    })
}

fn flat_name_in_scan_window(path: &Path, range: &CostUsageDayRange) -> bool {
    path.file_name().is_none_or(|name| {
        JsonlScanner::codex_flat_name_in_range(
            &name.to_string_lossy(),
            &range.scan_since_key,
            &range.scan_until_key,
        )
    })
}

/// Whether `path` sits directly in a flat directory of a scanned home and
/// its name keeps it in the scan window.
pub(super) fn is_flat_codex_path_in_scan_window(
    path: &Path,
    sessions_dirs: &[PathBuf],
    range: &CostUsageDayRange,
) -> bool {
    path.parent().is_some_and(|parent| {
        sessions_dirs
            .iter()
            .any(|sessions_dir| is_flat_dir_of(parent, sessions_dir))
    }) && flat_name_in_scan_window(path, range)
}

/// `sessions/YYYY/MM/DD/<name>` for a name starting `rollout-YYYY-MM-DD`:
/// the partition Codex unarchives the file into.
fn codex_partition_path(sessions_dir: &Path, name: &str) -> Option<PathBuf> {
    let stamp = name.strip_prefix("rollout-")?.get(..10)?;
    let day = CostUsageDayRange::parse_day_key(JsonlScanner::codex_filename_day_key(stamp)?)?;
    Some(
        sessions_dir
            .join(day.format("%Y").to_string())
            .join(day.format("%m").to_string())
            .join(day.format("%d").to_string())
            .join(name),
    )
}

fn rollout_file_name(path: &Path) -> Option<&str> {
    path.file_name()?
        .to_str()
        .filter(|name| name.starts_with("rollout-"))
}

fn current_identity(path: &Path) -> Option<String> {
    let metadata = fs::metadata(path).ok()?;
    JsonlScanner::codex_file_identity(path, &metadata)
}

fn cache_has_codex_flat_path_in(cache: &CostUsageCache, dir: &Path) -> bool {
    cache
        .files
        .keys()
        .chain(cache.codex_pending_paths.iter())
        .any(|path| Path::new(path).parent() == Some(dir))
}

/// The flat rollouts one scan pass found.
pub(super) struct CodexFlatListing {
    /// In-window flat rollouts with their modification times.
    pub(super) files: Vec<(PathBuf, i64)>,
    /// Flat copies of a rollout whose dated twin exists. Only the twin is
    /// scanned, so one session is never counted twice.
    copies: Vec<String>,
    /// False when a flat directory that fed the cache could not be listed.
    pub(super) complete: bool,
}

impl CodexFlatListing {
    pub(super) fn read(
        sessions_dirs: &[PathBuf],
        range: &CostUsageDayRange,
        cache: &CostUsageCache,
        cancel: Option<&AtomicBool>,
    ) -> Self {
        let mut listing = Self {
            files: Vec::new(),
            copies: Vec::new(),
            complete: true,
        };
        for (dir, home) in codex_flat_dirs(sessions_dirs) {
            if is_cancelled(cancel) {
                listing.complete = false;
                break;
            }
            let Ok(paths) = JsonlScanner::list_codex_flat_session_files(
                &dir,
                &range.scan_since_key,
                &range.scan_until_key,
            ) else {
                // A missing archive in a reachable home only means nothing
                // was archived, or the archive was removed. An unreachable
                // home (offline drive, stopped WSL distro) that fed the cache
                // is a source failure.
                let absent = !dir.exists() && dir.parent().is_some_and(Path::is_dir);
                if !absent && cache_has_codex_flat_path_in(cache, &dir) {
                    listing.complete = false;
                }
                continue;
            };
            let sessions_dir = sessions_dirs.get(home);
            for path in paths {
                let Ok(metadata) = fs::metadata(&path) else {
                    continue;
                };
                if !metadata.is_file() {
                    continue;
                }
                let has_dated_twin = sessions_dir
                    .zip(path.file_name().and_then(|name| name.to_str()))
                    .and_then(|(sessions_dir, name)| codex_partition_path(sessions_dir, name))
                    .is_some_and(|twin| twin.is_file());
                if has_dated_twin {
                    listing.copies.push(path.to_string_lossy().to_string());
                } else {
                    let mtime_unix_ms = system_time_to_unix_ms(metadata.modified().ok());
                    listing.files.push((path, mtime_unix_ms));
                }
            }
        }
        listing
    }
}

/// Follow archive moves in the cache before discovery reads it. A flat copy
/// of a dated rollout is dropped, an archived rollout takes over the cached
/// usage of the dated file it was moved from, and an unarchived one moves
/// back. A move is matched by name within one Codex home and confirmed by
/// file identity, so a look-alike file is scanned as new instead.
pub(super) fn relocate_moved_codex_rollouts(
    cache: &mut CostUsageCache,
    sessions_dirs: &[PathBuf],
    listing: &CodexFlatListing,
    range: &CostUsageDayRange,
) {
    for copy in &listing.copies {
        drop_codex_cache_path(cache, copy);
    }
    follow_archived_rollouts(cache, sessions_dirs, listing);
    follow_unarchived_rollouts(cache, sessions_dirs, listing, range);
}

/// Archive: a newly listed flat rollout whose same-named cached file in the
/// same home is gone took that file's place.
fn follow_archived_rollouts(
    cache: &mut CostUsageCache,
    sessions_dirs: &[PathBuf],
    listing: &CodexFlatListing,
) {
    let arrivals: Vec<(&Path, &str, usize)> = listing
        .files
        .iter()
        .filter(|(path, _)| !cache.files.contains_key(path.to_string_lossy().as_ref()))
        .filter_map(|(path, _)| {
            let name = rollout_file_name(path)?;
            Some((path.as_path(), name, codex_home_of(path, sessions_dirs)?))
        })
        .collect();
    if arrivals.is_empty() {
        return;
    }
    let names: HashSet<&str> = arrivals.iter().map(|(_, name, _)| *name).collect();
    let mut departed: HashMap<(usize, String), Vec<String>> = HashMap::new();
    for key in cache.files.keys() {
        let path = Path::new(key);
        let Some(name) = rollout_file_name(path).filter(|name| names.contains(name)) else {
            continue;
        };
        let Some(home) = codex_home_of(path, sessions_dirs) else {
            continue;
        };
        if !path.exists() {
            departed
                .entry((home, name.to_string()))
                .or_default()
                .push(key.clone());
        }
    }
    for (arrival, name, home) in arrivals {
        let Some(sources) = departed.get(&(home, name.to_string())) else {
            continue;
        };
        if let Some(source) = moved_from(cache, sources, current_identity(arrival).as_deref()) {
            move_codex_cache_path(cache, &source, &arrival.to_string_lossy());
        }
    }
}

/// The departed cache key an arrival was moved from: the only one whose
/// recorded identity matches the arrival; else, when none matches, the only
/// one recorded before identities were kept.
fn moved_from(
    cache: &CostUsageCache,
    departed: &[String],
    arrival_identity: Option<&str>,
) -> Option<String> {
    let recorded: Vec<(&String, Option<&str>)> = departed
        .iter()
        .filter_map(|key| {
            let usage = cache.files.get(key)?;
            Some((key, usage.codex_file_identity.as_deref()))
        })
        .collect();
    let matching: Vec<&String> = recorded
        .iter()
        .filter(|(_, identity)| identity.is_some() && *identity == arrival_identity)
        .map(|(key, _)| *key)
        .collect();
    let unrecorded: Vec<&String> = recorded
        .iter()
        .filter(|(_, identity)| identity.is_none())
        .map(|(key, _)| *key)
        .collect();
    match (matching.as_slice(), unrecorded.as_slice()) {
        ([only], _) | ([], [only]) => Some((*only).clone()),
        _ => None,
    }
}

/// Unarchive: a cached flat rollout that is gone while the dated file its
/// name points to exists was moved back into its partition.
fn follow_unarchived_rollouts(
    cache: &mut CostUsageCache,
    sessions_dirs: &[PathBuf],
    listing: &CodexFlatListing,
    range: &CostUsageDayRange,
) {
    let listed: HashSet<&Path> = listing
        .files
        .iter()
        .map(|(path, _)| path.as_path())
        .collect();
    let returned: Vec<(String, PathBuf)> = cache
        .files
        .keys()
        .filter_map(|key| {
            let path = Path::new(key);
            if listed.contains(path)
                || !is_flat_codex_path_in_scan_window(path, sessions_dirs, range)
            {
                return None;
            }
            let sessions_dir = sessions_dirs.get(codex_home_of(path, sessions_dirs)?)?;
            let target = codex_partition_path(sessions_dir, rollout_file_name(path)?)?;
            (!path.exists() && target.is_file()).then(|| (key.clone(), target))
        })
        .collect();
    for (from, target) in returned {
        let to = target.to_string_lossy().to_string();
        if cache.files.contains_key(&to) {
            // The dated file is already counted; the flat entry is stale.
            drop_codex_cache_path(cache, &from);
            continue;
        }
        let recorded = cache
            .files
            .get(&from)
            .and_then(|usage| usage.codex_file_identity.clone());
        if recorded.is_none() || recorded == current_identity(&target) {
            move_codex_cache_path(cache, &from, &to);
        }
    }
}

fn move_codex_cache_path(cache: &mut CostUsageCache, from: &str, to: &str) {
    if let Some(usage) = cache.files.remove(from) {
        cache.files.insert(to.to_string(), usage);
    }
    if let Some(rows) = cache.codex_source_rows.remove(from) {
        cache.codex_source_rows.insert(to.to_string(), rows);
    }
    if let Some(rows) = cache.codex_fork_rows.remove(from) {
        cache.codex_fork_rows.insert(to.to_string(), rows);
    }
    if cache.codex_pending_paths.iter().any(|path| path == to) {
        cache.codex_pending_paths.retain(|path| path != from);
    } else {
        for path in &mut cache.codex_pending_paths {
            if path == from {
                *path = to.to_string();
            }
        }
    }
}

fn drop_codex_cache_path(cache: &mut CostUsageCache, path: &str) {
    cache.files.remove(path);
    cache.codex_source_rows.remove(path);
    cache.codex_fork_rows.remove(path);
    cache.codex_pending_paths.retain(|pending| pending != path);
}

/// The oldest day a flat rollout name carries, so an all-available window
/// also reaches archived history older than the first date partition. Days
/// before 1970 are stray names, not sessions.
pub(super) fn earliest_flat_codex_day(sessions_dirs: &[PathBuf]) -> Option<NaiveDate> {
    codex_flat_dirs(sessions_dirs)
        .iter()
        .filter_map(|(dir, _)| {
            JsonlScanner::list_codex_flat_session_files(dir, "1970-01-01", "9999-12-31").ok()
        })
        .flatten()
        .filter_map(|path| {
            let name = path.file_name()?.to_string_lossy().into_owned();
            CostUsageDayRange::parse_day_key(JsonlScanner::codex_filename_day_key(&name)?)
        })
        .min()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::CodexSourceRowCache;

    fn usage(identity: Option<&str>) -> CostUsageFileUsage {
        CostUsageFileUsage {
            mtime_unix_ms: 0,
            size: 100,
            codex_file_identity: identity.map(str::to_string),
            days: HashMap::new(),
            parsed_bytes: Some(100),
            codex_scan_target_size: None,
            last_model: None,
            last_totals: None,
            codex_token_timestamps_monotonic: Some(true),
            codex_last_token_timestamp: None,
            codex_session_id: None,
            codex_forked_from_id: None,
            codex_fork_accounting_state: None,
            codex_lineage: CodexSessionLineage::Root,
            codex_fork_timestamp: None,
            codex_unresolved_fork_parent: false,
        }
    }

    fn cache_with(entries: &[(&str, Option<&str>)]) -> CostUsageCache {
        CostUsageCache {
            files: entries
                .iter()
                .map(|(key, identity)| ((*key).to_string(), usage(*identity)))
                .collect(),
            ..CostUsageCache::default()
        }
    }

    fn keys(keys: &[&str]) -> Vec<String> {
        keys.iter().map(|key| (*key).to_string()).collect()
    }

    #[test]
    fn a_move_is_confirmed_by_file_identity() {
        let cache = cache_with(&[("a", Some("7:1")), ("b", Some("7:2")), ("c", None)]);
        let departed = keys(&["a", "b", "c"]);
        assert_eq!(
            moved_from(&cache, &departed, Some("7:2")),
            Some("b".to_string())
        );
        // With no identity match, only an entry cached before identities were
        // recorded can be the source.
        assert_eq!(
            moved_from(&cache, &departed, Some("7:9")),
            Some("c".to_string())
        );
        assert_eq!(moved_from(&cache, &keys(&["a", "b"]), Some("7:9")), None);

        let unrecorded = cache_with(&[("a", None), ("b", None)]);
        assert_eq!(
            moved_from(&unrecorded, &keys(&["a", "b"]), Some("7:1")),
            None,
            "two unrecorded sources are ambiguous"
        );
        assert_eq!(
            moved_from(&unrecorded, &keys(&["a", "gone"]), None),
            Some("a".to_string()),
            "keys no longer cached are ignored"
        );
    }

    #[test]
    fn moving_a_cache_path_carries_its_rows_and_pending_entry() {
        let mut cache = cache_with(&[("from", Some("1:1"))]);
        cache.codex_source_rows.insert(
            "from".to_string(),
            CodexSourceRowCache {
                file_identity: "1:1".to_string(),
                size: 100,
                mtime_unix_ms: 0,
                prefix_hash: 0,
                rows: Vec::new(),
                scan_since_key: None,
                scan_until_key: None,
            },
        );
        cache.codex_fork_rows.insert("from".to_string(), Vec::new());
        cache.codex_pending_paths = keys(&["from", "other"]);

        move_codex_cache_path(&mut cache, "from", "to");

        assert!(cache.files.contains_key("to") && !cache.files.contains_key("from"));
        assert!(cache.codex_source_rows.contains_key("to"));
        assert!(!cache.codex_source_rows.contains_key("from"));
        assert!(cache.codex_fork_rows.contains_key("to"));
        assert!(!cache.codex_fork_rows.contains_key("from"));
        assert_eq!(cache.codex_pending_paths, keys(&["to", "other"]));

        let mut queued = cache_with(&[("from", None)]);
        queued.codex_pending_paths = keys(&["from", "to"]);
        move_codex_cache_path(&mut queued, "from", "to");
        assert_eq!(queued.codex_pending_paths, keys(&["to"]));

        drop_codex_cache_path(&mut cache, "to");
        assert!(cache.files.is_empty() && cache.codex_source_rows.is_empty());
        assert!(cache.codex_fork_rows.is_empty());
        assert_eq!(cache.codex_pending_paths, keys(&["other"]));
    }

    #[test]
    fn rollout_names_map_to_their_partition_and_archive() {
        let home = PathBuf::from("codex-home");
        let sessions = home.join("sessions");
        let name = "rollout-2025-10-03T10-00-00-0199a213.jsonl";
        assert_eq!(
            codex_partition_path(&sessions, name),
            Some(sessions.join("2025").join("10").join("03").join(name))
        );
        assert_eq!(
            codex_partition_path(&sessions, "imported-session.jsonl"),
            None
        );
        assert_eq!(
            codex_partition_path(&sessions, "rollout-2025-13-03T10-00-00-x.jsonl"),
            None
        );

        assert_eq!(
            codex_archived_sessions_dir(&sessions),
            Some(home.join("archived_sessions"))
        );
        assert_eq!(
            codex_archived_sessions_dir(&home.join("Sessions")),
            Some(home.join("archived_sessions"))
        );
        assert_eq!(codex_archived_sessions_dir(&home.join("custom-root")), None);
    }

    #[test]
    fn a_cached_rollout_belongs_to_one_codex_home() {
        let home_a = PathBuf::from("home-a");
        let home_b = PathBuf::from("home-b");
        let dirs = vec![home_a.join("sessions"), home_b.join("sessions")];
        let dated = dirs[0].join("2025").join("10").join("03").join("x.jsonl");
        let archived = home_b.join("archived_sessions").join("x.jsonl");
        let nested = home_b
            .join("archived_sessions")
            .join("nested")
            .join("x.jsonl");

        assert_eq!(codex_home_of(&dated, &dirs), Some(0));
        assert_eq!(codex_home_of(&archived, &dirs), Some(1));
        assert_eq!(codex_home_of(&nested, &dirs), None);
        assert_eq!(
            codex_flat_dirs(&dirs),
            vec![
                (dirs[0].clone(), 0),
                (home_a.join("archived_sessions"), 0),
                (dirs[1].clone(), 1),
                (home_b.join("archived_sessions"), 1),
            ]
        );

        let range = CostUsageDayRange::new(
            NaiveDate::from_ymd_opt(2025, 10, 2).unwrap(),
            NaiveDate::from_ymd_opt(2025, 10, 4).unwrap(),
        );
        let flat = |name: &str| home_b.join("archived_sessions").join(name);
        assert!(is_flat_codex_path_in_scan_window(
            &flat("rollout-2025-10-05T10-00-00-x.jsonl"),
            &dirs,
            &range
        ));
        assert!(!is_flat_codex_path_in_scan_window(
            &flat("rollout-2025-10-06T10-00-00-x.jsonl"),
            &dirs,
            &range
        ));
        assert!(is_flat_codex_path_in_scan_window(
            &dirs[0].join("legacy.jsonl"),
            &dirs,
            &range
        ));
        assert!(!is_flat_codex_path_in_scan_window(&dated, &dirs, &range));
        assert!(!is_flat_codex_path_in_scan_window(&nested, &dirs, &range));
    }
}
