//! StepFun provider implementation.
//!
//! Supports an existing Oasis-Token via Preferences/environment. The upstream
//! username/password login flow is intentionally not automated in the Windows
//! shell yet; storing the resulting token keeps the provider usable without
//! retaining a password.

use async_trait::async_trait;
use chrono::{DateTime, TimeZone, Utc};
use reqwest::Client;
use serde::Deserialize;

use crate::core::{
    FetchContext, Provider, ProviderError, ProviderFetchResult, ProviderId, ProviderMetadata,
    RateWindow, SourceMode, UsageSnapshot,
};

const STEPFUN_RATE_LIMIT_URL: &str =
    "https://platform.stepfun.com/api/step.openapi.devcenter.Dashboard/QueryStepPlanRateLimit";
const STEPFUN_PLAN_STATUS_URL: &str =
    "https://platform.stepfun.com/api/step.openapi.devcenter.Dashboard/GetStepPlanStatus";
const STEPFUN_REFRESH_TOKEN_URL: &str =
    "https://platform.stepfun.com/passport/proto.api.passport.v1.PassportService/RefreshToken";
const STEPFUN_CREDENTIAL_TARGET: &str = "codexbar-stepfun";
const STEPFUN_WEB_ID: &str = "734152690100432";
const STEPFUN_APP_ID: &str = "111003695";

#[derive(Debug, Deserialize)]
struct StepFunRateLimitResponse {
    status: Option<i64>,
    code: Option<i64>,
    message: Option<String>,
    desc: Option<String>,
    five_hour_usage_left_rate: Option<FlexibleNumber>,
    weekly_usage_left_rate: Option<FlexibleNumber>,
    five_hour_usage_reset_time: Option<FlexibleTimestamp>,
    weekly_usage_reset_time: Option<FlexibleTimestamp>,
}

#[derive(Debug, Deserialize)]
struct FlexibleNumber(#[serde(deserialize_with = "deserialize_f64")] f64);

#[derive(Debug, Deserialize)]
struct FlexibleTimestamp(#[serde(deserialize_with = "deserialize_i64")] i64);

#[derive(Debug, Deserialize)]
struct StepFunPlanStatusResponse {
    status: Option<i64>,
    subscription: Option<StepFunSubscription>,
}

#[derive(Debug, Deserialize)]
struct StepFunSubscription {
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StepFunRefreshTokenResponse {
    access_token: Option<StepFunTokenPair>,
    refresh_token: Option<StepFunTokenPair>,
}

#[derive(Debug, Deserialize)]
struct StepFunTokenPair {
    raw: String,
}

pub struct StepFunProvider {
    metadata: ProviderMetadata,
    client: Client,
}

impl StepFunProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: ProviderId::StepFun,
                display_name: "StepFun",
                session_label: "5-hour",
                weekly_label: "Weekly",
                supports_opus: false,
                supports_credits: false,
                default_enabled: false,
                is_primary: false,
                dashboard_url: Some("https://platform.stepfun.com/dashboard"),
                status_page_url: None,
            },
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }

    fn token(api_key: Option<&str>) -> Result<String, ProviderError> {
        resolve_token(
            api_key,
            STEPFUN_CREDENTIAL_TARGET,
            &["STEPFUN_OASIS_TOKEN", "STEPFUN_TOKEN"],
        )
    }

    async fn fetch_token(&self, token: &str) -> Result<UsageSnapshot, ProviderError> {
        match self.fetch_token_once(token).await {
            Ok(snapshot) => Ok(snapshot),
            Err(error)
                if is_authentication_failure(&error)
                    && token_parts(token).refresh_token.is_some() =>
            {
                let refreshed = self.refresh_token(token).await?;
                self.persist_refreshed_token(&refreshed);
                self.fetch_token_once(&refreshed)
                    .await
                    .map_err(|retry_error| {
                        if is_authentication_failure(&retry_error) {
                            ProviderError::AuthRequired
                        } else {
                            retry_error
                        }
                    })
            }
            Err(error) => Err(error),
        }
    }

    async fn fetch_token_once(&self, token: &str) -> Result<UsageSnapshot, ProviderError> {
        let normalized = normalize_token(token);
        let rate_limit = self
            .post_json::<StepFunRateLimitResponse>(STEPFUN_RATE_LIMIT_URL, &normalized)
            .await?;
        let plan_name = self
            .post_json::<StepFunPlanStatusResponse>(STEPFUN_PLAN_STATUS_URL, &normalized)
            .await
            .ok()
            .and_then(|response| {
                (response.status == Some(1))
                    .then_some(response.subscription)
                    .flatten()
                    .and_then(|subscription| subscription.name)
            });
        snapshot_from_response(&rate_limit, plan_name)
    }

    async fn refresh_token(&self, token: &str) -> Result<String, ProviderError> {
        let normalized = normalize_token(token);
        let response = self
            .post_json::<StepFunRefreshTokenResponse>(STEPFUN_REFRESH_TOKEN_URL, &normalized)
            .await?;
        let access = response
            .access_token
            .map(|token| token.raw)
            .filter(|token| !token.trim().is_empty())
            .ok_or_else(|| ProviderError::AuthRequired)?;
        Ok(combined_token(
            &access,
            response
                .refresh_token
                .as_ref()
                .map(|token| token.raw.as_str()),
        ))
    }

    fn persist_refreshed_token(&self, token: &str) {
        if let Ok(entry) = keyring::Entry::new(STEPFUN_CREDENTIAL_TARGET, "api_key")
            && let Err(error) = entry.set_password(token)
        {
            tracing::debug!("Could not persist refreshed StepFun token: {error}");
        }
    }

    async fn post_json<T: for<'de> Deserialize<'de>>(
        &self,
        url: &str,
        token: &str,
    ) -> Result<T, ProviderError> {
        let response = self
            .client
            .post(url)
            .header("content-type", "application/json")
            .header("oasis-appid", STEPFUN_APP_ID)
            .header("oasis-platform", "web")
            .header("oasis-webid", STEPFUN_WEB_ID)
            .header(
                "Cookie",
                format!("Oasis-Token={token}; Oasis-Webid={STEPFUN_WEB_ID}"),
            )
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
                "StepFun API returned status {}",
                response.status()
            )));
        }
        response
            .json::<T>()
            .await
            .map_err(|e| ProviderError::Parse(format!("Failed to parse StepFun response: {e}")))
    }
}

