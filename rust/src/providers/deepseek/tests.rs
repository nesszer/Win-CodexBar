use super::*;

#[test]
fn prefers_usd_balance_and_formats_paid_and_granted() {
    let snapshot = DeepSeekProvider::snapshot_from_balance(BalanceResponse {
        is_available: true,
        balance_infos: vec![
            BalanceInfo {
                currency: "CNY".into(),
                total_balance: "10".into(),
                granted_balance: "1".into(),
                topped_up_balance: "9".into(),
            },
            BalanceInfo {
                currency: "USD".into(),
                total_balance: "3.50".into(),
                granted_balance: "0.50".into(),
                topped_up_balance: "3.00".into(),
            },
        ],
    });

    assert_eq!(snapshot.primary.used_percent, 0.0);
    assert_eq!(
        snapshot.primary.reset_description.as_deref(),
        Some("$3.50 (Paid: $3.00 / Granted: $0.50)")
    );
}

#[test]
fn uses_cny_balance_when_usd_is_empty() {
    let snapshot = DeepSeekProvider::snapshot_from_balance(BalanceResponse {
        is_available: true,
        balance_infos: vec![
            BalanceInfo {
                currency: "USD".into(),
                total_balance: "0".into(),
                granted_balance: "0".into(),
                topped_up_balance: "0".into(),
            },
            BalanceInfo {
                currency: "CNY".into(),
                total_balance: "42.25".into(),
                granted_balance: "2.25".into(),
                topped_up_balance: "40".into(),
            },
        ],
    });

    assert_eq!(snapshot.primary.used_percent, 0.0);
    assert_eq!(
        snapshot.primary.reset_description.as_deref(),
        Some("¥42.25 (Paid: ¥40.00 / Granted: ¥2.25)")
    );
    assert_eq!(
        snapshot.login_method.as_deref(),
        Some("CNY balance: ¥42.25")
    );
}

#[test]
fn keeps_zero_usd_when_no_currency_has_balance() {
    let snapshot = DeepSeekProvider::snapshot_from_balance(BalanceResponse {
        is_available: true,
        balance_infos: vec![
            BalanceInfo {
                currency: "CNY".into(),
                total_balance: "0".into(),
                granted_balance: "0".into(),
                topped_up_balance: "0".into(),
            },
            BalanceInfo {
                currency: "USD".into(),
                total_balance: "0".into(),
                granted_balance: "0".into(),
                topped_up_balance: "0".into(),
            },
        ],
    });

    assert_eq!(snapshot.primary.used_percent, 100.0);
    assert_eq!(
        snapshot.primary.reset_description.as_deref(),
        Some("$0.00 — add credits at platform.deepseek.com")
    );
    assert_eq!(snapshot.login_method.as_deref(), Some("USD balance: $0.00"));
}

#[test]
fn exhausted_when_balance_unavailable() {
    let snapshot = DeepSeekProvider::snapshot_from_balance(BalanceResponse {
        is_available: false,
        balance_infos: vec![BalanceInfo {
            currency: "USD".into(),
            total_balance: "1".into(),
            granted_balance: "1".into(),
            topped_up_balance: "0".into(),
        }],
    });

    assert_eq!(snapshot.primary.used_percent, 100.0);
    assert_eq!(
        snapshot.primary.reset_description.as_deref(),
        Some("Balance unavailable for API calls")
    );
}

