use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;

use chrono::Utc;
use tempfile::TempDir;

use super::*;
use crate::codex_workspaces::SessionUsage;
use crate::cost_reporting_period::cost_bucket_zone;

struct Fixture {
    _tmp: TempDir,
    home: PathBuf,
    sidecar: PathBuf,
    rollout: PathBuf,
    day: String,
}

fn meta(day: &str, id: &str, model: &str) -> String {
    format!(
        "{{\"timestamp\":\"{day}T10:00:00.000Z\",\"type\":\"session_meta\",\"payload\":{{\"session_id\":\"{id}\",\"originator\":\"codex_exec\",\"source\":\"cli\"}}}}\n\
         {{\"timestamp\":\"{day}T10:00:01.000Z\",\"type\":\"turn_context\",\"payload\":{{\"model\":\"{model}\"}}}}\n"
    )
}

fn turn(day: &str, second: u32, turn_id: &str) -> String {
    format!(
        "{{\"timestamp\":\"{day}T10:00:{second:02}.000Z\",\"type\":\"event_msg\",\"payload\":{{\"type\":\"task_started\",\"turn_id\":\"{turn_id}\"}}}}\n"
    )
}

fn tokens(day: &str, second: u32, input: u32, cached: u32, output: u32, reasoning: u32) -> String {
    format!(
        "{{\"timestamp\":\"{day}T10:00:{second:02}.000Z\",\"type\":\"event_msg\",\"payload\":{{\"type\":\"token_count\",\"info\":{{\"last_token_usage\":{{\"input_tokens\":{input},\"cached_input_tokens\":{cached},\"output_tokens\":{output},\"reasoning_output_tokens\":{reasoning}}}}}}}}}\n"
    )
}

fn fixture(id: &str, model: &str, body: impl Fn(&str) -> String) -> Fixture {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path().join("codex");
    let day = cost_bucket_zone()
        .date(Utc::now())
        .format("%Y-%m-%d")
        .to_string();
    let folder = day
        .split('-')
        .fold(home.join("sessions"), |path, part| path.join(part));
    fs::create_dir_all(&folder).unwrap();
    let rollout = folder.join(format!("rollout-{id}.jsonl"));
    let mut file = File::create(&rollout).unwrap();
    file.write_all(meta(&day, id, model).as_bytes()).unwrap();
    file.write_all(body(&day).as_bytes()).unwrap();
    let sidecar = tmp.path().join("sidecar.sqlite");
    Fixture {
        _tmp: tmp,
        home,
        sidecar,
        rollout,
        day,
    }
}

fn index(fixture: &Fixture) -> CodexWorkspacesIndex {
    CodexWorkspacesIndex::new(30)
        .with_codex_home(&fixture.home)
        .with_sidecar_path(&fixture.sidecar)
}

fn summary(fixture: &Fixture, id: &str) -> SessionUsage {
    index(fixture)
        .load_snapshot(true, |_| {})
        .unwrap()
        .sessions
        .into_iter()
        .find(|session| session.id == id)
        .expect("session summary")
}

fn checkpoint(session: &SessionUsage) -> SessionDetailCheckpoint {
    SessionDetailCheckpoint {
        totals: session.totals,
        cost_estimate: session.cost_estimate,
    }
}

fn load(
    fixture: &Fixture,
    id: &str,
    checkpoint: &SessionDetailCheckpoint,
    limits: SessionDetailLimits,
) -> Result<CodexSessionDetails, SessionDetailError> {
    index(fixture).load_session_details_at(id, checkpoint, Utc::now(), limits)
}

fn three_requests(day: &str) -> String {
    [
        tokens(day, 2, 100, 20, 10, 4),
        turn(day, 3, "turn-b"),
        tokens(day, 4, 300, 0, 30, 6),
        tokens(day, 5, 200, 50, 20, 2),
    ]
    .concat()
}

