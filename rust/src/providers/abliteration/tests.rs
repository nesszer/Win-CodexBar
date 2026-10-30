//! Fixture tests ported from upstream `TestsPlugin/AbliterationPluginTests.swift`.
//! Fetches use an injected API key and a local mock server, so no test reads
//! the keyring, the environment, or the real abliteration.ai API.

use super::*;
use mockito::{Matcher, Server};

const KEY: &str = "abl_fixture";
const CONTRACT: &str = r#"{"data":{"total_credits":50.0,"total_usage":0.010637}}"#;

fn context(source_mode: SourceMode) -> FetchContext {
    FetchContext {
        source_mode,
        api_key: Some(KEY.to_string()),
        ..FetchContext::default()
    }
}

async fn fetch_with(status: usize, body: &str) -> Result<ProviderFetchResult, ProviderError> {
    let mut server = Server::new_async().await;
    let mock = server
        .mock("GET", CREDITS_PATH)
        .match_header("authorization", format!("Bearer {KEY}").as_str())
        .match_header("cookie", Matcher::Missing)
        .with_status(status)
        .with_header("content-type", "application/json")
        .with_body(body)
        .expect(1)
        .create_async()
        .await;
    let result = AbliterationProvider::with_base_url(&server.url())
        .fetch_usage(&context(SourceMode::Auto))
        .await;
    mock.assert_async().await;
    result
}

async fn fetch(body: &str) -> ProviderFetchResult {
    fetch_with(200, body).await.expect("fixture fetch succeeds")
}

fn detail(result: &ProviderFetchResult, title: &str) -> Option<String> {
    result
        .display_details()
        .iter()
        .find(|detail| detail.title() == title)
        .map(|detail| {
            assert_eq!(detail.section_title(), Some(SECTION));
            detail.value().to_string()
        })
}

#[test]
fn metadata_matches_the_upstream_descriptor() {
    let provider = AbliterationProvider::new();
    assert_eq!(provider.id(), ProviderId::Abliteration);
    let metadata = provider.metadata();
    assert_eq!(metadata.display_name, "abliteration.ai");
    assert_eq!(metadata.session_label, "Credits");
    assert_eq!(metadata.weekly_label, "Usage");
    assert_eq!(metadata.dashboard_url, Some("https://abliteration.ai"));
    assert!(!metadata.default_enabled);
    assert_eq!(
        provider.available_sources(),
        vec![SourceMode::Auto, SourceMode::OAuth]
    );
    assert_eq!(ProviderId::Abliteration.cookie_domain(), None);
    assert_eq!(
        ProviderId::from_cli_name("abliteration"),
        Some(ProviderId::Abliteration)
    );
    assert_eq!(
        ProviderId::from_cli_name("abliteration.ai"),
        Some(ProviderId::Abliteration)
    );
}

#[tokio::test]
async fn contract_response_yields_usd_balance_and_a_cumulative_meter_without_reset() {
    let result = fetch(CONTRACT).await;
    let primary = &result.usage.primary;
    assert!((primary.used_percent - 0.021274).abs() < 0.000001);
    assert_eq!(primary.reset_description.as_deref(), Some("$0.01 / $50.00"));
    assert!(primary.description_is_detail);
    assert_eq!(primary.resets_at, None);
    assert_eq!(primary.window_minutes, None);
    assert!(result.usage.secondary.is_none());
    assert!(result.cost.is_none());
    assert_eq!(detail(&result, "Balance").as_deref(), Some("$49.99"));
    assert_eq!(detail(&result, "Credits used").as_deref(), Some("$0.01"));
    assert_eq!(detail(&result, "Total credits").as_deref(), Some("$50.00"));
}

#[tokio::test]
async fn zero_usage_overspending_and_zero_credits_preserve_honest_amounts() {
    let unused = fetch(r#"{"data":{"total_credits":50,"total_usage":0}}"#).await;
    assert_eq!(unused.usage.primary.used_percent, 0.0);
    assert_eq!(detail(&unused, "Balance").as_deref(), Some("$50.00"));

    let overspent = fetch(r#"{"data":{"total_credits":50,"total_usage":60}}"#).await;
    assert_eq!(overspent.usage.primary.used_percent, 100.0);
    assert_eq!(detail(&overspent, "Balance").as_deref(), Some("$0.00"));
    assert_eq!(
        detail(&overspent, "Credits used").as_deref(),
        Some("$60.00")
    );

    for used in [0, 1] {
        let zero = fetch(&format!(
            r#"{{"data":{{"total_credits":0,"total_usage":{used}}}}}"#
        ))
        .await;
        assert!(zero.usage.primary.is_informational);
        assert!(zero.usage.primary.hidden_on_card);
        assert_eq!(detail(&zero, "Balance").as_deref(), Some("$0.00"));
        assert_eq!(detail(&zero, "Total credits").as_deref(), Some("$0.00"));
    }
}

#[tokio::test]
async fn malformed_counters_fail_closed_without_disclosing_the_response() {
    for body in [
        "not-json",
        "[]",
        "{}",
        r#"{"data":[]}"#,
        r#"{"data":{"total_credits":50}}"#,
        r#"{"data":{"total_credits":"50","total_usage":0}}"#,
        r#"{"data":{"total_credits":null,"total_usage":0}}"#,
        r#"{"data":{"total_credits":-1,"total_usage":0}}"#,
        r#"{"data":{"total_credits":50,"total_usage":-1}}"#,
        r#"{"data":{"total_credits":50,"total_usage":true}}"#,
        r#"{"data":{"total_credits":1e309,"total_usage":0}}"#,
    ] {
        let error = fetch_with(200, body).await.expect_err(body);
        assert!(
            matches!(error, ProviderError::Parse(_)),
            "{body}: {error:?}"
        );
        assert!(!error.to_string().contains("total_credits"));
    }
}

#[tokio::test]
async fn http_errors_use_friendly_guidance_and_never_echo_the_body() {
    for (status, expected) in [
        (401, CHECK_KEY),
        (403, CHECK_KEY),
        (429, RATE_LIMITED),
        (500, REQUEST_FAILED),
        (404, REQUEST_FAILED),
    ] {
        let error = fetch_with(status, "private-response-marker")
            .await
            .expect_err("HTTP error");
        let message = error.to_string();
        assert!(message.contains(expected), "{status}: {message}");
        assert!(!message.contains("private-response-marker"));
    }
}

#[tokio::test]
async fn web_and_cli_sources_are_unsupported() {
    let provider = AbliterationProvider::with_base_url("http://127.0.0.1:9");
    for mode in [SourceMode::Web, SourceMode::Cli] {
        assert!(matches!(
            provider.fetch_usage(&context(mode)).await,
            Err(ProviderError::UnsupportedSource(_))
        ));
    }
}
