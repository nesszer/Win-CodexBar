use super::credentials::parse_timestamp;
use super::parse::{normalize_array_windows, normalize_named_windows};
use super::reset_credits::{
    RESET_CREDITS_CACHE_TTL, RESET_CREDITS_PATH, decode_reset_credits, reset_credits_rate_window,
};
use super::*;
use crate::core::RateWindow;
use crate::providers::test_support::{mock_response, mock_response_expect, mock_status_expect};
use base64::Engine;
use chrono::DateTime;
use serde_json::json;
use std::time::Duration;

#[test]
fn non_chatgpt_model_provider_is_detected_for_guidance() {
    // Upstream 0.50.0 #2679: Bedrock and other custom backends get
    // rate-limit guidance instead of login instructions.
    assert!(config_uses_non_chatgpt_provider(
        "model_provider = \"bedrock\"\n"
    ));
    assert!(config_uses_non_chatgpt_provider(
        "# relay\nmodel_provider = 'ollama'"
    ));
    assert!(!config_uses_non_chatgpt_provider(
        "model_provider = \"openai\""
    ));
    assert!(!config_uses_non_chatgpt_provider(
        "model = \"gpt-5\"\napproval_policy = \"never\""
    ));
}

#[test]
fn parses_codex_credentials_without_retaining_refresh_token() {
    let credentials = CodexApi::parse_credentials_json(
        r#"{
                "tokens": {
                    "access_token": "access",
                    "refresh_token": "refresh",
                    "account_id": "acct_123"
                }
            }"#,
    )
    .expect("credentials");

    assert_eq!(credentials.access_token, "access");
    assert_eq!(credentials.account_id.as_deref(), Some("acct_123"));
}

#[test]
fn decodes_reset_credits() {
    let credits = decode_reset_credits(
        br#"{"available_count":2,"credits":[{"id":"a","status":"available","expires_at":"2026-08-01T12:00:00Z"}]}"#,
    )
    .expect("reset credits");
    assert_eq!(credits.available_count, 2);
    assert_eq!(credits.credits.len(), 1);
    assert_eq!(credits.credits[0].status.as_deref(), Some("available"));
    assert_eq!(
        credits.credits[0].expires_at.as_deref(),
        Some("2026-08-01T12:00:00Z")
    );
}

