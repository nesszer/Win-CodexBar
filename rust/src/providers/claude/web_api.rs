//! Claude Web API fetcher - uses browser cookies to fetch usage from claude.ai

use chrono::{DateTime, Utc};
use reqwest::{Client, StatusCode, header};
use serde::Deserialize;

use crate::core::{
    CostSnapshot, NamedRateWindow, ProviderError, ProviderFetchResult, RateWindow, UsageSnapshot,
};

use super::CLOUDFLARE_CHALLENGE_MESSAGE;
use super::reset_credits;

const CLOUDFLARE_BODY_PREFIX_BYTES: usize = 64 * 1024;

/// Query flag that opts the usage request in to the `cedar_ember` block.
const RESET_OPT_IN_QUERY: &str = "cedar_ember=1";

fn is_cloudflare_challenge_response(
    status: StatusCode,
    headers: &header::HeaderMap,
    body: &[u8],
) -> bool {
    if status != StatusCode::FORBIDDEN {
        return false;
    }

    if headers
        .get("cf-mitigated")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .is_some_and(|value| value.eq_ignore_ascii_case("challenge"))
    {
        return true;
    }

    let prefix = &body[..body.len().min(CLOUDFLARE_BODY_PREFIX_BYTES)];
    std::str::from_utf8(prefix)
        .ok()
        .is_some_and(|text| text.to_ascii_lowercase().contains("just a moment"))
}

fn classify_web_http_error(
    label: &str,
    status: StatusCode,
    headers: &header::HeaderMap,
    body: &[u8],
) -> ProviderError {
    if status == StatusCode::UNAUTHORIZED {
        return ProviderError::AuthRequired;
    }
    if status == StatusCode::FORBIDDEN {
        if is_cloudflare_challenge_response(status, headers, body) {
            return ProviderError::Other(CLOUDFLARE_CHALLENGE_MESSAGE.to_string());
        }
        return ProviderError::AuthRequired;
    }
    ProviderError::Other(format!("Failed to get {label}: {status}"))
}

/// Pass a success through; otherwise read the body and classify the status.
async fn ensure_success(
    response: reqwest::Response,
    label: &str,
) -> Result<reqwest::Response, ProviderError> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let response_headers = response.headers().clone();
    let body = response.bytes().await?;
    Err(classify_web_http_error(
        label,
        status,
        &response_headers,
        &body,
    ))
}

/// Read the response body as text, then deserialize as JSON. On failure, include
/// non-sensitive shape metadata so auth redirects, error envelopes, and schema
/// changes are distinguishable without exposing account data in UI/log output.
async fn parse_json_with_body<T: serde::de::DeserializeOwned>(
    response: reqwest::Response,
    label: &str,
) -> Result<T, ProviderError> {
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let body = response
        .text()
        .await
        .map_err(|e| ProviderError::Parse(format!("Failed to read {label} response body: {e}")))?;

    serde_json::from_str::<T>(&body).map_err(|e| {
        ProviderError::Parse(format!(
            "Failed to parse {label}: {e} ({})",
            describe_json_body_shape(&body, content_type.as_deref())
        ))
    })
}

fn describe_json_body_shape(body: &str, content_type: Option<&str>) -> String {
    let content_type = content_type.unwrap_or("unknown");
    let body_len = body.len();

    match serde_json::from_str::<serde_json::Value>(body) {
        Ok(serde_json::Value::Object(map)) => {
            let mut keys: Vec<&str> = map.keys().map(String::as_str).collect();
            keys.sort_unstable();
            let suffix = if keys.len() > 12 { ", ..." } else { "" };
            let keys = keys.into_iter().take(12).collect::<Vec<_>>().join(", ");
            format!("content_type={content_type}, body_len={body_len}, json_keys=[{keys}{suffix}]")
        }
        Ok(value) => format!(
            "content_type={content_type}, body_len={body_len}, json_type={}",
            json_value_kind(&value)
        ),
        Err(_) => format!("content_type={content_type}, body_len={body_len}, body_kind=non-json"),
    }
}

fn json_value_kind(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "bool",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

/// Claude Web API fetcher
pub struct ClaudeWebApiFetcher {
    client: Client,
    base_url: String,
}

/// Organization info from Claude API
#[derive(Debug, Deserialize)]
struct Organization {
    uuid: String,
    #[allow(
        dead_code,
        reason = "field mirrors the Claude API org payload; deserialized for round-trip fidelity but not read yet"
    )]
    name: Option<String>,
}

