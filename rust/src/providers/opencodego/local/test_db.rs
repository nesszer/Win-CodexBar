//! Synthetic OpenCode SQLite fixtures shared by the local reader tests.

use rusqlite::Connection;
use std::path::Path;

pub(super) fn iso_ms(iso: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(iso)
        .unwrap()
        .timestamp_millis()
}

pub(super) fn open_db(path: &Path, with_part_table: bool) -> Connection {
    let conn = Connection::open(path).unwrap();
    conn.execute_batch(
        "CREATE TABLE message (id TEXT PRIMARY KEY, data TEXT, time_created INTEGER);",
    )
    .unwrap();
    if with_part_table {
        conn.execute_batch(
            "CREATE TABLE part (id TEXT PRIMARY KEY, message_id TEXT, data TEXT, time_created INTEGER);",
        )
        .unwrap();
    }
    conn
}

/// Insert one `opencode-go` assistant message. `tokens` is spliced in as raw
/// JSON so malformed shapes can be exercised.
pub(super) fn insert_message(
    conn: &Connection,
    id: &str,
    created_ms: i64,
    cost: Option<f64>,
    model: Option<&str>,
    tokens: Option<&str>,
) {
    insert_message_for(conn, id, created_ms, cost, model, "opencode-go", tokens);
}

/// [`insert_message`] attributed to `provider_id`.
pub(super) fn insert_message_for(
    conn: &Connection,
    id: &str,
    created_ms: i64,
    cost: Option<f64>,
    model: Option<&str>,
    provider_id: &str,
    tokens: Option<&str>,
) {
    let cost_json = cost.map_or_else(String::new, |c| format!(r#","cost":{c}"#));
    let model_json = model.map_or_else(String::new, |m| format!(r#","modelID":"{m}""#));
    let tokens_json = tokens.map_or_else(String::new, |t| format!(r#","tokens":{t}"#));
    let data = format!(
        r#"{{"providerID":"{provider_id}","role":"assistant"{cost_json}{model_json}{tokens_json},"time":{{"created":{created_ms}}}}}"#
    );
    conn.execute(
        "INSERT INTO message (id, data, time_created) VALUES (?1, ?2, ?3)",
        rusqlite::params![id, data, created_ms],
    )
    .unwrap();
}

/// Add the OpenCode v2 `session_message` table with its upstream column layout.
pub(super) fn create_session_message_table(conn: &Connection) {
    conn.execute_batch(
        "CREATE TABLE session_message (
          id TEXT PRIMARY KEY,
          session_id TEXT NOT NULL,
          type TEXT NOT NULL,
          seq INTEGER NOT NULL,
          time_created INTEGER NOT NULL,
          time_updated INTEGER NOT NULL,
          data TEXT NOT NULL
        );",
    )
    .unwrap();
}

/// Insert one v2 `session_message` row. `tokens` is spliced in as raw JSON.
pub(super) fn insert_session_message(
    conn: &Connection,
    id: &str,
    created_ms: i64,
    cost: f64,
    model: &str,
    provider_id: &str,
    tokens: Option<&str>,
) {
    let tokens_json = tokens.map_or_else(String::new, |t| format!(r#","tokens":{t}"#));
    let data = format!(
        r#"{{"model":{{"providerID":"{provider_id}","id":"{model}"}},"cost":{cost}{tokens_json},"time":{{"created":{created_ms}}}}}"#
    );
    conn.execute(
        "INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data)
         VALUES (?1, 'session-1', 'assistant', 0, ?2, ?2, ?3)",
        rusqlite::params![id, created_ms, data],
    )
    .unwrap();
}

/// Insert a raw v2 `session_message` row with any `type` and payload, including malformed JSON.
pub(super) fn insert_session_message_raw(
    conn: &Connection,
    id: &str,
    ty: &str,
    created_ms: i64,
    data: &str,
) {
    conn.execute(
        "INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data)
         VALUES (?1, 'session-1', ?2, 0, ?3, ?3, ?4)",
        rusqlite::params![id, ty, created_ms, data],
    )
    .unwrap();
}

/// Insert a step-finish part carrying a cost, attached to `message_id`.
pub(super) fn insert_step_finish(
    conn: &Connection,
    id: &str,
    message_id: &str,
    created_ms: i64,
    cost: f64,
    tokens: Option<&str>,
) {
    let tokens_json = tokens.map_or_else(String::new, |t| format!(r#","tokens":{t}"#));
    let data = format!(
        r#"{{"type":"step-finish","cost":{cost}{tokens_json},"time":{{"created":{created_ms}}}}}"#
    );
    conn.execute(
        "INSERT INTO part (id, message_id, data, time_created) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![id, message_id, data, created_ms],
    )
    .unwrap();
}

/// Message-only DB with one assistant row per `(created_ms, cost, modelID)`.
pub(super) fn write_message_db(path: &Path, rows: &[(i64, f64, Option<&str>)]) {
    let conn = open_db(path, false);
    for (i, (created_ms, cost, model)) in rows.iter().enumerate() {
        insert_message(
            &conn,
            &format!("m{i}"),
            *created_ms,
            Some(*cost),
            *model,
            None,
        );
    }
}
