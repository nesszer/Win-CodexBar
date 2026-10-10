use super::*;
use crate::providers::test_support::{mock_response, mock_response_expect};
use chrono::{TimeZone, Utc};
use serde_json::{Value, json};

/// Public `/v1/key` example with synthetic values and a fixed reset
/// (upstream `DevPassPluginTests.fixture`).
fn fixture() -> Value {
    json!({
        "label": "Fixture key",
        "usage": "31.42",
        "limit": null,
        "devPlan": "pro",
        "devPlanCreditsUsed": "25",
        "devPlanCreditsLimit": "237",
        "devPlanCreditsRemaining": "212.00",
        "devPlanPremiumWeeklyLimit": "35.55",
        "devPlanPremiumCreditsUsed": "5.00",
        "devPlanPremiumWeekResetsAt": "2026-10-01T12:00:00.000Z",
    })
}

fn body(data: &Value) -> Vec<u8> {
    serde_json::to_vec(&json!({ "data": data })).unwrap()
}

fn parse(data: &Value) -> Result<ProviderFetchResult, ProviderError> {
    parse_result(&body(data))
}

fn with(key: &str, value: Value) -> Value {
    let mut data = fixture();
    data[key] = value;
    data
}

fn without(key: &str) -> Value {
    let mut data = fixture();
    data.as_object_mut().unwrap().remove(key);
    data
}

fn row<'a>(result: &'a ProviderFetchResult, id: &str) -> &'a ProviderDisplayDetail {
    result
        .display_details()
        .iter()
        .find(|row| row.id() == id)
        .unwrap_or_else(|| panic!("missing detail row {id}"))
}

fn assert_close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() < 1e-4,
        "{actual} is not close to {expected}"
    );
}

#[test]
fn documented_cycle_and_weekly_credits_keep_their_scope() {
    let result = parse(&fixture()).unwrap();
    let usage = &result.usage;

    assert_close(usage.primary.used_percent, 25.0 / 237.0 * 100.0);
    assert!(!usage.primary.is_informational);
    assert_eq!(usage.primary.resets_at, None);
    assert_eq!(usage.primary.window_minutes, None);

    let weekly = usage.secondary.as_ref().expect("premium weekly window");
    assert_close(weekly.used_percent, 5.0 / 35.55 * 100.0);
    assert_eq!(weekly.window_minutes, Some(10080));
    assert_eq!(
        weekly.resets_at,
        Some(Utc.with_ymd_and_hms(2026, 10, 1, 12, 0, 0).unwrap())
    );

    assert!(
        result.cost.is_none(),
        "an allowance is not a wallet balance"
    );
    assert_eq!(usage.login_method.as_deref(), Some("DevPass Pro"));
    assert_eq!(row(&result, "cycle-used").value(), "$25.00 / $237.00");
    assert_eq!(row(&result, "cycle-remaining").value(), "$212.00");
    assert_eq!(row(&result, "premium-weekly").value(), "$5.00 / $35.55");
    assert_eq!(row(&result, "key-usage").value(), "$31.42");
    assert!(
        result
            .display_details()
            .iter()
            .all(|row| row.id() != "key-limit"),
        "a null key limit has no row"
    );
    let progress = row(&result, "cycle-used").progress().unwrap();
    assert_close(progress.used(), 25.0);
    assert_close(progress.total(), 237.0);
}

#[test]
fn key_spending_limit_row_appears_when_the_key_has_one() {
    let result = parse(&with("limit", json!("50.5"))).unwrap();
    assert_eq!(row(&result, "key-limit").value(), "$50.50");
}

#[test]
fn every_plan_names_its_login_method() {
    for (plan, expected) in [
        ("lite", "DevPass Lite"),
        ("pro", "DevPass Pro"),
        ("max", "DevPass Max"),
    ] {
        let result = parse(&with("devPlan", json!(plan))).unwrap();
        assert_eq!(result.usage.login_method.as_deref(), Some(expected));
    }
}

#[test]
fn inactive_weekly_window_has_full_allowance_without_an_invented_reset() {
    let mut data = with("devPlanPremiumCreditsUsed", json!("0.00"));
    data["devPlanPremiumWeekResetsAt"] = Value::Null;
    let weekly = parse(&data).unwrap().usage.secondary.unwrap();
    assert_eq!(weekly.used_percent, 0.0);
    assert_eq!(weekly.resets_at, None);
    assert_eq!(weekly.window_minutes, Some(10080));
}

#[test]
fn offset_reset_instants_normalize_to_utc() {
    let data = with(
        "devPlanPremiumWeekResetsAt",
        json!("2026-10-01T14:00:00+02:00"),
    );
    let weekly = parse(&data).unwrap().usage.secondary.unwrap();
    assert_eq!(
        weekly.resets_at,
        Some(Utc.with_ymd_and_hms(2026, 10, 1, 12, 0, 0).unwrap())
    );
}

