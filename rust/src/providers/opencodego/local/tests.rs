use super::test_db::{
    create_session_message_table, insert_message, insert_session_message,
    insert_session_message_raw, insert_step_finish, iso_ms, open_db, write_message_db,
};
use super::*;
use crate::core::RateWindow;
use chrono::Weekday;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn local_fetch_result_keeps_estimate_source_and_reset_windows() {
    let result = LocalUsageSnapshot {
        rolling_usage_percent: 12.0,
        weekly_usage_percent: 23.0,
        monthly_usage_percent: 34.0,
        rolling_reset_in_sec: 300,
        weekly_reset_in_sec: 1_000,
        monthly_reset_in_sec: 2_000,
    }
    .to_fetch_result();

    assert_eq!(
        result.source_label,
        super::super::LOCAL_ESTIMATE_SOURCE_LABEL
    );
    assert!(result.quota_estimated);
    assert!(!result.pace_authoritative);
    assert_eq!(result.usage.primary.used_percent, 12.0);
    assert_eq!(result.usage.secondary.as_ref().unwrap().used_percent, 23.0);
    assert_eq!(result.usage.tertiary.as_ref().unwrap().used_percent, 34.0);
    assert!(result.usage.primary.resets_at.is_some());
    assert!(result.usage.secondary.as_ref().unwrap().resets_at.is_some());
    assert!(result.usage.tertiary.as_ref().unwrap().resets_at.is_some());
}

#[test]
fn local_fetch_result_pins_window_minutes_and_reset_offsets() {
    let before = Utc::now();
    let result = LocalUsageSnapshot {
        rolling_usage_percent: 12.0,
        weekly_usage_percent: 23.0,
        monthly_usage_percent: 34.0,
        rolling_reset_in_sec: 300,
        weekly_reset_in_sec: 1_000,
        monthly_reset_in_sec: 2_000,
    }
    .to_fetch_result();
    let after = Utc::now();
    let usage = &result.usage;
    assert_eq!(usage.login_method.as_deref(), Some("OpenCode Go"));
    let windows = [
        (&usage.primary, 300, Some(300)),
        (usage.secondary.as_ref().unwrap(), 1_000, Some(10080)),
        (usage.tertiary.as_ref().unwrap(), 2_000, None),
    ];
    for (window, reset_in, minutes) in windows {
        let resets_at = window.resets_at.unwrap();
        assert!(resets_at >= before + Duration::seconds(reset_in));
        assert!(resets_at <= after + Duration::seconds(reset_in));
        let expected = minutes.or(RateWindow::monthly_window_minutes(Some(resets_at)));
        assert_eq!(window.window_minutes, expected);
        assert!(window.reset_description.is_none());
    }
    assert!(usage.extra_rate_windows.is_empty());
}

fn temp_db_path(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("opencodego-local-{label}-{nanos}.db"))
}

