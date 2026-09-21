//! Bounded local Muse session history.
//!
//! Muse's subscription endpoint exposes quota windows but not historical token
//! usage.  The CLI records durable model turns in date-partitioned JSONL files;
//! this module reads those files without credentials and keeps only a bounded,
//! provider-owned cache of parsed events.

mod cache;
mod parse;

use cache::{CachedFile, FileStamp, digest_bytes, file_stamp, load_cache, save_cache};
use parse::{parse_line, read_bounded_line};

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

impl From<Report> for crate::spend_contract::LocalTokenHistorySummary {
    fn from(report: Report) -> Self {
        Self {
            total_tokens: report.total_tokens.unwrap_or(0),
            session_count: report.session_count,
            coverage: report.coverage,
        }
    }
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

struct ScanState<'a> {
    started: SystemTime,
    files: usize,
    bytes: u64,
    retained_event_bytes: usize,
    cancelled: Option<&'a AtomicBool>,
}

impl ScanState<'_> {
    fn check(&self) -> bool {
        if self
            .cancelled
            .is_some_and(|flag| flag.load(Ordering::Relaxed))
        {
            return true;
        }
        SystemTime::now()
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

#[cfg(windows)]
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
            Err(()) => saw_invalid_line = true,
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
        cancelled: cancel,
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
