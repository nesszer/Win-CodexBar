//! Replicate billing provider.
//!
//! Replicate exposes spend and prepaid credit information through its
//! authenticated billing page and read-only account endpoints. The Windows
//! port keeps credential selection native: it accepts a manually supplied
//! Cookie header or imports the `replicate.com` browser session. Browser
//! credentials stay in memory for the current fetch only. It never uses a
//! Replicate API token as a website credential and never logs cookie material.

use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use futures::StreamExt;
use reqwest::{Client, StatusCode, Url, header::HeaderMap};
use serde_json::Value;
use std::collections::VecDeque;
use std::time::Duration;
use tokio::time::timeout;

use crate::core::{
    CostSnapshot, FetchContext, ManualEmptyCookiePolicy, Provider, ProviderDisplayDetail,
    ProviderError, ProviderFetchResult, ProviderId, ProviderMetadata, RateWindow, SourceMode,
    UsageSnapshot,
};

const BILLING_URL: &str = "https://replicate.com/account/billing";
const REPLICATE_ORIGIN: &str = "https://replicate.com";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(8);
const OPTIONAL_CREDIT_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
const MAX_REACT_NODES: usize = 4000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AccountKind {
    User,
    Organization,
}

impl AccountKind {
    fn api_segment(self) -> &'static str {
        match self {
            Self::User => "users",
            Self::Organization => "organizations",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReplicateAccount {
    kind: AccountKind,
    username: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct InvoiceSpend {
    used: f64,
}

pub struct ReplicateProvider {
    metadata: ProviderMetadata,
    client: Client,
}

impl ReplicateProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: ProviderId::Replicate,
                display_name: "Replicate",
                session_label: "Spend",
                weekly_label: "Spend",
                supports_opus: false,
                supports_credits: true,
                default_enabled: false,
                is_primary: false,
                dashboard_url: Some(BILLING_URL),
                status_page_url: None,
                tertiary_label_key: None,
            },
            client: crate::core::credentialed_http_client_builder()
                .timeout(REQUEST_TIMEOUT)
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }

    async fn fetch_with_cookie(
        &self,
        cookie_header: &str,
        source_label: &str,
    ) -> Result<ProviderFetchResult, ProviderError> {
        let cookie_header = normalize_cookie_header(cookie_header).ok_or_else(|| {
            ProviderError::Other(
                "Replicate needs a Cookie header containing a nonempty sessionid.".to_string(),
            )
        })?;
        let billing_body = self
            .get_text(
                Url::parse(BILLING_URL).expect("valid Replicate billing URL"),
                &cookie_header,
                "text/html",
                REQUEST_TIMEOUT,
            )
            .await?;
        let account = parse_billing_account(&billing_body)?;
        let invoices_url = account_endpoint(&account, "invoices")?;
        let invoices_body = self
            .get_text(
                invoices_url,
                &cookie_header,
                "application/json",
                REQUEST_TIMEOUT,
            )
            .await?;
        let spend = parse_current_invoice(&invoices_body, Utc::now())?;

        let balance = self.fetch_optional_credit(&account, &cookie_header).await;
        Ok(result_from_billing(account, spend, balance, source_label))
    }

    async fn fetch_optional_credit(
        &self,
        account: &ReplicateAccount,
        cookie_header: &str,
    ) -> Option<f64> {
        let url = account_endpoint(account, "unused-credit").ok()?;
        let body = self
            .get_text(
                url,
                cookie_header,
                "application/json",
                OPTIONAL_CREDIT_TIMEOUT,
            )
            .await
            .ok()?;
        let value: Value = serde_json::from_str(&body).ok()?;
        parse_money(value.get("unused_credit")?)
    }

