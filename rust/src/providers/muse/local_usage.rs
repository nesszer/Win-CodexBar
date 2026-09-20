//! Bounded local Muse session history.
//!
//! Muse's subscription endpoint exposes quota windows but not historical token
//! usage.  The CLI records durable model turns in date-partitioned JSONL files;
//! this module reads those files without credentials and keeps only a bounded,
//! provider-owned cache of parsed events.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use chrono::{Local, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::spend_contract::LocalHistoryCoverage;

const CACHE_VERSION: u32 = 2;
const CACHE_FILE: &str = "muse-sessions-v1.json";
const MAX_CACHE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_FILES: usize = 20_000;
const MAX_FILE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_LINE_BYTES: usize = 4 * 1024 * 1024;
const MAX_RETAINED_EVENT_BYTES: usize = 16 * 1024 * 1024;
const SCAN_BUDGET: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DailyUsage {
    pub day: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub reasoning_tokens: u64,
    pub total_tokens: u64,
    pub request_count: u32,
    pub models: Vec<(String, u64)>,
}

#[derive(Debug, Clone)]
pub struct Report {
    pub daily: Vec<DailyUsage>,
    pub total_tokens: Option<u64>,
    pub today_tokens: Option<u64>,
    pub session_count: usize,
    pub top_model: Option<String>,
    pub coverage: LocalHistoryCoverage,
}

impl Report {
    pub fn is_available(&self) -> bool {
        self.coverage != LocalHistoryCoverage::Unavailable
    }

