//! GitKraken AI usage provider.
//!
//! The API returns personal credits and, when an organization is selected,
//! its shared-pool total. Shared usage is a slice of that total and is only
//! exposed as display detail; it must not be added to personal usage.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::StatusCode;
use serde_json::Value;
use std::time::Duration;

use crate::core::{
    FetchContext, Provider, ProviderDisplayDetail, ProviderError, ProviderFetchResult, ProviderId,
    ProviderMetadata, RateWindow, SourceMode, UsageSnapshot,
};
use crate::providers::{BoundedBodyError, read_bounded_response};

const USAGE_URL: &str = "https://api.gitkraken.dev/v1/ai-tasks/usage";
const CREDENTIAL_TARGET: &str = "codexbar-gitkraken";
const TOKEN_ENV: &str = "GITKRAKEN_API_TOKEN";
const ORGANIZATION_ENV: &str = "GITKRAKEN_ORG_ID";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_RESPONSE_BYTES: usize = 64 * 1024;
const WEEKLY_WINDOW_MINUTES: u32 = 10_080;

#[derive(Debug, Clone, Copy, PartialEq)]
struct Quota {
    used: f64,
    limit: f64,
}

#[derive(Debug, Clone, PartialEq)]
struct GitKrakenUsage {
    personal: Quota,
    organization: Option<Quota>,
    shared_used: Option<f64>,
    resets_at: DateTime<Utc>,
}

/// GitKraken AI provider.
pub struct GitKrakenProvider {
    metadata: ProviderMetadata,
}

impl GitKrakenProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: ProviderId::GitKraken,
                display_name: "GitKraken AI",
                session_label: "Personal",
                weekly_label: "Shared pool",
                supports_opus: false,
                supports_credits: false,
                default_enabled: false,
                is_primary: false,
                dashboard_url: Some("https://gitkraken.dev/account#ai-usage"),
                status_page_url: None,
                tertiary_label_key: None,
            },
        }
    }

    async fn fetch(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        self.fetch_from(ctx, USAGE_URL).await
    }

    async fn fetch_from(
        &self,
        ctx: &FetchContext,
        usage_url: &str,
    ) -> Result<ProviderFetchResult, ProviderError> {
        let token = resolve_token(ctx.api_key.as_deref())?;
        let organization = resolve_organization(ctx.workspace_id.as_deref())?;
        let client = crate::core::credentialed_http_client_builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .map_err(|error| {
                ProviderError::Other(format!("Could not create GitKraken client: {error}"))
            })?;

        let mut request = client
            .get(usage_url)
            .bearer_auth(token)
            .header("Client-Name", "CodexBar")
            .header("Client-Version", env!("CARGO_PKG_VERSION"))
            .header("User-Agent", "CodexBar");
        if let Some(organization) = organization.as_deref() {
            request = request.header("gk-org-id", organization);
        }

        let response = request.send().await?;
        let status = response.status();
        if status != StatusCode::OK {
            return Err(status_error(status));
        }

        let body = read_bounded_response(response, MAX_RESPONSE_BYTES)
            .await
            .map_err(|error| match error {
                BoundedBodyError::TooLarge => parse_failure("response exceeded the size limit"),
                BoundedBodyError::Read(error) => ProviderError::Network(error),
            })?;
        let body = std::str::from_utf8(&body)
            .map_err(|_| parse_failure("response was not valid UTF-8"))?;
        let value: Value =
            serde_json::from_str(body).map_err(|_| parse_failure("response was not valid JSON"))?;
        let usage = parse_usage(&value)?;

        Ok(result_from_usage(usage))
    }
}

impl Default for GitKrakenProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for GitKrakenProvider {
    fn id(&self) -> ProviderId {
        ProviderId::GitKraken
    }

    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        match ctx.source_mode {
            SourceMode::Auto | SourceMode::OAuth => self.fetch(ctx).await,
            source => Err(ProviderError::UnsupportedSource(source)),
        }
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::OAuth]
    }
}

fn resolve_token(configured: Option<&str>) -> Result<String, ProviderError> {
    let keyring_token = keyring::Entry::new(CREDENTIAL_TARGET, "api_token")
        .ok()
        .and_then(|entry| entry.get_password().ok());
    let token = configured
        .filter(|token| !token.trim().is_empty())
        .map(str::to_string)
        .or(keyring_token)
        .or_else(|| std::env::var(TOKEN_ENV).ok())
        .unwrap_or_default();
    let token = token.trim();
    if token.is_empty() {
        return Err(ProviderError::NotInstalled(format!(
            "Set a GitKraken access token in Settings or {TOKEN_ENV}."
        )));
    }
    if token.len() > 16_384 || !token.bytes().all(|byte| (0x21..=0x7e).contains(&byte)) {
        return Err(ProviderError::AuthRequired);
    }
    Ok(token.to_string())
}

