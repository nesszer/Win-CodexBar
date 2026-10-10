use chrono::{TimeZone, Utc};
use reqwest::redirect::Policy;

use super::snapshot::{editor_result, web_result};
use super::*;
use crate::providers::test_support::{mock_response_expect, mock_status, mock_status_expect};

// Fixtures mirror upstream v0.65.0 `ZedPluginTests.swift` / `ZedStatusProbeTests.swift`.
const BILLING: &str = r#"{"plan":"zed_pro","current_usage":{
  "token_spend":{"spend_in_cents":250,"limit_in_cents":1000},
  "edit_predictions":{"used":12,"limit":100}}}"#;

fn editor_body(plan: &str, used: u32, limit: &str, overdue: bool) -> String {
    format!(
        r#"{{"user":{{"id":4242,"github_login":"octocat","name":"The Octocat"}},
        "feature_flags":[],
        "plan":{{"plan_v3":"{plan}",
          "subscription_period":{{"started_at":"2026-05-13T00:00:00.000Z","ended_at":"2026-06-13T00:00:00.000Z"}},
          "usage":{{"edit_predictions":{{"used":{used},"limit":{limit}}}}},
          "has_overdue_invoices":{overdue}}}}}"#
    )
}

fn detail_value<'a>(result: &'a ProviderFetchResult, id: &str) -> Option<&'a str> {
    result
        .display_details()
        .iter()
        .find(|row| row.id() == id)
        .map(|row| row.value())
}

fn now() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 6, 1, 0, 0, 0).unwrap()
}

#[test]
fn browser_billing_reports_dollar_spend_and_plan() {
    let result = web_result(BILLING.as_bytes()).unwrap();
    assert_eq!(result.source_label, "web");
    let cost = result.cost.as_ref().unwrap();
    assert_eq!(cost.used, 2.5);
    assert_eq!(cost.limit, Some(10.0));
    assert_eq!(cost.currency_code, "USD");
    assert_eq!(cost.period, "Current billing period");
    assert_eq!(cost.balance, None);
    assert_eq!(detail_value(&result, "token-spend"), Some("$2.50"));
    assert_eq!(detail_value(&result, "token-spend-limit"), Some("$10.00"));
    assert_eq!(
        detail_value(&result, "token-spend-remaining"),
        Some("$7.50")
    );
    assert_eq!(result.usage.primary.used_percent, 12.0);
    assert_eq!(
        result.usage.primary.reset_description.as_deref(),
        Some("12 / 100 predictions")
    );
    assert_eq!(result.usage.login_method.as_deref(), Some("Zed Pro"));
    assert!(result.usage.account_email.is_none());
}

#[test]
fn zero_and_overage_spend_keep_exact_amounts_and_floor_remaining() {
    for cents in ["0", "12.5", "250", "1500"] {
        let body = BILLING.replace("250", cents);
        let result = web_result(body.as_bytes()).unwrap();
        let spent = cents.parse::<f64>().unwrap() / 100.0;
        assert_eq!(result.cost.as_ref().unwrap().used, spent);
        assert_eq!(
            detail_value(&result, "token-spend-remaining"),
            Some(format!("${:.2}", (10.0 - spent).max(0.0)).as_str())
        );
    }
}

#[test]
fn missing_or_null_spend_cap_and_null_limit_do_not_invent_limits() {
    for cap in [
        BILLING.replace("1000", "null"),
        BILLING.replace(",\"limit_in_cents\":1000", ""),
    ] {
        let body = cap.replace("\"limit\":100", "\"limit\":null");
        let result = web_result(body.as_bytes()).unwrap();
        assert!(result.cost.is_none());
        assert_eq!(detail_value(&result, "token-spend"), Some("$2.50"));
        assert_eq!(
            detail_value(&result, "token-spend-limit"),
            Some("Not reported")
        );
        assert_eq!(detail_value(&result, "token-spend-remaining"), None);
        assert_eq!(
            result.usage.primary.reset_description.as_deref(),
            Some("Unlimited")
        );
        assert_eq!(result.usage.primary.used_percent, 0.0);
    }
}

