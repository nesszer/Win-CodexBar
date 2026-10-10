use chrono::FixedOffset;
use serde_json::json;

use super::cookies::{SESSION_COOKIE_NAMES, request_cookies};
use super::*;
use crate::providers::test_support::{mock_response, mock_status_expect};

// Fixture epochs: 2030-01-01T00:00:00Z renewal, promo expiries around it.
const NOW: i64 = 1_750_000_000; // 2025-06-15T15:06:40Z
const RENEWAL: i64 = 1_893_456_000;

fn now() -> DateTime<Utc> {
    Utc.timestamp_opt(NOW, 0).single().expect("valid epoch")
}

fn response(value: serde_json::Value) -> CreditsResponse {
    serde_json::from_value(value).expect("valid credits payload")
}

fn snapshot(value: serde_json::Value) -> UsageSnapshot {
    PerplexityProvider::parse_response(&response(value), now(), &Utc)
}

fn description(window: &RateWindow) -> &str {
    window.reset_description.as_deref().unwrap_or_default()
}

fn body(usage: f64) -> String {
    json!({
        "balance_cents": 500,
        "renewal_date_ts": RENEWAL,
        "current_period_purchased_cents": 0,
        "credit_grants": [{ "type": "recurring", "amount_cents": 1000 }],
        "total_usage_cents": usage,
    })
    .to_string()
}

// ---- cookies ----

#[test]
fn bare_token_is_tried_under_every_session_cookie_name() {
    let cookies = request_cookies("  manual-fixture ");
    let expected: Vec<String> = SESSION_COOKIE_NAMES
        .iter()
        .map(|name| format!("{name}=manual-fixture"))
        .collect();
    assert_eq!(cookies, expected);
}

#[test]
fn cookie_header_sends_only_the_session_cookie() {
    let cookies = request_cookies("Cookie: theme=dark; next-auth.session-token=abc; other=1");
    assert_eq!(cookies, vec!["next-auth.session-token=abc"]);
}

#[test]
fn earlier_supported_names_win_regardless_of_header_order() {
    let cookies = request_cookies("next-auth.session-token=old; __Secure-authjs.session-token=new");
    assert_eq!(cookies, vec!["__Secure-authjs.session-token=new"]);
}

#[test]
fn chunked_cookie_is_reassembled_in_index_order() {
    let cookies = request_cookies(
        "authjs.session-token.1=second; ignored=fixture; authjs.session-token.0=first",
    );
    assert_eq!(cookies, vec!["authjs.session-token=firstsecond"]);
}

#[test]
fn chunk_gap_or_missing_zero_yields_no_cookie() {
    assert!(
        request_cookies("authjs.session-token.0=first; authjs.session-token.2=third").is_empty()
    );
    assert!(request_cookies("authjs.session-token.1=second").is_empty());
}

#[test]
fn header_without_a_session_cookie_yields_nothing() {
    assert!(request_cookies("theme=dark; other=1").is_empty());
    assert!(request_cookies("").is_empty());
    assert!(request_cookies("a=b\r\nc=d").is_empty());
}

#[test]
fn cookie_names_match_case_insensitively_and_keep_header_casing() {
    let cookies = request_cookies("Authjs.Session-Token=abc");
    assert_eq!(cookies, vec!["Authjs.Session-Token=abc"]);
}

#[test]
fn empty_values_are_ignored_and_repeats_keep_the_last_value() {
    assert!(request_cookies("authjs.session-token=").is_empty());
    let cookies = request_cookies("authjs.session-token=first; authjs.session-token=second");
    assert_eq!(cookies, vec!["authjs.session-token=second"]);
}

#[test]
fn chunk_suffix_follows_upstream_integer_rules() {
    // `+1` and `-0` count as indices 1 and 0; `-1` and `1e3` do not.
    let cookies = request_cookies("authjs.session-token.-0=a; authjs.session-token.+1=b");
    assert_eq!(cookies, vec!["authjs.session-token=ab"]);
    assert!(request_cookies("authjs.session-token.-1=a").is_empty());
    assert!(request_cookies("authjs.session-token.1e3=a").is_empty());
}

