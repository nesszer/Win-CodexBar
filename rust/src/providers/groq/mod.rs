//! GroqCloud provider implementation.
//!
//! Auto tries the console session first ([`console`]: spend, requests and
//! tokens from console.groq.com), then falls back to Enterprise Prometheus
//! metrics from Groq's metrics API
//! (`https://api.groq.com/v1/metrics/prometheus/api/v1/query`) with an API key
//! when there is no usable session. Standard (non-Enterprise) keys get HTTP
//! 404 there, which is reported as a plan requirement instead of a raw status.

mod console;
#[cfg(test)]
mod console_tests;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::{Client, Url};
use serde::Deserialize;

use console::{ConsoleEndpoints, ConsoleError, ConsoleSession};

use crate::core::{
    FetchContext, Provider, ProviderError, ProviderFetchResult, ProviderId, ProviderMetadata,
    RateWindow, SourceMode, UsageSnapshot,
};

const GROQ_API_BASE: &str = "https://api.groq.com/v1";
const GROQ_METRICS_QUERY_PATH: [&str; 5] = ["metrics", "prometheus", "api", "v1", "query"];
const GROQ_ENTERPRISE_REQUIRED: &str = "Groq usage metrics require a Groq Enterprise plan. \
     The Prometheus metrics API returned 404 Not Found for this API key.";
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
                dashboard_url: Some("https://console.groq.com/dashboard/usage"),
                status_page_url: Some("https://status.groq.com"),
                tertiary_label_key: None,
            },
            client: crate::core::credentialed_http_client_builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }

    async fn fetch_api(&self, base: &Url, api_key: &str) -> Result<UsageSnapshot, ProviderError> {
        let endpoint = metrics_query_url(base)?;
        let metrics = GroqMetrics {
            request_rate_per_second: self
                .query_scalar(
                    &endpoint,
                    api_key,
                    "sum(model_project_id_status_code:requests:rate5m)",
                )
                .await?,
            input_token_rate_per_second: self
                .query_scalar(&endpoint, api_key, "sum(model_project_id:tokens_in:rate5m)")
                .await?,
            output_token_rate_per_second: self
                .query_scalar(
                    &endpoint,
                    api_key,
                    "sum(model_project_id:tokens_out:rate5m)",
                )
                .await?,
            prompt_cache_hit_rate_per_second: self
                .query_scalar(
                    &endpoint,
                    api_key,
                    "sum(model_project_id:prompt_cache_hits:rate5m)",
                )
                .await?,
        };

        Ok(snapshot_from_metrics(&metrics))
    }

    async fn query_scalar(
        &self,
        endpoint: &Url,
        api_key: &str,
        query: &str,
    ) -> Result<f64, ProviderError> {
        let mut url = endpoint.clone();
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
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            // Groq serves Prometheus metrics only to Enterprise organizations;
            // standard keys get 404 here (console.groq.com/docs/prometheus-metrics).
            return Err(ProviderError::Other(GROQ_ENTERPRISE_REQUIRED.to_string()));
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
        let endpoints = ConsoleEndpoints::from_env(api_base_url());
        self.fetch_routed(
            ctx,
            &endpoints,
            &|name| std::env::var(name).ok(),
            Utc::now(),
        )
        .await
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::Web, SourceMode::OAuth]
    }

    fn supports_web(&self) -> bool {
        true
    }

    fn cookie_source_scopes_session_only(&self) -> bool {
        true
    }

    /// The API-key source must not read browser cookies, so the provider
    /// imports the console session only when it will use it.
    fn owns_browser_cookie_resolution(&self) -> bool {
        true
    }
}

impl GroqProvider {
    /// Web uses only the console session and OAuth (the API-key source) only
    /// Prometheus metrics. Auto tries the console first and falls back to
    /// metrics when there is no usable session and an API key is configured.
    async fn fetch_routed(
        &self,
        ctx: &FetchContext,
        endpoints: &ConsoleEndpoints,
        env: &(dyn Fn(&str) -> Option<String> + Sync),
        now: DateTime<Utc>,
    ) -> Result<ProviderFetchResult, ProviderError> {
        match ctx.source_mode {
            SourceMode::Web => Ok(self.fetch_console(ctx, endpoints, env, now).await?),
            SourceMode::OAuth => {
                let api_key = metrics_api_key(ctx)?;
                self.fetch_metrics(&endpoints.api_base, &api_key).await
            }
            SourceMode::Auto => match self.fetch_console(ctx, endpoints, env, now).await {
                Ok(result) => Ok(result),
                Err(error) if error.allows_metrics_fallback() => {
                    let Ok(api_key) = metrics_api_key(ctx) else {
                        return Err(error.into());
                    };
                    tracing::debug!("No usable Groq console session; using Prometheus metrics");
                    self.fetch_metrics(&endpoints.api_base, &api_key).await
                }
                Err(error) => Err(error.into()),
            },
            SourceMode::Cli => Err(ProviderError::UnsupportedSource(ctx.source_mode)),
        }
    }

