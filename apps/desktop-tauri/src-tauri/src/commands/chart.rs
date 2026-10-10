//! Provider chart data commands and DTOs.
//!
//! Cost history comes from the shared JSONL cost scanner and is available for
//! every provider. Credits history + usage breakdowns currently only apply to
//! the Codex / OpenAI dashboard cache and require an `account_email` to scope
//! reads to the right cached bundle.

use crate::commands::bridge::RateWindowSnapshot;
use crate::state::AppState;
use chrono::{DateTime, Utc};
use codexbar::core::OpenAIDashboardCacheStore;
use codexbar::cost_reporting_period::{CostReportingPeriod, CostTimeZone};
use codexbar::cost_scanner::{
    CostScanner, CostSummary, TodayUsage, get_daily_cost_history, get_daily_token_history,
};
use codexbar::locale::{self, LocaleKey};
use codexbar::providers::muse::local_usage as muse_local_usage;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::{
    Arc, Mutex, OnceLock,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

mod local_usage_cache;
pub(crate) use local_usage_cache::*;

const LOCAL_USAGE_TTL: Duration = Duration::from_secs(30);

/// A single (date, value) point for cost or credits history charts.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyCostPoint {
    pub date: String,
    pub value: Option<f64>,
    /// Claude requests that only produced a preliminary proxy usage row that
    /// day (upstream 0.60.5 #3688). Omitted when zero or not applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub incomplete_request_count: Option<u32>,
}

/// A single (date, tokens) point for the Tokens chart mode (upstream 0.50.0
/// #2930 — exact local token totals).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyTokenPoint {
    pub date: String,
    pub tokens: u64,
}

/// A single service's usage within a day for the stacked usage breakdown chart.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceUsagePoint {
    pub service: String,
    pub credits_used: f64,
}

/// One day's stacked usage breakdown.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyUsageBreakdown {
    pub day: String,
    pub services: Vec<ServiceUsagePoint>,
    pub total_credits_used: f64,
}

/// Real local usage summary from Codex / Claude log files.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderLocalUsageSummary {
    pub today_cost: Option<f64>,
    /// Always the trailing 30 days (the published PowerToys pipe reads it).
    pub thirty_day_cost: Option<f64>,
    pub thirty_day_tokens: Option<u64>,
    /// Cost over the selected History window (`reporting_period`).
    #[serde(default)]
    pub period_cost: Option<f64>,
    /// Tokens over the selected History window.
    #[serde(default)]
    pub period_tokens: Option<u64>,
    /// Raw reporting period the `period_*` fields cover; empty when unknown.
    #[serde(default)]
    pub reporting_period: String,
    pub latest_tokens: Option<u64>,
    pub top_model: Option<String>,
    pub estimate_note: String,
    pub token_cost_updated_at_ms: i64,
    /// Incomplete Claude requests excluded from the selected-period totals
    /// (`period_cost` / `period_tokens`). Omitted when zero or not applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub incomplete_request_count: Option<u32>,
}

/// One display-only quota-window history row.  Completeness is tracked per
/// metric so a known token subtotal never makes an unknown cost look exact.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaWindowHistoryPoint {
    pub offset: usize,
    pub start: String,
    pub end: String,
    pub total_tokens: Option<u64>,
    pub total_cost_usd: Option<f64>,
    pub tokens_are_complete: bool,
    pub cost_is_complete: bool,
    pub entry_count: usize,
    pub boundaries_are_estimated: bool,
}

/// Provider-scoped quota-window history carried alongside the existing chart
/// data.  It never participates in the live provider snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaWindowHistoryBridge {
    pub provider_id: String,
    pub account_scope: Option<String>,
    pub windows: Vec<QuotaWindowHistoryPoint>,
    pub history_coverage_established: bool,
}

