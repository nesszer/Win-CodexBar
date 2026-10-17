//! Exa API usage provider.
//!
//! Fetches selected API key monthly spend (UTC calendar month to date) from
//! `GET https://admin-api.exa.ai/team-management/api-keys/{id}/usage` with
//! x-api-key (Team Management service key) authentication.
//!
//! Ported from upstream CodexBar v0.74.1 (`Resources/Plugins/exa.js`).

use async_trait::async_trait;
use chrono::{DateTime, Datelike, Timelike, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::core::{
    FetchContext, Provider, ProviderDisplayDetail, ProviderError, ProviderFetchResult, ProviderId,
    ProviderMetadata, ProviderStateKind, RateWindow, SourceMode, UsageSnapshot,
};
use crate::providers::{BoundedBodyError, read_bounded_response};

/// Usage history cannot back a limit reset detector (upstream excludes this
/// provider from plan-utilization history).
pub(crate) const LIMIT_RESET_POLICY: crate::core::ProviderLimitResetPolicy =
    crate::core::ProviderLimitResetPolicy::UNSUPPORTED;

const USAGE_URL_BASE: &str = "https://admin-api.exa.ai/team-management/api-keys";
const CREDENTIAL_TARGET: &str = "codexbar-exa";
pub const SERVICE_KEY_ENV: &str = "EXA_SERVICE_KEY";
pub const API_KEY_ID_ENV: &str = "EXA_API_KEY_ID";
const MAX_RESPONSE_BYTES: usize = 256 * 1024;
const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

#[derive(Debug, Deserialize, Serialize)]
struct UsageResponse {
    total_cost_usd: f64,
    period: PeriodInfo,
}

#[derive(Debug, Deserialize, Serialize)]
struct PeriodInfo {
    start: String,
    end: String,
}

pub struct ExaProvider {
    metadata: ProviderMetadata,
    client: Option<Client>,
}

impl ExaProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: ProviderId::Exa,
                display_name: "Exa",
                session_label: "Spend",
                weekly_label: "Spend",
                supports_opus: false,
                supports_credits: false,
                default_enabled: false,
                is_primary: false,
                dashboard_url: Some("https://dashboard.exa.ai/api-keys"),
                status_page_url: None,
                tertiary_label_key: None,
            },
            client: crate::core::credentialed_http_client_builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(REQUEST_TIMEOUT)
                .build()
                .ok(),
        }
    }

    async fn fetch_usage_impl(
        &self,
        ctx: &FetchContext,
    ) -> Result<ProviderFetchResult, ProviderError> {
        let client = self.client.as_ref().ok_or_else(|| {
            ProviderError::Other("Could not create a secure Exa HTTP client.".into())
        })?;
        // Get service key (for header auth)
        let service_key = crate::providers::resolve_api_key(
            ctx.api_key.as_deref(),
            CREDENTIAL_TARGET,
            &[SERVICE_KEY_ENV],
        )?;

        let key_id = resolve_key_id(
            ctx.workspace_id.as_deref(),
            std::env::var(API_KEY_ID_ENV).ok().as_deref(),
        )
        .ok_or_else(|| ProviderError::Other(MISSING_KEY_ID.into()))?;

        let (month_start, end_time) = month_to_date_range(Utc::now());
        let start_date = month_start.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
        let end_date = end_time.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();

        let url = format!(
            "{}/{}/usage?start_date={}&end_date={}",
            USAGE_URL_BASE,
            encode_uri_component(&key_id),
            encode_uri_component(&start_date),
            encode_uri_component(&end_date)
        );

        let response = client
            .get(&url)
            .header("x-api-key", &service_key)
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await?;

        check_status(response.status().as_u16())?;

        let body = read_bounded_response(response, MAX_RESPONSE_BYTES)
            .await
            .map_err(|error| match error {
                BoundedBodyError::TooLarge => ProviderError::Parse(UNRECOGNIZED.into()),
                BoundedBodyError::Read(error) => ProviderError::from(error),
            })?;

        let data: Value =
            serde_json::from_slice(&body).map_err(|_| ProviderError::Parse(UNRECOGNIZED.into()))?;

        let parsed = parse_usage(&data, month_start, end_time)?;
        let usage = parsed.usage;
        let mut result = ProviderFetchResult::new(usage, "api");
        for detail in parsed.details {
            result = result.with_display_detail(Some(detail));
        }
        Ok(result)
    }
}

