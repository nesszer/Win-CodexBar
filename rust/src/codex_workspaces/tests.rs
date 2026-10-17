//! End-to-end checks for thread names and session ranking in the workspaces snapshot.

mod freshness;

use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

use chrono::Local;
use rusqlite::Connection;
use tempfile::TempDir;

use super::{CodexLocalProjectUsageSnapshot, CodexWorkspacesIndex, short_session_id};

fn write_session(home: &Path, id: &str, cwd: &Path, input: i32, output: i32) {
    let day = Local::now().date_naive().format("%Y-%m-%d").to_string();
    let parts: Vec<_> = day.split('-').collect();
    let folder = home
        .join("sessions")
        .join(parts[0])
        .join(parts[1])
        .join(parts[2]);
    fs::create_dir_all(&folder).unwrap();
    let mut file = File::create(folder.join(format!("{id}.jsonl"))).unwrap();
    writeln!(
        file,
        r#"{{"timestamp":"{day}T10:00:00.000Z","type":"session_meta","payload":{{"session_id":"{id}","cwd":"{cwd}","originator":"codex_exec","source":"cli"}}}}"#,
        cwd = cwd.to_string_lossy().replace('\\', "\\\\"),
    )
    .unwrap();
    writeln!(
        file,
        r#"{{"timestamp":"{day}T10:00:01.000Z","type":"turn_context","payload":{{"model":"gpt-5"}}}}"#
    )
    .unwrap();
    writeln!(
        file,
        r#"{{"timestamp":"{day}T10:00:02.000Z","type":"event_msg","payload":{{"type":"token_count","info":{{"last_token_usage":{{"input_tokens":{input},"cached_input_tokens":0,"output_tokens":{output}}}}}}}}}"#
    )
    .unwrap();
}

/// Three sessions in one project; `cheap` < `middle` < `dear` by cost.
fn build_home(root: &Path, named: bool) -> CodexLocalProjectUsageSnapshot {
    let home = root.join("codex");
    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    let short = "short-id";
    let long = "019f79b9-1790-7921-8d6f-258a1e92b191";
    write_session(&home, short, &project, 1_000, 100);
    write_session(&home, "middle-session", &project, 20_000, 1_000);
    write_session(&home, long, &project, 900_000, 90_000);

    if named {
        // Pin the SQLite home so a developer's CODEX_SQLITE_HOME cannot interfere.
        fs::write(
            home.join("config.toml"),
            format!(
                "sqlite_home = '{}'\n",
                home.canonicalize().unwrap().display()
            ),
        )
        .unwrap();
        fs::write(
            home.join("session_index.jsonl"),
            "{\"id\":\"middle-session\",\"thread_name\":\"Fix the icon\"}\n",
        )
        .unwrap();
        let conn = Connection::open(home.join("state_5.sqlite")).unwrap();
        conn.execute_batch(
            "CREATE TABLE threads (id TEXT PRIMARY KEY, title TEXT);
             INSERT INTO threads (id, title) VALUES ('short-id', 'Database title');
             INSERT INTO threads (id, title) VALUES ('middle-session', 'Ignored, index wins');",
        )
        .unwrap();
    }

    CodexWorkspacesIndex::new(30)
        .with_codex_home(&home)
        .with_sidecar_path(root.join("sidecar.sqlite"))
        .load_snapshot(true, |_| {})
        .expect("snapshot")
}

#[test]
fn sessions_get_thread_names_and_rank_by_cost() {
    let tmp = TempDir::new().unwrap();
    let snapshot = build_home(tmp.path(), true);

    let titles: Vec<_> = snapshot
        .sessions
        .iter()
        .map(|s| s.display_title.as_str())
        .collect();
    // Most expensive first; an untitled session shows its short id.
    assert_eq!(
        titles,
        ["Session 019f...1e92b191", "Fix the icon", "Database title"]
    );
    let top: Vec<_> = snapshot.projects[0]
        .top_sessions
        .iter()
        .map(|s| s.display_title.as_str())
        .collect();
    assert_eq!(top, titles);
}