    pub fn is_complete(&self) -> bool {
        self.coverage == LocalHistoryCoverage::Complete
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct Event {
    id: String,
    day: String,
    model: String,
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    cache_write_tokens: u64,
    reasoning_tokens: u64,
    total_tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct FileStamp {
    identity: String,
    length: u64,
    modified_ms: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedFile {
    stamp: FileStamp,
    events: Vec<Event>,
    complete: bool,
    digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct Cache {
    version: u32,
    sessions_root: String,
    since_day: String,
    until_day: String,
    timezone: String,
    files: BTreeMap<String, CachedFile>,
}

struct ScanState {
    started: SystemTime,
    files: usize,
    bytes: u64,
    retained_event_bytes: usize,
    cancelled: Option<*const AtomicBool>,
}

impl ScanState {
    fn check(&self) -> bool {
        self.cancelled.is_some_and(|ptr| {
            // SAFETY: The pointer borrows the caller-owned cancellation flag for this scan.
            unsafe { (*ptr).load(Ordering::Relaxed) }
        }) || SystemTime::now()
            .duration_since(self.started)
            .map(|elapsed| elapsed >= SCAN_BUDGET)
            .unwrap_or(false)
    }

    fn charge_file(&mut self, bytes: u64) -> bool {
        if self.check() || self.files >= MAX_FILES {
            return false;
        }
        self.files += 1;
        self.bytes = self.bytes.saturating_add(bytes);
        self.bytes <= MAX_TOTAL_BYTES
    }

    fn charge_event(&mut self, event: &Event) -> bool {
        let estimate = 512usize.saturating_add(
            event
                .id
                .len()
                .saturating_add(event.model.len())
                .saturating_mul(6),
        );
        self.retained_event_bytes = self.retained_event_bytes.saturating_add(estimate);
        self.retained_event_bytes <= MAX_RETAINED_EVENT_BYTES
    }
}

fn sessions_root() -> PathBuf {
    if let Some(path) = std::env::var_os("MUSE_SESSIONS_DIR").filter(|v| !v.is_empty()) {
        return PathBuf::from(path);
    }
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    std::env::var_os("XDG_DATA_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local").join("share"))
        .join("muse")
        .join("sessions")
}

fn cache_root() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("CodexBar")
}

fn day_window(days: u32) -> (String, String) {
    let days = days.clamp(1, 365);
    let today = Local::now().date_naive();
    (
        (today - chrono::Duration::days(i64::from(days - 1))).to_string(),
        today.to_string(),
    )
}

fn file_stamp(path: &Path) -> Option<FileStamp> {
    let metadata = fs::metadata(path).ok()?;
    Some(FileStamp {
        identity: platform_file_identity(path, &metadata)?,
        length: metadata.len(),
        modified_ms: metadata
            .modified()
            .ok()?
            .duration_since(UNIX_EPOCH)
            .ok()?
            .as_millis(),
    })
}

#[cfg(windows)]
fn platform_file_identity(path: &Path, _metadata: &fs::Metadata) -> Option<String> {
    use std::os::windows::io::AsRawHandle;

    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
    };

    let file = File::open(path).ok()?;
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: The file handle is open for the duration of this call and the
    // output structure is valid for writes.
    let ok = unsafe { GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &mut info) };
    if ok.is_err() {
        return None;
    }
    let file_index = ((info.nFileIndexHigh as u64) << 32) | info.nFileIndexLow as u64;
    Some(format!("{}:{file_index}", info.dwVolumeSerialNumber))
}

#[cfg(unix)]
fn platform_file_identity(_path: &Path, metadata: &fs::Metadata) -> Option<String> {
    use std::os::unix::fs::MetadataExt;

    Some(format!("{}:{}", metadata.dev(), metadata.ino()))
}

#[cfg(not(any(unix, windows)))]
fn platform_file_identity(_path: &Path, metadata: &fs::Metadata) -> Option<String> {
    metadata
        .created()
        .ok()
        .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
        .map(|value| value.as_nanos().to_string())
}

fn digest_bytes(path: &Path, stamp: &FileStamp, state: &mut ScanState) -> Option<String> {
    if !state.charge_file(stamp.length) {
        return None;
    }
    let mut file = File::open(path).ok()?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut bytes_read = 0_u64;
    loop {
        let read = file.read(&mut buffer).ok()?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        bytes_read = bytes_read.checked_add(u64::try_from(read).ok()?)?;
        if state.check() {
            return None;
        }
    }
    if bytes_read != stamp.length || file_stamp(path).as_ref() != Some(stamp) {
        return None;
    }
    Some(format!("{:x}", hasher.finalize()))
}

fn cache_path(root: &Path) -> PathBuf {
    root.join("cost-usage").join(CACHE_FILE)
}

fn load_cache(root: &Path, sessions: &Path, since: &str, until: &str) -> Cache {
    let path = cache_path(root);
    let Ok(metadata) = fs::metadata(&path) else {
        return Cache::default();
    };
    if metadata.len() > MAX_CACHE_BYTES {
        return Cache::default();
    }
    let Ok(bytes) = fs::read(&path) else {
        return Cache::default();
    };
    let Ok(cache) = serde_json::from_slice::<Cache>(&bytes) else {
        return Cache::default();
    };
    if cache.version == CACHE_VERSION
        && cache.sessions_root == sessions.to_string_lossy()
        && cache.since_day == since
        && cache.until_day == until
        && cache.timezone == Local::now().offset().to_string()
    {
        cache
    } else {
        Cache::default()
    }
}

fn save_cache(root: &Path, sessions: &Path, since: &str, until: &str, mut cache: Cache) {
    cache.version = CACHE_VERSION;
    cache.sessions_root = sessions.to_string_lossy().into_owned();
    cache.since_day = since.to_string();
    cache.until_day = until.to_string();
    cache.timezone = Local::now().offset().to_string();
    let Ok(bytes) = serde_json::to_vec(&cache) else {
        return;
    };
    if bytes.len() as u64 > MAX_CACHE_BYTES {
        return;
    }
    let path = cache_path(root);
    let Some(parent) = path.parent() else { return };
    if fs::create_dir_all(parent).is_err() {
        return;
    }
    if let Err(error) = crate::atomic_file::write_atomic(&path, &bytes) {
        tracing::debug!(?error, "failed to write Muse usage cache");
    }
}

fn discover(root: &Path, state: &mut ScanState) -> (Vec<PathBuf>, bool) {
    let mut files = Vec::new();
    let mut complete = true;
    let years = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(_) => return (files, false),
    };
    for year in years {
        let year = match year {
            Ok(entry) => entry,
            Err(_) => {
                complete = false;
                continue;
            }
        };
        if state.check() {
            return (files, false);
        }
        let year_name = year.file_name().to_string_lossy().into_owned();
        if year_name.len() != 4 || year_name.parse::<u32>().is_err() {
            continue;
        }
        let months = match fs::read_dir(year.path()) {
            Ok(entries) => entries,
            Err(_) => {
                complete = false;
                continue;
            }
        };
        for month in months {
            let month = match month {
                Ok(entry) => entry,
                Err(_) => {
                    complete = false;
                    continue;
                }
            };
            let month_name = month.file_name().to_string_lossy().into_owned();
            let Ok(month_number) = month_name.parse::<u32>() else {
                continue;
            };
            if month_name.len() != 2 || !(1..=12).contains(&month_number) {
                continue;
            }
            let days = match fs::read_dir(month.path()) {
                Ok(entries) => entries,
                Err(_) => {
                    complete = false;
                    continue;
                }
            };
            for day in days {
                let day = match day {
                    Ok(entry) => entry,
                    Err(_) => {
                        complete = false;
                        continue;
                    }
                };
                let day_name = day.file_name().to_string_lossy().into_owned();
                let Ok(day_number) = day_name.parse::<u32>() else {
                    continue;
                };
                if day_name.len() != 2 || !(1..=31).contains(&day_number) {
                    continue;
                }
                let sessions = match fs::read_dir(day.path()) {
                    Ok(entries) => entries,
                    Err(_) => {
                        complete = false;
                        continue;
                    }
                };
                for session in sessions {
                    let session = match session {
                        Ok(entry) => entry,
                        Err(_) => {
                            complete = false;
                            continue;
                        }
                    };
                    if state.check() || files.len() >= MAX_FILES {
                        return (files, false);
                    }
                    let path = session.path().join("session.jsonl");
                    match fs::metadata(&path) {
                        Ok(metadata) if metadata.is_file() => files.push(path),
                        Ok(_) => {}
                        Err(_) => complete = false,
                    }
                }
            }
        }
    }
    (files, complete)
}