fn resolve_organization(configured: Option<&str>) -> Result<Option<String>, ProviderError> {
    let organization = configured
        .map(str::to_string)
        .or_else(|| std::env::var(ORGANIZATION_ENV).ok())
        .unwrap_or_default();
    let organization = organization.trim();
    if organization.is_empty() {
        return Ok(None);
    }
    if organization.len() > 256
        || !organization
            .bytes()
            .all(|byte| (0x21..=0x7e).contains(&byte))
    {
        return Err(ProviderError::Other(
            "Invalid GitKraken organization ID. Enter a single ID without whitespace.".into(),
        ));
    }
    Ok(Some(organization.to_string()))
}

fn status_error(status: StatusCode) -> ProviderError {
    match status {
        StatusCode::UNAUTHORIZED => ProviderError::AuthRequired,
        StatusCode::FORBIDDEN => {
            ProviderError::Other("GitKraken denied access to this account or organization.".into())
        }
        StatusCode::TOO_MANY_REQUESTS => {
            ProviderError::Other("GitKraken rate limited usage requests.".into())
        }
        status if status.is_server_error() => {
            ProviderError::Other("GitKraken usage is temporarily unavailable.".into())
        }
        _ => ProviderError::Other(format!("GitKraken returned HTTP {status}.")),
    }
}

fn parse_usage(body: &Value) -> Result<GitKrakenUsage, ProviderError> {
    let data = body
        .get("data")
        .and_then(Value::as_object)
        .ok_or_else(|| parse_failure("missing data object"))?;
    if body.get("error").is_some_and(|error| !error.is_null()) {
        return Err(parse_failure("API returned an error envelope"));
    }

    let personal = parse_quota(&Value::Object(data.clone()))
        .ok_or_else(|| parse_failure("invalid personal quota"))?;
    let resets_at = data
        .get("resetsOn")
        .and_then(Value::as_str)
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.with_timezone(&Utc))
        .ok_or_else(|| parse_failure("invalid reset timestamp"))?;

    // Upstream deliberately ignores a malformed optional organization quota
    // without discarding the valid personal quota.
    let organization = data.get("organization").and_then(parse_quota);
    let shared_used = data
        .get("sharedUsed")
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite() && *value >= 0.0)
        .filter(|value| organization.is_some_and(|pool| *value <= pool.used));

    Ok(GitKrakenUsage {
        personal,
        organization,
        shared_used,
        resets_at,
    })
}

fn parse_quota(value: &Value) -> Option<Quota> {
    let object = value.as_object()?;
    let used = object.get("used")?.as_f64()?;
    let limit = object.get("limit")?.as_f64()?;
    (used.is_finite()
        && used >= 0.0
        && limit.is_finite()
        && (limit >= 0.0 || limit == -1.0)
        && (limit <= 0.0 || ((used / limit) * 100.0).is_finite()))
    .then_some(Quota { used, limit })
}

fn result_from_usage(usage: GitKrakenUsage) -> ProviderFetchResult {
    let personal = usage.personal;
    let primary = rate_window(personal, usage.resets_at)
        .unwrap_or_else(|| RateWindow::informational(quota_description(personal)));
    let mut snapshot = UsageSnapshot::new(primary)
        .with_primary_label("Personal")
        .with_login_method("API");
    if let Some(pool) = usage.organization
        && let Some(window) = rate_window(pool, usage.resets_at)
    {
        snapshot = snapshot
            .with_secondary(window)
            .with_secondary_label("Shared pool");
    }

    let mut result = ProviderFetchResult::new(snapshot, "api").with_display_detail(
        ProviderDisplayDetail::new("personal-credits", "Personal", quota_description(personal)),
    );
    if let Some(pool) = usage.organization {
        result = result.with_display_detail(ProviderDisplayDetail::new(
            "shared-pool-credits",
            "Shared pool",
            quota_description(pool),
        ));
        if let Some(shared_used) = usage.shared_used {
            result = result
                .with_display_detail(ProviderDisplayDetail::new(
                    "your-shared-usage",
                    "Your shared usage",
                    format!("{} credits", format_number(shared_used)),
                ))
                .with_display_detail(ProviderDisplayDetail::new(
                    "organization-rest-usage",
                    "Rest of organization",
                    format!("{} credits", format_number(pool.used - shared_used)),
                ));
        }
    }
    if personal.limit <= 0.0 && usage.organization.is_none_or(|pool| pool.limit <= 0.0) {
        result = result.with_display_detail(ProviderDisplayDetail::new(
            "reset",
            "Reset",
            usage.resets_at.to_rfc3339(),
        ));
    }
    result
}

