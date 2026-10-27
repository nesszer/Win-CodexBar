//! Soniox organization billing.
//!
//! Ported from upstream CodexBar (`Resources/Plugins/soniox.ts`,
//! `Providers/Soniox/SonioxProviderDescriptor.swift`, upstream #4678). Reads two
//! read-only organization endpoints with an organization API key as a bearer
//! token: `GET https://api.soniox.com/v1/organization/balance` for the prepaid
//! USD balance and invoice billing state, then
//! `GET /v1/organization/usage/monthly` for this UTC month's USD usage and
//! budget. Refresh never submits transcription or synthesis work. Project API
//! keys cannot read organization billing. Response bodies are never echoed in
//! errors and the key is never logged.

#[cfg(test)]
mod tests;

use std::sync::LazyLock;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, TimeZone, Utc};
use regex_lite::Regex;
use reqwest::{Client, StatusCode, header::ACCEPT};
use serde_json::{Map, Value};

use crate::core::{
    CostSnapshot, FetchContext, Provider, ProviderDisplayDetail, ProviderError,
    ProviderFetchResult, ProviderId, ProviderMetadata, RateWindow, SourceMode, UsageSnapshot,
};
use crate::providers::{BoundedBodyError, format, read_bounded_response, resolve_api_key};

const BASE_URL: &str = "https://api.soniox.com/v1/organization";
/// Optional override for the organization API base (proof runs and fake servers).
/// Accepts `https://` or loopback `http://` only; anything else is ignored.
const BASE_URL_ENV: &str = "SONIOX_BASE_URL";
const ORGANIZATION_PATH: &str = "/v1/organization";
const CREDENTIAL_TARGET: &str = "codexbar-soniox";
pub const API_KEY_ENV: &str = "SONIOX_ORG_API_KEY";
const MAX_RESPONSE_BYTES: usize = 256 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const LOGIN_METHOD: &str = "Organization API key";
const SPEND_SECTION: &str = "Spend";
const BILLING_SECTION: &str = "Billing";
const COST_PERIOD: &str = "This month";
const MONTHLY_BUDGET: &str = "Monthly budget";
const NO_BUDGET: &str = "No monthly budget";

const MISSING_KEY: &str = "Set a Soniox organization API key in Settings or SONIOX_ORG_API_KEY. Project API keys are not supported.";
const PERMISSION_DENIED: &str = "Soniox requires an organization API key with Balance and spend and Usage and limits access. Project API keys are not supported.";
const RATE_LIMITED: &str = "Soniox rate limit reached (HTTP 429). Wait a moment, then refresh.";

static MONEY_TEXT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^-?\d+(?:\.\d+)?$").expect("static money regex"));
static MONTH_TEXT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\d{4}-(0[1-9]|1[0-2])$").expect("static month regex"));

pub struct SonioxProvider {
    client: Client,
    base_url: String,
}

impl SonioxProvider {
    pub fn new() -> Self {
        let override_url = std::env::var(BASE_URL_ENV).ok();
        Self::with_base_url(&base_url_from_override(override_url.as_deref()))
    }

    fn with_base_url(base_url: &str) -> Self {
        Self {
            client: crate::core::credentialed_http_client_builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(REQUEST_TIMEOUT)
                .build()
                .unwrap_or_else(|_| Client::new()),
            base_url: base_url.trim_end_matches('/').to_owned(),
        }
    }

    async fn fetch_usage_impl(
        &self,
        ctx: &FetchContext,
    ) -> Result<ProviderFetchResult, ProviderError> {
        let key = resolve_key(ctx.api_key.as_deref())?;
        let billing = parse_billing(&self.get_object(&key, "balance").await?)?;
        let monthly = parse_monthly(&self.get_object(&key, "usage/monthly").await?)?;
        Ok(build_result(&billing, &monthly))
    }

    async fn get_object(&self, key: &str, path: &str) -> Result<Map<String, Value>, ProviderError> {
        let response = self
            .client
            .get(format!("{}/{path}", self.base_url))
            .bearer_auth(key)
            .header(ACCEPT, "application/json")
            .send()
            .await?;
        let status = response.status();
        if status != StatusCode::OK {
            return Err(status_error(status));
        }
        let body = read_bounded_response(response, MAX_RESPONSE_BYTES)
            .await
            .map_err(|error| match error {
                BoundedBodyError::Read(error) => ProviderError::Network(error),
                BoundedBodyError::TooLarge => invalid("response body"),
            })?;
        match serde_json::from_slice::<Value>(&body) {
            Ok(Value::Object(object)) => Ok(object),
            Ok(_) => Err(invalid("expected an object")),
            Err(_) => Err(invalid("invalid JSON")),
        }
    }
}

impl Default for SonioxProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for SonioxProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Soniox
    }

    fn metadata(&self) -> &ProviderMetadata {
        &SONIOX_METADATA
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
}

