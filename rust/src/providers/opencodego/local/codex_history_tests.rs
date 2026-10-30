//! OpenAI-backed OpenCode history for the opt-in Codex cost view.

use super::codex_history::{codex_home_is_ambient, merge_rows_into_daily, merge_rows_into_summary};
use super::test_db::{
    create_session_message_table, insert_message_for, insert_session_message,
    insert_session_message_raw, insert_step_finish, iso_ms, open_db,
};
use super::*;
use crate::cost_reporting_period::CostTimeZone;
use crate::cost_scanner::ModelTokenCounts;

const UTC: CostTimeZone = CostTimeZone::Named(chrono_tz::UTC);
const TOKENS: &str =
    r#"{"total":180,"input":100,"output":40,"reasoning":10,"cache":{"read":25,"write":5}}"#;

fn day(iso: &str) -> NaiveDate {
    NaiveDate::parse_from_str(iso, "%Y-%m-%d").unwrap()
}

#[test]
fn openai_source_selects_only_openai_assistant_turns() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("opencode.db");
    let conn = open_db(&db, false);
    let at = iso_ms("2026-03-06T11:00:00.000Z");
    insert_message_for(
        &conn,
        "o1",
        at,
        Some(0.75),
        Some("gpt-5.1-codex"),
        "openai",
        None,
    );
    insert_message_for(
        &conn,
        "g1",
        at,
        Some(2.0),
        Some("kimi-k2"),
        "opencode-go",
        None,
    );
    insert_message_for(
        &conn,
        "a1",
        at,
        Some(4.0),
        Some("claude"),
        "anthropic",
        None,
    );
    drop(conn);

    let openai = read_rows(&db, RowSource::OpenAi).unwrap();
    assert_eq!(openai.len(), 1);
    assert_eq!(openai[0].model, "gpt-5.1-codex");
    assert!((openai[0].cost - 0.75).abs() < 1e-9);

    let go = read_rows(&db, RowSource::OpenCodeGo).unwrap();
    assert_eq!(go.len(), 1);
    assert_eq!(go[0].model, "kimi-k2");
}

#[test]
fn openai_source_prefers_step_finish_costs_and_keeps_recorded_zero() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("opencode.db");
    let conn = open_db(&db, true);
    let at = iso_ms("2026-03-06T11:00:00.000Z");
    insert_message_for(&conn, "o1", at, Some(9.0), Some("gpt-5"), "openai", None);
    insert_step_finish(&conn, "p1", "o1", at, 0.25, None);
    insert_step_finish(&conn, "p2", "o1", at, 0.5, None);
    insert_message_for(&conn, "o2", at, Some(0.0), Some("gpt-5"), "openai", None);
    drop(conn);

    let rows = read_rows(&db, RowSource::OpenAi).unwrap();
    let total: f64 = rows.iter().map(|row| row.cost).sum();
    assert_eq!(rows.len(), 3, "two steps plus the step-less $0 message");
    assert!((total - 0.75).abs() < 1e-9);
}

#[test]
fn openai_v2_rows_count_assistant_turns_only_and_skip_migrated_copies() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("opencode.db");
    let conn = open_db(&db, false);
    create_session_message_table(&conn);
    let at = iso_ms("2026-03-06T11:00:00.000Z");
    insert_message_for(&conn, "dup", at, Some(1.0), Some("gpt-5"), "openai", None);
    insert_session_message(&conn, "dup", at, 1.0, "gpt-5", "openai", None);
    insert_session_message(&conn, "v2", at, 0.5, "gpt-5", "openai", None);
    insert_session_message(&conn, "go", at, 3.0, "kimi-k2", "opencode-go", None);
    let compaction = format!(
        r#"{{"model":{{"providerID":"openai","id":"gpt-5"}},"cost":7.0,"time":{{"created":{at}}}}}"#
    );
    insert_session_message_raw(&conn, "c1", "compaction", at, &compaction);
    drop(conn);

    let rows = read_rows(&db, RowSource::OpenAi).unwrap();
    let total: f64 = rows.iter().map(|row| row.cost).sum();
    assert_eq!(rows.len(), 2);
    assert!((total - 1.5).abs() < 1e-9);
}

