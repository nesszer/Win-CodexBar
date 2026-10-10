use super::*;
use crate::providers::test_support::mock_response;
use chrono::TimeZone;

/// https://docs.xkiro.com/api/usage/ pay-as-you-go example, synthetic identity
/// (upstream `XKiroPluginTests.fixture`).
const FIXTURE: &str = r#"{"object":"usage","plan":null,"user":{"email":"dev@example.com"},"windows":[],
 "free_tokens":{"used_today":124035,"limit_per_day":5000000,"remaining":4875965},
 "wallet":{"balance_usd":"4.812300","held_usd":"0.150000"}}"#;
const API_KEY: &str = "fixture-key";

fn now() -> DateTime<Utc> {
    Utc.timestamp_opt(1_790_251_200, 0).unwrap()
}

fn result_for(body: &str) -> Result<ProviderFetchResult, ProviderError> {
    parse_usage(body.as_bytes()).map(|usage| result_from_usage(&usage, now()))
}

fn free_tokens(fields: &str) -> String {
    format!(r#"{{"object":"usage","free_tokens":{{{fields}}}}}"#)
}

fn detail_values(result: &ProviderFetchResult) -> Vec<&str> {
    result
        .display_details()
        .iter()
        .map(ProviderDisplayDetail::value)
        .collect()
}

fn assert_parse_failure(body: &str) {
    match result_for(body) {
        Err(ProviderError::Parse(message)) => {
            assert!(!message.contains("private-response"), "{message}");
        }
        other => panic!("expected a parse failure for {body}, got {other:?}"),
    }
}

#[test]
fn documented_free_token_quota_stays_separate_from_paid_balance() {
    let result = result_for(FIXTURE).unwrap();
    let usage = &result.usage;
    assert!((usage.primary.used_percent - 2.4807).abs() < 0.00001);
    assert_eq!(usage.primary.window_minutes, Some(1440));
    assert_eq!(
        usage.primary.resets_at,
        Some(Utc.timestamp_opt(1_790_294_400, 0).unwrap())
    );
    assert!(usage.secondary.is_none());
    assert!(result.cost.is_none());
    assert_eq!(usage.account_email.as_deref(), Some("dev@example.com"));
    assert_eq!(usage.login_method.as_deref(), Some("Pay as you go"));
    assert_eq!(result.source_label, "api");
    assert_eq!(
        detail_values(&result),
        ["124,035", "5,000,000", "4,875,965", "00:00 UTC"]
    );
}

#[test]
fn named_plan_replaces_pay_as_you_go_and_is_trimmed() {
    let body = r#"{"object":"usage","plan":"  Pro  ","user":{"email":"  "},
        "free_tokens":{"used_today":1,"limit_per_day":4,"remaining":3}}"#;
    let usage = result_for(body).unwrap().usage;
    assert_eq!(usage.login_method.as_deref(), Some("Pro"));
    assert!(usage.account_email.is_none());
    assert_eq!(usage.primary.used_percent, 25.0);
}

#[test]
fn reset_is_the_next_midnight_utc_from_the_supplied_clock() {
    let before_midnight = Utc.timestamp_opt(1_790_294_399, 0).unwrap();
    let at_midnight = Utc.timestamp_opt(1_790_294_400, 0).unwrap();
    assert_eq!(next_utc_midnight(before_midnight), at_midnight);
    assert_eq!(
        next_utc_midnight(at_midnight),
        Utc.timestamp_opt(1_790_380_800, 0).unwrap()
    );
}

#[test]
fn missing_counters_and_uncapped_accounts_never_invent_headroom() {
    for (fields, expected) in [
        (
            r#""used_today":0,"limit_per_day":null,"remaining":null"#,
            vec!["0", "No cap reported", "00:00 UTC"],
        ),
        (r#""remaining":12"#, vec!["12", "00:00 UTC"]),
        (r#""limit_per_day":500000"#, vec!["500,000", "00:00 UTC"]),
    ] {
        let result = result_for(&free_tokens(fields)).unwrap();
        assert!(
            result.usage.primary.is_informational,
            "no percentage without both used and limit: {fields}"
        );
        assert!(result.usage.login_method.is_none(), "{fields}");
        assert_eq!(detail_values(&result), expected, "{fields}");
    }
}

#[test]
fn exhaustion_and_zero_allowance_do_not_use_wallet_capacity() {
    for limit in [0, 500_000] {
        let body = format!(
            r#"{{"object":"usage","free_tokens":{{"used_today":{limit},"limit_per_day":{limit},"remaining":0}},
            "wallet":{{"balance_usd":"99.000000","held_usd":"0.000000"}}}}"#
        );
        let result = result_for(&body).unwrap();
        assert_eq!(result.usage.primary.used_percent, 100.0);
        assert_eq!(detail_values(&result)[2], "0");
        assert!(result.cost.is_none());
    }
}

