#[path = "local_bot_id.rs"]
mod local_bot_id;

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration as StdDuration, Instant};

use chrono::{DateTime, Duration, Local, TimeZone, Utc};
use rusqlite::{Connection, OpenFlags, TransactionBehavior, types::ValueRef};

use self::local_bot_id::{ExactStepTimestamp, embedded_timestamps_agree, record_exact_bot_id};
use super::cost::estimate_cost_usd;
use super::local_proto::{ParsedTurn, parse_step_metadata, parse_turn};
use super::local_step_resolver::{StepOccurrence, resolve_step_timestamps};
#[cfg(test)]
use crate::spend_contract::LocalTokenHistorySummary as LocalSessionSummary;
use crate::spend_contract::{LocalHistoryCoverage, LocalTokenHistorySummary};

const MAX_DATABASES: usize = 500;
const MAX_DIRECTORY_ENTRIES: usize = 10_000;
const MAX_ROWS_PER_DATABASE: usize = 10_000;
const MAX_ROWS: usize = 50_000;
const MAX_BLOB_BYTES: usize = 16 * 1024 * 1024;
const MAX_DATABASE_BYTES: usize = 64 * 1024 * 1024;
const MAX_TOTAL_BYTES: usize = 128 * 1024 * 1024;
const MAX_SCHEMA_ENTRIES: usize = 128;
const MAX_SCHEMA_COLUMNS: usize = 64;
const MAX_SCHEMA_BYTES: usize = 64 * 1024;
const MAX_SCAN_DURATION: StdDuration = StdDuration::from_secs(5);

#[derive(Debug)]
pub(super) enum SQLiteScan {
    NoDatabases,
    /// Discovered SQLite files were present, but none had an Antigravity schema.
    ///
    /// This is non-authoritative: callers may continue with another local
    /// history source instead of treating the scan as known-empty history.
    Unsupported,
    Summary(LocalTokenHistorySummary),
}

#[derive(Debug)]
enum DatabaseScan {
    Supported { events: Vec<Event>, complete: bool },
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SchemaInspection {
    Supported,
    Unsupported,
    Incomplete,
}

struct Budget {
    directory_entries: usize,
    databases: usize,
    rows: usize,
    bytes: usize,
    schema_bytes: usize,
    deadline: Instant,
}

impl Budget {
    fn new() -> Self {
        Self::with_deadline(Instant::now() + MAX_SCAN_DURATION)
    }

    fn with_deadline(deadline: Instant) -> Self {
        Self {
            directory_entries: 0,
            databases: 0,
            rows: 0,
            bytes: 0,
            schema_bytes: 0,
            deadline,
        }
    }

    fn check(&self) -> bool {
        Instant::now() < self.deadline
    }

