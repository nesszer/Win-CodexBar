use super::*;
use crate::core::ProviderDisplayDetail;
use crate::providers::test_support::{mock_response, mock_response_expect};
use mockito::Matcher;
use serde_json::{Value, json};

const FIXTURE: &str = include_str!("fixtures/key-usage.json");
const API_KEY: &str = "gak_fixture";
const KEY_ID: &str = "11111111-1111-4111-8111-111111111111";

fn body() -> Value {
    serde_json::from_str(FIXTURE).unwrap()
}

fn result(body: &Value) -> Result<ProviderFetchResult, ProviderError> {
    model::parse_key_usage(&body.to_string()).map(present::build_result)
}

fn assert_parse_error(body: &Value) {
    match result(body) {
        Err(ProviderError::Parse(message)) => {
            assert!(message.starts_with("Aixy returned"), "{message}");
        }
        other => panic!("expected a parse failure, got {other:?}"),
    }
}

fn context(base: &str) -> FetchContext {
    FetchContext {
        gateway_url: Some(base.to_owned()),
        api_key: Some(API_KEY.to_owned()),
        ..FetchContext::default()
    }
}

fn detail<'a>(result: &'a ProviderFetchResult, id: &str) -> &'a ProviderDisplayDetail {
    result
        .display_details()
        .iter()
        .find(|row| row.id() == id)
        .unwrap_or_else(|| panic!("missing detail row {id}"))
}

#[test]
fn key_usage_keeps_overlapping_budgets_and_reservations_separate() {
    let result = result(&body()).unwrap();
    let usage = &result.usage;

    assert_eq!(usage.primary.used_percent, 30.0);
    assert_eq!(usage.primary.window_minutes, Some(43_200));
    let secondary = usage.secondary.as_ref().unwrap();
    assert_eq!(secondary.used_percent, 40.0);
    assert_eq!(secondary.window_minutes, Some(10_080));
    let description = usage.primary.reset_description.as_deref().unwrap();
    assert!(description.contains("Shared · Hard"), "{description}");
    assert!(description.contains("$70.00 remaining"), "{description}");
    assert!(usage.primary.resets_at.is_some());

    assert_eq!(usage.extra_rate_windows.len(), 1);
    let extra = &usage.extra_rate_windows[0];
    assert_eq!(
        extra.id,
        "aixy-55555555-5555-4555-8555-555555555555".to_owned()
    );
    assert!(!extra.usage_known);
    assert!(!extra.window.usage_known());
    assert!(extra.window.resets_at.is_some());
    assert!(
        extra
            .window
            .reset_description
            .as_deref()
            .unwrap()
            .ends_with("Unavailable")
    );

    let cost = result.cost.as_ref().unwrap();
    assert_eq!(cost.used, 1.25);
    assert_eq!(cost.currency_code, "USD");
    assert_eq!(cost.period, "Last 7 days · attributed");

    assert_eq!(usage.login_method.as_deref(), Some("API key"));
    assert!(usage.account_email.is_none());
    assert!(usage.account_organization.is_none());
    assert_eq!(result.account_identity(), Some(KEY_ID));
    assert_eq!(result.source_label, "api");

    assert_eq!(detail(&result, "key").value(), "Developer CLI");
    assert_eq!(detail(&result, "key").section_title(), Some("Aixy key"));
    assert_eq!(detail(&result, "project").value(), "Engineering");
    assert_eq!(
        detail(&result, "observed").value(),
        "2026-09-24T12:00:00.000Z"
    );
    let hard = detail(&result, "budget-0");
    assert_eq!(hard.section_title(), Some("Applicable budgets"));
    assert_eq!(hard.title(), "Project · Monthly · Shared · Hard");
    assert_eq!(hard.value(), "$70.00 / $100.00 remaining");
    assert_eq!(
        hard.secondary_value(),
        Some("$20.00 spent · $10.00 reserved")
    );
    assert_eq!(hard.progress().unwrap().used(), 30.0);
    let monitor = detail(&result, "budget-2");
    assert_eq!(monitor.title(), "User · Weekly · Personal · Monitor");
    assert_eq!(monitor.secondary_value(), Some("$40.00 spent"));
    assert_eq!(detail(&result, "budget-1").value(), "Unavailable");
    assert_eq!(detail(&result, "requests-7d").value(), "12");
    assert_eq!(detail(&result, "tokens-7d").value(), "1,200");
    assert_eq!(detail(&result, "spend-7d").value(), "$1.25");
    let coverage = detail(&result, "coverage-7d");
    assert_eq!(coverage.section_title(), Some("Last 7 days · this key"));
    assert_eq!(coverage.value(), "10 / 12 requests");
    assert_eq!(coverage.secondary_value(), Some("2 partial"));
}

