//! Codex (OpenAI/ChatGPT) provider implementation
//!
//! Fetches usage data from ChatGPT's backend API using OAuth credentials
//! stored by the Codex CLI in ~/.codex/auth.json

mod api;
mod pat;
pub mod reset_observations;
mod weekly_reset;

use async_trait::async_trait;
#[cfg(windows)]
use std::os::windows::process::CommandExt;

use crate::core::{
    FetchContext, Provider, ProviderDisplayDetail, ProviderError, ProviderFetchResult, ProviderId,
    ProviderMetadata, ResetCreditsObservation, SourceMode,
};

pub use api::CodexApi;

/// Limit reset detection (upstream `UsageStore+LimitResetCelebration.swift`):
/// a weekly reset needs a known boundary that advanced, and samples without an
/// owner key are not tracked.
pub(crate) const LIMIT_RESET_POLICY: crate::core::ProviderLimitResetPolicy =
    crate::core::ProviderLimitResetPolicy {
        supported: true,
        weekly: crate::core::WeeklyResetRule::AdvancedBoundary {
            reset_threshold_percent: 5.0,
            requires_previous_boundary: true,
        },
        missing_identity: crate::core::MissingIdentityRule::Skip,
    };

/// Codex provider for fetching AI usage limits
pub struct CodexProvider {
    metadata: ProviderMetadata,
    api: CodexApi,
}

impl CodexProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: ProviderId::Codex,
                display_name: "Codex",
                session_label: "Session",
                weekly_label: "Weekly",
                supports_opus: false,
                supports_credits: true,
                default_enabled: true,
                is_primary: true,
                dashboard_url: Some("https://chatgpt.com/codex/cloud/settings/analytics#usage"),
                status_page_url: Some("https://status.openai.com"),
                tertiary_label_key: None,
            },
            api: CodexApi::new(),
        }
    }
}

fn fetch_result(
    usage: crate::core::UsageSnapshot,
    cost: Option<crate::core::CostSnapshot>,
    source: &str,
    account_identity: Option<String>,
    reset_credits: Option<&api::ResetCredits>,
) -> ProviderFetchResult {
    let account_email = usage.account_email.clone();
    let credits = cost.as_ref().and_then(credits_detail);
    let mut result = ProviderFetchResult::new(usage, source).with_display_detail(credits);
    if let Some(cost) = cost.map(|cost| {
        if cost.account_id.is_none()
            && let Some(account) = account_email.as_deref()
        {
            cost.with_account_id(account)
        } else {
            cost
        }
    }) {
        result = result.with_cost(cost);
    }
    if let Some(account_identity) = account_identity {
        result = result.with_account_identity(account_identity);
    }
    // Reset credits are already shown through the informational
    // `reset-credits` window; the observation only feeds monitoring exports,
    // so it stays off the display inventory.
    if let Some(credits) = reset_credits {
        result = result.with_reset_credits(ResetCreditsObservation {
            available_count: credits.available_count,
            next_expires_at: api::next_available_reset_credit_expiry(
                &credits.credits,
                chrono::Utc::now(),
            ),
        });
    }
    result
}

/// Upstream Credits section (`MenuCardView+Costs`): "125.5 left" over a bar.
/// A spend-control limit gives "of <limit>" and the remaining share; a plain
/// balance is drawn against the next power of ten, labelled in tokens.
/// Unlimited or absent credits produce no cost and so no row.
fn credits_detail(cost: &crate::core::CostSnapshot) -> Option<ProviderDisplayDetail> {
    use crate::providers::format;
    let (remaining, scale, scale_text) = match cost.limit {
        Some(limit) if cost.period == "Monthly credits" => {
            let remaining = (limit - cost.used).max(0.0);
            (remaining, limit, format!("of {}", format::number(limit, 2)))
        }
        Some(_) => return None,
        None if cost.period == "Credits" => {
            let balance = cost.used;
            let scale = fallback_credits_scale(balance)?;
            let scale_text = if scale <= u64::MAX as f64 {
                #[expect(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "scale is a positive power of ten within u64 range"
                )]
                let whole = scale as u64;
                format::token_count(whole)
            } else {
                format::number(scale, 2)
            };
            (balance, scale, format!("{scale_text} tokens"))
        }
        None => return None,
    };
    if !remaining.is_finite() {
        return None;
    }
    ProviderDisplayDetail::new(
        "codex-credits",
        "Credits",
        format!("{} left", format::number(remaining, 2)),
    )?
    .with_secondary_value(scale_text)?
    .with_progress(remaining.clamp(0.0, scale), scale)
}

