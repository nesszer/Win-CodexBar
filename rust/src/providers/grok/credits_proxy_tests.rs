use super::credits_proxy::{BearerBilling, parse_credits_response};
use super::tests::billing_response_with_percent;
use super::*;
use crate::providers::test_support::{
    mock_response, mock_response_expect, mock_status, mock_status_expect,
};

/// Upstream live capture shape (`GrokCreditsProxyFetcherTests`), including
/// fields the port does not read.
const WEEKLY_CREDITS_BODY: &str = r#"{"config":{"currentPeriod":{"type":"USAGE_PERIOD_TYPE_WEEKLY","start":"2026-09-20T18:42:45.537749+00:00","end":"2026-09-27T18:42:45.537749+00:00"},"creditUsagePercent":1.0,"onDemandCap":{"val":0},"onDemandUsed":{"val":0},"productUsage":[{"product":"GrokBuild","usagePercent":1.0}],"isUnifiedBillingUser":true,"prepaidBalance":{"val":0},"topUpMethod":"TOP_UP_METHOD_SAVED_PAYMENT_METHOD","billingPeriodStart":"2026-09-20T18:42:45.537749+00:00","billingPeriodEnd":"2026-09-27T18:42:45.537749+00:00"}}"#;
const PERIOD_ONLY_BODY: &str = r#"{"config":{"currentPeriod":{"start":"2026-08-06T00:00:00Z","end":"2026-08-13T00:00:00Z"}},"subscriptionTier":"SUPERGROK_HEAVY"}"#;

fn at(raw: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(raw)
        .unwrap()
        .with_timezone(&Utc)
}

fn parse(body: &str) -> BearerBilling {
    parse_credits_response(body.as_bytes(), at("2026-08-12T00:00:00Z")).unwrap()
}

fn provider_for(server: &mockito::ServerGuard) -> GrokProvider {
    GrokProvider::new()
        .with_billing_endpoint_for_tests(format!("{}/billing", server.url()))
        .with_credits_proxy_endpoint_for_tests(format!("{}/credits", server.url()))
}

#[test]
fn parses_the_live_weekly_capture() {
    let parsed =
        parse_credits_response(WEEKLY_CREDITS_BODY.as_bytes(), at("2026-09-25T00:00:00Z")).unwrap();
    assert_eq!(parsed.billing.used_percent, Some(1.0));
    assert!(parsed.billing.used_percent_is_wire_published);
    assert_eq!(
        parsed.billing.resets_at,
        Some(at("2026-09-27T18:42:45.537749+00:00"))
    );
    assert_eq!(
        parsed.billing.window_minutes,
        Some(crate::core::WEEKLY_WINDOW_MINUTES)
    );
    assert_eq!(parsed.subscription_tier, None);
}

