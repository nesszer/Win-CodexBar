use super::*;
use mockito::{Matcher, Server, ServerGuard};
use serde_json::json;

const CREDITS_PATH: &str = "/user.v1.UserService/GetAvailableCredits";

fn provider_for(server: &ServerGuard) -> ManusProvider {
    ManusProvider {
        client: Client::builder()
            .no_proxy()
            .build()
            .expect("the test client should build"),
        credits_url: format!("{}{CREDITS_PATH}", server.url()),
    }
}

fn no_env(_: &str) -> Option<String> {
    None
}

fn candidates(headers: &[&str]) -> Result<Vec<(String, String)>, ProviderError> {
    Ok(headers
        .iter()
        .map(|header| ("Fixture".to_string(), (*header).to_string()))
        .collect())
}

async fn token_mock(
    server: &mut Server,
    token: &str,
    status: usize,
    body: &str,
    hits: usize,
) -> mockito::Mock {
    server
        .mock("POST", CREDITS_PATH)
        .match_header("authorization", format!("Bearer {token}").as_str())
        .match_header("connect-protocol-version", "1")
        .match_header("origin", "https://manus.im")
        .match_body(Matcher::Exact("{}".into()))
        .with_status(status)
        .with_body(body)
        .expect(hits)
        .create_async()
        .await
}

// --- ManusCookieHeaderTests.swift (v0.66.0) ---

#[test]
fn bare_token_resolves_directly() {
    assert_eq!(session_token("abc123").as_deref(), Some("abc123"));
}

#[test]
fn extracts_session_id_from_cookie_header() {
    assert_eq!(
        session_token("foo=bar; session_id=token-a; baz=qux").as_deref(),
        Some("token-a")
    );
}

#[test]
fn extracts_mixed_case_session_id_from_cookie_header() {
    assert_eq!(
        session_token("foo=bar; Session_ID=token-b; baz=qux").as_deref(),
        Some("token-b")
    );
}

#[test]
fn unsupported_cookie_header_returns_none() {
    assert_eq!(session_token("foo=bar; hello=world"), None);
}

// --- Tolerant extraction (regression: one chunk without `=` aborted the scan) ---

#[test]
fn chunk_without_equals_does_not_abort_the_scan() {
    assert_eq!(
        session_token("garbage; ; session_id=abc").as_deref(),
        Some("abc")
    );
    assert_eq!(
        session_token("session_id=; other=1; session_id=second").as_deref(),
        Some("second")
    );
}

#[test]
fn whitespace_around_equals_is_accepted() {
    assert_eq!(
        session_token("Session_ID = fixture").as_deref(),
        Some("fixture")
    );
}

#[test]
fn blank_and_unsendable_values_are_not_tokens() {
    assert_eq!(session_token("   "), None);
    assert_eq!(session_token("session_id=  "), None);
    assert_eq!(session_token("session_id=bad\u{7f}token"), None);
}

// --- Environment fallback (v0.65.0 ManusSettingsReader) ---

fn env_from(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
    move |name| {
        pairs
            .iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| (*value).to_string())
    }
}

#[test]
fn env_prefers_session_token_then_session_id_then_cookie() {
    let env = env_from(&[
        ("MANUS_SESSION_ID", "from-id"),
        ("MANUS_SESSION_TOKEN", "from-token"),
        ("MANUS_COOKIE", "session_id=from-cookie"),
    ]);
    assert_eq!(env_session_credential(&env).as_deref(), Some("from-token"));

    let env = env_from(&[
        ("MANUS_SESSION_ID", "from-id"),
        ("MANUS_COOKIE", "session_id=from-cookie"),
    ]);
    assert_eq!(env_session_credential(&env).as_deref(), Some("from-id"));

    let env = env_from(&[("MANUS_COOKIE", "foo=bar; session_id=from-cookie")]);
    assert_eq!(
        session_token(&env_session_credential(&env).unwrap()).as_deref(),
        Some("from-cookie")
    );
}

#[test]
fn env_values_are_cleaned_and_unusable_tokens_fall_through() {
    let env = env_from(&[("MANUS_SESSION_TOKEN", "  \"quoted\"  ")]);
    assert_eq!(env_session_credential(&env).as_deref(), Some("quoted"));

    // A token variable without a usable token falls through to the cookie.
    let env = env_from(&[
        ("MANUS_SESSION_TOKEN", "unrelated=1"),
        ("MANUS_COOKIE", "session_id=fallback"),
    ]);
    assert_eq!(
        session_token(&env_session_credential(&env).unwrap()).as_deref(),
        Some("fallback")
    );

    assert_eq!(env_session_credential(&env_from(&[])), None);
    assert_eq!(
        env_session_credential(&env_from(&[("MANUS_SESSION_TOKEN", "  ")])),
        None
    );
}

// --- ManusPluginTests.swift (v0.66.0) ---

