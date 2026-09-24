use super::*;
use rusqlite::params;

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

fn valid_turn_blob(input: u64, timestamp_seconds: u64) -> Vec<u8> {
    valid_turn_blob_with_model(input, timestamp_seconds, None)
}

fn valid_turn_blob_with_model(input: u64, timestamp_seconds: u64, model: Option<&str>) -> Vec<u8> {
    let mut usage = field_varint(1, 11);
    usage.extend(field_varint(2, input));
    usage.extend(field_varint(5, 50));
    usage.extend(field_varint(9, 30));
    usage.extend(field_varint(10, 7));

    let mut timestamp = field_varint(1, timestamp_seconds);
    timestamp.extend(field_varint(2, 0));
    let mut chat = field_bytes(4, &usage);
    chat.extend(field_bytes(9, &field_bytes(4, &timestamp)));
    if let Some(model) = model {
        chat.extend(field_bytes(19, model.as_bytes()));
    }
    field_bytes(1, &chat)
}

fn zero_token_turn_blob(timestamp_seconds: u64, model: &str) -> Vec<u8> {
    let timestamp = field_varint(1, timestamp_seconds);
    let mut chat = field_bytes(4, &[]);
    chat.extend(field_bytes(9, &field_bytes(4, &timestamp)));
    chat.extend(field_bytes(19, model.as_bytes()));
    field_bytes(1, &chat)
}

#[test]
fn missing_databases_falls_through() {
    let dir = tempfile::tempdir().unwrap();
    assert!(matches!(
        summarize(&database_roots(&dir.path().join(".gemini")), Utc::now(), 30),
        SQLiteScan::NoDatabases
    ));
}

#[test]
fn foreign_database_is_non_authoritative() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join(".gemini/antigravity-cli/conversations");
    fs::create_dir_all(&root).unwrap();
    let conn = Connection::open(root.join("one.db")).unwrap();
    conn.execute("CREATE TABLE wrong(idx INTEGER, data BLOB)", [])
        .unwrap();
    drop(conn);
    assert!(matches!(
        summarize(&database_roots(&dir.path().join(".gemini")), Utc::now(), 30),
        SQLiteScan::Unsupported
    ));
}

#[test]
fn empty_supported_database_is_confirmed_zero() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join(".gemini/antigravity-cli/conversations");
    fs::create_dir_all(&root).unwrap();
    let conn = Connection::open(root.join("one.db")).unwrap();
    conn.execute("CREATE TABLE gen_metadata(idx INTEGER, data BLOB)", [])
        .unwrap();
    drop(conn);
    let SQLiteScan::Summary(summary) =
        summarize(&database_roots(&dir.path().join(".gemini")), Utc::now(), 30)
    else {
        panic!("supported database should produce coverage");
    };
    assert_eq!(summary.coverage, LocalHistoryCoverage::Complete);
    assert_eq!(summary.total_tokens, 0);
    assert_eq!(summary.session_count, 0);
}

#[test]
fn same_named_databases_in_separate_roots_keep_distinct_rows_and_sessions() {
    let dir = tempfile::tempdir().unwrap();
    let first_root = dir.path().join("first");
    let second_root = dir.path().join("second");
    fs::create_dir_all(&first_root).unwrap();
    fs::create_dir_all(&second_root).unwrap();
    let timestamp = u64::try_from(Utc::now().timestamp()).unwrap();

    for (root, input) in [(&first_root, 100_u64), (&second_root, 200_u64)] {
        let conn = Connection::open(root.join("session.db")).unwrap();
        conn.execute("CREATE TABLE gen_metadata(idx INTEGER, data BLOB)", [])
            .unwrap();
        conn.execute(
            "INSERT INTO gen_metadata(idx, data) VALUES(1, ?1)",
            [valid_turn_blob(input, timestamp)],
        )
        .unwrap();
    }

    let SQLiteScan::Summary(summary) = summarize(&[first_root, second_root], Utc::now(), 30) else {
        panic!("supported databases should produce coverage");
    };

    assert_eq!(summary.coverage, LocalHistoryCoverage::Complete);
    assert_eq!(summary.total_tokens, 496);
    assert_eq!(summary.session_count, 2);
}

