//! In-process cache of parsed Claude transcript lines.
//!
//! Claude Code transcripts are append-only JSONL files, and one Usage & Spend
//! build walks them several times (7-day, 30-day and selected-period
//! summaries, the 365-day token activity grid and the trend history). Without
//! a cache every walk re-read and re-parsed every transcript in its window,
//! which on a large `~/.claude/projects` tree means gigabytes of JSON per
//! build.
//!
//! This cache keeps, per transcript path, the lines that matter to cost
//! scanning in the order they appear, and remembers how far the file was
//! parsed. A later scan reuses an unchanged file as is and parses only the
//! bytes appended since. Everything that depends on the walk rather than on the
//! file (the cutoff, cross-file de-duplication, incomplete-request
//! reconciliation and pricing) is still applied on every scan when the lines
//! are replayed, so results match a cold scan.
//!
//! Claude Code writes one line per content block of an assistant message, and
//! each of those lines repeats the message's usage under the same dedup key.
//! A later line whose usage equals the first line with its key is stored as a
//! small [`ClaudeParsedLine::Repeat`] (an index and its own timestamp) instead
//! of a second copy of the record, which keeps the cache close to one full
//! entry per request without changing any scan result.
//!
//! A file is re-parsed from the start when it shrank or when the bytes just
//! before the cached offset no longer match (the file was replaced). A final
//! line without a trailing newline is parsed on every scan and never
//! committed, because the writer may still be appending to it.

use std::collections::HashMap;
use std::fs::File;
use std::hash::{DefaultHasher, Hasher};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, LazyLock, Mutex, PoisonError};
use std::time::{Duration, Instant, SystemTime};

use chrono::{DateTime, Utc};

use super::claude_pricing::ClaudeScanPricingResolver;
use super::claude_scan::claude_parsed_line;
use super::claude_usage::ClaudeUsageDedupKey;
use super::{ClaudeUsageRecord, is_cancelled};

/// Upper bound on cached lines across all transcripts. Past it, new files are
/// still scanned but not retained, so memory stays bounded on huge trees.
const MAX_CACHED_LINES: usize = 4_000_000;

/// Bytes before the committed offset that must still match to resume.
const FINGERPRINT_BYTES: u64 = 4096;

/// How often entries for deleted transcripts are dropped.
const PRUNE_INTERVAL: Duration = Duration::from_secs(30 * 60);

/// One transcript line that cost scanning acts on.
#[derive(Debug, Clone)]
pub(super) enum ClaudeParsedLine {
    /// A line that was not valid UTF-8 or not valid JSON.
    Malformed,
    /// A preliminary (incomplete) usage row. `row` is absent when the event
    /// carried no message, which still counts as an incomplete request.
    Preliminary {
        row: Option<(Option<ClaudeUsageDedupKey>, String, Option<DateTime<Utc>>)>,
    },
    /// A usage row. Pricing fields hold the values from when it was parsed;
    /// they are recomputed on replay.
    Usage {
        record: ClaudeUsageRecord,
        cache_create_1h: u64,
    },
    /// A usage line equal to the earlier `Usage` line at index `of` (in
    /// [`ClaudeFileLines::get`] order) except for its timestamp.
    Repeat {
        of: usize,
        timestamp: Option<DateTime<Utc>>,
    },
}

/// Parsed lines of one transcript.
#[derive(Debug, Default)]
pub(super) struct ClaudeFileLines {
    /// Lines that ended with a newline, up to `committed_len`.
    pub(super) committed: Vec<ClaudeParsedLine>,
    /// A trailing line without a newline, if any.
    pub(super) tail: Vec<ClaudeParsedLine>,
    committed_len: u64,
    file_len: u64,
    modified: Option<SystemTime>,
    fingerprint: Option<u64>,
}

impl ClaudeFileLines {
    pub(super) fn lines(&self) -> impl Iterator<Item = &ClaudeParsedLine> {
        self.committed.iter().chain(self.tail.iter())
    }

    /// The line at `index`, counting committed lines first.
    pub(super) fn get(&self, index: usize) -> Option<&ClaudeParsedLine> {
        match index.checked_sub(self.committed.len()) {
            None => self.committed.get(index),
            Some(tail_index) => self.tail.get(tail_index),
        }
    }
}

/// Lines for one scan, plus the read failures seen while producing them.
pub(super) struct ClaudeFileSnapshot {
    pub(super) lines: Arc<ClaudeFileLines>,
    pub(super) read_failures: u32,
}

#[derive(Default)]
struct ClaudeFileCache {
    entries: HashMap<PathBuf, Arc<ClaudeFileLines>>,
    cached_lines: usize,
    last_prune: Option<Instant>,
}

