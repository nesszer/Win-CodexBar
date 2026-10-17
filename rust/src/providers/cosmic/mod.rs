//! Cosmic AI project token usage provider
//!
//! Ported from upstream CodexBar v0.74.1 (`Resources/Plugins/cosmic.ts`,
//! `Providers/Cosmic/CosmicProviderDescriptor.swift`). Reads project AI-token
//! usage from the dashboard API with a Personal Access Token (bearer) and a
//! Project ID. Response bodies are never echoed in errors.

use std::sync::LazyLock;
use std::time::Duration;

use async_trait::async_trait;
use regex_lite::Regex;
use reqwest::Client;
use serde_json::{Map, Value};

use super::format;
use crate::core::{
    FetchContext, Provider, ProviderDisplayDetail, ProviderError, ProviderFetchResult, ProviderId,
    ProviderMetadata, ProviderStateKind, RateWindow, SourceMode, UsageSnapshot,
};
use crate::providers::{BoundedBodyError, read_bounded_response};

const USAGE_URL: &str = "https://dapi.cosmicjs.com/v3/projects/usage";
const ORIGIN: &str = "https://app.cosmicjs.com";
const CREDENTIAL_TARGET: &str = "codexbar-cosmic";
const TOKEN_ENV: &str = "COSMIC_TOKEN";
const PROJECT_ENV: &str = "COSMIC_PROJECT_ID";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_RESPONSE_BYTES: usize = 256 * 1024;

const MISSING_PROJECT: &str = "Set a Cosmic Project ID in Settings or COSMIC_PROJECT_ID.";
const MISSING_TOKEN: &str = "Set a Cosmic Personal Access Token in Settings or COSMIC_TOKEN.";
const UNRECOGNIZED: &str = "Cosmic AI returned an unrecognized project AI-token usage response.";
const NO_CLIENT: &str = "Could not create a secure Cosmic AI HTTP client.";

/// Upstream `count` safe-integer bound (`Number.MAX_SAFE_INTEGER`).
const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

/// Upstream allowance pattern, case-insensitive like the `/i` flag.
static ALLOWANCE: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"(?i)^(\d+(?:\.\d+)?)\s*([km]?)$").ok());

pub struct CosmicProvider {
    client: Option<Client>,
}

impl CosmicProvider {
    pub fn new() -> Self {
        Self {
            client: crate::core::credentialed_http_client_builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(REQUEST_TIMEOUT)
                .build()
                .ok(),
        }
    }

    async fn fetch_api(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        let project_id = resolve_project_id(
            ctx.workspace_id.as_deref(),
            std::env::var(PROJECT_ENV).ok().as_deref(),
        )
        .ok_or_else(|| ProviderError::Other(MISSING_PROJECT.into()))?;

        let token = crate::providers::resolve_api_key(
            ctx.api_key.as_deref(),
            CREDENTIAL_TARGET,
            &[TOKEN_ENV],
        )
        .map_err(|_| ProviderError::Other(MISSING_TOKEN.into()))?;

        let client = self
            .client
            .as_ref()
            .ok_or_else(|| ProviderError::Other(NO_CLIENT.into()))?;

        let response = client
            .execute(build_request(client, &project_id, &token)?)
            .await?;

        check_status(response.status().as_u16())?;

        let body = read_bounded_response(response, MAX_RESPONSE_BYTES)
            .await
            .map_err(|error| match error {
                BoundedBodyError::TooLarge => unrecognized(),
                BoundedBodyError::Read(error) => ProviderError::Network(error),
            })?;

        let root: Value = serde_json::from_slice(&body).map_err(|_| unrecognized())?;
        parse_cosmic_response(&root)
    }
}

impl Default for CosmicProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for CosmicProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Cosmic
    }

    fn metadata(&self) -> &ProviderMetadata {
        &COSMIC_METADATA
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        match ctx.source_mode {
            SourceMode::Auto | SourceMode::OAuth => self.fetch_api(ctx).await,
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
            ProviderError::Other(message)
                if message == MISSING_PROJECT || message == MISSING_TOKEN =>
            {
                ProviderStateKind::NeedsAuthentication
            }
            // Upstream classifies 401 as `authenticationExpired`.
            ProviderError::Other(message) if *message == http_message(401) => {
                ProviderStateKind::ExpiredSession
            }
            _ => error.state_kind(),
        }
    }
}

