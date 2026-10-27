//! Fixture tests ported from upstream `traecn.js` behavior. Every fetch runs
//! against a local mock of the Trae API with a pasted Cookie header or an
//! in-memory browser store, so no test reads a real browser or account.

use super::*;
use crate::browser::detection::BrowserType;
use crate::browser::session_scan::test_support::FakeCookieStore;
use mockito::{Matcher, Mock, Server, ServerGuard};

/// Where Trae keeps automatic browser sessions (upstream #4732).
const PARENT_DOMAIN: &str = "trae.cn";
const COOKIE: &str = "trae_session=synthetic-1";
const OTHER_COOKIE: &str = "trae_session=synthetic-2";
const TOKEN: &str = "synthetic-token";
const TOKEN_BODY: &str = r#"{"Result":{"Token":"synthetic-token"}}"#;
const ENTITLEMENTS: &str = r#"{"is_credits_billing":true,
 "usage_summary":{"total_amount":10000,"consumed_amount":2000},
 "user_entitlement_pack_list":[
   {"group_type":4,"status":1,"display_desc":"Synthetic Add-on"},
   {"group_type":0,"status":1,"display_desc":"  Fixture Pro  "}]}"#;
const SECRET_BODY: &str = "synthetic-secret-echo";

fn manual(cookie: &str) -> FetchContext {
    FetchContext {
        source_mode: SourceMode::Auto,
        manual_cookie_header: Some(cookie.to_owned()),
        ..FetchContext::default()
    }
}

/// Token exchange then entitlement read, both bound to one Cookie header.
async fn exchange(
    server: &mut ServerGuard,
    cookie: &str,
    token: (usize, &str),
    usage: (usize, &str),
) -> Vec<Mock> {
    vec![
        server
            .mock("POST", TOKEN_PATH)
            .match_header("cookie", cookie)
            .match_header("origin", WEB_ORIGIN)
            .match_body(Matcher::Json(json!({})))
            .with_status(token.0)
            .with_header("content-type", "application/json")
            .with_body(token.1)
            .create_async()
            .await,
        server
            .mock("POST", ENTITLEMENTS_PATH)
            .match_header("cookie", cookie)
            .match_header("authorization", format!("Cloud-IDE-JWT {TOKEN}").as_str())
            .match_body(Matcher::Json(
                json!({"require_usage": true, "full_data": true}),
            ))
            .with_status(usage.0)
            .with_header("content-type", "application/json")
            .with_body(usage.1)
            .create_async()
            .await,
    ]
}

async fn fetch(
    server: &ServerGuard,
    ctx: &FetchContext,
    store: &FakeCookieStore,
) -> Result<ProviderFetchResult, ProviderError> {
    TraeCnProvider::with_base_url(&server.url())
        .fetch_web(ctx, store)
        .await
}

fn rows_in<'a>(result: &'a ProviderFetchResult, section: &str) -> Vec<(&'a str, &'a str)> {
    result
        .display_details
        .iter()
        .filter(|detail| detail.section_title() == Some(section))
        .map(|detail| (detail.title(), detail.value()))
        .collect()
}

fn assert_malformed(result: Result<ProviderFetchResult, ProviderError>) {
    assert!(
        matches!(&result, Err(ProviderError::Parse(message)) if message == MALFORMED),
        "expected malformed response, got {result:?}"
    );
}

