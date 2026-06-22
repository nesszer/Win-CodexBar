use async_trait::async_trait;
use reqwest::{Client, Url};
use serde_json::Value;

use crate::core::{
    CostSnapshot, FetchContext, Provider, ProviderError, ProviderFetchResult, ProviderId,
    ProviderMetadata, RateWindow, SourceMode, UsageSnapshot,
};

const CREDENTIAL_TARGET: &str = "codexbar-litellm";

pub struct LiteLLMProvider {
    metadata: ProviderMetadata,
    client: Client,
}

impl LiteLLMProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: ProviderId::LiteLLM,
                display_name: "LiteLLM",
                session_label: "Budget",
                weekly_label: "Spend",
                supports_opus: false,
                supports_credits: true,
                default_enabled: false,
                is_primary: false,
                dashboard_url: None,
                status_page_url: None,
            },
            client: crate::core::credentialed_http_client_builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }
}

impl Default for LiteLLMProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for LiteLLMProvider {
    fn id(&self) -> ProviderId {
        ProviderId::LiteLLM
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
                    &["LITELLM_API_KEY"],
                )?;
                let base = ctx
                    .workspace_id
                    .as_deref()
                    .map(str::to_string)
                    .or_else(|| std::env::var("LITELLM_BASE_URL").ok())
                    .or_else(|| std::env::var("LITELLM_API_BASE").ok())
                    .ok_or_else(|| {
                        ProviderError::NotInstalled(
                            "LiteLLM base URL not found. Set it in provider extras or LITELLM_BASE_URL.".into(),
                        )
                    })?;
                let response = self
                    .client
                    .get(management_url(&base, "key/info")?)
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
                        "LiteLLM key/info returned status {}",
                        response.status()
                    )));
                }
                let value: Value = response.json().await.map_err(|e| {
                    ProviderError::Parse(format!("Failed to parse LiteLLM key/info: {e}"))
                })?;
                Ok(result_from_key_info(&value))
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

fn management_url(base: &str, path: &str) -> Result<Url, ProviderError> {
    let mut url = crate::providers::validated_https_url(base, "LiteLLM base")?;
    if url.path().trim_end_matches('/').ends_with("/v1") {
        let stripped = url
            .path()
            .trim_end_matches('/')
            .trim_end_matches("/v1")
            .to_string();
        url.set_path(&stripped);
    }
    url.join(path)
        .map_err(|e| ProviderError::Other(format!("Invalid LiteLLM URL: {e}")))
}

fn result_from_key_info(value: &Value) -> ProviderFetchResult {
    let root = value
        .get("info")
        .or_else(|| value.get("key"))
        .unwrap_or(value);
    let spend = number(root, &["spend", "spend_usd", "spendUSD"]).unwrap_or(0.0);
    let limit = number(root, &["max_budget", "maxBudget", "budget", "limit"]);
    let percent = limit
        .filter(|v| *v > 0.0)
        .map_or(0.0, |limit| spend / limit * 100.0);
    let mut snapshot = UsageSnapshot::new(RateWindow::new(percent))
        .with_login_method(format!("Spend ${spend:.2}"));
    if let Some(team) = root.get("team_info").or_else(|| root.get("teamInfo"))
        && let Some(team_spend) = number(team, &["spend", "team_spend", "teamSpend"])
    {
        let team_limit = number(team, &["max_budget", "budget", "limit"]);
        let team_percent = team_limit
            .filter(|v| *v > 0.0)
            .map_or(0.0, |limit| team_spend / limit * 100.0);
        snapshot =
            snapshot.with_extra_rate_window("team", "Team budget", RateWindow::new(team_percent));
    }
    let mut result = ProviderFetchResult::new(snapshot, "api");
    if spend > 0.0 {
        let mut cost = CostSnapshot::new(spend, "USD", "Spend");
        if let Some(limit) = limit {
            cost = cost.with_limit(limit);
        }
        result = result.with_cost(cost);
    }
    result
}

fn number(value: &Value, keys: &[&str]) -> Option<f64> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_f64))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_spend_budget() {
        let result =
            result_from_key_info(&serde_json::json!({"info":{"spend":25.0,"max_budget":100.0}}));
        assert_eq!(result.usage.primary.used_percent, 25.0);
    }
}
