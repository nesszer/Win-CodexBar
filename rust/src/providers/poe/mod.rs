use async_trait::async_trait;
use reqwest::Client;
use serde_json::Value;

use crate::core::{
    FetchContext, Provider, ProviderError, ProviderFetchResult, ProviderId, ProviderMetadata,
    RateWindow, SourceMode, UsageSnapshot,
};

const POE_BALANCE_URL: &str = "https://api.poe.com/usage/current_balance";
const CREDENTIAL_TARGET: &str = "codexbar-poe";

pub struct PoeProvider {
    metadata: ProviderMetadata,
    client: Client,
}

impl PoeProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: ProviderId::Poe,
                display_name: "Poe",
                session_label: "Balance",
                weekly_label: "Points",
                supports_opus: false,
                supports_credits: true,
                default_enabled: false,
                is_primary: false,
                dashboard_url: Some("https://poe.com/settings/subscription"),
                status_page_url: None,
            },
            client: crate::core::credentialed_http_client_builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }
}

impl Default for PoeProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for PoeProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Poe
    }

    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        match ctx.source_mode {
            SourceMode::Auto | SourceMode::OAuth => {
                let key = crate::providers::resolve_api_key(
                    ctx.api_key.as_deref(),
                    CREDENTIAL_TARGET,
                    &["POE_API_KEY"],
                )?;
                let response = self
                    .client
                    .get(POE_BALANCE_URL)
                    .bearer_auth(key)
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
                        "Poe usage returned status {}",
                        response.status()
                    )));
                }
                let value: Value = response.json().await.map_err(|e| {
                    ProviderError::Parse(format!("Failed to parse Poe balance: {e}"))
                })?;
                Ok(ProviderFetchResult::new(
                    snapshot_from_balance(&value),
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

fn snapshot_from_balance(value: &Value) -> UsageSnapshot {
    let balance = first_number(
        value,
        &[
            "current_point_balance",
            "currentPointBalance",
            "balance",
            "points",
        ],
    );
    let mut primary = RateWindow::new(0.0);
    primary.reset_description = balance.map(|v| format!("Balance: {v:.0} points"));
    let label = primary
        .reset_description
        .clone()
        .unwrap_or_else(|| "Poe API".into());
    UsageSnapshot::new(primary).with_login_method(label)
}

fn first_number(value: &Value, keys: &[&str]) -> Option<f64> {
    match value {
        Value::Object(map) => keys
            .iter()
            .find_map(|key| map.get(*key).and_then(Value::as_f64))
            .or_else(|| map.values().find_map(|v| first_number(v, keys))),
        Value::Array(items) => items.iter().find_map(|v| first_number(v, keys)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_balance_label() {
        let snapshot = snapshot_from_balance(&serde_json::json!({"current_point_balance": 1234}));
        assert_eq!(
            snapshot.login_method.as_deref(),
            Some("Balance: 1234 points")
        );
    }
}
