use super::*;
use crate::codex_workspaces::types::SourceStatus;
use chrono::Local;
use rusqlite::Connection;
use std::fs::File;
use std::io::Write;
use tempfile::TempDir;

fn write_session(
    dir: &Path,
    day: &str,
    name: &str,
    cwd: &str,
    model: &str,
    input: i32,
    output: i32,
) {
    // day = YYYY-MM-DD
    let parts: Vec<_> = day.split('-').collect();
    let folder = dir.join(parts[0]).join(parts[1]).join(parts[2]);
    fs::create_dir_all(&folder).unwrap();
    let path = folder.join(name);
    let mut f = File::create(&path).unwrap();
    writeln!(
            f,
            r#"{{"timestamp":"{day}T10:00:00.000Z","type":"session_meta","payload":{{"session_id":"{id}","cwd":"{cwd}","originator":"codex_exec","source":"cli"}}}}"#,
            id = name.trim_end_matches(".jsonl"),
            cwd = cwd.replace('\\', "\\\\"),
        )
        .unwrap();
    writeln!(
            f,
            r#"{{"timestamp":"{day}T10:00:01.000Z","type":"turn_context","payload":{{"model":"{model}"}}}}"#
        )
        .unwrap();
    writeln!(
            f,
            r#"{{"timestamp":"{day}T10:00:02.000Z","type":"event_msg","payload":{{"type":"token_count","info":{{"last_token_usage":{{"input_tokens":{input},"cached_input_tokens":0,"output_tokens":{output}}}}}}}}}"#
        )
        .unwrap();
}

#[test]
fn indexes_two_sessions_into_projects_and_daily() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path().join("codex");
    let sessions = home.join("sessions");
    fs::create_dir_all(&sessions).unwrap();
    let day = Local::now().date_naive().format("%Y-%m-%d").to_string();

    write_session(
        &sessions,
        &day,
        "sess-aaa.jsonl",
        &tmp.path().join("proj-a").to_string_lossy(),
        "gpt-5",
        1000,
        500,
    );
    write_session(
        &sessions,
        &day,
        "sess-bbb.jsonl",
        &tmp.path().join("proj-b").to_string_lossy(),
        "gpt-5",
        2000,
        100,
    );

    // Ensure project roots exist so resolve doesn't collapse oddly.
    fs::create_dir_all(tmp.path().join("proj-a")).unwrap();
    fs::create_dir_all(tmp.path().join("proj-b")).unwrap();

    let sidecar = tmp.path().join("sidecar.sqlite");
    let index = CodexWorkspacesIndex::new(30)
        .with_codex_home(&home)
        .with_sidecar_path(&sidecar);

    let snap = index.load_snapshot(true, |_| {}).expect("snapshot");
    assert_eq!(snap.indexed_file_count, 2);
    assert_eq!(snap.projects.len(), 2);
    assert!(!snap.daily.is_empty());
    assert_eq!(snap.source_status, SourceStatus::CatalogMissing);
    let total_sessions: u32 = snap.projects.iter().map(|p| p.session_count).sum();
    assert_eq!(total_sessions, 2);
    assert!(snap.total.input_tokens >= 3000);
    assert!(snap.total.output_tokens >= 600);

    // Cached load works.
    let cached = index.load_cached_snapshot().unwrap().expect("cached");
    assert_eq!(cached.indexed_file_count, 2);
}

#[test]
fn archived_and_flat_rollouts_are_indexed_once() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path().join("codex");
    let sessions = home.join("sessions");
    let archived = home.join("archived_sessions");
    fs::create_dir_all(&archived).unwrap();
    let day = Local::now().date_naive().format("%Y-%m-%d").to_string();
    let parts: Vec<&str> = day.split('-').collect();
    let dated_dir = sessions.join(parts[0]).join(parts[1]).join(parts[2]);
    let project = tmp.path().join("proj");
    fs::create_dir_all(&project).unwrap();
    for (name, input, output) in [
        ("sess-dated.jsonl", 1000, 10),
        ("sess-archived.jsonl", 200, 20),
        ("sess-legacy.jsonl", 30, 3),
    ] {
        write_session(
            &sessions,
            &day,
            name,
            &project.to_string_lossy(),
            "gpt-5",
            input,
            output,
        );
    }
    // Codex archives a thread by moving its rollout into the flat
    // archive; older builds kept rollouts flat in the sessions root.
    fs::rename(
        dated_dir.join("sess-archived.jsonl"),
        archived.join("sess-archived.jsonl"),
    )
    .unwrap();
    fs::rename(
        dated_dir.join("sess-legacy.jsonl"),
        sessions.join("sess-legacy.jsonl"),
    )
    .unwrap();
    // An archived copy of a dated rollout is not counted twice.
    fs::copy(
        dated_dir.join("sess-dated.jsonl"),
        archived.join("sess-dated.jsonl"),
    )
    .unwrap();

    let snap = CodexWorkspacesIndex::new(30)
        .with_codex_home(&home)
        .with_sidecar_path(tmp.path().join("sidecar.sqlite"))
        .load_snapshot(true, |_| {})
        .expect("snapshot");

    assert_eq!(snap.indexed_file_count, 3);
    assert_eq!(snap.total.input_tokens, 1230);
    assert_eq!(snap.total.output_tokens, 33);
    let sessions_indexed: u32 = snap.projects.iter().map(|p| p.session_count).sum();
    assert_eq!(sessions_indexed, 3);
}

