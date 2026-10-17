use super::*;
use codexbar::providers::openaiapi::live::LiveUsageError;
use codexbar::providers::openaiapi::org_limits::OrgRateLimitsError;

use codexbar::settings::ModelRateLimit;

use super::bridge::ModelRateLimitSnapshot;
use super::openai_live::{
    live_usage_error_message, openai_project_scope, org_rate_limits_error_message,
    validated_model_rate_limits,
};

fn row(model: &str, tpm: u64, rpm: Option<u64>) -> ModelRateLimitSnapshot {
    ModelRateLimitSnapshot {
        model: model.to_string(),
        tokens_per_minute: tpm,
        requests_per_minute: rpm,
    }
}

#[test]
fn live_usage_errors_map_to_friendly_text() {
    assert!(live_usage_error_message(&LiveUsageError::MissingCredential).contains("Admin API key"));
    assert!(live_usage_error_message(&LiveUsageError::AdminKeyRequired).contains("sk-admin"));
    assert!(live_usage_error_message(&LiveUsageError::RateLimited).contains("rate limited"));
    assert!(
        live_usage_error_message(&LiveUsageError::ApiFailure(503)).contains("HTTP 503"),
        "API failures keep the status code but no body"
    );
    assert!(live_usage_error_message(&LiveUsageError::ParseFailure).contains("response"));
    assert!(live_usage_error_message(&LiveUsageError::InvalidWindow).contains("15-minute"));
    assert!(live_usage_error_message(&LiveUsageError::Network).contains("reach"));
}

#[test]
fn org_rate_limit_errors_map_to_friendly_text() {
    assert!(
        org_rate_limits_error_message(&OrgRateLimitsError::MissingCredential)
            .contains("Admin API key")
    );
    assert!(
        org_rate_limits_error_message(&OrgRateLimitsError::AdminKeyRequired)
            .contains("project read")
    );
    assert!(
        org_rate_limits_error_message(&OrgRateLimitsError::RateLimited).contains("rate limited")
    );
    assert!(
        org_rate_limits_error_message(&OrgRateLimitsError::ApiFailure(500)).contains("HTTP 500")
    );
    assert!(org_rate_limits_error_message(&OrgRateLimitsError::ParseFailure).contains("response"));
    assert!(org_rate_limits_error_message(&OrgRateLimitsError::Network).contains("reach"));
}

#[test]
fn validated_rows_trim_drop_zero_rpm_and_keep_order() {
    let limits = validated_model_rate_limits(&[
        row("  gpt-5 ", 40_000_000, Some(0)),
        row("gpt-4o", 2_000_000, Some(5_000)),
    ])
    .unwrap();
    assert_eq!(
        limits,
        vec![
            ModelRateLimit::new("gpt-5", 40_000_000, None),
            ModelRateLimit::new("gpt-4o", 2_000_000, Some(5_000)),
        ]
    );
}

#[test]
fn validated_rows_reject_bad_input_instead_of_dropping_it() {
    let empty = validated_model_rate_limits(&[row("  ", 1, None)]).unwrap_err();
    assert!(empty.contains("model ID"));

    let zero = validated_model_rate_limits(&[row("gpt-5", 0, None)]).unwrap_err();
    assert!(zero.contains("gpt-5"));

    let too_big =
        validated_model_rate_limits(&[row("gpt-5", 9_007_199_254_740_992, None)]).unwrap_err();
    assert!(too_big.contains("Tokens per minute"));

    let rpm_too_big =
        validated_model_rate_limits(&[row("gpt-5", 1, Some(9_007_199_254_740_992))]).unwrap_err();
    assert!(rpm_too_big.contains("Requests per minute"));

    let duplicate =
        validated_model_rate_limits(&[row("gpt-5", 1, None), row(" gpt-5", 2, None)]).unwrap_err();
    assert!(duplicate.contains("more than once"));

    let multiline = validated_model_rate_limits(&[row("gpt-\n5", 1, None)]).unwrap_err();
    assert!(multiline.contains("one line"));

    let at_limit = "é".repeat(200);
    assert!(validated_model_rate_limits(&[row(&at_limit, 1, None)]).is_ok());
    let over_limit = "é".repeat(201);
    assert!(validated_model_rate_limits(&[row(&over_limit, 1, None)]).is_err());
}

#[test]
fn saving_limits_and_the_live_switch_round_trips_through_settings() {
    let mut settings = Settings::default();
    SettingsUpdate {
        openai_live_usage_enabled: Some(false),
        openai_model_rate_limits: Some(vec![row(" gpt-5 ", 40_000_000, Some(0))]),
        ..Default::default()
    }
    .apply_openai_live_settings(&mut settings)
    .unwrap();

    assert!(!settings.live_usage_enabled(ProviderId::OpenAIApi));
    assert_eq!(
        settings.model_rate_limits(ProviderId::OpenAIApi).to_vec(),
        vec![ModelRateLimit::new("gpt-5", 40_000_000, None)]
    );

    // A rejected editor row leaves the stored list as it was.
    let rejected = SettingsUpdate {
        openai_model_rate_limits: Some(vec![row("", 1, None)]),
        ..Default::default()
    }
    .apply_openai_live_settings(&mut settings);
    assert!(rejected.is_err());
    assert_eq!(settings.model_rate_limits(ProviderId::OpenAIApi).len(), 1);

    // An empty list clears the limits.
    SettingsUpdate {
        openai_model_rate_limits: Some(Vec::new()),
        ..Default::default()
    }
    .apply_openai_live_settings(&mut settings)
    .unwrap();
    assert!(settings.model_rate_limits(ProviderId::OpenAIApi).is_empty());
}

#[test]
fn live_usage_fields_use_the_frontend_wire_names() {
    let update: SettingsUpdate = serde_json::from_value(serde_json::json!({
        "openAiLiveUsageEnabled": false,
        "openAiModelRateLimits": [{ "model": "gpt-5", "tokensPerMinute": 1000 }],
    }))
    .unwrap();
    assert_eq!(update.openai_live_usage_enabled, Some(false));
    assert_eq!(
        update.openai_model_rate_limits.as_ref().map(Vec::len),
        Some(1)
    );

    let snapshot = serde_json::to_value(SettingsSnapshot::from(Settings::default())).unwrap();
    assert_eq!(snapshot["openAiLiveUsageEnabled"], serde_json::json!(true));
    assert!(snapshot["openAiModelRateLimits"].is_array());
}

#[test]
fn live_usage_defaults_on_and_is_not_offered_for_other_providers() {
    let settings = Settings::default();
    assert!(settings.live_usage_enabled(ProviderId::OpenAIApi));
    assert!(!settings.live_usage_enabled(ProviderId::Codex));
}

#[test]
fn project_scope_is_trimmed_and_empty_means_organization() {
    let mut settings = Settings::default();
    assert_eq!(openai_project_scope(&settings), None);
    settings.set_workspace_id(ProviderId::OpenAIApi, "   ");
    assert_eq!(openai_project_scope(&settings), None);
    settings.set_workspace_id(ProviderId::OpenAIApi, " proj_123 ");
    assert_eq!(openai_project_scope(&settings).as_deref(), Some("proj_123"));
}