#[test]
fn openai_scan_skips_installs_without_openai_rows() {
    let dir = tempfile::tempdir().unwrap();
    let auth = dir.path().join("auth.json");
    let db = dir.path().join("opencode.db");
    std::fs::write(&auth, r#"{"opencode-go":{"key":"synthetic"}}"#).unwrap();
    let conn = open_db(&db, false);
    let at = iso_ms("2026-03-06T11:00:00.000Z");
    insert_message_for(
        &conn,
        "g1",
        at,
        Some(2.0),
        Some("kimi-k2"),
        "opencode-go",
        None,
    );
    drop(conn);

    let candidates = vec![(auth, db)];
    assert!(matches!(
        scan_rows_from(candidates.clone(), &ScanCache::new(), RowSource::OpenAi),
        Ok(None)
    ));
    assert!(matches!(
        scan_rows_from(candidates, &ScanCache::new(), RowSource::OpenCodeGo),
        Ok(Some(rows)) if rows.len() == 1
    ));
}

fn openai_rows(rows: &[(&str, &str, f64, Option<&str>)]) -> Vec<UsageRow> {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("opencode.db");
    let conn = open_db(&db, false);
    for (i, (iso, model, cost, tokens)) in rows.iter().enumerate() {
        insert_message_for(
            &conn,
            &format!("m{i}"),
            iso_ms(iso),
            Some(*cost),
            Some(model),
            "openai",
            *tokens,
        );
    }
    drop(conn);
    read_rows(&db, RowSource::OpenAi).unwrap()
}

#[test]
fn summary_merge_adds_recorded_costs_and_codex_convention_tokens() {
    let rows = openai_rows(&[
        ("2026-03-05T23:30:00.000Z", "gpt-5", 1.25, Some(TOKENS)),
        ("2026-03-06T10:00:00.000Z", "gpt-5", 0.0, Some(TOKENS)),
        ("2026-03-01T10:00:00.000Z", "gpt-5", 50.0, Some(TOKENS)),
    ]);
    let mut summary = CostSummary {
        total_cost_usd: 2.0,
        input_tokens: 1_000,
        output_tokens: 100,
        cached_tokens: 400,
        sessions_count: 3,
        ..CostSummary::default()
    };
    summary.by_model.insert("gpt-5".into(), 2.0);
    summary.by_model_tokens.insert(
        "gpt-5".into(),
        ModelTokenCounts {
            input_tokens: 1_000,
            output_tokens: 100,
            cached_tokens: 400,
            reasoning_tokens: None,
        },
    );

    merge_rows_into_summary(
        &rows,
        UTC,
        day("2026-03-05"),
        day("2026-03-06"),
        &mut summary,
    );

    // The 2026-03-01 row is outside the window; the recorded $0 still counts.
    assert!((summary.total_cost_usd - 3.25).abs() < 1e-9);
    assert_eq!(summary.recorded_cost_rows, 2);
    assert_eq!(summary.sessions_count, 5);
    // Per row: input 100 + cache 30 = 130; output 40 + reasoning 10 = 50; cached 25.
    assert_eq!(summary.input_tokens, 1_000 + 260);
    assert_eq!(summary.output_tokens, 100 + 100);
    assert_eq!(summary.cached_tokens, 400 + 50);
    assert!(summary.tokens_known());
    assert!((summary.by_model["gpt-5"] - 3.25).abs() < 1e-9);
    assert_eq!(summary.by_model_tokens["gpt-5"].input_tokens, 1_260);
    assert_eq!(summary.by_model_tokens["gpt-5"].reasoning_tokens, None);
}

#[test]
fn summary_merge_marks_rows_without_tokens_unknown() {
    let rows = openai_rows(&[
        ("2026-03-06T10:00:00.000Z", "gpt-5", 1.0, Some(TOKENS)),
        ("2026-03-06T11:00:00.000Z", "", 0.5, None),
    ]);
    let mut summary = CostSummary::default();

    merge_rows_into_summary(
        &rows,
        UTC,
        day("2026-03-06"),
        day("2026-03-06"),
        &mut summary,
    );

    assert!((summary.total_cost_usd - 1.5).abs() < 1e-9);
    assert_eq!(summary.unknown_token_rows, 1);
    assert!(!summary.tokens_known());
    assert_eq!(summary.unknown_token_rows_by_model.get("unknown"), Some(&1));
    assert!((summary.by_model["unknown"] - 0.5).abs() < 1e-9);
}

#[test]
fn summary_merge_without_rows_in_window_leaves_the_summary_unchanged() {
    let rows = openai_rows(&[("2026-02-01T10:00:00.000Z", "gpt-5", 1.0, Some(TOKENS))]);
    let mut summary = CostSummary {
        reasoning_tokens: Some(7),
        ..CostSummary::default()
    };

    merge_rows_into_summary(
        &rows,
        UTC,
        day("2026-03-05"),
        day("2026-03-06"),
        &mut summary,
    );

    assert_eq!(summary.total_cost_usd, 0.0);
    assert_eq!(summary.recorded_cost_rows, 0);
    assert_eq!(summary.reasoning_tokens, Some(7));
}

#[test]
fn daily_merge_adds_to_known_days_and_keeps_unknown_days_unknown() {
    let rows = openai_rows(&[
        ("2026-03-05T10:00:00.000Z", "gpt-5", 1.0, Some(TOKENS)),
        ("2026-03-06T10:00:00.000Z", "gpt-5", 2.0, Some(TOKENS)),
        ("2026-03-07T10:00:00.000Z", "gpt-5", 4.0, Some(TOKENS)),
    ]);
    let mut costs = vec![
        ("2026-03-05".to_string(), Some(0.5)),
        ("2026-03-06".to_string(), None),
    ];
    let mut tokens = vec![
        ("2026-03-05".to_string(), 10),
        ("2026-03-06".to_string(), 0),
    ];

    let incomplete = merge_rows_into_daily(&rows, UTC, &mut costs, &mut tokens);

    assert!(!incomplete);
    assert_eq!(costs[0].1, Some(1.5));
    assert_eq!(
        costs[1].1, None,
        "a day Codex has not established stays unknown"
    );
    // Codex total = input (incl. cache) + output (incl. reasoning) = 130 + 50.
    assert_eq!(tokens[0].1, 10 + 180);
    assert_eq!(tokens[1].1, 180);
}

#[test]
fn daily_merge_reports_days_with_unknown_tokens() {
    let rows = openai_rows(&[("2026-03-05T10:00:00.000Z", "gpt-5", 1.0, None)]);
    let mut costs = vec![("2026-03-05".to_string(), Some(0.0))];
    let mut tokens = vec![("2026-03-05".to_string(), 0)];

    assert!(merge_rows_into_daily(&rows, UTC, &mut costs, &mut tokens));
    assert_eq!(costs[0].1, Some(1.0));
    assert_eq!(tokens[0].1, 0);
}

#[test]
fn cost_scans_leave_opencode_history_out_unless_asked() {
    assert!(!crate::core::CostScanOptions::default().include_opencode_history);
    assert!(!crate::core::CostScanOptions::app_driven().include_opencode_history);
}

#[test]
fn only_an_unset_or_blank_codex_home_is_ambient() {
    assert!(codex_home_is_ambient(None));
    assert!(codex_home_is_ambient(Some("  ")));
    assert!(!codex_home_is_ambient(Some(r"D:\alt-codex")));
}