#[test]
fn drifted_and_unsafe_billing_values_fail_without_publishing_totals() {
    let bodies = [
        "{}".to_string(),
        "[]".to_string(),
        "<html>login</html>".to_string(),
        BILLING.replace("250", "-1"),
        BILLING.replace("250", "true"),
        BILLING.replace("250", "9007199254740992"),
        BILLING.replace("1000", "-1"),
        BILLING.replace("\"plan\":\"zed_pro\"", "\"plan\":{}"),
        BILLING.replace("\"plan\":\"zed_pro\"", "\"plan\":\"  \""),
        BILLING.replace("\"used\":12", "\"used\":1.5"),
        BILLING.replace("\"used\":12", "\"used\":-1"),
        BILLING.replace("\"limit\":100", "\"limit\":\"lots\""),
        BILLING.replace("\"limit\":100", "\"other\":100"),
        BILLING.replace("\"limit\":100", "\"limit\":[5]"),
        BILLING.replace("\"limit\":100", "\"limit\":{\"limited\":-2}"),
        BILLING.replace("edit_predictions", "edit_prediction"),
    ];
    for body in bodies {
        assert!(
            matches!(web_result(body.as_bytes()), Err(ProviderError::Parse(_))),
            "{body}"
        );
    }
}

#[test]
fn edit_prediction_limit_accepts_unlimited_null_integer_and_limited_object() {
    let unlimited = BILLING.replace("\"limit\":100", "\"limit\":\"unlimited\"");
    let limited = BILLING.replace("\"limit\":100", "\"limit\":{\"limited\":20}");
    let overage = BILLING.replace("\"used\":12,\"limit\":100", "\"used\":150,\"limit\":100");
    let zero = BILLING.replace("\"limit\":100", "\"limit\":0");
    let primary = |body: &str| web_result(body.as_bytes()).unwrap().usage.primary;

    assert_eq!(
        primary(&unlimited).reset_description.as_deref(),
        Some("Unlimited")
    );
    let limited = primary(&limited);
    assert_eq!(limited.used_percent, 60.0);
    assert_eq!(
        limited.reset_description.as_deref(),
        Some("12 / 20 predictions")
    );
    let overage = primary(&overage);
    assert_eq!(overage.used_percent, 100.0);
    assert_eq!(
        overage.reset_description.as_deref(),
        Some("100 / 100 predictions")
    );
    assert!(primary(&zero).is_informational);
}

#[test]
fn editor_payload_retains_identity_quota_billing_dates_and_overdue_warning() {
    let body = editor_body("zed_pro_trial", 10, r#"{"limited":20}"#, true);
    let result = editor_result(body.as_bytes(), now()).unwrap();
    let usage = &result.usage;
    assert_eq!(result.source_label, "api");
    assert_eq!(usage.account_email.as_deref(), Some("octocat"));
    assert_eq!(usage.account_organization.as_deref(), Some("The Octocat"));
    assert_eq!(usage.login_method.as_deref(), Some("Zed Pro Trial"));
    assert_eq!(usage.primary.used_percent, 50.0);
    assert_eq!(
        usage.primary.reset_description.as_deref(),
        Some("10 / 20 predictions")
    );
    let cycle = usage.secondary.as_ref().unwrap();
    let end = Utc.with_ymd_and_hms(2026, 6, 13, 0, 0, 0).unwrap();
    assert_eq!(cycle.resets_at, Some(end));
    assert_eq!(
        cycle.reset_description.as_deref(),
        Some("Cycle ends in 12d 0h")
    );
    // 19 of 31 days elapsed.
    assert!((cycle.used_percent - 19.0 / 31.0 * 100.0).abs() < 1e-9);
    assert_eq!(usage.subscription.as_ref().unwrap().renews_at, Some(end));
    let overdue = &usage.extra_rate_windows[0];
    assert_eq!(overdue.id, "zed.overdue-invoices");
    assert_eq!(overdue.title, "Billing");
    assert!(!overdue.usage_known);
    assert_eq!(
        overdue.window.reset_description.as_deref(),
        Some("Overdue invoices")
    );
    assert!(result.cost.is_none());
}

#[test]
fn editor_cycle_reports_ended_and_short_countdowns() {
    let body = editor_body("zed_pro", 0, "\"unlimited\"", false);
    let describe = |now| {
        editor_result(body.as_bytes(), now)
            .unwrap()
            .usage
            .secondary
            .unwrap()
            .reset_description
            .unwrap()
    };
    let end = Utc.with_ymd_and_hms(2026, 6, 13, 0, 0, 0).unwrap();
    assert_eq!(describe(end + chrono::Duration::seconds(1)), "Cycle ended");
    assert_eq!(
        describe(end - chrono::Duration::minutes(150)),
        "Cycle ends in 2h 30m"
    );
    assert_eq!(
        describe(end - chrono::Duration::minutes(45)),
        "Cycle ends in 45m"
    );
}

#[test]
fn editor_plans_retain_names_and_unlimited_predictions() {
    for (plan, label) in [
        ("zed_free", "Zed Free"),
        ("zed_pro", "Zed Pro"),
        ("zed_pro_trial", "Zed Pro Trial"),
        ("zed_student", "Zed Student"),
        ("zed_business", "Zed Business"),
    ] {
        let body = editor_body(plan, 3, "\"unlimited\"", false);
        let usage = editor_result(body.as_bytes(), now()).unwrap().usage;
        assert_eq!(usage.login_method.as_deref(), Some(label));
        assert_eq!(usage.primary.used_percent, 0.0);
        assert_eq!(
            usage.primary.reset_description.as_deref(),
            Some("Unlimited")
        );
        assert!(usage.extra_rate_windows.is_empty());
    }
}

#[test]
fn editor_payload_rejects_drifted_shapes() {
    let good = editor_body("zed_pro", 1, "5", false);
    let bodies = [
        good.replace("\"id\":4242", "\"id\":\"4242\""),
        good.replace("\"github_login\":\"octocat\"", "\"github_login\":7"),
        good.replace("\"name\":\"The Octocat\"", "\"name\":7"),
        good.replace(
            "\"has_overdue_invoices\":false",
            "\"has_overdue_invoices\":\"no\"",
        ),
        good.replace("2026-06-13T00:00:00.000Z", "not-a-date"),
        good.replace("\"plan_v3\":\"zed_pro\"", "\"plan_v3\":\"\""),
        // `null` is unlimited only in the browser payload.
        editor_body("zed_pro", 1, "null", false),
        // The legacy lenient shape is not an upstream wire shape.
        r#"{"plan":{"usage":{"editPredictions":{"used":50,"limit":200}}}}"#.to_string(),
    ];
    for body in bodies {
        assert!(
            matches!(
                editor_result(body.as_bytes(), now()),
                Err(ProviderError::Parse(_))
            ),
            "{body}"
        );
    }
}

#[test]
fn editor_optional_identity_and_period_may_be_absent_or_blank() {
    let body = r#"{"user":{"id":1,"github_login":" ","name":null},
        "plan":{"plan_v3":"zed_free","usage":{"edit_predictions":{"used":0,"limit":50}},
        "has_overdue_invoices":false}}"#;
    let usage = editor_result(body.as_bytes(), now()).unwrap().usage;
    assert!(usage.account_email.is_none());
    assert!(usage.account_organization.is_none());
    assert!(usage.secondary.is_none());
    assert!(usage.subscription.is_none());
    assert!(usage.extra_rate_windows.is_empty());
}

