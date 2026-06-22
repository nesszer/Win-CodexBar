//! Azure OpenAI provider implementation.
//!
//! Validates an Azure OpenAI deployment with a tiny chat completion request.

use async_trait::async_trait;
use reqwest::Url;
use serde::Deserialize;

use crate::core::{
    FetchContext, Provider, ProviderError, ProviderFetchResult, ProviderId, ProviderMetadata,
    RateWindow, SourceMode, UsageSnapshot,
};
use crate::settings::ApiKeys;

const DEFAULT_API_VERSION: &str = "2024-10-21";

pub struct AzureOpenAIProvider {
    metadata: ProviderMetadata,
}

#[derive(Debug, Clone)]
struct AzureOpenAIConfig {
    api_key: String,
    endpoint: Url,
    deployment: String,
    api_version: String,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    model: Option<String>,
}

impl AzureOpenAIProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: ProviderId::AzureOpenAI,
                display_name: "Azure OpenAI",
                session_label: "Deployment",
                weekly_label: "Status",
                supports_opus: false,
                supports_credits: false,
                default_enabled: false,
                is_primary: false,
                dashboard_url: Some("https://ai.azure.com"),
                status_page_url: Some("https://status.azure.com"),
            },
        }
    }

    async fn fetch_via_api(&self, ctx: &FetchContext) -> Result<UsageSnapshot, ProviderError> {
        let config = Self::resolve_config(ctx)?;
        let url = Self::chat_completions_url(
            config.endpoint.clone(),
            &config.deployment,
            &config.api_version,
        )?;

        let body = Self::validation_body(&config.deployment, &config.api_version);
        let client = crate::core::credentialed_http_client_builder()
            .timeout(std::time::Duration::from_secs(ctx.web_timeout.max(1)))
            .build()
            .map_err(|e| ProviderError::Other(e.to_string()))?;

        let response = client
            .post(url)
            .header("api-key", &config.api_key)
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        let status = response.status();
        let bytes = response.bytes().await?;
        if !status.is_success() {
            let summary = Self::response_summary(&bytes);
            if status == reqwest::StatusCode::UNAUTHORIZED
                || status == reqwest::StatusCode::FORBIDDEN
            {
                return Err(ProviderError::AuthRequired);
            }
            return Err(ProviderError::Other(format!(
                "Azure OpenAI API error: HTTP {status}: {summary}"
            )));
        }

        let parsed: ChatCompletionResponse = serde_json::from_slice(&bytes)
            .map_err(|e| ProviderError::Parse(format!("Azure OpenAI response parse error: {e}")))?;

        let mut primary = RateWindow::new(0.0);
        primary.reset_description = Some(Self::detail_text(
            &config.deployment,
            parsed.model.as_deref(),
        ));
        let usage = UsageSnapshot::new(primary)
            .with_organization(config.endpoint.host_str().unwrap_or_default().to_string())
            .with_login_method(format!("Deployment: {}", config.deployment));
        Ok(usage)
    }

    fn resolve_config(ctx: &FetchContext) -> Result<AzureOpenAIConfig, ProviderError> {
        if let Some(raw) = ctx.api_key.as_deref().and_then(clean_string) {
            return Self::parse_saved_config(&raw);
        }
        if let Some(config) = Self::config_from_env()? {
            return Ok(config);
        }
        if let Some(raw) = ApiKeys::load().get("azureopenai") {
            return Self::parse_saved_config(raw);
        }
        Err(ProviderError::AuthRequired)
    }

    fn config_from_env() -> Result<Option<AzureOpenAIConfig>, ProviderError> {
        let Some(api_key) = clean_env("AZURE_OPENAI_API_KEY") else {
            return Ok(None);
        };
        let endpoint = clean_env("AZURE_OPENAI_ENDPOINT")
            .ok_or_else(|| ProviderError::Other("Azure OpenAI endpoint not configured".into()))?;
        let deployment = clean_env("AZURE_OPENAI_DEPLOYMENT")
            .or_else(|| clean_env("AZURE_OPENAI_DEPLOYMENT_NAME"))
            .ok_or_else(|| ProviderError::Other("Azure OpenAI deployment not configured".into()))?;
        let api_version =
            clean_env("AZURE_OPENAI_API_VERSION").unwrap_or_else(|| DEFAULT_API_VERSION.into());
        Ok(Some(AzureOpenAIConfig {
            api_key,
            endpoint: Self::parse_endpoint(&endpoint)?,
            deployment,
            api_version,
        }))
    }

    fn parse_saved_config(raw: &str) -> Result<AzureOpenAIConfig, ProviderError> {
        #[derive(Deserialize)]
        struct Saved {
            api_key: String,
            endpoint: String,
            deployment: String,
            #[serde(default)]
            api_version: Option<String>,
        }

        if let Ok(saved) = serde_json::from_str::<Saved>(raw) {
            return Ok(AzureOpenAIConfig {
                api_key: clean_string(&saved.api_key)
                    .ok_or_else(|| ProviderError::Other("Azure OpenAI API key is empty".into()))?,
                endpoint: Self::parse_endpoint(&saved.endpoint)?,
                deployment: clean_string(&saved.deployment).ok_or_else(|| {
                    ProviderError::Other("Azure OpenAI deployment not configured".into())
                })?,
                api_version: saved
                    .api_version
                    .as_deref()
                    .and_then(clean_string)
                    .unwrap_or_else(|| DEFAULT_API_VERSION.into()),
            });
        }

        let parts: Vec<_> = raw.split('|').map(str::trim).collect();
        if parts.len() < 3 {
            return Err(ProviderError::Other(
                "Azure OpenAI key must be JSON or api_key|endpoint|deployment[|api_version]".into(),
            ));
        }
        Ok(AzureOpenAIConfig {
            api_key: clean_string(parts[0])
                .ok_or_else(|| ProviderError::Other("Azure OpenAI API key is empty".into()))?,
            endpoint: Self::parse_endpoint(parts[1])?,
            deployment: clean_string(parts[2])
                .ok_or_else(|| ProviderError::Other("Azure OpenAI deployment is empty".into()))?,
            api_version: parts
                .get(3)
                .and_then(|value| clean_string(value))
                .unwrap_or_else(|| DEFAULT_API_VERSION.into()),
        })
    }

    fn parse_endpoint(raw: &str) -> Result<Url, ProviderError> {
        let cleaned = clean_string(raw)
            .ok_or_else(|| ProviderError::Other("Azure OpenAI endpoint not configured".into()))?;
        crate::providers::validated_https_url(&cleaned, "Azure OpenAI endpoint")
    }

    fn chat_completions_url(
        endpoint: Url,
        deployment: &str,
        api_version: &str,
    ) -> Result<Url, ProviderError> {
        if api_version.trim().eq_ignore_ascii_case("v1") {
            let mut url = Self::api_root(endpoint, &["openai", "v1"])?;
            url.path_segments_mut()
                .map_err(|_| ProviderError::Other("Azure OpenAI URL cannot be a base".into()))?
                .extend(["chat", "completions"]);
            return Ok(url);
        }

        let mut url = Self::api_root(endpoint, &["openai"])?;
        url.path_segments_mut()
            .map_err(|_| ProviderError::Other("Azure OpenAI URL cannot be a base".into()))?
            .extend(["deployments", deployment, "chat", "completions"]);
        url.query_pairs_mut()
            .append_pair("api-version", api_version.trim());
        Ok(url)
    }

    fn api_root(mut endpoint: Url, expected: &[&str]) -> Result<Url, ProviderError> {
        let existing: Vec<String> = endpoint
            .path_segments()
            .map(|segments| {
                segments
                    .filter(|segment| !segment.is_empty())
                    .map(|segment| segment.to_lowercase())
                    .collect()
            })
            .unwrap_or_default();
        let expected_lower: Vec<String> =
            expected.iter().map(|segment| segment.to_string()).collect();
        let shared = (0..=existing.len().min(expected_lower.len()))
            .rev()
            .find(|count| {
                *count == 0 || existing[existing.len() - count..] == expected_lower[..*count]
            })
            .unwrap_or(0);
        endpoint
            .path_segments_mut()
            .map_err(|_| ProviderError::Other("Azure OpenAI URL cannot be a base".into()))?
            .extend(expected.iter().skip(shared).copied());
        Ok(endpoint)
    }

    fn validation_body(deployment: &str, api_version: &str) -> serde_json::Value {
        if api_version.trim().eq_ignore_ascii_case("v1") {
            serde_json::json!({
                "model": deployment,
                "messages": [{"role": "user", "content": "ping"}],
                "max_completion_tokens": 1
            })
        } else {
            serde_json::json!({
                "messages": [{"role": "user", "content": "ping"}],
                "max_tokens": 1
            })
        }
    }

    fn detail_text(deployment: &str, model: Option<&str>) -> String {
        let model = model.map(str::trim).filter(|model| !model.is_empty());
        match model {
            Some(model) => format!("Deployment: {deployment} · Model: {model}"),
            None => format!("Deployment: {deployment}"),
        }
    }

    fn response_summary(bytes: &[u8]) -> String {
        let body = String::from_utf8_lossy(bytes);
        let collapsed = body.split_whitespace().collect::<Vec<_>>().join(" ");
        if collapsed.len() > 240 {
            let prefix = collapsed.chars().take(240).collect::<String>();
            format!("{prefix}... [truncated]")
        } else {
            collapsed
        }
    }
}