#[test]
fn cached_scan_tracks_live_wal_commits() {
    let db = temp_db_path("wal-cache");
    let auth = db.with_extension("auth.json");
    std::fs::write(&auth, r#"{"opencode-go":{"key":"k"}}"#).unwrap();
    let now = Utc.with_ymd_and_hms(2026, 3, 18, 12, 0, 0).unwrap();
    let created = now.timestamp_millis() - 60_000;

    // The writer stays open so commits land in the -wal sidecar, not the main file.
    let writer = Connection::open(&db).unwrap();
    writer.execute_batch("PRAGMA journal_mode=WAL;").unwrap();
    writer
        .execute_batch(
            "CREATE TABLE message (id TEXT PRIMARY KEY, data TEXT, time_created INTEGER);",
        )
        .unwrap();
    insert_message(&writer, "m1", created, Some(3.0), None, None);

    let cache = ScanCache::new();
    let first = fetch_from_paths_with_cache(&auth, &db, now, &cache).unwrap();
    assert!(
        (first.rolling_usage_percent - 25.0).abs() < 0.05,
        "{first:?}"
    );
    let cached = cache.read(&db, || -> Result<Vec<UsageRow>, ProviderError> {
        panic!("unchanged database must not be rescanned")
    });
    assert_eq!(cached.unwrap().len(), 1);

    insert_message(&writer, "m2", created, Some(3.0), None, None);
    let second = fetch_from_paths_with_cache(&auth, &db, now, &cache).unwrap();
    assert!(
        (second.rolling_usage_percent - 50.0).abs() < 0.05,
        "a WAL commit must invalidate the cached scan: {second:?}"
    );
}

#[test]
fn unreadable_database_reports_the_sqlite_error_prefix() {
    let db = temp_db_path("garbage");
    std::fs::write(&db, b"not a sqlite database, just synthetic bytes").unwrap();
    let err = read_rows(&db, RowSource::OpenCodeGo).unwrap_err();
    // Best-effort teardown; the temp file may already be gone.
    let _removed = std::fs::remove_file(&db);
    assert!(
        matches!(&err, ProviderError::Other(message)
            if message.starts_with("SQLite error reading OpenCode Go usage: ")),
        "{err:?}"
    );
}

#[test]
fn not_detected_without_db_or_auth() {
    let dir = std::env::temp_dir().join(format!(
        "opencodego-missing-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    // Best-effort fixture setup; a failure surfaces as the fetch error below.
    let _created = std::fs::create_dir_all(&dir);
    let auth = dir.join("auth.json");
    let db = dir.join("opencode.db");
    let err = fetch_from_paths(&auth, &db, Utc::now()).unwrap_err();
    assert!(matches!(
        &err,
        ProviderError::NotInstalled(message)
            if message == "OpenCode Go not detected. Log in with OpenCode Go or use it locally first."
    ));
    // Best-effort teardown; temp dir may already be gone.
    let _removed_dir = std::fs::remove_dir_all(&dir);
}

#[test]
fn sums_session_weekly_monthly_costs() {
    let db = temp_db_path("sums");
    let now = Utc.with_ymd_and_hms(2026, 3, 18, 12, 0, 0).unwrap(); // Wednesday
    let now_ms = now.timestamp_millis();
    // $6 in the rolling 5h window → 50% of $12
    // $15 in ISO week → 50% of $30
    // $30 in anchored month → 50% of $60
    let session_ms = now_ms - 60_000;
    let week_ms = start_of_utc_iso_week_ms(now) + 3_600_000;
    let month_anchor_ms = now_ms - 10 * 24 * 60 * 60 * 1000;
    write_message_db(
        &db,
        &[
            (session_ms, 6.0, None),
            (week_ms, 9.0, None), // plus session = 15 in week if session also in week
            (month_anchor_ms, 15.0, None),
        ],
    );

    // auth present so empty-rows path is not used; auth not required when rows exist
    let auth = db.with_extension("auth.json");
    // Fixture write; failure would make fetch_from_paths return an error.
    let _written = std::fs::write(&auth, r#"{"opencode-go":{"key":"test-key"}}"#);

    let snap = fetch_from_paths(&auth, &db, now).unwrap();
    assert!((snap.rolling_usage_percent - 50.0).abs() < 0.05, "{snap:?}");
    // session 6 + week-only 9 = 15 → 50%
    assert!((snap.weekly_usage_percent - 50.0).abs() < 0.05, "{snap:?}");
    // session 6 + week 9 + month 15 = 30 → 50%
    assert!((snap.monthly_usage_percent - 50.0).abs() < 0.05, "{snap:?}");

    // Best-effort teardown; leftover temp files are harmless.
    let _removed_db = std::fs::remove_file(&db);
    let _removed_auth = std::fs::remove_file(&auth);
}

#[test]
fn prefers_step_finish_parts_when_present() {
    let db = temp_db_path("parts");
    let conn = open_db(&db, true);
    let now = Utc.with_ymd_and_hms(2026, 3, 18, 12, 0, 0).unwrap();
    let created = now.timestamp_millis() - 1_000;
    // Message cost would be $12 (100%), but step-finish parts sum to $3 (25%).
    insert_message(&conn, "m1", created, Some(12.0), None, None);
    insert_step_finish(&conn, "p1", "m1", created, 3.0, None);
    drop(conn);

    let auth = db.with_extension("auth.json");
    // Fixture write; a failure would surface in fetch_from_paths.
    let _written_auth = std::fs::write(&auth, r#"{"opencode-go":{"key":"k"}}"#);
    let snap = fetch_from_paths(&auth, &db, now).unwrap();
    assert!(
        (snap.rolling_usage_percent - 25.0).abs() < 0.05,
        "expected step-finish cost only, got {snap:?}"
    );
    // Best-effort teardown; leftover temp files are harmless.
    let _removed_db = std::fs::remove_file(&db);
    let _removed_auth = std::fs::remove_file(&auth);
}

#[test]
fn percent_rounds_to_one_decimal() {
    assert!((percent(1.0, 12.0) - 8.3).abs() < 0.05);
    assert_eq!(percent(0.0, 12.0), 0.0);
    assert_eq!(percent(f64::NAN, 12.0), 0.0);
}

#[test]
fn month_bounds_follow_calendar_or_anchor_day() {
    let at = |iso: &str| {
        DateTime::parse_from_rfc3339(iso)
            .unwrap()
            .with_timezone(&Utc)
    };
    let cases = [
        // (now, anchor, expected start, expected end)
        (
            "2026-12-15T12:00:00Z",
            None,
            "2026-12-01T00:00:00Z",
            "2027-01-01T00:00:00Z",
        ),
        (
            "2026-03-18T12:00:00Z",
            None,
            "2026-03-01T00:00:00Z",
            "2026-04-01T00:00:00Z",
        ),
        // Day 31 clamps to Feb 28, which is after now, so the window steps back.
        (
            "2026-02-20T10:00:00Z",
            Some("2026-01-31T08:30:00Z"),
            "2026-01-31T08:30:00Z",
            "2026-02-28T08:30:00Z",
        ),
        (
            "2026-12-20T00:00:00Z",
            Some("2026-03-15T00:00:00Z"),
            "2026-12-15T00:00:00Z",
            "2027-01-15T00:00:00Z",
        ),
        (
            "2026-01-10T00:00:00Z",
            Some("2025-11-15T06:00:00Z"),
            "2025-12-15T06:00:00Z",
            "2026-01-15T06:00:00Z",
        ),
        (
            "2026-11-10T12:00:00Z",
            Some("2026-08-31T00:00:00Z"),
            "2026-10-31T00:00:00Z",
            "2026-11-30T00:00:00Z",
        ),
    ];
    for (now, anchor, start, end) in cases {
        let bounds = month_bounds_ms(at(now), anchor.map(|a| at(a).timestamp_millis()));
        assert_eq!(
            bounds,
            (at(start).timestamp_millis(), at(end).timestamp_millis()),
            "{now} {anchor:?}"
        );
    }
}

#[test]
fn daily_and_summary_windows_share_the_local_midnight_cutoff() {
    let now = a14_now();
    let since = local_today_from_utc(now) - Duration::days(1);
    let since_ms = Local
        .from_local_datetime(&since.and_hms_opt(0, 0, 0).unwrap())
        .single()
        .unwrap()
        .timestamp_millis();
    let row = |created_ms: i64, model: &str| UsageRow {
        created_ms,
        cost: 1.0,
        request_count: 1,
        model: model.to_string(),
        tokens: None,
    };
    let rows = [
        row(since_ms - 1, "dropped"),
        row(since_ms, " kept "),
        row(now.timestamp_millis(), ""),
        row(now.timestamp_millis() + 1, "future"),
    ];
    let daily = daily_model_costs(&rows, now, 2);
    let models: Vec<_> = daily.iter().map(|b| b.model.as_str()).collect();
    let mut sorted = models.clone();
    sorted.sort_unstable();
    assert_eq!(sorted, ["kept", UNKNOWN_MODEL_NAME]);
    let summary = model_cost_summary_from_rows(&rows, now, 2);
    assert_eq!(summary.request_count, 2);
    assert!((summary.total_cost_usd - 2.0).abs() < 1e-9);
    let mut keys: Vec<_> = summary.by_model.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(keys, ["kept", UNKNOWN_MODEL_NAME]);
}

#[test]
fn iso_week_starts_monday_utc() {
    // 2026-03-18 is a Wednesday; week start should be 2026-03-16 00:00 UTC.
    let wed = Utc.with_ymd_and_hms(2026, 3, 18, 15, 0, 0).unwrap();
    let start = start_of_utc_iso_week_ms(wed);
    let expected = Utc
        .with_ymd_and_hms(2026, 3, 16, 0, 0, 0)
        .unwrap()
        .timestamp_millis();
    assert_eq!(start, expected);
    assert_eq!(wed.weekday(), Weekday::Wed);
}

#[test]
fn idle_wal_mode_read_creates_no_sidecars() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("opencode.db");
    let auth = dir.path().join("auth.json");
    std::fs::write(&auth, r#"{"opencode-go":{"key":"k"}}"#).unwrap();

    // Build a WAL-mode DB, insert a row, leave journal_mode=WAL, then drop
    // any writer-created sidecars so the main file is an idle WAL header.
    {
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch("PRAGMA journal_mode=WAL;").unwrap();
        conn.execute_batch(
            "CREATE TABLE message (
                id TEXT PRIMARY KEY,
                data TEXT,
                time_created INTEGER
            );",
        )
        .unwrap();
        let now = Utc::now();
        let created = now.timestamp_millis() - 1_000;
        let data = format!(
            r#"{{"providerID":"opencode-go","role":"assistant","cost":3,"time":{{"created":{created}}}}}"#
        );
        conn.execute(
            "INSERT INTO message (id, data, time_created) VALUES ('m1', ?1, ?2)",
            rusqlite::params![data, created],
        )
        .unwrap();
        // Truncate empties WAL content before close; journal_mode stays WAL.
        // Best-effort checkpoint; a truncated WAL is just tidier.
        let _checkpointed = conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()));
        drop(conn);
    }

    // Ensure idle-WAL case: header says WAL, no live sidecars.
    let wal = crate::core::sqlite_sidecar_path(&db, "-wal");
    let shm = crate::core::sqlite_sidecar_path(&db, "-shm");
    // Prefer rename-away over delete if OS still holds handles.
    for side in [&wal, &shm] {
        if side.exists() {
            let parked = side.with_extension("parked");
            // Best-effort parking; a locked sidecar just stays put.
            let _parked = std::fs::rename(side, parked);
        }
    }
    assert!(!wal.exists(), "precondition: no -wal");
    assert!(!shm.exists(), "precondition: no -shm");

    let snap = fetch_from_paths(&auth, &db, Utc::now()).expect("read idle WAL db");
    assert!(snap.rolling_usage_percent > 0.0, "{snap:?}");

    assert!(
        !wal.exists() && !shm.exists(),
        "reader must not create -wal/-shm sidecars"
    );
}

// ---- A14: per-model daily cost breakdown (upstream #2649) -------------

fn a14_now() -> DateTime<Utc> {
    Utc.timestamp_opt(1_772_798_400, 0).unwrap()
}

fn a14_now_afternoon() -> DateTime<Utc> {
    Utc.timestamp_opt(1_772_798_400 + 4 * 3600, 0).unwrap()
}

#[test]
fn daily_entries_group_cost_by_model_within_a_day() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("opencode.db");
    let now = a14_now_afternoon();
    write_message_db(
        &db,
        &[
            (
                iso_ms("2026-03-06T11:00:00.000Z"),
                3.0,
                Some("claude-sonnet-4-5"),
            ),
            (
                iso_ms("2026-03-06T12:00:00.000Z"),
                2.0,
                Some("gpt-5.1-codex"),
            ),
            (
                iso_ms("2026-03-06T13:00:00.000Z"),
                1.0,
                Some("claude-sonnet-4-5"),
            ),
        ],
    );
    let rows = read_rows(&db, RowSource::OpenCodeGo).unwrap();
    let buckets = daily_model_costs(&rows, now, 30);

    // Day key is local-calendar; assert on tz-independent model aggregation.
    let total: f64 = buckets.iter().map(|b| b.cost).sum();
    assert!((total - 6.0).abs() < 1e-6, "total {total}");
    assert_eq!(buckets.iter().map(|b| b.request_count).sum::<u32>(), 3);
    let by_model: std::collections::HashMap<&str, f64> =
        buckets.iter().map(|b| (b.model.as_str(), b.cost)).collect();
    assert!((by_model["claude-sonnet-4-5"] - 4.0).abs() < 1e-6);
    assert!((by_model["gpt-5.1-codex"] - 2.0).abs() < 1e-6);
}

#[test]
fn v2_session_messages_add_new_turns_without_double_counting_migrated_ones() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("opencode.db");
    let conn = open_db(&db, false);
    create_session_message_table(&conn);
    let migrated_ms = iso_ms("2026-03-06T11:00:00.000Z");
    // Migrated turn: present in both tables under one id, counted once via the legacy row.
    insert_message(
        &conn,
        "migrated",
        migrated_ms,
        Some(3.0),
        Some("claude-sonnet-4-5"),
        None,
    );
    insert_session_message(
        &conn,
        "migrated",
        migrated_ms,
        3.0,
        "claude-sonnet-4-5",
        "opencode-go",
        None,
    );
    // New v2-only opencode-go turn with recorded tokens: 10 + 5 + 1 + 4 + 0 = 20.
    insert_session_message(
        &conn,
        "v2-new",
        iso_ms("2026-03-06T12:00:00.000Z"),
        2.0,
        "gpt-5.1-codex",
        "opencode-go",
        Some(r#"{"input":10,"output":5,"reasoning":1,"cache":{"read":4,"write":0}}"#),
    );
    // Other providers never count toward OpenCode Go.
    insert_session_message(
        &conn,
        "v2-other",
        iso_ms("2026-03-06T13:00:00.000Z"),
        7.0,
        "gpt-5.1-codex",
        "openrouter",
        None,
    );
    drop(conn);

    let now = a14_now_afternoon();
    let rows = read_rows(&db, RowSource::OpenCodeGo).unwrap();
    assert_eq!(rows.len(), 2, "{rows:?}");
    let v2_new = rows
        .iter()
        .find(|row| row.model == "gpt-5.1-codex")
        .expect("v2 turn row");
    assert_eq!(v2_new.tokens.as_ref().map(|t| t.total), Some(20));

    let buckets = daily_model_costs(&rows, now, 30);
    let total: f64 = buckets.iter().map(|b| b.cost).sum();
    assert!((total - 5.0).abs() < 1e-6, "total {total}");
    assert_eq!(buckets.iter().map(|b| b.request_count).sum::<u32>(), 2);
    let by_model: std::collections::HashMap<&str, f64> =
        buckets.iter().map(|b| (b.model.as_str(), b.cost)).collect();
    assert!((by_model["claude-sonnet-4-5"] - 3.0).abs() < 1e-6);
    assert!((by_model["gpt-5.1-codex"] - 2.0).abs() < 1e-6);
}

#[test]
fn v2_session_messages_join_step_finish_parts_path_without_double_counting() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("opencode.db");
    // A `part` table selects the step-finish legacy arm alongside `session_message`.
    let conn = open_db(&db, true);
    create_session_message_table(&conn);
    let migrated_ms = iso_ms("2026-03-06T11:00:00.000Z");
    // Migrated turn: the $12 message cost must be ignored in favor of the $3 step-finish part,
    // and the v2 copy under the same id must not be counted again.
    insert_message(
        &conn,
        "migrated",
        migrated_ms,
        Some(12.0),
        Some("claude-sonnet-4-5"),
        None,
    );
    insert_step_finish(&conn, "p-migrated", "migrated", migrated_ms, 3.0, None);
    insert_session_message(
        &conn,
        "migrated",
        migrated_ms,
        3.0,
        "claude-sonnet-4-5",
        "opencode-go",
        None,
    );
    // Legacy turn without parts: counted once from its message cost.
    insert_message(
        &conn,
        "legacy",
        iso_ms("2026-03-06T10:00:00.000Z"),
        Some(1.0),
        Some("claude-sonnet-4-5"),
        None,
    );
    // v2-only turn: must be added.
    insert_session_message(
        &conn,
        "v2-new",
        iso_ms("2026-03-06T12:00:00.000Z"),
        2.0,
        "gpt-5.1-codex",
        "opencode-go",
        None,
    );
    drop(conn);

    let rows = read_rows(&db, RowSource::OpenCodeGo).unwrap();
    assert_eq!(rows.len(), 3, "{rows:?}");

    let now = a14_now_afternoon();
    let buckets = daily_model_costs(&rows, now, 30);
    let total: f64 = buckets.iter().map(|b| b.cost).sum();
    assert!((total - 6.0).abs() < 1e-6, "total {total}");
    assert_eq!(buckets.iter().map(|b| b.request_count).sum::<u32>(), 3);
    let by_model: std::collections::HashMap<&str, f64> =
        buckets.iter().map(|b| (b.model.as_str(), b.cost)).collect();
    assert!((by_model["claude-sonnet-4-5"] - 4.0).abs() < 1e-6);
    assert!((by_model["gpt-5.1-codex"] - 2.0).abs() < 1e-6);
}

