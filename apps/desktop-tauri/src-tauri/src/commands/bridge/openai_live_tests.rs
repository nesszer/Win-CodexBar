use std::collections::BTreeMap;

use codexbar::providers::openaiapi::org_limits::{OrgRateLimit, OrgRateLimits};
use codexbar::settings::ModelRateLimit;

use super::openai_live::{ModelRateLimitSnapshot, OrgRateLimitsSnapshot};

#[test]
fn model_rate_limit_snapshot_is_camel_case_with_null_rpm() {
    let limit = ModelRateLimit::new("gpt-5", 40_000_000, None);
    let json = serde_json::to_value(ModelRateLimitSnapshot::from(&limit)).unwrap();
    assert_eq!(
        json,
        serde_json::json!({
            "model": "gpt-5",
            "tokensPerMinute": 40_000_000,
            "requestsPerMinute": null,
        })
    );
}

#[test]
fn model_rate_limit_snapshot_accepts_a_missing_rpm_from_the_editor() {
    let snapshot: ModelRateLimitSnapshot =
        serde_json::from_str(r#"{"model":"gpt-5","tokensPerMinute":1}"#).unwrap();
    assert_eq!(snapshot.requests_per_minute, None);
    assert_eq!(snapshot.tokens_per_minute, 1);
}

#[test]
fn org_rate_limits_snapshot_lists_models_sorted_with_camel_case_fields() {
    let mut by_model = BTreeMap::new();
    by_model.insert(
        "gpt-5-mini".to_string(),
        OrgRateLimit {
            tokens_per_minute: 2,
            requests_per_minute: Some(3),
        },
    );
    by_model.insert(
        "gpt-4o".to_string(),
        OrgRateLimit {
            tokens_per_minute: 5,
            requests_per_minute: None,
        },
    );
    let limits = OrgRateLimits {
        by_model,
        projects_sampled: 2,
        truncated: true,
        fetched_at: 1_800,
    };
    let json = serde_json::to_value(OrgRateLimitsSnapshot::from(&limits)).unwrap();
    assert_eq!(
        json,
        serde_json::json!({
            "byModel": [
                { "model": "gpt-4o", "tokensPerMinute": 5, "requestsPerMinute": null },
                { "model": "gpt-5-mini", "tokensPerMinute": 2, "requestsPerMinute": 3 },
            ],
            "projectsSampled": 2,
            "truncated": true,
            "fetchedAt": 1_800,
        })
    );
}
