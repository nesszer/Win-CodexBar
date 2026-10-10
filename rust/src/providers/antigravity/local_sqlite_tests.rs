use super::*;
use crate::providers::antigravity::local_proto::test_fixtures::{field_bytes, field_varint};
use rusqlite::params;

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

/// The CLI conversations directory under `dir`.
fn cli_root(dir: &tempfile::TempDir) -> PathBuf {
    dir.path().join(".gemini/antigravity-cli/conversations")
}

/// Open `root/name`, creating `root`, with an empty `gen_metadata` table.
fn database_at(root: &Path, name: &str) -> Connection {
    fs::create_dir_all(root).unwrap();
    let conn = Connection::open(root.join(name)).unwrap();
    conn.execute("CREATE TABLE gen_metadata(idx INTEGER, data BLOB)", [])
        .unwrap();
    conn
}

fn insert_row(conn: &Connection, idx: i64, data: impl rusqlite::ToSql) {
    conn.execute(
        "INSERT INTO gen_metadata(idx, data) VALUES(?1, ?2)",
        params![idx, data],
    )
    .unwrap();
}

/// Scan the default `.gemini` roots under `dir`.
fn expect_summary(dir: &tempfile::TempDir) -> LocalSessionSummary {
    let SQLiteScan::Summary(summary) =
        summarize(&database_roots(&dir.path().join(".gemini")), Utc::now(), 30)
    else {
        panic!("supported database should produce coverage");
    };
    summary
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
    let root = cli_root(&dir);
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
    drop(database_at(&cli_root(&dir), "one.db"));
    let summary = expect_summary(&dir);
    assert_eq!(summary.coverage, LocalHistoryCoverage::Complete);
    assert_eq!(summary.total_tokens, 0);
    assert_eq!(summary.session_count, 0);
}

#[test]
fn same_named_databases_in_separate_roots_keep_distinct_rows_and_sessions() {
    let dir = tempfile::tempdir().unwrap();
    let first_root = dir.path().join("first");
    let second_root = dir.path().join("second");
    let timestamp = u64::try_from(Utc::now().timestamp()).unwrap();

    for (root, input) in [(&first_root, 100_u64), (&second_root, 200_u64)] {
        let conn = database_at(root, "session.db");
        insert_row(&conn, 1, valid_turn_blob(input, timestamp));
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
    let root = cli_root(&dir);
    let now = Utc::now();
    let timestamp = u64::try_from(now.timestamp()).unwrap();

    let priced = database_at(&root, "priced.db");
    insert_row(
        &priced,
        1,
        valid_turn_blob_with_model(100, timestamp, Some("claude-sonnet-4-6")),
    );
    let zero = database_at(&root, "zero.db");
    insert_row(&zero, 1, zero_token_turn_blob(timestamp, "unknown-model"));

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
    let conn = database_at(&cli_root(&dir), "one.db");
    insert_row(&conn, 1, "not-a-blob");
    drop(conn);
    assert_eq!(expect_summary(&dir).coverage, LocalHistoryCoverage::Partial);
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
        table_schema(&conn, &mut budget, "gen_metadata", &["idx", "data"]).unwrap(),
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
        table_schema(&conn, &mut budget, "gen_metadata", &["idx", "data"]).unwrap(),
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
        table_schema(&conn, &mut budget, "gen_metadata", &["idx", "data"]).unwrap(),
        SchemaInspection::Incomplete
    );
}

#[test]
fn undecodable_row_beside_valid_rows_yields_a_lower_bound() {
    let dir = tempfile::tempdir().unwrap();
    let timestamp = u64::try_from(Utc::now().timestamp()).unwrap();
    let conn = database_at(&cli_root(&dir), "one.db");
    insert_row(&conn, 1, valid_turn_blob(100, timestamp));
    insert_row(&conn, 2, "not-a-blob");
    drop(conn);

    let summary = expect_summary(&dir);

    assert_eq!(summary.coverage, LocalHistoryCoverage::Partial);
    assert!(summary.total_tokens > 0);
    assert!(summary.lower_bound);
    assert_eq!(summary.published_tokens(), Some(summary.total_tokens));
    assert_eq!(summary.total_usd(), None);
}

