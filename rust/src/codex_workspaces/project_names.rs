//! Saved Codex project names (upstream 0.71.1 `CodexThreadMetadataReader+Projects`).
//!
//! Codex stores renamed projects in the `projects` table of the newest
//! `state_<n>.sqlite`, joined to `project_roots`. A saved name replaces the
//! folder label of the project whose root is the nearest unambiguous match.
//! Names are presentation metadata only: totals, cost, and project identity never
//! change. A missing or unreadable database, blank names, and ambiguous roots all
//! keep the folder label. Everything is read-only, and the database is never
//! created.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags, Row};

use super::indexer::sort_projects;
use super::thread_names::{
    BUSY_TIMEOUT, configured_sqlite_home, newest_state_database, sqlite_home,
};
use super::types::CodexLocalProjectUsageSnapshot;

/// Root rows read from the database. A database with more rows is skipped
/// entirely, so a huge project table cannot stall a usage load.
const ROOT_ROW_LIMIT: usize = 1024;

const ROOTS_QUERY: &str = "SELECT p.id, p.name, r.path FROM projects p \
     JOIN project_roots r ON r.project_id = p.id LIMIT 1025";

/// One saved root: the owning project, its saved name, and the root's match key.
struct SavedRoot {
    project_id: String,
    name: String,
    key: String,
}

/// Overlay saved project names onto fresh and cached snapshots.
///
/// Each project with an absolute path is labelled with its saved name when the
/// nearest root match is unambiguous, otherwise with its folder name. The folder
/// label is recomputed here, so a name removed in Codex does not survive in a
/// cached snapshot. Projects are then re-sorted in the indexer's order.
pub(super) fn apply_saved_project_names(
    snapshot: &mut CodexLocalProjectUsageSnapshot,
    codex_home: &Path,
    env_sqlite_home: Option<&str>,
) {
    let configured_home = configured_sqlite_home(codex_home);
    let mut newest_by_dir: HashMap<PathBuf, PathBuf> = HashMap::new();
    let mut by_database: HashMap<PathBuf, Vec<(usize, String)>> = HashMap::new();
    for (index, project) in snapshot.projects.iter().enumerate() {
        let Some(path) = project
            .path
            .as_deref()
            .filter(|path| Path::new(path).is_absolute())
        else {
            continue;
        };
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
        by_database
            .entry(database)
            .or_default()
            .push((index, path.to_string()));
    }

    for (database, projects) in by_database {
        let roots = saved_roots(&database);
        for (index, path) in projects {
            let saved = roots
                .as_deref()
                .and_then(|roots| saved_name_for(roots, &match_key(&path)));
            snapshot.projects[index].display_name = match saved {
                Some(name) => name.to_string(),
                None => folder_label(&path),
            };
        }
    }
    sort_projects(&mut snapshot.projects);
}

/// The saved roots of one database; `None` when it is absent, unreadable, has no
/// project tables, or exceeds the row limit.
fn saved_roots(database: &Path) -> Option<Vec<SavedRoot>> {
    if !database.is_file() {
        return None;
    }
    let conn = Connection::open_with_flags(
        database,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .ok()?;
    conn.busy_timeout(BUSY_TIMEOUT).ok()?;
    let mut statement = conn.prepare(ROOTS_QUERY).ok()?;
    let mut rows = statement.query([]).ok()?;
    let mut roots = Vec::new();
    let mut rows_read = 0usize;
    while let Some(row) = rows.next().ok()? {
        if rows_read >= ROOT_ROW_LIMIT {
            return None;
        }
        rows_read += 1;
        let (Some(project_id), Some(name), Some(path)) = (
            text_column(row, 0),
            text_column(row, 1),
            text_column(row, 2),
        ) else {
            continue;
        };
        if !Path::new(&path).is_absolute() {
            continue;
        }
        roots.push(SavedRoot {
            project_id,
            name,
            key: match_key(&path),
        });
    }
    Some(roots)
}

/// Text of a column as SQLite's text conversion gives it, trimmed. NULL, blobs,
/// reals, and blank text yield `None`.
fn text_column(row: &Row<'_>, index: usize) -> Option<String> {
    let value = match row.get_ref(index).ok()? {
        ValueRef::Text(bytes) => String::from_utf8_lossy(bytes).into_owned(),
        ValueRef::Integer(number) => number.to_string(),
        ValueRef::Null | ValueRef::Real(_) | ValueRef::Blob(_) => return None,
    };
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// The saved name for `key`: the longest root that equals it or contains it.
/// Returns `None` when the longest roots belong to different projects.
fn saved_name_for<'a>(roots: &'a [SavedRoot], key: &str) -> Option<&'a str> {
    let matches: Vec<&SavedRoot> = roots
        .iter()
        .filter(|root| key == root.key || key.starts_with(&with_separator(&root.key)))
        .collect();
    let longest = matches.iter().map(|root| root.key.len()).max()?;
    let closest: Vec<&SavedRoot> = matches
        .into_iter()
        .filter(|root| root.key.len() == longest)
        .collect();
    let owner = &closest.first()?.project_id;
    if closest.iter().any(|root| &root.project_id != owner) {
        return None;
    }
    closest.first().map(|root| root.name.as_str())
}

/// A key ending in a separator, so `/work` matches `/work/x` but not `/worker`.
pub(super) fn with_separator(key: &str) -> String {
    if key.ends_with('/') {
        key.to_string()
    } else {
        format!("{key}/")
    }
}

/// Comparison key for a path: forward slashes, no verbatim (`\\?\`) prefix,
/// lexically normalized, and lowercase because Windows paths are
/// case-insensitive.
pub(super) fn match_key(raw: &str) -> String {
    let unified = raw.trim().replace('\\', "/");
    let body = unified
        .strip_prefix("//?/")
        .or_else(|| unified.strip_prefix("//./"))
        .unwrap_or(&unified);
    let rooted = body.starts_with('/');
    let mut parts: Vec<&str> = Vec::new();
    for part in body.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    let mut key = String::new();
    if rooted {
        key.push('/');
    }
    key.push_str(&parts.join("/"));
    if key.ends_with(':') {
        // A bare drive (`C:`) is a root only with its separator (`C:/`).
        key.push('/');
    }
    key.to_lowercase()
}

/// Folder label of a project path, as the indexer labels projects.
fn folder_label(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| path.to_string())
}

#[cfg(test)]
mod tests;
