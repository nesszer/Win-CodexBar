use std::time::Duration;

use mockito::Matcher;
use tokio::time::Instant;

use super::*;
use crate::providers::test_support::mock_response;

// Wire fixtures copied from upstream `TestsPlugin/AbacusPluginTests.swift` (v0.68.0).
const POINTS: &str =
    r#"{"success":true,"result":{"totalComputePoints":1000,"computePointsLeft":750}}"#;
const BILLING: &str =
    r#"{"success":true,"result":{"currentTier":"Pro","nextBillingDate":"2024-03-31T12:30:00Z"}}"#;
const JSON: &str = "application/json";

fn compute(total: f64, left: f64) -> ComputePoints {
    ComputePoints { total, left }
}

fn candidate(label: &str, header: &str) -> (String, String) {
    (label.to_string(), header.to_string())
}

fn web_context(manual_cookie_header: Option<&str>) -> FetchContext {
    FetchContext {
        source_mode: SourceMode::Web,
        web_timeout: 2,
        manual_cookie_header: manual_cookie_header.map(str::to_string),
        ..FetchContext::default()
    }
}

#[test]
fn parses_compute_points_and_tier() {
    let billing = BillingInfo {
        next_billing_date: Some("2025-03-01T00:00:00Z".into()),
        current_tier: Some("Pro".into()),
    };
    let snap = AbacusProvider::build_snapshot(compute(1000.0, 750.0), Some(billing)).unwrap();
    assert!((snap.primary.used_percent - 25.0).abs() < 0.001);
    assert_eq!(
        snap.primary.reset_description.as_deref(),
        Some("250 / 1,000 credits")
    );
    assert_eq!(
        snap.primary.resets_at,
        Some(
            DateTime::parse_from_rfc3339("2025-03-01T00:00:00Z")
                .unwrap()
                .with_timezone(&Utc)
        )
    );
    assert_eq!(snap.primary.window_minutes, Some(28 * 24 * 60));
    assert_eq!(snap.primary_label.as_deref(), Some(CREDITS_LABEL));
    assert_eq!(snap.login_method.as_deref(), Some("Pro"));
    assert!(snap.account_email.is_none());
    assert!(snap.account_organization.is_none());
}

#[test]
fn handles_missing_billing() {
    let snap = AbacusProvider::build_snapshot(compute(500.0, 500.0), None).unwrap();
    assert!((snap.primary.used_percent - 0.0).abs() < f64::EPSILON);
    assert_eq!(
        snap.primary.reset_description.as_deref(),
        Some("0 / 500 credits")
    );
    assert_eq!(
        snap.primary.window_minutes,
        Some(FALLBACK_MONTHLY_WINDOW_MINUTES)
    );
    assert!(snap.primary.resets_at.is_none());
    assert_eq!(snap.primary_label.as_deref(), Some(CREDITS_LABEL));
    assert!(snap.login_method.is_none());
}

#[test]
fn upstream_credit_fixtures_match_percent_and_detail() {
    // (total, left, percent, detail) from the upstream fixture matrix.
    for (total, left, percent, detail) in [
        (1000.0, 750.0, 25.0, "250 / 1,000 credits"),
        (500.0, 500.0, 0.0, "0 / 500 credits"),
        (1000.0, -500.0, 100.0, "1,500 / 1,000 credits"),
        (0.0, 0.0, 0.0, "0 / 0 credits"),
        (100.0, 57.5, 42.5, "42.5 / 100 credits"),
        // 1000.5 rounds half-even, like NumberFormatter and the plugin helper.
        (2000.0, 999.5, 50.025, "1,000 / 2,000 credits"),
    ] {
        let snap = AbacusProvider::build_snapshot(compute(total, left), None).unwrap();
        assert!(
            (snap.primary.used_percent - percent).abs() < 1e-9,
            "{total}/{left}"
        );
        assert_eq!(snap.primary.reset_description.as_deref(), Some(detail));
    }
}

