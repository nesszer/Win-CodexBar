//! Translations of upstream `HyperPluginTests` (v0.65.0). Every test passes
//! the key to `fetch_with_key` (or an explicit `api_key`) and a manual or
//! missing cookie, so no test reads the keyring or a real browser.

use super::*;
use crate::providers::test_support::mock_status_expect;
use mockito::{Matcher, Mock, Server, ServerGuard};

const KEY: &str = "fixture-key";
const BALANCE_BODY: &str = r#"{"balance":42.5}"#;

fn test_provider(base_url: &str) -> HyperProvider {
    let client = Client::builder()
        .redirect(Policy::none())
        .timeout(API_TIMEOUT)
        .build()
        .unwrap();
    HyperProvider::with_client(format!("{base_url}/v1/credits"), client)
}

/// `cookie: None` is the shell's off / empty-manual cookie source.
fn context(source_mode: SourceMode, cookie: Option<&str>) -> FetchContext {
    FetchContext {
        source_mode,
        manual_cookie_header: cookie.map(str::to_string),
        manual_cookie_missing: cookie.is_none(),
        ..FetchContext::default()
    }
}

/// A session request: only the cookie, never a bearer key.
async fn session_mock(
    server: &mut ServerGuard,
    cookie: &str,
    status: usize,
    html: bool,
    hits: usize,
) -> Mock {
    let (content_type, body) = if html {
        ("text/html; charset=utf-8", "<html>Log in</html>")
    } else {
        ("application/json", BALANCE_BODY)
    };
    server
        .mock("GET", "/v1/credits")
        .match_header("accept", "application/json")
        .match_header("cookie", cookie)
        .match_header("authorization", Matcher::Missing)
        .with_status(status)
        .with_header("content-type", content_type)
        .with_body(body)
        .expect(hits)
        .create_async()
        .await
}

/// An API-key request: only the bearer key, never a cookie.
async fn api_mock(server: &mut ServerGuard, status: usize, body: &str, hits: usize) -> Mock {
    server
        .mock("GET", "/v1/credits")
        .match_header("accept", "application/json")
        .match_header("authorization", format!("Bearer {KEY}").as_str())
        .match_header("cookie", Matcher::Missing)
        .with_status(status)
        .with_header("content-type", "application/json")
        .with_body(body)
        .expect(hits)
        .create_async()
        .await
}

/// Fails the test if the provider sends any request at all.
async fn no_request_mock(server: &mut ServerGuard) -> Mock {
    mock_status_expect(server, "GET", Matcher::Any, 200, 0).await
}

fn closed_port_url() -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    format!("http://{address}")
}

fn message(error: &ProviderError) -> String {
    error.to_string()
}