    async fn fetch_console(
        &self,
        ctx: &FetchContext,
        endpoints: &ConsoleEndpoints,
        env: &(dyn Fn(&str) -> Option<String> + Sync),
        now: DateTime<Utc>,
    ) -> Result<ProviderFetchResult, ConsoleError> {
        let session = console_session(ctx, env).ok_or(ConsoleError::MissingSession)?;
        console::fetch_usage(&self.client, endpoints, &session, now).await
    }

    async fn fetch_metrics(
        &self,
        base: &Url,
        api_key: &str,
    ) -> Result<ProviderFetchResult, ProviderError> {
        Ok(ProviderFetchResult::new(
            self.fetch_api(base, api_key).await?,
            "api",
        ))
    }
}

/// The session in upstream's order: the environment override, then the
/// manual cookie, then a browser session unless the cookie source rules the
/// browser out.
fn console_session(
    ctx: &FetchContext,
    env: &(dyn Fn(&str) -> Option<String> + Sync),
) -> Option<ConsoleSession> {
    if let Some(session) = ConsoleSession::from_env(env) {
        return Some(session);
    }
    if let Some(header) = ctx.manual_cookie_header.as_deref() {
        return ConsoleSession::from_cookie_header(header);
    }
    if ctx.manual_cookie_missing {
        return None;
    }
    match crate::providers::browser_cookie_header(&console::COOKIE_DOMAINS) {
        Ok(header) => ConsoleSession::from_cookie_header(&header),
        Err(error) => {
            tracing::debug!(%error, "Groq console browser session is unavailable");
            None
        }
    }
}

fn metrics_api_key(ctx: &FetchContext) -> Result<String, ProviderError> {
    resolve_api_key(
        ctx.api_key.as_deref(),
        GROQ_CREDENTIAL_TARGET,
        &["GROQ_API_KEY"],
    )
}

fn api_base_url() -> Url {
    std::env::var("GROQ_API_URL")
        .ok()
        .and_then(|raw| crate::providers::validated_https_url(&raw, "Groq API").ok())
        .map(migrate_legacy_api_base)
        .unwrap_or_else(|| Url::parse(GROQ_API_BASE).expect("static Groq URL is valid"))
}

/// Before 0.70.0 the default base was Groq's OpenAI-compatible root
/// (`https://api.groq.com/openai/v1`), which has no metrics route. A
/// `GROQ_API_URL` still set to that value is rewritten to the metrics base;
/// other hosts (gateways, proxies) are left as configured.
fn migrate_legacy_api_base(url: Url) -> Url {
    let is_groq_host = url
        .host_str()
        .is_some_and(|host| host.eq_ignore_ascii_case("api.groq.com"));
    let path = url.path().trim_end_matches('/');
    if !is_groq_host || !path.eq_ignore_ascii_case("/openai/v1") {
        return url;
    }
    tracing::warn!(
        "GROQ_API_URL uses the legacy OpenAI-compatible Groq base; using {GROQ_API_BASE}"
    );
    let mut migrated = url;
    migrated.set_path("/v1");
    migrated
}

