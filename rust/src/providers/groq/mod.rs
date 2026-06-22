//! GroqCloud provider implementation.
//!
//! Fetches Enterprise Prometheus metrics from Groq's metrics API.

use async_trait::async_trait;
use reqwest::{Client, Url};
use serde::Deserialize;

use crate::core::{
    FetchContext, Provider, ProviderError, ProviderFetchResult, ProviderId, ProviderMetadata,
    RateWindow, SourceMode, UsageSnapshot,
};

const GROQ_API_BASE: &str = "https://api.groq.com/openai/v1";
const GROQ_CREDENTIAL_TARGET: &str = "codexbar-groq";

#[derive(Debug, Deserialize)]
struct PrometheusResponse {
    status: String,
    data: Option<PrometheusPayload>,
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PrometheusPayload {
    #[serde(default)]
    result: Vec<PrometheusSeries>,
}

#[derive(Debug, Deserialize)]
struct PrometheusSeries {
    value: Option<Vec<PrometheusValue>>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum PrometheusValue {
    Number(f64),
    String(String),
}

impl PrometheusValue {
    fn as_f64(&self) -> Option<f64> {
        match self {
            PrometheusValue::Number(value) => Some(*value),
            PrometheusValue::String(value) => value.parse::<f64>().ok(),
        }
    }
}

#[derive(Debug, Clone)]
struct GroqMetrics {
    request_rate_per_second: f64,
    input_token_rate_per_second: f64,
    output_token_rate_per_second: f64,
    prompt_cache_hit_rate_per_second: f64,
}

pub struct GroqProvider {
    metadata: ProviderMetadata,
    client: Client,
}

impl GroqProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: ProviderId::Groq,
                display_name: "Groq",
                session_label: "Requests",
                weekly_label: "Tokens",
                supports_opus: false,
                supports_credits: true,
                default_enabled: false,
                is_primary: false,
                dashboard_url: Some("https://console.groq.com/settings/metrics"),
                status_page_url: Some("https://status.groq.com"),
            },
            client: crate::core::credentialed_http_client_builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }

    async fn fetch_api(&self, api_key: &str) -> Result<UsageSnapshot, ProviderError> {
        let metrics = GroqMetrics {
            request_rate_per_second: self
                .query_scalar(api_key, "sum(model_project_id_status_code:requests:rate5m)")
                .await?,
            input_token_rate_per_second: self
                .query_scalar(api_key, "sum(model_project_id:tokens_in:rate5m)")
                .await?,
            output_token_rate_per_second: self
                .query_scalar(api_key, "sum(model_project_id:tokens_out:rate5m)")
                .await?,
            prompt_cache_hit_rate_per_second: self
                .query_scalar(api_key, "sum(model_project_id:prompt_cache_hits:rate5m)")
                .await?,
        };

        Ok(snapshot_from_metrics(&metrics))
    }

    async fn query_scalar(&self, api_key: &str, query: &str) -> Result<f64, ProviderError> {
        let base = api_base_url()
            .join("metrics/prometheus/api/v1/query")
            .map_err(|e| ProviderError::Other(format!("Invalid Groq metrics URL: {e}")))?;
        let mut url = base;
        url.query_pairs_mut().append_pair("query", query);

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
                "Groq metrics API returned status {}",
                response.status()
            )));
        }

        let body = response
            .bytes()
            .await
            .map_err(|e| ProviderError::Parse(format!("Failed to read Groq metrics: {e}")))?;
        parse_scalar(&body)
    }
}

impl Default for GroqProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for GroqProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Groq
    }

    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        match ctx.source_mode {
            SourceMode::Auto | SourceMode::OAuth => {
                let api_key = resolve_api_key(
                    ctx.api_key.as_deref(),
                    GROQ_CREDENTIAL_TARGET,
                    &["GROQ_API_KEY"],
                )?;
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

fn api_base_url() -> Url {
    std::env::var("GROQ_API_URL")
        .ok()
        .and_then(|raw| crate::providers::validated_https_url(&raw, "Groq API").ok())
        .unwrap_or_else(|| Url::parse(GROQ_API_BASE).expect("static Groq URL is valid"))
}

fn parse_scalar(data: &[u8]) -> Result<f64, ProviderError> {
    let decoded: PrometheusResponse = serde_json::from_slice(data)
        .map_err(|e| ProviderError::Parse(format!("Failed to parse Groq metrics: {e}")))?;
    if decoded.status != "success" {
        return Err(ProviderError::Other(
            decoded
                .error
                .unwrap_or_else(|| "Groq metrics query failed.".to_string()),
        ));
    }
    Ok(decoded
        .data
        .map(|payload| {
            payload
                .result
                .iter()
                .filter_map(|series| series.value.as_ref())
                .filter_map(|value| value.last())
                .filter_map(PrometheusValue::as_f64)
                .sum()
        })
        .unwrap_or(0.0))
}

fn snapshot_from_metrics(metrics: &GroqMetrics) -> UsageSnapshot {
    let requests_per_minute = metrics.request_rate_per_second * 60.0;
    let tokens_per_minute =
        (metrics.input_token_rate_per_second + metrics.output_token_rate_per_second) * 60.0;
    let cache_hits_per_minute = metrics.prompt_cache_hit_rate_per_second * 60.0;

    let mut primary = RateWindow::with_details(
        0.0,
        Some(5),
        None,
        Some(format!("{} req/min", format_metric(requests_per_minute))),
    );
    primary.used_percent = 0.0;

    let secondary = RateWindow::with_details(
        0.0,
        Some(5),
        None,
        Some(format!("{} tok/min", format_metric(tokens_per_minute))),
    );

    let tertiary = RateWindow::with_details(
        0.0,
        Some(5),
        None,
        Some(format!(
            "{} cache/min",
            format_metric(cache_hits_per_minute)
        )),
    );

    UsageSnapshot::new(primary)
        .with_secondary(secondary)
        .with_tertiary(tertiary)
        .with_login_method("Prometheus metrics")
}

fn format_metric(value: f64) -> String {
    if value >= 100.0 {
        format!("{value:.0}")
    } else if value >= 10.0 {
        format!("{value:.1}")
    } else {
        format!("{value:.2}")
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
    fn parses_prometheus_scalar_strings() {
        let value = parse_scalar(
            br#"{
                "status": "success",
                "data": {
                    "result": [
                        {"value": [1710000000, "1.5"]},
                        {"value": [1710000000, 2.25]}
                    ]
                }
            }"#,
        )
        .unwrap();

        assert_eq!(value, 3.75);
    }

    #[test]
    fn snapshot_formats_minute_rates() {
        let snapshot = snapshot_from_metrics(&GroqMetrics {
            request_rate_per_second: 2.0,
            input_token_rate_per_second: 10.0,
            output_token_rate_per_second: 5.0,
            prompt_cache_hit_rate_per_second: 0.5,
        });

        assert_eq!(
            snapshot.primary.reset_description.as_deref(),
            Some("120 req/min")
        );
        assert_eq!(
            snapshot
                .secondary
                .as_ref()
                .and_then(|w| w.reset_description.as_deref()),
            Some("900 tok/min")
        );
    }
}
