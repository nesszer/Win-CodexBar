//! Helmcode Cloud and NaN Builders dashboard quota provider.

use async_trait::async_trait;
use chrono::{DateTime, TimeZone, Utc};
use reqwest::{Client, StatusCode};
use serde_json::Value;
use std::time::Duration;

use crate::core::{
    CostSnapshot, FetchContext, Provider, ProviderError, ProviderFetchResult, ProviderId,
    RateWindow, SourceMode, UsageSnapshot,
};
use crate::providers::{BoundedBodyError, read_bounded_response};

const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tenant {
    Helmcode,
    NanBuilders,
}

impl Tenant {
    fn domain(self) -> &'static str {
        match self {
            Self::Helmcode => "helmcode.com",
            Self::NanBuilders => "nan.builders",
        }
    }
    fn name(self) -> &'static str {
        match self {
            Self::Helmcode => "Helmcode Cloud",
            Self::NanBuilders => "NaN Builders",
        }
    }
    fn api(self) -> String {
        format!("https://cloud-api.{}", self.domain())
    }
    fn origin(self) -> String {
        format!("https://cloud.{}", self.domain())
    }
}

#[derive(Debug, Clone, PartialEq)]
struct ModelQuota {
    name: String,
    cap: f64,
    used: f64,
    credit: f64,
    window_hours: Option<u32>,
    resets_at: Option<DateTime<Utc>>,
}

pub struct HelmcodeProvider {
    client: Client,
    #[cfg(test)]
    api_base_override: Option<String>,
}

impl HelmcodeProvider {
    pub fn new() -> Self {
        Self {
            client: crate::core::credentialed_http_client_builder()
                .timeout(Duration::from_secs(10))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .unwrap_or_else(|_| Client::new()),
            #[cfg(test)]
            api_base_override: None,
        }
    }

    fn api_base(&self, tenant: Tenant) -> String {
        #[cfg(test)]
        if let Some(api_base) = &self.api_base_override {
            return api_base.trim_end_matches('/').to_string();
        }
        tenant.api()
    }

    async fn fetch_web(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        let manual_tenant = ctx
            .workspace_id
            .as_deref()
            .is_some_and(|value| value.eq_ignore_ascii_case("nanBuilders"));
        let candidates = if ctx.manual_cookie_header.is_some() {
            vec![if manual_tenant {
                Tenant::NanBuilders
            } else {
                Tenant::Helmcode
            }]
        } else {
            vec![Tenant::Helmcode, Tenant::NanBuilders]
        };
        let mut rejected = false;
        for tenant in candidates {
            let cookie = match ctx.manual_cookie_header.as_deref() {
                Some(raw) => crate::providers::normalize_cookie_header(raw)
                    .ok_or(ProviderError::NoCookies)?,
                None => match crate::providers::browser_cookie_header(&[tenant.domain()]) {
                    Ok(header) => header,
                    Err(_) => continue,
                },
            };
            match self.fetch_tenant(tenant, &cookie).await {
                Ok(result) => return Ok(result),
                Err(ProviderError::AuthRequired) => rejected = true,
                Err(error) => return Err(error),
            }
        }
        if rejected {
            Err(ProviderError::AuthRequired)
        } else {
            Err(ProviderError::NotInstalled(
                "Sign in to cloud.helmcode.com or cloud.nan.builders, or paste a Cookie header."
                    .into(),
            ))
        }
    }