#[tokio::test]
async fn rejected_candidates_advance_before_environment_fallback() {
    let mut server = Server::new_async().await;
    let cached = token_mock(&mut server, "cached", 401, "expired", 1).await;
    // The duplicate `session_id=browser` candidate must be skipped.
    let browser = token_mock(&mut server, "browser", 401, "expired", 1).await;
    let environment = token_mock(&mut server, "environment", 200, r#"{"totalCredits":5}"#, 1).await;

    let snapshot = provider_for(&server)
        .fetch_automatic(
            candidates(&[
                "session_id=cached",
                "session_id=browser",
                "session_id=browser",
            ]),
            env_from(&[("MANUS_SESSION_TOKEN", "environment")]),
        )
        .await
        .expect("the environment token should be accepted");

    assert_eq!(snapshot.login_method.as_deref(), Some("Balance: 5 credits"));
    cached.assert_async().await;
    browser.assert_async().await;
    environment.assert_async().await;
}

#[tokio::test]
async fn manual_header_never_uses_environment_after_rejection() {
    let mut server = Server::new_async().await;
    let manual = token_mock(&mut server, "manual", 403, "expired", 1).await;
    let environment = token_mock(&mut server, "environment", 200, r#"{"totalCredits":5}"#, 0).await;

    let ctx = FetchContext {
        manual_cookie_header: Some("manual".into()),
        ..FetchContext::default()
    };
    let error = provider_for(&server)
        .fetch_usage(&ctx)
        .await
        .expect_err("a rejected manual session is an authentication failure");

    assert!(matches!(error, ProviderError::AuthRequired));
    manual.assert_async().await;
    environment.assert_async().await;
}

#[tokio::test]
async fn manual_source_without_a_cookie_fails_closed() {
    let server = Server::new_async().await;
    let ctx = FetchContext {
        manual_cookie_missing: true,
        ..FetchContext::default()
    };
    let error = provider_for(&server)
        .fetch_usage(&ctx)
        .await
        .expect_err("manual without a cookie must not import or read the environment");
    assert!(matches!(error, ProviderError::Other(message) if message.contains("Manual")));
}

#[tokio::test]
async fn cookie_source_off_reaches_the_provider_as_cli_and_sends_nothing() {
    let server = Server::new_async().await;
    let ctx = FetchContext {
        source_mode: SourceMode::Cli,
        ..FetchContext::default()
    };
    let error = provider_for(&server)
        .fetch_usage(&ctx)
        .await
        .expect_err("off maps to an unsupported source");
    assert!(matches!(
        error,
        ProviderError::UnsupportedSource(SourceMode::Cli)
    ));
}

#[tokio::test]
async fn sparse_credits_and_numeric_dates_retain_native_projection() {
    let mut server = Server::new_async().await;
    let body = r#"{"totalCredits":"1200","periodicCredits":"300","proMonthlyCredits":1000,
        "refreshCredits":"bad","maxRefreshCredits":100,"nextRefreshTime":0,
        "refreshInterval":"DAILY REFRESH"}"#;
    let mock = token_mock(&mut server, "fixture", 200, body, 1).await;

    let snapshot = provider_for(&server)
        .fetch_automatic(candidates(&["Session_ID = fixture"]), no_env)
        .await
        .expect("sparse credits should parse");

    assert_eq!(snapshot.primary.used_percent, 70.0);
    let secondary = snapshot.secondary.expect("refresh lane");
    assert_eq!(secondary.used_percent, 100.0);
    // Numeric dates are offsets from the 2001-01-01 reference epoch.
    assert_eq!(
        secondary.resets_at,
        Some(
            DateTime::parse_from_rfc3339("2001-01-01T00:00:00Z")
                .unwrap()
                .with_timezone(&Utc)
        )
    );
    mock.assert_async().await;
}

// --- Candidate loop behavior ---

#[tokio::test]
async fn first_accepted_candidate_wins_and_later_ones_are_not_tried() {
    let mut server = Server::new_async().await;
    let first = token_mock(&mut server, "first", 200, r#"{"totalCredits":9}"#, 1).await;
    let second = token_mock(&mut server, "second", 200, r#"{"totalCredits":1}"#, 0).await;

    let snapshot = provider_for(&server)
        .fetch_automatic(
            candidates(&["session_id=first", "session_id=second"]),
            no_env,
        )
        .await
        .unwrap();

    assert_eq!(snapshot.login_method.as_deref(), Some("Balance: 9 credits"));
    first.assert_async().await;
    second.assert_async().await;
}

#[tokio::test]
async fn non_auth_failure_stops_iteration() {
    let mut server = Server::new_async().await;
    let broken = token_mock(&mut server, "broken", 500, "boom", 1).await;
    let next = token_mock(&mut server, "next", 200, r#"{"totalCredits":1}"#, 0).await;

    let error = provider_for(&server)
        .fetch_automatic(
            candidates(&["session_id=broken", "session_id=next"]),
            no_env,
        )
        .await
        .expect_err("a server error is final");

    assert!(matches!(error, ProviderError::Other(message) if message.contains("500")));
    broken.assert_async().await;
    next.assert_async().await;
}

#[tokio::test]
async fn malformed_candidate_is_skipped_not_fatal() {
    let mut server = Server::new_async().await;
    let good = token_mock(&mut server, "good", 200, r#"{"totalCredits":3}"#, 1).await;

    // The first two carry no `session_id`; neither may send a request or end
    // the scan.
    let snapshot = provider_for(&server)
        .fetch_automatic(
            candidates(&["not-a-cookie; also bad", "a=b; c=d", "a=b; session_id=good"]),
            no_env,
        )
        .await
        .expect("the third candidate should be reached");

    assert_eq!(snapshot.login_method.as_deref(), Some("Balance: 3 credits"));
    good.assert_async().await;
}

#[tokio::test]
async fn all_rejected_is_auth_required_and_none_is_no_cookies() {
    let mut server = Server::new_async().await;
    let only = token_mock(&mut server, "only", 401, "expired", 1).await;
    let provider = provider_for(&server);

    let rejected = provider
        .fetch_automatic(candidates(&["session_id=only"]), no_env)
        .await
        .unwrap_err();
    assert!(matches!(rejected, ProviderError::AuthRequired));
    only.assert_async().await;

    let none = provider
        .fetch_automatic(Err(ProviderError::NoCookies), no_env)
        .await
        .unwrap_err();
    assert!(matches!(none, ProviderError::NoCookies));
    let empty = provider.fetch_automatic(candidates(&[]), no_env).await;
    assert!(matches!(empty, Err(ProviderError::NoCookies)));
}

#[tokio::test]
async fn browser_read_failure_still_reaches_the_environment_fallback() {
    let mut server = Server::new_async().await;
    let environment = token_mock(&mut server, "environment", 200, r#"{"totalCredits":2}"#, 1).await;
    let provider = provider_for(&server);

    let snapshot = provider
        .fetch_automatic(
            Err(ProviderError::Other(
                "Failed to read browser cookies".into(),
            )),
            env_from(&[("MANUS_SESSION_ID", "environment")]),
        )
        .await
        .unwrap();
    assert_eq!(snapshot.login_method.as_deref(), Some("Balance: 2 credits"));
    environment.assert_async().await;

    let error = provider
        .fetch_automatic(
            Err(ProviderError::Other(
                "Failed to read browser cookies".into(),
            )),
            no_env,
        )
        .await
        .unwrap_err();
    assert!(matches!(error, ProviderError::Other(message) if message.contains("browser cookies")));
}

// --- Response shape ---

#[test]
fn response_without_any_credit_key_is_a_parse_failure() {
    for body in [
        json!({}),
        json!([]),
        json!(null),
        json!({"data": {"unrelated": 1}}),
        json!({"data": [1]}),
        json!({"unrelated": 1}),
    ] {
        let error = parse_credits(&body).expect_err("missing credit keys must fail");
        assert!(
            matches!(&error, ProviderError::Parse(message) if message.contains("missing expected credits fields")),
            "unexpected result for {body}: {error:?}"
        );
    }
}

#[test]
fn credits_are_read_from_the_first_present_envelope_key() {
    let credits = parse_credits(&json!({"result": {"totalCredits": 7}})).unwrap();
    assert_eq!(credits.total_credits, 7.0);
    let credits = parse_credits(&json!({"totalCredits": 4})).unwrap();
    assert_eq!(credits.total_credits, 4.0);
    // A present-but-non-object envelope is not skipped (upstream `??`).
    assert!(parse_credits(&json!({"data": 3, "totalCredits": 4})).is_err());
}

#[test]
fn non_finite_and_wrongly_typed_numbers_count_as_zero() {
    let credits = parse_credits(&json!({
        "totalCredits": "Infinity",
        "freeCredits": true,
        "periodicCredits": null,
        "addonCredits": "12",
        "refreshInterval": 5
    }))
    .unwrap();
    assert_eq!(credits.total_credits, 0.0);
    assert_eq!(credits.free_credits, 0.0);
    assert_eq!(credits.periodic_credits, 0.0);
    assert_eq!(credits.addon_credits, 12.0);
    assert_eq!(credits.refresh_interval, None);
}

#[test]
fn refresh_time_accepts_legacy_numbers_and_iso_strings_only() {
    let parse = |value: Value| {
        parse_credits(&json!({"totalCredits": 1, "nextRefreshTime": value}))
            .unwrap()
            .next_refresh_time
    };
    let at = |text: &str| {
        Some(
            DateTime::parse_from_rfc3339(text)
                .unwrap()
                .with_timezone(&Utc),
        )
    };
    assert_eq!(parse(json!(0)), at("2001-01-01T00:00:00Z"));
    assert_eq!(parse(json!(86_400)), at("2001-01-02T00:00:00Z"));
    assert_eq!(
        parse(json!("2026-10-01T12:00:00.000Z")),
        at("2026-10-01T12:00:00Z")
    );
    assert_eq!(parse(json!("tomorrow")), None);
    assert_eq!(parse(json!(true)), None);
    assert_eq!(parse(Value::Null), None);
}