/// `fallbackCreditsScale`: the next power of ten above the balance (1 for 0).
fn fallback_credits_scale(balance: f64) -> Option<f64> {
    if !balance.is_finite() {
        return None;
    }
    let maximum = balance.max(0.0);
    let exponent = if maximum > 0.0 {
        maximum.log10().floor() + 1.0
    } else {
        0.0
    };
    Some(10f64.powf(exponent).min(f64::MAX))
}

fn pat_allows_auto_fallback(error: &ProviderError) -> bool {
    matches!(
        error,
        ProviderError::AuthRequired | ProviderError::NotInstalled(_)
    )
}

async fn authenticated_http_error(response: reqwest::Response, endpoint: &str) -> ProviderError {
    let status = response.status();
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return ProviderError::AuthRequired;
    }

    let body = response.text().await.unwrap_or_default();
    if body.is_empty() {
        ProviderError::Other(format!("{endpoint} returned {status}"))
    } else {
        ProviderError::Other(format!("{endpoint} returned {status}: {body}"))
    }
}

impl Default for CodexProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for CodexProvider {
    fn automatic_metric_prioritizes_exhausted_window(&self) -> bool {
        false
    }

    /// Upstream `menuBarPaceWindow` paces Codex on its weekly lane, which
    /// `normalize_named_windows` puts in the secondary slot.
    fn menu_bar_pace_lane(&self) -> Option<crate::core::IconLane> {
        Some(crate::core::IconLane::Secondary)
    }

    fn id(&self) -> ProviderId {
        ProviderId::Codex
    }

    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    fn retains_last_good_on_transport_failure(&self) -> bool {
        true
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        tracing::debug!("Fetching Codex usage");

        if ctx.source_mode == SourceMode::Web {
            return Err(ProviderError::UnsupportedSource(SourceMode::Web));
        }

        if ctx.source_mode == SourceMode::Auto && self.api.has_pat_credentials() {
            let version = detect_codex_version();
            match self.api.fetch_usage_pat(version.as_deref()).await {
                Ok((usage, cost, account_identity)) => {
                    return Ok(fetch_result(usage, cost, "pat", account_identity, None));
                }
                Err(error) if pat_allows_auto_fallback(&error) => {
                    tracing::debug!("Codex PAT unavailable in Auto; trying OAuth: {error}");
                }
                Err(error) => return Err(error),
            }
        }

        match self.api.fetch_usage_with_reset_credits().await {
            Ok((usage, cost, account_identity, reset_credits)) => Ok(fetch_result(
                usage,
                cost,
                "oauth",
                account_identity,
                reset_credits.as_ref(),
            )),
            Err(error) => {
                tracing::warn!("Codex API fetch failed: {error}");
                Err(error)
            }
        }
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::OAuth, SourceMode::Cli]
    }

    fn supports_oauth(&self) -> bool {
        true
    }

    fn supports_cli(&self) -> bool {
        true
    }

    fn detect_version(&self) -> Option<String> {
        detect_codex_version()
    }
}

/// Detect the version of the codex CLI
fn detect_codex_version() -> Option<String> {
    let codex_path = crate::codex_cli::locate_codex_binary()?;

    #[cfg(windows)]
    const CREATE_NO_WINDOW: u32 = 0x08000000;

    let mut cmd = std::process::Command::new(codex_path);
    cmd.args(["--version"]);
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);

    let output = cmd.output().ok()?;

    if output.status.success() {
        let version_str = String::from_utf8_lossy(&output.stdout);
        super::extract_semver(&version_str)
    } else {
        None
    }
}

