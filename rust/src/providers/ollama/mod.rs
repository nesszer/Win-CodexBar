//! Ollama provider implementation
//!
//! Fetches usage data by scraping the Ollama settings page
//! Uses session cookies from browser or manual input

mod cookies;

use cookies::*;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use regex_lite::Regex;
use reqwest::Url;

use crate::core::{
    CostSnapshot, FetchContext, ManualEmptyCookiePolicy, Provider, ProviderError,
    ProviderFetchResult, ProviderId, ProviderMetadata, RateWindow, SourceMode, UsageSnapshot,
};
use crate::settings::ApiKeys;

/// Ollama settings page URL
const OLLAMA_SETTINGS_URL: &str = "https://ollama.com/settings";
/// API-key usage and wallet endpoint (upstream `ollama-api.ts`, CodexBar v0.74.1).
const OLLAMA_BALANCE_URL: &str = "https://ollama.com/api/balance";
const OLLAMA_MONTHLY_WINDOW_MINUTES: u32 = 30 * 24 * 60;
/// Period label for the purchased-credit cost. It ends in "balance", so the
/// shared card treats the cost as balance-only (same convention as Grok).
const OLLAMA_CREDIT_BALANCE_PERIOD: &str = "Credit balance";
/// One generic message for every unrecognized `/api/balance` body. It never
/// echoes the response, so provider text cannot leak into logs or the UI.
const OLLAMA_BALANCE_PARSE_ERROR: &str = "Ollama returned an unrecognized credit balance response.";
/// Current settings use monthly credits; "Free usage" is the free-plan meter label.
const OLLAMA_MONTHLY_USAGE_LABELS: [&str; 2] = ["Monthly usage", "Free usage"];

/// Reset-window pace (upstream `OllamaProviderDescriptor.pace`, v0.70.0):
/// `.calendarMonthResetWindow`.
pub(crate) const PACE_CAPABILITY: crate::core::ProviderPaceCapability =
    crate::core::ProviderPaceCapability::CALENDAR_MONTH_RESET_WINDOW;

/// Ollama provider
pub struct OllamaProvider {
    metadata: ProviderMetadata,
}

#[derive(Debug, Clone, PartialEq)]
struct UsageBlock {
    used_percent: f64,
    window_minutes: Option<u32>,
    resets_at: Option<DateTime<Utc>>,
    reset_description: Option<String>,
}

impl OllamaProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: ProviderId::Ollama,
                display_name: "Ollama",
                session_label: "Session",
                weekly_label: "Weekly",
                supports_opus: false,
                supports_credits: false,
                default_enabled: false,
                is_primary: false,
                dashboard_url: Some("https://ollama.com/settings"),
                status_page_url: None,
                tertiary_label_key: None,
            },
        }
    }

    /// Fetch usage by scraping ollama.com/settings
    async fn fetch_usage_web(
        &self,
        ctx: &FetchContext,
    ) -> Result<ProviderFetchResult, ProviderError> {
        let cookies = resolve_cookie_source(ctx)?;

        let client = crate::core::credentialed_http_client_builder()
            .timeout(std::time::Duration::from_secs(ctx.web_timeout))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| ProviderError::Other(e.to_string()))?;
        let start_url =
            Url::parse(OLLAMA_SETTINGS_URL).map_err(|e| ProviderError::Other(e.to_string()))?;

        match fetch_settings_html_at(&client, &cookies, start_url.clone()).await {
            Ok(html) => {
                // Only cache non-manual browser/validated sessions for reuse.
                if ctx.manual_cookie_header.is_none() {
                    cache_validated_session_cookie(&cookies);
                }
                self.parse_usage_result(&html)
            }
            Err(ProviderError::AuthRequired) if ctx.manual_cookie_header.is_none() => {
                // Cached/imported session expired — clear and re-import once.
                invalidate_cached_session_cookie();
                let fresh = resolve_browser_cookie_header(true)?
                    .map(OllamaCookieSource::Manual)
                    .ok_or(ProviderError::AuthRequired)?;
                let html = fetch_settings_html_at(&client, &fresh, start_url).await?;
                cache_validated_session_cookie(&fresh);
                self.parse_usage_result(&html)
            }
            Err(err) => Err(err),
        }
    }

    async fn fetch_usage_api(
        &self,
        ctx: &FetchContext,
    ) -> Result<ProviderFetchResult, ProviderError> {
        let api_key = Self::resolve_api_key(ctx).ok_or(ProviderError::AuthRequired)?;
        let client = crate::core::credentialed_http_client_builder()
            .timeout(std::time::Duration::from_secs(ctx.web_timeout.max(1)))
            .build()
            .map_err(|e| ProviderError::Other(e.to_string()))?;

        let balance_url =
            Url::parse(OLLAMA_BALANCE_URL).map_err(|e| ProviderError::Other(e.to_string()))?;
        Self::fetch_usage_api_at(&client, &api_key, balance_url).await
    }

    /// One bearer-authenticated GET to `/api/balance`. No cookies are sent and
    /// no other endpoint is probed during refresh.
    async fn fetch_usage_api_at(
        client: &reqwest::Client,
        api_key: &str,
        balance_url: Url,
    ) -> Result<ProviderFetchResult, ProviderError> {
        let api_key = clean_secret(Some(api_key)).ok_or(ProviderError::AuthRequired)?;
        let response = client
            .get(balance_url)
            .bearer_auth(&api_key)
            .header("Accept", "application/json")
            .header("User-Agent", "CodexBar/1.0")
            .send()
            .await?;
        let status = response.status();
        let bytes = response.bytes().await?;
        match status {
            reqwest::StatusCode::OK => parse_api_balance(&bytes),
            reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN => {
                Err(ollama_api_key_error())
            }
            reqwest::StatusCode::TOO_MANY_REQUESTS => Err(ProviderError::Other(
                "Ollama API rate limit reached. Try again later.".to_string(),
            )),
            status if status.is_server_error() => Err(ProviderError::Other(format!(
                "Ollama API is unavailable (HTTP {}).",
                status.as_u16()
            ))),
            status => Err(ProviderError::Other(format!(
                "Ollama API returned status {}",
                status.as_u16()
            ))),
        }
    }

    fn resolve_api_key(ctx: &FetchContext) -> Option<String> {
        ctx.api_key
            .as_deref()
            .and_then(|key| clean_secret(Some(key)))
            .or_else(|| {
                ["OLLAMA_API_KEY", "OLLAMA_KEY"].iter().find_map(|name| {
                    std::env::var(name)
                        .ok()
                        .and_then(|value| clean_secret(Some(&value)))
                })
            })
            .or_else(|| {
                ApiKeys::load()
                    .get("ollama")
                    .and_then(|key| clean_secret(Some(key)))
            })
    }

    fn has_api_key(ctx: &FetchContext) -> bool {
        ctx.api_key
            .as_deref()
            .and_then(|key| clean_secret(Some(key)))
            .is_some()
            || ["OLLAMA_API_KEY", "OLLAMA_KEY"].iter().any(|name| {
                std::env::var(name)
                    .ok()
                    .and_then(|value| clean_secret(Some(&value)))
                    .is_some()
            })
            || ApiKeys::load()
                .get("ollama")
                .and_then(|key| clean_secret(Some(key)))
                .is_some()
    }
}

