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
                let org = ctx
                    .workspace_id
                    .as_deref()
                    .or(env_org.as_deref())
                    .ok_or_else(|| {
                        ProviderError::NotInstalled(
                            "Devin organization not found. Set it in provider extras or DEVIN_ORG."
                                .into(),
                        )
                    })?
                    .to_string();
                let mut last_error: Option<ProviderError> = None;
                let mut auth_error: Option<ProviderError> = None;
                for url in devin_urls(&org)? {
                    let response = self
                        .client
                        .get(url)
                        .bearer_auth(&token)
                        .header("Accept", "application/json")
                        .send()
                        .await?;
                    let status = response.status();
                    if status.is_success() {
                        let value: Value = response.json().await.map_err(|e| {
                            ProviderError::Parse(format!("Failed to parse Devin quota: {e}"))
                        })?;
                        return Ok(fetch_result_from_quota(&value, &org));
                    }
                    let body = response.bytes().await.unwrap_or_default();
                    // Web-session tokens (auth1_) are rejected on the API host
                    // but work on the web host, and vice versa for service
                    // keys, so every candidate is tried before giving up.
                    if let Some(error) = auth_response_error(status, &body) {
                        auth_error = auth_error.or(Some(error));
                    } else {
                        last_error = Some(ProviderError::Other(format!(
                            "Devin quota returned status {}",
                            status
                        )));
                    }
                }
                Err(auth_error
                    .or(last_error)
                    .unwrap_or_else(|| ProviderError::Other("Devin quota request failed".into())))
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

fn devin_urls(org: &str) -> Result<Vec<Url>, ProviderError> {
    let org = normalized_org(org);
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

fn snapshot_from_quota(value: &Value, org: &str) -> UsageSnapshot {
    let daily = percent(value, &["daily_percentage", "dailyPercentage"])
        .unwrap_or_else(|| percent(value, &["used_percent", "usedPercent"]).unwrap_or(0.0));
    let mut snapshot =
        UsageSnapshot::new(RateWindow::new(daily)).with_organization(org.to_string());
    if let Some(weekly) = percent(value, &["weekly_percentage", "weeklyPercentage"]) {
        snapshot = snapshot.with_secondary(RateWindow::new(weekly));
    }
    snapshot
}

fn fetch_result_from_quota(value: &Value, org: &str) -> ProviderFetchResult {
    let mut result = ProviderFetchResult::new(snapshot_from_quota(value, org), "api");
    if let Some(balance) = extra_usage_balance(value) {
        result = result.with_cost(CostSnapshot::new(balance, "USD", "Extra usage balance"));
    }
    result
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
            snapshot_from_quota(&serde_json::json!({"daily_percentage":0.25}), "org/demo");
        assert_eq!(snapshot.primary.used_percent, 25.0);
    }

    #[test]
    fn parses_exact_one_as_one_percent() {
        let snapshot =
            snapshot_from_quota(&serde_json::json!({"daily_percentage":1.0}), "org/demo");
        assert_eq!(snapshot.primary.used_percent, 1.0);
    }

    #[test]
    fn parses_extra_usage_balance() {
        let result = fetch_result_from_quota(
            &serde_json::json!({"daily_percentage": 0.2, "overage_balance": 12.34}),
            "org/demo",
        );

        let cost = result.cost.unwrap();
        assert_eq!(cost.used, 12.34);
        assert_eq!(cost.period, "Extra usage balance");
    }

    #[test]
    fn parses_extra_usage_balance_cents() {
        let result = fetch_result_from_quota(
            &serde_json::json!({"daily_percentage": 0.2, "overage_balance_cents": 7087}),
            "org/demo",
        );

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
        let urls = devin_urls("org/org_TJ2demo").expect("candidate urls");
        assert_eq!(
            urls.iter().map(|u| u.as_str()).collect::<Vec<_>>(),
            vec![
                "https://api.devin.ai/org_TJ2demo/billing/quota/usage",
                "https://app.devin.ai/api/org_TJ2demo/billing/quota/usage",
            ]
        );
    }
}