#[test]
fn formats_credit_details_with_grouping_and_fraction() {
    assert_eq!(
        format_credit_detail(12_345.0, 50_000.0),
        "12,345 / 50,000 credits"
    );
    assert_eq!(format_credit_detail(42.5, 100.0), "42.5 / 100 credits");
}

#[test]
fn billing_date_must_look_like_an_iso_date_time() {
    assert!(parse_billing_date("2024-03-31T12:30:00Z").is_some());
    assert!(parse_billing_date("2024-03-31T12:30:00+02:00").is_some());
    for rejected in [
        "not-a-date",
        "2024-03-31",
        "2024/03/31T12:30:00Z",
        "",
        "2024-03-31 12:30:00Z",
    ] {
        assert!(parse_billing_date(rejected).is_none(), "{rejected}");
    }
}

#[test]
fn envelope_success_decodes_credit_fields() {
    let points: RawComputePoints = decode_envelope(POINTS.as_bytes()).unwrap();
    assert_eq!(points.total_compute_points, Some(1000.0));
    assert_eq!(points.compute_points_left, Some(750.0));
    let billing: BillingInfo = decode_envelope(BILLING.as_bytes()).unwrap();
    assert_eq!(billing.current_tier.as_deref(), Some("Pro"));
    assert_eq!(
        billing.next_billing_date.as_deref(),
        Some("2024-03-31T12:30:00Z")
    );
}

