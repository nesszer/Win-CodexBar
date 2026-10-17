//! OpenAI Live Usage commands (upstream 0.75.0 #4495): the on-demand Tokens/min window
//! and the organization project caps, plus the validation for manual per-model limits.
//!
//! Both fetches resolve the stored Admin key and the saved project scope the same way the
//! refresh path does. Errors are mapped to friendly text without the key or raw bodies.

use std::collections::HashSet;

use codexbar::providers::OpenAIApiProvider;
use codexbar::providers::openaiapi::live::LiveUsageError;
use codexbar::providers::openaiapi::org_limits::OrgRateLimitsError;
use codexbar::settings::ModelRateLimit;

use super::bridge::{ModelRateLimitSnapshot, OrgRateLimitsSnapshot, ProviderThroughputSnapshot};
use super::*;

/// Largest integer a JavaScript number holds exactly; the editor cannot send more.
const MAX_RATE_LIMIT: u64 = 9_007_199_254_740_991;
const MAX_MODEL_ID_LEN: usize = 200;

/// Friendly, key-free text for a failed Live Usage window.
pub(crate) fn live_usage_error_message(error: &LiveUsageError) -> String {
    match error {
        LiveUsageError::MissingCredential => {
            "Add an OpenAI organization Admin API key in Settings > Providers > OpenAI to see live usage."
                .to_string()
        }
        LiveUsageError::AdminKeyRequired => {
            "Live usage needs an OpenAI organization Admin API key (sk-admin-...). Project and service-account keys cannot read organization usage."
                .to_string()
        }
        LiveUsageError::RateLimited => {
            "OpenAI live usage is rate limited. Try again in a minute.".to_string()
        }
        LiveUsageError::ApiFailure(code) => {
            format!("OpenAI live usage API failed (HTTP {code}).")
        }
        LiveUsageError::ParseFailure => {
            "Could not read the OpenAI live usage response.".to_string()
        }
        LiveUsageError::InvalidWindow => {
            "OpenAI live usage supports 15-minute, 1-hour, and 6-hour windows.".to_string()
        }
        LiveUsageError::Network => "Could not reach the OpenAI live usage API.".to_string(),
    }
}

/// Friendly, key-free text for a failed organization limits read.
pub(crate) fn org_rate_limits_error_message(error: &OrgRateLimitsError) -> String {
    match error {
        OrgRateLimitsError::MissingCredential => {
            "Add an OpenAI organization Admin API key to read project limits.".to_string()
        }
        OrgRateLimitsError::AdminKeyRequired => {
            "Project limits need an OpenAI Admin API key with project read permissions.".to_string()
        }
        OrgRateLimitsError::RateLimited => {
            "OpenAI project limits are temporarily rate limited. Try again in a minute.".to_string()
        }
        OrgRateLimitsError::ApiFailure(code) => {
            format!("OpenAI project limits API failed (HTTP {code}).")
        }
        OrgRateLimitsError::ParseFailure => {
            "Could not read the OpenAI project limits response.".to_string()
        }
        OrgRateLimitsError::Network => "Could not reach the OpenAI project limits API.".to_string(),
    }
}

/// Validates the editor's rows before they replace the stored list. Unlike the settings
/// loader, which silently drops malformed rows, a bad editor row is reported to the user.
/// A zero requests-per-minute means "no RPM limit", as upstream.
pub(crate) fn validated_model_rate_limits(
    rows: &[ModelRateLimitSnapshot],
) -> Result<Vec<ModelRateLimit>, String> {
    let mut seen = HashSet::new();
    let mut limits = Vec::with_capacity(rows.len());
    for row in rows {
        let model = row.model.trim();
        if model.is_empty() {
            return Err("Each rate limit needs a model ID.".to_string());
        }
        if model.chars().count() > MAX_MODEL_ID_LEN || model.chars().any(char::is_control) {
            return Err(format!(
                "Model IDs must be on one line and at most {MAX_MODEL_ID_LEN} characters."
            ));
        }
        if !seen.insert(model) {
            return Err(format!("Model {model} is listed more than once."));
        }
        if row.tokens_per_minute == 0 || row.tokens_per_minute > MAX_RATE_LIMIT {
            return Err(format!(
                "Tokens per minute for {model} must be between 1 and {MAX_RATE_LIMIT}."
            ));
        }
        if row
            .requests_per_minute
            .is_some_and(|rpm| rpm > MAX_RATE_LIMIT)
        {
            return Err(format!(
                "Requests per minute for {model} must be at most {MAX_RATE_LIMIT}."
            ));
        }
        limits.push(ModelRateLimit::new(
            model,
            row.tokens_per_minute,
            row.requests_per_minute.filter(|rpm| *rpm > 0),
        ));
    }
    Ok(ModelRateLimit::validated(limits))
}

/// The saved OpenAI project scope, or `None` for the whole organization.
pub(super) fn openai_project_scope(settings: &Settings) -> Option<String> {
    let id = settings.workspace_id(ProviderId::OpenAIApi).trim();
    (!id.is_empty()).then(|| id.to_string())
}

/// The stored Admin/API key, read the way the refresh path reads it. Never logged.
fn stored_openai_api_key() -> Option<String> {
    ApiKeys::load()
        .get(ProviderId::OpenAIApi.cli_name())
        .map(str::to_string)
}

/// Last-hour-or-longer Tokens/min window for the OpenAI API card's Live Usage view.
/// `window_minutes` must be 15, 60 or 360.
#[tauri::command]
pub async fn get_openai_live_usage(
    window_minutes: u32,
) -> Result<ProviderThroughputSnapshot, String> {
    let project_id = openai_project_scope(&Settings::load());
    let key = stored_openai_api_key();
    let usage = OpenAIApiProvider::new()
        .fetch_live_usage(key.as_deref(), project_id.as_deref(), window_minutes)
        .await
        .map_err(|error| live_usage_error_message(&error))?;
    Ok(ProviderThroughputSnapshot::from(&usage.throughput()))
}

/// Project caps across the organization, shown as context in the Live Usage view. The
/// caller skips this when a project scope is saved.
#[tauri::command]
pub async fn get_openai_org_rate_limits() -> Result<OrgRateLimitsSnapshot, String> {
    let key = stored_openai_api_key();
    let limits = OpenAIApiProvider::new()
        .fetch_org_rate_limits(key.as_deref())
        .await
        .map_err(|error| org_rate_limits_error_message(&error))?;
    Ok(OrgRateLimitsSnapshot::from(&limits))
}
