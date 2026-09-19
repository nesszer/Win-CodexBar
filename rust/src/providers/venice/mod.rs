//! Venice provider implementation.
//!
//! Fetches API balance data from Venice's billing endpoint.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;

use crate::core::{
    FetchContext, Provider, ProviderDisplayDetail, ProviderError, ProviderFetchResult, ProviderId,
    ProviderMetadata, RateWindow, SourceMode, UsageSnapshot,
};

const VENICE_BALANCE_URL: &str = "https://api.venice.ai/api/v1/billing/balance";
const VENICE_SESSION_URL: &str = "https://outerface.venice.ai/api/user/session";
const VENICE_CREDENTIAL_TARGET: &str = "codexbar-venice";
const VENICE_SESSION_COOKIE: &str = "__venice-auth.session-token";
const VENICE_COOKIE_DOMAINS: &[&str] = &["venice.ai", "outerface.venice.ai"];
const VENICE_EXPIRATION_SKEW_SECS: i64 = 60;
const MAX_VENICE_COOKIE_HEADER_LEN: usize = 1_048_576;
const MAX_VENICE_COOKIE_VALUE_LEN: usize = 16_384;
const MAX_VENICE_COOKIE_CHUNKS: usize = 64;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VeniceBalanceResponse {
    can_consume: bool,
    consumption_currency: Option<String>,
    balances: VeniceBalances,
    diem_epoch_allocation: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct VeniceBalances {
    diem: Option<f64>,
    usd: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct VeniceSessionResponse {
    token: Option<String>,
}

pub struct VeniceProvider {
    metadata: ProviderMetadata,
    client: Client,
}

impl VeniceProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: ProviderId::Venice,
                display_name: "Venice",
                session_label: "Balance",
                weekly_label: "DIEM",
                supports_opus: false,
                supports_credits: true,
                default_enabled: false,
                is_primary: false,
                dashboard_url: Some("https://venice.ai/settings/api"),
                status_page_url: None,
            },
            client: crate::core::credentialed_http_client_builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }

    fn api_key(api_key: Option<&str>) -> Result<String, ProviderError> {
        resolve_api_key(api_key, VENICE_CREDENTIAL_TARGET, &["VENICE_API_KEY"])
    }

    async fn fetch_api(&self, api_key: &str) -> Result<UsageSnapshot, ProviderError> {
        let response = self
            .client
            .get(VENICE_BALANCE_URL)
            .bearer_auth(api_key)
            .header("Accept", "application/json")
            .send()
            .await?;

        if response.status() == reqwest::StatusCode::UNAUTHORIZED
            || response.status() == reqwest::StatusCode::FORBIDDEN
        {
            return Err(ProviderError::AuthRequired);
        }
        if !response.status().is_success() {
            return Err(ProviderError::Other(format!(
                "Venice API returned status {}",
                response.status()
            )));
        }

        let balance: VeniceBalanceResponse = response
            .json()
            .await
            .map_err(|e| ProviderError::Parse(format!("Failed to parse Venice balance: {e}")))?;
        Ok(snapshot_from_balance(&balance))
    }

    async fn fetch_web(
        &self,
        manual_cookie_header: Option<&str>,
    ) -> Result<ProviderFetchResult, ProviderError> {
        let raw_cookie_header = match manual_cookie_header {
            Some(header) => header.to_string(),
            None => crate::providers::browser_cookie_header(VENICE_COOKIE_DOMAINS)?,
        };
        let cookie_header =
            session_cookie_header(&raw_cookie_header).ok_or(ProviderError::NoCookies)?;

        let response = self
            .client
            .get(VENICE_SESSION_URL)
            .header("Cookie", cookie_header)
            .header("Accept", "application/json")
            .send()
            .await?;

        if response.status() == reqwest::StatusCode::UNAUTHORIZED
            || response.status() == reqwest::StatusCode::FORBIDDEN
        {
            return Err(ProviderError::AuthRequired);
        }
        if !response.status().is_success() {
            return Err(ProviderError::Other(format!(
                "Venice web session returned status {}",
                response.status()
            )));
        }

        let session: VeniceSessionResponse = response.json().await.map_err(|e| {
            ProviderError::Parse(format!("Failed to parse Venice web session: {e}"))
        })?;
        let token = session
            .token
            .as_deref()
            .filter(|token| !token.trim().is_empty())
            .ok_or(ProviderError::AuthRequired)?;
        let claims = crate::codex_accounts::api::jwt_payload(token)
            .ok_or_else(|| ProviderError::Parse("Venice session token is not a JWT".into()))?;
        let (usage, details) = snapshot_from_web_claims(&claims, Utc::now())?;
        let mut result = ProviderFetchResult::new(usage, "web").with_non_authoritative_pace();
        for detail in details {
            result = result.with_display_detail(detail);
        }
        Ok(result)
    }
}

