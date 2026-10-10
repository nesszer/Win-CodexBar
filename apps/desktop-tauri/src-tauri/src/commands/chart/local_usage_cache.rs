use super::*;

pub(crate) fn load_provider_local_usage_summary(
    provider_id: &str,
) -> Option<ProviderLocalUsageSummary> {
    load_local_usage_summary_cached(provider_id, None)
}

pub(super) struct CachedLocalUsage {
    loaded_at: Instant,
    /// `CostReportingPeriod::identity` the entry was built for; a period or
    /// month change makes the entry stale before its TTL.
    period_identity: String,
    summary: Option<ProviderLocalUsageSummary>,
}

fn local_usage_period_identity() -> String {
    current_reporting_period().identity(Utc::now(), CostTimeZone::Local)
}

/// Cache a completed scan under the reporting window it actually used. The
/// summary timestamp preserves the month for month-to-date identities.
pub(super) fn local_usage_summary_period_identity(
    summary: &ProviderLocalUsageSummary,
) -> Option<String> {
    let period = CostReportingPeriod::parse(&summary.reporting_period)?;
    let scanned_at = DateTime::<Utc>::from_timestamp_millis(summary.token_cost_updated_at_ms)?;
    Some(period.identity(scanned_at, CostTimeZone::Local))
}

pub(super) fn local_usage_cache() -> &'static Mutex<HashMap<String, CachedLocalUsage>> {
    static CACHE: OnceLock<Mutex<HashMap<String, CachedLocalUsage>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) fn clear_provider_local_usage_cache() {
    if let Ok(mut guard) = local_usage_cache().lock() {
        guard.clear();
    }
}

pub(crate) fn cached_provider_local_usage_summary(
    provider_id: &str,
) -> Option<ProviderLocalUsageSummary> {
    let Ok(guard) = local_usage_cache().lock() else {
        return None;
    };
    guard
        .get(provider_id)
        .and_then(|entry| entry.summary.clone())
}

pub(crate) async fn refresh_provider_local_usage_cache(provider_ids: Vec<String>) {
    if provider_ids.is_empty() {
        return;
    }

    let failure_provider_ids = provider_ids.clone();
    let scans = match tauri::async_runtime::spawn_blocking(move || {
        provider_ids
            .into_iter()
            .map(|provider_id| {
                let (summary, unknown_models) =
                    load_local_usage_summary_with_unknown_models(&provider_id, None);
                (provider_id, summary, unknown_models)
            })
            .collect::<Vec<_>>()
    })
    .await
    {
        Ok(scans) => scans,
        Err(err) => {
            tracing::warn!("Provider local usage refresh worker failed: {err}");
            for provider_id in failure_provider_ids {
                record_local_usage_fetch_failure(&provider_id, CostFetchFailure::Failed);
            }
            return;
        }
    };

    for (provider_id, mut summary, unknown_models) in scans {
        let pricing_provider = match provider_id.as_str() {
            "codex" => Some("openai"),
            "claude" => Some("anthropic"),
            _ => None,
        };
        if let Some(pricing_provider) = pricing_provider
            && codexbar::core::refresh_unknown_models_if_needed(pricing_provider, &unknown_models)
                .await
        {
            let rescan_provider = provider_id.clone();
            summary = tauri::async_runtime::spawn_blocking(move || {
                load_local_usage_summary(&rescan_provider, None)
            })
            .await
            .unwrap_or(summary);
        }
        store_local_usage_summary(&provider_id, summary);
    }
}

#[cfg(test)]
pub(crate) fn cache_provider_local_usage_summary_for_test(
    provider_id: &str,
    summary: Option<ProviderLocalUsageSummary>,
) {
    store_local_usage_summary(provider_id, summary);
}

pub(super) fn load_local_usage_summary_cached(
    provider_id: &str,
    cancel: Option<&AtomicBool>,
) -> Option<ProviderLocalUsageSummary> {
    let cache = local_usage_cache();
    let period_identity = local_usage_period_identity();
    if let Ok(guard) = cache.lock()
        && let Some(entry) = guard.get(provider_id)
        && entry.period_identity == period_identity
        && token_cost_cache_is_fresh(Some(entry.loaded_at), Instant::now(), LOCAL_USAGE_TTL)
    {
        return entry.summary.clone();
    }

    if cancel.is_some_and(|flag| flag.load(Ordering::Relaxed)) {
        return None;
    }

    let summary = load_local_usage_summary(provider_id, cancel);
    if cancel.is_some_and(|flag| flag.load(Ordering::Relaxed)) {
        return None;
    }

    store_local_usage_summary(provider_id, summary.clone());
    summary
}

pub(super) fn store_local_usage_summary(
    provider_id: &str,
    summary: Option<ProviderLocalUsageSummary>,
) {
    let period_identity = match summary.as_ref() {
        Some(summary) => {
            let Some(period_identity) = local_usage_summary_period_identity(summary) else {
                tracing::warn!("Skipping local usage cache entry with invalid period metadata");
                return;
            };
            period_identity
        }
        None => local_usage_period_identity(),
    };
    if let Ok(mut guard) = local_usage_cache().lock() {
        guard.insert(
            provider_id.to_string(),
            CachedLocalUsage {
                loaded_at: Instant::now(),
                period_identity,
                summary,
            },
        );
    }
}

pub(super) fn record_local_usage_fetch_failure(provider_id: &str, failure: CostFetchFailure) {
    let loaded_at = if cost_fetch_failure_allows_early_retry(failure) {
        Instant::now() - LOCAL_USAGE_TTL - Duration::from_secs(1)
    } else {
        Instant::now()
    };
    let period_identity = local_usage_period_identity();
    if let Ok(mut guard) = local_usage_cache().lock() {
        guard.insert(
            provider_id.to_string(),
            CachedLocalUsage {
                loaded_at,
                period_identity,
                summary: None,
            },
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(
    dead_code,
    reason = "chart command helper reserved for future dashboard integration"
)]
pub(crate) enum CostFetchFailure {
    Failed,
    TimedOut,
}

pub(crate) fn token_cost_cache_is_fresh(
    loaded_at: Option<Instant>,
    now: Instant,
    ttl: Duration,
) -> bool {
    loaded_at
        .and_then(|loaded| now.checked_duration_since(loaded))
        .map(|age| age <= ttl)
        .unwrap_or(false)
}

pub(crate) fn cost_fetch_failure_allows_early_retry(failure: CostFetchFailure) -> bool {
    !matches!(failure, CostFetchFailure::TimedOut)
}

pub(super) fn current_unix_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}
