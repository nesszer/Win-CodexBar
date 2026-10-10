//! Recorded-token coverage for the OpenCode Go local reader (upstream 0.67.0).

use super::test_db::{insert_message, insert_step_finish, iso_ms, open_db};
use super::*;

const FULL: &str =
    r#"{"total":1600,"input":100,"output":20,"reasoning":30,"cache":{"read":1400,"write":50}}"#;
/// Older OpenCode rows record no `total`; the five components sum to 20.
const NO_TOTAL: &str = r#"{"input":10,"output":5,"reasoning":1,"cache":{"read":4,"write":0}}"#;

fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 3, 8, 0, 0, 0).unwrap()
}

fn parse(json: &str) -> Option<RowTokens> {
    RowTokens::parse(json)
}

#[test]
fn row_tokens_resolve_total_or_sum_all_five_components() {
    assert_eq!(parse(FULL).unwrap().total, 1600);
    let older = parse(NO_TOTAL).unwrap();
    assert_eq!(older.total, 20);
    assert_eq!(older.cache_write, Some(0));
    // A recorded total is used as-is even when components are absent.
    let total_only = parse(r#"{"total":7}"#).unwrap();
    assert_eq!((total_only.total, total_only.input), (7, None));
}

#[test]
fn input_output_total_requires_complete_class_counts_for_every_row() {
    let full = parse(FULL).unwrap();
    let older = parse(NO_TOTAL).unwrap();
    let mut sums = TokenSums::default();
    sums.add(Some(&full));
    sums.add(Some(&older));
    assert_eq!(sums.complete_input_output(), Some(135));

    let total_only = parse(r#"{"total":7}"#).unwrap();
    let mut sums = TokenSums::default();
    sums.add(Some(&total_only));
    assert_eq!(sums.complete_total(), Some(7));
    assert_eq!(sums.complete_input_output(), None);

    sums.add(None);
    assert_eq!(sums.complete_input_output(), None);
}

#[test]
fn input_output_total_is_unknown_when_the_sum_overflows() {
    let max = i64::MAX;
    let json = format!(r#"{{"total":0,"input":{max},"output":1}}"#);
    let counts = parse(&json).unwrap();
    let mut sums = TokenSums::default();
    sums.add(Some(&counts));
    sums.add(Some(&counts));
    assert_eq!(sums.complete_input_output(), None);
}

#[test]
fn shared_cache_count_is_omitted_when_read_plus_write_overflows() {
    let max = i64::MAX;
    let counts = parse(&format!(
        r#"{{"total":1,"cache":{{"read":{max},"write":{max}}}}}"#
    ))
    .unwrap();
    let mut sums = TokenSums::default();
    for _ in 0..2 {
        sums.add(Some(&counts));
    }
    assert!(sums.complete_total().is_some());
    assert_eq!(sums.to_model_token_counts(), None);
}

#[test]
fn row_tokens_reject_unresolvable_negative_malformed_and_overflowing_rows() {
    // No total and a missing component: nothing to resolve.
    assert_eq!(parse(r#"{"input":1,"output":2}"#), None);
    assert_eq!(parse("{}"), None);
    // Any negative count, including the total, makes the row unusable.
    assert_eq!(parse(r#"{"total":-1}"#), None);
    assert_eq!(parse(r#"{"total":5,"input":-1}"#), None);
    assert_eq!(parse(r#"{"total":5,"cache":{"write":-3}}"#), None);
    // Malformed shapes are unknown, never zero.
    assert_eq!(parse(r#"{"total":"12"}"#), None);
    assert_eq!(parse(r#"{"total":1.5}"#), None);
    assert_eq!(parse(r#"{"total":5,"cache":7}"#), None);
    assert_eq!(parse("not json"), None);
    // Checked sum overflow when the total has to be derived.
    let max = i64::MAX;
    let overflow = format!(
        r#"{{"input":{max},"output":{max},"reasoning":{max},"cache":{{"read":{max},"write":{max}}}}}"#
    );
    assert_eq!(parse(&overflow), None);
}

#[test]
fn daily_entries_carry_message_token_counts_per_day_and_model() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("opencode.db");
    let conn = open_db(&db, false);
    let (first, second) = (
        iso_ms("2026-03-06T11:00:00.000Z"),
        iso_ms("2026-03-06T11:30:00.000Z"),
    );
    insert_message(&conn, "m1", first, Some(1.5), Some("kimi-k2"), Some(FULL));
    insert_message(
        &conn,
        "m2",
        second,
        Some(1.5),
        Some("kimi-k2"),
        Some(NO_TOTAL),
    );
    drop(conn);

    let rows = read_rows(&db).unwrap();
    let daily = daily_model_costs(&rows, now(), 30);
    assert_eq!(daily.len(), 1);
    let tokens = &daily[0].tokens;
    assert_eq!(tokens.complete_total(), Some(1620));
    assert_eq!(tokens.input, 110);
    assert_eq!(tokens.output, 25);
    assert_eq!(tokens.reasoning, 31);
    assert_eq!(tokens.complete_reasoning(), Some(31));
    assert_eq!(tokens.cache_read, 1404);
    assert_eq!(tokens.cache_write, 50);
    // Costs stay the recorded field, never derived from tokens.
    assert!((daily[0].cost - 3.0).abs() < 1e-9);
}

#[test]
fn step_finish_tokens_replace_the_parent_message_tokens() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("opencode.db");
    let conn = open_db(&db, true);
    let created = iso_ms("2026-03-06T11:00:00.000Z");
    // The parent message records the session total; the steps record each call.
    insert_message(&conn, "m1", created, Some(1.5), Some("kimi-k2"), Some(FULL));
    let step_a =
        r#"{"total":30,"input":10,"output":5,"reasoning":5,"cache":{"read":10,"write":0}}"#;
    let step_b = r#"{"total":20,"input":8,"output":2,"reasoning":0,"cache":{"read":10,"write":0}}"#;
    insert_step_finish(&conn, "p1", "m1", created, 0.5, Some(step_a));
    insert_step_finish(&conn, "p2", "m1", created, 0.5, Some(step_b));
    // A message with no step-finish parts keeps using its own tokens.
    insert_message(
        &conn,
        "m2",
        created,
        Some(1.5),
        Some("kimi-k2"),
        Some(NO_TOTAL),
    );
    drop(conn);

    let rows = read_rows(&db).unwrap();
    assert_eq!(rows.len(), 3);
    let daily = daily_model_costs(&rows, now(), 30);
    let tokens = &daily[0].tokens;
    assert_eq!(tokens.complete_total(), Some(30 + 20 + 20));
    assert_eq!(tokens.input, 10 + 8 + 10);
    assert_eq!(tokens.cache_read, 10 + 10 + 4);
}

#[test]
fn rows_without_usable_tokens_leave_the_day_incomplete_not_zero() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("opencode.db");
    let conn = open_db(&db, false);
    let created = iso_ms("2026-03-06T11:00:00.000Z");
    insert_message(&conn, "m1", created, Some(1.5), Some("kimi-k2"), Some(FULL));
    insert_message(&conn, "m2", created, Some(1.5), Some("kimi-k2"), None);
    insert_message(&conn, "m3", created, Some(1.5), Some("kimi-k2"), Some("5"));
    insert_message(
        &conn,
        "m4",
        created,
        Some(1.5),
        Some("kimi-k2"),
        Some(r#"{"total":-4}"#),
    );
    drop(conn);

    let rows = read_rows(&db).unwrap();
    // Costs survive even when tokens do not.
    assert_eq!(rows.len(), 4);
    let daily = daily_model_costs(&rows, now(), 30);
    let tokens = &daily[0].tokens;
    assert_eq!(tokens.complete_total(), None);
    assert_eq!(tokens.complete_input_output(), None);
    assert_eq!(tokens.complete_reasoning(), None);
    assert!(tokens.has_usable_rows());
    assert_eq!(tokens.total, 1600);
    assert!((daily[0].cost - 6.0).abs() < 1e-9);
}

#[test]
fn model_summary_carries_per_model_tokens_and_marks_partial_models() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("opencode.db");
    let conn = open_db(&db, false);
    let created = iso_ms("2026-03-06T11:00:00.000Z");
    insert_message(&conn, "m1", created, Some(1.5), Some("kimi-k2"), Some(FULL));
    insert_message(
        &conn,
        "m2",
        created,
        Some(1.5),
        Some("kimi-k2"),
        Some(NO_TOTAL),
    );
    insert_message(&conn, "m3", created, Some(1.5), Some("glm-5"), None);
    insert_message(
        &conn,
        "m4",
        created,
        Some(1.5),
        Some("mimo"),
        Some(r#"{"total":9,"input":4}"#),
    );
    drop(conn);

    let rows = read_rows(&db).unwrap();
    let summary = model_cost_summary_from_rows(&rows, now(), 30);

    let kimi = summary.by_model_tokens["kimi-k2"]
        .to_model_token_counts()
        .unwrap();
    assert_eq!(
        (kimi.input_tokens, kimi.output_tokens, kimi.cached_tokens),
        (110, 25, 1404 + 50)
    );
    assert_eq!(kimi.reasoning_tokens, Some(31));

    // A model with no usable row has no token counts at all (unknown, not zero).
    assert_eq!(
        summary.by_model_tokens["glm-5"].to_model_token_counts(),
        None
    );
    // A row that omits classes keeps reasoning unknown.
    let mimo = summary.by_model_tokens["mimo"]
        .to_model_token_counts()
        .unwrap();
    assert_eq!((mimo.input_tokens, mimo.reasoning_tokens), (4, None));
    assert_eq!(summary.by_model_tokens["mimo"].complete_total(), Some(9));

    // The window total is incomplete because one model has no usable row.
    assert_eq!(summary.tokens.complete_total(), None);
    assert_eq!(summary.tokens.total, 1600 + 20 + 9);
}

#[test]
fn token_sums_treat_overflow_as_an_unusable_row() {
    let big = RowTokens::parse(&format!(r#"{{"total":{},"input":1}}"#, i64::MAX)).unwrap();
    let mut sums = TokenSums::default();
    sums.add(Some(&big));
    sums.add(Some(&big));
    assert_eq!(sums.complete_total(), Some(i64::MAX as u64 * 2));
    sums.add(Some(&big));
    assert_eq!(sums.complete_total(), None);
    assert!(sums.has_usable_rows());
    assert_eq!(sums.total, i64::MAX as u64 * 2);
}