/// Usage response from Claude API.
///
/// Anthropic ships overlapping field names for the design and routines
/// windows (e.g. both `seven_day_design` and `seven_day_omelette` may appear
/// in the same payload). Serde aliases can't accept that — it errors with
/// "duplicate field" if more than one alias is present. We deserialize into
/// a generic map and pick the first alias that yields a non-null value.
#[derive(Debug)]
struct UsageResponse {
    five_hour: Option<UsageWindow>,
    seven_day: Option<UsageWindow>,
    seven_day_opus: Option<UsageWindow>,
    seven_day_sonnet: Option<UsageWindow>,
    seven_day_oauth_apps: Option<UsageWindow>,
    seven_day_design: Option<UsageWindow>,
    seven_day_routines: Option<UsageWindow>,
    extra_usage: Option<ExtraUsageResponse>,
    limits: Vec<super::scoped_weekly::ScopedWeeklyLimit>,
    /// Raw limit-reset block; decoded leniently by `reset_credits` so an
    /// unreadable block never fails the usage windows.
    cedar_ember: Option<serde_json::Value>,
}

impl<'de> Deserialize<'de> for UsageResponse {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let mut map: std::collections::HashMap<String, serde_json::Value> =
            std::collections::HashMap::deserialize(deserializer)?;

        let take = |map: &mut std::collections::HashMap<String, serde_json::Value>,
                    keys: &[&str]|
         -> Result<Option<UsageWindow>, D::Error> {
            for key in keys {
                if let Some(value) = map.remove(*key) {
                    if value.is_null() {
                        continue;
                    }
                    let window: UsageWindow =
                        serde_json::from_value(value).map_err(serde::de::Error::custom)?;
                    return Ok(Some(window));
                }
            }
            Ok(None)
        };

        Ok(UsageResponse {
            five_hour: take(&mut map, &["five_hour"])?,
            seven_day: take(&mut map, &["seven_day"])?,
            seven_day_opus: take(&mut map, &["seven_day_opus"])?,
            seven_day_sonnet: take(&mut map, &["seven_day_sonnet"])?,
            seven_day_oauth_apps: take(
                &mut map,
                &[
                    "seven_day_oauth_apps",
                    "seven_day_claude_oauth_apps",
                    "oauth_apps",
                    "oauth",
                ],
            )?,
            seven_day_design: take(
                &mut map,
                &[
                    "seven_day_design",
                    "seven_day_claude_design",
                    "claude_design",
                    "design",
                    "seven_day_omelette",
                    "omelette",
                    "omelette_promotional",
                ],
            )?,
            seven_day_routines: take(
                &mut map,
                &[
                    "seven_day_routines",
                    "seven_day_claude_routines",
                    "claude_routines",
                    "routines",
                    "routine",
                    "seven_day_cowork",
                    "cowork",
                ],
            )?,
            limits: map
                .get("limits")
                .filter(|value| !value.is_null())
                .cloned()
                .map(serde_json::from_value)
                .transpose()
                .map_err(serde::de::Error::custom)?
                .unwrap_or_default(),
            extra_usage: map
                .remove("extra_usage")
                .filter(|value| !value.is_null())
                .map(serde_json::from_value)
                .transpose()
                .map_err(serde::de::Error::custom)?,
            cedar_ember: map.remove("cedar_ember"),
        })
    }
}

/// A usage window from the API
#[derive(Debug, Deserialize)]
struct UsageWindow {
    utilization: Option<f64>,

    #[serde(rename = "resets_at")]
    resets_at: Option<String>,
}

/// Extra usage (credits) response
#[derive(Debug, Clone, Deserialize)]
struct ExtraUsageResponse {
    #[serde(rename = "monthly_credit_limit")]
    monthly_credit_limit: Option<f64>,

    #[serde(rename = "used_credits")]
    used_credits: Option<f64>,

    currency: Option<String>,

    #[serde(rename = "is_enabled")]
    is_enabled: Option<bool>,
}

/// Account info response
#[derive(Debug, Deserialize)]
struct AccountResponse {
    email_address: Option<String>,

    #[serde(rename = "rate_limit_tier")]
    rate_limit_tier: Option<String>,

    #[serde(default)]
    memberships: Vec<AccountMembership>,
}

#[derive(Debug, Deserialize)]
struct AccountMembership {
    uuid: Option<String>,
    organization: Option<AccountOrganization>,
}

