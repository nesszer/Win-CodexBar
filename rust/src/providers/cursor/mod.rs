//! Cursor provider implementation
//!
//! Fetches usage data from Cursor's API using browser cookies

mod api;

use async_trait::async_trait;

use crate::core::{
    CostSnapshot, FetchContext, Provider, ProviderError, ProviderFetchResult, ProviderId,
    ProviderMetadata, RateWindow, SourceMode, UsageSnapshot,
};

pub use api::CursorApi;

/// Cursor provider for fetching AI usage limits
pub struct CursorProvider {
    metadata: ProviderMetadata,
    api: CursorApi,
}

impl CursorProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: ProviderId::Cursor,
                display_name: "Cursor",
                session_label: "Plan",
                weekly_label: "Auto",
                supports_opus: false,
                supports_credits: true,
                default_enabled: true,
                is_primary: false,
                dashboard_url: Some("https://cursor.com/dashboard/usage"),
                status_page_url: None,
            },
            api: CursorApi::new(),
        }
    }

    async fn fetch_usage_parts(
        &self,
        ctx: &FetchContext,
    ) -> Result<api::CursorUsageResult, ProviderError> {
        match ctx.source_mode {
            SourceMode::Auto | SourceMode::Web => self.fetch_web_usage_parts(ctx).await,
            SourceMode::Cli | SourceMode::OAuth => {
                Err(ProviderError::UnsupportedSource(ctx.source_mode))
            }
        }
    }

    async fn fetch_web_usage_parts(
        &self,
        ctx: &FetchContext,
    ) -> Result<api::CursorUsageResult, ProviderError> {
        if let Some(cookie_header) = ctx.manual_cookie_header.as_deref() {
            self.api.fetch_usage_with_cookie_header(cookie_header).await
        } else {
            self.api.fetch_usage().await
        }
    }

    fn build_usage_snapshot(
        primary: RateWindow,
        secondary: Option<RateWindow>,
        model_specific: Option<RateWindow>,
        email: Option<String>,
        plan_type: Option<String>,
    ) -> UsageSnapshot {
        let mut usage = UsageSnapshot::new(primary);
        if let Some(sec) = secondary {
            usage = usage.with_secondary(sec);
        }
        if let Some(ms) = model_specific {
            usage = usage.with_model_specific(ms);
        }
        if let Some(e) = email {
            usage = usage.with_email(e);
        }
        if let Some(plan) = plan_type {
            usage = usage.with_login_method(plan);
        }
        usage
    }

    fn build_fetch_result(usage: UsageSnapshot, cost: Option<CostSnapshot>) -> ProviderFetchResult {
        let mut result = ProviderFetchResult::new(usage, "web");
        if let Some(c) = cost {
            result = result.with_cost(c);
        }
        result
    }
}

impl Default for CursorProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for CursorProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Cursor
    }

    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        tracing::debug!("Fetching Cursor usage via web API");

        match self.fetch_usage_parts(ctx).await {
            Ok((primary, secondary, model_specific, cost, email, plan_type)) => {
                let usage = Self::build_usage_snapshot(
                    primary,
                    secondary,
                    model_specific,
                    email,
                    plan_type,
                );
                Ok(Self::build_fetch_result(usage, cost))
            }
            Err(e) => {
                tracing::warn!("Cursor API fetch failed: {}", e);
                Err(e)
            }
        }
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::Web]
    }

    fn supports_web(&self) -> bool {
        true
    }
}