#[test]
fn malformed_v2_session_messages_are_ignored_without_failing_the_read() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("opencode.db");
    let conn = open_db(&db, false);
    create_session_message_table(&conn);
    let created = iso_ms("2026-03-06T12:00:00.000Z");
    let insert_raw = |id: &str, ty: &str, data: &str| {
        conn.execute(
            "INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data)
             VALUES (?1, 'session-1', ?2, 0, ?3, ?3, ?4)",
            rusqlite::params![id, ty, created, data],
        )
        .unwrap();
    };
    insert_raw(
        "good",
        "assistant",
        &format!(
            r#"{{"model":{{"providerID":"opencode-go","id":"good-model"}},"cost":4,"time":{{"created":{created}}}}}"#
        ),
    );
    // Missing $.model.providerID.
    insert_raw(
        "no-provider",
        "assistant",
        &format!(r#"{{"model":{{"id":"x"}},"cost":5,"time":{{"created":{created}}}}}"#),
    );
    // Non-numeric cost (string).
    insert_raw(
        "string-cost",
        "assistant",
        &format!(
            r#"{{"model":{{"providerID":"opencode-go","id":"x"}},"cost":"5","time":{{"created":{created}}}}}"#
        ),
    );
    // Invalid JSON in data.
    insert_raw(
        "bad-json",
        "assistant",
        r#"{"model":{"providerID":"opencode-go""#,
    );
    // Not an assistant turn.
    insert_raw(
        "user-turn",
        "user",
        &format!(
            r#"{{"model":{{"providerID":"opencode-go","id":"x"}},"cost":6,"time":{{"created":{created}}}}}"#
        ),
    );
    drop(conn);

    let rows = read_rows(&db, RowSource::OpenCodeGo)
        .expect("malformed v2 rows must not fail the whole read");
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].model, "good-model");
    assert!((rows[0].cost - 4.0).abs() < 1e-6);
}