#[test]
fn missing_reset_credit_count_is_unavailable_not_zero() {
    assert!(decode_reset_credits(br#"{"credits":[]}"#).is_err());
}

#[test]
fn next_expiry_picks_soonest_available() {
    let now = utc("2026-07-01T00:00:00Z");
    let credits = vec![
        credit(Some("available"), "2026-07-10T00:00:00Z"),
        credit(Some("available"), "2026-07-05T00:00:00Z"),
        credit(Some("available"), "2026-07-20T00:00:00Z"),
    ];
    let expiry = next_available_reset_credit_expiry(&credits, now).expect("expiry");
    assert_eq!(expiry, utc("2026-07-05T00:00:00Z"));
}

#[test]
fn next_expiry_skips_past_and_non_available() {
    let now = utc("2026-07-01T00:00:00Z");
    let credits = vec![
        credit(Some("available"), "2026-06-01T00:00:00Z"),
        credit(Some("used"), "2026-07-03T00:00:00Z"),
        credit(Some("AVAILABLE"), "2026-07-08T00:00:00Z"),
        credit(None, "2026-07-09T00:00:00Z"),
    ];
    let expiry = next_available_reset_credit_expiry(&credits, now).expect("expiry");
    assert_eq!(expiry, utc("2026-07-08T00:00:00Z"));
}

#[test]
fn reset_credits_window_sets_informational_and_expiry() {
    let now = utc("2026-07-01T00:00:00Z");
    let reset = ResetCredits {
        available_count: 2,
        credits: vec![
            credit(Some("available"), "2026-07-15T12:00:00Z"),
            credit(Some("available"), "2026-07-10T12:00:00Z"),
        ],
    };
    let window = reset_credits_rate_window(&reset, now);
    assert!(window.is_informational);
    assert_eq!(
        window.reset_description.as_deref(),
        Some("2 reset credits available")
    );
    assert_eq!(window.resets_at, Some(utc("2026-07-10T12:00:00Z")));
}

#[test]
fn reset_credits_window_count_only_without_expiry() {
    let now = utc("2026-07-01T00:00:00Z");
    let reset = ResetCredits {
        available_count: 1,
        credits: vec![],
    };
    let window = reset_credits_rate_window(&reset, now);
    assert!(window.is_informational);
    assert_eq!(
        window.reset_description.as_deref(),
        Some("1 reset credit available")
    );
    assert!(window.resets_at.is_none());
}

const PLUS_USAGE: &str = r#"{"plan_type":"plus","rate_limit":{"primary_window":{"used_percent":10,"limit_window_seconds":18000}}}"#;

fn utc(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value)
        .unwrap()
        .with_timezone(&Utc)
}

fn credit(status: Option<&str>, expires_at: &str) -> ResetCredit {
    ResetCredit {
        id: None,
        reset_type: None,
        status: status.map(str::to_string),
        expires_at: Some(expires_at.to_string()),
    }
}

/// An unsigned JWT whose payload carries only `exp` (seconds since the epoch).
fn jwt_with_exp(exp: i64) -> String {
    let payload =
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(format!(r#"{{"exp":{exp}}}"#));
    format!("header.{payload}.signature")
}

fn external_creds(access_token: &str, last_refresh: Option<DateTime<Utc>>) -> CodexCredentials {
    CodexCredentials {
        access_token: access_token.to_string(),
        account_id: None,
        is_external_oauth: true,
        access_token_expires_at: None,
        last_refresh,
    }
}

async fn mock_reset_credits(
    server: &mut mockito::ServerGuard,
    available_count: u32,
) -> mockito::Mock {
    mock_response_expect(
        server,
        "GET",
        RESET_CREDITS_PATH,
        200,
        format!(r#"{{"available_count":{available_count},"credits":[]}}"#),
        1,
    )
    .await
}

fn write_codex_home(base_url: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temp codex home");
    std::fs::write(
        dir.path().join("auth.json"),
        r#"{"tokens":{"access_token":"test-token","account_id":"acct_test"}}"#,
    )
    .expect("auth.json");
    std::fs::write(
        dir.path().join("config.toml"),
        format!("chatgpt_base_url = \"{base_url}\""),
    )
    .expect("config.toml");
    dir
}

#[tokio::test]
async fn reset_credit_cache_single_flight_and_unknown_are_ten_minute_observations() {
    let mut server = mockito::Server::new_async().await;
    let request = mock_status_expect(&mut server, "GET", RESET_CREDITS_PATH, 503, 1).await;
    let home = write_codex_home(&server.url());
    let api = CodexApi::new().with_codex_home(home.path());
    let creds = api.load_credentials().await.unwrap();
    let base = server.url();
    let (first, second) = tokio::join!(
        api.fetch_rate_limit_reset_credits_cached(&creds, &base),
        api.fetch_rate_limit_reset_credits_cached(&creds, &base),
    );
    assert!(first.is_none() && second.is_none());
    assert!(
        api.fetch_rate_limit_reset_credits_cached(&creds, &base)
            .await
            .is_none()
    );
    request.assert_async().await;
    assert!(RESET_CREDITS_CACHE_TTL == Duration::from_secs(600));
}

#[tokio::test]
async fn reset_credit_cache_expires_and_token_rotation_uses_new_scope() {
    let mut server = mockito::Server::new_async().await;
    let first = server
        .mock("GET", "/wham/rate-limit-reset-credits")
        .match_header("authorization", "Bearer test-token")
        .expect(1)
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"available_count":2,"credits":[]}"#)
        .create_async()
        .await;
    let home = write_codex_home(&server.url());
    let api = CodexApi::new().with_codex_home(home.path());
    let mut creds = api.load_credentials().await.unwrap();
    let base = server.url();
    assert_eq!(
        api.fetch_rate_limit_reset_credits_cached(&creds, &base)
            .await
            .unwrap()
            .available_count,
        2
    );
    assert_eq!(
        api.fetch_rate_limit_reset_credits_cached(&creds, &base)
            .await
            .unwrap()
            .available_count,
        2
    );
    first.assert_async().await;
    first.remove_async().await;
    let second = server
        .mock("GET", "/wham/rate-limit-reset-credits")
        .match_header("authorization", "Bearer test-token")
        .expect(1)
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"available_count":1,"credits":[]}"#)
        .create_async()
        .await;
    api.reset_credits_cache_slot(&creds, &base)
        .lock()
        .await
        .loaded_at = Some(Instant::now() - RESET_CREDITS_CACHE_TTL);
    assert_eq!(
        api.fetch_rate_limit_reset_credits_cached(&creds, &base)
            .await
            .unwrap()
            .available_count,
        1
    );
    second.assert_async().await;
    second.remove_async().await;
    creds.access_token = "rotated-token".into();
    let rotated = server
        .mock("GET", "/wham/rate-limit-reset-credits")
        .match_header("authorization", "Bearer rotated-token")
        .expect(1)
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"available_count":3,"credits":[]}"#)
        .create_async()
        .await;
    assert_eq!(
        api.fetch_rate_limit_reset_credits_cached(&creds, &base)
            .await
            .unwrap()
            .available_count,
        3
    );
    rotated.assert_async().await;
}

#[tokio::test]
async fn reset_credit_cache_is_scoped_to_the_codex_home() {
    // Same base URL, account and token, different Codex homes: each home
    // makes its own observation instead of reading the other's.
    let mut server = mockito::Server::new_async().await;
    let base = server.url();
    let first = mock_reset_credits(&mut server, 2).await;
    let first_home = write_codex_home(&base);
    let first_api = CodexApi::new().with_codex_home(first_home.path());
    let first_creds = first_api.load_credentials().await.unwrap();
    let first_count = first_api
        .fetch_rate_limit_reset_credits_cached(&first_creds, &base)
        .await
        .map(|credits| credits.available_count);
    assert_eq!(first_count, Some(2));
    first.assert_async().await;
    first.remove_async().await;

    let second = mock_status_expect(&mut server, "GET", RESET_CREDITS_PATH, 503, 1).await;
    let second_home = write_codex_home(&base);
    let second_api = CodexApi::new().with_codex_home(second_home.path());
    let second_creds = second_api.load_credentials().await.unwrap();
    assert_eq!(first_creds.access_token, second_creds.access_token);
    assert_eq!(first_creds.account_id, second_creds.account_id);
    assert!(
        second_api
            .fetch_rate_limit_reset_credits_cached(&second_creds, &base)
            .await
            .is_none()
    );
    second.assert_async().await;
}

#[tokio::test]
async fn suspicious_weekly_reset_uses_independent_credit_observations() {
    let mut server = mockito::Server::new_async().await;
    let cached_response = mock_reset_credits(&mut server, 2).await;
    let home = write_codex_home(&server.url());
    let api = CodexApi::new().with_codex_home(home.path());
    let creds = api.load_credentials().await.unwrap();
    let base = server.url();
    api.fetch_rate_limit_reset_credits_cached(&creds, &base)
        .await
        .unwrap();
    cached_response.assert_async().await;
    cached_response.remove_async().await;

    let started = Instant::now();
    let initial_response = mock_reset_credits(&mut server, 1).await;
    let initial = api
        .fresh_reset_credits_for_confirmation(&creds, &base, started)
        .await
        .unwrap();
    assert_eq!(initial.available_count, 1);
    initial_response.assert_async().await;
    initial_response.remove_async().await;

    let confirmation_response = mock_reset_credits(&mut server, 0).await;
    let confirmation = api
        .fetch_rate_limit_reset_credits_fresh(&creds, &base)
        .await
        .unwrap();
    assert_eq!(confirmation.available_count, 0);
    confirmation_response.assert_async().await;
}

#[tokio::test]
async fn pending_delayed_candidate_revalidates_with_a_fresh_credit_observation() {
    let mut server = mockito::Server::new_async().await;
    let candidate_observation = mock_reset_credits(&mut server, 2).await;
    let home = write_codex_home(&server.url());
    let api = CodexApi::new().with_codex_home(home.path());
    let creds = api.load_credentials().await.unwrap();
    let base = server.url();
    let cached = api
        .fetch_rate_limit_reset_credits_cached(&creds, &base)
        .await;
    candidate_observation.assert_async().await;
    candidate_observation.remove_async().await;

    // A later refresh: the ten-minute cache still holds the observation
    // that created the candidate.
    let started = Instant::now();
    let changed = mock_reset_credits(&mut server, 1).await;
    let without_candidate = api
        .initial_reset_credits(&creds, &base, started, cached.clone(), false)
        .await;
    assert_eq!(
        without_candidate.map(|credits| credits.available_count),
        Some(2)
    );
    let with_candidate = api
        .initial_reset_credits(&creds, &base, started, cached, true)
        .await;
    assert_eq!(
        with_candidate.map(|credits| credits.available_count),
        Some(1)
    );
    changed.assert_async().await;
}

#[tokio::test]
async fn fetch_usage_attaches_reset_credits_from_http() {
    let mut server = mockito::Server::new_async().await;
    let soonest = (Utc::now() + chrono::Duration::days(5)).to_rfc3339();
    let later = (Utc::now() + chrono::Duration::days(12)).to_rfc3339();

    let usage_mock = server
        .mock("GET", "/wham/usage")
        .match_header("authorization", "Bearer test-token")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(PLUS_USAGE)
        .create_async()
        .await;

    let reset_body = format!(
        r#"{{"available_count":2,"credits":[
                {{"status":"available","expires_at":"{later}"}},
                {{"status":"available","expires_at":"{soonest}"}}
            ]}}"#
    );
    let reset_mock = server
        .mock("GET", "/wham/rate-limit-reset-credits")
        .match_header("authorization", "Bearer test-token")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(reset_body)
        .create_async()
        .await;

    let home = write_codex_home(&server.url());
    let api = CodexApi::new().with_codex_home(home.path());
    let (usage, _, _) = api.fetch_usage().await.expect("fetch_usage");

    usage_mock.assert_async().await;
    reset_mock.assert_async().await;

    let extra = usage
        .extra_rate_windows
        .iter()
        .find(|w| w.id == "reset-credits")
        .expect("reset-credits window attached");
    assert_eq!(extra.title, "Reset credits");
    assert!(extra.window.is_informational);
    assert_eq!(
        extra.window.reset_description.as_deref(),
        Some("2 reset credits available")
    );
    let expected = utc(&soonest);
    assert_eq!(extra.window.resets_at, Some(expected));
}

