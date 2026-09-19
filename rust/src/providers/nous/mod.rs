//! Nous Portal subscription provider.
//!
//! Nous Portal issues short-lived access tokens through the Hermes Agent
//! device-code login. The Windows port reads those credentials without
//! refreshing or writing them, then projects the account endpoint into the
//! monthly subscription-credit display used by the rest of the app.

use async_trait::async_trait;
use chrono::{DateTime, Datelike, Duration as ChronoDuration, Utc};
use futures::StreamExt;
use reqwest::{Client, StatusCode, Url};
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::time::{Duration, timeout};

use crate::core::{
    FetchContext, Provider, ProviderDisplayDetail, ProviderError, ProviderFetchResult, ProviderId,
    ProviderMetadata, RateWindow, SourceMode, SubscriptionMetadata, UsageSnapshot,
};

const DEFAULT_PORTAL_URL: &str = "https://portal.nousresearch.com";
const PORTAL_ACCOUNT_PATH: &str = "api/oauth/account";
const ACCESS_TOKEN_ENV: &str = "NOUS_PORTAL_ACCESS_TOKEN";
const PORTAL_URL_ENVS: &[&str] = &["NOUS_PORTAL_BASE_URL", "HERMES_PORTAL_BASE_URL"];
const HERMES_HOME_ENV: &str = "HERMES_HOME";
const MAX_RESPONSE_BYTES: usize = 512 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const EXPIRY_SKEW: i64 = 60;
const TRUSTED_PORTAL_HOST: &str = "nousresearch.com";

#[derive(Debug, Clone)]
struct Credential {
    token: String,
    portal_url: Url,
    expires_at: Option<DateTime<Utc>>,
}

impl Credential {
    fn is_expired(&self, now: DateTime<Utc>) -> bool {
        self.expires_at
            .is_some_and(|expires_at| expires_at <= now + ChronoDuration::seconds(EXPIRY_SKEW))
    }
}

#[derive(Debug, Clone)]
struct StoredCredential {
    token: String,
    portal_base_url: Option<String>,
    expires_at: Option<DateTime<Utc>>,
}

pub struct NousProvider {
    metadata: ProviderMetadata,
    client: Client,
}

impl NousProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: ProviderId::Nous,
                display_name: "Nous Portal",
                session_label: "Monthly credits",
                weekly_label: "Weekly",
                supports_opus: false,
                supports_credits: false,
                default_enabled: false,
                is_primary: false,
                dashboard_url: Some("https://portal.nousresearch.com/usage"),
                status_page_url: None,
            },
            client: crate::core::credentialed_http_client_builder()
                .timeout(REQUEST_TIMEOUT)
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }

    async fn fetch_api(
        &self,
        explicit_token: Option<&str>,
    ) -> Result<ProviderFetchResult, ProviderError> {
        let credential = resolve_credential(explicit_token)?;
        let endpoint = credential
            .portal_url
            .join(PORTAL_ACCOUNT_PATH)
            .map_err(|_| ProviderError::Parse("Nous Portal URL is invalid.".to_string()))?;
        let response = timeout(
            REQUEST_TIMEOUT,
            self.client
                .get(endpoint)
                .bearer_auth(&credential.token)
                .header("Accept", "application/json")
                .header("User-Agent", "CodexBar")
                .send(),
        )
        .await
        .map_err(|_| ProviderError::Timeout)??;

        let status = response.status();
        if status != StatusCode::OK {
            return Err(status_error(status));
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
        {
            return Err(ProviderError::Parse(
                "Nous Portal returned an oversized response.".to_string(),
            ));
        }

        let body = read_bounded_body(response).await?;
        parse_response(&body)
    }
}

impl Default for NousProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for NousProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Nous
    }

    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        match ctx.source_mode {
            SourceMode::Auto | SourceMode::OAuth => self.fetch_api(ctx.api_key.as_deref()).await,
            SourceMode::Web | SourceMode::Cli => {
                Err(ProviderError::UnsupportedSource(ctx.source_mode))
            }
        }
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::OAuth]
    }

    fn supports_oauth(&self) -> bool {
        true
    }
}

fn resolve_credential(explicit_token: Option<&str>) -> Result<Credential, ProviderError> {
    let environment: HashMap<String, String> = std::env::vars().collect();
    let home = dirs::home_dir().ok_or_else(missing_credentials)?;
    resolve_credential_from(explicit_token, &environment, &home, Utc::now())
}