/// Append the Prometheus query path to the API base as path segments.
/// `Url::join` would drop the base's last segment when it has no trailing
/// slash (`.../v1` + `metrics/...` -> `.../metrics/...`), which is how the
/// request used to miss Groq's documented `/v1/metrics/prometheus` route.
fn metrics_query_url(base: &Url) -> Result<Url, ProviderError> {
    let mut url = base.clone();
    url.set_query(None);
    url.set_fragment(None);
    url.path_segments_mut()
        .map_err(|()| ProviderError::Other("Invalid Groq metrics URL".to_string()))?
        .pop_if_empty()
        .extend(GROQ_METRICS_QUERY_PATH);
    Ok(url)
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
    fn metrics_url_keeps_the_documented_v1_prefix() {
        let default = metrics_query_url(&Url::parse(GROQ_API_BASE).unwrap()).unwrap();
        assert_eq!(
            default.as_str(),
            "https://api.groq.com/v1/metrics/prometheus/api/v1/query"
        );
        for base in [
            "https://gateway.example.test/groq/v1",
            "https://gateway.example.test/groq/v1/",
            "https://gateway.example.test/groq/v1?ignored=1#frag",
        ] {
            assert_eq!(
                metrics_query_url(&Url::parse(base).unwrap())
                    .unwrap()
                    .as_str(),
                "https://gateway.example.test/groq/v1/metrics/prometheus/api/v1/query",
                "{base}"
            );
        }
    }

    #[test]
    fn legacy_openai_compatible_base_migrates_to_the_metrics_base() {
        for legacy in [
            "https://api.groq.com/openai/v1",
            "https://api.groq.com/openai/v1/",
            "https://API.GROQ.COM/openai/v1",
        ] {
            let base = migrate_legacy_api_base(Url::parse(legacy).unwrap());
            assert_eq!(
                metrics_query_url(&base).unwrap().as_str(),
                "https://api.groq.com/v1/metrics/prometheus/api/v1/query",
                "{legacy}"
            );
        }
        for kept in [
            "https://api.groq.com/v1",
            "https://gateway.example.test/openai/v1",
            "https://api.groq.com/openai/v2",
        ] {
            let url = Url::parse(kept).unwrap();
            assert_eq!(migrate_legacy_api_base(url.clone()), url, "{kept}");
        }
    }

    const KEY: &str = "gsk_test_key";

    async fn fetch_against(server: &mockito::ServerGuard) -> Result<UsageSnapshot, ProviderError> {
        let base = Url::parse(&format!("{}/v1", server.url())).unwrap();
        GroqProvider::new().fetch_api(&base, KEY).await
    }

    #[tokio::test]
    async fn queries_the_documented_prometheus_route() {
        let mut server = mockito::Server::new_async().await;
        let mut mocks = Vec::new();
        for (query, value) in [
            ("sum(model_project_id_status_code:requests:rate5m)", "2"),
            ("sum(model_project_id:tokens_in:rate5m)", "10"),
            ("sum(model_project_id:tokens_out:rate5m)", "5"),
            ("sum(model_project_id:prompt_cache_hits:rate5m)", "0.5"),
        ] {
            let body = format!(
                r#"{{"status":"success","data":{{"resultType":"vector","result":[{{"metric":{{}},"value":[1710000000,"{value}"]}}]}}}}"#
            );
            mocks.push(
                server
                    .mock("GET", "/v1/metrics/prometheus/api/v1/query")
                    .match_query(mockito::Matcher::UrlEncoded("query".into(), query.into()))
                    .match_header("authorization", format!("Bearer {KEY}").as_str())
                    .with_status(200)
                    .with_header("content-type", "application/json")
                    .with_body(body)
                    .expect(1)
                    .create_async()
                    .await,
            );
        }

        let snapshot = fetch_against(&server).await.unwrap();

        for mock in &mocks {
            mock.assert_async().await;
        }
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

    #[tokio::test]
    async fn non_enterprise_404_explains_the_plan_requirement() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("GET", "/v1/metrics/prometheus/api/v1/query")
            .match_query(mockito::Matcher::Any)
            .with_status(404)
            .with_body(
                r#"{"error":{"message":"Unknown request URL","type":"invalid_request_error"}}"#,
            )
            .create_async()
            .await;

        let error = fetch_against(&server).await.unwrap_err();

        let ProviderError::Other(message) = &error else {
            panic!("expected a plan message, got {error:?}");
        };
        assert!(message.contains("Enterprise plan"), "{message}");
        assert!(message.contains("404"), "{message}");
        assert!(!message.contains("Unknown request URL"), "{message}");
    }

    #[tokio::test]
    async fn unauthorized_key_still_asks_for_authentication() {
        for status in [401, 403] {
            let mut server = mockito::Server::new_async().await;
            let _mock = server
                .mock("GET", "/v1/metrics/prometheus/api/v1/query")
                .match_query(mockito::Matcher::Any)
                .with_status(status)
                .create_async()
                .await;

            let error = fetch_against(&server).await.unwrap_err();

            assert!(matches!(error, ProviderError::AuthRequired), "{status}");
        }
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