    fn charge_schema_text(&mut self, value: &str) -> bool {
        let Some(next) = self.schema_bytes.checked_add(value.len()) else {
            return false;
        };
        self.schema_bytes = next;
        next <= MAX_SCHEMA_BYTES
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Event {
    session: String,
    row: i64,
    turn: ParsedTurn,
    total: u64,
}

#[derive(Debug)]
struct PendingTimestampRow {
    row: i64,
    step_uuid: String,
    turn: ParsedTurn,
    total: u64,
}

#[derive(Debug)]
struct ParsedRows {
    events: Vec<Event>,
    pending: Vec<PendingTimestampRow>,
    occurrences: HashMap<String, Vec<StepOccurrence>>,
    bot_id_uses: HashMap<String, usize>,
    database_bytes: usize,
    complete: bool,
}

#[derive(Debug)]
struct StepTimestampScan {
    timestamps: HashMap<String, Vec<StepOccurrence>>,
    by_bot_id: HashMap<String, ExactStepTimestamp>,
    ambiguous_bot_ids: HashSet<String>,
    unidentified_rows_present: bool,
    complete: bool,
}

pub(super) fn database_roots(gemini_base: &Path) -> [PathBuf; 3] {
    [
        gemini_base.join("antigravity-cli").join("conversations"),
        gemini_base.join("antigravity"),
        gemini_base.join("antigravity").join("conversations"),
    ]
}

pub(super) fn summarize(roots: &[PathBuf], now: DateTime<Utc>, days: u32) -> SQLiteScan {
    let mut budget = Budget::new();
    let (paths, discovery_complete) = discover_databases(roots, &mut budget);
    if paths.is_empty() && discovery_complete {
        return SQLiteScan::NoDatabases;
    }

    let first_day = now.with_timezone(&Local).date_naive()
        - Duration::days(i64::from(days.clamp(1, 365).saturating_sub(1)));
    let mut complete = discovery_complete && budget.check();
    let mut events = Vec::new();
    let mut authoritative_database = false;
    let mut unsupported_database = false;

    for path in &paths {
        if !budget.check() {
            complete = false;
            break;
        }
        budget.databases += 1;
        if budget.databases > MAX_DATABASES {
            complete = false;
            break;
        }
        match read_database(path, &mut budget) {
            Ok(DatabaseScan::Supported {
                events: mut database_events,
                complete: database_complete,
            }) => {
                authoritative_database = true;
                events.append(&mut database_events);
                complete &= database_complete;
            }
            Ok(DatabaseScan::Unsupported) => unsupported_database = true,
            Err(_) => {
                // Preserve existing source-specific read failures as partial
                // native history rather than treating them as foreign files.
                authoritative_database = true;
                complete = false;
            }
        }
        complete &= budget.check();
        if budget.rows >= MAX_ROWS || budget.bytes >= MAX_TOTAL_BYTES {
            complete = false;
            break;
        }
    }

    if unsupported_database && !authoritative_database && complete {
        return SQLiteScan::Unsupported;
    }

    let mut total_tokens = 0_u64;
    let mut cost_estimate = crate::spend_contract::LocalCostEstimate::default();
    let mut sessions = HashSet::new();
    let mut rows: HashMap<(String, i64), Event> = HashMap::new();
    let mut responses: HashMap<(String, String), Event> = HashMap::new();

    let mut label_models = HashMap::<(String, String), String>::new();
    let mut conflicting_labels = HashSet::<(String, String)>::new();
    for event in &events {
        let (Some(label), Some(model)) = (event.turn.label.as_ref(), event.turn.model.as_ref())
        else {
            continue;
        };
        let key = (event.session.clone(), label.clone());
        if label_models.get(&key).is_some_and(|prior| prior != model) {
            conflicting_labels.insert(key);
        } else {
            label_models.insert(key, model.clone());
        }
    }

    for event in events {
        let row_key = (event.session.clone(), event.row);
        if let Some(prior) = rows.get(&row_key) {
            if prior != &event {
                complete = false;
            }
            continue;
        }

        if let Some(response_id) = event
            .turn
            .usage
            .as_ref()
            .and_then(|usage| usage.response_id.as_ref())
        {
            let response_key = (event.session.clone(), response_id.clone());
            if let Some(prior) = responses.get(&response_key) {
                if prior.turn != event.turn {
                    complete = false;
                } else {
                    rows.insert(row_key, event);
                }
                continue;
            }
            responses.insert(response_key, event.clone());
        }

        let Some(timestamp_ms) = event.turn.timestamp_ms else {
            complete = false;
            continue;
        };
        let Some(at) = Utc.timestamp_millis_opt(timestamp_ms).single() else {
            complete = false;
            continue;
        };
        rows.insert(row_key, event.clone());
        if at > now || at.with_timezone(&Local).date_naive() < first_day {
            continue;
        }
        match total_tokens.checked_add(event.total) {
            Some(total) => total_tokens = total,
            None => {
                complete = false;
                continue;
            }
        }
        if event.total > 0 {
            let estimated_cost = event.turn.usage.as_ref().and_then(|usage| {
                let inherited_model = event.turn.label.as_ref().and_then(|label| {
                    let key = (event.session.clone(), label.clone());
                    (!conflicting_labels.contains(&key))
                        .then(|| label_models.get(&key))
                        .flatten()
                        .map(String::as_str)
                });
                let model = event.turn.model.as_deref().or(inherited_model);
                let input = usage.system_prompt.checked_add(usage.new_input);
                let output = usage.output.checked_add(usage.reasoning);
                if let (Some(input), Some(output)) = (input, output) {
                    estimate_cost_usd(model, input, usage.cache_read, 0, output)
                } else {
                    None
                }
            });
            cost_estimate.record_list_price(estimated_cost);
        }
        sessions.insert(event.session);
    }

    SQLiteScan::Summary(LocalTokenHistorySummary {
        total_tokens,
        session_count: sessions.len(),
        coverage: if complete {
            LocalHistoryCoverage::Complete
        } else {
            LocalHistoryCoverage::Partial
        },
        cost_estimate,
    })
}

fn discover_databases(roots: &[PathBuf], budget: &mut Budget) -> (Vec<PathBuf>, bool) {
    let mut paths = Vec::new();
    let mut complete = true;

    for root in roots {
        if !budget.check() {
            return (paths, false);
        }
        let resolved_root = match fs::canonicalize(root) {
            Ok(path) => path,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => {
                complete = false;
                continue;
            }
        };
        match fs::metadata(&resolved_root) {
            Ok(metadata) if metadata.is_dir() => {}
            Ok(_) | Err(_) => {
                complete = false;
                continue;
            }
        }
        let entries = match fs::read_dir(&resolved_root) {
            Ok(entries) => entries,
            Err(_) => {
                complete = false;
                continue;
            }
        };
        for entry in entries {
            if !budget.check() {
                return (paths, false);
            }
            budget.directory_entries += 1;
            if budget.directory_entries > MAX_DIRECTORY_ENTRIES {
                return (paths, false);
            }
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => {
                    complete = false;
                    continue;
                }
            };
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
                complete = false;
                continue;
            };
            if name.starts_with('.') {
                continue;
            }
            let is_db = path
                .extension()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.eq_ignore_ascii_case("db"));
            if !is_db {
                continue;
            }
            let resolved = match fs::canonicalize(&path) {
                Ok(path) => path,
                Err(_) => {
                    complete = false;
                    continue;
                }
            };
            match fs::metadata(&resolved) {
                Ok(metadata) if metadata.is_file() => {}
                Ok(_) | Err(_) => {
                    complete = false;
                    continue;
                }
            }
            if paths.len() >= MAX_DATABASES {
                return (paths, false);
            }
            paths.push(resolved);
        }
    }
    paths.sort();
    paths.dedup();
    (paths, complete)
}