    async fn fetch_tenant(
        &self,
        tenant: Tenant,
        cookie: &str,
    ) -> Result<ProviderFetchResult, ProviderError> {
        let quota_request = self.get(tenant, cookie, "/api/usage/quota", false);
        let billing_request = self.get(tenant, cookie, "/api/billing", true);
        let (quota_result, billing_result) = tokio::join!(quota_request, billing_request);

        let quota = quota_result?.ok_or_else(|| parse_failure("quota"))?;
        let billing = billing_result?;
        let premium = billing
            .as_ref()
            .and_then(|value| value.get("subscription"))
            .and_then(|value| value.get("premium"))
            .and_then(Value::as_bool)
            == Some(true);
        let mut models = parse_models(&quota, premium)?;
        models.sort_by(|a, b| {
            (b.used / b.cap)
                .partial_cmp(&(a.used / a.cap))
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.name.cmp(&b.name))
        });
        let credits = if tenant == Tenant::Helmcode {
            self.get(tenant, cookie, "/api/billing/credits", true)
                .await?
        } else {
            None
        };
        let primary = models
            .first()
            .map(model_window)
            .unwrap_or_else(|| RateWindow::informational("No active model quota"));
        let mut usage = UsageSnapshot::new(primary)
            .with_organization(tenant.name())
            .with_login_method("Dashboard session");
        for model in models.iter().skip(1) {
            usage = usage.with_extra_rate_window(
                format!("helmcode-{}", model.name),
                model.name.clone(),
                model_window(model),
            );
        }
        let mut result = ProviderFetchResult::new(usage, "web");
        if let Some(cost) = credits.as_ref().and_then(credits_cost) {
            result = result.with_cost(cost);
        }
        Ok(result)
    }

    async fn get(
        &self,
        tenant: Tenant,
        cookie: &str,
        path: &str,
        optional: bool,
    ) -> Result<Option<Value>, ProviderError> {
        let request_timeout = Duration::from_secs(if optional { 2 } else { 8 });
        let response = match self
            .client
            .get(format!("{}{path}", self.api_base(tenant)))
            .header("Cookie", cookie)
            .header("Origin", tenant.origin())
            .header("Referer", format!("{}/dashboard", tenant.origin()))
            .timeout(request_timeout)
            .send()
            .await
        {
            Ok(response) => response,
            Err(_) if optional => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let status = response.status();
        if optional && status != StatusCode::OK {
            return Ok(None);
        }
        if status == StatusCode::UNAUTHORIZED
            || status == StatusCode::FORBIDDEN
            || status.is_redirection()
        {
            return Err(ProviderError::AuthRequired);
        }
        if status == StatusCode::TOO_MANY_REQUESTS {
            return Err(ProviderError::Other("Helmcode rate limit reached.".into()));
        }
        if status == StatusCode::REQUEST_TIMEOUT || status.is_server_error() {
            return Err(ProviderError::Other(
                "Helmcode dashboard is unavailable.".into(),
            ));
        }
        if !status.is_success() {
            return Err(ProviderError::Other(format!(
                "Helmcode dashboard returned HTTP {status}."
            )));
        }
        let body = match read_bounded_response(response, MAX_RESPONSE_BYTES).await {
            Ok(body) => body,
            Err(_) if optional => return Ok(None),
            Err(error) => {
                return Err(parse_failure(match error {
                    BoundedBodyError::TooLarge => "response too large",
                    BoundedBodyError::Read(_) => "invalid JSON",
                }));
            }
        };
        let value: Value = match serde_json::from_slice(&body) {
            Ok(value) => value,
            Err(_) if optional => return Ok(None),
            Err(_) => return Err(parse_failure("invalid JSON")),
        };
        if optional && !value.is_object() {
            return Ok(None);
        }
        Ok(Some(value))
    }
}

impl Default for HelmcodeProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for HelmcodeProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Helmcode
    }
    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        match ctx.source_mode {
            SourceMode::Auto | SourceMode::Web => self.fetch_web(ctx).await,
            SourceMode::OAuth | SourceMode::Cli => {
                Err(ProviderError::UnsupportedSource(ctx.source_mode))
            }
        }
    }
    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::Web]
    }
    fn supports_web(&self) -> bool {
        true
    }
    fn owns_browser_cookie_resolution(&self) -> bool {
        true
    }
}

