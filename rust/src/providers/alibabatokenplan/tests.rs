
use super::*;

#[test]
fn missing_bailian_cli_is_local_runtime_offline() {
    let provider = AlibabaTokenPlanProvider::new();
    let error = ProviderError::NotInstalled(
        "Bailian CLI 'bl' is not installed or not on PATH.".to_string(),
    );
    assert_eq!(
        provider.error_state_kind(&error),
        crate::core::ProviderStateKind::LocalRuntimeOffline
    );
}
#[test]
fn parses_token_plan_instance_payload() {
    let payload = serde_json::json!({
        "data": {
            "tokenPlanInstanceInfo": {
                "commodityName": "Token Plan Pro",
                "quotaInfo": {
                    "usedQuota": "1250",
                    "totalQuota": "5000"
                },
                "nextRefreshTime": 1780763009000_i64
            }
        }
    });
    let snapshot =
        AlibabaTokenPlanProvider::parse_usage_snapshot(payload.to_string().as_bytes()).unwrap();
    assert_eq!(snapshot.plan_name.as_deref(), Some("Token Plan Pro"));
    assert_eq!(snapshot.used_quota, Some(1250.0));
    assert_eq!(snapshot.total_quota, Some(5000.0));

    let usage = AlibabaTokenPlanProvider::snapshot_to_usage(snapshot).unwrap();
    assert_eq!(usage.primary.used_percent, 25.0);
    assert_eq!(
        usage.primary.reset_description.as_deref(),
        Some("1,250 / 5,000 credits used")
    );
    assert_eq!(usage.login_method.as_deref(), Some("Token Plan Pro"));
}

#[test]
fn expands_nested_string_payloads_and_uses_remaining_quota() {
    let nested = serde_json::json!({
        "successResponse": serde_json::json!({
            "instances": [
                {"status": "EXPIRED", "quota": 1000, "remaining": 1000},
                {"status": "ACTIVE", "packageName": "Team", "quota": 1000, "remaining": 250}
            ]
        }).to_string()
    });
    let snapshot =
        AlibabaTokenPlanProvider::parse_usage_snapshot(nested.to_string().as_bytes()).unwrap();
    assert_eq!(snapshot.plan_name.as_deref(), Some("Team"));
    assert_eq!(
        used_percent(
            snapshot.used_quota,
            snapshot.total_quota,
            snapshot.remaining_quota
        ),
        Some(75.0)
    );
}

#[test]
fn parses_new_subscription_summary_payload() {
    let payload = serde_json::json!({
        "success": true,
        "Data": {
            "ProductName": "Token Plan Team",
            "TotalValue": "1000000",
            "TotalSurplusValue": "250000",
            "NearestExpireDate": "2026-06-30"
        }
    });
    let snapshot =
        AlibabaTokenPlanProvider::parse_usage_snapshot(payload.to_string().as_bytes()).unwrap();
    assert_eq!(snapshot.plan_name.as_deref(), Some("Token Plan Team"));
    assert_eq!(snapshot.total_quota, Some(1_000_000.0));
    assert_eq!(snapshot.remaining_quota, Some(250_000.0));
    assert_eq!(
        used_percent(
            snapshot.used_quota,
            snapshot.total_quota,
            snapshot.remaining_quota
        ),
        Some(75.0)
    );
    assert!(snapshot.resets_at.is_some());
}

#[test]
fn detects_login_payloads() {
    let err = AlibabaTokenPlanProvider::parse_usage_snapshot(
        br#"{"code":"NeedLogin","message":"please login"}"#,
    )
    .unwrap_err();
    assert!(matches!(err, ProviderError::AuthRequired));
}

#[test]
fn extracts_sec_token_from_html_or_cookie() {
    assert_eq!(
        extract_sec_token(r#"<script>{"secToken":"abc123"}</script>"#).as_deref(),
        Some("abc123")
    );
    assert_eq!(
        extract_sec_token(
            r#"<script>window.ALIYUN_CONSOLE_CONFIG = { SEC_TOKEN: "upper123" };</script>"#
        )
        .as_deref(),
        Some("upper123")
    );
    assert_eq!(
        cookie_value("sec_token", "foo=bar; sec_token=xyz"),
        Some("xyz".to_string())
    );
}

#[test]
fn sec_token_shell_referer_uses_same_origin_root() {
    assert_eq!(
        dashboard_referer(Region::CnPersonal),
        "https://bailian.console.aliyun.com/"
    );
    assert_eq!(
        dashboard_referer(Region::IntlPersonal),
        "https://modelstudio.console.alibabacloud.com/"
    );
}

#[test]
fn default_region_cn_team_urls_match_legacy() {
    let region = Region::Cn;
    assert_eq!(
        AlibabaTokenPlanProvider::team_quota_url(region),
        "https://bailian.console.aliyun.com/data/api.json?action=GetSubscriptionSummary&product=BssOpenAPI-V3&_tag="
    );
    assert_eq!(
        AlibabaTokenPlanProvider::team_request_params(region),
        serde_json::json!({"ProductCode": "sfm_tokenplanteams_dp_cn"}).to_string()
    );
    assert_eq!(region.current_region_id(), "cn-beijing");
}