#[tokio::test]
async fn authenticated_codex_http_distinguishes_401_from_403() {
    for (status, expects_authentication) in [(401, true), (403, false)] {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/wham/usage")
            .with_status(status)
            .with_body("fixture refusal")
            .create_async()
            .await;

        let home = write_codex_home(&server.url());
        let api = CodexApi::new().with_codex_home(home.path());
        let error = match api.fetch_usage().await {
            Ok(_) => panic!("expected HTTP {status} to fail"),
            Err(error) => error,
        };

        if expects_authentication {
            assert!(matches!(error, ProviderError::AuthRequired));
        } else {
            let message = error.to_string();
            assert!(message.contains("403"));
            assert!(message.contains("fixture refusal"));
            assert!(!matches!(error, ProviderError::AuthRequired));
        }
        mock.assert_async().await;
    }
}

#[tokio::test]
async fn old_opaque_external_oauth_reaches_usage_request() {
    let mut server = mockito::Server::new_async().await;
    let usage_mock = server
        .mock("GET", "/wham/usage")
        .match_header("authorization", "Bearer opaque-token")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(PLUS_USAGE)
        .create_async()
        .await;
    let reset_mock = server
        .mock("GET", "/wham/rate-limit-reset-credits")
        .match_header("authorization", "Bearer opaque-token")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"available_count":0,"credits":[]}"#)
        .create_async()
        .await;

    let creds = CodexApi::parse_credentials_json(
        r#"{
                "tokens": {
                    "access_token": "opaque-token",
                    "refresh_token": "refresh",
                    "account_id": "acct_test"
                },
                "last_refresh": "2026-01-01T00:00:00Z"
            }"#,
    )
    .expect("credentials");
    assert!(CodexApi::enforce_external_oauth_gate_at(&creds, false, Utc::now()).is_ok());

    let api = CodexApi::new();
    let (usage, _, _) = api
        .fetch_usage_once(&creds, &server.url())
        .await
        .expect("opaque OAuth usage request");
    assert_eq!(usage.primary.used_percent, 10.0);
    usage_mock.assert_async().await;
    reset_mock.assert_async().await;
}

