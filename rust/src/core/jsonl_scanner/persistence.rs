//! Loading and saving the per-provider cost cache artifact.

use super::*;

#[derive(Deserialize, Default)]
struct CachedCostReadStatusProjection {
    #[serde(default)]
    codex_cache_schema_version: u32,
    #[serde(
        default,
        rename = "days",
        deserialize_with = "deserialize_nonempty_object"
    )]
    has_days: bool,
    #[serde(default)]
    previous_report: Option<CachedCostReport>,
    #[serde(default)]
    codex_scan_pause_reason: Option<CodexScanPauseReason>,
    #[serde(default)]
    bucket_time_zone: Option<String>,
}

fn deserialize_nonempty_object<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{IgnoredAny, MapAccess, Visitor};

    struct NonemptyObjectVisitor;

    impl<'de> Visitor<'de> for NonemptyObjectVisitor {
        type Value = bool;

        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("a JSON object")
        }

        fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
        where
            A: MapAccess<'de>,
        {
            let mut nonempty = false;
            while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {
                nonempty = true;
            }
            Ok(nonempty)
        }
    }

    deserializer.deserialize_map(NonemptyObjectVisitor)
}

impl JsonlScanner {
    /// Load cache from disk.
    ///
    /// Refuses to decode artifacts larger than the load cap
    /// (`crate::core::CostUsageCacheBudget::MAX_LOAD_BYTES`); an oversized artifact is
    /// cheaper to rebuild bounded than to decode in one shot, so the caller
    /// gets a fresh empty cache instead (upstream 0.48.0 overshoot contract).
    /// Only Codex persistence is bounded; other providers load unbounded.
    pub fn load_cache(provider: ProviderId, cache_root: Option<&Path>) -> CostUsageCache {
        let cache_path = Self::cache_path(provider, cache_root);

        if crate::core::is_bounded_provider(provider) {
            // Artifacts are bounded by MAX_LOAD_BYTES (320 MiB), fitting usize on
            // any supported target even before the budget comparison below.
            #[allow(
                clippy::cast_possible_truncation,
                reason = "bounded artifacts fit usize on any supported target"
            )]
            let file_bytes = crate::core::artifact_file_size(&cache_path) as usize;
            if file_bytes > crate::core::CostUsageCacheBudget::MAX_LOAD_BYTES {
                return CostUsageCache::default();
            }
        }

        if let Ok(contents) = fs::read_to_string(&cache_path)
            && let Ok(mut cache) = serde_json::from_str::<CostUsageCache>(&contents)
        {
            let stamp = CacheStamp::from_bytes(contents.as_bytes());
            cache.loaded_payload_stamp = CacheStamp::from_cache_payload(contents.as_bytes());
            if let Some(scan_unix_ms) = save_skip::recorded_scan_time(&cache_path, &stamp) {
                cache.last_scan_unix_ms = scan_unix_ms;
            }
            if provider == ProviderId::Codex {
                return codex::codex_cache_apply_load_policy(cache, stamp);
            }
            cache.loaded_stamp = Some(Some(stamp));
            return cache;
        }

        // Track a missing or unreadable baseline separately from a manually
        // constructed cache so a concurrent first writer can invalidate it.
        CostUsageCache {
            loaded_stamp: Some(Self::cache_stamp(&cache_path)),
            ..CostUsageCache::default()
        }
    }

    /// Read only the cache metadata needed by presentation surfaces.
    ///
    /// v0.56.0 performance parity: skip raw per-file scanner state and day
    /// payloads when callers only need stale/catch-up status.
    pub fn load_cache_status(
        provider: ProviderId,
        cache_root: Option<&Path>,
    ) -> CachedCostReadStatus {
        let cache_path = Self::cache_path(provider, cache_root);
        if crate::core::is_bounded_provider(provider) {
            #[allow(
                clippy::cast_possible_truncation,
                reason = "bounded artifacts fit usize on any supported target"
            )]
            let file_bytes = crate::core::artifact_file_size(&cache_path) as usize;
            if file_bytes > crate::core::CostUsageCacheBudget::MAX_LOAD_BYTES {
                return CachedCostReadStatus::default();
            }
        }

        let Ok(file) = File::open(cache_path) else {
            return CachedCostReadStatus::default();
        };
        let Ok(projection) =
            serde_json::from_reader::<_, CachedCostReadStatusProjection>(BufReader::new(file))
        else {
            return CachedCostReadStatus::default();
        };
        if provider == ProviderId::Codex
            && (!codex::codex_cache_schema_is_current(projection.codex_cache_schema_version)
                || !codex::codex_cache_zone_is_current(projection.bucket_time_zone.as_deref()))
        {
            return CachedCostReadStatus::default();
        }
        CachedCostReadStatus {
            has_days: projection.has_days,
            previous_report: projection.previous_report,
            codex_scan_pause_reason: projection.codex_scan_pause_reason,
        }
    }

    /// Save cache to disk (temp sibling + copy into place).
    ///
    /// Before encoding, prunes the cache to the persistence budget so the
    /// artifact stays small enough to decode in one shot (upstream 0.48.0
    /// #2637). Only Codex persistence is bounded; the overshoot contract lets
    /// the encoded size exceed `MAX_FILE_BYTES` up to `MAX_LOAD_BYTES`
    /// when protected (partially parsed) entries cannot be trimmed further.
    pub fn save_cache(provider: ProviderId, cache: &mut CostUsageCache, cache_root: Option<&Path>) {
        Self::save_cache_with_limit(
            provider,
            cache,
            cache_root,
            crate::core::CostUsageCacheBudget::MAX_LOAD_BYTES,
        );
    }

    /// Save with an explicit post-encode refusal limit, injected by tests.
    ///
    /// Identical to `save_cache` except the post-encode oversize check uses
    /// `max_load_bytes` rather than the production `MAX_LOAD_BYTES` const.
    /// Production callers MUST use `save_cache`; this helper exists so the
    /// refusal / stale-destination removal can be exercised without encoding a
    /// ~320 MiB test artifact.
    pub(super) fn save_cache_with_limit(
        provider: ProviderId,
        cache: &mut CostUsageCache,
        cache_root: Option<&Path>,
        max_load_bytes: usize,
    ) {
        let cache_path = Self::cache_path(provider, cache_root);

        // A decoded baseline is only valid for the file contents that produced
        // it. Refuse a stale writer before pruning or creating directories so a
        // concurrent scan remains authoritative.
        if let Some(expected) = cache.loaded_stamp.as_ref()
            && Self::cache_stamp(&cache_path).as_ref() != expected.as_ref()
        {
            return;
        }
        if provider == ProviderId::Codex {
            codex::codex_cache_stamp_schema_version(cache);
        }

        let Some(parent) = cache_path.parent() else {
            return;
        };
        // Best-effort cache dir creation; a missing dir surfaces as the write error below.
        let _dir_created = fs::create_dir_all(parent);

        if crate::core::is_bounded_provider(provider) {
            // v0.55.1 #3051: snapshot the fully validated report BEFORE persistence
            // pruning. If budget trimming creates a catch-up cycle, this is the
            // established spend/tokens users should keep seeing until replacement
            // history finishes, not a zero-cost reconstruction of the trimmed cache.
            let established_report = cache
                .previous_report
                .clone()
                .unwrap_or_else(|| Self::cached_cost_report_from_days(cache));
            let pruned = crate::core::prune_out_of_window_for_budget(
                &mut cache.files,
                &mut cache.days,
                cache.scan_since_key.as_deref(),
                cache.scan_until_key.as_deref(),
                false,
            );
            let estimate = crate::core::estimated_cache_bytes(&cache.files, &cache.days);
            let trimmed = if estimate > crate::core::CostUsageCacheBudget::MAX_FILE_BYTES {
                crate::core::trim_in_window_for_budget(
                    &mut cache.files,
                    &mut cache.days,
                    cache.scan_since_key.as_deref(),
                    cache.scan_until_key.as_deref(),
                    crate::core::CostUsageCacheBudget::MAX_FILE_BYTES,
                )
            } else {
                Vec::new()
            };
            // A16 (upstream 0.48.0): when entries were trimmed for budget, the persisted
            // artifact no longer covers the full window — set previous_report so the
            // next refresh can signal catch-up is pending (and spend surfaces can show
            // the last-validated snapshot during the rescan).
            if (!pruned.is_empty() || !trimmed.is_empty()) && cache.previous_report.is_none() {
                cache.previous_report = Some(established_report);
            }
        }

        let Ok(json) = serde_json::to_string(cache) else {
            return;
        };

        // F19 (upstream 0.48.0): after bounded encode, if the artifact still
        // exceeds MAX_LOAD_BYTES, refuse persistence. Also remove any existing
        // destination artifact so a stale/oversized file cannot persist and
        // trip the load-refusal path on the next scan (which would force an
        // unnecessary full rebuild from a poisoned artifact). This is a
        // one-shot refusal (not a persist/refuse/rebuild loop): the budget
        // enforcement above already pruned and trimmed; if the result is still
        // too large (e.g. a single protected entry exceeds the limit), the
        // artifact is dropped and the next scan rebuilds from scratch.
        if crate::core::is_bounded_provider(provider)
            && crate::core::CostUsageCacheBudget::should_refuse_persistence(
                json.len(),
                max_load_bytes,
            )
        {
            // Best-effort removal; ignore errors (file may not exist).
            let _cleared = fs::remove_file(&cache_path);
            return;
        }
        if save_skip::skip_unchanged_save(&cache_path, cache, &json) {
            return;
        }

        let tmp_name = format!(
            ".{}.{}-{}.tmp",
            provider.cli_name(),
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );
        let tmp_path = parent.join(tmp_name);
        if fs::write(&tmp_path, json.as_bytes()).is_err() {
            return;
        }
        // Recheck after encoding/pruning: another scan may have replaced the
        // destination while this writer was preparing its payload.
        if let Some(expected) = cache.loaded_stamp.as_ref()
            && Self::cache_stamp(&cache_path).as_ref() != expected.as_ref()
        {
            let _removed_tmp = fs::remove_file(&tmp_path);
            return;
        }
        // `copy` replaces an existing target on Windows; prefer it over rename.
        let wrote = if fs::copy(&tmp_path, &cache_path).is_ok() {
            true
        } else {
            // Fallback direct write when copy fails; the copy error already surfaced.
            fs::write(&cache_path, json.as_bytes()).is_ok()
        };
        if wrote {
            cache.loaded_stamp = Some(Some(CacheStamp::from_bytes(json.as_bytes())));
            cache.loaded_payload_stamp = CacheStamp::from_cache_payload(json.as_bytes());
        }
        // Best-effort temp cleanup (ignore errors — unique name avoids clashes).
        let _truncated_tmp = fs::File::create(&tmp_path).and_then(|f| f.set_len(0));
    }

    /// Default on-disk cache root: `%LOCALAPPDATA%\CodexBar` (via `dirs::cache_dir`).
    pub fn default_cache_root() -> Option<PathBuf> {
        dirs::cache_dir().map(|d| d.join("CodexBar"))
    }

    pub(super) fn cache_path(provider: ProviderId, cache_root: Option<&Path>) -> PathBuf {
        let root = cache_root
            .map(|p| p.to_path_buf())
            .or_else(Self::default_cache_root)
            .unwrap_or_else(|| PathBuf::from("."));

        // Mirror upstream layout: {cacheRoot}/cost-usage/{provider}-v1.json
        root.join("cost-usage")
            .join(format!("{}-v1.json", provider.cli_name()))
    }

    pub(super) fn cache_stamp(cache_path: &Path) -> Option<CacheStamp> {
        fs::read(cache_path)
            .ok()
            .map(|contents| CacheStamp::from_bytes(&contents))
    }
}