fn snapshot_from_balance(balance: &VeniceBalanceResponse) -> UsageSnapshot {
    let active_currency = balance
        .consumption_currency
        .as_deref()
        .unwrap_or("")
        .to_ascii_uppercase();

    let (used_percent, detail) = if !balance.can_consume {
        (100.0, "Balance unavailable for API calls".to_string())
    } else if active_currency == "USD" && balance.balances.usd.unwrap_or(0.0) > 0.0 {
        (
            0.0,
            format!("${:.2} USD remaining", balance.balances.usd.unwrap_or(0.0)),
        )
    } else if active_currency != "USD" {
        if let (Some(diem), Some(allocation)) =
            (balance.balances.diem, balance.diem_epoch_allocation)
            && allocation > 0.0
        {
            let used = ((allocation - diem) / allocation * 100.0).clamp(0.0, 100.0);
            (
                used,
                format!("DIEM {:.2} / {:.2} epoch allocation", diem, allocation),
            )
        } else if let Some(diem) = balance.balances.diem
            && diem > 0.0
        {
            (0.0, format!("DIEM {diem:.2} remaining"))
        } else if let Some(usd) = balance.balances.usd
            && usd > 0.0
        {
            (0.0, format!("${usd:.2} USD remaining"))
        } else {
            (100.0, "No Venice API balance available".to_string())
        }
    } else {
        (100.0, "No Venice API balance available".to_string())
    };

    let mut window = RateWindow::new(used_percent);
    window.reset_description = Some(detail.clone());
    UsageSnapshot::new(window).with_login_method(detail)
}

impl Default for VeniceProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for VeniceProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Venice
    }

    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        match ctx.source_mode {
            SourceMode::Auto | SourceMode::OAuth => {
                let api_key = Self::api_key(ctx.api_key.as_deref())?;
                Ok(ProviderFetchResult::new(
                    self.fetch_api(&api_key).await?,
                    "api",
                ))
            }
            SourceMode::Web => self.fetch_web(ctx.manual_cookie_header.as_deref()).await,
            SourceMode::Cli => Err(ProviderError::UnsupportedSource(ctx.source_mode)),
        }
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::OAuth, SourceMode::Web]
    }

    fn supports_web(&self) -> bool {
        true
    }

    fn owns_browser_cookie_resolution(&self) -> bool {
        true
    }
}

fn session_cookie_header(raw: &str) -> Option<String> {
    if raw.len() > MAX_VENICE_COOKIE_HEADER_LEN {
        return None;
    }

    let mut exact = None;
    let mut chunks = BTreeMap::new();
    let chunk_prefix = format!("{VENICE_SESSION_COOKIE}.");

    for part in raw.split(';') {
        let Some((raw_name, raw_value)) = part.split_once('=') else {
            continue;
        };
        let name = raw_name.trim();
        let value = raw_value.trim();
        if value.is_empty()
            || value.len() > MAX_VENICE_COOKIE_VALUE_LEN
            || value.chars().any(char::is_control)
        {
            continue;
        }
        if name == VENICE_SESSION_COOKIE {
            if exact.is_some() {
                return None;
            }
            exact = Some(value.to_string());
            continue;
        }
        let Some(index) = name
            .strip_prefix(&chunk_prefix)
            .and_then(|value| value.parse::<usize>().ok())
        else {
            continue;
        };
        if index >= MAX_VENICE_COOKIE_CHUNKS || chunks.contains_key(&index) {
            return None;
        }
        chunks.insert(index, value.to_string());
    }

    if let Some(value) = exact {
        return Some(format!("{VENICE_SESSION_COOKIE}={value}"));
    }
    if chunks.is_empty() || !chunks.keys().next().is_some_and(|index| *index == 0) {
        return None;
    }

    let mut values = Vec::with_capacity(chunks.len());
    for index in 0..chunks.len() {
        values.push(chunks.get(&index)?.as_str());
    }
    Some(format!("{VENICE_SESSION_COOKIE}={}", values.concat()))
}

