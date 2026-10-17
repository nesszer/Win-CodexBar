//! Opt-in Codex identified-request subtotals (upstream #4447).
//!
//! Only owned native `token_usage_record` events that carry a thread and a
//! response identity contribute. Identities are length-framed SHA-256 hashes,
//! so exports carry timestamps and token categories without conversation
//! text, model names, paths, or raw session ids. Combined prices are
//! deliberately unavailable (`costUSD: null`).

use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::future::Future;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::agent_sessions::RemoteSessionFetcher;
use crate::core::{CostUsagePricing, JsonlScanner};

pub(crate) const MAX_IDENTIFIED_RECORDS: usize = 50_000;
pub(crate) const MAX_IDENTIFIED_OUTPUT_BYTES: usize = 32 * 1024 * 1024;
pub(crate) const MAX_IDENTIFIED_END_UNIX_MS: i64 = 253_402_300_799_999;
const DAY_MS: i64 = 86_400_000;
const MAX_LINE_BYTES: usize = 256 * 1024;

pub(crate) const LOCAL_IDENTIFIED_UNAVAILABLE: &str =
    "Local native Codex identified usage is unavailable.";
pub(crate) const REMOTE_IDENTIFIED_UNAVAILABLE: &str =
    "Remote identified usage is unavailable. Check SSH and update its CLI for --identified-only.";
pub(crate) const IDENTIFIED_USAGE_ARGS_ERROR: &str = "Use --identified-only --provider codex with JSON, --days 1...365 and one optional --remote host. --identified-end must be Unix milliseconds; period, grouping, breakdown and separate host summaries are unsupported.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IdentifiedUsageError {
    Unavailable,
    InvalidReport,
}

/// A provider-owned request; input includes cached input, while reasoning is
/// part of output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodexRequestRecord {
    pub id: String,
    pub timestamp_unix_ms: i64,
    pub input_tokens: i64,
    pub cache_read_tokens: i64,
    pub output_tokens: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_tokens: Option<i64>,
}

impl CodexRequestRecord {
    fn usage(&self) -> (i64, i64, i64, Option<i64>) {
        (
            self.input_tokens,
            self.cache_read_tokens,
            self.output_tokens,
            self.reasoning_tokens,
        )
    }
}

/// One exact inclusive UTC window. This identifies requests without claiming
/// account-wide usage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodexRequestLedger {
    pub schema_version: u32,
    pub provider: String,
    pub start_unix_ms: i64,
    pub end_unix_ms: i64,
    pub records: Vec<CodexRequestRecord>,
    #[serde(rename = "conflictingIDs")]
    pub conflicting_ids: Vec<String>,
    pub excluded_count: i64,
    pub partial: bool,
}

