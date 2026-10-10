use super::*;
use crate::browser::cookies::Cookie;
use crate::providers::test_support::{mock_response_expect, mock_status_expect};
use chrono::TimeZone;
use parse::parse_credits;

const CREDITS: &str = r#"{
  "remaining_balance_credits": "125",
  "total_balance_credits": "500",
  "next_credits_at": "2026-10-18T00:00:00.000Z",
  "can_top_up": true,
  "can_upgrade_plan": true,
  "funding_subscription": {
    "tier": "pro",
    "source": "personal",
    "provider": "stripe",
    "status": "active",
    "canceled": false
  }
}"#;
const FIXTURE_COOKIE: &str = "__raycast_session=fixture-session; csrf_token=fixture-csrf";

fn result(body: &str) -> ProviderFetchResult {
    parse_credits(body)
        .expect("fixture parses")
        .into_result("web")
}

fn cookie(name: &str, value: &str, domain: &str) -> Cookie {
    Cookie {
        name: name.to_string(),
        value: value.to_string(),
        domain: domain.to_string(),
        path: "/".to_string(),
        expires: None,
        is_secure: true,
        is_http_only: true,
    }
}

fn detail_pairs(result: &ProviderFetchResult) -> Vec<(&str, &str)> {
    result
        .display_details()
        .iter()
        .map(|row| (row.title(), row.value()))
        .collect()
}

fn ctx(source_mode: SourceMode, manual: Option<&str>) -> FetchContext {
    FetchContext {
        source_mode,
        manual_cookie_header: manual.map(str::to_string),
        ..FetchContext::default()
    }
}

// ---- parsing and mapping -------------------------------------------------

#[test]
fn monthly_credits_become_one_meter_with_the_remaining_balance() {
    let result = result(CREDITS);
    let primary = &result.usage.primary;
    assert_eq!(primary.used_percent, 75.0);
    assert_eq!(primary.window_minutes, None);
    assert!(!primary.is_informational);
    assert_eq!(
        primary.resets_at,
        Some(chrono::Utc.with_ymd_and_hms(2026, 10, 18, 0, 0, 0).unwrap())
    );
    assert_eq!(
        primary.reset_description.as_deref(),
        Some("125 / 500 credits left")
    );
    assert!(primary.description_is_detail);
    assert_eq!(result.usage.login_method.as_deref(), Some("Pro"));
    assert!(result.usage.subscription.is_none());
    assert!(result.display_details().is_empty());
    assert!(result.cost.is_none());
}

#[test]
fn website_account_payload_maps_left_and_total() {
    let result = result(
        r#"{"remaining_balance_credits":"337.3751","total_balance_credits":"500.0",
            "next_credits_at":"2026-10-18T08:34:44Z",
            "funding_subscription":{"tier":"pro","status":"active"}}"#,
    );
    assert!((result.usage.primary.used_percent - 32.525).abs() < 0.01);
    assert_eq!(
        result.usage.primary.reset_description.as_deref(),
        Some("337.38 / 500 credits left")
    );
    assert!(result.display_details().is_empty());
    assert_eq!(result.usage.login_method.as_deref(), Some("Pro"));
}

#[test]
fn numeric_amounts_and_plan_labels_are_preserved() {
    let result = result(
        r#"{"remaining_balance_credits":12.5,"total_balance_credits":50,
            "funding_subscription":{"tier":"pro_plus"}}"#,
    );
    assert_eq!(result.usage.primary.used_percent, 75.0);
    assert_eq!(
        result.usage.primary.reset_description.as_deref(),
        Some("12.5 / 50 credits left")
    );
    assert_eq!(result.usage.login_method.as_deref(), Some("Pro+"));

    let max = self::result(
        r#"{"remaining_balance_credits":1,"total_balance_credits":2,"funding_subscription":{"tier":"max"}}"#,
    );
    assert_eq!(max.usage.login_method.as_deref(), Some("Max"));
    let other = self::result(
        r#"{"remaining_balance_credits":1,"total_balance_credits":2,"funding_subscription":{"tier":" team "}}"#,
    );
    assert_eq!(other.usage.login_method.as_deref(), Some("team"));
    let none = self::result(
        r#"{"remaining_balance_credits":1,"total_balance_credits":2,"funding_subscription":{"tier":""}}"#,
    );
    assert_eq!(none.usage.login_method, None);
}