#[tokio::test]
async fn fetch_usage_skips_reset_credits_when_available_count_zero() {
    let mut server = mockito::Server::new_async().await;

    let usage_mock = mock_response(&mut server, "GET", USAGE_PATH, 200, PLUS_USAGE).await;

    let reset_mock = mock_reset_credits(&mut server, 0).await;

    let home = write_codex_home(&server.url());
    let api = CodexApi::new().with_codex_home(home.path());
    let (usage, _, _) = api.fetch_usage().await.expect("fetch_usage");

    usage_mock.assert_async().await;
    reset_mock.assert_async().await;

    assert!(
        usage
            .extra_rate_windows
            .iter()
            .all(|w| w.id != "reset-credits"),
        "available_count=0 must not attach reset-credits"
    );
}

#[test]
fn keeps_weekly_window_in_secondary_when_session_is_absent() {
    let api = CodexApi::new();
    let (usage, _) = api
        .build_result_from_json(&json!({
            "rate_limit": {
                "secondary_window": {
                    "used_percent": 25,
                    "limit_window_seconds": 604800,
                    "reset_at": 1783036800
                }
            }
        }))
        .expect("codex usage");

    assert!(usage.primary.is_informational);
    assert_eq!(usage.primary.window_minutes, Some(300));
    assert_eq!(
        usage.primary.reset_description.as_deref(),
        Some("No active 5h session")
    );

    let weekly = usage.secondary.expect("weekly window");
    assert!(!weekly.is_informational);
    assert_eq!(weekly.used_percent, 25.0);
    assert_eq!(weekly.window_minutes, Some(10080));
}

