//! v0 billing and API rate-limit provider.

use async_trait::async_trait;
use chrono::{TimeZone, Utc};
use reqwest::{Client, StatusCode, Url};
use serde_json::Value;
use std::time::Duration;

use crate::core::{
    FetchContext, Provider, ProviderDisplayDetail, ProviderError, ProviderFetchResult, ProviderId,
    RateWindow, SourceMode, UsageSnapshot,
};
use crate::providers::{BoundedBodyError, read_bounded_response};

const API_BASE: &str = "https://api.v0.dev/v1";
const CREDENTIAL_TARGET: &str = "codexbar-v0";
const ENV_KEYS: &[&str] = &["V0_API_KEY"];
const MAX_RESPONSE_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq)]
struct Quota {
    used_percent: Option<f64>,
    resets_at: Option<chrono::DateTime<Utc>>,
    remaining: Option<f64>,
    limit: f64,
}

#[derive(Debug, Clone, PartialEq)]
struct Billing {
    quota: Quota,
    billing_type: String,
    on_demand_balance: Option<f64>,
}

pub struct V0Provider {
    client: Client,
}

impl V0Provider {
    pub fn new() -> Self {
        Self {
            client: crate::core::credentialed_http_client_builder()
                .timeout(Duration::from_secs(15))
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }

    async fn fetch_api(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        let api_key =
            crate::providers::resolve_api_key(ctx.api_key.as_deref(), CREDENTIAL_TARGET, ENV_KEYS)?;
        let scope = ctx
            .workspace_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .or_else(|| {
                std::env::var("V0_SCOPE")
                    .ok()
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty())
            });
        let (billing_result, rate_limit_result) = tokio::join!(
            self.get_json("/user/billing", scope.as_deref(), &api_key),
            self.get_json("/rate-limits", scope.as_deref(), &api_key),
        );
        let billing = parse_billing(&billing_result?)?;
        let rate_limit = parse_quota(&rate_limit_result?, "rate limit response")?;
        Ok(build_result(billing, rate_limit, scope.as_deref()))
    }

    async fn get_json(
        &self,
        path: &str,
        scope: Option<&str>,
        api_key: &str,
    ) -> Result<Value, ProviderError> {
        let mut url = Url::parse(&format!("{API_BASE}{path}"))
            .map_err(|_| ProviderError::Other("Invalid v0 API URL.".into()))?;
        if let Some(scope) = scope {
            url.query_pairs_mut().append_pair("scope", scope);
        }
        let response = self.client.get(url).bearer_auth(api_key).send().await?;
        classify_status(response.status())?;
        let body = read_bounded_response(response, MAX_RESPONSE_BYTES)
            .await
            .map_err(|error| match error {
                BoundedBodyError::TooLarge => parse_failure(format!("{path} response too large")),
                BoundedBodyError::Read(_) => parse_failure(format!("{path} returned invalid JSON")),
            })?;
        serde_json::from_slice(&body)
            .map_err(|_| parse_failure(format!("{path} returned invalid JSON")))
    }
}

impl Default for V0Provider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for V0Provider {
    fn id(&self) -> ProviderId {
        ProviderId::V0
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        match ctx.source_mode {
            SourceMode::Auto | SourceMode::OAuth => self.fetch_api(ctx).await,
            SourceMode::Web | SourceMode::Cli => {
                Err(ProviderError::UnsupportedSource(ctx.source_mode))
            }
        }
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::OAuth]
    }
}

fn classify_status(status: StatusCode) -> Result<(), ProviderError> {
    match status {
        status if status.is_success() => Ok(()),
        StatusCode::UNAUTHORIZED => Err(ProviderError::AuthRequired),
        StatusCode::FORBIDDEN => Err(ProviderError::Other(
            "v0 denied access to this scope.".into(),
        )),
        StatusCode::TOO_MANY_REQUESTS => {
            Err(ProviderError::Other("v0 API rate limit reached.".into()))
        }
        status if status.is_server_error() => Err(ProviderError::Other(format!(
            "v0 API returned HTTP {status}."
        ))),
        status => Err(ProviderError::Other(format!(
            "v0 API returned HTTP {status}."
        ))),
    }
}

