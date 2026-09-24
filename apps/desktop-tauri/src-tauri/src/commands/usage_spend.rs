//! Usage & Spend settings tab: 7-day / 30-day local cost aggregates.

use codexbar::cost_scanner::{CostScanner, CostSummary};
use codexbar::spend_contract::{
    SpendContract, build_local_spend_contract, build_local_spend_contract_from_summary,
};
use serde::Serialize;
use tauri::State;

use super::ProviderUsageSnapshot;
use crate::proof_runtime::ContainmentProof;
use crate::state::AppState;
use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seven_day_estimate: Option<codexbar::spend_contract::LocalCostEstimate>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thirty_day_estimate: Option<codexbar::spend_contract::LocalCostEstimate>,
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
    let (cached, containment_proof) = {
        let guard = state.lock().map_err(|e| e.to_string())?;
        let containment_proof = guard.containment_proof.clone();
        if guard.is_containment_proof() && containment_proof.is_none() {
            return Err("containment proof state is missing its manifest".to_string());
        }
        (guard.provider_cache.clone(), containment_proof)
    };

    let selected_days = history_days.unwrap_or(30);
    let force_refresh = force_refresh.unwrap_or(false);
    let built = tauri::async_runtime::spawn_blocking(move || {
        build_usage_spend_summary_cached(
            &cached,
            selected_days,
            force_refresh,
            containment_proof.as_ref(),
        )
    })
    .await
    .map_err(|e| format!("usage spend worker failed: {e}"))??;
    let (current_cached, current_containment_proof) = {
        let guard = state.lock().map_err(|e| e.to_string())?;
        let containment_proof = guard.containment_proof.clone();
        if guard.is_containment_proof() && containment_proof.is_none() {
            return Err("containment proof state is missing its manifest".to_string());
        }
        (guard.provider_cache.clone(), containment_proof)
    };
    let current_settings = current_containment_proof
        .as_ref()
        .map(ContainmentProof::proof_settings)
        .unwrap_or_else(codexbar::settings::Settings::load);
    let current_key = usage_spend_cache_key_for_runtime(
        &current_cached,
        selected_days,
        &current_settings,
        current_containment_proof.as_ref(),
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
    containment_proof: Option<&ContainmentProof>,
) -> Result<BuiltUsageSpendSummary, String> {
    let settings = containment_proof
        .map(ContainmentProof::proof_settings)
        .unwrap_or_else(codexbar::settings::Settings::load);
    let key =
        usage_spend_cache_key_for_runtime(cached, selected_days, &settings, containment_proof);
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
    let summary = containment_proof.map_or_else(
        || build_usage_spend_summary(cached, selected_days, &settings, force_refresh),
        |proof| build_usage_spend_summary_in_containment_proof(cached, selected_days, proof),
    );
    let refreshing = summary_is_refreshing(&summary);
    let codex_scan_pause_reason = containment_proof.is_none().then(|| {
        codexbar::core::JsonlScanner::load_cache_status(codexbar::core::ProviderId::Codex, None)
            .codex_scan_pause_reason
    });
    let codex_scan_pause_reason = codex_scan_pause_reason.flatten();

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
    usage_spend_cache_key_with_identity(
        cached,
        selected_days,
        settings.open_codex_usage_logs_enabled,
        settings.hide_native_codex_cost_when_open_codex_present,
        settings.hide_personal_info,
        None,
    )
}

fn usage_spend_cache_key_for_runtime(
    cached: &[ProviderUsageSnapshot],
    selected_days: u32,
    settings: &codexbar::settings::Settings,
    containment_proof: Option<&ContainmentProof>,
) -> String {
    if containment_proof.is_none() {
        return usage_spend_cache_key(cached, selected_days, settings);
    }
    let proof_identity = containment_proof.map(containment_proof_cache_identity);
    usage_spend_cache_key_with_identity(
        cached,
        selected_days,
        settings.open_codex_usage_logs_enabled,
        settings.hide_native_codex_cost_when_open_codex_present,
        settings.hide_personal_info,
        proof_identity.as_deref(),
    )
}

