//! Droid (Factory) provider implementation
//!
//! Fetches usage data from Factory.ai (Droid)
//! Uses browser cookies or WorkOS refresh tokens for authentication

use async_trait::async_trait;
use serde::Deserialize;

use crate::core::{
    FetchContext, Provider, ProviderError, ProviderFetchResult, ProviderId, ProviderMetadata,
    RateWindow, SourceMode, UsageSnapshot,
};
use crate::providers::browser_cookie_header;

/// Factory.ai API endpoints
const FACTORY_AUTH_URL: &str = "https://app.factory.ai/api/app/auth/me";
const FACTORY_USAGE_URL: &str = "https://app.factory.ai/api/organization/subscription/usage";

/// Factory usage response
#[derive(Debug, Deserialize)]
struct FactoryUsageResponse {
    #[serde(default)]
    standard: Option<FactoryUsageWindow>,
    #[serde(default)]
    premium: Option<FactoryUsageWindow>,
}

#[derive(Debug, Deserialize)]
struct FactoryUsageWindow {
    used: Option<f64>,
    allowance: Option<f64>,
}

impl FactoryUsageWindow {
    fn percent_used(&self) -> f64 {
        let used = self.used.unwrap_or(0.0);
        let allowance = self.allowance.unwrap_or(1.0);
        if allowance > 0.0 {
            (used / allowance) * 100.0
        } else {
            0.0
        }
    }
}

/// Factory auth response
#[derive(Debug, Deserialize)]
struct FactoryAuthResponse {
    user: Option<FactoryUser>,
    organization: Option<FactoryOrganization>,
}

#[derive(Debug, Deserialize)]
struct FactoryUser {
    email: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FactoryOrganization {
    name: Option<String>,
    tier: Option<String>,
    #[serde(rename = "planName")]
    plan_name: Option<String>,
}

impl FactoryOrganization {
    fn login_method(&self) -> String {
        let tier = self.tier.as_deref().unwrap_or("Droid");
        match self.plan_name.as_deref().filter(|plan| !plan.is_empty()) {
            Some(plan) => format!("{tier} ({plan})"),
            None => tier.to_string(),
        }
    }
}

/// Droid (Factory) provider
pub struct FactoryProvider {
    metadata: ProviderMetadata,
}

impl FactoryProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: ProviderId::Factory,
                display_name: "Droid",
                session_label: "Standard",
                weekly_label: "Premium",
                supports_opus: false,
                supports_credits: true,
                default_enabled: false,
                is_primary: false,
                dashboard_url: Some("https://app.factory.ai"),
                status_page_url: None,
            },
        }
    }

    /// Get cookies for Factory.ai from browser
    fn get_cookies(&self) -> Result<String, ProviderError> {
        browser_cookie_header(&["app.factory.ai"])
    }

    /// Fetch auth info from Factory API
    async fn fetch_auth_info(&self, cookies: &str) -> Result<FactoryAuthResponse, ProviderError> {
        let client = crate::core::credentialed_http_client_builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| ProviderError::Other(e.to_string()))?;

        let resp = client
            .get(FACTORY_AUTH_URL)
            .header("Cookie", cookies)
            .header("Accept", "application/json")
            .send()
            .await?;

        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Err(ProviderError::AuthRequired);
        }

        if !resp.status().is_success() {
            return Err(ProviderError::Other(format!(
                "Factory auth API returned status {}",
                resp.status()
            )));
        }

        resp.json()
            .await
            .map_err(|e| ProviderError::Parse(e.to_string()))
    }

    /// Fetch usage from Factory API
    async fn fetch_usage_api(&self, cookies: &str) -> Result<FactoryUsageResponse, ProviderError> {
        let client = crate::core::credentialed_http_client_builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| ProviderError::Other(e.to_string()))?;

        let resp = client
            .get(FACTORY_USAGE_URL)
            .header("Cookie", cookies)
            .header("Accept", "application/json")
            .send()
            .await?;

        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Err(ProviderError::AuthRequired);
        }

        if !resp.status().is_success() {
            return Err(ProviderError::Other(format!(
                "Factory usage API returned status {}",
                resp.status()
            )));
        }

        resp.json()
            .await
            .map_err(|e| ProviderError::Parse(e.to_string()))
    }

    /// Fetch usage via web cookies
    async fn fetch_via_web(&self) -> Result<UsageSnapshot, ProviderError> {
        let cookies = self.get_cookies()?;
        let auth_info = self.fetch_auth_info(&cookies).await.ok();
        let usage_data = self.fetch_usage_api(&cookies).await?;

        Ok(Self::apply_auth_info(
            Self::usage_snapshot_from_response(&usage_data),
            auth_info,
        ))
    }

    fn usage_snapshot_from_response(usage_data: &FactoryUsageResponse) -> UsageSnapshot {
        let standard_percent = usage_data
            .standard
            .as_ref()
            .map(FactoryUsageWindow::percent_used)
            .unwrap_or(0.0);

        let mut usage = UsageSnapshot::new(RateWindow::new(standard_percent));
        if let Some(premium) = &usage_data.premium {
            usage = usage.with_secondary(RateWindow::new(premium.percent_used()));
        }

        usage
    }

    fn apply_auth_info(
        mut usage: UsageSnapshot,
        auth_info: Option<FactoryAuthResponse>,
    ) -> UsageSnapshot {
        let Some(auth) = auth_info else {
            return usage.with_login_method("Droid");
        };

        if let Some(user) = auth.user
            && let Some(email) = user.email
        {
            usage = usage.with_email(email);
        }

        if let Some(org) = auth.organization {
            usage = usage.with_login_method(org.login_method());
            if let Some(org_name) = org.name {
                usage = usage.with_organization(org_name);
            }
        }

        usage
    }
}

impl Default for FactoryProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for FactoryProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Factory
    }

    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        tracing::debug!("Fetching Droid (Factory) usage");

        match ctx.source_mode {
            SourceMode::Auto | SourceMode::Web => {
                let usage = self.fetch_via_web().await?;
                Ok(ProviderFetchResult::new(usage, "web"))
            }
            SourceMode::Cli | SourceMode::OAuth => {
                // Droid doesn't have CLI or OAuth support
                Err(ProviderError::UnsupportedSource(ctx.source_mode))
            }
        }
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::Web]
    }

    fn supports_web(&self) -> bool {
        true
    }

    fn supports_cli(&self) -> bool {
        false
    }
}
