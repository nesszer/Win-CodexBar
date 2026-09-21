//! Muse Code subscription provider.
//!
//! Muse usage is minted from the Muse CLI's device-code OAuth token (`dca:`).
//! The Windows port reads the CLI auth file without writing or refreshing it;
//! the optional environment override is intended for controlled deployments,
//! not for inference keys (`LLM_` / `LLM|`).

pub mod local_usage;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use futures::StreamExt;
use reqwest::{Client, StatusCode};
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::time::{Duration, timeout};

use crate::core::{
    FetchContext, Provider, ProviderDisplayDetail, ProviderError, ProviderFetchResult, ProviderId,
    ProviderMetadata, RateWindow, SourceMode, UsageSnapshot,
};

const MUSE_USAGE_URL: &str = "https://api.meta.ai/muse-code/key";
const MUSE_DEVICE_TOKEN_ENV: &str = "MUSE_DEVICE_TOKEN";
const MUSE_AUTH_PATH_ENV: &str = "MUSE_AUTH_PATH";
const DEVICE_TOKEN_PREFIX: &str = "dca:";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_RESPONSE_BYTES: usize = 512 * 1024;
const WEEKLY_WINDOW_MINUTES: u32 = 7 * 24 * 60;
const MAX_RESET_SECONDS: f64 = 64_092_211_200.0;

#[derive(Debug, Deserialize)]
struct MuseAuthFile {
    providers: Option<MuseProviders>,
}

#[derive(Debug, Deserialize)]
struct MuseProviders {
    meta: Option<MuseMetaCredentials>,
}

#[derive(Debug, Deserialize)]
struct MuseMetaCredentials {
    mechanism: Option<String>,
    // Upstream Muse emits camelCase in auth.json (`accessToken`); the snake_case
    // field name is this port's local convention and the alias bridges the two.
    #[serde(alias = "accessToken")]
    access_token: Option<String>,
}

pub struct MuseProvider {
    metadata: ProviderMetadata,
    client: Client,
}

impl MuseProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: ProviderId::Muse,
                display_name: "Muse Code",
                session_label: "5 hours",
                weekly_label: "Weekly",
                supports_opus: false,
                supports_credits: false,
                default_enabled: false,
                is_primary: false,
                dashboard_url: Some("https://dev.meta.ai"),
                status_page_url: None,
                tertiary_label_key: None,
            },
            client: crate::core::credentialed_http_client_builder()
                .timeout(REQUEST_TIMEOUT)
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }

    async fn fetch_api(&self) -> Result<ProviderFetchResult, ProviderError> {
        let token = resolve_device_token()?;
        let response = timeout(
            REQUEST_TIMEOUT,
            self.client
                .post(MUSE_USAGE_URL)
                .bearer_auth(token)
                .header("x-api-version", "1.0.0")
                .header("User-Agent", "CodexBar")
                .header("Accept", "application/json")
                .json(&serde_json::json!({}))
                .send(),
        )
        .await
        .map_err(|_| ProviderError::Timeout)??;

        let status = response.status();
        if status != StatusCode::OK {
            return Err(status_error(status));
        }
        // Oversized responses are rejected by the streaming cap in
        // read_bounded_body, which also covers unknown or lying
        // content-length headers; no separate precheck is needed.
        let body = read_bounded_body(response).await?;
        parse_response(&body)
    }
}

async fn read_bounded_body(response: reqwest::Response) -> Result<Vec<u8>, ProviderError> {
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
        return Err(ProviderError::Parse(
            "Muse Code returned an oversized response.".to_string(),
        ));
    }
    body.extend_from_slice(chunk);
    Ok(())
}