fn resolve_credential_from(
    explicit_token: Option<&str>,
    environment: &HashMap<String, String>,
    home_directory: &Path,
    now: DateTime<Utc>,
) -> Result<Credential, ProviderError> {
    if let Some(token) = cleaned(explicit_token) {
        return usable_credential(token, resolve_portal_url(environment, None), now);
    }
    if let Some(token) = cleaned(environment.get(ACCESS_TOKEN_ENV).map(String::as_str)) {
        return usable_credential(token, resolve_portal_url(environment, None), now);
    }

    let candidates = auth_file_candidates(environment, home_directory);
    let mut saw_file = false;
    let mut expired: Option<Credential> = None;
    for path in &candidates {
        let Ok(contents) = std::fs::read(path) else {
            continue;
        };
        saw_file = true;
        let Some(stored) = parse_auth_file(&contents) else {
            continue;
        };
        let credential = Credential {
            expires_at: stored.expires_at.or_else(|| jwt_expiry(&stored.token)),
            portal_url: resolve_portal_url(environment, stored.portal_base_url.as_deref()),
            token: stored.token,
        };
        if credential.is_expired(now) {
            expired.get_or_insert(credential);
        } else {
            return Ok(credential);
        }
    }

    if expired.is_some() {
        return Err(ProviderError::OAuthExpired(
            "Nous Portal Hermes login expired. Run hermes to refresh it.".to_string(),
        ));
    }
    if saw_file {
        return Err(ProviderError::NotInstalled(
            "Nous Portal auth files contain no usable login. Run hermes to sign in again."
                .to_string(),
        ));
    }
    Err(missing_credentials())
}

fn usable_credential(
    token: String,
    portal_url: Url,
    now: DateTime<Utc>,
) -> Result<Credential, ProviderError> {
    let credential = Credential {
        expires_at: jwt_expiry(&token),
        token,
        portal_url,
    };
    if credential.is_expired(now) {
        return Err(ProviderError::OAuthExpired(
            "Nous Portal access token expired. Run hermes to refresh it.".to_string(),
        ));
    }
    Ok(credential)
}

fn missing_credentials() -> ProviderError {
    ProviderError::NotInstalled(
        "Nous Portal login not found. Run hermes to sign in, then refresh CodexBar.".to_string(),
    )
}

fn auth_file_candidates(
    environment: &HashMap<String, String>,
    home_directory: &Path,
) -> Vec<PathBuf> {
    let root = environment
        .get(HERMES_HOME_ENV)
        .and_then(|raw| cleaned(Some(raw.as_str())))
        .map(|raw| expand_home(&raw, home_directory))
        .unwrap_or_else(|| {
            let home = environment
                .get("HOME")
                .and_then(|raw| cleaned(Some(raw.as_str())))
                .map(|raw| expand_home(&raw, home_directory))
                .unwrap_or_else(|| home_directory.to_path_buf());
            home.join(".hermes")
        });
    vec![
        root.join("auth.json"),
        root.join("shared").join("nous_auth.json"),
    ]
}

fn expand_home(raw: &str, home_directory: &Path) -> PathBuf {
    if raw == "~" {
        return home_directory.to_path_buf();
    }
    if let Some(rest) = raw.strip_prefix("~/").or_else(|| raw.strip_prefix("~\\")) {
        return home_directory.join(rest);
    }
    PathBuf::from(raw)
}

fn parse_auth_file(contents: &[u8]) -> Option<StoredCredential> {
    let root: Value = serde_json::from_slice(contents).ok()?;
    let root_object = root.as_object()?;

    if let Some(providers) = root_object.get("providers").and_then(Value::as_object)
        && let Some(nous) = providers.get("nous")
        && let Some(stored) = stored_credential(nous)
    {
        return Some(stored);
    }

    if let Some(entries) = root_object
        .get("credential_pool")
        .and_then(Value::as_object)
        .and_then(|pool| pool.get("nous"))
        .and_then(Value::as_array)
    {
        return select_pool_credential(entries);
    }

    stored_credential(&root)
}