#[tokio::test]
async fn balance_fixtures_render_native_hc_without_fabricated_limits() {
    for (body, expected) in [
        (r#"{"balance":0}"#, "0 HC"),
        (r#"{"balance":12}"#, "12 HC"),
        (r#"{"balance":18.5}"#, "18.5 HC"),
        (r#"{"balance":42.5}"#, "42.5 HC"),
    ] {
        let mut server = Server::new_async().await;
        let api = api_mock(&mut server, 200, body, 1).await;
        let provider = test_provider(&server.url());

        let result = provider
            .fetch_with_key(&context(SourceMode::Auto, None), Some(KEY))
            .await
            .unwrap();

        api.assert_async().await;
        assert_eq!(result.source_label, "api");
        assert_eq!(result.usage.login_method.as_deref(), Some("API key"));
        assert!(result.usage.primary.is_informational);
        assert!(result.usage.secondary.is_none());
        assert!(result.cost.is_none());
        let details = result.display_details();
        assert_eq!(details.len(), 1);
        assert_eq!(details[0].title(), "Hypercredits");
        assert_eq!(details[0].value(), expected);
    }
}

#[tokio::test]
async fn fetch_usage_uses_the_configured_api_key_for_the_api_source() {
    let mut server = Server::new_async().await;
    let api = api_mock(&mut server, 200, BALANCE_BODY, 1).await;
    let provider = test_provider(&server.url());
    let ctx = FetchContext {
        api_key: Some(format!(" '{KEY}' ")),
        ..context(SourceMode::OAuth, None)
    };

    let result = provider.fetch_usage(&ctx).await.unwrap();

    api.assert_async().await;
    assert_eq!(result.display_details()[0].value(), "42.5 HC");
    assert_eq!(result.usage.login_method.as_deref(), Some("API key"));
}

#[tokio::test]
async fn session_wins_over_key_and_sends_only_cookies() {
    let mut server = Server::new_async().await;
    let session = session_mock(&mut server, "session=fixture-session", 200, false, 1).await;
    let api = api_mock(&mut server, 200, BALANCE_BODY, 0).await;
    let provider = test_provider(&server.url());

    let result = provider
        .fetch_with_key(
            &context(SourceMode::Auto, Some("session=fixture-session")),
            Some(KEY),
        )
        .await
        .unwrap();

    session.assert_async().await;
    api.assert_async().await;
    assert_eq!(result.source_label, "web");
    assert_eq!(
        result.usage.login_method.as_deref(),
        Some("Browser session")
    );
    assert_eq!(result.display_details()[0].value(), "42.5 HC");
}

#[tokio::test]
async fn api_source_bypasses_the_session_cookie() {
    let mut server = Server::new_async().await;
    let session = session_mock(&mut server, "session=unused", 200, false, 0).await;
    let api = api_mock(&mut server, 200, BALANCE_BODY, 1).await;
    let provider = test_provider(&server.url());

    let result = provider
        .fetch_with_key(
            &context(SourceMode::OAuth, Some("session=unused")),
            Some(KEY),
        )
        .await
        .unwrap();

    session.assert_async().await;
    api.assert_async().await;
    assert_eq!(result.usage.login_method.as_deref(), Some("API key"));
}

#[tokio::test]
async fn missing_session_falls_back_to_key() {
    // An empty manual cookie and an off / empty-manual cookie source both
    // leave Auto without a session.
    for ctx in [
        context(SourceMode::Auto, None),
        context(SourceMode::Auto, Some("  ")),
    ] {
        let mut server = Server::new_async().await;
        let api = api_mock(&mut server, 200, BALANCE_BODY, 1).await;
        let provider = test_provider(&server.url());

        let result = provider.fetch_with_key(&ctx, Some(KEY)).await.unwrap();

        api.assert_async().await;
        assert_eq!(result.usage.login_method.as_deref(), Some("API key"));
    }
}

#[tokio::test]
async fn expired_session_is_rejected_before_api_fallback() {
    for (status, html) in [(401, false), (403, false), (302, false), (200, true)] {
        let mut server = Server::new_async().await;
        let session = session_mock(
            &mut server,
            "session=fixture-rejected-session",
            status,
            html,
            1,
        )
        .await;
        let api = api_mock(&mut server, 200, BALANCE_BODY, 1).await;
        let provider = test_provider(&server.url());

        let result = provider
            .fetch_with_key(
                &context(SourceMode::Auto, Some("session=fixture-rejected-session")),
                Some(KEY),
            )
            .await
            .unwrap();

        session.assert_async().await;
        api.assert_async().await;
        assert_eq!(result.source_label, "api", "status {status}");
        assert_eq!(result.usage.login_method.as_deref(), Some("API key"));
    }
}

#[tokio::test]
async fn web_only_never_falls_back_to_configured_key() {
    for (status, html) in [(401, false), (403, false), (302, false), (200, true)] {
        let mut server = Server::new_async().await;
        let session = session_mock(
            &mut server,
            "session=fixture-rejected-session",
            status,
            html,
            1,
        )
        .await;
        let api = api_mock(&mut server, 200, BALANCE_BODY, 0).await;
        let provider = test_provider(&server.url());

        let error = provider
            .fetch_with_key(
                &context(SourceMode::Web, Some("session=fixture-rejected-session")),
                Some(KEY),
            )
            .await
            .unwrap_err();

        session.assert_async().await;
        api.assert_async().await;
        assert!(
            matches!(&error, ProviderError::Other(message) if message == SESSION_EXPIRED),
            "status {status}: {error:?}"
        );
        assert_eq!(
            provider.error_state_kind(&error),
            ProviderStateKind::ExpiredSession
        );
    }
}

#[tokio::test]
async fn rejected_session_without_a_key_reports_expired_session() {
    let mut server = Server::new_async().await;
    let session = session_mock(&mut server, "session=fixture", 401, false, 1).await;
    let provider = test_provider(&server.url());

    let error = provider
        .fetch_with_key(&context(SourceMode::Auto, Some("session=fixture")), None)
        .await
        .unwrap_err();

    session.assert_async().await;
    assert!(matches!(&error, ProviderError::Other(message) if message == SESSION_EXPIRED));
}

#[tokio::test]
async fn invalid_bodies_fail_closed_without_key_fallback() {
    for body in [
        "",
        "{}",
        r#"{"balance":"#,
        r#"{"balance":-1}"#,
        r#"{"balance":"invalid"}"#,
        r#"{"balance":null}"#,
        r#"{"balance":true}"#,
        r#"{"balance":1e400}"#,
        "null",
        "[]",
    ] {
        // Without a session the key request returns the invalid body.
        let mut server = Server::new_async().await;
        let api = api_mock(&mut server, 200, body, 1).await;
        let provider = test_provider(&server.url());
        let error = provider
            .fetch_with_key(&context(SourceMode::Auto, None), Some(KEY))
            .await
            .unwrap_err();
        api.assert_async().await;
        assert!(
            matches!(error, ProviderError::Parse(_)),
            "{body}: {error:?}"
        );

        // With a session the invalid session body is final.
        let mut server = Server::new_async().await;
        let session = server
            .mock("GET", "/v1/credits")
            .match_header("cookie", "session=fixture")
            .match_header("authorization", Matcher::Missing)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(body)
            .create_async()
            .await;
        let api = api_mock(&mut server, 200, BALANCE_BODY, 0).await;
        let provider = test_provider(&server.url());
        let error = provider
            .fetch_with_key(
                &context(SourceMode::Auto, Some("session=fixture")),
                Some(KEY),
            )
            .await
            .unwrap_err();
        session.assert_async().await;
        api.assert_async().await;
        assert!(
            matches!(error, ProviderError::Parse(_)),
            "{body}: {error:?}"
        );
    }
}

#[test]
fn invalid_balances_report_the_upstream_messages() {
    assert_eq!(
        message(&parse_balance(b"not-json").unwrap_err()),
        format!("Parse error: {INVALID_JSON}")
    );
    for body in [
        r#"{}"#,
        r#"{"balance":"12"}"#,
        r#"{"balance":-1}"#,
        r#"{"balance":null}"#,
        "null",
        "[]",
    ] {
        assert_eq!(
            message(&parse_balance(body.as_bytes()).unwrap_err()),
            format!("Parse error: {INVALID_BALANCE}"),
            "{body}"
        );
    }
    assert_eq!(parse_balance(br#"{"balance":0}"#).unwrap(), 0.0);
    assert_eq!(
        parse_balance(br#"{"balance":12500.125}"#).unwrap(),
        12_500.125
    );
}

#[tokio::test]
async fn http_errors_are_classified_without_exposing_response_or_key() {
    for (status, expected, kind) in [
        (
            401,
            API_KEY_REJECTED.to_string(),
            ProviderStateKind::ExpiredSession,
        ),
        (
            403,
            API_KEY_FORBIDDEN.to_string(),
            ProviderStateKind::Unknown,
        ),
        (429, RATE_LIMITED.to_string(), ProviderStateKind::Unknown),
        (
            503,
            "Charm Hyper API error: HTTP 503".to_string(),
            ProviderStateKind::Unknown,
        ),
        (
            404,
            "Charm Hyper API error: HTTP 404".to_string(),
            ProviderStateKind::Unknown,
        ),
    ] {
        let mut server = Server::new_async().await;
        let api = api_mock(&mut server, status, "private upstream body fixture-key", 1).await;
        let provider = test_provider(&server.url());

        let error = provider
            .fetch_with_key(&context(SourceMode::Auto, None), Some(KEY))
            .await
            .unwrap_err();

        api.assert_async().await;
        let text = message(&error);
        assert_eq!(text, expected);
        assert!(!text.contains("private upstream body"));
        assert!(!text.contains(KEY));
        assert_eq!(provider.error_state_kind(&error), kind, "status {status}");
    }
}

#[tokio::test]
async fn session_rate_limits_and_server_errors_are_final() {
    for (status, expected) in [
        (429, RATE_LIMITED.to_string()),
        (503, "Charm Hyper API error: HTTP 503".to_string()),
    ] {
        let mut server = Server::new_async().await;
        let session = session_mock(&mut server, "session=fixture", status, false, 1).await;
        let api = api_mock(&mut server, 200, BALANCE_BODY, 0).await;
        let provider = test_provider(&server.url());

        let error = provider
            .fetch_with_key(
                &context(SourceMode::Auto, Some("session=fixture")),
                Some(KEY),
            )
            .await
            .unwrap_err();

        session.assert_async().await;
        api.assert_async().await;
        assert_eq!(message(&error), expected);
    }
}

#[tokio::test]
async fn no_credentials_produce_guidance_without_http() {
    for (source_mode, key) in [
        (SourceMode::Auto, None),
        (SourceMode::Auto, Some("  ")),
        (SourceMode::Auto, Some("''")),
        (SourceMode::Web, Some(KEY)),
        (SourceMode::OAuth, None),
    ] {
        let mut server = Server::new_async().await;
        let none = no_request_mock(&mut server).await;
        let provider = test_provider(&server.url());

        let error = provider
            .fetch_with_key(&context(source_mode, None), key)
            .await
            .unwrap_err();

        none.assert_async().await;
        assert!(
            matches!(&error, ProviderError::NotInstalled(message) if message == MISSING_CREDENTIAL),
            "{source_mode:?}: {error:?}"
        );
        let text = message(&error);
        assert!(text.contains("Sign in"));
        assert!(text.contains("API key"));
        assert_eq!(
            provider.error_state_kind(&error),
            ProviderStateKind::NeedsAuthentication
        );
    }
}

#[tokio::test]
async fn session_transport_failure_fails_closed_without_a_fallback_key() {
    let provider = test_provider(&closed_port_url());
    for (source_mode, key) in [(SourceMode::Web, Some(KEY)), (SourceMode::Auto, None)] {
        let error = provider
            .fetch_with_key(&context(source_mode, Some("session=fixture")), key)
            .await
            .unwrap_err();

        assert!(
            matches!(&error, ProviderError::Other(message) if message == SESSION_REQUEST_FAILED),
            "{source_mode:?}: {error:?}"
        );
    }
}

#[tokio::test]
async fn failed_session_request_falls_back_to_key_in_auto() {
    // An oversized session body fails the session request the way upstream's
    // response-size limit does; Auto then tries the key.
    let mut server = Server::new_async().await;
    let session = server
        .mock("GET", "/v1/credits")
        .match_header("cookie", "session=fixture")
        .match_header("authorization", Matcher::Missing)
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(vec![b' '; MAX_RESPONSE_BYTES + 1])
        .create_async()
        .await;
    let api = api_mock(&mut server, 200, BALANCE_BODY, 1).await;
    let provider = test_provider(&server.url());

    let result = provider
        .fetch_with_key(
            &context(SourceMode::Auto, Some("session=fixture")),
            Some(KEY),
        )
        .await
        .unwrap();

    session.assert_async().await;
    api.assert_async().await;
    assert_eq!(result.usage.login_method.as_deref(), Some("API key"));
}

#[tokio::test]
async fn cli_source_is_unsupported() {
    let mut server = Server::new_async().await;
    let none = no_request_mock(&mut server).await;
    let provider = test_provider(&server.url());

    let error = provider
        .fetch_with_key(
            &context(SourceMode::Cli, Some("session=fixture")),
            Some(KEY),
        )
        .await
        .unwrap_err();

    none.assert_async().await;
    assert!(matches!(
        error,
        ProviderError::UnsupportedSource(SourceMode::Cli)
    ));
}

#[test]
fn keys_are_cleaned_like_upstream_settings_values() {
    assert_eq!(cleaned_key(" 'fixture-key' "), Some("fixture-key"));
    assert_eq!(cleaned_key("\" fixture-key \""), Some("fixture-key"));
    assert_eq!(cleaned_key("fixture-key"), Some("fixture-key"));
    assert_eq!(cleaned_key("'fixture-key"), Some("'fixture-key"));
    assert_eq!(cleaned_key("  "), None);
    assert_eq!(cleaned_key("''"), None);
    assert_eq!(cleaned_key("\""), None);
}

#[test]
fn provider_capabilities_match_the_upstream_descriptor() {
    let provider = HyperProvider::new();
    assert_eq!(
        provider.available_sources(),
        vec![SourceMode::Auto, SourceMode::Web, SourceMode::OAuth]
    );
    assert!(provider.supports_web());
    assert!(provider.cookie_source_scopes_session_only());
    assert!(provider.owns_browser_cookie_resolution());
    assert!(!provider.metadata().default_enabled);
    assert_eq!(crate::core::brand_color(ProviderId::Hyper), "#FF60FF");
    assert_eq!(
        provider.error_state_kind(&ProviderError::Other(RATE_LIMITED.into())),
        ProviderStateKind::Unknown
    );
    assert_eq!(
        provider.error_state_kind(&ProviderError::Parse(INVALID_BALANCE.into())),
        ProviderStateKind::Unknown
    );
}
