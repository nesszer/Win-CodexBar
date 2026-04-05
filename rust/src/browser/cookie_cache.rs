//! Cookie Header Cache
//!
//! Caches cookie headers for providers to avoid repeated browser cookie extraction.
//! Stores normalized cookie headers with timestamps and source labels.

#![allow(dead_code)]

use crate::core::{CredentialStore, ProviderId, WindowsCredentialStore};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Cached cookie header entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CookieHeaderEntry {
    /// The normalized cookie header string
    #[serde(skip_serializing, default)]
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

const COOKIE_CACHE_SERVICE: &str = "CodexBar";

impl CookieHeaderCache {
    fn credential_key(provider: ProviderId) -> String {
        format!("{}-cookie-cache", provider.cli_name())
    }

    fn credential_store() -> WindowsCredentialStore {
        WindowsCredentialStore::new()
    }

    /// Load cached cookie header for a provider
    pub fn load(provider: ProviderId) -> Option<CookieHeaderEntry> {
        let path = Self::cache_path(provider)?;
        let data = fs::read_to_string(&path).ok()?;
        let mut entry: CookieHeaderEntry = serde_json::from_str(&data).ok()?;
        entry.cookie_header = Self::credential_store()
            .get(COOKIE_CACHE_SERVICE, &Self::credential_key(provider))
            .ok()?;
        Some(entry)
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

        let entry = CookieHeaderEntry::new(normalized.clone(), source_label);
        let path = Self::cache_path(provider).ok_or(CookieHeaderCacheError::PathNotAvailable)?;

        Self::credential_store()
            .set(
                COOKIE_CACHE_SERVICE,
                &Self::credential_key(provider),
                &normalized,
            )
            .map_err(|e| CookieHeaderCacheError::Credential(e.to_string()))?;

        // Create parent directory
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let json = serde_json::to_string_pretty(&entry)?;
        fs::write(&path, json)?;

        tracing::debug!(
            provider = %provider.cli_name(),
            source = source_label,
            "Stored cookie header to cache"
        );

        Ok(())
    }

    /// Clear cached cookie header for a provider
    pub fn clear(provider: ProviderId) {
        let _ =
            Self::credential_store().delete(COOKIE_CACHE_SERVICE, &Self::credential_key(provider));
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

    /// Check if a cached entry exists and is fresh
    pub fn has_fresh_cache(provider: ProviderId, max_age_secs: i64) -> bool {
        match Self::load(provider) {
            Some(entry) => !entry.is_stale(max_age_secs),
            None => false,
        }
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
    #[error("Credential error: {0}")]
    Credential(String),
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
    fn test_cookie_cache_entry_serialization_does_not_include_cookie_value() {
        let entry = CookieHeaderEntry::new("session=secret", "Chrome");
        let json = serde_json::to_string(&entry).unwrap();
        assert!(!json.contains("session=secret"));
    }
}