fn clean_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .and_then(|value| clean_string(&value))
}

fn clean_string(value: &str) -> Option<String> {
    let mut value = value.trim().to_string();
    if value.len() >= 2
        && ((value.starts_with('"') && value.ends_with('"'))
            || (value.starts_with('\'') && value.ends_with('\'')))
    {
        value.remove(0);
        value.pop();
    }
    let value = value.trim().to_string();
    (!value.is_empty()).then_some(value)
}

impl Default for AzureOpenAIProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for AzureOpenAIProvider {
    fn id(&self) -> ProviderId {
        ProviderId::AzureOpenAI
    }

    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        match ctx.source_mode {
            SourceMode::Auto | SourceMode::Web => {
                let usage = self.fetch_via_api(ctx).await?;
                Ok(ProviderFetchResult::new(usage, "api"))
            }
            SourceMode::Cli | SourceMode::OAuth => {
                Err(ProviderError::UnsupportedSource(ctx.source_mode))
            }
        }
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::Web]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_classic_chat_completions_url() {
        let endpoint = Url::parse("https://example.openai.azure.com").unwrap();
        let url =
            AzureOpenAIProvider::chat_completions_url(endpoint, "chat prod", "2024-10-21").unwrap();
        assert_eq!(
            url.as_str(),
            "https://example.openai.azure.com/openai/deployments/chat%20prod/chat/completions?api-version=2024-10-21"
        );
    }

    #[test]
    fn builds_v1_chat_completions_url_without_duplicate_suffix() {
        let endpoint = Url::parse("https://example.openai.azure.com/openai/v1").unwrap();
        let url = AzureOpenAIProvider::chat_completions_url(endpoint, "chat-prod", "v1").unwrap();
        assert_eq!(
            url.as_str(),
            "https://example.openai.azure.com/openai/v1/chat/completions"
        );
    }

    #[test]
    fn parses_saved_composite_config() {
        let config = AzureOpenAIProvider::parse_saved_config(
            " 'key' | example.openai.azure.com | \"chat-prod\" | v1 ",
        )
        .unwrap();
        assert_eq!(config.api_key, "key");
        assert_eq!(
            config.endpoint.as_str(),
            "https://example.openai.azure.com/"
        );
        assert_eq!(config.deployment, "chat-prod");
        assert_eq!(config.api_version, "v1");
    }

    #[test]
    fn rejects_insecure_or_tricky_endpoint_overrides() {
        assert!(AzureOpenAIProvider::parse_endpoint("http://example.com").is_err());
        assert!(AzureOpenAIProvider::parse_endpoint("https://user@example.com").is_err());
        assert!(AzureOpenAIProvider::parse_endpoint("https://example.com%2f.evil.test").is_err());
    }
}
