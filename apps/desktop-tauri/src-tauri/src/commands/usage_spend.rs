//! Usage & Spend settings tab: 7-day / 30-day compat columns plus the
//! selected reporting period (History window).

use chrono::Utc;
use codexbar::cost_reporting_period::{CostReportingPeriod, CostTimeZone};
use codexbar::cost_scanner::CostScanner;
use codexbar::spend_contract::{
    SpendContract, build_contract_from_period_summary, build_local_spend_contract,
    build_local_spend_contract_for_period, build_local_spend_contract_from_summary,
};
use serde::Serialize;
use tauri::State;

use super::ProviderUsageSnapshot;
use crate::state::AppState;
use std::collections::{BTreeSet, HashMap};
use std::sync::{Mutex, OnceLock};

mod build;
use build::build_usage_spend_summary;
#[cfg(test)]
use build::{
    antigravity_spend_values, cached_spend, include_in_shared_overview, period_cost_from_daily,
    selected_period_scan,
};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageSpendDailyPoint {
    pub day: String,
    pub amount: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageSpendRow {
    pub provider_id: String,
    pub display_name: String,
    pub seven_day: Option<f64>,
    pub thirty_day: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seven_day_estimate: Option<codexbar::spend_contract::LocalCostEstimate>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thirty_day_estimate: Option<codexbar::spend_contract::LocalCostEstimate>,
    pub seven_day_tokens: Option<u64>,
    pub thirty_day_tokens: Option<u64>,
    /// The token figure is a floor from an incomplete scan ("at least N").
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub seven_day_tokens_lower_bound: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub thirty_day_tokens_lower_bound: bool,
    /// Cost over the selected reporting period (History window).
    pub period_cost: Option<f64>,
    /// Tokens over the selected reporting period.
    pub period_tokens: Option<u64>,
    pub currency: String,
    pub source: String,
    /// Included in the shared Overview spend denominator.
    pub included_in_overview: bool,
    #[serde(default)]
    pub daily: Vec<UsageSpendDailyPoint>,
    /// F8 (upstream 0.48.0): true when the totals are served from a stale cache
    /// while a background re-scan rebuilds the artifact. Frontend shows a
    /// "refreshing" indicator.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub refreshing: bool,
    /// ISO 8601 timestamp of the stale snapshot (when `refreshing` is true).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stale_updated_at: Option<String>,
}

#[derive(Debug, Clone)]
struct SpendValues {
    seven_day: Option<f64>,
    thirty_day: Option<f64>,
    seven_day_tokens: Option<u64>,
    thirty_day_tokens: Option<u64>,
    period_cost: Option<f64>,
    period_tokens: Option<u64>,
    source: String,
    refreshing: bool,
    stale_updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageSpendSummary {
    pub rows: Vec<UsageSpendRow>,
    pub contract: SpendContract,
    /// Raw reporting period the rows' `period*` columns were built for.
    pub reporting_period: String,
    pub reporting_day: String,
    pub dashboard_timezone: String,
}

#[derive(Debug, Clone)]
struct CachedUsageSpendSummary {
    key: String,
    summary: UsageSpendSummary,
    refresh_owner: Option<UsageSpendRefreshOwner>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UsageSpendRefreshPhase {
    Indexing,
    Paused,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct UsageSpendRefreshOwner {
    generation: u64,
    scope: String,
}

#[derive(Default)]
struct UsageSpendCoordinator {
    next_generation: u64,
    current: Option<(UsageSpendRefreshOwner, UsageSpendRefreshPhase)>,
    cache: Option<CachedUsageSpendSummary>,
}

impl UsageSpendCoordinator {
    fn begin(&mut self, scope: String) -> UsageSpendRefreshOwner {
        self.next_generation = self.next_generation.wrapping_add(1).max(1);
        let owner = UsageSpendRefreshOwner {
            generation: self.next_generation,
            scope,
        };
        self.current = Some((owner.clone(), UsageSpendRefreshPhase::Indexing));
        owner
    }

    fn pause(&mut self, owner: &UsageSpendRefreshOwner) {
        if let Some((current, phase)) = self.current.as_mut()
            && current == owner
            && *phase == UsageSpendRefreshPhase::Indexing
        {
            *phase = UsageSpendRefreshPhase::Paused;
        }
    }

    fn is_current(&self, owner: &UsageSpendRefreshOwner) -> bool {
        self.current
            .as_ref()
            .is_some_and(|(current, _)| current == owner)
    }

    fn clear_if_indexing(&mut self, owner: &UsageSpendRefreshOwner) -> bool {
        let Some((current, phase)) = self.current.as_ref() else {
            return false;
        };
        if current != owner || *phase != UsageSpendRefreshPhase::Indexing {
            return false;
        }
        self.current = None;
        true
    }

    /// The cached summary for `key`, unless the caller forces a rebuild.
    fn reusable(&self, key: &str, force_refresh: bool) -> Option<&CachedUsageSpendSummary> {
        self.cache
            .as_ref()
            .filter(|existing| !force_refresh && existing.key == key)
    }
}

static USAGE_SPEND_COORDINATOR: OnceLock<Mutex<UsageSpendCoordinator>> = OnceLock::new();

fn usage_spend_coordinator() -> &'static Mutex<UsageSpendCoordinator> {
    USAGE_SPEND_COORDINATOR.get_or_init(|| Mutex::new(UsageSpendCoordinator::default()))
}

fn clear_summary_refreshing(summary: &mut UsageSpendSummary) {
    for row in &mut summary.rows {
        row.refreshing = false;
        row.stale_updated_at = None;
    }
}

fn summary_is_refreshing(summary: &UsageSpendSummary) -> bool {
    summary.rows.iter().any(|row| row.refreshing)
}

fn mark_refresh_paused_if_codex_scan_paused(
    coordinator: &mut UsageSpendCoordinator,
    owner: &UsageSpendRefreshOwner,
    refreshing: bool,
    codex_scan_pause_reason: Option<&codexbar::core::CodexScanPauseReason>,
) {
    if refreshing && codex_scan_pause_reason.is_some() {
        coordinator.pause(owner);
    }
}

/// Retire an invalidated owner without allowing it to clear a replacement.
fn clear_usage_spend_refresh_if_owned(owner: &UsageSpendRefreshOwner) {
    let Ok(mut coordinator) = usage_spend_coordinator().lock() else {
        return;
    };
    if !coordinator.clear_if_indexing(owner) {
        return;
    }
    if let Some(existing) = coordinator.cache.as_mut()
        && existing.refresh_owner.as_ref() == Some(owner)
    {
        clear_summary_refreshing(&mut existing.summary);
        existing.refresh_owner = None;
    }
}

struct BuiltUsageSpendSummary {
    key: String,
    summary: UsageSpendSummary,
    refresh_owner: Option<UsageSpendRefreshOwner>,
}

#[tauri::command]
pub async fn get_usage_spend_summary(
    state: State<'_, Mutex<AppState>>,
    period: Option<String>,
    force_refresh: Option<bool>,
) -> Result<UsageSpendSummary, String> {
    let cached = {
        let guard = state.lock().map_err(|e| e.to_string())?;
        guard.provider_cache.clone()
    };

    // An explicit `period` wins; otherwise use the saved History window.
    let period = CostReportingPeriod::resolve_request(
        period.as_deref(),
        codexbar::settings::Settings::load().cost_reporting_period,
    );
    let force_refresh = force_refresh.unwrap_or(false);
    refresh_opencodex_pricing_before_build(&cached, period, force_refresh).await;
    let built = tauri::async_runtime::spawn_blocking(move || {
        build_usage_spend_summary_cached(&cached, period, force_refresh)
    })
    .await
    .map_err(|e| format!("usage spend worker failed: {e}"))??;
    let current_cached = state
        .lock()
        .map_err(|e| e.to_string())
        .map(|guard| guard.provider_cache.clone())?;
    let current_key = usage_spend_cache_key(
        &current_cached,
        period,
        &codexbar::settings::Settings::load(),
    );
    if current_key != built.key {
        if let Some(owner) = built.refresh_owner.as_ref() {
            clear_usage_spend_refresh_if_owned(owner);
        }
        let mut summary = built.summary;
        clear_summary_refreshing(&mut summary);
        return Ok(summary);
    }
    Ok(built.summary)
}

/// Refreshes models.dev prices for the OpenCodex ledger when this request
/// will rebuild the summary (upstream 0.60.4 fresh-load refresh). A cached
/// summary read never starts network work.
async fn refresh_opencodex_pricing_before_build(
    cached: &[ProviderUsageSnapshot],
    period: CostReportingPeriod,
    force_refresh: bool,
) {
    let settings = codexbar::settings::Settings::load();
    if !settings.open_codex_usage_logs_enabled {
        return;
    }
    let key = usage_spend_cache_key(cached, period, &settings);
    let rebuilds = usage_spend_coordinator()
        .lock()
        .is_ok_and(|coordinator| coordinator.reusable(&key, force_refresh).is_none());
    if rebuilds {
        codexbar::spend_contract::refresh_opencodex_pricing_if_needed().await;
    }
}

#[tauri::command]
pub fn write_usage_spend_export(path: String, payload: String) -> Result<(), String> {
    const MAX_EXPORT_BYTES: usize = 8 * 1024 * 1024;
    let path = path.trim();
    if path.is_empty() {
        return Err("Export path must not be empty".to_string());
    }
    if payload.len() > MAX_EXPORT_BYTES {
        return Err("Usage & Spend export exceeds 8 MiB".to_string());
    }
    std::fs::write(path, payload.as_bytes()).map_err(|error| error.to_string())
}

fn build_usage_spend_summary_cached(
    cached: &[ProviderUsageSnapshot],
    period: CostReportingPeriod,
    force_refresh: bool,
) -> Result<BuiltUsageSpendSummary, String> {
    let settings = codexbar::settings::Settings::load();
    let key = usage_spend_cache_key(cached, period, &settings);
    {
        let guard = usage_spend_coordinator()
            .lock()
            .map_err(|error| error.to_string())?;
        if let Some(existing) = guard.reusable(&key, force_refresh) {
            return Ok(BuiltUsageSpendSummary {
                key: existing.key.clone(),
                summary: existing.summary.clone(),
                refresh_owner: existing.refresh_owner.clone(),
            });
        }
    }
    let owner = {
        let mut coordinator = usage_spend_coordinator()
            .lock()
            .map_err(|error| error.to_string())?;
        coordinator.begin(key.clone())
    };
    let summary = build_usage_spend_summary(cached, period, &settings, force_refresh);
    let refreshing = summary_is_refreshing(&summary);
    let codex_scan_pause_reason =
        codexbar::core::JsonlScanner::load_cache_status(codexbar::core::ProviderId::Codex, None)
            .codex_scan_pause_reason;

    let mut coordinator = usage_spend_coordinator()
        .lock()
        .map_err(|error| error.to_string())?;
    mark_refresh_paused_if_codex_scan_paused(
        &mut coordinator,
        &owner,
        refreshing,
        codex_scan_pause_reason.as_ref(),
    );
    if !coordinator.is_current(&owner) {
        let mut summary = summary;
        clear_summary_refreshing(&mut summary);
        return Ok(BuiltUsageSpendSummary {
            key,
            summary,
            refresh_owner: Some(owner),
        });
    }
    if !refreshing {
        coordinator.clear_if_indexing(&owner);
    }
    let refresh_owner = refreshing.then(|| owner.clone());
    coordinator.cache = Some(CachedUsageSpendSummary {
        key: key.clone(),
        summary: summary.clone(),
        refresh_owner: refresh_owner.clone(),
    });
    Ok(BuiltUsageSpendSummary {
        key,
        summary,
        refresh_owner,
    })
}

fn usage_spend_cache_key(
    cached: &[ProviderUsageSnapshot],
    period: CostReportingPeriod,
    settings: &codexbar::settings::Settings,
) -> String {
    usage_spend_cache_key_with_privacy(
        cached,
        &period.identity(Utc::now(), CostTimeZone::Local),
        settings.open_codex_usage_logs_enabled,
        settings.hide_native_codex_cost_when_open_codex_present,
        settings.hide_personal_info,
    )
}

fn usage_spend_cache_key_with_privacy(
    cached: &[ProviderUsageSnapshot],
    period_identity: &str,
    include_opencodex: bool,
    hide_native: bool,
    hide_personal_info: bool,
) -> String {
    let mut revisions: Vec<String> = cached
        .iter()
        .map(|snapshot| {
            let cost = snapshot
                .cost
                .as_ref()
                .map(|cost| {
                    let daily = cost
                        .daily
                        .iter()
                        .map(|point| format!("{}:{:.8}", point.day, point.amount))
                        .collect::<Vec<_>>()
                        .join(",");
                    format!(
                        "{:.8}:{:?}:{:?}:{}:{}:{}",
                        cost.used, cost.limit, cost.balance, cost.currency_code, cost.period, daily
                    )
                })
                .unwrap_or_default();
            format!(
                "{}:{}:{}:{}",
                snapshot.provider_id, snapshot.updated_at, snapshot.source_label, cost
            )
        })
        .collect();
    revisions.sort();
    format!(
        "{}|{}|{}|{}|{}|{}",
        chrono::Local::now().date_naive(),
        period_identity,
        include_opencodex,
        hide_native,
        hide_personal_info,
        revisions.join(";")
    )
}

#[cfg(test)]
mod cache_key_tests;
#[cfg(test)]
mod tests;
