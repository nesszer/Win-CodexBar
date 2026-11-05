//! Projection from the dashboard snapshot contract into bounded metric data.

use std::collections::BTreeSet;

use chrono::{DateTime, Utc};

use crate::cli::serve::collection::SnapshotCollection;
use crate::core::{ProviderId, RateWindow, RateWindowCadence};

#[derive(Debug, Clone)]
pub(crate) struct MetricsSnapshot {
    pub(in crate::cli::serve::metrics) schema_version: u32,
    pub(in crate::cli::serve::metrics) generated_at: DateTime<Utc>,
    pub(in crate::cli::serve::metrics) stale_after_seconds: u32,
    pub(in crate::cli::serve::metrics) refresh_interval_seconds: u32,
    pub(in crate::cli::serve::metrics) providers: Vec<ProviderMetrics>,
}

#[derive(Debug, Clone)]
pub(in crate::cli::serve::metrics) struct ProviderMetrics {
    pub(in crate::cli::serve::metrics) provider: &'static str,
    pub(in crate::cli::serve::metrics) up: bool,
    pub(in crate::cli::serve::metrics) updated_at: Option<DateTime<Utc>>,
    pub(in crate::cli::serve::metrics) codex_quota: Option<CodexQuotaMetrics>,
    pub(in crate::cli::serve::metrics) reset_credits_available: Option<i64>,
    pub(in crate::cli::serve::metrics) reset_credits_next_expiry: Option<DateTime<Utc>>,
    pub(in crate::cli::serve::metrics) subscription_start: Option<DateTime<Utc>>,
    pub(in crate::cli::serve::metrics) subscription_expiry: Option<DateTime<Utc>>,
    pub(in crate::cli::serve::metrics) cost_today_usd: Option<f64>,
    pub(in crate::cli::serve::metrics) cost_last_30_days_usd: Option<f64>,
}

#[derive(Debug, Clone)]
pub(in crate::cli::serve::metrics) struct CodexQuotaMetrics {
    pub(in crate::cli::serve::metrics) session: Option<QuotaMetric>,
    pub(in crate::cli::serve::metrics) weekly: Option<QuotaMetric>,
    pub(in crate::cli::serve::metrics) monthly: Option<QuotaMetric>,
    pub(in crate::cli::serve::metrics) code_review: Option<QuotaMetric>,
}

#[derive(Debug, Clone)]
pub(in crate::cli::serve::metrics) struct QuotaMetric {
    pub(in crate::cli::serve::metrics) used_ratio: f64,
    pub(in crate::cli::serve::metrics) reset_at: Option<DateTime<Utc>>,
}

impl MetricsSnapshot {
    pub(crate) fn from_collection(input: &SnapshotCollection) -> Self {
        let enabled = input
            .enabled
            .iter()
            .filter_map(|name| ProviderId::from_cli_name(name))
            .map(|id| id.cli_name())
            .collect::<BTreeSet<_>>();

        let providers = ProviderId::all()
            .iter()
            .copied()
            .filter(|id| enabled.contains(id.cli_name()))
            .map(|id| {
                let provider = input
                    .providers
                    .iter()
                    .find(|provider| ProviderId::from_cli_name(&provider.id) == Some(id));
                let cost = (id == ProviderId::Codex).then(|| {
                    input.costs.iter().find_map(|(name, cost)| {
                        (ProviderId::from_cli_name(name) == Some(ProviderId::Codex)).then_some(cost)
                    })
                });
                let cost = cost.flatten();
                let cost_today_usd = cost.and_then(|value| finite_value(value.today_usd));
                let cost_last_30_days_usd =
                    cost.and_then(|value| finite_value(value.last_30_days_usd));
                match provider.map(|provider| &provider.fetch) {
                    Some(Ok(result)) => ProviderMetrics {
                        provider: id.cli_name(),
                        up: true,
                        updated_at: Some(result.usage.updated_at),
                        codex_quota: (id == ProviderId::Codex).then(|| CodexQuotaMetrics {
                            session: quota_metric_for_cadence(
                                &result.usage.primary,
                                RateWindowCadence::Session,
                            ),
                            weekly: result.usage.secondary.as_ref().and_then(|window| {
                                quota_metric_for_cadence(window, RateWindowCadence::Weekly)
                            }),
                            monthly: result.usage.tertiary.as_ref().and_then(|window| {
                                quota_metric_for_cadence(window, RateWindowCadence::Monthly)
                            }),
                            code_review: result.usage.code_review_window().and_then(quota_metric),
                        }),
                        reset_credits_available: (id == ProviderId::Codex).then(|| {
                            result
                                .reset_credits
                                .map_or(-1, |credits| i64::from(credits.available_count))
                        }),
                        reset_credits_next_expiry: (id == ProviderId::Codex)
                            .then_some(result.reset_credits)
                            .flatten()
                            .filter(|credits| credits.available_count > 0)
                            .and_then(|credits| credits.next_expires_at)
                            .filter(|expiry| *expiry > input.generated_at),
                        // 只使用 Codex 明确提供的订阅日期，不以续费或额度重置时间代替。
                        subscription_start: (id == ProviderId::Codex)
                            .then_some(result.usage.subscription.as_ref())
                            .flatten()
                            .and_then(|subscription| subscription.starts_at),
                        subscription_expiry: (id == ProviderId::Codex)
                            .then_some(result.usage.subscription.as_ref())
                            .flatten()
                            .and_then(|subscription| subscription.expires_at),
                        cost_today_usd,
                        cost_last_30_days_usd,
                    },
                    Some(Err(_)) | None => ProviderMetrics {
                        provider: id.cli_name(),
                        up: false,
                        updated_at: None,
                        codex_quota: None,
                        reset_credits_available: None,
                        reset_credits_next_expiry: None,
                        subscription_start: None,
                        subscription_expiry: None,
                        cost_today_usd,
                        cost_last_30_days_usd,
                    },
                }
            })
            .collect();

        Self {
            schema_version: 1,
            generated_at: input.generated_at,
            stale_after_seconds: input.refresh_seconds.saturating_mul(3).max(180),
            refresh_interval_seconds: input.refresh_seconds,
            providers,
        }
    }
}

fn finite_value(value: Option<f64>) -> Option<f64> {
    value.filter(|value| value.is_finite())
}

fn quota_metric(window: &RateWindow) -> Option<QuotaMetric> {
    if !window.usage_known()
        || window.is_informational
        || !window.used_percent.is_finite()
        || !(0.0..=100.0).contains(&window.used_percent)
    {
        return None;
    }
    Some(QuotaMetric {
        used_ratio: window.used_percent / 100.0,
        reset_at: window.resets_at,
    })
}

fn quota_metric_for_cadence(
    window: &RateWindow,
    expected: RateWindowCadence,
) -> Option<QuotaMetric> {
    let cadence = window
        .window_minutes
        .map(RateWindowCadence::from_minutes)
        .unwrap_or(RateWindowCadence::Unknown);
    (cadence == expected)
        .then(|| quota_metric(window))
        .flatten()
}