#[cfg(test)]
fn usage_spend_cache_key_with_privacy(
    cached: &[ProviderUsageSnapshot],
    selected_days: u32,
    include_opencodex: bool,
    hide_native: bool,
    hide_personal_info: bool,
) -> String {
    usage_spend_cache_key_with_identity(
        cached,
        selected_days,
        include_opencodex,
        hide_native,
        hide_personal_info,
        None,
    )
}

fn usage_spend_cache_key_with_identity(
    cached: &[ProviderUsageSnapshot],
    selected_days: u32,
    include_opencodex: bool,
    hide_native: bool,
    hide_personal_info: bool,
    proof_identity: Option<&str>,
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
    let key = format!(
        "{}|{}|{}|{}|{}|{}",
        chrono::Local::now().date_naive(),
        selected_days,
        include_opencodex,
        hide_native,
        hide_personal_info,
        revisions.join(";")
    );
    match proof_identity {
        Some(identity) => format!("{key}|proof:{identity}"),
        None => key,
    }
}

fn containment_proof_cache_identity(proof: &ContainmentProof) -> String {
    format!(
        "{}:{}:{}:{}:{}:{}:{}",
        proof.manifest.schema,
        proof.manifest.version,
        proof.manifest.kind,
        proof.manifest.provider,
        proof.manifest.fixture_roots.gemini_cli_home.display(),
        proof.manifest.fixture_roots.tokscale_config_dir.display(),
        proof.manifest.now_utc.to_rfc3339(),
    )
}

fn containment_proof_database_roots(proof: &ContainmentProof) -> [PathBuf; 3] {
    let gemini_cli_home = &proof.manifest.fixture_roots.gemini_cli_home;
    [
        gemini_cli_home
            .join("antigravity-cli")
            .join("conversations"),
        gemini_cli_home.join("antigravity"),
        gemini_cli_home.join("antigravity").join("conversations"),
    ]
}

fn containment_proof_jsonl_sessions_root(proof: &ContainmentProof) -> PathBuf {
    proof
        .manifest
        .fixture_roots
        .tokscale_config_dir
        .join("antigravity-cache")
        .join("sessions")
}

fn unavailable_codex_spend_contract(history_days: u32) -> SpendContract {
    SpendContract {
        provider_id: "codex".to_string(),
        history_days: if history_days == 0 {
            365
        } else {
            history_days.clamp(1, 365)
        },
        known_cost_usd: None,
        known_zero: false,
        provenance: codexbar::spend_contract::CostProvenance::Unknown,
        price_coverage: Default::default(),
        price_coverage_ratio: None,
        history_coverage_established: false,
        token_mix: Default::default(),
        conversation_count: 0,
        models: Vec::new(),
        projects: Vec::new(),
        conversations: Vec::new(),
        daily: Vec::new(),
        hourly_activity: Vec::new(),
        project_source_status: None,
        custom_pricing_active: false,
        imports: Vec::new(),
    }
}

fn build_usage_spend_summary_in_containment_proof(
    cached: &[ProviderUsageSnapshot],
    selected_days: u32,
    proof: &ContainmentProof,
) -> UsageSpendSummary {
    let database_roots = containment_proof_database_roots(proof);
    let jsonl_sessions_root = containment_proof_jsonl_sessions_root(proof);
    let seven = codexbar::providers::antigravity::local_sessions::summarize_from_roots(
        &database_roots,
        &jsonl_sessions_root,
        proof.manifest.now_utc,
        7,
    );
    let thirty = codexbar::providers::antigravity::local_sessions::summarize_from_roots(
        &database_roots,
        &jsonl_sessions_root,
        proof.manifest.now_utc,
        30,
    );
    let cached_snapshot = cached
        .iter()
        .find(|snapshot| snapshot.provider_id == "antigravity");
    let spend = antigravity_spend_values(cached_spend(cached_snapshot), &seven, &thirty);
    let display_name = cached_snapshot
        .map(|snapshot| snapshot.display_name.trim())
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| "Antigravity".to_string());
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
    let seven_day_estimate = Some(seven.cost_estimate.clone());
    let thirty_day_estimate = Some(thirty.cost_estimate.clone());
    let row = UsageSpendRow {
        provider_id: "antigravity".to_string(),
        display_name,
        seven_day: spend.seven_day,
        thirty_day: spend.thirty_day,
        seven_day_estimate,
        thirty_day_estimate,
        seven_day_tokens: spend.seven_day_tokens,
        thirty_day_tokens: spend.thirty_day_tokens,
        currency,
        source: spend.source,
        included_in_overview: true,
        daily,
        refreshing: false,
        stale_updated_at: None,
    };
    UsageSpendSummary {
        rows: vec![row],
        contract: unavailable_codex_spend_contract(selected_days),
        reporting_day: proof
            .manifest
            .now_utc
            .date_naive()
            .format("%Y-%m-%d")
            .to_string(),
        dashboard_timezone: codexbar::core::local_timezone_name(),
    }
}

