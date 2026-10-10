use super::*;
use chrono::TimeZone;

#[test]
fn parses_current_token_plan_5h_and_weekly() {
    let inner = r#"{
          "code": 0,
          "data": {
            "per5HourPercentage": 0.03,
            "per5HourResetTime": 1700003600000,
            "per1WeekPercentage": 0.01,
            "per1WeekResetTime": 1700086400000
          },
          "success": true
        }"#;
    let payload = serde_json::json!({
        "data": {
            "DataV2": {
                "data": inner,
            },
        },
        "httpStatusCode": 200,
    });

    let snapshot = QwenCloudProvider::parse(payload.to_string().as_bytes(), None, None).unwrap();
    assert_eq!(snapshot.five_hour_used_percent, Some(3.0));
    assert_eq!(
        snapshot.five_hour_resets_at,
        Some(Utc.timestamp_opt(1_700_003_600, 0).single().unwrap())
    );
    assert_eq!(snapshot.weekly_used_percent, Some(1.0));
    assert_eq!(
        snapshot.weekly_resets_at,
        Some(Utc.timestamp_opt(1_700_086_400, 0).single().unwrap())
    );

    let usage = QwenCloudProvider::new()
        .snapshot_to_usage(snapshot)
        .unwrap();
    assert_eq!(usage.primary.used_percent, 3.0);
    assert_eq!(usage.primary.window_minutes, Some(FIVE_HOUR_MINUTES));
    assert_eq!(usage.primary_label, None);
    assert_eq!(usage.secondary.as_ref().map(|w| w.used_percent), Some(1.0));
    assert_eq!(
        usage.secondary.as_ref().and_then(|w| w.window_minutes),
        Some(WEEKLY_MINUTES)
    );
}

#[test]
fn parses_personal_usage_fixture_shape() {
    let payload = serde_json::json!({
        "code": "200",
        "data": {
            "DataV2": {
                "data": {
                    "success": true,
                    "data": {
                        "per5HourPercentage": 0.0009973083333333333,
                        "per5HourResetTime": 1784813220000_i64,
                        "per1WeekPercentage": 0.0003014725,
                        "per1WeekResetTime": 1785234900000_i64
                    }
                },
                "success": true,
                "httpStatus": 200
            }
        },
        "successResponse": true
    });
    let usage = QwenCloudProvider::new()
        .snapshot_to_usage(
            QwenCloudProvider::parse(payload.to_string().as_bytes(), None, None).unwrap(),
        )
        .unwrap();
    assert!((usage.primary.used_percent - 0.09973083333333333).abs() < 1e-9);
    assert_eq!(usage.primary.window_minutes, Some(300));
    assert!(usage.secondary.is_some());
    assert_eq!(
        usage.secondary.as_ref().and_then(|w| w.window_minutes),
        Some(10080)
    );
}

#[test]
fn parses_nested_equity_list_legacy() {
    let payload = serde_json::json!({
        "code": "200",
        "successResponse": true,
        "data": {
            "TotalCount": 1,
            "Data": [
                {
                    "InstanceCode": "qwen-token-plan",
                    "Status": "NORMAL",
                    "EndTime": 1_701_000_000_000_i64,
                    "EquityList": [
                        {
                            "Type": "CREDITS",
                            "CycleTotalValue": "1000",
                            "CycleSurplusValue": "875"
                        }
                    ]
                }
            ]
        }
    });
    let snapshot = QwenCloudProvider::parse(payload.to_string().as_bytes(), None, None).unwrap();
    assert_eq!(snapshot.total_quota, Some(1000.0));
    assert_eq!(snapshot.remaining_quota, Some(875.0));
    let usage = QwenCloudProvider::new()
        .snapshot_to_usage(snapshot)
        .unwrap();
    assert_eq!(usage.primary.used_percent, 12.5);
    assert_eq!(usage.primary.window_minutes, Some(LEGACY_MINUTES));
}

#[test]
fn parses_flat_subscription_summary_legacy() {
    let payload = serde_json::json!({
        "Success": true,
        "Data": {
            "TotalCount": 1,
            "TotalValue": 2000,
            "TotalSurplusValue": 1500
        }
    });
    let snapshot = QwenCloudProvider::parse(payload.to_string().as_bytes(), None, None).unwrap();
    assert_eq!(snapshot.total_quota, Some(2000.0));
    assert_eq!(snapshot.remaining_quota, Some(1500.0));
    assert_eq!(
        used_percent(
            snapshot.used_quota,
            snapshot.total_quota,
            snapshot.remaining_quota
        ),
        Some(25.0)
    );
}