/// Noon local time on the day the migrated-copy fixture rows are anchored to (upstream #4587).
fn v2_history_now() -> DateTime<Utc> {
    Local
        .with_ymd_and_hms(2026, 3, 6, 12, 0, 0)
        .single()
        .unwrap()
        .with_timezone(&Utc)
}

#[test]
fn v2_only_database_without_message_table_reads_requests() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("opencode.db");
    let conn = Connection::open(&db).unwrap();
    create_session_message_table(&conn);
    insert_session_message_raw(
        &conn,
        "msg_go",
        "assistant",
        1_772_794_800_000,
        r#"{"agent":"build","model":{"providerID":"opencode-go","id":"fixture-model"},"content":[],"cost":3,"time":{"created":1772794800000}}"#,
    );
    drop(conn);

    // No auth.json and no legacy `message` table: v2 history alone must still produce usage.
    let auth = dir.path().join("auth.json");
    let now = a14_now();
    let snap = fetch_from_paths(&auth, &db, now).expect("v2-only history must read");
    assert!((snap.rolling_usage_percent - 25.0).abs() < 0.05, "{snap:?}");

    let rows = read_rows(&db, RowSource::OpenCodeGo).unwrap();
    assert_eq!(rows.len(), 1, "{rows:?}");
    let buckets = daily_model_costs(&rows, now, 30);
    assert_eq!(buckets.len(), 1, "{buckets:?}");
    assert_eq!(buckets[0].model, "fixture-model");
    assert!((buckets[0].cost - 3.0).abs() < 1e-6);
    assert_eq!(buckets[0].request_count, 1);
}