#[test]
fn rollover_above_the_grant_does_not_invent_usage() {
    let result = result(r#"{"remaining_balance_credits":"750","total_balance_credits":"500"}"#);
    assert_eq!(result.usage.primary.used_percent, 0.0);
    assert_eq!(
        result.usage.primary.reset_description.as_deref(),
        Some("750 / 500 credits left")
    );
    assert!(result.display_details().is_empty());
}

#[test]
fn zero_allowance_keeps_rows_and_renewal_without_a_meter() {
    let result = result(
        r#"{"remaining_balance_credits":"0","total_balance_credits":"0",
            "next_credits_at":"2026-10-18T00:00:00.000Z","funding_subscription":{"tier":"max"}}"#,
    );
    assert!(result.usage.primary.is_informational);
    assert_eq!(result.usage.primary.resets_at, None);
    assert_eq!(
        result.usage.primary.reset_description.as_deref(),
        Some("0 credits left")
    );
    assert!(!result.usage.primary.description_is_detail);
    assert_eq!(detail_pairs(&result), vec![("Left", "0"), ("Total", "0")]);
    assert_eq!(
        result.usage.subscription.as_ref().and_then(|s| s.renews_at),
        Some(chrono::Utc.with_ymd_and_hms(2026, 10, 18, 0, 0, 0).unwrap())
    );
    assert_eq!(result.usage.login_method.as_deref(), Some("Max"));
}