#[test]
fn cached_snapshot_is_not_reused_for_another_codex_home() {
    let tmp = TempDir::new().unwrap();
    let first_home = tmp.path().join("first-codex");
    let first_sessions = first_home.join("sessions");
    fs::create_dir_all(&first_sessions).unwrap();
    let day = Local::now().date_naive().format("%Y-%m-%d").to_string();
    write_session(
        &first_sessions,
        &day,
        "first-session.jsonl",
        &tmp.path().join("first-project").to_string_lossy(),
        "gpt-5",
        100,
        20,
    );

    let sidecar = tmp.path().join("sidecar.sqlite");
    let first = CodexWorkspacesIndex::new(30)
        .with_codex_home(&first_home)
        .with_sidecar_path(&sidecar);
    let first_snapshot = first.load_snapshot(true, |_| {}).unwrap();
    assert_eq!(first_snapshot.indexed_file_count, 1);

    let second_home = tmp.path().join("second-codex");
    fs::create_dir_all(second_home.join("sessions")).unwrap();
    let second = CodexWorkspacesIndex::new(30)
        .with_codex_home(&second_home)
        .with_sidecar_path(&sidecar);

    assert!(second.load_cached_snapshot().unwrap().is_none());
    let second_snapshot = second.load_snapshot(false, |_| {}).unwrap();
    assert_eq!(second_snapshot.indexed_file_count, 0);
    assert!(second_snapshot.projects.is_empty());
    assert_ne!(
        first_snapshot.scope_signature,
        second_snapshot.scope_signature
    );
}

#[test]
fn daily_costs_use_each_day_rates() {
    let sol = |day: &str| {
        let models = HashMap::from([("gpt-5.6-sol".to_string(), (100, 10, 5))]);
        let mut daily = HashMap::new();
        merge_daily(&mut daily, &HashMap::from([(day.to_string(), models)]));
        daily[day].known_usd
    };
    let historical = 90.0 * 5e-6 + 10.0 * 5e-7 + 5.0 * 3e-5;
    let current = 90.0 * 4e-6 + 10.0 * 4e-7 + 5.0 * 2e-5;
    assert!((sol("2026-08-20") - historical).abs() < 1e-12);
    assert!((sol("2026-08-21") - current).abs() < 1e-12);
}

#[test]
fn foreign_user_version_is_rejected() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("bad.sqlite");
    {
        let conn = Connection::open(&path).unwrap();
        conn.pragma_update(None, "user_version", 99).unwrap();
    }
    let home = tmp.path().join("codex");
    fs::create_dir_all(home.join("sessions")).unwrap();
    let index = CodexWorkspacesIndex::new(7)
        .with_codex_home(home)
        .with_sidecar_path(path);
    let err = index.load_snapshot(true, |_| {}).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("incompatible") || msg.contains("user_version"),
        "unexpected error: {msg}"
    );
}

#[test]
fn missing_catalog_reports_catalog_missing() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path().join("codex");
    fs::create_dir_all(home.join("sessions")).unwrap();
    let index = CodexWorkspacesIndex::new(7)
        .with_codex_home(&home)
        .with_sidecar_path(tmp.path().join("side.sqlite"));
    let snap = index.load_snapshot(true, |_| {}).unwrap();
    assert_eq!(snap.source_status, SourceStatus::CatalogMissing);
}

#[test]
fn privacy_redaction_strips_paths_and_titles() {
    let mut snap = CodexLocalProjectUsageSnapshot {
        updated_at: Utc::now(),
        history_days: 30,
        scope_signature: "x".into(),
        indexed_file_count: 1,
        skipped_file_count: 0,
        total: UsageTotals::from_parts(10, 0, 5),
        sessions: vec![SessionUsage {
            id: "s1".into(),
            project_id: "project-abc".into(),
            display_title: "do not leak".into(),
            cwd: Some("/Users/me/secret-repo".into()),
            started_at: None,
            latest_activity: None,
            totals: UsageTotals::from_parts(10, 0, 5),
            cost_estimate: CostEstimate::default(),
            top_model: Some("gpt-5".into()),
        }],
        projects: vec![ProjectUsage {
            id: "project-abc".into(),
            display_name: "secret-repo".into(),
            path: Some("/Users/me/secret-repo".into()),
            totals: UsageTotals::from_parts(10, 0, 5),
            cost_estimate: CostEstimate::default(),
            session_count: 1,
            latest_activity: None,
            top_model: Some("gpt-5".into()),
            top_sessions: vec![SessionUsage {
                id: "s1".into(),
                project_id: "project-abc".into(),
                display_title: "do not leak".into(),
                cwd: Some("/Users/me/secret-repo".into()),
                started_at: None,
                latest_activity: None,
                totals: UsageTotals::from_parts(10, 0, 5),
                cost_estimate: CostEstimate::default(),
                top_model: Some("gpt-5".into()),
            }],
        }],
        daily: vec![],
        source_status: SourceStatus::Complete,
    };
    snap.redact_for_privacy();
    assert_eq!(snap.projects[0].display_name, "Workspace");
    assert!(snap.projects[0].path.is_none());
    assert_eq!(snap.projects[0].top_sessions[0].display_title, "Session s1");
    assert!(snap.projects[0].top_sessions[0].cwd.is_none());
}