#[test]
fn login_payload_maps_to_auth_required() {
    let err = QwenCloudProvider::parse(
        br#"{"code":"ConsoleNeedLogin","message":"You need to log in.","successResponse":false}"#,
        None,
        None,
    )
    .unwrap_err();
    assert!(matches!(err, ProviderError::AuthRequired));
}

#[test]
fn attaches_plan_name_from_subscription() {
    let usage_payload = serde_json::json!({
        "data": {
            "per5HourPercentage": 0.1,
            "per5HourResetTime": 1700003600000_i64,
            "per1WeekPercentage": 0.2,
            "per1WeekResetTime": 1700086400000_i64
        }
    });
    let subscription = serde_json::json!({
        "data": { "specCode": "pro" }
    });
    let snapshot = QwenCloudProvider::parse(
        usage_payload.to_string().as_bytes(),
        Some(subscription.to_string().as_bytes()),
        None,
    )
    .unwrap();
    assert_eq!(snapshot.plan_name.as_deref(), Some("Pro"));
    let usage = QwenCloudProvider::new()
        .snapshot_to_usage(snapshot)
        .unwrap();
    assert_eq!(usage.login_method.as_deref(), Some("Pro"));
}

#[test]
fn extracts_sec_token_from_html() {
    assert_eq!(
        extract_sec_token(r#"<script>sec_token = "qwen-html-token";</script>"#).as_deref(),
        Some("qwen-html-token")
    );
    assert_eq!(
        cookie_value("login_aliyunid_csrf", "foo=bar; login_aliyunid_csrf=tok"),
        Some("tok".to_string())
    );
}

#[test]
fn metadata_labels_match_upstream() {
    let provider = QwenCloudProvider::new();
    assert_eq!(provider.metadata().session_label, "5-hour");
    assert_eq!(provider.metadata().weekly_label, "Weekly");
    assert!(!provider.metadata().default_enabled);
    assert_eq!(provider.metadata().dashboard_url, Some(DASHBOARD_URL));
}

#[test]
fn weekly_only_shape_promotes_weekly_to_primary() {
    // Real-world fixture: the account exposes only the weekly window, so
    // there is no per5HourPercentage at all. Previously this failed with
    // "Qwen Cloud usage windows missing"; now the weekly window becomes
    // the primary window instead.
    let payload = serde_json::json!({
        "code": "200",
        "data": {
            "DataV2": {
                "data": {
                    "success": true,
                    "data": {
                        "per1WeekPercentage": 0.8439116574633999,
                        "per1WeekResetTime": 1785234900000_i64
                    }
                },
                "success": true,
                "httpStatus": 200
            }
        },
        "successResponse": true
    });
    let usage = QwenCloudProvider::new()
        .snapshot_to_usage(
            QwenCloudProvider::parse(payload.to_string().as_bytes(), None, None).unwrap(),
        )
        .unwrap();
    assert!((usage.primary.used_percent - 84.39116574634).abs() < 1e-9);
    assert_eq!(usage.primary.window_minutes, Some(WEEKLY_MINUTES));
    assert_eq!(usage.primary_label.as_deref(), Some("Weekly"));
    assert!(usage.secondary.is_none());
}

/// Pins the console `params` payload: the cornerstone fields, the optional
/// `cna` anonymous id, and the caller's data parameters.
#[test]
fn params_json_wraps_data_with_cornerstone_fields() {
    for (cookie, anonymous_id) in [("cna=anon-1; other=x", Some("anon-1")), ("other=x", None)] {
        let mut data = Map::new();
        data.insert("commodityCode".into(), json!("sfm_tokenplan_public_cn"));
        let params = build_params_json("zeldaEasy.test.api", data, cookie);
        let value: Value = serde_json::from_str(&params).unwrap();
        let trace = value["Data"]["cornerstoneParam"]["feTraceId"]
            .as_str()
            .unwrap();
        assert_eq!(trace.len(), 36);
        assert_eq!(trace, trace.to_lowercase());
        let mut cornerstone = json!({
            "feTraceId": trace,
            "feURL": DASHBOARD_URL,
            "protocol": "V2",
            "console": "ONE_CONSOLE",
            "productCode": "p_efm",
            "domain": "home.qwencloud.com",
            "consoleSite": "QWENCLOUD",
            "userNickName": "",
            "userPrincipalName": "",
            "xsp_lang": "en-US",
        });
        if let Some(id) = anonymous_id {
            cornerstone["X-Anonymous-Id"] = json!(id);
        }
        let expected = json!({
            "Api": "zeldaEasy.test.api",
            "V": "1.0",
            "Data": {"commodityCode": "sfm_tokenplan_public_cn", "cornerstoneParam": cornerstone},
        });
        assert_eq!(params, expected.to_string());
    }
}