/// Numeric identified-request subtotal; pricing is deliberately unavailable.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodexRequestSubtotal {
    pub kind: &'static str,
    pub total_tokens: i64,
    pub request_count: usize,
    pub conflict_count: usize,
    pub excluded_count: i64,
    pub partial: bool,
    #[serde(rename = "costUSD")]
    pub cost_usd: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct CodexIdentifiedSource {
    pub host: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ledger: Option<CodexRequestLedger>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodexIdentifiedResult {
    pub reports: Vec<CodexIdentifiedSource>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtotal: Option<CodexRequestSubtotal>,
    pub all_sources_available: bool,
    pub partial: bool,
}

fn is_valid_window(start_unix_ms: i64, end_unix_ms: i64) -> bool {
    start_unix_ms >= 0
        && end_unix_ms <= MAX_IDENTIFIED_END_UNIX_MS
        && end_unix_ms >= start_unix_ms
        && start_unix_ms % DAY_MS == 0
        && end_unix_ms - start_unix_ms < 365 * DAY_MS
}

/// The inclusive start of a `days`-long UTC window ending at `end_unix_ms`:
/// UTC midnight `days - 1` days before the end's day.
pub(crate) fn identified_window_start(days: u32, end_unix_ms: i64) -> Option<i64> {
    if !(1..=365).contains(&days) || !(0..=MAX_IDENTIFIED_END_UNIX_MS).contains(&end_unix_ms) {
        return None;
    }
    let start = end_unix_ms.div_euclid(DAY_MS) * DAY_MS - i64::from(days - 1) * DAY_MS;
    (start >= 0).then_some(start)
}

fn is_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn digest(components: &[&str]) -> String {
    let framed: String = components
        .iter()
        .map(|component| format!("{}:{component}", component.len()))
        .collect();
    Sha256::digest(framed.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

impl CodexRequestLedger {
    pub(crate) fn empty(start_unix_ms: i64, end_unix_ms: i64) -> Self {
        Self {
            schema_version: 1,
            provider: "codex".to_string(),
            start_unix_ms,
            end_unix_ms,
            records: Vec::new(),
            conflicting_ids: Vec::new(),
            excluded_count: 0,
            partial: false,
        }
    }

    pub(crate) fn validate(
        &self,
        start_unix_ms: i64,
        end_unix_ms: i64,
    ) -> Result<(), IdentifiedUsageError> {
        let unique_conflicts: HashSet<&str> =
            self.conflicting_ids.iter().map(String::as_str).collect();
        let header_ok = self.schema_version == 1
            && self.provider == "codex"
            && self.start_unix_ms == start_unix_ms
            && self.end_unix_ms == end_unix_ms
            && is_valid_window(start_unix_ms, end_unix_ms)
            && self.records.len() <= MAX_IDENTIFIED_RECORDS
            && self.conflicting_ids.len() <= MAX_IDENTIFIED_RECORDS
            && self.excluded_count >= 0
            && unique_conflicts.len() == self.conflicting_ids.len()
            && self.conflicting_ids.iter().all(|id| is_digest(id));
        if !header_ok {
            return Err(IdentifiedUsageError::InvalidReport);
        }
        let mut total: i64 = 0;
        for record in &self.records {
            let record_ok = is_digest(&record.id)
                && (start_unix_ms..=end_unix_ms).contains(&record.timestamp_unix_ms)
                && record.input_tokens >= 0
                && (0..=record.input_tokens).contains(&record.cache_read_tokens)
                && record.output_tokens >= 0
                && record
                    .reasoning_tokens
                    .is_none_or(|reasoning| (0..=record.output_tokens).contains(&reasoning));
            let sum = record.input_tokens.checked_add(record.output_tokens);
            total = match (record_ok, sum.and_then(|sum| total.checked_add(sum))) {
                (true, Some(total)) => total,
                _ => return Err(IdentifiedUsageError::InvalidReport),
            };
        }
        Ok(())
    }

    /// Merge source ledgers into one subtotal. Conflicts remain excluded even
    /// when a later source supplies a clean observation of the identity.
    pub(crate) fn merge(ledgers: &[Self]) -> Result<CodexRequestSubtotal, IdentifiedUsageError> {
        let first = ledgers.first().ok_or(IdentifiedUsageError::Unavailable)?;
        let mut records: HashMap<&str, &CodexRequestRecord> = HashMap::new();
        let mut conflicts: HashSet<&str> = HashSet::new();
        let mut excluded: i64 = 0;
        for ledger in ledgers {
            ledger.validate(first.start_unix_ms, first.end_unix_ms)?;
            excluded = excluded
                .checked_add(ledger.excluded_count)
                .ok_or(IdentifiedUsageError::InvalidReport)?;
            conflicts.extend(ledger.conflicting_ids.iter().map(String::as_str));
            for row in &ledger.records {
                if let Some(previous) = records.get(row.id.as_str())
                    && previous.usage() != row.usage()
                {
                    conflicts.insert(row.id.as_str());
                }
                if records
                    .get(row.id.as_str())
                    .is_none_or(|previous| previous.timestamp_unix_ms > row.timestamp_unix_ms)
                {
                    records.insert(row.id.as_str(), row);
                }
            }
        }
        let mut total: i64 = 0;
        let mut request_count = 0;
        for row in records
            .values()
            .filter(|row| !conflicts.contains(row.id.as_str()))
        {
            total = row
                .input_tokens
                .checked_add(row.output_tokens)
                .and_then(|sum| total.checked_add(sum))
                .ok_or(IdentifiedUsageError::InvalidReport)?;
            request_count += 1;
        }
        Ok(CodexRequestSubtotal {
            kind: "identified-request-subtotal",
            total_tokens: total,
            request_count,
            conflict_count: conflicts.len(),
            excluded_count: excluded,
            partial: excluded > 0 || !conflicts.is_empty() || ledgers.iter().any(|l| l.partial),
            cost_usd: None,
        })
    }
}

/// Collect the local ledger and, optionally, one remote ledger. A failed
/// source keeps the available ones and marks the result partial.
pub(crate) async fn collect_identified_usage<L, R, Fut>(
    remote: Option<&str>,
    start_unix_ms: i64,
    end_unix_ms: i64,
    local: L,
    fetch_remote: R,
) -> CodexIdentifiedResult
where
    L: FnOnce() -> Result<CodexRequestLedger, IdentifiedUsageError>,
    R: FnOnce(String) -> Fut,
    Fut: Future<Output = Result<CodexRequestLedger, IdentifiedUsageError>>,
{
    let mut reports: Vec<CodexIdentifiedSource> = Vec::new();
    let mut subtotal = None;

    let mut accept = |host: &str,
                      fetched: Result<CodexRequestLedger, IdentifiedUsageError>,
                      unavailable: &str,
                      reports: &mut Vec<CodexIdentifiedSource>| {
        let accepted = fetched.and_then(|ledger| {
            ledger.validate(start_unix_ms, end_unix_ms)?;
            // Reject an overflowing source or union while the earlier sources
            // remain available.
            let mut ledgers: Vec<CodexRequestLedger> = reports
                .iter()
                .filter_map(|report| report.ledger.clone())
                .collect();
            ledgers.push(ledger.clone());
            let merged = CodexRequestLedger::merge(&ledgers)?;
            Ok((ledger, merged))
        });
        match accepted {
            Ok((ledger, merged)) => {
                subtotal = Some(merged);
                reports.push(CodexIdentifiedSource {
                    host: host.to_string(),
                    ledger: Some(ledger),
                    error: None,
                });
            }
            Err(_) => reports.push(CodexIdentifiedSource {
                host: host.to_string(),
                ledger: None,
                error: Some(unavailable.to_string()),
            }),
        }
    };

    accept("local", local(), LOCAL_IDENTIFIED_UNAVAILABLE, &mut reports);
    if let Some(host) = remote {
        let fetched = fetch_remote(host.to_string()).await;
        accept(host, fetched, REMOTE_IDENTIFIED_UNAVAILABLE, &mut reports);
    }

    let all_sources_available = reports.iter().all(|report| report.ledger.is_some());
    let partial = !all_sources_available || subtotal.as_ref().is_none_or(|s| s.partial);
    CodexIdentifiedResult {
        reports,
        subtotal,
        all_sources_available,
        partial,
    }
}

/// Native Codex session roots: `sessions` plus its sibling `archived_sessions`.
fn default_codex_roots() -> Vec<PathBuf> {
    let Some(root) = JsonlScanner::default_codex_sessions_root() else {
        return Vec::new();
    };
    let archived = (root.file_name().is_some_and(|name| name == "sessions"))
        .then(|| root.parent().map(|parent| parent.join("archived_sessions")))
        .flatten();
    std::iter::once(root).chain(archived).collect()
}

pub(crate) fn load_local_ledger(
    start_unix_ms: i64,
    end_unix_ms: i64,
) -> Result<CodexRequestLedger, IdentifiedUsageError> {
    load_ledger_from_roots(&default_codex_roots(), start_unix_ms, end_unix_ms)
}

#[derive(Default)]
struct LedgerBuilder {
    records: HashMap<String, CodexRequestRecord>,
    conflicts: HashSet<String>,
    exceeded_limit: bool,
    excluded: i64,
    partial: bool,
}

impl LedgerBuilder {
    fn mark_conflict(&mut self, id: String) {
        if self.conflicts.len() < MAX_IDENTIFIED_RECORDS || self.conflicts.contains(&id) {
            self.conflicts.insert(id);
        } else {
            self.exceeded_limit = true;
        }
    }

    fn exclude(&mut self) {
        self.partial = true;
        self.excluded = self.excluded.saturating_add(1);
    }

    fn observe(&mut self, row: CodexRequestRecord) {
        if let Some(previous) = self.records.get(&row.id)
            && previous.usage() != row.usage()
        {
            self.mark_conflict(row.id.clone());
        }
        if self.records.len() < MAX_IDENTIFIED_RECORDS || self.records.contains_key(&row.id) {
            if self
                .records
                .get(&row.id)
                .is_none_or(|previous| previous.timestamp_unix_ms > row.timestamp_unix_ms)
            {
                self.records.insert(row.id.clone(), row);
            }
        } else {
            self.exceeded_limit = true;
        }
    }
}

pub(crate) fn load_ledger_from_roots(
    roots: &[PathBuf],
    start_unix_ms: i64,
    end_unix_ms: i64,
) -> Result<CodexRequestLedger, IdentifiedUsageError> {
    let mut ledger = CodexRequestLedger::empty(start_unix_ms, end_unix_ms);
    ledger.validate(start_unix_ms, end_unix_ms)?;
    let mut builder = LedgerBuilder::default();
    let mut files = Vec::new();
    let mut found_root = false;
    for root in roots {
        match fs::metadata(root) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Ok(metadata) if metadata.is_dir() => {}
            _ => {
                builder.partial = true;
                continue;
            }
        }
        match collect_jsonl_files(root, &mut files) {
            Ok(complete) => {
                found_root = true;
                builder.partial |= !complete;
            }
            Err(_) => builder.partial = true,
        }
    }
    if !found_root {
        return Err(IdentifiedUsageError::Unavailable);
    }
    files.sort();
    for file in &files {
        if last_written_before(file, start_unix_ms) {
            continue;
        }
        scan_file(file, start_unix_ms, end_unix_ms, &mut builder);
        if builder.exceeded_limit {
            return Err(IdentifiedUsageError::InvalidReport);
        }
    }

    let mut records: Vec<_> = builder.records.into_values().collect();
    records.sort_by(|lhs, rhs| lhs.id.cmp(&rhs.id));
    let mut conflicts: Vec<_> = builder.conflicts.into_iter().collect();
    conflicts.sort();
    ledger.records = records;
    ledger.excluded_count = builder.excluded;
    ledger.partial = builder.partial || builder.excluded > 0 || !conflicts.is_empty();
    ledger.conflicting_ids = conflicts;
    ledger.validate(start_unix_ms, end_unix_ms)?;
    Ok(ledger)
}

/// Clock-skew allowance between a record's timestamp and the file's mtime.
const MTIME_SKEW_MARGIN_MS: i64 = 24 * 60 * 60 * 1000;

/// A file last modified before the window cannot hold an in-window record,
/// so the scan (and the remote SSH budget) can skip it. Unreadable metadata
/// keeps the file so the scan still decides.
fn last_written_before(file: &Path, start_unix_ms: i64) -> bool {
    let Some(modified_ms) = fs::metadata(file)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
        .and_then(|elapsed| i64::try_from(elapsed.as_millis()).ok())
    else {
        return false;
    };
    modified_ms.saturating_add(MTIME_SKEW_MARGIN_MS) < start_unix_ms
}

/// Recursively list `*.jsonl` files. Returns `Ok(false)` when a nested
/// directory could not be read, so the caller can mark the scan partial.
fn collect_jsonl_files(root: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<bool> {
    let mut complete = true;
    let mut pending = vec![root.to_path_buf()];
    let mut is_root = true;
    while let Some(dir) = pending.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) if is_root => return Err(error),
            Err(_) => {
                complete = false;
                continue;
            }
        };
        is_root = false;
        for entry in entries {
            let Ok(entry) = entry else {
                complete = false;
                continue;
            };
            let path = entry.path();
            match entry.file_type() {
                Ok(kind) if kind.is_dir() => pending.push(path),
                Ok(_) => {
                    if path
                        .extension()
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("jsonl"))
                    {
                        files.push(path);
                    }
                }
                Err(_) => complete = false,
            }
        }
    }
    Ok(complete)
}