#[test]
fn zero_token_unknown_model_does_not_poison_priced_history() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join(".gemini/antigravity-cli/conversations");
    fs::create_dir_all(&root).unwrap();
    let now = Utc::now();
    let timestamp = u64::try_from(now.timestamp()).unwrap();

    let priced = Connection::open(root.join("priced.db")).unwrap();
    priced
        .execute("CREATE TABLE gen_metadata(idx INTEGER, data BLOB)", [])
        .unwrap();
    priced
        .execute(
            "INSERT INTO gen_metadata(idx, data) VALUES(1, ?1)",
            [valid_turn_blob_with_model(
                100,
                timestamp,
                Some("claude-sonnet-4-6"),
            )],
        )
        .unwrap();

    let zero = Connection::open(root.join("zero.db")).unwrap();
    zero.execute("CREATE TABLE gen_metadata(idx INTEGER, data BLOB)", [])
        .unwrap();
    zero.execute(
        "INSERT INTO gen_metadata(idx, data) VALUES(1, ?1)",
        [zero_token_turn_blob(timestamp, "unknown-model")],
    )
    .unwrap();

    let SQLiteScan::Summary(summary) = summarize(&[root], now, 30) else {
        panic!("supported databases should produce coverage");
    };

    assert_eq!(summary.coverage, LocalHistoryCoverage::Complete);
    assert_eq!(summary.total_tokens, 198);
    assert_eq!(summary.session_count, 2);
    assert_eq!(summary.cost_estimate.coverage.estimated, 1);
    assert_eq!(summary.cost_estimate.coverage.unpriced, 0);
    assert!(
        summary
            .cost_estimate
            .known_subtotal_usd
            .is_some_and(|cost| cost > 0.0)
    );
    assert_eq!(
        summary.total_usd(),
        summary.cost_estimate.known_subtotal_usd
    );
}

#[test]
fn non_blob_rows_make_coverage_partial() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join(".gemini/antigravity-cli/conversations");
    fs::create_dir_all(&root).unwrap();
    let conn = Connection::open(root.join("one.db")).unwrap();
    conn.execute("CREATE TABLE gen_metadata(idx INTEGER, data BLOB)", [])
        .unwrap();
    conn.execute(
        "INSERT INTO gen_metadata(idx,data) VALUES(?1,?2)",
        params![1_i64, "not-a-blob"],
    )
    .unwrap();
    drop(conn);
    let SQLiteScan::Summary(summary) =
        summarize(&database_roots(&dir.path().join(".gemini")), Utc::now(), 30)
    else {
        panic!("supported database should produce coverage");
    };
    assert_eq!(summary.coverage, LocalHistoryCoverage::Partial);
}
#[test]
fn discovery_allows_exactly_500_databases_but_marks_501_partial() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("dbs");
    fs::create_dir_all(&root).unwrap();
    for index in 0..MAX_DATABASES {
        fs::write(root.join(format!("{index:03}.db")), b"").unwrap();
    }
    let mut budget = Budget::new();
    let (paths, complete) = discover_databases(std::slice::from_ref(&root), &mut budget);
    assert_eq!(paths.len(), MAX_DATABASES);
    assert!(complete);

    fs::write(root.join("overflow.db"), b"").unwrap();
    let mut budget = Budget::new();
    let (paths, complete) = discover_databases(std::slice::from_ref(&root), &mut budget);
    assert_eq!(paths.len(), MAX_DATABASES);
    assert!(!complete);
}

#[test]
fn expired_budget_marks_discovery_incomplete() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("dbs");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("one.db"), b"").unwrap();
    let mut budget = Budget::with_deadline(Instant::now());
    let (_, complete) = discover_databases(std::slice::from_ref(&root), &mut budget);
    assert!(!complete);
}

#[test]
fn extra_columns_and_without_rowid_schema_is_supported() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute(
        "CREATE TABLE gen_metadata(idx INTEGER PRIMARY KEY, data BLOB, extra TEXT) WITHOUT ROWID",
        [],
    )
    .unwrap();
    let mut budget = Budget::new();
    assert_eq!(
        supported_schema(&conn, &mut budget).unwrap(),
        SchemaInspection::Supported
    );
}

#[test]
fn generated_columns_are_rejected() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute(
            "CREATE TABLE gen_metadata(idx INTEGER, data BLOB, derived TEXT GENERATED ALWAYS AS (idx || 'x') VIRTUAL)",
            [],
        )
        .unwrap();
    let mut budget = Budget::new();
    assert_eq!(
        supported_schema(&conn, &mut budget).unwrap(),
        SchemaInspection::Unsupported
    );
}

#[test]
fn schema_entry_budget_is_incomplete_not_foreign() {
    let conn = Connection::open_in_memory().unwrap();
    for index in 0..=MAX_SCHEMA_ENTRIES {
        conn.execute(&format!("CREATE TABLE unrelated_{index}(value TEXT)"), [])
            .unwrap();
    }
    let mut budget = Budget::new();
    assert_eq!(
        supported_schema(&conn, &mut budget).unwrap(),
        SchemaInspection::Incomplete
    );
}
