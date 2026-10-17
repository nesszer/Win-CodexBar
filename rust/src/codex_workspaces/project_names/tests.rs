use std::fs;
use std::path::{Path, PathBuf};

use chrono::{TimeZone, Utc};
use rusqlite::{Connection, params};
use tempfile::TempDir;

use super::apply_saved_project_names;
use crate::codex_workspaces::types::{
    CodexLocalProjectUsageSnapshot, CostEstimate, ProjectUsage, SessionUsage, SourceStatus,
    UsageTotals,
};

const SCHEMA: &str = "CREATE TABLE projects (id TEXT PRIMARY KEY, name TEXT);
     CREATE TABLE project_roots (project_id TEXT, path TEXT);";

/// Fresh `<home>/state_5.sqlite` with the given projects and roots.
fn write_database(home: &Path, projects: &[(&str, &str)], roots: &[(&str, &str)]) -> PathBuf {
    fs::create_dir_all(home).unwrap();
    let path = home.join("state_5.sqlite");
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(SCHEMA).unwrap();
    for (id, name) in projects {
        conn.execute("INSERT INTO projects VALUES (?1, ?2)", params![id, name])
            .unwrap();
    }
    for (id, root) in roots {
        conn.execute(
            "INSERT INTO project_roots VALUES (?1, ?2)",
            params![id, root],
        )
        .unwrap();
    }
    path
}

fn folder_label(path: &str) -> String {
    Path::new(path)
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned()
}

/// A project whose label is its folder name, as the indexer produces it.
fn project(id: &str, path: &str) -> ProjectUsage {
    ProjectUsage {
        id: id.into(),
        display_name: folder_label(path),
        path: Some(path.into()),
        totals: UsageTotals::from_parts(10, 0, 5),
        cost_estimate: CostEstimate::default(),
        session_count: 1,
        latest_activity: None,
        top_model: None,
        top_sessions: Vec::new(),
        is_projectless: false,
    }
}

fn snapshot(projects: Vec<ProjectUsage>) -> CodexLocalProjectUsageSnapshot {
    CodexLocalProjectUsageSnapshot {
        updated_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
        history_days: 30,
        scope_signature: "scope".into(),
        indexed_file_count: 1,
        skipped_file_count: 0,
        total: UsageTotals::from_parts(10, 0, 5),
        sessions: Vec::new(),
        projects,
        daily: Vec::new(),
        source_status: SourceStatus::Complete,
    }
}

fn label<'a>(snapshot: &'a CodexLocalProjectUsageSnapshot, id: &str) -> &'a str {
    &snapshot
        .projects
        .iter()
        .find(|p| p.id == id)
        .unwrap()
        .display_name
}

fn names(snapshot: &CodexLocalProjectUsageSnapshot) -> Vec<String> {
    snapshot
        .projects
        .iter()
        .map(|p| p.display_name.clone())
        .collect()
}

#[test]
fn nearest_root_wins_and_prefix_collisions_keep_the_folder_label() {
    let tmp = TempDir::new().unwrap();
    let base = tmp.path().to_string_lossy().into_owned();
    let home = tmp.path().join("codex");
    write_database(
        &home,
        &[("outer", "My Workspace"), ("inner", "Nested Project")],
        &[
            ("outer", &format!("{base}/work")),
            ("inner", &format!("{base}/work/nested")),
        ],
    );
    let mut snap = snapshot(vec![
        project("p1", &format!("{base}/work")),
        project("p2", &format!("{base}/work/sub")),
        project("p3", &format!("{base}/work/nested/src")),
        project("p4", &format!("{base}/worker")),
    ]);

    apply_saved_project_names(&mut snap, &home, None);

    assert_eq!(label(&snap, "p1"), "My Workspace");
    assert_eq!(label(&snap, "p2"), "My Workspace");
    assert_eq!(label(&snap, "p3"), "Nested Project");
    assert_eq!(label(&snap, "p4"), "worker");
}