pub(crate) type Usage = (i64, i64, i64, i64);

pub(crate) fn non_empty_str(value: Option<&Value>) -> Option<&str> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn token_int(
    usage: &serde_json::Map<String, Value>,
    key: &str,
    default: Option<i64>,
) -> Option<i64> {
    let Some(value) = usage.get(key) else {
        return default;
    };
    if let Some(number) = value.as_i64() {
        return (number >= 0).then_some(number);
    }
    value
        .as_f64()
        .filter(|number| number.is_finite() && *number >= 0.0 && number.fract() == 0.0)
        .filter(|number| *number < i64::MAX as f64)
        .map(|number| {
            #[allow(
                clippy::cast_possible_truncation,
                reason = "the value is a finite, non-negative whole number below i64::MAX"
            )]
            let whole = number as i64;
            whole
        })
}

/// `(input, cached, output, reasoning)` from one request-usage object.
pub(crate) fn request_usage(value: Option<&Value>) -> Option<Usage> {
    let usage = value?.as_object()?;
    let input = token_int(usage, "input_tokens", None)?;
    let output = token_int(usage, "output_tokens", None)?;
    let cached = token_int(usage, "cached_input_tokens", Some(0))?;
    let reasoning = token_int(usage, "reasoning_output_tokens", Some(0))?;
    (cached <= input && reasoning <= output && input.checked_add(output).is_some())
        .then_some((input, cached, output, reasoning))
}

