//! Translations of upstream `HyperPluginTests` (v0.75.0). Every test passes
//! the key to `fetch_with_key` (or an explicit `api_key`) and a manual or
//! missing cookie, so no test reads the keyring or a real browser. Dashboard
//! and team-page markup are synthetic fixtures, not captured pages.

use super::*;
use mockito::{Matcher, Mock, Server, ServerGuard};

const KEY: &str = "fixture-key";
const BALANCE_BODY: &str = r#"{"balance":42.5}"#;
const TEAM_ID: &str = "11111111-2222-3333-4444-555555555555";
const DASHBOARD_PATH: &str = "/teams/11111111-2222-3333-4444-555555555555/dashboard";

/// Mirrors the signed-in team dashboard markup: balance card, plan card, refresh note, API key count.
const DASHBOARD_HTML: &str = r#"<html><head><title>Dashboard — Charm™ Hyper</title></head><body>
<div class="rounded-xl bg-card p-5">
  <p class="text-sm">Hypercredits Available</p>
  <div class="mt-1 flex items-center gap-1">
    <img src="/static/hexy.webp" alt="" width="34" height="34" />
    <p class="text-5xl font-bold">100</p>
  </div>
</div>
<div class="rounded-xl bg-card p-5">
  <p class="text-sm">Plan</p>
  <p class="text-3xl font-bold">Free</p>
  <a href="/teams/11111111-2222-3333-4444-555555555555/billing">Upgrade Plan</a>
  <p class="text-sm">Next Hypercredit refresh in 1 week</p>
</div>
<div class="rounded-xl bg-card p-5">
  <p class="text-sm">API Keys</p>
  <p class="mt-1 text-5xl font-bold">0</p>
</div>
</body></html>"#;

/// Signed-out marketing page served for a session cookie that no longer authenticates.
const LANDING_HTML: &str = r#"<html><head><title>Charm™ Hyper — Built for Coding Agents</title></head><body>
<p>Hypercredits per month.</p>
</body></html>"#;

