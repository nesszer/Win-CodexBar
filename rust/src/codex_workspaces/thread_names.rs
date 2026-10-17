//! Codex thread names for indexed sessions (upstream 0.68.0 `CodexThreadMetadataReader`).
//!
//! Codex keeps thread names outside the rollout files, so they are overlaid onto
//! usage snapshots. Order: `session_index.jsonl` in the Codex home (latest line
//! wins), then `threads.title` in the newest `state_<n>.sqlite`. Everything is
//! read-only and best effort: any failure leaves the session untitled.

use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde::Deserialize;

use super::types::{CodexLocalProjectUsageSnapshot, SessionUsage, untitled_session_label};

/// Lines longer than this in `session_index.jsonl` are skipped.
const SESSION_INDEX_MAX_LINE_BYTES: u64 = 64 * 1024;
/// Only the head of `config.toml` is read when looking for `sqlite_home`.
const CONFIG_PREFIX_BYTES: u64 = 256 * 1024;
pub(super) const BUSY_TIMEOUT: Duration = Duration::from_millis(100);
const DEFAULT_STATE_DATABASE: &str = "state_5.sqlite";
const PROJECT_TOP_SESSION_LIMIT: usize = 5;

/// A session whose thread name is wanted: its id and original working directory.
pub(super) struct SessionRef<'a> {
    pub id: &'a str,
    pub cwd: Option<&'a str>,
}

#[derive(Deserialize)]
struct SessionIndexEntry {
    id: String,
    thread_name: String,
}