impl Default for MuseProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for MuseProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Muse
    }

    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        match ctx.source_mode {
            SourceMode::Auto | SourceMode::OAuth => self.fetch_api().await,
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

fn resolve_device_token() -> Result<String, ProviderError> {
    let environment: HashMap<String, String> = [MUSE_DEVICE_TOKEN_ENV, MUSE_AUTH_PATH_ENV]
        .into_iter()
        .filter_map(|key| {
            std::env::var(key)
                .ok()
                .map(|value| (key.to_string(), value))
        })
        .collect();
    let home = dirs::home_dir().ok_or_else(missing_credentials)?;
    resolve_device_token_from(&environment, &home)
}

fn resolve_device_token_from(
    environment: &HashMap<String, String>,
    home_directory: &Path,
) -> Result<String, ProviderError> {
    if let Some(raw) = environment.get(MUSE_DEVICE_TOKEN_ENV) {
        return require_device_token(raw);
    }

    let path = auth_file_path(environment, home_directory);
    let contents = std::fs::read_to_string(&path).map_err(|_| missing_credentials())?;
    let file: MuseAuthFile = serde_json::from_str(&contents).map_err(|_| missing_credentials())?;
    let Some(meta) = file.providers.and_then(|providers| providers.meta) else {
        return Err(missing_credentials());
    };

    if let Some(raw) = meta.access_token {
        // An invalid inline credential must fail closed. Do not fall through
        // to another store and silently switch the selected Muse account.
        return require_device_token(&raw);
    }

    let mechanism = meta
        .mechanism
        .as_deref()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    if mechanism == "oauth" {
        return Err(ProviderError::AuthRequired);
    }
    Err(missing_credentials())
}

fn auth_file_path(environment: &HashMap<String, String>, home_directory: &Path) -> PathBuf {
    environment
        .get(MUSE_AUTH_PATH_ENV)
        .map(|value| value.trim())
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            home_directory
                .join(".config")
                .join("muse")
                .join("auth.json")
        })
}

fn require_device_token(raw: &str) -> Result<String, ProviderError> {
    let token = raw.trim();
    if token.is_empty() || !token.starts_with(DEVICE_TOKEN_PREFIX) {
        return Err(ProviderError::AuthRequired);
    }
    Ok(token.to_string())
}

fn missing_credentials() -> ProviderError {
    ProviderError::NotInstalled(
        "Muse Code login not found. Run `muse login`, then refresh CodexBar.".to_string(),
    )
}

fn status_error(status: StatusCode) -> ProviderError {
    match status {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => ProviderError::AuthRequired,
        StatusCode::TOO_MANY_REQUESTS => {
            ProviderError::Other("Muse Code usage requests are rate limited.".to_string())
        }
        status if status.is_server_error() => {
            ProviderError::Other("Muse Code API is unavailable.".to_string())
        }
        status => ProviderError::Other(format!("Muse Code API returned HTTP {}.", status.as_u16())),
    }
}

