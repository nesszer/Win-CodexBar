//! Nous Portal subscription provider.
//!
//! Nous Portal issues short-lived access tokens through the Hermes Agent
//! device-code login. The Windows port reads those credentials without
//! refreshing or writing them, then projects the account endpoint into the
//! monthly subscription-credit display used by the rest of the app.

use async_trait::async_trait;
use chrono::{DateTime, Datelike, Utc};
use futures::StreamExt;
use reqwest::{Client, StatusCode};
use serde_json::{Map, Value};
use tokio::time::{Duration, timeout};

use crate::core::{
    FetchContext, Provider, ProviderDisplayDetail, ProviderError, ProviderFetchResult, ProviderId,
    ProviderMetadata, RateWindow, SourceMode, SubscriptionMetadata, UsageSnapshot,
};

const PORTAL_ACCOUNT_PATH: &str = "api/oauth/account";
const MAX_RESPONSE_BYTES: usize = 512 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

mod credentials;

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
                tertiary_label_key: None,
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
        let credential = credentials::resolve_credential(explicit_token)?;
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
    if root.get("error").is_some_and(reports_error) {
        return Err(ProviderError::Other(
            "Nous Portal account endpoint reported an error.".to_string(),
        ));
    }
    let subscription = optional_object(root.get("subscription"), "subscription")?;
    let access = optional_object(root.get("paid_service_access"), "paid_service_access")?;
    let user = optional_object(root.get("user"), "user")?;
    let organization = optional_object(root.get("organisation"), "organisation")?;

    let monthly = field_number(subscription, "monthly_credits")?;
    if monthly.is_some_and(|value| value < 0.0) {
        return Err(parse_failure("monthly_credits"));
    }
    let remaining = first_number(
        &[subscription, access],
        &["credits_remaining", "subscription_credits_remaining"],
    )?;
    let rollover = field_number(subscription, "rollover_credits")?;
    let purchased = first_number(&[Some(root), access], &["purchased_credits_remaining"])?;
    let total = field_number(access, "total_usable_credits")?;
    if [monthly, remaining, rollover, purchased, total]
        .iter()
        .all(Option::is_none)
    {
        return Err(parse_failure("no credit amounts"));
    }

    let renewal = optional_date(
        field(subscription, "current_period_end"),
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

    let plan = text(field(subscription, "plan"), "subscription.plan")?;
    let active_subscription = access
        .and_then(|value| value.get("has_active_subscription"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let login_method = plan.or_else(|| active_subscription.then(|| "Subscription".to_string()));
    let email = text(field(user, "email"), "user.email")?;
    let organization_name = text(field(organization, "name"), "organisation.name")?;

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
        let detail = ProviderDisplayDetail::new(
            "subscription-credits",
            "Subscription credits",
            monthly
                .filter(|value| *value > 0.0)
                .map(|monthly| format!("{} of {} left", format_usd(remaining), format_usd(monthly)))
                .unwrap_or_else(|| format!("{} left", format_usd(remaining))),
        );
        let detail = if let Some(monthly) = monthly.filter(|value| *value > 0.0) {
            let used = (monthly - remaining).clamp(0.0, monthly);
            detail.and_then(|row| row.with_progress(used, monthly))
        } else {
            detail
        };
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

/// Numeric value of one field on an optional object; the name appears once.
fn field_number(
    object: Option<&Map<String, Value>>,
    name: &str,
) -> Result<Option<f64>, ProviderError> {
    number(object.and_then(|object| object.get(name)), name)
}

/// Read one field from an optional object; the name appears once per call.
fn field<'a>(object: Option<&'a Map<String, Value>>, name: &str) -> Option<&'a Value> {
    object.and_then(|object| object.get(name))
}

/// First numeric value found for any of `names`, scanned across the
/// response dialects in `sources` order. `sources` names the dialect
/// objects; `names` the per-dialect field names — a rename between
/// dialects stays explicit here.
fn first_number(
    sources: &[Option<&Map<String, Value>>],
    names: &[&str],
) -> Result<Option<f64>, ProviderError> {
    for source in sources.iter().flatten() {
        for name in names {
            if let Some(value) = source.get(*name) {
                return number(Some(value), name);
            }
        }
    }
    Ok(None)
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
    credentials::parse_iso(raw)
        .map(Some)
        .ok_or_else(|| parse_failure(field))
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

/// Pinned error-field policy, matching the upstream Hermes plugin's
/// JavaScript `if (root.error)` check (Plugins/nous.js in the 0.61.0 port):
/// an error is reported exactly when the field is a non-empty object/array
/// or a non-empty string. Booleans, numbers (including 0), empty strings,
/// and empty arrays are treated as "no error" — the portal never reports
/// errors through numeric or boolean fields, so treating them as truthy
/// here would only manufacture failures the API does not send.
fn reports_error(value: &Value) -> bool {
    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) => false,
        Value::String(value) => !value.is_empty(),
        Value::Array(value) => !value.is_empty(),
        Value::Object(value) => !value.is_empty(),
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
    use std::collections::HashMap;
    use std::fs;
    use std::path::Path;
    use tempfile::tempdir;

    use credentials::{
        ACCESS_TOKEN_ENV, HERMES_HOME_ENV, PORTAL_URL_ENVS, parse_auth_file,
        resolve_credential_from, resolve_portal_url,
    };

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
        let details: Vec<_> = result.display_details().iter().collect();
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
                .iter()
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
    fn error_field_is_reported_only_for_meaningful_shapes() {
        // Reported: non-empty string, non-empty array, non-empty object.
        for payload in [
            serde_json::json!({"error": "invalid token"}),
            serde_json::json!({"error": ["details"]}),
            serde_json::json!({"error": {"code": 7}}),
        ] {
            let error = parse_response(&serde_json::to_vec(&payload).unwrap()).unwrap_err();
            assert!(
                error.to_string().contains("reported an error"),
                "shape {:?} must report an error",
                payload
            );
        }
        // Not reported: null, false, empty string, empty array, empty
        // object, zero. The portal reports errors via a truthy `error` field
        // (upstream Hermes plugin `if (root.error)`), so these shapes mean
        // "no error here".
        for payload in [
            serde_json::json!({"subscription": {"monthly_credits": 1}, "error": null}),
            serde_json::json!({"subscription": {"monthly_credits": 1}, "error": false}),
            serde_json::json!({"subscription": {"monthly_credits": 1}, "error": true}),
            serde_json::json!({"subscription": {"monthly_credits": 1}, "error": ""}),
            serde_json::json!({"subscription": {"monthly_credits": 1}, "error": []}),
            serde_json::json!({"subscription": {"monthly_credits": 1}, "error": {}}),
            serde_json::json!({"subscription": {"monthly_credits": 1}, "error": 0}),
        ] {
            let result = parse_response(&serde_json::to_vec(&payload).unwrap()).unwrap();
            assert_eq!(result.source_label, "api");
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
