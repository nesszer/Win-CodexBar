//! Independent Codex chats (upstream 0.72.0 `CodexProjectlessWorkspaceMetadata`
//! and `CodexThreadMetadataReader+Projects`).
//!
//! Codex lists desktop chats that run in a temporary folder under
//! `projectless-thread-ids` in `<codex home>/.codex-global-state.json`. A project
//! whose every session is listed is an independent chat, unless one of its
//! sessions has been assigned to a real project. Assignments come from the global
//! state (`thread-project-assignments`), from a non-null `threads.project_id` in
//! the state database, and from a folder covered by a registered project root.
//! Any uncertainty (missing or unreadable database, malformed roots, too many
//! candidates) keeps the project a project.
//!
//! Only presentation changes: the label becomes the chat title (or a neutral
//! label). Totals, costs, and project identity are untouched.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags};
use serde::Deserialize;

use super::indexer::sort_projects;
use super::project_names::{match_key, with_separator};
use super::thread_names::{
    BUSY_TIMEOUT, configured_sqlite_home, newest_state_database, sqlite_home,
};
use super::types::{CodexLocalProjectUsageSnapshot, SessionUsage, untitled_session_label};

const STATE_FILE: &str = ".codex-global-state.json";
/// Larger global-state files are ignored entirely.
const STATE_BYTE_LIMIT: u64 = 8 * 1024 * 1024;
/// Root rows read from the database. More rows keep every project a project.
const ROOT_ROW_LIMIT: usize = 1024;
/// Candidate sessions checked against the database. More keep every project a project.
const CANDIDATE_LIMIT: usize = 4096;
/// Title length kept for a chat name, in characters.
const TITLE_CHAR_LIMIT: usize = 128;

const CHAT_NAME: &str = "Independent chat";
const CHATS_NAME: &str = "Independent chats";

#[derive(Deserialize)]
struct GlobalState {
    #[serde(rename = "projectless-thread-ids", default)]
    projectless_thread_ids: Option<Vec<String>>,
    #[serde(rename = "thread-project-assignments", default)]
    thread_project_assignments: Option<HashMap<String, Assignment>>,
}

/// Only the presence of an assignment matters, not its project.
#[derive(Deserialize)]
struct Assignment {
    #[serde(rename = "projectId", default)]
    _project_id: Option<String>,
}

/// Markers read from the global state.
struct Markers {
    thread_ids: HashSet<String>,
    assigned_thread_ids: HashSet<String>,
}

/// What the state database says about one project's candidate sessions.
struct Ownership {
    /// Match keys of registered project roots.
    root_keys: Vec<String>,
    /// Candidates with a non-null `threads.project_id`.
    assigned: HashSet<String>,
}

/// Mark independent chats and label them, on fresh and cached snapshots alike.
///
/// Runs after the saved-name overlay, so a chat's title replaces any saved project
/// name. Every project's flag is reset first, so a marker removed since a cached
/// snapshot was written does not linger.
pub(super) fn apply_projectless_chats(
    snapshot: &mut CodexLocalProjectUsageSnapshot,
    codex_home: &Path,
    env_sqlite_home: Option<&str>,
) {
    for project in &mut snapshot.projects {
        project.is_projectless = false;
    }
    let Some(markers) = load_markers(codex_home) else {
        return;
    };
    if markers.thread_ids.is_empty() {
        return;
    }

    let mut sessions_by_project: HashMap<&str, Vec<&SessionUsage>> = HashMap::new();
    for session in &snapshot.sessions {
        sessions_by_project
            .entry(session.project_id.as_str())
            .or_default()
            .push(session);
    }

    // Only projects whose every session is a marked chat need database evidence.
    let configured_home = configured_sqlite_home(codex_home);
    let mut newest_by_dir: HashMap<PathBuf, PathBuf> = HashMap::new();
    let mut by_database: HashMap<PathBuf, Vec<usize>> = HashMap::new();
    for (index, project) in snapshot.projects.iter().enumerate() {
        let Some(path) = project
            .path
            .as_deref()
            .filter(|path| Path::new(path).is_absolute())
        else {
            continue;
        };
        let Some(sessions) = sessions_by_project.get(project.id.as_str()) else {
            continue;
        };
        if !sessions
            .iter()
            .all(|session| markers.thread_ids.contains(&session.id))
        {
            continue;
        }
        let dir = sqlite_home(
            codex_home,
            configured_home.as_deref(),
            env_sqlite_home,
            Some(path),
        );
        let database = newest_by_dir
            .entry(dir.clone())
            .or_insert_with(|| newest_state_database(&dir))
            .clone();
        by_database.entry(database).or_default().push(index);
    }

    let mut decisions: Vec<(usize, String)> = Vec::new();
    for (database, indices) in by_database {
        let candidate_ids: Vec<&str> = indices
            .iter()
            .flat_map(|&index| {
                sessions_by_project[snapshot.projects[index].id.as_str()]
                    .iter()
                    .map(|session| session.id.as_str())
            })
            .collect();
        // Unknown ownership vetoes every candidate in this database.
        let Some(ownership) = inspect_database(&database, &candidate_ids) else {
            continue;
        };
        for index in indices {
            let project = &snapshot.projects[index];
            let Some(path) = project.path.as_deref() else {
                continue;
            };
            let sessions = &sessions_by_project[project.id.as_str()];
            if owned_by_root(&ownership.root_keys, &match_key(path)) {
                continue;
            }
            let assigned = sessions.iter().any(|session| {
                ownership.assigned.contains(&session.id)
                    || markers.assigned_thread_ids.contains(&session.id)
            });
            if assigned {
                continue;
            }
            decisions.push((index, chat_name(sessions)));
        }
    }

    for (index, name) in decisions {
        let project = &mut snapshot.projects[index];
        project.is_projectless = true;
        project.display_name = name;
    }
    sort_projects(&mut snapshot.projects);
}