#[test]
fn identifies_rate_limit_array_windows_by_duration() {
    let api = CodexApi::new();
    let (usage, _) = api
        .build_result_from_json(&json!({
            "rate_limits": [
                {
                    "used_percent": 25,
                    "limit_window_seconds": 604800,
                    "reset_at": 1783036800
                },
                {
                    "used_percent": 10,
                    "limit_window_seconds": 18000,
                    "reset_at": 1783018800
                }
            ]
        }))
        .expect("codex usage");

    assert!(!usage.primary.is_informational);
    assert_eq!(usage.primary.used_percent, 10.0);
    assert_eq!(usage.primary.window_minutes, Some(300));

    let weekly = usage.secondary.expect("weekly window");
    assert_eq!(weekly.used_percent, 25.0);
    assert_eq!(weekly.window_minutes, Some(10080));
}

#[test]
fn identifies_weekly_only_rate_limit_array_without_a_session() {
    let api = CodexApi::new();
    let (usage, _) = api
        .build_result_from_json(&json!({
            "rate_limits": [{
                "used_percent": 25,
                "limit_window_seconds": 604800,
                "reset_at": 1783036800
            }]
        }))
        .expect("codex usage");

    assert!(usage.primary.is_informational);
    assert_eq!(usage.secondary.expect("weekly window").used_percent, 25.0);
}

#[test]
fn maps_codex_spark_additional_rate_limits() {
    let api = CodexApi::new();
    let (usage, _) = api
        .build_result_from_json(&json!({
            "plan_type": "pro",
            "rate_limit": {
                "primary_window": { "used_percent": 20, "limit_window_seconds": 18000 },
                "secondary_window": { "used_percent": 40, "limit_window_seconds": 604800 }
            },
            "additional_rate_limits": [
                {
                    "limit_name": "Codex Spark",
                    "metered_feature": "codex_spark",
                    "rate_limit": {
                        "primary_window": { "used_percent": "17", "limit_window_seconds": 18000 }
                    }
                },
                {
                    "limit_name": "Codex Spark Weekly",
                    "metered_feature": "codex_spark",
                    "rate_limit": {
                        "secondary_window": { "used_percent": 62, "limit_window_seconds": 604800 }
                    }
                }
            ]
        }))
        .expect("codex usage");

    assert_eq!(usage.extra_rate_windows.len(), 2);
    assert_eq!(usage.extra_rate_windows[0].id, "codex-spark");
    assert_eq!(usage.extra_rate_windows[0].title, "Codex Spark 5-hour");
    assert_eq!(usage.extra_rate_windows[0].window.used_percent, 17.0);
    assert_eq!(usage.extra_rate_windows[1].id, "codex-spark-weekly");
    assert_eq!(usage.extra_rate_windows[1].title, "Codex Spark Weekly");
    assert_eq!(usage.extra_rate_windows[1].window.used_percent, 62.0);
}

