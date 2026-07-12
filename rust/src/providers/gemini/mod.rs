//! Gemini provider implementation
//!
//! Fetches usage data from Google's Cloud Code API using OAuth credentials
//! stored by the Gemini CLI in ~/.gemini/oauth_creds.json

mod api;

use async_trait::async_trait;

use crate::core::{
    FetchContext, Provider, ProviderError, ProviderFetchResult, ProviderId, ProviderMetadata,
    SourceMode, UsageSnapshot,
};

pub use api::GeminiApi;

/// Gemini provider for fetching AI usage limits
pub struct GeminiProvider {
    metadata: ProviderMetadata,
    api: GeminiApi,
}

impl GeminiProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: ProviderId::Gemini,
                display_name: "Gemini",
                session_label: "Daily",
                weekly_label: "Daily",
                supports_opus: false,
                supports_credits: false,
                default_enabled: true,
                is_primary: false,
                dashboard_url: Some("https://aistudio.google.com"),
                status_page_url: Some("https://status.cloud.google.com"),
            },
            api: GeminiApi::new(),
        }
    }
}

impl Default for GeminiProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for GeminiProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Gemini
    }

    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        tracing::debug!("Fetching Gemini usage via API");

        match self.api.fetch_quota(ctx).await {
            Ok((primary, model_specific, email, plan)) => {
                let mut usage = UsageSnapshot::new(primary);
                if let Some(ms) = model_specific {
                    usage = usage.with_model_specific(ms);
                }
                if let Some(e) = email {
                    usage = usage.with_email(e);
                }
                usage = usage.with_login_method(plan.unwrap_or_else(|| "Gemini CLI".to_string()));

                Ok(ProviderFetchResult::new(usage, "cli"))
            }
            Err(e) => {
                tracing::warn!("Gemini API fetch failed: {}", e);
                Err(e)
            }
        }
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::Cli]
    }

    fn supports_cli(&self) -> bool {
        true
    }
}
