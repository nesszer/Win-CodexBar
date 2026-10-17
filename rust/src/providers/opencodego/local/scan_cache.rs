//! Reuse of unchanged OpenCode Go SQLite scans (upstream #4410 / #4445).
//!
//! A scan is keyed by the database file and its SQLite `-wal` sidecar (size
//! and modification time), so WAL commits, checkpoints, and sidecar removal
//! all invalidate it. Only one parsed-row payload of at most
//! [`MAX_CACHED_PAYLOAD_BYTES`] stays resident; larger histories are still
//! read, just not retained between scans.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::SystemTime;

use super::UsageRow;

/// Row storage plus model text, excluding allocator overhead.
const MAX_CACHED_PAYLOAD_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileStamp {
    size: u64,
    modified: Option<SystemTime>,
}

impl FileStamp {
    fn of(path: &Path) -> Option<Self> {
        let metadata = std::fs::metadata(path).ok()?;
        if !metadata.is_file() {
            return None;
        }
        Some(Self {
            size: metadata.len(),
            modified: metadata.modified().ok(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DatabaseFingerprint {
    path: PathBuf,
    database: FileStamp,
    wal: Option<FileStamp>,
}

impl DatabaseFingerprint {
    fn current(db_path: &Path) -> Option<Self> {
        let path = std::fs::canonicalize(db_path).unwrap_or_else(|_| db_path.to_path_buf());
        let database = FileStamp::of(&path)?;
        let wal = FileStamp::of(&wal_path(&path));
        Some(Self {
            path,
            database,
            wal,
        })
    }
}

fn wal_path(db_path: &Path) -> PathBuf {
    crate::core::sqlite_sidecar_path(db_path, "-wal")
}

fn fits_cache_budget(rows: &[UsageRow]) -> bool {
    let row_size = std::mem::size_of::<UsageRow>();
    if rows.len() > MAX_CACHED_PAYLOAD_BYTES / row_size {
        return false;
    }
    let payload = rows
        .iter()
        .fold(0usize, |total, row| total + row_size + row.model.len());
    payload <= MAX_CACHED_PAYLOAD_BYTES
}

type CacheEntry = (DatabaseFingerprint, Arc<Vec<UsageRow>>);

pub(in crate::providers::opencodego) struct ScanCache {
    entry: Mutex<Option<CacheEntry>>,
}

impl ScanCache {
    pub(in crate::providers::opencodego) const fn new() -> Self {
        Self {
            entry: Mutex::new(None),
        }
    }

    /// Process-wide cache shared by the provider fetch and cost-history scans.
    pub(in crate::providers::opencodego) fn shared() -> &'static Self {
        static SHARED: ScanCache = ScanCache::new();
        &SHARED
    }

    /// Return cached rows when the database fingerprint is unchanged,
    /// otherwise run `scan`. The lock is held across the scan so concurrent
    /// refreshes share one read instead of scanning the same file twice.
    pub(in crate::providers::opencodego) fn read<E>(
        &self,
        db_path: &Path,
        scan: impl FnOnce() -> Result<Vec<UsageRow>, E>,
    ) -> Result<Arc<Vec<UsageRow>>, E> {
        let mut entry = self.entry.lock().unwrap_or_else(PoisonError::into_inner);
        let fingerprint = DatabaseFingerprint::current(db_path);
        if let (Some(fingerprint), Some((cached, rows))) = (&fingerprint, entry.as_ref())
            && fingerprint == cached
        {
            return Ok(Arc::clone(rows));
        }
        *entry = None;
        let rows = Arc::new(scan()?);
        // A commit landing mid-scan changes the fingerprint; caching those rows
        // under the pre-scan key would serve stale data until the next write.
        if let Some(fingerprint) = fingerprint
            && DatabaseFingerprint::current(db_path).as_ref() == Some(&fingerprint)
            && fits_cache_budget(&rows)
        {
            *entry = Some((fingerprint, Arc::clone(&rows)));
        }
        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::io::Write;

    fn temp_dir(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        let dir = std::env::temp_dir().join(format!(
            "codexbar-opencodego-scan-cache-{label}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    fn row(created_ms: i64, model: &str) -> UsageRow {
        UsageRow {
            created_ms,
            cost: 1.0,
            request_count: 1,
            model: model.to_string(),
            tokens: None,
        }
    }

    fn append(path: &Path, bytes: &[u8]) {
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .expect("open fixture");
        file.write_all(bytes).expect("write fixture");
    }

    fn counting_scan<'a>(
        scans: &'a Cell<usize>,
        rows: Vec<UsageRow>,
    ) -> impl FnOnce() -> Result<Vec<UsageRow>, ()> + 'a {
        move || {
            scans.set(scans.get() + 1);
            Ok(rows)
        }
    }

    #[test]
    fn unchanged_fingerprint_reuses_the_previous_scan() {
        let dir = temp_dir("hit");
        let db = dir.join("opencode.db");
        append(&db, b"db");
        append(&wal_path(&db), b"wal");
        let cache = ScanCache::new();
        let scans = Cell::new(0);

        let first = cache
            .read(&db, counting_scan(&scans, vec![row(1, "a")]))
            .unwrap();
        let second = cache
            .read(&db, counting_scan(&scans, vec![row(2, "b")]))
            .unwrap();

        assert_eq!(scans.get(), 1);
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(second[0].created_ms, 1);
    }

    #[test]
    fn wal_commit_appearance_and_removal_invalidate_the_scan() {
        let dir = temp_dir("wal");
        let db = dir.join("opencode.db");
        let wal = wal_path(&db);
        append(&db, b"db");
        let cache = ScanCache::new();
        let scans = Cell::new(0);

        cache.read(&db, counting_scan(&scans, vec![])).unwrap();
        append(&wal, b"frame-1");
        cache.read(&db, counting_scan(&scans, vec![])).unwrap();
        assert_eq!(scans.get(), 2, "a new WAL sidecar must rescan");

        append(&wal, b"frame-2");
        let rows = cache
            .read(&db, counting_scan(&scans, vec![row(9, "after-commit")]))
            .unwrap();
        assert_eq!(scans.get(), 3, "a WAL commit must rescan");
        assert_eq!(rows[0].model, "after-commit");

        std::fs::rename(&wal, dir.join("checkpointed-wal")).expect("move fixture wal");
        cache.read(&db, counting_scan(&scans, vec![])).unwrap();
        assert_eq!(scans.get(), 4, "a checkpointed (removed) WAL must rescan");

        cache.read(&db, counting_scan(&scans, vec![])).unwrap();
        assert_eq!(scans.get(), 4);
    }

    #[test]
    fn database_change_and_other_paths_invalidate_the_scan() {
        let dir = temp_dir("db");
        let db = dir.join("opencode.db");
        let other = dir.join("other.db");
        append(&db, b"db");
        append(&other, b"db");
        let cache = ScanCache::new();
        let scans = Cell::new(0);

        cache.read(&db, counting_scan(&scans, vec![])).unwrap();
        append(&db, b"grown");
        cache.read(&db, counting_scan(&scans, vec![])).unwrap();
        cache.read(&other, counting_scan(&scans, vec![])).unwrap();
        assert_eq!(scans.get(), 3);
    }

    #[test]
    fn change_during_scan_is_not_cached() {
        let dir = temp_dir("mid-scan");
        let db = dir.join("opencode.db");
        append(&db, b"db");
        let cache = ScanCache::new();
        let scans = Cell::new(0);

        cache
            .read(&db, || {
                scans.set(scans.get() + 1);
                append(&wal_path(&db), b"concurrent-commit");
                Ok::<_, ()>(vec![row(1, "stale")])
            })
            .unwrap();
        cache.read(&db, counting_scan(&scans, vec![])).unwrap();
        assert_eq!(scans.get(), 2);
    }

    #[test]
    fn scan_errors_and_missing_databases_are_not_cached() {
        let dir = temp_dir("errors");
        let db = dir.join("opencode.db");
        let cache = ScanCache::new();
        let scans = Cell::new(0);

        cache.read(&db, counting_scan(&scans, vec![])).unwrap();
        cache.read(&db, counting_scan(&scans, vec![])).unwrap();
        assert_eq!(scans.get(), 2, "a missing database has no fingerprint");

        append(&db, b"db");
        let error = cache.read(&db, || Err::<Vec<UsageRow>, _>("locked"));
        assert_eq!(error.unwrap_err(), "locked");
        cache.read(&db, counting_scan(&scans, vec![])).unwrap();
        assert_eq!(scans.get(), 3);
    }

    #[test]
    fn payloads_over_the_memory_budget_are_returned_but_not_retained() {
        let dir = temp_dir("budget");
        let db = dir.join("opencode.db");
        append(&db, b"db");
        let cache = ScanCache::new();
        let scans = Cell::new(0);
        let oversized_model = "m".repeat(MAX_CACHED_PAYLOAD_BYTES);

        let rows = cache
            .read(&db, counting_scan(&scans, vec![row(1, &oversized_model)]))
            .unwrap();
        assert_eq!(rows.len(), 1);
        cache.read(&db, counting_scan(&scans, vec![])).unwrap();
        assert_eq!(scans.get(), 2);

        let row_size = std::mem::size_of::<UsageRow>();
        let too_many_rows = MAX_CACHED_PAYLOAD_BYTES / row_size + 1;
        assert!(!fits_cache_budget(&vec![row(1, ""); too_many_rows]));
        assert!(fits_cache_budget(&vec![row(1, ""); too_many_rows - 1]));
    }
}