impl Default for ExaProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for ExaProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Exa
    }

    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        match ctx.source_mode {
            SourceMode::Auto | SourceMode::OAuth => self.fetch_usage_impl(ctx).await,
            SourceMode::Web | SourceMode::Cli => {
                Err(ProviderError::UnsupportedSource(ctx.source_mode))
            }
        }
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::OAuth]
    }

    fn error_state_kind(&self, error: &ProviderError) -> ProviderStateKind {
        match error {
            ProviderError::Other(message) if message == SERVICE_KEY_REJECTED => {
                ProviderStateKind::ExpiredSession
            }
            ProviderError::Other(message) if message == MISSING_KEY_ID => {
                ProviderStateKind::NeedsAuthentication
            }
            _ => error.state_kind(),
        }
    }
}

const MISSING_KEY_ID: &str =
    "Set the Exa API key ID in Scope or EXA_API_KEY_ID; this is not the service key.";
const UNRECOGNIZED: &str = "Exa returned an unrecognized API key usage response.";
const SERVICE_KEY_REJECTED: &str =
    "Exa rejected the service key. Ordinary search API keys are not supported.";
const TEAM_MANAGEMENT_DENIED: &str =
    "Exa Team Management must be enabled and both keys must belong to the same team.";

/// Upstream `ctx.fail.*` category for a non-200 usage response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StatusFailure {
    AuthenticationExpired,
    PermissionDenied,
    RateLimited,
    ProviderUnavailable,
    ApiFailure,
}

fn classify_status(status: u16) -> Option<StatusFailure> {
    match status {
        200 => None,
        401 => Some(StatusFailure::AuthenticationExpired),
        403 => Some(StatusFailure::PermissionDenied),
        429 => Some(StatusFailure::RateLimited),
        500.. => Some(StatusFailure::ProviderUnavailable),
        _ => Some(StatusFailure::ApiFailure),
    }
}

fn check_status(status: u16) -> Result<(), ProviderError> {
    let message = match classify_status(status) {
        None => return Ok(()),
        Some(StatusFailure::AuthenticationExpired) => SERVICE_KEY_REJECTED.to_owned(),
        Some(StatusFailure::PermissionDenied) => TEAM_MANAGEMENT_DENIED.to_owned(),
        Some(_) => format!("Exa returned HTTP {}.", status),
    };
    Err(ProviderError::Other(message))
}

/// Start of the UTC month containing `now`, and `now` truncated to whole seconds.
fn month_to_date_range(now: DateTime<Utc>) -> (DateTime<Utc>, DateTime<Utc>) {
    let end_time = now.with_nanosecond(0).unwrap_or(now);
    let month_start = now
        .date_naive()
        .with_day(1)
        .and_then(|day| day.and_hms_opt(0, 0, 0))
        .map_or(end_time, |start| start.and_utc());
    (month_start, end_time)
}

/// Saved Scope value first, then `EXA_API_KEY_ID`.
fn resolve_key_id(workspace_id: Option<&str>, env_value: Option<&str>) -> Option<String> {
    let usable = |value: &str| {
        let trimmed = value.trim();
        (!trimmed.is_empty() && trimmed != "." && trimmed != "..").then(|| trimmed.to_string())
    };
    workspace_id
        .and_then(usable)
        .or_else(|| env_value.and_then(usable))
}

/// JavaScript `encodeURIComponent`: UTF-8 bytes, leaving `A-Z a-z 0-9 - _ . ! ~ * ' ( )`.
fn encode_uri_component(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'!'
            | b'~'
            | b'*'
            | b'\''
            | b'('
            | b')' => encoded.push(byte as char),
            _ => encoded.push_str(&format!("%{:02X}", byte)),
        }
    }
    encoded
}

#[derive(Debug, Clone)]
struct ParsedUsage {
    usage: UsageSnapshot,
    details: Vec<ProviderDisplayDetail>,
}

