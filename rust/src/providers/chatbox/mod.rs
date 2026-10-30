//! Chatbox AI license quotas.
//!
//! Ported from upstream CodexBar (`Resources/Plugins/chatbox.ts`,
//! `Providers/Chatbox/ChatboxProviderDescriptor.swift`, #4742). The user pastes
//! a Chatbox license key into the API-key field. One read-only request,
//! `GET https://api.chatboxai.app/api/license/detail/realtime`, sends the raw
//! key as the `Authorization` header (no `Bearer` prefix), as Chatbox's public
//! client does. Nothing reads the Chatbox desktop profile, activates a
//! license, or makes a billable call. Response bodies are never echoed in
//! errors and the key is never logged.

#[cfg(test)]
mod tests;

use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Datelike, Utc};
use reqwest::{
    Client, StatusCode,
    header::{AUTHORIZATION, CACHE_CONTROL, HeaderValue},
};
use serde_json::{Map, Value};

use crate::core::{
    FetchContext, Provider, ProviderDisplayDetail, ProviderError, ProviderFetchResult, ProviderId,
    ProviderMetadata, ProviderStateKind, RateWindow, SourceMode, SubscriptionMetadata,
    UsageSnapshot,
};
use crate::providers::{BoundedBodyError, format, read_bounded_response, resolve_api_key};

const BASE_URL: &str = "https://api.chatboxai.app";
/// Optional override for the API origin (proof runs and fake servers).
/// Accepts `https://` or loopback `http://` only; anything else is ignored.
const BASE_URL_ENV: &str = "CHATBOX_BASE_URL";
const LICENSE_PATH: &str = "/api/license/detail/realtime";
const CREDENTIAL_TARGET: &str = "codexbar-chatbox";
pub const API_KEY_ENV: &str = "CHATBOX_LICENSE_KEY";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_RESPONSE_BYTES: usize = 256 * 1024;
/// Upstream's key bound, in UTF-16 code units (JavaScript `String.length`).
const MAX_KEY_LENGTH: usize = 4096;
/// Upstream rejects a detail list longer than this as malformed.
const MAX_DETAIL_ITEMS: usize = 100;
/// Upstream's bound on the license name, in UTF-16 code units.
const MAX_TEXT_LENGTH: usize = 100;
/// JavaScript `Number.MAX_SAFE_INTEGER`; upstream counters must stay below it.
const MAX_SAFE_INTEGER: u64 = (1 << 53) - 1;
const REMAINING_SECTION: &str = "Remaining";
const NO_PLAN_QUOTA: &str = "No active Chatbox AI plan quota";

const MISSING_KEY: &str = "Set a Chatbox AI license key in Settings or CHATBOX_LICENSE_KEY.";
const INVALID_KEY: &str = "Chatbox AI license key is invalid. Update it in Settings.";
const DENIED: &str = "Chatbox AI denied access. Check your license.";
const RATE_LIMITED: &str = "Chatbox AI is rate limiting requests. Try again later.";
const FAILED: &str = "Chatbox AI request failed. Try refreshing later.";
const MALFORMED: &str =
    "Chatbox AI returned an unrecognized response. Update CodexBar and try again.";

static CHATBOX_METADATA: ProviderMetadata = ProviderMetadata {
    id: ProviderId::Chatbox,
    display_name: "Chatbox AI",
    session_label: "Plan",
    weekly_label: "Top-up credits",
    supports_opus: false,
    supports_credits: false,
    default_enabled: false,
    is_primary: false,
    dashboard_url: Some("https://chatboxai.app"),
    status_page_url: None,
    tertiary_label_key: None,
};

pub struct ChatboxProvider {
    client: Client,
    base_url: String,
}

impl ChatboxProvider {
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

    async fn fetch_at(
        &self,
        ctx: &FetchContext,
        now: DateTime<Utc>,
    ) -> Result<ProviderFetchResult, ProviderError> {
        let key = checked_key(
            resolve_api_key(ctx.api_key.as_deref(), CREDENTIAL_TARGET, &[API_KEY_ENV]).ok(),
        )?;
        let body = self.get_license(&key).await?;
        parse_license(&body, now)
    }