#[test]
fn legacy_copies_count_once_and_new_v2_turns_are_added() {
    for has_parts in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("opencode.db");
        let conn = open_db(&db, has_parts);
        create_session_message_table(&conn);
        let day_start_ms = Local
            .with_ymd_and_hms(2026, 3, 6, 0, 0, 0)
            .single()
            .unwrap()
            .timestamp_millis();
        let before = day_start_ms - 60_000;
        let after = day_start_ms + 60_000;
        insert_message(&conn, "msg_old", before, Some(4.0), None, None);
        insert_message(&conn, "msg_migrated", before, Some(3.0), None, None);
        if has_parts {
            // Step-finish parts take precedence over the migrated message's own cost.
            insert_step_finish(&conn, "prt_1", "msg_migrated", before, 1.0, None);
            insert_step_finish(&conn, "prt_2", "msg_migrated", after, 2.0, None);
        }
        insert_session_message(
            &conn,
            "msg_migrated",
            before,
            3.0,
            "fixture-model",
            "opencode-go",
            None,
        );
        insert_session_message(
            &conn,
            "msg_new",
            after,
            5.0,
            "fixture-model",
            "opencode-go",
            None,
        );
        drop(conn);

        let rows = read_rows(&db, RowSource::OpenCodeGo).unwrap();
        let buckets = daily_model_costs(&rows, v2_history_now(), 30);
        let mut by_day: std::collections::BTreeMap<String, (f64, u32)> =
            std::collections::BTreeMap::new();
        for bucket in &buckets {
            let entry = by_day.entry(bucket.day_key.clone()).or_default();
            entry.0 += bucket.cost;
            entry.1 += bucket.request_count;
        }
        let days: Vec<(f64, u32)> = by_day.values().copied().collect();
        assert_eq!(days.len(), 2, "has_parts={has_parts}: {buckets:?}");
        let (before_cost, after_cost) = if has_parts { (5.0, 7.0) } else { (7.0, 5.0) };
        assert!(
            (days[0].0 - before_cost).abs() < 1e-6,
            "has_parts={has_parts}: {days:?}"
        );
        assert!(
            (days[1].0 - after_cost).abs() < 1e-6,
            "has_parts={has_parts}: {days:?}"
        );
        let requests: u32 = days.iter().map(|day| day.1).sum();
        assert_eq!(
            requests,
            if has_parts { 4 } else { 3 },
            "has_parts={has_parts}"
        );
    }
}