fn read_database(path: &Path, budget: &mut Budget) -> rusqlite::Result<DatabaseScan> {
    if !budget.check() {
        return Ok(DatabaseScan::Supported {
            events: Vec::new(),
            complete: false,
        });
    }
    let mut conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let tx = conn.transaction_with_behavior(TransactionBehavior::Deferred)?;
    match supported_schema(&tx, budget)? {
        SchemaInspection::Supported => {}
        SchemaInspection::Unsupported => return Ok(DatabaseScan::Unsupported),
        SchemaInspection::Incomplete => {
            return Ok(DatabaseScan::Supported {
                events: Vec::new(),
                complete: false,
            });
        }
    }

    let session = path
        .canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .into_owned();
    let mut rows = read_generation_rows(&tx, &session, budget)?;
    if rows.pending.is_empty() {
        return Ok(DatabaseScan::Supported {
            events: rows.events,
            complete: rows.complete,
        });
    }

    // Never realign step timestamps after a malformed or truncated primary scan.
    if !rows.complete {
        return Ok(DatabaseScan::Supported {
            events: rows.events,
            complete: false,
        });
    }

    if !matches!(
        supported_steps_schema(&tx, budget),
        Ok(SchemaInspection::Supported)
    ) {
        return Ok(DatabaseScan::Supported {
            events: rows.events,
            complete: false,
        });
    }

    let needed_occurrences = rows
        .pending
        .iter()
        .map(|pending| pending.step_uuid.clone())
        .collect::<HashSet<_>>()
        .into_iter()
        .map(|step_uuid| {
            let occurrences = rows
                .occurrences
                .get(&step_uuid)
                .cloned()
                .unwrap_or_default();
            (step_uuid, occurrences)
        })
        .collect::<HashMap<_, _>>();
    let step_scan =
        match read_step_timestamps(&tx, &needed_occurrences, budget, &mut rows.database_bytes) {
            Ok(scan) => scan,
            Err(_) => {
                return Ok(DatabaseScan::Supported {
                    events: rows.events,
                    complete: false,
                });
            }
        };
    if !step_scan.complete {
        return Ok(DatabaseScan::Supported {
            events: rows.events,
            complete: false,
        });
    }

    if !embedded_timestamps_agree(&rows.occurrences, &step_scan, &rows.bot_id_uses) {
        return Ok(DatabaseScan::Supported {
            events: rows.events,
            complete: false,
        });
    }

    let resolved = resolve_step_timestamps(
        &step_scan.timestamps,
        &needed_occurrences,
        &step_scan.ambiguous_bot_ids,
        step_scan.unidentified_rows_present,
    );
    let recovered = local_bot_id::append_recovered_events(
        &mut rows.events,
        &session,
        &rows.pending,
        &resolved,
        &rows.occurrences,
        &step_scan,
        &rows.bot_id_uses,
    );
    if recovered < rows.pending.len() {
        rows.complete = false;
    }
    Ok(DatabaseScan::Supported {
        events: rows.events,
        complete: rows.complete,
    })
}