/// Parse `GET /api/balance` (upstream `ollama-api.ts`, CodexBar v0.74.1, #4401).
///
/// - `included`: the monthly allowance becomes the Monthly window with
///   `used = allowance - balance` (clamped at zero) and resets at `period.until`.
/// - `purchased`: purchased credits become a balance-only cost.
///
/// Any unrecognized shape fails with one generic parse error. Numeric and
/// decimal-string amounts are accepted; booleans, nulls, non-finite values and
/// malformed reset timestamps are rejected.
fn parse_api_balance(bytes: &[u8]) -> Result<ProviderFetchResult, ProviderError> {
    let fail = || ProviderError::Parse(OLLAMA_BALANCE_PARSE_ERROR.to_string());
    let root: serde_json::Value = serde_json::from_slice(bytes).map_err(|_| fail())?;
    let root = root.as_object().ok_or_else(fail)?;
    let included = root.get("included").filter(|value| !value.is_null());
    let purchased = root.get("purchased").filter(|value| !value.is_null());
    if included.is_none() && purchased.is_none() {
        return Err(fail());
    }

    let mut monthly: Option<RateWindow> = None;
    if let Some(included) = included {
        let included = included.as_object().ok_or_else(fail)?;
        let allowance = balance_amount(included.get("allowance_usd")).ok_or_else(fail)?;
        let balance = balance_amount(included.get("balance_usd")).ok_or_else(fail)?;
        if allowance < 0.0 {
            return Err(fail());
        }
        let used = (allowance - balance).max(0.0);
        if !used.is_finite() {
            return Err(fail());
        }
        let resets_at = match included.get("period").filter(|value| !value.is_null()) {
            None => None,
            Some(period) => {
                let period = period.as_object().ok_or_else(fail)?;
                match period.get("until").filter(|value| !value.is_null()) {
                    None => None,
                    Some(until) => Some(parse_balance_until(until).ok_or_else(fail)?),
                }
            }
        };
        // A zero allowance has no meter to draw; the cost/identity still show.
        if allowance > 0.0 {
            let percent = (used / allowance * 100.0).min(100.0);
            monthly = Some(RateWindow::with_details(
                percent,
                Some(OLLAMA_MONTHLY_WINDOW_MINUTES),
                resets_at,
                None,
            ));
        }
    }

    let mut cost = None;
    if let Some(purchased) = purchased {
        let purchased = purchased.as_object().ok_or_else(fail)?;
        let balance = balance_amount(purchased.get("balance_usd")).ok_or_else(fail)?;
        cost = Some(credit_balance_cost(balance));
    }

    let has_monthly = monthly.is_some();
    // Without a monthly allowance there is no quota to show; keep the primary
    // lane informational and hidden so no fake 0% meter is drawn.
    let primary =
        monthly.unwrap_or_else(|| RateWindow::informational("No quota reported").hidden_on_card());
    let mut usage = UsageSnapshot::new(primary).with_login_method("API key");
    if has_monthly {
        usage = usage.with_primary_label("Monthly");
    }
    let mut result = ProviderFetchResult::new(usage, "api");
    if let Some(cost) = cost {
        result = result.with_cost(cost);
    }
    Ok(result)
}

/// Purchased-credit balance shared by the API path (`/api/balance`) and the
/// cookie path (the settings page "Usage credits" wallet), so both modes expose
/// the same balance-only cost.
fn credit_balance_cost(balance: f64) -> CostSnapshot {
    CostSnapshot::new(0.0, "USD", OLLAMA_CREDIT_BALANCE_PERIOD).with_balance(balance)
}

/// Upstream `amount()`: a finite JSON number, or a string of plain decimal digits.
fn balance_amount(value: Option<&serde_json::Value>) -> Option<f64> {
    let amount = match value? {
        serde_json::Value::Number(number) => number.as_f64()?,
        serde_json::Value::String(text) => {
            let text = text.trim();
            let plain_decimal = Regex::new(r"^-?\d+(?:\.\d+)?$").ok()?;
            if !plain_decimal.is_match(text) {
                return None;
            }
            text.parse::<f64>().ok()?
        }
        _ => return None,
    };
    amount.is_finite().then_some(amount)
}

/// Upstream `period.until`: an RFC 3339 timestamp with an explicit offset.
fn parse_balance_until(value: &serde_json::Value) -> Option<DateTime<Utc>> {
    let text = value.as_str()?;
    let shape =
        Regex::new(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:\d{2})$").ok()?;
    if !shape.is_match(text) {
        return None;
    }
    DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|date| date.with_timezone(&Utc))
}

/// Pure decision helper for the Ollama session reuse path (unit-tested).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OllamaSessionAction {
    UseCached,
    ReimportBrowser,
}

fn ollama_session_action(
    has_cached_validated: bool,
    auth_failed: bool,
    force_reimport: bool,
) -> OllamaSessionAction {
    if force_reimport || auth_failed || !has_cached_validated {
        OllamaSessionAction::ReimportBrowser
    } else {
        OllamaSessionAction::UseCached
    }
}

impl OllamaProvider {
    /// Parse the settings page into the usage snapshot plus the wallet cost
    /// (the same balance-only cost the API path reports).
    fn parse_usage_result(&self, html: &str) -> Result<ProviderFetchResult, ProviderError> {
        let usage = self.parse_usage_html(html)?;
        let mut result = ProviderFetchResult::new(usage, "web");
        if let Some(balance) = parse_credit_wallet_balance(html) {
            result = result.with_cost(credit_balance_cost(balance));
        }
        Ok(result)
    }