#[test]
fn invalid_legacy_copies_do_not_hide_valid_v2_requests() {
    for has_legacy in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("opencode.db");
        let conn = open_db(&db, false);
        create_session_message_table(&conn);
        let created = 1_772_794_800_000;
        if has_legacy {
            // A legacy copy with a negative cost is unusable and must not hide the v2 request.
            insert_message(&conn, "msg_go", created, Some(-3.0), None, None);
        }
        insert_session_message(
            &conn,
            "msg_go",
            created,
            3.0,
            "fixture-model",
            "opencode-go",
            None,
        );
        insert_session_message_raw(
            &conn,
            "msg_zero",
            "assistant",
            created,
            r#"{"model":{"providerID":"opencode-go"},"cost":0}"#,
        );
        let invalid_payloads = [
            "{",
            r#"{"model":{"providerID":"opencode"},"cost":99}"#,
            r#"{"model":{"providerID":"another-provider"},"cost":99}"#,
            r#"{"providerID":"opencode-go","role":"assistant","cost":99}"#,
            r#"{"model":{"providerID":"opencode-go"},"cost":"99"}"#,
            r#"{"model":{"providerID":"opencode-go"},"cost":null}"#,
            r#"{"model":{"providerID":"opencode-go"}}"#,
            r#"{"model":{"providerID":"opencode-go"},"cost":-1}"#,
            r#"{"model":{"providerID":"opencode-go"},"cost":1e999}"#,
        ];
        for (index, data) in invalid_payloads.iter().enumerate() {
            insert_session_message_raw(
                &conn,
                &format!("msg_invalid_{index}"),
                "assistant",
                created,
                data,
            );
        }
        insert_session_message_raw(
            &conn,
            "msg_user",
            "user",
            created,
            r#"{"model":{"providerID":"opencode-go"},"cost":99}"#,
        );
        drop(conn);

        let now = a14_now();
        let rows = read_rows(&db, RowSource::OpenCodeGo).unwrap();
        assert_eq!(rows.len(), 2, "has_legacy={has_legacy}: {rows:?}");
        let buckets = daily_model_costs(&rows, now, 30);
        assert_eq!(buckets.len(), 2, "has_legacy={has_legacy}: {buckets:?}");
        assert_eq!(buckets[0].model, "fixture-model");
        assert!((buckets[0].cost - 3.0).abs() < 1e-6);
        assert_eq!(buckets[0].request_count, 1);
        assert_eq!(buckets[1].model, UNKNOWN_MODEL_NAME);
        assert!(buckets[1].cost.abs() < 1e-6);
        assert_eq!(buckets[1].request_count, 1);
    }
}

#[test]
fn legacy_copy_with_unusable_timestamp_does_not_hide_valid_v2_request() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("opencode.db");
    let conn = open_db(&db, false);
    create_session_message_table(&conn);
    // No `time.created` and `time_created` 0 gives a legacy createdMs of 0, which is unusable.
    conn.execute(
        "INSERT INTO message (id, data, time_created) VALUES ('msg_clock', ?1, 0)",
        rusqlite::params![r#"{"providerID":"opencode-go","role":"assistant","cost":3}"#],
    )
    .unwrap();
    insert_session_message(
        &conn,
        "msg_clock",
        1_772_794_800_000,
        3.0,
        "fixture-model",
        "opencode-go",
        None,
    );
    drop(conn);

    let rows = read_rows(&db, RowSource::OpenCodeGo).unwrap();
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].model, "fixture-model");
    assert!((rows[0].cost - 3.0).abs() < 1e-6);
}

#[test]
fn idle_v2_wal_history_reads_without_changing_the_database_or_creating_sidecars() {
    for has_legacy in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("opencode.db");
        {
            let conn = Connection::open(&db).unwrap();
            conn.execute_batch("PRAGMA journal_mode=WAL;").unwrap();
            create_session_message_table(&conn);
            if has_legacy {
                conn.execute_batch(
                    "CREATE TABLE message (id TEXT PRIMARY KEY, data TEXT NOT NULL, time_created INTEGER);",
                )
                .unwrap();
            }
            insert_session_message(
                &conn,
                "msg_go",
                1_772_794_800_000,
                3.0,
                "fixture-model",
                "opencode-go",
                None,
            );
            conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))
                .unwrap();
            drop(conn);
        }
        // Idle WAL: the writer's checkpointed sidecars are gone. Park them rather than delete.
        for suffix in ["-wal", "-shm"] {
            let side = crate::core::sqlite_sidecar_path(&db, suffix);
            if side.exists() {
                let parked = PathBuf::from(format!("{}.parked", side.display()));
                std::fs::rename(&side, parked).unwrap();
            }
        }
        let wal = crate::core::sqlite_sidecar_path(&db, "-wal");
        let shm = crate::core::sqlite_sidecar_path(&db, "-shm");
        assert!(
            !wal.exists() && !shm.exists(),
            "precondition: no sidecars (has_legacy={has_legacy})"
        );
        let contents = std::fs::read(&db).unwrap();

        let rows = read_rows(&db, RowSource::OpenCodeGo).expect("read idle v2 WAL history");
        assert_eq!(rows.len(), 1, "has_legacy={has_legacy}: {rows:?}");
        assert!((rows[0].cost - 3.0).abs() < 1e-6);

        assert_eq!(
            std::fs::read(&db).unwrap(),
            contents,
            "reader must not modify the database"
        );
        assert!(
            !wal.exists() && !shm.exists(),
            "reader must not create -wal/-shm sidecars (has_legacy={has_legacy})"
        );
    }
}