/// Full chart data bundle for one provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderChartData {
    pub provider_id: String,
    pub cost_history: Vec<DailyCostPoint>,
    pub credits_history: Vec<DailyCostPoint>,
    pub usage_breakdown: Vec<DailyUsageBreakdown>,
    pub local_usage: Option<ProviderLocalUsageSummary>,
    /// Daily exact local token totals for the Tokens mode; incomplete
    /// backfill keeps the marker true so the UI can show "Refreshing".
    pub tokens_history: Vec<DailyTokenPoint>,
    pub tokens_incomplete: bool,
    #[serde(default)]
    pub quota_window_history: Option<QuotaWindowHistoryBridge>,
}

#[tauri::command]
pub async fn get_provider_chart_data(
    state: tauri::State<'_, std::sync::Mutex<AppState>>,
    provider_id: String,
    account_email: Option<String>,
) -> Result<ProviderChartData, String> {
    let fallback_provider_id = provider_id.clone();
    let (weekly_window, cached_account_email) =
        current_history_context(&state, &provider_id).unwrap_or((None, None));
    let account_email = account_email.or(cached_account_email);
    let cancel = register_chart_scan(&provider_id);
    tauri::async_runtime::spawn_blocking(move || {
        build_provider_chart_data_with_cancel(
            provider_id,
            account_email,
            Some(cancel),
            weekly_window,
        )
    })
    .await
    .map(Ok)
    .unwrap_or_else(|err| {
        tracing::warn!("Provider chart data worker failed: {}", err);
        Ok(ProviderChartData::empty(fallback_provider_id))
    })
}

#[tauri::command]
pub async fn get_provider_local_usage_summary(
    provider_id: String,
) -> Option<ProviderLocalUsageSummary> {
    let failure_provider_id = provider_id.clone();
    tauri::async_runtime::spawn_blocking(move || load_provider_local_usage_summary(&provider_id))
        .await
        .unwrap_or_else(|err| {
            tracing::warn!("Provider local usage worker failed: {}", err);
            record_local_usage_fetch_failure(&failure_provider_id, CostFetchFailure::Failed);
            None
        })
}

#[cfg(test)]
pub(crate) fn build_provider_chart_data(
    provider_id: String,
    account_email: Option<String>,
) -> ProviderChartData {
    build_provider_chart_data_with_cancel(provider_id, account_email, None, None)
}

