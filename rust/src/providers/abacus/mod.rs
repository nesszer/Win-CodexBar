//! Abacus AI provider implementation
//!
//! Fetches compute-point usage and billing info via apps.abacus.ai web APIs.
//! Uses browser cookies for authentication.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::Deserialize;

use crate::core::{
    FetchContext, Provider, ProviderError, ProviderFetchResult, ProviderId, ProviderMetadata,
    RateWindow, SourceMode, UsageSnapshot,
};

const COMPUTE_URL: &str = "https://apps.abacus.ai/api/_getOrganizationComputePoints";
const BILLING_URL: &str = "https://apps.abacus.ai/api/_getBillingInfo";

#[derive(Debug, Deserialize)]
struct ApiEnvelope<T> {
    #[serde(default)]
    success: bool,
    result: Option<T>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ComputePoints {
    #[serde(default)]
    total_compute_points: f64,
    #[serde(default)]
    compute_points_left: f64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BillingInfo {
    #[serde(default)]
    next_billing_date: Option<String>,
    #[serde(default)]
    current_tier: Option<String>,
}

pub struct AbacusProvider {
    metadata: ProviderMetadata,
    client: Client,
}

impl AbacusProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: ProviderId::Abacus,
                display_name: "Abacus AI",
                session_label: "Credits",
                weekly_label: "",
                supports_opus: false,
                supports_credits: true,
                default_enabled: false,
                is_primary: false,
                dashboard_url: Some("https://apps.abacus.ai/app/billing"),
                status_page_url: None,
            },
            client: crate::core::credentialed_http_client_builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }

    fn build_snapshot(
        compute: ComputePoints,
        billing: Option<BillingInfo>,
    ) -> Result<UsageSnapshot, ProviderError> {
        let total = compute.total_compute_points.max(0.0);
        let left = compute.compute_points_left.max(0.0);
        let used = (total - left).max(0.0);
        let percent = if total > 0.0 {
            ((used / total) * 100.0).clamp(0.0, 100.0)
        } else {
            0.0
        };

        let mut primary = RateWindow::new(percent);
        primary.reset_description = Some(format!("{:.0}/{:.0} cp", used, total));

        if let Some(ref b) = billing
            && let Some(ts) = b.next_billing_date.as_deref()
            && let Ok(dt) = DateTime::parse_from_rfc3339(ts)
        {
            primary.resets_at = Some(dt.with_timezone(&Utc));
        }

        let mut snapshot = UsageSnapshot::new(primary);
        if let Some(b) = billing
            && let Some(tier) = b.current_tier
            && !tier.is_empty()
        {
            snapshot = snapshot.with_login_method(tier);
        }
        Ok(snapshot)
    }

    async fn fetch_compute(&self, cookie_header: &str) -> Result<ComputePoints, ProviderError> {
        let resp = self
            .client
            .get(COMPUTE_URL)
            .header("Cookie", cookie_header)
            .header("Accept", "application/json")
            .send()
            .await?;

        let status = resp.status();
        if status.as_u16() == 401 || status.as_u16() == 403 {
            return Err(ProviderError::AuthRequired);
        }
        if !status.is_success() {
            return Err(ProviderError::Other(format!(
                "Abacus compute API returned {}",
                status
            )));
        }

        let body = resp.text().await?;
        let env: ApiEnvelope<ComputePoints> = serde_json::from_str(&body)
            .map_err(|e| ProviderError::Parse(format!("Failed to parse compute points: {}", e)))?;

        if !env.success {
            return Err(ProviderError::AuthRequired);
        }
        env.result
            .ok_or_else(|| ProviderError::Parse("Missing compute points result".to_string()))
    }

    async fn fetch_billing(&self, cookie_header: &str) -> Option<BillingInfo> {
        let resp = self
            .client
            .post(BILLING_URL)
            .header("Cookie", cookie_header)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .body("{}")
            .send()
            .await
            .ok()?;

        if !resp.status().is_success() {
            return None;
        }

        let body = resp.text().await.ok()?;
        let env: ApiEnvelope<BillingInfo> = serde_json::from_str(&body).ok()?;
        env.result
    }

    async fn fetch_with_cookies(
        &self,
        cookie_header: &str,
    ) -> Result<UsageSnapshot, ProviderError> {
        let compute = self.fetch_compute(cookie_header).await?;
        let billing = self.fetch_billing(cookie_header).await;
        Self::build_snapshot(compute, billing)
    }
}

impl Default for AbacusProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for AbacusProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Abacus
    }

    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        tracing::debug!("Fetching Abacus AI usage");

        match ctx.source_mode {
            SourceMode::Auto | SourceMode::Web => {
                if let Some(ref cookie_header) = ctx.manual_cookie_header {
                    let usage = self.fetch_with_cookies(cookie_header).await?;
                    return Ok(ProviderFetchResult::new(usage, "web"));
                }

                match crate::providers::browser_cookie_header(&["apps.abacus.ai"]) {
                    Ok(cookie_header) => match self.fetch_with_cookies(&cookie_header).await {
                        Ok(usage) => return Ok(ProviderFetchResult::new(usage, "web")),
                        Err(ProviderError::AuthRequired) => {}
                        Err(e) => return Err(e),
                    },
                    Err(ProviderError::NoCookies) => {}
                    Err(e) => return Err(e),
                }

                Err(ProviderError::AuthRequired)
            }
            SourceMode::Cli => Err(ProviderError::UnsupportedSource(SourceMode::Cli)),
            SourceMode::OAuth => Err(ProviderError::UnsupportedSource(SourceMode::OAuth)),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_compute_points_and_tier() {
        let compute = ComputePoints {
            total_compute_points: 1000.0,
            compute_points_left: 750.0,
        };
        let billing = BillingInfo {
            next_billing_date: Some("2025-01-01T00:00:00Z".into()),
            current_tier: Some("Pro".into()),
        };
        let snap = AbacusProvider::build_snapshot(compute, Some(billing)).unwrap();
        assert!((snap.primary.used_percent - 25.0).abs() < 0.001);
        assert!(snap.primary.resets_at.is_some());
        assert_eq!(snap.login_method.as_deref(), Some("Pro"));
    }

    #[test]
    fn handles_missing_billing() {
        let compute = ComputePoints {
            total_compute_points: 0.0,
            compute_points_left: 0.0,
        };
        let snap = AbacusProvider::build_snapshot(compute, None).unwrap();
        assert!((snap.primary.used_percent - 0.0).abs() < f64::EPSILON);
        assert!(snap.login_method.is_none());
    }
}