#[test]
fn metadata_and_source_contract_match_upstream_descriptor() {
    let provider = TraeCnProvider::new();
    assert_eq!(provider.id(), ProviderId::TraeCn);
    let metadata = provider.metadata();
    assert_eq!(metadata.display_name, "Trae CN");
    assert_eq!(metadata.session_label, "Credits");
    assert_eq!(metadata.weekly_label, "Window");
    assert_eq!(
        metadata.dashboard_url,
        Some("https://www.trae.cn/dashboard#usage")
    );
    assert!(!metadata.default_enabled);
    assert_eq!(
        provider.available_sources(),
        vec![SourceMode::Auto, SourceMode::Web]
    );
    assert!(provider.supports_web());
    assert!(provider.owns_browser_cookie_resolution());
    assert!(matches!(
        provider.manual_empty_cookie_policy(),
        ManualEmptyCookiePolicy::FailClosedWeb
    ));
    // Manual cookies stay on the API origin. Automatic sessions and Settings
    // browser import both read the parent `trae.cn` domain (#4732) and keep
    // only the cookies a browser sends to the API origin.
    assert_eq!(ProviderId::TraeCn.cookie_domain(), Some("api.trae.cn"));
    assert_eq!(provider.cookie_request_scope(), Some(COOKIE_SCOPE));
    assert_eq!(COOKIE_SCOPE.scan_domain, PARENT_DOMAIN);
    assert_eq!(
        ProviderId::from_cli_name("traecn"),
        Some(ProviderId::TraeCn)
    );
    assert_eq!(
        ProviderId::from_cli_name("trae-cn"),
        Some(ProviderId::TraeCn)
    );
}

#[tokio::test]
async fn cli_and_oauth_sources_never_reach_the_network() {
    let provider = TraeCnProvider::new();
    let cli = FetchContext {
        source_mode: SourceMode::Cli,
        ..FetchContext::default()
    };
    assert!(matches!(
        provider.fetch_usage(&cli).await,
        Err(ProviderError::Other(message)) if message == COOKIES_OFF
    ));
    let oauth = FetchContext {
        source_mode: SourceMode::OAuth,
        ..FetchContext::default()
    };
    assert!(matches!(
        provider.fetch_usage(&oauth).await,
        Err(ProviderError::UnsupportedSource(SourceMode::OAuth))
    ));
}

#[test]
fn plan_is_the_active_group_zero_pack_and_is_trimmed() {
    let payload: Value = serde_json::from_str(
        r#"{"is_credits_billing":true,
           "usage_summary":{"total_amount":10,"consumed_amount":1},
           "user_entitlement_pack_list":[
             {"group_type":0,"status":0,"display_desc":"Lapsed"},
             {"group_type":4,"status":1,"display_desc":"Add-on"},
             {"group_type":0,"display_desc":"  Fixture Pro  "}]}"#,
    )
    .unwrap();
    let entitlements = parse_entitlements(&payload).unwrap();
    assert_eq!(entitlements.plan.as_deref(), Some("Fixture Pro"));
    assert_eq!(entitlements.total, 10.0);
    assert_eq!(entitlements.used, 1.0);

    let blank: Value = serde_json::from_str(
        r#"{"is_credits_billing":true,"usage_summary":{"total_amount":10,"consumed_amount":1},
           "user_entitlement_pack_list":[{"group_type":0,"status":1,"display_desc":"   "}]}"#,
    )
    .unwrap();
    assert_eq!(parse_entitlements(&blank).unwrap().plan, None);
}

#[tokio::test]
async fn entitlement_usage_fills_the_gauge_and_credits_details() {
    let mut server = Server::new_async().await;
    let mocks = exchange(&mut server, COOKIE, (200, TOKEN_BODY), (200, ENTITLEMENTS)).await;
    let store = FakeCookieStore::default();
    let result = fetch(&server, &manual(COOKIE), &store).await.unwrap();
    for mock in mocks {
        mock.assert_async().await;
    }

    assert_eq!(result.usage.primary.used_percent, 20.0);
    assert_eq!(result.usage.primary.reset_description, None);
    assert_eq!(result.usage.login_method.as_deref(), Some("Fixture Pro"));
    assert!(result.cost.is_none());
    assert_eq!(
        rows_in(&result, CREDITS_SECTION),
        vec![
            ("Used", "2,000"),
            ("Total", "10,000"),
            ("Remaining", "8,000"),
        ]
    );
    // The manual header is the only session, so no browser is read.
    assert!(store.reads().is_empty());
}

