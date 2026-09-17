use async_trait::async_trait;
use reqwest::{Client, Url};
use serde_json::Value;

use crate::core::{
    CostSnapshot, FetchContext, Provider, ProviderError, ProviderFetchResult, ProviderId,
    ProviderMetadata, RateWindow, SourceMode, UsageSnapshot,
};

const CREDENTIAL_TARGET: &str = "codexbar-devin";
const BASE_URLS: [&str; 2] = ["https://api.devin.ai", "https://app.devin.ai/api"];
const MISSING_ORGANIZATION_DETAIL: &str = "No organizations found for auth1 user";
const MISSING_ORGANIZATION_MESSAGE: &str = "Devin organization context is missing. Set the organization in provider extras or DEVIN_ORG, then refresh.";

pub struct DevinProvider {
    metadata: ProviderMetadata,
    client: Client,
}

impl DevinProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: ProviderId::Devin,
                display_name: "Devin",
                session_label: "Daily",
                weekly_label: "Weekly",
                supports_opus: false,
                supports_credits: true,
                default_enabled: false,
                is_primary: false,
                dashboard_url: Some("https://app.devin.ai/settings/billing"),
                status_page_url: None,
            },
            client: crate::core::credentialed_http_client_builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }
}

impl Default for DevinProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for DevinProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Devin
    }

    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        match ctx.source_mode {
            SourceMode::Auto | SourceMode::OAuth => {
                let token = crate::providers::resolve_api_key(
                    ctx.api_key.as_deref(),
                    CREDENTIAL_TARGET,
                    &["DEVIN_BEARER_TOKEN", "DEVIN_API_KEY"],
                )?;
                let env_org = std::env::var("DEVIN_ORG").ok();
                let raw_org = ctx
                    .workspace_id
                    .as_deref()
                    .or(env_org.as_deref())
                    .ok_or_else(|| {
                        ProviderError::NotInstalled(
                            "Devin organization not found. Set it in provider extras or DEVIN_ORG."
                                .into(),
                        )
                    })?;
                let org = normalized_org(raw_org);
                fetch_quota(&self.client, &token, &org, devin_urls(&org)?).await
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

async fn fetch_quota(
    client: &Client,
    token: &str,
    org: &str,
    urls: impl IntoIterator<Item = Url>,
) -> Result<ProviderFetchResult, ProviderError> {
    let mut last_non_auth_error: Option<ProviderError> = None;
    let mut candidate_count = 0usize;
    let mut auth_failures = 0usize;
    for url in urls {
        candidate_count += 1;
        let response = match client
            .get(url)
            .bearer_auth(token)
            // Auth1 sessions resolve their organization context
            // from this header; without it the gateway answers 401
            // "No organizations found for auth1 user" even for a
            // valid session token.
            .header("x-cog-org-id", org)
            .header("Accept", "application/json")
            .send()
            .await
        {
            Ok(response) => response,
            Err(error) => {
                last_non_auth_error = Some(ProviderError::Network(error));
                continue;
            }
        };
        let status = response.status();
        if status.is_success() {
            let body = match response.bytes().await {
                Ok(body) => body,
                Err(error) => {
                    last_non_auth_error = Some(ProviderError::Network(error));
                    continue;
                }
            };
            let value: Value = match serde_json::from_slice(&body) {
                Ok(value) => value,
                Err(error) => {
                    last_non_auth_error = Some(ProviderError::Parse(format!(
                        "Failed to parse Devin quota: {error}"
                    )));
                    continue;
                }
            };
            match fetch_result_from_quota(&value, org) {
                Ok(result) => return Ok(result),
                Err(error) => {
                    last_non_auth_error = Some(error);
                    continue;
                }
            }
        }
        let body = match response.bytes().await {
            Ok(body) => body,
            Err(error) => {
                last_non_auth_error = Some(ProviderError::Network(error));
                continue;
            }
        };
        // Web-session tokens (auth1_) are rejected on the API host
        // but work on the web host, and vice versa for service
        // keys, so every candidate is tried before giving up.
        if let Some(error) = auth_response_error(status, &body) {
            if matches!(error, ProviderError::AuthRequired) {
                auth_failures += 1;
            } else {
                last_non_auth_error = Some(error);
            }
        } else {
            last_non_auth_error = Some(ProviderError::Other(format!(
                "Devin quota returned status {}",
                status
            )));
        }
    }
    if candidate_count > 0 && auth_failures == candidate_count {
        Err(ProviderError::AuthRequired)
    } else {
        Err(last_non_auth_error
            .unwrap_or_else(|| ProviderError::Other("Devin quota request failed".into())))
    }
}

fn devin_urls(org: &str) -> Result<Vec<Url>, ProviderError> {
    BASE_URLS
        .iter()
        .map(|base| Url::parse(&format!("{base}/{org}/billing/quota/usage")))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| ProviderError::Other(format!("Invalid Devin quota URL: {e}")))
}

fn auth_response_error(status: reqwest::StatusCode, body: &[u8]) -> Option<ProviderError> {
    if status != reqwest::StatusCode::UNAUTHORIZED && status != reqwest::StatusCode::FORBIDDEN {
        return None;
    }

    let is_missing_organization = serde_json::from_slice::<Value>(body)
        .ok()
        .is_some_and(|value| {
            value.get("detail").and_then(Value::as_str) == Some(MISSING_ORGANIZATION_DETAIL)
        });
    if is_missing_organization {
        Some(ProviderError::Other(
            MISSING_ORGANIZATION_MESSAGE.to_string(),
        ))
    } else {
        Some(ProviderError::AuthRequired)
    }
}

fn normalized_org(raw: &str) -> String {
    // Both hosts serve the quota at /{org}/billing/quota/usage with the bare
    // organization id (org_...); a prefixed path 404s server-side.
    let trimmed = raw.trim().trim_matches('/');
    trimmed
        .strip_prefix("organizations/")
        .or_else(|| trimmed.strip_prefix("org/"))
        .unwrap_or(trimmed)
        .to_string()
}

fn snapshot_from_quota(value: &Value, org: &str) -> Result<UsageSnapshot, ProviderError> {
    let daily = percent(value, &["daily_percentage", "dailyPercentage"])
        .or_else(|| percent(value, &["used_percent", "usedPercent"]))
        .ok_or_else(|| {
            ProviderError::Parse("Devin quota response missing daily usage".to_string())
        })?;
    let mut snapshot =
        UsageSnapshot::new(RateWindow::new(daily)).with_organization(org.to_string());
    if let Some(weekly) = percent(value, &["weekly_percentage", "weeklyPercentage"]) {
        snapshot = snapshot.with_secondary(RateWindow::new(weekly));
    }
    Ok(snapshot)
}

fn fetch_result_from_quota(value: &Value, org: &str) -> Result<ProviderFetchResult, ProviderError> {
    let mut result = ProviderFetchResult::new(snapshot_from_quota(value, org)?, "api");
    if let Some(balance) = extra_usage_balance(value) {
        result = result.with_cost(CostSnapshot::new(balance, "USD", "Extra usage balance"));
    }
    Ok(result)
}

fn percent(value: &Value, keys: &[&str]) -> Option<f64> {
    for key in keys {
        if let Some(v) = value.get(*key).and_then(Value::as_f64) {
            return Some(if v < 1.0 { v * 100.0 } else { v });
        }
    }
    let used = ["used", "usage", "used_count", "usedCount", "consumed"]
        .iter()
        .find_map(|k| value.get(*k).and_then(Value::as_f64));
    let limit = ["limit", "quota", "total", "max", "available"]
        .iter()
        .find_map(|k| value.get(*k).and_then(Value::as_f64));
    match (used, limit) {
        (Some(used), Some(limit)) if limit > 0.0 => Some(used / limit * 100.0),
        _ => None,
    }
}

fn extra_usage_balance(value: &Value) -> Option<f64> {
    let dollars = [
        "overage_balance",
        "overageBalance",
        "extra_usage_balance",
        "extraUsageBalance",
    ]
    .iter()
    .find_map(|key| value.get(*key).and_then(Value::as_f64))
    .filter(|value| value.is_finite() && *value >= 0.0);
    dollars.or_else(|| {
        ["overage_balance_cents", "overageBalanceCents"]
            .iter()
            .find_map(|key| value.get(*key).and_then(Value::as_f64))
            .filter(|value| value.is_finite() && *value >= 0.0)
            .map(|cents| cents / 100.0)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fraction_percent() {
        let snapshot =
            snapshot_from_quota(&serde_json::json!({"daily_percentage":0.25}), "org/demo")
                .expect("daily usage");
        assert_eq!(snapshot.primary.used_percent, 25.0);
    }

    #[test]
    fn parses_exact_one_as_one_percent() {
        let snapshot =
            snapshot_from_quota(&serde_json::json!({"daily_percentage":1.0}), "org/demo")
                .expect("daily usage");
        assert_eq!(snapshot.primary.used_percent, 1.0);
    }

    #[test]
    fn parses_extra_usage_balance() {
        let result = fetch_result_from_quota(
            &serde_json::json!({"daily_percentage": 0.2, "overage_balance": 12.34}),
            "org/demo",
        )
        .expect("daily usage");

        let cost = result.cost.unwrap();
        assert_eq!(cost.used, 12.34);
        assert_eq!(cost.period, "Extra usage balance");
    }

    #[test]
    fn parses_extra_usage_balance_cents() {
        let result = fetch_result_from_quota(
            &serde_json::json!({"daily_percentage": 0.2, "overage_balance_cents": 7087}),
            "org/demo",
        )
        .expect("daily usage");

        assert_eq!(result.cost.unwrap().used, 70.87);
    }

    #[test]
    fn identifies_missing_organization_without_exposing_response_body() {
        let body = br#"{"detail":"No organizations found for auth1 user","trace":"private-trace","token":"Bearer sk-private-fixture"}"#;

        for status in [
            reqwest::StatusCode::UNAUTHORIZED,
            reqwest::StatusCode::FORBIDDEN,
        ] {
            let error = auth_response_error(status, body).expect("authorization error");
            assert!(matches!(error, ProviderError::Other(_)));
            assert_eq!(error.to_string(), MISSING_ORGANIZATION_MESSAGE);
            assert!(!error.to_string().contains("private-trace"));
            assert!(!error.to_string().contains("sk-private-fixture"));
        }
    }

    #[test]
    fn keeps_unrelated_authorization_failures_as_auth_required() {
        for body in [
            br#"{"detail":"Unauthorized","trace":"private-trace"}"#.as_slice(),
            br#"{"detail":"Token expired","trace":"private-trace"}"#.as_slice(),
            br#"{"detail":"No organizations found for another user"}"#.as_slice(),
            b"not-json".as_slice(),
        ] {
            let error = auth_response_error(reqwest::StatusCode::UNAUTHORIZED, body)
                .expect("authorization error");
            assert!(matches!(error, ProviderError::AuthRequired));
            assert_eq!(error.to_string(), "Authentication required");
        }
    }

    #[test]
    fn ignores_organization_detail_on_non_authorization_responses() {
        let body = br#"{"detail":"No organizations found for auth1 user"}"#;
        assert!(auth_response_error(reqwest::StatusCode::NOT_FOUND, body).is_none());
    }

    #[test]
    fn normalized_org_strips_known_prefixes() {
        assert_eq!(normalized_org("org_TJ2demo"), "org_TJ2demo");
        assert_eq!(normalized_org(" org_TJ2demo/ "), "org_TJ2demo");
        assert_eq!(normalized_org("org/org_TJ2demo"), "org_TJ2demo");
        assert_eq!(normalized_org("organizations/org_TJ2demo"), "org_TJ2demo");
    }

    #[test]
    fn devin_urls_use_bare_org_on_both_hosts() {
        let org = normalized_org("org/org_TJ2demo");
        let urls = devin_urls(&org).expect("candidate urls");
        assert_eq!(
            urls.iter().map(|u| u.as_str()).collect::<Vec<_>>(),
            vec![
                "https://api.devin.ai/org_TJ2demo/billing/quota/usage",
                "https://app.devin.ai/api/org_TJ2demo/billing/quota/usage",
            ]
        );
    }

    async fn quota_mock(status: usize, body: &str) -> (mockito::ServerGuard, Url) {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/org_TJ2demo/billing/quota/usage")
            .match_header("x-cog-org-id", "org_TJ2demo")
            .with_status(status)
            .with_body(body)
            .create_async()
            .await;
        let url = Url::parse(&format!("{}/org_TJ2demo/billing/quota/usage", server.url()))
            .expect("the mock server URL should be valid");
        (server, url)
    }

    fn test_client() -> Client {
        Client::builder()
            .no_proxy()
            .build()
            .expect("the test client should build")
    }

    #[tokio::test]
    async fn retries_the_next_quota_url_after_auth_failure() {
        let (_first_server, first_url) = quota_mock(401, r#"{"detail":"Unauthorized"}"#).await;
        let (_second_server, second_url) = quota_mock(200, r#"{"daily_percentage":0.25}"#).await;

        let result = fetch_quota(
            &test_client(),
            "test-token",
            "org_TJ2demo",
            [first_url, second_url],
        )
        .await
        .expect("the second host should succeed");

        assert_eq!(result.usage.primary.used_percent, 25.0);
    }

    #[tokio::test]
    async fn normalized_organization_is_sent_in_quota_request() {
        let (_server, url) = quota_mock(200, r#"{"daily_percentage":0.25}"#).await;
        let org = normalized_org("organizations/org_TJ2demo");

        let result = fetch_quota(&test_client(), "test-token", &org, [url])
            .await
            .expect("the normalized organization should authenticate");

        assert_eq!(result.usage.primary.used_percent, 25.0);
    }

    #[tokio::test]
    async fn mixed_auth_and_server_failures_do_not_become_auth_required() {
        let (_first_server, first_url) = quota_mock(401, r#"{"detail":"Unauthorized"}"#).await;
        let (_second_server, second_url) = quota_mock(500, "server failure").await;

        let error = fetch_quota(
            &test_client(),
            "test-token",
            "org_TJ2demo",
            [first_url, second_url],
        )
        .await
        .expect_err("mixed failures should return the non-auth failure");

        assert!(matches!(error, ProviderError::Other(message) if message.contains("500")));
    }

    #[tokio::test]
    async fn server_failure_followed_by_auth_failure_preserves_server_failure() {
        let (_first_server, first_url) = quota_mock(500, "server failure").await;
        let (_second_server, second_url) = quota_mock(401, r#"{"detail":"Unauthorized"}"#).await;

        let error = fetch_quota(
            &test_client(),
            "test-token",
            "org_TJ2demo",
            [first_url, second_url],
        )
        .await
        .expect_err("mixed failures should return the non-auth failure");

        assert!(matches!(error, ProviderError::Other(message) if message.contains("500")));
    }

    #[tokio::test]
    async fn transport_failure_followed_by_auth_failure_is_not_auth_required() {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0))
            .expect("a local ephemeral port should be available");
        let refused_port = listener
            .local_addr()
            .expect("the local listener should expose its address")
            .port();
        drop(listener);
        let (_second_server, second_url) = quota_mock(401, r#"{"detail":"Unauthorized"}"#).await;
        let refused_url = Url::parse(&format!(
            "http://127.0.0.1:{refused_port}/org_TJ2demo/billing/quota/usage"
        ))
        .expect("the refused URL should be valid");

        let error = fetch_quota(
            &test_client(),
            "test-token",
            "org_TJ2demo",
            [refused_url, second_url],
        )
        .await
        .expect_err("a transport failure must not be hidden as auth");

        assert!(matches!(error, ProviderError::Network(_)));
    }

    #[tokio::test]
    async fn invalid_json_success_followed_by_valid_json_retries() {
        let (_first_server, first_url) = quota_mock(200, "not-json").await;
        let (_second_server, second_url) = quota_mock(200, r#"{"daily_percentage":0.25}"#).await;

        let result = fetch_quota(
            &test_client(),
            "test-token",
            "org_TJ2demo",
            [first_url, second_url],
        )
        .await
        .expect("the second host should provide valid JSON");

        assert_eq!(result.usage.primary.used_percent, 25.0);
    }

    #[tokio::test]
    async fn unrecognized_success_schema_followed_by_valid_json_retries() {
        let (_first_server, first_url) = quota_mock(200, r#"{"status":"ok"}"#).await;
        let (_second_server, second_url) = quota_mock(200, r#"{"daily_percentage":0.25}"#).await;

        let result = fetch_quota(
            &test_client(),
            "test-token",
            "org_TJ2demo",
            [first_url, second_url],
        )
        .await
        .expect("the second host should provide a recognized schema");

        assert_eq!(result.usage.primary.used_percent, 25.0);
    }

    #[tokio::test]
    async fn all_unrecognized_success_schemas_return_parse_error() {
        let (_first_server, first_url) = quota_mock(200, r#"{"status":"ok"}"#).await;
        let (_second_server, second_url) = quota_mock(200, r#"{"status":"still-ok"}"#).await;

        let error = fetch_quota(
            &test_client(),
            "test-token",
            "org_TJ2demo",
            [first_url, second_url],
        )
        .await
        .expect_err("unrecognized success schemas should remain a parse error");

        assert!(matches!(
            error,
            ProviderError::Parse(message)
                if message == "Devin quota response missing daily usage"
        ));
    }

    #[tokio::test]
    async fn all_auth_failures_return_auth_required() {
        let (_first_server, first_url) = quota_mock(401, r#"{"detail":"Unauthorized"}"#).await;
        let (_second_server, second_url) = quota_mock(403, r#"{"detail":"Forbidden"}"#).await;

        let error = fetch_quota(
            &test_client(),
            "test-token",
            "org_TJ2demo",
            [first_url, second_url],
        )
        .await
        .expect_err("all credential failures should remain authentication errors");

        assert!(matches!(error, ProviderError::AuthRequired));
    }

    #[tokio::test]
    async fn retries_the_next_quota_url_after_a_transport_failure() {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0))
            .expect("a local ephemeral port should be available");
        let refused_port = listener
            .local_addr()
            .expect("the local listener should expose its address")
            .port();
        drop(listener);

        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/org_TJ2demo/billing/quota/usage")
            .match_header("x-cog-org-id", "org_TJ2demo")
            .with_status(200)
            .with_body(r#"{"daily_percentage":0.25}"#)
            .create_async()
            .await;
        let fallback_url = Url::parse(&format!("{}/org_TJ2demo/billing/quota/usage", server.url()))
            .expect("the mock server URL should be valid");
        let refused_url = Url::parse(&format!(
            "http://127.0.0.1:{refused_port}/org_TJ2demo/billing/quota/usage"
        ))
        .expect("the refused URL should be valid");
        let client = Client::builder()
            .no_proxy()
            .build()
            .expect("the test client should build");

        let result = fetch_quota(
            &client,
            "test-token",
            "org_TJ2demo",
            [refused_url, fallback_url],
        )
        .await
        .expect("the fallback URL should succeed");

        assert_eq!(result.usage.primary.used_percent, 25.0);
        mock.assert_async().await;
    }
}
