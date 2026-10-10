//! OpenCode Go local usage reader (SQLite).
//!
//! Mirrors upstream `OpenCodeGoLocalUsageReader`: sums `opencode-go` assistant
//! message / step-finish costs from the local OpenCode database and maps them
//! onto session ($12 / 5h), weekly ($30), and monthly ($60) windows.

use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, TimeZone, Timelike, Utc};
use rusqlite::Connection;
use std::path::{Path, PathBuf};

use super::tokens::{RowTokens, TokenSums};
use crate::core::{ProviderError, ProviderFetchResult};

const FIVE_HOURS_MS: i64 = 5 * 60 * 60 * 1000;
const WEEK_MS: i64 = 7 * 24 * 60 * 60 * 1000;
const SESSION_LIMIT_USD: f64 = 12.0;
const WEEKLY_LIMIT_USD: f64 = 30.0;
const MONTHLY_LIMIT_USD: f64 = 60.0;
const PROVIDER_ID: &str = "opencode-go";

const MESSAGE_USAGE_SQL: &str = r#"
SELECT
  CAST(COALESCE(json_extract(data, '$.time.created'), time_created) AS INTEGER) AS createdMs,
  CAST(json_extract(data, '$.cost') AS REAL) AS cost,
  1 AS requestCount,
  COALESCE(json_extract(data, '$.modelID'), '') AS modelID,
  CASE WHEN json_type(data, '$.tokens') = 'object'
    THEN json_extract(data, '$.tokens') END AS tokens
FROM message
WHERE json_valid(data)
  AND json_extract(data, '$.providerID') = 'opencode-go'
  AND json_extract(data, '$.role') = 'assistant'
  AND json_type(data, '$.cost') IN ('integer', 'real')
"#;

const MESSAGE_AND_PART_USAGE_SQL: &str = r#"
WITH provider_messages AS (
  SELECT
    id AS messageID,
    CAST(COALESCE(json_extract(data, '$.time.created'), time_created) AS INTEGER) AS createdMs,
    CAST(json_extract(data, '$.cost') AS REAL) AS cost,
    json_type(data, '$.cost') IN ('integer', 'real') AS hasCost,
    COALESCE(json_extract(data, '$.modelID'), '') AS modelID,
    CASE WHEN json_type(data, '$.tokens') = 'object'
      THEN json_extract(data, '$.tokens') END AS tokens
  FROM message
  WHERE json_valid(data)
    AND json_extract(data, '$.providerID') = 'opencode-go'
    AND json_extract(data, '$.role') = 'assistant'
)
SELECT
  CAST(COALESCE(json_extract(p.data, '$.time.created'), p.time_created, m.createdMs) AS INTEGER)
    AS createdMs,
  CAST(json_extract(p.data, '$.cost') AS REAL) AS cost,
  1 AS requestCount,
  m.modelID AS modelID,
  CASE WHEN json_type(p.data, '$.tokens') = 'object'
    THEN json_extract(p.data, '$.tokens') END AS tokens
FROM part p
JOIN provider_messages m ON m.messageID = p.message_id
WHERE json_valid(p.data)
  AND json_extract(p.data, '$.type') = 'step-finish'
  AND json_type(p.data, '$.cost') IN ('integer', 'real')
UNION ALL
SELECT createdMs, cost, 1 AS requestCount, modelID, tokens
FROM provider_messages m
WHERE hasCost
  AND NOT EXISTS (
    SELECT 1
    FROM part p
    WHERE p.message_id = m.messageID
      AND json_valid(p.data)
      AND json_extract(p.data, '$.type') = 'step-finish'
      AND json_type(p.data, '$.cost') IN ('integer', 'real')
  )
"#;

#[derive(Debug, Clone)]
pub(crate) struct UsageRow {
    created_ms: i64,
    cost: f64,
    /// One provider invocation per step-finish part; message-only databases fall back to one.
    request_count: u32,
    /// The underlying model behind the `opencode-go` Zen proxy; empty when unattributed.
    model: String,
    /// Recorded token counts; `None` when absent or unusable (never zero).
    tokens: Option<RowTokens>,
}