fn select_pool_credential(entries: &[Value]) -> Option<StoredCredential> {
    let mut selected: Option<(StoredCredential, i64, i64, i64)> = None;
    for entry in entries {
        let Some(stored) = stored_credential(entry) else {
            continue;
        };
        let agent_expiry = entry
            .get("agent_key_expires_at")
            .and_then(Value::as_str)
            .and_then(parse_iso)
            .map(|value| value.timestamp())
            .unwrap_or(0);
        let access_expiry = stored
            .expires_at
            .or_else(|| jwt_expiry(&stored.token))
            .map(|value| value.timestamp())
            .unwrap_or(0);
        let priority = entry.get("priority").and_then(Value::as_i64).unwrap_or(0);
        let should_replace =
            selected
                .as_ref()
                .is_none_or(|(_, old_agent, old_access, old_priority)| {
                    agent_expiry > *old_agent
                        || (agent_expiry == *old_agent
                            && (access_expiry > *old_access
                                || (access_expiry == *old_access && priority < *old_priority)))
                });
        if should_replace {
            selected = Some((stored, agent_expiry, access_expiry, priority));
        }
    }
    selected.map(|(stored, _, _, _)| stored)
}

fn stored_credential(value: &Value) -> Option<StoredCredential> {
    let object = value.as_object()?;
    let token = cleaned(object.get("access_token").and_then(Value::as_str))?;
    Some(StoredCredential {
        token,
        portal_base_url: cleaned(object.get("portal_base_url").and_then(Value::as_str)),
        expires_at: object
            .get("expires_at")
            .and_then(Value::as_str)
            .and_then(parse_iso),
    })
}

fn resolve_portal_url(environment: &HashMap<String, String>, stored: Option<&str>) -> Url {
    for key in PORTAL_URL_ENVS {
        if let Some(raw) = environment.get(*key).and_then(|value| cleaned(Some(value)))
            && let Some(url) = normalized_https_url(&raw)
        {
            return url;
        }
    }
    if let Some(raw) = stored.and_then(|value| cleaned(Some(value)))
        && let Some(url) = normalized_https_url(&raw)
        && is_trusted_portal_host(url.host_str())
    {
        return url;
    }
    Url::parse(DEFAULT_PORTAL_URL).expect("default Nous Portal URL is valid")
}

fn normalized_https_url(raw: &str) -> Option<Url> {
    let value = raw.trim_end_matches('/').trim();
    let url = Url::parse(value).ok()?;
    if url.scheme() != "https"
        || url.host_str().is_none_or(str::is_empty)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || (!url.path().is_empty() && url.path() != "/")
    {
        return None;
    }
    Some(url)
}

fn is_trusted_portal_host(host: Option<&str>) -> bool {
    let Some(host) = host.map(str::to_ascii_lowercase) else {
        return false;
    };
    host == TRUSTED_PORTAL_HOST || host.ends_with(&format!(".{TRUSTED_PORTAL_HOST}"))
}

fn cleaned(value: Option<&str>) -> Option<String> {
    let mut value = value?.trim().to_string();
    if value.len() >= 2
        && ((value.starts_with('"') && value.ends_with('"'))
            || (value.starts_with('\'') && value.ends_with('\'')))
    {
        value = value[1..value.len() - 1].trim().to_string();
    }
    (!value.is_empty()).then_some(value)
}

fn parse_iso(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|value| value.with_timezone(&Utc))
}

fn jwt_expiry(token: &str) -> Option<DateTime<Utc>> {
    use base64::Engine;

    let payload = token.split('.').nth(1)?;
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(payload))
        .ok()?;
    let claims: Value = serde_json::from_slice(&decoded).ok()?;
    let seconds = claims.get("exp")?.as_f64()?;
    if !seconds.is_finite() || seconds <= 0.0 {
        return None;
    }
    #[allow(
        clippy::cast_possible_truncation,
        reason = "JWT expiration is converted to whole epoch seconds"
    )]
    let seconds = seconds.trunc() as i64;
    DateTime::from_timestamp(seconds, 0)
}

fn status_error(status: StatusCode) -> ProviderError {
    match status {
        StatusCode::UNAUTHORIZED => ProviderError::OAuthExpired(
            "Nous Portal rejected the access token. Run hermes to refresh the Hermes login."
                .to_string(),
        ),
        StatusCode::FORBIDDEN => {
            ProviderError::Other("Nous Portal denied account access.".to_string())
        }
        StatusCode::TOO_MANY_REQUESTS => {
            ProviderError::Other("Nous Portal account requests are rate limited.".to_string())
        }
        status if status.is_server_error() => ProviderError::Other(format!(
            "Nous Portal API is unavailable (HTTP {}).",
            status.as_u16()
        )),
        status => ProviderError::Other(format!(
            "Nous Portal account API returned HTTP {}.",
            status.as_u16()
        )),
    }
}