    async fn get_text(
        &self,
        url: Url,
        cookie_header: &str,
        accept: &str,
        request_timeout: Duration,
    ) -> Result<String, ProviderError> {
        let response = timeout(
            request_timeout,
            self.client
                .get(url)
                .header("Cookie", cookie_header)
                .header("Accept", accept)
                .send(),
        )
        .await
        .map_err(|_| ProviderError::Timeout)??;
        let status = response.status();
        let headers = response.headers().clone();
        validate_status(status, &headers)?;
        let body = read_bounded_body(response).await?;
        String::from_utf8(body).map_err(|_| {
            ProviderError::Parse("Replicate returned a response that was not valid UTF-8.".into())
        })
    }

    async fn fetch_browser_cookie(&self) -> Result<ProviderFetchResult, ProviderError> {
        let candidates = normalized_browser_candidates(
            crate::providers::browser_cookie_headers_for_domain("replicate.com")?,
        );
        let mut authentication_failed = false;
        for (source_label, normalized) in candidates {
            match self.fetch_with_cookie(&normalized, &source_label).await {
                Ok(result) => return Ok(result),
                Err(error) if is_authentication_failure(&error) => {
                    authentication_failed = true;
                }
                Err(error) => return Err(error),
            }
        }

        if authentication_failed {
            Err(ProviderError::AuthRequired)
        } else {
            Err(ProviderError::NoCookies)
        }
    }

    /// Auto and Web share one path: a manual header wins, otherwise the
    /// provider tries browser candidates. There is no divergence today; if
    /// Auto and Web ever need one, state it here.
    async fn fetch_with_cookie_source(
        &self,
        ctx: &FetchContext,
    ) -> Result<ProviderFetchResult, ProviderError> {
        if let Some(cookie_header) = ctx.manual_cookie_header.as_deref() {
            return self.fetch_with_cookie(cookie_header, "manual").await;
        }
        // The shell signals "manual source selected, no cookie stored". Fail
        // closed instead of importing a browser account the user did not
        // select; browser candidates remain available for Auto without a
        // manual-cookie scope.
        if ctx.manual_cookie_missing {
            return Err(ProviderError::Other(
                "Replicate needs a Cookie header containing a nonempty sessionid.".to_string(),
            ));
        }
        self.fetch_browser_cookie().await
    }
}

impl Default for ReplicateProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for ReplicateProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Replicate
    }

    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        match ctx.source_mode {
            SourceMode::Auto | SourceMode::Web => self.fetch_with_cookie_source(ctx).await,
            source => Err(ProviderError::UnsupportedSource(source)),
        }
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::Web]
    }

    fn supports_web(&self) -> bool {
        true
    }

    fn manual_cookie_precedes_token_account(&self) -> bool {
        true
    }

    fn manual_empty_cookie_policy(&self) -> ManualEmptyCookiePolicy {
        ManualEmptyCookiePolicy::FailClosedWeb
    }
}

async fn read_bounded_body(response: reqwest::Response) -> Result<Vec<u8>, ProviderError> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
    {
        return Err(response_too_large());
    }
    let mut stream = response.bytes_stream();
    let mut body = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(ProviderError::Network)?;
        append_bounded_body(&mut body, &chunk)?;
    }
    Ok(body)
}

fn append_bounded_body(body: &mut Vec<u8>, chunk: &[u8]) -> Result<(), ProviderError> {
    if chunk.len() > MAX_RESPONSE_BYTES.saturating_sub(body.len()) {
        return Err(response_too_large());
    }
    body.extend_from_slice(chunk);
    Ok(())
}

fn response_too_large() -> ProviderError {
    ProviderError::Parse("Replicate returned an oversized response.".to_string())
}

fn account_endpoint(account: &ReplicateAccount, suffix: &str) -> Result<Url, ProviderError> {
    let mut url = Url::parse(REPLICATE_ORIGIN)
        .map_err(|_| ProviderError::Parse("Invalid Replicate endpoint.".to_string()))?;
    {
        let mut segments = url
            .path_segments_mut()
            .map_err(|_| ProviderError::Parse("Invalid Replicate endpoint.".to_string()))?;
        segments
            .push("api")
            .push(account.kind.api_segment())
            .push(&account.username)
            .push(suffix);
    }
    Ok(url)
}