/// Global markers, or `None` when the file is absent, oversized, or malformed.
fn load_markers(codex_home: &Path) -> Option<Markers> {
    let file = File::open(codex_home.join(STATE_FILE)).ok()?;
    let mut bytes = Vec::new();
    file.take(STATE_BYTE_LIMIT + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if u64::try_from(bytes.len()).ok()? > STATE_BYTE_LIMIT {
        return None;
    }
    let state: GlobalState = serde_json::from_slice(&bytes).ok()?;
    Some(Markers {
        thread_ids: state
            .projectless_thread_ids
            .unwrap_or_default()
            .into_iter()
            .collect(),
        assigned_thread_ids: state
            .thread_project_assignments
            .unwrap_or_default()
            .into_keys()
            .collect(),
    })
}

/// Read-only ownership evidence from one state database, or `None` when any part
/// of it cannot be read or is outside the supported bounds.
fn inspect_database(database: &Path, candidates: &[&str]) -> Option<Ownership> {
    if !database.is_file() {
        return None;
    }
    let conn = Connection::open_with_flags(
        database,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .ok()?;
    conn.busy_timeout(BUSY_TIMEOUT).ok()?;
    let root_keys = root_keys(&conn)?;
    let assigned = thread_assignments(&conn, candidates)?;
    Some(Ownership {
        root_keys,
        assigned,
    })
}

/// Match keys of every registered root. A database with neither project table is
/// a supported legacy schema with no roots; a partial schema or a bad row is not.
fn root_keys(conn: &Connection) -> Option<Vec<String>> {
    const QUERY: &str = "SELECT r.path FROM project_roots r \
         LEFT JOIN projects p ON r.project_id = p.id LIMIT 1025";
    let mut statement = match conn.prepare(QUERY) {
        Ok(statement) => statement,
        Err(_) => {
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE name IN ('projects', 'project_roots')",
                    [],
                    |row| row.get(0),
                )
                .ok()?;
            return (count == 0).then(Vec::new);
        }
    };
    let mut rows = statement.query([]).ok()?;
    let mut keys = Vec::new();
    while let Some(row) = rows.next().ok()? {
        if keys.len() >= ROOT_ROW_LIMIT {
            return None;
        }
        let ValueRef::Text(bytes) = row.get_ref(0).ok()? else {
            return None;
        };
        let path = String::from_utf8_lossy(bytes).into_owned();
        if !Path::new(&path).is_absolute() {
            return None;
        }
        keys.push(match_key(&path));
    }
    Some(keys)
}

/// Candidates with a non-null `threads.project_id`. A missing `threads` table or
/// `id` column is unknown ownership; a missing `project_id` column assigns nothing.
fn thread_assignments(conn: &Connection, candidates: &[&str]) -> Option<HashSet<String>> {
    if candidates.is_empty() {
        return Some(HashSet::new());
    }
    if candidates.len() > CANDIDATE_LIMIT {
        return None;
    }
    let mut info = conn.prepare("PRAGMA table_info(threads)").ok()?;
    let columns: HashSet<String> = info
        .query_map([], |row| row.get::<_, String>(1))
        .ok()?
        .collect::<Result<_, _>>()
        .ok()?;
    if !columns.contains("id") {
        return None;
    }
    if !columns.contains("project_id") {
        return Some(HashSet::new());
    }
    let mut lookup = conn
        .prepare("SELECT project_id FROM threads WHERE id = ?1")
        .ok()?;
    let mut assigned = HashSet::new();
    for id in candidates {
        let mut rows = lookup.query([*id]).ok()?;
        if let Some(row) = rows.next().ok()? {
            // Any non-null value, including a blank or malformed one, is an assignment.
            if !matches!(row.get_ref(0).ok()?, ValueRef::Null) {
                assigned.insert((*id).to_string());
            }
            // Two rows for one thread is ambiguous.
            if rows.next().ok()?.is_some() {
                return None;
            }
        }
    }
    Some(assigned)
}

/// True when `path_key` is a root or lies beneath one. A sibling that only shares a
/// name prefix (`/work` and `/workshop`) is not covered.
fn owned_by_root(root_keys: &[String], path_key: &str) -> bool {
    root_keys
        .iter()
        .any(|root| path_key == root || path_key.starts_with(&with_separator(root)))
}

/// Name for a chat project: a single session's title, or a neutral label.
fn chat_name(sessions: &[&SessionUsage]) -> String {
    match sessions {
        [only] => chat_title(only).unwrap_or_else(|| CHAT_NAME.to_string()),
        _ => CHATS_NAME.to_string(),
    }
}

/// The session's title when it is worth showing: not blank, not the untitled
/// fallback, not an attachment or referenced-conversation preamble, and capped.
fn chat_title(session: &SessionUsage) -> Option<String> {
    let title = session.display_title.trim();
    if title.is_empty()
        || title == untitled_session_label(&session.id)
        || title.starts_with("# Files mentioned by the user:")
        || title.starts_with("## Referenced ChatGPT conversation:")
    {
        return None;
    }
    Some(title.chars().take(TITLE_CHAR_LIMIT).collect())
}

#[cfg(test)]
mod tests;
