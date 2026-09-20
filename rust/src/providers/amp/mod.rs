//! Amp provider implementation
//!
//! Amp is Sourcegraph's AI coding assistant
//! Fetches usage data from Amp's local config or API

use async_trait::async_trait;
use chrono::Utc;
use serde_json::Value;
use std::path::PathBuf;

mod cli;
mod subscription;

use subscription::usage_snapshot_from_amp_display_text;

use crate::core::{
    FetchContext, Provider, ProviderError, ProviderFetchResult, ProviderId, ProviderMetadata,
    RateWindow, SourceMode, UsageSnapshot,
};

/// Amp provider (Sourcegraph)
pub struct AmpProvider {
    metadata: ProviderMetadata,
}

impl AmpProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: ProviderId::Amp,
                display_name: "Amp",
                session_label: "Usage",
                weekly_label: "Monthly",
                supports_opus: false,
                supports_credits: true,
                default_enabled: false,
                is_primary: false,
                dashboard_url: Some("https://ampcode.com/settings/usage"),
                status_page_url: Some("https://sourcegraphstatus.com"),
                tertiary_label_key: None,
            },
        }
    }

    /// Get Amp config directory
    fn get_amp_config_path() -> Option<PathBuf> {
        #[cfg(target_os = "windows")]
        {
            dirs::config_dir().map(|p| p.join("amp"))
        }
        #[cfg(not(target_os = "windows"))]
        {
            dirs::home_dir().map(|p| p.join(".amp"))
        }
    }

    /// Get Sourcegraph/Cody config directory (Amp might use this)
    fn get_cody_config_path() -> Option<PathBuf> {
        #[cfg(target_os = "windows")]
        {
            dirs::config_dir().map(|p| p.join("sourcegraph-cody"))
        }
        #[cfg(not(target_os = "windows"))]
        {
            dirs::home_dir().map(|p| p.join(".sourcegraph"))
        }
    }

    /// Read Amp/Sourcegraph access token
    async fn read_access_token(&self, ctx: &FetchContext) -> Result<String, ProviderError> {
        if let Some(token) = access_token_from_context(ctx) {
            return Ok(token);
        }

        if let Some(token) = access_token_from_environment() {
            return Ok(token);
        }

        if let Some(token) = Self::read_local_config_token().await {
            return Ok(token);
        }

        Err(ProviderError::AuthRequired)
    }

    async fn read_local_config_token() -> Option<String> {
        let amp_token = read_access_token_config(Self::get_amp_config_path()).await;
        if amp_token.is_some() {
            return amp_token;
        }

        read_access_token_config(Self::get_cody_config_path()).await
    }

    /// Fetch usage via Sourcegraph API
    async fn fetch_via_web(&self, ctx: &FetchContext) -> Result<UsageSnapshot, ProviderError> {
        let token = self.read_access_token(ctx).await?;

        let client = crate::core::credentialed_http_client_builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| ProviderError::Other(e.to_string()))?;

        // Sourcegraph Cody usage API
        let resp = client
            .get("https://sourcegraph.com/.api/cody/current-user/usage")
            .header("Authorization", format!("token {}", token))
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(ProviderError::AuthRequired);
        }

        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| ProviderError::Parse(e.to_string()))?;

        self.parse_usage_response_at(&json, Utc::now())
    }

    fn parse_usage_response_at(
        &self,
        json: &serde_json::Value,
        now: chrono::DateTime<Utc>,
    ) -> Result<UsageSnapshot, ProviderError> {
        if let Some(display_text) = json
            .get("displayText")
            .or_else(|| json.get("display_text"))
            .or_else(|| {
                json.get("result")
                    .and_then(|result| result.get("displayText"))
            })
            .and_then(Value::as_str)
            && let Some(usage) = usage_snapshot_from_amp_display_text(display_text, now)
        {
            return Ok(usage);
        }

        // Parse Sourcegraph/Amp usage response
        let used = json
            .get("completionsUsed")
            .or_else(|| json.get("used"))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);

        let limit = json
            .get("completionsLimit")
            .or_else(|| json.get("limit"))
            .and_then(|v| v.as_f64())
            .unwrap_or(500.0);

        let used_percent = if limit > 0.0 {
            (used / limit) * 100.0
        } else {
            0.0
        };

        let plan = json
            .get("plan")
            .or_else(|| json.get("tier"))
            .and_then(|v| v.as_str())
            .unwrap_or("Pro");

        let reset_time = json
            .get("resetAt")
            .or_else(|| json.get("periodEnd"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let primary_window = RateWindow::with_details(used_percent, None, None, reset_time);
        let usage = UsageSnapshot::new(primary_window).with_login_method(plan);

        Ok(usage)
    }
}