fn parse_models(quota: &Value, premium: bool) -> Result<Vec<ModelQuota>, ProviderError> {
    let object = quota
        .as_object()
        .ok_or_else(|| parse_failure("quota object"))?;
    let period_start = object
        .get("periodStart")
        .and_then(Value::as_str)
        .ok_or_else(|| parse_failure("periodStart"))?;
    let fallback = monthly_reset_fallback(period_start);
    let models = object
        .get("models")
        .and_then(Value::as_array)
        .ok_or_else(|| parse_failure("models"))?;
    models
        .iter()
        .map(|value| {
            let row = value.as_object().ok_or_else(|| parse_failure("model"))?;
            let name = row
                .get("model")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|name| {
                    !name.is_empty()
                        && name.chars().count() <= 120
                        && !name.chars().any(char::is_control)
                })
                .ok_or_else(|| parse_failure("model name"))?
                .to_string();
            let cap = nonnegative(row.get("cap"), "cap")?;
            let used = nonnegative(row.get("tokensUsed"), "tokensUsed")?;
            let credit =
                optional_nonnegative(row.get("creditTokens"), "creditTokens")?.unwrap_or(0.0);
            let window_hours = optional_nonnegative(row.get("windowHours"), "windowHours")?
                .map(|value| format!("{value:.0}").parse::<u32>())
                .transpose()
                .map_err(|_| parse_failure("windowHours"))?;
            if window_hours.is_some_and(|hours| hours == 0 || hours > 8_760) {
                return Err(parse_failure("windowHours"));
            }
            let resets_at = row
                .get("periodEnd")
                .and_then(Value::as_str)
                .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
                .map(|date| date.with_timezone(&Utc))
                .or(fallback);
            Ok(ModelQuota {
                name,
                cap,
                used,
                credit,
                window_hours,
                resets_at,
            })
        })
        .filter_map(|result| match result {
            Ok(model) if model.cap > 0.0 && (model.window_hours.is_none() || premium) => {
                Some(Ok(model))
            }
            Ok(_) => None,
            Err(error) => Some(Err(error)),
        })
        .collect()
}

fn model_window(model: &ModelQuota) -> RateWindow {
    let mut window = RateWindow::with_details(
        (model.used / model.cap * 100.0).clamp(0.0, 100.0),
        model.window_hours.map(|hours| hours * 60),
        model.resets_at,
        None,
    );
    window.reset_description = Some(format!(
        "{} · {:.0} / {:.0} tokens{}",
        model.name,
        model.used,
        model.cap,
        if model.credit > 0.0 {
            format!(" · {:.0} credit-funded", model.credit)
        } else {
            String::new()
        }
    ));
    window
}

fn nonnegative(value: Option<&Value>, field: &str) -> Result<f64, ProviderError> {
    value
        .and_then(safe_integer_number)
        .filter(|value| *value >= 0.0)
        .ok_or_else(|| parse_failure(field))
}
fn optional_nonnegative(value: Option<&Value>, field: &str) -> Result<Option<f64>, ProviderError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(value) => nonnegative(Some(value), field).map(Some),
    }
}
fn safe_integer_number(value: &Value) -> Option<f64> {
    value.as_f64().filter(|number| {
        number.is_finite() && number.fract() == 0.0 && number.abs() <= MAX_SAFE_INTEGER
    })
}

fn credits_cost(credits: &Value) -> Option<CostSnapshot> {
    let balance_micros = safe_integer_number(credits.get("balanceMicros")?)?;
    let currency = match credits.get("currency") {
        None | Some(Value::Null) => "EUR".to_string(),
        Some(Value::String(currency)) => currency.to_ascii_uppercase(),
        Some(_) => return None,
    };
    if currency.len() != 3 || !currency.chars().all(|ch| ch.is_ascii_uppercase()) {
        return None;
    }
    Some(
        CostSnapshot::new(0.0, currency, "Prepaid balance")
            .with_balance(balance_micros.max(0.0) / 1_000_000.0),
    )
}

