use super::billing::*;
use super::*;
use chrono::TimeZone;

#[test]
fn minimax_region_defaults_to_global_io_urls() {
    let region = MiniMaxRegion::from_settings_value(None);
    assert_eq!(region, MiniMaxRegion::Global);
    assert_eq!(region.settings_value(), "global");
    assert_eq!(region.cookie_domain(), "platform.minimax.io");
    assert_eq!(
        region.coding_plan_url(),
        "https://platform.minimax.io/user-center/payment/coding-plan?cycle_type=3"
    );
    assert_eq!(
        MiniMaxProvider::dashboard_url_for_region(None),
        "https://platform.minimax.io/user-center/payment/coding-plan?cycle_type=3"
    );
}

#[test]
fn minimax_region_accepts_legacy_china_value() {
    for value in ["cn", "china", "china-mainland", "china_mainland"] {
        let region = MiniMaxRegion::from_settings_value(Some(value));
        assert_eq!(region, MiniMaxRegion::ChinaMainland);
        assert_eq!(region.settings_value(), "cn");
        assert_eq!(region.cookie_domain(), "platform.minimaxi.com");
        assert_eq!(
            region.coding_plan_url(),
            "https://platform.minimaxi.com/user-center/payment/coding-plan?cycle_type=3"
        );
    }
}

#[test]
fn minimax_region_cookie_search_domains_and_www_base_url() {
    let global = MiniMaxRegion::Global;
    assert_eq!(global.www_base_url(), "https://www.minimax.io");
    assert_eq!(global.cookie_search_domains(), ["minimax.io"]);
    assert_eq!(
        global.coding_plan_remains_url(),
        "https://platform.minimax.io/v1/api/openplatform/coding_plan/remains"
    );
    assert_eq!(
        global.www_remains_url(),
        "https://www.minimax.io/v1/api/openplatform/coding_plan/remains"
    );

    let cn = MiniMaxRegion::ChinaMainland;
    assert_eq!(cn.www_base_url(), "https://www.minimaxi.com");
    assert_eq!(cn.cookie_search_domains(), ["minimaxi.com"]);
    assert_eq!(
        cn.coding_plan_remains_url(),
        "https://platform.minimaxi.com/v1/api/openplatform/coding_plan/remains"
    );
    assert_eq!(
        cn.www_remains_url(),
        "https://www.minimaxi.com/v1/api/openplatform/coding_plan/remains"
    );
}

/// A dated billing record with every other field absent.
fn record(ymd: &str, method: &str, model: &str) -> MiniMaxBillingRecord {
    MiniMaxBillingRecord {
        consume_token: None,
        consume_input_token: None,
        consume_output_token: None,
        consume_cash: None,
        consume_cash_after_voucher: None,
        created_at: None,
        ymd: Some(ymd.to_string()),
        consume_time: None,
        method: Some(method.to_string()),
        model: Some(model.to_string()),
        result: None,
        status: None,
    }
}

#[test]
fn aggregates_billing_history_records() {
    let records = vec![
        MiniMaxBillingRecord {
            consume_token: Some(serde_json::json!(1200)),
            consume_cash: Some(serde_json::json!("0.42")),
            result: Some(serde_json::json!("SUCCESS")),
            ..record("2026-12-16", "chat", "abab6.5")
        },
        MiniMaxBillingRecord {
            consume_input_token: Some(serde_json::json!(300)),
            consume_output_token: Some(serde_json::json!(500)),
            consume_cash_after_voucher: Some(serde_json::json!(0.21)),
            ..record("2026-12-15", "completion", "abab6.5")
        },
    ];
    let now = Utc.with_ymd_and_hms(2026, 12, 16, 12, 0, 0).unwrap();
    let summary = aggregate_billing(&records, now);
    assert_eq!(summary.last_30_days_tokens, 2000);
    assert!((summary.last_30_days_cash.unwrap() - 0.63).abs() < 0.001);
    assert_eq!(summary.top_models[0].name, "abab6.5");
    assert_eq!(summary.top_models[0].tokens, 2000);
}

#[test]
fn parses_billing_payload_into_result_extras() {
    let json = serde_json::json!({
        "base_resp": { "status_code": 0 },
        "charge_records": [{
            "consume_token": "2500",
            "consume_cash_after_voucher": "1.25",
            "ymd": Utc::now().format("%Y-%m-%d").to_string(),
            "method": "chat",
            "model": "abab6.5"
        }]
    });
    let summary = parse_billing_summary(&json).unwrap();
    let result = attach_billing_summary(
        ProviderFetchResult::new(UsageSnapshot::new(RateWindow::new(0.0)), "web-billing"),
        summary,
    );
    assert!(result.cost.is_some());
    assert!(
        result
            .usage
            .extra_rate_windows
            .iter()
            .any(|window| window.id == "billing-tokens-30d")
    );
}