#[test]
fn classifies_statuses_per_lane() {
    assert!(matches!(
        status_error(StatusCode::UNAUTHORIZED, true),
        ProviderError::Other(message) if message == SESSION_EXPIRED
    ));
    assert!(matches!(
        status_error(StatusCode::FORBIDDEN, true),
        ProviderError::Other(message) if message == SESSION_EXPIRED
    ));
    assert!(matches!(
        status_error(StatusCode::FORBIDDEN, false),
        ProviderError::AuthRequired
    ));
    assert!(matches!(
        status_error(StatusCode::TOO_MANY_REQUESTS, true),
        ProviderError::Other(message) if message.contains("rate limited")
    ));
    assert!(matches!(
        status_error(StatusCode::BAD_GATEWAY, true),
        ProviderError::Other(message) if message.contains("unavailable") && message.contains("502")
    ));
    assert!(matches!(
        status_error(StatusCode::NOT_FOUND, true),
        ProviderError::Other(message) if message.contains("404")
    ));
}

#[test]
fn expired_browser_session_is_classified_as_expired_not_unknown() {
    let provider = ZedProvider::new();
    assert_eq!(
        provider.error_state_kind(&status_error(StatusCode::UNAUTHORIZED, true)),
        ProviderStateKind::ExpiredSession
    );
    assert_eq!(
        provider.error_state_kind(&ProviderError::Other("other".into())),
        ProviderStateKind::Unknown
    );
}

#[test]
fn sources_are_auto_web_and_api_with_web_opt_in() {
    let provider = ZedProvider::new();
    assert_eq!(
        provider.available_sources(),
        vec![SourceMode::Auto, SourceMode::Web, SourceMode::OAuth]
    );
    assert!(provider.web_is_opt_in());
}

#[tokio::test]
async fn web_source_sends_only_the_zed_cookie_and_no_authorization() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/frontend/billing/usage")
        .match_header("accept", "application/json")
        .match_header("cookie", "zed.session=fixture-session")
        .match_header("authorization", mockito::Matcher::Missing)
        .with_status(200)
        .with_body(BILLING)
        .create_async()
        .await;
    let provider = test_provider(&server.url());
    let ctx = FetchContext {
        source_mode: SourceMode::Web,
        manual_cookie_header: Some("Cookie: zed.session=fixture-session".into()),
        // An editor credential must never be sent by, or substituted for, Web.
        api_key: Some("4242 editor-token".into()),
        ..FetchContext::default()
    };

    let result = provider.fetch_usage(&ctx).await.unwrap();
    mock.assert_async().await;
    assert_eq!(result.source_label, "web");
    assert_eq!(result.cost.unwrap().used, 2.5);
}