pub(crate) fn totals_at_least(total: Usage, usage: Usage) -> bool {
    total.0 >= usage.0 && total.1 >= usage.1 && total.2 >= usage.2 && total.3 >= usage.3
}

pub(crate) fn turn_id(payload: &Value) -> Option<String> {
    [Some(payload), payload.get("info")]
        .into_iter()
        .flatten()
        .find_map(|container| {
            ["turn_id", "turnId", "id"].iter().find_map(|key| {
                container
                    .get(*key)
                    .and_then(Value::as_str)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
            })
        })
}

struct RequestUsageRecord {
    timestamp: String,
    thread_id: String,
    session_id: Option<String>,
    response_id: String,
    turn_id: Option<String>,
    model: Option<String>,
    usage: Usage,
    thread_total: Usage,
}

fn request_usage_record(object: &Value) -> Option<RequestUsageRecord> {
    let payload = object.get("payload")?;
    let thread_id = non_empty_str(payload.get("thread_id"))?;
    let response_id = non_empty_str(payload.get("response_id"))?;
    let timestamp = object.get("timestamp")?.as_str()?;
    let usage = request_usage(payload.get("usage"))?;
    let thread_total = request_usage(payload.get("thread_token_usage"))?;
    if !totals_at_least(thread_total, usage) {
        return None;
    }
    let session_id = non_empty_str(payload.get("session_id"));
    if payload.get("session_id").is_some() && session_id.is_none() {
        return None;
    }
    Some(RequestUsageRecord {
        timestamp: timestamp.to_string(),
        thread_id: thread_id.to_string(),
        session_id: session_id.map(str::to_string),
        response_id: response_id.to_string(),
        turn_id: turn_id(payload),
        model: non_empty_str(payload.get("model")).map(str::to_string),
        usage,
        thread_total,
    })
}