#[test]
fn equal_length_owners_are_ambiguous_and_blank_names_are_ignored() {
    let tmp = TempDir::new().unwrap();
    let base = tmp.path().to_string_lossy().into_owned();
    let home = tmp.path().join("codex");
    write_database(
        &home,
        &[
            ("a", "Alpha"),
            ("b", "Beta"),
            ("c", "Gamma"),
            ("blank", "   "),
        ],
        &[
            // Two different projects claim the same root.
            ("a", &format!("{base}/shared")),
            ("b", &format!("{base}/shared")),
            // The nearest root is owned by two projects: no name, even though a
            // shorter root would have matched.
            ("a", &format!("{base}/x")),
            ("b", &format!("{base}/x/y")),
            ("c", &format!("{base}/x/y")),
            // A blank name is no name.
            ("blank", &format!("{base}/blank")),
        ],
    );
    let mut snap = snapshot(vec![
        project("p1", &format!("{base}/shared")),
        project("p2", &format!("{base}/x/y/z")),
        project("p3", &format!("{base}/blank")),
    ]);

    apply_saved_project_names(&mut snap, &home, None);

    assert_eq!(label(&snap, "p1"), "shared");
    assert_eq!(label(&snap, "p2"), "z");
    assert_eq!(label(&snap, "p3"), "blank");
}

#[test]
fn renames_are_read_on_every_apply_and_removed_names_do_not_linger() {
    let tmp = TempDir::new().unwrap();
    let base = tmp.path().to_string_lossy().into_owned();
    let home = tmp.path().join("codex");
    let database = write_database(
        &home,
        &[("outer", "Old Name")],
        &[("outer", &format!("{base}/work"))],
    );
    let workdir = format!("{base}/work");

    // A cached snapshot still carries the name saved when it was written.
    let mut cached = snapshot(vec![ProjectUsage {
        display_name: "Old Name".into(),
        ..project("p1", &workdir)
    }]);
    apply_saved_project_names(&mut cached, &home, None);
    assert_eq!(names(&cached), vec!["Old Name"]);

    let conn = Connection::open(&database).unwrap();
    conn.execute(
        "UPDATE projects SET name = 'New Name' WHERE id = 'outer'",
        [],
    )
    .unwrap();
    drop(conn);
    let mut renamed = snapshot(vec![project("p1", &workdir)]);
    apply_saved_project_names(&mut renamed, &home, None);
    assert_eq!(names(&renamed), vec!["New Name"]);

    // The name was removed in Codex: the cached snapshot must fall back to the folder.
    let conn = Connection::open(&database).unwrap();
    conn.execute("DELETE FROM project_roots", []).unwrap();
    drop(conn);
    let mut removed = snapshot(vec![ProjectUsage {
        display_name: "New Name".into(),
        ..project("p1", &workdir)
    }]);
    apply_saved_project_names(&mut removed, &home, None);
    assert_eq!(names(&removed), vec!["work"]);
}

#[test]
fn overlay_changes_only_labels_and_keeps_totals_sessions_and_order() {
    let tmp = TempDir::new().unwrap();
    let base = tmp.path().to_string_lossy().into_owned();
    let home = tmp.path().join("codex");
    write_database(
        &home,
        &[("first", "Zeta Label")],
        &[("first", &format!("{base}/alpha"))],
    );
    let session = SessionUsage {
        id: "s1".into(),
        project_id: "p2".into(),
        display_title: "Session s1".into(),
        cwd: Some(format!("{base}/beta")),
        started_at: None,
        latest_activity: None,
        totals: UsageTotals::from_parts(3, 0, 1),
        cost_estimate: CostEstimate::default(),
        top_model: None,
        turn_performance: None,
    };
    let mut snap = snapshot(vec![
        project("p1", &format!("{base}/alpha")),
        project("p2", &format!("{base}/beta")),
    ]);
    snap.sessions = vec![session.clone()];
    let before = snap.clone();

    apply_saved_project_names(&mut snap, &home, None);

    assert_eq!(snap.projects[0].display_name, "Zeta Label");
    assert_eq!(snap.projects[1].display_name, "beta");
    let ids: Vec<&str> = snap.projects.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(ids, vec!["p1", "p2"]);
    assert_eq!(snap.total, before.total);
    assert_eq!(snap.sessions, before.sessions);
    assert_eq!(snap.projects[0].totals, before.projects[0].totals);
    assert_eq!(snap.projects[1].path, before.projects[1].path);
    assert_eq!(
        snap.projects[1].cost_estimate,
        before.projects[1].cost_estimate
    );
}