fn parse_billing(value: &Value) -> Result<Billing, ProviderError> {
    let object = value
        .as_object()
        .ok_or_else(|| parse_failure("billing response"))?;
    let billing_type = text(object.get("billingType"), "billingType")?
        .ok_or_else(|| parse_failure("billingType"))?;
    let data = object
        .get("data")
        .ok_or_else(|| parse_failure("billing.data"))?;
    match billing_type.as_str() {
        "legacy" => Ok(Billing {
            quota: parse_quota(data, "billing.data")?,
            billing_type,
            on_demand_balance: None,
        }),
        "token" => {
            let data = data
                .as_object()
                .ok_or_else(|| parse_failure("billing.data"))?;
            let balance = data
                .get("balance")
                .and_then(Value::as_object)
                .ok_or_else(|| parse_failure("billing.data.balance"))?;
            let total = nonnegative_number(balance.get("total"), "billing.data.balance.total")?;
            let remaining =
                nonnegative_number(balance.get("remaining"), "billing.data.balance.remaining")?;
            let resets_at = match data.get("billingCycle").and_then(Value::as_object) {
                Some(cycle) => parse_reset(cycle.get("end"))?,
                None => None,
            };
            let on_demand_balance = data
                .get("onDemand")
                .filter(|value| !value.is_null())
                .and_then(Value::as_object)
                .map(|on_demand| {
                    nonnegative_number(on_demand.get("balance"), "billing.data.onDemand.balance")
                })
                .transpose()?;
            Ok(Billing {
                quota: Quota {
                    used_percent: percent(total - remaining, total),
                    resets_at,
                    remaining: Some(remaining),
                    limit: total,
                },
                billing_type,
                on_demand_balance,
            })
        }
        _ => Err(parse_failure("billingType")),
    }
}

fn parse_quota(value: &Value, field: &str) -> Result<Quota, ProviderError> {
    let object = value.as_object().ok_or_else(|| parse_failure(field))?;
    let limit = nonnegative_number(object.get("limit"), &format!("{field}.limit"))?;
    let remaining =
        optional_nonnegative_number(object.get("remaining"), &format!("{field}.remaining"))?;
    Ok(Quota {
        used_percent: remaining.and_then(|remaining| percent(limit - remaining, limit)),
        resets_at: parse_reset(object.get("reset"))?,
        remaining,
        limit,
    })
}

fn build_result(billing: Billing, rate_limit: Quota, scope: Option<&str>) -> ProviderFetchResult {
    let primary = billing.quota.used_percent.map_or_else(
        || RateWindow::informational("Billing usage unavailable"),
        |used| RateWindow::with_details(used, None, billing.quota.resets_at, None),
    );
    let mut usage = UsageSnapshot::new(primary).with_login_method("API key");
    if let Some(used) = rate_limit.used_percent {
        usage = usage.with_secondary(RateWindow::with_details(
            used,
            None,
            rate_limit.resets_at,
            None,
        ));
    }
    let mut result = ProviderFetchResult::new(usage, "api");
    let remaining = billing
        .quota
        .remaining
        .map_or_else(|| "Unavailable".into(), format_number);
    result = result.with_display_detail(ProviderDisplayDetail::new(
        "billing-remaining",
        "Billing remaining",
        format!("{remaining} of {}", format_number(billing.quota.limit)),
    ));
    if let Some(balance) = billing.on_demand_balance {
        result = result.with_display_detail(ProviderDisplayDetail::new(
            "on-demand",
            "On-demand balance",
            format_number(balance),
        ));
    }
    let rate_remaining = rate_limit
        .remaining
        .map_or_else(|| "Unavailable".into(), format_number);
    result = result.with_display_detail(ProviderDisplayDetail::new(
        "rate-limit-remaining",
        "Rate-limit remaining",
        format!("{rate_remaining} of {}", format_number(rate_limit.limit)),
    ));
    result = result.with_display_detail(ProviderDisplayDetail::new(
        "billing-type",
        "Billing type",
        billing.billing_type,
    ));
    if let Some(scope) = scope {
        result = result.with_display_detail(ProviderDisplayDetail::new(
            "scope",
            "Scope",
            scope.chars().take(120).collect::<String>(),
        ));
    }
    result
}

fn finite_number(value: Option<&Value>, field: &str) -> Result<f64, ProviderError> {
    value
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
        .ok_or_else(|| parse_failure(field))
}

fn nonnegative_number(value: Option<&Value>, field: &str) -> Result<f64, ProviderError> {
    let value = finite_number(value, field)?;
    if value < 0.0 {
        return Err(parse_failure(format!("{field} must not be negative")));
    }
    Ok(value)
}

fn optional_nonnegative_number(
    value: Option<&Value>,
    field: &str,
) -> Result<Option<f64>, ProviderError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(value) => nonnegative_number(Some(value), field).map(Some),
    }
}

fn text(value: Option<&Value>, field: &str) -> Result<Option<String>, ProviderError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => {
            Ok((!value.trim().is_empty()).then(|| value.trim().to_string()))
        }
        Some(_) => Err(parse_failure(field)),
    }
}