#[test]
fn pay_as_you_go_shows_only_key_scoped_all_time_spend() {
    let mut data = with("devPlan", json!("none"));
    for key in [
        "devPlanCreditsUsed",
        "devPlanCreditsLimit",
        "devPlanCreditsRemaining",
        "devPlanPremiumWeeklyLimit",
        "devPlanPremiumCreditsUsed",
    ] {
        data[key] = json!("0");
    }
    data["devPlanPremiumWeekResetsAt"] = Value::Null;

    let result = parse(&data).unwrap();
    assert!(result.usage.primary.is_informational);
    assert!(result.usage.secondary.is_none());
    assert!(result.cost.is_none());
    assert_eq!(result.usage.login_method.as_deref(), Some("Pay as you go"));
    let ids: Vec<_> = result.display_details().iter().map(|r| r.id()).collect();
    assert_eq!(ids, ["key-usage"]);
    assert_eq!(row(&result, "key-usage").value(), "$31.42");
}

#[test]
fn pay_as_you_go_ignores_plan_fields_and_keeps_the_key_limit() {
    let data = json!({
        "usage": "1.5",
        "limit": "10",
        "devPlan": "none",
        "devPlanCreditsUsed": 7,
    });
    let result = parse(&data).unwrap();
    assert_eq!(row(&result, "key-usage").value(), "$1.50");
    assert_eq!(row(&result, "key-limit").value(), "$10.00");
    assert_eq!(result.display_details().len(), 2);
}

#[test]
fn zero_limits_omit_bars_and_overages_keep_actual_spend() {
    let mut data = with("devPlanCreditsUsed", json!("250"));
    data["devPlanCreditsRemaining"] = json!("0");
    data["devPlanPremiumWeeklyLimit"] = json!("0");
    let result = parse(&data).unwrap();

    assert_eq!(result.usage.primary.used_percent, 100.0);
    assert!(result.usage.secondary.is_none());
    assert_eq!(row(&result, "cycle-used").value(), "$250.00 / $237.00");
    let progress = row(&result, "cycle-used").progress().unwrap();
    assert_close(progress.used(), 237.0);
    assert_eq!(row(&result, "premium-weekly").value(), "$5.00 / $0.00");
    assert!(row(&result, "premium-weekly").progress().is_none());
}

#[test]
fn a_zero_plan_allowance_is_informational() {
    let mut data = with("devPlanCreditsLimit", json!("0"));
    data["devPlanCreditsUsed"] = json!("0");
    let result = parse(&data).unwrap();
    assert!(result.usage.primary.is_informational);
    assert!(row(&result, "cycle-used").progress().is_none());
    assert!(result.usage.secondary.is_some());
}

#[test]
fn invalid_monetary_strings_fail_closed() {
    let huge = "9".repeat(400);
    for value in [
        "",
        " ",
        "NaN",
        "Infinity",
        "1e999",
        "1e2",
        "-1",
        "+1",
        "0x10",
        "1.",
        ".5",
        "1.2.3",
        "1,5",
        "25 ",
        "２５",
        huge.as_str(),
    ] {
        for key in ["usage", "devPlanCreditsUsed", "devPlanPremiumWeeklyLimit"] {
            assert!(
                matches!(
                    parse(&with(key, json!(value))),
                    Err(ProviderError::Parse(_))
                ),
                "{key} = {value:?}"
            );
        }
    }
}

#[test]
fn non_string_and_missing_amounts_fail_closed() {
    for key in [
        "usage",
        "limit",
        "devPlanCreditsUsed",
        "devPlanCreditsLimit",
        "devPlanCreditsRemaining",
        "devPlanPremiumWeeklyLimit",
        "devPlanPremiumCreditsUsed",
        "devPlanPremiumWeekResetsAt",
    ] {
        assert!(
            matches!(parse(&without(key)), Err(ProviderError::Parse(_))),
            "missing {key}"
        );
    }
    for key in ["usage", "limit", "devPlanCreditsUsed"] {
        assert!(
            matches!(parse(&with(key, json!(12))), Err(ProviderError::Parse(_))),
            "numeric {key}"
        );
    }
    assert!(matches!(
        parse(&with("usage", Value::Null)),
        Err(ProviderError::Parse(_))
    ));
}

#[test]
fn unknown_plans_and_invalid_reset_dates_fail_closed() {
    for plan in ["unexpected", "", "Pro", "PRO", "team"] {
        assert!(
            matches!(
                parse(&with("devPlan", json!(plan))),
                Err(ProviderError::Parse(_))
            ),
            "plan {plan:?}"
        );
    }
    assert!(matches!(
        parse(&without("devPlan")),
        Err(ProviderError::Parse(_))
    ));
    for reset in [
        "unexpected",
        "",
        "2026-10-01",
        "2026-10-01T12:00:00",
        "2026-10-01 12:00:00Z",
        "2026-10-01t12:00:00z",
        "2026-10-01T12:00:00z",
        "2026-13-01T12:00:00Z",
        "2026-10-01T12:00:60Z",
        "2026-10-01T12:00:00+0200",
    ] {
        assert!(
            matches!(
                parse(&with("devPlanPremiumWeekResetsAt", json!(reset))),
                Err(ProviderError::Parse(_))
            ),
            "reset {reset:?}"
        );
    }
    assert!(matches!(
        parse(&with("devPlanPremiumWeekResetsAt", json!(1_790_000_000))),
        Err(ProviderError::Parse(_))
    ));
}

