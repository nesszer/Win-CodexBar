//! Fixture tests for the Soniox organization billing provider, ported from
//! upstream `TestsPlugin/SonioxPluginTests.swift` and
//! `Tests/CodexBarTests/SonioxProviderTests.swift`. Fetch tests use an injected
//! API key and a local mock server, so no test reads the keyring, the
//! environment, or the real Soniox API.

use super::*;
use mockito::{Matcher, Server, ServerGuard};

const KEY: &str = "fixture-key";
const BALANCE_PATH: &str = "/v1/organization/balance";
const MONTHLY_PATH: &str = "/v1/organization/usage/monthly";

/// Synthetic bodies in the documented Soniox shape.
const BALANCE_FIXTURE: &str =
    r#"{"available_balance_usd":"18.50","invoice_billing_enabled":false}"#;
const MONTHLY_FIXTURE: &str =
    r#"{"month":"2026-10","usage_usd":"6.00","monthly_budget_usd":"100.00"}"#;

fn context(source_mode: SourceMode) -> FetchContext {
    FetchContext {
        source_mode,
        api_key: Some(KEY.to_string()),
        ..FetchContext::default()
    }
}

async fn serve(balance: (usize, &str), monthly: (usize, &str)) -> ServerGuard {
    let mut server = Server::new_async().await;
    for (path, (code, body)) in [(BALANCE_PATH, balance), (MONTHLY_PATH, monthly)] {
        server
            .mock("GET", path)
            .match_header("authorization", format!("Bearer {KEY}").as_str())
            .match_header("cookie", Matcher::Missing)
            .with_status(code)
            .with_header("content-type", "application/json")
            .with_body(body)
            .create_async()
            .await;
    }
    server
}

async fn fetch_with(
    balance: (usize, &str),
    monthly: (usize, &str),
) -> Result<ProviderFetchResult, ProviderError> {
    let server = serve(balance, monthly).await;
    SonioxProvider::with_base_url(&format!("{}/v1/organization", server.url()))
        .fetch_usage(&context(SourceMode::Auto))
        .await
}

async fn fetch_bodies(balance: &str, monthly: &str) -> Result<ProviderFetchResult, ProviderError> {
    fetch_with((200, balance), (200, monthly)).await
}

async fn fetch() -> ProviderFetchResult {
    fetch_bodies(BALANCE_FIXTURE, MONTHLY_FIXTURE)
        .await
        .expect("fixture fetch succeeds")
}

fn sections(result: &ProviderFetchResult) -> Vec<String> {
    let mut titles: Vec<String> = Vec::new();
    for detail in result.display_details() {
        let title = detail.section_title().unwrap_or_default().to_string();
        if !titles.contains(&title) {
            titles.push(title);
        }
    }
    titles
}

fn rows_in(result: &ProviderFetchResult, section: &str) -> Vec<(String, String)> {
    result
        .display_details()
        .iter()
        .filter(|detail| detail.section_title() == Some(section))
        .map(|detail| (detail.title().to_string(), detail.value().to_string()))
        .collect()
}

fn utc(year: i32, month: u32, day: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(year, month, day, 0, 0, 0)
        .single()
        .expect("valid fixture date")
}

#[test]
fn metadata_and_source_contract_match_the_api_key_provider() {
    let provider = SonioxProvider::new();
    assert_eq!(provider.id(), ProviderId::Soniox);
    assert_eq!(provider.metadata().display_name, "Soniox");
    assert_eq!(provider.metadata().session_label, "Monthly budget");
    assert_eq!(provider.metadata().weekly_label, "Usage");
    assert_eq!(
        provider.metadata().dashboard_url,
        Some("https://console.soniox.com")
    );
    assert_eq!(
        provider.metadata().status_page_url,
        Some("https://status.soniox.com")
    );
    assert!(!provider.metadata().default_enabled);
    assert_eq!(
        provider.available_sources(),
        vec![SourceMode::Auto, SourceMode::OAuth]
    );
    assert_eq!(ProviderId::Soniox.cookie_domain(), None);
    assert_eq!(
        ProviderId::from_cli_name("soniox"),
        Some(ProviderId::Soniox)
    );
    assert_eq!(
        ProviderId::from_cli_name("SONIOX"),
        Some(ProviderId::Soniox)
    );
}