fn parse_reset(value: Option<&Value>) -> Result<Option<chrono::DateTime<Utc>>, ProviderError> {
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return Ok(None);
    };
    let raw = finite_number(Some(value), "reset")?;
    if raw <= 0.0 {
        return Ok(None);
    }
    if raw.fract() != 0.0 {
        return Err(parse_failure(
            "reset must use integral seconds or milliseconds",
        ));
    }
    let raw = format!("{raw:.0}")
        .parse::<i64>()
        .map_err(|_| parse_failure("reset"))?;
    let (seconds, nanos) = if raw >= 1_000_000_000_000 {
        (raw / 1000, ((raw % 1000) as u32) * 1_000_000)
    } else {
        (raw, 0)
    };
    Ok(Utc.timestamp_opt(seconds, nanos).single())
}

fn percent(used: f64, limit: f64) -> Option<f64> {
    (limit > 0.0 && used.is_finite()).then(|| (used.max(0.0) / limit * 100.0).clamp(0.0, 100.0))
}

fn format_number(value: f64) -> String {
    format!("{value:.2}")
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string()
}
fn parse_failure(field: impl AsRef<str>) -> ProviderError {
    ProviderError::Parse(format!("Could not parse v0 usage: {}", field.as_ref()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn dashboard_url_matches_upstream_billing_page() {
        const BILLING: &str = "https://v0.app/settings/billing";
        assert_eq!(V0Provider::new().metadata().dashboard_url, Some(BILLING));
        let configured = crate::settings::get_api_key_providers()
            .into_iter()
            .find(|info| info.id == ProviderId::V0)
            .expect("v0 api key config");
        assert_eq!(configured.dashboard_url, Some(BILLING));
    }

    #[test]
    fn parses_token_billing_and_rate_limit_without_fabricating_windows() {
        let billing = parse_billing(&json!({
            "billingType": "token",
            "data": {"balance": {"total": 100, "remaining": 75}, "billingCycle": {"end": 1_800_000_000}, "onDemand": {"balance": 12.5}}
        })).unwrap();
        assert_eq!(billing.quota.used_percent, Some(25.0));
        assert_eq!(billing.quota.limit, 100.0);
        assert_eq!(billing.on_demand_balance, Some(12.5));
        let unknown = parse_quota(
            &json!({"limit": 50, "remaining": null, "reset": null}),
            "rate",
        )
        .unwrap();
        assert_eq!(unknown.limit, 50.0);
        assert_eq!(unknown.used_percent, None);
        assert_eq!(unknown.remaining, None);
    }

    #[test]
    fn rejects_negative_token_total_and_accepts_zero_limits() {
        assert!(
            parse_billing(&json!({
                "billingType": "token",
                "data": {"balance": {"total": -1, "remaining": 0}}
            }))
            .is_err()
        );

        let zero_billing = parse_billing(&json!({
            "billingType": "token",
            "data": {"balance": {"total": 0, "remaining": 0}}
        }))
        .unwrap();
        assert_eq!(zero_billing.quota.limit, 0.0);

        let zero_quota = parse_quota(&json!({"limit": 0, "remaining": 0}), "rate").unwrap();
        assert_eq!(zero_quota.limit, 0.0);
    }

    #[test]
    fn rejects_unknown_billing_type_and_negative_limits() {
        assert!(parse_billing(&json!({"billingType": "future", "data": {}})).is_err());
        assert!(parse_quota(&json!({"limit": -1}), "rate").is_err());
        assert!(parse_reset(Some(&json!(1e30))).is_err());
    }

    #[test]
    fn rejects_negative_balances_in_every_response_shape() {
        assert!(
            parse_billing(&json!({
                "billingType": "token",
                "data": {"balance": {"total": 100, "remaining": -1}}
            }))
            .is_err()
        );
        assert!(
            parse_billing(&json!({
                "billingType": "token",
                "data": {
                    "balance": {"total": 100, "remaining": 75},
                    "onDemand": {"balance": -1}
                }
            }))
            .is_err()
        );
        assert!(
            parse_billing(&json!({
                "billingType": "legacy",
                "data": {"limit": 100, "remaining": -1}
            }))
            .is_err()
        );
        assert!(parse_quota(&json!({"limit": 100, "remaining": -1}), "rate").is_err());
    }

    #[test]
    fn rejects_fractional_resets_and_preserves_integral_milliseconds() {
        assert!(parse_reset(Some(&json!(1_800_000_000.5))).is_err());
        let reset = parse_reset(Some(&json!(1_800_000_000_500_i64)))
            .unwrap()
            .unwrap();
        assert_eq!(reset.timestamp(), 1_800_000_000);
        assert_eq!(reset.timestamp_subsec_millis(), 500);
    }
}