// ---- environment secret ----

#[test]
fn environment_session_token_wins_over_cookie_and_strips_quotes() {
    let env = |name: &str| match name {
        "PERPLEXITY_SESSION_TOKEN" => Some("  \"token-value\" ".to_string()),
        "PERPLEXITY_COOKIE" => Some("authjs.session-token=other".to_string()),
        _ => None,
    };
    assert_eq!(environment_cookie(env).as_deref(), Some("token-value"));
}

#[test]
fn environment_falls_back_to_cookie_when_token_is_blank() {
    let env = |name: &str| match name {
        "PERPLEXITY_SESSION_TOKEN" => Some("   ".to_string()),
        "PERPLEXITY_COOKIE" => Some("'authjs.session-token=abc'".to_string()),
        _ => None,
    };
    assert_eq!(
        environment_cookie(env).as_deref(),
        Some("authjs.session-token=abc")
    );
    assert_eq!(environment_cookie(|_| None), None);
}

// ---- payload validation ----

#[test]
fn camel_case_payload_is_accepted() {
    let parsed: Result<CreditsResponse, _> = serde_json::from_value(json!({
        "balanceCents": 1,
        "renewalDateTs": RENEWAL,
        "currentPeriodPurchasedCents": 0,
        "creditGrants": [{ "type": "recurring", "amountCents": 100, "expiresAtTs": null }],
        "totalUsageCents": 10,
    }));
    assert!(parsed.is_ok());
}

#[test]
fn missing_or_non_numeric_fields_are_parse_failures() {
    let complete = json!({
        "balance_cents": 1,
        "renewal_date_ts": RENEWAL,
        "current_period_purchased_cents": 0,
        "credit_grants": [],
        "total_usage_cents": 0,
    });
    for field in [
        "balance_cents",
        "renewal_date_ts",
        "current_period_purchased_cents",
        "credit_grants",
        "total_usage_cents",
    ] {
        let mut value = complete.clone();
        value.as_object_mut().expect("object").remove(field);
        assert!(
            serde_json::from_value::<CreditsResponse>(value).is_err(),
            "missing {field} must fail"
        );
    }
    let mut string_number = complete.clone();
    string_number["total_usage_cents"] = json!("10");
    assert!(serde_json::from_value::<CreditsResponse>(string_number).is_err());

    let mut bad_grant = complete;
    bad_grant["credit_grants"] = json!([{ "type": "recurring" }]);
    assert!(serde_json::from_value::<CreditsResponse>(bad_grant).is_err());
}

// ---- lanes ----

fn base(grants: serde_json::Value, usage: f64, purchased: f64) -> serde_json::Value {
    json!({
        "balance_cents": 0,
        "renewal_date_ts": RENEWAL,
        "current_period_purchased_cents": purchased,
        "credit_grants": grants,
        "total_usage_cents": usage,
    })
}

#[test]
fn recurring_credits_fill_the_primary_lane_first() {
    let snap = snapshot(base(
        json!([
            { "type": "recurring", "amount_cents": 500 },
            { "type": "promotional", "amount_cents": 100, "expires_at_ts": RENEWAL },
            { "type": "purchased", "amount_cents": 200 },
        ]),
        150.0,
        0.0,
    ));
    assert!((snap.primary.used_percent - 30.0).abs() < 1e-9);
    assert_eq!(description(&snap.primary), "150/500 credits");
    assert_eq!(
        snap.primary.resets_at,
        Utc.timestamp_opt(RENEWAL, 0).single()
    );
    let bonus = snap.secondary.expect("secondary lane");
    assert!(bonus.used_percent.abs() < 1e-9);
    assert!(description(&bonus).starts_with("0/100 bonus"));
    let purchased = snap.tertiary.expect("tertiary lane");
    assert!(purchased.used_percent.abs() < 1e-9);
    assert_eq!(description(&purchased), "0/200 credits");
    assert_eq!(snap.login_method.as_deref(), Some("Pro"));
}

