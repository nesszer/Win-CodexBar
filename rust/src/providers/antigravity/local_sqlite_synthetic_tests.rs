use std::fs;
use std::path::PathBuf;

use chrono::{DateTime, TimeZone, Utc};
use rusqlite::{Connection, params};
use tempfile::TempDir;

use super::*;

const NOW_SECONDS: u64 = 1_800_000_000;

fn now() -> DateTime<Utc> {
    Utc.timestamp_opt(i64::try_from(NOW_SECONDS).unwrap(), 0)
        .single()
        .unwrap()
}

fn varint(mut value: u64) -> Vec<u8> {
    let mut bytes = Vec::new();
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        bytes.push(byte);
        if value == 0 {
            return bytes;
        }
    }
}

fn field_varint(number: u64, value: u64) -> Vec<u8> {
    let mut bytes = varint(number << 3);
    bytes.extend(varint(value));
    bytes
}

fn field_bytes(number: u64, value: &[u8]) -> Vec<u8> {
    let mut bytes = varint((number << 3) | 2);
    bytes.extend(varint(value.len() as u64));
    bytes.extend(value);
    bytes
}

fn turn_blob(step_uuid: Option<&str>, input: u64, timestamp: Option<u64>) -> Vec<u8> {
    turn_blob_with_bot_id(step_uuid, input, timestamp, None)
}

fn turn_blob_with_bot_id(
    step_uuid: Option<&str>,
    input: u64,
    timestamp: Option<u64>,
    bot_id: Option<&str>,
) -> Vec<u8> {
    let mut usage = field_varint(1, 11);
    usage.extend(field_varint(2, input));
    usage.extend(field_varint(5, 50));
    usage.extend(field_varint(9, 30));
    usage.extend(field_varint(10, 7));
    if let Some(bot_id) = bot_id {
        usage.extend(field_bytes(7, bot_id.as_bytes()));
    }

    let mut chat = field_bytes(4, &usage);
    if let Some(seconds) = timestamp {
        let mut stamp = field_varint(1, seconds);
        stamp.extend(field_varint(2, 250_000_000));
        chat.extend(field_bytes(9, &field_bytes(4, &stamp)));
    }

    let mut root = Vec::new();
    if let Some(step_uuid) = step_uuid {
        // Newer records can begin with the turn-coordination envelope (field 2).
        root.extend(field_bytes(2, &[1, 2]));
        root.extend(field_bytes(4, step_uuid.as_bytes()));
    }
    root.extend(field_bytes(1, &chat));
    root
}

fn step_metadata(step_uuid: Option<&str>, timestamp: Option<u64>) -> Vec<u8> {
    step_metadata_with_bot_id(step_uuid, timestamp, None)
}

fn step_metadata_with_bot_id(
    step_uuid: Option<&str>,
    timestamp: Option<u64>,
    bot_id: Option<&str>,
) -> Vec<u8> {
    let mut metadata = Vec::new();
    if let Some(seconds) = timestamp {
        let mut stamp = field_varint(1, seconds);
        stamp.extend(field_varint(2, 0));
        metadata.extend(field_bytes(1, &stamp));
    }
    if let Some(step_uuid) = step_uuid {
        metadata.extend(field_bytes(12, step_uuid.as_bytes()));
    }
    if let Some(bot_id) = bot_id {
        metadata.extend(field_bytes(9, &field_bytes(7, bot_id.as_bytes())));
    }
    metadata
}

fn malformed_step_metadata(step_uuid: &str) -> Vec<u8> {
    let mut metadata = field_bytes(12, step_uuid.as_bytes());
    metadata.extend([0x0a, 0x80]);
    metadata
}

type SyntheticStepRows<'a> = &'a [(i64, Option<Vec<u8>>)];

fn database(
    dir: &TempDir,
    session: &str,
    generation_rows: &[(i64, Vec<u8>)],
    step_rows: Option<SyntheticStepRows<'_>>,
) -> PathBuf {
    let root = dir.path().join("conversations");
    fs::create_dir_all(&root).unwrap();
    let path = root.join(format!("{session}.db"));
    let conn = Connection::open(&path).unwrap();
    conn.execute("CREATE TABLE gen_metadata (idx INTEGER, data BLOB)", [])
        .unwrap();
    for (row, blob) in generation_rows {
        conn.execute(
            "INSERT INTO gen_metadata (idx, data) VALUES (?1, ?2)",
            params![*row, blob.as_slice()],
        )
        .unwrap();
    }
    if let Some(step_rows) = step_rows {
        conn.execute("CREATE TABLE steps (idx INTEGER, metadata BLOB)", [])
            .unwrap();
        for (row, blob) in step_rows {
            conn.execute(
                "INSERT INTO steps (idx, metadata) VALUES (?1, ?2)",
                params![*row, blob.as_deref()],
            )
            .unwrap();
        }
    }
    drop(conn);
    path
}