async fn read_bounded_body(response: reqwest::Response) -> Result<Vec<u8>, ProviderError> {
    let mut stream = response.bytes_stream();
    let mut body = Vec::new();
    while let Some(chunk) = stream.next().await {
        append_bounded_body(&mut body, &chunk?)?;
    }
    Ok(body)
}

fn append_bounded_body(body: &mut Vec<u8>, chunk: &[u8]) -> Result<(), ProviderError> {
    if chunk.len() > MAX_RESPONSE_BYTES.saturating_sub(body.len()) {
        return Err(ProviderError::Parse(
            "Nous Portal returned an oversized response.".to_string(),
        ));
    }
    body.extend_from_slice(chunk);
    Ok(())
}

fn parse_response(body: &[u8]) -> Result<ProviderFetchResult, ProviderError> {
    let decoded: Value = serde_json::from_slice(body).map_err(|_| {
        ProviderError::Parse("Invalid Nous Portal account response: expected JSON.".to_string())
    })?;
    let root = decoded.as_object().ok_or_else(|| {
        ProviderError::Parse(
            "Invalid Nous Portal account response: expected an object.".to_string(),
        )
    })?;
    if root.get("error").is_some_and(is_truthy) {
        return Err(ProviderError::Other(
            "Nous Portal account endpoint reported an error.".to_string(),
        ));
    }

    let subscription = optional_object(root.get("subscription"), "subscription")?;
    let access = optional_object(root.get("paid_service_access"), "paid_service_access")?;
    let user = optional_object(root.get("user"), "user")?;
    let organization = optional_object(root.get("organisation"), "organisation")?;

    let monthly = number(
        subscription.and_then(|value| value.get("monthly_credits")),
        "monthly_credits",
    )?;
    if monthly.is_some_and(|value| value < 0.0) {
        return Err(parse_failure("monthly_credits"));
    }
    let remaining = number(
        subscription.and_then(|value| value.get("credits_remaining")),
        "credits_remaining",
    )?
    .or(number(
        access.and_then(|value| value.get("subscription_credits_remaining")),
        "subscription_credits_remaining",
    )?);
    let rollover = number(
        subscription.and_then(|value| value.get("rollover_credits")),
        "rollover_credits",
    )?;
    let purchased = number(
        root.get("purchased_credits_remaining"),
        "purchased_credits_remaining",
    )?
    .or(number(
        access.and_then(|value| value.get("purchased_credits_remaining")),
        "paid_service_access.purchased_credits_remaining",
    )?);
    let total = number(
        access.and_then(|value| value.get("total_usable_credits")),
        "total_usable_credits",
    )?;
    if [monthly, remaining, rollover, purchased, total]
        .iter()
        .all(Option::is_none)
    {
        return Err(parse_failure("no credit amounts"));
    }

    let renewal = optional_date(
        subscription.and_then(|value| value.get("current_period_end")),
        "current_period_end",
    )?;
    let primary = if let (Some(monthly), Some(remaining)) =
        (monthly.filter(|value| *value > 0.0), remaining)
    {
        let used = (monthly - remaining.max(0.0)).clamp(0.0, monthly);
        RateWindow::with_details(
            used / monthly * 100.0,
            RateWindow::monthly_window_minutes(renewal),
            renewal,
            None,
        )
    } else {
        RateWindow::informational(
            monthly
                .map(|value| format!("{} monthly grant", format_usd(value)))
                .or_else(|| total.map(|value| format!("{} total usable", format_usd(value))))
                .unwrap_or_else(|| "Nous Portal credits".to_string()),
        )
    };

    let plan = text(
        subscription.and_then(|value| value.get("plan")),
        "subscription.plan",
    )?;
    let active_subscription = access
        .and_then(|value| value.get("has_active_subscription"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let login_method = plan.or_else(|| active_subscription.then(|| "Subscription".to_string()));
    let email = text(user.and_then(|value| value.get("email")), "user.email")?;
    let organization_name = text(
        organization.and_then(|value| value.get("name")),
        "organisation.name",
    )?;

    let mut usage = UsageSnapshot::new(primary);
    if let Some(plan) = login_method {
        usage = usage.with_login_method(plan);
    }
    if let Some(email) = email {
        usage = usage.with_email(email);
    }
    if let Some(organization) = organization_name {
        usage = usage.with_organization(organization);
    }
    if renewal.is_some() {
        usage = usage.with_subscription(Some(SubscriptionMetadata::new(None, None, renewal)));
    }

    let mut result = ProviderFetchResult::new(usage, "api");
    if let Some(remaining) = remaining {
        let remaining = remaining.max(0.0);
        let mut detail = ProviderDisplayDetail::new(
            "subscription-credits",
            "Subscription credits",
            monthly
                .filter(|value| *value > 0.0)
                .map(|monthly| format!("{} of {} left", format_usd(remaining), format_usd(monthly)))
                .unwrap_or_else(|| format!("{} left", format_usd(remaining))),
        );
        if let Some(monthly) = monthly.filter(|value| *value > 0.0) {
            let used = (monthly - remaining).clamp(0.0, monthly);
            detail = detail.with_progress(used, monthly);
        }
        result = result.with_display_detail(detail);
    } else if let Some(monthly) = monthly {
        result = result.with_display_detail(ProviderDisplayDetail::new(
            "monthly-grant",
            "Monthly grant",
            format_usd(monthly),
        ));
    }
    if let Some(rollover) = rollover.filter(|value| *value > 0.0) {
        result = result.with_display_detail(ProviderDisplayDetail::new(
            "rollover-credits",
            "Rollover credits",
            format_usd(rollover),
        ));
    }
    if let Some(renewal) = renewal {
        result = result.with_display_detail(ProviderDisplayDetail::new(
            "renewal",
            "Renews",
            format_month_day(renewal),
        ));
    }
    if let Some(purchased) = purchased {
        result = result.with_display_detail(ProviderDisplayDetail::new(
            "top-up-credits",
            "Top-up credits",
            format_usd(purchased),
        ));
    }
    if let Some(total) = total {
        result = result.with_display_detail(ProviderDisplayDetail::new(
            "total-usable",
            "Total usable",
            format_usd(total),
        ));
    }
    Ok(result)
}

fn optional_object<'a>(
    value: Option<&'a Value>,
    field: &str,
) -> Result<Option<&'a Map<String, Value>>, ProviderError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_object()
            .map(Some)
            .ok_or_else(|| parse_failure(field)),
    }
}