fn integer(value: Option<&Value>) -> Option<u64> {
    value?.as_u64()
}

fn counter(value: &Value, names: &[&str]) -> Option<u64> {
    let mut found = None;
    for name in names {
        if let Some(raw) = value.get(*name) {
            let parsed = integer(Some(raw))?;
            if found.is_some_and(|previous| previous != parsed) {
                return None;
            }
            found = Some(parsed);
        }
    }
    Some(found.unwrap_or(0))
}

fn has_token_fields(value: &Value) -> bool {
    [
        "input_tokens",
        "output_tokens",
        "total_tokens",
        "cache_read_tokens",
        "cached_input_tokens",
        "cached_tokens",
        "cache_write_tokens",
        "reasoning_tokens",
    ]
    .iter()
    .any(|key| value.get(*key).is_some())
}

fn parse_line(line: &[u8]) -> Result<Option<Event>, bool> {
    let Ok(object) = serde_json::from_slice::<Value>(line) else {
        return Err(true);
    };
    let valid_envelope = object.get("schema_version").and_then(Value::as_u64) == Some(1)
        && object.get("record_type").and_then(Value::as_str) == Some("event")
        && object.get("payload_type").and_then(Value::as_str) == Some("runtime.session")
        && object.get("payload_schema_version").and_then(Value::as_u64) == Some(1);
    if !valid_envelope {
        return Err(true);
    }
    let event = object
        .get("payload")
        .ok_or(true)?
        .get("event")
        .and_then(Value::as_object);
    let Some(event) = event else { return Err(true) };
    let kind = event
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if matches!(
        kind,
        "resource_usage_sampled" | "workflow_child_lifecycle" | "goal_usage_attribution"
    ) {
        return Ok(None);
    }
    if !matches!(kind, "model_completed" | "automated_review_completed") {
        return if event.get("usage").is_some_and(has_token_fields) {
            Err(true)
        } else {
            Ok(None)
        };
    }
    let usage = event.get("usage").and_then(Value::as_object).ok_or(true)?;
    let id = object
        .get("id")
        .and_then(Value::as_str)
        .filter(|v| !v.is_empty())
        .ok_or(true)?;
    let micros = integer(object.get("recorded_at"))
        .filter(|v| *v > 0)
        .ok_or(true)?;
    let input = integer(usage.get("input_tokens")).ok_or(true)?;
    let output = integer(usage.get("output_tokens")).ok_or(true)?;
    let total = input.checked_add(output).ok_or(true)?;
    if usage.get("total_tokens").is_some() && integer(usage.get("total_tokens")) != Some(total) {
        return Err(true);
    }
    let cache_read = counter(
        &Value::Object(usage.clone()),
        &["cache_read_tokens", "cached_input_tokens", "cached_tokens"],
    )
    .ok_or(true)?;
    let cache_write =
        counter(&Value::Object(usage.clone()), &["cache_write_tokens"]).ok_or(true)?;
    let reasoning = counter(&Value::Object(usage.clone()), &["reasoning_tokens"]).ok_or(true)?;
    if cache_read > input || cache_write > input || reasoning > output {
        return Err(true);
    }
    let model = event
        .get("model")
        .and_then(Value::as_str)
        .or_else(|| {
            event
                .get("model")
                .and_then(|v| v.get("model_id"))
                .and_then(Value::as_str)
        })
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .unwrap_or("unknown")
        .to_string();
    let seconds = micros / 1_000_000;
    let nanos = u32::try_from((micros % 1_000_000) * 1_000).map_err(|_| true)?;
    let seconds = i64::try_from(seconds).map_err(|_| true)?;
    let timestamp = Utc.timestamp_opt(seconds, nanos).single().ok_or(true)?;
    Ok(Some(Event {
        id: id.to_string(),
        day: timestamp.with_timezone(&Local).date_naive().to_string(),
        model,
        input_tokens: input,
        output_tokens: output,
        cache_read_tokens: cache_read,
        cache_write_tokens: cache_write,
        reasoning_tokens: reasoning,
        total_tokens: total,
    }))
}