/// Settings Project ID first, then `COSMIC_PROJECT_ID` (upstream
/// `workspaceField.environmentKey`); blank values are absent.
fn resolve_project_id(workspace_id: Option<&str>, env_value: Option<&str>) -> Option<String> {
    [workspace_id, env_value]
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|value| !value.is_empty())
        .map(str::to_owned)
}

fn build_request(
    client: &Client,
    project_id: &str,
    token: &str,
) -> Result<reqwest::Request, ProviderError> {
    client
        .get(USAGE_URL)
        .query(&[("project_id", project_id)])
        .bearer_auth(token)
        .header("Origin", ORIGIN)
        .build()
        .map_err(ProviderError::Network)
}

fn http_message(status: u16) -> String {
    format!("Cosmic AI returned HTTP {status}.")
}

/// Upstream status classification: 401 authenticationExpired, 403
/// permissionDenied, 429 rateLimited, 5xx providerUnavailable, any other
/// non-200 apiFailure. All share one message.
fn check_status(status: u16) -> Result<(), ProviderError> {
    if status == 200 {
        Ok(())
    } else {
        Err(ProviderError::Other(http_message(status)))
    }
}

fn unrecognized() -> ProviderError {
    ProviderError::Other(UNRECOGNIZED.into())
}

/// Upstream `record`: a JSON object, never an array.
fn record(value: Option<&Value>) -> Option<&Map<String, Value>> {
    value.and_then(Value::as_object)
}

/// Upstream `count`: absent or null is unknown; anything else must be a
/// non-negative safe integer.
fn count(value: Option<&Value>) -> Result<Option<f64>, ProviderError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(n)) => safe_count(n.as_f64().ok_or_else(unrecognized)?).map(Some),
        Some(_) => Err(unrecognized()),
    }
}

fn safe_count(value: f64) -> Result<f64, ProviderError> {
    if value.fract() == 0.0 && (0.0..=MAX_SAFE_INTEGER).contains(&value) {
        // `+ 0.0` turns -0 into 0.
        Ok(value + 0.0)
    } else {
        Err(unrecognized())
    }
}

/// Upstream `limit`: non-strings go through `count`; strings must match the
/// allowance pattern (otherwise unknown) and scale to a safe integer.
fn limit(value: Option<&Value>) -> Result<Option<f64>, ProviderError> {
    let Some(Value::String(raw)) = value else {
        return count(value);
    };
    let Some(captures) = ALLOWANCE
        .as_ref()
        .and_then(|pattern| pattern.captures(raw.trim()))
    else {
        return Ok(None);
    };
    let number: f64 = captures[1].parse().map_err(|_| unrecognized())?;
    let multiplier = match captures[2].to_ascii_lowercase().as_str() {
        "m" => 1_000_000.0,
        "k" => 1_000.0,
        _ => 1.0,
    };
    safe_count(number * multiplier).map(Some)
}

/// Upstream `window`: detail rows for one counter and its percentage when
/// both sides are known and the allowance is positive.
fn window(
    details: &mut Vec<ProviderDisplayDetail>,
    key: &str,
    title: &str,
    raw_used: Option<&Value>,
    raw_limit: Option<&Value>,
) -> Result<Option<f64>, ProviderError> {
    let used = count(raw_used)?;
    let total = limit(raw_limit)?;

    let mut rows: Vec<(&str, &str, String)> = Vec::new();
    if let Some(used) = used {
        rows.push(("used", "Used", format::number(used, 0)));
    }
    if let Some(total) = total {
        rows.push(("allowance", "Allowance", format::number(total, 0)));
    } else if let Some(Value::String(raw)) = raw_limit
        && !raw.trim().is_empty()
    {
        let label = if raw.trim().eq_ignore_ascii_case("unlimited") {
            "Unlimited"
        } else {
            "Not reported"
        };
        rows.push(("allowance", "Allowance", label.to_string()));
    }
    if let (Some(used), Some(total)) = (used, total) {
        rows.push((
            "remaining",
            "Remaining",
            format::number((total - used).max(0.0), 0),
        ));
        if used > total {
            rows.push(("above", "Above allowance", format::number(used - total, 0)));
        }
    }
    for (suffix, label, value) in rows {
        if let Some(detail) = ProviderDisplayDetail::new(format!("{key}_{suffix}"), label, value)
            .and_then(|row| row.with_section_title(title))
        {
            details.push(detail);
        }
    }

    Ok(match (used, total) {
        (Some(used), Some(total)) if total > 0.0 => Some((used / total * 100.0).clamp(0.0, 100.0)),
        _ => None,
    })
}