#[derive(Debug, Deserialize)]
struct AccountOrganization {
    uuid: Option<String>,
}

impl AccountResponse {
    fn first_membership_org_id(&self) -> Option<String> {
        self.memberships.iter().find_map(|membership| {
            membership
                .organization
                .as_ref()
                .and_then(|organization| organization.uuid.as_deref())
                .or(membership.uuid.as_deref())
                .map(str::trim)
                .filter(|uuid| !uuid.is_empty())
                .map(ToString::to_string)
        })
    }
}

impl ClaudeWebApiFetcher {
    const DEFAULT_BASE_URL: &'static str = "https://claude.ai/api";

    /// Create a new fetcher
    pub fn new() -> Self {
        Self {
            client: crate::core::credentialed_http_client_builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("Failed to create HTTP client"),
            base_url: Self::DEFAULT_BASE_URL.to_string(),
        }
    }

    #[cfg(test)]
    fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    /// Fetch usage using browser cookies or env-var session key
    pub async fn fetch_with_cookies(&self) -> Result<ProviderFetchResult, ProviderError> {
        if let Some(session_key) = Self::resolve_session_key_from_env() {
            tracing::debug!("Using Claude session key from environment variable");
            let cookie_header = format!("sessionKey={session_key}");
            return self.fetch_with_cookie_header(&cookie_header).await;
        }

        let domains = [
            "claude.ai",
            "claude.com",
            "console.anthropic.com",
            "anthropic.com",
        ];

        // A challenge is a network-path failure, not evidence that a cached
        // session is invalid. Keep the last validated cookie for the next
        // refresh and only invalidate it for an ordinary auth response.
        use crate::browser::cookie_cache::CookieHeaderCache;
        if let Some(cached) = CookieHeaderCache::load(crate::core::ProviderId::Claude) {
            match self.fetch_with_cookie_header(&cached.cookie_header).await {
                Ok(result) => return Ok(result),
                Err(error) if is_cookie_authentication_failure(&error) => {
                    CookieHeaderCache::clear(crate::core::ProviderId::Claude);
                }
                Err(error) => return Err(error),
            }
        }

        let cookie_header = crate::providers::browser_cookie_header(&domains)?;
        let result = self.fetch_with_cookie_header(&cookie_header).await?;
        let _stored =
            CookieHeaderCache::store(crate::core::ProviderId::Claude, &cookie_header, "browser");
        Ok(result)
    }

    /// Fetch usage with a provided cookie header
    pub async fn fetch_with_cookie_header(
        &self,
        cookie_header: &str,
    ) -> Result<ProviderFetchResult, ProviderError> {
        tracing::debug!("Fetching Claude usage via web API");

        let headers = Self::build_headers(cookie_header);

        // Step 1: Get organization ID
        let org_id = self.get_organization_id(cookie_header, &headers).await?;
        tracing::debug!("Got organization ID: {}", org_id);

        // Step 2: Fetch usage data
        let usage = self.get_usage(&org_id, &headers).await?;

        // Step 3: Fetch extra usage (credits) - optional
        let extra_usage = self
            .get_extra_usage(&org_id, &headers)
            .await
            .ok()
            .or_else(|| usage.extra_usage.clone());

        // Step 4: Fetch account info - optional
        let account = self.get_account_info(&headers).await.ok();

        let (primary, secondary, model_specific) = self.build_rate_windows(&usage);

        let mut snapshot = UsageSnapshot::new(primary);

        if let Some(s) = secondary {
            snapshot = snapshot.with_secondary(s);
        }

        if let Some(m) = model_specific {
            snapshot = snapshot.with_model_specific(m);
        }

        append_web_extra_windows(
            &mut snapshot,
            usage
                .seven_day_oauth_apps
                .as_ref()
                .map(|w| self.to_rate_window(w, Some(10080))),
            super::scoped_weekly::scoped_weekly_windows(&usage.limits),
            usage
                .seven_day_routines
                .as_ref()
                .map(|w| self.to_rate_window(w, Some(10080))),
        );

        if let Some(acc) = &account {
            if let Some(email) = &acc.email_address {
                snapshot = snapshot.with_email(email.clone());
            }
            if let Some(tier) = &acc.rate_limit_tier {
                snapshot = snapshot.with_login_method(super::claude_plan_label(tier));
            }
        }

        let mut result = ProviderFetchResult::new(snapshot, "web");

        // Web-only, display-only limit-reset inventory (never persisted).
        if let Some(item) = usage
            .cedar_ember
            .as_ref()
            .and_then(|block| reset_credits::inventory_from_block(block, Utc::now()))
        {
            result = result.with_inventory_item(item);
        }

        // Add cost info if available
        let mut cost = extra_usage.and_then(|extra| {
            if !extra.is_enabled.unwrap_or(false) {
                return None;
            }
            let used_cents = extra.used_credits.unwrap_or(0.0);
            let limit_cents = extra.monthly_credit_limit;
            let currency = extra.currency.unwrap_or_else(|| "USD".to_string());

            let mut cost = CostSnapshot::new(
                used_cents / 100.0, // Convert cents to dollars
                currency,
                "Monthly",
            );

            if let Some(limit) = limit_cents {
                cost = cost.with_limit(limit / 100.0);
            }
            Some(cost)
        });

        // Best-effort prepaid Extra usage balance (non-fatal).
        // Gate: cookie session is already available on this path; skip only when
        // cookie source is explicitly off.
        let settings = crate::settings::Settings::load();
        let cookie_source = settings.claude_cookie_source();
        if !cookie_source.eq_ignore_ascii_case("off")
            && let Some(balance) = self.get_prepaid_credits(&org_id, &headers).await
        {
            cost = Some(apply_prepaid_balance(balance, cost));
        }

        if let Some(cost) = cost {
            result = result.with_cost(cost);
        }

        Ok(result)
    }