#[derive(Debug, Clone)]
pub struct LocalUsageSnapshot {
    pub rolling_usage_percent: f64,
    pub weekly_usage_percent: f64,
    pub monthly_usage_percent: f64,
    pub rolling_reset_in_sec: i64,
    pub weekly_reset_in_sec: i64,
    pub monthly_reset_in_sec: i64,
}

impl LocalUsageSnapshot {
    pub fn to_fetch_result(&self) -> ProviderFetchResult {
        let now = Utc::now();
        let at = |percent: f64, reset: i64| (percent, now + Duration::seconds(reset));
        let snap = super::go_snapshot(
            at(self.rolling_usage_percent, self.rolling_reset_in_sec),
            Some(at(self.weekly_usage_percent, self.weekly_reset_in_sec)),
            Some(at(self.monthly_usage_percent, self.monthly_reset_in_sec)),
        );
        // Upstream 0.51 (#2982): local SQLite quota reconstruction is useful
        // but it is not server-confirmed authority. Keep that distinction in
        // the data contract so CLI/React can present it without guessing.
        ProviderFetchResult::new(snap, super::LOCAL_ESTIMATE_SOURCE_LABEL)
            .with_non_authoritative_pace()
    }
}

/// Candidate (auth.json, opencode.db) pairs for local OpenCode installs.
pub fn candidate_paths() -> Vec<(PathBuf, PathBuf)> {
    let mut out = Vec::new();

    if let Ok(home) = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")) {
        let base = PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("opencode");
        out.push((base.join("auth.json"), base.join("opencode.db")));
    }

    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        let base = PathBuf::from(local).join("opencode");
        out.push((base.join("auth.json"), base.join("opencode.db")));
    }

    if let Some(home) = dirs::home_dir() {
        let base = home.join(".local").join("share").join("opencode");
        let pair = (base.join("auth.json"), base.join("opencode.db"));
        if !out.iter().any(|existing| existing.1 == pair.1) {
            out.push(pair);
        }
    }

    out
}