#[test]
fn groups_requests_by_turn_with_unattributed_usage_last() {
    let fixture = fixture("sess-a", "gpt-5", |day| {
        [turn(day, 1, " turn-a "), three_requests(day)].concat()
    });
    let session = summary(&fixture, "sess-a");
    let details = load(
        &fixture,
        "sess-a",
        &checkpoint(&session),
        SessionDetailLimits::default(),
    )
    .unwrap();

    assert_eq!(details.request_count, 3);
    let ids: Vec<_> = details.turns.iter().map(|t| t.turn_id.as_deref()).collect();
    assert_eq!(ids, [Some("turn-a"), Some("turn-b")]);
    let second = &details.turns[1];
    assert_eq!(second.request_count, 2);
    assert_eq!(second.input_tokens, 500);
    assert_eq!(second.cached_input_tokens, 50);
    assert_eq!(second.output_tokens, 50);
    assert_eq!(second.reasoning_tokens, Some(8));
    assert_eq!(second.total_tokens, 550);
    let requests: Vec<_> = second
        .requests
        .iter()
        .map(|r| (r.id.as_str(), r.input_tokens))
        .collect();
    assert_eq!(requests, [("request-0", 300), ("request-1", 200)]);
    let turn_cost: f64 = details.turns.iter().filter_map(|t| t.cost_usd).sum();
    assert!((turn_cost - session.cost_estimate.known_usd).abs() < 1e-12);
    let request_cost: f64 = second.requests.iter().filter_map(|r| r.cost_usd).sum();
    assert!((second.cost_usd.unwrap() - request_cost).abs() < 1e-12);

    let unattributed = fixture_with_leading_rows();
    let details = load(
        &unattributed.0,
        "sess-u",
        &checkpoint(&unattributed.1),
        SessionDetailLimits::default(),
    )
    .unwrap();
    let ids: Vec<_> = details.turns.iter().map(|t| t.turn_id.as_deref()).collect();
    assert_eq!(ids, [Some("turn-b"), None]);
    assert_eq!(details.turns[1].input_tokens, 100);
    assert_eq!(details.turns[1].first_activity, None);
}

fn fixture_with_leading_rows() -> (Fixture, SessionUsage) {
    let fixture = fixture("sess-u", "gpt-5", three_requests);
    let session = summary(&fixture, "sess-u");
    (fixture, session)
}

#[test]
fn refuses_details_that_no_longer_match_the_summary() {
    let fixture = fixture("sess-a", "gpt-5", three_requests);
    let session = summary(&fixture, "sess-a");
    let mut file = fs::OpenOptions::new()
        .append(true)
        .open(&fixture.rollout)
        .unwrap();
    file.write_all(tokens(&fixture.day, 9, 999, 0, 9, 0).as_bytes())
        .unwrap();

    let error = load(
        &fixture,
        "sess-a",
        &checkpoint(&session),
        SessionDetailLimits::default(),
    )
    .unwrap_err();
    assert_eq!(error, SessionDetailError::SourceChanged);
    assert_eq!(error.code(), "sourceChanged");
}

#[test]
fn fails_instead_of_truncating_when_limits_are_exceeded() {
    let fixture = fixture("sess-a", "gpt-5", three_requests);
    let session = summary(&fixture, "sess-a");
    let rows = load(
        &fixture,
        "sess-a",
        &checkpoint(&session),
        SessionDetailLimits {
            max_requests: 2,
            max_bytes: SESSION_DETAIL_MAX_BYTES,
        },
    );
    assert_eq!(rows.unwrap_err(), SessionDetailError::LimitExceeded);
    let bytes = load(
        &fixture,
        "sess-a",
        &checkpoint(&session),
        SessionDetailLimits {
            max_requests: SESSION_DETAIL_MAX_REQUESTS,
            max_bytes: 64,
        },
    );
    assert_eq!(bytes.unwrap_err(), SessionDetailError::LimitExceeded);
    let exact = load(
        &fixture,
        "sess-a",
        &checkpoint(&session),
        SessionDetailLimits {
            max_requests: 3,
            max_bytes: fs::metadata(&fixture.rollout).unwrap().len(),
        },
    );
    assert_eq!(exact.unwrap().request_count, 3);
}

#[test]
fn unknown_sessions_are_unavailable() {
    let fixture = fixture("sess-a", "gpt-5", three_requests);
    let session = summary(&fixture, "sess-a");
    let error = load(
        &fixture,
        "sess-missing",
        &checkpoint(&session),
        SessionDetailLimits::default(),
    )
    .unwrap_err();
    assert_eq!(error, SessionDetailError::Unavailable);
}

#[test]
fn unpriced_requests_keep_an_unknown_cost() {
    let fixture = fixture("sess-a", "not-a-real-model-xyz", |day| {
        [turn(day, 1, "turn-a"), tokens(day, 2, 100, 0, 10, 1)].concat()
    });
    let session = summary(&fixture, "sess-a");
    assert_eq!(session.cost_estimate.unknown_tokens, 110);
    let details = load(
        &fixture,
        "sess-a",
        &checkpoint(&session),
        SessionDetailLimits::default(),
    )
    .unwrap();
    assert_eq!(details.turns[0].cost_usd, None);
    assert_eq!(details.turns[0].requests[0].cost_usd, None);
}

#[test]
fn checkpoint_round_trips_through_the_bridge_shape() {
    let json = r#"{"totals":{"inputTokens":10,"cachedInputTokens":2,"outputTokens":3,"totalTokens":13},"costEstimate":{"knownUsd":0.5,"unknownTokens":0}}"#;
    let parsed: SessionDetailCheckpoint = serde_json::from_str(json).unwrap();
    assert_eq!(parsed.totals.total_tokens, 13);
    assert_eq!(parsed.cost_estimate.known_usd, 0.5);
}