fn parse_billing_account(body: &str) -> Result<ReplicateAccount, ProviderError> {
    let mut scanned = 0usize;
    let lower = body.to_ascii_lowercase();
    let mut cursor = 0usize;
    while cursor < lower.len() {
        scanned += 1;
        if scanned > MAX_REACT_NODES {
            break;
        }
        let Some(relative_start) = lower[cursor..].find("<script") else {
            break;
        };
        let start = cursor + relative_start;
        let after_name = start.saturating_add("<script".len());
        if !lower[after_name..]
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_whitespace() || character == '>')
        {
            cursor = after_name;
            continue;
        }
        let Some(relative_tag_end) = lower[after_name..].find('>') else {
            break;
        };
        let tag_end = after_name + relative_tag_end;
        let content_start = tag_end + 1;
        let Some(relative_close) = lower[content_start..].find("</script") else {
            break;
        };
        let close = content_start + relative_close;
        if parse_script_attributes(&body[after_name..tag_end]).is_some_and(|attrs| {
            attrs
                .iter()
                .any(|(name, value)| name == "id" && value == "react-component-props")
                && attrs
                    .iter()
                    .any(|(name, value)| name == "type" && value == "application/json")
        }) && let Ok(value) = serde_json::from_str::<Value>(&body[content_start..close])
            && let Some(account) = find_account_value(&value)
        {
            return Ok(account);
        }
        cursor = close + "</script".len();
    }

    if is_signed_out_billing_page(body) {
        return Err(ProviderError::AuthRequired);
    }
    Err(ProviderError::Parse(
        "Replicate billing response format changed: unrecognized billing account props".into(),
    ))
}

/// Parse `name="value"` / `name='value'` pairs from a raw script tag attribute
/// string. Unquoted and malformed attributes are skipped, matching the lenient
/// reading the previous hand-rolled matcher accepted for the target tags.
fn parse_script_attributes(raw: &str) -> Option<Vec<(String, String)>> {
    let mut attrs = Vec::new();
    let mut rest = raw.trim_start();
    while !rest.is_empty() {
        let name_len = rest
            .chars()
            .position(|c| c.is_ascii_whitespace() || c == '=')
            .unwrap_or(rest.len());
        let (name, after_name) = rest.split_at(name_len);
        let after_name = after_name.trim_start();
        if let Some(after_eq) = after_name.strip_prefix('=') {
            let after_eq = after_eq.trim_start();
            let (value, tail) = if let Some(quoted) = after_eq.strip_prefix('"') {
                quoted.split_once('"')?
            } else if let Some(quoted) = after_eq.strip_prefix('\'') {
                quoted.split_once('\'')?
            } else {
                let end = after_eq
                    .char_indices()
                    .find(|(_, c)| c.is_ascii_whitespace())
                    .map(|(i, _)| i)
                    .unwrap_or(after_eq.len());
                after_eq.split_at(end)
            };
            if !name.is_empty() {
                attrs.push((name.to_ascii_lowercase(), value.to_ascii_lowercase()));
            }
            rest = tail.trim_start();
        } else {
            if !name.is_empty() {
                attrs.push((name.to_ascii_lowercase(), String::new()));
            }
            rest = after_name;
        }
    }
    Some(attrs)
}

fn find_account_value(root: &Value) -> Option<ReplicateAccount> {
    let mut queue = VecDeque::from([root]);
    let mut visited = 0usize;
    while let Some(value) = queue.pop_front() {
        visited += 1;
        if visited > MAX_REACT_NODES {
            return None;
        }
        if let Some(account) = value
            .as_object()
            .and_then(|object| object.get("account"))
            .and_then(parse_account_value)
        {
            return Some(account);
        }
        match value {
            Value::Array(values) => queue.extend(values),
            Value::Object(object) => queue.extend(object.values()),
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
        }
    }
    None
}