#[cfg(test)]
mod pat_strategy_tests {
    use super::*;
    use crate::core::LastGoodFailurePolicy;

    #[test]
    fn pat_auto_fallback_is_narrow() {
        assert!(pat_allows_auto_fallback(&ProviderError::AuthRequired));
        assert!(pat_allows_auto_fallback(&ProviderError::NotInstalled(
            "missing".into()
        )));
        assert!(!pat_allows_auto_fallback(&ProviderError::Parse(
            "bad".into()
        )));
        assert!(!pat_allows_auto_fallback(&ProviderError::Other(
            "server".into()
        )));
    }

    #[test]
    fn transport_failures_retain_but_authentication_failures_replace() {
        let provider = CodexProvider::new();
        assert_eq!(
            provider.last_good_failure_policy_for_error(&ProviderError::Timeout),
            LastGoodFailurePolicy::Preserve
        );
        assert_eq!(
            provider.last_good_failure_policy_for_error(&ProviderError::AuthRequired),
            LastGoodFailurePolicy::Replace
        );
    }
}

#[cfg(test)]
mod reset_credit_result_tests {
    use super::*;
    use crate::core::{RateWindow, UsageSnapshot};

    fn credits(body: &str) -> api::ResetCredits {
        serde_json::from_str(body).expect("reset credits fixture")
    }

    #[test]
    fn reset_credits_feed_monitoring_without_a_display_inventory_row() {
        let expiry = (chrono::Utc::now() + chrono::Duration::days(3)).to_rfc3339();
        let available = credits(&format!(
            r#"{{"available_count":1,"credits":[{{"status":"available","expires_at":"{expiry}"}}]}}"#
        ));
        let usage = UsageSnapshot::new(RateWindow::new(10.0));
        let result = fetch_result(usage.clone(), None, "oauth", None, Some(&available));
        assert!(result.inventory.is_empty());
        let observation = result.reset_credits.expect("observation");
        assert_eq!(observation.available_count, 1);
        assert_eq!(
            observation.next_expires_at.map(|at| at.to_rfc3339()),
            chrono::DateTime::parse_from_rfc3339(&expiry)
                .ok()
                .map(|at| at.with_timezone(&chrono::Utc).to_rfc3339())
        );

        let exhausted = credits(r#"{"available_count":0,"credits":[]}"#);
        let result = fetch_result(usage.clone(), None, "oauth", None, Some(&exhausted));
        assert!(result.inventory.is_empty());
        assert_eq!(
            result.reset_credits,
            Some(ResetCreditsObservation {
                available_count: 0,
                next_expires_at: None,
            })
        );

        let unknown = fetch_result(usage, None, "pat", None, None);
        assert!(unknown.inventory.is_empty());
        assert!(unknown.reset_credits.is_none());
    }

    /// Cost history scoping: the email only labels the cost snapshot when no
    /// account id came from the API, and the account identity (the
    /// ChatGPT account id that scopes weekly-reset state) is untouched by it.
    #[test]
    fn oauth_email_labels_cost_but_does_not_replace_account_identity() {
        let usage = UsageSnapshot::new(RateWindow::new(10.0)).with_email("parity.user@example.com");
        let cost = crate::core::CostSnapshot::new(125.5, "USD", "Credits");
        let result = fetch_result(
            usage.clone(),
            Some(cost),
            "oauth",
            Some("acct_123".to_string()),
            None,
        );
        assert_eq!(
            result.usage.account_email.as_deref(),
            Some("parity.user@example.com")
        );
        assert_eq!(
            result.cost.as_ref().and_then(|c| c.account_id.as_deref()),
            Some("parity.user@example.com")
        );
        assert_eq!(result.account_identity.as_deref(), Some("acct_123"));

        let keyed = crate::core::CostSnapshot::new(125.5, "USD", "Credits").with_account_id("api");
        let result = fetch_result(usage, Some(keyed), "oauth", None, None);
        assert_eq!(
            result.cost.as_ref().and_then(|c| c.account_id.as_deref()),
            Some("api")
        );
        assert!(result.account_identity.is_none());
    }
}

#[cfg(test)]
mod credits_detail_tests {
    use super::*;
    use crate::core::CostSnapshot;