fn read_generation_rows(
    conn: &Connection,
    session: &str,
    budget: &mut Budget,
) -> rusqlite::Result<ParsedRows> {
    let mut statement = conn.prepare(
        "SELECT idx, CASE WHEN typeof(data) = 'blob' THEN length(data) END, CASE WHEN typeof(data) = 'blob' AND length(data) <= ?2 THEN data END FROM main.gen_metadata NOT INDEXED LIMIT ?1",
    )?;
    let row_limit = i64::try_from(MAX_ROWS_PER_DATABASE + 1).unwrap_or(i64::MAX);
    let blob_limit = i64::try_from(MAX_BLOB_BYTES).unwrap_or(i64::MAX);
    let mut query = statement.query(rusqlite::params![row_limit, blob_limit])?;
    let mut database_bytes = 0usize;
    let mut database_rows = 0usize;
    let mut complete = true;
    let mut events = Vec::new();
    let mut pending = Vec::new();
    let mut occurrences: HashMap<String, Vec<StepOccurrence>> = HashMap::new();
    let mut bot_id_uses = HashMap::new();

    while let Some(row) = query.next()? {
        if !budget.check() {
            complete = false;
            break;
        }
        database_rows += 1;
        budget.rows += 1;
        if database_rows > MAX_ROWS_PER_DATABASE || budget.rows > MAX_ROWS {
            complete = false;
            break;
        }

        let idx: i64 = match row.get(0) {
            Ok(value) if value >= 0 => value,
            _ => {
                complete = false;
                continue;
            }
        };
        let declared: Option<i64> = row.get(1).ok();
        let Some(declared) = declared.and_then(|value| usize::try_from(value).ok()) else {
            complete = false;
            continue;
        };
        database_bytes = match database_bytes.checked_add(declared) {
            Some(value) if value <= MAX_DATABASE_BYTES => value,
            _ => {
                complete = false;
                break;
            }
        };
        budget.bytes = match budget.bytes.checked_add(declared) {
            Some(value) if value <= MAX_TOTAL_BYTES => value,
            _ => {
                complete = false;
                break;
            }
        };
        if declared == 0 || declared > MAX_BLOB_BYTES {
            complete = false;
            continue;
        }

        let blob = match row.get_ref(2)? {
            ValueRef::Blob(bytes) if bytes.len() == declared => bytes,
            _ => {
                complete = false;
                continue;
            }
        };
        let Some(turn) = parse_turn(blob) else {
            complete = false;
            continue;
        };
        let Some(usage) = turn.usage.as_ref() else {
            complete = false;
            continue;
        };
        let Some(total) = token_total(usage) else {
            complete = false;
            continue;
        };
        let step_uuid = turn.step_uuid.clone();
        if let Some(bot_id) = turn.usage.as_ref().and_then(|usage| usage.bot_id.as_ref()) {
            *bot_id_uses.entry(bot_id.clone()).or_insert(0) += 1;
        }
        if let Some(step_uuid) = step_uuid.as_deref() {
            occurrences
                .entry(step_uuid.to_string())
                .or_default()
                .push(StepOccurrence {
                    row: idx,
                    timestamp_ms: turn.timestamp_ms,
                    bot_id: turn.usage.as_ref().and_then(|usage| usage.bot_id.clone()),
                });
        }
        match (turn.timestamp_ms, step_uuid) {
            (Some(_), _) => events.push(Event {
                session: session.to_string(),
                row: idx,
                turn,
                total,
            }),
            (None, Some(step_uuid)) => pending.push(PendingTimestampRow {
                row: idx,
                step_uuid,
                turn,
                total,
            }),
            (None, None) => complete = false,
        }
    }

    Ok(ParsedRows {
        events,
        pending,
        occurrences,
        bot_id_uses,
        database_bytes,
        complete,
    })
}