fn parse_response(body: &[u8]) -> Result<ProviderFetchResult, ProviderError> {
    let decoded: Value = serde_json::from_slice(body).map_err(|_| {
        ProviderError::Parse(
            "Could not parse Muse Code subscription usage: expected JSON".to_string(),
        )
    })?;
    let root = object(&decoded, "expected a response object")?;

    if optional_bool(root.get("require_payment"), "require_payment")? == Some(true) {
        return Err(ProviderError::Other(
            "Muse Code requires a payment method. Finish billing at https://dev.meta.ai."
                .to_string(),
        ));
    }
    if optional_bool(root.get("is_subs_active"), "is_subs_active")? != Some(true) {
        return Err(ProviderError::Other(
            "No Muse Code subscription is active on this login.".to_string(),
        ));
    }

    let usage = object(
        root.get("subs_usage")
            .ok_or_else(|| parse_failure("missing subs_usage"))?,
        "subs_usage",
    )?;
    let window = object(
        usage
            .get("window")
            .ok_or_else(|| parse_failure("missing subscription window"))?,
        "window",
    )?;
    let weekly = object(
        usage
            .get("weekly")
            .ok_or_else(|| parse_failure("missing weekly window"))?,
        "weekly",
    )?;

    let duration = positive_safe_minutes(number(
        window
            .get("window_duration_mins")
            .ok_or_else(|| parse_failure("window_duration_mins"))?,
        "window_duration_mins",
    )?)?;
    let primary_percent = number(
        window
            .get("used_percent")
            .ok_or_else(|| parse_failure("used_percent"))?,
        "used_percent",
    )?;
    let weekly_percent = number(
        weekly
            .get("used_percent")
            .ok_or_else(|| parse_failure("weekly.used_percent"))?,
        "weekly.used_percent",
    )?;
    let primary_reset = parse_reset(window.get("resets_at"))?;
    let weekly_reset = parse_reset(weekly.get("resets_at"))?;
    let plan = optional_text(root.get("subs_tier_name"), "subs_tier_name")?;
    let email = optional_text(root.get("user_email"), "user_email")?;

    let primary = RateWindow::with_details(
        primary_percent.clamp(0.0, 100.0),
        Some(duration),
        primary_reset,
        None,
    );
    let secondary = RateWindow::with_details(
        weekly_percent.clamp(0.0, 100.0),
        Some(WEEKLY_WINDOW_MINUTES),
        weekly_reset,
        None,
    );
    // The plan ("Pro" etc.) is subscription metadata, not a login mechanism;
    // it travels as the "plan" display detail row, not in login_method.
    let mut usage = UsageSnapshot::new(primary)
        .with_secondary(secondary)
        .with_login_method("Muse login");
    if let Some(email) = email {
        usage = usage.with_email(email);
    }

    let mut result = ProviderFetchResult::new(usage, "oauth")
        .with_display_detail(ProviderDisplayDetail::new(
            "five-hour",
            "5 hours",
            format_percent(primary_percent),
        ))
        .with_display_detail(ProviderDisplayDetail::new(
            "weekly",
            "Weekly",
            format_percent(weekly_percent),
        ));
    if let Some(plan) = plan {
        result = result.with_display_detail(ProviderDisplayDetail::new("plan", "Plan", plan));
    }
    Ok(result)
}

fn object<'a>(
    value: &'a Value,
    field: &str,
) -> Result<&'a serde_json::Map<String, Value>, ProviderError> {
    value
        .as_object()
        .ok_or_else(|| parse_failure(format!("{field} must be an object")))
}

/// Strict boolean policy for Muse response fields: a present non-bool value
/// is a parse failure, not a truthiness coercion. Muse's API is typed JSON;
/// silently treating "yes"/1 as true would mask protocol drift. Non-null
/// absence of the field stays `None` and callers treat that as false.
fn optional_bool(value: Option<&Value>, field: &str) -> Result<Option<bool>, ProviderError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_bool()
            .map(Some)
            .ok_or_else(|| parse_failure(field)),
    }
}

fn number(value: &Value, field: &str) -> Result<f64, ProviderError> {
    let value = value.as_f64().ok_or_else(|| parse_failure(field))?;
    value
        .is_finite()
        .then_some(value)
        .ok_or_else(|| parse_failure(field))
}

fn optional_text(value: Option<&Value>, field: &str) -> Result<Option<String>, ProviderError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_str()
            .map(|text| text.trim().to_string())
            .filter(|text| !text.is_empty())
            .map(Some)
            .ok_or_else(|| parse_failure(field)),
    }
}

fn positive_safe_minutes(value: f64) -> Result<u32, ProviderError> {
    if !value.is_finite() || value <= 0.0 {
        return Err(parse_failure("window_duration_mins"));
    }
    // f64 has no TryFrom<u32> conversion; round first, then bound-check the
    // integer result instead of the pre-merge manual u32::MAX float guard.
    let rounded = value.round();
    if rounded <= 0.0 || rounded >= u32::MAX as f64 {
        return Err(parse_failure("window_duration_mins"));
    }
    #[expect(
        clippy::cast_possible_truncation,
        reason = "rounded is bounded below u32::MAX by the check above"
    )]
    Ok(rounded as u32)
}