fn test_provider(base_url: &str) -> HyperProvider {
    let client = Client::builder()
        .redirect(Policy::none())
        .timeout(API_TIMEOUT)
        .build()
        .unwrap();
    let dashboard_client = crate::core::credentialed_http_client_builder()
        .timeout(API_TIMEOUT)
        .build()
        .unwrap();
    HyperProvider::with_clients(
        format!("{base_url}/v1/credits"),
        format!("{base_url}/"),
        client,
        dashboard_client,
    )
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

/// A browser-session request: `GET /` with only the cookie, never a bearer key.
async fn session_mock(
    server: &mut ServerGuard,
    cookie: &str,
    status: usize,
    body: &str,
    hits: usize,
) -> Mock {
    server
        .mock("GET", "/")
        .match_header("accept", "text/html")
        .match_header("cookie", cookie)
        .match_header("authorization", Matcher::Missing)
        .with_status(status)
        .with_header("content-type", "text/html; charset=utf-8")
        .with_body(body)
        .expect(hits)
        .create_async()
        .await
}

/// The team dashboard a signed-in session reaches after a same-origin redirect.
async fn team_dashboard_mock(server: &mut ServerGuard, cookie: &str, hits: usize) -> Mock {
    server
        .mock("GET", DASHBOARD_PATH)
        .match_header("accept", "text/html")
        .match_header("cookie", cookie)
        .match_header("authorization", Matcher::Missing)
        .with_status(200)
        .with_header("content-type", "text/html; charset=utf-8")
        .with_body(DASHBOARD_HTML)
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
    server
        .mock("GET", Matcher::Any)
        .expect(0)
        .create_async()
        .await
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

fn dashboard_reply(status: u16, url: &str, body: &str) -> DashboardReply {
    DashboardReply {
        status: StatusCode::from_u16(status).unwrap(),
        url: url.to_string(),
        body: body.as_bytes().to_vec(),
    }
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
async fn session_dashboard_wins_over_key_without_a_credits_request() {
    let mut server = Server::new_async().await;
    let session = session_mock(
        &mut server,
        "JSESSIONID=fixture-session",
        200,
        DASHBOARD_HTML,
        1,
    )
    .await;
    let api = api_mock(&mut server, 200, BALANCE_BODY, 0).await;
    let provider = test_provider(&server.url());

    let result = provider
        .fetch_with_key(
            &context(SourceMode::Auto, Some("JSESSIONID=fixture-session")),
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
    let details = result.display_details();
    assert_eq!(
        details.iter().map(|d| d.title()).collect::<Vec<_>>(),
        ["Balance", "Plan", "Refresh"]
    );
    assert_eq!(
        details.iter().map(|d| d.value()).collect::<Vec<_>>(),
        ["100 HC", "Free", "1 week"]
    );
    assert!(
        details
            .iter()
            .all(|d| d.section_title() == Some("Hypercredits"))
    );
}

#[tokio::test]
async fn session_follows_same_origin_redirect_to_the_team_dashboard() {
    let mut server = Server::new_async().await;
    let root = server
        .mock("GET", "/")
        .match_header("accept", "text/html")
        .match_header("cookie", "session=fixture-session")
        .match_header("authorization", Matcher::Missing)
        .with_status(302)
        .with_header("location", DASHBOARD_PATH)
        .expect(1)
        .create_async()
        .await;
    let dashboard = team_dashboard_mock(&mut server, "session=fixture-session", 1).await;
    let api = api_mock(&mut server, 200, BALANCE_BODY, 0).await;
    let provider = test_provider(&server.url());

    let result = provider
        .fetch_with_key(
            &context(SourceMode::Auto, Some("session=fixture-session")),
            Some(KEY),
        )
        .await
        .unwrap();

    root.assert_async().await;
    dashboard.assert_async().await;
    api.assert_async().await;
    assert_eq!(result.source_label, "web");
    assert_eq!(result.display_details()[0].value(), "100 HC");
}

#[tokio::test]
async fn api_source_bypasses_the_session_cookie() {
    let mut server = Server::new_async().await;
    let session = session_mock(&mut server, "session=unused", 200, DASHBOARD_HTML, 0).await;
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
    for (status, body) in [
        (401, DASHBOARD_HTML),
        (403, DASHBOARD_HTML),
        (302, LANDING_HTML),
        (200, LANDING_HTML),
    ] {
        let mut server = Server::new_async().await;
        let session = session_mock(
            &mut server,
            "session=fixture-rejected-session",
            status,
            body,
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
    for (status, body) in [
        (401, DASHBOARD_HTML),
        (403, DASHBOARD_HTML),
        (302, LANDING_HTML),
        (200, LANDING_HTML),
    ] {
        let mut server = Server::new_async().await;
        let session = session_mock(
            &mut server,
            "session=fixture-rejected-session",
            status,
            body,
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
    let session = session_mock(&mut server, "session=fixture", 401, DASHBOARD_HTML, 1).await;
    let provider = test_provider(&server.url());

    let error = provider
        .fetch_with_key(&context(SourceMode::Auto, Some("session=fixture")), None)
        .await
        .unwrap_err();

    session.assert_async().await;
    assert!(matches!(&error, ProviderError::Other(message) if message == SESSION_EXPIRED));
}

#[tokio::test]
async fn dashboard_without_a_balance_card_fails_closed_instead_of_guessing() {
    let body = format!(
        r#"<html><body><a href="/teams/{TEAM_ID}/dashboard">Dashboard</a>
<p class="text-sm">Hypercredits Available</p></body></html>"#
    );
    let mut server = Server::new_async().await;
    let session = session_mock(&mut server, "session=fixture", 200, &body, 1).await;
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
        "unexpected error: {error:?}"
    );
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

        // A rejected session (signed-out page) falls back to the key, and the
        // invalid key response is then final.
        let mut server = Server::new_async().await;
        let session = session_mock(&mut server, "session=fixture", 200, LANDING_HTML, 1).await;
        let api = api_mock(&mut server, 200, body, 1).await;
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

#[test]
fn dashboard_balance_parses_comma_separated_amounts_and_optional_rows() {
    let page = r#"<p class="text-sm">Hypercredits Available</p>
<p class="text-5xl font-bold">1,250.5</p>
<p class="text-sm">Plan</p>
<p class="text-3xl font-bold">
   Pro   Team
</p>
<p class="text-sm">Next Hypercredit refresh in 3 days</p>"#;
    let result = dashboard_balance(page).unwrap();
    let details = result.display_details();
    assert_eq!(
        details.iter().map(|d| d.title()).collect::<Vec<_>>(),
        ["Balance", "Plan", "Refresh"]
    );
    assert_eq!(
        details[0].value(),
        format!("{} HC", format::number(1250.5, 2))
    );
    assert_eq!(details[1].value(), "Pro Team");
    assert_eq!(details[2].value(), "3 days");

    // Plan and refresh are optional; the balance alone is a valid dashboard.
    let bare =
        r#"<p class="text-sm">Hypercredits Available</p><p class="text-5xl font-bold">0</p>"#;
    let result = dashboard_balance(bare).unwrap();
    assert_eq!(result.display_details().len(), 1);
    assert_eq!(result.display_details()[0].title(), "Balance");
}

#[test]
fn dashboard_balance_rejects_a_missing_or_unusable_amount() {
    for page in [
        r#"<p class="text-sm">Hypercredits Available</p><p class="text-5xl font-bold">abc</p>"#,
        r#"<p class="text-sm">Hypercredits Available</p><p class="text-5xl font-bold"></p>"#,
        r#"<p class="text-sm">Hypercredits Available</p>"#,
    ] {
        let error = dashboard_balance(page).unwrap_err();
        assert!(
            matches!(&error, ProviderError::Parse(message) if message == DASHBOARD_BALANCE_INVALID),
            "{page}: {error:?}"
        );
    }
}

#[test]
fn dashboard_statuses_follow_the_upstream_classification() {
    // Only a 200 on the team dashboard (by final URL or body) is a session.
    let team_url = format!("https://hyper.charm.land{DASHBOARD_PATH}");
    let by_url = classify_dashboard(&dashboard_reply(200, &team_url, DASHBOARD_HTML))
        .unwrap()
        .unwrap();
    assert_eq!(by_url.display_details()[0].value(), "100 HC");
    let by_body = classify_dashboard(&dashboard_reply(
        200,
        "https://hyper.charm.land/",
        DASHBOARD_HTML,
    ))
    .unwrap()
    .unwrap();
    assert_eq!(by_body.display_details()[0].value(), "100 HC");
    // A team page without a balance card is a parse failure, not a rejection.
    assert!(matches!(
        classify_dashboard(&dashboard_reply(200, &team_url, "<p>no card</p>")),
        Err(ProviderError::Parse(_))
    ));
}

#[test]
fn dashboard_statuses_reject_or_fail_as_upstream_does() {
    let root = "https://hyper.charm.land/";
    // Signed-out page, redirect and auth failures reject the session.
    for (status, body) in [
        (200, LANDING_HTML),
        (302, ""),
        (401, DASHBOARD_HTML),
        (403, DASHBOARD_HTML),
    ] {
        assert!(
            classify_dashboard(&dashboard_reply(status, root, body))
                .unwrap()
                .is_none(),
            "status {status}"
        );
    }
    // Rate limits, server errors and other statuses are final.
    let error = classify_dashboard(&dashboard_reply(429, root, "")).unwrap_err();
    assert_eq!(message(&error), DASHBOARD_RATE_LIMITED);
    let error = classify_dashboard(&dashboard_reply(503, root, "")).unwrap_err();
    assert_eq!(message(&error), "Charm Hyper dashboard error: HTTP 503");
    let error = classify_dashboard(&dashboard_reply(404, root, "")).unwrap_err();
    assert_eq!(message(&error), "Charm Hyper dashboard returned HTTP 404");
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
        (429, DASHBOARD_RATE_LIMITED.to_string()),
        (503, "Charm Hyper dashboard error: HTTP 503".to_string()),
    ] {
        let mut server = Server::new_async().await;
        let session = session_mock(&mut server, "session=fixture", status, "", 1).await;
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
    // An oversized dashboard body fails the session request the way upstream's
    // response-size limit does; Auto then tries the key.
    let mut server = Server::new_async().await;
    let session = server
        .mock("GET", "/")
        .match_header("cookie", "session=fixture")
        .match_header("authorization", Matcher::Missing)
        .with_status(200)
        .with_header("content-type", "text/html; charset=utf-8")
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

/// Browser sessions as the lazy scan yields them: Edge's stale cookie first,
/// then Brave's signed-in one. No real browser store is read.
fn browser_sessions<'a>(
    log: &'a std::sync::Mutex<Vec<&'static str>>,
) -> impl Iterator<Item = String> + Send + 'a {
    [
        "session=stale-edge",
        "session=valid-brave",
        "session=never-read",
    ]
    .into_iter()
    .inspect(move |cookie| log.lock().unwrap().push(cookie))
    .map(str::to_string)
}

#[tokio::test]
async fn rejected_browser_session_moves_on_to_the_next_browser() {
    let mut server = Server::new_async().await;
    let stale = session_mock(&mut server, "session=stale-edge", 200, LANDING_HTML, 1).await;
    let valid = session_mock(&mut server, "session=valid-brave", 200, DASHBOARD_HTML, 1).await;
    let api = api_mock(&mut server, 200, BALANCE_BODY, 0).await;
    let provider = test_provider(&server.url());
    let ctx = FetchContext {
        source_mode: SourceMode::Auto,
        ..FetchContext::default()
    };
    let read = std::sync::Mutex::new(Vec::new());

    let result = provider
        .fetch_with_sessions(&ctx, Some(KEY), || browser_sessions(&read))
        .await
        .unwrap();

    stale.assert_async().await;
    valid.assert_async().await;
    api.assert_async().await;
    assert_eq!(result.source_label, "web");
    assert_eq!(
        *read.lock().unwrap(),
        ["session=stale-edge", "session=valid-brave"],
        "the browser after the accepted session must not be read"
    );
}

#[tokio::test]
async fn api_source_never_asks_for_browser_sessions() {
    let mut server = Server::new_async().await;
    let api = api_mock(&mut server, 200, BALANCE_BODY, 1).await;
    let provider = test_provider(&server.url());
    let ctx = FetchContext {
        source_mode: SourceMode::OAuth,
        ..FetchContext::default()
    };

    provider
        .fetch_with_sessions(&ctx, Some(KEY), || -> std::vec::IntoIter<String> {
            panic!("the API source must not read a browser")
        })
        .await
        .unwrap();

    api.assert_async().await;
}

#[tokio::test]
async fn every_rejected_browser_session_reports_an_expired_session_in_web() {
    let mut server = Server::new_async().await;
    let stale = server
        .mock("GET", "/")
        .match_header("authorization", Matcher::Missing)
        .with_status(401)
        .expect(2)
        .create_async()
        .await;
    let provider = test_provider(&server.url());
    let ctx = FetchContext {
        source_mode: SourceMode::Web,
        ..FetchContext::default()
    };

    let error = provider
        .fetch_with_sessions(&ctx, None, || {
            ["session=a", "session=b"].into_iter().map(str::to_string)
        })
        .await
        .unwrap_err();

    stale.assert_async().await;
    assert_eq!(message(&error), SESSION_EXPIRED);
}
