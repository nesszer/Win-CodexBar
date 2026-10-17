use std::fs;
use std::path::Path;

use chrono::{TimeZone, Utc};
use rusqlite::Connection;
use serde_json::json;
use tempfile::TempDir;

use super::apply_projectless_chats;
use crate::codex_workspaces::types::{
    CodexLocalProjectUsageSnapshot, CostEstimate, ProjectUsage, SessionUsage, SourceStatus,
    UsageTotals, untitled_session_label,
};

const THREADS: &str = "CREATE TABLE threads (id TEXT PRIMARY KEY, title TEXT, project_id TEXT);";

fn write_state(home: &Path, state: &serde_json::Value) {
    fs::create_dir_all(home).unwrap();
    fs::write(home.join(".codex-global-state.json"), state.to_string()).unwrap();
}

fn write_db(home: &Path, sql: &str) {
    fs::create_dir_all(home).unwrap();
    let conn = Connection::open(home.join("state_5.sqlite")).unwrap();
    conn.execute_batch(sql).unwrap();
}

fn temp_root(tmp: &TempDir) -> String {
    tmp.path().to_string_lossy().into_owned()
}

fn folder_label(path: &str) -> String {
    Path::new(path)
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned()
}

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

fn session(id: &str, project_id: &str, title: &str) -> SessionUsage {
    SessionUsage {
        id: id.into(),
        project_id: project_id.into(),
        display_title: title.into(),
        cwd: None,
        started_at: None,
        latest_activity: None,
        totals: UsageTotals::from_parts(10, 0, 5),
        cost_estimate: CostEstimate::default(),
        top_model: None,
        turn_performance: None,
    }
}

fn snapshot(
    projects: Vec<ProjectUsage>,
    sessions: Vec<SessionUsage>,
) -> CodexLocalProjectUsageSnapshot {
    CodexLocalProjectUsageSnapshot {
        updated_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
        history_days: 30,
        scope_signature: "scope".into(),
        indexed_file_count: 1,
        skipped_file_count: 0,
        total: UsageTotals::from_parts(10, 0, 5),
        sessions,
        projects,
        daily: Vec::new(),
        source_status: SourceStatus::Complete,
    }
}

fn find<'a>(snap: &'a CodexLocalProjectUsageSnapshot, id: &str) -> &'a ProjectUsage {
    snap.projects.iter().find(|p| p.id == id).unwrap()
}

fn is_chat(snap: &CodexLocalProjectUsageSnapshot, id: &str) -> bool {
    find(snap, id).is_projectless
}

#[test]
fn explicit_desktop_chat_takes_its_title_and_other_folders_stay_projects() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path().join("codex");
    let base = temp_root(&tmp);
    write_state(&home, &json!({"projectless-thread-ids": ["chat"]}));
    write_db(
        &home,
        &format!(
            "{THREADS} INSERT INTO threads VALUES ('chat', 'Repair the local service', NULL), \
             ('cli', 'A code project', NULL);"
        ),
    );
    let mut snap = snapshot(
        vec![
            project("p-chat", &format!("{base}/chat")),
            project("p-cli", &format!("{base}/cli")),
        ],
        vec![
            session("chat", "p-chat", "Repair the local service"),
            session("cli", "p-cli", "A code project"),
        ],
    );

    apply_projectless_chats(&mut snap, &home, None);

    assert!(is_chat(&snap, "p-chat"));
    assert_eq!(
        find(&snap, "p-chat").display_name,
        "Repair the local service"
    );
    assert!(!is_chat(&snap, "p-cli"));
    assert_eq!(find(&snap, "p-cli").display_name, "cli");
    // Accounting is untouched.
    assert_eq!(
        find(&snap, "p-chat").totals,
        UsageTotals::from_parts(10, 0, 5)
    );
}

