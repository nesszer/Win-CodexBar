//! Command Code API-key lane (upstream #4462, Windows port).
//!
//! Upstream's explicit `api` source is `SourceMode::OAuth` in this port, the
//! API-key lane every non-OAuth provider uses (see Venice and Codebuff).

use super::*;

#[test]
fn api_key_lane_is_offered_alongside_auto_and_web() {
    let sources = CommandCodeProvider::new().available_sources();
    assert!(
        sources.contains(&SourceMode::OAuth),
        "API-key lane missing from {sources:?}"
    );
    assert!(sources.contains(&SourceMode::Auto));
    assert!(sources.contains(&SourceMode::Web));
}

#[tokio::test]
async fn explicit_api_source_without_key_reports_missing_key() {
    // Resolution reads COMMAND_CODE_API_KEY and the keyring; skip on a machine
    // that has one so the test never reaches the network.
    if !no_api_key_resolvable() {
        return;
    }
    let provider = CommandCodeProvider::new();
    let ctx = FetchContext {
        source_mode: SourceMode::OAuth,
        ..FetchContext::default()
    };
    let err = provider
        .fetch_usage(&ctx)
        .await
        .expect_err("an explicit API source without a key must fail");
    assert!(
        matches!(&err, ProviderError::NotInstalled(message) if message.contains("COMMAND_CODE_API_KEY")),
        "unexpected error: {err:?}"
    );
}

const CREDITS_BODY: &str = r#"{"credits":{"monthlyCredits":4.0,"purchasedCredits":1.0,"premiumMonthlyCredits":0,"opensourceMonthlyCredits":0,"monthlyCreditsGranted":10.0}}"#;
const SUBSCRIPTION_BODY: &str =
    r#"{"data":{"planId":"individual-go","currentPeriodEnd":"2030-01-01T00:00:00Z"}}"#;
const SESSION_COOKIE: &str = "__Secure-commandcode_prod_.session_token=fixture-session";

fn provider_for(server: &mockito::ServerGuard) -> CommandCodeProvider {
    CommandCodeProvider::with_api_base(server.url())
}

/// True when no API key resolves from the keyring or environment, so Auto
/// tests that expect the cookie lane never see a real credential.
fn no_api_key_resolvable() -> bool {
    resolve_command_code_key(None).is_err()
}

#[tokio::test]
async fn api_key_lane_reads_alpha_routes_with_bearer_auth() {
    let mut server = mockito::Server::new_async().await;
    let credits = server
        .mock("GET", "/alpha/billing/credits")
        .match_header("authorization", "Bearer fixture-key")
        .with_status(200)
        .with_body(CREDITS_BODY)
        .expect(1)
        .create_async()
        .await;
    let subscriptions = server
        .mock("GET", "/alpha/billing/subscriptions")
        .match_header("authorization", "Bearer fixture-key")
        .with_status(200)
        .with_body(SUBSCRIPTION_BODY)
        .expect(1)
        .create_async()
        .await;
    let cookie_route = server
        .mock("GET", mockito::Matcher::Regex("^/internal/".into()))
        .expect(0)
        .create_async()
        .await;
    let ctx = FetchContext {
        source_mode: SourceMode::OAuth,
        api_key: Some("fixture-key".into()),
        ..FetchContext::default()
    };

    let result = provider_for(&server).fetch_usage(&ctx).await.unwrap();
    credits.assert_async().await;
    subscriptions.assert_async().await;
    cookie_route.assert_async().await;
    assert_eq!(result.source_label, "api");
}