#[test]
fn v2_compaction_requests_count_with_their_cost_and_tokens() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("opencode.db");
    let conn = open_db(&db, false);
    create_session_message_table(&conn);
    insert_session_message_raw(
        &conn,
        "msg_compaction",
        "compaction",
        1_772_794_800_000,
        r#"{"status":"completed","reason":"auto","summary":"Summary","recent":"",
           "model":{"providerID":"opencode-go","id":"fixture-model"},"cost":2,
           "tokens":{"input":2,"output":1,"reasoning":0,"cache":{"read":0,"write":0}}}"#,
    );
    drop(conn);

    let rows = read_rows(&db, RowSource::OpenCodeGo).unwrap();
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].model, "fixture-model");
    assert!((rows[0].cost - 2.0).abs() < 1e-6);
    assert_eq!(rows[0].tokens.as_ref().map(|t| t.total), Some(3));

    let buckets = daily_model_costs(&rows, a14_now(), 30);
    assert_eq!(buckets.len(), 1, "{buckets:?}");
    assert!((buckets[0].cost - 2.0).abs() < 1e-6);
    assert_eq!(buckets[0].request_count, 1);
}

#[test]
fn step_finish_parts_inherit_their_model_from_the_parent_message() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("opencode.db");
    let conn = open_db(&db, true);
    let created = iso_ms("2026-03-06T11:00:00.000Z");
    insert_message(&conn, "m1", created, None, Some("grok-code-fast-1"), None);
    insert_step_finish(&conn, "p1", "m1", created, 3.0, None);
    drop(conn);

    let rows = read_rows(&db, RowSource::OpenCodeGo).unwrap();
    let buckets = daily_model_costs(&rows, a14_now(), 30);
    assert_eq!(buckets.len(), 1, "{buckets:?}");
    assert_eq!(buckets[0].model, "grok-code-fast-1");
    assert!((buckets[0].cost - 3.0).abs() < 1e-6);
}

#[test]
fn messages_without_a_model_fall_back_to_the_unknown_bucket() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("opencode.db");
    write_message_db(&db, &[(iso_ms("2026-03-06T11:00:00.000Z"), 4.0, None)]);
    let rows = read_rows(&db, RowSource::OpenCodeGo).unwrap();
    let buckets = daily_model_costs(&rows, a14_now(), 30);
    assert_eq!(buckets.len(), 1, "{buckets:?}");
    assert_eq!(buckets[0].model, UNKNOWN_MODEL_NAME);
    assert!((buckets[0].cost - 4.0).abs() < 1e-6);
}

#[test]
fn whitespace_only_model_ids_fall_back_to_the_unknown_bucket() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("opencode.db");
    write_message_db(
        &db,
        &[(iso_ms("2026-03-06T11:00:00.000Z"), 5.0, Some("   "))],
    );
    let rows = read_rows(&db, RowSource::OpenCodeGo).unwrap();
    let buckets = daily_model_costs(&rows, a14_now(), 30);
    assert_eq!(buckets.len(), 1, "{buckets:?}");
    assert_eq!(buckets[0].model, UNKNOWN_MODEL_NAME);
    assert!((buckets[0].cost - 5.0).abs() < 1e-6);
}

#[test]
fn model_ids_with_incidental_whitespace_merge_with_the_trimmed_bucket() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("opencode.db");
    write_message_db(
        &db,
        &[
            (
                iso_ms("2026-03-06T11:00:00.000Z"),
                2.0,
                Some("claude-sonnet-4-5"),
            ),
            (
                iso_ms("2026-03-06T12:00:00.000Z"),
                3.0,
                Some("  claude-sonnet-4-5  "),
            ),
        ],
    );
    let rows = read_rows(&db, RowSource::OpenCodeGo).unwrap();
    let buckets = daily_model_costs(&rows, a14_now_afternoon(), 30);
    assert_eq!(buckets.len(), 1, "{buckets:?}");
    assert_eq!(buckets[0].model, "claude-sonnet-4-5");
    assert!((buckets[0].cost - 5.0).abs() < 1e-6);
    assert_eq!(buckets[0].request_count, 2);
}

#[test]
fn multiple_days_bucket_separately_and_sort_deterministically() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("opencode.db");
    write_message_db(
        &db,
        &[
            (iso_ms("2026-03-05T11:00:00.000Z"), 1.0, Some("a")),
            (iso_ms("2026-03-06T11:00:00.000Z"), 2.0, Some("b")),
            (iso_ms("2026-03-07T11:00:00.000Z"), 3.0, Some("a")),
        ],
    );
    let rows = read_rows(&db, RowSource::OpenCodeGo).unwrap();
    let buckets = daily_model_costs(&rows, a14_now_afternoon(), 30);
    assert!(
        buckets
            .windows(2)
            .all(|w| (w[0].day_key.as_str(), w[0].model.as_str())
                <= (w[1].day_key.as_str(), w[1].model.as_str())),
        "not sorted: {buckets:?}"
    );
    assert!(
        buckets
            .iter()
            .map(|b| b.day_key.as_str())
            .collect::<std::collections::HashSet<_>>()
            .len()
            >= 2
    );
}

#[test]
fn zero_cost_rows_are_kept_and_aggregated() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("opencode.db");
    write_message_db(
        &db,
        &[
            (iso_ms("2026-03-06T11:00:00.000Z"), 0.0, Some("a")),
            (iso_ms("2026-03-06T12:00:00.000Z"), 4.0, Some("a")),
        ],
    );
    let rows = read_rows(&db, RowSource::OpenCodeGo).unwrap();
    let buckets = daily_model_costs(&rows, a14_now_afternoon(), 30);
    assert_eq!(buckets.len(), 1, "{buckets:?}");
    assert!((buckets[0].cost - 4.0).abs() < 1e-6);
    assert_eq!(buckets[0].request_count, 2);
}