pub fn fetch_local_usage(now: DateTime<Utc>) -> Result<LocalUsageSnapshot, ProviderError> {
    let mut last_err: Option<ProviderError> = None;
    for (auth, db) in candidate_paths() {
        match fetch_from_paths(&auth, &db, now) {
            Ok(snap) => return Ok(snap),
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.unwrap_or_else(not_detected))
}

pub fn fetch_from_paths(
    auth_path: &Path,
    db_path: &Path,
    now: DateTime<Utc>,
) -> Result<LocalUsageSnapshot, ProviderError> {
    let has_auth = has_auth_key(auth_path);
    if !db_path.exists() {
        return Err(if has_auth {
            ProviderError::Other(
                "OpenCode Go local usage history is unavailable: database not found".into(),
            )
        } else {
            not_detected()
        });
    }

    let rows = read_rows(db_path)?;
    if !has_auth && rows.is_empty() {
        return Err(not_detected());
    }
    if rows.is_empty() {
        return Err(ProviderError::Other(
            "OpenCode Go local usage history is unavailable: no local usage rows".into(),
        ));
    }

    Ok(snapshot_from_rows(&rows, now))
}

fn not_detected() -> ProviderError {
    ProviderError::NotInstalled(
        "OpenCode Go not detected. Log in with OpenCode Go or use it locally first.".into(),
    )
}

fn sqlite_err(e: rusqlite::Error) -> ProviderError {
    ProviderError::Other(format!("SQLite error reading OpenCode Go usage: {e}"))
}

fn has_auth_key(path: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(path) else {
        return false;
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
        return false;
    };
    value
        .get(PROVIDER_ID)
        .and_then(|entry| entry.get("key"))
        .and_then(|key| key.as_str())
        .is_some_and(|key| !key.trim().is_empty())
}

fn read_rows(db_path: &Path) -> Result<Vec<UsageRow>, ProviderError> {
    let conn = open_readonly_connection(db_path)?;
    conn.busy_timeout(std::time::Duration::from_millis(250))
        .map_err(sqlite_err)?;

    let sql = if has_table(&conn, "part") {
        MESSAGE_AND_PART_USAGE_SQL
    } else {
        MESSAGE_USAGE_SQL
    };

    let mut stmt = conn.prepare(sql).map_err(sqlite_err)?;
    let rows = stmt
        .query_map([], |row| {
            Ok(UsageRow {
                created_ms: row.get::<_, i64>(0)?,
                cost: row.get::<_, f64>(1)?,
                // Request counts from the DB are clamped to ≥1 and realistically
                // far below u32::MAX; try_from guards the pathological case.
                request_count: row
                    .get::<_, i64>(2)
                    .map(|n| u32::try_from(n.max(1)).unwrap_or(u32::MAX))
                    .unwrap_or(1),
                model: row.get::<_, String>(3).unwrap_or_default(),
                tokens: row
                    .get::<_, Option<String>>(4)
                    .ok()
                    .flatten()
                    .and_then(|json| RowTokens::parse(&json)),
            })
        })
        .map_err(sqlite_err)?;

    let mut out = Vec::new();
    for row in rows {
        let row = row.map_err(sqlite_err)?;
        if row.created_ms > 0 && row.cost.is_finite() && row.cost >= 0.0 {
            out.push(row);
        }
    }
    Ok(out)
}

/// Open a read-only connection without creating `-wal`/`-shm` sidecars for idle
/// WAL-mode databases (upstream #2544).
fn open_readonly_connection(db_path: &Path) -> Result<Connection, ProviderError> {
    crate::core::open_readonly_sqlite_connection(db_path, std::time::Duration::from_millis(250))
        .map_err(sqlite_err)
}

fn has_table(conn: &Connection, name: &str) -> bool {
    conn.query_row(
        "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1 LIMIT 1",
        [name],
        |_| Ok(()),
    )
    .is_ok()
}

fn snapshot_from_rows(rows: &[UsageRow], now: DateTime<Utc>) -> LocalUsageSnapshot {
    let now_ms = now.timestamp_millis();
    let session_start = now_ms - FIVE_HOURS_MS;
    let week_start_ms = start_of_utc_iso_week_ms(now);
    let week_end_ms = week_start_ms + WEEK_MS;
    let earliest_ms = rows.iter().map(|r| r.created_ms).min();
    let (month_start_ms, month_end_ms) = month_bounds_ms(now, earliest_ms);

    let mut session_cost = 0.0;
    let mut weekly_cost = 0.0;
    let mut monthly_cost = 0.0;
    let mut oldest_session_ms: Option<i64> = None;

    for row in rows {
        if row.created_ms >= session_start && row.created_ms < now_ms {
            session_cost += row.cost;
            oldest_session_ms = Some(match oldest_session_ms {
                Some(prev) => prev.min(row.created_ms),
                None => row.created_ms,
            });
        }
        if row.created_ms >= week_start_ms && row.created_ms < week_end_ms {
            weekly_cost += row.cost;
        }
        if row.created_ms >= month_start_ms && row.created_ms < month_end_ms {
            monthly_cost += row.cost;
        }
    }

    let oldest = oldest_session_ms.unwrap_or(now_ms);
    let rolling_reset_in_sec = ((oldest + FIVE_HOURS_MS - now_ms) / 1000).max(0);

    LocalUsageSnapshot {
        rolling_usage_percent: percent(session_cost, SESSION_LIMIT_USD),
        weekly_usage_percent: percent(weekly_cost, WEEKLY_LIMIT_USD),
        monthly_usage_percent: percent(monthly_cost, MONTHLY_LIMIT_USD),
        rolling_reset_in_sec,
        weekly_reset_in_sec: ((week_end_ms - now_ms) / 1000).max(0),
        monthly_reset_in_sec: ((month_end_ms - now_ms) / 1000).max(0),
    }
}

fn percent(used: f64, limit: f64) -> f64 {
    if !used.is_finite() || limit <= 0.0 {
        return 0.0;
    }
    let value = (used / limit * 100.0).clamp(0.0, 100.0);
    (value * 10.0).round() / 10.0
}

/// Bucket label for rows whose local `modelID` is missing or blank (upstream #2649).
const UNKNOWN_MODEL_NAME: &str = "unknown";

/// One (day, model) cost bucket for the daily per-model breakdown (upstream #2649).
///
/// Mirrors `CostUsageDailyReport.ModelBreakdown` plus the day key, so the shared
/// cost-history chart can render OpenCode Go the same way it renders Codex/Claude
/// without a bespoke chart surface. Entries are sorted by `(day_key, model)`.
#[derive(Debug, Clone, PartialEq)]
pub struct DailyModelCost {
    /// `yyyy-MM-dd` local calendar day (matches Codex/Claude cost-history keying).
    pub day_key: String,
    /// Trimmed model id, or `UNKNOWN_MODEL_NAME` when the row had none.
    pub model: String,
    /// Cost in USD accumulated for this (day, model) bucket.
    pub cost: f64,
    /// Number of provider invocations (step-finish parts, or one per message).
    pub request_count: u32,
    /// Recorded token sums for this bucket; incomplete when any row lacked usable tokens.
    pub tokens: TokenSums,
}

/// Provider-local cost summary reusing the shared `CostSummary` fields the chart
/// already consumes (`total_cost_usd`, `by_model`, `period_start/end`). Built
/// from the same local rows as the daily breakdown so the two surfaces agree.
#[derive(Debug, Clone, Default)]
pub struct ModelCostSummary {
    pub total_cost_usd: f64,
    pub by_model: std::collections::HashMap<String, f64>,
    pub request_count: u32,
    /// Recorded token sums across the window (costs never derive from these).
    pub tokens: TokenSums,
    /// Recorded token sums per trimmed model id, keyed like `by_model`.
    pub by_model_tokens: std::collections::HashMap<String, TokenSums>,
    pub period_start: Option<NaiveDate>,
    pub period_end: Option<NaiveDate>,
}

/// Local calendar-day key (`yyyy-MM-dd`) for a UTC millisecond timestamp,
/// matching how Codex/Claude cost history is keyed.
fn day_key_local(ms: i64) -> Option<String> {
    Local
        .timestamp_millis_opt(ms)
        .single()
        .map(|dt| dt.date_naive().format("%Y-%m-%d").to_string())
}

/// Local "today" derived from a UTC instant, so the day window is deterministic
/// under test rather than snapping to wall-clock `Local::now()`.
fn local_today_from_utc(now: DateTime<Utc>) -> NaiveDate {
    Local.from_utc_datetime(&now.naive_utc()).date_naive()
}

/// Local midnight that opens a `days`-long window ending today.
fn window_since_ms(now: DateTime<Utc>, days: u32) -> i64 {
    let clamped = crate::cost_reporting_period::clamp_window_days(days);
    let since = local_today_from_utc(now) - Duration::days(clamped as i64 - 1);
    Local
        .from_local_datetime(&since.and_hms_opt(0, 0, 0).unwrap_or_default())
        .single()
        .map(|dt| dt.timestamp_millis())
        .unwrap_or(0)
}

/// Trimmed model id, with blanks bucketed as `UNKNOWN_MODEL_NAME`.
fn model_name(raw: &str) -> &str {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        UNKNOWN_MODEL_NAME
    } else {
        trimmed
    }
}