#[tokio::test]
async fn auto_prefers_configured_key_over_session_cookie() {
    let mut server = mockito::Server::new_async().await;
    let credits = server
        .mock("GET", "/alpha/billing/credits")
        .match_header("authorization", "Bearer fixture-key")
        .with_status(200)
        .with_body(CREDITS_BODY)
        .expect(1)
        .create_async()
        .await;
    let subscriptions = server
        .mock("GET", "/alpha/billing/subscriptions")
        .with_status(200)
        .with_body(SUBSCRIPTION_BODY)
        .expect(1)
        .create_async()
        .await;
    let cookie_route = server
        .mock("GET", mockito::Matcher::Regex("^/internal/".into()))
        .expect(0)
        .create_async()
        .await;
    let ctx = FetchContext {
        source_mode: SourceMode::Auto,
        api_key: Some("fixture-key".into()),
        manual_cookie_header: Some(SESSION_COOKIE.into()),
        ..FetchContext::default()
    };

    let result = provider_for(&server).fetch_usage(&ctx).await.unwrap();
    credits.assert_async().await;
    subscriptions.assert_async().await;
    cookie_route.assert_async().await;
    assert_eq!(result.source_label, "api");
}

#[tokio::test]
async fn rejected_api_key_reports_expired_without_cookie_fallback() {
    let mut server = mockito::Server::new_async().await;
    let credits = server
        .mock("GET", "/alpha/billing/credits")
        .with_status(401)
        .expect(1)
        .create_async()
        .await;
    let cookie_route = server
        .mock("GET", mockito::Matcher::Regex("^/internal/".into()))
        .expect(0)
        .create_async()
        .await;
    let ctx = FetchContext {
        source_mode: SourceMode::Auto,
        api_key: Some("fixture-key".into()),
        manual_cookie_header: Some(SESSION_COOKIE.into()),
        ..FetchContext::default()
    };
    let provider = provider_for(&server);

    let error = provider.fetch_usage(&ctx).await.err().unwrap();
    credits.assert_async().await;
    cookie_route.assert_async().await;
    assert!(
        matches!(&error, ProviderError::Other(message) if message == API_KEY_REJECTED),
        "unexpected error: {error}"
    );
    assert!(matches!(
        provider.error_state_kind(&error),
        ProviderStateKind::ExpiredSession
    ));
}

#[tokio::test]
async fn web_source_keeps_internal_routes_and_ignores_the_api_key() {
    let mut server = mockito::Server::new_async().await;
    let credits = server
        .mock("GET", "/internal/billing/credits")
        .match_header("cookie", SESSION_COOKIE)
        .with_status(200)
        .with_body(CREDITS_BODY)
        .expect(1)
        .create_async()
        .await;
    let subscriptions = server
        .mock("GET", "/internal/billing/subscriptions")
        .match_header("cookie", SESSION_COOKIE)
        .with_status(200)
        .with_body(SUBSCRIPTION_BODY)
        .expect(1)
        .create_async()
        .await;
    let key_route = server
        .mock("GET", mockito::Matcher::Regex("^/alpha/".into()))
        .expect(0)
        .create_async()
        .await;
    let ctx = FetchContext {
        source_mode: SourceMode::Web,
        manual_cookie_header: Some(SESSION_COOKIE.into()),
        api_key: Some("fixture-key".into()),
        ..FetchContext::default()
    };

    let result = provider_for(&server).fetch_usage(&ctx).await.unwrap();
    credits.assert_async().await;
    subscriptions.assert_async().await;
    key_route.assert_async().await;
    assert_eq!(result.source_label, "web");
}

#[tokio::test]
async fn cookie_source_off_never_reaches_the_network_in_auto() {
    if !no_api_key_resolvable() {
        return;
    }
    let mut server = mockito::Server::new_async().await;
    let any_request = server
        .mock("GET", mockito::Matcher::Any)
        .expect(0)
        .create_async()
        .await;
    let ctx = FetchContext {
        source_mode: SourceMode::Auto,
        manual_cookie_missing: true,
        ..FetchContext::default()
    };

    let error = provider_for(&server).fetch_usage(&ctx).await.err().unwrap();
    any_request.assert_async().await;
    assert!(
        matches!(error, ProviderError::NoCookies),
        "unexpected error: {error}"
    );
}