#[test]
fn budgets_sort_hard_first_then_known_then_utilization_then_id() {
    let usage = model::parse_key_usage(FIXTURE).unwrap();
    let order = usage
        .budgets
        .iter()
        .map(|budget| (budget.hard, budget.known))
        .collect::<Vec<_>>();
    assert_eq!(order, [(true, true), (true, false), (false, true)]);
}

#[test]
fn absent_budgets_never_become_an_invented_quota() {
    let mut body = body();
    body["budgets"] = json!([]);
    let with_usage = result(&body).unwrap();
    assert!(with_usage.usage.primary.is_informational);
    assert_eq!(
        with_usage.usage.primary.reset_description.as_deref(),
        Some("No applicable budgets reported")
    );
    assert!(with_usage.usage.secondary.is_none());
    assert_eq!(with_usage.cost.as_ref().unwrap().used, 1.25);

    body["usage"] = Value::Null;
    let without_usage = result(&body).unwrap();
    assert!(without_usage.cost.is_none());
    let rows = without_usage.display_details();
    assert_eq!(rows.last().unwrap().value(), "Unavailable");
}

#[test]
fn all_unknown_budgets_are_listed_without_a_measured_lane() {
    let mut body = body();
    let budgets = body["budgets"].as_array_mut().unwrap();
    budgets.retain(|budget| budget["id"] == "55555555-5555-4555-8555-555555555555");
    let result = result(&body).unwrap();
    assert!(result.usage.primary.is_informational);
    assert_eq!(
        result.usage.primary.reset_description.as_deref(),
        Some("Budget balance unavailable")
    );
    assert_eq!(result.usage.extra_rate_windows.len(), 1);
    assert!(!result.usage.extra_rate_windows[0].usage_known);
}

#[test]
fn zero_activity_distinguishes_zero_from_unavailable_spend() {
    let mut body = body();
    for field in [
        "requests",
        "input_tokens",
        "output_tokens",
        "total_tokens",
        "spend_usd",
        "attributed_requests",
        "estimated_requests",
        "provider_reported_requests",
        "reconciled_requests",
        "partial_requests",
    ] {
        body["usage"][field] = json!(0);
    }
    let zero = result(&body).unwrap();
    assert_eq!(zero.usage.primary.used_percent, 30.0);
    assert_eq!(zero.cost.as_ref().unwrap().used, 0.0);
    assert_eq!(detail(&zero, "spend-7d").value(), "$0.00");

    body["usage"]["spend_usd"] = Value::Null;
    let unavailable = result(&body).unwrap();
    assert!(unavailable.cost.is_none());
    assert_eq!(detail(&unavailable, "spend-7d").value(), "Unavailable");

    body["usage"]["spend_usd"] = json!(1);
    assert_parse_error(&body);
}

#[test]
fn invalid_or_cross_key_responses_cannot_publish_misleading_balances() {
    type Mutation = fn(&mut Value);
    let cases: [(&str, Mutation); 12] = [
        ("currency", |b| b["currency"] = json!("EUR")),
        ("contract", |b| b["object"] = json!("other")),
        ("key", |b| {
            b["key"] = json!({ "id": "different-key", "project_id": "different-project" });
        }),
        ("amount", |b| b["budgets"][0]["limit_usd"] = json!("NaN")),
        ("negative", |b| b["budgets"][0]["limit_usd"] = json!(-1)),
        ("zero limit", |b| b["budgets"][0]["limit_usd"] = json!(0)),
        ("duplicate", |b| {
            let first = b["budgets"][0].clone();
            b["budgets"].as_array_mut().unwrap().push(first);
        }),
        ("date", |b| b["budgets"][0]["resets_at"] = json!("invalid")),
        ("period", |b| {
            b["budgets"][0]["resets_at"] = b["budgets"][0]["starts_at"].clone();
        }),
        ("coverage", |b| {
            b["usage"]["attributed_requests"] = json!(13)
        }),
        ("tokens", |b| b["usage"]["total_tokens"] = json!(1e30)),
        ("window", |b| b["usage"]["window"] = json!("30d")),
    ];
    for (name, mutate) in cases {
        let mut body = body();
        mutate(&mut body);
        match result(&body) {
            Err(ProviderError::Parse(_)) => {}
            other => panic!("{name}: expected a parse failure, got {other:?}"),
        }
    }
}

