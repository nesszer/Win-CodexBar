//! Meta provider implementation.
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

const META_API_BASE: &str = "https://api.meta.ai/v1";
const META_CREDENTIAL_TARGET: &str = "codexbar-meta";
/// Previous credential target, kept as a read fallback for existing installs.
const LEGACY_METASPARK_CREDENTIAL_TARGET: &str = "codexbar-metaspark";
const META_ENV_KEYS: &[&str] = &["MODEL_API_KEY", "META_API_KEY"];

#[derive(Debug, Deserialize)]
struct ModelsResponse {
    data: Vec<ModelEntry>,
}

#[derive(Debug, Deserialize)]
struct ModelEntry {
    #[serde(default)]
    id: Option<String>,
}

pub struct MetaProvider {
    metadata: ProviderMetadata,
    client: Client,
}

impl MetaProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: ProviderId::Meta,
                display_name: "Meta",
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
        let url = models_url(&api_base_url()?)?;
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
                "Meta API returned status {}",
                response.status()
            )));
        }

        let body = response
            .text()
            .await
            .map_err(|e| ProviderError::Parse(format!("Could not read Meta models: {e}")))?;
        Ok(snapshot_from_models(&parse_muse_spark_models(&body)?))
    }
}

impl Default for MetaProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for MetaProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Meta
    }

    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        match ctx.source_mode {
            SourceMode::Auto | SourceMode::OAuth => {
                let api_key = resolve_meta_api_key(ctx.api_key.as_deref())?;
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

/// Resolve the Meta API key, falling back to the legacy `codexbar-metaspark`
/// credential target for existing installs.
fn resolve_meta_api_key(explicit: Option<&str>) -> Result<String, ProviderError> {
    match crate::providers::resolve_api_key(explicit, META_CREDENTIAL_TARGET, META_ENV_KEYS) {
        Ok(key) => Ok(key),
        Err(first_err) => {
            if explicit.is_some_and(|key| !key.trim().is_empty()) {
                return Err(first_err);
            }
            crate::providers::resolve_api_key(
                None,
                LEGACY_METASPARK_CREDENTIAL_TARGET,
                META_ENV_KEYS,
            )
            .or(Err(first_err))
        }
    }
}

fn api_base_url() -> Result<Url, ProviderError> {
    let preferred = std::env::var("META_API_URL").ok();
    let legacy = std::env::var("METASPARK_API_URL").ok();
    resolve_api_base_url(preferred.as_deref(), legacy.as_deref())
}

/// Resolve the configured Meta API base.
///
/// Precedence is preferred (`META_API_URL`) then legacy (`METASPARK_API_URL`),
/// falling back to the production default only when neither variable is set.
/// A variable that *is* set but invalid is an error: an invalid preferred value
/// never silently falls through to the legacy value or the default.
fn resolve_api_base_url(
    preferred: Option<&str>,
    legacy: Option<&str>,
) -> Result<Url, ProviderError> {
    if let Some(raw) = preferred {
        return crate::providers::validated_https_url(raw, "Meta API");
    }
    if let Some(raw) = legacy {
        return crate::providers::validated_https_url(raw, "Meta API");
    }
    Ok(Url::parse(META_API_BASE).expect("static Meta URL is valid"))
}

/// Resolve the `GET /v1/models` endpoint from a configured API base.
///
/// The base is treated as a directory, so both `https://api.meta.ai/v1` and
/// `https://api.meta.ai/v1/` resolve to `https://api.meta.ai/v1/models`
/// instead of `Url::join` dropping the `v1` segment for the no-trailing-slash
/// form. A bare host resolves to `https://api.meta.ai/models`.
fn models_url(base: &Url) -> Result<Url, ProviderError> {
    let mut url = base.clone();
    url.path_segments_mut()
        .map_err(|_| ProviderError::Other("Meta API URL cannot be used as a base URL".to_string()))?
        .pop_if_empty()
        .push("models");
    Ok(url)
}

/// Parse the `muse-spark-*` model ids from a `GET /v1/models` payload.
///
/// The upstream shape is OpenAI-style (`{"data": [{"id": ...}]}`); entries
/// without an id and non-`muse-spark-*` models are ignored.
fn parse_muse_spark_models(body: &str) -> Result<Vec<String>, ProviderError> {
    let response: ModelsResponse = serde_json::from_str(body)
        .map_err(|e| ProviderError::Parse(format!("Could not parse Meta models: {e}")))?;
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
    // list as its note. It is marked informational so consumers never treat
    // the placeholder as real quota usage. Cost is left absent (unknown,
    // never $0).
    let note = if models.is_empty() {
        "Key valid, no muse-spark models listed".to_string()
    } else {
        format!("Key valid: {}", models.join(", "))
    };
    let primary = RateWindow::informational(note);
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
        assert!(snapshot.primary.is_informational);
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
    fn empty_top_level_object_is_rejected() {
        let err = parse_muse_spark_models("{}").unwrap_err();
        assert!(matches!(err, ProviderError::Parse(_)));
    }

    #[test]
    fn missing_data_field_is_rejected() {
        let err = parse_muse_spark_models(r#"{"object": "list"}"#).unwrap_err();
        assert!(matches!(err, ProviderError::Parse(_)));
    }

    #[test]
    fn wrong_data_type_is_rejected() {
        let err = parse_muse_spark_models(r#"{"data": {"id": "muse-spark-1.3"}}"#).unwrap_err();
        assert!(matches!(err, ProviderError::Parse(_)));

        let err = parse_muse_spark_models(r#"{"data": "muse-spark-1.3"}"#).unwrap_err();
        assert!(matches!(err, ProviderError::Parse(_)));
    }

    #[test]
    fn minimal_valid_response_parses() {
        assert!(
            parse_muse_spark_models(r#"{"data": []}"#)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            parse_muse_spark_models(r#"{"data": [{"id": "muse-spark-1.3"}]}"#).unwrap(),
            vec!["muse-spark-1.3".to_string()]
        );
    }

    #[test]
    fn snapshot_lists_models_without_cost() {
        let snapshot =
            snapshot_from_models(&["muse-spark-1.3".to_string(), "muse-spark-1.2".to_string()]);
        // Sorted by the parser in practice; this unit path preserves order.
        assert_eq!(snapshot.primary.used_percent, 0.0);
        assert!(snapshot.primary.is_informational);
        assert_eq!(
            snapshot.primary.reset_description.as_deref(),
            Some("Key valid: muse-spark-1.3, muse-spark-1.2")
        );
        assert_eq!(snapshot.login_method.as_deref(), Some("Meta Model API"));

        // Cost stays absent through the fetch result: no quota API exists, so
        // a synthesized $0 would be a false reading.
        let result = ProviderFetchResult::new(snapshot, "api");
        assert!(result.cost.is_none());
        assert!(result.usage.primary.is_informational);
    }

    #[test]
    fn default_models_url_preserves_v1_path() {
        let base = Url::parse(META_API_BASE).unwrap();
        assert_eq!(
            models_url(&base).unwrap().as_str(),
            "https://api.meta.ai/v1/models"
        );
    }

    #[test]
    fn models_url_treats_configured_base_as_directory() {
        for (base, expected) in [
            ("https://api.meta.ai/v1", "https://api.meta.ai/v1/models"),
            ("https://api.meta.ai/v1/", "https://api.meta.ai/v1/models"),
            ("https://api.meta.ai", "https://api.meta.ai/models"),
            ("https://api.meta.ai/", "https://api.meta.ai/models"),
            (
                "https://gateway.example.com/meta/v1",
                "https://gateway.example.com/meta/v1/models",
            ),
            (
                "https://gateway.example.com/meta/v1/",
                "https://gateway.example.com/meta/v1/models",
            ),
            (
                "https://gateway.example.com/proxy",
                "https://gateway.example.com/proxy/models",
            ),
        ] {
            let parsed = Url::parse(base).unwrap();
            assert_eq!(
                models_url(&parsed).unwrap().as_str(),
                expected,
                "base {base}"
            );
        }
    }

    #[test]
    fn default_api_base_used_when_neither_env_is_set() {
        let base = resolve_api_base_url(None, None).unwrap();
        assert_eq!(base.as_str(), META_API_BASE);
        assert_eq!(
            models_url(&base).unwrap().as_str(),
            "https://api.meta.ai/v1/models"
        );
    }

    #[test]
    fn preferred_api_url_wins_over_legacy() {
        let base = resolve_api_base_url(
            Some("https://preferred.example.com/meta/v1"),
            Some("https://legacy.example.com/meta/v1"),
        )
        .unwrap();
        assert_eq!(base.as_str(), "https://preferred.example.com/meta/v1");
        assert_eq!(
            models_url(&base).unwrap().as_str(),
            "https://preferred.example.com/meta/v1/models"
        );
    }

    #[test]
    fn legacy_api_url_used_when_preferred_is_absent() {
        let base = resolve_api_base_url(None, Some("https://legacy.example.com/meta/v1")).unwrap();
        assert_eq!(base.as_str(), "https://legacy.example.com/meta/v1");
    }

    #[test]
    fn invalid_preferred_url_errors_instead_of_falling_through() {
        for bad in ["", "http://preferred.example.com/v1", "not a url"] {
            let result =
                resolve_api_base_url(Some(bad), Some("https://legacy.example.com/meta/v1"));
            assert!(result.is_err(), "expected {bad:?} to be rejected");
            assert!(matches!(result.unwrap_err(), ProviderError::Other(_)));
        }
    }

    #[test]
    fn invalid_legacy_url_errors_instead_of_defaulting() {
        let err = resolve_api_base_url(None, Some("ftp://legacy.example.com")).unwrap_err();
        assert!(matches!(err, ProviderError::Other(_)));
    }

    #[test]
    fn metadata_matches_descriptor() {
        let provider = MetaProvider::new();
        assert_eq!(provider.id(), ProviderId::Meta);
        assert_eq!(provider.metadata().display_name, "Meta");
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
