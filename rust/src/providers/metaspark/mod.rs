//! Meta Muse Spark provider implementation.
//!
//! Meta's Model API (`https://api.meta.ai/v1`) is OpenAI-compatible and
//! exposes no public billing or usage REST endpoint — usage lives in the web
//! dashboard. This provider therefore validates the API key with
//! `GET /v1/models` and reports the reachable `muse-spark-*` models as an
//! informational snapshot. Cost stays unknown (never synthesized as $0).

use async_trait::async_trait;
use reqwest::{Client, Url};
use serde::Deserialize;

use crate::core::{
    FetchContext, Provider, ProviderError, ProviderFetchResult, ProviderId, ProviderMetadata,
    RateWindow, SourceMode, UsageSnapshot,
};

const METASPARK_API_BASE: &str = "https://api.meta.ai/v1";
const METASPARK_CREDENTIAL_TARGET: &str = "codexbar-metaspark";
const METASPARK_ENV_KEYS: &[&str] = &["MODEL_API_KEY", "META_API_KEY"];

#[derive(Debug, Deserialize)]
struct ModelsResponse {
    #[serde(default)]
    data: Vec<ModelEntry>,
}

#[derive(Debug, Deserialize)]
struct ModelEntry {
    #[serde(default)]
    id: Option<String>,
}

pub struct MetaSparkProvider {
    metadata: ProviderMetadata,
    client: Client,
}

impl MetaSparkProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: ProviderId::MetaSpark,
                display_name: "Meta Muse Spark",
                session_label: "Status",
                weekly_label: "Models",
                supports_opus: false,
                supports_credits: false,
                default_enabled: false,
                is_primary: false,
                dashboard_url: Some("https://dev.meta.ai/docs"),
                status_page_url: None,
            },
            client: crate::core::credentialed_http_client_builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }

    async fn probe_models(&self, api_key: &str) -> Result<UsageSnapshot, ProviderError> {
        let url = api_base_url()
            .join("models")
            .map_err(|e| ProviderError::Other(format!("Invalid Meta Muse Spark URL: {e}")))?;
        let response = self
            .client
            .get(url)
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
                "Meta Muse Spark API returned status {}",
                response.status()
            )));
        }

        let body = response.text().await.map_err(|e| {
            ProviderError::Parse(format!("Could not read Meta Muse Spark models: {e}"))
        })?;
        Ok(snapshot_from_models(&parse_muse_spark_models(&body)?))
    }
}

impl Default for MetaSparkProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for MetaSparkProvider {
    fn id(&self) -> ProviderId {
        ProviderId::MetaSpark
    }

    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        match ctx.source_mode {
            SourceMode::Auto | SourceMode::OAuth => {
                let api_key = crate::providers::resolve_api_key(
                    ctx.api_key.as_deref(),
                    METASPARK_CREDENTIAL_TARGET,
                    METASPARK_ENV_KEYS,
                )?;
                Ok(ProviderFetchResult::new(
                    self.probe_models(&api_key).await?,
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

fn api_base_url() -> Url {
    std::env::var("METASPARK_API_URL")
        .ok()
        .and_then(|raw| crate::providers::validated_https_url(&raw, "Meta Muse Spark API").ok())
        .unwrap_or_else(|| Url::parse(METASPARK_API_BASE).expect("static Meta URL is valid"))
}

/// Parse the `muse-spark-*` model ids from a `GET /v1/models` payload.
///
/// The upstream shape is OpenAI-style (`{"data": [{"id": ...}]}`); entries
/// without an id and non-`muse-spark-*` models are ignored.
fn parse_muse_spark_models(body: &str) -> Result<Vec<String>, ProviderError> {
    let response: ModelsResponse = serde_json::from_str(body).map_err(|e| {
        ProviderError::Parse(format!("Could not parse Meta Muse Spark models: {e}"))
    })?;
    let mut models: Vec<String> = response
        .data
        .into_iter()
        .filter_map(|entry| entry.id)
        .map(|id| id.trim().to_string())
        .filter(|id| id.starts_with("muse-spark-"))
        .collect();
    models.sort();
    models.dedup();
    Ok(models)
}

fn snapshot_from_models(models: &[String]) -> UsageSnapshot {
    // No usage percentages exist: Meta publishes no billing/usage REST API,
    // so the primary window stays at 0% and carries the reachable model
    // list as its note. Cost is left absent (unknown, never $0).
    let note = if models.is_empty() {
        "Key valid, no muse-spark models listed".to_string()
    } else {
        format!("Key valid: {}", models.join(", "))
    };
    let primary = RateWindow::with_details(0.0, None, None, Some(note));
    UsageSnapshot::new(primary).with_login_method("Meta Model API")
}

#[cfg(test)]
mod tests {
    use super::*;

    const MODELS_FIXTURE: &str = r#"{
        "object": "list",
        "data": [
            {"id": "muse-spark-1.3", "object": "model", "owned_by": "meta"},
            {"id": "muse-spark-1.3-contributor", "object": "model", "owned_by": "meta"},
            {"id": "muse-spark-1.2", "object": "model", "owned_by": "meta"},
            {"id": "muse-spark-1.1", "object": "model", "owned_by": "meta"},
            {"id": "other-model", "object": "model", "owned_by": "meta"},
            {"object": "model", "owned_by": "meta"}
        ]
    }"#;

    #[test]
    fn parses_and_filters_muse_spark_models() {
        let models = parse_muse_spark_models(MODELS_FIXTURE).unwrap();
        assert_eq!(
            models,
            vec![
                "muse-spark-1.1".to_string(),
                "muse-spark-1.2".to_string(),
                "muse-spark-1.3".to_string(),
                "muse-spark-1.3-contributor".to_string(),
            ]
        );
    }

    #[test]
    fn empty_model_list_parses() {
        let models = parse_muse_spark_models(r#"{"data": []}"#).unwrap();
        assert!(models.is_empty());

        let snapshot = snapshot_from_models(&models);
        assert_eq!(snapshot.primary.used_percent, 0.0);
        assert_eq!(
            snapshot.primary.reset_description.as_deref(),
            Some("Key valid, no muse-spark models listed")
        );
    }

    #[test]
    fn malformed_payload_is_a_parse_error() {
        let err = parse_muse_spark_models("not json").unwrap_err();
        assert!(matches!(err, ProviderError::Parse(_)));
    }

    #[test]
    fn snapshot_lists_models_without_cost() {
        let snapshot =
            snapshot_from_models(&["muse-spark-1.3".to_string(), "muse-spark-1.2".to_string()]);
        // Sorted by the parser in practice; this unit path preserves order.
        assert_eq!(snapshot.primary.used_percent, 0.0);
        assert_eq!(
            snapshot.primary.reset_description.as_deref(),
            Some("Key valid: muse-spark-1.3, muse-spark-1.2")
        );
        assert_eq!(snapshot.login_method.as_deref(), Some("Meta Model API"));
    }

    #[test]
    fn metadata_matches_descriptor() {
        let provider = MetaSparkProvider::new();
        assert_eq!(provider.id(), ProviderId::MetaSpark);
        assert_eq!(provider.metadata().display_name, "Meta Muse Spark");
        assert_eq!(
            provider.metadata().dashboard_url,
            Some("https://dev.meta.ai/docs")
        );
        assert_eq!(provider.metadata().status_page_url, None);
        assert!(!provider.metadata().supports_credits);
        assert!(!provider.metadata().default_enabled);
        assert_eq!(
            provider.available_sources(),
            vec![SourceMode::Auto, SourceMode::OAuth]
        );
    }
}
