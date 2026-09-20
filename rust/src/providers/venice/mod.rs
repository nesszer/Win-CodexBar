//! Venice provider implementation.
//!
//! Fetches API balance data from Venice's billing endpoint.

use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;

use crate::core::{
    FetchContext, Provider, ProviderError, ProviderFetchResult, ProviderId, ProviderMetadata,
    RateWindow, SourceMode, UsageSnapshot,
};

const VENICE_BALANCE_URL: &str = "https://api.venice.ai/api/v1/billing/balance";
const VENICE_CREDENTIAL_TARGET: &str = "codexbar-venice";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VeniceBalanceResponse {
    can_consume: bool,
    consumption_currency: Option<String>,
    balances: VeniceBalances,
    diem_epoch_allocation: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct VeniceBalances {
    diem: Option<f64>,
    usd: Option<f64>,
}

pub struct VeniceProvider {
    metadata: ProviderMetadata,
    client: Client,
}

impl VeniceProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: ProviderId::Venice,
                display_name: "Venice",
                session_label: "Balance",
                weekly_label: "DIEM",
                supports_opus: false,
                supports_credits: true,
                default_enabled: false,
                is_primary: false,
                dashboard_url: Some("https://venice.ai/settings/api"),
                status_page_url: None,
                tertiary_label_key: None,
            },
            client: crate::core::credentialed_http_client_builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }

    fn api_key(api_key: Option<&str>) -> Result<String, ProviderError> {
        resolve_api_key(api_key, VENICE_CREDENTIAL_TARGET, &["VENICE_API_KEY"])
    }

    async fn fetch_api(&self, api_key: &str) -> Result<UsageSnapshot, ProviderError> {
        let response = self
            .client
            .get(VENICE_BALANCE_URL)
            .bearer_auth(api_key)
            .header("Accept", "application/json")
            .send()
            .await?;

        if response.status() == reqwest::StatusCode::UNAUTHORIZED
            || response.status() == reqwest::StatusCode::FORBIDDEN
        {
            return Err(ProviderError::AuthRequired);
        }
        if !response.status().is_success() {
            return Err(ProviderError::Other(format!(
                "Venice API returned status {}",
                response.status()
            )));
        }

        let balance: VeniceBalanceResponse = response
            .json()
            .await
            .map_err(|e| ProviderError::Parse(format!("Failed to parse Venice balance: {e}")))?;
        Ok(snapshot_from_balance(&balance))
    }
}

fn snapshot_from_balance(balance: &VeniceBalanceResponse) -> UsageSnapshot {
    let active_currency = balance
        .consumption_currency
        .as_deref()
        .unwrap_or("")
        .to_ascii_uppercase();

    let (used_percent, detail) = if !balance.can_consume {
        (100.0, "Balance unavailable for API calls".to_string())
    } else if active_currency == "USD" && balance.balances.usd.unwrap_or(0.0) > 0.0 {
        (
            0.0,
            format!("${:.2} USD remaining", balance.balances.usd.unwrap_or(0.0)),
        )
    } else if active_currency != "USD" {
        if let (Some(diem), Some(allocation)) =
            (balance.balances.diem, balance.diem_epoch_allocation)
            && allocation > 0.0
        {
            let used = ((allocation - diem) / allocation * 100.0).clamp(0.0, 100.0);
            (
                used,
                format!("DIEM {:.2} / {:.2} epoch allocation", diem, allocation),
            )
        } else if let Some(diem) = balance.balances.diem
            && diem > 0.0
        {
            (0.0, format!("DIEM {diem:.2} remaining"))
        } else if let Some(usd) = balance.balances.usd
            && usd > 0.0
        {
            (0.0, format!("${usd:.2} USD remaining"))
        } else {
            (100.0, "No Venice API balance available".to_string())
        }
    } else {
        (100.0, "No Venice API balance available".to_string())
    };

    let mut window = RateWindow::new(used_percent);
    window.reset_description = Some(detail.clone());
    UsageSnapshot::new(window).with_login_method(detail)
}

impl Default for VeniceProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for VeniceProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Venice
    }

    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        match ctx.source_mode {
            SourceMode::Auto | SourceMode::OAuth => {
                let api_key = Self::api_key(ctx.api_key.as_deref())?;
                Ok(ProviderFetchResult::new(
                    self.fetch_api(&api_key).await?,
                    "api",
                ))
            }
            SourceMode::Web | SourceMode::Cli => {
                Err(ProviderError::UnsupportedSource(ctx.source_mode))
            }
        }
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::OAuth]
    }
}

fn resolve_api_key(
    explicit: Option<&str>,
    credential_target: &str,
    env_names: &[&str],
) -> Result<String, ProviderError> {
    if let Some(key) = explicit
        && !key.trim().is_empty()
    {
        return Ok(key.trim().to_string());
    }
    if let Ok(entry) = keyring::Entry::new(credential_target, "api_key")
        && let Ok(key) = entry.get_password()
        && !key.trim().is_empty()
    {
        return Ok(key);
    }
    for env in env_names {
        if let Ok(key) = std::env::var(env)
            && !key.trim().is_empty()
        {
            return Ok(key);
        }
    }
    Err(ProviderError::NotInstalled(format!(
        "API key not found. Set {} in Preferences or environment.",
        env_names.join(" / ")
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn venice_snapshot_uses_diem_allocation() {
        let snapshot = snapshot_from_balance(&VeniceBalanceResponse {
            can_consume: true,
            consumption_currency: Some("DIEM".into()),
            balances: VeniceBalances {
                diem: Some(25.0),
                usd: None,
            },
            diem_epoch_allocation: Some(100.0),
        });
        assert_eq!(snapshot.primary.used_percent, 75.0);
    }
}