#[test]
fn a_zero_or_missing_total_does_not_appear_in_the_balance_summary() {
    let zero_total = result(r#"{"remaining_balance_credits":"12.5","total_balance_credits":"0"}"#);
    assert_eq!(
        zero_total.usage.primary.reset_description.as_deref(),
        Some("12.5 credits left")
    );
    assert_eq!(
        detail_pairs(&zero_total),
        vec![("Left", "12.5"), ("Total", "0")]
    );

    let missing_total = result(r#"{"remaining_balance_credits":"12.5"}"#);
    assert_eq!(
        missing_total.usage.primary.reset_description.as_deref(),
        Some("12.5 credits left")
    );
    assert_eq!(detail_pairs(&missing_total), vec![("Left", "12.5")]);
}

#[test]
fn a_single_amount_reports_only_that_row() {
    let left = result(r#"{"remaining_balance_credits":"12.345"}"#);
    assert_eq!(detail_pairs(&left), vec![("Left", "12.35")]);
    assert!(left.usage.primary.is_informational);

    let total = result(r#"{"total_balance_credits":500}"#);
    assert_eq!(detail_pairs(&total), vec![("Total", "500")]);
}

#[test]
fn amounts_use_no_thousands_grouping_and_trim_trailing_zeros() {
    let result =
        result(r#"{"remaining_balance_credits":"1234.50","total_balance_credits":"12345"}"#);
    assert_eq!(
        result.usage.primary.reset_description.as_deref(),
        Some("1234.5 / 12345 credits left")
    );
}

#[test]
fn invalid_amounts_fail_instead_of_publishing_zero() {
    for value in [
        "true",
        "false",
        "\"NaN\"",
        "\"Infinity\"",
        "\"inf\"",
        "\"\"",
        "\"  \"",
        "[]",
        "{}",
        "1e400",
        "-1",
        "\"-0.5\"",
        "\"1,000\"",
        "\"12abc\"",
        "\"0x10\"",
    ] {
        let body = format!(r#"{{"remaining_balance_credits":{value}}}"#);
        assert!(
            matches!(parse_credits(&body), Err(ProviderError::Parse(_))),
            "{value}"
        );
        let body = format!(r#"{{"total_balance_credits":{value}}}"#);
        assert!(
            matches!(parse_credits(&body), Err(ProviderError::Parse(_))),
            "{value}"
        );
    }
}

#[test]
fn numeric_string_forms_match_the_upstream_grammar() {
    for (raw, expected) in [
        ("\"7\"", "7"),
        ("\" 7.25 \"", "7.25"),
        ("\"+3\"", "3"),
        ("\".5\"", "0.5"),
        ("\"1e2\"", "100"),
        ("\"1.5E1\"", "15"),
    ] {
        let body = format!(r#"{{"remaining_balance_credits":{raw}}}"#);
        assert_eq!(
            detail_pairs(&result(&body)),
            vec![("Left", expected)],
            "{raw}"
        );
    }
}

#[test]
fn empty_or_malformed_bodies_are_parse_failures() {
    for body in [
        "null",
        "[]",
        "{}",
        r#"{"funding_subscription":{}}"#,
        "private-response",
        "",
        r#"{"remaining_balance_credits":null,"total_balance_credits":null}"#,
    ] {
        assert!(
            matches!(parse_credits(body), Err(ProviderError::Parse(_))),
            "{body}"
        );
    }
}

#[test]
fn invalid_dates_and_funding_shapes_are_parse_failures() {
    for extra in [
        r#""next_credits_at": 1760745600"#,
        r#""next_credits_at": "soon""#,
        r#""next_credits_at": {}"#,
        r#""funding_subscription": []"#,
        r#""funding_subscription": "pro""#,
    ] {
        let body =
            format!(r#"{{"remaining_balance_credits":"1","total_balance_credits":"2",{extra}}}"#);
        assert!(
            matches!(parse_credits(&body), Err(ProviderError::Parse(_))),
            "{extra}"
        );
    }
    let nulls = result(
        r#"{"remaining_balance_credits":"1","total_balance_credits":"2","next_credits_at":null,"funding_subscription":null}"#,
    );
    assert_eq!(nulls.usage.primary.resets_at, None);
}

// ---- cookie selection ----------------------------------------------------

#[test]
fn manual_header_forwards_only_the_two_raycast_cookies() {
    assert_eq!(
        cookies::manual_header(
            "Cookie: theme=dark; csrf_token=fixture-csrf; other=1; __raycast_session=fixture-session"
        )
        .as_deref(),
        Some(FIXTURE_COOKIE)
    );
    assert_eq!(
        cookies::manual_header("__raycast_session=only").as_deref(),
        Some("__raycast_session=only")
    );
    assert_eq!(
        cookies::manual_header("__raycast_session=first; __raycast_session=second").as_deref(),
        Some("__raycast_session=first")
    );
}

#[test]
fn manual_header_requires_a_nonempty_session() {
    for raw in [
        "",
        "csrf_token=only",
        "__raycast_session=; csrf_token=fixture-csrf",
        "other=1",
        "__raycast_session=bad\r\nvalue",
    ] {
        assert_eq!(cookies::manual_header(raw), None, "{raw}");
    }
}

#[test]
fn exact_host_cookie_wins_over_a_parent_domain_cookie() {
    let jar = [
        cookie("__raycast_session", "parent", ".raycast.com"),
        cookie("csrf_token", "parent-csrf", ".raycast.com"),
        cookie("__raycast_session", "exact", "www.raycast.com"),
        cookie("csrf_token", "exact-csrf", "www.raycast.com"),
    ];
    assert_eq!(
        cookies::browser_candidates(&jar),
        vec![
            "__raycast_session=exact; csrf_token=exact-csrf".to_string(),
            "__raycast_session=parent; csrf_token=exact-csrf".to_string(),
        ]
    );
}

#[test]
fn parent_only_cookies_are_still_used_and_other_hosts_are_ignored() {
    let jar = [
        cookie("__raycast_session", "parent", "raycast.com"),
        cookie("__raycast_session", "backend", "backend.raycast.com"),
        cookie("csrf_token", "backend-csrf", "backend.raycast.com"),
        cookie("__raycast_session", "lookalike", "evilraycast.com"),
        cookie("unrelated", "x", "www.raycast.com"),
    ];
    assert_eq!(
        cookies::browser_candidates(&jar),
        vec!["__raycast_session=parent".to_string()]
    );
}

#[test]
fn browser_candidates_skip_empty_and_duplicate_sessions() {
    let jar = [
        cookie("__raycast_session", "", "www.raycast.com"),
        cookie("__raycast_session", "same", "www.raycast.com"),
        cookie("__raycast_session", "same", ".raycast.com"),
        cookie("csrf_token", "bad;value", "www.raycast.com"),
    ];
    assert_eq!(
        cookies::browser_candidates(&jar),
        vec!["__raycast_session=same".to_string()]
    );
    assert!(
        cookies::browser_candidates(&[cookie("csrf_token", "only", "www.raycast.com")]).is_empty()
    );
}

// ---- provider surface ----------------------------------------------------

#[test]
fn provider_metadata_and_sources_are_cookie_only() {
    let provider = RaycastProvider::new();
    assert_eq!(provider.id(), ProviderId::Raycast);
    assert_eq!(provider.metadata().display_name, "Raycast");
    assert_eq!(provider.metadata().session_label, "Credits");
    assert_eq!(
        provider.metadata().dashboard_url,
        Some("https://www.raycast.com/settings")
    );
    assert!(!provider.metadata().default_enabled);
    assert_eq!(
        provider.available_sources(),
        vec![SourceMode::Auto, SourceMode::Web]
    );
    assert!(provider.supports_web());
    assert!(!provider.supports_cli());
    assert!(provider.owns_browser_cookie_resolution());
    assert_eq!(
        provider.manual_empty_cookie_policy(),
        ManualEmptyCookiePolicy::FailClosedWeb
    );
    assert_eq!(ProviderId::Raycast.cookie_domain(), Some("www.raycast.com"));
    assert_eq!(
        ProviderId::from_cli_name("raycast-ai"),
        Some(ProviderId::Raycast)
    );
}

#[test]
fn credential_failures_are_classified_for_the_ui() {
    let provider = RaycastProvider::new();
    let missing = ProviderError::Other(MISSING_SESSION.to_string());
    let expired = ProviderError::Other(SESSION_EXPIRED.to_string());
    assert_eq!(
        provider.error_state_kind(&missing),
        ProviderStateKind::NeedsAuthentication
    );
    assert_eq!(
        provider.error_state_kind(&expired),
        ProviderStateKind::ExpiredSession
    );
    assert_eq!(
        provider.error_state_kind(&ProviderError::Other("other".into())),
        ProviderStateKind::Unknown
    );
}

#[test]
fn request_timeout_is_clamped_to_one_through_thirty_seconds() {
    assert_eq!(request_timeout(0), Duration::from_secs(1));
    assert_eq!(request_timeout(15), Duration::from_secs(15));
    assert_eq!(request_timeout(60), Duration::from_secs(30));
}

// ---- HTTP behavior (local mock server, no live calls) ---------------------

fn headers(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| value.to_string()).collect()
}

fn error_message(error: ProviderError) -> String {
    match error {
        ProviderError::Other(message) => message,
        other => other.to_string(),
    }
}

#[tokio::test]
async fn request_uses_the_website_route_cookie_and_site_headers() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/frontend_api/current_user/ai_credits")
        .match_header("cookie", FIXTURE_COOKIE)
        .match_header("accept", "application/json")
        .match_header("origin", "https://www.raycast.com")
        .match_header("referer", "https://www.raycast.com/settings")
        .match_header("authorization", mockito::Matcher::Missing)
        .match_header("user-agent", mockito::Matcher::Regex("Chrome/".into()))
        .with_status(200)
        .with_body(CREDITS)
        .expect(1)
        .create_async()
        .await;

    let provider = RaycastProvider::with_origin(&server.url());
    let result = provider
        .fetch_candidates(
            &headers(&[FIXTURE_COOKIE]),
            "manual",
            Duration::from_secs(5),
        )
        .await
        .expect("credits should parse");

    mock.assert_async().await;
    assert_eq!(result.source_label, "manual");
    assert_eq!(result.usage.primary.used_percent, 75.0);
}

#[tokio::test]
async fn manual_fetch_forwards_only_the_two_raycast_cookies() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/frontend_api/current_user/ai_credits")
        .match_header("cookie", FIXTURE_COOKIE)
        .with_status(200)
        .with_body(CREDITS)
        .expect(1)
        .create_async()
        .await;

    let provider = RaycastProvider::with_origin(&server.url());
    let ctx = ctx(
        SourceMode::Web,
        Some(
            "Cookie: theme=dark; csrf_token=fixture-csrf; __raycast_session=fixture-session; _ga=1",
        ),
    );
    let result = provider.fetch_usage(&ctx).await.expect("manual fetch");

    mock.assert_async().await;
    assert_eq!(result.source_label, "manual");
}

#[tokio::test]
async fn rejected_candidates_advance_within_the_same_refresh() {
    let mut server = mockito::Server::new_async().await;
    let rejected = server
        .mock("GET", "/frontend_api/current_user/ai_credits")
        .match_header("cookie", "__raycast_session=rejected-fixture-session")
        .with_status(401)
        .expect(1)
        .create_async()
        .await;
    let accepted = server
        .mock("GET", "/frontend_api/current_user/ai_credits")
        .match_header("cookie", FIXTURE_COOKIE)
        .with_status(200)
        .with_body(CREDITS)
        .expect(1)
        .create_async()
        .await;

    let provider = RaycastProvider::with_origin(&server.url());
    let result = provider
        .fetch_candidates(
            &headers(&["__raycast_session=rejected-fixture-session", FIXTURE_COOKIE]),
            "Google Chrome",
            Duration::from_secs(5),
        )
        .await
        .expect("second candidate should be accepted");

    rejected.assert_async().await;
    accepted.assert_async().await;
    assert_eq!(result.usage.primary.used_percent, 75.0);
}

#[tokio::test]
async fn all_candidates_rejected_means_the_session_expired() {
    let mut server = mockito::Server::new_async().await;
    let mock = mock_status_expect(
        &mut server,
        "GET",
        "/frontend_api/current_user/ai_credits",
        401,
        2,
    )
    .await;

    let provider = RaycastProvider::with_origin(&server.url());
    let error = provider
        .fetch_candidates(
            &headers(&["__raycast_session=a", "__raycast_session=b"]),
            "Google Chrome",
            Duration::from_secs(5),
        )
        .await
        .expect_err("all sessions rejected");

    mock.assert_async().await;
    assert_eq!(error_message(error), SESSION_EXPIRED);
}

#[tokio::test]
async fn manual_stops_after_its_single_rejected_candidate() {
    let mut server = mockito::Server::new_async().await;
    let mock = mock_status_expect(
        &mut server,
        "GET",
        "/frontend_api/current_user/ai_credits",
        401,
        1,
    )
    .await;

    let provider = RaycastProvider::with_origin(&server.url());
    let error = provider
        .fetch_usage(&ctx(
            SourceMode::Web,
            Some("__raycast_session=rejected-fixture-session"),
        ))
        .await
        .expect_err("manual session rejected");

    mock.assert_async().await;
    assert_eq!(error_message(error), SESSION_EXPIRED);
}

#[tokio::test]
async fn no_candidates_is_a_missing_credential_without_a_request() {
    let mut server = mockito::Server::new_async().await;
    let mock = mock_status_expect(&mut server, "GET", mockito::Matcher::Any, 200, 0).await;

    let provider = RaycastProvider::with_origin(&server.url());
    let error = provider
        .fetch_candidates(&[], "Google Chrome", Duration::from_secs(5))
        .await
        .expect_err("nothing to try");
    assert_eq!(error_message(error), MISSING_SESSION);

    // A manual header without a session cookie never reaches the network.
    for manual in ["csrf_token=only", "__raycast_session=; csrf_token=x"] {
        let error = provider
            .fetch_usage(&ctx(SourceMode::Web, Some(manual)))
            .await
            .expect_err("no session cookie");
        assert_eq!(error_message(error), MISSING_SESSION);
    }
    mock.assert_async().await;
}

#[tokio::test]
async fn empty_manual_selection_fails_closed_without_browser_access() {
    let mut server = mockito::Server::new_async().await;
    let mock = mock_status_expect(&mut server, "GET", mockito::Matcher::Any, 200, 0).await;

    let provider = RaycastProvider::with_origin(&server.url());
    let mut fetch_context = ctx(SourceMode::Web, None);
    fetch_context.manual_cookie_missing = true;
    let error = provider
        .fetch_usage(&fetch_context)
        .await
        .expect_err("manual without a cookie");

    mock.assert_async().await;
    assert_eq!(error_message(error), MISSING_SESSION);
}

#[tokio::test]
async fn off_makes_no_request_and_no_cookie_access() {
    let mut server = mockito::Server::new_async().await;
    let mock = mock_status_expect(&mut server, "GET", mockito::Matcher::Any, 200, 0).await;

    let provider = RaycastProvider::with_origin(&server.url());
    // The shell maps the Off cookie source to Cli, and a stored header must
    // not be used while cookies are disabled.
    let error = provider
        .fetch_usage(&ctx(SourceMode::Cli, Some(FIXTURE_COOKIE)))
        .await
        .expect_err("cookies are disabled");

    mock.assert_async().await;
    assert_eq!(error_message(error), COOKIES_DISABLED);
    assert!(matches!(
        provider.fetch_usage(&ctx(SourceMode::OAuth, None)).await,
        Err(ProviderError::UnsupportedSource(SourceMode::OAuth))
    ));
}

#[tokio::test]
async fn non_authentication_failures_stop_retries_and_keep_the_session() {
    for (status, expected) in [
        (403, "Raycast denied access to AI credits for this account."),
        (429, "Raycast credits requests are rate limited."),
        (503, "Raycast credits service is unavailable"),
        (400, "Raycast credits API returned HTTP 400"),
        (204, "Raycast credits API returned HTTP 204"),
    ] {
        let mut server = mockito::Server::new_async().await;
        let mock = mock_response_expect(
            &mut server,
            "GET",
            "/frontend_api/current_user/ai_credits",
            status,
            "invalid JSON",
            1,
        )
        .await;

        let provider = RaycastProvider::with_origin(&server.url());
        let error = provider
            .fetch_candidates(
                &headers(&[FIXTURE_COOKIE, "__raycast_session=other"]),
                "Google Chrome",
                Duration::from_secs(5),
            )
            .await
            .expect_err("failure status");

        mock.assert_async().await;
        let message = error_message(error);
        assert!(message.contains(expected), "{status}: {message}");
        assert_ne!(message, SESSION_EXPIRED, "{status}");
    }
}

#[tokio::test]
async fn malformed_success_body_is_a_parse_failure_that_stops_retries() {
    let mut server = mockito::Server::new_async().await;
    let mock = mock_response_expect(
        &mut server,
        "GET",
        "/frontend_api/current_user/ai_credits",
        200,
        "invalid JSON",
        1,
    )
    .await;

    let provider = RaycastProvider::with_origin(&server.url());
    let error = provider
        .fetch_candidates(
            &headers(&[FIXTURE_COOKIE, "__raycast_session=other"]),
            "Google Chrome",
            Duration::from_secs(5),
        )
        .await
        .expect_err("malformed body");

    mock.assert_async().await;
    assert!(matches!(error, ProviderError::Parse(_)));
}