fn parse_cosmic_response(root: &Value) -> Result<ProviderFetchResult, ProviderError> {
    let root = root.as_object();
    let usage = record(record(root.and_then(|r| r.get("usage"))).and_then(|u| u.get("ai")));
    let allowance =
        record(record(root.and_then(|r| r.get("plan_info"))).and_then(|p| p.get("ai_tokens")));

    let mut details = Vec::new();
    let primary = window(
        &mut details,
        "input_tokens",
        "Input tokens",
        usage.and_then(|u| u.get("input_tokens")),
        allowance.and_then(|a| a.get("max_input")),
    )?;
    let secondary = window(
        &mut details,
        "output_tokens",
        "Output tokens",
        usage.and_then(|u| u.get("output_tokens")),
        allowance.and_then(|a| a.get("max_output")),
    )?;
    if details.is_empty() {
        return Err(unrecognized());
    }

    // Without a window upstream draws no input bar, so the placeholder stays
    // off the card.
    let primary = primary.map_or_else(
        || RateWindow::informational("Input tokens").hidden_on_card(),
        RateWindow::new,
    );
    let mut snapshot = UsageSnapshot::new(primary).with_secondary_label("Output tokens");
    if let Some(percent) = secondary {
        snapshot = snapshot.with_secondary(RateWindow::new(percent));
    }

    let mut result = ProviderFetchResult::new(snapshot, "api");
    result.display_details = details;
    Ok(result)
}

const COSMIC_METADATA: ProviderMetadata = ProviderMetadata {
    id: ProviderId::Cosmic,
    display_name: "Cosmic AI",
    session_label: "Input tokens",
    weekly_label: "Output tokens",
    supports_opus: false,
    supports_credits: false,
    default_enabled: false,
    is_primary: false,
    dashboard_url: Some("https://app.cosmicjs.com"),
    status_page_url: None,
    tertiary_label_key: None,
};

#[cfg(test)]
mod tests {
    use super::*;