fn token_total(usage: &super::local_proto::ParsedUsage) -> Option<u64> {
    usage
        .system_prompt
        .checked_add(usage.new_input)
        .and_then(|value| value.checked_add(usage.output))
        .and_then(|value| value.checked_add(usage.cache_read))
        .and_then(|value| value.checked_add(usage.reasoning))
}

fn read_step_timestamps(
    conn: &Connection,
    needed_occurrences: &HashMap<String, Vec<StepOccurrence>>,
    budget: &mut Budget,
    database_bytes: &mut usize,
) -> rusqlite::Result<StepTimestampScan> {
    let mut statement = conn.prepare(
        "SELECT idx, CASE WHEN typeof(metadata) = 'blob' THEN length(metadata) END, CASE WHEN typeof(metadata) = 'blob' AND length(metadata) <= ?1 THEN metadata END FROM main.steps NOT INDEXED",
    )?;
    let blob_limit = i64::try_from(MAX_BLOB_BYTES).unwrap_or(i64::MAX);
    let mut query = statement.query([blob_limit])?;
    let mut timestamps = HashMap::<String, Vec<StepOccurrence>>::new();
    let mut by_bot_id = HashMap::<String, ExactStepTimestamp>::new();
    let mut ambiguous_bot_ids = HashSet::new();
    let mut complete = true;
    let mut rows_are_valid = true;
    let mut unidentified_rows_present = false;

    while let Some(row) = query.next()? {
        if !budget.check() {
            complete = false;
            break;
        }
        budget.rows += 1;
        if budget.rows > MAX_ROWS {
            complete = false;
            break;
        }

        let idx: i64 = match row.get(0) {
            Ok(value) if value >= 0 => value,
            _ => {
                rows_are_valid = false;
                continue;
            }
        };
        let declared: Option<i64> = row.get(1).ok();
        let Some(declared) = declared.and_then(|value| usize::try_from(value).ok()) else {
            rows_are_valid = false;
            continue;
        };
        *database_bytes = match (*database_bytes).checked_add(declared) {
            Some(value) if value <= MAX_DATABASE_BYTES => value,
            _ => {
                complete = false;
                break;
            }
        };
        budget.bytes = match budget.bytes.checked_add(declared) {
            Some(value) if value <= MAX_TOTAL_BYTES => value,
            _ => {
                complete = false;
                break;
            }
        };
        if declared == 0 || declared > MAX_BLOB_BYTES {
            rows_are_valid = false;
            continue;
        }

        let blob = match row.get_ref(2)? {
            ValueRef::Blob(bytes) if bytes.len() == declared => bytes,
            _ => {
                rows_are_valid = false;
                continue;
            }
        };
        let Some(metadata) = parse_step_metadata(blob) else {
            rows_are_valid = false;
            continue;
        };
        if let Some(bot_id) = metadata.bot_id.as_deref() {
            record_exact_bot_id(
                bot_id,
                metadata.step_uuid.as_deref(),
                metadata.timestamp_ms,
                &mut by_bot_id,
                &mut ambiguous_bot_ids,
            );
        }
        // Unidentified rows cannot supply UUID positions, but their bot IDs still count as evidence.
        let Some(step_uuid) = metadata.step_uuid else {
            unidentified_rows_present = true;
            continue;
        };
        if needed_occurrences.contains_key(&step_uuid) {
            timestamps
                .entry(step_uuid)
                .or_default()
                .push(StepOccurrence {
                    row: idx,
                    timestamp_ms: metadata.timestamp_ms,
                    bot_id: metadata.bot_id,
                });
        }
    }

    Ok(StepTimestampScan {
        timestamps,
        by_bot_id,
        ambiguous_bot_ids,
        unidentified_rows_present,
        complete: complete && rows_are_valid,
    })
}