#[test]
fn ignores_placeholder_additional_rate_limits() {
    let api = CodexApi::new();
    let (usage, _) = api
        .build_result_from_json(&json!({
            "rate_limit": {
                "primary_window": { "used_percent": 0, "limit_window_seconds": 18000 }
            },
            "additional_rate_limits": [
                {
                    "limit_name": "placeholder",
                    "metered_feature": "placeholder",
                    "rate_limit": { "primary_window": {} }
                }
            ]
        }))
        .expect("codex usage");

    assert!(usage.extra_rate_windows.is_empty());
}

fn win(minutes: u32, used: f64) -> RateWindow {
    RateWindow::with_details(used, Some(minutes), None, None)
}

#[test]
fn normalize_named_windows_routes_by_role() {
    // Primary is reported as -1.0 for the "No active 5h session" placeholder.
    let s = |used| win(300, used);
    let w = |used| win(10_080, used);
    let u = |used| win(999, used);
    let m = |used| win(43_200, used);
    type Row = (Option<RateWindow>, Option<RateWindow>, f64, Option<f64>);
    let rows: Vec<Row> = vec![
        (None, None, -1.0, None),
        (Some(w(1.0)), None, -1.0, Some(1.0)),
        (Some(s(1.0)), None, 1.0, None),
        (Some(u(1.0)), None, 1.0, None),
        (Some(m(1.0)), None, 1.0, None),
        (None, Some(w(2.0)), -1.0, Some(2.0)),
        (None, Some(s(2.0)), 2.0, None),
        (None, Some(u(2.0)), 2.0, None),
        (Some(w(1.0)), Some(s(2.0)), 2.0, Some(1.0)),
        (Some(w(1.0)), Some(u(2.0)), -1.0, Some(1.0)),
        (Some(u(1.0)), Some(s(2.0)), 2.0, Some(1.0)),
        (Some(s(1.0)), Some(w(2.0)), 1.0, Some(2.0)),
        (Some(u(1.0)), Some(w(2.0)), 1.0, Some(2.0)),
        (Some(s(1.0)), Some(s(2.0)), 1.0, Some(2.0)),
        (Some(w(1.0)), Some(w(2.0)), 1.0, Some(2.0)),
        (Some(u(1.0)), Some(u(2.0)), 1.0, Some(2.0)),
        (Some(m(1.0)), Some(w(2.0)), 1.0, Some(2.0)),
    ];
    for (index, (primary, secondary, want_primary, want_secondary)) in rows.into_iter().enumerate()
    {
        let (got_primary, got_secondary) = normalize_named_windows(primary, secondary);
        let got_primary = if got_primary.is_informational {
            -1.0
        } else {
            got_primary.used_percent
        };
        assert_eq!(got_primary, want_primary, "row {index} primary");
        assert_eq!(
            got_secondary.map(|window| window.used_percent),
            want_secondary,
            "row {index} secondary"
        );
    }
}

#[tokio::test]
async fn authed_get_sends_account_header_only_when_non_empty() {
    let mut server = mockito::Server::new_async().await;
    let with_account = server
        .mock("GET", "/with")
        .match_header("authorization", "Bearer tok")
        .match_header("user-agent", "CodexBar")
        .match_header("accept", "application/json")
        .match_header("chatgpt-account-id", "acct-1")
        .with_status(200)
        .create_async()
        .await;
    let without_account = server
        .mock("GET", "/without")
        .match_header("authorization", "Bearer tok")
        .match_header("chatgpt-account-id", mockito::Matcher::Missing)
        .expect(2)
        .with_status(200)
        .create_async()
        .await;
    let api = CodexApi::new();
    for (path, account_id) in [
        ("/with", Some("acct-1")),
        ("/without", Some("")),
        ("/without", None),
    ] {
        let status = api
            .authed_get(&format!("{}{path}", server.url()), "tok", account_id)
            .send()
            .await
            .expect("send")
            .status();
        assert_eq!(status.as_u16(), 200, "{path} {account_id:?}");
    }
    with_account.assert_async().await;
    without_account.assert_async().await;
}