fn snapshot_from_web_claims(
    claims: &serde_json::Map<String, Value>,
    now: DateTime<Utc>,
) -> Result<(UsageSnapshot, Vec<ProviderDisplayDetail>), ProviderError> {
    let expiration = finite_non_negative(claims.get("exp"))
        .filter(|value| (1_000_000_000.0..=4_000_000_000.0).contains(value))
        .and_then(unix_seconds_to_datetime)
        .ok_or_else(|| ProviderError::AuthRequired)?;
    if expiration < now - chrono::Duration::seconds(VENICE_EXPIRATION_SKEW_SECS) {
        return Err(ProviderError::AuthRequired);
    }

    if claims
        .get("userType")
        .and_then(Value::as_str)
        .is_some_and(is_anonymous_user_type)
    {
        return Err(ProviderError::AuthRequired);
    }

    let usage = claims
        .get("bundledCreditsUsage")
        .and_then(Value::as_object)
        .ok_or_else(|| ProviderError::Parse("Venice web session has no credits usage".into()))?;
    let used_this_cycle = finite_non_negative(usage.get("usedThisCycle"))
        .ok_or_else(|| ProviderError::Parse("Venice web session has invalid usage".into()))?;
    let monthly_refill_credits = finite_non_negative(usage.get("monthlyRefillCredits"))
        .filter(|value| *value > 0.0)
        .ok_or_else(|| {
            ProviderError::Parse("Venice web session has invalid refill credits".into())
        })?;

    let available_credits = finite_non_negative(usage.get("availableCredits"))
        .or_else(|| finite_non_negative(claims.get("bundledCredits")));
    let venice_credits = finite_non_negative(claims.get("veniceCredits"));
    let tier_cap = finite_non_negative(usage.get("tierCap"));
    let next_refill_at = epoch_to_datetime(usage.get("nextRefillAt"));

    let mut details = Vec::new();
    if let Some(available) = available_credits {
        details.push(ProviderDisplayDetail::new(
            "subscription-credits",
            "Subscription credits available",
            format_credits(available),
        ));
    }
    if let Some(total) = venice_credits {
        details.push(ProviderDisplayDetail::new(
            "total-credits",
            "Total credits available",
            format_credits(total),
        ));
    }
    details.push(
        ProviderDisplayDetail::new(
            "used-this-cycle",
            "Used this cycle",
            format_credits(used_this_cycle),
        )
        .with_secondary_value(format!(
            "Monthly refill: {}",
            format_credits(monthly_refill_credits)
        ))
        .with_progress(used_this_cycle, monthly_refill_credits),
    );
    if let Some(cap) = tier_cap {
        details.push(ProviderDisplayDetail::new(
            "bank-cap",
            "Bank cap",
            format_credits(cap),
        ));
    }
    if let Some(next_refill) = next_refill_at {
        details.push(ProviderDisplayDetail::new(
            "next-refill",
            "Next refill",
            next_refill.to_rfc3339(),
        ));
    }
    if let Some(user_type) = claims.get("userType").and_then(Value::as_str) {
        let user_type = user_type.trim();
        if !user_type.is_empty() {
            details.push(ProviderDisplayDetail::new("plan", "Plan", user_type));
        }
    }

    Ok((
        UsageSnapshot::new(RateWindow::informational("Venice web credits")),
        details,
    ))
}

fn finite_non_negative(value: Option<&Value>) -> Option<f64> {
    match value {
        Some(Value::Number(number)) => number
            .as_f64()
            .filter(|value| value.is_finite() && *value >= 0.0),
        Some(Value::String(value)) => value
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite() && *value >= 0.0),
        _ => None,
    }
}

fn epoch_to_datetime(value: Option<&Value>) -> Option<DateTime<Utc>> {
    let value = finite_non_negative(value)?;
    let seconds = if value > 4_000_000_000.0 {
        value / 1000.0
    } else {
        value
    };
    if !(1_000_000_000.0..=4_000_000_000.0).contains(&seconds) {
        return None;
    }
    unix_seconds_to_datetime(seconds)
}

fn unix_seconds_to_datetime(seconds: f64) -> Option<DateTime<Utc>> {
    if !(1_000_000_000.0..=4_000_000_000.0).contains(&seconds) {
        return None;
    }
    // The range check above proves this conversion is within i64 bounds; the
    // fractional part is intentionally discarded because JWT/epoch values are
    // rendered at second precision.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "the preceding range check bounds this conversion to valid Unix seconds"
    )]
    let seconds = seconds as i64;
    DateTime::<Utc>::from_timestamp(seconds, 0)
}

fn is_anonymous_user_type(value: &str) -> bool {
    matches!(
        value.to_ascii_lowercase().as_str(),
        "anonymous" | "anon" | "guest" | "unauthenticated" | "logged_out"
    )
}