#[tokio::test]
async fn web_source_maps_expired_and_failing_responses_without_editor_fallback() {
    for (status, expected) in [
        (401, SESSION_EXPIRED),
        (403, SESSION_EXPIRED),
        (429, "Zed usage requests are rate limited."),
        (503, "Zed cloud API is unavailable (HTTP 503)."),
        (404, "Zed cloud API returned HTTP 404."),
    ] {
        let mut server = mockito::Server::new_async().await;
        let mock = mock_response_expect(
            &mut server,
            "GET",
            "/frontend/billing/usage",
            status,
            "<html>login</html>",
            1,
        )
        .await;
        let ctx = FetchContext {
            source_mode: SourceMode::Web,
            manual_cookie_header: Some("zed.session=fixture-session".into()),
            api_key: Some("4242 editor-token".into()),
            // Would be hit if Web fell back to the editor credential.
            workspace_id: Some(format!("{}/client/users/me", server.url())),
            ..FetchContext::default()
        };

        let error = test_provider(&server.url())
            .fetch_usage(&ctx)
            .await
            .err()
            .unwrap();
        mock.assert_async().await;
        assert!(
            matches!(&error, ProviderError::Other(message) if message == expected),
            "{status}: {error}"
        );
    }
}

#[tokio::test]
async fn web_source_without_a_usable_cookie_fails_before_any_request() {
    for ctx in [
        FetchContext {
            source_mode: SourceMode::Web,
            manual_cookie_header: Some("  ".into()),
            ..FetchContext::default()
        },
        FetchContext {
            source_mode: SourceMode::Web,
            manual_cookie_header: Some("zed.session=bad\nvalue".into()),
            ..FetchContext::default()
        },
        FetchContext {
            source_mode: SourceMode::Web,
            manual_cookie_missing: true,
            ..FetchContext::default()
        },
        // Cookie source Off: the shell maps it to Cli.
        FetchContext {
            source_mode: SourceMode::Cli,
            manual_cookie_header: Some("zed.session=fixture-session".into()),
            ..FetchContext::default()
        },
    ] {
        let mut server = mockito::Server::new_async().await;
        let mock = mock_status_expect(&mut server, "GET", mockito::Matcher::Any, 200, 0).await;
        let error = test_provider(&server.url())
            .fetch_usage(&ctx)
            .await
            .err()
            .unwrap();
        mock.assert_async().await;
        assert!(matches!(error, ProviderError::NotInstalled(_)), "{error}");
    }
}

#[tokio::test]
async fn auto_and_api_sources_use_the_editor_credential_and_never_the_browser() {
    for source_mode in [SourceMode::Auto, SourceMode::OAuth] {
        let mut server = mockito::Server::new_async().await;
        let editor = server
            .mock("GET", "/client/users/me")
            .match_header("authorization", "4242 fixture-token")
            .match_header("cookie", mockito::Matcher::Missing)
            .with_status(200)
            .with_body(editor_body("zed_pro", 10, r#"{"limited":20}"#, false))
            .create_async()
            .await;
        let billing =
            mock_status_expect(&mut server, "GET", "/frontend/billing/usage", 200, 0).await;
        let ctx = FetchContext {
            source_mode,
            api_key: Some("4242 fixture-token".into()),
            manual_cookie_header: Some("zed.session=fixture-session".into()),
            workspace_id: Some(format!("{}/client/users/me", server.url())),
            ..FetchContext::default()
        };

        let result = test_provider(&server.url())
            .fetch_usage(&ctx)
            .await
            .unwrap();
        editor.assert_async().await;
        billing.assert_async().await;
        assert_eq!(result.source_label, "api");
        assert_eq!(result.usage.account_email.as_deref(), Some("octocat"));
    }
}

#[tokio::test]
async fn editor_lane_keeps_auth_required_for_rejected_credentials() {
    let mut server = mockito::Server::new_async().await;
    let _mock = mock_status(&mut server, "GET", "/client/users/me", 401).await;
    let ctx = FetchContext {
        source_mode: SourceMode::Auto,
        api_key: Some("4242 stale".into()),
        workspace_id: Some(format!("{}/client/users/me", server.url())),
        ..FetchContext::default()
    };

    assert!(matches!(
        test_provider(&server.url()).fetch_usage(&ctx).await,
        Err(ProviderError::AuthRequired)
    ));
}

fn test_provider(base_url: &str) -> ZedProvider {
    let client = Client::builder().redirect(Policy::none()).build().unwrap();
    ZedProvider::with_client(format!("{base_url}/frontend/billing/usage"), client)
}