fn build_provider_chart_data_with_cancel(
    provider_id: String,
    account_email: Option<String>,
    cancel: Option<Arc<AtomicBool>>,
    weekly_window: Option<RateWindowSnapshot>,
) -> ProviderChartData {
    let live_window = weekly_window
        .as_ref()
        .map(RateWindowSnapshot::to_rate_window);
    let provider_snapshot = codexbar::providers::chart::build_chart_snapshot(
        &provider_id,
        account_email.as_deref(),
        live_window.as_ref(),
        cancel.as_deref(),
    );
    let (cost_history, tokens_history, tokens_incomplete, local_usage) = if let Some(snapshot) =
        provider_snapshot
            .as_ref()
            .filter(|_| provider_id == "claude")
    {
        let cost_history = snapshot
            .daily_cost
            .iter()
            .cloned()
            .map(|(date, value)| {
                let incomplete_request_count = snapshot
                    .daily_incomplete
                    .iter()
                    .find(|(day, _)| *day == date)
                    .map(|(_, count)| *count);
                DailyCostPoint {
                    date,
                    value,
                    incomplete_request_count,
                }
            })
            .collect();
        let tokens_history = snapshot
            .daily_tokens
            .iter()
            .cloned()
            .map(|(date, tokens)| DailyTokenPoint { date, tokens })
            .collect();
        let period = current_reporting_period();
        let local_usage = snapshot.local_summary.as_ref().and_then(|summary| {
            let period_summary =
                period_summary_for(&provider_id, period, summary, cancel.as_deref());
            local_usage_summary_from_cost_summary(
                &provider_id,
                summary,
                period,
                period_summary.as_ref(),
                snapshot.today.as_ref(),
            )
        });
        store_local_usage_summary(&provider_id, local_usage.clone());
        (
            cost_history,
            tokens_history,
            snapshot.tokens_incomplete,
            local_usage,
        )
    } else if provider_id == "muse" {
        if cancel
            .as_ref()
            .is_some_and(|flag| flag.load(Ordering::Relaxed))
        {
            (Vec::new(), Vec::new(), true, None)
        } else {
            let report = muse_local_usage::scan(30, cancel.as_deref());
            let tokens_history = report
                .daily
                .iter()
                .map(|day| DailyTokenPoint {
                    date: day.day.clone(),
                    tokens: day.total_tokens,
                })
                .collect();
            let period = current_reporting_period();
            let period_report = muse_period_report(period, &report, cancel.as_deref());
            let local_usage = muse_local_usage_summary(
                &report,
                &period_report,
                period,
                locale::current_language(),
            );
            (
                Vec::new(),
                tokens_history,
                !report.is_complete(),
                local_usage,
            )
        }
    } else {
        let raw_cost = get_daily_cost_history(&provider_id, 30);
        let cost_history: Vec<DailyCostPoint> = raw_cost
            .into_iter()
            .map(|(date, value)| DailyCostPoint {
                date,
                value,
                incomplete_request_count: None,
            })
            .collect();

        let (raw_tokens, tokens_incomplete) = get_daily_token_history(&provider_id, 30);
        let tokens_history: Vec<DailyTokenPoint> = raw_tokens
            .into_iter()
            .map(|(date, tokens)| DailyTokenPoint { date, tokens })
            .collect();
        let local_usage = if cancel
            .as_deref()
            .is_some_and(|flag| flag.load(Ordering::Relaxed))
        {
            None
        } else {
            load_local_usage_summary_cached(&provider_id, cancel.as_deref())
        };
        (cost_history, tokens_history, tokens_incomplete, local_usage)
    };

    let (credits_history, usage_breakdown) =
        load_openai_dashboard_chart_data(&provider_id, account_email.as_deref());

    let quota_window_history = provider_snapshot
        .as_ref()
        .and_then(|snapshot| snapshot.quota_window_history.as_ref())
        .map(map_quota_window_history);

    ProviderChartData {
        provider_id,
        cost_history,
        credits_history,
        usage_breakdown,
        local_usage,
        tokens_history,
        tokens_incomplete,
        quota_window_history,
    }
}

impl ProviderChartData {
    fn empty(provider_id: String) -> Self {
        Self {
            provider_id,
            cost_history: Vec::new(),
            credits_history: Vec::new(),
            usage_breakdown: Vec::new(),
            local_usage: None,
            tokens_history: Vec::new(),
            tokens_incomplete: false,
            quota_window_history: None,
        }
    }
}

fn current_history_context(
    state: &tauri::State<'_, std::sync::Mutex<AppState>>,
    provider_id: &str,
) -> Option<(Option<RateWindowSnapshot>, Option<String>)> {
    let guard = state.lock().ok()?;
    let snapshot = guard
        .provider_cache
        .iter()
        .find(|snapshot| snapshot.provider_id.eq_ignore_ascii_case(provider_id))?;
    let weekly_window = snapshot.secondary.clone().or_else(|| {
        snapshot
            .primary
            .window_minutes
            .filter(|minutes| *minutes >= 7 * 24 * 60)
            .map(|_| snapshot.primary.clone())
    });
    Some((weekly_window, snapshot.account_email.clone()))
}