#[test]
fn contradicting_rows_for_one_index_are_withheld_not_published() {
    let dir = tempfile::tempdir().unwrap();
    let timestamp = u64::try_from(Utc::now().timestamp()).unwrap();
    let conn = database_at(&cli_root(&dir), "one.db");
    for input in [100_u64, 900_u64] {
        insert_row(&conn, 1, valid_turn_blob(input, timestamp));
    }
    drop(conn);

    let summary = expect_summary(&dir);

    assert_eq!(summary.coverage, LocalHistoryCoverage::Partial);
    assert_eq!(summary.total_tokens, 0);
    assert!(!summary.lower_bound);
    assert_eq!(summary.published_tokens(), None);
}

#[test]
fn list_price_uses_prompt_plus_input_and_output_plus_reasoning() {
    // Upstream AntigravityLocalReaderTests: a known model gets a list-price estimate, a routing
    // variant prices from its base model, and an unknown model stays unpriced.
    let dir = tempfile::tempdir().unwrap();
    let root = cli_root(&dir);
    let now = Utc::now();
    let timestamp = u64::try_from(now.timestamp()).unwrap();
    for (name, model) in [
        ("direct.db", "claude-sonnet-4-6"),
        ("routed.db", "claude-sonnet-4-6-Thinking"),
    ] {
        let conn = database_at(&root, name);
        insert_row(
            &conn,
            1,
            valid_turn_blob_with_model(100, timestamp, Some(model)),
        );
    }

    let SQLiteScan::Summary(summary) = summarize(std::slice::from_ref(&root), now, 30) else {
        panic!("supported databases should produce coverage");
    };
    // system prompt 11 + input 100, cache read 50, output 30 + reasoning 7.
    let per_request =
        crate::core::CostUsagePricing::claude_cost_usd("claude-sonnet-4-6", 111, 50, 0, 37)
            .expect("built-in public price");
    assert_eq!(summary.coverage, LocalHistoryCoverage::Complete);
    assert_eq!(summary.cost_estimate.coverage.estimated, 2);
    assert_eq!(summary.cost_estimate.coverage.unpriced, 0);
    assert_eq!(summary.total_usd(), Some(per_request * 2.0));

    let conn = database_at(&root, "unknown.db");
    insert_row(
        &conn,
        1,
        valid_turn_blob_with_model(100, timestamp, Some("fixture-unpriced")),
    );
    drop(conn);

    let SQLiteScan::Summary(summary) = summarize(std::slice::from_ref(&root), now, 30) else {
        panic!("supported databases should produce coverage");
    };
    assert_eq!(summary.cost_estimate.coverage.estimated, 2);
    assert_eq!(summary.cost_estimate.coverage.unpriced, 1);
    assert_eq!(summary.total_usd(), None);
    assert_eq!(
        summary.cost_estimate.known_subtotal_usd,
        Some(per_request * 2.0)
    );
    assert_eq!(
        summary
            .cost_estimate
            .unpriced_models
            .iter()
            .collect::<Vec<_>>(),
        ["fixture-unpriced"]
    );
}

