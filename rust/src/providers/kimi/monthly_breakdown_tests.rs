use super::*;
use crate::providers::kimi::KimiCodeApiUsageResponse;
use serde_json::json;

fn code_api(value: serde_json::Value) -> KimiCodeApiUsageResponse {
    serde_json::from_value(value).expect("Code API fixture decodes")
}

fn stats(value: serde_json::Value) -> KimiSubscriptionStatsResponse {
    serde_json::from_value(value).expect("stats fixture decodes")
}

fn code_api_rows(value: serde_json::Value) -> Vec<ProviderDisplayDetail> {
    let response = code_api(value);
    monthly_breakdown_details(
        response
            .usages
            .as_ref()
            .and_then(KimiCodeUsagePools::monthly_shares),
    )
}

fn assert_rows(rows: &[ProviderDisplayDetail], values: [&str; 2], progress: [f64; 2]) {
    let ids: Vec<&str> = rows.iter().map(ProviderDisplayDetail::id).collect();
    assert_eq!(ids, ["kimi-monthly-code", "kimi-monthly-other"]);
    let titles: Vec<&str> = rows.iter().map(ProviderDisplayDetail::title).collect();
    assert_eq!(titles, ["Kimi Code", "Other features"]);
    assert!(
        rows.iter()
            .all(|row| row.section_title() == Some("Total usage"))
    );
    let actual: Vec<&str> = rows.iter().map(ProviderDisplayDetail::value).collect();
    assert_eq!(actual, values);
    for (row, expected) in rows.iter().zip(progress) {
        let bar = row.progress().expect("progress");
        assert_eq!(bar.total(), 1.0);
        assert!((bar.used() - expected).abs() < 1e-9, "{row:?}");
    }
}

#[test]
fn code_api_month_pools_split_total_usage() {
    let rows = code_api_rows(json!({"usages": {
        "limit_5h": {"used_ratio": 0.0021},
        "limit_month_total": {"used_ratio": 0.5761, "reset_time": "2026-10-16T21:08:39Z"},
        "limit_month_code": {"used_ratio": 0.0426, "reset_time": "2026-10-16T21:08:39Z"}
    }}));
    assert_rows(&rows, ["4% used", "53% used"], [0.0426, 0.5335]);
}

#[test]
fn web_membership_ratios_split_total_usage() {
    let subscription = stats(json!({"subscriptionBalance": {
        "feature": "FEATURE_OMNI", "type": "SUBSCRIPTION",
        "amountUsedRatio": 0.5761, "kimiCodeUsedRatio": 0.0426,
        "expireTime": "2026-10-22T00:00:00Z"
    }}));
    let rows = monthly_breakdown_details(subscription_monthly_shares(&subscription));
    assert_rows(&rows, ["4% used", "53% used"], [0.0426, 0.5335]);
}

#[test]
fn missing_or_invalid_ratios_leave_total_usage_without_a_breakdown() {
    for pools in [
        json!({"limit_month_total": {"used_ratio": 0.5}}),
        json!({"limit_month_total": {"used_ratio": 0.5}, "limit_month_code": {"used_ratio": -0.1}}),
        json!({"limit_month_total": {"used_ratio": 0.5}, "limit_month_code": {"reset_time": "2026-10-17T00:00:00Z"}}),
        json!({"limit_month_total": {"used_ratio": -1}, "limit_month_code": {"used_ratio": 0.1}}),
        json!({"limit_month_code": {"used_ratio": 0.1}}),
    ] {
        assert!(
            code_api_rows(json!({ "usages": pools })).is_empty(),
            "{pools}"
        );
    }
    let without_code_share = stats(json!({"subscriptionBalance": {"amountUsedRatio": 0.5}}));
    assert!(monthly_breakdown_details(subscription_monthly_shares(&without_code_share)).is_empty());
    let feature_pool = stats(json!({"subscriptionBalance": {
        "feature": "FEATURE_CODING", "amountUsedRatio": 0.5, "kimiCodeUsedRatio": 0.1
    }}));
    assert!(subscription_monthly_shares(&feature_pool).is_none());
}

#[test]
fn code_share_above_the_total_floors_other_features_at_zero() {
    let rows = code_api_rows(json!({"usages": {
        "limit_month_total": {"used_ratio": 0.2},
        "limit_month_code": {"used_ratio": 0.25}
    }}));
    assert_rows(&rows, ["25% used", "0% used"], [0.25, 0.0]);
}

#[test]
fn malformed_code_share_keeps_the_valid_quota_windows() {
    for code in [
        json!({"used_ratio": "malformed"}),
        json!("not a pool"),
        json!([1, 2]),
    ] {
        let response = code_api(json!({"usages": {
            "limit_5h": {"used_ratio": 0.25},
            "limit_month_total": {"used_ratio": 0.5},
            "limit_month_code": code
        }}));
        let shares = response
            .usages
            .as_ref()
            .and_then(KimiCodeUsagePools::monthly_shares);
        assert!(monthly_breakdown_details(shares).is_empty());
        let usage = crate::providers::kimi::code_api::snapshot_from_code_api_response(response)
            .expect("quota windows survive");
        assert_eq!(
            usage.secondary.map(|window| window.used_percent),
            Some(25.0)
        );
        assert_eq!(usage.extra_rate_windows[0].window.used_percent, 50.0);
    }
}

#[test]
fn tiny_shares_render_below_one_percent() {
    assert_eq!(percent_text(0.4), "<1%");
    assert_eq!(percent_text(0.8), "<1%");
    assert_eq!(percent_text(1.0), "1%");
    assert_eq!(percent_text(0.0), "0%");
    assert_eq!(percent_text(140.0), "100%");
}