    fn build_headers(cookie_header: &str) -> reqwest::header::HeaderMap {
        use reqwest::header::HeaderValue;

        let mut headers = reqwest::header::HeaderMap::new();
        if let Ok(cookie) = HeaderValue::from_str(cookie_header) {
            headers.insert(header::COOKIE, cookie);
        }
        headers.insert(header::ACCEPT, HeaderValue::from_static("application/json"));
        headers.insert(
            header::ORIGIN,
            HeaderValue::from_static("https://claude.ai"),
        );
        headers.insert(
            header::REFERER,
            HeaderValue::from_static("https://claude.ai/settings/usage"),
        );
        headers.insert(
            header::USER_AGENT,
            HeaderValue::from_static(
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
                 (KHTML, like Gecko) Chrome/144.0.0.0 Safari/537.36",
            ),
        );
        headers.insert(
            reqwest::header::HeaderName::from_static("anthropic-client-platform"),
            HeaderValue::from_static("web_claude_ai"),
        );

        headers
    }

    fn resolve_session_key_from_env() -> Option<String> {
        for env_name in ["CLAUDE_AI_SESSION_KEY", "CLAUDE_WEB_SESSION_KEY"] {
            let Ok(value) = std::env::var(env_name) else {
                continue;
            };

            let trimmed = value.trim();
            if trimmed.is_empty() {
                continue;
            }

            let normalized = trimmed
                .strip_prefix("sessionKey=")
                .unwrap_or(trimmed)
                .trim();

            if !normalized.is_empty() {
                return Some(normalized.to_string());
            }
        }

        None
    }

    /// Get the organization ID
    async fn get_organization_id(
        &self,
        cookie_header: &str,
        headers: &reqwest::header::HeaderMap,
    ) -> Result<String, ProviderError> {
        if let Some(org_id) = cookie_value(cookie_header, "lastActiveOrg") {
            return Ok(org_id);
        }

        if let Ok(account) = self.get_account_info(headers).await
            && let Some(org_id) = account.first_membership_org_id()
        {
            return Ok(org_id);
        }

        let url = format!("{}/organizations", self.base_url);
        let response = ensure_success(self.get(&url, headers).await?, "organizations").await?;
        let orgs: Vec<Organization> = parse_json_with_body(response, "organizations").await?;

        orgs.into_iter()
            .next()
            .map(|o| o.uuid)
            .ok_or_else(|| ProviderError::Parse("No organizations found".to_string()))
    }