    /// Parse usage data from the Ollama settings HTML page
    fn parse_usage_html(&self, html: &str) -> Result<UsageSnapshot, ProviderError> {
        let wallet_balance = parse_credit_wallet_balance(html);
        // Check if we're signed out
        if html.contains("Sign in")
            && !html.contains("Cloud Usage")
            && !html.contains("Included usage")
            && !OLLAMA_MONTHLY_USAGE_LABELS
                .iter()
                .any(|label| html.contains(label))
            && !html.contains("Session usage")
            && wallet_balance.is_none()
        {
            return Err(ProviderError::AuthRequired);
        }

        let monthly_block = self.parse_usage_block(
            &OLLAMA_MONTHLY_USAGE_LABELS,
            html,
            Some(OLLAMA_MONTHLY_WINDOW_MINUTES),
        );
        let session_block =
            self.parse_usage_block(&["Session usage", "Hourly usage"], html, Some(5 * 60));
        let weekly_block = self.parse_usage_block(&["Weekly usage"], html, Some(7 * 24 * 60));

        if monthly_block.is_none()
            && session_block.is_none()
            && weekly_block.is_none()
            && wallet_balance.is_none()
        {
            return Err(ProviderError::Parse(
                "Could not find usage data on Ollama settings page".to_string(),
            ));
        }

        // A wallet-only page reports no quota: keep the primary lane informational
        // and hidden so no fake 0% meter is drawn (same as the API path).
        let primary = match monthly_block.as_ref().or(session_block.as_ref()) {
            Some(block) => rate_window_from_usage_block(Some(block)),
            None if wallet_balance.is_some() => {
                RateWindow::informational("No quota reported").hidden_on_card()
            }
            None => rate_window_from_usage_block(None),
        };
        let mut usage = UsageSnapshot::new(primary);
        if monthly_block.is_some() {
            usage = usage.with_primary_label("Monthly");
        }

        // Parse plan name
        if let Some(plan) = self.parse_plan_name(html) {
            usage = usage.with_login_method(&plan);
        }

        // Parse account email
        if let Some(email) = self.parse_account_email(html) {
            usage = usage.with_email(&email);
        }

        if let Some(weekly) = weekly_block.as_ref() {
            usage = usage.with_secondary(rate_window_from_usage_block(Some(weekly)));
        }

        Ok(usage)
    }

    /// Parse a usage block by looking for a label then extracting the percentage
    fn parse_usage_block(
        &self,
        labels: &[&str],
        html: &str,
        window_minutes: Option<u32>,
    ) -> Option<UsageBlock> {
        for label in labels {
            // Anchor on the element text so prose that merely mentions a label
            // (e.g. "Free usage credits can be used...") is not treated as a meter.
            let Some(label_match) = usage_label_regex(label).and_then(|re| re.find(html)) else {
                continue;
            };
            let tail = &html[label_match.end()..];
            let end = usage_block_end(tail, label).unwrap_or_else(|| tail.len().min(4000));
            let window = &tail[..end.min(tail.len())];

            // Try "XX% used" pattern
            let used_re = Regex::new(r"(\d+(?:\.\d+)?)\s*%\s*used").ok()?;
            if let Some(caps) = used_re.captures(window)
                && let Ok(val) = caps[1].parse::<f64>()
            {
                return Some(UsageBlock {
                    used_percent: val,
                    window_minutes,
                    resets_at: parse_first_datetime(window),
                    reset_description: parse_reset_description(window),
                });
            }

            if let Some(val) = parse_dollar_used_percent(window) {
                return Some(UsageBlock {
                    used_percent: val,
                    window_minutes,
                    resets_at: parse_first_datetime(window),
                    reset_description: parse_reset_description(window),
                });
            }

            // Try "width: XX%" pattern (progress bar CSS)
            let width_re = Regex::new(r"width:\s*(\d+(?:\.\d+)?)%").ok()?;
            if let Some(caps) = width_re.captures(window)
                && let Ok(val) = caps[1].parse::<f64>()
            {
                return Some(UsageBlock {
                    used_percent: val,
                    window_minutes,
                    resets_at: parse_first_datetime(window),
                    reset_description: parse_reset_description(window),
                });
            }
        }
        None
    }

    /// Parse plan name from the current "Included usage" or legacy "Cloud Usage" section.
    fn parse_plan_name(&self, html: &str) -> Option<String> {
        for pattern in [
            r#"Included usage\s*</span\s*>\s*<span[^>]*>([^<]+)</span"#,
            r#"Cloud Usage\s*</span>\s*<span[^>]*>([^<]+)</span>"#,
            // Current wallet heading carries the plan as a badge: "Usage credits" + <span>pro</span>.
            r#"(?i)<h[1-6][^>]*>\s*Usage credits\s*<span[^>]*>\s*([^<]+)</span"#,
        ] {
            let re = Regex::new(pattern).ok()?;
            if let Some(plan) = re
                .captures(html)
                .and_then(|caps| caps.get(1))
                .map(|m| m.as_str().trim().to_string())
                .filter(|value| !value.is_empty())
            {
                return Some(plan);
            }
        }
        None
    }

    /// Parse the account email from the settings page header (`id="header-email"`).
    ///
    /// Only that element is read, matching upstream CodexBar: a page-wide scan would
    /// pick up unrelated addresses (footer/support links). A missing header yields no email.
    fn parse_account_email(&self, html: &str) -> Option<String> {
        // Walk start tags attribute by attribute: a quoted value is consumed whole, so text
        // such as `data-x=' id="header-email"'` or `data-id="header-email"` cannot match.
        let tag_re = Regex::new(
            r#"<[A-Za-z][A-Za-z0-9:-]*((?:\s+[^\s=>/"']+(?:\s*=\s*(?:"[^"]*"|'[^']*'|[^\s>"']+))?)*)\s*/?>([^<]*)"#,
        )
        .ok()?;
        let attr_re =
            Regex::new(r#"([^\s=>/"']+)(?:\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>"']+)))?"#).ok()?;
        let raw = tag_re.captures_iter(html).find_map(|tag| {
            let is_header = attr_re.captures_iter(tag.get(1)?.as_str()).any(|attr| {
                let value = attr.get(2).or(attr.get(3)).or(attr.get(4));
                attr[1].eq_ignore_ascii_case("id")
                    && value.is_some_and(|v| v.as_str() == "header-email")
            });
            is_header.then(|| tag.get(2).map_or("", |m| m.as_str()).trim())
        })?;
        raw.contains('@').then(|| raw.to_string())
    }
}

impl Default for OllamaProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for OllamaProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Ollama
    }

    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        tracing::debug!("Fetching Ollama usage");

        match ctx.source_mode {
            SourceMode::Auto => {
                if Self::has_api_key(ctx) {
                    // Non-transport API failures (HTTP status, malformed body) fall
                    // through to the web source, so a bad balance response does not
                    // fail the refresh while a browser session is available.
                    match self.fetch_usage_api(ctx).await {
                        Ok(result) => return Ok(result),
                        Err(error) if error.is_transport_failure() => return Err(error),
                        Err(_) => {}
                    }
                }
                self.fetch_usage_web(ctx).await
            }
            SourceMode::Web => self.fetch_usage_web(ctx).await,
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

    fn supports_cli(&self) -> bool {
        false
    }

    fn retains_last_good_on_transport_failure(&self) -> bool {
        true
    }

    fn manual_empty_cookie_policy(&self) -> ManualEmptyCookiePolicy {
        ManualEmptyCookiePolicy::FailClosedWeb
    }
}

