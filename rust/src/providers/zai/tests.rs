use super::*;
use settings::EnvMap;
use std::collections::HashMap;

fn env_map(pairs: &[(&str, &str)]) -> EnvMap {
    pairs
        .iter()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect::<HashMap<_, _>>()
}

#[test]
fn request_url_adds_team_type_query_for_team_context() {
    let env = env_map(&[]);
    let team = ZaiTeamContext {
        organization_id: "org".to_string(),
        project_id: "project".to_string(),
    };

    let url = ZaiProvider::request_url(&env, ZaiRegion::Global, Some(&team)).expect("url");

    assert_eq!(
        url.as_str(),
        "https://api.z.ai/api/monitor/usage/quota/limit?type=2"
    );
}

#[test]
fn quota_url_uses_bigmodel_cn_region_aliases() {
    let ctx = FetchContext {
        api_region: Some("bigmodel-cn".to_string()),
        ..FetchContext::default()
    };
    let env = env_map(&[]);
    let region = ZaiProvider::effective_region(&ctx, &env);
    assert_eq!(region, ZaiRegion::BigModelCn);

    let url = ZaiProvider::quota_url(&env, region).expect("url");

    assert_eq!(
        url.as_str(),
        "https://open.bigmodel.cn/api/monitor/usage/quota/limit"
    );
}

#[test]
fn global_region_defaults_to_api_z_ai() {
    let env = env_map(&[]);

    let url = ZaiProvider::quota_url(&env, ZaiRegion::Global).expect("url");

    assert_eq!(
        url.as_str(),
        "https://api.z.ai/api/monitor/usage/quota/limit"
    );
}

#[test]
fn parses_workspace_pair_as_team_context() {
    let parsed = parse_team_context_pair(" org-team | project-team ").expect("team context");

    assert_eq!(parsed.organization_id, "org-team");
    assert_eq!(parsed.project_id, "project-team");
}

#[test]
fn parses_successful_response_without_message() {
    let provider = ZaiProvider::new();
    let quota: ZaiQuotaResponse = serde_json::from_value(serde_json::json!({
        "code": 200,
        "data": {
            "planName": "BigModel CN",
            "limits": [{
                "type": "TOKENS_LIMIT",
                "used": 10,
                "limit": 100,
                "unit": 3,
                "number": 5
            }]
        }
    }))
    .unwrap();

    let usage = provider.parse_quota_response(&quota).unwrap();

    assert_eq!(usage.login_method.as_deref(), Some("BigModel CN"));
    assert_eq!(usage.primary.used_percent, 10.0);
}

#[test]
fn parses_current_api_percentage_and_reset_time() {
    let provider = ZaiProvider::new();
    let quota: ZaiQuotaResponse = serde_json::from_value(serde_json::json!({
        "code": 200,
        "data": {
            "limits": [{
                "type": "TOKENS_LIMIT",
                "unit": 3,
                "number": 5,
                "usage": 800000000,
                "currentValue": 600000000,
                "remaining": 200000000,
                "percentage": 75,
                "nextResetTime": 1770648402389_i64
            }]
        }
    }))
    .unwrap();

    let usage = provider.parse_quota_response(&quota).unwrap();

    assert_eq!(usage.primary.used_percent, 75.0);
    assert_eq!(usage.primary.window_minutes, Some(300));
    assert!(usage.primary.resets_at.is_some());
}

#[test]
fn five_hour_reset_plausibility_drops_impossible_timestamp() {
    let now = Utc::now();
    let quota: ZaiQuotaResponse = serde_json::from_value(serde_json::json!({
        "code": 200,
        "data": {"limits": [
            {
                "type": "TOKENS_LIMIT",
                "unit": 3,
                "number": 5,
                "percentage": 25,
                "nextResetTime": (now + chrono::Duration::hours(10)).timestamp_millis()
            },
            {
                "type": "TOKENS_LIMIT",
                "unit": 6,
                "number": 1,
                "percentage": 9,
                "nextResetTime": (now + chrono::Duration::days(6)).timestamp_millis()
            },
            {
                "type": "TIME_LIMIT",
                "unit": 5,
                "number": 1,
                "percentage": 22,
                "nextResetTime": (now + chrono::Duration::days(20)).timestamp_millis()
            }
        ]}
    }))
    .unwrap();

    let usage = ZaiProvider::new().parse_quota_response(&quota).unwrap();
    assert_eq!(usage.primary.used_percent, 25.0);
    assert_eq!(usage.primary.window_minutes, Some(300));
    assert_eq!(usage.primary.reset_description.as_deref(), Some("5-hour"));
    assert!(usage.primary.resets_at.is_none());
    assert!(usage.secondary.as_ref().is_some_and(|window| {
        window.window_minutes == Some(10080) && window.resets_at.is_some()
    }));
    let mcp = usage
        .extra_rate_windows
        .iter()
        .find(|window| window.id == "zai-mcp")
        .expect("MCP extra window");
    assert!(mcp.window.resets_at.is_some());
}