#[test]
fn other_contract_violations_are_rejected() {
    type Mutation = fn(&mut Value);
    let cases: [(&str, Mutation); 9] = [
        ("scope", |b| b["budgets"][0]["scope"] = json!("galaxy")),
        ("interval", |b| {
            b["budgets"][0]["interval"] = json!("hourly")
        }),
        ("enforcement", |b| {
            b["budgets"][0]["enforcement"] = json!("soft")
        }),
        ("shared", |b| b["budgets"][0]["shared"] = json!("yes")),
        ("empty applies_to", |b| {
            b["budgets"][0]["applies_to"] = json!([])
        }),
        ("availability", |b| {
            b["budgets"][0]["availability"]["status"] = json!("maybe");
        }),
        ("spend status", |b| {
            b["budgets"][0]["spend_status"] = json!("maybe")
        }),
        ("as_of", |b| b["as_of"] = json!("yesterday")),
        ("incomplete balance", |b| {
            b["budgets"][0]["availability"]["reserved_usd"] = Value::Null;
        }),
    ];
    for (name, mutate) in cases {
        let mut body = body();
        mutate(&mut body);
        match result(&body) {
            Err(ProviderError::Parse(_)) => {}
            other => panic!("{name}: expected a parse failure, got {other:?}"),
        }
    }

    let mut body = body();
    let template = body["budgets"][0].clone();
    body["budgets"] = Value::Array(
        (0..65)
            .map(|index| {
                let mut budget = template.clone();
                budget["id"] = json!(format!("budget-{index}"));
                budget
            })
            .collect(),
    );
    assert_parse_error(&body);
    body["budgets"].as_array_mut().unwrap().pop();
    assert_eq!(result(&body).unwrap().usage.extra_rate_windows.len(), 62);
}

#[test]
fn only_the_selected_balance_branch_is_validated() {
    // A monitor budget reads its spend fields; a malformed hard availability
    // block is ignored, and vice versa.
    let mut body = body();
    body["budgets"][1]["availability"]["spent_usd"] = json!("not a number");
    assert!(result(&body).is_ok());
    body["budgets"][0]["spend_usd"] = json!("not a number");
    assert!(result(&body).is_ok());
}

#[test]
fn amounts_and_counts_accept_numbers_and_decimal_strings_only() {
    let mut body = body();
    body["budgets"][0]["limit_usd"] = json!("100.50");
    body["usage"]["requests"] = json!("12");
    assert!(result(&body).is_ok());
    for bad in [
        json!("1e2"),
        json!(" 5"),
        json!("5."),
        json!(".5"),
        json!(true),
    ] {
        body["budgets"][0]["limit_usd"] = bad.clone();
        assert_parse_error(&body);
    }
    body["budgets"][0]["limit_usd"] = json!("100");
    body["usage"]["requests"] = json!(12.5);
    assert_parse_error(&body);
}

#[test]
fn display_text_is_sanitized_and_bounded() {
    let mut body = body();
    body["key"]["name"] = json!(format!("bad\u{7}\nname{}", "x".repeat(300)));
    let result = result(&body).unwrap();
    let key = detail(&result, "key").value();
    assert!(!key.chars().any(char::is_control));
    assert_eq!(key.chars().count(), 120);
}

#[test]
fn usage_url_supports_every_documented_base_form() {
    for (base, expected) in [
        (
            "https://api.aixy-gateway.com",
            "https://api.aixy-gateway.com/v1/usage",
        ),
        (
            "https://api.aixy-gateway.com/",
            "https://api.aixy-gateway.com/v1/usage",
        ),
        (
            "https://api.aixy-gateway.com/v1",
            "https://api.aixy-gateway.com/v1/usage",
        ),
        (
            "https://aixy.example.com/prefix/v1/",
            "https://aixy.example.com/prefix/v1/usage",
        ),
        ("aixy.example.com", "https://aixy.example.com/v1/usage"),
        ("http://localhost:8080", "http://localhost:8080/v1/usage"),
        (
            "http://gateway.local/team",
            "http://gateway.local/team/v1/usage",
        ),
        ("http://10.1.2.3:8080/v1", "http://10.1.2.3:8080/v1/usage"),
    ] {
        assert_eq!(usage_url(base).unwrap().as_str(), expected, "{base}");
    }
}