fn supported_schema(conn: &Connection, budget: &mut Budget) -> rusqlite::Result<SchemaInspection> {
    let mut statement =
        conn.prepare("SELECT name, type, rootpage FROM main.sqlite_master LIMIT ?1")?;
    let mut rows = statement.query([i64::try_from(MAX_SCHEMA_ENTRIES + 1).unwrap_or(i64::MAX)])?;
    let mut found = false;
    let mut schema_entries = 0usize;
    while let Some(row) = rows.next()? {
        if !budget.check() {
            return Ok(SchemaInspection::Incomplete);
        }
        schema_entries += 1;
        if schema_entries > MAX_SCHEMA_ENTRIES {
            return Ok(SchemaInspection::Incomplete);
        }
        let name: String = row.get(0)?;
        let kind: String = row.get(1)?;
        if !budget.charge_schema_text(&name) || !budget.charge_schema_text(&kind) {
            return Ok(SchemaInspection::Incomplete);
        }
        if !name.eq_ignore_ascii_case("gen_metadata") {
            continue;
        }
        let rootpage: i64 = row.get(2)?;
        if kind != "table" || rootpage <= 0 || found {
            return Ok(SchemaInspection::Unsupported);
        }
        found = true;
    }
    if !found {
        return Ok(SchemaInspection::Unsupported);
    }

    has_stored_columns(conn, "gen_metadata", &["idx", "data"], budget)
}

fn supported_steps_schema(
    conn: &Connection,
    budget: &mut Budget,
) -> rusqlite::Result<SchemaInspection> {
    let mut statement =
        conn.prepare("SELECT name, type, rootpage FROM main.sqlite_master LIMIT ?1")?;
    let mut rows = statement.query([i64::try_from(MAX_SCHEMA_ENTRIES + 1).unwrap_or(i64::MAX)])?;
    let mut found = false;
    let mut schema_entries = 0usize;
    while let Some(row) = rows.next()? {
        if !budget.check() {
            return Ok(SchemaInspection::Incomplete);
        }
        schema_entries += 1;
        if schema_entries > MAX_SCHEMA_ENTRIES {
            return Ok(SchemaInspection::Incomplete);
        }
        let name: String = row.get(0)?;
        let kind: String = row.get(1)?;
        if !budget.charge_schema_text(&name) || !budget.charge_schema_text(&kind) {
            return Ok(SchemaInspection::Incomplete);
        }
        if !name.eq_ignore_ascii_case("steps") {
            continue;
        }
        let rootpage: i64 = row.get(2)?;
        if kind != "table" || rootpage <= 0 || found {
            return Ok(SchemaInspection::Unsupported);
        }
        found = true;
    }
    if !found {
        return Ok(SchemaInspection::Unsupported);
    }

    has_stored_columns(conn, "steps", &["idx", "metadata"], budget)
}

fn has_stored_columns(
    conn: &Connection,
    table: &str,
    required: &[&str],
    budget: &mut Budget,
) -> rusqlite::Result<SchemaInspection> {
    let table = match table {
        "gen_metadata" | "steps" => table,
        _ => return Ok(SchemaInspection::Unsupported),
    };

    let mut columns = HashSet::new();
    let mut schema_columns = 0usize;
    let query = format!("PRAGMA main.table_xinfo('{table}')");
    let mut info = conn.prepare(&query)?;
    let mut rows = info.query([])?;
    while let Some(row) = rows.next()? {
        if !budget.check() {
            return Ok(SchemaInspection::Incomplete);
        }
        schema_columns += 1;
        if schema_columns > MAX_SCHEMA_COLUMNS {
            return Ok(SchemaInspection::Incomplete);
        }
        let hidden: i64 = row.get(6)?;
        if hidden != 0 {
            return Ok(SchemaInspection::Unsupported);
        }
        let name: String = row.get(1)?;
        let column_type: String = row.get(2).unwrap_or_default();
        let default_value: Option<String> = row.get(4).ok();
        if !budget.charge_schema_text(&name) || !budget.charge_schema_text(&column_type) {
            return Ok(SchemaInspection::Incomplete);
        }
        if let Some(default_value) = default_value.as_deref()
            && !budget.charge_schema_text(default_value)
        {
            return Ok(SchemaInspection::Incomplete);
        }
        columns.insert(name.to_ascii_lowercase());
    }
    Ok(if required.iter().all(|column| columns.contains(*column)) {
        SchemaInspection::Supported
    } else {
        SchemaInspection::Unsupported
    })
}

#[cfg(test)]
#[path = "local_sqlite_synthetic_tests.rs"]
mod synthetic_tests;

#[cfg(test)]
#[path = "local_sqlite_tests.rs"]
mod tests;
