
use super::*;

#[test]
fn parses_quota_limited_key_usage() {
    let json = r#"
        {
          "mode": "quota_limited",
          "isValid": true,
          "status": "active",
          "remaining": 75,
          "unit": "USD",
          "quota": {
            "limit": 100,
            "used": 25,
            "remaining": 75,
            "unit": "USD"
          },
          "rate_limits": [
            {
              "window": "5h",
              "limit": 20,
              "used": 5,
              "remaining": 15,
              "reset_at": "2026-07-11T12:30:00Z"
            },
            {
              "window": "7d",
              "limit": 200,
              "used": 40,
              "remaining": 160
            }
          ],
          "expires_at": "2026-08-01T00:00:00Z",
          "usage": {
            "today": {
              "requests": 4,
              "total_tokens": 1200,
              "actual_cost": 1.25
            },
            "total": {
              "requests": 40,
              "total_tokens": 12000,
              "actual_cost": 25
            }
          }
        }
        "#;

    let parsed = parse_usage_body(json).unwrap();
    assert_eq!(parsed.mode, "quota_limited");
    assert_eq!(parsed.quota.as_ref().unwrap().remaining, 75.0);
    assert_eq!(parsed.rate_limits.len(), 2);
    assert_eq!(parsed.today.as_ref().unwrap().total_tokens, 1200);

    let result = snapshot_from_parsed(parsed);
    assert!((result.usage.primary.used_percent - 25.0).abs() < f64::EPSILON);
    assert!(result.cost.is_none());
    assert!(
        result
            .usage
            .extra_rate_windows
            .iter()
            .any(|w| w.id == "5h" && w.window.window_minutes == Some(300))
    );
    assert!(result.usage.extra_rate_windows.iter().any(|w| {
        w.id == "today"
            && w.window
                .reset_description
                .as_deref()
                .is_some_and(|d| d.contains("1200 tokens"))
    }));
    assert!(
        result
            .usage
            .extra_rate_windows
            .iter()
            .any(|w| w.id == "expires")
    );
}

#[test]
fn parses_subscription_usage_windows() {
    let json = r#"
        {
          "mode": "unrestricted",
          "isValid": true,
          "planName": "Claude Team",
          "remaining": 8,
          "unit": "USD",
          "subscription": {
            "daily_usage_usd": 2,
            "weekly_usage_usd": 10,
            "monthly_usage_usd": 30,
            "daily_limit_usd": 10,
            "weekly_limit_usd": 40,
            "monthly_limit_usd": 100,
            "expires_at": "2026-08-15T00:00:00.123Z"
          }
        }
        "#;

    let result = snapshot_from_parsed(parse_usage_body(json).unwrap());
    assert!((result.usage.primary.used_percent - 20.0).abs() < f64::EPSILON);
    assert!((result.usage.secondary.as_ref().unwrap().used_percent - 25.0).abs() < f64::EPSILON);
    assert!((result.usage.tertiary.as_ref().unwrap().used_percent - 30.0).abs() < f64::EPSILON);
    assert!(result.usage.account_organization.is_none());
    assert_eq!(
        result.usage.login_method.as_deref(),
        Some("Claude Team (unrestricted)")
    );
    assert!(
        result
            .usage
            .extra_rate_windows
            .iter()
            .any(|w| w.id == "expires")
    );
}

#[test]
fn subscription_without_limits_shows_usage_not_empty() {
    let json = r#"
        {
          "mode": "unrestricted",
          "planName": "Soft plan",
          "unit": "USD",
          "subscription": {
            "daily_usage_usd": 2.5,
            "weekly_usage_usd": 10,
            "monthly_usage_usd": 30
          },
          "balance": 12.0
        }
        "#;
    let result = snapshot_from_parsed(parse_usage_body(json).unwrap());
    assert!(result.usage.primary.is_informational);
    assert!(
        result
            .usage
            .primary
            .reset_description
            .as_deref()
            .is_some_and(|d| d.contains("$2.50") && d.contains("day"))
    );
    assert!(result.cost.is_some());
    assert_eq!(result.cost.as_ref().unwrap().limit, Some(12.0));
    assert!(result.usage.account_organization.is_none());
    assert!(
        result
            .usage
            .login_method
            .as_deref()
            .is_some_and(|m| m.starts_with("Soft plan"))
    );
}

