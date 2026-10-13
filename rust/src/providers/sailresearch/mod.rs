//! Sail Research organization credit balance and spend provider.
//!
//! Fetches organization billing data from `GET https://api.sailresearch.com/v2/usage/summary?range=30d`
//! with bearer authentication.
//!
//! Ported from upstream CodexBar v0.74.1 (`Resources/Plugins/sailresearch.ts`).

use async_trait::async_trait;
use reqwest::{Client, StatusCode};
use serde_json::Value;

use super::format;
use crate::core::{
    FetchContext, Provider, ProviderDisplayDetail, ProviderError, ProviderFetchResult, ProviderId,
    ProviderMetadata, ProviderStateKind, RateWindow, SourceMode, UsageSnapshot,
};
use crate::providers::{BoundedBodyError, read_bounded_response};

const USAGE_URL: &str = "https://api.sailresearch.com/v2/usage/summary?range=30d";
const CREDENTIAL_TARGET: &str = "codexbar-sailresearch";
pub const API_KEY_ENV: &str = "SAIL_API_KEY";
const MAX_RESPONSE_BYTES: usize = 256 * 1024;
const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

pub struct SailResearchProvider {
    metadata: ProviderMetadata,
    client: Option<Client>,
}

impl SailResearchProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: ProviderId::SailResearch,
                display_name: "Sail Research",
                session_label: "Balance",
                weekly_label: "Spend",
                supports_opus: false,
                supports_credits: false,
                default_enabled: false,
                is_primary: false,
                dashboard_url: Some("https://app.sailresearch.com"),
                status_page_url: None,
                tertiary_label_key: None,
            },
            client: crate::core::credentialed_http_client_builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(REQUEST_TIMEOUT)
                .build()
                .ok(),
        }
    }

    async fn fetch_usage_impl(
        &self,
        ctx: &FetchContext,
    ) -> Result<ProviderFetchResult, ProviderError> {
        let client = self.client.as_ref().ok_or_else(|| {
            ProviderError::Other("Could not create a secure Sail Research HTTP client.".into())
        })?;
        let key = crate::providers::resolve_api_key(
            ctx.api_key.as_deref(),
            CREDENTIAL_TARGET,
            &[API_KEY_ENV],
        )?;

        let response = client.get(USAGE_URL).bearer_auth(&key).send().await?;

        let status = response.status();
        let message = format!("Sail Research returned HTTP {}.", status.as_u16());

        match status {
            StatusCode::OK => {}
            StatusCode::UNAUTHORIZED => {
                return Err(ProviderError::Other(message));
            }
            StatusCode::PAYMENT_REQUIRED => {
                return Err(ProviderError::Other(
                    "Sail Research disabled this API key because credits are exhausted.".into(),
                ));
            }
            StatusCode::FORBIDDEN => {
                return Err(ProviderError::Other(
                    "Sail Research API key has no organization access.".into(),
                ));
            }
            _ => {
                return Err(ProviderError::Other(message));
            }
        }

        let body = read_bounded_response(response, MAX_RESPONSE_BYTES)
            .await
            .map_err(|error| match error {
                BoundedBodyError::TooLarge => ProviderError::Other(
                    "Sail Research returned an unrecognized billing summary.".into(),
                ),
                BoundedBodyError::Read(error) => ProviderError::from(error),
            })?;

        let data: Value = serde_json::from_slice(&body).map_err(|_| {
            ProviderError::Other("Sail Research returned an unrecognized billing summary.".into())
        })?;

        parse_usage(&data)
    }
}

impl Default for SailResearchProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for SailResearchProvider {
    fn id(&self) -> ProviderId {
        ProviderId::SailResearch
    }

    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        match ctx.source_mode {
            SourceMode::Auto | SourceMode::OAuth => self.fetch_usage_impl(ctx).await,
            SourceMode::Web | SourceMode::Cli => {
                Err(ProviderError::UnsupportedSource(ctx.source_mode))
            }
        }
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::OAuth]
    }

    fn error_state_kind(&self, error: &ProviderError) -> ProviderStateKind {
        match error {
            ProviderError::Other(message) if message == KEY_REJECTED => {
                ProviderStateKind::ExpiredSession
            }
            _ => error.state_kind(),
        }
    }
}

