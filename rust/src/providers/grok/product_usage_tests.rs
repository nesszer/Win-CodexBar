use super::credits_proxy::{BearerBilling, parse_credits_response};
use super::product_usage::{GrokProductUsage, display_details};
use super::tests::billing_response_with_percent;
use super::*;
use crate::providers::test_support::{mock_response, mock_status};

/// Upstream live capture (`LiveMultiProductCreditsPayload.p6`).
const LIVE_MULTI_PRODUCT_BODY: &str = r#"{"config":{"currentPeriod":{"type":"USAGE_PERIOD_TYPE_WEEKLY","start":"2026-09-20T18:42:45.537749+00:00","end":"2026-09-27T18:42:45.537749+00:00"},"creditUsagePercent":6.0,"onDemandCap":{"val":0},"onDemandUsed":{"val":0},"productUsage":[{"product":"GrokChat","usagePercent":4.0},{"product":"GrokBuild","usagePercent":2.0}],"isUnifiedBillingUser":true,"prepaidBalance":{"val":0},"topUpMethod":"TOP_UP_METHOD_SAVED_PAYMENT_METHOD","billingPeriodStart":"2026-09-20T18:42:45.537749+00:00","billingPeriodEnd":"2026-09-27T18:42:45.537749+00:00"}}"#;
const BASE_PERIOD: &str =
    r#""currentPeriod":{"start":"2026-09-20T00:00:00Z","end":"2026-09-27T00:00:00Z"}"#;

fn product(name: &str, used_percent: f64) -> GrokProductUsage {
    GrokProductUsage {
        product: name.to_string(),
        used_percent,
    }
}