fn map_quota_window_history(
    history: &codexbar::providers::chart::QuotaWindowHistorySnapshot,
) -> QuotaWindowHistoryBridge {
    QuotaWindowHistoryBridge {
        provider_id: history.provider_id.clone(),
        account_scope: history.account_scope.clone(),
        windows: history
            .windows
            .iter()
            .map(|window| QuotaWindowHistoryPoint {
                offset: window.offset,
                start: window.start.to_rfc3339(),
                end: window.end.to_rfc3339(),
                total_tokens: window.total_tokens,
                total_cost_usd: window.total_cost_usd,
                tokens_are_complete: window.tokens_are_complete,
                cost_is_complete: window.cost_is_complete,
                entry_count: window.entry_count,
                boundaries_are_estimated: window.boundaries_are_estimated,
            })
            .collect(),
        history_coverage_established: history.history_coverage_established,
    }
}

/// The History window the desktop is configured to report.
fn current_reporting_period() -> CostReportingPeriod {
    codexbar::settings::Settings::load().cost_reporting_period
}

/// The period summary for `provider_id`, reusing the 30-day scan when the
/// selected window is exactly the trailing 30 days.
fn period_summary_for(
    provider_id: &str,
    period: CostReportingPeriod,
    thirty_day: &CostSummary,
    cancel: Option<&AtomicBool>,
) -> Option<CostSummary> {
    if period == CostReportingPeriod::Rolling(30) {
        return Some(thirty_day.clone());
    }
    scan_local_cost(provider_id, period, cancel)
}

fn local_usage_summary_from_cost_summary(
    provider_id: &str,
    summary: &CostSummary,
    period: CostReportingPeriod,
    period_summary: Option<&CostSummary>,
    today: Option<&TodayUsage>,
) -> Option<ProviderLocalUsageSummary> {
    let thirty_tokens = total_tokens(provider_id, summary);
    let has_usage = summary.sessions_count > 0
        || summary.total_cost_usd > 0.0
        || thirty_tokens > 0
        || summary.incomplete_request_count > 0;
    has_usage.then(|| ProviderLocalUsageSummary {
        today_cost: today.and_then(|t| t.cost_usd).and_then(non_zero_f64),
        thirty_day_cost: non_zero_f64(summary.total_cost_usd),
        thirty_day_tokens: non_zero_u64(thirty_tokens),
        period_cost: period_summary.and_then(|s| non_zero_f64(s.total_cost_usd)),
        period_tokens: period_summary.and_then(|s| non_zero_u64(total_tokens(provider_id, s))),
        reporting_period: period.raw(),
        latest_tokens: today.and_then(|t| non_zero_u64(t.tokens)),
        top_model: top_model(provider_id, summary),
        estimate_note: localized_estimate_note(provider_id, locale::current_language()),
        token_cost_updated_at_ms: current_unix_ms(),
        // The note sits under the selected-period totals, so count that window.
        incomplete_request_count: period_summary
            .and_then(|s| non_zero_u32(s.incomplete_request_count)),
    })
}

fn active_chart_scans() -> &'static Mutex<HashMap<String, Arc<AtomicBool>>> {
    static ACTIVE: OnceLock<Mutex<HashMap<String, Arc<AtomicBool>>>> = OnceLock::new();
    ACTIVE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn register_chart_scan(provider_id: &str) -> Arc<AtomicBool> {
    let next = Arc::new(AtomicBool::new(false));
    if let Ok(mut active) = active_chart_scans().lock()
        && let Some(previous) = active.insert(provider_id.to_string(), next.clone())
    {
        previous.store(true, Ordering::Relaxed);
    }
    next
}

fn load_local_usage_summary(
    provider_id: &str,
    cancel: Option<&AtomicBool>,
) -> Option<ProviderLocalUsageSummary> {
    load_local_usage_summary_with_unknown_models(provider_id, cancel).0
}