#[tokio::test]
async fn zero_allowance_keeps_details_without_a_gauge() {
    let mut server = Server::new_async().await;
    let _mocks = exchange(
        &mut server,
        COOKIE,
        (200, TOKEN_BODY),
        (
            200,
            r#"{"is_credits_billing":true,"usage_summary":{"total_amount":0,"consumed_amount":0},"user_entitlement_pack_list":[]}"#,
        ),
    )
    .await;
    let result = fetch(&server, &manual(COOKIE), &FakeCookieStore::default())
        .await
        .unwrap();
    assert_eq!(
        result.usage.primary.reset_description.as_deref(),
        Some(NO_ALLOWANCE)
    );
    assert_eq!(result.usage.login_method, None);
    assert_eq!(
        rows_in(&result, CREDITS_SECTION),
        vec![("Used", "0"), ("Total", "0"), ("Remaining", "0")]
    );
}

#[tokio::test]
async fn overspend_clamps_the_gauge_and_keeps_the_true_numbers() {
    let mut server = Server::new_async().await;
    let _mocks = exchange(
        &mut server,
        COOKIE,
        (200, TOKEN_BODY),
        (
            200,
            r#"{"is_credits_billing":true,"usage_summary":{"total_amount":10000,"consumed_amount":12500}}"#,
        ),
    )
    .await;
    let result = fetch(&server, &manual(COOKIE), &FakeCookieStore::default())
        .await
        .unwrap();
    assert_eq!(result.usage.primary.used_percent, 100.0);
    assert_eq!(
        rows_in(&result, CREDITS_SECTION),
        vec![("Used", "12,500"), ("Total", "10,000"), ("Remaining", "0")]
    );
}

#[test]
fn malformed_entitlements_fail_closed() {
    for body in [
        r#"{"is_credits_billing":false,"usage_summary":{"total_amount":10,"consumed_amount":1}}"#,
        r#"{"is_credits_billing":"true","usage_summary":{"total_amount":10,"consumed_amount":1}}"#,
        r#"{"is_credits_billing":true,"usage_summary":{"total_amount":"10000","consumed_amount":2000}}"#,
        r#"{"is_credits_billing":true,"usage_summary":{"total_amount":10000,"consumed_amount":-1}}"#,
        r#"{"is_credits_billing":true,"usage_summary":{"total_amount":10000}}"#,
        r#"{"is_credits_billing":true}"#,
        r#"[]"#,
    ] {
        let payload: Value = serde_json::from_str(body).unwrap();
        assert!(
            matches!(parse_entitlements(&payload), Err(ProviderError::Parse(message)) if message == MALFORMED),
            "accepted {body}"
        );
    }
}