fn snapshot_from_response(
    response: &StepFunRateLimitResponse,
    plan_name: Option<String>,
) -> Result<UsageSnapshot, ProviderError> {
    if response.status != Some(1) {
        let msg = response
            .message
            .clone()
            .or_else(|| response.desc.clone())
            .or_else(|| response.code.map(|code| code.to_string()))
            .unwrap_or_else(|| "unknown".into());
        if is_authentication_message(&msg) {
            return Err(ProviderError::AuthRequired);
        }
        return Err(ProviderError::Other(format!("StepFun API error: {msg}")));
    }

    let five_left = response
        .five_hour_usage_left_rate
        .as_ref()
        .ok_or_else(|| ProviderError::Parse("Missing StepFun five-hour usage".into()))?
        .0;
    let weekly_left = response
        .weekly_usage_left_rate
        .as_ref()
        .ok_or_else(|| ProviderError::Parse("Missing StepFun weekly usage".into()))?
        .0;
    let five_reset = response
        .five_hour_usage_reset_time
        .as_ref()
        .and_then(|ts| Utc.timestamp_opt(ts.0, 0).single());
    let weekly_reset = response
        .weekly_usage_reset_time
        .as_ref()
        .and_then(|ts| Utc.timestamp_opt(ts.0, 0).single());

    let primary = RateWindow::with_details(
        (1.0 - five_left).clamp(0.0, 1.0) * 100.0,
        Some(300),
        five_reset,
        five_reset.map(reset_description),
    );
    let secondary = RateWindow::with_details(
        (1.0 - weekly_left).clamp(0.0, 1.0) * 100.0,
        Some(10080),
        weekly_reset,
        weekly_reset.map(reset_description),
    );

    let mut snapshot = UsageSnapshot::new(primary).with_secondary(secondary);
    if let Some(plan_name) = plan_name.filter(|value| !value.trim().is_empty()) {
        snapshot = snapshot.with_login_method(plan_name);
    } else {
        snapshot = snapshot.with_login_method("Oasis-Token");
    }
    Ok(snapshot)
}

struct StepFunTokenParts {
    access_token: String,
    refresh_token: Option<String>,
}

fn token_parts(token: &str) -> StepFunTokenParts {
    let normalized = normalize_token(token);
    let (access_token, refresh_token) = normalized
        .split_once("...")
        .map(|(access, refresh)| {
            (
                access.trim().to_string(),
                Some(refresh.trim().to_string()).filter(|value| !value.is_empty()),
            )
        })
        .unwrap_or_else(|| (normalized.trim().to_string(), None));
    StepFunTokenParts {
        access_token,
        refresh_token,
    }
}

