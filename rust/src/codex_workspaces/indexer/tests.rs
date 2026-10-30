use super::*;
use crate::codex_workspaces::types::SourceStatus;
use crate::cost_reporting_period::cost_bucket_zone;
use rusqlite::Connection;
use std::fs::{File, OpenOptions};
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
    let now = Utc::now();
    let day = cost_bucket_zone().date(now).format("%Y-%m-%d").to_string();

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

    let snap = index.load_snapshot_at(true, |_| {}, now).expect("snapshot");
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
    let now = Utc::now();
    let day = cost_bucket_zone().date(now).format("%Y-%m-%d").to_string();
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
        .load_snapshot_at(true, |_| {}, now)
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
    let now = Utc::now();
    let day = cost_bucket_zone().date(now).format("%Y-%m-%d").to_string();
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
    let first_snapshot = first.load_snapshot_at(true, |_| {}, now).unwrap();
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

fn append_tool_operation(path: &Path, owner: &str, item_id: &str, timestamp: &str) {
    let mut file = fs::OpenOptions::new().append(true).open(path).unwrap();
    writeln!(
        file,
        r#"{{"timestamp":"{timestamp}","type":"event_msg","payload":{{"type":"item_completed","thread_id":"{owner}","turn_id":"turn","item":{{"type":"CommandExecution","id":"{item_id}","command":["cargo","test"],"exit_code":0,"duration":{{"secs":2,"nanos":0}}}}}}}}"#
    )
    .unwrap();
}

#[test]
fn session_tool_activity_reads_only_that_sessions_rollouts_in_the_window() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path().join("codex");
    let sessions = home.join("sessions");
    let now = Utc::now();
    let day = cost_bucket_zone().date(now).format("%Y-%m-%d").to_string();
    let cwd = tmp.path().join("proj").to_string_lossy().into_owned();
    write_session(&sessions, &day, "sess-tools.jsonl", &cwd, "gpt-5", 10, 5);
    write_session(&sessions, &day, "sess-other.jsonl", &cwd, "gpt-5", 10, 5);
    let folder = day
        .split('-')
        .fold(sessions.clone(), |path, part| path.join(part));
    let recent = (now - chrono::Duration::minutes(1)).to_rfc3339();
    let stale = (now - chrono::Duration::days(400)).to_rfc3339();
    let tools = folder.join("sess-tools.jsonl");
    append_tool_operation(&tools, "sess-tools", "recent", &recent);
    append_tool_operation(&tools, "sess-tools", "stale", &stale);
    append_tool_operation(&tools, "sess-other", "copied", &recent);
    append_tool_operation(
        &folder.join("sess-other.jsonl"),
        "sess-other",
        "other",
        &recent,
    );

    let index = CodexWorkspacesIndex::new(30)
        .with_codex_home(&home)
        .with_sidecar_path(tmp.path().join("side.sqlite"));
    let activity = index
        .load_session_tool_activity_at("sess-tools", now)
        .unwrap();

    let ids: Vec<_> = activity
        .operations
        .iter()
        .map(|operation| operation.item_id.as_str())
        .collect();
    assert_eq!(ids, ["recent"]);
    assert_eq!(activity.operations[0].duration_milliseconds, Some(2000.0));
    assert!(!activity.is_partial);
    assert!(
        !tmp.path().join("side.sqlite").exists(),
        "tool activity never writes the sidecar"
    );
}

#[test]
fn session_activity_uses_token_event_times() {
    let tmp = TempDir::new().unwrap();
    let sessions = tmp.path().join("sessions");
    write_session(
        &sessions,
        "2026-03-04",
        "sess-times.jsonl",
        &tmp.path().to_string_lossy(),
        "gpt-5",
        10,
        5,
    );
    let path = sessions.join("2026/03/04/sess-times.jsonl");
    let mut file = OpenOptions::new().append(true).open(&path).unwrap();
    writeln!(
        file,
        r#"{{"timestamp":"2026-03-04T10:00:03.000Z","type":"event_msg","payload":{{"type":"token_count","info":{{"last_token_usage":{{"input_tokens":10,"cached_input_tokens":0,"output_tokens":5}}}}}}}}"#
    )
    .unwrap();
    drop(file);
    let range = CostUsageDayRange::new(
        NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
        NaiveDate::from_ymd_opt(2026, 3, 10).unwrap(),
    );
    let parsed = index_one_file(&path, &range).expect("parsed");
    let first = Utc.with_ymd_and_hms(2026, 3, 4, 10, 0, 2).unwrap();
    let last = Utc.with_ymd_and_hms(2026, 3, 4, 10, 0, 3).unwrap();
    assert_eq!(parsed.started_at, Some(first));
    assert_eq!(parsed.latest_activity, Some(last));
}

#[test]
fn day_key_activity_never_ends_after_the_file_was_written() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("rollout.jsonl");
    File::create(&path).unwrap();
    let modified = DateTime::<Utc>::from(fs::metadata(&path).unwrap().modified().unwrap());
    let today = modified.date_naive().format("%Y-%m-%d").to_string();
    let day_models = HashMap::from([(today, HashMap::new())]);

    let (started, latest) = file_activity_bounds(&path, &day_models, [None, None]);
    assert_eq!(latest, Some(modified));
    assert!(started.unwrap() <= modified);
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
            turn_performance: None,
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
                turn_performance: None,
            }],
            is_projectless: false,
        }],
        daily: vec![],
        source_status: SourceStatus::Complete,
    };
    snap.redact_for_privacy();
    assert_eq!(snap.sessions[0].display_title, "Session s1");
    assert!(snap.sessions[0].cwd.is_none());
    assert_eq!(snap.projects[0].display_name, "Workspace #1");
    assert!(snap.projects[0].path.is_none());
    assert_eq!(snap.projects[0].top_sessions[0].display_title, "Session s1");
    assert!(snap.projects[0].top_sessions[0].cwd.is_none());
}