pub(crate) fn timestamp_unix_ms(timestamp: &str) -> Option<i64> {
    let parsed = chrono::DateTime::parse_from_rfc3339(timestamp.trim()).ok()?;
    let round_up = parsed.timestamp_subsec_nanos() % 1_000_000 >= 500_000;
    Some(parsed.timestamp_millis() + i64::from(round_up))
}

fn file_fingerprint(path: &Path) -> Option<(u64, Option<SystemTime>)> {
    fs::metadata(path)
        .ok()
        .map(|metadata| (metadata.len(), metadata.modified().ok()))
}

pub(crate) enum JsonlLine {
    Complete(Vec<u8>),
    /// Over [`MAX_LINE_BYTES`]; only the retained prefix is kept.
    Oversized(Vec<u8>),
}

pub(crate) fn read_line<R: BufRead>(
    reader: &mut R,
    consumed: &mut u64,
) -> std::io::Result<Option<JsonlLine>> {
    let mut bytes = Vec::new();
    let mut length = 0_usize;
    let mut saw_any = false;
    loop {
        let buffer = reader.fill_buf()?;
        if buffer.is_empty() {
            break;
        }
        saw_any = true;
        let (chunk, done) = match buffer.iter().position(|byte| *byte == b'\n') {
            Some(index) => (&buffer[..index], Some(index + 1)),
            None => (buffer, None),
        };
        let room = MAX_LINE_BYTES.saturating_sub(bytes.len());
        bytes.extend_from_slice(&chunk[..chunk.len().min(room)]);
        length = length.saturating_add(chunk.len());
        let used = done.unwrap_or(buffer.len());
        reader.consume(used);
        *consumed = consumed.saturating_add(used as u64);
        if done.is_some() {
            break;
        }
    }
    if !saw_any {
        return Ok(None);
    }
    if bytes.last() == Some(&b'\r') {
        bytes.pop();
    }
    Ok(Some(if length > MAX_LINE_BYTES {
        JsonlLine::Oversized(bytes)
    } else {
        JsonlLine::Complete(bytes)
    }))
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

struct LegacyCandidate {
    usage: Usage,
    total: Option<Usage>,
}

fn scan_file(path: &Path, start_unix_ms: i64, end_unix_ms: i64, builder: &mut LedgerBuilder) {
    let before = file_fingerprint(path);
    let session_id = JsonlScanner::read_codex_session_metadata(path)
        .ok()
        .and_then(|metadata| metadata.session_id);
    let Ok(file) = File::open(path) else {
        builder.partial = true;
        return;
    };
    let mut reader = BufReader::new(file);
    let mut parsed_bytes = 0_u64;
    let mut current_model: Option<String> = None;
    let mut current_turn: Option<String> = None;
    let mut turn_models: HashMap<String, String> = HashMap::new();
    let mut record_totals: HashSet<Usage> = HashSet::new();
    let mut record_usages: HashSet<Usage> = HashSet::new();
    let mut legacy: Vec<LegacyCandidate> = Vec::new();
    let mut previous_legacy_total: Option<Usage> = None;
    let in_window = |ms: i64| (start_unix_ms..=end_unix_ms).contains(&ms);

    loop {
        let line = match read_line(&mut reader, &mut parsed_bytes) {
            Ok(Some(line)) => line,
            Ok(None) => break,
            Err(_) => {
                builder.partial = true;
                break;
            }
        };
        let bytes = match line {
            JsonlLine::Complete(bytes) => bytes,
            JsonlLine::Oversized(prefix) => {
                // Oversized prompt-bearing lines are common; only a dropped
                // usage line makes the identified history incomplete.
                if contains_bytes(&prefix, b"token_usage_record")
                    || contains_bytes(&prefix, b"\"token_count\"")
                {
                    builder.partial = true;
                }
                continue;
            }
        };
        if bytes.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let object = match serde_json::from_slice::<Value>(&bytes) {
            Ok(object) if object.is_object() => object,
            _ => {
                builder.partial = true;
                continue;
            }
        };
        let payload = object.get("payload").unwrap_or(&Value::Null);
        match object.get("type").and_then(Value::as_str) {
            Some("turn_context") => {
                let model = non_empty_str(payload.get("model")).map(str::to_string);
                let turn = turn_id(payload);
                if let (Some(turn), Some(model)) = (&turn, &model) {
                    turn_models.insert(turn.clone(), model.clone());
                }
                if model.is_some() {
                    current_model = model;
                }
                if turn.is_some() {
                    current_turn = turn;
                }
            }
            Some("event_msg") => match payload.get("type").and_then(Value::as_str) {
                Some("task_started") => {
                    if let Some(turn) = turn_id(payload) {
                        current_turn = Some(turn);
                    }
                }
                Some("token_count") => {
                    let info = payload.get("info");
                    let Some(usage) = request_usage(info.and_then(|v| v.get("last_token_usage")))
                    else {
                        continue;
                    };
                    let total = request_usage(info.and_then(|v| v.get("total_token_usage")));
                    // Codex repeats an unchanged cumulative counter; only a
                    // changed total describes new legacy usage.
                    if total.is_some() && total == previous_legacy_total {
                        continue;
                    }
                    previous_legacy_total = total;
                    let model = current_model.as_deref().unwrap_or_default();
                    let timestamp = object
                        .get("timestamp")
                        .and_then(Value::as_str)
                        .and_then(timestamp_unix_ms);
                    if usage.0.saturating_add(usage.2) > 0
                        && timestamp.is_some_and(in_window)
                        && CostUsagePricing::counts_toward_codex_subscription(model)
                    {
                        legacy.push(LegacyCandidate { usage, total });
                    }
                }
                _ => {}
            },
            Some("token_usage_record") => {
                let Some(session_id) = session_id.as_deref() else {
                    // Without a session identity no record can be proven owned.
                    builder.partial = true;
                    continue;
                };
                match request_usage_record(&object) {
                    Some(record) => {
                        // Copied parent records still mirror their copied
                        // legacy counters, so both identify those counters.
                        record_totals.insert(record.thread_total);
                        record_usages.insert(record.usage);
                        let owned = record.thread_id == session_id
                            && record
                                .session_id
                                .as_deref()
                                .is_none_or(|id| id == session_id);
                        if !owned {
                            continue;
                        }
                        let turn = record.turn_id.clone().or_else(|| current_turn.clone());
                        let model = record
                            .model
                            .clone()
                            .or_else(|| turn.as_ref().and_then(|t| turn_models.get(t).cloned()))
                            .or_else(|| current_model.clone());
                        if model.as_deref().is_some_and(|model| {
                            !CostUsagePricing::counts_toward_codex_subscription(model)
                        }) {
                            continue;
                        }
                        let Some(milliseconds) = timestamp_unix_ms(&record.timestamp) else {
                            builder.exclude();
                            continue;
                        };
                        if !in_window(milliseconds) {
                            continue;
                        }
                        let (input, cached, output, reasoning) = record.usage;
                        builder.observe(CodexRequestRecord {
                            id: digest(&["codex", &record.thread_id, &record.response_id]),
                            timestamp_unix_ms: milliseconds,
                            input_tokens: input,
                            cache_read_tokens: cached,
                            output_tokens: output,
                            reasoning_tokens: Some(reasoning.clamp(0, output.max(0))),
                        });
                    }
                    None => {
                        let Some(thread_id) = non_empty_str(payload.get("thread_id")) else {
                            builder.partial = true;
                            continue;
                        };
                        let record_session = non_empty_str(payload.get("session_id"));
                        if payload.get("session_id").is_some() && record_session.is_none() {
                            builder.partial = true;
                            continue;
                        }
                        let owned = thread_id == session_id
                            && record_session.is_none_or(|id| id == session_id);
                        if !owned {
                            continue;
                        }
                        let timestamp = object
                            .get("timestamp")
                            .and_then(Value::as_str)
                            .and_then(timestamp_unix_ms);
                        if let Some(milliseconds) = timestamp {
                            if !in_window(milliseconds) {
                                continue;
                            }
                            if let Some(response_id) = non_empty_str(payload.get("response_id")) {
                                builder.mark_conflict(digest(&["codex", thread_id, response_id]));
                            }
                        }
                        builder.exclude();
                    }
                }
            }
            _ => {}
        }
    }

    // A legacy counter that mirrors any identified request in this file
    // (including a copied parent request) is the same usage; only unmatched
    // legacy usage is reported as excluded.
    let unmatched = legacy
        .iter()
        .filter(|candidate| {
            !candidate
                .total
                .is_some_and(|total| record_totals.contains(&total))
                && !record_usages.contains(&candidate.usage)
        })
        .count();
    builder.excluded = builder
        .excluded
        .saturating_add(i64::try_from(unmatched).unwrap_or(i64::MAX));

    let after = file_fingerprint(path);
    if before.is_none() || before.map(|(size, _)| size) != Some(parsed_bytes) || before != after {
        builder.partial = true;
    }
}

/// Decode a remote CLI export; anything unexpected fails closed.
pub(crate) fn decode_remote_ledger(
    output: &str,
    start_unix_ms: i64,
    end_unix_ms: i64,
) -> Result<CodexRequestLedger, IdentifiedUsageError> {
    if output.len() > MAX_IDENTIFIED_OUTPUT_BYTES {
        return Err(IdentifiedUsageError::InvalidReport);
    }
    let ledger: CodexRequestLedger =
        serde_json::from_str(output.trim()).map_err(|_| IdentifiedUsageError::InvalidReport)?;
    ledger.validate(start_unix_ms, end_unix_ms)?;
    Ok(ledger)
}

/// Validated inputs for `cost --identified-only`.
pub(crate) struct IdentifiedUsageArgs {
    pub(crate) identified_only: bool,
    pub(crate) provider_is_codex_only: bool,
    pub(crate) json: bool,
    pub(crate) days: Option<u32>,
    pub(crate) end_unix_ms: Option<i64>,
    pub(crate) remote: Option<String>,
    pub(crate) period_set: bool,
    pub(crate) group_by_set: bool,
    pub(crate) summary_only: bool,
    pub(crate) pretty: bool,
}

/// The resolved `(days, start, end)` window, or the shared usage error.
pub(crate) fn identified_usage_window(
    args: &IdentifiedUsageArgs,
    now_unix_ms: i64,
) -> Result<(u32, i64, i64), &'static str> {
    let days = args.days.unwrap_or(30);
    let end = args.end_unix_ms.unwrap_or(now_unix_ms);
    let valid = args.identified_only
        && args.provider_is_codex_only
        && args.json
        && !args.period_set
        && !args.group_by_set
        && !args.summary_only;
    if !valid {
        return Err(IDENTIFIED_USAGE_ARGS_ERROR);
    }
    let start = identified_window_start(days, end).ok_or(IDENTIFIED_USAGE_ARGS_ERROR)?;
    Ok((days, start, end))
}