fn number(value: Option<&Value>, field: &str) -> Result<Option<f64>, ProviderError> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    let parsed = match value {
        Value::Number(value) => value.as_f64(),
        Value::String(value) => value.trim().parse::<f64>().ok(),
        _ => None,
    };
    parsed
        .filter(|value| value.is_finite())
        .map(Some)
        .ok_or_else(|| parse_failure(field))
}

fn optional_date(
    value: Option<&Value>,
    field: &str,
) -> Result<Option<DateTime<Utc>>, ProviderError> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    let raw = value.as_str().ok_or_else(|| parse_failure(field))?;
    parse_iso(raw).map(Some).ok_or_else(|| parse_failure(field))
}

fn text(value: Option<&Value>, field: &str) -> Result<Option<String>, ProviderError> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    let Some(value) = value
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };
    if value.chars().count() > 256 || value.chars().any(char::is_control) {
        return Err(parse_failure(field));
    }
    Ok(Some(value.to_string()))
}

fn is_truthy(value: &Value) -> bool {
    match value {
        Value::Null | Value::Bool(false) => false,
        Value::Number(value) => value.as_f64().is_none_or(|number| number != 0.0),
        Value::String(value) => !value.is_empty(),
        Value::Array(_) | Value::Object(_) | Value::Bool(true) => true,
    }
}

fn format_usd(value: f64) -> String {
    let value = value.max(0.0);
    let prefix = "$";
    if value.abs() < 100.0 {
        format!("{prefix}{value:.2}")
    } else {
        format!("{prefix}{value:.0}")
    }
}

fn format_month_day(value: DateTime<Utc>) -> String {
    format!("{} {}", value.format("%b"), value.day())
}

