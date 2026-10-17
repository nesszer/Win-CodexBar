//! Fixture tests for the fal credit balance provider. Fetch tests use an
//! injected API key and a local mock server, so no test reads the keyring,
//! the environment, or the real fal API.

use super::*;
use mockito::{Matcher, Server};
use serde_json::json;

const KEY: &str = "fal-test-key";
const BILLING_PATH: &str = "/v1/account/billing";
const PERMISSION_MESSAGE: &str = "fal rejected the key: billing needs the billing:usage:read permission (BILLING, READONLY or FULL preset).";
const UNAVAILABLE_MESSAGE: &str =
    "fal did not return the credit balance this time. Try Refresh in a moment.";

fn test_provider(base_url: &str) -> FalProvider {
    FalProvider::with_billing_url(&format!("{base_url}{BILLING_PATH}?expand=credits"))
}

fn context(source_mode: SourceMode, api_key: Option<&str>) -> FetchContext {
    FetchContext {
        source_mode,
        api_key: api_key.map(str::to_string),
        ..FetchContext::default()
    }
}

fn rows(result: &ProviderFetchResult) -> Vec<(String, String)> {
    result
        .display_details
        .iter()
        .map(|detail| {
            assert_eq!(detail.section_title(), Some(SECTION_TITLE));
            (detail.title().to_string(), detail.value().to_string())
        })
        .collect()
}

fn row(title: &str, value: &str) -> (String, String) {
    (title.to_string(), value.to_string())
}

fn with_credits(credits: Value) -> Value {
    json!({ "credits": credits })
}

fn assert_other(result: Result<ProviderFetchResult, ProviderError>, expected: &str) {
    match result {
        Err(ProviderError::Other(message)) => assert_eq!(message, expected),
        other => panic!("expected Other({expected:?}), got {other:?}"),
    }
}

fn assert_parse(result: Result<ProviderFetchResult, ProviderError>, expected: &str) {
    match result {
        Err(ProviderError::Parse(message)) => assert_eq!(message, expected),
        other => panic!("expected Parse({expected:?}), got {other:?}"),
    }
}

#[test]
fn metadata_and_source_contract_match_the_api_key_provider() {
    let provider = FalProvider::new();
    assert_eq!(provider.id(), ProviderId::Fal);
    assert_eq!(provider.metadata().display_name, "fal");
    assert_eq!(
        provider.metadata().dashboard_url,
        Some("https://fal.ai/dashboard/billing")
    );
    assert!(!provider.metadata().default_enabled);
    assert_eq!(
        provider.available_sources(),
        vec![SourceMode::Auto, SourceMode::OAuth]
    );
    assert!(!provider.supports_web());
    assert_eq!(ProviderId::Fal.cookie_domain(), None);
    assert_eq!(ProviderId::from_cli_name("fal"), Some(ProviderId::Fal));
}

#[test]
fn balance_payload_matches_upstream_row_identity_and_organization() {
    let result = parse_billing(
        br#"{"username":"  acme-team ","credits":{"current_balance":12.5,"currency":"USD"}}"#,
    )
    .unwrap();

    assert_eq!(rows(&result), vec![row("Credit balance", "$12.50")]);
    assert_eq!(result.source_label, "api");
    assert_eq!(result.usage.login_method.as_deref(), Some("API key"));
    assert_eq!(
        result.usage.account_organization.as_deref(),
        Some("acme-team")
    );
    assert!(result.usage.primary.hidden_on_card);
    assert!(result.usage.primary.is_informational);
}

#[test]
fn string_balances_are_accepted_when_they_are_plain_decimals() {
    let result = parse_billing(
        serde_json::to_vec(&with_credits(json!({
            "current_balance": " 3.25 ",
            "currency": "EUR"
        })))
        .unwrap()
        .as_slice(),
    )
    .unwrap();
    assert_eq!(
        rows(&result),
        vec![row("Credit balance", &format_currency(3.25, "EUR"))]
    );

    let result = parse_billing(
        serde_json::to_vec(&with_credits(json!({ "current_balance": "7" })))
            .unwrap()
            .as_slice(),
    )
    .unwrap();
    assert_eq!(rows(&result), vec![row("Credit balance", "$7.00")]);
}

#[test]
fn unsupported_currency_codes_fall_back_to_usd_and_blank_names_are_dropped() {
    let result = parse_billing(
        serde_json::to_vec(&json!({
            "username": "   ",
            "credits": { "current_balance": 1, "currency": "eur" }
        }))
        .unwrap()
        .as_slice(),
    )
    .unwrap();
    assert_eq!(rows(&result), vec![row("Credit balance", "$1.00")]);
    assert_eq!(result.usage.account_organization, None);
}

#[test]
fn unrecognized_balances_fail_parsing_and_name_the_value_kind() {
    let cases = [
        (json!({ "current_balance": null }), "null"),
        (json!({ "current_balance": "abc" }), "string"),
        (json!({ "current_balance": "1e3" }), "string"),
        (json!({ "current_balance": [1] }), "array"),
        (json!({ "current_balance": true }), "boolean"),
        (json!({ "current_balance": { "value": 1 } }), "object"),
        (json!({}), "undefined"),
    ];
    for (credits, kind) in cases {
        let body = serde_json::to_vec(&with_credits(credits)).unwrap();
        assert_parse(
            parse_billing(&body),
            &format!("fal returned an unrecognized credit balance ({kind})."),
        );
    }
}