fn parse_reset(value: Option<&Value>) -> Result<Option<DateTime<Utc>>, ProviderError> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    let seconds = number(value, "resets_at")?;
    // MAX_RESET_SECONDS is the single bound: it caps seconds inside the i64
    // range, so truncation cannot overflow and from_timestamp always
    // succeeds for values that pass this check.
    if seconds <= 0.0 || seconds > MAX_RESET_SECONDS {
        return Ok(None);
    }
    #[expect(
        clippy::cast_possible_truncation,
        reason = "MAX_RESET_SECONDS bounds the value inside i64"
    )]
    let whole_seconds = seconds.trunc() as i64;
    Ok(DateTime::<Utc>::from_timestamp(whole_seconds, 0))
}

fn format_percent(percent: f64) -> String {
    format!("{percent:.0}%")
}

fn parse_failure(field: impl Into<String>) -> ProviderError {
    ProviderError::Parse(format!(
        "Could not parse Muse Code subscription usage: {}",
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
            "require_payment": false,
            "is_subs_active": true,
            "subs_tier_name": "Pro",
            "user_email": "muse@example.com",
            "api_key": "LLM_should_not_surface",
            "payment_method": "card_should_not_surface",
            "subs_usage": {
                "window": {
                    "window_duration_mins": 300,
                    "used_percent": 96,
                    "resets_at": 1788580000
                },
                "weekly": {
                    "used_percent": 40,
                    "resets_at": 1789000000
                }
            }
        })
    }

    #[test]
    fn provider_metadata_and_sources_are_oauth_only() {
        let provider = MuseProvider::new();
        assert_eq!(provider.id(), ProviderId::Muse);
        assert_eq!(provider.metadata().display_name, "Muse Code");
        assert!(!provider.metadata().default_enabled);
        assert_eq!(
            provider.available_sources(),
            vec![SourceMode::Auto, SourceMode::OAuth]
        );
        assert!(provider.supports_oauth());
        assert!(!provider.supports_web());
        assert!(!provider.supports_cli());
    }

    #[test]
    fn auth_file_uses_override_and_requires_device_token_prefix() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("custom-auth.json");
        fs::write(
            &path,
            r#"{"providers":{"meta":{"mechanism":"oauth","access_token":"  dca:device-token  "}}}"#,
        )
        .unwrap();
        let path = path.to_string_lossy().into_owned();
        let env = environment(&[(MUSE_AUTH_PATH_ENV, &path)]);
        assert_eq!(
            resolve_device_token_from(&env, Path::new("C:\\unused")).unwrap(),
            "dca:device-token"
        );

        fs::write(
            &path,
            r#"{"providers":{"meta":{"mechanism":"oauth","access_token":"LLM-inference-key"}}}"#,
        )
        .unwrap();
        assert!(matches!(
            resolve_device_token_from(&env, Path::new("C:\\unused")),
            Err(ProviderError::AuthRequired)
        ));
    }

    #[test]
    fn auth_file_default_path_and_env_override_are_deterministic() {
        let dir = tempdir().unwrap();
        let auth_path = dir.path().join(".config/muse/auth.json");
        fs::create_dir_all(auth_path.parent().unwrap()).unwrap();
        fs::write(
            &auth_path,
            r#"{"providers":{"meta":{"mechanism":"oauth","access_token":"dca:file-token"}}}"#,
        )
        .unwrap();
        assert_eq!(
            resolve_device_token_from(&HashMap::new(), dir.path()).unwrap(),
            "dca:file-token"
        );
        let env = environment(&[(MUSE_DEVICE_TOKEN_ENV, " dca:env-token ")]);
        assert_eq!(
            resolve_device_token_from(&env, dir.path()).unwrap(),
            "dca:env-token"
        );
    }

    #[test]
    fn oauth_without_inline_token_does_not_fall_through_to_inference_key() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("auth.json");
        fs::write(
            &path,
            r#"{"providers":{"meta":{"mechanism":"oauth","api_key":"LLM-nope"}}}"#,
        )
        .unwrap();
        let path = path.to_string_lossy().into_owned();
        let env = environment(&[(MUSE_AUTH_PATH_ENV, &path)]);
        assert!(matches!(
            resolve_device_token_from(&env, dir.path()),
            Err(ProviderError::AuthRequired)
        ));
    }

    #[test]
    fn success_payload_maps_windows_identity_details_and_ignores_secrets() {
        let body = serde_json::to_vec(&success_payload()).unwrap();
        let result = parse_response(&body).unwrap();
        assert_eq!(result.source_label, "oauth");
        assert_eq!(result.usage.primary.used_percent, 96.0);
        assert_eq!(result.usage.primary.window_minutes, Some(300));
        assert_eq!(result.usage.secondary.as_ref().unwrap().used_percent, 40.0);
        assert_eq!(
            result.usage.secondary.as_ref().unwrap().window_minutes,
            Some(10080)
        );
        assert_eq!(
            result.usage.account_email.as_deref(),
            Some("muse@example.com")
        );
        assert_eq!(result.usage.login_method.as_deref(), Some("Muse login"));
        let details: Vec<_> = result.display_details().iter().collect();
        assert_eq!(details.len(), 3);
        assert!(
            details
                .iter()
                .any(|detail| detail.title() == "Plan" && detail.value() == "Pro")
        );
        assert!(
            details
                .iter()
                .all(|detail| !detail.value().contains("LLM_"))
        );
        assert!(
            details
                .iter()
                .all(|detail| !detail.value().contains("card_"))
        );
    }

    #[test]
    fn percentage_values_clamp_and_invalid_reset_omits_only_reset() {
        let mut payload = success_payload();
        payload["subs_usage"]["window"]["used_percent"] = serde_json::json!(-5.0);
        payload["subs_usage"]["weekly"]["used_percent"] = serde_json::json!(150.0);
        payload["subs_usage"]["window"]["resets_at"] = serde_json::json!(64092211201_i64);
        let result = parse_response(&serde_json::to_vec(&payload).unwrap()).unwrap();
        assert_eq!(result.usage.primary.used_percent, 0.0);
        assert_eq!(result.usage.secondary.as_ref().unwrap().used_percent, 100.0);
        assert!(result.usage.primary.resets_at.is_none());
        assert!(result.usage.secondary.as_ref().unwrap().resets_at.is_some());
    }

    #[test]
    fn malformed_required_fields_fail_closed_without_echoing_payload() {
        for payload in [
            serde_json::json!({"is_subs_active": true}),
            serde_json::json!({"is_subs_active": true, "subs_usage": {"window": {}, "weekly": {}}}),
            serde_json::json!({"require_payment": "yes", "is_subs_active": true}),
            serde_json::json!({"require_payment": false, "is_subs_active": true, "subs_usage": {"window": {"window_duration_mins": "300"}, "weekly": {"used_percent": 1}}}),
        ] {
            let error = parse_response(&serde_json::to_vec(&payload).unwrap()).unwrap_err();
            assert!(matches!(error, ProviderError::Parse(_)));
            assert!(!error.to_string().contains("LLM-nope"));
        }
    }

    #[test]
    fn subscription_errors_are_safe_and_statuses_are_classified() {
        let mut payload = success_payload();
        payload["require_payment"] = serde_json::json!(true);
        let error = parse_response(&serde_json::to_vec(&payload).unwrap()).unwrap_err();
        assert!(error.to_string().contains("dev.meta.ai"));
        assert!(!error.to_string().contains("LLM_"));

        assert!(matches!(
            status_error(StatusCode::UNAUTHORIZED),
            ProviderError::AuthRequired
        ));
        assert!(matches!(
            status_error(StatusCode::FORBIDDEN),
            ProviderError::AuthRequired
        ));
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
    }

    #[test]
    fn fractional_window_duration_rounds_safely() {
        let mut payload = success_payload();
        payload["subs_usage"]["window"]["window_duration_mins"] = serde_json::json!(300.6);
        let result = parse_response(&serde_json::to_vec(&payload).unwrap()).unwrap();
        assert_eq!(result.usage.primary.window_minutes, Some(301));
    }

    #[test]
    fn streaming_response_cap_rejects_oversized_chunk_without_content_length() {
        let mut body = vec![0_u8; MAX_RESPONSE_BYTES];
        assert!(append_bounded_body(&mut body, &[0]).is_err());
    }
}