fn monthly_reset_fallback(period_start: &str) -> Option<DateTime<Utc>> {
    let bytes = period_start.as_bytes();
    if bytes.len() < 10
        || (bytes.len() > 10 && bytes[10] != b'T')
        || bytes[4] != b'-'
        || bytes[7] != b'-'
    {
        return None;
    }
    let year = parse_ascii_digits(&bytes[..4])?;
    let month = parse_ascii_digits(&bytes[5..7])?;
    let day = parse_ascii_digits(&bytes[8..10])?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let (year, month) = if month == 12 {
        (year.checked_add(1)?, 1)
    } else {
        (year, month + 1)
    };
    Utc.with_ymd_and_hms(year, month as u32, 1, 0, 0, 0)
        .single()
}

fn parse_ascii_digits(bytes: &[u8]) -> Option<i32> {
    if bytes.is_empty() || !bytes.iter().all(u8::is_ascii_digit) {
        return None;
    }
    bytes.iter().try_fold(0_i32, |value, digit| {
        value.checked_mul(10)?.checked_add(i32::from(*digit - b'0'))
    })
}

/// Return the tenant dashboard for a Helmcode account organization.
pub fn dashboard_url_for_organization(organization: Option<&str>) -> &'static str {
    if organization == Some("NaN Builders") {
        "https://cloud.nan.builders/dashboard"
    } else {
        "https://cloud.helmcode.com/dashboard"
    }
}
fn parse_failure(field: impl AsRef<str>) -> ProviderError {
    ProviderError::Parse(format!(
        "Helmcode quota response format changed: {}",
        field.as_ref()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const QUOTA_GOLDEN: &str = r#"{"periodStart":"2026-09-01","models":[
        {"model":"helm-monthly","cap":2000000000,"tokensUsed":73854494,"remaining":1926145506,"periodEnd":"2026-10-01T00:00:00Z","updatedAt":"2026-09-05T01:30:34Z"},
        {"model":"helm-rolling-a","cap":3000000000,"tokensUsed":0,"remaining":3000000000,"periodEnd":"2026-10-04T19:25:48Z","windowHours":4,"fullWindowTokens":400000000},
        {"model":"helm-rolling-b","cap":3000000000,"tokensUsed":0,"remaining":3000000000,"periodEnd":"2026-10-04T19:25:48Z","windowHours":4,"fullWindowTokens":400000000},
        {"model":"helm-monthly-b","cap":3000000000,"tokensUsed":0,"remaining":3000000000,"periodEnd":"2026-10-01T00:00:00Z"},
        {"model":"helm-monthly-c","cap":500000000,"tokensUsed":0,"remaining":500000000,"periodEnd":"2026-10-01T00:00:00Z"},
        {"model":"helm-monthly-d","cap":1000000000,"tokensUsed":0,"remaining":1000000000,"periodEnd":"2026-10-01T00:00:00Z"}
    ]}"#;
    const BILLING_FREE: &str = r#"{"subscription":{"status":"active","premium":false,"currency":"eur","currentPeriodStart":1788549948,"currentPeriodEnd":1791141948,"cancelAtPeriodEnd":false,"cancelAt":null,"id":"sub_redacted"},"paymentMethod":null,"address":{}}"#;
    const BILLING_PREMIUM: &str = r#"{"subscription":{"status":"active","premium":true,"currency":"eur","currentPeriodStart":1788549948,"currentPeriodEnd":1791141948,"cancelAtPeriodEnd":false,"cancelAt":null,"id":"sub_redacted"},"paymentMethod":null,"address":{}}"#;
    const CREDITS: &str = r#"{"balanceMicros":12500000,"currency":"eur"}"#;

    fn provider_at(api_base: &str) -> HelmcodeProvider {
        let mut provider = HelmcodeProvider::new();
        provider.api_base_override = Some(api_base.to_string());
        provider.client = Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("test client builds");
        provider
    }

    fn fetch_context(workspace_id: Option<&str>) -> FetchContext {
        FetchContext {
            source_mode: SourceMode::Web,
            manual_cookie_header: Some("session=test-cookie".to_string()),
            workspace_id: workspace_id.map(str::to_string),
            ..FetchContext::default()
        }
    }

    async fn mock_fetch(
        billing_status: usize,
        billing_body: &str,
        credits_status: usize,
        credits_body: &str,
        workspace_id: Option<&str>,
    ) -> Result<ProviderFetchResult, ProviderError> {
        let mut server = mockito::Server::new_async().await;
        let quota = server
            .mock("GET", "/api/usage/quota")
            .with_status(200)
            .with_body(QUOTA_GOLDEN)
            .create_async()
            .await;
        let billing = server
            .mock("GET", "/api/billing")
            .with_status(billing_status)
            .with_body(billing_body)
            .create_async()
            .await;
        let credits = server
            .mock("GET", "/api/billing/credits")
            .with_status(credits_status)
            .with_body(credits_body)
            .expect(
                if workspace_id.is_some_and(|id| id.eq_ignore_ascii_case("nanBuilders")) {
                    0
                } else {
                    1
                },
            )
            .create_async()
            .await;

        let result = provider_at(&server.url())
            .fetch_usage(&fetch_context(workspace_id))
            .await;
        quota.assert_async().await;
        billing.assert_async().await;
        credits.assert_async().await;
        result
    }

    #[test]
    fn dashboard_url_uses_the_account_tenant() {
        assert_eq!(
            dashboard_url_for_organization(Some("NaN Builders")),
            "https://cloud.nan.builders/dashboard"
        );
        assert_eq!(
            dashboard_url_for_organization(None),
            "https://cloud.helmcode.com/dashboard"
        );
        assert_eq!(
            dashboard_url_for_organization(Some("Other")),
            "https://cloud.helmcode.com/dashboard"
        );
    }

    #[test]
    fn parses_cloud_golden_as_free_quota_with_prepaid_balance() {
        let quota: Value = serde_json::from_str(QUOTA_GOLDEN).unwrap();
        let models = parse_models(&quota, false).unwrap();
        let mut models = models;
        models.sort_by(|a, b| {
            (b.used / b.cap)
                .partial_cmp(&(a.used / a.cap))
                .unwrap()
                .then_with(|| a.name.cmp(&b.name))
        });
        let usage =
            UsageSnapshot::new(model_window(&models[0])).with_organization("Helmcode Cloud");
        let primary = &usage.primary;
        assert!((primary.used_percent - 73_854_494.0 / 2_000_000_000.0 * 100.0).abs() < 1e-10);
        assert_eq!(
            primary.resets_at,
            Some(
                DateTime::parse_from_rfc3339("2026-10-01T00:00:00Z")
                    .unwrap()
                    .into()
            )
        );
        assert_eq!(
            usage.account_organization.as_deref(),
            Some("Helmcode Cloud")
        );
        assert_eq!(
            models
                .iter()
                .skip(1)
                .map(|m| m.name.as_str())
                .collect::<Vec<_>>(),
            ["helm-monthly-b", "helm-monthly-c", "helm-monthly-d"]
        );
        let cost = credits_cost(&serde_json::from_str(CREDITS).unwrap()).unwrap();
        assert_eq!(cost.balance, Some(12.5));
        assert_eq!(cost.currency_code, "EUR");
        assert_eq!(cost.period, "Prepaid balance");
    }

    #[tokio::test]
    async fn premium_billing_reveals_rolling_windows_and_requires_boolean_true() {
        let quota: Value = serde_json::from_str(QUOTA_GOLDEN).unwrap();
        let premium = parse_models(&quota, true).unwrap();
        assert_eq!(premium.len(), 6);
        let rolling = premium
            .iter()
            .find(|model| model.name == "helm-rolling-a")
            .unwrap();
        assert_eq!(rolling.window_hours, Some(4));
        assert_eq!(rolling.window_hours.map(|hours| hours * 60), Some(240));
        assert_eq!(
            rolling.resets_at,
            Some(
                DateTime::parse_from_rfc3339("2026-10-04T19:25:48Z")
                    .unwrap()
                    .into()
            )
        );
        assert_eq!(premium[0].window_hours, None);

        for billing in [
            "{}",
            r#"{"subscription":{"premium":"true"}}"#,
            r#"{"subscription":{"premium":1}}"#,
            r#"{"subscription":{"premium":null}}"#,
        ] {
            let result = mock_fetch(200, billing, 200, CREDITS, None)
                .await
                .unwrap_or_else(|error| panic!("billing {billing} failed quota fetch: {error}"));
            assert_eq!(result.usage.extra_rate_windows.len(), 3, "{billing}");
            assert_eq!(result.usage.primary.window_minutes, None, "{billing}");
            for window in &result.usage.extra_rate_windows {
                assert_eq!(window.window.window_minutes, None, "{billing}");
            }
        }
    }

    #[test]
    fn monthly_fallback_is_first_day_of_the_next_month_and_clamps_usage() {
        let quota = json!({"periodStart":"2026-12-15","models":[
            {"model":"helm-unlimited","cap":0,"tokensUsed":100},
            {"model":"helm-a","cap":1000,"tokensUsed":2000,"creditTokens":20}
        ]});
        let models = parse_models(&quota, false).unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].name, "helm-a");
        assert_eq!(model_window(&models[0]).used_percent, 100.0);
        assert_eq!(
            models[0].resets_at,
            Some(
                DateTime::parse_from_rfc3339("2027-01-01T00:00:00Z")
                    .unwrap()
                    .into()
            )
        );
        assert!(
            model_window(&models[0])
                .reset_description
                .unwrap()
                .contains("20 credit-funded")
        );

        assert_eq!(
            monthly_reset_fallback("2026-09-15T08:00:00Z"),
            Some(
                DateTime::parse_from_rfc3339("2026-10-01T00:00:00Z")
                    .unwrap()
                    .into()
            )
        );

        let malformed_end = json!({"periodStart":"2026-09-01","models":[
            {"model":"helm-a","cap":1000,"tokensUsed":2000,"periodEnd":"not a date"}
        ]});
        assert_eq!(
            parse_models(&malformed_end, false).unwrap()[0].resets_at,
            Some(
                DateTime::parse_from_rfc3339("2026-10-01T00:00:00Z")
                    .unwrap()
                    .into()
            )
        );
    }

    #[test]
    fn period_start_fallback_matches_upstream_date_prefix_rules() {
        for invalid in [
            "2026-13-01",
            "2026-00-01",
            "2026-09-00",
            "2026-09-32",
            "garbage",
            "2026-09-01x",
        ] {
            assert_eq!(monthly_reset_fallback(invalid), None, "{invalid}");
        }
        assert!(monthly_reset_fallback("2026-02-31").is_some());
    }

    #[test]
    fn malformed_quota_schema_and_unsafe_integer_values_are_rejected() {
        let drifted = json!({"periodStart":"2026-09-01T00:00:00Z","models":[{"model":"helm-model-a","limit":1000000,"consumed":250000}]});
        assert!(matches!(
            parse_models(&drifted, false),
            Err(ProviderError::Parse(_))
        ));
        for cap in [json!(1.5), json!(-1), json!(9_007_199_254_740_992_u64)] {
            let quota = json!({"periodStart":"2026-09-01","models":[{"model":"x","cap":cap,"tokensUsed":0}]});
            assert!(parse_models(&quota, true).is_err());
        }
        let too_many_hours = json!({"periodStart":"2026-09-01","models":[{"model":"x","cap":1,"tokensUsed":0,"windowHours":8761}]});
        assert!(parse_models(&too_many_hours, true).is_err());
    }

    #[test]
    fn credits_require_safe_integer_balance_and_valid_currency() {
        let cost = credits_cost(&json!({"balanceMicros":-1,"currency":"eur"})).unwrap();
        assert_eq!(cost.balance, Some(0.0));
        assert_eq!(cost.currency_code, "EUR");
        assert_eq!(
            credits_cost(&json!({"balanceMicros":12_500_000}))
                .unwrap()
                .currency_code,
            "EUR"
        );
        for malformed in [
            json!({"balanceMicros":12_500_000,"currency":12}),
            json!({"balanceMicros":"12500000"}),
            json!({"balanceMicros":12_500_000.5,"currency":"EUR"}),
            json!({"balanceMicros":9_007_199_254_740_992_u64,"currency":"EUR"}),
            json!({"balanceMicros":12_500_000,"currency":"EURO"}),
        ] {
            assert!(credits_cost(&malformed).is_none());
        }
    }

    #[tokio::test]
    async fn cloud_http_fetch_preserves_quota_and_enriches_premium_cost() {
        let result = mock_fetch(200, BILLING_FREE, 200, CREDITS, None)
            .await
            .unwrap();
        assert!(
            (result.usage.primary.used_percent - 73_854_494.0 / 2_000_000_000.0 * 100.0).abs()
                < 1e-10
        );
        assert_eq!(result.usage.primary.window_minutes, None);
        assert_eq!(result.usage.extra_rate_windows.len(), 3);
        assert_eq!(
            result.usage.account_organization.as_deref(),
            Some("Helmcode Cloud")
        );
        assert_eq!(
            result.cost.as_ref().and_then(|cost| cost.balance),
            Some(12.5)
        );
    }

    #[tokio::test]
    async fn nan_tenant_uses_its_identity_and_skips_credits_request() {
        let result = mock_fetch(200, BILLING_FREE, 200, CREDITS, Some("nanBuilders"))
            .await
            .unwrap();
        assert_eq!(
            result.usage.account_organization.as_deref(),
            Some("NaN Builders")
        );
        assert!(result.cost.is_none());
        assert_eq!(
            dashboard_url_for_organization(result.usage.account_organization.as_deref()),
            "https://cloud.nan.builders/dashboard"
        );
    }

    #[tokio::test]
    async fn premium_http_billing_exposes_rolling_windows() {
        let result = mock_fetch(200, BILLING_PREMIUM, 200, CREDITS, None)
            .await
            .unwrap();
        assert_eq!(result.usage.extra_rate_windows.len(), 5);
        let rolling = result
            .usage
            .extra_rate_windows
            .iter()
            .find(|window| window.title == "helm-rolling-a")
            .unwrap();
        assert_eq!(rolling.window.window_minutes, Some(240));
        assert_eq!(result.usage.primary.window_minutes, None);
    }

    #[tokio::test]
    async fn monthly_fallback_http_snapshot_drops_zero_cap_and_has_no_extra_windows() {
        let quota_body = r#"{"periodStart":"2026-12-15","models":[
            {"model":"helm-unlimited","cap":0,"tokensUsed":100},
            {"model":"helm-a","cap":1000,"tokensUsed":2000,"creditTokens":20}
        ]}"#;
        let mut server = mockito::Server::new_async().await;
        let quota = server
            .mock("GET", "/api/usage/quota")
            .with_status(200)
            .with_body(quota_body)
            .create_async()
            .await;
        let billing = server
            .mock("GET", "/api/billing")
            .with_status(200)
            .with_body("{}")
            .create_async()
            .await;
        let credits = server
            .mock("GET", "/api/billing/credits")
            .with_status(200)
            .with_body("{}")
            .create_async()
            .await;
        let result = provider_at(&server.url())
            .fetch_tenant(Tenant::Helmcode, "session=test-cookie")
            .await
            .unwrap();
        assert_eq!(result.usage.primary.used_percent, 100.0);
        assert_eq!(
            result.usage.primary.resets_at,
            Some(
                DateTime::parse_from_rfc3339("2027-01-01T00:00:00Z")
                    .unwrap()
                    .into()
            )
        );
        assert!(
            result
                .usage
                .primary
                .reset_description
                .as_deref()
                .unwrap()
                .contains("20 credit-funded")
        );
        assert!(result.usage.extra_rate_windows.is_empty());
        quota.assert_async().await;
        billing.assert_async().await;
        credits.assert_async().await;
    }

    #[tokio::test]
    async fn optional_http_failures_never_discard_quota_or_become_auth_errors() {
        for status in [401, 403, 302, 429, 503] {
            let billing_failed = mock_fetch(status, "{}", 200, CREDITS, None)
                .await
                .unwrap_or_else(|error| {
                    panic!("billing HTTP {status} failed quota fetch: {error}")
                });
            assert_eq!(billing_failed.usage.extra_rate_windows.len(), 3);
            assert!(billing_failed.cost.is_some());

            let credits_failed = mock_fetch(200, BILLING_FREE, status, "{}", None)
                .await
                .unwrap_or_else(|error| {
                    panic!("credits HTTP {status} failed quota fetch: {error}")
                });
            assert_eq!(credits_failed.usage.extra_rate_windows.len(), 3);
            assert!(credits_failed.cost.is_none());
        }

        let unavailable = mock_fetch(503, "bad JSON", 503, "bad JSON", None)
            .await
            .expect("503 optional endpoints leave quota available");
        assert_eq!(unavailable.usage.extra_rate_windows.len(), 3);
        assert!(unavailable.cost.is_none());
    }

    #[tokio::test]
    async fn malformed_or_oversized_optional_bodies_are_absent() {
        let oversized = " ".repeat(MAX_RESPONSE_BYTES + 1);
        for body in ["not json", "[]", oversized.as_str()] {
            let result = mock_fetch(200, body, 200, CREDITS, None).await.unwrap();
            assert_eq!(result.usage.extra_rate_windows.len(), 3);
            assert_eq!(
                result.cost.as_ref().and_then(|cost| cost.balance),
                Some(12.5)
            );
        }
        let result = mock_fetch(200, BILLING_FREE, 200, "not json", None)
            .await
            .unwrap();
        assert_eq!(result.usage.extra_rate_windows.len(), 3);
        assert!(result.cost.is_none());

        let malformed_credits = mock_fetch(
            200,
            BILLING_FREE,
            200,
            r#"{"balanceMicros":12500000,"currency":12}"#,
            None,
        )
        .await
        .unwrap();
        assert_eq!(malformed_credits.usage.extra_rate_windows.len(), 3);
        assert!(malformed_credits.cost.is_none());
    }

    #[tokio::test]
    async fn quota_auth_responses_still_require_authentication() {
        for status in [401, 403, 302] {
            let mut server = mockito::Server::new_async().await;
            let quota = server
                .mock("GET", "/api/usage/quota")
                .with_status(status)
                .create_async()
                .await;
            let billing = server
                .mock("GET", "/api/billing")
                .with_status(200)
                .with_body(BILLING_FREE)
                .create_async()
                .await;
            let result = provider_at(&server.url())
                .fetch_usage(&fetch_context(None))
                .await;
            assert!(
                matches!(result, Err(ProviderError::AuthRequired)),
                "HTTP {status}"
            );
            quota.assert_async().await;
            billing.assert_async().await;
        }
    }

    #[tokio::test]
    async fn quota_schema_drift_remains_a_parse_error_when_optional_billing_fails() {
        let mut server = mockito::Server::new_async().await;
        let quota = server
            .mock("GET", "/api/usage/quota")
            .with_status(200)
            .with_body(r#"{"periodStart":"2026-09-01T00:00:00Z","models":[{"model":"helm-model-a","limit":1000000,"consumed":250000}]}"#)
            .create_async()
            .await;
        let billing = server
            .mock("GET", "/api/billing")
            .with_status(503)
            .with_body("bad JSON")
            .create_async()
            .await;
        let result = provider_at(&server.url())
            .fetch_usage(&fetch_context(None))
            .await;
        assert!(matches!(result, Err(ProviderError::Parse(_))));
        quota.assert_async().await;
        billing.assert_async().await;
    }
}