fn parse_account_value(value: &Value) -> Option<ReplicateAccount> {
    let object = value.as_object()?;
    let kind = match object.get("kind")?.as_str()?.trim() {
        "user" => AccountKind::User,
        "organization" => AccountKind::Organization,
        _ => return None,
    };
    let username = object.get("username")?.as_str()?.trim();
    if username.is_empty() || username.len() > 256 || username.chars().any(char::is_control) {
        return None;
    }
    Some(ReplicateAccount {
        kind,
        username: username.to_string(),
    })
}

fn is_signed_out_billing_page(body: &str) -> bool {
    let lower = body.to_ascii_lowercase();
    let title = lower
        .split_once("<title>")
        .and_then(|(_, rest)| rest.split_once("</title>").map(|(title, _)| title.trim()))
        .is_some_and(|title| title == "sign in | replicate");
    title && lower.contains("/login/github/")
}

fn parse_current_invoice(body: &str, now: DateTime<Utc>) -> Result<InvoiceSpend, ProviderError> {
    let value: Value = serde_json::from_str(body).map_err(|_| parse_failure("invalid JSON"))?;
    let invoices = value
        .get("invoices")
        .and_then(Value::as_array)
        .ok_or_else(|| parse_failure("missing invoices"))?;
    let current = invoices.iter().find(|invoice| {
        let Some(object) = invoice.as_object() else {
            return false;
        };
        if object.get("type").and_then(Value::as_str) != Some("monthly-usage") {
            return false;
        }
        match object.get("ended_before") {
            None | Some(Value::Null) => true,
            Some(Value::String(value)) if !value.trim().is_empty() => {
                parse_invoice_end(value).is_some_and(|end| end > now)
            }
            _ => false,
        }
    });
    let current = current.ok_or_else(|| parse_failure("no current monthly-usage invoice"))?;
    let used = current
        .get("total_cost_before_adjustments")
        .and_then(parse_money)
        .ok_or_else(|| parse_failure("missing or invalid total_cost_before_adjustments"))?;
    Ok(InvoiceSpend { used })
}

fn parse_invoice_end(value: &str) -> Option<DateTime<Utc>> {
    let value = value.trim();
    if let Ok(end) = DateTime::parse_from_rfc3339(value) {
        return Some(end.with_timezone(&Utc));
    }
    if let Ok(date) = NaiveDate::parse_from_str(value, "%Y-%m-%d") {
        return Some(DateTime::<Utc>::from_naive_utc_and_offset(
            date.and_hms_opt(0, 0, 0)?,
            Utc,
        ));
    }
    [
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%dT%H:%M:%S%.f",
    ]
    .into_iter()
    .find_map(|format| {
        NaiveDateTime::parse_from_str(value, format)
            .ok()
            .map(|datetime| DateTime::<Utc>::from_naive_utc_and_offset(datetime, Utc))
    })
}

fn parse_money(value: &Value) -> Option<f64> {
    let text = value.as_str()?.trim();
    if text.is_empty()
        || !text
            .chars()
            .enumerate()
            .all(|(index, character)| character.is_ascii_digit() || (character == '.' && index > 0))
        || text.matches('.').count() > 1
        || text.ends_with('.')
    {
        return None;
    }
    let number = text.parse::<f64>().ok()?;
    number.is_finite().then_some(number)
}