/// The organization API base: a valid `SONIOX_BASE_URL` override, otherwise
/// production. An invalid or blank override is ignored with a warning.
fn base_url_from_override(raw: Option<&str>) -> String {
    let Some(raw) = raw.map(clean_override) else {
        return BASE_URL.to_owned();
    };
    match Some(raw.as_str())
        .filter(|raw| !raw.is_empty())
        .and_then(validated_base_url)
    {
        Some(base) => base,
        None => {
            tracing::warn!(
                "Ignoring {BASE_URL_ENV}: use an https:// URL or a loopback http:// URL ending in {ORGANIZATION_PATH}, without credentials, query, or fragment"
            );
            BASE_URL.to_owned()
        }
    }
}

fn clean_override(raw: &str) -> String {
    let value = raw.trim();
    let unquoted = value
        .strip_prefix('"')
        .and_then(|inner| inner.strip_suffix('"'))
        .or_else(|| {
            value
                .strip_prefix('\'')
                .and_then(|inner| inner.strip_suffix('\''))
        })
        .unwrap_or(value);
    unquoted.trim().to_owned()
}

/// Same policy as the Sub2API base URL: `https://`, or `http://` only for a
/// loopback host, with no userinfo, query, fragment, or encoded host delimiters.
/// The path must end in `/v1/organization`, because requests append only the
/// endpoint name (`/balance`, `/usage/monthly`).
/// Returns the URL without a trailing slash.
fn validated_base_url(raw: &str) -> Option<String> {
    let lower = raw.to_ascii_lowercase();
    if ["%2f", "%5c", "%3f", "%23", "%40", "%3a"]
        .iter()
        .any(|encoded| lower.contains(encoded))
    {
        return None;
    }
    let url = reqwest::Url::parse(raw).ok()?;
    let host = url.host_str()?;
    let scheme_ok = match url.scheme() {
        "https" => true,
        "http" => is_loopback_host(host),
        _ => false,
    };
    if !scheme_ok
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || host.contains('%')
        || host.chars().any(|c| c.is_control() || c.is_whitespace())
        || !url
            .path()
            .trim_end_matches('/')
            .ends_with(ORGANIZATION_PATH)
    {
        return None;
    }
    Some(url.as_str().trim_end_matches('/').to_owned())
}

fn is_loopback_host(host: &str) -> bool {
    let normalized = host.trim_end_matches('.').to_ascii_lowercase();
    if normalized == "localhost" {
        return true;
    }
    let ip_candidate = normalized
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .unwrap_or(&normalized);
    match ip_candidate.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(ip)) => ip.is_loopback(),
        Ok(std::net::IpAddr::V6(ip)) => ip.is_loopback(),
        Err(_) => false,
    }
}

/// Settings key, then the keyring, then `SONIOX_ORG_API_KEY`.
fn resolve_key(explicit: Option<&str>) -> Result<String, ProviderError> {
    resolve_api_key(explicit, CREDENTIAL_TARGET, &[API_KEY_ENV])
        .map_err(|_| ProviderError::NotInstalled(MISSING_KEY.into()))
}

fn status_error(status: StatusCode) -> ProviderError {
    let code = status.as_u16();
    match code {
        401 => ProviderError::AuthRequired,
        403 => ProviderError::Other(PERMISSION_DENIED.into()),
        429 => ProviderError::Other(RATE_LIMITED.into()),
        500..=599 => ProviderError::Other(format!(
            "Soniox is temporarily unavailable (HTTP {code}). Try again later or check https://status.soniox.com."
        )),
        _ => ProviderError::Other(format!(
            "Soniox billing request failed (HTTP {code}). Refresh to try again."
        )),
    }
}

fn invalid(field: &str) -> ProviderError {
    ProviderError::Parse(format!(
        "Soniox returned invalid billing data: {field}. Refresh to try again. If this persists, update CodexBar or report the issue."
    ))
}

/// A decimal string such as `"18.50"`, the only money encoding Soniox uses.
/// Negative values are accepted only where the field may be signed.
fn money(value: Option<&Value>, field: &str, signed: bool) -> Result<f64, ProviderError> {
    let text = value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| MONEY_TEXT.is_match(text))
        .ok_or_else(|| invalid(field))?;
    let amount: f64 = text.parse().map_err(|_| invalid(field))?;
    if !amount.is_finite() || (!signed && amount < 0.0) {
        return Err(invalid(field));
    }
    Ok(amount)
}

struct Billing {
    available: f64,
    invoice_billing: bool,
}

fn parse_billing(body: &Map<String, Value>) -> Result<Billing, ProviderError> {
    let available = money(
        body.get("available_balance_usd"),
        "available_balance_usd",
        true,
    )?;
    let invoice_billing = body
        .get("invoice_billing_enabled")
        .and_then(Value::as_bool)
        .ok_or_else(|| invalid("invoice_billing_enabled"))?;
    Ok(Billing {
        available,
        invoice_billing,
    })
}