fn access_token_from_context(ctx: &FetchContext) -> Option<String> {
    ctx.api_key
        .as_deref()
        .filter(|api_key| !api_key.is_empty())
        .map(str::to_string)
}

fn access_token_from_environment() -> Option<String> {
    std::env::var("SRC_ACCESS_TOKEN")
        .ok()
        .or_else(|| std::env::var("AMP_ACCESS_TOKEN").ok())
}

async fn read_access_token_config(config_dir: Option<PathBuf>) -> Option<String> {
    let config_file = config_dir?.join("config.json");
    if !config_file.exists() {
        return None;
    }

    let content = tokio::fs::read_to_string(config_file).await.ok()?;
    let json = serde_json::from_str::<serde_json::Value>(&content).ok()?;
    json.get("accessToken")
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

impl Default for AmpProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for AmpProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Amp
    }

    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        tracing::debug!("Fetching Amp usage");

        match ctx.source_mode {
            SourceMode::Auto => {
                if let Ok(usage) = cli::fetch_usage().await {
                    return Ok(ProviderFetchResult::new(usage, "cli"));
                }
                let usage = self.fetch_via_web(ctx).await?;
                Ok(ProviderFetchResult::new(usage, "web"))
            }
            SourceMode::Web => {
                let usage = self.fetch_via_web(ctx).await?;
                Ok(ProviderFetchResult::new(usage, "web"))
            }
            SourceMode::Cli => {
                let usage = cli::fetch_usage().await?;
                Ok(ProviderFetchResult::new(usage, "cli"))
            }
            SourceMode::OAuth => Err(ProviderError::UnsupportedSource(SourceMode::OAuth)),
        }
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::Web, SourceMode::Cli]
    }

    fn supports_web(&self) -> bool {
        true
    }

    fn supports_cli(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::subscription::{
        AMP_MONTHLY_WINDOW_MINUTES, parse_amp_free_percent_remaining, parse_amp_subscription_usage,
    };
    use super::*;
    use chrono::{TimeZone, Utc};

    #[test]
    fn dashboard_points_to_current_usage_page() {
        assert_eq!(
            AmpProvider::new().metadata().dashboard_url,
            Some("https://ampcode.com/settings/usage")
        );
    }

    #[test]
    fn parses_amp_free_percent_remaining_today() {
        let text = "Signed in as user@example.com\nAmp Free: 72% remaining today\n";
        assert_eq!(parse_amp_free_percent_remaining(text), Some(28.0));
    }

    #[test]
    fn parses_amp_free_percent_resets_daily() {
        let text = "Amp Free: 100% remaining (resets daily)";
        assert_eq!(parse_amp_free_percent_remaining(text), Some(0.0));
    }

    #[test]
    fn parses_bold_amp_free_and_current_subscription_labels() {
        let now = Utc.with_ymd_and_hms(2026, 8, 24, 12, 0, 0).unwrap();
        assert_eq!(
            parse_amp_free_percent_remaining("**Amp Free:** 0% remaining today (resets daily)"),
            Some(100.0)
        );

        let sub = parse_amp_subscription_usage(
            "**Amp Megawatt Subscription:** 68% other usage and 97% orb usage remaining - resets upon renewal in 5 days",
            now,
        )
        .expect("bold subscription");
        assert_eq!(sub.plan, "Megawatt");
        assert!((sub.other_used_percent() - 32.0).abs() < f64::EPSILON);
        assert!((sub.orb_used_percent().unwrap() - 3.0).abs() < f64::EPSILON);
        assert_eq!(sub.resets_at(), Some(now + chrono::Duration::days(5)));
    }

    #[test]
    fn ignores_dollar_remaining_form() {
        let text = "Amp Free: $4.20 / $10 remaining (replenishes +$1 / hour)";
        assert_eq!(parse_amp_free_percent_remaining(text), None);
    }

    #[test]
    fn returns_none_when_amp_free_missing() {
        assert_eq!(
            parse_amp_free_percent_remaining("Individual credits: $3 remaining"),
            None
        );
    }

    #[test]
    fn parses_megawatt_subscription_dual_windows() {
        let now = Utc.with_ymd_and_hms(2026, 7, 1, 12, 0, 0).unwrap();
        let text = "Signed in as user@example.com (Acme)\n\
Subscription Megawatt: 42% other usage and 88% orb usage remaining - resets upon renewal in 12 days\n";
        let sub = parse_amp_subscription_usage(text, now).expect("subscription");
        assert_eq!(sub.plan, "Megawatt");
        assert!((sub.other_used_percent() - 58.0).abs() < f64::EPSILON);
        assert!((sub.orb_used_percent().unwrap() - 12.0).abs() < f64::EPSILON);
        assert_eq!(sub.reset_description, "renews in 12 days");
        assert_eq!(sub.resets_at(), Some(now + chrono::Duration::days(12)));

        let snapshot = usage_snapshot_from_amp_display_text(text, now).expect("snapshot");
        assert!((snapshot.primary.used_percent - 58.0).abs() < f64::EPSILON);
        assert_eq!(
            snapshot.primary.window_minutes,
            RateWindow::monthly_window_minutes(snapshot.primary.resets_at)
                .or(Some(AMP_MONTHLY_WINDOW_MINUTES))
        );
        assert_eq!(
            snapshot.primary.reset_description.as_deref(),
            Some("renews in 12 days")
        );
        let secondary = snapshot.secondary.expect("orb secondary");
        assert!((secondary.used_percent - 12.0).abs() < f64::EPSILON);
        assert_eq!(
            secondary.window_minutes,
            RateWindow::monthly_window_minutes(secondary.resets_at)
                .or(Some(AMP_MONTHLY_WINDOW_MINUTES))
        );
        assert_eq!(snapshot.login_method.as_deref(), Some("Megawatt"));
    }

    #[test]
    fn megawatt_one_day_renewal_wording() {
        let now = Utc.with_ymd_and_hms(2026, 7, 1, 0, 0, 0).unwrap();
        let text = "Subscription Megawatt: 0% other usage and 100% orb usage remaining - resets upon renewal in 1 day";
        let sub = parse_amp_subscription_usage(text, now).unwrap();
        assert_eq!(sub.reset_description, "renews in 1 day");
        assert!((sub.other_used_percent() - 100.0).abs() < f64::EPSILON);
        assert!((sub.orb_used_percent().unwrap() - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn gigawatt_monthly_renewal_advances_calendar_months() {
        // Upstream 0.49.6 #2601: monthly renewals (Gigawatt) use calendar
        // months, not 30-day buckets.
        let now = Utc.with_ymd_and_hms(2026, 8, 17, 12, 0, 0).unwrap();
        let text = "Subscription Gigawatt: 10% other usage and 95% orb usage remaining - resets upon renewal in 2 months";
        let sub = parse_amp_subscription_usage(text, now).expect("subscription");
        assert_eq!(sub.plan, "Gigawatt");
        assert_eq!(sub.reset_description, "renews in 2 months");
        assert_eq!(
            sub.resets_at(),
            Some(Utc.with_ymd_and_hms(2026, 10, 17, 12, 0, 0).unwrap())
        );
    }

    #[test]
    fn free_tier_resets_at_8pm_new_york() {
        // Upstream 0.49.6 #2601: Amp Free resets at 8:00 PM America/New_York.
        // 2026-08-17 18:00 UTC = 14:00 EDT → same-day 20:00 EDT = 00:00 UTC Aug 18.
        let now = Utc.with_ymd_and_hms(2026, 8, 17, 18, 0, 0).unwrap();
        let snapshot =
            usage_snapshot_from_amp_display_text("Amp Free: 72% remaining (resets daily)", now)
                .expect("snapshot");
        assert_eq!(
            snapshot.primary.resets_at,
            Some(Utc.with_ymd_and_hms(2026, 8, 18, 0, 0, 0).unwrap())
        );

        // 2026-08-18 00:30 UTC = 20:30 EDT Aug 17 (after the boundary) → the
        // next reset is Aug 18 20:00 EDT = Aug 19 00:00 UTC.
        let later = Utc.with_ymd_and_hms(2026, 8, 18, 0, 30, 0).unwrap();
        let snapshot =
            usage_snapshot_from_amp_display_text("Amp Free: 72% remaining (resets daily)", later)
                .expect("snapshot");
        assert_eq!(
            snapshot.primary.resets_at,
            Some(Utc.with_ymd_and_hms(2026, 8, 19, 0, 0, 0).unwrap())
        );
    }

    #[test]
    fn free_path_still_builds_snapshot() {
        let now = Utc.with_ymd_and_hms(2026, 7, 1, 0, 0, 0).unwrap();
        let text = "Amp Free: 72% remaining today";
        let snapshot = usage_snapshot_from_amp_display_text(text, now).unwrap();
        assert!((snapshot.primary.used_percent - 28.0).abs() < f64::EPSILON);
        assert!(snapshot.secondary.is_none());
        assert_eq!(snapshot.login_method.as_deref(), Some("Amp Free"));
    }

    #[test]
    fn provider_cli_boundary_projects_tier_output_into_usage_snapshot() {
        let now = Utc.with_ymd_and_hms(2026, 9, 16, 12, 0, 0).unwrap();
        let text = "Amp Example Tier: agent usage $18.57 of $20 remaining, \
orb usage 732.8h of 750h a1.small orb hours remaining - \
period 2026-09-13 to 2026-10-13, resets upon renewal in 27 days";

        let usage = super::cli::usage_from_amp_cli_output(text, now).expect("tier CLI output");
        assert_eq!(usage.primary_label.as_deref(), Some("Agent usage"));
        assert_eq!(usage.secondary_label.as_deref(), Some("Orb usage"));
        assert!((usage.primary.used_percent - 7.15).abs() < 0.0001);
        assert!((usage.secondary.expect("orb").used_percent - 2.2933333333).abs() < 0.0001);
    }
}

#[cfg(test)]
mod current_subscription_tests {
    use super::subscription::parse_amp_subscription_usage;
    use super::*;
    use chrono::{TimeZone, Utc};
    #[test]
    fn parses_current_amp_subscription_line_format() {
        let now = Utc.with_ymd_and_hms(2026, 8, 18, 12, 0, 0).unwrap();
        let text = "Signed in as user@example.com\nAmp Megawatt Subscription: 100% other usage and 100% orb usage remaining - resets upon renewal in 1 month\n";
        let sub = parse_amp_subscription_usage(text, now).expect("subscription");

        assert_eq!(sub.plan, "Megawatt");
        assert!((sub.other_used_percent() - 0.0).abs() < f64::EPSILON);
        assert!((sub.orb_used_percent().unwrap() - 0.0).abs() < f64::EPSILON);
        assert_eq!(
            sub.resets_at(),
            Some(Utc.with_ymd_and_hms(2026, 9, 18, 12, 0, 0).unwrap())
        );
        assert_eq!(sub.reset_description, "renews in 1 month");
    }

    #[test]
    fn parses_tier_allowances_from_exact_balances_and_period() {
        let now = Utc.with_ymd_and_hms(2026, 9, 16, 12, 0, 0).unwrap();
        let text = "Amp Megawatt Tier: agent usage $18.57 of $20 remaining (93%), \
orb usage 732.8h of 750h a1.small orb hours remaining (98%) - \
period 2026-09-13 to 2026-10-13, resets upon renewal in 27 days";

        let sub = parse_amp_subscription_usage(text, now).expect("tier");
        assert_eq!(sub.plan, "Megawatt");
        assert!((sub.other_used_percent() - 7.15).abs() < 0.0001);
        assert!((sub.orb_used_percent().unwrap() - 2.2933333333).abs() < 0.0001);
        assert_eq!(sub.agent_remaining(), Some(18.57));
        assert_eq!(sub.agent_limit(), Some(20.0));
        assert_eq!(sub.orb_hours_remaining(), Some(732.8));
        assert_eq!(sub.orb_hours_limit(), Some(750.0));
        assert_eq!(
            sub.resets_at(),
            Some(Utc.with_ymd_and_hms(2026, 10, 13, 0, 0, 0).unwrap())
        );

        let snapshot = usage_snapshot_from_amp_display_text(text, now).expect("snapshot");
        assert!((snapshot.primary.used_percent - 7.15).abs() < 0.0001);
        assert_eq!(snapshot.primary.window_minutes, Some(30 * 24 * 60));
        assert_eq!(snapshot.primary_label.as_deref(), Some("Agent usage"));
        let secondary = snapshot.secondary.expect("orb");
        assert!((secondary.used_percent - 2.2933333333).abs() < 0.0001);
        assert_eq!(snapshot.secondary_label.as_deref(), Some("Orb usage"));
    }

    #[test]
    fn invalid_tier_period_does_not_invent_reset_window() {
        let now = Utc.with_ymd_and_hms(2026, 9, 16, 12, 0, 0).unwrap();
        let text = "Amp Example Tier: agent usage $18 of $20 remaining - \
period 2026-02-30 to 2026-03-30, resets upon renewal in 27 days";
        let sub = parse_amp_subscription_usage(text, now).expect("tier");
        assert!(sub.period_start().is_none());
        assert!(sub.resets_at().is_none());

        let snapshot = usage_snapshot_from_amp_display_text(text, now).expect("snapshot");
        assert_eq!(snapshot.primary.used_percent, 10.0);
        assert!(snapshot.primary.window_minutes.is_none());
        assert!(snapshot.primary.resets_at.is_none());
        assert_eq!(
            snapshot.primary.reset_description.as_deref(),
            Some("renews in 27 days · $18.00/$20.00 remaining")
        );
    }

    #[test]
    fn tier_without_orb_keeps_agent_lane() {
        let now = Utc.with_ymd_and_hms(2026, 9, 16, 12, 0, 0).unwrap();
        let text = "Amp Example Tier: agent usage $3 of $20 remaining - \
period 2026-09-13 to 2026-10-13, resets upon renewal in 27 days";
        let snapshot = usage_snapshot_from_amp_display_text(text, now).expect("snapshot");
        assert!(snapshot.secondary.is_none());
        assert_eq!(snapshot.primary.used_percent, 85.0);
    }
}
