//! Setup shared by the cost scanner tests.

use super::*;

/// A temp root holding the `sessions` and `cache` dirs most Codex scans use.
pub(super) fn codex_scan_dirs() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let root = tempfile::tempdir().unwrap();
    let sessions = root.path().join("sessions");
    let cache_root = root.path().join("cache");
    (root, sessions, cache_root)
}

/// The app-driven scanner over one sessions dir.
pub(super) fn app_scanner(
    days: u32,
    cache_root: impl Into<PathBuf>,
    sessions: &Path,
) -> CostScanner {
    CostScanner::new(days)
        .with_options(CostScanOptions::app_driven())
        .with_cache_root(cache_root)
        .with_sessions_dirs(vec![sessions.to_path_buf()])
}

/// The `YYYY/MM/DD` partition for `day` under a sessions dir.
pub(super) fn partition_dir(sessions: &Path, day: NaiveDate) -> PathBuf {
    sessions
        .join(day.format("%Y").to_string())
        .join(day.format("%m").to_string())
        .join(day.format("%d").to_string())
}

pub(super) fn cached_file<'a>(cache: &'a CostUsageCache, path: &Path) -> &'a CostUsageFileUsage {
    cache
        .files
        .get(&path.to_string_lossy().to_string())
        .unwrap()
}