/// Upstream reports HTTP 401 as `authenticationExpired` with this message.
const KEY_REJECTED: &str = "Sail Research returned HTTP 401.";

fn parse_usage(payload: &Value) -> Result<ProviderFetchResult, ProviderError> {
    let data = payload.as_object().ok_or_else(|| {
        ProviderError::Other("Sail Research returned an unrecognized billing summary.".into())
    })?;

    // Validate required fields in order
    if data.get("object").and_then(Value::as_str) != Some("usage.summary") {
        return Err(ProviderError::Other(
            "Sail Research returned an unrecognized billing summary.".into(),
        ));
    }

    let available = data
        .get("available")
        .and_then(Value::as_bool)
        .ok_or_else(|| {
            ProviderError::Other("Sail Research returned an unrecognized billing summary.".into())
        })?;

    if !available {
        return Err(ProviderError::Other(
            "Sail Research billing data is unavailable.".into(),
        ));
    }

    let has_metronome_customer = data
        .get("has_metronome_customer")
        .and_then(Value::as_bool)
        .ok_or_else(|| {
            ProviderError::Other("Sail Research returned an unrecognized billing summary.".into())
        })?;

    if !has_metronome_customer {
        return Err(ProviderError::Other(
            "Sail Research organization has no billing account.".into(),
        ));
    }

    let balance_unavailable = data
        .get("balance_unavailable")
        .and_then(Value::as_bool)
        .ok_or_else(|| {
            ProviderError::Other("Sail Research returned an unrecognized billing summary.".into())
        })?;

    // Helper to extract and convert fractional cents to dollars
    fn dollars(data: &serde_json::Map<String, Value>, key: &str) -> Result<f64, ProviderError> {
        let value = data.get(key).and_then(Value::as_f64).ok_or_else(|| {
            ProviderError::Other("Sail Research returned an unrecognized billing summary.".into())
        })?;

        if !value.is_finite() {
            return Err(ProviderError::Other(
                "Sail Research returned an unrecognized billing summary.".into(),
            ));
        }

        Ok(value / 100.0)
    }

    let spend = dollars(data, "period_spend")?;
    if spend < 0.0 {
        return Err(ProviderError::Other(
            "Sail Research returned an unrecognized billing summary.".into(),
        ));
    }

    // Map effective_range to label
    let range_labels: std::collections::HashMap<&str, &str> = [
        ("1h", "Last hour spend"),
        ("6h", "Last 6 hours spend"),
        ("24h", "Last 24 hours spend"),
        ("7d", "Last 7 days spend"),
        ("30d", "Last 30 days spend"),
        ("period", "Billing-period spend"),
    ]
    .iter()
    .copied()
    .collect();

    let effective_range = data
        .get("effective_range")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            ProviderError::Other("Sail Research returned an unrecognized billing summary.".into())
        })?;

    let range_label = range_labels.get(effective_range).copied().ok_or_else(|| {
        ProviderError::Other("Sail Research returned an unrecognized billing summary.".into())
    })?;

    let balance_value = if balance_unavailable {
        "Unavailable".to_string()
    } else {
        format::usd(dollars(data, "balance")?)
    };

    let mut result = ProviderFetchResult::new(
        UsageSnapshot::new(
            RateWindow::informational(format!("Credit balance: {balance_value}")).hidden_on_card(),
        )
        .with_login_method("API key"),
        "api",
    );

    // Credit balance row
    result = result.with_display_detail(
        ProviderDisplayDetail::new(
            "sailresearch-credit-balance",
            "Credit balance",
            balance_value,
        )
        .and_then(|d| d.with_section_title("Organization billing")),
    );

    // Spend row
    result = result.with_display_detail(
        ProviderDisplayDetail::new("sailresearch-period-spend", range_label, format::usd(spend))
            .and_then(|d| d.with_section_title("Organization billing")),
    );

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture() -> Value {
        json!({
            "object": "usage.summary",
            "available": true,
            "has_metronome_customer": true,
            "balance_unavailable": false,
            "balance": 12345.67,
            "period_spend": 1234,
            "effective_range": "30d",
        })
    }

    fn rows(result: &ProviderFetchResult) -> Vec<(String, String)> {
        result
            .display_details
            .iter()
            .map(|d| {
                assert_eq!(d.section_title(), Some("Organization billing"));
                (d.title().to_string(), d.value().to_string())
            })
            .collect()
    }

    #[test]
    fn fractional_cents_become_organization_usd() {
        let result = parse_usage(&fixture()).unwrap();
        assert_eq!(
            rows(&result),
            vec![
                ("Credit balance".to_string(), "$123.46".to_string()),
                ("Last 30 days spend".to_string(), "$12.34".to_string()),
            ]
        );
        assert_eq!(result.usage.login_method.as_deref(), Some("API key"));
        assert_eq!(
            result.usage.primary.reset_description.as_deref(),
            Some("Credit balance: $123.46")
        );
    }

    #[test]
    fn plan_limited_history_uses_effective_range() {
        let mut data = fixture();
        data["effective_range"] = json!("7d");
        let result = parse_usage(&data).unwrap();
        let row_labels: Vec<String> = rows(&result).iter().map(|(l, _)| l.clone()).collect();
        assert!(row_labels.contains(&"Last 7 days spend".to_string()));
    }

    #[test]
    fn zero_and_negative_balances_remain_authoritative() {
        for balance in &[0.0, -125.0] {
            let mut data = fixture();
            data["balance"] = json!(balance);
            data["period_spend"] = json!(0);
            let result = parse_usage(&data).unwrap();
            let row_values: Vec<String> = rows(&result).iter().map(|(_, v)| v.clone()).collect();

            let expected = if *balance == 0.0 { "$0.00" } else { "-$1.25" };
            assert_eq!(row_values[0], expected);
            assert_eq!(row_values[1], "$0.00");
        }
    }

    #[test]
    fn unavailable_balance_preserves_spend() {
        let mut data = fixture();
        data["balance_unavailable"] = json!(true);
        data.as_object_mut().unwrap().remove("balance");
        let result = parse_usage(&data).unwrap();
        let row_values: Vec<String> = rows(&result).iter().map(|(_, v)| v.clone()).collect();
        assert_eq!(row_values[0], "Unavailable");
        assert_eq!(row_values[1], "$12.34");
    }

    #[test]
    fn missing_billing_data_fails_closed() {
        let test_cases = vec![("available", false), ("has_metronome_customer", false)];

        for (field, value) in test_cases {
            let mut data = fixture();
            data[field] = json!(value);
            assert!(parse_usage(&data).is_err());
        }
    }

    #[test]
    fn malformed_monetary_values_fail_closed() {
        let malformed_values = vec![json!("100"), json!(true), json!(null), json!(["value"])];

        for value in malformed_values {
            for key in &["balance", "period_spend"] {
                let mut data = fixture();
                data[key] = value.clone();
                assert!(
                    parse_usage(&data).is_err(),
                    "Should fail for {}={}",
                    key,
                    value
                );
            }
        }
    }

    #[test]
    fn missing_required_fields_fail_closed() {
        let required_fields = vec![
            "object",
            "available",
            "has_metronome_customer",
            "balance_unavailable",
            "balance",
            "period_spend",
            "effective_range",
        ];

        for field in required_fields {
            let mut data = fixture();
            data.as_object_mut().unwrap().remove(field);
            assert!(
                parse_usage(&data).is_err(),
                "Should fail when {} is missing",
                field
            );
        }
    }

    #[test]
    fn negative_spend_fails_closed() {
        let mut data = fixture();
        data["period_spend"] = json!(-1);
        assert!(parse_usage(&data).is_err());
    }

    #[test]
    fn unknown_range_fails_closed() {
        let mut data = fixture();
        data["effective_range"] = json!("unknown");
        assert!(parse_usage(&data).is_err());
    }

    #[test]
    fn only_a_rejected_key_reports_an_expired_session() {
        let provider = SailResearchProvider::new();
        let kind = |message: &str| provider.error_state_kind(&ProviderError::Other(message.into()));
        assert_eq!(
            kind("Sail Research returned HTTP 401."),
            ProviderStateKind::ExpiredSession
        );
        for message in [
            "Sail Research returned HTTP 429.",
            "Sail Research API key has no organization access.",
            "Sail Research disabled this API key because credits are exhausted.",
        ] {
            assert_eq!(kind(message), ProviderStateKind::Unknown, "{message}");
        }
    }
}