    async fn get_license(&self, key: &str) -> Result<Vec<u8>, ProviderError> {
        // `from_bytes` admits the non-ASCII bytes a pasted key may carry;
        // `checked_key` has already rejected control characters and spaces.
        let mut authorization = HeaderValue::from_bytes(key.as_bytes())
            .map_err(|_| ProviderError::Other(INVALID_KEY.into()))?;
        authorization.set_sensitive(true);
        let response = self
            .client
            .get(format!("{}{LICENSE_PATH}", self.base_url))
            .header(AUTHORIZATION, authorization)
            .header(CACHE_CONTROL, "no-store")
            .send()
            .await?;
        let status = response.status();
        if status != StatusCode::OK {
            return Err(status_error(status));
        }
        read_bounded_response(response, MAX_RESPONSE_BYTES)
            .await
            .map_err(|error| match error {
                BoundedBodyError::Read(error) => ProviderError::Network(error),
                BoundedBodyError::TooLarge => malformed(),
            })
    }
}

impl Default for ChatboxProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for ChatboxProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Chatbox
    }

    fn metadata(&self) -> &ProviderMetadata {
        &CHATBOX_METADATA
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        match ctx.source_mode {
            SourceMode::Auto | SourceMode::OAuth => self.fetch_at(ctx, Utc::now()).await,
            SourceMode::Web | SourceMode::Cli => {
                Err(ProviderError::UnsupportedSource(ctx.source_mode))
            }
        }
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::OAuth]
    }

    /// Upstream classifies a malformed or rejected key as an expired
    /// credential, so the card asks for a new key.
    fn error_state_kind(&self, error: &ProviderError) -> ProviderStateKind {
        match error {
            ProviderError::Other(message) if message.starts_with(INVALID_KEY) => {
                ProviderStateKind::ExpiredSession
            }
            _ => error.state_kind(),
        }
    }
}

/// The API origin: a valid `CHATBOX_BASE_URL` override, otherwise production.
/// An invalid or blank override is ignored with a warning.
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
                "Ignoring {BASE_URL_ENV}: use an https:// URL or a loopback http:// URL without credentials, query, or fragment"
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

/// Upstream's key gate, applied before any request: at most 4096 UTF-16 code
/// units, no whitespace, and no C0, DEL or C1 control characters.
fn checked_key(resolved: Option<String>) -> Result<String, ProviderError> {
    let key = resolved
        .map(|key| key.trim().to_owned())
        .filter(|key| !key.is_empty())
        .ok_or_else(|| ProviderError::NotInstalled(MISSING_KEY.into()))?;
    let invalid = key.encode_utf16().count() > MAX_KEY_LENGTH
        || key
            .chars()
            .any(|c| c.is_whitespace() || (c as u32) < 32 || (127..=159).contains(&(c as u32)));
    if invalid {
        return Err(ProviderError::Other(INVALID_KEY.into()));
    }
    Ok(key)
}

fn with_status(message: &str, status: StatusCode) -> String {
    format!("{message} (HTTP {})", status.as_u16())
}

fn status_error(status: StatusCode) -> ProviderError {
    let message = match status.as_u16() {
        401 => INVALID_KEY,
        403 => DENIED,
        429 => RATE_LIMITED,
        _ => FAILED,
    };
    ProviderError::Other(with_status(message, status))
}

fn malformed() -> ProviderError {
    ProviderError::Parse(MALFORMED.into())
}

/// One remaining-over-total quota pool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Pool {
    used: u64,
    limit: u64,
}

impl Pool {
    fn is_empty(self) -> bool {
        self.used == 0 && self.limit == 0
    }

    /// Upstream `ctx.pct`, clamped to 0...100.
    #[expect(
        clippy::cast_precision_loss,
        reason = "counters are bounded by MAX_SAFE_INTEGER"
    )]
    fn used_percent(self) -> f64 {
        (self.used as f64 / self.limit as f64 * 100.0).clamp(0.0, 100.0)
    }
}