#[test]
fn floating_point_counters_are_rejected() {
    for value in ["0.5", "5.0", "1e3"] {
        assert_parse_failure(&free_tokens(&format!(r#""used_today":{value}"#)));
    }
}

#[test]
fn invalid_counters_fail_without_echoing_response_data() {
    for value in [
        "-1",
        "true",
        "\"private-response\"",
        "9007199254740992",
        "{}",
        "[]",
    ] {
        assert_parse_failure(&free_tokens(&format!(r#""used_today":{value}"#)));
    }
    for body in [
        "private-response",
        "null",
        "[]",
        "{}",
        r#"{"object":"usage","free_tokens":{}}"#,
        r#"{"object":"usage","free_tokens":[]}"#,
        r#"{"object":"other","free_tokens":{"used_today":1}}"#,
        r#"{"free_tokens":{"used_today":1}}"#,
        r#"{"object":"usage","free_tokens":{"used_today":null,"remaining":null}}"#,
    ] {
        assert_parse_failure(body);
    }
}

#[test]
fn largest_safe_integer_is_accepted() {
    let result = result_for(&free_tokens(r#""remaining":9007199254740991"#)).unwrap();
    assert_eq!(detail_values(&result)[0], "9,007,199,254,740,991");
}

#[test]
fn retry_after_is_bounded_and_defaults_to_one_second() {
    assert_eq!(retry_after_seconds(Some("30")), 10.0);
    assert_eq!(retry_after_seconds(Some("2.5")), 2.5);
    assert_eq!(retry_after_seconds(Some("0")), 0.0);
    assert_eq!(retry_after_seconds(Some("-3")), 1.0);
    assert_eq!(retry_after_seconds(Some("soon")), 1.0);
    assert_eq!(retry_after_seconds(None), 1.0);

    let mut headers = HeaderMap::new();
    headers.insert("retry-after", "0.5".parse().unwrap());
    let error = validate_status(StatusCode::TOO_MANY_REQUESTS, &headers).unwrap_err();
    assert!(error.to_string().contains("retry after 0.5s"), "{error}");
}

fn provider_for(server: &mockito::ServerGuard) -> XKiroProvider {
    XKiroProvider::new().with_usage_url(format!("{}/v1/usage", server.url()))
}

#[tokio::test]
async fn fetch_sends_a_bearer_get_to_the_usage_path() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/v1/usage")
        .match_header("authorization", "Bearer fixture-key")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(FIXTURE)
        .create_async()
        .await;

    let result = provider_for(&server)
        .fetch_api(API_KEY, now())
        .await
        .unwrap();

    mock.assert_async().await;
    assert!((result.usage.primary.used_percent - 2.4807).abs() < 0.00001);
    assert_eq!(result.usage.login_method.as_deref(), Some("Pay as you go"));
}

#[tokio::test]
async fn http_failures_are_classified_without_echoing_the_body() {
    for (status, expected) in [
        (401, "Authentication required"),
        (403, "denied this API key"),
        (429, "rate limit reached; retry after 10s"),
        (503, "unavailable (HTTP 503)"),
        (400, "returned HTTP 400"),
    ] {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/v1/usage")
            .with_status(status)
            .with_header("retry-after", "30")
            .with_body("private-response")
            .create_async()
            .await;

        let error = provider_for(&server)
            .fetch_api(API_KEY, now())
            .await
            .unwrap_err();
        let message = error.to_string();
        assert!(message.contains(expected), "{status}: {message}");
        assert!(!message.contains("private-response"), "{status}: {message}");
    }
}

#[tokio::test]
async fn oversized_responses_are_rejected_as_unrecognized() {
    let mut server = mockito::Server::new_async().await;
    mock_response(
        &mut server,
        "GET",
        "/v1/usage",
        200,
        "x".repeat(MAX_RESPONSE_BYTES + 1),
    )
    .await;

    let error = provider_for(&server)
        .fetch_api(API_KEY, now())
        .await
        .unwrap_err();
    assert!(matches!(error, ProviderError::Parse(_)), "{error:?}");
}

#[tokio::test]
async fn unsupported_source_modes_do_not_reach_the_network() {
    let ctx = FetchContext {
        source_mode: SourceMode::Web,
        api_key: Some(API_KEY.to_string()),
        ..FetchContext::default()
    };
    let error = XKiroProvider::new().fetch_usage(&ctx).await.unwrap_err();
    assert!(matches!(
        error,
        ProviderError::UnsupportedSource(SourceMode::Web)
    ));
}

#[test]
fn registration_defaults_off_with_the_documented_metadata() {
    let provider = XKiroProvider::new();
    let metadata = provider.metadata();
    assert_eq!(provider.id(), ProviderId::XKiro);
    assert!(!metadata.default_enabled);
    assert!(!metadata.supports_credits);
    assert_eq!(metadata.display_name, "xKiro");
    assert_eq!(metadata.session_label, "Daily free tokens");
    assert_eq!(metadata.dashboard_url, Some("https://xkiro.com"));
    assert_eq!(
        provider.available_sources(),
        vec![SourceMode::Auto, SourceMode::OAuth]
    );
    assert_eq!(ENV_KEYS, ["XKIRO_API_KEY"]);
    assert_eq!(USAGE_URL, "https://api.xkiro.com/v1/usage");
}