#[test]
fn untitled_sessions_show_the_short_id() {
    let tmp = TempDir::new().unwrap();
    let snapshot = build_home(tmp.path(), false);
    let titles: Vec<_> = snapshot
        .sessions
        .iter()
        .map(|s| s.display_title.clone())
        .collect();
    assert_eq!(
        titles,
        [
            format!(
                "Session {}",
                short_session_id("019f79b9-1790-7921-8d6f-258a1e92b191")
            ),
            "Session midd...-session".to_string(),
            "Session short-id".to_string(),
        ]
    );
}

#[test]
fn naming_and_ranking_do_not_change_totals_or_costs() {
    let plain = tempfile::tempdir().unwrap();
    let named = tempfile::tempdir().unwrap();
    let plain = build_home(plain.path(), false);
    let named = build_home(named.path(), true);

    assert_eq!(plain.total, named.total);
    assert_eq!(plain.daily, named.daily);
    assert_eq!(plain.projects.len(), named.projects.len());
    assert_eq!(
        plain.projects[0].cost_estimate,
        named.projects[0].cost_estimate
    );
    let costs = |snapshot: &CodexLocalProjectUsageSnapshot| {
        snapshot
            .sessions
            .iter()
            .map(|s| (s.id.clone(), s.cost_estimate, s.totals))
            .collect::<Vec<_>>()
    };
    assert_eq!(costs(&plain), costs(&named));
}

#[test]
fn hide_personal_info_masks_names_with_the_short_id() {
    let tmp = TempDir::new().unwrap();
    let mut snapshot = build_home(tmp.path(), true);
    snapshot.redact_for_privacy();

    let titles: Vec<_> = snapshot
        .sessions
        .iter()
        .map(|s| s.display_title.as_str())
        .collect();
    assert_eq!(
        titles,
        [
            "Session 019f...1e92b191",
            "Session midd...-session",
            "Session short-id"
        ]
    );
    assert!(snapshot.sessions.iter().all(|s| s.cwd.is_none()));
    assert!(snapshot.projects.iter().all(|p| p.path.is_none()));
    assert!(
        snapshot.projects[0]
            .top_sessions
            .iter()
            .all(|s| s.display_title.starts_with("Session "))
    );
}

#[test]
fn independent_chats_follow_the_desktop_marker_on_fresh_and_cached_loads() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path().join("codex");
    let folder = tmp.path().join("chat-folder");
    fs::create_dir_all(&folder).unwrap();
    write_session(&home, "chat-1", &folder, 1_000, 100);
    let state = home.join(".codex-global-state.json");
    fs::write(&state, r#"{"projectless-thread-ids":["chat-1"]}"#).unwrap();
    {
        let conn = Connection::open(home.join("state_5.sqlite")).unwrap();
        conn.execute_batch(
            "CREATE TABLE threads (id TEXT PRIMARY KEY, title TEXT, project_id TEXT);
             INSERT INTO threads (id, title) VALUES ('chat-1', 'Repair the local service');",
        )
        .unwrap();
    }
    let index = CodexWorkspacesIndex::new(30)
        .with_codex_home(&home)
        .with_sidecar_path(tmp.path().join("sidecar.sqlite"));

    let fresh = index.load_snapshot(true, |_| {}).unwrap();
    assert_eq!(fresh.projects.len(), 1);
    assert!(fresh.projects[0].is_projectless);
    assert_eq!(fresh.projects[0].display_name, "Repair the local service");
    assert_eq!(fresh.total.input_tokens, 1_000);

    // Cached loads re-read the marker and the title.
    let cached = index.load_snapshot(false, |_| {}).unwrap();
    assert!(cached.projects[0].is_projectless);
    assert_eq!(cached.projects[0].display_name, "Repair the local service");

    fs::write(&state, r#"{"projectless-thread-ids":[]}"#).unwrap();
    let unmarked = index.load_snapshot(false, |_| {}).unwrap();
    assert!(!unmarked.projects[0].is_projectless);
    assert_eq!(unmarked.projects[0].display_name, "chat-folder");
    assert_eq!(unmarked.total, fresh.total);
}