#[test]
fn waterfall_consumes_recurring_then_purchased_then_promotional() {
    let snap = snapshot(base(
        json!([
            { "type": "recurring", "amount_cents": 100 },
            { "type": "promotional", "amount_cents": 300, "expires_at_ts": RENEWAL },
            { "type": "purchased", "amount_cents": 200 },
        ]),
        450.0,
        0.0,
    ));
    assert_eq!(description(&snap.primary), "100/100 credits");
    assert_eq!(
        description(&snap.tertiary.expect("purchased")),
        "200/200 credits"
    );
    let bonus = snap.secondary.expect("bonus");
    assert!(description(&bonus).starts_with("150/300 bonus"));
    assert!((bonus.used_percent - 50.0).abs() < 1e-9);
}

#[test]
fn purchased_uses_the_larger_of_grants_and_period_field() {
    let from_field = snapshot(base(json!([]), 0.0, 700.0));
    assert_eq!(
        description(&from_field.tertiary.expect("tertiary")),
        "0/700 credits"
    );
    let from_grants = snapshot(base(
        json!([{ "type": "purchased", "amount_cents": 900 }]),
        0.0,
        700.0,
    ));
    assert_eq!(
        description(&from_grants.tertiary.expect("tertiary")),
        "0/900 credits"
    );
}

#[test]
fn only_unexpired_promotional_grants_count_and_bonus_type_is_ignored() {
    let snap = snapshot(base(
        json!([
            { "type": "recurring", "amount_cents": 100 },
            { "type": "promotional", "amount_cents": 50, "expires_at_ts": NOW - 1 },
            { "type": "promotional", "amount_cents": 70, "expires_at_ts": NOW + 86_400 * 3 },
            { "type": "promotional", "amount_cents": 30, "expires_at_ts": null },
            { "type": "bonus", "amount_cents": 999, "expires_at_ts": NOW + 10 },
        ]),
        0.0,
        0.0,
    ));
    // 70 + 30 = 100; the expired and `bonus`-typed grants are excluded.
    let bonus = snap.secondary.expect("bonus");
    assert_eq!(description(&bonus), "0/100 bonus · exp. Jun 18");
}

#[test]
fn promotional_expiry_uses_the_earliest_unexpired_grant_in_the_given_zone() {
    let value = base(
        json!([
            { "type": "recurring", "amount_cents": 100 },
            { "type": "promotional", "amount_cents": 10, "expires_at_ts": 1_750_550_400 },
            { "type": "promotional", "amount_cents": 10, "expires_at_ts": 1_751_000_000 },
        ]),
        0.0,
        0.0,
    );
    // 1_750_550_400 is 2025-06-22T00:00:00Z.
    let utc = PerplexityProvider::parse_response(&response(value.clone()), now(), &Utc);
    assert!(description(&utc.secondary.expect("bonus")).ends_with("exp. Jun 22"));
    let west = FixedOffset::west_opt(5 * 3600).expect("offset");
    let local = PerplexityProvider::parse_response(&response(value), now(), &west);
    assert!(description(&local.secondary.expect("bonus")).ends_with("exp. Jun 21"));
}

#[test]
fn promotional_without_expiry_has_no_suffix() {
    let snap = snapshot(base(
        json!([{ "type": "promotional", "amount_cents": 40 }]),
        0.0,
        0.0,
    ));
    assert_eq!(description(&snap.secondary.expect("bonus")), "0/40 bonus");
}

#[test]
fn primary_is_a_placeholder_when_only_promotional_or_purchased_exist() {
    let snap = snapshot(base(
        json!([{ "type": "promotional", "amount_cents": 40 }]),
        10.0,
        0.0,
    ));
    assert!(snap.primary.is_informational);
    assert_eq!(description(&snap.primary), NO_RECURRING_CREDITS);
    assert!(snap.login_method.is_none());

    let purchased_only = snapshot(base(json!([]), 0.0, 100.0));
    assert!(purchased_only.primary.is_informational);
}