#[test]
fn parses_deepseek_usage_summary_payloads() {
    let amount: UsageEnvelope<DeepSeekAmountData> = serde_json::from_value(serde_json::json!({
        "code": 0,
        "data": {
            "biz_code": "0",
            "biz_data": {
                "total": [
                    {"model": "deepseek-chat", "category": "PROMPT_CACHE_HIT_TOKEN", "amount": "100"},
                    {"model": "deepseek-chat", "category": "RESPONSE_TOKEN", "amount": 50},
                    {"model": "deepseek-reasoner", "category": "REQUEST", "amount": "3"}
                ],
                "days": [
                    {"date": "2026-05-26", "models": [{"model": "deepseek-chat", "category": "RESPONSE_TOKEN", "amount": 1}]},
                    {"date": "2026-05-27", "models": [
                        {"model": "deepseek-chat", "category": "PROMPT_CACHE_MISS_TOKEN", "amount": "20"},
                        {"model": "deepseek-chat", "category": "REQUEST", "amount": "2"}
                    ]}
                ]
            }
        }
    })).unwrap();
    let cost: UsageEnvelope<DeepSeekCostData> = serde_json::from_value(serde_json::json!({
        "code": "0",
        "data": {
            "biz_code": 0,
            "biz_data": {
                "currency": "CNY",
                "total": [{"model": "deepseek-chat", "category": "RESPONSE_TOKEN", "cost": "1.25"}],
                "days": [{"date": "2026-05-27", "models": [{"model": "deepseek-chat", "category": "RESPONSE_TOKEN", "cost": "0.10"}]}]
            }
        }
    })).unwrap();
    let summary = DeepSeekUsageSummary::from_payloads(amount, cost).unwrap();
    assert_eq!(summary.month_tokens, 150.0);
    assert_eq!(summary.today_tokens, 20.0);
    assert_eq!(summary.month_requests, 3.0);
    assert_eq!(summary.today_requests, 2.0);
    assert_eq!(summary.month_cost, 1.25);
    assert_eq!(summary.today_cost, 0.10);
    assert_eq!(summary.top_model.as_deref(), Some("deepseek-chat"));
    assert_eq!(summary.currency, "CNY");
    assert_eq!(summary.period, DeepSeekUsagePeriod::CurrentMonth);
    assert_eq!(
        summary.model_costs,
        vec![DeepSeekModelCost {
            model: "deepseek-chat".to_string(),
            cost: 1.25,
        }]
    );
}

#[test]
fn model_costs_keep_reported_zero_and_omit_incomplete_totals() {
    let amount: UsageEnvelope<DeepSeekAmountData> = serde_json::from_value(serde_json::json!({
        "code": 0,
        "data": {"biz_data": {"total": [], "days": []}}
    }))
    .unwrap();
    let cost: UsageEnvelope<DeepSeekCostData> = serde_json::from_value(serde_json::json!({
        "code": 0,
        "data": {
            "biz_data": {
                "currency": "USD",
                "total": [
                    {"model": "beta", "category": "RESPONSE_TOKEN", "cost": "2"},
                    {"model": " beta ", "category": "PROMPT_CACHE_MISS_TOKEN", "cost": "1"},
                    {"model": "beta", "category": "RESPONSE_TOKEN", "cost": "invalid"},
                    {"model": "alpha", "category": "RESPONSE_TOKEN", "cost": "0"},
                    {"model": "unknown", "category": "UNSUPPORTED", "cost": "7"},
                    {"model": "request-only", "category": "REQUEST", "cost": "11"}
                ],
                "days": []
            }
        }
    }))
    .unwrap();

    let summary = DeepSeekUsageSummary::from_payloads(amount, cost).unwrap();
    assert_eq!(summary.currency, "USD");
    assert_eq!(
        summary.model_costs,
        vec![DeepSeekModelCost {
            model: "alpha".to_string(),
            cost: 0.0,
        }]
    );
}

#[test]
fn applies_deepseek_summary_as_extra_windows() {
    let usage = UsageSnapshot::new(RateWindow::new(0.0));
    let usage = DeepSeekProvider::apply_usage_summary(
        usage,
        &DeepSeekUsageSummary {
            today_tokens: 20.0,
            month_tokens: 150.0,
            today_requests: 2.0,
            month_requests: 3.0,
            today_cost: 0.10,
            month_cost: 1.25,
            top_model: Some("deepseek-chat".to_string()),
            category_tokens: vec![("RESPONSE_TOKEN".to_string(), 50.0)],
            model_costs: vec![DeepSeekModelCost {
                model: "deepseek-chat".to_string(),
                cost: 0.0,
            }],
            currency: "CNY".to_string(),
            period: DeepSeekUsagePeriod::CurrentMonth,
        },
    );
    assert!(
        usage
            .extra_rate_windows
            .iter()
            .any(|window| window.id == "tokens-today")
    );
    assert!(
        usage
            .extra_rate_windows
            .iter()
            .any(|window| window.title == "Top model")
    );
    let model_cost = usage
        .extra_rate_windows
        .iter()
        .find(|window| window.id == "deepseek-model-cost-0")
        .expect("model spend row");
    assert!(model_cost.window.is_informational);
    assert_eq!(
        model_cost.window.reset_description.as_deref(),
        Some("¥0.0000 · Current month")
    );
}
