use chrono::{DateTime, Utc};
use reqwest::Client;

use crate::core::{FetchContext, ProviderError, ProviderFetchResult, UsageSnapshot};

const USAGE_API_URL: &str = "https://opencode.ai/zen/go/v1/usage";

pub(super) fn normalized_api_key(raw: Option<&str>) -> Option<String> {
    let mut value = raw?.trim();
    if value.len() >= 2
        && ((value.starts_with('"') && value.ends_with('"'))
            || (value.starts_with('\'') && value.ends_with('\'')))
    {
        value = value[1..value.len() - 1].trim();
    }
    (!value.is_empty()).then(|| value.to_string())
}

pub(super) fn resolve_api_key(ctx: &FetchContext) -> Option<String> {
    normalized_api_key(ctx.api_key.as_deref()).or_else(|| {
        std::env::var("OPENCODE_API_KEY")
            .ok()
            .and_then(|value| normalized_api_key(Some(&value)))
    })
}

pub(super) fn selected_account_api_key(ctx: &FetchContext) -> Option<String> {
    normalized_api_key(ctx.api_key.as_deref())
}

/// Source label of a quota read made with the OpenCode CLI login.
pub(super) const CLI_SOURCE_LABEL: &str = "cli";

/// A selected account, manual cookie or workspace override names a specific
/// account, so the device-wide CLI login must not stand in for it.
pub(super) fn is_explicitly_scoped(ctx: &FetchContext) -> bool {
    ctx.token_account_isolated
        || ctx.auto_prefer_web
        || ctx
            .manual_cookie_header
            .as_deref()
            .is_some_and(|header| !header.trim().is_empty())
        || ctx
            .workspace_id
            .as_deref()
            .is_some_and(|id| !id.trim().is_empty())
}

/// The credential for the usage API and its source label (upstream
/// `usageCredential`, #4771): a configured API key, else the active OpenCode
/// CLI login unless Web mode or an explicit scope rules it out. A rejected
/// configured key never falls through to the CLI login.
pub(super) fn usage_credential(
    ctx: &FetchContext,
    load_cli_credential: impl FnOnce() -> Result<Option<String>, ProviderError>,
) -> Result<Option<(String, &'static str)>, ProviderError> {
    usage_credential_with(resolve_api_key(ctx), ctx, load_cli_credential)
}

fn usage_credential_with(
    api_key: Option<String>,
    ctx: &FetchContext,
    load_cli_credential: impl FnOnce() -> Result<Option<String>, ProviderError>,
) -> Result<Option<(String, &'static str)>, ProviderError> {
    if let Some(api_key) = api_key {
        return Ok(Some((api_key, "api")));
    }
    if ctx.source_mode == crate::core::SourceMode::Web || is_explicitly_scoped(ctx) {
        return Ok(None);
    }
    Ok(load_cli_credential()?.map(|token| (token, CLI_SOURCE_LABEL)))
}

pub(super) async fn fetch(
    client: &Client,
    ctx: &FetchContext,
    api_key: &str,
    source_label: &str,
) -> Result<ProviderFetchResult, ProviderError> {
    let response = client
        .get(USAGE_API_URL)
        .timeout(std::time::Duration::from_secs(ctx.web_timeout.max(1)))
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Accept", "application/json")
        .header("User-Agent", "CodexBar")
        .send()
        .await?;
    let status = response.status();
    let body = response.text().await?;
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Err(ProviderError::AuthRequired);
    }
    if !status.is_success() {
        return Err(api_error(status, &body));
    }
    let usage = parse_usage_text(&body, Utc::now())?;
    Ok(ProviderFetchResult::new(usage, source_label))
}

/// Build the error for a non-success usage response. The server-controlled
/// message is redacted before it can reach logs or the UI; a non-JSON body is
/// never echoed.
fn api_error(status: reqwest::StatusCode, body: &str) -> ProviderError {
    let message = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|value| {
            ["message", "error", "detail"]
                .into_iter()
                .find_map(|key| value.get(key).and_then(serde_json::Value::as_str))
                .map(str::to_string)
        })
        .unwrap_or_else(|| format!("HTTP {status}"));
    ProviderError::Other(format!(
        "OpenCode Go API error: {}",
        crate::logging::safe_error_message(&message)
    ))
}

fn parse_usage_text(text: &str, now: DateTime<Utc>) -> Result<UsageSnapshot, ProviderError> {
    let value: serde_json::Value = serde_json::from_str(text)
        .map_err(|e| ProviderError::Parse(format!("Invalid OpenCode Go API JSON: {e}")))?;
    let usage = value.get("usage").unwrap_or(&value);
    let rolling = api_window(usage, &["rolling", "rollingUsage", "rolling_usage"], now)
        .ok_or_else(|| ProviderError::Parse("Missing rolling usage window".to_string()))?;
    Ok(super::go_snapshot(
        rolling,
        api_window(usage, &["weekly", "weeklyUsage", "weekly_usage"], now),
        api_window(usage, &["monthly", "monthlyUsage", "monthly_usage"], now),
    ))
}