fn parse_failure(field: impl Into<String>) -> ProviderError {
    ProviderError::Parse(format!(
        "Invalid Nous Portal account response: {}",
        field.into()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn environment(entries: &[(&str, &str)]) -> HashMap<String, String> {
        entries
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect()
    }

    fn success_payload() -> Value {
        serde_json::json!({
            "subscription": {
                "monthly_credits": 70,
                "credits_remaining": 61.5,
                "rollover_credits": 2,
                "current_period_end": "2026-10-18T12:00:00Z",
                "plan": "Pro"
            },
            "paid_service_access": {
                "subscription_credits_remaining": 61.5,
                "purchased_credits_remaining": 4.25,
                "total_usable_credits": 67.75,
                "has_active_subscription": true
            },
            "purchased_credits_remaining": 4.25,
            "user": {"email": "user@example.com"},
            "organisation": {"name": "Nous Research"},
            "secret": "must never appear in display data"
        })
    }

    #[test]
    fn metadata_and_sources_match_oauth_port() {
        let provider = NousProvider::new();
        assert_eq!(provider.id(), ProviderId::Nous);
        assert_eq!(provider.metadata().display_name, "Nous Portal");
        assert_eq!(provider.metadata().session_label, "Monthly credits");
        assert!(!provider.metadata().default_enabled);
        assert_eq!(
            provider.available_sources(),
            vec![SourceMode::Auto, SourceMode::OAuth]
        );
        assert!(provider.supports_oauth());
    }

    #[test]
    fn success_payload_maps_monthly_credits_identity_and_details() {
        let result = parse_response(&serde_json::to_vec(&success_payload()).unwrap()).unwrap();
        assert_eq!(result.source_label, "api");
        assert!((result.usage.primary.used_percent - 12.142857).abs() < 0.001);
        assert_eq!(
            result
                .usage
                .primary
                .resets_at
                .map(|value| value.to_rfc3339()),
            Some("2026-10-18T12:00:00+00:00".to_string())
        );
        assert_eq!(
            result.usage.account_email.as_deref(),
            Some("user@example.com")
        );
        assert_eq!(
            result.usage.account_organization.as_deref(),
            Some("Nous Research")
        );
        assert_eq!(result.usage.login_method.as_deref(), Some("Pro"));
        assert_eq!(
            result
                .usage
                .subscription
                .as_ref()
                .and_then(|value| value.renews_at),
            result.usage.primary.resets_at
        );
        let details: Vec<_> = result.display_details().collect();
        assert!(details.iter().any(|detail| {
            detail.id() == "subscription-credits"
                && detail.value().contains("$61.50")
                && detail.value().contains("$70.00")
        }));
        assert!(details.iter().any(|detail| detail.id() == "top-up-credits"));
        assert!(details.iter().any(|detail| detail.id() == "total-usable"));
        assert!(
            details
                .iter()
                .all(|detail| !detail.value().contains("must never appear"))
        );
    }

    #[test]
    fn fallback_credit_locations_and_informational_primary_are_supported() {
        let payload = serde_json::json!({
            "subscription": {"monthly_credits": 0},
            "paid_service_access": {
                "subscription_credits_remaining": 0,
                "purchased_credits_remaining": 3,
                "total_usable_credits": 3
            }
        });
        let result = parse_response(&serde_json::to_vec(&payload).unwrap()).unwrap();
        assert!(result.usage.primary.is_informational);
        assert!(
            result
                .display_details()
                .any(|detail| detail.id() == "top-up-credits")
        );
    }

    #[test]
    fn malformed_credit_fields_fail_closed_without_echoing_payload() {
        for payload in [
            serde_json::json!({"subscription": {"monthly_credits": "nope"}}),
            serde_json::json!({"subscription": {"monthly_credits": -1}}),
            serde_json::json!({"subscription": {"current_period_end": 42}, "purchased_credits_remaining": 1}),
            serde_json::json!({"subscription": {}, "paid_service_access": {}}),
        ] {
            let error = parse_response(&serde_json::to_vec(&payload).unwrap()).unwrap_err();
            assert!(matches!(error, ProviderError::Parse(_)));
            assert!(!error.to_string().contains("nope"));
        }
    }

    #[test]
    fn endpoint_errors_are_classified_without_response_body() {
        assert!(matches!(
            status_error(StatusCode::UNAUTHORIZED),
            ProviderError::OAuthExpired(_)
        ));
        assert!(
            status_error(StatusCode::FORBIDDEN)
                .to_string()
                .contains("denied")
        );
        assert!(
            status_error(StatusCode::TOO_MANY_REQUESTS)
                .to_string()
                .contains("rate limited")
        );
        assert!(
            status_error(StatusCode::INTERNAL_SERVER_ERROR)
                .to_string()
                .contains("unavailable")
        );
        assert!(
            status_error(StatusCode::BAD_REQUEST)
                .to_string()
                .contains("HTTP 400")
        );
    }

    #[test]
    fn streaming_response_cap_rejects_oversized_chunk_without_content_length() {
        let mut body = vec![0_u8; MAX_RESPONSE_BYTES];
        assert!(append_bounded_body(&mut body, &[0]).is_err());
    }

    #[test]
    fn auth_file_supports_hermes_provider_state_and_custom_home() {
        let dir = tempdir().unwrap();
        let auth = dir.path().join("auth.json");
        fs::write(
            &auth,
            r#"{"providers":{"nous":{"access_token":"token-value","portal_base_url":"https://api.nousresearch.com","expires_at":"2099-01-01T00:00:00Z"}}}"#,
        )
        .unwrap();
        let env = environment(&[(HERMES_HOME_ENV, dir.path().to_str().unwrap())]);
        let credential =
            resolve_credential_from(None, &env, Path::new("C:\\unused"), Utc::now()).unwrap();
        assert_eq!(credential.token, "token-value");
        assert_eq!(
            credential.portal_url.host_str(),
            Some("api.nousresearch.com")
        );
    }

    #[test]
    fn credential_pool_uses_agent_expiry_then_access_expiry_then_priority() {
        let payload = serde_json::json!({
            "credential_pool": {
                "nous": [
                    {"access_token": "first", "agent_key_expires_at": "2026-10-01T00:00:00Z", "expires_at": "2026-12-01T00:00:00Z", "priority": 0},
                    {"access_token": "second", "agent_key_expires_at": "2026-11-01T00:00:00Z", "expires_at": "2026-10-01T00:00:00Z", "priority": 10},
                    {"access_token": "third", "agent_key_expires_at": "2026-11-01T00:00:00Z", "expires_at": "2026-10-01T00:00:00Z", "priority": 1}
                ]
            }
        });
        let stored = parse_auth_file(&serde_json::to_vec(&payload).unwrap()).unwrap();
        assert_eq!(stored.token, "third");
    }

    #[test]
    fn explicit_hermes_home_is_exclusive_and_expired_tokens_fail_closed() {
        let fallback = tempdir().unwrap();
        let custom = tempdir().unwrap();
        fs::create_dir_all(fallback.path().join(".hermes")).unwrap();
        fs::write(
            fallback.path().join(".hermes").join("auth.json"),
            r#"{"access_token":"fallback"}"#,
        )
        .unwrap();
        fs::write(
            custom.path().join("auth.json"),
            r#"{"access_token":"expired","expires_at":"2020-01-01T00:00:00Z"}"#,
        )
        .unwrap();
        let env = environment(&[
            (HERMES_HOME_ENV, custom.path().to_str().unwrap()),
            ("HOME", fallback.path().to_str().unwrap()),
        ]);
        assert!(matches!(
            resolve_credential_from(None, &env, fallback.path(), Utc::now()),
            Err(ProviderError::OAuthExpired(_))
        ));
    }

    #[test]
    fn portal_origin_requires_https_and_trusts_only_nousresearch_stored_hosts() {
        let empty = HashMap::new();
        let trusted = resolve_portal_url(&empty, Some("https://api.nousresearch.com/"));
        assert_eq!(trusted.host_str(), Some("api.nousresearch.com"));
        let untrusted = resolve_portal_url(&empty, Some("https://evil.example"));
        assert_eq!(untrusted.host_str(), Some("portal.nousresearch.com"));
        let invalid_env = environment(&[(PORTAL_URL_ENVS[0], "http://localhost:1234")]);
        let defaulted = resolve_portal_url(&invalid_env, None);
        assert_eq!(defaulted.host_str(), Some("portal.nousresearch.com"));
        let env = environment(&[(PORTAL_URL_ENVS[0], "https://localhost:1234")]);
        let overridden = resolve_portal_url(&env, None);
        assert_eq!(overridden.host_str(), Some("localhost"));
    }

    #[test]
    fn jwt_expiry_is_read_only_and_environment_override_wins() {
        use base64::Engine;

        let header = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(br#"{"alg":"none"}"#);
        let payload =
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(br#"{"exp":1893456000}"#);
        let token = format!("{header}.{payload}.signature");
        let env = environment(&[(ACCESS_TOKEN_ENV, &token)]);
        let credential =
            resolve_credential_from(None, &env, Path::new("C:\\unused"), Utc::now()).unwrap();
        assert_eq!(credential.token, token);
    }
}