#[test]
fn missing_or_malformed_state_and_unmarked_chats_classify_nothing() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path().join("codex");
    let base = temp_root(&tmp);
    write_db(&home, THREADS);
    let build = || {
        snapshot(
            vec![project("p", &format!("{base}/chat"))],
            vec![session("chat", "p", "Titled")],
        )
    };

    let mut missing = build();
    apply_projectless_chats(&mut missing, &home, None);
    assert!(!is_chat(&missing, "p"));

    fs::write(home.join(".codex-global-state.json"), "{").unwrap();
    let mut malformed = build();
    apply_projectless_chats(&mut malformed, &home, None);
    assert!(!is_chat(&malformed, "p"));

    let oversized = vec![b' '; 8 * 1024 * 1024 + 1];
    fs::write(home.join(".codex-global-state.json"), oversized).unwrap();
    let mut too_big = build();
    apply_projectless_chats(&mut too_big, &home, None);
    assert!(!is_chat(&too_big, "p"));

    write_state(&home, &json!({"projectless-thread-ids": ["someone-else"]}));
    let mut unmarked = build();
    apply_projectless_chats(&mut unmarked, &home, None);
    assert!(!is_chat(&unmarked, "p"));
}

#[test]
fn global_assignments_veto_only_the_assigned_chat() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path().join("codex");
    let base = temp_root(&tmp);
    write_state(
        &home,
        &json!({
            "projectless-thread-ids": ["chat", "assigned"],
            "thread-project-assignments": {"assigned": {"projectId": "real-project"}}
        }),
    );
    write_db(&home, THREADS);
    let mut snap = snapshot(
        vec![
            project("p-chat", &format!("{base}/chat")),
            project("p-assigned", &format!("{base}/assigned")),
        ],
        vec![
            session("chat", "p-chat", "Chat title"),
            session("assigned", "p-assigned", "Assigned title"),
        ],
    );

    apply_projectless_chats(&mut snap, &home, None);

    assert!(is_chat(&snap, "p-chat"));
    assert!(!is_chat(&snap, "p-assigned"));
    assert_eq!(find(&snap, "p-assigned").display_name, "assigned");
}

#[test]
fn legacy_thread_assignments_veto_any_non_null_project_id() {
    for (row, expected) in [
        ("('chat', 'T', NULL)", true),
        ("('chat', 'T', '')", false),
        ("('chat', 'T', '  ')", false),
        ("('chat', 'T', 'real-project')", false),
    ] {
        let tmp = TempDir::new().unwrap();
        let home = tmp.path().join("codex");
        let base = temp_root(&tmp);
        write_state(&home, &json!({"projectless-thread-ids": ["chat"]}));
        write_db(
            &home,
            &format!("{THREADS} INSERT INTO threads VALUES {row};"),
        );
        let mut snap = snapshot(
            vec![project("p", &format!("{base}/chat"))],
            vec![session("chat", "p", "T")],
        );
        apply_projectless_chats(&mut snap, &home, None);
        assert_eq!(is_chat(&snap, "p"), expected, "row {row}");
    }
}

#[test]
fn threads_without_a_project_column_do_not_veto() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path().join("codex");
    let base = temp_root(&tmp);
    write_state(&home, &json!({"projectless-thread-ids": ["chat"]}));
    write_db(
        &home,
        "CREATE TABLE threads (id TEXT, title TEXT, agent_path TEXT);",
    );
    let mut snap = snapshot(
        vec![project("p", &format!("{base}/chat"))],
        vec![session("chat", "p", "Legacy title")],
    );

    apply_projectless_chats(&mut snap, &home, None);

    assert!(is_chat(&snap, "p"));
    assert_eq!(find(&snap, "p").display_name, "Legacy title");
}

#[test]
fn registered_project_roots_veto_even_without_a_thread_assignment() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path().join("codex");
    let base = temp_root(&tmp);
    write_state(
        &home,
        &json!({"projectless-thread-ids": ["nested", "exact", "outside"]}),
    );
    write_db(
        &home,
        &format!(
            "{THREADS} INSERT INTO threads (id, title) VALUES ('nested', 'N'), ('exact', 'E'), \
             ('outside', 'O');
             CREATE TABLE projects (id TEXT, name TEXT);
             CREATE TABLE project_roots (project_id TEXT, path TEXT);
             INSERT INTO projects VALUES ('real', 'Real project');
             INSERT INTO project_roots VALUES ('real', '{base}/work');
             INSERT INTO project_roots VALUES ('real', '{base}/exact');"
        ),
    );
    let mut snap = snapshot(
        vec![
            project("p-nested", &format!("{base}/work/src")),
            project("p-exact", &format!("{base}/exact")),
            project("p-outside", &format!("{base}/workshop")),
        ],
        vec![
            session("nested", "p-nested", "N"),
            session("exact", "p-exact", "E"),
            session("outside", "p-outside", "O"),
        ],
    );

    apply_projectless_chats(&mut snap, &home, None);

    assert!(!is_chat(&snap, "p-nested"));
    assert!(!is_chat(&snap, "p-exact"));
    // A sibling folder that merely shares a name prefix is not owned.
    assert!(is_chat(&snap, "p-outside"));
}

