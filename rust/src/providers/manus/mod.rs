//! Manus provider implementation.
//!
//! Fetches credit balance using a Manus browser session token.

use std::collections::HashSet;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::Client;
use reqwest::header::HeaderValue;
use serde::{Deserialize, Deserializer};
use serde_json::Value;

use crate::core::{
    FetchContext, ManualEmptyCookiePolicy, Provider, ProviderError, ProviderFetchResult,
    ProviderId, RateWindow, SourceMode, UsageSnapshot,
};

const MANUS_CREDITS_URL: &str = "https://api.manus.im/user.v1.UserService/GetAvailableCredits";
const MANUS_COOKIE_DOMAIN: &str = "manus.im";

/// Environment fallbacks in upstream `ManusSettingsReader` order: a token
/// variable first, then a full cookie header.
const ENV_TOKEN_NAMES: [&str; 2] = ["MANUS_SESSION_TOKEN", "MANUS_SESSION_ID"];
const ENV_COOKIE_NAME: &str = "MANUS_COOKIE";

/// A usable credits payload carries at least one of these keys.
const CREDIT_KEYS: [&str; 8] = [
    "totalCredits",
    "freeCredits",
    "periodicCredits",
    "addonCredits",
    "refreshCredits",
    "maxRefreshCredits",
    "proMonthlyCredits",
    "eventCredits",
];

/// Numeric `nextRefreshTime` values count seconds from Foundation's 2001
/// reference date; this is the offset to the Unix epoch.
const LEGACY_REFERENCE_EPOCH_OFFSET_SECS: f64 = 978_307_200.0;

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ManusCreditsResponse {
    #[serde(default, deserialize_with = "lenient_number")]
    total_credits: f64,
    #[serde(default, deserialize_with = "lenient_number")]
    free_credits: f64,
    #[serde(default, deserialize_with = "lenient_number")]
    periodic_credits: f64,
    #[serde(default, deserialize_with = "lenient_number")]
    addon_credits: f64,
    #[serde(default, deserialize_with = "lenient_number")]
    refresh_credits: f64,
    #[serde(default, deserialize_with = "lenient_number")]
    max_refresh_credits: f64,
    #[serde(default, deserialize_with = "lenient_number")]
    pro_monthly_credits: f64,
    #[serde(default, deserialize_with = "lenient_number")]
    event_credits: f64,
    #[serde(default, deserialize_with = "lenient_refresh_time")]
    next_refresh_time: Option<DateTime<Utc>>,
    #[serde(default, deserialize_with = "lenient_string")]
    refresh_interval: Option<String>,
}

/// Numbers may arrive as JSON numbers or numeric strings. Anything else, or a
/// non-finite value, counts as zero (upstream's `number()` helper).
fn lenient_number<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f64, D::Error> {
    let parsed = match Value::deserialize(deserializer)? {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.trim().parse::<f64>().ok(),
        _ => None,
    };
    Ok(parsed.filter(|value| value.is_finite()).unwrap_or(0.0))
}

fn lenient_string<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<String>, D::Error> {
    Ok(match Value::deserialize(deserializer)? {
        Value::String(text) if !text.is_empty() => Some(text),
        _ => None,
    })
}

fn legacy_reset_time(reference_seconds: f64) -> Option<DateTime<Utc>> {
    let unix_seconds = reference_seconds + LEGACY_REFERENCE_EPOCH_OFFSET_SECS;
    if !unix_seconds.is_finite() {
        return None;
    }
    #[allow(
        clippy::cast_possible_truncation,
        reason = "out-of-range values saturate and are rejected by from_timestamp; sub-second precision is irrelevant for a reset date"
    )]
    DateTime::from_timestamp(unix_seconds as i64, 0)
}

/// A numeric reset is a legacy reference-epoch offset; a string must be an
/// ISO-8601 timestamp. Any other shape means no reset date.
fn lenient_refresh_time<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<DateTime<Utc>>, D::Error> {
    Ok(match Value::deserialize(deserializer)? {
        Value::Number(number) => number.as_f64().and_then(legacy_reset_time),
        Value::String(text) => DateTime::parse_from_rfc3339(text.trim())
            .ok()
            .map(|parsed| parsed.with_timezone(&Utc)),
        _ => None,
    })
}

/// Tokens already tried in this refresh, and whether any was rejected.
#[derive(Default)]
struct Attempts {
    seen: HashSet<String>,
    rejected: bool,
}

pub struct ManusProvider {
    client: Client,
    credits_url: String,
}

impl ManusProvider {
    pub fn new() -> Self {
        Self {
            client: crate::core::credentialed_http_client_builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap_or_else(|_| Client::new()),
            credits_url: MANUS_CREDITS_URL.to_string(),
        }
    }