#[tokio::test]
async fn budget_meter_and_prepaid_balance_stay_separate() {
    let result = fetch().await;

    let primary = &result.usage.primary;
    assert!(!primary.is_informational);
    assert!(primary.hidden_on_card);
    assert_eq!(primary.used_percent, 6.0);
    assert_eq!(primary.resets_at, Some(utc(2026, 11, 1)));
    assert_eq!(
        primary.reset_description.as_deref(),
        Some("$6.00 / $100.00")
    );
    assert!(result.usage.secondary.is_none());

    let cost = result.cost.as_ref().expect("cost");
    assert_eq!(cost.used, 6.0);
    assert_eq!(cost.limit, Some(100.0));
    assert_eq!(cost.currency_code, "USD");
    assert_eq!(cost.period, "This month");
    assert_eq!(cost.resets_at, primary.resets_at);
    assert_eq!(cost.balance, Some(18.5));
    assert!(cost.pay_as_you_go, "upstream payAsYouGoSpend card style");

    assert_eq!(sections(&result), vec!["Spend", "Billing"]);
    assert_eq!(
        rows_in(&result, "Spend"),
        vec![("Monthly budget".into(), "$6.00 of $100.00".into())]
    );
    let progress = result.display_details()[0]
        .progress()
        .expect("budget progress");
    assert!((progress.used() / progress.total() * 100.0 - 6.0).abs() < 0.000_001);
    assert_eq!(
        rows_in(&result, "Billing"),
        vec![("Prepaid".into(), "USD".into())]
    );

    assert_eq!(
        result.usage.login_method.as_deref(),
        Some("Organization API key")
    );
    assert_eq!(result.usage.account_email, None);
    assert_eq!(result.account_identity(), None);
    assert_eq!(result.source_label, "api");
}

#[tokio::test]
async fn invoice_billing_does_not_present_zero_prepaid_credits_as_exhausted() {
    let result = fetch_bodies(
        r#"{"available_balance_usd":"0","invoice_billing_enabled":true}"#,
        MONTHLY_FIXTURE,
    )
    .await
    .expect("invoice fetch succeeds");

    assert_eq!(result.usage.primary.used_percent, 6.0);
    let cost = result.cost.as_ref().expect("cost");
    assert_eq!(cost.used, 6.0);
    assert_eq!(cost.balance, None);
    assert_eq!(
        rows_in(&result, "Billing"),
        vec![("Invoice billing".into(), "USD".into())]
    );
}