#[test]
fn malformed_responses_are_parse_failures_that_do_not_echo_the_body() {
    for raw in [
        "not-json",
        "private-response",
        "{}",
        r#"{"data":null}"#,
        r#"{"data":[]}"#,
        "[]",
        "",
    ] {
        let error = parse_result(raw.as_bytes()).unwrap_err();
        assert!(matches!(error, ProviderError::Parse(_)), "{raw:?}");
        assert!(!error.to_string().contains("private-response"));
    }
}

#[test]
fn http_failures_are_classified_without_exposing_bodies() {
    for (code, expected) in [
        (401, "DevPass API key was rejected or is inactive."),
        (403, "DevPass requires a regular gateway API key."),
        (429, "DevPass usage requests are rate limited."),
        (500, "DevPass usage is temporarily unavailable."),
        (503, "DevPass usage is temporarily unavailable."),
        (400, "DevPass returned HTTP 400."),
        (404, "DevPass returned HTTP 404."),
        (204, "DevPass returned HTTP 204."),
    ] {
        let error = check_status(StatusCode::from_u16(code).unwrap()).unwrap_err();
        assert!(error.to_string().contains(expected), "{code}: {error}");
    }
    assert!(matches!(
        check_status(StatusCode::UNAUTHORIZED),
        Err(ProviderError::OAuthExpired(_))
    ));
    assert!(check_status(StatusCode::OK).is_ok());
}

fn key_url(server: &mockito::ServerGuard) -> String {
    format!("{}/v1/key", server.url())
}

#[tokio::test]
async fn fetch_sends_the_bearer_key_and_maps_the_fixture() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/v1/key")
        .match_header("authorization", "Bearer fixture-key")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(body(&fixture()))
        .create_async()
        .await;

    let result = fetch_key(
        &DevPassProvider::new().client,
        &key_url(&server),
        "fixture-key",
    )
    .await
    .unwrap();

    mock.assert_async().await;
    assert_eq!(result.source_label, "api");
    assert_eq!(row(&result, "cycle-remaining").value(), "$212.00");
}

#[tokio::test]
async fn fetch_never_echoes_an_error_body() {
    for (status, kind) in [
        (401, "OAuth"),
        (403, "Other"),
        (429, "Other"),
        (503, "Other"),
    ] {
        let mut server = mockito::Server::new_async().await;
        mock_response(&mut server, "GET", "/v1/key", status, "private-response").await;

        let error = fetch_key(&DevPassProvider::new().client, &key_url(&server), "k")
            .await
            .unwrap_err();

        assert!(
            !error.to_string().contains("private-response"),
            "{status}: {error}"
        );
        assert!(
            format!("{error:?}").starts_with(kind),
            "{status}: {error:?}"
        );
    }
}

#[tokio::test]
async fn fetch_does_not_follow_redirects_with_the_key() {
    let mut target = mockito::Server::new_async().await;
    let leaked =
        mock_response_expect(&mut target, "GET", "/v1/key", 200, body(&fixture()), 0).await;
    let mut origin = mockito::Server::new_async().await;
    origin
        .mock("GET", "/v1/key")
        .with_status(302)
        .with_header("location", &key_url(&target))
        .create_async()
        .await;

    let error = fetch_key(&DevPassProvider::new().client, &key_url(&origin), "secret")
        .await
        .unwrap_err();

    leaked.assert_async().await;
    assert!(error.to_string().contains("HTTP 302"), "{error}");
}

#[tokio::test]
async fn fetch_rejects_an_oversized_body() {
    let mut server = mockito::Server::new_async().await;
    mock_response(
        &mut server,
        "GET",
        "/v1/key",
        200,
        vec![b' '; MAX_BODY_BYTES + 1],
    )
    .await;

    let error = fetch_key(&DevPassProvider::new().client, &key_url(&server), "k")
        .await
        .unwrap_err();

    assert!(matches!(error, ProviderError::Parse(_)), "{error}");
}

#[test]
fn provider_registration_matches_upstream_metadata() {
    let provider = DevPassProvider::new();
    let metadata = provider.metadata();
    assert_eq!(metadata.display_name, "DevPass");
    assert_eq!(metadata.session_label, "Plan credits");
    assert_eq!(metadata.weekly_label, "Premium weekly");
    assert!(!metadata.default_enabled);
    assert_eq!(
        metadata.dashboard_url,
        Some("https://devpass.llmgateway.io/dashboard")
    );
    assert_eq!(
        ProviderId::from_cli_name("devpass"),
        Some(ProviderId::DevPass)
    );
    assert_eq!(ProviderId::DevPass.cli_name(), "devpass");
    assert_eq!(
        provider.available_sources(),
        [SourceMode::Auto, SourceMode::OAuth]
    );
}