struct Monthly {
    used: f64,
    budget: f64,
    resets_at: DateTime<Utc>,
}

fn parse_monthly(body: &Map<String, Value>) -> Result<Monthly, ProviderError> {
    let used = money(body.get("usage_usd"), "usage_usd", false)?;
    let budget = money(body.get("monthly_budget_usd"), "monthly_budget_usd", false)?;
    let month = body
        .get("month")
        .and_then(Value::as_str)
        .filter(|month| MONTH_TEXT.is_match(month))
        .ok_or_else(|| invalid("month"))?;
    Ok(Monthly {
        used,
        budget,
        resets_at: next_month_start(month)?,
    })
}

/// First instant of the UTC month after `YYYY-MM`, the monthly reset.
fn next_month_start(month: &str) -> Result<DateTime<Utc>, ProviderError> {
    let (year, number) = month.split_once('-').ok_or_else(|| invalid("month"))?;
    let year: i32 = year.parse().map_err(|_| invalid("month"))?;
    let number: u32 = number.parse().map_err(|_| invalid("month"))?;
    if !(1..=12).contains(&number) {
        return Err(invalid("month"));
    }
    let (year, number) = if number == 12 {
        (year + 1, 1)
    } else {
        (year, number + 1)
    };
    Utc.with_ymd_and_hms(year, number, 1, 0, 0, 0)
        .single()
        .ok_or_else(|| invalid("month"))
}

/// Upstream `ctx.pct`: clamped to 0...100.
fn percent(used: f64, budget: f64) -> f64 {
    (used / budget * 100.0).clamp(0.0, 100.0)
}

fn spend_row(id: &str, title: &str, value: String) -> Option<ProviderDisplayDetail> {
    ProviderDisplayDetail::new(id, title, value)
        .and_then(|row| row.with_section_title(SPEND_SECTION))
}

fn billing_row(id: &str, title: &str, value: &str) -> Option<ProviderDisplayDetail> {
    ProviderDisplayDetail::new(id, title, value)
        .and_then(|row| row.with_section_title(BILLING_SECTION))
}

fn build_result(billing: &Billing, monthly: &Monthly) -> ProviderFetchResult {
    let Monthly {
        used,
        budget,
        resets_at,
    } = *monthly;

    // Upstream shows the budget meter only when a budget is set, and hides it
    // from the card by default. Windows has no default-hidden usage item, so
    // the meter stays off the card (it still feeds the tray and float bar) and
    // the card shows the same numbers as a progress detail row instead.
    let primary = if budget > 0.0 {
        RateWindow::with_details(
            percent(used, budget),
            None,
            Some(resets_at),
            Some(format!("{} / {}", format::usd(used), format::usd(budget))),
        )
        .hidden_on_card()
    } else {
        RateWindow::informational(NO_BUDGET).hidden_on_card()
    };
    let usage = UsageSnapshot::new(primary).with_login_method(LOGIN_METHOD);

    let mut rows = Vec::new();
    if budget > 0.0 {
        rows.extend(
            spend_row(
                "monthly-budget",
                MONTHLY_BUDGET,
                format!("{} of {}", format::usd(used), format::usd(budget)),
            )
            .and_then(|row| row.with_progress(used.min(budget), budget)),
        );
    }
    let billing_method = if billing.invoice_billing {
        "Invoice billing"
    } else {
        "Prepaid"
    };
    rows.extend(billing_row("billing-method", billing_method, "USD"));
    if budget <= 0.0 {
        rows.extend(billing_row(
            "billing-monthly-budget",
            MONTHLY_BUDGET,
            &format::usd(budget),
        ));
    }

    let cost = CostSnapshot::new(used, "USD", COST_PERIOD)
        .with_limit(budget)
        .with_resets_at(resets_at)
        .pay_as_you_go();
    // Invoice accounts show spend only; a zero prepaid balance there would
    // read as exhausted credits. A negative prepaid balance is omitted too,
    // because CostSnapshot balances are non-negative and clamping it to zero
    // would also read as exhausted credits.
    let cost = if billing.invoice_billing || billing.available < 0.0 {
        cost
    } else {
        cost.with_balance(billing.available)
    };

    ProviderFetchResult::new(usage, "api")
        .with_display_details(rows)
        .with_cost(cost)
}

const SONIOX_METADATA: ProviderMetadata = ProviderMetadata {
    id: ProviderId::Soniox,
    display_name: "Soniox",
    session_label: "Monthly budget",
    weekly_label: "Usage",
    supports_opus: false,
    supports_credits: false,
    default_enabled: false,
    is_primary: false,
    dashboard_url: Some("https://console.soniox.com"),
    status_page_url: Some("https://status.soniox.com"),
    tertiary_label_key: None,
};