fn build_usage_spend_summary(
    cached: &[ProviderUsageSnapshot],
    selected_days: u32,
    settings: &codexbar::settings::Settings,
    force_refresh: bool,
) -> UsageSpendSummary {
    let include_opencodex = settings.open_codex_usage_logs_enabled;
    let hide_native = settings.hide_native_codex_cost_when_open_codex_present;
    let pi_selected = settings.enabled_providers.iter().any(|id| id == "pi")
        || cached.iter().any(|snapshot| snapshot.provider_id == "pi");
    let include_pi_in_native = !pi_selected;

    // Upstream 0.55.0 #3105: independent provider baselines load in parallel.
    // Keep each provider's 7d/30d scans serial so they can safely share that
    // provider's incremental cache, while Codex and Claude run concurrently.
    let codex_scan_options = if force_refresh {
        codexbar::core::CostScanOptions::app_driven()
    } else {
        codexbar::core::CostScanOptions::default()
    };
    let mut codex_scan_options = codex_scan_options;
    codex_scan_options.include_pi_sessions = include_pi_in_native;
    let (
        (codex_7_summary, codex_30_summary),
        (claude_7_summary, claude_30_summary),
        (pi_7_summary, pi_30_summary),
    ) = std::thread::scope(|scope| {
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
                CostScanner::new(7)
                    .scan_claude_with_cancel_and_pi_sessions(None, include_pi_in_native),
                CostScanner::new(30)
                    .scan_claude_with_cancel_and_pi_sessions(None, include_pi_in_native),
            )
        });
        let pi = scope.spawn(|| {
            (
                CostScanner::new(7).scan_pi(),
                CostScanner::new(30).scan_pi(),
            )
        });
        (
            codex.join().expect("Codex spend scan worker panicked"),
            claude.join().expect("Claude spend scan worker panicked"),
            pi.join().expect("Pi spend scan worker panicked"),
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
    let pi_7_contract = build_local_spend_contract_from_summary(
        "pi",
        7,
        false,
        false,
        settings.hide_personal_info,
        pi_7_summary.clone(),
    );
    let pi_30_contract = build_local_spend_contract_from_summary(
        "pi",
        30,
        false,
        false,
        settings.hide_personal_info,
        pi_30_summary.clone(),
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

        let mut local_cost_estimates = None;
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
            "pi" => SpendValues {
                seven_day: pi_7_contract.known_cost_usd,
                thirty_day: pi_30_contract.known_cost_usd,
                seven_day_tokens: total_token_mix(&pi_7_contract.token_mix),
                thirty_day_tokens: total_token_mix(&pi_30_contract.token_mix),
                source: "local Pi/OMP history".to_string(),
                refreshing: !pi_30_summary.history_coverage_established,
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
                let seven = codexbar::providers::antigravity::local_sessions::summarize(7);
                let thirty = codexbar::providers::antigravity::local_sessions::summarize(30);
                let spend =
                    antigravity_spend_values(cached_spend(cached_snapshot), &seven, &thirty);
                local_cost_estimates = Some((seven.cost_estimate, thirty.cost_estimate));
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
        let (seven_day_estimate, thirty_day_estimate) = local_cost_estimates
            .map(|(seven, thirty)| (Some(seven), Some(thirty)))
            .unwrap_or((None, None));
        rows.push(UsageSpendRow {
            provider_id: provider_id.clone(),
            display_name,
            seven_day: spend.seven_day,
            thirty_day: spend.thirty_day,
            seven_day_estimate,
            thirty_day_estimate,
            seven_day_tokens: spend.seven_day_tokens,
            thirty_day_tokens: spend.thirty_day_tokens,
            currency,
            source: spend.source,
            included_in_overview: include_in_shared_overview(
                &provider_id,
                settings.enabled_providers.contains(&provider_id),
                cached_snapshot.is_some(),
            ),
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
    let reporting_day = last_included_reporting_day(&contract);
    let dashboard_timezone = codexbar::core::local_timezone_name();
    UsageSpendSummary {
        rows,
        contract,
        reporting_day,
        dashboard_timezone,
    }
}

/// Pi is an alternate local-history view over rows that may already be
/// projected into Codex or Claude. Keep it out of the shared denominator so
/// enabling Pi cannot double-count the same physical usage.
fn include_in_shared_overview(provider_id: &str, enabled: bool, cached: bool) -> bool {
    provider_id != "pi" && (enabled || cached)
}

fn last_included_reporting_day(contract: &SpendContract) -> String {
    contract
        .daily
        .iter()
        .filter_map(|point| chrono::NaiveDate::parse_from_str(&point.day, "%Y-%m-%d").ok())
        .max()
        .unwrap_or_else(|| chrono::Local::now().date_naive())
        .format("%Y-%m-%d")
        .to_string()
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

fn antigravity_spend_values(
    mut spend: SpendValues,
    seven: &codexbar::spend_contract::LocalTokenHistorySummary,
    thirty: &codexbar::spend_contract::LocalTokenHistorySummary,
) -> SpendValues {
    use codexbar::spend_contract::LocalHistoryCoverage;

    spend.seven_day = seven.total_usd();
    spend.thirty_day = thirty.total_usd();
    spend.seven_day_tokens =
        (seven.coverage == LocalHistoryCoverage::Complete).then_some(seven.total_tokens);
    spend.thirty_day_tokens =
        (thirty.coverage == LocalHistoryCoverage::Complete).then_some(thirty.total_tokens);
    if spend.thirty_day.is_some() {
        spend.source = "local Antigravity history · API list-price estimate".to_string();
    } else if thirty.cost_estimate.known_subtotal_usd.is_some() {
        spend.source = "local Antigravity history · known API list-price subtotal".to_string();
    } else if thirty.coverage == LocalHistoryCoverage::Complete {
        spend.source = "local Antigravity history · unpriced".to_string();
    }
    spend
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
    use std::fs;

    fn local_history(
        total_tokens: u64,
        coverage: codexbar::spend_contract::LocalHistoryCoverage,
        known_subtotal_usd: Option<f64>,
        unpriced: u32,
    ) -> codexbar::spend_contract::LocalTokenHistorySummary {
        codexbar::spend_contract::LocalTokenHistorySummary {
            total_tokens,
            session_count: if total_tokens > 0 { 1 } else { 0 },
            coverage,
            cost_estimate: codexbar::spend_contract::LocalCostEstimate {
                known_subtotal_usd,
                coverage: codexbar::spend_contract::CostCoverageCounts {
                    estimated: if known_subtotal_usd.is_some() { 1 } else { 0 },
                    unpriced,
                    ..Default::default()
                },
            },
        }
    }

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

    #[test]
    fn pi_history_is_an_alternate_view_not_a_shared_overview_source() {
        assert!(!include_in_shared_overview("pi", true, true));
        assert!(include_in_shared_overview("codex", true, false));
        assert!(include_in_shared_overview("claude", false, true));
        assert!(!include_in_shared_overview("codex", false, false));
    }

    #[test]
    fn antigravity_partial_history_exposes_only_the_known_subtotal() {
        use codexbar::spend_contract::LocalHistoryCoverage;

        let seven = local_history(100, LocalHistoryCoverage::Partial, Some(1.25), 0);
        let thirty = local_history(200, LocalHistoryCoverage::Partial, Some(2.50), 0);
        let spend = antigravity_spend_values(cached_spend(None), &seven, &thirty);

        assert_eq!(spend.seven_day, None);
        assert_eq!(spend.thirty_day, None);
        assert_eq!(spend.seven_day_tokens, None);
        assert_eq!(spend.thirty_day_tokens, None);
        assert!(spend.source.contains("known API list-price subtotal"));
    }

    #[test]
    fn antigravity_complete_empty_history_is_a_known_zero() {
        use codexbar::spend_contract::LocalHistoryCoverage;

        let seven = local_history(0, LocalHistoryCoverage::Complete, None, 0);
        let thirty = local_history(0, LocalHistoryCoverage::Complete, None, 0);
        let spend = antigravity_spend_values(cached_spend(None), &seven, &thirty);

        assert_eq!(spend.seven_day, Some(0.0));
        assert_eq!(spend.thirty_day, Some(0.0));
        assert_eq!(spend.seven_day_tokens, Some(0));
        assert_eq!(spend.thirty_day_tokens, Some(0));
        assert!(spend.source.contains("API list-price estimate"));
    }

    fn test_containment_proof() -> (crate::test_support::TempDir, ContainmentProof) {
        let root = crate::test_support::TempDir::new();
        let gemini_cli_home = root.path().join("gemini");
        let tokscale_config_dir = root.path().join("tokscale");
        let scratch_root = root.path().join("scratch");
        fs::create_dir_all(&gemini_cli_home).unwrap();
        fs::create_dir_all(&tokscale_config_dir).unwrap();
        fs::create_dir_all(&scratch_root).unwrap();
        let manifest = serde_json::json!({
            "schema": "codexbar.containment-proof",
            "version": 1,
            "kind": "antigravityUsageSpend",
            "provider": "antigravity",
            "fixtureRoots": {
                "geminiCliHome": gemini_cli_home,
                "tokscaleConfigDir": tokscale_config_dir
            },
            "scratchRoot": scratch_root,
            "nowUtc": "2026-09-24T12:34:56Z"
        });
        let proof = ContainmentProof::from_manifest_json(&manifest.to_string()).unwrap();
        (root, proof)
    }

    fn write_proof_history(proof: &ContainmentProof) {
        let sessions = containment_proof_jsonl_sessions_root(proof);
        fs::create_dir_all(&sessions).unwrap();
        let now = proof.manifest.now_utc;
        let recent = (now - chrono::Duration::days(1)).timestamp_millis();
        let older = (now - chrono::Duration::days(10)).timestamp_millis();
        let lines = [
            serde_json::json!({
                "type": "session_meta",
                "modelId": "claude-sonnet-4-6"
            })
            .to_string(),
            serde_json::json!({
                "type": "usage",
                "responseId": "recent",
                "timestamp": recent,
                "input": 100,
                "output": 20
            })
            .to_string(),
            serde_json::json!({
                "type": "usage",
                "responseId": "older",
                "timestamp": older,
                "input": 200,
                "output": 30
            })
            .to_string(),
        ]
        .join("\n");
        fs::write(sessions.join("fixture.jsonl"), lines).unwrap();
    }

    #[test]
    fn containment_proof_routes_to_exact_antigravity_singleton_with_fixed_totals() {
        let (_root, proof) = test_containment_proof();
        write_proof_history(&proof);

        let summary = build_usage_spend_summary_in_containment_proof(&[], 30, &proof);

        assert_eq!(summary.rows.len(), 1);
        let row = &summary.rows[0];
        assert_eq!(row.provider_id, "antigravity");
        assert_eq!(row.seven_day_tokens, Some(120));
        assert_eq!(row.thirty_day_tokens, Some(350));
        assert_eq!(summary.reporting_day, "2026-09-24");
    }

    #[test]
    fn containment_proof_returns_empty_unavailable_codex_contract() {
        let (_root, proof) = test_containment_proof();
        let summary = build_usage_spend_summary_in_containment_proof(&[], 30, &proof);

        assert_eq!(summary.contract.provider_id, "codex");
        assert_eq!(summary.contract.known_cost_usd, None);
        assert!(!summary.contract.history_coverage_established);
        assert_eq!(
            summary.contract.provenance,
            codexbar::spend_contract::CostProvenance::Unknown
        );
        assert!(!summary.contract.custom_pricing_active);
        assert!(summary.contract.models.is_empty());
        assert!(summary.contract.imports.is_empty());
    }

    #[test]
    fn containment_proof_identity_is_part_of_cache_key() {
        let (_root, proof) = test_containment_proof();
        let settings = proof.proof_settings();
        let proof_key = usage_spend_cache_key_for_runtime(&[], 30, &settings, Some(&proof));
        let production_key = usage_spend_cache_key_for_runtime(&[], 30, &settings, None);

        assert_ne!(proof_key, production_key);
        assert!(proof_key.contains("proof:"));
    }
}