    /// Upstream fixture: public cosmicjs/cli ProjectUsageResponse schema with
    /// synthetic counters and unrelated paid data.
    const FIXTURE: &str = r#"{"usage":{"ai":{"input_tokens":75000,"output_tokens":150000}},
     "plan_info":{"ai_tokens":{"max_input":"300k","max_output":"1.5M"}},
     "token_pack":{"remaining":10000000},"resets_at":"2026-11-01T00:00:00Z"}"#;

    /// The provider's body path: JSON parse failure is a parse failure too.
    fn parse(body: &str) -> Result<ProviderFetchResult, ProviderError> {
        let root: Value = serde_json::from_str(body).map_err(|_| unrecognized())?;
        parse_cosmic_response(&root)
    }

    fn titles(result: &ProviderFetchResult) -> Vec<String> {
        let mut titles: Vec<String> = Vec::new();
        for detail in &result.display_details {
            let title = detail.section_title().unwrap_or_default().to_string();
            if titles.last() != Some(&title) {
                titles.push(title);
            }
        }
        titles
    }

    fn section_values(result: &ProviderFetchResult, title: &str) -> Vec<String> {
        result
            .display_details
            .iter()
            .filter(|d| d.section_title() == Some(title))
            .map(|d| d.value().to_string())
            .collect()
    }

    fn section_labels(result: &ProviderFetchResult, title: &str) -> Vec<String> {
        result
            .display_details
            .iter()
            .filter(|d| d.section_title() == Some(title))
            .map(|d| d.title().to_string())
            .collect()
    }

    fn first_section_values(result: &ProviderFetchResult) -> Vec<String> {
        let first = titles(result).into_iter().next().unwrap_or_default();
        section_values(result, &first)
    }

    /// Upstream `primary == nil`.
    fn assert_no_primary(result: &ProviderFetchResult) {
        assert!(result.usage.primary.is_informational);
        assert!(result.usage.primary.hidden_on_card);
    }

    fn assert_parse_failure(body: &str) {
        match parse(body) {
            Err(ProviderError::Other(message)) => {
                assert_eq!(message, UNRECOGNIZED, "body {body}");
                assert!(!message.contains("private-response"));
            }
            other => panic!("expected parse failure for {body}, got {other:?}"),
        }
    }

    #[test]
    fn project_input_and_output_allowances_stay_separate() {
        let result = parse(FIXTURE).unwrap();
        assert!(!result.usage.primary.is_informational);
        assert_eq!(result.usage.primary.used_percent, 25.0);
        let secondary = result.usage.secondary.as_ref().unwrap();
        assert_eq!(secondary.used_percent, 10.0);
        assert!(result.usage.primary.resets_at.is_none());
        assert!(secondary.resets_at.is_none());
        assert!(result.usage.primary.window_minutes.is_none());
        assert!(result.cost.is_none());
        assert_eq!(titles(&result), ["Input tokens", "Output tokens"]);
        assert_eq!(
            section_values(&result, "Input tokens"),
            ["75,000", "300,000", "225,000"]
        );
        assert_eq!(
            section_values(&result, "Output tokens"),
            ["150,000", "1,500,000", "1,350,000"]
        );
        assert_eq!(
            section_labels(&result, "Input tokens"),
            ["Used", "Allowance", "Remaining"]
        );
    }

    #[test]
    fn missing_counters_and_zero_allowances_are_never_defaulted() {
        for (body, values) in [
            (r#"{"usage":{"ai":{"input_tokens":0}}}"#, vec!["0"]),
            (
                r#"{"plan_info":{"ai_tokens":{"max_input":"300k"}}}"#,
                vec!["300,000"],
            ),
            (
                r#"{"usage":{"ai":{"input_tokens":0}},"plan_info":{"ai_tokens":{"max_input":"0"}}}"#,
                vec!["0", "0", "0"],
            ),
        ] {
            let result = parse(body).unwrap();
            assert_no_primary(&result);
            assert!(result.usage.secondary.is_none(), "{body}");
            assert_eq!(first_section_values(&result), values, "{body}");
        }
    }

    #[test]
    fn nonnumeric_allowances_never_become_numeric_quotas() {
        for (allowance, label) in [("Unlimited", "Unlimited"), ("300k-junk", "Not reported")] {
            let body = format!(
                r#"{{"usage":{{"ai":{{"input_tokens":10}}}},"plan_info":{{"ai_tokens":{{"max_input":"{allowance}"}}}}}}"#
            );
            let result = parse(&body).unwrap();
            assert_no_primary(&result);
            assert_eq!(first_section_values(&result), ["10", label]);
        }
    }

    #[test]
    fn observed_over_allowance_counters_are_preserved() {
        let result = parse(
            r#"{"usage":{"ai":{"output_tokens":400000}},"plan_info":{"ai_tokens":{"max_output":"300K"}}}"#,
        )
        .unwrap();
        assert_no_primary(&result);
        assert_eq!(result.usage.secondary.as_ref().unwrap().used_percent, 100.0);
        assert_eq!(
            first_section_values(&result),
            ["400,000", "300,000", "0", "100,000"]
        );
        assert_eq!(
            section_labels(&result, "Output tokens"),
            ["Used", "Allowance", "Remaining", "Above allowance"]
        );
    }

    #[test]
    fn project_identifier_is_encoded_as_one_query_value() {
        let client = Client::new();
        let project_id = "project &other=value";
        let request = build_request(&client, project_id, "fixture-token").unwrap();
        let url = request.url();
        assert_eq!(url.scheme(), "https");
        assert_eq!(url.host_str(), Some("dapi.cosmicjs.com"));
        assert_eq!(url.path(), "/v3/projects/usage");
        let pairs: Vec<(String, String)> = url
            .query_pairs()
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
        assert_eq!(pairs, [("project_id".to_string(), project_id.to_string())]);
        assert_eq!(request.method(), reqwest::Method::GET);
        assert_eq!(
            request.headers().get("Authorization").unwrap(),
            "Bearer fixture-token"
        );
        assert_eq!(request.headers().get("Origin").unwrap(), ORIGIN);
    }

    #[test]
    fn missing_project_fails_before_a_request() {
        assert_eq!(resolve_project_id(Some(" "), None), None);
        assert_eq!(resolve_project_id(None, Some("  ")), None);
        assert_eq!(
            resolve_project_id(Some(" project-fixture "), Some("env-project")).as_deref(),
            Some("project-fixture")
        );
        assert_eq!(
            resolve_project_id(Some(" "), Some(" env-project ")).as_deref(),
            Some("env-project")
        );
    }

    #[tokio::test]
    async fn missing_project_is_a_missing_credential() {
        if std::env::var(PROJECT_ENV).is_ok_and(|v| !v.trim().is_empty()) {
            return;
        }
        let provider = CosmicProvider::new();
        let ctx = FetchContext {
            api_key: Some("fixture-token".to_string()),
            workspace_id: Some(" ".to_string()),
            ..Default::default()
        };
        let error = provider.fetch_usage(&ctx).await.unwrap_err();
        assert!(matches!(&error, ProviderError::Other(m) if m == MISSING_PROJECT));
        assert_eq!(
            provider.error_state_kind(&error),
            ProviderStateKind::NeedsAuthentication
        );
    }

    #[test]
    fn malformed_quota_data_fails_without_response_disclosure() {
        for value in [
            "-1",
            "0.5",
            "true",
            "\"private-response\"",
            "9007199254740992",
            "{}",
            "[]",
        ] {
            assert_parse_failure(&format!(
                r#"{{"usage":{{"ai":{{"input_tokens":{value}}}}}}}"#
            ));
        }
        for value in ["-1", "true", "\"0.0001k\"", "{}", "[]"] {
            assert_parse_failure(&format!(
                r#"{{"plan_info":{{"ai_tokens":{{"max_input":{value}}}}}}}"#
            ));
        }
        for body in [
            "private-response",
            "null",
            "[]",
            "{}",
            r#"{"usage":{"ai":{}}}"#,
        ] {
            assert_parse_failure(body);
        }
    }

    #[test]
    fn allowance_strings_follow_the_upstream_pattern() {
        let parse_limit = |raw: &str| limit(Some(&Value::String(raw.into())));
        assert_eq!(parse_limit("300k").unwrap(), Some(300_000.0));
        assert_eq!(parse_limit("300K").unwrap(), Some(300_000.0));
        assert_eq!(parse_limit("300 k").unwrap(), Some(300_000.0));
        assert_eq!(parse_limit(" 1.5M ").unwrap(), Some(1_500_000.0));
        assert_eq!(parse_limit("1000").unwrap(), Some(1_000.0));
        for unmatched in ["", "1.", ".5", "Unlimited", "300k-junk", "1kk", "-1"] {
            assert_eq!(parse_limit(unmatched).unwrap(), None, "{unmatched}");
        }
        for unsafe_value in ["0.0001k", "0.5", "1.0000001k"] {
            assert!(parse_limit(unsafe_value).is_err(), "{unsafe_value}");
        }
    }

    #[test]
    fn project_http_failures_are_classified() {
        let provider = CosmicProvider::new();
        assert!(check_status(200).is_ok());
        for (code, kind) in [
            (401, ProviderStateKind::ExpiredSession),
            (403, ProviderStateKind::Unknown),
            (429, ProviderStateKind::Unknown),
            (503, ProviderStateKind::Unknown),
            (400, ProviderStateKind::Unknown),
        ] {
            let error = check_status(code).unwrap_err();
            match &error {
                ProviderError::Other(message) => {
                    assert_eq!(*message, format!("Cosmic AI returned HTTP {code}."));
                    assert!(!message.contains("private-response"));
                }
                other => panic!("unexpected error {other:?}"),
            }
            assert_eq!(provider.error_state_kind(&error), kind, "{code}");
        }
    }

    #[test]
    fn registration_is_disabled_by_default_and_api_only() {
        let provider = CosmicProvider::new();
        assert!(!provider.metadata().default_enabled);
        assert_eq!(
            provider.available_sources(),
            [SourceMode::Auto, SourceMode::OAuth]
        );
    }

    #[tokio::test]
    async fn web_and_cli_sources_are_unsupported() {
        let provider = CosmicProvider::new();
        for mode in [SourceMode::Web, SourceMode::Cli] {
            let ctx = FetchContext {
                source_mode: mode,
                api_key: Some("fixture-token".to_string()),
                workspace_id: Some("project-fixture".to_string()),
                ..Default::default()
            };
            assert!(matches!(
                provider.fetch_usage(&ctx).await,
                Err(ProviderError::UnsupportedSource(m)) if m == mode
            ));
        }
    }

    #[test]
    fn unlimited_plan_draws_no_card_bars() {
        let root = serde_json::json!({
            "usage": { "ai": { "input_tokens": 1200, "output_tokens": 300 } },
            "plan_info": { "ai_tokens": { "max_input": "unlimited", "max_output": "unlimited" } }
        });
        let result = parse_cosmic_response(&root).unwrap();
        assert_no_primary(&result);
        assert!(result.usage.secondary.is_none());
        assert_eq!(
            section_values(&result, "Input tokens"),
            ["1,200", "Unlimited"]
        );
        assert_eq!(
            section_values(&result, "Output tokens"),
            ["300", "Unlimited"]
        );
    }
}