fn normalize_token(raw: &str) -> String {
    let trimmed = raw.trim();
    if let Some((_, tail)) = trimmed.split_once("Oasis-Token=") {
        return tail.split(';').next().unwrap_or(tail).trim().to_string();
    }
    trimmed.to_string()
}

fn combined_token(access_token: &str, refresh_token: Option<&str>) -> String {
    match refresh_token
        .map(str::trim)
        .filter(|token| !token.is_empty())
    {
        Some(refresh_token) => format!("{}...{}", access_token.trim(), refresh_token),
        None => access_token.trim().to_string(),
    }
}

fn is_authentication_failure(error: &ProviderError) -> bool {
    matches!(error, ProviderError::AuthRequired)
        || match error {
            ProviderError::Other(message) | ProviderError::Parse(message) => {
                is_authentication_message(message)
            }
            _ => false,
        }
}

fn is_authentication_message(message: &str) -> bool {
    let lower = message.to_lowercase();
    lower.contains("401")
        || lower.contains("403")
        || lower.contains("unauthorized")
        || lower.contains("unauthenticated")
        || lower.contains("invalid credentials")
        || lower.contains("invalid token")
        || lower.contains("token expired")
        || lower.contains("expired token")
}

fn reset_description(date: DateTime<Utc>) -> String {
    let now = Utc::now();
    if date <= now {
        return "resets now".into();
    }
    let duration = date - now;
    let hours = duration.num_hours();
    let minutes = duration.num_minutes() % 60;
    if hours > 0 {
        format!("resets in {hours}h {minutes}m")
    } else {
        format!("resets in {minutes}m")
    }
}

fn deserialize_f64<'de, D>(deserializer: D) -> Result<f64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    match value {
        serde_json::Value::Number(n) => n
            .as_f64()
            .ok_or_else(|| serde::de::Error::custom("invalid number")),
        serde_json::Value::String(s) => s
            .parse::<f64>()
            .map_err(|_| serde::de::Error::custom("invalid number string")),
        _ => Ok(0.0),
    }
}

fn deserialize_i64<'de, D>(deserializer: D) -> Result<i64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    match value {
        serde_json::Value::Number(n) => n
            .as_i64()
            .ok_or_else(|| serde::de::Error::custom("invalid timestamp")),
        serde_json::Value::String(s) => s
            .parse::<i64>()
            .map_err(|_| serde::de::Error::custom("invalid timestamp string")),
        _ => Ok(0),
    }
}

impl Default for StepFunProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for StepFunProvider {
    fn id(&self) -> ProviderId {
        ProviderId::StepFun
    }

    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        match ctx.source_mode {
            SourceMode::Auto | SourceMode::OAuth => {
                let token = Self::token(ctx.api_key.as_deref())?;
                Ok(ProviderFetchResult::new(
                    self.fetch_token(&token).await?,
                    "api",
                ))
            }
            SourceMode::Web | SourceMode::Cli => {
                Err(ProviderError::UnsupportedSource(ctx.source_mode))
            }
        }
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::OAuth]
    }
}

fn resolve_token(
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
        "StepFun token not found. Set {} in Preferences or environment.",
        env_names.join(" / ")
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stepfun_snapshot_converts_left_rates_to_used_percent() {
        let response = StepFunRateLimitResponse {
            status: Some(1),
            code: None,
            message: None,
            desc: None,
            five_hour_usage_left_rate: Some(FlexibleNumber(0.25)),
            weekly_usage_left_rate: Some(FlexibleNumber(0.75)),
            five_hour_usage_reset_time: Some(FlexibleTimestamp(1_800_000_000)),
            weekly_usage_reset_time: Some(FlexibleTimestamp(1_800_000_000)),
        };
        let snapshot = snapshot_from_response(&response, Some("Step Plan".into())).unwrap();
        assert_eq!(snapshot.primary.used_percent, 75.0);
        assert_eq!(snapshot.secondary.unwrap().used_percent, 25.0);
    }

    #[test]
    fn stepfun_token_parts_extract_cookie_and_refresh_token() {
        let parts = token_parts("Cookie: Oasis-Token=access...refresh; Oasis-Webid=abc");
        assert_eq!(parts.access_token, "access");
        assert_eq!(parts.refresh_token.as_deref(), Some("refresh"));
        assert_eq!(
            combined_token("new-access", Some("new-refresh")),
            "new-access...new-refresh"
        );
    }

    #[test]
    fn stepfun_authentication_messages_are_actionable() {
        assert!(is_authentication_message("token expired"));
        assert!(is_authentication_message("HTTP 401"));
        assert!(!is_authentication_message("rate limit"));
    }
}