/// Group rows into `(day, model)` cost buckets (upstream #2649).
///
/// Rows outside the `[since, now]` window are dropped; model ids are trimmed and
/// blanks collapse to `UNKNOWN_MODEL_NAME`. The result is sorted by
/// `(day_key, model)` for deterministic ordering.
pub fn daily_model_costs(
    rows: &[UsageRow],
    now: DateTime<Utc>,
    history_days: u32,
) -> Vec<DailyModelCost> {
    let since_ms = window_since_ms(now, history_days);
    let now_ms = now.timestamp_millis();

    let mut by_day_model: std::collections::BTreeMap<
        String,
        std::collections::BTreeMap<String, (f64, u32, TokenSums)>,
    > = std::collections::BTreeMap::new();
    for row in rows {
        if row.created_ms < since_ms || row.created_ms > now_ms {
            continue;
        }
        let Some(key) = day_key_local(row.created_ms) else {
            continue;
        };
        let model = model_name(&row.model);
        let entry = by_day_model.entry(key).or_default();
        let bucket = entry.entry(model.to_string()).or_default();
        bucket.0 += row.cost;
        bucket.1 = bucket.1.saturating_add(row.request_count);
        bucket.2.add(row.tokens.as_ref());
    }

    let mut out = Vec::new();
    for (day_key, models) in by_day_model {
        for (model, (cost, request_count, tokens)) in models {
            out.push(DailyModelCost {
                day_key: day_key.clone(),
                model,
                cost,
                request_count,
                tokens,
            });
        }
    }
    out
}