fn load_local_usage_summary_with_unknown_models(
    provider_id: &str,
    cancel: Option<&AtomicBool>,
) -> (Option<ProviderLocalUsageSummary>, HashSet<String>) {
    let period = current_reporting_period();
    if provider_id == "muse" {
        let summary = if cancel.is_some_and(|flag| flag.load(Ordering::Relaxed)) {
            None
        } else {
            let report = muse_local_usage::scan(30, cancel);
            let period_report = muse_period_report(period, &report, cancel);
            muse_local_usage_summary(&report, &period_report, period, locale::current_language())
        };
        return (summary, HashSet::new());
    }
    let Some(thirty_day) = scan_local_cost(provider_id, CostReportingPeriod::Rolling(30), cancel)
    else {
        return (None, HashSet::new());
    };
    if cancel.is_some_and(|flag| flag.load(Ordering::Relaxed)) {
        return (None, HashSet::new());
    }
    let today =
        scan_local_cost(provider_id, CostReportingPeriod::Rolling(1), cancel).unwrap_or_default();
    let period_summary = period_summary_for(provider_id, period, &thirty_day, cancel);
    let mut unknown_models: HashSet<String> = thirty_day
        .unknown_models
        .union(&today.unknown_models)
        .cloned()
        .collect();
    if let Some(period_summary) = period_summary.as_ref() {
        unknown_models.extend(period_summary.unknown_models.iter().cloned());
    }

    let thirty_day_tokens = total_tokens(provider_id, &thirty_day);
    let latest_tokens = total_tokens(provider_id, &today);
    let has_usage = thirty_day.sessions_count > 0
        || thirty_day.total_cost_usd > 0.0
        || thirty_day_tokens > 0
        || thirty_day.incomplete_request_count > 0;
    if !has_usage {
        return (None, unknown_models);
    }

    let lang = locale::current_language();
    (
        Some(ProviderLocalUsageSummary {
            today_cost: non_zero_f64(today.total_cost_usd),
            thirty_day_cost: non_zero_f64(thirty_day.total_cost_usd),
            thirty_day_tokens: non_zero_u64(thirty_day_tokens),
            period_cost: period_summary
                .as_ref()
                .and_then(|s| non_zero_f64(s.total_cost_usd)),
            period_tokens: period_summary
                .as_ref()
                .and_then(|s| non_zero_u64(total_tokens(provider_id, s))),
            reporting_period: period.raw(),
            latest_tokens: non_zero_u64(latest_tokens),
            top_model: top_model(provider_id, &thirty_day),
            estimate_note: localized_estimate_note(provider_id, lang),
            token_cost_updated_at_ms: current_unix_ms(),
            incomplete_request_count: period_summary
                .as_ref()
                .and_then(|s| non_zero_u32(s.incomplete_request_count)),
        }),
        unknown_models,
    )
}

/// The Muse report for the selected period, reusing the 30-day scan when the
/// selection is exactly the trailing 30 days.
fn muse_period_report(
    period: CostReportingPeriod,
    thirty_day: &muse_local_usage::Report,
    cancel: Option<&AtomicBool>,
) -> muse_local_usage::Report {
    if period == CostReportingPeriod::Rolling(30) {
        return thirty_day.clone();
    }
    muse_local_usage::scan(period.scan_days(Utc::now()), cancel)
}

fn muse_local_usage_summary(
    report: &muse_local_usage::Report,
    period_report: &muse_local_usage::Report,
    period: CostReportingPeriod,
    lang: codexbar::settings::Language,
) -> Option<ProviderLocalUsageSummary> {
    if !report.is_available() || !report.is_complete() {
        return None;
    }
    let total_tokens = report.total_tokens?;
    Some(ProviderLocalUsageSummary {
        today_cost: None,
        thirty_day_cost: None,
        thirty_day_tokens: Some(total_tokens),
        period_cost: None,
        period_tokens: period_report
            .is_complete()
            .then_some(period_report.total_tokens)
            .flatten(),
        reporting_period: period.raw(),
        latest_tokens: report.today_tokens,
        top_model: report.top_model.clone(),
        estimate_note: locale::get_text(lang, LocaleKey::PanelEstimatedFromLocalLogsMuse),
        token_cost_updated_at_ms: current_unix_ms(),
        incomplete_request_count: None,
    })
}