    /// One credits request. A 401/403 is `AuthRequired` so callers can move on
    /// to the next candidate; every other failure is final.
    async fn fetch_with_token(&self, token: &str) -> Result<UsageSnapshot, ProviderError> {
        let response = self
            .client
            .post(&self.credits_url)
            .bearer_auth(token)
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .header("Origin", "https://manus.im")
            .header("Referer", "https://manus.im/")
            .header("Connect-Protocol-Version", "1")
            .body("{}")
            .send()
            .await?;

        if response.status() == reqwest::StatusCode::UNAUTHORIZED
            || response.status() == reqwest::StatusCode::FORBIDDEN
        {
            return Err(ProviderError::AuthRequired);
        }
        if !response.status().is_success() {
            return Err(ProviderError::Other(format!(
                "Manus API returned status {}",
                response.status()
            )));
        }

        let body: Value = response
            .json()
            .await
            .map_err(|e| ProviderError::Parse(format!("Failed to parse Manus response: {e}")))?;
        let credits = parse_credits(&body)?;
        Ok(snapshot_from_credits(&credits))
    }

    /// Try one raw header or bare token. `Ok(None)` means "nothing usable
    /// here, keep going": no `session_id`, a token already tried, or a
    /// rejected session.
    async fn try_candidate(
        &self,
        attempts: &mut Attempts,
        raw: &str,
    ) -> Result<Option<UsageSnapshot>, ProviderError> {
        let Some(token) = session_token(raw) else {
            return Ok(None);
        };
        if !attempts.seen.insert(token.clone()) {
            return Ok(None);
        }
        match self.fetch_with_token(&token).await {
            Ok(snapshot) => Ok(Some(snapshot)),
            Err(ProviderError::AuthRequired) => {
                attempts.rejected = true;
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }

    /// A pasted or token-account header is the user's explicit choice: try only
    /// it, never other browser sessions or the environment.
    async fn fetch_manual(&self, header: &str) -> Result<UsageSnapshot, ProviderError> {
        let mut attempts = Attempts::default();
        match self.try_candidate(&mut attempts, header).await? {
            Some(snapshot) => Ok(snapshot),
            None if attempts.rejected => Err(ProviderError::AuthRequired),
            None => Err(ProviderError::NoCookies),
        }
    }

    /// Automatic source: every browser session in turn (a rejected one moves
    /// on to the next), then the environment fallback.
    async fn fetch_automatic(
        &self,
        browser_candidates: Result<Vec<(String, String)>, ProviderError>,
        env: impl Fn(&str) -> Option<String>,
    ) -> Result<UsageSnapshot, ProviderError> {
        let mut attempts = Attempts::default();
        // A browser read failure must not hide the environment fallback; keep
        // it and report it only if nothing else works.
        let mut browser_error = None;
        match browser_candidates {
            Ok(candidates) => {
                for (source_label, header) in candidates {
                    if let Some(snapshot) = self.try_candidate(&mut attempts, &header).await? {
                        return Ok(snapshot);
                    }
                    tracing::debug!(source = %source_label, "Manus browser session not usable");
                }
            }
            Err(ProviderError::NoCookies) => {}
            Err(error) => browser_error = Some(error),
        }

        if let Some(raw) = env_session_credential(&env)
            && let Some(snapshot) = self.try_candidate(&mut attempts, &raw).await?
        {
            return Ok(snapshot);
        }

        if attempts.rejected {
            Err(ProviderError::AuthRequired)
        } else {
            Err(browser_error.unwrap_or(ProviderError::NoCookies))
        }
    }
}

/// Extract the `session_id` bearer token from a cookie header, or accept a
/// bare token. A chunk without `=` is skipped instead of aborting the scan.
fn session_token(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let token = if trimmed.contains('=') || trimmed.contains(';') {
        trimmed.split(';').find_map(|chunk| {
            let (name, value) = chunk.split_once('=')?;
            let value = value.trim();
            (name.trim().eq_ignore_ascii_case("session_id") && !value.is_empty())
                .then(|| value.to_string())
        })?
    } else {
        trimmed.to_string()
    };
    // A candidate that cannot be a bearer value would fail the request build
    // and abort the whole scan; treat it as unusable instead.
    HeaderValue::from_str(&format!("Bearer {token}")).ok()?;
    Some(token)
}

/// Environment credential: `MANUS_SESSION_TOKEN`, then `MANUS_SESSION_ID`,
/// then a `MANUS_COOKIE` header, each cleaned of whitespace and quotes.
fn env_session_credential(env: &impl Fn(&str) -> Option<String>) -> Option<String> {
    let cleaned = |name: &str| {
        env(name)
            .map(|value| clean_env_value(&value))
            .filter(|value| !value.is_empty())
    };
    ENV_TOKEN_NAMES
        .iter()
        .find_map(|name| cleaned(name).filter(|value| session_token(value).is_some()))
        .or_else(|| cleaned(ENV_COOKIE_NAME))
}

fn clean_env_value(value: &str) -> String {
    let trimmed = value.trim();
    for quote in ['"', '\''] {
        if let Some(inner) = trimmed
            .strip_prefix(quote)
            .and_then(|rest| rest.strip_suffix(quote))
        {
            return inner.trim().to_string();
        }
    }
    trimmed.to_string()
}

fn parse_credits(body: &Value) -> Result<ManusCreditsResponse, ProviderError> {
    let data = ["data", "result", "response", "availableCredits"]
        .iter()
        .find_map(|key| body.get(key).filter(|value| !value.is_null()))
        .unwrap_or(body);
    let has_credit_key = data
        .as_object()
        .is_some_and(|map| CREDIT_KEYS.iter().any(|key| map.contains_key(*key)));
    if !has_credit_key {
        return Err(ProviderError::Parse(
            "Manus response missing expected credits fields".into(),
        ));
    }
    serde_json::from_value(data.clone())
        .map_err(|e| ProviderError::Parse(format!("Failed to parse Manus credits: {e}")))
}

fn snapshot_from_credits(credits: &ManusCreditsResponse) -> UsageSnapshot {
    let primary = if credits.pro_monthly_credits > 0.0 {
        let used = (credits.pro_monthly_credits - credits.periodic_credits).max(0.0);
        let percent = used / credits.pro_monthly_credits * 100.0;
        RateWindow::with_details(
            percent,
            None,
            None,
            Some(format!(
                "{:.0} total credits ({:.0} free)",
                credits.total_credits, credits.free_credits
            )),
        )
    } else {
        RateWindow::with_details(
            0.0,
            None,
            None,
            Some(format!("{:.0} credits available", credits.total_credits)),
        )
    };

    let mut snapshot = UsageSnapshot::new(primary).with_login_method(format!(
        "Balance: {:.0} credits",
        credits.total_credits.round()
    ));

    if credits.max_refresh_credits > 0.0 {
        let used = (credits.max_refresh_credits - credits.refresh_credits).max(0.0);
        let mut secondary = RateWindow::with_details(
            used / credits.max_refresh_credits * 100.0,
            None,
            credits.next_refresh_time,
            Some(format!(
                "{:.0}/{:.0} refresh credits{}",
                credits.refresh_credits,
                credits.max_refresh_credits,
                credits
                    .refresh_interval
                    .as_deref()
                    .map(|value| format!(" ({value})"))
                    .unwrap_or_default()
            )),
        );
        if !secondary.used_percent.is_finite() {
            secondary.used_percent = 0.0;
        }
        snapshot = snapshot.with_secondary(secondary);
    }

    if credits.addon_credits > 0.0 {
        let mut addon = RateWindow::new(0.0);
        addon.reset_description = Some(format!("{:.0} add-on credits", credits.addon_credits));
        snapshot = snapshot.with_extra_rate_window("addon", "Add-on credits", addon);
    }
    if credits.event_credits > 0.0 {
        let mut event = RateWindow::new(0.0);
        event.reset_description = Some(format!("{:.0} event credits", credits.event_credits));
        snapshot = snapshot.with_extra_rate_window("event", "Event credits", event);
    }
    snapshot
}

impl Default for ManusProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for ManusProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Manus
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        match ctx.source_mode {
            SourceMode::Auto | SourceMode::Web => {
                let snapshot = if let Some(header) = ctx.manual_cookie_header.as_deref() {
                    self.fetch_manual(header).await?
                } else if ctx.manual_cookie_missing {
                    // Manual source with nothing stored: never import a browser
                    // session or read the environment on the user's behalf.
                    return Err(ProviderError::Other(
                        "Manus cookie source is Manual, but no session cookie is configured. \
                         Paste a Cookie header containing session_id, or set Cookie source to Auto."
                            .into(),
                    ));
                } else {
                    self.fetch_automatic(
                        crate::providers::browser_cookie_headers_for_domain(MANUS_COOKIE_DOMAIN),
                        |name| std::env::var(name).ok(),
                    )
                    .await?
                };
                Ok(ProviderFetchResult::new(snapshot, "web"))
            }
            SourceMode::OAuth | SourceMode::Cli => {
                Err(ProviderError::UnsupportedSource(ctx.source_mode))
            }
        }
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::Web]
    }

    /// The provider iterates every browser session itself, so the shell must
    /// not pre-resolve a single merged header for it.
    fn owns_browser_cookie_resolution(&self) -> bool {
        true
    }

    fn manual_empty_cookie_policy(&self) -> ManualEmptyCookiePolicy {
        ManualEmptyCookiePolicy::FailClosedWeb
    }
}

#[cfg(test)]
mod tests;
