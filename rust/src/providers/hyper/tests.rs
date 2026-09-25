use super::*;

#[test]
fn parses_exact_non_negative_hypercredit_balances() {
    for (body, expected) in [
        (br#"{"balance":0}"#.as_slice(), 0.0),
        (br#"{"balance":12}"#.as_slice(), 12.0),
        (br#"{"balance":18.5}"#.as_slice(), 18.5),
    ] {
        assert_eq!(parse_balance(body).unwrap(), expected);
    }
}

#[test]
fn rejects_missing_or_invalid_balance_values() {
    for body in [
        br#"{}"#.as_slice(),
        br#"{"balance":"12"}"#.as_slice(),
        br#"{"balance":-1}"#.as_slice(),
        br#"{"balance":null}"#.as_slice(),
        br#"{"balance":1e400}"#.as_slice(),
        b"null".as_slice(),
        b"[]".as_slice(),
        b"not-json".as_slice(),
    ] {
        assert!(matches!(parse_balance(body), Err(ProviderError::Parse(_))));
    }
}

#[test]
fn formats_balance_to_two_fractional_digits_at_most() {
    assert_eq!(format_balance(0.0), "0");
    assert_eq!(format_balance(12.0), "12");
    assert_eq!(format_balance(18.5), "18.5");
    assert_eq!(format_balance(42.567), "42.57");
    assert_eq!(format_balance(12_500.125), "12,500.13");
}

#[test]
fn only_session_auth_redirect_and_html_responses_are_rejected_sessions() {
    let mut html = reqwest::header::HeaderMap::new();
    html.insert(
        reqwest::header::CONTENT_TYPE,
        "text/html; charset=utf-8".parse().unwrap(),
    );
    assert!(is_rejected_session(StatusCode::UNAUTHORIZED, &html));
    assert!(is_rejected_session(StatusCode::FORBIDDEN, &html));
    assert!(is_rejected_session(StatusCode::FOUND, &html));
    assert!(is_rejected_session(StatusCode::OK, &html));
    assert!(!is_rejected_session(
        StatusCode::OK,
        &reqwest::header::HeaderMap::new()
    ));
    assert!(!is_rejected_session(StatusCode::TOO_MANY_REQUESTS, &html));
}

#[test]
fn classifies_api_statuses_without_echoing_response_bodies() {
    assert!(matches!(
        status_error(StatusCode::UNAUTHORIZED),
        ProviderError::AuthRequired
    ));
    assert!(matches!(
        status_error(StatusCode::FORBIDDEN),
        ProviderError::Other(message) if message.contains("permissions") || message.contains("access")
    ));
    assert!(matches!(
        status_error(StatusCode::TOO_MANY_REQUESTS),
        ProviderError::Other(message) if message.contains("rate limited")
    ));
    assert!(matches!(
        status_error(StatusCode::SERVICE_UNAVAILABLE),
        ProviderError::Other(message) if message.contains("unavailable")
    ));
}

#[tokio::test]
async fn api_source_uses_bearer_key_and_displays_balance_without_quota_math() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/v1/credits")
        .match_header("accept", "application/json")
        .match_header("authorization", "Bearer fixture-api-key")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"balance":42.5}"#)
        .create_async()
        .await;
    let provider = test_provider(&server.url());
    let ctx = FetchContext {
        source_mode: SourceMode::OAuth,
        api_key: Some("fixture-api-key".into()),
        ..FetchContext::default()
    };

    let result = provider.fetch_usage(&ctx).await.unwrap();
    mock.assert_async().await;
    assert_eq!(result.source_label, "api");
    assert!(result.usage.primary.is_informational);
    assert_eq!(result.display_details().len(), 1);
    assert_eq!(result.display_details()[0].title(), "Hypercredits");
    assert_eq!(result.display_details()[0].value(), "42.5 HC");
    assert_eq!(result.usage.login_method.as_deref(), Some("API key"));
}

#[tokio::test]
async fn auto_source_falls_back_from_rejected_session_to_api_key() {
    let mut server = mockito::Server::new_async().await;
    let session = server
        .mock("GET", "/v1/credits")
        .match_header("cookie", "session=fixture")
        .with_status(401)
        .create_async()
        .await;
    let api = server
        .mock("GET", "/v1/credits")
        .match_header("authorization", "Bearer fixture-api-key")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"balance":42.5}"#)
        .create_async()
        .await;
    let provider = test_provider(&server.url());
    let ctx = FetchContext {
        source_mode: SourceMode::Auto,
        manual_cookie_header: Some("session=fixture".into()),
        api_key: Some("fixture-api-key".into()),
        ..FetchContext::default()
    };

    let result = provider.fetch_usage(&ctx).await.unwrap();
    session.assert_async().await;
    api.assert_async().await;
    assert_eq!(result.source_label, "api");
    assert_eq!(result.usage.login_method.as_deref(), Some("API key"));
}

#[tokio::test]
async fn auto_source_falls_back_after_session_rate_limit_or_server_error() {
    for status in [429, 503] {
        let mut server = mockito::Server::new_async().await;
        let session = server
            .mock("GET", "/v1/credits")
            .match_header("cookie", "session=fixture")
            .with_status(status)
            .create_async()
            .await;
        let api = server
            .mock("GET", "/v1/credits")
            .match_header("authorization", "Bearer fixture-api-key")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"balance":42.5}"#)
            .create_async()
            .await;
        let provider = test_provider(&server.url());
        let ctx = FetchContext {
            source_mode: SourceMode::Auto,
            manual_cookie_header: Some("session=fixture".into()),
            api_key: Some("fixture-api-key".into()),
            ..FetchContext::default()
        };

        let result = provider.fetch_usage(&ctx).await.unwrap();
        session.assert_async().await;
        api.assert_async().await;
        assert_eq!(result.source_label, "api");
        assert_eq!(result.usage.login_method.as_deref(), Some("API key"));
    }
}

#[tokio::test]
async fn web_only_source_does_not_fall_back_to_configured_api_key() {
    let mut server = mockito::Server::new_async().await;
    let session = server
        .mock("GET", "/v1/credits")
        .match_header("cookie", "session=fixture")
        .with_status(401)
        .create_async()
        .await;
    let provider = test_provider(&server.url());
    let ctx = FetchContext {
        source_mode: SourceMode::Web,
        manual_cookie_header: Some("session=fixture".into()),
        api_key: Some("fixture-api-key".into()),
        ..FetchContext::default()
    };

    assert!(matches!(
        provider.fetch_usage(&ctx).await,
        Err(ProviderError::AuthRequired)
    ));
    session.assert_async().await;
}

fn test_provider(base_url: &str) -> HyperProvider {
    let client = Client::builder()
        .redirect(Policy::none())
        .timeout(API_TIMEOUT)
        .build()
        .unwrap();
    HyperProvider::with_client(format!("{base_url}/v1/credits"), client)
}