/// Build the provider-local cost summary for the last `days` days.
pub fn model_cost_summary_from_rows(
    rows: &[UsageRow],
    now: DateTime<Utc>,
    days: u32,
) -> ModelCostSummary {
    let since_ms = window_since_ms(now, days);
    let now_ms = now.timestamp_millis();

    let mut total = 0.0;
    let mut request_count = 0u32;
    let mut by_model: std::collections::HashMap<String, f64> = std::collections::HashMap::new();
    let mut tokens = TokenSums::default();
    let mut by_model_tokens: std::collections::HashMap<String, TokenSums> =
        std::collections::HashMap::new();
    let mut earliest: Option<NaiveDate> = None;
    let mut latest: Option<NaiveDate> = None;
    for row in rows {
        if row.created_ms < since_ms || row.created_ms > now_ms {
            continue;
        }
        total += row.cost;
        request_count = request_count.saturating_add(row.request_count);
        let model = model_name(&row.model);
        *by_model.entry(model.to_string()).or_insert(0.0) += row.cost;
        tokens.add(row.tokens.as_ref());
        by_model_tokens
            .entry(model.to_string())
            .or_default()
            .add(row.tokens.as_ref());
        if let Some(day) = day_key_local(row.created_ms)
            .and_then(|k| NaiveDate::parse_from_str(&k, "%Y-%m-%d").ok())
        {
            earliest = Some(earliest.map(|e| e.min(day)).unwrap_or(day));
            latest = Some(latest.map(|l| l.max(day)).unwrap_or(day));
        }
    }
    ModelCostSummary {
        total_cost_usd: total,
        by_model,
        request_count,
        tokens,
        by_model_tokens,
        period_start: earliest,
        period_end: latest,
    }
}

/// Per-day cost series (`yyyy-MM-dd`, cost USD) for the shared cost-history chart,
/// summed across all models. Reads the first available local OpenCode install.
/// Empty when no install is detected.
pub fn daily_cost_series(now: DateTime<Utc>, history_days: u32) -> Vec<(String, f64)> {
    let Some(rows) = read_available_rows() else {
        return Vec::new();
    };
    let buckets = daily_model_costs(&rows, now, history_days);
    let mut by_day: std::collections::BTreeMap<String, f64> = std::collections::BTreeMap::new();
    for b in &buckets {
        *by_day.entry(b.day_key.clone()).or_insert(0.0) += b.cost;
    }
    by_day.into_iter().collect()
}

/// Provider-local cost summary for the chart's local-usage panel. `None` when no
/// local OpenCode install is detected.
pub fn model_cost_summary_scan(now: DateTime<Utc>, days: u32) -> Option<ModelCostSummary> {
    let rows = read_available_rows()?;
    Some(model_cost_summary_from_rows(&rows, now, days))
}