#[test]
fn billing_summary_windows_follow_a_fixed_order() {
    let breakdown = |name: &str, tokens: i64, cash: Option<f64>| MiniMaxBillingBreakdown {
        name: name.to_string(),
        tokens,
        cash,
    };
    let summary = MiniMaxBillingSummary {
        today_tokens: 1234,
        last_30_days_tokens: 56789,
        today_cash: Some(0.5),
        last_30_days_cash: Some(12.345),
        top_methods: vec![
            breakdown("chat", 5000, Some(1.0)),
            breakdown("audio", 10, None),
        ],
        top_models: vec![breakdown("abab6.5", 7000, None)],
    };
    let base = || ProviderFetchResult::new(UsageSnapshot::new(RateWindow::new(1.0)), "web");
    let windows = |result: &ProviderFetchResult| -> Vec<(String, String, Option<String>)> {
        result
            .usage
            .extra_rate_windows
            .iter()
            .map(|named| {
                assert_eq!(named.window.used_percent, 0.0);
                assert_eq!(named.window.window_minutes, None);
                assert_eq!(named.window.resets_at, None);
                (
                    named.id.clone(),
                    named.title.clone(),
                    named.window.reset_description.clone(),
                )
            })
            .collect()
    };
    let row = |id: &str, title: &str, detail: &str| {
        (id.to_string(), title.to_string(), Some(detail.to_string()))
    };

    let result = attach_billing_summary(base(), summary.clone());
    assert_eq!(
        windows(&result),
        vec![
            row("billing-tokens-today", "Tokens today", "1,234"),
            row("billing-tokens-30d", "Tokens (30 days)", "56,789"),
            row("billing-cash-today", "Spend today", "$0.50"),
            row("billing-cash-30d", "Spend (30 days)", "$12.35"),
            row("billing-method-0", "Method: chat", "5,000 tokens / $1.00"),
            row("billing-method-1", "Method: audio", "10 tokens"),
            row("billing-model-0", "Model: abab6.5", "7,000 tokens"),
        ]
    );
    let cost = result.cost.expect("30-day spend sets cost");
    assert_eq!(cost.used, 12.345);
    assert_eq!(cost.currency_code, "USD");
    assert_eq!(result.usage.primary.used_percent, 1.0);

    let no_cash = MiniMaxBillingSummary {
        today_cash: None,
        last_30_days_cash: None,
        top_methods: Vec::new(),
        top_models: Vec::new(),
        ..summary
    };
    let result = attach_billing_summary(base(), no_cash);
    assert_eq!(
        windows(&result),
        vec![
            row("billing-tokens-today", "Tokens today", "1,234"),
            row("billing-tokens-30d", "Tokens (30 days)", "56,789"),
        ]
    );
    assert!(result.cost.is_none());
}

#[test]
fn parses_plan_title_from_coding_plan_fields() {
    let provider = MiniMaxProvider::new();
    for (field, expected) in [
        ("plan_name", "MiniMax Star"),
        ("current_plan_title", "Coding Plan Pro"),
        ("current_subscribe_title", "Max"),
        ("combo_title", "Combo Star"),
    ] {
        let snapshot = provider
            .parse_usage_response(&serde_json::json!({
                "base_resp": { "status_code": 0 },
                field: expected,
                "used_amount": 0,
                "total_quota": 100
            }))
            .unwrap();
        assert_eq!(snapshot.login_method.as_deref(), Some(expected));
    }

    let snapshot = provider
        .parse_usage_response(&serde_json::json!({
            "base_resp": { "status_code": 0 },
            "current_combo_card": { "title": "Card Title" },
            "used_amount": 0,
            "total_quota": 100
        }))
        .unwrap();
    assert_eq!(snapshot.login_method.as_deref(), Some("Card Title"));
}

#[test]
fn filters_failed_billing_records() {
    let records = vec![
        MiniMaxBillingRecord {
            consume_token: Some(serde_json::json!(1000)),
            result: Some(serde_json::json!("SUCCESS")),
            ..record("2026-05-17", "chat", "MiniMax-M1")
        },
        MiniMaxBillingRecord {
            consume_token: Some(serde_json::json!(2000)),
            result: Some(serde_json::json!("FAILED")),
            ..record("2026-05-17", "chat", "MiniMax-M1")
        },
        MiniMaxBillingRecord {
            consume_token: Some(serde_json::json!(3000)),
            ..record("2026-05-17", "audio", "speech")
        },
        MiniMaxBillingRecord {
            consume_token: Some(serde_json::json!(4000)),
            status: Some(serde_json::json!(0)),
            ..record("2026-05-17", "video", "video")
        },
    ];
    let now = Utc.with_ymd_and_hms(2026, 5, 17, 12, 0, 0).unwrap();
    let summary = aggregate_billing(&records, now);
    assert_eq!(summary.today_tokens, 4000);
    assert_eq!(summary.last_30_days_tokens, 4000);
    assert_eq!(summary.top_methods.len(), 2);
    assert_eq!(summary.top_methods[0].name, "audio");
    assert_eq!(summary.top_methods[0].tokens, 3000);
    assert_eq!(summary.top_methods[1].name, "chat");
    assert_eq!(summary.top_methods[1].tokens, 1000);
}