/// A non-negative safe integer. Absent or null is unknown; anything else
/// (negative, fractional, Boolean, string, unsafe) rejects the payload.
fn counter(value: Option<&Value>) -> Result<Option<u64>, ProviderError> {
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return Ok(None);
    };
    if let Some(number) = value.as_u64() {
        return (number <= MAX_SAFE_INTEGER)
            .then_some(Some(number))
            .ok_or_else(malformed);
    }
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss,
        reason = "only whole, non-negative values below MAX_SAFE_INTEGER are converted"
    )]
    match value.as_f64() {
        Some(number)
            if number.is_finite()
                && number.fract() == 0.0
                && number >= 0.0
                && number <= MAX_SAFE_INTEGER as f64 =>
        {
            Ok(Some(number as u64))
        }
        _ => Err(malformed()),
    }
}

fn pool(
    object: &Map<String, Value>,
    used: &str,
    limit: &str,
) -> Result<Option<Pool>, ProviderError> {
    let used = counter(object.get(used))?;
    let limit = counter(object.get(limit))?;
    Ok(used.zip(limit).map(|(used, limit)| Pool { used, limit }))
}

/// Upstream `date`: an ISO 8601 date-time string from 1970 on; anything else
/// is unknown rather than malformed.
fn date(value: Option<&Value>) -> Option<DateTime<Utc>> {
    let text = value?.as_str()?;
    let bytes = text.as_bytes();
    let shaped = bytes.len() > 10
        && bytes[..4].iter().all(u8::is_ascii_digit)
        && bytes[4] == b'-'
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && bytes[7] == b'-'
        && bytes[8..10].iter().all(u8::is_ascii_digit)
        && bytes[10] == b'T';
    if !shaped {
        return None;
    }
    DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|parsed| parsed.with_timezone(&Utc))
        .filter(|parsed| parsed.year() >= 1970)
}

/// Upstream `text`: control characters become spaces, then the value is
/// trimmed and cut to 100 UTF-16 code units. Empty is unknown.
fn text(value: Option<&Value>) -> Option<String> {
    let cleaned: String = value?
        .as_str()?
        .chars()
        .map(|c| {
            if (c as u32) < 32 || c as u32 == 127 {
                ' '
            } else {
                c
            }
        })
        .collect();
    let mut units = 0;
    let cut: String = cleaned
        .trim()
        .chars()
        .take_while(|c| {
            units += c.len_utf16();
            units <= MAX_TEXT_LENGTH
        })
        .collect();
    (!cut.is_empty()).then_some(cut)
}

/// JavaScript truthiness for the envelope's `error` member.
fn is_truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(flag)) => *flag,
        Some(Value::Number(number)) => number.as_f64().is_some_and(|n| n != 0.0 && !n.is_nan()),
        Some(Value::String(text)) => !text.is_empty(),
        Some(Value::Array(_) | Value::Object(_)) => true,
    }
}

/// The `plan` pool and the summed unexpired non-plan pools from
/// `unified_token_usage_details`.
fn detail_pools(
    data: &Map<String, Value>,
    now: DateTime<Utc>,
) -> Result<(Option<Pool>, Option<Pool>), ProviderError> {
    let details = match data.get("unified_token_usage_details") {
        None | Some(Value::Null) => return Ok((None, None)),
        Some(Value::Array(items)) if items.len() <= MAX_DETAIL_ITEMS => items,
        Some(_) => return Err(malformed()),
    };
    let mut plan = None;
    let mut rewards: Option<Pool> = None;
    for entry in details {
        let item = entry.as_object().ok_or_else(malformed)?;
        let kind = item
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(malformed)?;
        let value = pool(item, "token_usage", "token_limit")?;
        if kind == "plan" {
            // Upstream only rejects a second plan after one carried a pool.
            if plan.is_some() {
                return Err(malformed());
            }
            plan = value;
        } else if let Some(value) = value {
            if date(item.get("expires_at")).is_some_and(|expires| expires <= now) {
                continue;
            }
            let total = rewards.unwrap_or(Pool { used: 0, limit: 0 });
            let sum = Pool {
                used: total.used + value.used,
                limit: total.limit + value.limit,
            };
            if sum.used > MAX_SAFE_INTEGER || sum.limit > MAX_SAFE_INTEGER {
                return Err(malformed());
            }
            rewards = Some(sum);
        }
    }
    Ok((plan, rewards))
}