#[test]
fn usage_url_rejects_unsafe_bases() {
    for base in [
        "",
        "http://api.aixy-gateway.com",
        "http://8.8.8.8",
        "ftp://10.1.2.3",
        "https://user:secret@aixy.example.com",
        "https://aixy.example.com?x=1",
        "https://aixy.example.com/prefix#frag",
    ] {
        assert!(usage_url(base).is_err(), "{base}");
    }
    let message = usage_url("https://aixy.example.com?x=1")
        .unwrap_err()
        .to_string();
    assert!(message.contains("must not contain a query or fragment"));
    assert!(
        validate_gateway_url("").is_ok(),
        "empty selects the hosted gateway"
    );
    assert!(validate_gateway_url("http://localhost:9").is_ok());
    assert!(validate_gateway_url("http://public.example.com").is_err());
}

#[tokio::test]
async fn rejects_public_http_before_resolving_any_credential() {
    let provider = AixyProvider::new();
    let ctx = FetchContext {
        gateway_url: Some("http://public.example.com".into()),
        ..FetchContext::default()
    };
    let error = provider.fetch_api(&ctx).await.unwrap_err();
    assert!(error.to_string().contains("must use HTTPS"), "{error}");
}

#[tokio::test]
async fn unsupported_source_modes_are_rejected() {
    let provider = AixyProvider::new();
    let ctx = FetchContext {
        source_mode: SourceMode::Web,
        ..context("https://aixy.example.com")
    };
    assert!(matches!(
        provider.fetch_usage(&ctx).await,
        Err(ProviderError::UnsupportedSource(SourceMode::Web))
    ));
    assert_eq!(
        provider.available_sources(),
        vec![SourceMode::Auto, SourceMode::OAuth]
    );
}

#[tokio::test]
async fn fetch_sends_only_a_bearer_key_to_the_usage_endpoint() {
    for (prefix, path) in [("", "/v1/usage"), ("/prefix/v1/", "/prefix/v1/usage")] {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", path)
            .match_header("authorization", format!("Bearer {API_KEY}").as_str())
            .match_header("cookie", Matcher::Missing)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(FIXTURE)
            .expect(1)
            .create_async()
            .await;

        let provider = AixyProvider::new();
        let base = format!("{}{prefix}", server.url());
        let result = provider.fetch_usage(&context(&base)).await.unwrap();

        mock.assert_async().await;
        assert_eq!(result.usage.primary.used_percent, 30.0);
    }
}

#[tokio::test]
async fn statuses_are_classified_without_echoing_the_body() {
    for (status, expected) in [
        (401, "auth"),
        (403, "denied access"),
        (404, "does not provide the usage endpoint"),
        (429, "429"),
        (503, "unavailable"),
        (418, "failed (HTTP 418"),
    ] {
        let mut server = mockito::Server::new_async().await;
        let _mock = mock_response(
            &mut server,
            "GET",
            "/v1/usage",
            status,
            "private upstream body",
        )
        .await;

        let provider = AixyProvider::new();
        let error = provider
            .fetch_usage(&context(&server.url()))
            .await
            .unwrap_err();
        if status == 401 {
            assert!(matches!(error, ProviderError::AuthRequired), "{expected}");
        } else {
            let message = error.to_string();
            assert!(message.contains(expected), "{status}: {message}");
        }
        assert!(!error.to_string().contains("private upstream body"));
    }
}

#[tokio::test]
async fn malformed_bodies_fail_without_echoing_them() {
    let mut server = mockito::Server::new_async().await;
    let _mock = mock_response(
        &mut server,
        "GET",
        "/v1/usage",
        200,
        "private upstream body",
    )
    .await;
    let error = AixyProvider::new()
        .fetch_usage(&context(&server.url()))
        .await
        .unwrap_err();
    assert!(matches!(error, ProviderError::Parse(_)));
    assert!(!error.to_string().contains("private upstream body"));
}

#[tokio::test]
async fn does_not_forward_the_key_through_gateway_redirects() {
    let mut server = mockito::Server::new_async().await;
    let redirect = server
        .mock("GET", "/v1/usage")
        .match_header("authorization", format!("Bearer {API_KEY}").as_str())
        .with_status(302)
        .with_header("location", &format!("{}/redirected", server.url()))
        .expect(1)
        .create_async()
        .await;
    let target = mock_response_expect(&mut server, "GET", "/redirected", 200, FIXTURE, 0).await;

    let error = AixyProvider::new()
        .fetch_usage(&context(&server.url()))
        .await
        .unwrap_err();

    redirect.assert_async().await;
    target.assert_async().await;
    assert!(error.to_string().contains("302"), "{error}");
}