#[test]
fn f5_normalize_array_routes_session_weekly_monthly_to_lanes() {
    // 5h session + weekly + monthly → (session, weekly, monthly, None)
    let windows = vec![win(300, 10.0), win(10_080, 20.0), win(43_200, 30.0)];
    let (primary, secondary, tertiary, code_review) = normalize_array_windows(windows);
    assert_eq!(primary.window_minutes, Some(300));
    assert_eq!(secondary.unwrap().window_minutes, Some(10_080));
    assert_eq!(tertiary.unwrap().window_minutes, Some(43_200));
    assert!(code_review.is_none());
}

#[test]
fn f5_normalize_array_monthly_routes_to_tertiary_not_secondary() {
    // Monthly must go to tertiary, NOT secondary — so #268's weekly math
    // and "Weekly" label stay untouched.
    let windows = vec![win(43_200, 50.0), win(10_080, 20.0)];
    let (primary, secondary, tertiary, _) = normalize_array_windows(windows);
    assert_eq!(primary.window_minutes, Some(300)); // no session → placeholder
    assert_eq!(secondary.unwrap().window_minutes, Some(10_080));
    assert_eq!(tertiary.unwrap().window_minutes, Some(43_200));
}

#[test]
fn f5_normalize_array_empty_returns_placeholder_primary() {
    let (primary, secondary, tertiary, code_review) = normalize_array_windows(vec![]);
    assert!(primary.is_informational);
    assert!(secondary.is_none());
    assert!(tertiary.is_none());
    assert!(code_review.is_none());
}

#[test]
fn f5_normalize_array_unknown_windows_fall_to_code_review() {
    // Windows with unrecognized durations (not 300/10080/43200) go to the
    // remaining/code_review bucket.
    let windows = vec![win(300, 10.0), win(999, 5.0)];
    let (primary, secondary, tertiary, code_review) = normalize_array_windows(windows);
    assert_eq!(primary.window_minutes, Some(300));
    assert!(secondary.is_none());
    assert!(tertiary.is_none());
    assert_eq!(code_review.unwrap().window_minutes, Some(999));
}

// ── Upstream 0.50.1 #2944: external OAuth source gate ──────────────────

#[test]
fn confirmation_failure_fallback_keeps_first_successful_usage_and_cost() {
    let state = weekly_reset::AccountState::default();
    let first = UsageSnapshot::new(RateWindow::new(10.0)).with_secondary(RateWindow::new(0.5));
    let cost = Some(CostSnapshot::new(3.25, "USD", "Monthly"));
    let (usage, kept_cost) = (weekly_reset::preserve_weekly(&state, first), cost);
    assert!((usage.secondary.expect("weekly").used_percent - 0.5).abs() < f64::EPSILON);
    assert_eq!(kept_cost.expect("cost").used, 3.25);
}
#[test]
fn api_key_credentials_are_not_external_oauth() {
    let creds =
        CodexApi::parse_credentials_json(r#"{"OPENAI_API_KEY": "sk-test"}"#).expect("credentials");
    assert!(!creds.is_external_oauth);
    assert!(creds.access_token_expires_at.is_none());
    assert!(creds.last_refresh.is_none());
    assert!(CodexApi::enforce_external_oauth_gate(&creds).is_ok());
}

#[test]
fn oauth_tokens_with_refresh_token_are_external_source() {
    let creds = CodexApi::parse_credentials_json(
        r#"{
                "tokens": {
                    "access_token": "access",
                    "refresh_token": "refresh",
                    "account_id": "acct_123"
                }
            }"#,
    )
    .expect("credentials");
    assert!(creds.is_external_oauth);
    assert!(creds.access_token_expires_at.is_none());
    assert!(creds.last_refresh.is_none());
}

#[test]
fn oauth_tokens_without_refresh_token_are_not_external() {
    let creds = CodexApi::parse_credentials_json(
        r#"{
                "tokens": {
                    "access_token": "access",
                    "account_id": "acct_123"
                }
            }"#,
    )
    .expect("credentials");
    assert!(!creds.is_external_oauth);
}