fn remaining_row(id: &str, label: &str, pool: Pool) -> Option<ProviderDisplayDetail> {
    let remaining = pool.limit.saturating_sub(pool.used);
    ProviderDisplayDetail::new(
        id,
        label,
        format!(
            "{} / {}",
            format::count(remaining),
            format::count(pool.limit)
        ),
    )
    .and_then(|row| row.with_section_title(REMAINING_SECTION))
}

fn parse_license(body: &[u8], now: DateTime<Utc>) -> Result<ProviderFetchResult, ProviderError> {
    let root = match serde_json::from_slice::<Value>(body) {
        Ok(Value::Object(root)) => root,
        _ => return Err(malformed()),
    };
    let data = root
        .get("data")
        .and_then(Value::as_object)
        .ok_or_else(malformed)?;
    if is_truthy(root.get("error")) {
        return Err(malformed());
    }

    let (plan, detail_rewards) = detail_pools(data, now)?;
    let total = pool(data, "unified_token_usage", "unified_token_limit")?;
    let rewards = match data.get("aggregated_reward_details") {
        None | Some(Value::Null) => detail_rewards,
        Some(Value::Object(record)) => pool(record, "token_usage", "token_limit")?,
        Some(_) => return Err(malformed()),
    };
    let expansion = pool(data, "expansion_pack_usage", "expansion_pack_limit")?;
    let images = pool(data, "image_used_count", "image_total_quota")?;
    if plan.is_none()
        && total.is_none()
        && rewards.is_none()
        && expansion.is_none()
        && images.is_none()
    {
        return Err(malformed());
    }

    // Combined counters already include rewards and exclude top-ups, so the
    // combined total stands in only when no separate plan pool exists.
    let mut rows: Vec<ProviderDisplayDetail> = [
        ("plan", "Plan", plan),
        ("quota", "Quota", if plan.is_none() { total } else { None }),
        ("credits", "Credits", rewards),
        ("top-up-credits", "Top-up credits", expansion),
        ("image-generation", "Image Generation", images),
    ]
    .into_iter()
    .filter_map(|(id, label, pool)| {
        pool.filter(|pool| !pool.is_empty())
            .and_then(|pool| remaining_row(id, label, pool))
    })
    .collect();
    if rows.is_empty() {
        rows.extend(remaining_row("quota", "Quota", Pool { used: 0, limit: 0 }));
    }

    let name = text(data.get("name"));
    let status = text(data.get("status"));
    let expires = date(data.get("token_expire_time")).or_else(|| date(data.get("expires_at")));
    let next = date(data.get("token_next_refresh_time"));
    let usable_plan = status.as_deref().is_none_or(|status| status == "active")
        && expires.is_none_or(|expires| expires > now);
    let metered_plan = plan.filter(|plan| usable_plan && plan.limit > 0);
    // Rewards, top-ups and images do not share the plan's reset schedule, and
    // a reset at or after expiry is not a replenishment.
    let reset = next.filter(|next| {
        metered_plan.is_some() && *next > now && expires.is_none_or(|expires| *next < expires)
    });

    let primary = match metered_plan {
        Some(plan) => RateWindow::with_details(plan.used_percent(), None, reset, None),
        None => RateWindow::informational(NO_PLAN_QUOTA).hidden_on_card(),
    };
    let mut usage = UsageSnapshot::new(primary);
    if let Some(expansion) = expansion.filter(|expansion| expansion.limit > 0) {
        usage = usage.with_secondary(RateWindow::new(expansion.used_percent()));
    }
    if let Some(name) = name {
        usage = usage.with_login_method(name);
    }
    if expires.is_some() {
        usage = usage.with_subscription(Some(SubscriptionMetadata::new(None, expires, None)));
    }
    Ok(ProviderFetchResult::new(usage, "api").with_display_details(rows))
}