#[tokio::test]
async fn malformed_bodies_are_rejected_not_reported_as_zero_usage() {
    let cases: [(&str, &str); 4] = [
        (TOKEN_BODY, "not-json"),
        (
            TOKEN_BODY,
            r#"{"is_credits_billing":false,"usage_summary":{"total_amount":10,"consumed_amount":1}}"#,
        ),
        (r#"{"Result":{}}"#, ENTITLEMENTS),
        (r#"{"Result":{"Token":"   "}}"#, ENTITLEMENTS),
    ];
    for (token_body, usage_body) in cases {
        let mut server = Server::new_async().await;
        let _mocks = exchange(&mut server, COOKIE, (200, token_body), (200, usage_body)).await;
        let result = fetch(&server, &manual(COOKIE), &FakeCookieStore::default()).await;
        assert_malformed(result);
    }
}

#[tokio::test]
async fn rejected_token_exchange_reports_an_expired_session() {
    for status in [401, 403, 302] {
        let mut server = Server::new_async().await;
        let _token = server
            .mock("POST", TOKEN_PATH)
            .match_header("cookie", COOKIE)
            .with_status(status)
            .with_body("{}")
            .create_async()
            .await;
        let provider = TraeCnProvider::with_base_url(&server.url());
        let error = provider
            .fetch_web(&manual(COOKIE), &FakeCookieStore::default())
            .await
            .unwrap_err();
        assert!(matches!(&error, ProviderError::Other(message) if message == SESSION_EXPIRED));
        assert_eq!(
            provider.error_state_kind(&error),
            ProviderStateKind::ExpiredSession
        );
    }
}

#[tokio::test]
async fn rejected_entitlement_read_reports_an_expired_session() {
    for (status, body) in [(403, "{}"), (200, r#"{"code":1001,"msg":"synthetic"}"#)] {
        let mut server = Server::new_async().await;
        let _mocks = exchange(&mut server, COOKIE, (200, TOKEN_BODY), (status, body)).await;
        let result = fetch(&server, &manual(COOKIE), &FakeCookieStore::default()).await;
        assert!(
            matches!(&result, Err(ProviderError::Other(message)) if message == SESSION_EXPIRED),
            "expected expired session, got {result:?}"
        );
    }
}

#[tokio::test]
async fn a_rejected_profile_advances_to_the_next_browser_session() {
    let mut server = Server::new_async().await;
    let _first = exchange(&mut server, COOKIE, (401, "{}"), (200, ENTITLEMENTS)).await;
    let _second = exchange(
        &mut server,
        OTHER_COOKIE,
        (200, TOKEN_BODY),
        (200, ENTITLEMENTS),
    )
    .await;
    let store = FakeCookieStore::default()
        .host_cookies(
            BrowserType::Chrome,
            PARENT_DOMAIN,
            &[(".trae.cn", "trae_session", "synthetic-1")],
        )
        .host_cookies(
            BrowserType::Edge,
            PARENT_DOMAIN,
            &[(".trae.cn", "trae_session", "synthetic-2")],
        );
    let result = fetch(&server, &FetchContext::default(), &store)
        .await
        .unwrap();
    assert_eq!(result.usage.primary.used_percent, 20.0);
    assert_eq!(store.reads().len(), 2);
}

#[tokio::test]
async fn all_rejected_browser_sessions_report_an_expired_session() {
    let mut server = Server::new_async().await;
    let _first = exchange(&mut server, COOKIE, (401, "{}"), (200, ENTITLEMENTS)).await;
    let _second = exchange(&mut server, OTHER_COOKIE, (403, "{}"), (200, ENTITLEMENTS)).await;
    let store = FakeCookieStore::default()
        .host_cookies(
            BrowserType::Chrome,
            PARENT_DOMAIN,
            &[(".trae.cn", "trae_session", "synthetic-1")],
        )
        .host_cookies(
            BrowserType::Edge,
            PARENT_DOMAIN,
            &[(".trae.cn", "trae_session", "synthetic-2")],
        );
    let provider = TraeCnProvider::with_base_url(&server.url());
    let error = provider
        .fetch_web(&FetchContext::default(), &store)
        .await
        .unwrap_err();
    assert!(matches!(&error, ProviderError::Other(message) if message == SESSION_EXPIRED));
    assert_eq!(
        provider.error_state_kind(&error),
        ProviderStateKind::ExpiredSession
    );
    assert_eq!(store.reads().len(), 2);
}

#[tokio::test]
async fn transient_failures_keep_the_session_and_stop_the_walk() {
    for (status, expected) in [(429, RATE_LIMITED), (503, UNAVAILABLE), (408, UNAVAILABLE)] {
        let mut server = Server::new_async().await;
        let _token = server
            .mock("POST", TOKEN_PATH)
            .match_header("cookie", COOKIE)
            .with_status(status)
            .with_body("{}")
            .create_async()
            .await;
        // A second browser would only be tried after a rejected session.
        let store = FakeCookieStore::default()
            .host_cookies(
                BrowserType::Chrome,
                PARENT_DOMAIN,
                &[(".trae.cn", "trae_session", "synthetic-1")],
            )
            .host_cookies(
                BrowserType::Edge,
                PARENT_DOMAIN,
                &[(".trae.cn", "trae_session", "synthetic-2")],
            );
        let result = TraeCnProvider::with_base_url(&server.url())
            .fetch_web(&FetchContext::default(), &store)
            .await;
        assert!(
            matches!(&result, Err(ProviderError::Other(message)) if message == expected),
            "status {status} gave {result:?}"
        );
        assert_eq!(store.reads().len(), 1);
    }
}

#[tokio::test]
async fn other_http_failures_name_the_status_without_echoing_the_body() {
    let mut server = Server::new_async().await;
    let _token = server
        .mock("POST", TOKEN_PATH)
        .match_header("cookie", COOKIE)
        .with_status(404)
        .with_body(SECRET_BODY)
        .create_async()
        .await;
    let result = fetch(&server, &manual(COOKIE), &FakeCookieStore::default()).await;
    match result {
        Err(ProviderError::Other(message)) => {
            assert_eq!(
                message,
                "Trae CN request failed (HTTP 404). Try refreshing later."
            );
            assert!(!message.contains(SECRET_BODY));
        }
        other => panic!("expected Other, got {other:?}"),
    }
}

#[tokio::test]
async fn pasted_header_is_the_only_session_and_is_normalized() {
    let mut server = Server::new_async().await;
    let mocks = exchange(&mut server, COOKIE, (200, TOKEN_BODY), (200, ENTITLEMENTS)).await;
    let store = FakeCookieStore::default().host_cookies(
        BrowserType::Chrome,
        PARENT_DOMAIN,
        &[(".trae.cn", "trae_session", "synthetic-2")],
    );
    let result = fetch(&server, &manual(&format!("Cookie: {COOKIE}")), &store)
        .await
        .unwrap();
    for mock in mocks {
        mock.assert_async().await;
    }
    assert_eq!(result.usage.primary.used_percent, 20.0);
    assert!(store.reads().is_empty());
}

#[tokio::test]
async fn no_browser_session_asks_the_user_to_sign_in() {
    let server = Server::new_async().await;
    let result = fetch(
        &server,
        &FetchContext::default(),
        &FakeCookieStore::default(),
    )
    .await;
    assert!(matches!(
        result,
        Err(ProviderError::BrowserSignInRequired { sign_in_url, .. }) if sign_in_url == SIGN_IN_URL
    ));
}

#[tokio::test]
async fn an_empty_manual_source_never_falls_back_to_a_browser() {
    let server = Server::new_async().await;
    let store = FakeCookieStore::default().host_cookies(
        BrowserType::Chrome,
        PARENT_DOMAIN,
        &[(".trae.cn", "trae_session", "synthetic-1")],
    );
    let ctx = FetchContext {
        source_mode: SourceMode::Auto,
        manual_cookie_missing: true,
        ..FetchContext::default()
    };
    let result = fetch(&server, &ctx, &store).await;
    assert!(matches!(
        result,
        Err(ProviderError::BrowserSignInRequired { .. })
    ));
    assert!(store.reads().is_empty());
}

#[tokio::test]
async fn automatic_scan_reads_the_parent_domain_and_keeps_only_api_bound_cookies() {
    let mut server = Server::new_async().await;
    let mocks = exchange(&mut server, COOKIE, (200, TOKEN_BODY), (200, ENTITLEMENTS)).await;
    // Only the `.trae.cn` cookie is sent to api.trae.cn. The host-only parent
    // cookie and the sibling www host are not, so Chrome yields no session.
    let store = FakeCookieStore::default()
        .host_cookies(
            BrowserType::Chrome,
            PARENT_DOMAIN,
            &[
                ("trae.cn", "parent_host_only", "1"),
                ("www.trae.cn", "www_only", "2"),
            ],
        )
        .host_cookies(
            BrowserType::Edge,
            PARENT_DOMAIN,
            &[
                (".trae.cn", "trae_session", "synthetic-1"),
                ("www.trae.cn", "www_only", "3"),
            ],
        );
    let result = fetch(&server, &FetchContext::default(), &store)
        .await
        .unwrap();
    for mock in mocks {
        mock.assert_async().await;
    }
    assert_eq!(result.usage.primary.used_percent, 20.0);
    assert_eq!(
        store.reads(),
        vec![
            (BrowserType::Chrome, PARENT_DOMAIN.to_string()),
            (BrowserType::Edge, PARENT_DOMAIN.to_string()),
        ]
    );
    // Requests and manual cookies stay on the API origin.
    assert_eq!(Some(COOKIE_SCOPE.host), ProviderId::TraeCn.cookie_domain());
}

#[tokio::test]
async fn parent_host_only_cookie_is_not_sent_to_the_api_origin() {
    let server = Server::new_async().await;
    let store = FakeCookieStore::default().host_cookies(
        BrowserType::Chrome,
        PARENT_DOMAIN,
        &[("trae.cn", "trae_session", "synthetic-1")],
    );
    // No request may leave: a sent cookie would reach the mock server, which
    // has no route and answers with a non-sign-in error.
    let result = fetch(&server, &FetchContext::default(), &store).await;
    assert!(matches!(
        result,
        Err(ProviderError::BrowserSignInRequired { .. })
    ));
    assert_eq!(store.reads().len(), 1);
}

#[test]
fn amounts_must_be_non_negative_json_numbers() {
    assert_eq!(amount(Some(&json!(0))).unwrap(), 0.0);
    assert_eq!(amount(Some(&json!(12.5))).unwrap(), 12.5);
    for value in [json!("1"), json!(-0.5), json!(null), json!([])] {
        assert!(amount(Some(&value)).is_err(), "accepted {value}");
    }
    assert!(amount(None).is_err());
}

#[test]
fn zero_allowance_is_informational_and_hidden_from_the_card() {
    let result = build_result(&Entitlements {
        used: 0.0,
        total: 0.0,
        plan: None,
    });
    assert!(result.usage.primary.is_informational);
    assert_eq!(
        result.usage.primary.reset_description.as_deref(),
        Some(NO_ALLOWANCE)
    );
}

#[test]
fn base_url_override_accepts_https_and_loopback_http() {
    assert_eq!(
        validated_base_url("https://api.trae.cn").as_deref(),
        Some("https://api.trae.cn")
    );
    assert_eq!(
        validated_base_url("http://127.0.0.1:4011/").as_deref(),
        Some("http://127.0.0.1:4011")
    );
    assert_eq!(
        validated_base_url("http://localhost:9").as_deref(),
        Some("http://localhost:9")
    );
    assert_eq!(
        validated_base_url("http://[::1]:9").as_deref(),
        Some("http://[::1]:9")
    );
}

#[test]
fn base_url_override_rejects_remote_plain_http_and_unsafe_forms() {
    for raw in [
        "http://api.trae.cn",
        "http://localhost.attacker.test",
        "ftp://127.0.0.1",
        "https://user:pass@api.trae.cn",
        "https://api.trae.cn?x=1",
        "https://api.trae.cn#frag",
        "http://127.0.0.1%2f.attacker.test",
        "not a url",
        "",
    ] {
        assert!(
            validated_base_url(raw).is_none(),
            "{raw} should be rejected"
        );
    }
}

#[test]
fn base_url_override_falls_back_to_production_when_unset_or_invalid() {
    assert_eq!(base_url_from_override(None), BASE_URL);
    assert_eq!(base_url_from_override(Some("   ")), BASE_URL);
    assert_eq!(
        base_url_from_override(Some("http://attacker.example.com")),
        BASE_URL
    );
    assert_eq!(
        base_url_from_override(Some("'http://127.0.0.1:4011'")),
        "http://127.0.0.1:4011"
    );
}
