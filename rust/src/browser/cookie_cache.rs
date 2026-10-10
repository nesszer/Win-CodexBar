//! Cookie Header Cache
//!
//! Caches cookie headers for providers to avoid repeated browser cookie extraction.
//! Stores normalized cookie headers with timestamps and source labels. Entries
//! are session secrets, so they are written through `secure_file` (DPAPI on
//! Windows, staged and published atomically) rather than as plaintext.

use crate::core::ProviderId;
use crate::secure_file;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// Cached cookie header entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CookieHeaderEntry {
    /// The normalized cookie header string
    pub cookie_header: String,
    /// When this entry was stored
    pub stored_at: DateTime<Utc>,
    /// Source of the cookie (e.g., "Chrome", "Edge", "Manual")
    pub source_label: String,
}

impl CookieHeaderEntry {
    pub fn new(cookie_header: impl Into<String>, source_label: impl Into<String>) -> Self {
        Self {
            cookie_header: cookie_header.into(),
            stored_at: Utc::now(),
            source_label: source_label.into(),
        }
    }

    /// Check if the entry is stale (older than max_age_secs)
    pub fn is_stale(&self, max_age_secs: i64) -> bool {
        let age = Utc::now().signed_duration_since(self.stored_at);
        age.num_seconds() > max_age_secs
    }
}

/// Cookie header cache store
pub struct CookieHeaderCache;

impl CookieHeaderCache {
    /// Load cached cookie header for a provider
    pub fn load(provider: ProviderId) -> Option<CookieHeaderEntry> {
        Self::load_from(&Self::cache_path(provider)?)
    }

    fn load_from(path: &Path) -> Option<CookieHeaderEntry> {
        let data = secure_file::read_string(path).ok()?;
        serde_json::from_str(&data).ok()
    }

    /// Store a cookie header for a provider
    pub fn store(
        provider: ProviderId,
        cookie_header: &str,
        source_label: &str,
    ) -> Result<(), CookieHeaderCacheError> {
        let trimmed = cookie_header.trim();

        // Normalize the cookie header
        let normalized = Self::normalize_cookie_header(trimmed);

        if normalized.is_empty() {
            // Clear the cache if the normalized header is empty
            Self::clear(provider);
            return Ok(());
        }

        let entry = CookieHeaderEntry::new(normalized, source_label);
        let path = Self::cache_path(provider).ok_or(CookieHeaderCacheError::PathNotAvailable)?;
        Self::store_to(&path, &entry)?;

        tracing::debug!(
            provider = %provider.cli_name(),
            source = source_label,
            "Stored cookie header to cache"
        );

        Ok(())
    }

    /// Persist `entry` to `path`, creating the parent directory. The write is
    /// staged and published atomically, so a failure keeps the previous entry.
    fn store_to(path: &Path, entry: &CookieHeaderEntry) -> Result<(), CookieHeaderCacheError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(entry)?;
        secure_file::write_string(path, &json)?;
        Ok(())
    }

    /// Clear cached cookie header for a provider
    pub fn clear(provider: ProviderId) {
        if let Some(path) = Self::cache_path(provider)
            && let Err(e) = fs::remove_file(&path)
            && e.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!(
                provider = %provider.cli_name(),
                error = %e,
                "Failed to remove cookie cache"
            );
        }
    }

    /// Get the cache file path for a provider
    fn cache_path(provider: ProviderId) -> Option<PathBuf> {
        dirs::data_local_dir().map(|d| {
            d.join("CodexBar")
                .join(format!("{}-cookie.json", provider.cli_name()))
        })
    }

    /// Normalize a cookie header string
    fn normalize_cookie_header(header: &str) -> String {
        // Remove duplicate cookies, normalize whitespace, and sort for consistency
        let mut cookies: Vec<(&str, &str)> = Vec::new();

        for part in header.split(';') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }

            if let Some((name, value)) = part.split_once('=') {
                let name = name.trim();
                let value = value.trim();

                // Only keep the last occurrence of each cookie name
                cookies.retain(|(n, _)| *n != name);
                cookies.push((name, value));
            }
        }

        // Sort by cookie name for consistency
        cookies.sort_by(|a, b| a.0.cmp(b.0));

        // Rebuild the header
        cookies
            .into_iter()
            .map(|(name, value)| format!("{}={}", name, value))
            .collect::<Vec<_>>()
            .join("; ")
    }
}

/// Cookie header cache errors
#[derive(Debug, thiserror::Error)]
pub enum CookieHeaderCacheError {
    #[error("Cache path not available")]
    PathNotAvailable,
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_cookie_header() {
        let header = "foo=bar; baz=qux; foo=updated";
        let normalized = CookieHeaderCache::normalize_cookie_header(header);
        // foo should be updated (last occurrence), sorted alphabetically
        assert!(normalized.contains("baz=qux"));
        assert!(normalized.contains("foo=updated"));
        assert!(!normalized.contains("foo=bar"));
    }

    #[test]
    fn test_entry_staleness() {
        let entry = CookieHeaderEntry::new("foo=bar", "test");
        assert!(!entry.is_stale(60)); // Not stale within 60 seconds

        // We can't easily test staleness without mocking time
    }

    #[test]
    fn test_empty_normalization() {
        let empty = CookieHeaderCache::normalize_cookie_header("");
        assert!(empty.is_empty());

        let whitespace = CookieHeaderCache::normalize_cookie_header("   ;  ;  ");
        assert!(whitespace.is_empty());
    }

    #[test]
    fn store_round_trips_and_replaces_the_previous_entry() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("codex-cookie.json");

        CookieHeaderCache::store_to(&path, &CookieHeaderEntry::new("a=1", "Chrome")).unwrap();
        CookieHeaderCache::store_to(&path, &CookieHeaderEntry::new("a=2", "Edge")).unwrap();

        let loaded = CookieHeaderCache::load_from(&path).expect("entry");
        assert_eq!(loaded.cookie_header, "a=2");
        assert_eq!(loaded.source_label, "Edge");
        let names: Vec<_> = fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, vec![std::ffi::OsString::from("codex-cookie.json")]);
    }

    #[test]
    fn failed_store_leaves_no_cookie_bytes_behind() {
        let dir = tempfile::tempdir().unwrap();
        // A directory at the destination makes publishing fail after the
        // staged sibling was fully written.
        let blocked = dir.path().join("codex-cookie.json");
        fs::create_dir(&blocked).unwrap();
        let entry = CookieHeaderEntry::new("secret=leaked-cookie", "Chrome");

        assert!(CookieHeaderCache::store_to(&blocked, &entry).is_err());

        assert!(blocked.is_dir());
        for sibling in fs::read_dir(dir.path()).unwrap() {
            let sibling = sibling.unwrap().path();
            if sibling.is_file() {
                assert_eq!(
                    fs::metadata(&sibling).unwrap().len(),
                    0,
                    "failed store must truncate the staged cookie secret"
                );
            }
        }
    }

    #[test]
    fn load_still_reads_entries_written_as_plaintext_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("codex-cookie.json");
        let entry = CookieHeaderEntry::new("legacy=1", "Chrome");
        fs::write(&path, serde_json::to_string_pretty(&entry).unwrap()).unwrap();

        let loaded = CookieHeaderCache::load_from(&path).expect("legacy entry");
        assert_eq!(loaded.cookie_header, "legacy=1");
    }
}