fn result_from_billing(
    account: ReplicateAccount,
    spend: InvoiceSpend,
    balance: Option<f64>,
    source_label: &str,
) -> ProviderFetchResult {
    let spend_display = format!("${:.2}", spend.used);
    let account_id = format!(
        "replicate:{}:{}",
        account.kind.api_segment(),
        account.username
    );
    let mut usage = UsageSnapshot::new(RateWindow::informational(format!(
        "Spent this month: {spend_display}"
    )))
    .with_login_method("Replicate");
    if account.kind == AccountKind::Organization {
        usage = usage.with_organization(account.username.clone());
    }
    let mut cost = CostSnapshot::new(spend.used, "USD", "This month")
        .with_account_id(account.username.clone())
        .always_visible();
    if let Some(balance) = balance {
        cost = cost.with_balance(balance);
    }
    let mut result = ProviderFetchResult::new(usage, source_label)
        .with_non_authoritative_pace()
        .with_cost(cost)
        .with_account_identity(account_id)
        .with_display_detail(ProviderDisplayDetail::new(
            "spent-this-month",
            "Spent this month",
            spend_display,
        ));
    if let Some(balance) = balance {
        result = result.with_display_detail(ProviderDisplayDetail::new(
            "credit-balance",
            "Credit balance",
            format!("${balance:.2}"),
        ));
    }
    result
}

fn normalize_cookie_header(raw: &str) -> Option<String> {
    let mut value = raw.trim();
    if value
        .get(.."cookie:".len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("cookie:"))
    {
        value = value["cookie:".len()..].trim();
    }
    let mut pairs = Vec::new();
    for part in value.split(';') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let (name, cookie_value) = part.split_once('=')?;
        let name = name.trim();
        let cookie_value = cookie_value.trim();
        if name.is_empty()
            || cookie_value.is_empty()
            || name.chars().any(char::is_control)
            || cookie_value.chars().any(char::is_control)
        {
            return None;
        }
        pairs.retain(|(existing, _): &(String, String)| existing != name);
        pairs.push((name.to_string(), cookie_value.to_string()));
    }
    pairs
        .iter()
        .any(|(name, cookie_value)| name == "sessionid" && !cookie_value.is_empty())
        .then(|| {
            pairs
                .into_iter()
                .map(|(name, value)| format!("{name}={value}"))
                .collect::<Vec<_>>()
                .join("; ")
        })
}

fn normalized_browser_candidates(candidates: Vec<(String, String)>) -> Vec<(String, String)> {
    candidates
        .into_iter()
        .filter_map(|(source_label, header)| {
            normalize_cookie_header(&header).map(|normalized| (source_label, normalized))
        })
        .collect()
}

fn validate_status(status: StatusCode, headers: &HeaderMap) -> Result<(), ProviderError> {
    if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
        return Err(ProviderError::AuthRequired);
    }
    let retry_after = retry_after_seconds(
        headers
            .get("retry-after")
            .and_then(|value| value.to_str().ok()),
    );
    if status == StatusCode::TOO_MANY_REQUESTS {
        return Err(ProviderError::Other(format!(
            "Replicate rate limit reached; retry after {retry_after:.3}s."
        )));
    }
    if status == StatusCode::REQUEST_TIMEOUT || status.is_server_error() {
        return Err(ProviderError::Other(format!(
            "Replicate billing is unavailable; retry after {retry_after:.3}s."
        )));
    }
    if !status.is_success() {
        return Err(ProviderError::Other(format!(
            "Replicate returned HTTP {status}."
        )));
    }
    Ok(())
}

fn retry_after_seconds(value: Option<&str>) -> f64 {
    value
        .and_then(|value| value.trim().parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value >= 0.0)
        .map(|value| value.min(10.0))
        .unwrap_or(1.0)
}

fn parse_failure(field: &str) -> ProviderError {
    ProviderError::Parse(format!(
        "Replicate billing response format changed: {field}"
    ))
}