fn localized_estimate_note(provider_id: &str, lang: codexbar::settings::Language) -> String {
    match provider_id {
        "claude" => locale::get_text(lang, LocaleKey::PanelEstimatedFromLocalLogsClaude),
        _ => locale::get_text(lang, LocaleKey::PanelEstimatedFromLocalLogs),
    }
}

fn scan_local_cost(
    provider_id: &str,
    period: CostReportingPeriod,
    cancel: Option<&AtomicBool>,
) -> Option<CostSummary> {
    let scanner = CostScanner::for_period(period);
    match provider_id {
        "codex" => Some(scanner.scan_codex_with_cancel(cancel)),
        "claude" => Some(scanner.scan_claude_with_cancel(cancel)),
        "pi" => Some(scanner.scan_pi_with_cancel(cancel)),
        "opencodego" => Some(scanner.scan_opencodego_with_cancel(cancel)),
        _ => None,
    }
}

fn total_tokens(provider_id: &str, summary: &CostSummary) -> u64 {
    summary.total_tokens_for_provider(provider_id)
}

fn non_zero_f64(value: f64) -> Option<f64> {
    (value > 0.0).then_some(value)
}

fn non_zero_u64(value: u64) -> Option<u64> {
    (value > 0).then_some(value)
}

fn non_zero_u32(value: u32) -> Option<u32> {
    (value > 0).then_some(value)
}

fn top_model(provider_id: &str, summary: &CostSummary) -> Option<String> {
    summary
        .by_model_tokens
        .iter()
        .max_by_key(|(_, counts)| counts.total_for_provider(provider_id))
        .map(|(model, _)| model.clone())
        .or_else(|| {
            summary
                .by_model
                .iter()
                .max_by(|a, b| a.1.total_cmp(b.1))
                .map(|(model, _)| model.clone())
        })
}

fn load_openai_dashboard_chart_data(
    provider_id: &str,
    account_email: Option<&str>,
) -> (Vec<DailyCostPoint>, Vec<DailyUsageBreakdown>) {
    if provider_id != "codex" && provider_id != "openai" {
        return (Vec::new(), Vec::new());
    }

    let Some(account_email) = account_email else {
        return (Vec::new(), Vec::new());
    };

    let Some(cache) = OpenAIDashboardCacheStore::load() else {
        return (Vec::new(), Vec::new());
    };

    if !cache.account_email.eq_ignore_ascii_case(account_email) {
        return (Vec::new(), Vec::new());
    }

    let snapshot = &cache.snapshot;

    let breakdown_source = if !snapshot.daily_breakdown.is_empty() {
        &snapshot.daily_breakdown
    } else if !snapshot.usage_breakdown.is_empty() {
        &snapshot.usage_breakdown
    } else {
        return (Vec::new(), Vec::new());
    };

    let credits_history: Vec<DailyCostPoint> = breakdown_source
        .iter()
        .map(|d| DailyCostPoint {
            date: d.day.clone(),
            value: Some(d.total_credits_used),
            incomplete_request_count: None,
        })
        .collect();

    let usage_breakdown: Vec<DailyUsageBreakdown> = snapshot
        .usage_breakdown
        .iter()
        .map(|d| DailyUsageBreakdown {
            day: d.day.clone(),
            services: d
                .services
                .iter()
                .map(|s| ServiceUsagePoint {
                    service: s.service.clone(),
                    credits_used: s.credits_used,
                })
                .collect(),
            total_credits_used: d.total_credits_used,
        })
        .collect();

    (credits_history, usage_breakdown)
}

#[cfg(test)]
pub(crate) fn load_openai_dashboard_chart_data_for_test(
    provider_id: &str,
    account_email: Option<&str>,
) -> (Vec<DailyCostPoint>, Vec<DailyUsageBreakdown>) {
    load_openai_dashboard_chart_data(provider_id, account_email)
}

#[cfg(test)]
mod tests;