#[test]
fn external_oauth_gate_fails_closed_without_last_refresh() {
    let creds = external_creds("access", None);
    let err = CodexApi::enforce_external_oauth_gate(&creds)
        .expect_err("external OAuth without provenance must fail closed");
    assert!(matches!(err, ProviderError::AuthRequired));
}

#[test]
fn external_oauth_gate_ignores_old_last_refresh_for_opaque_token() {
    let old = Utc::now() - chrono::Duration::days(10);
    let creds = external_creds("access", Some(old));
    assert!(CodexApi::enforce_external_oauth_gate(&creds).is_ok());
}

#[test]
fn external_oauth_gate_allows_refresh_provenance() {
    let fresh = Utc::now() - chrono::Duration::hours(1);
    let creds = external_creds("access", Some(fresh));
    assert!(CodexApi::enforce_external_oauth_gate(&creds).is_ok());
}

#[test]
fn external_oauth_gate_uses_future_jwt_expiry_over_old_last_refresh() {
    let now = Utc::now();
    let future = now + chrono::Duration::hours(2);
    let token = jwt_with_exp(future.timestamp());
    let json = format!(
        r#"{{"tokens":{{"access_token":"{token}","refresh_token":"refresh"}},"last_refresh":"2026-01-01T00:00:00Z"}}"#
    );
    let creds = CodexApi::parse_credentials_json(&json).expect("credentials");
    assert!(creds.access_token_expires_at.is_some());
    assert!(CodexApi::enforce_external_oauth_gate_at(&creds, false, now).is_ok());
    assert!(CodexApi::enforce_external_oauth_gate_at(&creds, true, now).is_ok());
}

#[test]
fn external_oauth_gate_rejects_expired_jwt() {
    let expired = Utc::now() - chrono::Duration::minutes(1);
    let token = jwt_with_exp(expired.timestamp());
    let json = format!(
        r#"{{"tokens":{{"access_token":"{token}","refresh_token":"refresh"}},"last_refresh":"{}"}}"#,
        Utc::now().to_rfc3339()
    );
    let creds = CodexApi::parse_credentials_json(&json).expect("credentials");
    let err = CodexApi::enforce_external_oauth_gate(&creds)
        .expect_err("expired native OAuth must be rejected");
    assert!(matches!(err, ProviderError::AuthRequired));
}

#[test]
fn external_oauth_gate_requires_cli_refresh_when_jwt_is_near_expiry() {
    let soon = Utc::now() + chrono::Duration::minutes(2);
    let token = jwt_with_exp(soon.timestamp());
    let json = format!(
        r#"{{"tokens":{{"access_token":"{token}","refresh_token":"refresh"}},"last_refresh":"{}"}}"#,
        Utc::now().to_rfc3339()
    );
    let creds = CodexApi::parse_credentials_json(&json).expect("credentials");
    let err = CodexApi::enforce_external_oauth_gate(&creds)
        .expect_err("near-expiry native OAuth must refresh through the CLI");
    assert!(matches!(err, ProviderError::AuthRequired));
}

#[test]
fn external_oauth_gate_allows_missing_last_refresh_when_opted_in() {
    let creds = external_creds("opaque-token", None);
    assert!(CodexApi::enforce_external_oauth_gate_at(&creds, true, Utc::now()).is_ok());
}

#[test]
fn opaque_token_uses_refresh_provenance_when_no_jwt_expiry_exists() {
    let fresh = Utc::now().to_rfc3339();
    let json = format!(
        r#"{{"tokens":{{"access_token":"opaque-token","refresh_token":"refresh"}},"last_refresh":"{fresh}"}}"#
    );
    let creds = CodexApi::parse_credentials_json(&json).expect("credentials");
    assert!(creds.access_token_expires_at.is_none());
    assert!(CodexApi::enforce_external_oauth_gate(&creds).is_ok());
}
#[test]
fn parse_timestamp_reads_iso8601() {
    assert!(parse_timestamp("2026-08-17T10:00:00Z").is_some());
    assert!(parse_timestamp("2026-08-17T10:00:00.123Z").is_some());
    assert!(parse_timestamp("  2026-08-17T10:00:00Z  ").is_some());
    assert!(parse_timestamp("").is_none());
    assert!(parse_timestamp("not-a-date").is_none());
}
