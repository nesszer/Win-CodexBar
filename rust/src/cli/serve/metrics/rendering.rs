//! Prometheus response projection and freshness rendering.

use chrono::{DateTime, Utc};

use super::encoding::MetricsWriter;
use super::snapshot::{MetricsSnapshot, QuotaMetric};
use super::{CONTENT_TYPE, MetricsRenderError};

pub(in crate::cli::serve) fn metrics_response(snapshot: Option<&MetricsSnapshot>) -> String {
    let (status, body) = match snapshot {
        Some(snapshot) => match render(snapshot) {
            Ok(body) => (200, body),
            Err(_) => {
                tracing::warn!("Prometheus rendering failed because metric series were duplicated");
                (500, render_unavailable())
            }
        },
        None => (200, render_unavailable()),
    };
    super::super::http_response(status, CONTENT_TYPE, body, &[("Cache-Control", "no-store")])
}

fn render(snapshot: &MetricsSnapshot) -> Result<String, MetricsRenderError> {
    render_at(snapshot, Utc::now())
}

pub(in crate::cli::serve::metrics) fn render_at(
    snapshot: &MetricsSnapshot,
    now: DateTime<Utc>,
) -> Result<String, MetricsRenderError> {
    let mut writer = MetricsWriter::new();
    let snapshot_age = age_seconds(now, snapshot.generated_at);
    writer.sample("codexbar_up", &[], 1)?;
    writer.sample(
        "codexbar_snapshot_schema_version",
        &[],
        snapshot.schema_version,
    )?;
    writer.sample(
        "codexbar_snapshot_generated_timestamp_seconds",
        &[],
        snapshot.generated_at.timestamp(),
    )?;
    writer.sample("codexbar_snapshot_age_seconds", &[], snapshot_age)?;
    writer.sample(
        "codexbar_snapshot_stale_after_seconds",
        &[],
        snapshot.stale_after_seconds,
    )?;
    writer.sample(
        "codexbar_snapshot_stale",
        &[],
        bool_value(snapshot_age > i64::from(snapshot.stale_after_seconds)),
    )?;
    writer.sample(
        "codexbar_refresh_interval_seconds",
        &[],
        snapshot.refresh_interval_seconds,
    )?;

    for provider in &snapshot.providers {
        let provider_labels = [("provider", provider.provider)];
        writer.sample(
            "codexbar_provider_up",
            &provider_labels,
            bool_value(provider.up),
        )?;
        if provider.up
            && let Some(updated_at) = provider.updated_at
        {
            writer.sample(
                "codexbar_provider_updated_timestamp_seconds",
                &provider_labels,
                updated_at.timestamp(),
            )?;
            writer.sample(
                "codexbar_provider_data_age_seconds",
                &provider_labels,
                age_seconds(now, updated_at),
            )?;
        }
        if let Some(quota) = &provider.codex_quota {
            render_quota(
                &mut writer,
                &provider_labels,
                quota.session.as_ref(),
                QuotaMetricNames::SESSION,
            )?;
            render_quota(
                &mut writer,
                &provider_labels,
                quota.weekly.as_ref(),
                QuotaMetricNames::WEEKLY,
            )?;
            render_quota(
                &mut writer,
                &provider_labels,
                quota.monthly.as_ref(),
                QuotaMetricNames::MONTHLY,
            )?;
            render_quota(
                &mut writer,
                &provider_labels,
                quota.code_review.as_ref(),
                QuotaMetricNames::CODE_REVIEW,
            )?;
        }
        if let Some(count) = provider.reset_credits_available {
            writer.sample("codexbar_reset_credits_available", &provider_labels, count)?;
        }
        if let Some(expiry) = provider
            .reset_credits_next_expiry
            .filter(|expiry| *expiry > now)
        {
            writer.sample(
                "codexbar_reset_credits_next_expiry_timestamp_seconds",
                &provider_labels,
                expiry.timestamp(),
            )?;
        }
        if let Some(value) = provider.cost_today_usd {
            writer.sample_f64("codexbar_cost_today_usd", &provider_labels, value)?;
        }
        if let Some(value) = provider.cost_last_30_days_usd {
            writer.sample_f64("codexbar_cost_last_30_days_usd", &provider_labels, value)?;
        }
    }

    Ok(writer.finish())
}

fn render_unavailable() -> String {
    let mut writer = MetricsWriter::new();
    writer
        .sample("codexbar_up", &[], 0)
        .expect("the exporter health series is unique");
    writer.finish()
}

#[derive(Clone, Copy)]
struct QuotaMetricNames {
    used: &'static str,
    remaining: &'static str,
    reset: &'static str,
}

impl QuotaMetricNames {
    const SESSION: Self = Self {
        used: "codexbar_quota_session_used_ratio",
        remaining: "codexbar_quota_session_remaining_ratio",
        reset: "codexbar_quota_session_reset_timestamp_seconds",
    };
    const WEEKLY: Self = Self {
        used: "codexbar_quota_weekly_used_ratio",
        remaining: "codexbar_quota_weekly_remaining_ratio",
        reset: "codexbar_quota_weekly_reset_timestamp_seconds",
    };
    const MONTHLY: Self = Self {
        used: "codexbar_quota_monthly_used_ratio",
        remaining: "codexbar_quota_monthly_remaining_ratio",
        reset: "codexbar_quota_monthly_reset_timestamp_seconds",
    };
    const CODE_REVIEW: Self = Self {
        used: "codexbar_quota_code_review_used_ratio",
        remaining: "codexbar_quota_code_review_remaining_ratio",
        reset: "codexbar_quota_code_review_reset_timestamp_seconds",
    };
}

fn render_quota(
    writer: &mut MetricsWriter,
    labels: &[(&str, &str)],
    quota: Option<&QuotaMetric>,
    names: QuotaMetricNames,
) -> Result<(), MetricsRenderError> {
    if let Some(quota) = quota {
        writer.sample_f64(names.used, labels, quota.used_ratio)?;
        writer.sample_f64(names.remaining, labels, 1.0 - quota.used_ratio)?;
        if let Some(reset_at) = quota.reset_at {
            writer.sample(names.reset, labels, reset_at.timestamp())?;
        }
    }
    Ok(())
}

fn bool_value(value: bool) -> u8 {
    u8::from(value)
}

fn age_seconds(now: DateTime<Utc>, updated_at: DateTime<Utc>) -> i64 {
    (now - updated_at).num_seconds().max(0)
}
