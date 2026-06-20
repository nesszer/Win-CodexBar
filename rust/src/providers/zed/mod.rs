use async_trait::async_trait;
use reqwest::Client;
use serde_json::Value;

use crate::core::{
    FetchContext, Provider, ProviderError, ProviderFetchResult, ProviderId, ProviderMetadata,
    RateWindow, SourceMode, UsageSnapshot,
};

const CREDENTIAL_TARGET: &str = "codexbar-zed";
const DEFAULT_URL: &str = "https://cloud.zed.dev/client/users/me";

pub struct ZedProvider {
    metadata: ProviderMetadata,
    client: Client,
}

impl ZedProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: ProviderId::Zed,
                display_name: "Zed",
                session_label: "Edits",
                weekly_label: "Cycle",
                supports_opus: false,
                supports_credits: true,
                default_enabled: false,
                is_primary: false,
                dashboard_url: Some("https://zed.dev/account"),
                status_page_url: None,
            },
            client: crate::core::credentialed_http_client_builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }
}

impl Default for ZedProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for ZedProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Zed
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
                    &["ZED_API_KEY", "ZED_CREDENTIALS"],
                )?;
                let url = ctx.workspace_id.as_deref().unwrap_or(DEFAULT_URL);
                let response = self
                    .client
                    .get(url)
                    .header("Authorization", key.trim())
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
                        "Zed usage returned status {}",
                        response.status()
                    )));
                }
                let value: Value = response
                    .json()
                    .await
                    .map_err(|e| ProviderError::Parse(format!("Failed to parse Zed usage: {e}")))?;
                Ok(ProviderFetchResult::new(snapshot_from_user(&value), "api"))
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

fn snapshot_from_user(value: &Value) -> UsageSnapshot {
    let plan = value.get("plan").unwrap_or(value);
    let usage = plan.get("usage").unwrap_or(plan);
    let edits = usage
        .pointer("/edit_predictions")
        .or_else(|| usage.pointer("/editPredictions"))
        .unwrap_or(usage);
    let used = number(edits, &["used"]).unwrap_or(0.0);
    let limit = number(edits, &["limit"]);
    let percent = limit
        .filter(|v| *v > 0.0)
        .map_or(0.0, |limit| used / limit * 100.0);
    UsageSnapshot::new(RateWindow::new(percent)).with_login_method("Zed")
}

fn number(value: &Value, keys: &[&str]) -> Option<f64> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_f64))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_edit_predictions() {
        let snapshot = snapshot_from_user(
            &serde_json::json!({"plan":{"usage":{"editPredictions":{"used":50,"limit":200}}}}),
        );
        assert_eq!(snapshot.primary.used_percent, 25.0);
    }
}