fn api_window(
    usage: &serde_json::Value,
    names: &[&str],
    now: DateTime<Utc>,
) -> Option<(f64, DateTime<Utc>)> {
    let object = names.iter().find_map(|name| usage.get(*name))?;
    let percent = [
        "percent",
        "usagePercent",
        "usedPercent",
        "percentUsed",
        "utilization",
    ]
    .into_iter()
    .find_map(|key| {
        object.get(key).and_then(|value| {
            value
                .as_f64()
                .or_else(|| value.as_str().and_then(|text| text.trim().parse().ok()))
        })
    })?;
    // GET /zen/go/v1/usage returns whole percentages in 0..=100 (including exact
    // 1% / 0.5%). Do not treat values in (0, 1] as 0..=1 fractions: that turns
    // a real 1% into a false 100% exhausted state (win-fork #247 / upstream #3216).
    let percent = percent.clamp(0.0, 100.0);
    let reset = [
        "resetInSec",
        "resetInSeconds",
        "resetSeconds",
        "reset_in_sec",
    ]
    .into_iter()
    .find_map(|key| {
        object.get(key).and_then(|value| {
            value
                .as_i64()
                .or_else(|| value.as_str().and_then(|text| text.trim().parse().ok()))
        })
    })
    .map(|seconds| now + chrono::Duration::seconds(seconds.max(0)))
    .or_else(|| {
        ["resetsAt", "resetAt", "resets_at", "reset_at"]
            .into_iter()
            .find_map(|key| object.get(key).and_then(serde_json::Value::as_str))
            .and_then(|text| chrono::DateTime::parse_from_rfc3339(text).ok())
            .map(|timestamp| timestamp.with_timezone(&Utc))
    })?;
    Some((percent, reset))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cli_login() -> Result<Option<String>, ProviderError> {
        Ok(Some("cli-token".to_string()))
    }

    #[test]
    fn configured_key_wins_over_the_cli_login() {
        let ctx = FetchContext::default();
        let credential = usage_credential_with(Some("configured".into()), &ctx, || {
            panic!("a configured key must not read the CLI login")
        })
        .unwrap();
        assert_eq!(credential, Some(("configured".to_string(), "api")));
    }

    #[test]
    fn unscoped_auto_and_api_modes_use_the_cli_login() {
        for source_mode in [
            crate::core::SourceMode::Auto,
            crate::core::SourceMode::OAuth,
        ] {
            let ctx = FetchContext {
                source_mode,
                ..FetchContext::default()
            };
            assert_eq!(
                usage_credential_with(None, &ctx, cli_login).unwrap(),
                Some(("cli-token".to_string(), CLI_SOURCE_LABEL))
            );
        }
    }

    #[test]
    fn web_mode_and_explicit_scopes_never_borrow_the_cli_login() {
        let contexts = [
            FetchContext {
                source_mode: crate::core::SourceMode::Web,
                ..FetchContext::default()
            },
            FetchContext {
                token_account_isolated: true,
                ..FetchContext::default()
            },
            FetchContext {
                auto_prefer_web: true,
                ..FetchContext::default()
            },
            FetchContext {
                manual_cookie_header: Some("auth=manual".into()),
                ..FetchContext::default()
            },
            FetchContext {
                workspace_id: Some("wrk_override".into()),
                ..FetchContext::default()
            },
        ];
        for ctx in contexts {
            let credential = usage_credential_with(None, &ctx, || {
                panic!("a scoped or Web fetch must not read the CLI login")
            })
            .unwrap();
            assert_eq!(credential, None);
        }
        let blank = FetchContext {
            manual_cookie_header: Some("  ".into()),
            workspace_id: Some(" ".into()),
            ..FetchContext::default()
        };
        assert!(!is_explicitly_scoped(&blank));
    }

    #[test]
    fn unreadable_cli_login_is_reported_to_the_caller() {
        let ctx = FetchContext::default();
        assert!(matches!(
            usage_credential_with(None, &ctx, || Err(ProviderError::AuthRequired)),
            Err(ProviderError::AuthRequired)
        ));
    }

    #[test]
    fn selected_account_key_does_not_consult_global_environment() {
        let ctx = FetchContext {
            token_account_isolated: true,
            token_account_kind: Some(crate::core::TokenAccountKind::ApiKey),
            api_key: Some(" selected-account-key ".into()),
            ..FetchContext::default()
        };
        assert_eq!(
            selected_account_api_key(&ctx).as_deref(),
            Some("selected-account-key")
        );
        let missing = FetchContext {
            api_key: None,
            ..ctx
        };
        assert_eq!(selected_account_api_key(&missing), None);
    }

    #[test]
    fn api_key_normalization_matches_upstream_settings_reader() {
        assert_eq!(
            normalized_api_key(Some("  go_test  ")).as_deref(),
            Some("go_test")
        );
        assert_eq!(
            normalized_api_key(Some("'go_quoted'")).as_deref(),
            Some("go_quoted")
        );
        assert_eq!(normalized_api_key(Some("   ")), None);
    }

    #[test]
    fn parses_public_usage_api_windows() {
        let now = DateTime::parse_from_rfc3339("2026-08-12T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let text = r#"{"usage":{"rolling":{"percent":12,"resetsAt":"2026-08-12T02:00:00.000Z"},"weekly":{"percent":8,"resetsAt":"2026-08-18T00:00:00.000Z"},"monthly":{"percent":35,"resetsAt":"2026-09-01T00:00:00.000Z"}}}"#;
        let snapshot = parse_usage_text(text, now).unwrap();
        assert!((snapshot.primary.used_percent - 12.0).abs() < 0.001);
        assert!((snapshot.secondary.as_ref().unwrap().used_percent - 8.0).abs() < 0.001);
        assert!((snapshot.tertiary.as_ref().unwrap().used_percent - 35.0).abs() < 0.001);
        assert_eq!(snapshot.login_method.as_deref(), Some("OpenCode Go"));
        let windows = [
            &snapshot.primary,
            snapshot.secondary.as_ref().unwrap(),
            snapshot.tertiary.as_ref().unwrap(),
        ];
        let shape: Vec<_> = windows
            .iter()
            .map(|w| (w.window_minutes, w.resets_at.map(|r| r.to_rfc3339())))
            .collect();
        assert_eq!(
            shape,
            [
                (Some(300), Some("2026-08-12T02:00:00+00:00".to_string())),
                (Some(10080), Some("2026-08-18T00:00:00+00:00".to_string())),
                // Aug 1 to Sep 1 is a 31-day calendar month.
                (Some(44640), Some("2026-09-01T00:00:00+00:00".to_string())),
            ]
        );
        assert!(windows.iter().all(|w| w.reset_description.is_none()));
        assert!(snapshot.extra_rate_windows.is_empty());
    }

    #[test]
    fn preserves_whole_percent_units_including_exact_one() {
        let now = DateTime::parse_from_rfc3339("2026-08-31T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        // Live Go API shape: percent is already 0..=100. Exact 1 must stay 1%.
        let text = r#"{"usage":{"rolling":{"status":"ok","percent":1,"resetsAt":"2026-08-31T17:12:09.085Z"},"weekly":{"status":"ok","percent":0,"resetsAt":"2026-09-07T00:00:00.085Z"},"monthly":{"status":"ok","percent":23,"resetsAt":"2026-09-20T04:39:33.085Z"}}}"#;
        let snapshot = parse_usage_text(text, now).unwrap();
        assert!(
            (snapshot.primary.used_percent - 1.0).abs() < 0.001,
            "rolling 1% must not be rescaled to 100%, got {}",
            snapshot.primary.used_percent
        );
        assert!((snapshot.secondary.as_ref().unwrap().used_percent - 0.0).abs() < 0.001);
        assert!((snapshot.tertiary.as_ref().unwrap().used_percent - 23.0).abs() < 0.001);
    }

    #[test]
    fn preserves_sub_one_percent_without_fraction_rescale() {
        let now = DateTime::parse_from_rfc3339("2026-08-31T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let text = r#"{"usage":{"rolling":{"percent":0.5,"resetsAt":"2026-08-31T17:00:00Z"},"weekly":{"percent":100,"resetsAt":"2026-09-07T00:00:00Z"}}}"#;
        let snapshot = parse_usage_text(text, now).unwrap();
        assert!(
            (snapshot.primary.used_percent - 0.5).abs() < 0.001,
            "0.5% must stay 0.5, got {}",
            snapshot.primary.used_percent
        );
        assert!((snapshot.secondary.as_ref().unwrap().used_percent - 100.0).abs() < 0.001);
    }

    #[test]
    fn api_error_redacts_secrets_echoed_in_the_response_body() {
        let status = reqwest::StatusCode::BAD_REQUEST;
        for (body, secret) in [
            (
                r#"{"message":"Authorization: Bearer live-bearer-9f3a rejected"}"#,
                "live-bearer-9f3a",
            ),
            (
                r#"{"error":"invalid key sk-live-apikey-4417xyz"}"#,
                "sk-live-apikey-4417xyz",
            ),
            (
                r#"{"detail":"cookie: session=cookie-secret-7d21; other=x"}"#,
                "cookie-secret-7d21",
            ),
            (
                r#"{"message":"access_token=oauth-token-b802 expired"}"#,
                "oauth-token-b802",
            ),
        ] {
            let text = api_error(status, body).to_string();
            assert!(!text.contains(secret), "{text}");
            assert!(text.contains("[REDACTED]"), "{text}");
            assert!(text.starts_with("OpenCode Go API error:"), "{text}");
        }
    }

    #[test]
    fn api_error_keeps_plain_server_messages_and_never_echoes_raw_bodies() {
        let status = reqwest::StatusCode::TOO_MANY_REQUESTS;
        assert_eq!(
            api_error(status, r#"{"message":"rate limit reached"}"#).to_string(),
            "OpenCode Go API error: rate limit reached"
        );
        assert_eq!(
            api_error(reqwest::StatusCode::BAD_GATEWAY, "Bearer raw-body-secret").to_string(),
            "OpenCode Go API error: HTTP 502 Bad Gateway"
        );
    }
}