fn read_bounded_line<R: BufRead>(
    reader: &mut R,
    max_bytes: usize,
) -> std::io::Result<Option<Vec<u8>>> {
    let mut line = Vec::new();
    let mut saw_input = false;
    let mut discarding = false;
    loop {
        let chunk = reader.fill_buf()?;
        if chunk.is_empty() {
            return Ok(saw_input.then_some(if discarding { Vec::new() } else { line }));
        }
        let newline = chunk.iter().position(|byte| *byte == b'\n');
        let segment_len = newline.unwrap_or(chunk.len());
        let consumed_len = segment_len + usize::from(newline.is_some());
        saw_input = true;
        if !discarding {
            if line.len().saturating_add(consumed_len) <= max_bytes {
                line.extend_from_slice(&chunk[..segment_len]);
                if newline.is_some() {
                    line.push(b'\n');
                }
            } else {
                line.clear();
                discarding = true;
            }
        }
        reader.consume(consumed_len);
        if newline.is_some() {
            return Ok(Some(if discarding { Vec::new() } else { line }));
        }
    }
}

fn parse_file(
    path: &Path,
    stamp: &FileStamp,
    state: &mut ScanState,
    since: &str,
    until: &str,
) -> (Vec<Event>, bool, Option<String>) {
    if stamp.length > MAX_FILE_BYTES || !state.charge_file(stamp.length) {
        return (Vec::new(), false, None);
    }
    let Ok(file) = fs::File::open(path) else {
        return (Vec::new(), false, None);
    };
    let mut reader = BufReader::new(file);
    let mut events = Vec::new();
    let mut hasher = Sha256::new();
    let mut complete = true;
    let mut saw_in_window_event = false;
    let mut saw_invalid_line = false;
    loop {
        let line = match read_bounded_line(&mut reader, MAX_LINE_BYTES) {
            Ok(Some(line)) => line,
            Ok(None) => break,
            Err(_) => return (events, false, None),
        };
        if line.is_empty() {
            complete = false;
            break;
        }
        hasher.update(&line);
        match parse_line(&line) {
            Ok(Some(event)) => {
                if event.day.as_str() < since || event.day.as_str() > until {
                    continue;
                }
                saw_in_window_event = true;
                if !state.charge_event(&event) {
                    return (events, false, None);
                }
                events.push(event);
            }
            Ok(None) => {}
            Err(drift) => saw_invalid_line |= drift,
        }
        if state.check() {
            return (events, false, None);
        }
    }
    if saw_invalid_line && saw_in_window_event {
        complete = false;
    }
    let stable = complete && file_stamp(path).as_ref() == Some(stamp);
    let digest = stable.then(|| format!("{:x}", hasher.finalize()));
    (events, stable && digest.is_some(), digest)
}