fn summary(dir: &TempDir) -> LocalSessionSummary {
    let root = dir.path().join("conversations");
    let SQLiteScan::Summary(summary) = summarize(&[root], now(), 30) else {
        panic!("database should be attempted");
    };
    summary
}

#[test]
fn newer_step_timestamp_recovery_preserves_legacy_and_new_totals() {
    let dir = tempfile::tempdir().unwrap();
    let uuid = "new-step";
    database(
        &dir,
        "mixed",
        &[
            (0, turn_blob(None, 100, Some(NOW_SECONDS - 120))),
            (1, turn_blob(Some(uuid), 200, None)),
        ],
        Some(&[(10, Some(step_metadata(Some(uuid), Some(NOW_SECONDS - 60))))]),
    );

    let summary = summary(&dir);

    assert_eq!(summary.coverage, LocalHistoryCoverage::Complete);
    assert_eq!(summary.total_tokens, 496);
    assert_eq!(summary.session_count, 1);
}

#[test]
fn absent_steps_table_withholds_newer_rows_but_keeps_embedded_totals() {
    let dir = tempfile::tempdir().unwrap();
    database(
        &dir,
        "missing-table",
        &[
            (0, turn_blob(None, 100, Some(NOW_SECONDS - 120))),
            (1, turn_blob(Some("missing-table"), 200, None)),
        ],
        None,
    );

    let summary = summary(&dir);

    assert_eq!(summary.coverage, LocalHistoryCoverage::Partial);
    assert_eq!(summary.total_tokens, 198);
}

#[test]
fn reused_step_uuid_follows_stored_idx_order() {
    let dir = tempfile::tempdir().unwrap();
    let uuid = "reused-step";
    database(
        &dir,
        "ordered",
        &[
            (0, turn_blob(Some(uuid), 100, None)),
            (1, turn_blob(Some(uuid), 200, None)),
        ],
        Some(&[
            (20, Some(step_metadata(Some(uuid), Some(NOW_SECONDS - 60)))),
            (10, Some(step_metadata(Some(uuid), Some(NOW_SECONDS - 120)))),
        ]),
    );

    let summary = summary(&dir);

    assert_eq!(summary.coverage, LocalHistoryCoverage::Complete);
    assert_eq!(summary.total_tokens, 496);
}

#[test]
fn missing_lowest_step_timestamp_withholds_pending_tokens() {
    let dir = tempfile::tempdir().unwrap();
    let uuid = "missing-lowest";
    database(
        &dir,
        "missing",
        &[(0, turn_blob(Some(uuid), 100, None))],
        Some(&[
            (10, Some(step_metadata(Some(uuid), None))),
            (20, Some(step_metadata(Some(uuid), Some(NOW_SECONDS - 60)))),
        ]),
    );

    let summary = summary(&dir);

    assert_eq!(summary.coverage, LocalHistoryCoverage::Partial);
    assert_eq!(summary.total_tokens, 0);
}

#[test]
fn duplicate_or_malformed_step_rows_fail_closed() {
    for (session, step_rows) in [
        (
            "duplicate",
            vec![
                (
                    10,
                    Some(step_metadata(Some("duplicate"), Some(NOW_SECONDS - 120))),
                ),
                (
                    10,
                    Some(step_metadata(Some("duplicate"), Some(NOW_SECONDS - 60))),
                ),
            ],
        ),
        (
            "malformed",
            vec![(10, Some(malformed_step_metadata("malformed")))],
        ),
    ] {
        let dir = tempfile::tempdir().unwrap();
        database(
            &dir,
            session,
            &[(0, turn_blob(Some(session), 100, None))],
            Some(&step_rows),
        );

        let summary = summary(&dir);

        assert_eq!(summary.coverage, LocalHistoryCoverage::Partial, "{session}");
        assert_eq!(summary.total_tokens, 0, "{session}");
    }
}

