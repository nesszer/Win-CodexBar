use super::*;
use base64::Engine;
use serde_json::json;

fn credentials_with_claims(claims: serde_json::Value, account: &str) -> CodexCredentials {
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(serde_json::to_vec(&claims).unwrap());
    CodexApi::parse_credentials_json(
        &json!({"tokens": {"access_token": "opaque", "account_id": account,
            "id_token": format!("header.{payload}.signature")}})
        .to_string(),
    )
    .unwrap()
}

#[test]
fn subscription_period_reads_explicit_id_token_dates_without_inventing_expiry() {
    // 复现现场：用量成功，订阅请求不可用，而 id_token 明确包含周期日期。
    let credentials = credentials_with_claims(
        json!({
            "iat": 1_790_000_000, "exp": 1_990_000_000,
            "https://api.openai.com/auth": {
                "chatgpt_account_id": "account-a",
                "chatgpt_subscription_active_start": "2026-10-06T13:04:47Z",
                "chatgpt_subscription_active_until": "2026-11-06T13:04:47Z"
            }
        }),
        "account-a",
    );
    let period = credentials.subscription_period.expect("explicit dates");
    assert_eq!(
        period.starts_at.unwrap().to_rfc3339(),
        "2026-10-06T13:04:47+00:00"
    );
    assert_eq!(
        period.ends_at.unwrap().to_rfc3339(),
        "2026-11-06T13:04:47+00:00"
    );
    assert_eq!(period.recorded_at.unwrap().timestamp(), 1_790_000_000);

    // 固定采集时间，避免运行日恰好等于订阅日期时产生序列化误报。
    let mut usage = UsageSnapshot::new(crate::core::RateWindow::new(12.0));
    usage.updated_at = "2026-01-01T00:00:00Z".parse().unwrap();
    let usage = subscription::apply_subscription_result(
        usage,
        crate::providers::openai::OpenAISubscriptionFetchResult::Unavailable,
        Some(&period),
    );
    assert!(usage.subscription.is_none());
    let observation = usage.subscription_observation.unwrap();
    assert_eq!(observation.starts_at, period.starts_at);
    assert_eq!(observation.period_end, period.ends_at);
    assert_eq!(observation.recorded_at, period.recorded_at);
    assert!(observation.expires_at.is_none());
    assert!(observation.renews_at.is_none());
    assert_eq!(
        observation.source,
        crate::core::SubscriptionDateSource::CredentialRecord
    );
    let serialized = serde_json::to_string(&usage).unwrap();
    assert!(!serialized.contains("subscription_observation"));
    assert!(!serialized.contains("2026-11-06"));
}

#[test]
fn subscription_period_rejects_another_account_and_malformed_or_inverted_dates() {
    for (account, start, end) in [
        ("account-b", "2026-10-06T13:04:47Z", "2026-11-06T13:04:47Z"),
        ("account-a", "bad-date", "2026-11-06T13:04:47Z"),
        ("account-a", "2026-12-06T13:04:47Z", "2026-11-06T13:04:47Z"),
    ] {
        let credentials = credentials_with_claims(
            json!({"https://api.openai.com/auth": {
                "chatgpt_account_id": account,
                "chatgpt_subscription_active_start": start,
                "chatgpt_subscription_active_until": end
            }}),
            "account-a",
        );
        assert!(credentials.subscription_period.is_none());
    }
    // 单独的 JWT exp/iat、缺失身份和空日期都不能构成订阅周期。
    for claims in [
        json!({"exp": 1_990_000_000, "iat": 1_790_000_000}),
        json!({"https://api.openai.com/auth": {"chatgpt_subscription_active_until": "2026-11-06T13:04:47Z"}}),
        json!({"https://api.openai.com/auth": {"chatgpt_account_id": "account-a"}}),
    ] {
        assert!(
            credentials_with_claims(claims, "account-a")
                .subscription_period
                .is_none()
        );
    }
}

#[test]
fn subscription_period_successful_api_response_suppresses_credential_fallback() {
    use crate::core::{CredentialSubscriptionPeriod, RateWindow, SubscriptionMetadata};
    use crate::providers::openai::OpenAISubscriptionFetchResult;
    let at = super::credentials::parse_timestamp("2026-11-06T13:04:47Z").unwrap();
    let period = CredentialSubscriptionPeriod {
        starts_at: None,
        ends_at: Some(at),
        recorded_at: None,
    };
    for metadata in [None, Some(SubscriptionMetadata::new(None, None, Some(at)))] {
        // 明确的空订阅结果必须压过旧登录记录，防止已取消订阅被重新填回。
        let stale = subscription::apply_subscription_result(
            UsageSnapshot::new(RateWindow::new(1.0)),
            OpenAISubscriptionFetchResult::Unavailable,
            Some(&period),
        );
        let usage = subscription::apply_subscription_result(
            stale,
            OpenAISubscriptionFetchResult::Success(metadata.clone()),
            Some(&period),
        );
        assert_eq!(usage.subscription, metadata);
        match (usage.subscription_observation, metadata) {
            (Some(observation), Some(metadata)) => {
                assert_eq!(observation.renews_at, metadata.renews_at);
                assert!(observation.period_end.is_none());
                assert!(observation.recorded_at.is_none());
                assert_eq!(
                    observation.source,
                    crate::core::SubscriptionDateSource::ProviderApi
                );
            }
            (None, None) => {}
            _ => panic!("实时结果必须完整替换历史观测，包括明确的空结果"),
        }
    }
}