fn add_checked(target: &mut u64, value: u64) -> bool {
    if let Some(next) = target.checked_add(value) {
        *target = next;
        true
    } else {
        false
    }
}

pub fn scan(days: u32, cancel: Option<&AtomicBool>) -> Report {
    let (since, until) = day_window(days);
    let root = sessions_root();
    let cache = cache_root();
    scan_in(&root, &cache, &since, &until, cancel)
}

pub fn scan_in(
    root: &Path,
    cache_root: &Path,
    since: &str,
    until: &str,
    cancel: Option<&AtomicBool>,
) -> Report {
    if !root.is_dir() {
        return Report {
            daily: Vec::new(),
            total_tokens: None,
            today_tokens: None,
            session_count: 0,
            top_model: None,
            coverage: LocalHistoryCoverage::Unavailable,
        };
    }
    let mut state = ScanState {
        started: SystemTime::now(),
        files: 0,
        bytes: 0,
        retained_event_bytes: 0,
        cancelled: cancel.map(|flag| flag as *const _),
    };
    let mut cache_data = load_cache(cache_root, root, since, until);
    let (paths, discovery_complete) = discover(root, &mut state);
    let paths_discovered = paths.len();
    let mut seen = HashMap::<String, Event>::new();
    let mut days = BTreeMap::<String, DailyUsage>::new();
    let mut complete = discovery_complete;
    let mut scanned = HashSet::new();
    let mut sessions_with_usage = 0_usize;
    for path in paths {
        let key = path.to_string_lossy().into_owned();
        scanned.insert(key.clone());
        let Some(stamp) = file_stamp(&path) else {
            complete = false;
            continue;
        };
        let cached = cache_data
            .files
            .get(&key)
            .filter(|entry| entry.stamp == stamp && entry.complete)
            .cloned();
        let cached = cached.filter(|entry| {
            digest_bytes(&path, &stamp, &mut state).as_deref() == Some(entry.digest.as_str())
        });
        let (events, file_complete, digest) = cached
            .map(|entry| (entry.events, true, Some(entry.digest)))
            .unwrap_or_else(|| parse_file(&path, &stamp, &mut state, since, until));
        cache_data.files.insert(
            key,
            CachedFile {
                stamp,
                events: events.clone(),
                complete: file_complete,
                digest: digest.unwrap_or_default(),
            },
        );
        complete &= file_complete;
        let mut file_had_usage = false;
        for event in events
            .into_iter()
            .filter(|event| event.day.as_str() >= since && event.day.as_str() <= until)
        {
            if let Some(previous) = seen.get(&event.id) {
                if previous != &event {
                    complete = false;
                }
                continue;
            }
            file_had_usage = true;
            seen.insert(event.id.clone(), event.clone());
            let day = days.entry(event.day.clone()).or_insert_with(|| DailyUsage {
                day: event.day.clone(),
                input_tokens: 0,
                output_tokens: 0,
                cache_read_tokens: 0,
                cache_write_tokens: 0,
                reasoning_tokens: 0,
                total_tokens: 0,
                request_count: 0,
                models: Vec::new(),
            });
            let model_idx = day
                .models
                .iter()
                .position(|(model, _)| model == &event.model);
            let model_total = model_idx.map(|idx| day.models[idx].1).unwrap_or(0);
            let valid = add_checked(&mut day.input_tokens, event.input_tokens)
                && add_checked(&mut day.output_tokens, event.output_tokens)
                && add_checked(&mut day.cache_read_tokens, event.cache_read_tokens)
                && add_checked(&mut day.cache_write_tokens, event.cache_write_tokens)
                && add_checked(&mut day.reasoning_tokens, event.reasoning_tokens)
                && add_checked(&mut day.total_tokens, event.total_tokens)
                && day
                    .request_count
                    .checked_add(1)
                    .map(|next| {
                        day.request_count = next;
                        true
                    })
                    .unwrap_or(false)
                && model_total
                    .checked_add(event.total_tokens)
                    .map(|next| {
                        if let Some(idx) = model_idx {
                            day.models[idx].1 = next;
                        } else {
                            day.models.push((event.model.clone(), next));
                        }
                        true
                    })
                    .unwrap_or(false);
            complete &= valid;
        }
        if file_had_usage {
            sessions_with_usage = sessions_with_usage.saturating_add(1);
        }
        if state.check() {
            complete = false;
            break;
        }
    }
    if complete {
        cache_data.files.retain(|path, _| scanned.contains(path));
    }
    save_cache(cache_root, root, since, until, cache_data);

    let mut daily: Vec<_> = days.into_values().collect();
    daily.sort_by(|a, b| a.day.cmp(&b.day));
    let total_tokens = daily
        .iter()
        .try_fold(0u64, |total, day| total.checked_add(day.total_tokens));
    if total_tokens.is_none() {
        complete = false;
    }
    let today = daily
        .last()
        .filter(|day| day.day == until)
        .map(|day| day.total_tokens);
    let mut model_totals = BTreeMap::<String, u64>::new();
    for day in &daily {
        for (model, total) in &day.models {
            *model_totals.entry(model.clone()).or_default() = model_totals
                .get(model)
                .copied()
                .unwrap_or(0)
                .saturating_add(*total);
        }
    }
    let top_model = model_totals
        .into_iter()
        .max_by_key(|(_, total)| *total)
        .map(|(model, _)| model);
    let coverage = if paths_discovered == 0 {
        if discovery_complete {
            LocalHistoryCoverage::Unavailable
        } else {
            LocalHistoryCoverage::Partial
        }
    } else if complete {
        LocalHistoryCoverage::Complete
    } else {
        LocalHistoryCoverage::Partial
    };
    Report {
        daily,
        total_tokens,
        today_tokens: today,
        session_count: sessions_with_usage,
        top_model,
        coverage,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn record(id: &str, micros: u64, kind: &str, usage: &str, model: &str) -> String {
        format!(
            r#"{{"schema_version":1,"id":"{id}","recorded_at":{micros},"record_type":"event","payload_type":"runtime.session","payload_schema_version":1,"payload":{{"event":{{"kind":"{kind}","model":"{model}","usage":{usage}}}}}}}"#
        )
    }

    #[test]
    fn parses_token_turns_without_double_counting_cache_or_reasoning() {
        let value = record(
            "e1",
            1_788_177_600_000_000,
            "model_completed",
            r#"{"input_tokens":10,"output_tokens":2,"cached_tokens":9,"reasoning_tokens":1}"#,
            "muse-1",
        );
        assert_eq!(
            parse_line(value.as_bytes()).unwrap().unwrap().total_tokens,
            12
        );
    }

    #[test]
    fn ignores_telemetry_and_rejects_unknown_token_shapes() {
        let telemetry = record(
            "e1",
            1_788_177_600_000_000,
            "resource_usage_sampled",
            r#"{"cpu_self_ms":1}"#,
            "unknown",
        );
        assert_eq!(parse_line(telemetry.as_bytes()).unwrap(), None);
        let unknown = record(
            "e2",
            1_788_177_600_000_000,
            "future_inference",
            r#"{"input_tokens":1,"output_tokens":1}"#,
            "muse-1",
        );
        assert_eq!(parse_line(unknown.as_bytes(),), Err(true));

        let schema_drift = telemetry.replace("\"schema_version\":1", "\"schema_version\":2");
        assert_eq!(parse_line(schema_drift.as_bytes()), Err(true));
    }

    #[test]
    fn scans_deduplicated_events_and_reuses_cache() {
        let root = tempdir().unwrap();
        let session = root.path().join("2026/08/31/a");
        fs::create_dir_all(&session).unwrap();
        let first = record(
            "first",
            1_788_177_600_000_000,
            "model_completed",
            r#"{"input_tokens":10,"output_tokens":2}"#,
            "muse-1",
        );
        let second = record(
            "second",
            1_788_177_601_000_000,
            "model_completed",
            r#"{"input_tokens":20,"output_tokens":4}"#,
            "muse-1",
        );
        fs::write(
            session.join("session.jsonl"),
            format!("{first}\n{second}\n{first}\n"),
        )
        .unwrap();
        let duplicate_session = root.path().join("2026/08/31/b");
        fs::create_dir_all(&duplicate_session).unwrap();
        fs::write(
            duplicate_session.join("session.jsonl"),
            format!("{first}\n"),
        )
        .unwrap();
        let cache = tempdir().unwrap();
        let cold = scan_in(root.path(), cache.path(), "2026-08-31", "2026-08-31", None);
        let warm = scan_in(root.path(), cache.path(), "2026-08-31", "2026-08-31", None);
        assert_eq!(cold.coverage, LocalHistoryCoverage::Complete);
        assert_eq!(cold.total_tokens, Some(36));
        assert_eq!(cold.session_count, 1);
        assert_eq!(warm.total_tokens, cold.total_tokens);
    }

    #[test]
    fn aggregate_overflow_downgrades_coverage_without_publishing_total() {
        let root = tempdir().unwrap();
        let first_session = root.path().join("2026/08/30/first");
        let second_session = root.path().join("2026/08/31/second");
        fs::create_dir_all(&first_session).unwrap();
        fs::create_dir_all(&second_session).unwrap();
        let value = u64::MAX;
        fs::write(
            first_session.join("session.jsonl"),
            record(
                "first",
                1_777_000_000_000_000,
                "model_completed",
                &format!(r#"{{"input_tokens":{value},"output_tokens":0}}"#),
                "muse-1",
            ),
        )
        .unwrap();
        fs::write(
            second_session.join("session.jsonl"),
            record(
                "second",
                1_777_086_400_000_000,
                "model_completed",
                &format!(r#"{{"input_tokens":{value},"output_tokens":0}}"#),
                "muse-1",
            ),
        )
        .unwrap();

        let report = scan_in(
            root.path(),
            tempdir().unwrap().path(),
            "2026-04-01",
            "2026-12-31",
            None,
        );
        assert_eq!(report.total_tokens, None);
        assert_eq!(report.coverage, LocalHistoryCoverage::Partial);
    }

    #[test]
    fn complete_scan_with_only_ignored_records_is_a_known_zero() {
        let root = tempdir().unwrap();
        let session = root.path().join("2026/08/31/empty");
        fs::create_dir_all(&session).unwrap();
        fs::write(
            session.join("session.jsonl"),
            record(
                "telemetry",
                1_788_177_600_000_000,
                "resource_usage_sampled",
                r#"{"cpu_self_ms":1}"#,
                "unknown",
            ),
        )
        .unwrap();

        let report = scan_in(
            root.path(),
            tempdir().unwrap().path(),
            "2026-08-31",
            "2026-08-31",
            None,
        );
        assert!(report.is_available());
        assert!(report.is_complete());
        assert_eq!(report.total_tokens, Some(0));
        assert_eq!(report.coverage, LocalHistoryCoverage::Complete);
    }

    #[test]
    fn discovery_errors_downgrade_coverage() {
        let root = tempdir().unwrap();
        fs::write(root.path().join("2026"), b"not a directory").unwrap();

        let report = scan_in(
            root.path(),
            tempdir().unwrap().path(),
            "2026-08-31",
            "2026-08-31",
            None,
        );
        assert!(!report.is_complete());
        assert_eq!(report.coverage, LocalHistoryCoverage::Partial);
    }

    #[test]
    fn old_malformed_history_does_not_downgrade_current_window() {
        let root = tempdir().unwrap();
        let old_session = root.path().join("2020/01/01/old");
        fs::create_dir_all(&old_session).unwrap();
        fs::write(old_session.join("session.jsonl"), b"not-json\n").unwrap();

        let report = scan_in(
            root.path(),
            tempdir().unwrap().path(),
            "2026-08-31",
            "2026-08-31",
            None,
        );
        assert_eq!(report.coverage, LocalHistoryCoverage::Complete);
        assert_eq!(report.total_tokens, Some(0));
    }

    #[test]
    fn continuing_session_in_old_directory_contributes_current_event() {
        let root = tempdir().unwrap();
        let old_session = root.path().join("2020/01/01/old");
        fs::create_dir_all(&old_session).unwrap();
        fs::write(
            old_session.join("session.jsonl"),
            record(
                "current",
                1_788_177_600_000_000,
                "model_completed",
                r#"{"input_tokens":10,"output_tokens":2}"#,
                "muse-1",
            ),
        )
        .unwrap();

        let report = scan_in(
            root.path(),
            tempdir().unwrap().path(),
            "2026-08-31",
            "2026-08-31",
            None,
        );
        assert_eq!(report.coverage, LocalHistoryCoverage::Complete);
        assert_eq!(report.total_tokens, Some(12));
        assert_eq!(report.session_count, 1);
    }

    #[test]
    fn oversized_line_is_discarded_without_consuming_the_next_record() {
        let mut input = vec![b'x'; MAX_LINE_BYTES + 1];
        input.push(b'\n');
        input.extend_from_slice(b"{}\n");
        let mut reader = BufReader::new(std::io::Cursor::new(input));
        assert!(
            read_bounded_line(&mut reader, MAX_LINE_BYTES)
                .unwrap()
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            read_bounded_line(&mut reader, MAX_LINE_BYTES)
                .unwrap()
                .unwrap(),
            b"{}\n"
        );
    }

    #[test]
    fn content_digest_changes_when_same_size_content_changes() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("session.jsonl");
        fs::write(&path, b"aaaaaaaa").unwrap();
        let stamp = file_stamp(&path).unwrap();
        let mut state = ScanState {
            started: SystemTime::now(),
            files: 0,
            bytes: 0,
            retained_event_bytes: 0,
            cancelled: None,
        };
        let before = digest_bytes(&path, &stamp, &mut state).unwrap();
        fs::write(&path, b"bbbbbbbb").unwrap();
        let after = digest_bytes(&path, &stamp, &mut state);
        assert_ne!(after.as_deref(), Some(before.as_str()));
    }
}