#[tokio::test]
async fn zero_and_subcent_prepaid_balances_are_kept() {
    for amount in ["0", "0.001"] {
        let balance =
            format!(r#"{{"available_balance_usd":"{amount}","invoice_billing_enabled":false}}"#);
        let result = fetch_bodies(&balance, MONTHLY_FIXTURE)
            .await
            .expect("balance fetch succeeds");
        assert_eq!(
            result.cost.as_ref().and_then(|cost| cost.balance),
            Some(amount.parse::<f64>().expect("fixture amount")),
            "{amount}"
        );
    }
}

#[tokio::test]
async fn negative_prepaid_balance_is_accepted_but_not_shown_as_credits() {
    // Upstream displays a negative balance. The shared CostSnapshot balance is
    // non-negative, so the card omits it instead of inventing a value.
    let result = fetch_bodies(
        r#"{"available_balance_usd":"-0.50","invoice_billing_enabled":false}"#,
        MONTHLY_FIXTURE,
    )
    .await
    .expect("negative balance is not a parse failure");
    assert_eq!(result.cost.as_ref().and_then(|cost| cost.balance), None);
    assert_eq!(result.usage.primary.used_percent, 6.0);
}

#[tokio::test]
async fn zero_budget_keeps_spend_without_a_percentage() {
    let result = fetch_bodies(
        BALANCE_FIXTURE,
        r#"{"month":"2026-10","usage_usd":"0","monthly_budget_usd":"0"}"#,
    )
    .await
    .expect("zero budget fetch succeeds");

    assert!(result.usage.primary.is_informational);
    assert!(result.usage.primary.hidden_on_card);
    let cost = result.cost.as_ref().expect("cost");
    assert_eq!(cost.used, 0.0);
    assert_eq!(cost.limit, Some(0.0));
    assert_eq!(
        rows_in(&result, "Spend"),
        Vec::<(String, String)>::new(),
        "no budget progress row without a budget"
    );
    assert_eq!(
        rows_in(&result, "Billing"),
        vec![
            ("Prepaid".into(), "USD".into()),
            ("Monthly budget".into(), "$0.00".into()),
        ]
    );
}

#[tokio::test]
async fn exceeded_budget_clamps_the_percentage_and_keeps_the_true_spend() {
    let result = fetch_bodies(
        BALANCE_FIXTURE,
        r#"{"month":"2026-10","usage_usd":"100.25","monthly_budget_usd":"100"}"#,
    )
    .await
    .expect("exceeded budget fetch succeeds");

    assert_eq!(result.usage.primary.used_percent, 100.0);
    assert_eq!(result.cost.as_ref().map(|cost| cost.used), Some(100.25));
    let progress = result.display_details()[0]
        .progress()
        .expect("budget progress");
    assert_eq!(progress.used(), 100.0);
    assert_eq!(progress.total(), 100.0);
}

#[tokio::test]
async fn reset_rolls_over_the_year_and_leap_day_boundaries() {
    for (month, expected) in [("2026-12", utc(2027, 1, 1)), ("2028-02", utc(2028, 3, 1))] {
        let monthly =
            format!(r#"{{"month":"{month}","usage_usd":"0","monthly_budget_usd":"100"}}"#);
        let result = fetch_bodies(BALANCE_FIXTURE, &monthly)
            .await
            .expect("reset fetch succeeds");
        assert_eq!(result.usage.primary.resets_at, Some(expected), "{month}");
    }
}

#[tokio::test]
async fn malformed_billing_is_rejected_rather_than_reported_as_zero_usage() {
    let cases: [(&str, &str, &str); 10] = [
        ("balance", "not-json", "invalid JSON"),
        ("balance", "[]", "expected an object"),
        (
            "balance",
            r#"{"available_balance_usd":18.5,"invoice_billing_enabled":false}"#,
            "available_balance_usd",
        ),
        (
            "balance",
            r#"{"available_balance_usd":"","invoice_billing_enabled":false}"#,
            "available_balance_usd",
        ),
        (
            "balance",
            r#"{"available_balance_usd":"NaN","invoice_billing_enabled":false}"#,
            "available_balance_usd",
        ),
        (
            "balance",
            r#"{"available_balance_usd":"18.5","invoice_billing_enabled":"false"}"#,
            "invoice_billing_enabled",
        ),
        ("monthly", "{}", "usage_usd"),
        (
            "monthly",
            r#"{"month":"2026-10","usage_usd":"-1","monthly_budget_usd":"100"}"#,
            "usage_usd",
        ),
        (
            "monthly",
            r#"{"month":"2026-10","usage_usd":"6","monthly_budget_usd":null}"#,
            "monthly_budget_usd",
        ),
        (
            "monthly",
            r#"{"month":"2026-13","usage_usd":"6","monthly_budget_usd":"100"}"#,
            "month",
        ),
    ];
    for (endpoint, body, field) in cases {
        let (balance, monthly) = if endpoint == "balance" {
            (body, MONTHLY_FIXTURE)
        } else {
            (BALANCE_FIXTURE, body)
        };
        let error = fetch_bodies(balance, monthly)
            .await
            .expect_err("malformed billing must fail");
        let message = error.to_string();
        assert!(
            message.contains(&format!("invalid billing data: {field}.")),
            "{endpoint} {body}: {message}"
        );
        assert!(message.contains("Refresh to try again"), "{message}");
        assert!(
            message.contains("update CodexBar or report the issue"),
            "{message}"
        );
        assert!(matches!(error, ProviderError::Parse(_)), "{message}");
    }
}

#[tokio::test]
async fn http_failures_are_classified_without_echoing_response_bodies() {
    let private = r#"{"detail":"private-response-marker"}"#;
    for endpoint in ["balance", "monthly"] {
        for status in [401, 403, 429, 500, 503, 400, 404] {
            let (balance, monthly) = if endpoint == "balance" {
                ((status, private), (200, MONTHLY_FIXTURE))
            } else {
                ((200, BALANCE_FIXTURE), (status, private))
            };
            let error = fetch_with(balance, monthly)
                .await
                .expect_err("HTTP failure must surface");
            let message = error.to_string();
            assert!(!message.contains("private-response-marker"), "{message}");
            match status {
                401 => assert!(matches!(error, ProviderError::AuthRequired), "{message}"),
                403 => assert_eq!(message, PERMISSION_DENIED),
                429 => assert_eq!(message, RATE_LIMITED),
                500..=599 => assert_eq!(
                    message,
                    format!(
                        "Soniox is temporarily unavailable (HTTP {status}). Try again later or check https://status.soniox.com."
                    )
                ),
                _ => assert_eq!(
                    message,
                    format!("Soniox billing request failed (HTTP {status}). Refresh to try again.")
                ),
            }
        }
    }
}

#[tokio::test]
async fn a_failed_balance_read_stops_before_the_monthly_request() {
    // The monthly mock would succeed, so a surfaced 401 proves the balance
    // error decides the outcome and the monthly read was never needed.
    let error = fetch_with((401, "{}"), (200, MONTHLY_FIXTURE))
        .await
        .expect_err("balance 401 must surface");
    assert!(matches!(error, ProviderError::AuthRequired));
}

#[tokio::test]
async fn web_and_cli_sources_are_unsupported_and_oauth_uses_the_key_lane() {
    let provider = SonioxProvider::new();
    for source in [SourceMode::Web, SourceMode::Cli] {
        let error = provider
            .fetch_usage(&context(source))
            .await
            .expect_err("unsupported source");
        assert!(matches!(error, ProviderError::UnsupportedSource(_)));
    }

    let server = serve((200, BALANCE_FIXTURE), (200, MONTHLY_FIXTURE)).await;
    let oauth = SonioxProvider::with_base_url(&format!("{}/v1/organization", server.url()))
        .fetch_usage(&context(SourceMode::OAuth))
        .await
        .expect("oauth uses the same key lane");
    assert_eq!(oauth.source_label, "api");
}

#[test]
fn explicit_key_is_trimmed_and_used_before_any_lookup() {
    assert_eq!(
        resolve_key(Some("  snx_org_fixture  ")).expect("explicit key"),
        "snx_org_fixture"
    );
}

#[test]
fn money_accepts_only_plain_decimal_strings() {
    let body = |value: &str| serde_json::json!({ "usage_usd": value });
    assert_eq!(
        money(body("6.00").get("usage_usd"), "usage_usd", false).ok(),
        Some(6.0)
    );
    assert_eq!(
        money(body(" 6 ").get("usage_usd"), "usage_usd", false).ok(),
        Some(6.0)
    );
    assert!(money(body("1e3").get("usage_usd"), "usage_usd", false).is_err());
    assert!(money(body("+1").get("usage_usd"), "usage_usd", false).is_err());
    assert!(money(body("-1").get("usage_usd"), "usage_usd", false).is_err());
    assert_eq!(
        money(body("-1").get("usage_usd"), "usage_usd", true).ok(),
        Some(-1.0)
    );
}

#[test]
fn next_month_start_handles_december_and_leap_february() {
    assert_eq!(next_month_start("2026-12").ok(), Some(utc(2027, 1, 1)));
    assert_eq!(next_month_start("2028-02").ok(), Some(utc(2028, 3, 1)));
    assert!(next_month_start("2026-00").is_err());
}

#[test]
fn base_url_override_accepts_https_and_loopback_http() {
    assert_eq!(
        validated_base_url("https://api.soniox.com/v1/organization").as_deref(),
        Some("https://api.soniox.com/v1/organization")
    );
    assert_eq!(
        validated_base_url("http://127.0.0.1:4010/v1/organization/").as_deref(),
        Some("http://127.0.0.1:4010/v1/organization")
    );
    assert_eq!(
        validated_base_url("http://localhost:9/v1/organization").as_deref(),
        Some("http://localhost:9/v1/organization")
    );
    assert_eq!(
        validated_base_url("http://[::1]:9/v1/organization").as_deref(),
        Some("http://[::1]:9/v1/organization")
    );
    assert_eq!(
        validated_base_url("http://127.0.0.1:9/proxy/v1/organization").as_deref(),
        Some("http://127.0.0.1:9/proxy/v1/organization")
    );
}

#[test]
fn base_url_override_rejects_remote_plain_http_and_unsafe_forms() {
    for raw in [
        "http://api.soniox.com/v1/organization",
        "http://localhost.attacker.test/v1/organization",
        "ftp://127.0.0.1/v1/organization",
        "https://user:pass@api.soniox.com/v1/organization",
        "https://api.soniox.com/v1/organization?x=1",
        "https://api.soniox.com/v1/organization#frag",
        "http://127.0.0.1%2f.attacker.test/v1/organization",
        "https://api.soniox.com",
        "https://api.soniox.com/v1",
        "http://127.0.0.1:4010/",
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
        base_url_from_override(Some("http://attacker.example.com/v1/organization")),
        BASE_URL
    );
    assert_eq!(
        base_url_from_override(Some("\"http://127.0.0.1:4010/v1/organization\"")),
        "http://127.0.0.1:4010/v1/organization"
    );
}