fn clean_secret(raw: Option<&str>) -> Option<String> {
    let mut value = raw?.trim().to_string();
    if value.len() >= 2
        && ((value.starts_with('"') && value.ends_with('"'))
            || (value.starts_with('\'') && value.ends_with('\'')))
    {
        value.remove(0);
        value.pop();
    }
    let value = value.trim().to_string();
    (!value.is_empty()).then_some(value)
}

/// Matches a usage label rendered as complete element text, i.e. `>Label<`.
fn usage_label_regex(label: &str) -> Option<Regex> {
    Regex::new(&format!(r">\s*{label}\s*<")).ok()
}

/// `tail` starts just after the current label's closing `<`.
fn usage_block_end(tail: &str, current_label: &str) -> Option<usize> {
    OLLAMA_MONTHLY_USAGE_LABELS
        .iter()
        .chain(["Session usage", "Hourly usage", "Weekly usage"].iter())
        .filter(|label| **label != current_label)
        .filter_map(|label| usage_label_regex(label)?.find(tail))
        .map(|label_match| label_match.start())
        .min()
        .map(|idx| idx.min(4000))
}

/// Convert included dollar credits to quota utilization, not a spend estimate.
fn parse_dollar_used_percent(text: &str) -> Option<f64> {
    let amount = r"([0-9]{1,3}(?:,[0-9]{3})*(?:\.[0-9]+)?|[0-9]+(?:\.[0-9]+)?)";
    let pattern = format!(r"(?i)\$\s*{amount}\s+of\s+\$\s*{amount}\s+used");
    let re = Regex::new(&pattern).ok()?;
    let caps = re.captures(text)?;
    let used = caps.get(1)?.as_str().replace(',', "").parse::<f64>().ok()?;
    let limit = caps.get(2)?.as_str().replace(',', "").parse::<f64>().ok()?;
    if !used.is_finite() || !limit.is_finite() || limit <= 0.0 {
        return None;
    }
    let percent = used / limit * 100.0;
    percent.is_finite().then_some(percent)
}

/// Dollar amount inside a wallet element: `$` with optional thousands separators and cents.
const OLLAMA_WALLET_AMOUNT: &str = r"(\$(?:[0-9]{1,3}(?:,[0-9]{3})+|[0-9]+)(?:\.[0-9]+)?)";

/// Balance from the current settings page "Usage credits" wallet (upstream #4370).
///
/// The balance must be the first element after the wallet heading (only `<div>`
/// openers and whitespace may precede it). Any closing tag or next heading ends
/// that match, so a missing value cannot borrow a monthly-spend figure, prose, or
/// an amount from another section. Script and style text is ignored. Returns
/// `None` when no balance is reported.
fn parse_credit_wallet_balance(html: &str) -> Option<f64> {
    let page = strip_script_and_style(html).replace("&nbsp;", " ");
    let heading = Regex::new(
        r"(?i)<h[1-6][^>]*>\s*Usage credits(?:\s*<span[^>]*>[^<]*</span\s*>)?\s*</h[1-6]\s*>",
    )
    .ok()?;
    let body = &page[heading.find(&page)?.end()..];
    let balance = Regex::new(&format!(
        r"(?i)^\s*(?:<div[^>]*>\s*)*<span[^>]*>\s*{OLLAMA_WALLET_AMOUNT}\s*</span\s*>"
    ))
    .ok()?;
    let raw = balance.captures(body)?.get(1)?.as_str();
    parse_wallet_amount(raw)
}

fn strip_script_and_style(html: &str) -> String {
    let mut page = html.to_string();
    for tag in ["script", "style"] {
        if let Ok(re) = Regex::new(&format!(r"(?is)<{tag}\b[^>]*>.*?</{tag}\s*>")) {
            page = re.replace_all(&page, "").into_owned();
        }
    }
    page
}

/// Wallet amount as a finite dollar value. The leading `$` is required.
fn parse_wallet_amount(raw: &str) -> Option<f64> {
    let amount = raw
        .trim()
        .strip_prefix('$')?
        .replace(',', "")
        .parse::<f64>()
        .ok()?;
    amount.is_finite().then_some(amount)
}

fn rate_window_from_usage_block(block: Option<&UsageBlock>) -> RateWindow {
    block
        .map(|block| {
            RateWindow::with_details(
                block.used_percent,
                block.window_minutes,
                block.resets_at,
                block.reset_description.clone(),
            )
        })
        .unwrap_or_else(|| RateWindow::new(0.0))
}

fn parse_first_datetime(html: &str) -> Option<DateTime<Utc>> {
    let re =
        Regex::new(r#"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:\d{2})"#).ok()?;
    let raw = re.find(html)?.as_str();
    DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|date| date.with_timezone(&Utc))
}

fn parse_reset_description(html: &str) -> Option<String> {
    let re = Regex::new(r"(?i)(resets?\s+in\s+[^<\n\r]+|reset\s+[^<\n\r]+)").ok()?;
    re.find(html)
        .map(|m| strip_html_entities(m.as_str()).trim().to_string())
        .filter(|value| !value.is_empty())
}

fn strip_html_entities(value: &str) -> String {
    value
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&#x2F;", "/")
}

