//! Usage & Spend settings tab: 7-day / 30-day local cost aggregates.

use codexbar::cost_scanner::{CostScanner, CostSummary};
use codexbar::spend_contract::{
    SpendContract, build_local_spend_contract, build_local_spend_contract_from_summary,
};
use serde::Serialize;
use tauri::State;

use super::ProviderUsageSnapshot;
use crate::state::AppState;
use std::collections::{BTreeSet, HashMap};
use std::sync::{Mutex, OnceLock};

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
    pub seven_day_tokens: Option<u64>,
    pub thirty_day_tokens: Option<u64>,
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
    source: String,
    refreshing: bool,
    stale_updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageSpendSummary {
    pub rows: Vec<UsageSpendRow>,
    pub contract: SpendContract,
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
    history_days: Option<u32>,
    force_refresh: Option<bool>,
) -> Result<UsageSpendSummary, String> {
    let cached = {
        let guard = state.lock().map_err(|e| e.to_string())?;
        guard.provider_cache.clone()
    };

    let selected_days = history_days.unwrap_or(30);
    let force_refresh = force_refresh.unwrap_or(false);
    let built = tauri::async_runtime::spawn_blocking(move || {
        build_usage_spend_summary_cached(&cached, selected_days, force_refresh)
    })
    .await
    .map_err(|e| format!("usage spend worker failed: {e}"))??;
    let current_cached = state
        .lock()
        .map_err(|e| e.to_string())
        .map(|guard| guard.provider_cache.clone())?;
    let current_key = usage_spend_cache_key(
        &current_cached,
        selected_days,
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
    selected_days: u32,
    force_refresh: bool,
) -> Result<BuiltUsageSpendSummary, String> {
    let settings = codexbar::settings::Settings::load();
    let key = usage_spend_cache_key(cached, selected_days, &settings);
    {
        let guard = usage_spend_coordinator()
            .lock()
            .map_err(|error| error.to_string())?;
        if !force_refresh
            && let Some(existing) = guard.cache.as_ref()
            && existing.key == key
        {
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
    let summary = build_usage_spend_summary(cached, selected_days, &settings, force_refresh);
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
    selected_days: u32,
    settings: &codexbar::settings::Settings,
) -> String {
    usage_spend_cache_key_with_privacy(
        cached,
        selected_days,
        settings.open_codex_usage_logs_enabled,
        settings.hide_native_codex_cost_when_open_codex_present,
        settings.hide_personal_info,
    )
}

fn usage_spend_cache_key_with_privacy(
    cached: &[ProviderUsageSnapshot],
    selected_days: u32,
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
        selected_days,
        include_opencodex,
        hide_native,
        hide_personal_info,
        revisions.join(";")
    )
}

fn build_usage_spend_summary(
    cached: &[ProviderUsageSnapshot],
    selected_days: u32,
    settings: &codexbar::settings::Settings,
    force_refresh: bool,
) -> UsageSpendSummary {
    let include_opencodex = settings.open_codex_usage_logs_enabled;
    let hide_native = settings.hide_native_codex_cost_when_open_codex_present;

    // Upstream 0.55.0 #3105: independent provider baselines load in parallel.
    // Keep each provider's 7d/30d scans serial so they can safely share that
    // provider's incremental cache, while Codex and Claude run concurrently.
    let codex_scan_options = if force_refresh {
        codexbar::core::CostScanOptions::app_driven()
    } else {
        codexbar::core::CostScanOptions::default()
    };
    let ((codex_7_summary, codex_30_summary), (claude_7_summary, claude_30_summary)) =
        std::thread::scope(|scope| {
            let codex = scope.spawn(move || {
                (
                    CostScanner::new(7)
                        .with_options(codex_scan_options)
                        .scan_codex(),
                    CostScanner::new(30)
                        .with_options(codex_scan_options)
                        .scan_codex(),
                )
            });
            let claude = scope.spawn(|| {
                (
                    CostScanner::new(7).scan_claude(),
                    CostScanner::new(30).scan_claude(),
                )
            });
            (
                codex.join().expect("Codex spend scan worker panicked"),
                claude.join().expect("Claude spend scan worker panicked"),
            )
        });

    let codex_stale = !codex_30_summary.history_coverage_established;
    let codex_stale_updated_at = codex_stale
        .then(|| {
            codexbar::core::JsonlScanner::load_cache_status(codexbar::core::ProviderId::Codex, None)
                .previous_report
                .and_then(|report| report.updated_at)
        })
        .flatten();

    let codex_7_contract = build_local_spend_contract_from_summary(
        "codex",
        7,
        include_opencodex,
        hide_native,
        settings.hide_personal_info,
        codex_7_summary.clone(),
    );
    let codex_30_contract = build_local_spend_contract_from_summary(
        "codex",
        30,
        include_opencodex,
        hide_native,
        settings.hide_personal_info,
        codex_30_summary.clone(),
    );

    let mut provider_ids: BTreeSet<String> = settings.enabled_providers.iter().cloned().collect();
    provider_ids.extend(cached.iter().map(|snapshot| snapshot.provider_id.clone()));
    if include_opencodex {
        // OpenCodex is an enrichment source, never a standalone provider row.
        // Publish routed subscriptions even when no live provider snapshot exists.
        for id in ["codex", "opencodego", "kimi", "deepseek"] {
            let contract = match id {
                "codex" => None,
                _ => Some(build_local_spend_contract(id, 30, true)),
            };
            if contract
                .as_ref()
                .is_some_and(|contract| !contract.imports.is_empty())
            {
                provider_ids.insert(id.to_string());
            }
        }
    }

    let cached_by_id: HashMap<&str, &ProviderUsageSnapshot> = cached
        .iter()
        .map(|snapshot| (snapshot.provider_id.as_str(), snapshot))
        .collect();

    let mut rows = Vec::new();
    for provider_id in provider_ids {
        let cached_snapshot = cached_by_id.get(provider_id.as_str()).copied();
        let display_name = cached_snapshot
            .map(|snapshot| snapshot.display_name.trim())
            .filter(|name| !name.is_empty())
            .map(str::to_string)
            .or_else(|| {
                codexbar::core::ProviderId::from_cli_name(&provider_id).map(|id| {
                    codexbar::core::instantiate_provider(id)
                        .metadata()
                        .display_name
                        .to_string()
                })
            })
            .unwrap_or_else(|| provider_id.clone());

        let spend = match provider_id.as_str() {
            "codex" => SpendValues {
                seven_day: codex_7_contract.known_cost_usd,
                thirty_day: codex_30_contract.known_cost_usd,
                seven_day_tokens: total_token_mix(&codex_7_contract.token_mix),
                thirty_day_tokens: total_token_mix(&codex_30_contract.token_mix),
                source: if include_opencodex && !codex_30_contract.imports.is_empty() {
                    "local logs + OpenCodex".to_string()
                } else {
                    "local logs".to_string()
                },
                refreshing: codex_stale,
                stale_updated_at: codex_stale_updated_at.clone(),
            },
            "claude" => SpendValues {
                seven_day: Some(claude_7_summary.total_cost_usd),
                thirty_day: Some(claude_30_summary.total_cost_usd),
                seven_day_tokens: Some(
                    claude_7_summary
                        .input_tokens
                        .saturating_add(claude_7_summary.output_tokens),
                ),
                thirty_day_tokens: Some(
                    claude_30_summary
                        .input_tokens
                        .saturating_add(claude_30_summary.output_tokens),
                ),
                source: "local logs".to_string(),
                refreshing: false,
                stale_updated_at: None,
            },
            "opencodego" | "kimi" | "deepseek" if include_opencodex => {
                let seven = build_local_spend_contract(&provider_id, 7, true);
                let thirty = build_local_spend_contract(&provider_id, 30, true);
                if !thirty.imports.is_empty() {
                    SpendValues {
                        seven_day: seven.known_cost_usd,
                        thirty_day: thirty.known_cost_usd,
                        seven_day_tokens: total_token_mix(&seven.token_mix),
                        thirty_day_tokens: total_token_mix(&thirty.token_mix),
                        source: if provider_id == "opencodego" {
                            "local logs + OpenCodex".to_string()
                        } else {
                            "OpenCodex".to_string()
                        },
                        refreshing: false,
                        stale_updated_at: None,
                    }
                } else {
                    cached_spend(cached_snapshot)
                }
            }
            "cursor" => {
                let seven = codexbar::providers::cursor::local_csv::summarize(7);
                let thirty = codexbar::providers::cursor::local_csv::summarize(30);
                if thirty.row_count > 0 {
                    SpendValues {
                        seven_day: (seven.row_count > 0).then_some(seven.total_cost_usd),
                        thirty_day: Some(thirty.total_cost_usd),
                        seven_day_tokens: (seven.row_count > 0).then_some(seven.total_tokens),
                        thirty_day_tokens: Some(thirty.total_tokens),
                        source: "local Cursor tokscale cache".to_string(),
                        refreshing: false,
                        stale_updated_at: None,
                    }
                } else {
                    cached_spend(cached_snapshot)
                }
            }
            "grok" => {
                let seven = codexbar::providers::grok::local_sessions::summarize(7);
                let thirty = codexbar::providers::grok::local_sessions::summarize(30);
                let mut spend = cached_spend(cached_snapshot);
                spend.seven_day_tokens = (seven.session_count > 0).then_some(seven.total_tokens);
                spend.thirty_day_tokens = (thirty.session_count > 0).then_some(thirty.total_tokens);
                if thirty.session_count > 0 {
                    spend.source = "local Grok sessions".to_string();
                }
                spend
            }
            "antigravity" => {
                use codexbar::providers::antigravity::local_sessions::LocalHistoryCoverage;
                let seven = codexbar::providers::antigravity::local_sessions::summarize(7);
                let thirty = codexbar::providers::antigravity::local_sessions::summarize(30);
                let mut spend = cached_spend(cached_snapshot);
                spend.seven_day_tokens = matches!(seven.coverage, LocalHistoryCoverage::Complete)
                    .then_some(seven.total_tokens);
                spend.thirty_day_tokens = matches!(thirty.coverage, LocalHistoryCoverage::Complete)
                    .then_some(thirty.total_tokens);
                if matches!(thirty.coverage, LocalHistoryCoverage::Complete) {
                    spend.source = "local Antigravity history".to_string();
                }
                spend
            }
            _ => cached_spend(cached_snapshot),
        };

        let currency = cached_snapshot
            .and_then(|snapshot| snapshot.cost.as_ref())
            .map(|cost| cost.currency_code.clone())
            .unwrap_or_else(|| "USD".to_string());
        let daily = cached_snapshot
            .and_then(|snapshot| snapshot.cost.as_ref())
            .map(|cost| {
                cost.daily
                    .iter()
                    .map(|point| UsageSpendDailyPoint {
                        day: point.day.clone(),
                        amount: point.amount,
                    })
                    .collect()
            })
            .unwrap_or_default();
        rows.push(UsageSpendRow {
            provider_id: provider_id.clone(),
            display_name,
            seven_day: spend.seven_day,
            thirty_day: spend.thirty_day,
            seven_day_tokens: spend.seven_day_tokens,
            thirty_day_tokens: spend.thirty_day_tokens,
            currency,
            source: spend.source,
            included_in_overview: settings.enabled_providers.contains(&provider_id)
                || cached_snapshot.is_some(),
            daily,
            refreshing: spend.refreshing,
            stale_updated_at: spend.stale_updated_at,
        });
    }

    let history_days = if selected_days == 0 {
        365
    } else {
        selected_days.clamp(1, 365)
    };
    let selected_summary: CostSummary = match history_days {
        7 => codex_7_summary,
        30 => codex_30_summary,
        days => CostScanner::new(days)
            .with_options(codex_scan_options)
            .scan_codex(),
    };
    let contract = build_local_spend_contract_from_summary(
        "codex",
        history_days,
        include_opencodex,
        hide_native,
        settings.hide_personal_info,
        selected_summary,
    );
    UsageSpendSummary { rows, contract }
}

fn total_token_mix(mix: &codexbar::spend_contract::SpendTokenMix) -> Option<u64> {
    let values = [
        mix.input_tokens,
        mix.output_tokens,
        mix.cache_creation_tokens,
    ];
    let mut saw = false;
    let mut total = 0u64;
    for value in values.into_iter().flatten() {
        saw = true;
        total = total.saturating_add(value);
    }
    saw.then_some(total)
}

fn cached_spend(snapshot: Option<&ProviderUsageSnapshot>) -> SpendValues {
    let Some(snapshot) = snapshot else {
        return SpendValues {
            seven_day: None,
            thirty_day: None,
            seven_day_tokens: None,
            thirty_day_tokens: None,
            source: "unavailable".to_string(),
            refreshing: false,
            stale_updated_at: None,
        };
    };
    let Some(cost) = snapshot.cost.as_ref() else {
        return SpendValues {
            seven_day: None,
            thirty_day: None,
            seven_day_tokens: None,
            thirty_day_tokens: None,
            source: if snapshot.error.is_some() {
                "unavailable".to_string()
            } else {
                snapshot.source_label.clone()
            },
            refreshing: false,
            stale_updated_at: None,
        };
    };
    let period = cost.period.trim();
    let period_lower = period.to_ascii_lowercase();
    let (seven_day, thirty_day) = if cost.daily.is_empty() {
        (
            None,
            (period_lower.contains("30 day") || period_lower.contains("30-day"))
                .then_some(cost.used),
        )
    } else {
        let today = chrono::Utc::now().date_naive();
        let seven_cutoff = today - chrono::Duration::days(6);
        let mut seven = 0.0;
        let mut thirty = 0.0;
        let mut saw_seven = false;
        let mut saw_thirty = false;
        for point in &cost.daily {
            let Ok(day) = chrono::NaiveDate::parse_from_str(&point.day, "%Y-%m-%d") else {
                continue;
            };
            if day > today {
                continue;
            }
            thirty += point.amount;
            saw_thirty = true;
            if day >= seven_cutoff {
                seven += point.amount;
                saw_seven = true;
            }
        }
        (saw_seven.then_some(seven), saw_thirty.then_some(thirty))
    };
    SpendValues {
        seven_day,
        thirty_day,
        seven_day_tokens: None,
        thirty_day_tokens: None,
        source: if period.is_empty() {
            snapshot.source_label.clone()
        } else {
            format!("period ({period})")
        },
        refreshing: false,
        stale_updated_at: None,
    }
}

#[cfg(test)]
mod cache_key_tests {
    use super::*;

    #[test]
    fn invalidated_owner_clears_orphaned_indexing_activity() {
        let mut coordinator = UsageSpendCoordinator::default();
        let owner = coordinator.begin("account:old".to_string());

        assert!(coordinator.clear_if_indexing(&owner));
        assert!(!coordinator.is_current(&owner));
    }

    #[test]
    fn old_owner_cleanup_cannot_clear_a_replacement() {
        let mut coordinator = UsageSpendCoordinator::default();
        let old = coordinator.begin("account:old".to_string());
        let replacement = coordinator.begin("account:new".to_string());

        assert!(!coordinator.clear_if_indexing(&old));
        assert!(coordinator.is_current(&replacement));
    }

    #[test]
    fn settings_replacement_preserves_an_intentional_pause() {
        let mut coordinator = UsageSpendCoordinator::default();
        let old = coordinator.begin("settings:old".to_string());
        let replacement = coordinator.begin("settings:new".to_string());
        let status = codexbar::core::CachedCostReadStatus {
            codex_scan_pause_reason: Some(codexbar::core::CodexScanPauseReason::NoProgress),
            ..Default::default()
        };
        mark_refresh_paused_if_codex_scan_paused(
            &mut coordinator,
            &replacement,
            true,
            status.codex_scan_pause_reason.as_ref(),
        );

        assert!(!coordinator.clear_if_indexing(&old));
        assert_eq!(
            coordinator.current.as_ref().map(|(_, phase)| *phase),
            Some(UsageSpendRefreshPhase::Paused)
        );
    }

    #[test]
    fn privacy_mode_is_part_of_usage_spend_cache_identity() {
        let public = usage_spend_cache_key_with_privacy(&[], 30, false, false, false);
        let private = usage_spend_cache_key_with_privacy(&[], 30, false, false, true);
        assert_ne!(public, private);
    }
}