    /// Get usage data.
    ///
    /// The first request opts in to the `cedar_ember` limit-reset block. A
    /// surface-specific rejection may reject only that opt-in, so any other
    /// status is retried once on the plain URL. Success, 401, 429, and a
    /// Cloudflare challenge keep their normal handling without a retry.
    async fn get_usage(
        &self,
        org_id: &str,
        headers: &reqwest::header::HeaderMap,
    ) -> Result<UsageResponse, ProviderError> {
        let url = format!("{}/organizations/{}/usage", self.base_url, org_id);
        let opted_in = self
            .get(&format!("{url}?{RESET_OPT_IN_QUERY}"), headers)
            .await?;

        let response = match opted_in.status() {
            StatusCode::OK | StatusCode::UNAUTHORIZED | StatusCode::TOO_MANY_REQUESTS => opted_in,
            StatusCode::FORBIDDEN => {
                let response_headers = opted_in.headers().clone();
                let body = opted_in.bytes().await?;
                if is_cloudflare_challenge_response(StatusCode::FORBIDDEN, &response_headers, &body)
                {
                    return Err(classify_web_http_error(
                        "usage",
                        StatusCode::FORBIDDEN,
                        &response_headers,
                        &body,
                    ));
                }
                self.get(&url, headers).await?
            }
            _ => self.get(&url, headers).await?,
        };

        parse_json_with_body(ensure_success(response, "usage").await?, "usage").await
    }

    async fn get(
        &self,
        url: &str,
        headers: &reqwest::header::HeaderMap,
    ) -> Result<reqwest::Response, ProviderError> {
        Ok(self.client.get(url).headers(headers.clone()).send().await?)
    }

    /// GET and parse JSON. Unlike [`ensure_success`], a failure reports only
    /// the status, without reading the body or mapping 401/403 to auth.
    async fn get_json<T: serde::de::DeserializeOwned>(
        &self,
        url: &str,
        headers: &reqwest::header::HeaderMap,
        label: &str,
    ) -> Result<T, ProviderError> {
        let response = self.get(url, headers).await?;
        if !response.status().is_success() {
            return Err(ProviderError::Other(format!(
                "Failed to get {label}: {}",
                response.status()
            )));
        }
        parse_json_with_body(response, label).await
    }

    /// Get extra usage (credits)
    async fn get_extra_usage(
        &self,
        org_id: &str,
        headers: &reqwest::header::HeaderMap,
    ) -> Result<ExtraUsageResponse, ProviderError> {
        let url = format!(
            "{}/organizations/{}/overage_spend_limit",
            self.base_url, org_id
        );
        self.get_json(&url, headers, "extra usage").await
    }

    /// Best-effort prepaid Extra usage balance. Non-fatal on any failure.
    async fn get_prepaid_credits(
        &self,
        org_id: &str,
        headers: &reqwest::header::HeaderMap,
    ) -> Option<PrepaidBalance> {
        let url = format!("{}/organizations/{}/prepaid/credits", self.base_url, org_id);

        let response = self
            .client
            .get(&url)
            .headers(headers.clone())
            .timeout(std::time::Duration::from_secs(15))
            .send()
            .await
            .ok()?;

        if !response.status().is_success() {
            return None;
        }

        let body = response.text().await.ok()?;
        parse_prepaid_balance(&body)
    }

    /// Get account info
    async fn get_account_info(
        &self,
        headers: &reqwest::header::HeaderMap,
    ) -> Result<AccountResponse, ProviderError> {
        let url = format!("{}/account", self.base_url);
        self.get_json(&url, headers, "account").await
    }

    /// Convert a usage window to a RateWindow
    fn to_rate_window(&self, window: &UsageWindow, window_minutes: Option<u32>) -> RateWindow {
        // `utilization` is already expressed in percent units: `1.0` means 1%,
        // not 100%. Treating values <= 1 as fractions reported a 1% session as a
        // fully consumed quota.
        let used_percent = window.utilization.unwrap_or(0.0);

        let resets_at = window
            .resets_at
            .as_ref()
            .and_then(|s| Self::parse_iso8601(s));

        let reset_description = resets_at.map(Self::format_reset_time);

        RateWindow::with_details(used_percent, window_minutes, resets_at, reset_description)
    }