/// Upstream 0.64 `AntigravityPricingRefreshTests` ("routine local reads do not
/// wait for pricing", "empty history starts no download"): a routine read
/// returns its scan as is and offers a background pricing refresh only when
/// the history records a model with no known public price.
#[test]
fn routine_read_offers_background_pricing_only_for_unpriced_history() {
    use crate::providers::antigravity::local_sessions::background_pricing_refresh;

    let now = Utc::now();
    let timestamp = u64::try_from(now.timestamp()).unwrap();
    // `None`: no database at all; `Some(None)`: a supported database without
    // rows; `Some(Some(model))`: one recorded request for `model`.
    let routine_read = |database: Option<Option<&str>>| {
        let dir = tempfile::tempdir().unwrap();
        let root = cli_root(&dir);
        fs::create_dir_all(&root).unwrap();
        if let Some(model) = database {
            let conn = database_at(&root, "one.db");
            if let Some(model) = model {
                insert_row(
                    &conn,
                    1,
                    valid_turn_blob_with_model(100, timestamp, Some(model)),
                );
            }
        }
        match summarize(&[root], now, 30) {
            SQLiteScan::Summary(summary) => summary,
            SQLiteScan::NoDatabases | SQLiteScan::Unsupported => {
                LocalTokenHistorySummary::default()
            }
        }
    };

    for empty in [routine_read(None), routine_read(Some(None))] {
        assert_eq!(empty.total_tokens, 0);
        assert!(empty.cost_estimate.unpriced_models.is_empty());
        assert!(background_pricing_refresh(&empty).is_none());
    }

    let known = routine_read(Some(Some("claude-sonnet-4-6")));
    assert_eq!(known.total_tokens, 198);
    assert!(known.total_usd().is_some());
    assert!(background_pricing_refresh(&known).is_none());

    let unknown = routine_read(Some(Some("gemini-fixture-unpriced")));
    assert_eq!(unknown.coverage, LocalHistoryCoverage::Complete);
    assert_eq!(unknown.total_tokens, 198);
    assert_eq!(unknown.total_usd(), None);
    assert!(background_pricing_refresh(&unknown).is_some());
}

fn inspect_schema(
    table: &str,
    conn: &Connection,
    budget: &mut Budget,
) -> rusqlite::Result<SchemaInspection> {
    let required: &[&str] = match table {
        "gen_metadata" => &["idx", "data"],
        _ => &["idx", "metadata"],
    };
    table_schema(conn, budget, table, required)
}

#[test]
fn schema_inspection_requires_one_stored_table_with_the_reader_columns() {
    use SchemaInspection::{Incomplete, Supported, Unsupported};
    for (table, column) in [("gen_metadata", "data"), ("steps", "metadata")] {
        let upper = (table.to_ascii_uppercase(), column.to_ascii_uppercase());
        let unrelated = (0..=MAX_SCHEMA_ENTRIES)
            .map(|index| format!("CREATE TABLE unrelated_{index}(value TEXT);"))
            .collect::<String>();
        let cases = [
            (
                format!("CREATE TABLE {table}(idx INTEGER, {column} BLOB);"),
                false,
                Supported,
            ),
            (
                format!("CREATE TABLE {}(IDX INTEGER, {} BLOB);", upper.0, upper.1),
                false,
                Supported,
            ),
            (
                format!("CREATE TABLE {table}(idx INTEGER);"),
                false,
                Unsupported,
            ),
            (
                format!("CREATE VIEW {table} AS SELECT 1 AS idx, x'00' AS {column};"),
                false,
                Unsupported,
            ),
            (
                format!("CREATE TABLE other(idx INTEGER, {column} BLOB);"),
                false,
                Unsupported,
            ),
            (String::new(), false, Unsupported),
            (
                format!(
                    "CREATE TABLE {table}(idx INTEGER, {column} BLOB, derived TEXT GENERATED ALWAYS AS (idx || 'x') VIRTUAL);"
                ),
                false,
                Unsupported,
            ),
            (
                format!("CREATE TABLE {table}(idx INTEGER, {column} BLOB);"),
                true,
                Incomplete,
            ),
            (unrelated, false, Incomplete),
        ];
        for (ddl, expired, expected) in cases {
            let conn = Connection::open_in_memory().unwrap();
            conn.execute_batch(&ddl).unwrap();
            let mut budget = if expired {
                Budget::with_deadline(Instant::now())
            } else {
                Budget::new()
            };
            assert_eq!(
                inspect_schema(table, &conn, &mut budget).unwrap(),
                expected,
                "{table}: {ddl:.80}"
            );
        }
    }
}