impl ClaudeFileCache {
    fn insert(&mut self, path: PathBuf, lines: Arc<ClaudeFileLines>) {
        let added = lines.committed.len() + lines.tail.len();
        let removed = self
            .entries
            .get(&path)
            .map_or(0, |old| old.committed.len() + old.tail.len());
        if self.cached_lines - removed + added > MAX_CACHED_LINES {
            if let Some(old) = self.entries.remove(&path) {
                self.cached_lines -= old.committed.len() + old.tail.len();
            }
            return;
        }
        self.cached_lines = self.cached_lines - removed + added;
        self.entries.insert(path, lines);
    }

    fn remove(&mut self, path: &Path) {
        if let Some(old) = self.entries.remove(path) {
            self.cached_lines -= old.committed.len() + old.tail.len();
        }
    }
}

static CACHE: LazyLock<Mutex<ClaudeFileCache>> =
    LazyLock::new(|| Mutex::new(ClaudeFileCache::default()));

fn cache() -> std::sync::MutexGuard<'static, ClaudeFileCache> {
    CACHE.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Lines of `path`, parsing only what the cache does not already hold.
///
/// `file` must be an open handle to `path`.
pub(super) fn claude_file_lines(
    path: &Path,
    mut file: File,
    cancel: Option<&AtomicBool>,
    pricing: &mut ClaudeScanPricingResolver,
) -> ClaudeFileSnapshot {
    prune_deleted_entries();
    let (file_len, modified) = match file.metadata() {
        Ok(metadata) => (metadata.len(), metadata.modified().ok()),
        Err(_) => (0, None),
    };
    let cached = cache().entries.get(path).cloned();

    if let Some(cached) = &cached
        && modified.is_some()
        && cached.modified == modified
        && cached.file_len == file_len
    {
        return ClaudeFileSnapshot {
            lines: Arc::clone(cached),
            read_failures: 0,
        };
    }

    let base = cached.filter(|cached| {
        cached.committed_len <= file_len
            && cached.fingerprint.is_some()
            && fingerprint(&mut file, cached.committed_len) == cached.fingerprint
    });
    let start = base.as_ref().map_or(0, |base| base.committed_len);
    let segment = parse_segment(&mut file, start, cancel, pricing);

    let mut committed = base
        .as_ref()
        .map(|base| base.committed.clone())
        .unwrap_or_default();
    let mut anchors = RepeatAnchors::default();
    for (index, line) in committed.iter().enumerate() {
        anchors.note(index, line);
    }
    for line in segment.committed {
        let line = anchors.compact(&committed, &[], line);
        anchors.note(committed.len(), &line);
        committed.push(line);
    }
    let mut tail = Vec::with_capacity(segment.tail.len());
    for line in segment.tail {
        let line = anchors.compact(&committed, &tail, line);
        anchors.note(committed.len() + tail.len(), &line);
        tail.push(line);
    }
    let committed_len = start + segment.committed_bytes;
    let lines = Arc::new(ClaudeFileLines {
        committed,
        tail,
        committed_len,
        file_len,
        modified,
        fingerprint: fingerprint(&mut file, committed_len),
    });

    // A partial walk (read error or cancellation) or a file without a usable
    // modification time is never cached, so the next scan reads it again.
    let complete = !segment.read_failed && !segment.cancelled && modified.is_some();
    let mut cache = cache();
    if complete {
        cache.insert(path.to_path_buf(), Arc::clone(&lines));
    } else {
        cache.remove(path);
    }
    drop(cache);

    ClaudeFileSnapshot {
        lines,
        read_failures: u32::from(segment.read_failed),
    }
}

struct ParsedSegment {
    committed: Vec<ClaudeParsedLine>,
    committed_bytes: u64,
    tail: Vec<ClaudeParsedLine>,
    read_failed: bool,
    cancelled: bool,
}

/// Parse `file` from `start` to its end. Lines ending with a newline are
/// committed; a final unterminated line goes to `tail`.
fn parse_segment(
    file: &mut File,
    start: u64,
    cancel: Option<&AtomicBool>,
    pricing: &mut ClaudeScanPricingResolver,
) -> ParsedSegment {
    let mut segment = ParsedSegment {
        committed: Vec::new(),
        committed_bytes: 0,
        tail: Vec::new(),
        read_failed: false,
        cancelled: false,
    };
    if file.seek(SeekFrom::Start(start)).is_err() {
        segment.read_failed = true;
        return segment;
    }
    let mut reader = BufReader::new(file);
    let mut consumed = 0u64;
    let mut buf = Vec::new();
    loop {
        buf.clear();
        let read = match reader.read_until(b'\n', &mut buf) {
            Ok(0) => break,
            Ok(read) => read,
            Err(_) => {
                segment.read_failed = true;
                break;
            }
        };
        if is_cancelled(cancel) {
            segment.cancelled = true;
            break;
        }
        consumed += read as u64;
        let terminated = buf.last() == Some(&b'\n');
        while matches!(buf.last(), Some(b'\n' | b'\r')) {
            buf.pop();
        }
        let parsed = match std::str::from_utf8(&buf) {
            Ok(line) => claude_parsed_line(line, pricing),
            Err(_) => Some(ClaudeParsedLine::Malformed),
        };
        if terminated {
            segment.committed.extend(parsed);
            segment.committed_bytes = consumed;
        } else {
            segment.tail.extend(parsed);
        }
    }
    segment
}