    /// Build (primary, secondary, model_specific) rate windows from a usage
    /// response, applying the limits[]-over-legacy preference chain.
    ///
    /// Extracted so the exact chain tested in `issue_279_session_limits_win_*`
    /// and `session_falls_back_*` is the same code production runs — no
    /// duplicated inline copy in tests can silently drift.
    fn build_rate_windows(
        &self,
        usage: &UsageResponse,
    ) -> (RateWindow, Option<RateWindow>, Option<RateWindow>) {
        // Prefer limits[] session over legacy five_hour (mirrors the weekly
        // lane preferring weekly_all over seven_day). A stale
        // five_hour.utilization can transiently report 1.0 (100%) right after
        // a window rollover while the limits[] entry already reflects the
        // fresh value (#279, same bug class as #210). When both are absent,
        // fall back to the informational 5h placeholder below.
        let primary = super::scoped_weekly::session_window(&usage.limits)
            .or_else(|| {
                usage
                    .five_hour
                    .as_ref()
                    .map(|w| self.to_rate_window(w, Some(300))) // 5 hours = 300 minutes
            })
            .unwrap_or_else(RateWindow::no_active_session);

        // Prefer limits[] weekly_all over legacy seven_day (same as OAuth path).
        let secondary = super::scoped_weekly::weekly_all_window(&usage.limits).or_else(|| {
            usage
                .seven_day
                .as_ref()
                .map(|w| self.to_rate_window(w, Some(10080))) // 7 days = 10080 minutes
        });

        let model_specific = usage
            .seven_day_opus
            .as_ref()
            .map(|w| self.to_rate_window(w, Some(10080)));

        (primary, secondary, model_specific)
    }

    /// Parse ISO8601 date string
    fn parse_iso8601(s: &str) -> Option<DateTime<Utc>> {
        DateTime::parse_from_rfc3339(s)
            .ok()
            .map(|dt| dt.with_timezone(&Utc))
            .or_else(|| {
                chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f")
                    .ok()
                    .map(|ndt| ndt.and_utc())
            })
    }

    /// Format reset time for display
    fn format_reset_time(dt: DateTime<Utc>) -> String {
        dt.format("%b %-d at %-I:%M%p").to_string()
    }
}

impl Default for ClaudeWebApiFetcher {
    fn default() -> Self {
        Self::new()
    }
}

fn is_cookie_authentication_failure(error: &ProviderError) -> bool {
    matches!(error, ProviderError::AuthRequired)
}

fn cookie_value(cookie_header: &str, name: &str) -> Option<String> {
    cookie_header.split(';').find_map(|part| {
        let (key, value) = part.trim().split_once('=')?;
        if key.trim() != name {
            return None;
        }
        let value = value.trim();
        if value.is_empty() {
            None
        } else {
            Some(value.to_string())
        }
    })
}

#[derive(Debug, Clone, PartialEq)]
struct PrepaidBalance {
    amount_dollars: f64,
    currency_code: String,
}

#[derive(Debug, Deserialize)]
struct PrepaidCreditsResponse {
    amount: f64,
    currency: String,
}

/// Parse `{ amount: cents, currency }` → dollars when finite ≥ 0.
fn parse_prepaid_balance(body: &str) -> Option<PrepaidBalance> {
    let response: PrepaidCreditsResponse = serde_json::from_str(body).ok()?;
    if !response.amount.is_finite() || response.amount < 0.0 {
        return None;
    }
    let currency = response.currency.trim().to_ascii_uppercase();
    if currency.is_empty() {
        return None;
    }
    Some(PrepaidBalance {
        amount_dollars: response.amount / 100.0,
        currency_code: currency,
    })
}

/// Attach prepaid balance onto an existing same-currency cost, otherwise create
/// an "Extra usage" snapshot carrying only the balance.
fn apply_prepaid_balance(balance: PrepaidBalance, existing: Option<CostSnapshot>) -> CostSnapshot {
    match existing {
        Some(cost)
            if cost
                .currency_code
                .eq_ignore_ascii_case(&balance.currency_code) =>
        {
            cost.with_balance(balance.amount_dollars)
        }
        _ => CostSnapshot::new(0.0, balance.currency_code, "Extra usage")
            .with_balance(balance.amount_dollars),
    }
}

/// Push extras in upstream order: oauth-apps → scoped weekly → routines when present.
fn append_web_extra_windows(
    snapshot: &mut UsageSnapshot,
    oauth_apps: Option<RateWindow>,
    scoped_weekly: Vec<NamedRateWindow>,
    routines: Option<RateWindow>,
) {
    if let Some(window) = oauth_apps {
        snapshot.extra_rate_windows.push(NamedRateWindow::new(
            "claude-oauth-apps",
            "OAuth apps",
            window,
        ));
    }
    snapshot.extra_rate_windows.extend(scoped_weekly);
    if let Some(window) = routines {
        snapshot.extra_rate_windows.push(NamedRateWindow::new(
            "claude-routines",
            "Daily Routines",
            window,
        ));
    }
}

#[cfg(test)]
#[path = "web_api_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "cloudflare_tests.rs"]
mod cloudflare_tests;

#[cfg(test)]
#[path = "reset_opt_in_tests.rs"]
mod reset_opt_in_tests;
