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
    FetchContext, Provider, ProviderError, ProviderFetchResult, ProviderId,
    ResetCreditsObservation, SourceMode,
};

pub use api::CodexApi;

/// Codex provider for fetching AI usage limits
pub struct CodexProvider {
    api: CodexApi,
}

impl CodexProvider {
    pub fn new() -> Self {
        Self {
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
    let mut result = ProviderFetchResult::new(usage, source);
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

    fn id(&self) -> ProviderId {
        ProviderId::Codex
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