fn parse_usage(
    payload: &Value,
    month_start: chrono::DateTime<Utc>,
    end_time: chrono::DateTime<Utc>,
) -> Result<ParsedUsage, ProviderError> {
    let unrecognized = || ProviderError::Parse(UNRECOGNIZED.into());
    let data = payload.as_object().ok_or_else(unrecognized)?;

    let total_cost_usd = data
        .get("total_cost_usd")
        .and_then(Value::as_f64)
        .filter(|cost| cost.is_finite() && *cost >= 0.0)
        .ok_or_else(unrecognized)?;

    // Upstream compares `Date.parse(...)` instants, not the literal strings.
    let instant = |key: &str| {
        data.get("period")
            .and_then(|period| period.get(key))
            .and_then(Value::as_str)
            .and_then(|text| chrono::DateTime::parse_from_rfc3339(text).ok())
            .map(|time| time.with_timezone(&Utc))
    };
    if instant("start") != Some(month_start) || instant("end") != Some(end_time) {
        return Err(unrecognized());
    }
    let spend = super::format::usd(total_cost_usd);
    // Upstream has no quota window; the spend only reaches the tray and float bar.
    let usage =
        UsageSnapshot::new(RateWindow::informational(format!("Spend: {spend}")).hidden_on_card())
            .with_login_method("Service key");

    let mut details = Vec::new();

    if let Some(detail) = ProviderDisplayDetail::new("spend", "Spend", spend)
        .and_then(|d| d.with_section_title("API key this month (UTC)"))
    {
        details.push(detail);
    }

    Ok(ParsedUsage { usage, details })
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests_module;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn month_range_comes_from_one_instant() {
        let at = |text: &str| {
            chrono::DateTime::parse_from_rfc3339(text)
                .unwrap()
                .with_timezone(&Utc)
        };
        assert_eq!(
            month_to_date_range(at("2026-10-31T23:59:59.900Z")),
            (at("2026-10-01T00:00:00Z"), at("2026-10-31T23:59:59Z"))
        );
        assert_eq!(
            month_to_date_range(at("2026-11-01T00:00:00.000Z")),
            (at("2026-11-01T00:00:00Z"), at("2026-11-01T00:00:00Z"))
        );
    }

    #[test]
    fn test_parse_valid_usage() {
        let json = r#"{
            "total_cost_usd": 45.67,
            "period": {
                "start": "2026-10-01T00:00:00.000Z",
                "end": "2026-10-08T12:31:36.000Z"
            }
        }"#;

        let month_start = chrono::DateTime::parse_from_rfc3339("2026-10-01T00:00:00+00:00")
            .unwrap()
            .with_timezone(&Utc);
        let end_time = chrono::DateTime::parse_from_rfc3339("2026-10-08T12:31:36+00:00")
            .unwrap()
            .with_timezone(&Utc);

        let data: Value = serde_json::from_str(json).unwrap();
        let result = parse_usage(&data, month_start, end_time).unwrap();

        assert_eq!(result.usage.login_method, Some("Service key".to_string()));
        assert!(!result.details.is_empty());
        assert_eq!(result.details[0].title(), "Spend");
        assert!(result.details[0].value().contains("$45.67"));
        assert!(result.usage.primary.is_informational);
        assert!(result.usage.primary.hidden_on_card);
        assert_eq!(
            result.usage.primary.reset_description.as_deref(),
            Some("Spend: $45.67")
        );
    }

    #[test]
    fn test_parse_zero_spend() {
        let json = r#"{
            "total_cost_usd": 0.0,
            "period": {
                "start": "2026-10-01T00:00:00.000Z",
                "end": "2026-10-08T12:31:36.000Z"
            }
        }"#;

        let month_start = chrono::DateTime::parse_from_rfc3339("2026-10-01T00:00:00+00:00")
            .unwrap()
            .with_timezone(&Utc);
        let end_time = chrono::DateTime::parse_from_rfc3339("2026-10-08T12:31:36+00:00")
            .unwrap()
            .with_timezone(&Utc);

        let data: Value = serde_json::from_str(json).unwrap();
        let result = parse_usage(&data, month_start, end_time).unwrap();

        assert_eq!(result.details[0].value(), "$0.00");
    }

    #[test]
    fn test_parse_rejects_missing_cost() {
        let json = r#"{
            "period": {
                "start": "2026-10-01T00:00:00.000Z",
                "end": "2026-10-08T12:31:36.000Z"
            }
        }"#;

        let month_start = chrono::DateTime::parse_from_rfc3339("2026-10-01T00:00:00+00:00")
            .unwrap()
            .with_timezone(&Utc);
        let end_time = chrono::DateTime::parse_from_rfc3339("2026-10-08T12:31:36+00:00")
            .unwrap()
            .with_timezone(&Utc);

        let data: Value = serde_json::from_str(json).unwrap();
        assert!(parse_usage(&data, month_start, end_time).is_err());
    }

    #[test]
    fn test_parse_rejects_negative_cost() {
        let json = r#"{
            "total_cost_usd": -0.01,
            "period": {
                "start": "2026-10-01T00:00:00.000Z",
                "end": "2026-10-08T12:31:36.000Z"
            }
        }"#;

        let month_start = chrono::DateTime::parse_from_rfc3339("2026-10-01T00:00:00+00:00")
            .unwrap()
            .with_timezone(&Utc);
        let end_time = chrono::DateTime::parse_from_rfc3339("2026-10-08T12:31:36+00:00")
            .unwrap()
            .with_timezone(&Utc);

        let data: Value = serde_json::from_str(json).unwrap();
        assert!(parse_usage(&data, month_start, end_time).is_err());
    }

    #[test]
    fn test_parse_rejects_nonfinite_cost() {
        // Test non-numeric and invalid JSON values (note: serde_json doesn't support 1e400 as infinity)
        for json_val in ["null", "true", "\"string\"", "[]", "{}"] {
            let json = format!(
                r#"{{
                "total_cost_usd": {},
                "period": {{
                    "start": "2026-10-01T00:00:00.000Z",
                    "end": "2026-10-08T12:31:36.000Z"
                }}
            }}"#,
                json_val
            );

            let month_start = chrono::DateTime::parse_from_rfc3339("2026-10-01T00:00:00+00:00")
                .unwrap()
                .with_timezone(&Utc);
            let end_time = chrono::DateTime::parse_from_rfc3339("2026-10-08T12:31:36+00:00")
                .unwrap()
                .with_timezone(&Utc);

            let data: Value = serde_json::from_str(&json).unwrap();
            assert!(parse_usage(&data, month_start, end_time).is_err());
        }
    }

    #[test]
    fn test_parse_rejects_mismatched_period() {
        let json = r#"{
            "total_cost_usd": 45.67,
            "period": {
                "start": "2026-09-01T00:00:00.000Z",
                "end": "2026-10-08T12:31:36.000Z"
            }
        }"#;

        let month_start = chrono::DateTime::parse_from_rfc3339("2026-10-01T00:00:00+00:00")
            .unwrap()
            .with_timezone(&Utc);
        let end_time = chrono::DateTime::parse_from_rfc3339("2026-10-08T12:31:36+00:00")
            .unwrap()
            .with_timezone(&Utc);

        let data: Value = serde_json::from_str(json).unwrap();
        assert!(parse_usage(&data, month_start, end_time).is_err());
    }

    #[test]
    fn encodes_like_encode_uri_component() {
        assert_eq!(encode_uri_component("é"), "%C3%A9");
        assert_eq!(encode_uri_component("a b/c"), "a%20b%2Fc");
        assert_eq!(encode_uri_component("AZaz09-_.!~*'()"), "AZaz09-_.!~*'()");
        assert_eq!(
            encode_uri_component("2026-10-01T00:00:00.000Z"),
            "2026-10-01T00%3A00%3A00.000Z"
        );
        assert_eq!(encode_uri_component("%?#&=+"), "%25%3F%23%26%3D%2B");
    }

    #[test]
    fn key_id_prefers_scope_then_env() {
        assert_eq!(
            resolve_key_id(Some(" scope-id "), Some("env-id")).as_deref(),
            Some("scope-id")
        );
        assert_eq!(
            resolve_key_id(None, Some(" env-id ")).as_deref(),
            Some("env-id")
        );
        assert_eq!(
            resolve_key_id(Some("  "), Some("env-id")).as_deref(),
            Some("env-id")
        );
        for rejected in ["", "  ", ".", ".."] {
            assert_eq!(resolve_key_id(Some(rejected), None), None);
            assert_eq!(resolve_key_id(None, Some(rejected)), None);
        }
        assert_eq!(resolve_key_id(None, None), None);
    }

    #[test]
    fn status_classification_matches_upstream() {
        use StatusFailure::*;
        let cases = [
            (200, None, ""),
            (302, Some(ApiFailure), "Exa returned HTTP 302."),
            (400, Some(ApiFailure), "Exa returned HTTP 400."),
            (401, Some(AuthenticationExpired), SERVICE_KEY_REJECTED),
            (403, Some(PermissionDenied), TEAM_MANAGEMENT_DENIED),
            (429, Some(RateLimited), "Exa returned HTTP 429."),
            (503, Some(ProviderUnavailable), "Exa returned HTTP 503."),
        ];
        for (status, expected, text) in cases {
            assert_eq!(classify_status(status), expected, "status {status}");
            match check_status(status) {
                Ok(()) => assert!(expected.is_none()),
                Err(ProviderError::Other(message)) => assert_eq!(message, text),
                Err(other) => panic!("unexpected error {other:?}"),
            }
        }
    }

    #[test]
    fn only_rejected_service_key_maps_to_expired_session() {
        let provider = ExaProvider::new();
        let kind = |status| provider.error_state_kind(&check_status(status).unwrap_err());
        assert_eq!(kind(401), ProviderStateKind::ExpiredSession);
        for status in [302, 400, 403, 429, 503] {
            assert_eq!(kind(status), ProviderStateKind::Unknown, "status {status}");
        }
    }

    fn window() -> (chrono::DateTime<Utc>, chrono::DateTime<Utc>) {
        let parse = |text| {
            chrono::DateTime::parse_from_rfc3339(text)
                .unwrap()
                .with_timezone(&Utc)
        };
        (parse("2026-10-01T00:00:00Z"), parse("2026-10-08T12:31:36Z"))
    }

    #[test]
    fn period_matches_by_instant_not_text() {
        let (start, end) = window();
        let variants = [
            ("2026-10-01T00:00:00Z", "2026-10-08T12:31:36Z"),
            ("2026-10-01T00:00:00+00:00", "2026-10-08T12:31:36.000+00:00"),
            ("2026-10-01T02:00:00+02:00", "2026-10-08T07:31:36-05:00"),
        ];
        for (period_start, period_end) in variants {
            let data = serde_json::json!({
                "total_cost_usd": 1.5,
                "period": { "start": period_start, "end": period_end }
            });
            assert!(
                parse_usage(&data, start, end).is_ok(),
                "{period_start} / {period_end}"
            );
        }
        for (period_start, period_end) in [
            ("2026-10-01T00:00:01Z", "2026-10-08T12:31:36Z"),
            ("not a date", "2026-10-08T12:31:36Z"),
        ] {
            let data = serde_json::json!({
                "total_cost_usd": 1.5,
                "period": { "start": period_start, "end": period_end }
            });
            assert!(parse_usage(&data, start, end).is_err());
        }
    }

    #[test]
    fn parse_failures_use_the_upstream_message() {
        let (start, end) = window();
        for data in [
            serde_json::json!([]),
            serde_json::json!({ "period": {} }),
            serde_json::json!({ "total_cost_usd": 1.0 }),
        ] {
            match parse_usage(&data, start, end) {
                Err(ProviderError::Parse(message)) => assert_eq!(message, UNRECOGNIZED),
                other => panic!("unexpected result {other:?}"),
            }
        }
    }

    #[test]
    fn missing_key_id_needs_authentication() {
        let provider = ExaProvider::new();
        assert_eq!(
            provider.error_state_kind(&ProviderError::Other(MISSING_KEY_ID.into())),
            ProviderStateKind::NeedsAuthentication
        );
    }

    #[test]
    fn spend_uses_shared_usd_formatting() {
        let (start, end) = window();
        let data = serde_json::json!({
            "total_cost_usd": 1234.5,
            "period": {
                "start": "2026-10-01T00:00:00.000Z",
                "end": "2026-10-08T12:31:36.000Z"
            }
        });
        let result = parse_usage(&data, start, end).unwrap();
        assert_eq!(result.details[0].value(), "$1,234.50");
        assert_eq!(
            result.usage.primary.reset_description.as_deref(),
            Some("Spend: $1,234.50")
        );
    }
}