#[test]
fn missing_or_non_object_credits_report_the_balance_as_unavailable() {
    assert_other(
        parse_billing(br#"{"username":"acme"}"#),
        UNAVAILABLE_MESSAGE,
    );
    assert_other(parse_billing(br#"{"credits":[1,2]}"#), UNAVAILABLE_MESSAGE);
}

#[test]
fn malformed_bodies_fail_parsing_without_echoing_them() {
    assert_parse(
        parse_billing(b"<html>not json</html>"),
        "fal returned a non-JSON billing response.",
    );
    assert_parse(
        parse_billing(b"[]"),
        "fal returned an unrecognized billing response.",
    );
}

#[test]
fn plain_decimal_check_matches_upstream_pattern() {
    for accepted in ["12", "-3.5", " 4.0 ", "0.25"] {
        assert!(
            is_plain_decimal(accepted),
            "{accepted:?} should be accepted"
        );
    }
    for rejected in ["", "-", ".5", "1.", "1e3", "1,000", "NaN", "- 1"] {
        assert!(
            !is_plain_decimal(rejected),
            "{rejected:?} should be rejected"
        );
    }
}

#[tokio::test]
async fn fetch_sends_the_key_as_a_key_authorization_scheme() {
    let mut server = Server::new_async().await;
    let mock = server
        .mock("GET", BILLING_PATH)
        .match_query(Matcher::UrlEncoded("expand".into(), "credits".into()))
        .match_header("authorization", format!("Key {KEY}").as_str())
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"username":"acme","credits":{"current_balance":42,"currency":"USD"}}"#)
        .expect(1)
        .create_async()
        .await;

    let result = test_provider(&server.url())
        .fetch_usage(&context(SourceMode::Auto, Some(KEY)))
        .await
        .unwrap();

    mock.assert_async().await;
    assert_eq!(rows(&result), vec![row("Credit balance", "$42.00")]);
    assert_eq!(result.usage.account_organization.as_deref(), Some("acme"));
}

#[tokio::test]
async fn oauth_source_uses_the_same_api_key_lane() {
    let mut server = Server::new_async().await;
    let mock = server
        .mock("GET", BILLING_PATH)
        .match_query(Matcher::Any)
        .with_status(200)
        .with_body(r#"{"credits":{"current_balance":1}}"#)
        .expect(1)
        .create_async()
        .await;

    let result = test_provider(&server.url())
        .fetch_usage(&context(SourceMode::OAuth, Some(KEY)))
        .await
        .unwrap();

    mock.assert_async().await;
    assert_eq!(rows(&result), vec![row("Credit balance", "$1.00")]);
}

#[tokio::test]
async fn http_failures_report_their_class_without_a_body() {
    let mut server = Server::new_async().await;
    let unauthorized = server
        .mock("GET", BILLING_PATH)
        .match_query(Matcher::Any)
        .with_status(401)
        .with_body("secret echo")
        .expect(1)
        .create_async()
        .await;
    let result = test_provider(&server.url())
        .fetch_usage(&context(SourceMode::Auto, Some(KEY)))
        .await;
    unauthorized.assert_async().await;
    assert_other(
        result,
        "fal rejected the API key. Check or replace it in Settings.",
    );

    let forbidden = server
        .mock("GET", BILLING_PATH)
        .match_query(Matcher::Any)
        .with_status(403)
        .expect(1)
        .create_async()
        .await;
    let result = test_provider(&server.url())
        .fetch_usage(&context(SourceMode::Auto, Some(KEY)))
        .await;
    forbidden.assert_async().await;
    assert_other(result, PERMISSION_MESSAGE);

    let limited = server
        .mock("GET", BILLING_PATH)
        .match_query(Matcher::Any)
        .with_status(429)
        .expect(1)
        .create_async()
        .await;
    let result = test_provider(&server.url())
        .fetch_usage(&context(SourceMode::Auto, Some(KEY)))
        .await;
    limited.assert_async().await;
    assert_other(result, "fal returned HTTP 429.");

    let unavailable = server
        .mock("GET", BILLING_PATH)
        .match_query(Matcher::Any)
        .with_status(503)
        .expect(1)
        .create_async()
        .await;
    let result = test_provider(&server.url())
        .fetch_usage(&context(SourceMode::Auto, Some(KEY)))
        .await;
    unavailable.assert_async().await;
    assert_other(result, "fal returned HTTP 503.");
}

#[tokio::test]
async fn web_and_cli_sources_are_unsupported_and_send_nothing() {
    let mut server = Server::new_async().await;
    let never = server
        .mock("GET", BILLING_PATH)
        .match_query(Matcher::Any)
        .expect(0)
        .create_async()
        .await;
    let provider = test_provider(&server.url());

    for mode in [SourceMode::Web, SourceMode::Cli] {
        match provider.fetch_usage(&context(mode, Some(KEY))).await {
            Err(ProviderError::UnsupportedSource(source)) => assert_eq!(source, mode),
            other => panic!("expected UnsupportedSource, got {other:?}"),
        }
    }
    never.assert_async().await;
}