#[test]
fn malformed_rows_are_dropped() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("opencode.db");
    let conn = open_db(&db, false);
    conn.execute(
        "INSERT INTO message (id, data, time_created) VALUES (?1, ?2, ?3)",
        rusqlite::params![
            "m1",
            r#"{"providerID":"opencode-go","role":"user","cost":9,"time":{"created":1772798400000}}"#,
            1772798400000i64
        ],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO message (id, data, time_created) VALUES (?1, ?2, ?3)",
        rusqlite::params![
            "m2",
            r#"{"providerID":"opencode-go","role":"assistant","cost":null,"modelID":"x","time":{"created":1772798400000}}"#,
            1772798400000i64
        ],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO message (id, data, time_created) VALUES (?1, ?2, ?3)",
        rusqlite::params![
            "m3",
            r#"{"providerID":"opencode-go","role":"assistant","cost":7,"modelID":"good","time":{"created":1772798400000}}"#,
            1772798400000i64
        ],
    )
    .unwrap();
    drop(conn);

    let rows = read_rows(&db, RowSource::OpenCodeGo).unwrap();
    assert_eq!(rows.len(), 1, "only the valid assistant+cost row survives");
    assert_eq!(rows[0].model, "good");
}

#[test]
fn rows_outside_history_window_are_dropped() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("opencode.db");
    let far_past = iso_ms("2025-01-01T00:00:00.000Z");
    let recent = a14_now().timestamp_millis();
    write_message_db(
        &db,
        &[(far_past, 1.0, Some("old")), (recent, 2.0, Some("new"))],
    );
    let rows = read_rows(&db, RowSource::OpenCodeGo).unwrap();
    let buckets = daily_model_costs(&rows, a14_now(), 1);
    let models: Vec<&str> = buckets.iter().map(|b| b.model.as_str()).collect();
    assert!(
        !models.contains(&"old"),
        "old row should be outside the 1-day window: {buckets:?}"
    );
}

#[test]
fn day_boundary_keys_by_local_calendar_day() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("opencode.db");
    let just_after_utc_midnight = iso_ms("2026-03-06T00:30:00.000Z");
    write_message_db(&db, &[(just_after_utc_midnight, 1.5, Some("edge"))]);
    let rows = read_rows(&db, RowSource::OpenCodeGo).unwrap();
    let buckets = daily_model_costs(&rows, a14_now_afternoon(), 30);
    assert_eq!(buckets.len(), 1, "{buckets:?}");
    assert!(
        NaiveDate::parse_from_str(&buckets[0].day_key, "%Y-%m-%d").is_ok(),
        "day_key not yyyy-MM-dd: {}",
        buckets[0].day_key
    );
}

#[test]
fn model_cost_summary_aggregates_total_and_by_model() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("opencode.db");
    write_message_db(
        &db,
        &[
            (iso_ms("2026-03-06T11:00:00.000Z"), 3.0, Some("a")),
            (iso_ms("2026-03-06T12:00:00.000Z"), 1.0, Some("b")),
            (iso_ms("2026-03-06T13:00:00.000Z"), 2.0, None),
        ],
    );
    let rows = read_rows(&db, RowSource::OpenCodeGo).unwrap();
    let summary = model_cost_summary_from_rows(&rows, a14_now_afternoon(), 30);
    assert!((summary.total_cost_usd - 6.0).abs() < 1e-6, "{summary:?}");
    assert_eq!(summary.request_count, 3);
    assert!((summary.by_model["a"] - 3.0).abs() < 1e-6);
    assert!((summary.by_model["b"] - 1.0).abs() < 1e-6);
    assert!((summary.by_model[UNKNOWN_MODEL_NAME] - 2.0).abs() < 1e-6);
    assert!(summary.period_start.is_some() && summary.period_end.is_some());
}

#[test]
fn daily_series_sums_models_per_day_via_pure_aggregation() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("opencode.db");
    write_message_db(
        &db,
        &[
            (iso_ms("2026-03-06T11:00:00.000Z"), 3.0, Some("a")),
            (iso_ms("2026-03-06T12:00:00.000Z"), 2.0, Some("b")),
        ],
    );
    let rows = read_rows(&db, RowSource::OpenCodeGo).unwrap();
    let buckets = daily_model_costs(&rows, a14_now_afternoon(), 30);
    let mut by_day: std::collections::BTreeMap<String, f64> = std::collections::BTreeMap::new();
    for b in &buckets {
        *by_day.entry(b.day_key.clone()).or_insert(0.0) += b.cost;
    }
    let series: Vec<(String, f64)> = by_day.into_iter().collect();
    assert_eq!(series.len(), 1, "{series:?}");
    assert!((series[0].1 - 5.0).abs() < 1e-6);
}

#[test]
fn daily_aggregation_is_independent_of_zen_wait() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("opencode.db");
    write_message_db(&db, &[(iso_ms("2026-03-06T11:00:00.000Z"), 2.0, Some("a"))]);
    let rows = read_rows(&db, RowSource::OpenCodeGo).unwrap();
    let now = a14_now();
    let b1 = daily_model_costs(&rows, now, 30);
    let b2 = daily_model_costs(&rows, now, 30);
    assert_eq!(b1, b2, "pure aggregation must be deterministic");
    assert_eq!(b1.len(), 1);
}