#[test]
fn five_hour_reset_horizon_allows_one_minute_clock_skew() {
    let now = Utc::now();
    let edge = now + chrono::Duration::minutes(5 * 60 + 1);
    assert!(is_plausible_five_hour_reset(Some(300), edge, now));
    assert!(!is_plausible_five_hour_reset(
        Some(300),
        edge + chrono::Duration::milliseconds(1),
        now
    ));
    assert!(is_plausible_five_hour_reset(Some(10080), edge, now));
    assert!(is_plausible_five_hour_reset(None, edge, now));
}

#[test]
fn credit_limit_plan_drives_primary_and_weekly_windows() {
    // Upstream 0.49.0 #2724/#2712: credit-based Coding Plans report
    // CREDIT_LIMIT rows shaped like TOKENS_LIMIT. Without this, usage
    // sticks at 0% used / 100% remaining.
    let provider = ZaiProvider::new();
    let quota: ZaiQuotaResponse = serde_json::from_value(serde_json::json!({
        "code": 200,
        "data": {
            "planName": "GLM Coding Lite",
            "limits": [
                {
                    "type": "CREDIT_LIMIT",
                    "unit": 3,
                    "number": 5,
                    "usage": 500,
                    "currentValue": 475,
                    "remaining": 25,
                    "percentage": 95,
                    "nextResetTime": 1770648402389_i64
                },
                {
                    "type": "CREDIT_LIMIT",
                    "unit": 6,
                    "number": 1,
                    "usage": 3000,
                    "currentValue": 1200,
                    "remaining": 1800,
                    "percentage": 40
                }
            ]
        }
    }))
    .unwrap();

    let usage = provider.parse_quota_response(&quota).unwrap();

    // Shortest window (5h credits) is the primary; longest (weekly) secondary.
    assert!((usage.primary.used_percent - 95.0).abs() < f64::EPSILON);
    assert_eq!(usage.primary.window_minutes, Some(300));
    assert_eq!(usage.primary.reset_description.as_deref(), Some("5-hour"));
    assert!(usage.primary.resets_at.is_some());
    let secondary = usage.secondary.expect("weekly credit window");
    assert!((secondary.used_percent - 40.0).abs() < f64::EPSILON);
    assert_eq!(secondary.window_minutes, Some(10080));
}

#[test]
fn usage_signal_overrides_stale_percentage() {
    // Upstream 0.49.0 `parseLimit`: a positive `usage` total makes the
    // absolute used signal authoritative; the API's `percentage` is only
    // trusted without it.
    let provider = ZaiProvider::new();
    let quota: ZaiQuotaResponse = serde_json::from_value(serde_json::json!({
        "code": 200,
        "data": {
            "limits": [{
                "type": "CREDIT_LIMIT",
                "unit": 3,
                "number": 5,
                "usage": 500,
                "currentValue": 25,
                "remaining": 475,
                "percentage": 95
            }]
        }
    }))
    .unwrap();

    let usage = provider.parse_quota_response(&quota).unwrap();

    assert!((usage.primary.used_percent - 5.0).abs() < f64::EPSILON);
}

#[test]
fn time_limit_primary_carries_mcp_label_without_duration() {
    // Upstream 0.48.0: TIME_LIMIT (MCP) windows no longer keep explicit
    // duration minutes and label as "MCP", not the old monthly sentinel.
    let provider = ZaiProvider::new();
    let quota: ZaiQuotaResponse = serde_json::from_value(serde_json::json!({
        "code": 200,
        "data": {
            "limits": [{
                "type": "TIME_LIMIT",
                "unit": 3,
                "number": 5,
                "usage": 100,
                "currentValue": 20,
                "remaining": 80,
                "percentage": 25,
                "nextResetTime": 123000_i64
            }]
        }
    }))
    .unwrap();
    let usage = provider.parse_quota_response(&quota).unwrap();
    assert_eq!(usage.primary.window_minutes, None);
    assert_eq!(usage.primary.reset_description.as_deref(), Some("MCP"));
    assert!(usage.primary.resets_at.is_some());
}

#[test]
fn bare_time_limit_primary_has_no_window_duration() {
    let provider = ZaiProvider::new();
    let quota: ZaiQuotaResponse = serde_json::from_value(serde_json::json!({
        "code": 200,
        "data": {
            "limits": [{
                "type": "TIME_LIMIT",
                "unit": 1,
                "number": 0,
                "usage": 100,
                "currentValue": 20,
                "remaining": 80,
                "percentage": 25,
                "nextResetTime": 123000_i64
            }]
        }
    }))
    .unwrap();
    let usage = provider.parse_quota_response(&quota).unwrap();
    assert_eq!(usage.primary.window_minutes, None);
    assert_eq!(usage.primary.reset_description.as_deref(), Some("MCP"));
}