#[test]
fn envelope_failure_messages_split_auth_from_parse() {
    for message in [
        "session expired",
        "Please LOGIN first",
        "could not authenticate",
        "Unauthorized",
        "user is unauthenticated",
        "Forbidden",
    ] {
        let body = format!(r#"{{"success":false,"error":"{message}"}}"#);
        assert!(
            matches!(
                decode_envelope::<RawComputePoints>(body.as_bytes()),
                Err(ProviderError::AuthRequired)
            ),
            "{message}"
        );
    }
    for body in [
        r#"{"success":false,"error":"internal failure"}"#,
        r#"{"success":false}"#,
        r#"{"success":true}"#,
        r#"{"success":true,"result":[]}"#,
        r#"{"success":"true","result":{}}"#,
        r#"{"success":true,"result":null,"error":5}"#,
    ] {
        assert!(
            matches!(
                decode_envelope::<RawComputePoints>(body.as_bytes()),
                Err(ProviderError::Parse(_))
            ),
            "{body}"
        );
    }
}

#[test]
fn malformed_bodies_are_parse_failures() {
    for body in ["<html>error</html>", "", "[]", "\"text\"", "null", "7"] {
        assert!(
            matches!(
                decode_envelope::<RawComputePoints>(body.as_bytes()),
                Err(ProviderError::Parse(_))
            ),
            "{body}"
        );
    }
}

#[test]
fn non_numeric_credit_fields_are_missing() {
    for body in [
        r#"{"success":true,"result":{}}"#,
        r#"{"success":true,"result":{"totalComputePoints":"1000","computePointsLeft":750}}"#,
        r#"{"success":true,"result":{"totalComputePoints":1000,"computePointsLeft":null}}"#,
    ] {
        let points: RawComputePoints = decode_envelope(body.as_bytes()).unwrap();
        assert!(
            points.total_compute_points.is_none() || points.compute_points_left.is_none(),
            "{body}"
        );
    }
}

#[test]
fn session_cookie_names_follow_upstream_filter() {
    for accepted in [
        "sessionid=1",
        "SESSION_ID=1",
        "_ga=1; session_token=2",
        "access_token=1",
        "foo=1; abacus_session=2",
        "userAuth=1",
        "connect.sid=1",
        "jwt_value=1",
    ] {
        assert!(has_session_cookie(accepted), "{accepted}");
    }
    for rejected in [
        "csrftoken=1",
        "_ga=1; _gid=2",
        "tracking_session=1",
        "analytics_sid=1",
        "theme=dark; locale=en",
        "csrf_session=1",
        "",
    ] {
        assert!(!has_session_cookie(rejected), "{rejected}");
    }
}

#[test]
fn candidates_are_chrome_first_filtered_and_bounded() {
    let mut headers = vec![
        candidate("Firefox", "sessionid=firefox"),
        candidate("Microsoft Edge", "csrftoken=anon"),
        candidate("Google Chrome", "sessionid=chrome"),
    ];
    for index in 0..5 {
        headers.push(candidate("Brave", &format!("sessionid=extra{index}")));
    }
    let candidates = session_candidates(Ok(headers)).unwrap();
    assert_eq!(candidates.len(), MAX_COOKIE_CANDIDATES as usize);
    assert_eq!(
        candidates[0],
        candidate("Google Chrome", "sessionid=chrome")
    );
    assert_eq!(candidates[1], candidate("Firefox", "sessionid=firefox"));
    assert!(
        candidates
            .iter()
            .all(|(label, _)| label != "Microsoft Edge")
    );
}

#[test]
fn candidate_lookup_errors_are_classified() {
    assert!(
        session_candidates(Err(ProviderError::NoCookies))
            .unwrap()
            .is_empty()
    );
    assert!(matches!(
        session_candidates(Err(ProviderError::Other("locked".into()))),
        Err(ProviderError::Other(_))
    ));
}

#[test]
fn timeouts_match_upstream_budget() {
    for (web, request, refresh) in [
        (0, 1, 6),
        (1, 1, 6),
        (2, 2, 12),
        (15, 15, 80),
        (60, 60, 90),
        (500, 90, 90),
    ] {
        assert_eq!(request_timeout(web), Duration::from_secs(request), "{web}");
        assert_eq!(
            refresh_timeout(request_timeout(web)),
            Duration::from_secs(refresh),
            "{web}"
        );
    }
}

#[tokio::test(start_paused = true)]
async fn slow_billing_is_bounded_and_keeps_credits() {
    let started = Instant::now();
    let (points, billing) = credits_with_billing(
        async { Ok::<_, ProviderError>(1) },
        async {
            tokio::time::sleep(Duration::from_secs(30)).await;
            Some("late")
        },
        Duration::from_secs(5),
    )
    .await
    .unwrap();
    assert_eq!((points, billing), (1, None));
    assert_eq!(started.elapsed(), Duration::from_secs(5));
}

#[tokio::test(start_paused = true)]
async fn billing_runs_concurrently_with_slow_credits() {
    let started = Instant::now();
    let (points, billing) = credits_with_billing(
        async {
            tokio::time::sleep(Duration::from_secs(8)).await;
            Ok::<_, ProviderError>(1)
        },
        async {
            tokio::time::sleep(Duration::from_secs(3)).await;
            Some("billing")
        },
        Duration::from_secs(5),
    )
    .await
    .unwrap();
    assert_eq!((points, billing), (1, Some("billing")));
    assert_eq!(started.elapsed(), Duration::from_secs(8));
}

#[tokio::test(start_paused = true)]
async fn credits_failure_cancels_billing_immediately() {
    let started = Instant::now();
    let result = credits_with_billing(
        async { Err::<u8, _>(ProviderError::AuthRequired) },
        async {
            tokio::time::sleep(Duration::from_secs(30)).await;
            Some(())
        },
        Duration::from_secs(5),
    )
    .await;
    assert!(matches!(result, Err(ProviderError::AuthRequired)));
    assert_eq!(started.elapsed(), Duration::ZERO);
}

#[tokio::test]
async fn sends_upstream_wire_requests_and_builds_snapshot() {
    let mut server = mockito::Server::new_async().await;
    let credits = server
        .mock("GET", "/api/_getOrganizationComputePoints")
        .match_header("cookie", "sessionid=fixture")
        .match_header("accept", JSON)
        .match_header("content-type", JSON)
        .with_body(POINTS)
        .expect(1)
        .create_async()
        .await;
    let billing = server
        .mock("POST", "/api/_getBillingInfo")
        .match_header("cookie", "sessionid=fixture")
        .match_header("accept", JSON)
        .match_header("content-type", JSON)
        .match_body("{}")
        .with_body(BILLING)
        .expect(1)
        .create_async()
        .await;

    let provider = AbacusProvider::with_origin(&server.url());
    let result = provider
        .fetch_usage(&web_context(Some("sessionid=fixture")))
        .await
        .unwrap();

    credits.assert_async().await;
    billing.assert_async().await;
    assert_eq!(result.source_label, "web");
    let usage = result.usage;
    assert!((usage.primary.used_percent - 25.0).abs() < 1e-9);
    assert_eq!(usage.login_method.as_deref(), Some("Pro"));
    assert_eq!(
        usage.primary.resets_at,
        Some(
            DateTime::parse_from_rfc3339("2024-03-31T12:30:00Z")
                .unwrap()
                .with_timezone(&Utc)
        )
    );
}

#[tokio::test]
async fn billing_failures_keep_credits_and_fallback_window() {
    let cases: [(&str, u16, &str); 5] = [
        ("status", 500, "unavailable"),
        (
            "auth envelope",
            200,
            r#"{"success":false,"error":"session expired"}"#,
        ),
        ("unauthorized", 401, ""),
        ("html", 200, "<html>error</html>"),
        (
            "bad date",
            200,
            r#"{"success":true,"result":{"currentTier":"Pro","nextBillingDate":"not-a-date"}}"#,
        ),
    ];
    for (name, status, body) in cases {
        let mut server = mockito::Server::new_async().await;
        mock_response(
            &mut server,
            "GET",
            "/api/_getOrganizationComputePoints",
            200,
            POINTS,
        )
        .await;
        mock_response(
            &mut server,
            "POST",
            "/api/_getBillingInfo",
            status.into(),
            body,
        )
        .await;

        let provider = AbacusProvider::with_origin(&server.url());
        let usage = provider
            .fetch_with_cookies("sessionid=fixture", Duration::from_secs(2))
            .await
            .unwrap();
        assert!((usage.primary.used_percent - 25.0).abs() < 1e-9, "{name}");
        assert_eq!(
            usage.primary.reset_description.as_deref(),
            Some("250 / 1,000 credits"),
            "{name}"
        );
        assert!(usage.primary.resets_at.is_none(), "{name}");
        assert_eq!(
            usage.primary.window_minutes,
            Some(FALLBACK_MONTHLY_WINDOW_MINUTES),
            "{name}"
        );
        let expected_tier = (name == "bad date").then_some("Pro");
        assert_eq!(usage.login_method.as_deref(), expected_tier, "{name}");
    }
}

type ErrorCheck = fn(&ProviderError) -> bool;

#[tokio::test]
async fn required_failures_are_classified() {
    let cases: [(u16, &str, ErrorCheck); 5] = [
        (401, "", |e| matches!(e, ProviderError::AuthRequired)),
        (403, "", |e| matches!(e, ProviderError::AuthRequired)),
        (
            500,
            "boom",
            |e| matches!(e, ProviderError::Other(m) if m == "Abacus AI API error: HTTP 500"),
        ),
        (
            200,
            r#"{"success":true,"result":{}}"#,
            |e| matches!(e, ProviderError::Parse(m) if m.contains("Missing credit fields")),
        ),
        (200, "[]", |e| matches!(e, ProviderError::Parse(_))),
    ];
    for (status, body, check) in cases {
        let mut server = mockito::Server::new_async().await;
        mock_response(
            &mut server,
            "GET",
            "/api/_getOrganizationComputePoints",
            status.into(),
            body,
        )
        .await;
        mock_response(&mut server, "POST", "/api/_getBillingInfo", 200, BILLING).await;
        let provider = AbacusProvider::with_origin(&server.url());
        let error = provider
            .fetch_with_cookies("sessionid=fixture", Duration::from_secs(2))
            .await
            .unwrap_err();
        assert!(check(&error), "{status} {body}: {error:?}");
    }
}

/// Mount a credits endpoint that answers `status` for one cookie header.
async fn credits_for(
    server: &mut mockito::ServerGuard,
    cookie: &str,
    status: usize,
    body: &str,
) -> mockito::Mock {
    server
        .mock("GET", "/api/_getOrganizationComputePoints")
        .match_header("cookie", cookie)
        .with_status(status)
        .with_body(body)
        .expect(1)
        .create_async()
        .await
}

#[tokio::test]
async fn failed_candidates_advance_to_the_next_session() {
    for (stale_status, stale_body) in [
        (401, ""),
        (200, "[]"),
        (200, r#"{"success":true,"result":{}}"#),
        (500, "boom"),
    ] {
        let mut server = mockito::Server::new_async().await;
        let stale = credits_for(&mut server, "session=stale", stale_status, stale_body).await;
        let fresh = credits_for(&mut server, "session=fresh", 200, POINTS).await;
        mock_response(&mut server, "POST", "/api/_getBillingInfo", 200, BILLING).await;

        let provider = AbacusProvider::with_origin(&server.url());
        let usage = provider
            .fetch_with_candidates(
                &[
                    candidate("Google Chrome", "session=stale"),
                    candidate("Firefox", "session=fresh"),
                ],
                Duration::from_secs(2),
            )
            .await
            .unwrap();
        assert!((usage.primary.used_percent - 25.0).abs() < 1e-9);
        stale.assert_async().await;
        fresh.assert_async().await;
    }
}

#[tokio::test]
async fn exhausted_candidates_report_the_last_error() {
    let mut server = mockito::Server::new_async().await;
    let first = credits_for(&mut server, "session=one", 500, "boom").await;
    let second = credits_for(&mut server, "session=two", 401, "").await;
    mock_response(&mut server, "POST", "/api/_getBillingInfo", 200, BILLING).await;

    let provider = AbacusProvider::with_origin(&server.url());
    let error = provider
        .fetch_with_candidates(
            &[candidate("A", "session=one"), candidate("B", "session=two")],
            Duration::from_secs(2),
        )
        .await
        .unwrap_err();
    assert!(matches!(error, ProviderError::AuthRequired));
    first.assert_async().await;
    second.assert_async().await;
}

#[tokio::test]
async fn no_candidates_report_the_missing_session_message() {
    let provider = AbacusProvider::with_origin("http://127.0.0.1:9");
    let error = provider
        .fetch_with_candidates(&[], Duration::from_secs(2))
        .await
        .unwrap_err();
    assert!(matches!(&error, ProviderError::Other(m) if m == MISSING_SESSION_MESSAGE));
    assert_eq!(
        provider.error_state_kind(&error),
        ProviderStateKind::NeedsAuthentication
    );
    assert_eq!(
        provider.error_state_kind(&ProviderError::Other("other".into())),
        ProviderStateKind::Unknown
    );
}

#[tokio::test]
async fn manual_cookie_is_exclusive_and_errors_propagate() {
    let mut server = mockito::Server::new_async().await;
    let credits = server
        .mock("GET", "/api/_getOrganizationComputePoints")
        .match_header("cookie", Matcher::Exact("session=stale".into()))
        .with_status(401)
        .expect(1)
        .create_async()
        .await;
    mock_response(&mut server, "POST", "/api/_getBillingInfo", 200, BILLING).await;

    let provider = AbacusProvider::with_origin(&server.url());
    let error = provider
        .fetch_usage(&web_context(Some("session=stale")))
        .await
        .unwrap_err();
    assert!(matches!(error, ProviderError::AuthRequired));
    credits.assert_async().await;
}
