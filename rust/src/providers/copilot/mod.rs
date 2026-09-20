//! GitHub Copilot provider implementation
//!
//! Fetches usage data from GitHub's Copilot API using stored OAuth token

mod api;
pub mod device_flow;

use async_trait::async_trait;

use crate::core::{
    FetchContext, Provider, ProviderError, ProviderFetchResult, ProviderId, ProviderMetadata,
    SourceMode,
};

pub use api::{CopilotApi, SEAT_CREDIT_WINDOW_ID};

/// GitHub Copilot provider for fetching AI usage limits
pub struct CopilotProvider {
    metadata: ProviderMetadata,
    api: CopilotApi,
}

impl CopilotProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: ProviderId::Copilot,
                display_name: "GitHub Copilot",
                session_label: "Premium",
                weekly_label: "Chat",
                supports_opus: false,
                supports_credits: false,
                default_enabled: true,
                is_primary: false,
                dashboard_url: Some("https://github.com/settings/copilot"),
                status_page_url: Some("https://www.githubstatus.com/"),
                tertiary_label_key: None,
            },
            api: CopilotApi::new(),
        }
    }
}

impl Default for CopilotProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for CopilotProvider {
    fn automatic_metric_prioritizes_exhausted_window(&self) -> bool {
        false
    }

    fn explicit_preference_falls_through_to_automatic(&self) -> bool {
        false
    }

    fn automatic_metric_missing_core_is_terminal(&self) -> bool {
        false
    }

    fn id(&self) -> ProviderId {
        ProviderId::Copilot
    }

    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        tracing::debug!("Fetching GitHub Copilot usage via GitHub OAuth");

        let token = self.api.load_token(ctx.api_key.as_deref(), None)?;
        match self
            .api
            .fetch_usage_with_token(&token, None, ctx.seat_credit_entitlement)
            .await
        {
            Ok(usage) => Ok(ProviderFetchResult::new(usage, "oauth")),
            Err(e) => {
                tracing::warn!("Copilot API fetch failed: {}", e);
                Err(e)
            }
        }
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::OAuth]
    }

    fn supports_oauth(&self) -> bool {
        true
    }
}
