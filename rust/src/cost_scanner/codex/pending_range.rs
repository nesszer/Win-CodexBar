use super::*;

pub(super) struct CodexPendingScanContext {
    pub(super) scan_range: CostUsageDayRange,
    pub(super) is_incompatible: bool,
    pub(super) root_paths: Vec<String>,
    pub(super) timezone: String,
}

impl CodexPendingScanContext {
    pub(super) fn new(
        cache: &CostUsageCache,
        range: &CostUsageDayRange,
        sessions_dirs: &[PathBuf],
        is_app_driven: bool,
    ) -> Self {
        let root_paths = codex_scan_root_keys(sessions_dirs);
        let timezone = crate::core::local_timezone_name();
        let is_compatible = !is_app_driven
            && cache.codex_scan_incomplete
            && codex_pending_scan_context_is_compatible(
                cache,
                &range.scan_until_key,
                &root_paths,
                &timezone,
            );
        let is_incompatible = cache.codex_scan_incomplete
            && codex_pending_scan_context_is_known(cache)
            && !is_compatible;
        let scan_since_key = if is_compatible {
            std::cmp::min(
                cache
                    .codex_pending_scan_since_key
                    .as_deref()
                    .unwrap_or(range.scan_since_key.as_str()),
                range.scan_since_key.as_str(),
            )
            .to_string()
        } else {
            range.scan_since_key.clone()
        };

        Self {
            scan_range: CostUsageDayRange {
                since_key: range.since_key.clone(),
                until_key: range.until_key.clone(),
                scan_since_key,
                scan_until_key: range.scan_until_key.clone(),
            },
            is_incompatible,
            root_paths,
            timezone,
        }
    }

    pub(super) fn should_preserve_pause(
        &self,
        cache: &CostUsageCache,
        is_app_driven: bool,
    ) -> bool {
        cache.codex_scan_incomplete && cache.codex_scan_pause_reason.is_some() && !is_app_driven
    }
}

pub(super) fn codex_cache_has_validated_state(cache: &CostUsageCache) -> bool {
    cache.scan_since_key.is_some()
        || cache.scan_until_key.is_some()
        || cache.previous_report.is_some()
        || !cache.files.is_empty()
        || !cache.days.is_empty()
        || !cache.codex_pending_paths.is_empty()
        || cache.codex_pending_scan_since_key.is_some()
}

fn codex_scan_root_keys(sessions_dirs: &[PathBuf]) -> Vec<String> {
    let mut roots = sessions_dirs
        .iter()
        .map(|root| {
            let resolved = fs::canonicalize(root).unwrap_or_else(|_| root.clone());
            let key = resolved.to_string_lossy().to_string();
            if cfg!(windows) {
                key.to_ascii_lowercase()
            } else {
                key
            }
        })
        .collect::<Vec<_>>();
    roots.sort();
    roots.dedup();
    roots
}

fn codex_pending_scan_context_is_compatible(
    cache: &CostUsageCache,
    scan_until_key: &str,
    root_paths: &[String],
    timezone: &str,
) -> bool {
    cache
        .codex_pending_scan_since_key
        .as_deref()
        .is_some_and(|since| CostUsageDayRange::parse_day_key(since).is_some())
        && cache.codex_pending_scan_until_key.as_deref() == Some(scan_until_key)
        && cache.codex_pending_scan_root_paths == root_paths
        && cache.codex_pending_scan_timezone.as_deref() == Some(timezone)
}

fn codex_pending_scan_context_is_known(cache: &CostUsageCache) -> bool {
    cache.codex_pending_scan_since_key.is_some()
        || cache.codex_pending_scan_until_key.is_some()
        || !cache.codex_pending_scan_root_paths.is_empty()
        || cache.codex_pending_scan_timezone.is_some()
}