fn rate_window(quota: Quota, resets_at: DateTime<Utc>) -> Option<RateWindow> {
    (quota.limit > 0.0).then(|| {
        RateWindow::with_details(
            (quota.used / quota.limit) * 100.0,
            Some(WEEKLY_WINDOW_MINUTES),
            Some(resets_at),
            None,
        )
    })
}

fn quota_description(quota: Quota) -> String {
    if quota.limit <= 0.0 {
        format!(
            "{} credits used · {}",
            format_number(quota.used),
            if quota.limit == -1.0 {
                "Unlimited"
            } else {
                "No allowance"
            }
        )
    } else {
        format!(
            "{} / {} credits used",
            format_number(quota.used),
            format_number(quota.limit)
        )
    }
}

fn format_number(value: f64) -> String {
    let rounded = if value.abs() <= f64::MAX / 100.0 {
        (value * 100.0).round() / 100.0
    } else {
        value
    };
    let fixed = format!("{rounded:.2}");
    let trimmed = fixed.trim_end_matches('0').trim_end_matches('.');
    let (integer, fraction) = trimmed.split_once('.').unwrap_or((trimmed, ""));
    let mut grouped = String::with_capacity(trimmed.len() + integer.len() / 3);
    for (index, digit) in integer.chars().enumerate() {
        if index > 0 && (integer.len() - index) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    if !fraction.is_empty() {
        grouped.push('.');
        grouped.push_str(fraction);
    }
    grouped
}

fn parse_failure(reason: &str) -> ProviderError {
    ProviderError::Parse(format!(
        "GitKraken returned an unrecognized usage response ({reason})."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    fn response(personal_limit: Value, organization: Value, shared_used: Value) -> Value {
        json!({
            "data": {
                "used": 12_500,
                "limit": personal_limit,
                "resetsOn": "2026-09-27T00:00:00Z",
                "organization": organization,
                "sharedUsed": shared_used,
            },
            "error": null,
        })
    }

    async fn assert_request_headers(organization: Option<&str>) {
        let expected_organization = organization.map(str::to_owned);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut chunk = [0; 1024];
            loop {
                let read = stream.read(&mut chunk).await.unwrap();
                assert_ne!(read, 0, "client closed before sending HTTP headers");
                request.extend_from_slice(&chunk[..read]);
                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            let request = String::from_utf8(request).unwrap().to_ascii_lowercase();
            assert!(request.contains("authorization: bearer fixture-token\r\n"));
            match expected_organization.as_deref() {
                Some(organization) => assert!(request.contains(&format!(
                    "gk-org-id: {}\r\n",
                    organization.to_ascii_lowercase()
                ))),
                None => assert!(!request.contains("gk-org-id:")),
            }

            let body =
                r#"{"data":{"used":1,"limit":10,"resetsOn":"2026-09-27T00:00:00Z"},"error":null}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).await.unwrap();
        });

        let ctx = FetchContext {
            api_key: Some("fixture-token".into()),
            workspace_id: Some(organization.unwrap_or("").into()),
            ..FetchContext::default()
        };
        GitKrakenProvider::new()
            .fetch_from(&ctx, &format!("http://{address}/usage"))
            .await
            .unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn sends_bearer_token_and_optional_organization_header() {
        assert_request_headers(Some("org-fixture")).await;
        assert_request_headers(None).await;
    }

    #[test]
    fn parses_personal_and_organization_weekly_quotas_with_shared_usage_as_display_only() {
        let usage = parse_usage(&response(
            json!(400_000),
            json!({ "used": 20_000, "limit": 100_000 }),
            json!(5_000),
        ))
        .unwrap();
        assert_eq!(
            usage.personal,
            Quota {
                used: 12_500.0,
                limit: 400_000.0
            }
        );
        assert_eq!(
            usage.organization,
            Some(Quota {
                used: 20_000.0,
                limit: 100_000.0
            })
        );
        assert_eq!(usage.shared_used, Some(5_000.0));
        assert_eq!(
            usage.resets_at,
            DateTime::parse_from_rfc3339("2026-09-27T00:00:00Z")
                .unwrap()
                .with_timezone(&Utc)
        );

        let result = result_from_usage(usage);
        // Personal usage stays at 12,500 credits (3.125%); the shared slice
        // is display detail and must never be added to personal usage.
        assert_eq!(result.usage.primary.used_percent, 3.125);
        assert_eq!(
            result.usage.primary.window_minutes,
            Some(WEEKLY_WINDOW_MINUTES)
        );
        assert_eq!(result.usage.secondary.as_ref().unwrap().used_percent, 20.0);
        assert_eq!(
            result
                .display_details()
                .iter()
                .map(ProviderDisplayDetail::value)
                .collect::<Vec<_>>(),
            [
                "12,500 / 400,000 credits used",
                "20,000 / 100,000 credits used",
                "5,000 credits",
                "15,000 credits",
            ]
        );
    }

    #[test]
    fn malformed_optional_organization_does_not_discard_personal_usage() {
        for organization in [json!(null), json!({}), json!({"used":-1,"limit":100})] {
            let usage = parse_usage(&response(json!(400_000), organization, json!(1))).unwrap();
            assert_eq!(usage.personal.used, 12_500.0);
            assert!(usage.organization.is_none());
            assert!(usage.shared_used.is_none());
        }
    }

    #[test]
    fn invalid_shared_slice_is_ignored_without_changing_pool_total() {
        for shared_used in [
            json!(null),
            json!(true),
            json!("5"),
            json!(-1),
            json!(20_001),
        ] {
            let usage = parse_usage(&response(
                json!(400_000),
                json!({"used":20_000,"limit":100_000}),
                shared_used,
            ))
            .unwrap();
            assert_eq!(usage.organization.unwrap().used, 20_000.0);
            assert!(usage.shared_used.is_none());
        }
    }

    #[test]
    fn zero_and_unlimited_limits_are_informational_not_fake_percentages() {
        for (limit, expected) in [(json!(0), "No allowance"), (json!(-1), "Unlimited")] {
            let usage = parse_usage(&response(limit, json!(null), json!(null))).unwrap();
            let result = result_from_usage(usage);
            assert!(result.usage.primary.is_informational);
            assert_eq!(result.usage.primary.used_percent, 0.0);
            assert_eq!(
                result.display_details()[0].value(),
                format!("12,500 credits used · {expected}")
            );
            assert_eq!(result.display_details().last().unwrap().title(), "Reset");
        }
    }

    #[test]
    fn malformed_required_personal_quota_or_reset_fails_closed() {
        let invalid = [
            json!({"data":{"limit":10,"resetsOn":"2026-09-27T00:00:00Z"}}),
            json!({"data":{"used":true,"limit":10,"resetsOn":"2026-09-27T00:00:00Z"}}),
            json!({"data":{"used":-1,"limit":10,"resetsOn":"2026-09-27T00:00:00Z"}}),
            json!({"data":{"used":1,"limit":-0.5,"resetsOn":"2026-09-27T00:00:00Z"}}),
            json!({"data":{"used":1,"limit":10,"resetsOn":"2026-09-27"}}),
            json!({"data":{"used":1,"limit":10,"resetsOn":null}}),
            json!({"error":"failure","data":{"used":1,"limit":10,"resetsOn":"2026-09-27T00:00:00Z"}}),
        ];
        for value in invalid {
            assert!(matches!(parse_usage(&value), Err(ProviderError::Parse(_))));
        }
    }

    #[test]
    fn validates_configured_token_and_organization_values() {
        assert_eq!(
            resolve_token(Some(" fixture-token ")).unwrap(),
            "fixture-token"
        );
        assert!(matches!(
            resolve_token(Some("bad token")),
            Err(ProviderError::AuthRequired)
        ));
        assert!(matches!(
            resolve_token(Some("bad\ntoken")),
            Err(ProviderError::AuthRequired)
        ));
        assert!(matches!(
            resolve_token(Some(&"x".repeat(16_385))),
            Err(ProviderError::AuthRequired)
        ));

        assert_eq!(
            resolve_organization(Some(" org-fixture ")).unwrap(),
            Some("org-fixture".into())
        );
        assert_eq!(resolve_organization(Some("  ")).unwrap(), None);
        assert!(matches!(
            resolve_organization(Some("org id")),
            Err(ProviderError::Other(_))
        ));
        assert!(matches!(
            resolve_organization(Some("org\nid")),
            Err(ProviderError::Other(_))
        ));
        assert!(matches!(
            resolve_organization(Some(&"x".repeat(257))),
            Err(ProviderError::Other(_))
        ));
    }

    #[test]
    fn response_statuses_match_upstream_error_classes_without_reading_private_bodies() {
        assert!(matches!(
            status_error(StatusCode::UNAUTHORIZED),
            ProviderError::AuthRequired
        ));
        assert!(matches!(
            status_error(StatusCode::FORBIDDEN),
            ProviderError::Other(_)
        ));
        assert!(matches!(
            status_error(StatusCode::TOO_MANY_REQUESTS),
            ProviderError::Other(_)
        ));
        assert!(matches!(
            status_error(StatusCode::SERVICE_UNAVAILABLE),
            ProviderError::Other(_)
        ));
        assert!(matches!(
            status_error(StatusCode::NOT_FOUND),
            ProviderError::Other(_)
        ));
    }

    #[test]
    fn formats_counts_with_at_most_two_fractional_digits() {
        assert_eq!(format_number(12_500.0), "12,500");
        assert_eq!(format_number(12_500.125), "12,500.13");
    }
}