    fn row(cost: &CostSnapshot) -> Option<(String, Option<String>, f64, f64)> {
        credits_detail(cost).map(|detail| {
            let progress = detail.progress().expect("credits bar");
            (
                detail.value().to_string(),
                detail.secondary_value().map(str::to_string),
                progress.used(),
                progress.total(),
            )
        })
    }

    #[test]
    fn balance_matches_the_upstream_credits_line_and_token_scale() {
        let detail = credits_detail(&CostSnapshot::new(125.5, "USD", "Credits")).expect("row");
        assert_eq!(detail.id(), "codex-credits");
        assert_eq!(detail.title(), "Credits");
        assert_eq!(
            row(&CostSnapshot::new(125.5, "USD", "Credits")),
            Some((
                "125.5 left".to_string(),
                Some("1K tokens".to_string()),
                125.5,
                1000.0
            ))
        );
    }

    #[test]
    fn fallback_scale_is_the_next_power_of_ten() {
        let cases = [
            (0.0, "0 left", "1 tokens", 1.0),
            (9.5, "9.5 left", "10 tokens", 10.0),
            (999.99, "999.99 left", "1K tokens", 1000.0),
            (1000.0, "1,000 left", "10K tokens", 10000.0),
            (2_500_000.0, "2,500,000 left", "10M tokens", 10_000_000.0),
        ];
        for (balance, value, scale_text, scale) in cases {
            let (v, secondary, _, total) =
                row(&CostSnapshot::new(balance, "USD", "Credits")).expect("row");
            assert_eq!(v, value, "balance {balance}");
            assert_eq!(secondary.as_deref(), Some(scale_text), "balance {balance}");
            assert_eq!(total, scale, "balance {balance}");
        }
    }

    #[test]
    fn spend_control_limit_shows_remaining_of_limit() {
        let cost = CostSnapshot::new(40.0, "USD", "Monthly credits").with_limit(100.0);
        assert_eq!(
            row(&cost),
            Some((
                "60 left".to_string(),
                Some("of 100".to_string()),
                60.0,
                100.0
            ))
        );
    }

    #[test]
    fn other_costs_produce_no_credits_row() {
        assert!(credits_detail(&CostSnapshot::new(3.0, "USD", "Monthly")).is_none());
    }

    #[test]
    fn fetch_result_carries_the_credits_row_only_with_credits() {
        let usage = crate::core::UsageSnapshot::new(crate::core::RateWindow::new(10.0));
        let with = fetch_result(
            usage.clone(),
            Some(CostSnapshot::new(125.5, "USD", "Credits")),
            "oauth",
            None,
            None,
        );
        assert_eq!(with.display_details().len(), 1);
        let without = fetch_result(usage, None, "oauth", None, None);
        assert!(without.display_details().is_empty());
    }
}

#[cfg(test)]
mod descriptor_tests {
    use super::*;

    /// Upstream 0.68.0 (#4005): the Usage Dashboard resolves the Codex cloud
    /// analytics page, not the retired `codex/settings/usage` route.
    #[test]
    fn usage_dashboard_resolves_the_registered_codex_analytics_usage_url() {
        let provider = CodexProvider::new();
        let dashboard_url = provider
            .metadata()
            .dashboard_url
            .expect("Codex registers a Usage Dashboard URL");
        let url = reqwest::Url::parse(dashboard_url).expect("dashboard URL parses");
        assert_eq!(
            url.as_str(),
            "https://chatgpt.com/codex/cloud/settings/analytics#usage"
        );
        assert_eq!(url.scheme(), "https");
        assert_eq!(url.host_str(), Some("chatgpt.com"));
        assert_eq!(url.fragment(), Some("usage"));
    }
}