/// Read usage rows from the first candidate install that yields any. Returns
/// `None` when no install is reachable (auth+db both absent) rather than
/// propagating `NotInstalled`, since the cost surfaces treat "no data" as empty.
fn read_available_rows() -> Option<Vec<UsageRow>> {
    for (auth, db) in candidate_paths() {
        if !db.exists() {
            continue;
        }
        match read_rows(&db) {
            Ok(rows) if !rows.is_empty() || has_auth_key(&auth) => return Some(rows),
            Ok(_) => continue,
            Err(_) => continue,
        }
    }
    None
}

/// ISO week start (Monday 00:00 UTC), matching upstream calendar settings.
fn start_of_utc_iso_week_ms(now: DateTime<Utc>) -> i64 {
    let date = now.date_naive();
    let days_from_monday = date.weekday().num_days_from_monday() as i64;
    let monday = date - Duration::days(days_from_monday);
    Utc.from_utc_datetime(&monday.and_hms_opt(0, 0, 0).unwrap_or_default())
        .timestamp_millis()
}

fn month_bounds_ms(now: DateTime<Utc>, anchor_ms: Option<i64>) -> (i64, i64) {
    let Some(anchor_ms) = anchor_ms else {
        let start = NaiveDate::from_ymd_opt(now.year(), now.month(), 1)
            .unwrap_or_else(|| now.date_naive())
            .and_hms_opt(0, 0, 0)
            .unwrap_or_default();
        let start_dt = Utc.from_utc_datetime(&start);
        let (end_year, end_month) = next_month(now.year(), now.month());
        let end_dt = Utc
            .with_ymd_and_hms(end_year, end_month, 1, 0, 0, 0)
            .single()
            .unwrap_or(start_dt);
        return (start_dt.timestamp_millis(), end_dt.timestamp_millis());
    };

    let anchor = DateTime::<Utc>::from_timestamp_millis(anchor_ms).unwrap_or(now);
    let mut year = now.year();
    let mut month = now.month();
    let mut start = anchored_month(year, month, &anchor);
    if start > now {
        if month == 1 {
            year -= 1;
            month = 12;
        } else {
            month -= 1;
        }
        start = anchored_month(year, month, &anchor);
    }
    let (end_year, end_month) = next_month(year, month);
    let end = anchored_month(end_year, end_month, &anchor);
    (start.timestamp_millis(), end.timestamp_millis())
}

fn next_month(year: i32, month: u32) -> (i32, u32) {
    if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    }
}

fn anchored_month(year: i32, month: u32, anchor: &DateTime<Utc>) -> DateTime<Utc> {
    let day = anchor.day();
    let (hour, min, sec, nano) = (
        anchor.hour(),
        anchor.minute(),
        anchor.second(),
        anchor.nanosecond(),
    );
    if let Some(date) = NaiveDate::from_ymd_opt(year, month, day)
        && let Some(ndt) = date.and_hms_nano_opt(hour, min, sec, nano)
    {
        return Utc.from_utc_datetime(&ndt);
    }
    // Clamp to last day of month when anchor day overflows (e.g. 31 → Feb).
    let last_day = NaiveDate::from_ymd_opt(year, month, 1)
        .map(|d| {
            let (following_year, following_month) = next_month(year, month);
            NaiveDate::from_ymd_opt(following_year, following_month, 1).unwrap_or(d)
                - Duration::days(1)
        })
        .map(|d| d.day())
        .unwrap_or(28);
    let date = NaiveDate::from_ymd_opt(year, month, last_day).unwrap_or_else(|| {
        NaiveDate::from_ymd_opt(year, month, 1).unwrap_or_else(|| Utc::now().date_naive())
    });
    let ndt = date
        .and_hms_nano_opt(hour, min, sec, nano)
        .or_else(|| date.and_hms_opt(0, 0, 0))
        .unwrap_or_default();
    Utc.from_utc_datetime(&ndt)
}

#[cfg(test)]
mod test_db;
#[cfg(test)]
mod tokens_tests;

#[cfg(test)]
mod tests;