pub(crate) async fn run_codex_identified_usage(args: &IdentifiedUsageArgs) -> anyhow::Result<()> {
    let (days, start, end) = identified_usage_window(args, chrono::Utc::now().timestamp_millis())
        .map_err(anyhow::Error::msg)?;
    let remote = args
        .remote
        .as_deref()
        .map(RemoteSessionFetcher::validate_codex_cost_host)
        .transpose()
        .map_err(anyhow::Error::msg)?;

    let result = collect_identified_usage(
        remote.as_deref(),
        start,
        end,
        || load_local_ledger(start, end),
        |host| async move {
            let output = RemoteSessionFetcher::default()
                .fetch_codex_identified_usage(&host, days, end)
                .await
                .map_err(|_| IdentifiedUsageError::Unavailable)?;
            decode_remote_ledger(&output, start, end)
        },
    )
    .await;

    println!(
        "{}",
        identified_usage_json(remote.is_some(), &result, args.pretty)?
    );
    if !result.all_sources_available {
        anyhow::bail!(
            "Identified Codex usage is incomplete: a source was unavailable; available results were retained."
        );
    }
    Ok(())
}

/// A local-only success prints the bare ledger (the document a remote host
/// returns); any other outcome prints the source reports and subtotal.
pub(crate) fn identified_usage_json(
    has_remote: bool,
    result: &CodexIdentifiedResult,
    pretty: bool,
) -> serde_json::Result<String> {
    let local_ledger = result.reports.first().and_then(|r| r.ledger.as_ref());
    let value = match (has_remote, local_ledger) {
        (false, Some(ledger)) => serde_json::to_value(ledger)?,
        _ => serde_json::to_value(result)?,
    };
    if pretty {
        serde_json::to_string_pretty(&value)
    } else {
        serde_json::to_string(&value)
    }
}

#[cfg(test)]
#[path = "identified_requests_tests.rs"]
mod tests;