#[test]
fn saved_names_are_limited_to_1024_root_rows() {
    let tmp = TempDir::new().unwrap();
    let base = tmp.path().to_string_lossy().into_owned();
    let work = format!("{base}/work");

    // 1024 rows in total (the target root plus 1023 fillers): read.
    let within = tmp.path().join("within");
    let mut roots: Vec<(String, String)> = vec![("outer".into(), work.clone())];
    roots.extend((0..1023).map(|i| ("filler".to_string(), format!("{base}/filler/{i}"))));
    let root_refs: Vec<(&str, &str)> = roots
        .iter()
        .map(|(i, p)| (i.as_str(), p.as_str()))
        .collect();
    write_database(
        &within,
        &[("outer", "Within Limit"), ("filler", "Filler")],
        &root_refs,
    );
    let mut snap = snapshot(vec![project("p1", &work)]);
    apply_saved_project_names(&mut snap, &within, None);
    assert_eq!(names(&snap), vec!["Within Limit"]);

    // 1025 rows: the overlay is skipped entirely and the folder label stays.
    let over = tmp.path().join("over");
    roots.push(("filler".into(), format!("{base}/filler/extra")));
    let root_refs: Vec<(&str, &str)> = roots
        .iter()
        .map(|(i, p)| (i.as_str(), p.as_str()))
        .collect();
    write_database(
        &over,
        &[("outer", "Over Limit"), ("filler", "Filler")],
        &root_refs,
    );
    let mut snap = snapshot(vec![project("p1", &work)]);
    apply_saved_project_names(&mut snap, &over, None);
    assert_eq!(names(&snap), vec!["work"]);
}

#[test]
fn missing_legacy_or_corrupt_databases_keep_labels_and_create_nothing() {
    let tmp = TempDir::new().unwrap();
    let base = tmp.path().to_string_lossy().into_owned();
    let work = format!("{base}/work");

    let missing = tmp.path().join("missing");
    let mut snap = snapshot(vec![project("p1", &work)]);
    apply_saved_project_names(&mut snap, &missing, None);
    assert_eq!(names(&snap), vec!["work"]);
    assert!(!missing.join("state_5.sqlite").exists());
    assert!(!missing.exists());

    let legacy = tmp.path().join("legacy");
    fs::create_dir_all(&legacy).unwrap();
    Connection::open(legacy.join("state_5.sqlite"))
        .unwrap()
        .execute_batch("CREATE TABLE threads (id TEXT, title TEXT);")
        .unwrap();
    let mut snap = snapshot(vec![project("p1", &work)]);
    apply_saved_project_names(&mut snap, &legacy, None);
    assert_eq!(names(&snap), vec!["work"]);

    let corrupt = tmp.path().join("corrupt");
    fs::create_dir_all(&corrupt).unwrap();
    fs::write(corrupt.join("state_5.sqlite"), b"not a database at all").unwrap();
    let mut snap = snapshot(vec![project("p1", &work)]);
    apply_saved_project_names(&mut snap, &corrupt, None);
    assert_eq!(names(&snap), vec!["work"]);
}

#[test]
fn chats_and_relative_paths_are_left_alone() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path().join("codex");
    write_database(
        &home,
        &[("p", "Saved")],
        &[("p", &format!("{}/work", tmp.path().display()))],
    );
    let chats = ProjectUsage {
        id: "chats".into(),
        display_name: "Chats".into(),
        path: None,
        ..project("chats", "unused")
    };
    let relative = ProjectUsage {
        display_name: "rel".into(),
        ..project("rel", "relative/work")
    };
    let mut snap = snapshot(vec![chats, relative]);

    apply_saved_project_names(&mut snap, &home, None);

    assert_eq!(names(&snap), vec!["Chats", "rel"]);
}