/// (complete, exhausted, budget rows, budget bytes, database bytes) after one read.
type RowCharge = (bool, bool, usize, usize, usize);
/// (rows as (idx, SQL value), preset (rows, bytes, database bytes), expected charge).
type RowCase<'a> = (&'a [(i64, &'a str)], (usize, usize, usize), RowCharge);

fn charge_rows(steps: bool, rows: &[(i64, &str)], preset: (usize, usize, usize)) -> RowCharge {
    let (table, column) = if steps {
        ("steps", "metadata")
    } else {
        ("gen_metadata", "data")
    };
    let conn = Connection::open_in_memory().unwrap();
    conn.execute(
        &format!("CREATE TABLE {table}(idx INTEGER, {column} BLOB)"),
        [],
    )
    .unwrap();
    for (idx, value) in rows {
        conn.execute(&format!("INSERT INTO {table} VALUES (?1, {value})"), [idx])
            .unwrap();
    }
    let (preset_rows, preset_bytes, preset_database_bytes) = preset;
    let mut budget = Budget::new();
    budget.rows = preset_rows;
    budget.bytes = preset_bytes;
    let (complete, database_bytes) = if steps {
        let mut database_bytes = preset_database_bytes;
        let scan =
            read_step_timestamps(&conn, &HashMap::new(), &mut budget, &mut database_bytes).unwrap();
        (scan.complete, database_bytes)
    } else {
        assert_eq!(
            preset_database_bytes, 0,
            "the generation reader starts at zero"
        );
        let parsed = read_generation_rows(&conn, "session", &mut budget).unwrap();
        (parsed.complete, parsed.database_bytes)
    };
    (
        complete,
        budget.exhausted,
        budget.rows,
        budget.bytes,
        database_bytes,
    )
}

#[test]
fn row_readers_skip_unusable_rows_before_charging_bytes() {
    let oversized = format!("zeroblob({})", MAX_BLOB_BYTES + 1);
    let near_full = MAX_TOTAL_BYTES - 5;
    for steps in [false, true] {
        let cases: [RowCase<'_>; 8] = [
            (
                &[(-1, "zeroblob(10)")],
                (0, MAX_TOTAL_BYTES, 0),
                (false, false, 1, MAX_TOTAL_BYTES, 0),
            ),
            (
                &[(0, "NULL")],
                (0, MAX_TOTAL_BYTES, 0),
                (false, false, 1, MAX_TOTAL_BYTES, 0),
            ),
            (
                &[(0, "'text'")],
                (0, MAX_TOTAL_BYTES, 0),
                (false, false, 1, MAX_TOTAL_BYTES, 0),
            ),
            (&[(0, "zeroblob(0)")], (0, 0, 0), (false, false, 1, 0, 0)),
            (
                &[(0, oversized.as_str())],
                (0, 0, 0),
                (false, false, 1, MAX_BLOB_BYTES + 1, MAX_BLOB_BYTES + 1),
            ),
            (
                &[(0, "zeroblob(10)")],
                (0, near_full, 0),
                (false, true, 1, near_full, 10),
            ),
            (
                &[(0, "zeroblob(4)"), (1, "zeroblob(10)")],
                (0, MAX_TOTAL_BYTES - 9, 0),
                (false, true, 2, MAX_TOTAL_BYTES - 5, 14),
            ),
            (
                &[(0, "zeroblob(10)")],
                (MAX_ROWS, 0, 0),
                (false, true, MAX_ROWS + 1, 0, 0),
            ),
        ];
        for (rows, preset, expected) in cases {
            assert_eq!(
                charge_rows(steps, rows, preset),
                expected,
                "steps={steps} {rows:?} {preset:?}"
            );
        }
    }
    // The steps reader carries the database total from the generation scan.
    let full = MAX_DATABASE_BYTES - 5;
    assert_eq!(
        charge_rows(true, &[(0, "zeroblob(10)")], (0, 0, full)),
        (false, true, 1, 0, full)
    );
    assert_eq!(
        charge_rows(true, &[(0, "zeroblob(5)")], (0, 0, full)),
        (false, false, 1, 5, MAX_DATABASE_BYTES)
    );

    let valid = valid_turn_blob(100, 1_785_700_000);
    let hex = valid
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let literal = format!("x'{hex}'");
    assert_eq!(
        charge_rows(false, &[(0, literal.as_str())], (0, 0, 0)),
        (true, false, 1, valid.len(), valid.len())
    );
}