#[test]
fn preserves_authoritative_subscription_windows() {
    let json = r#"
        {
          "mode": "unrestricted",
          "subscription": {
            "daily_usage_usd": 120.23,
            "weekly_usage_usd": 229.20,
            "monthly_usage_usd": 1296.23,
            "daily_limit_usd": 120,
            "weekly_limit_usd": 700,
            "monthly_limit_usd": 2800
          }
        }
        "#;

    let result = snapshot_from_parsed(parse_usage_body(json).unwrap());
    assert!((result.usage.primary.used_percent - 100.0).abs() < f64::EPSILON);
    assert!(
        (result.usage.secondary.as_ref().unwrap().used_percent - (229.20 / 700.0 * 100.0)).abs()
            < 0.001
    );
    assert_eq!(
        result.usage.primary.reset_description.as_deref(),
        Some("$120.23 / $120.00")
    );
    assert_eq!(
        result
            .usage
            .secondary
            .as_ref()
            .and_then(|w| w.reset_description.as_deref()),
        Some("$229.20 / $700.00")
    );
}

#[test]
fn parses_wallet_balance_only() {
    let json = r#"
        {
          "mode": "unrestricted",
          "isValid": true,
          "planName": "Wallet plan",
          "remaining": 42.5,
          "unit": "USD",
          "balance": 42.5
        }
        "#;

    let result = snapshot_from_parsed(parse_usage_body(json).unwrap());
    assert!(result.usage.primary.is_informational);
    assert_eq!(
        result.usage.primary.reset_description.as_deref(),
        Some("$42.50 balance")
    );
    assert_eq!(
        result.usage.login_method.as_deref(),
        Some("Wallet plan (unrestricted)")
    );
    let cost = result.cost.unwrap();
    assert_eq!(cost.limit, Some(42.5));
    assert_eq!(cost.period, "balance");
}

#[test]
fn invalid_credentials_flag_is_detected() {
    let json = r#"{"mode":"unrestricted","isValid":false}"#;
    let parsed = parse_usage_body(json).unwrap();
    assert!(!parsed.is_valid);
}

#[test]
fn usage_url_accepts_root_versioned_and_complete_urls() {
    let root = Url::parse("https://api.example.com").unwrap();
    assert_eq!(
        usage_url(&root).unwrap().as_str(),
        "https://api.example.com/v1/usage"
    );

    let versioned = Url::parse("https://api.example.com/v1").unwrap();
    assert_eq!(
        usage_url(&versioned).unwrap().as_str(),
        "https://api.example.com/v1/usage"
    );

    let complete = Url::parse("https://api.example.com/v1/usage").unwrap();
    assert_eq!(
        usage_url(&complete).unwrap().as_str(),
        "https://api.example.com/v1/usage"
    );
}

#[test]
fn settings_allow_https_and_loopback_http_only() {
    assert!(validated_sub2api_base_url("https://api.example.com").is_ok());
    assert!(validated_sub2api_base_url("http://127.0.0.1:8080").is_ok());
    assert!(validated_sub2api_base_url("http://api.example.com").is_err());
    assert!(validated_sub2api_base_url("https://user:pass@api.example.com").is_err());
    assert!(validated_sub2api_base_url("https://api.example.com?token=secret").is_err());
    assert!(validated_sub2api_base_url("https://api.example.com#fragment").is_err());
}

#[test]
fn cleans_quoted_env_values() {
    assert_eq!(
        clean_env_value("  \"sk-test\"  ").as_deref(),
        Some("sk-test")
    );
    assert_eq!(clean_env_value("''"), None);
}

#[test]
fn usage_request_includes_days_and_timezone() {
    let base = Url::parse("https://api.example.com").unwrap();
    let url = usage_request_url(&base).unwrap();
    let query: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
    assert_eq!(query.get("days").map(String::as_str), Some("30"));
    assert!(query.contains_key("timezone"));
    assert!(!query.get("timezone").unwrap().is_empty());
}