/// Thread names keyed by session id for the given sessions.
///
/// `env_sqlite_home` is the raw `CODEX_SQLITE_HOME` value, passed in so callers
/// (and tests) control the environment.
pub(super) fn thread_titles(
    codex_home: &Path,
    env_sqlite_home: Option<&str>,
    sessions: &[SessionRef<'_>],
) -> HashMap<String, String> {
    if sessions.is_empty() {
        return HashMap::new();
    }
    let ids: HashSet<&str> = sessions.iter().map(|s| s.id).collect();
    let mut titles = indexed_thread_names(&codex_home.join("session_index.jsonl"), &ids);
    if titles.len() == ids.len() {
        return titles;
    }

    let configured_home = configured_sqlite_home(codex_home);
    let mut newest_by_dir: HashMap<PathBuf, PathBuf> = HashMap::new();
    let mut ids_by_database: HashMap<PathBuf, Vec<&str>> = HashMap::new();
    for session in sessions {
        if titles.contains_key(session.id) {
            continue;
        }
        let dir = sqlite_home(
            codex_home,
            configured_home.as_deref(),
            env_sqlite_home,
            session.cwd,
        );
        let database = newest_by_dir
            .entry(dir.clone())
            .or_insert_with(|| newest_state_database(&dir))
            .clone();
        ids_by_database
            .entry(database)
            .or_default()
            .push(session.id);
    }
    for (database, session_ids) in ids_by_database {
        for (id, title) in database_titles(&database, &session_ids) {
            // The session index is the explicit name; the database is the fallback.
            titles.entry(id).or_insert(title);
        }
    }
    titles
}

/// Apply metadata names and ranking to fresh and cached snapshots.
/// Names never touch totals, cost, or project grouping.
pub(super) fn apply_session_names_and_ranking(
    snapshot: &mut CodexLocalProjectUsageSnapshot,
    codex_home: &Path,
    env_sqlite_home: Option<&str>,
) {
    let has_sessions = !snapshot.sessions.is_empty();
    let titles = {
        let mut seen = HashSet::new();
        let mut refs = Vec::new();
        for session in &snapshot.sessions {
            if seen.insert(session.id.as_str()) {
                refs.push(SessionRef {
                    id: &session.id,
                    cwd: session.cwd.as_deref(),
                });
            }
        }
        if !has_sessions {
            for session in snapshot
                .projects
                .iter()
                .flat_map(|project| project.top_sessions.iter())
            {
                if seen.insert(session.id.as_str()) {
                    refs.push(SessionRef {
                        id: &session.id,
                        cwd: session.cwd.as_deref(),
                    });
                }
            }
        }
        thread_titles(codex_home, env_sqlite_home, &refs)
    };

    for session in &mut snapshot.sessions {
        apply_thread_title(session, &titles);
    }
    snapshot.sessions.sort_by(SessionUsage::rank_cmp);

    if has_sessions {
        let mut top_sessions_by_project: HashMap<String, Vec<SessionUsage>> = HashMap::new();
        for session in &snapshot.sessions {
            let top_sessions = top_sessions_by_project
                .entry(session.project_id.clone())
                .or_default();
            if top_sessions.len() < PROJECT_TOP_SESSION_LIMIT {
                top_sessions.push(session.clone());
            }
        }
        for project in &mut snapshot.projects {
            project.top_sessions = top_sessions_by_project
                .remove(&project.id)
                .unwrap_or_default();
        }
    } else {
        for project in &mut snapshot.projects {
            for session in &mut project.top_sessions {
                apply_thread_title(session, &titles);
            }
            project.top_sessions.sort_by(SessionUsage::rank_cmp);
            project.top_sessions.truncate(PROJECT_TOP_SESSION_LIMIT);
        }
    }
}

fn apply_thread_title(session: &mut SessionUsage, titles: &HashMap<String, String>) {
    session.display_title = titles
        .get(&session.id)
        .cloned()
        .unwrap_or_else(|| untitled_session_label(&session.id));
}

/// `session_index.jsonl`: `{"id": ..., "thread_name": ...}` per line, latest wins.
fn indexed_thread_names(path: &Path, ids: &HashSet<&str>) -> HashMap<String, String> {
    let mut names = HashMap::new();
    let Ok(file) = File::open(path) else {
        return names;
    };
    let mut reader = BufReader::new(file);
    let mut line = Vec::new();
    loop {
        line.clear();
        let Ok(read) = (&mut reader)
            .take(SESSION_INDEX_MAX_LINE_BYTES + 1)
            .read_until(b'\n', &mut line)
        else {
            break;
        };
        if read == 0 {
            break;
        }
        if line.last() != Some(&b'\n') && read as u64 > SESSION_INDEX_MAX_LINE_BYTES {
            // Over-long line: drop the remainder without buffering it.
            if !skip_to_line_end(&mut reader) {
                break;
            }
            continue;
        }
        let Ok(entry) = serde_json::from_slice::<SessionIndexEntry>(&line) else {
            continue;
        };
        let name = entry.thread_name.trim();
        if !name.is_empty() && ids.contains(entry.id.as_str()) {
            names.insert(entry.id, name.to_string());
        }
    }
    names
}

/// Consume input up to and including the next newline. Returns `false` at EOF or on error.
fn skip_to_line_end<R: BufRead>(reader: &mut R) -> bool {
    loop {
        let Ok(buffer) = reader.fill_buf() else {
            return false;
        };
        if buffer.is_empty() {
            return false;
        }
        match buffer.iter().position(|byte| *byte == b'\n') {
            Some(index) => {
                reader.consume(index + 1);
                return true;
            }
            None => {
                let len = buffer.len();
                reader.consume(len);
            }
        }
    }
}

/// Directory holding `state_<n>.sqlite` for one session.
pub(super) fn sqlite_home(
    codex_home: &Path,
    configured_home: Option<&Path>,
    env_sqlite_home: Option<&str>,
    cwd: Option<&str>,
) -> PathBuf {
    if let Some(configured) = configured_home {
        return configured.to_path_buf();
    }
    if let Some(raw) = env_sqlite_home.map(str::trim).filter(|v| !v.is_empty()) {
        let path = Path::new(raw);
        if path.is_absolute() {
            return path.to_path_buf();
        }
        if let Some(cwd) = cwd.map(str::trim).filter(|c| !c.is_empty()) {
            return Path::new(cwd).join(path);
        }
    }
    codex_home.to_path_buf()
}

/// Absolute top-level `sqlite_home` from `<codex home>/config.toml`.
///
/// Only the user config is read: project, managed and profile layers are not
/// persisted with the rollout and an untrusted checkout must not redirect the
/// lookup. Only the first 256 KiB is read.
pub(super) fn configured_sqlite_home(codex_home: &Path) -> Option<PathBuf> {
    let file = File::open(codex_home.join("config.toml")).ok()?;
    let mut bytes = Vec::new();
    file.take(CONFIG_PREFIX_BYTES)
        .read_to_end(&mut bytes)
        .ok()?;
    let contents = String::from_utf8_lossy(&bytes);
    let raw = top_level_sqlite_home(&contents)?;
    let path = PathBuf::from(raw);
    path.is_absolute().then_some(path)
}

fn top_level_sqlite_home(contents: &str) -> Option<String> {
    let table = contents.parse::<toml::Table>().ok().or_else(|| {
        // A truncated or malformed table section must not hide a valid
        // top-level key, so retry with everything before the first table header.
        let mut offset = 0usize;
        let mut end = contents.len();
        for line in contents.split_inclusive('\n') {
            if line.trim_start().starts_with('[') {
                end = offset;
                break;
            }
            offset += line.len();
        }
        contents[..end].parse::<toml::Table>().ok()
    })?;
    match table.get("sqlite_home")? {
        toml::Value::String(value) => Some(value.clone()),
        _ => None,
    }
}

/// Highest `state_<n>.sqlite` in `dir`, else the default `state_5.sqlite` path.
pub(super) fn newest_state_database(dir: &Path) -> PathBuf {
    let newest = fs::read_dir(dir).ok().and_then(|entries| {
        entries
            .flatten()
            .filter_map(|entry| {
                let name = entry.file_name().into_string().ok()?;
                let version = name
                    .strip_prefix("state_")?
                    .strip_suffix(".sqlite")?
                    .parse::<u64>()
                    .ok()?;
                if !entry.metadata().ok()?.is_file() {
                    return None;
                }
                Some((version, entry.path()))
            })
            .max_by(|a, b| a.0.cmp(&b.0).then_with(|| b.1.cmp(&a.1)))
            .map(|(_, path)| path)
    });
    newest.unwrap_or_else(|| dir.join(DEFAULT_STATE_DATABASE))
}

/// `SELECT title FROM threads WHERE id = ?1 LIMIT 1` for each id, read-only.
fn database_titles(database: &Path, ids: &[&str]) -> Vec<(String, String)> {
    if !database.is_file() {
        return Vec::new();
    }
    let Ok(conn) = Connection::open_with_flags(
        database,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) else {
        return Vec::new();
    };
    if conn.busy_timeout(BUSY_TIMEOUT).is_err() {
        return Vec::new();
    }
    let Ok(mut statement) = conn.prepare("SELECT title FROM threads WHERE id = ?1 LIMIT 1") else {
        return Vec::new();
    };
    ids.iter()
        .filter_map(|id| {
            let title = statement
                .query_row([id], |row| row.get::<_, Option<String>>(0))
                .optional()
                .ok()??;
            let title = title?.trim().to_string();
            (!title.is_empty()).then(|| ((*id).to_string(), title))
        })
        .collect()
}

#[cfg(test)]
mod tests;