#[test]
fn null_or_unidentified_step_rows_fail_closed() {
    for (session, step_row) in [
        ("null-step", (10, None)),
        (
            "unidentified-step",
            (10, Some(step_metadata(None, Some(NOW_SECONDS - 60)))),
        ),
    ] {
        let dir = tempfile::tempdir().unwrap();
        database(
            &dir,
            session,
            &[(0, turn_blob(Some(session), 100, None))],
            Some(&[step_row]),
        );

        let summary = summary(&dir);

        assert_eq!(summary.coverage, LocalHistoryCoverage::Partial, "{session}");
        assert_eq!(summary.total_tokens, 0, "{session}");
    }
}

#[test]
fn embedded_and_step_timestamps_must_agree() {
    let dir = tempfile::tempdir().unwrap();
    let uuid = "conflicting";
    database(
        &dir,
        "conflict",
        &[
            (0, turn_blob(Some(uuid), 100, Some(NOW_SECONDS - 120))),
            (1, turn_blob(Some(uuid), 200, None)),
        ],
        Some(&[
            (10, Some(step_metadata(Some(uuid), Some(NOW_SECONDS - 60)))),
            (20, Some(step_metadata(Some(uuid), Some(NOW_SECONDS - 30)))),
        ]),
    );

    let summary = summary(&dir);

    assert_eq!(summary.coverage, LocalHistoryCoverage::Partial);
    assert_eq!(summary.total_tokens, 198);
}

#[test]
fn unique_bot_ids_recover_each_turn_despite_auxiliary_step_rows() {
    let dir = tempfile::tempdir().unwrap();
    let uuid = "per-turn";
    database(
        &dir,
        "bot-correlation",
        &[
            (
                10,
                turn_blob_with_bot_id(Some(uuid), 100, None, Some("bot-a")),
            ),
            (
                20,
                turn_blob_with_bot_id(Some(uuid), 200, None, Some("bot-b")),
            ),
        ],
        Some(&[
            (
                1,
                Some(step_metadata_with_bot_id(
                    Some(uuid),
                    Some(NOW_SECONDS - 300),
                    Some("auxiliary"),
                )),
            ),
            (
                2,
                Some(step_metadata_with_bot_id(
                    Some(uuid),
                    Some(NOW_SECONDS - 120),
                    Some("bot-a"),
                )),
            ),
            (
                3,
                Some(step_metadata_with_bot_id(
                    Some(uuid),
                    Some(NOW_SECONDS - 60),
                    Some("bot-b"),
                )),
            ),
        ]),
    );

    let summary = summary(&dir);

    assert_eq!(summary.coverage, LocalHistoryCoverage::Complete);
    assert_eq!(summary.total_tokens, 496);
}

#[test]
fn conflicting_bot_id_evidence_is_withheld() {
    let dir = tempfile::tempdir().unwrap();
    let uuid = "ambiguous-bot";
    database(
        &dir,
        "ambiguous-bot",
        &[(
            (0),
            turn_blob_with_bot_id(Some(uuid), 100, None, Some("bot-x")),
        )],
        Some(&[
            (
                10,
                Some(step_metadata_with_bot_id(
                    Some(uuid),
                    Some(NOW_SECONDS - 120),
                    Some("bot-x"),
                )),
            ),
            (
                20,
                Some(step_metadata_with_bot_id(
                    Some(uuid),
                    Some(NOW_SECONDS - 60),
                    Some("bot-x"),
                )),
            ),
        ]),
    );

    let summary = summary(&dir);

    assert_eq!(summary.coverage, LocalHistoryCoverage::Partial);
    assert_eq!(summary.total_tokens, 0);
}

#[test]
fn cross_step_bot_id_evidence_is_withheld() {
    let dir = tempfile::tempdir().unwrap();
    database(
        &dir,
        "cross-step-bot",
        &[(
            (0),
            turn_blob_with_bot_id(Some("needed-step"), 100, None, Some("bot-x")),
        )],
        Some(&[
            (
                10,
                Some(step_metadata_with_bot_id(
                    Some("other-step"),
                    Some(NOW_SECONDS - 120),
                    Some("bot-x"),
                )),
            ),
            (
                20,
                Some(step_metadata_with_bot_id(
                    Some("needed-step"),
                    Some(NOW_SECONDS - 60),
                    Some("other-bot"),
                )),
            ),
        ]),
    );

    let summary = summary(&dir);

    assert_eq!(summary.coverage, LocalHistoryCoverage::Partial);
    assert_eq!(summary.total_tokens, 0);
}