#[test]
fn missing_database_or_unreadable_ownership_vetoes_every_candidate() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path().join("absent");
    let base = temp_root(&tmp);
    write_state(&home, &json!({"projectless-thread-ids": ["chat"]}));
    let build = || {
        snapshot(
            vec![project("p", &format!("{base}/chat"))],
            vec![session("chat", "p", "Title")],
        )
    };

    // No state database at all: the legacy assignment state is unknown.
    let mut absent = build();
    apply_projectless_chats(&mut absent, &home, None);
    assert!(!is_chat(&absent, "p"));

    let cases = [
        // Relative root path.
        "CREATE TABLE projects (id TEXT, name TEXT);
         CREATE TABLE project_roots (project_id TEXT, path TEXT);
         INSERT INTO project_roots VALUES ('x', 'relative/path');"
            .to_string(),
        // Root with no path.
        "CREATE TABLE projects (id TEXT, name TEXT);
         CREATE TABLE project_roots (project_id TEXT, path TEXT);
         INSERT INTO project_roots VALUES ('x', NULL);"
            .to_string(),
        // Only one of the two project tables exists.
        "CREATE TABLE projects (id TEXT, name TEXT);".to_string(),
        "CREATE TABLE project_roots (project_id TEXT, path TEXT);".to_string(),
        // More roots than the row budget.
        "CREATE TABLE projects (id TEXT, name TEXT);
         CREATE TABLE project_roots (project_id TEXT, path TEXT);
         WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM n WHERE x < 1025)
         INSERT INTO project_roots SELECT 'x', '/work/' || x FROM n;"
            .to_string(),
    ];
    for schema in cases {
        let tmp = TempDir::new().unwrap();
        let home = tmp.path().join("codex");
        let base = temp_root(&tmp);
        write_state(&home, &json!({"projectless-thread-ids": ["chat"]}));
        write_db(&home, &format!("{THREADS} {schema}"));
        let mut snap = snapshot(
            vec![project("p", &format!("{base}/chat"))],
            vec![session("chat", "p", "Title")],
        );
        apply_projectless_chats(&mut snap, &home, None);
        assert!(!is_chat(&snap, "p"), "schema: {schema}");
    }
}

#[test]
fn legacy_schema_without_project_tables_still_classifies() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path().join("codex");
    let base = temp_root(&tmp);
    write_state(&home, &json!({"projectless-thread-ids": ["chat"]}));
    write_db(&home, THREADS);
    let mut snap = snapshot(
        vec![project("p", &format!("{base}/chat"))],
        vec![session("chat", "p", "Title")],
    );

    apply_projectless_chats(&mut snap, &home, None);

    assert!(is_chat(&snap, "p"));
}

#[test]
fn thread_table_without_an_id_column_vetoes() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path().join("codex");
    let base = temp_root(&tmp);
    write_state(&home, &json!({"projectless-thread-ids": ["chat"]}));
    write_db(&home, "CREATE TABLE unrelated (x TEXT);");
    let mut snap = snapshot(
        vec![project("p", &format!("{base}/chat"))],
        vec![session("chat", "p", "Title")],
    );

    apply_projectless_chats(&mut snap, &home, None);

    assert!(!is_chat(&snap, "p"));
}

#[test]
fn candidate_budget_is_enforced_for_the_whole_database() {
    for count in [4096usize, 4097] {
        let tmp = TempDir::new().unwrap();
        let home = tmp.path().join("codex");
        let base = temp_root(&tmp);
        let ids: Vec<String> = (0..count).map(|i| format!("thread-{i}")).collect();
        write_state(&home, &json!({"projectless-thread-ids": ids}));
        write_db(&home, THREADS);
        let sessions = ids
            .iter()
            .map(|id| session(id, "p", "Bulk"))
            .collect::<Vec<_>>();
        let mut snap = snapshot(vec![project("p", &format!("{base}/bulk"))], sessions);

        apply_projectless_chats(&mut snap, &home, None);

        assert_eq!(is_chat(&snap, "p"), count <= 4096, "count {count}");
    }
}