async fn fetch_settings_html_at(
    client: &reqwest::Client,
    source: &OllamaCookieSource,
    start_url: Url,
) -> Result<String, ProviderError> {
    let mut current_url = start_url;

    for _ in 0..5 {
        let mut request = client
            .get(current_url.clone())
            .header(
                "Accept",
                "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
            )
            .header(
                "User-Agent",
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/143.0.0.0 Safari/537.36",
            );
        if let Some(cookie_header) = source.header_for_url(&current_url) {
            request = request.header("Cookie", cookie_header);
        }

        let response = request.send().await?;
        if response.status().is_redirection() {
            let Some(location) = response.headers().get(reqwest::header::LOCATION) else {
                return Err(ProviderError::Other(
                    "Ollama redirect missing Location header".to_string(),
                ));
            };
            let location = location
                .to_str()
                .map_err(|e| ProviderError::Other(e.to_string()))?;
            let next_url = current_url
                .join(location)
                .map_err(|e| ProviderError::Other(e.to_string()))?;
            if is_ollama_sign_in_redirect(&next_url)
                || !crate::core::is_same_origin(&current_url, &next_url)
            {
                return Err(ProviderError::AuthRequired);
            }
            current_url = next_url;
            continue;
        }

        if response.status() == reqwest::StatusCode::UNAUTHORIZED
            || response.status() == reqwest::StatusCode::FORBIDDEN
            || is_ollama_sign_in_redirect(response.url())
        {
            return Err(ProviderError::AuthRequired);
        }
        if !response.status().is_success() {
            return Err(ProviderError::Other(format!(
                "Ollama returned status {}",
                response.status()
            )));
        }
        return response
            .text()
            .await
            .map_err(|e| ProviderError::Other(e.to_string()));
    }

    Err(ProviderError::Other(
        "Ollama returned too many redirects".to_string(),
    ))
}

