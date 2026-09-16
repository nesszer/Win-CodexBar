//! Codex (OpenAI/ChatGPT) provider implementation
//!
//! Fetches usage data from ChatGPT's backend API using OAuth credentials
//! stored by the Codex CLI in ~/.codex/auth.json

mod api;
mod pat;
mod weekly_reset;

use async_trait::async_trait;
#[cfg(windows)]
use std::os::windows::process::CommandExt;

use crate::core::{
    FetchContext, LastGoodFailurePolicy, Provider, ProviderError, ProviderFetchResult, ProviderId,
    ProviderMetadata, SourceMode,
};

pub use api::CodexApi;

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
                dashboard_url: Some("https://chatgpt.com/codex/settings/usage"),
                status_page_url: Some("https://status.openai.com"),
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

    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    fn last_good_failure_policy_for_error(&self, error: &ProviderError) -> LastGoodFailurePolicy {
        if error.is_transport_failure() {
            LastGoodFailurePolicy::Preserve
        } else {
            LastGoodFailurePolicy::Replace
        }
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
                    return Ok(fetch_result(usage, cost, "pat", account_identity));
                }
                Err(error) if pat_allows_auto_fallback(&error) => {
                    tracing::debug!("Codex PAT unavailable in Auto; trying OAuth: {error}");
                }
                Err(error) => return Err(error),
            }
        }

        match self.api.fetch_usage().await {
            Ok((usage, cost, account_identity)) => {
                Ok(fetch_result(usage, cost, "oauth", account_identity))
            }
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