#[test]
fn empty_account_reads_zero_of_zero_at_full_usage() {
    let snap = snapshot(base(json!([]), 0.0, 0.0));
    assert!((snap.primary.used_percent - 100.0).abs() < 1e-9);
    assert_eq!(description(&snap.primary), "0/0 credits");
    // Empty pools stay present but informational, so they cannot look
    // depleted to notifications, hooks, or auto-resume.
    let bonus = snap.secondary.expect("secondary always present");
    assert!(bonus.is_informational);
    assert_eq!(description(&bonus), "0/0 bonus");
    let purchased = snap.tertiary.expect("tertiary always present");
    assert!(purchased.is_informational);
    assert_eq!(description(&purchased), "0/0 credits");
    assert!(snap.login_method.is_none());
}

#[test]
fn plan_is_pro_below_5000_recurring_cents_and_max_from_5000() {
    let plan = |cents: f64| {
        snapshot(base(
            json!([{ "type": "recurring", "amount_cents": cents }]),
            0.0,
            0.0,
        ))
        .login_method
    };
    assert_eq!(plan(4999.0).as_deref(), Some("Pro"));
    assert_eq!(plan(5000.0).as_deref(), Some("Max"));
}

#[test]
fn usage_beyond_all_pools_clamps_each_lane_at_full() {
    let snap = snapshot(base(
        json!([{ "type": "recurring", "amount_cents": 100 }]),
        10_000.0,
        0.0,
    ));
    assert!((snap.primary.used_percent - 100.0).abs() < 1e-9);
    assert_eq!(description(&snap.primary), "100/100 credits");
}

#[test]
fn negative_usage_is_shown_as_reported_and_negative_grants_floor_at_zero() {
    let snap = snapshot(base(
        json!([
            { "type": "recurring", "amount_cents": 100 },
            { "type": "promotional", "amount_cents": -50, "expires_at_ts": RENEWAL },
        ]),
        -20.0,
        0.0,
    ));
    assert_eq!(description(&snap.primary), "-20/100 credits");
    assert!(snap.primary.used_percent.abs() < 1e-9);
    let bonus = snap.secondary.expect("bonus");
    assert!(bonus.is_informational);
    assert!(description(&bonus).starts_with("0/0 bonus"));
}

#[test]
fn descriptions_round_used_and_truncate_total() {
    assert_eq!(
        PerplexityProvider::credit_description(2.5, 9.9, "credits").as_deref(),
        Some("3/9 credits")
    );
    assert_eq!(
        PerplexityProvider::credit_description(-0.2, 1.0, "credits").as_deref(),
        Some("0/1 credits")
    );
    assert!(PerplexityProvider::credit_description(f64::INFINITY, 1.0, "credits").is_none());
}

#[test]
fn oversized_credit_totals_do_not_render_non_finite_descriptions() {
    let snap = snapshot(base(
        json!([
            { "type": "recurring", "amount_cents": f64::MAX },
            { "type": "recurring", "amount_cents": f64::MAX },
        ]),
        0.0,
        0.0,
    ));
    assert!(snap.primary.reset_description.is_none());
}

// ---- HTTP ----

fn provider_for(server: &mockito::ServerGuard) -> PerplexityProvider {
    let mut provider = PerplexityProvider::new();
    provider.credits_url = format!("{}/rest/billing/credits", server.url());
    provider
}