fn ollama_api_key_error() -> ProviderError {
    ProviderError::Other("Ollama API key is invalid or revoked.".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::LastGoodFailurePolicy;

    #[tokio::test]
    async fn settings_fetch_follows_same_origin_redirects() {
        let mut server = mockito::Server::new_async().await;
        let first = server
            .mock("GET", "/settings")
            .with_status(302)
            .with_header("location", "/settings/account")
            .create_async()
            .await;
        let second = server
            .mock("GET", "/settings/account")
            .with_status(200)
            .with_body("<html>usage</html>")
            .create_async()
            .await;
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();

        let html = fetch_settings_html_at(
            &client,
            &OllamaCookieSource::Manual("__Secure-session=test".to_string()),
            Url::parse(&format!("{}/settings", server.url())).unwrap(),
        )
        .await
        .unwrap();

        first.assert_async().await;
        second.assert_async().await;
        assert_eq!(html, "<html>usage</html>");
    }

    #[tokio::test]
    async fn settings_fetch_stops_before_following_signin_or_workos_redirects() {
        for location in [
            "https://signin.ollama.com/?client_id=test",
            "https://auth.workos.com/user_management/authorize?client_id=test",
        ] {
            let mut server = mockito::Server::new_async().await;
            let first = server
                .mock("GET", "/settings")
                .with_status(302)
                .with_header("location", location)
                .create_async()
                .await;
            let client = reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .unwrap();

            let error = fetch_settings_html_at(
                &client,
                &OllamaCookieSource::Manual("__Secure-session=test".to_string()),
                Url::parse(&format!("{}/settings", server.url())).unwrap(),
            )
            .await
            .unwrap_err();

            first.assert_async().await;
            assert!(matches!(error, ProviderError::AuthRequired));
        }
    }

    #[tokio::test]
    async fn settings_fetch_reports_redirect_exhaustion() {
        let mut server = mockito::Server::new_async().await;
        let redirect = server
            .mock("GET", "/settings")
            .expect(5)
            .with_status(302)
            .with_header("location", "/settings")
            .create_async()
            .await;
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();

        let error = fetch_settings_html_at(
            &client,
            &OllamaCookieSource::Manual("__Secure-session=test".to_string()),
            Url::parse(&format!("{}/settings", server.url())).unwrap(),
        )
        .await
        .unwrap_err();

        redirect.assert_async().await;
        assert_eq!(error.to_string(), "Ollama returned too many redirects");
    }

    /// Synthetic `/api/balance` body following the schema reported in #4399
    /// (upstream `OllamaAPIBalanceTests.fixture`); never a live key or account.
    const API_BALANCE_FIXTURE: &str = r#"{"included":{"balance_usd":"52.50","allowance_usd":60,
        "period":{"from":"2026-09-30T15:14:29Z","until":"2026-10-30T15:14:29Z"}},
        "purchased":{"balance_usd":"18.25"}}"#;

    /// Serves one `/api/balance` response with a fixed status and body. The mock
    /// requires the trimmed bearer key and no cookies; it is asserted before returning.
    async fn run_api_balance(
        status: usize,
        body: &str,
    ) -> Result<ProviderFetchResult, ProviderError> {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/api/balance")
            .match_header("authorization", "Bearer ollama-key")
            .match_header("cookie", mockito::Matcher::Missing)
            .with_status(status)
            .with_body(body)
            .create_async()
            .await;
        let client = reqwest::Client::new();

        let result = OllamaProvider::fetch_usage_api_at(
            &client,
            "  ollama-key  ",
            Url::parse(&format!("{}/api/balance", server.url())).unwrap(),
        )
        .await;

        mock.assert_async().await;
        result
    }

    #[tokio::test]
    async fn api_balance_maps_included_allowance_and_purchased_credits() {
        let result = run_api_balance(200, API_BALANCE_FIXTURE)
            .await
            .expect("valid balance response");

        assert_eq!(result.source_label, "api");
        assert_eq!(result.usage.primary.used_percent, 12.5);
        assert_eq!(
            result.usage.primary.window_minutes,
            Some(OLLAMA_MONTHLY_WINDOW_MINUTES)
        );
        assert_eq!(
            result.usage.primary.resets_at,
            DateTime::parse_from_rfc3339("2026-10-30T15:14:29Z")
                .ok()
                .map(|date| date.with_timezone(&Utc))
        );
        assert!(!result.usage.primary.is_informational);
        assert_eq!(result.usage.primary_label.as_deref(), Some("Monthly"));
        assert!(result.usage.secondary.is_none());
        assert_eq!(result.usage.login_method.as_deref(), Some("API key"));

        let cost = result.cost.expect("purchased credits become a cost");
        assert_eq!(cost.period, "Credit balance");
        assert_eq!(cost.used, 0.0);
        assert_eq!(cost.limit, None);
        assert_eq!(cost.balance, Some(18.25));
    }

    #[tokio::test]
    async fn api_balance_allowance_only_account_has_no_cost_or_reset() {
        let result = run_api_balance(
            200,
            r#"{"included":{"allowance_usd":"60","balance_usd":45}}"#,
        )
        .await
        .expect("valid balance response");

        assert_eq!(result.usage.primary.used_percent, 25.0);
        assert_eq!(result.usage.primary.resets_at, None);
        assert_eq!(result.usage.primary_label.as_deref(), Some("Monthly"));
        assert!(result.cost.is_none());
    }

    #[tokio::test]
    async fn api_balance_purchased_only_account_shows_balance_without_meter() {
        let result = run_api_balance(200, r#"{"purchased":{"balance_usd":0}}"#)
            .await
            .expect("valid balance response");

        assert!(result.usage.primary.is_informational);
        assert!(result.usage.primary.hidden_on_card);
        assert_eq!(result.usage.primary_label, None);
        let cost = result
            .cost
            .expect("a confirmed zero balance is still shown");
        assert_eq!(cost.balance, Some(0.0));
        assert_eq!(cost.limit, None);
    }

    #[tokio::test]
    async fn api_balance_zero_allowance_draws_no_monthly_meter() {
        let result = run_api_balance(200, r#"{"included":{"allowance_usd":0,"balance_usd":"0"}}"#)
            .await
            .expect("valid balance response");

        assert!(result.usage.primary.is_informational);
        assert_eq!(result.usage.primary_label, None);
        assert!(result.cost.is_none());
    }

    #[tokio::test]
    async fn api_balance_clamps_exhausted_allowance_and_keeps_negative_balance() {
        let result = run_api_balance(
            200,
            r#"{"included":{"allowance_usd":"60.00","balance_usd":"-2.50"},"purchased":{"balance_usd":18.25}}"#,
        )
        .await
        .expect("valid balance response");

        assert_eq!(result.usage.primary.used_percent, 100.0);
        assert_eq!(result.cost.and_then(|cost| cost.balance), Some(18.25));
    }

    #[tokio::test]
    async fn api_balance_rejects_malformed_bodies_with_generic_parse_error() {
        for body in [
            "not-json",
            "null",
            "[]",
            "{}",
            r#"{"included":{}}"#,
            r#"{"included":{"allowance_usd":-1,"balance_usd":0}}"#,
            r#"{"included":{"allowance_usd":60,"balance_usd":true}}"#,
            r#"{"included":{"allowance_usd":60,"balance_usd":null}}"#,
            r#"{"included":[]}"#,
            r#"{"purchased":{"balance_usd":null}}"#,
            r#"{"purchased":{"balance_usd":""}}"#,
            r#"{"purchased":{"balance_usd":"0x10"}}"#,
            r#"{"purchased":{"balance_usd":"NaN"}}"#,
            r#"{"purchased":{"balance_usd":1e999}}"#,
            r#"{"purchased":{"balance_usd":[]}}"#,
            r#"{"purchased":"private-response"}"#,
        ] {
            let error = run_api_balance(200, body)
                .await
                .expect_err("malformed balance must fail");
            assert!(
                matches!(&error, ProviderError::Parse(message) if message == OLLAMA_BALANCE_PARSE_ERROR),
                "unexpected error for {body}: {error}"
            );
            assert!(!error.to_string().contains("private-response"));
        }
    }

    #[tokio::test]
    async fn api_balance_rejects_malformed_resets_without_leaking_response() {
        for until in [
            "2026-10-30",
            "2026-10-30T15:14:29",
            "2026-02-31T12:00:00Z",
            "2026-99-01T12:00:00Z",
            "private-response",
        ] {
            let body = API_BALANCE_FIXTURE.replace("2026-10-30T15:14:29Z", until);
            let error = run_api_balance(200, &body)
                .await
                .expect_err("malformed reset must fail");
            assert!(
                matches!(&error, ProviderError::Parse(message) if message == OLLAMA_BALANCE_PARSE_ERROR),
                "unexpected error for {until}: {error}"
            );
            assert!(!error.to_string().contains(until));
        }
    }

    #[tokio::test]
    async fn api_balance_http_failures_are_classified_without_body() {
        let cases = [
            (401, "Ollama API key is invalid or revoked."),
            (403, "Ollama API key is invalid or revoked."),
            (429, "Ollama API rate limit reached. Try again later."),
            (503, "Ollama API is unavailable (HTTP 503)."),
            (404, "Ollama API returned status 404"),
        ];
        for (status, expected) in cases {
            let error = run_api_balance(status, "private-response")
                .await
                .expect_err("HTTP failure must fail");
            assert_eq!(error.to_string(), expected, "status {status}");
            assert!(!error.to_string().contains("private-response"));
        }
    }

    #[test]
    fn strips_wrapping_quotes_from_api_key() {
        assert_eq!(
            clean_secret(Some("  'ollama-key'  ")),
            Some("ollama-key".to_string())
        );
        assert_eq!(
            clean_secret(Some("  \"ollama-key\"  ")),
            Some("ollama-key".to_string())
        );
    }

    #[test]
    fn api_auth_error_names_invalid_or_revoked_key() {
        assert_eq!(
            ollama_api_key_error().to_string(),
            "Ollama API key is invalid or revoked."
        );
    }

    #[test]
    fn preserves_legacy_ollama_session_and_weekly_payloads() {
        let provider = OllamaProvider::new();
        let snapshot = provider
            .parse_usage_html(
                r#"
                    <span>Cloud Usage</span><span>Free</span>
                    <section>Session usage <span>42% used</span></section>
                    <section>Weekly usage <span>84% used</span></section>
                "#,
            )
            .unwrap();

        assert_eq!(snapshot.primary.used_percent, 42.0);
        assert_eq!(snapshot.primary.window_minutes, Some(5 * 60));
        assert_eq!(snapshot.primary_label, None);
        assert_eq!(snapshot.secondary.unwrap().used_percent, 84.0);
        assert_eq!(snapshot.login_method.as_deref(), Some("Free"));
    }

    #[test]
    fn parses_monthly_included_credits_and_keeps_weekly_window() {
        let provider = OllamaProvider::new();
        let snapshot = provider
            .parse_usage_html(
                r#"
                    <span>Included usage</span><span>Pro</span>
                    <section>Monthly usage <span>$7.50 of $60 used</span>
                        <time>2026-09-30T00:00:00Z</time></section>
                    <section>Weekly usage <span>25% used</span></section>
                "#,
            )
            .unwrap();

        assert_eq!(snapshot.primary.used_percent, 12.5);
        assert_eq!(
            snapshot.primary.window_minutes,
            Some(OLLAMA_MONTHLY_WINDOW_MINUTES)
        );
        assert_eq!(snapshot.primary_label.as_deref(), Some("Monthly"));
        assert_eq!(snapshot.secondary.unwrap().used_percent, 25.0);
        assert_eq!(snapshot.login_method.as_deref(), Some("Pro"));
    }

    #[test]
    fn parses_free_usage_meter_as_monthly_window() {
        // Sanitized free-plan settings-page fragment (ported from upstream
        // OllamaUsageParserTests); the "Free usage credits..." prose must not be read as the meter label.
        let provider = OllamaProvider::new();
        let snapshot = provider
            .parse_usage_html(
                r#"
                    <h2 class="text-xl font-medium flex items-center space-x-2">
                      <span>Included usage</span>
                      <span
                        class="text-xs font-normal px-2 py-0.5 rounded-full bg-neutral-100 text-neutral-600 capitalize"
                        >free</span
                      >
                    </h2>
                    <h2 id="header-email">user@example.com</h2>
                    <div id="free-plan-models">
                      <p>Free usage credits can be used with the following cloud models:</p>
                    </div>
                    <div>
                      <div class="flex justify-between mb-2">
                        <span class="text-sm">Free usage</span>
                        <span class="text-sm">69.5% used</span>
                      </div>
                      <div class="relative group" data-usage-meter>
                        <div
                          class="relative h-3 overflow-hidden rounded-full bg-neutral-200"
                          data-usage-track
                          aria-label="Free usage 69.5% used"
                        >
                          <div class="flex h-full overflow-hidden bg-neutral-950" style="width: 69.5%; "></div>
                        </div>
                      </div>
                      <div class="text-xs text-neutral-500 mt-1 local-time" data-time="2026-10-26T15:19:11Z">
                        Resets in 2 weeks.
                      </div>
                    </div>
                "#,
            )
            .unwrap();

        assert_eq!(snapshot.primary.used_percent, 69.5);
        assert_eq!(
            snapshot.primary.window_minutes,
            Some(OLLAMA_MONTHLY_WINDOW_MINUTES)
        );
        assert_eq!(snapshot.primary_label.as_deref(), Some("Monthly"));
        assert_eq!(
            snapshot.primary.resets_at,
            DateTime::parse_from_rfc3339("2026-10-26T15:19:11Z")
                .ok()
                .map(|date| date.with_timezone(&Utc))
        );
        assert!(snapshot.secondary.is_none());
    }

    #[test]
    fn parses_ollama_usage_blocks_with_window_bounds() {
        let provider = OllamaProvider::new();
        let html = r#"
            <section>Session usage <div style="width: 42%"></div><span>resets in 2h</span></section>
            <section>Weekly usage <span>84% used</span><time>2026-06-01T00:00:00Z</time></section>
        "#;
        let session = provider
            .parse_usage_block(&["Session usage", "Hourly usage"], html, Some(300))
            .unwrap();
        let weekly = provider
            .parse_usage_block(&["Weekly usage"], html, Some(10080))
            .unwrap();
        assert_eq!(session.used_percent, 42.0);
        assert_eq!(session.window_minutes, Some(300));
        assert_eq!(session.reset_description.as_deref(), Some("resets in 2h"));
        assert_eq!(weekly.used_percent, 84.0);
        assert!(weekly.resets_at.is_some());
    }

    #[test]
    fn session_action_reuses_cached_until_auth_fails() {
        assert_eq!(
            ollama_session_action(true, false, false),
            OllamaSessionAction::UseCached
        );
        assert_eq!(
            ollama_session_action(true, true, false),
            OllamaSessionAction::ReimportBrowser
        );
        assert_eq!(
            ollama_session_action(false, false, false),
            OllamaSessionAction::ReimportBrowser
        );
        assert_eq!(
            ollama_session_action(true, false, true),
            OllamaSessionAction::ReimportBrowser
        );
    }

    /// Synthetic current settings-page wallet (upstream #4370 `OllamaCreditWalletTests.wallet`);
    /// never a live capture.
    const CREDIT_WALLET_FIXTURE: &str = r#"
        <section>
          <div>
            <h2>Usage credits<span>pro</span></h2>
            <span>$18.25</span>
            <p>Refills to $30 in 3 weeks.</p>
          </div>
          <div>
            <span>Monthly credits used</span>
            <span>$4.50</span>
          </div>
        </section>
    "#;

    #[test]
    fn credit_wallet_balance_reaches_cost_without_inventing_quota() {
        let provider = OllamaProvider::new();
        let result = provider
            .parse_usage_result(CREDIT_WALLET_FIXTURE)
            .expect("wallet page with a balance parses");

        assert_eq!(result.source_label, "web");
        assert!(result.usage.primary.is_informational);
        assert!(result.usage.primary.hidden_on_card);
        assert_eq!(result.usage.primary_label, None);
        assert!(result.usage.secondary.is_none());
        assert_eq!(result.usage.login_method.as_deref(), Some("pro"));

        // Same balance-only cost shape the API path reports for purchased credits.
        let cost = result.cost.expect("wallet balance becomes a cost");
        assert_eq!(cost.period, "Credit balance");
        assert_eq!(cost.used, 0.0);
        assert_eq!(cost.limit, None);
        assert_eq!(cost.balance, Some(18.25));
    }

    #[test]
    fn credit_wallet_email_is_account_email_and_plan_stays_login_method() {
        // The account email must not overwrite the wallet plan badge (login_method).
        let provider = OllamaProvider::new();
        let html = format!(r#"<h2 id="header-email">user@example.com</h2>{CREDIT_WALLET_FIXTURE}"#);
        let result = provider
            .parse_usage_result(&html)
            .expect("wallet page with an account email parses");

        assert_eq!(result.usage.login_method.as_deref(), Some("pro"));
        assert_eq!(
            result.usage.account_email.as_deref(),
            Some("user@example.com")
        );
    }

    #[test]
    fn account_email_comes_from_header_not_unrelated_page_addresses() {
        // A footer/support address that precedes the header must not be picked.
        let provider = OllamaProvider::new();
        let html = format!(
            r#"<footer><a href="mailto:support@ollama.com">support@ollama.com</a></footer>
            <h2 id="header-email">user@example.com</h2>{CREDIT_WALLET_FIXTURE}"#
        );
        assert_eq!(
            provider.parse_account_email(&html).as_deref(),
            Some("user@example.com")
        );
    }

    #[test]
    fn account_email_is_absent_without_header_even_if_page_has_addresses() {
        // No header-email element: no account email, and no page-wide fallback to the footer address.
        let provider = OllamaProvider::new();
        let html = format!(
            r#"{CREDIT_WALLET_FIXTURE}<footer><a href="mailto:support@ollama.com">support@ollama.com</a></footer>"#
        );
        assert_eq!(provider.parse_account_email(&html), None);

        let result = provider
            .parse_usage_result(&html)
            .expect("wallet page without a header still parses");
        assert_eq!(result.usage.account_email, None);
        assert_eq!(result.usage.login_method.as_deref(), Some("pro"));
    }

    #[test]
    fn account_email_header_without_at_sign_is_ignored() {
        let provider = OllamaProvider::new();
        assert_eq!(
            provider.parse_account_email(r#"<h2 id="header-email">  Signed in  </h2>"#),
            None
        );
    }

    #[test]
    fn account_email_ignores_data_id_lookalike() {
        let provider = OllamaProvider::new();
        assert_eq!(
            provider.parse_account_email(
                r#"<span data-id="header-email">other@example.com</span><h2 id="header-email">user@example.com</h2>"#
            )
            .as_deref(),
            Some("user@example.com")
        );
        assert_eq!(
            provider
                .parse_account_email(r#"<span data-id="header-email">other@example.com</span>"#),
            None
        );
        assert_eq!(
            provider.parse_account_email(
                r#"<span data-x=' id="header-email"'>other@example.com</span>"#
            ),
            None
        );
        assert_eq!(
            provider
                .parse_account_email(
                    r#"<span data-x=' id="header-email"'>other@example.com</span><h2 class="a" id='header-email'>user@example.com</h2>"#
                )
                .as_deref(),
            Some("user@example.com")
        );
    }

    #[test]
    fn credit_wallet_monthly_spending_cannot_become_missing_balance() {
        let provider = OllamaProvider::new();
        let html = CREDIT_WALLET_FIXTURE.replace("<span>$18.25</span>", "");

        assert_eq!(parse_credit_wallet_balance(&html), None);
        // Without a balance or quota the page has no usable data, matching the API path.
        assert!(matches!(
            provider.parse_usage_result(&html),
            Err(ProviderError::Parse(_))
        ));
    }

    #[test]
    fn credit_wallet_balance_must_lead_its_section() {
        // Labelled or prose text ahead of the amount is not the wallet balance.
        let html = CREDIT_WALLET_FIXTURE.replace(
            "<span>$18.25</span>",
            "<span>Estimated balance $99.00</span>",
        );
        assert_eq!(parse_credit_wallet_balance(&html), None);

        // A non-breaking space before the amount is still a complete element.
        let html =
            CREDIT_WALLET_FIXTURE.replace("<span>$18.25</span>", "<span>&nbsp;$18.25</span>");
        assert_eq!(parse_credit_wallet_balance(&html), Some(18.25));
    }

    #[test]
    fn credit_wallet_heading_must_be_a_heading_element() {
        // Prose that merely mentions usage credits is not the wallet heading.
        let html = r#"<div><p>Usage credits<span>$5.00</span></p></div>"#;
        assert_eq!(parse_credit_wallet_balance(html), None);
    }

    #[test]
    fn credit_wallet_does_not_borrow_amounts_outside_its_section() {
        let after_section_end = r#"
            <section><h2>Usage credits<span>pro</span></h2></section>
            <section><span>$99.00</span></section>
        "#;
        assert_eq!(parse_credit_wallet_balance(after_section_end), None);

        let after_next_heading = r#"
            <section><h2>Usage credits</h2><h3>Billing</h3><span>$99.00</span></section>
        "#;
        assert_eq!(parse_credit_wallet_balance(after_next_heading), None);

        let after_closing_tag = r#"
            <div><h2>Usage credits</h2></div><div><span>$99.00</span></div>
        "#;
        assert_eq!(parse_credit_wallet_balance(after_closing_tag), None);
    }

    #[test]
    fn credit_wallet_ignores_script_and_style_text() {
        let html = r#"
            <script>var x = "<h2>Usage credits</h2><span>$1.00</span>";</script>
            <style>.a { color: red; }</style>
        "#;
        assert_eq!(parse_credit_wallet_balance(html), None);
    }

    #[test]
    fn credit_wallet_coexists_with_unchanged_monthly_quota() {
        let provider = OllamaProvider::new();
        let html = format!(
            r#"{CREDIT_WALLET_FIXTURE}
            <div><span>Monthly usage</span><span>$7.50 of $60 used</span>
            <div data-time="2026-10-30T15:14:29Z">Resets in 4 weeks.</div></div>"#
        );
        let result = provider
            .parse_usage_result(&html)
            .expect("wallet and monthly quota parse together");

        assert_eq!(result.usage.primary.used_percent, 12.5);
        assert_eq!(
            result.usage.primary.window_minutes,
            Some(OLLAMA_MONTHLY_WINDOW_MINUTES)
        );
        assert_eq!(
            result.usage.primary.resets_at,
            DateTime::parse_from_rfc3339("2026-10-30T15:14:29Z")
                .ok()
                .map(|date| date.with_timezone(&Utc))
        );
        assert_eq!(result.usage.primary_label.as_deref(), Some("Monthly"));
        assert_eq!(result.cost.and_then(|cost| cost.balance), Some(18.25));
    }

    #[test]
    fn credit_wallet_page_with_sign_in_link_is_not_treated_as_signed_out() {
        let provider = OllamaProvider::new();
        let html = format!(r#"<a href="/signin">Sign in</a>{CREDIT_WALLET_FIXTURE}"#);
        let result = provider
            .parse_usage_result(&html)
            .expect("wallet balance proves a signed-in page");
        assert_eq!(result.cost.and_then(|cost| cost.balance), Some(18.25));

        assert!(matches!(
            provider.parse_usage_html(r#"<a href="/signin">Sign in</a>"#),
            Err(ProviderError::AuthRequired)
        ));
    }

    #[test]
    fn transport_policy_replaces_free_form_wrappers() {
        let provider = OllamaProvider::new();
        assert_eq!(
            provider.last_good_failure_policy_for_error(&ProviderError::Timeout),
            LastGoodFailurePolicy::Preserve
        );
        assert_eq!(
            provider.last_good_failure_policy_for_error(&ProviderError::Other(
                "Network error: arbitrary wrapper".to_string(),
            )),
            LastGoodFailurePolicy::Replace
        );
    }
}