#[test]
fn measures_only_matching_valid_period_bounds() {
    let cases: [(&str, Option<u32>); 10] = [
        (
            r#""billingPeriodStart":"2026-08-06T00:00:00Z","billingPeriodEnd":"2026-08-13T00:00:00Z""#,
            Some(10080),
        ),
        (
            r#""currentPeriod":{"start":"2026-07-13T00:00:00Z","end":"2026-08-13T00:00:00Z"}"#,
            Some(44640),
        ),
        (r#""currentPeriod":{"end":"2026-08-13T00:00:00Z"}"#, None),
        (
            r#""currentPeriod":{"end":"2026-08-13T00:00:00Z"},"billingPeriodStart":"2026-07-13T00:00:00Z","billingPeriodEnd":"2026-08-14T00:00:00Z""#,
            None,
        ),
        (
            r#""currentPeriod":{"start":"invalid","end":"2026-08-13T00:00:00Z"}"#,
            None,
        ),
        (
            r#""currentPeriod":{"start":"2026-08-14T00:00:00Z","end":"2026-08-21T00:00:00Z"}"#,
            None,
        ),
        (
            r#""currentPeriod":{"start":"2026-08-11T00:00:00Z","end":"2026-08-10T00:00:00Z"}"#,
            None,
        ),
        (
            r#""currentPeriod":{"start":"2026-08-11T00:00:00Z","end":"2026-08-11T00:00:00Z"}"#,
            None,
        ),
        (
            r#""currentPeriod":{"start":"2026-08-11T00:00:00Z","end":"2026-08-11T00:00:30Z"}"#,
            None,
        ),
        (
            r#""currentPeriod":{"start":"2026-07-01T00:00:00Z","end":"invalid"},"billingPeriodStart":"2026-08-06T00:00:00Z","billingPeriodEnd":"2026-08-13T00:00:00Z""#,
            Some(10080),
        ),
    ];
    for (period, expected_minutes) in cases {
        let parsed = parse(&format!(
            r#"{{"config":{{"creditUsagePercent":90,{period}}}}}"#
        ));
        assert_eq!(parsed.billing.used_percent, Some(90.0), "{period}");
        assert_eq!(parsed.billing.window_minutes, expected_minutes, "{period}");
    }
}

#[test]
fn derives_percent_from_on_demand_cap_and_usage() {
    let parsed = parse(r#"{"config":{"onDemandCap":{"val":1000.0},"onDemandUsed":{"val":250.5}}}"#);
    assert_eq!(parsed.billing.used_percent, Some(25.05));
    assert!(parsed.billing.used_percent_is_wire_published);
    assert_eq!(parsed.billing.resets_at, None);
}

#[test]
fn clamps_out_of_range_credit_usage() {
    let over = parse(
        r#"{"config":{"creditUsagePercent":104.2,"billingPeriodEnd":"2026-08-13T00:00:00Z"}}"#,
    );
    let under = parse(r#"{"config":{"creditUsagePercent":-3.5}}"#);
    assert_eq!(over.billing.used_percent, Some(100.0));
    assert_eq!(over.billing.resets_at, Some(at("2026-08-13T00:00:00Z")));
    assert_eq!(under.billing.used_percent, Some(0.0));
}

#[test]
fn period_without_usage_is_unknown_not_zero() {
    let parsed = parse(
        r#"{"config":{"currentPeriod":{"end":"2026-08-13T00:00:00.123Z"},"billingPeriodEnd":"2026-08-14T00:00:00Z"}}"#,
    );
    assert_eq!(parsed.billing.used_percent, None);
    assert!(!parsed.billing.used_percent_is_wire_published);
    assert_eq!(
        parsed.billing.resets_at,
        Some(at("2026-08-13T00:00:00.123Z"))
    );
    assert_eq!(parsed.subscription_tier, None);
}

#[test]
fn billing_period_end_backs_up_a_missing_current_period_end() {
    let parsed = parse(
        r#"{"config":{"currentPeriod":{"type":"USAGE_PERIOD_TYPE_WEEKLY"},"billingPeriodEnd":"2026-08-13T00:00:00Z"}}"#,
    );
    assert_eq!(parsed.billing.used_percent, None);
    assert_eq!(parsed.billing.resets_at, Some(at("2026-08-13T00:00:00Z")));
}

#[test]
fn subscription_tier_prefers_config_over_the_envelope() {
    let parsed = parse(
        r#"{"config":{"creditUsagePercent":8,"billingPeriodEnd":"2026-08-13T00:00:00Z","subscriptionTier":"SuperGrok Heavy"},"subscriptionTier":"SuperGrok"}"#,
    );
    assert_eq!(parsed.subscription_tier.as_deref(), Some("SuperGrok Heavy"));
    let envelope = parse(
        r#"{"config":{"creditUsagePercent":57,"billingPeriodEnd":"2026-08-13T00:00:00Z"},"subscriptionTier":"SUPERGROK"}"#,
    );
    assert_eq!(envelope.subscription_tier.as_deref(), Some("SuperGrok"));
    let heavy = parse(
        r#"{"config":{"subscriptionTier":"SUPERGROK_HEAVY","currentPeriod":{"end":"2026-08-13T00:00:00Z"}}}"#,
    );
    assert_eq!(heavy.billing.used_percent, None);
    assert_eq!(heavy.subscription_tier.as_deref(), Some("SuperGrok Heavy"));
}

#[test]
fn rejects_responses_with_neither_usage_nor_period() {
    let now = at("2026-08-12T00:00:00Z");
    for body in [
        r#"{"config":{}}"#,
        r#"{"config":{"onDemandCap":{"val":0}},"subscriptionTier":"supergrok_heavy"}"#,
        r#"{"subscriptionTier":"SuperGrok"}"#,
        r#"{"config":{"creditUsagePercent":"12"}}"#,
        "not json",
    ] {
        assert!(
            matches!(
                parse_credits_response(body.as_bytes(), now),
                Err(ProviderError::Parse(_))
            ),
            "{body}"
        );
    }
}

#[tokio::test]
async fn bearer_billing_sends_the_cli_proxy_request_and_skips_grpc() {
    let mut server = mockito::Server::new_async().await;
    let proxy = server
        .mock("GET", "/credits")
        .match_header("authorization", "Bearer token-123")
        .match_header("x-xai-token-auth", "xai-grok-cli")
        .match_header("accept", "application/json")
        .match_header("user-agent", "CodexBar")
        .with_status(200)
        .with_body(format!(
            r#"{{"config":{{"creditUsagePercent":12.5,"currentPeriod":{{"start":"{}","end":"{}"}}}}}}"#,
            (Utc::now() - chrono::Duration::days(1)).to_rfc3339(),
            (Utc::now() + chrono::Duration::days(6)).to_rfc3339(),
        ))
        .expect(1)
        .create_async()
        .await;
    let grpc = mock_status_expect(&mut server, "POST", "/billing", 200, 0).await;

    let bearer = provider_for(&server)
        .fetch_bearer_billing(&GrokCredentials::from_bearer("token-123"))
        .await
        .unwrap();

    proxy.assert_async().await;
    grpc.assert_async().await;
    assert_eq!(bearer.billing.used_percent, Some(12.5));
    assert_eq!(
        bearer.billing.window_minutes,
        Some(crate::core::WEEKLY_WINDOW_MINUTES)
    );
}

#[tokio::test]
async fn proxy_failure_falls_back_to_grpc_billing() {
    for status in [401, 500] {
        let mut server = mockito::Server::new_async().await;
        let proxy = mock_status(&mut server, "GET", "/credits", status).await;
        let grpc = server
            .mock("POST", "/billing")
            .match_header("authorization", "Bearer token-123")
            .with_status(200)
            .with_body(billing_response_with_percent(37.0))
            .create_async()
            .await;

        let bearer = provider_for(&server)
            .fetch_bearer_billing(&GrokCredentials::from_bearer("token-123"))
            .await
            .unwrap();

        proxy.assert_async().await;
        grpc.assert_async().await;
        assert_eq!(bearer.billing.used_percent, Some(37.0), "status {status}");
        assert_eq!(bearer.subscription_tier, None);
    }
}

#[tokio::test]
async fn period_only_answer_adopts_the_grpc_percent_and_keeps_proxy_metadata() {
    let mut server = mockito::Server::new_async().await;
    mock_response(&mut server, "GET", "/credits", 200, PERIOD_ONLY_BODY).await;
    let grpc = mock_response_expect(
        &mut server,
        "POST",
        "/billing",
        200,
        billing_response_with_percent(90.0),
        1,
    )
    .await;

    let bearer = provider_for(&server)
        .fetch_bearer_billing(&GrokCredentials::from_bearer("token-123"))
        .await
        .unwrap();

    grpc.assert_async().await;
    assert_eq!(bearer.billing.used_percent, Some(90.0));
    assert!(bearer.billing.used_percent_is_wire_published);
    assert_eq!(bearer.billing.resets_at, Some(at("2026-08-13T00:00:00Z")));
    assert_eq!(bearer.subscription_tier.as_deref(), Some("SuperGrok Heavy"));
}

#[tokio::test]
async fn period_only_answer_stays_unknown_when_grpc_has_no_percent_or_fails() {
    // An empty gRPC-web frame carries no percent; a 401 is a failed retry.
    for (status, body) in [(200, vec![0, 0, 0, 0, 0]), (401, Vec::new())] {
        let mut server = mockito::Server::new_async().await;
        mock_response(&mut server, "GET", "/credits", 200, PERIOD_ONLY_BODY).await;
        let grpc = mock_response(&mut server, "POST", "/billing", status, body).await;

        let bearer = provider_for(&server)
            .fetch_bearer_billing(&GrokCredentials::from_bearer("token-123"))
            .await
            .unwrap();

        grpc.assert_async().await;
        assert_eq!(bearer.billing.used_percent, None, "status {status}");
        assert_eq!(bearer.billing.resets_at, Some(at("2026-08-13T00:00:00Z")));
        assert_eq!(bearer.subscription_tier.as_deref(), Some("SuperGrok Heavy"));
    }
}

#[tokio::test]
async fn expired_credentials_are_never_sent() {
    let mut server = mockito::Server::new_async().await;
    let proxy = mock_status_expect(&mut server, "GET", "/credits", 200, 0).await;
    let grpc = mock_status_expect(&mut server, "POST", "/billing", 200, 0).await;
    let mut credentials = GrokCredentials::from_bearer("stale-token");
    credentials.expires_at = Some(Utc::now() - chrono::Duration::minutes(1));

    let context = FetchContext {
        include_credits: false,
        ..FetchContext::default()
    };
    let error = provider_for(&server)
        .fetch_with_auth(&credentials, GrokAuthKind::OAuth, &context)
        .await
        .unwrap_err();

    assert!(matches!(error, ProviderError::AuthRequired));
    proxy.assert_async().await;
    grpc.assert_async().await;
}

#[tokio::test]
async fn credits_plan_labels_oauth_results_without_settings_lookup() {
    let mut server = mockito::Server::new_async().await;
    mock_response(&mut server, "GET", "/credits", 200, r#"{"config":{"creditUsagePercent":40,"billingPeriodEnd":"2026-08-13T00:00:00Z"},"subscriptionTier":"SUPERGROK_HEAVY"}"#,).await;
    let context = FetchContext {
        include_credits: false,
        ..FetchContext::default()
    };

    let result = provider_for(&server)
        .fetch_with_auth(
            &GrokCredentials::from_bearer("token-123"),
            GrokAuthKind::OAuth,
            &context,
        )
        .await
        .unwrap();

    assert_eq!(result.source_label, "grok-oauth");
    assert_eq!(result.usage.primary.used_percent, 40.0);
    assert!(!result.usage.primary.is_informational);
    assert_eq!(
        result.usage.login_method.as_deref(),
        Some("SuperGrok Heavy")
    );
}