fn is_authentication_failure(error: &ProviderError) -> bool {
    matches!(error, ProviderError::AuthRequired)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn billing_page(account: &str) -> String {
        format!(
            r#"<html><title>Billing</title><script id="react-component-props" type="application/json">{{"props":{{"account":{account}}}}}</script></html>"#
        )
    }

    #[test]
    fn provider_metadata_and_sources_are_cookie_only() {
        let provider = ReplicateProvider::new();
        assert_eq!(provider.id(), ProviderId::Replicate);
        assert_eq!(provider.metadata().display_name, "Replicate");
        assert!(!provider.metadata().default_enabled);
        assert_eq!(
            provider.available_sources(),
            vec![SourceMode::Auto, SourceMode::Web]
        );
        assert!(provider.supports_web());
        assert!(!provider.supports_cli());
        assert!(!provider.supports_oauth());
    }

    #[test]
    fn parses_user_and_organization_accounts_from_bounded_react_props() {
        let user =
            parse_billing_account(&billing_page(r#"{"kind":"user","username":"alice"}"#)).unwrap();
        assert_eq!(user.kind, AccountKind::User);
        assert_eq!(user.username, "alice");

        let organization = parse_billing_account(&billing_page(
            r#"{"kind":"organization","username":"team/acme"}"#,
        ))
        .unwrap();
        assert_eq!(organization.kind, AccountKind::Organization);
        assert_eq!(organization.username, "team/acme");
        assert_eq!(
            account_endpoint(&organization, "invoices")
                .unwrap()
                .as_str(),
            "https://replicate.com/api/organizations/team%2Facme/invoices"
        );
    }

    #[test]
    fn signed_out_page_is_auth_failure_and_unknown_props_are_parse_failures() {
        let signed_out =
            r#"<title> Sign in | Replicate </title><a href="/login/github/">GitHub</a>"#;
        assert!(matches!(
            parse_billing_account(signed_out),
            Err(ProviderError::AuthRequired)
        ));
        assert!(matches!(
            parse_billing_account("<html>changed</html>"),
            Err(ProviderError::Parse(_))
        ));
    }

    #[test]
    fn current_invoice_selection_accepts_open_and_future_invoices() {
        let now = DateTime::parse_from_rfc3339("2026-09-20T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let body = serde_json::json!({
            "invoices": [
                {"type": "monthly-usage", "ended_before": "2026-09-19T00:00:00Z", "total_cost_before_adjustments": "9.00"},
                {"type": "monthly-usage", "ended_before": "2026-09-21T00:00:00Z", "total_cost_before_adjustments": "12.34"}
            ]
        });
        assert_eq!(
            parse_current_invoice(&body.to_string(), now).unwrap().used,
            12.34
        );

        let open = serde_json::json!({
            "invoices": [{"type": "monthly-usage", "ended_before": null, "total_cost_before_adjustments": "0"}]
        });
        assert_eq!(
            parse_current_invoice(&open.to_string(), now).unwrap().used,
            0.0
        );
    }

    #[test]
    fn invoice_selection_accepts_date_only_and_common_naive_dates() {
        let now = DateTime::parse_from_rfc3339("2026-09-20T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        for ended_before in ["2026-09-21", "2026-09-21 00:00:00", "2026-09-21T00:00:00"] {
            let body = serde_json::json!({
                "invoices": [{
                    "type": "monthly-usage",
                    "ended_before": ended_before,
                    "total_cost_before_adjustments": "3.25"
                }]
            });
            assert_eq!(
                parse_current_invoice(&body.to_string(), now).unwrap().used,
                3.25,
                "{ended_before}"
            );
        }
    }

    #[test]
    fn invoice_selection_fails_for_ended_missing_or_invalid_required_values() {
        let now = Utc::now();
        for value in [
            serde_json::json!({"invoices": []}),
            serde_json::json!({"invoices": [{"type": "monthly-usage", "ended_before": "2020-01-01T00:00:00Z", "total_cost_before_adjustments": "1"}]}),
            serde_json::json!({"invoices": [{"type": "monthly-usage", "ended_before": null, "total_cost_before_adjustments": "-1"}]}),
            serde_json::json!({"invoices": [{"type": "monthly-usage", "ended_before": null, "total_cost_before_adjustments": "1e2"}]}),
        ] {
            assert!(matches!(
                parse_current_invoice(&value.to_string(), now),
                Err(ProviderError::Parse(_))
            ));
        }
    }

    #[test]
    fn money_and_optional_credit_parsing_are_strict_and_nonnegative() {
        assert_eq!(parse_money(&Value::String("12.50".into())), Some(12.5));
        assert_eq!(parse_money(&Value::String("0".into())), Some(0.0));
        for text in ["", "-1", "+1", "1e2", "1.", ".5", "NaN"] {
            assert_eq!(parse_money(&Value::String(text.into())), None, "{text}");
        }
        let credit = serde_json::json!({"unused_credit": "4.25"});
        assert_eq!(
            parse_money(credit.get("unused_credit").unwrap()),
            Some(4.25)
        );
        assert_eq!(
            parse_money(&serde_json::json!({"unused_credit": 4.25})),
            None
        );
    }

    #[test]
    fn result_exposes_cost_and_display_details_without_quota_math() {
        let account = ReplicateAccount {
            kind: AccountKind::Organization,
            username: "acme".into(),
        };
        let result =
            result_from_billing(account, InvoiceSpend { used: 12.5 }, Some(4.25), "manual");
        assert_eq!(result.source_label, "manual");
        assert!(result.usage.primary.is_informational);
        assert!(!result.pace_authoritative);
        assert_eq!(result.cost.as_ref().unwrap().used, 12.5);
        assert_eq!(result.cost.as_ref().unwrap().balance, Some(4.25));
        let details = result.display_details();
        assert_eq!(details.len(), 2);
        assert_eq!(details[0].title(), "Spent this month");
        assert_eq!(details[1].title(), "Credit balance");
        assert_eq!(result.usage.account_organization.as_deref(), Some("acme"));
    }

    #[test]
    fn cookie_normalization_requires_sessionid_and_rejects_control_data() {
        assert_eq!(
            normalize_cookie_header("Cookie: other=1; sessionid=abc; other=2").as_deref(),
            Some("sessionid=abc; other=2")
        );
        assert_eq!(normalize_cookie_header("other=1"), None);
        assert_eq!(normalize_cookie_header("sessionid=\r\n"), None);
    }

    #[test]
    fn browser_candidates_skip_headers_without_a_session_cookie() {
        let candidates = normalized_browser_candidates(vec![
            ("Google Chrome".into(), "theme=dark".into()),
            ("Firefox".into(), "Cookie: sessionid=valid".into()),
        ]);

        assert_eq!(
            candidates,
            vec![("Firefox".to_string(), "sessionid=valid".to_string())]
        );
    }

    #[test]
    fn status_and_retry_after_classification_is_bounded() {
        let headers = HeaderMap::new();
        assert!(matches!(
            validate_status(StatusCode::UNAUTHORIZED, &headers),
            Err(ProviderError::AuthRequired)
        ));
        assert!(validate_status(StatusCode::OK, &headers).is_ok());
        assert_eq!(retry_after_seconds(Some("99")), 10.0);
        assert_eq!(retry_after_seconds(Some("bad")), 1.0);
        assert_eq!(retry_after_seconds(Some("0.5")), 0.5);
        assert!(
            validate_status(StatusCode::TOO_MANY_REQUESTS, &headers)
                .unwrap_err()
                .to_string()
                .contains("rate limit")
        );
        assert!(
            validate_status(StatusCode::INTERNAL_SERVER_ERROR, &headers)
                .unwrap_err()
                .to_string()
                .contains("unavailable")
        );

        let mut retry_headers = HeaderMap::new();
        retry_headers.insert("retry-after", "0.5".parse().unwrap());
        assert!(
            validate_status(StatusCode::TOO_MANY_REQUESTS, &retry_headers)
                .unwrap_err()
                .to_string()
                .contains("0.500s")
        );
    }

    #[test]
    fn streamed_response_cap_rejects_oversized_chunks() {
        let mut body = vec![0_u8; MAX_RESPONSE_BYTES];
        assert!(append_bounded_body(&mut body, &[0]).is_err());
    }
}
