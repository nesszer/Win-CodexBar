//! Unified Usage & Spend accounting contract for upstream 0.53 parity.
//! Accounting semantics live here so UI/CLI never infer unknown vs zero.

mod custom_pricing;
mod local_history;
mod merge;
mod opencodex;

pub use local_history::{
    LocalCostEstimate, LocalHistoryCoverage, LocalTokenHistorySummary, local_token_history_json,
};

use std::collections::{HashMap, HashSet};

use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::codex_workspaces::{CodexWorkspacesIndex, ProjectUsage, SessionUsage, SourceStatus};
use crate::cost_reporting_period::{CostReportingPeriod, MAX_ROLLING_DAYS};
use crate::cost_scanner::{
    CostScanner, CostSummary, get_daily_cost_history, get_daily_token_history,
};

use custom_pricing::{CustomPricing, CustomRates};
use merge::{
    ActivityHistogram, activity_from_sessions, add_optional, merge_activity, merge_coverage,
    merge_daily, merge_models, merge_token_mix, sum_optional_cost,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CostProvenance {
    ListPriceEstimate,
    VendorMetered,
    Mixed,
    Unknown,
}

impl CostProvenance {
    /// Narrow snapshot provenance to the costs actually present in a window.
    ///
    /// This mirrors upstream 0.56.2 `CostProvenance.forWindow`: a vendor source
    /// remains vendor-metered when it has window costs, while a mixed source is
    /// mixed only when both its list-price and metered sides are present.
    pub(crate) fn for_window(
        snapshot: Self,
        has_window_costs: bool,
        includes_metered: bool,
    ) -> Self {
        match snapshot {
            Self::VendorMetered => {
                if includes_metered || has_window_costs {
                    Self::VendorMetered
                } else {
                    Self::Unknown
                }
            }
            Self::Mixed => match (includes_metered, has_window_costs) {
                (true, true) => Self::Mixed,
                (true, false) => Self::VendorMetered,
                (false, true) => Self::ListPriceEstimate,
                (false, false) => Self::Unknown,
            },
            Self::ListPriceEstimate => {
                if has_window_costs {
                    Self::ListPriceEstimate
                } else {
                    Self::Unknown
                }
            }
            Self::Unknown => Self::Unknown,
        }
    }

    fn from_source_kinds(includes_vendor: bool, includes_list: bool) -> Self {
        match (includes_vendor, includes_list) {
            (true, true) => Self::Mixed,
            (true, false) => Self::VendorMetered,
            (false, true) => Self::ListPriceEstimate,
            (false, false) => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CostCoverageCounts {
    pub priced: u32,
    pub unpriced: u32,
    pub unmetered: u32,
    pub estimated: u32,
}

impl CostCoverageCounts {
    pub fn total(&self) -> u32 {
        self.checked_total().unwrap_or(u32::MAX)
    }

    fn checked_total(&self) -> Option<u32> {
        self.priced
            .checked_add(self.unpriced)?
            .checked_add(self.unmetered)?
            .checked_add(self.estimated)
    }

    pub fn coverage_ratio(&self) -> Option<f64> {
        let denominator = self.checked_total()?;
        let covered = self.priced.checked_add(self.estimated)?;
        (denominator > 0).then(|| covered as f64 / denominator as f64)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpendTokenMix {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub cache_creation_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
    /// Keeps arithmetic overflow distinct from an ordinary missing class while
    /// the report is merged in memory.  It is deliberately not part of the
    /// wire contract: both cases are exposed as unknown (`None`).
    #[serde(skip)]
    pub(crate) overflowed_classes: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpendModelRow {
    pub model: String,
    /// None = unknown/unpriced. Some(0.0) = known free.
    pub cost_usd: Option<f64>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub total_tokens: u64,
    pub custom_pricing: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpendDailyPoint {
    pub day: String,
    pub cost_usd: Option<f64>,
    pub total_tokens: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpendActivityCell {
    /// Monday=0, Sunday=6.
    pub weekday: u8,
    pub hour: u8,
    pub conversations: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedSpendSource {
    pub source_id: String,
    pub display_name: String,
    pub request_count: u32,
    pub conversation_count: u32,
    pub known_cost_usd: Option<f64>,
    pub provenance: CostProvenance,
    pub token_mix: SpendTokenMix,
    /// Sum of the importer's resolved per-entry totals: the authoritative
    /// `totalTokens` when present, else input + output + cache creation
    /// (`cache_read` is already part of input). Same basis as `models` and
    /// `daily`. Not part of the wire contract.
    #[serde(skip)]
    pub token_total: Option<u64>,
    pub coverage: CostCoverageCounts,
    pub models: Vec<SpendModelRow>,
    pub daily: Vec<SpendDailyPoint>,
    pub hourly_activity: Vec<SpendActivityCell>,
}

struct NativeSpendData {
    projects: Vec<ProjectUsage>,
    conversations: Vec<SessionUsage>,
    project_source_status: Option<SourceStatus>,
    activity: Vec<SpendActivityCell>,
    daily: Vec<SpendDailyPoint>,
}

struct ResolvedSpendData {
    known_cost_usd: Option<f64>,
    provenance: CostProvenance,
    price_coverage: CostCoverageCounts,
    price_coverage_exact: bool,
    token_mix: SpendTokenMix,
    models: Vec<SpendModelRow>,
    daily: Vec<SpendDailyPoint>,
    hourly_activity: Vec<SpendActivityCell>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpendContract {
    pub provider_id: String,
    /// Days of the rolling-only sidecars (workspaces, imports, per-day
    /// history). For month to date this is the days elapsed this month; for
    /// all available history it caps at 365. `reporting_period` names the
    /// window the totals actually cover.
    pub history_days: u32,
    /// Raw reporting period (`rolling:N`, `month-to-date`, `all`).
    #[serde(default)]
    pub reporting_period: String,
    /// Known subtotal for this window. None means unknown, never implicit zero.
    pub known_cost_usd: Option<f64>,
    pub known_zero: bool,
    pub provenance: CostProvenance,
    pub price_coverage: CostCoverageCounts,
    pub price_coverage_ratio: Option<f64>,
    pub history_coverage_established: bool,
    pub token_mix: SpendTokenMix,
    /// Window token total with each source's own rule applied
    /// (see [`resolve_token_total`]). Not part of the wire contract: the merged
    /// `token_mix` above cannot express native and imported rules at once.
    #[serde(skip)]
    pub token_total: Option<u64>,
    pub conversation_count: u32,
    pub models: Vec<SpendModelRow>,
    pub projects: Vec<ProjectUsage>,
    pub conversations: Vec<SessionUsage>,
    pub daily: Vec<SpendDailyPoint>,
    pub hourly_activity: Vec<SpendActivityCell>,
    pub project_source_status: Option<SourceStatus>,
    pub custom_pricing_active: bool,
    pub imports: Vec<ImportedSpendSource>,
}

/// Refreshes models.dev prices for the OpenCodex ledger before a fresh
/// Usage & Spend build (upstream 0.60.4 `refreshPricingIfNeeded`).
///
/// Call it only when a summary will be rebuilt: a cached read must never
/// start network work. It fetches at most once per models.dev cache path per
/// 15 minutes, and only when the catalog is stale or a priced row's exact
/// identity is missing from it.
pub async fn refresh_opencodex_pricing_if_needed() {
    opencodex::refresh_pricing_if_needed().await;
}

/// Build a stable accounting contract for a local-log provider.
/// `days == 0` means the legacy All-time window, bounded to 365 days locally.
pub fn build_local_spend_contract(
    provider_id: &str,
    days: u32,
    include_opencodex: bool,
) -> SpendContract {
    let history_days = if days == 0 { 365 } else { days.clamp(1, 365) };
    build_local_spend_contract_for_period(
        provider_id,
        CostReportingPeriod::Rolling(history_days),
        include_opencodex,
    )
}

/// Scan and build the accounting contract for a reporting period.
pub fn build_local_spend_contract_for_period(
    provider_id: &str,
    period: CostReportingPeriod,
    include_opencodex: bool,
) -> SpendContract {
    let scanner = CostScanner::for_period(period);
    let summary = match provider_id {
        "codex" => scanner.scan_codex(),
        "claude" => scanner.scan_claude(),
        "pi" => scanner.scan_pi(),
        "opencodego" => scanner.scan_opencodego_with_cancel(None),
        _ => CostSummary::default(),
    };
    build_contract_from_period_summary(
        provider_id,
        period,
        include_opencodex,
        false,
        crate::settings::Settings::load().hide_personal_info,
        summary,
    )
}

/// Build the accounting contract from an already-computed summary so callers do not rescan logs.
pub fn build_local_spend_contract_from_summary(
    provider_id: &str,
    history_days: u32,
    include_opencodex: bool,
    hide_native_codex_when_opencodex_present: bool,
    hide_personal_info: bool,
    summary: CostSummary,
) -> SpendContract {
    build_contract_from_period_summary(
        provider_id,
        CostReportingPeriod::Rolling(history_days.clamp(1, MAX_ROLLING_DAYS)),
        include_opencodex,
        hide_native_codex_when_opencodex_present,
        hide_personal_info,
        summary,
    )
}

/// Build the accounting contract for `period` from a summary scanned for that
/// same period.
///
/// Sources that only support rolling windows (Codex workspaces, per-day
/// history, OpenCodex imports) use [`CostReportingPeriod::sidecar_days`]:
/// month to date maps to the days elapsed this month and all available
/// history caps at a year. `history_days` reports that sidecar window.
pub fn build_contract_from_period_summary(
    provider_id: &str,
    period: CostReportingPeriod,
    include_opencodex: bool,
    hide_native_codex_when_opencodex_present: bool,
    hide_personal_info: bool,
    summary: CostSummary,
) -> SpendContract {
    let history_days = period.sidecar_days(Utc::now());
    let custom = CustomPricing::load();
    let native_models = model_rows(provider_id, &summary, &custom);
    let native_coverage = coverage_for_models(&native_models);
    let native_cost = known_subtotal(&native_models, &summary);
    let native_has_window_costs = native_models.iter().any(|model| model.cost_usd.is_some());
    let native_provenance = CostProvenance::for_window(
        CostProvenance::ListPriceEstimate,
        native_has_window_costs,
        false,
    );
    let native_token_mix = SpendTokenMix {
        input_tokens: Some(summary.input_tokens),
        output_tokens: Some(summary.output_tokens),
        cache_read_tokens: Some(summary.cached_tokens),
        cache_creation_tokens: None,
        reasoning_tokens: summary.reasoning_tokens,
        ..SpendTokenMix::default()
    };

    let native = load_native_spend(provider_id, history_days, hide_personal_info);
    let imports: Vec<_> = if include_opencodex {
        opencodex::load_for_subscription(provider_id, history_days, &custom)
            .into_iter()
            .collect()
    } else {
        Vec::new()
    };
    let imported = imports.first();
    let replace_native =
        provider_id == "codex" && hide_native_codex_when_opencodex_present && imported.is_some();
    let token_total = resolve_token_total(
        summary.total_tokens_for_provider(provider_id),
        imported,
        replace_native,
    );
    let resolved = resolve_spend(
        native_cost,
        native_provenance,
        native_has_window_costs,
        native_coverage,
        native_token_mix,
        native_models,
        native.daily.clone(),
        native.activity.clone(),
        imported,
        replace_native,
    );

    // Conversation count is clamped to u32::MAX before casting.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "clamped to u32::MAX before casting"
    )]
    let native_conversations = if native.conversations.is_empty() {
        summary.sessions_count
    } else {
        native.conversations.len().min(u32::MAX as usize) as u32
    };
    let imported_conversations = imported.map_or(0, |source| source.conversation_count);
    let conversation_count = if replace_native {
        imported_conversations
    } else {
        native_conversations.saturating_add(imported_conversations)
    };
    let known_zero = if replace_native {
        imported.is_some_and(|source| {
            source.known_cost_usd == Some(0.0) && source.coverage.unpriced == 0
        })
    } else {
        summary.known_zero && imports.is_empty()
    };

    SpendContract {
        provider_id: provider_id.to_string(),
        history_days,
        reporting_period: period.raw(),
        known_cost_usd: resolved.known_cost_usd,
        known_zero,
        provenance: resolved.provenance,
        price_coverage_ratio: if resolved.price_coverage_exact {
            resolved.price_coverage.coverage_ratio()
        } else {
            None
        },
        price_coverage: resolved.price_coverage,
        history_coverage_established: summary.history_coverage_established,
        token_mix: resolved.token_mix,
        token_total,
        conversation_count,
        models: resolved.models,
        projects: native.projects,
        conversations: native.conversations,
        daily: resolved.daily,
        hourly_activity: resolved.hourly_activity,
        project_source_status: native.project_source_status,
        custom_pricing_active: !custom.entries.is_empty(),
        imports,
    }
}

fn load_native_spend(
    provider_id: &str,
    history_days: u32,
    hide_personal_info: bool,
) -> NativeSpendData {
    if provider_id != "codex" {
        return NativeSpendData {
            projects: Vec::new(),
            conversations: Vec::new(),
            project_source_status: None,
            activity: Vec::new(),
            daily: daily_points(provider_id, history_days),
        };
    }
    match CodexWorkspacesIndex::new(history_days).load_snapshot(false, |_| {}) {
        Ok(mut snapshot) => {
            if hide_personal_info {
                snapshot.redact_for_privacy();
            }
            let activity = activity_from_sessions(&snapshot.sessions);
            let daily = snapshot
                .daily
                .iter()
                .map(|point| SpendDailyPoint {
                    day: point.day.clone(),
                    cost_usd: point.estimated_cost_usd,
                    total_tokens: Some(point.total_tokens),
                })
                .collect();
            NativeSpendData {
                projects: snapshot.projects,
                conversations: snapshot.sessions,
                project_source_status: Some(snapshot.source_status),
                activity,
                daily,
            }
        }
        Err(_) => NativeSpendData {
            projects: Vec::new(),
            conversations: Vec::new(),
            project_source_status: None,
            activity: Vec::new(),
            daily: daily_points(provider_id, history_days),
        },
    }
}

/// Totals native and imported sources separately, each with its own rule, then
/// combines them. The native total comes from
/// [`CostSummary::total_tokens_for_provider`], the same rule as the native
/// model and daily totals. The imported side uses the importer's resolved per-entry
/// totals (authoritative `totalTokens` when present) instead of re-deriving a
/// total from the merged `token_mix`, whose `cache_read` is already part of
/// input. `replace_native` mirrors [`resolve_spend`]: the imported source
/// replaces the native one entirely.
fn resolve_token_total(
    native_total: u64,
    imported: Option<&ImportedSpendSource>,
    replace_native: bool,
) -> Option<u64> {
    let imported_total = imported.and_then(|source| source.token_total);
    if replace_native {
        return imported_total;
    }
    Some(native_total.saturating_add(imported_total.unwrap_or(0)))
}

#[allow(
    clippy::too_many_arguments,
    reason = "signature mirrors the flat spend-contract config fields one-to-one"
)]
fn resolve_spend(
    native_cost: Option<f64>,
    native_provenance: CostProvenance,
    native_has_window_costs: bool,
    native_coverage: CostCoverageCounts,
    native_token_mix: SpendTokenMix,
    native_models: Vec<SpendModelRow>,
    native_daily: Vec<SpendDailyPoint>,
    native_activity: Vec<SpendActivityCell>,
    imported: Option<&ImportedSpendSource>,
    replace_native: bool,
) -> ResolvedSpendData {
    match imported {
        Some(imported) if replace_native => ResolvedSpendData {
            known_cost_usd: imported.known_cost_usd,
            provenance: imported.provenance,
            price_coverage: imported.coverage.clone(),
            price_coverage_exact: imported.coverage.checked_total().is_some(),
            token_mix: imported.token_mix.clone(),
            models: imported.models.clone(),
            daily: imported.daily.clone(),
            hourly_activity: imported.hourly_activity.clone(),
        },
        Some(imported) => {
            let (price_coverage, price_coverage_exact) =
                merge_coverage(native_coverage, &imported.coverage);
            ResolvedSpendData {
                known_cost_usd: sum_optional_cost(native_cost, imported.known_cost_usd),
                provenance: merge_provenance(
                    native_provenance,
                    native_has_window_costs,
                    imported.provenance,
                    imported.known_cost_usd.is_some(),
                ),
                price_coverage,
                price_coverage_exact,
                token_mix: merge_token_mix(native_token_mix, &imported.token_mix),
                models: merge_models(native_models, &imported.models),
                daily: merge_daily(native_daily, &imported.daily),
                hourly_activity: merge_activity(native_activity, &imported.hourly_activity),
            }
        }
        None => ResolvedSpendData {
            known_cost_usd: native_cost,
            provenance: native_provenance,
            price_coverage_exact: native_coverage.checked_total().is_some(),
            price_coverage: native_coverage,
            token_mix: native_token_mix,
            models: native_models,
            daily: native_daily,
            hourly_activity: native_activity,
        },
    }
}

fn merge_provenance(
    left: CostProvenance,
    left_has_window_costs: bool,
    right: CostProvenance,
    right_has_window_costs: bool,
) -> CostProvenance {
    let mut merged = None;
    for (provenance, has_window_costs) in [
        (left, left_has_window_costs),
        (right, right_has_window_costs),
    ] {
        if !has_window_costs {
            continue;
        }
        merged = Some(match merged {
            None => provenance,
            Some(existing) => combine_provenance(existing, provenance),
        });
    }
    merged.unwrap_or(CostProvenance::Unknown)
}

fn combine_provenance(left: CostProvenance, right: CostProvenance) -> CostProvenance {
    match (left, right) {
        (CostProvenance::Unknown, _) | (_, CostProvenance::Unknown) => CostProvenance::Unknown,
        (CostProvenance::Mixed, _) | (_, CostProvenance::Mixed) => CostProvenance::Mixed,
        (CostProvenance::ListPriceEstimate, CostProvenance::ListPriceEstimate) => {
            CostProvenance::ListPriceEstimate
        }
        (CostProvenance::VendorMetered, CostProvenance::VendorMetered) => {
            CostProvenance::VendorMetered
        }
        (CostProvenance::ListPriceEstimate, CostProvenance::VendorMetered)
        | (CostProvenance::VendorMetered, CostProvenance::ListPriceEstimate) => {
            CostProvenance::Mixed
        }
    }
}

fn model_rows(
    provider_id: &str,
    summary: &CostSummary,
    custom: &CustomPricing,
) -> Vec<SpendModelRow> {
    let mut names: HashSet<String> = summary.by_model.keys().cloned().collect();
    names.extend(summary.by_model_tokens.keys().cloned());
    names.extend(summary.unknown_models.iter().cloned());
    let mut rows: Vec<_> = names
        .into_iter()
        .map(|model| {
            let counts = summary
                .by_model_tokens
                .get(&model)
                .cloned()
                .unwrap_or_default();
            let custom_rates = custom.rates(provider_id, &model);
            // Exact-match overlay is authoritative when present. Missing fields
            // remain unknown rather than falling back to built-in/model.dev rates.
            let cost_usd = if let Some(rates) = custom_rates {
                rates.cost(&counts)
            } else if summary.unknown_models.contains(&model) {
                None
            } else {
                summary
                    .by_model
                    .get(&model)
                    .copied()
                    .filter(|value| value.is_finite() && *value >= 0.0)
            };
            SpendModelRow {
                model,
                cost_usd,
                input_tokens: counts.input_tokens,
                output_tokens: counts.output_tokens,
                cache_read_tokens: counts.cached_tokens,
                total_tokens: counts.total_for_provider(provider_id),
                custom_pricing: custom_rates.is_some(),
            }
        })
        .collect();
    rows.sort_by(|left, right| match (left.cost_usd, right.cost_usd) {
        (Some(a), Some(b)) => b
            .partial_cmp(&a)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.model.cmp(&right.model)),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => left.model.cmp(&right.model),
    });
    rows
}

fn coverage_for_models(models: &[SpendModelRow]) -> CostCoverageCounts {
    let mut coverage = CostCoverageCounts::default();
    for model in models {
        if model.cost_usd.is_some() {
            coverage.estimated = coverage.estimated.saturating_add(1);
        } else {
            coverage.unpriced = coverage.unpriced.saturating_add(1);
        }
    }
    coverage
}

fn known_subtotal(models: &[SpendModelRow], summary: &CostSummary) -> Option<f64> {
    if models.is_empty() {
        return summary.known_zero.then_some(0.0);
    }
    let mut total = 0.0;
    let mut saw_known = false;
    for model in models {
        if let Some(cost) = model.cost_usd {
            total += cost;
            saw_known = true;
        }
    }
    (saw_known && total.is_finite()).then_some(total)
}

fn daily_points(provider_id: &str, days: u32) -> Vec<SpendDailyPoint> {
    let costs: HashMap<String, Option<f64>> = get_daily_cost_history(provider_id, days)
        .into_iter()
        .collect();
    let (tokens, incomplete) = get_daily_token_history(provider_id, days);
    tokens
        .into_iter()
        .map(|(day, total_tokens)| SpendDailyPoint {
            cost_usd: costs.get(&day).copied().flatten().filter(|_| !incomplete),
            day,
            total_tokens: (!incomplete).then_some(total_tokens),
        })
        .collect()
}

#[cfg(test)]
mod tests;