#[test]
fn mcp_limit_renders_separate_named_window() {
    // Upstream 0.48.0 GLM Coding Plan layout: coding-limit primary +
    // MCP as a named extra window; MCP 1-minute marker no longer maps
    // to a monthly sentinel secondary.
    let provider = ZaiProvider::new();
    let quota: ZaiQuotaResponse = serde_json::from_value(serde_json::json!({
        "code": 200,
        "data": {
            "limits": [
                {
                    "type": "TOKENS_LIMIT",
                    "unit": 6,
                    "number": 1,
                    "percentage": 34
                },
                {
                    "type": "TIME_LIMIT",
                    "unit": 5,
                    "number": 1,
                    "percentage": 10
                }
            ]
        }
    }))
    .unwrap();
    let usage = provider.parse_quota_response(&quota).unwrap();

    assert_eq!(usage.primary.window_minutes, Some(10080));
    assert_eq!(
        usage.primary.reset_description.as_deref(),
        Some("1 week window")
    );
    assert!(usage.secondary.is_none());
    let mcp = usage
        .extra_rate_windows
        .iter()
        .find(|window| window.id == "zai-mcp")
        .expect("MCP extra window");
    assert_eq!(mcp.title, "MCP");
    assert_eq!(mcp.window.window_minutes, None);
    assert_eq!(mcp.window.reset_description.as_deref(), Some("MCP"));
    assert_eq!(mcp.window.used_percent, 10.0);
}

#[test]
fn session_five_hour_window_becomes_primary_over_weekly() {
    // Upstream 0.48.0 GLM Coding Plan: 2+ TOKENS_LIMIT entries →
    // shortest (5-hour) window primary, longest (weekly) secondary.
    let provider = ZaiProvider::new();
    let quota: ZaiQuotaResponse = serde_json::from_value(serde_json::json!({
        "code": 200,
        "data": {
            "limits": [
                {
                    "type": "TOKENS_LIMIT",
                    "unit": 3,
                    "number": 5,
                    "percentage": 55,
                    "nextResetTime": 1770648402389_i64
                },
                {
                    "type": "TOKENS_LIMIT",
                    "unit": 6,
                    "number": 1,
                    "percentage": 34
                }
            ]
        }
    }))
    .unwrap();
    let usage = provider.parse_quota_response(&quota).unwrap();

    assert_eq!(usage.primary.used_percent, 55.0);
    assert_eq!(usage.primary.window_minutes, Some(300));
    assert_eq!(usage.primary.reset_description.as_deref(), Some("5-hour"));
    assert!(usage.primary.resets_at.is_some());
    let secondary = usage.secondary.expect("weekly secondary");
    assert_eq!(secondary.used_percent, 34.0);
    assert_eq!(secondary.window_minutes, Some(10080));
    assert!(usage.model_specific.is_none());
}

#[test]
fn plan_name_falls_back_to_level_key() {
    let provider = ZaiProvider::new();
    let quota: ZaiQuotaResponse = serde_json::from_value(serde_json::json!({
        "code": 200,
        "data": {
            "level": "GLM Coding Plan",
            "limits": []
        }
    }))
    .unwrap();
    let usage = provider.parse_quota_response(&quota).unwrap();
    assert_eq!(usage.login_method.as_deref(), Some("GLM Coding Plan"));

    for key in ["plan", "plan_type", "packageName"] {
        let quota: ZaiQuotaResponse = serde_json::from_value(serde_json::json!({
            "code": 200,
            "data": { key: "Coding Plan", "limits": [] }
        }))
        .unwrap();
        let usage = provider.parse_quota_response(&quota).unwrap();
        assert_eq!(usage.login_method.as_deref(), Some("Coding Plan"), "{key}");
    }
}

#[test]
fn empty_plan_fields_fall_back_to_default() {
    let provider = ZaiProvider::new();
    let quota: ZaiQuotaResponse = serde_json::from_value(serde_json::json!({
        "code": 200,
        "data": { "planName": "  ", "level": "", "limits": [] }
    }))
    .unwrap();
    let usage = provider.parse_quota_response(&quota).unwrap();
    assert_eq!(usage.login_method.as_deref(), Some("z.ai"));
}

#[test]
fn preserves_api_code_error_message() {
    let provider = ZaiProvider::new();
    let quota: ZaiQuotaResponse = serde_json::from_value(serde_json::json!({
        "code": 401,
        "message": "invalid token"
    }))
    .unwrap();

    let error = provider.parse_quota_response(&quota).unwrap_err();

    assert!(error.to_string().contains("invalid token"));
}