#[tokio::test]
async fn rejected_cookie_moves_on_and_the_accepted_one_is_used() {
    let mut server = mockito::Server::new_async().await;
    let rejected = server
        .mock("GET", "/rest/billing/credits")
        .match_header("cookie", "__Secure-authjs.session-token=bare")
        .with_status(401)
        .create_async()
        .await;
    let accepted = server
        .mock("GET", "/rest/billing/credits")
        .match_header("cookie", "authjs.session-token=bare")
        .match_header("origin", ORIGIN)
        .match_header("referer", REFERER)
        .with_status(200)
        .with_body(body(500.0))
        .create_async()
        .await;
    let provider = provider_for(&server);

    let usage = provider
        .fetch_first_accepted(&["bare".to_string()])
        .await
        .expect("second cookie name accepted");

    rejected.assert_async().await;
    accepted.assert_async().await;
    assert!((usage.primary.used_percent - 50.0).abs() < 1e-9);
}

#[tokio::test]
async fn later_header_is_tried_after_an_earlier_one_is_rejected() {
    let mut server = mockito::Server::new_async().await;
    let first = server
        .mock("GET", "/rest/billing/credits")
        .match_header("cookie", "authjs.session-token=expired")
        .with_status(403)
        .create_async()
        .await;
    let second = server
        .mock("GET", "/rest/billing/credits")
        .match_header("cookie", "authjs.session-token=fresh")
        .with_status(200)
        .with_body(body(0.0))
        .create_async()
        .await;
    let provider = provider_for(&server);

    let headers = vec![
        "authjs.session-token=expired; theme=dark".to_string(),
        "authjs.session-token=fresh".to_string(),
    ];
    provider
        .fetch_first_accepted(&headers)
        .await
        .expect("fresh session accepted");

    first.assert_async().await;
    second.assert_async().await;
}

#[tokio::test]
async fn identical_cookies_are_requested_once() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/rest/billing/credits")
        .match_header("cookie", "authjs.session-token=same")
        .with_status(401)
        .expect(1)
        .create_async()
        .await;
    let provider = provider_for(&server);

    let headers = vec![
        "authjs.session-token=same".to_string(),
        "theme=dark; authjs.session-token=same".to_string(),
    ];
    let error = provider
        .fetch_first_accepted(&headers)
        .await
        .expect_err("rejected");

    mock.assert_async().await;
    assert!(matches!(error, ProviderError::AuthRequired));
}

#[tokio::test]
async fn no_usable_cookie_reports_missing_cookies_without_a_request() {
    let server = mockito::Server::new_async().await;
    let provider = provider_for(&server);

    let error = provider
        .fetch_first_accepted(&["theme=dark".to_string()])
        .await
        .expect_err("nothing to send");

    assert!(matches!(error, ProviderError::NoCookies));
}

#[tokio::test]
async fn server_errors_are_terminal_and_do_not_try_more_cookies() {
    let mut server = mockito::Server::new_async().await;
    let mock = mock_status_expect(&mut server, "GET", "/rest/billing/credits", 500, 1).await;
    let provider = provider_for(&server);

    let error = provider
        .fetch_first_accepted(&["bare".to_string()])
        .await
        .expect_err("HTTP 500");

    mock.assert_async().await;
    assert!(error.to_string().contains("HTTP 500"));
}

#[tokio::test]
async fn invalid_payloads_are_parse_failures_with_a_friendly_reason() {
    let mut server = mockito::Server::new_async().await;
    let mock = mock_response(
        &mut server,
        "GET",
        "/rest/billing/credits",
        200,
        r#"{"balance_cents": 1}"#,
    )
    .await;
    let provider = provider_for(&server);

    let error = provider
        .fetch_credits("authjs.session-token=x")
        .await
        .expect_err("missing fields");

    mock.assert_async().await;
    assert!(error.to_string().contains("invalid credit fields"));

    let mut html = mockito::Server::new_async().await;
    html.mock("GET", "/rest/billing/credits")
        .with_status(200)
        .with_body("<html>")
        .create_async()
        .await;
    let error = provider_for(&html)
        .fetch_credits("authjs.session-token=x")
        .await
        .expect_err("not json");
    assert!(error.to_string().contains("invalid JSON"));
}