fn format_credits(value: f64) -> String {
    if (value - value.round()).abs() < 0.005 {
        format!("{value:.0}")
    } else {
        format!("{value:.2}")
    }
}

fn resolve_api_key(
    explicit: Option<&str>,
    credential_target: &str,
    env_names: &[&str],
) -> Result<String, ProviderError> {
    if let Some(key) = explicit
        && !key.trim().is_empty()
    {
        return Ok(key.trim().to_string());
    }
    if let Ok(entry) = keyring::Entry::new(credential_target, "api_key")
        && let Ok(key) = entry.get_password()
        && !key.trim().is_empty()
    {
        return Ok(key);
    }
    for env in env_names {
        if let Ok(key) = std::env::var(env)
            && !key.trim().is_empty()
        {
            return Ok(key);
        }
    }
    Err(ProviderError::NotInstalled(format!(
        "API key not found. Set {} in Preferences or environment.",
        env_names.join(" / ")
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn web_claims() -> serde_json::Map<String, Value> {
        serde_json::from_value(serde_json::json!({
            "exp": 1_900_000_000,
            "userType": "paid",
            "bundledCredits": 80,
            "veniceCredits": 120,
            "bundledCreditsUsage": {
                "usedThisCycle": 12,
                "monthlyRefillCredits": 100,
                "availableCredits": 88,
                "tierCap": 200,
                "nextRefillAt": 1_900_000_000_000i64
            }
        }))
        .unwrap()
    }

    #[test]
    fn venice_snapshot_uses_diem_allocation() {
        let snapshot = snapshot_from_balance(&VeniceBalanceResponse {
            can_consume: true,
            consumption_currency: Some("DIEM".into()),
            balances: VeniceBalances {
                diem: Some(25.0),
                usd: None,
            },
            diem_epoch_allocation: Some(100.0),
        });
        assert_eq!(snapshot.primary.used_percent, 75.0);
    }

    #[test]
    fn session_cookie_prefers_exact_and_reassembles_contiguous_chunks() {
        assert_eq!(
            session_cookie_header(
                "other=x; __venice-auth.session-token.0=ab; __venice-auth.session-token.1=cd"
            ),
            Some("__venice-auth.session-token=abcd".to_string())
        );
        assert_eq!(
            session_cookie_header(
                "__venice-auth.session-token.0=ab; __venice-auth.session-token.2=cd"
            ),
            None
        );
        assert_eq!(
            session_cookie_header(
                "__venice-auth.session-token=exact; __venice-auth.session-token.0=chunk"
            ),
            Some("__venice-auth.session-token=exact".to_string())
        );
        assert_eq!(
            session_cookie_header("__venice-auth.session-token.0=a\nsecret"),
            None
        );
        assert_eq!(
            session_cookie_header(
                "__venice-auth.session-token=one; __venice-auth.session-token=two"
            ),
            None
        );
        assert_eq!(
            session_cookie_header("__venice-auth.session-token.not-a-chunk=value"),
            None
        );
        let oversized = format!(
            "__venice-auth.session-token={}",
            "x".repeat(MAX_VENICE_COOKIE_VALUE_LEN + 1)
        );
        assert_eq!(session_cookie_header(&oversized), None);
    }

    #[test]
    fn web_claims_produce_display_details_without_quota_math() {
        let (snapshot, details) = snapshot_from_web_claims(
            &web_claims(),
            DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap(),
        )
        .unwrap();

        assert!(snapshot.primary.is_informational);
        assert_eq!(details.len(), 6);
        assert_eq!(details[0].value(), "88");
        assert_eq!(
            details[2].progress().map(|progress| progress.total()),
            Some(100.0)
        );
    }

    #[test]
    fn web_claims_reject_expired_anonymous_and_missing_usage() {
        let now = DateTime::<Utc>::from_timestamp(1_900_000_000, 0).unwrap();
        let mut expired = web_claims();
        expired.insert("exp".into(), Value::from(1_800_000_000));
        assert!(matches!(
            snapshot_from_web_claims(&expired, now),
            Err(ProviderError::AuthRequired)
        ));

        let mut anonymous = web_claims();
        anonymous.insert("userType".into(), Value::from("guest"));
        assert!(matches!(
            snapshot_from_web_claims(&anonymous, now),
            Err(ProviderError::AuthRequired)
        ));

        let mut missing = web_claims();
        missing.remove("bundledCreditsUsage");
        assert!(matches!(
            snapshot_from_web_claims(&missing, now),
            Err(ProviderError::Parse(_))
        ));
    }
}