fn parse(body: &str) -> BearerBilling {
    let now = DateTime::parse_from_rfc3339("2026-09-23T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    parse_credits_response(body.as_bytes(), now).unwrap()
}

fn rows(products: &[GrokProductUsage]) -> Vec<(String, String, String)> {
    display_details(products)
        .iter()
        .map(|row| {
            assert!(row.progress().is_none() && row.secondary_value().is_none());
            (
                row.id().to_string(),
                row.title().to_string(),
                row.value().to_string(),
            )
        })
        .collect()
}

#[test]
fn live_multi_product_capture_keeps_the_composed_breakdown() {
    let parsed = parse(LIVE_MULTI_PRODUCT_BODY);
    assert_eq!(parsed.billing.used_percent, Some(6.0));
    assert_eq!(
        parsed.billing.product_usage,
        vec![product("GrokChat", 4.0), product("GrokBuild", 2.0)]
    );
    assert_eq!(
        rows(&parsed.billing.product_usage),
        vec![
            (
                "grok.product.GrokChat".to_string(),
                "Grok Chat".to_string(),
                "4%".to_string()
            ),
            (
                "grok.product.GrokBuild".to_string(),
                "Grok Build".to_string(),
                "2%".to_string()
            ),
        ]
    );
}

#[test]
fn weekly_capture_with_one_product_keeps_it() {
    let parsed = parse(
        r#"{"config":{"creditUsagePercent":1.0,"productUsage":[{"product":"GrokBuild","usagePercent":1.0}]}}"#,
    );
    assert_eq!(
        parsed.billing.product_usage,
        vec![product("GrokBuild", 1.0)]
    );
}

#[test]
fn malformed_products_drop_the_breakdown_but_not_total_or_period() {
    let baseline = parse(&format!(
        r#"{{"config":{{"creditUsagePercent":42,{BASE_PERIOD}}}}}"#
    ));
    assert!(baseline.billing.product_usage.is_empty());
    let fragments = [
        r#""productUsage":null"#,
        r#""productUsage":{}"#,
        r#""productUsage":"wrong""#,
        r#""productUsage":[]"#,
        r#""productUsage":[{"product":"GrokBuild","usagePercent":"1"}]"#,
        r#""productUsage":[{"usagePercent":1}]"#,
        r#""productUsage":[{"product":42,"usagePercent":1}]"#,
        r#""productUsage":[{"product":"GrokBuild"}]"#,
        r#""productUsage":[{"product":"GrokBuild","usagePercent":-1}]"#,
        r#""productUsage":[{"product":"  ","usagePercent":1}]"#,
        r#""productUsage":[{"product":"GrokChat","usagePercent":42},42]"#,
    ];
    for fragment in fragments {
        let parsed = parse(&format!(
            r#"{{"config":{{"creditUsagePercent":42,{BASE_PERIOD},{fragment}}}}}"#
        ));
        assert!(parsed.billing.product_usage.is_empty(), "{fragment}");
        assert_eq!(parsed.billing.used_percent, Some(42.0), "{fragment}");
        assert_eq!(parsed.billing.resets_at, baseline.billing.resets_at);
        assert_eq!(
            parsed.billing.window_minutes,
            baseline.billing.window_minutes
        );
        assert!(parsed.billing.used_percent_is_wire_published);
    }
}

#[test]
fn one_malformed_entry_cannot_hide_behind_the_tolerance() {
    // 5 + a missing entry would otherwise compose 6 within 1 point.
    let parsed = parse(
        r#"{"config":{"creditUsagePercent":6,"productUsage":[{"product":"GrokChat","usagePercent":5},{"product":"GrokBuild","usagePercent":"1"}]}}"#,
    );
    assert!(parsed.billing.product_usage.is_empty());
    assert_eq!(parsed.billing.used_percent, Some(6.0));
    let exact_remainder = parse(
        r#"{"config":{"creditUsagePercent":5,"productUsage":[{"product":"GrokChat","usagePercent":5},{"usagePercent":1}]}}"#,
    );
    assert!(exact_remainder.billing.product_usage.is_empty());
    assert_eq!(exact_remainder.billing.used_percent, Some(5.0));
}

#[test]
fn products_attach_only_to_the_wire_credit_percent() {
    let cases = [
        (
            r#""onDemandCap":{"val":100},"onDemandUsed":{"val":3}"#,
            Some(3.0),
        ),
        (r#""billingPeriodEnd":"2026-09-27T00:00:00Z""#, None),
    ];
    for (fields, percent) in cases {
        let parsed = parse(&format!(
            r#"{{"config":{{{fields},"productUsage":[{{"product":"GrokBuild","usagePercent":3}}]}}}}"#
        ));
        assert_eq!(parsed.billing.used_percent, percent, "{fields}");
        assert!(parsed.billing.product_usage.is_empty(), "{fields}");
    }
}

#[test]
fn shares_that_do_not_compose_the_percent_are_dropped() {
    for products in [
        r#"[{"product":"GrokBuild","usagePercent":60}]"#,
        r#"[{"product":"GrokBuild","usagePercent":20},{"product":"GrokChat","usagePercent":5}]"#,
    ] {
        let parsed = parse(&format!(
            r#"{{"config":{{"creditUsagePercent":30,{BASE_PERIOD},"productUsage":{products}}}}}"#
        ));
        assert!(parsed.billing.product_usage.is_empty(), "{products}");
        assert_eq!(parsed.billing.used_percent, Some(30.0));
        assert_eq!(parsed.billing.window_minutes, Some(10080));
    }
}

#[test]
fn shares_within_rounding_of_the_percent_are_kept() {
    let rounded = parse(
        r#"{"config":{"creditUsagePercent":10,"productUsage":[{"product":"GrokBuild","usagePercent":6.0},{"product":"GrokChat","usagePercent":3.6}]}}"#,
    );
    assert_eq!(
        rounded.billing.product_usage,
        vec![product("GrokBuild", 6.0), product("GrokChat", 3.6)]
    );
    let exactly_one_point_off = parse(
        r#"{"config":{"creditUsagePercent":10,"productUsage":[{"product":"GrokBuild","usagePercent":9.0}]}}"#,
    );
    assert_eq!(
        exactly_one_point_off.billing.product_usage,
        vec![product("GrokBuild", 9.0)]
    );
    let just_outside = parse(
        r#"{"config":{"creditUsagePercent":10,"productUsage":[{"product":"GrokBuild","usagePercent":8.9}]}}"#,
    );
    assert!(just_outside.billing.product_usage.is_empty());
}

#[test]
fn shares_compose_the_raw_unclamped_percent() {
    let parsed = parse(
        r#"{"config":{"creditUsagePercent":120,"productUsage":[{"product":"GrokBuild","usagePercent":120}]}}"#,
    );
    assert_eq!(parsed.billing.used_percent, Some(100.0));
    assert_eq!(
        parsed.billing.product_usage,
        vec![product("GrokBuild", 120.0)]
    );
}

#[test]
fn rows_sort_by_share_keep_ties_in_wire_order_and_omit_zero() {
    let products = [
        product("GrokBuild", 1.0),
        product("GrokChat", 5.0),
        product("GrokImagine", 0.0),
        product("FutureGrok", 0.2),
        product("GrokAppBuilder", 5.0),
    ];
    let rows = rows(&products);
    assert_eq!(
        rows.iter().map(|row| row.0.as_str()).collect::<Vec<_>>(),
        [
            "grok.product.GrokChat",
            "grok.product.GrokAppBuilder",
            "grok.product.GrokBuild",
            "grok.product.FutureGrok",
        ]
    );
    assert_eq!(
        rows.iter().map(|row| row.1.as_str()).collect::<Vec<_>>(),
        ["Grok Chat", "Grok App Builder", "Grok Build", "FutureGrok"]
    );
    assert_eq!(
        rows.iter().map(|row| row.2.as_str()).collect::<Vec<_>>(),
        ["5%", "5%", "1%", "<1%"]
    );
    assert!(display_details(&[]).is_empty());
    assert!(display_details(&[product("GrokChat", 0.0)]).is_empty());
}

#[test]
fn product_names_are_trimmed_and_imagine_is_labelled() {
    let parsed = parse(
        r#"{"config":{"creditUsagePercent":3,"productUsage":[{"product":" GrokImagine ","usagePercent":2.5},{"product":"Other","usagePercent":0.5}]}}"#,
    );
    assert_eq!(parsed.billing.product_usage[0].product, "GrokImagine");
    assert_eq!(
        rows(&parsed.billing.product_usage)[0],
        (
            "grok.product.GrokImagine".to_string(),
            "Grok Imagine".to_string(),
            "2%".to_string()
        )
    );
}

fn provider_for(server: &mockito::ServerGuard) -> GrokProvider {
    GrokProvider::new()
        .with_billing_endpoint_for_tests(format!("{}/billing", server.url()))
        .with_credits_proxy_endpoint_for_tests(format!("{}/credits", server.url()))
}

fn context_without_credits() -> FetchContext {
    FetchContext {
        include_credits: false,
        ..FetchContext::default()
    }
}

#[tokio::test]
async fn bearer_result_shows_the_breakdown_as_plain_rows_under_one_bar() {
    let mut server = mockito::Server::new_async().await;
    mock_response(&mut server, "GET", "/credits", 200, LIVE_MULTI_PRODUCT_BODY).await;

    let result = provider_for(&server)
        .fetch_with_auth(
            &GrokCredentials::from_bearer("token-123"),
            GrokAuthKind::OAuth,
            &context_without_credits(),
        )
        .await
        .unwrap();

    assert_eq!(result.usage.primary.used_percent, 6.0);
    assert!(result.usage.secondary.is_none());
    assert!(result.usage.tertiary.is_none());
    let shown: Vec<(&str, &str)> = result
        .display_details()
        .iter()
        .map(|row| (row.title(), row.value()))
        .collect();
    assert_eq!(shown, [("Grok Chat", "4%"), ("Grok Build", "2%")]);
    assert!(
        result
            .display_details()
            .iter()
            .all(|row| row.progress().is_none())
    );
}

#[tokio::test]
async fn reset_credit_enrichment_preserves_the_breakdown() {
    let mut server = mockito::Server::new_async().await;
    mock_response(&mut server, "GET", "/credits", 200, LIVE_MULTI_PRODUCT_BODY).await;
    let result = provider_for(&server)
        .fetch_with_auth(
            &GrokCredentials::from_bearer("token-123"),
            GrokAuthKind::OAuth,
            &context_without_credits(),
        )
        .await
        .unwrap()
        .with_inventory_item(ProviderInventoryItem {
            id: "reset-credits".to_string(),
            title: "Limit Reset Credits".to_string(),
            available_count: 1,
            next_expires_at: None,
        });

    assert_eq!(result.display_details().len(), 2);
    assert_eq!(result.inventory.len(), 1);
}

#[tokio::test]
async fn grpc_percent_never_borrows_the_proxy_products() {
    let mut server = mockito::Server::new_async().await;
    mock_response(&mut server, "GET", "/credits", 200, r#"{"config":{"currentPeriod":{"start":"2026-08-06T00:00:00Z","end":"2026-08-13T00:00:00Z"},"productUsage":[{"product":"GrokBuild","usagePercent":12}]}}"#,).await;
    mock_response(
        &mut server,
        "POST",
        "/billing",
        200,
        billing_response_with_percent(12.0),
    )
    .await;

    let result = provider_for(&server)
        .fetch_with_auth(
            &GrokCredentials::from_bearer("token-123"),
            GrokAuthKind::OAuth,
            &context_without_credits(),
        )
        .await
        .unwrap();

    assert_eq!(result.usage.primary.used_percent, 12.0);
    assert!(result.display_details().is_empty());
}

#[test]
fn repeated_product_names_keep_distinct_rows_and_overage_is_not_clamped() {
    let rows = rows(&[
        product("GrokChat", 3.0),
        product("GrokChat", 2.0),
        product("GrokBuild", 120.0),
    ]);
    assert_eq!(
        rows.iter().map(|row| row.0.as_str()).collect::<Vec<_>>(),
        [
            "grok.product.GrokBuild",
            "grok.product.row.0",
            "grok.product.row.1"
        ]
    );
    assert_eq!(rows[0].2, "120%");
}

fn shown(result: &ProviderFetchResult) -> Vec<(String, String)> {
    result
        .display_details()
        .iter()
        .map(|row| (row.title().to_string(), row.value().to_string()))
        .collect()
}

fn live_breakdown() -> Vec<(String, String)> {
    vec![
        ("Grok Chat".to_string(), "4%".to_string()),
        ("Grok Build".to_string(), "2%".to_string()),
    ]
}

#[tokio::test]
async fn grpc_breakdown_is_adopted_with_the_grpc_percent_on_a_period_only_proxy_answer() {
    let mut server = mockito::Server::new_async().await;
    mock_response(&mut server, "GET", "/credits", 200, r#"{"config":{"currentPeriod":{"start":"2026-08-06T00:00:00Z","end":"2026-08-13T00:00:00Z"},"productUsage":[{"product":"GrokBuild","usagePercent":12}]}}"#,).await;
    mock_response(
        &mut server,
        "POST",
        "/billing",
        200,
        super::billing::web_product_usage_tests::live_frame(),
    )
    .await;

    let result = provider_for(&server)
        .fetch_with_auth(
            &GrokCredentials::from_bearer("token-123"),
            GrokAuthKind::OAuth,
            &context_without_credits(),
        )
        .await
        .unwrap();

    assert_eq!(result.usage.primary.used_percent, 6.0);
    assert_eq!(shown(&result), live_breakdown());
}

#[tokio::test]
async fn grpc_only_bearer_fallback_shows_the_breakdown() {
    let mut server = mockito::Server::new_async().await;
    mock_status(&mut server, "GET", "/credits", 500).await;
    mock_response(
        &mut server,
        "POST",
        "/billing",
        200,
        super::billing::web_product_usage_tests::live_frame(),
    )
    .await;

    let result = provider_for(&server)
        .fetch_with_auth(
            &GrokCredentials::from_bearer("token-123"),
            GrokAuthKind::OAuth,
            &context_without_credits(),
        )
        .await
        .unwrap();

    assert_eq!(result.usage.primary.used_percent, 6.0);
    assert_eq!(shown(&result), live_breakdown());
}

#[test]
fn cookie_billing_result_shows_the_decoded_product_rows() {
    let billing = super::billing::parse_grpc_web_response(
        &super::billing::web_product_usage_tests::live_frame(),
    )
    .unwrap();
    let result = result_from_cookie_billing(billing);

    assert_eq!(result.source_label, "grok-browser");
    assert_eq!(result.usage.primary.used_percent, 6.0);
    assert!(result.usage.secondary.is_none());
    assert_eq!(shown(&result), live_breakdown());
}