#[test]
fn chat_names_use_a_capped_title_and_neutral_labels_otherwise() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path().join("codex");
    let base = temp_root(&tmp);
    write_state(
        &home,
        &json!({"projectless-thread-ids": ["long", "attachment", "untitled", "a", "b"]}),
    );
    write_db(&home, THREADS);
    let long_title = "x".repeat(200);
    let mut snap = snapshot(
        vec![
            project("p-long", &format!("{base}/long")),
            project("p-attachment", &format!("{base}/attachment")),
            project("p-untitled", &format!("{base}/untitled")),
            project("p-shared", &format!("{base}/shared")),
        ],
        vec![
            session("long", "p-long", &format!("  {long_title}  ")),
            session(
                "attachment",
                "p-attachment",
                "# Files mentioned by the user:\nnotes.md",
            ),
            session(
                "untitled",
                "p-untitled",
                &untitled_session_label("untitled"),
            ),
            session("a", "p-shared", "First"),
            session("b", "p-shared", "Second"),
        ],
    );

    apply_projectless_chats(&mut snap, &home, None);

    assert_eq!(find(&snap, "p-long").display_name, "x".repeat(128));
    assert_eq!(find(&snap, "p-attachment").display_name, "Independent chat");
    assert_eq!(find(&snap, "p-untitled").display_name, "Independent chat");
    assert_eq!(find(&snap, "p-shared").display_name, "Independent chats");
}

#[test]
fn stale_chat_markers_are_cleared_when_the_marker_is_gone() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path().join("codex");
    let base = temp_root(&tmp);
    write_state(&home, &json!({"projectless-thread-ids": []}));
    let mut cached = snapshot(
        vec![ProjectUsage {
            display_name: "Old chat".into(),
            is_projectless: true,
            ..project("p", &format!("{base}/chat"))
        }],
        vec![session("chat", "p", "Old chat")],
    );

    apply_projectless_chats(&mut cached, &home, None);

    assert!(!is_chat(&cached, "p"));
}

#[test]
fn privacy_redaction_numbers_chats_in_list_order_and_hides_paths() {
    let mut snap = snapshot(
        vec![
            ProjectUsage {
                display_name: "Private title".into(),
                is_projectless: true,
                ..project("c1", "/Users/me/chat-one")
            },
            project("p", "/Users/me/secret-repo"),
            ProjectUsage {
                display_name: "Second private title".into(),
                is_projectless: true,
                ..project("c2", "/Users/me/chat-two")
            },
        ],
        Vec::new(),
    );

    snap.redact_for_privacy();

    let names: Vec<_> = snap
        .projects
        .iter()
        .map(|p| p.display_name.as_str())
        .collect();
    assert_eq!(names, ["Chat 1", "Workspace #1", "Chat 2"]);
    assert!(snap.projects.iter().all(|p| p.path.is_none()));
}

#[test]
fn privacy_redaction_gives_same_named_workspaces_distinct_numbers_in_list_order() {
    let mut snap = snapshot(
        vec![
            project("p1", "/Users/me/work/app"),
            ProjectUsage {
                display_name: "Private title".into(),
                is_projectless: true,
                ..project("c1", "/Users/me/chat-one")
            },
            project("p2", "/Users/me/personal/app"),
            ProjectUsage {
                display_name: crate::codex_workspaces::CHATS_DISPLAY_NAME.into(),
                path: None,
                ..project(crate::codex_workspaces::CHATS_PROJECT_ID, "/unused")
            },
            project("p3", "/Users/me/secret-repo"),
        ],
        Vec::new(),
    );
    assert_eq!(
        find(&snap, "p1").display_name,
        find(&snap, "p2").display_name
    );

    snap.redact_for_privacy();

    let names: Vec<_> = snap
        .projects
        .iter()
        .map(|p| p.display_name.as_str())
        .collect();
    assert_eq!(
        names,
        [
            "Workspace #1",
            "Chat 1",
            "Workspace #2",
            "Chats",
            "Workspace #3"
        ]
    );
    assert!(snap.projects.iter().all(|p| p.path.is_none()));
}