/// Index of the first `Usage` line for each dedup key in one file.
#[derive(Default)]
struct RepeatAnchors {
    first: HashMap<ClaudeUsageDedupKey, usize>,
}

impl RepeatAnchors {
    /// Remember `line` (at `index`) as the anchor for its key if it is the
    /// first `Usage` line with that key.
    fn note(&mut self, index: usize, line: &ClaudeParsedLine) {
        if let ClaudeParsedLine::Usage { record, .. } = line
            && let Some(key) = &record.dedup_key
            && !self.first.contains_key(key)
        {
            self.first.insert(key.clone(), index);
        }
    }

    /// `line`, or a `Repeat` of its anchor when only the timestamp differs.
    /// `committed` and `tail` are the lines before it.
    fn compact(
        &self,
        committed: &[ClaudeParsedLine],
        tail: &[ClaudeParsedLine],
        line: ClaudeParsedLine,
    ) -> ClaudeParsedLine {
        let ClaudeParsedLine::Usage {
            record,
            cache_create_1h,
        } = &line
        else {
            return line;
        };
        let Some(&of) = record
            .dedup_key
            .as_ref()
            .and_then(|key| self.first.get(key))
        else {
            return line;
        };
        let anchor = match of.checked_sub(committed.len()) {
            None => committed.get(of),
            Some(tail_index) => tail.get(tail_index),
        };
        let Some(ClaudeParsedLine::Usage {
            record: anchor,
            cache_create_1h: anchor_1h,
        }) = anchor
        else {
            return line;
        };
        let same_usage = anchor.model == record.model
            && anchor.dedup_key == record.dedup_key
            && anchor.input == record.input
            && anchor.output == record.output
            && anchor.cache_create == record.cache_create
            && anchor.cache_read == record.cache_read
            && anchor_1h == cache_create_1h;
        if !same_usage {
            return line;
        }
        ClaudeParsedLine::Repeat {
            of,
            timestamp: record.timestamp,
        }
    }
}

/// Hash of up to [`FINGERPRINT_BYTES`] ending at `end`, or `None` when the
/// bytes cannot be read. An empty prefix has a fixed fingerprint.
fn fingerprint(file: &mut File, end: u64) -> Option<u64> {
    let start = end.saturating_sub(FINGERPRINT_BYTES);
    file.seek(SeekFrom::Start(start)).ok()?;
    let len = usize::try_from(end - start).ok()?;
    let mut bytes = vec![0u8; len];
    file.read_exact(&mut bytes).ok()?;
    let mut hasher = DefaultHasher::new();
    hasher.write_u64(end);
    hasher.write(&bytes);
    Some(hasher.finish())
}

/// Drop entries whose transcript no longer exists, at most once per
/// [`PRUNE_INTERVAL`]. File checks run without holding the cache lock.
fn prune_deleted_entries() {
    let paths: Vec<PathBuf> = {
        let mut cache = cache();
        let now = Instant::now();
        if cache
            .last_prune
            .is_some_and(|last| now.duration_since(last) < PRUNE_INTERVAL)
        {
            return;
        }
        cache.last_prune = Some(now);
        cache.entries.keys().cloned().collect()
    };
    let missing: Vec<PathBuf> = paths.into_iter().filter(|path| !path.exists()).collect();
    if missing.is_empty() {
        return;
    }
    let mut cache = cache();
    for path in &missing {
        cache.remove(path);
    }
}

#[cfg(test)]
pub(super) fn cached_line_count(path: &Path) -> Option<usize> {
    cache().entries.get(path).map(|lines| lines.committed.len())
}

#[cfg(test)]
pub(super) fn cached_repeat_count(path: &Path) -> Option<usize> {
    cache().entries.get(path).map(|lines| {
        lines
            .committed
            .iter()
            .filter(|line| matches!(line, ClaudeParsedLine::Repeat { .. }))
            .count()
    })
}
