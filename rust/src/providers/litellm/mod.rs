use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::{Client, Response, StatusCode, Url};
use serde::de::DeserializeOwned;

use crate::core::{
    FetchContext, Provider, ProviderError, ProviderFetchResult, ProviderId, SourceMode,
};
use crate::providers::{BoundedBodyError, read_bounded_response};

mod endpoint;
mod info;
mod model_activity;
mod spend_report;
#[cfg(test)]
mod tests;

use endpoint::management_url;
pub(crate) use endpoint::validated_base_url;
use info::{
    KeyInfoResponse, MISSING_KEY_IDS, TeamInfoResponse, UserInfoResponse, bind_key, parse_error,
    result_from_team, result_from_user,
};

const CREDENTIAL_TARGET: &str = "codexbar-litellm";
const MAX_RESPONSE_BYTES: usize = 1024 * 1024;

pub struct LiteLLMProvider {
    client: Client,
}

impl LiteLLMProvider {
    pub fn new() -> Self {
        Self {
            client: crate::core::credentialed_http_client_builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }
}

impl Default for LiteLLMProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for LiteLLMProvider {
    fn id(&self) -> ProviderId {
        ProviderId::LiteLLM
    }

    /// Upstream's LiteLLM menu bar resolver: the team budget is enforced for
    /// the key, so Automatic shows it unless a budget is already exhausted.
    fn automatic_metric_prefers_secondary_window(&self) -> bool {
        true
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        match ctx.source_mode {
            SourceMode::Auto | SourceMode::OAuth => {
                let (base, key) = resolve_base_and_key(ctx)?;
                fetch_key_usage(
                    &self.client,
                    |path, query| management_url(&base, path, query),
                    &key,
                    Utc::now(),
                    ctx.optional_details_enabled,
                )
                .await
            }
            SourceMode::Web | SourceMode::Cli => {
                Err(ProviderError::UnsupportedSource(ctx.source_mode))
            }
        }
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::OAuth]
    }
}

/// Upstream `litellm.ts`: `/key/info` names the key's user or team, whose
/// budgets are then read. When the management routes are unavailable to the
/// key (401, 403 or 404), the UTC month-to-date spend report is shown instead.
/// `url_for(path, query)` builds a management-route URL, so tests can route
/// individual requests; `now` fixes the report period and the model-activity
/// window, which is requested only for a user-bound key when
/// `include_model_activity` (the opt-in) is set (upstream 0.67.0).
async fn fetch_key_usage<F>(
    client: &Client,
    url_for: F,
    key: &str,
    now: DateTime<Utc>,
    include_model_activity: bool,
) -> Result<ProviderFetchResult, ProviderError>
where
    F: Fn(&str, Option<(&str, &str)>) -> Result<Url, ProviderError>,
{
    let url = url_for("key/info", None)?;
    let route = url.path().to_string();
    let response = send(client, url, key).await?;
    if route_unavailable(response.status()) {
        return spend_report::fetch(client, &url_for, key, now).await;
    }
    let key_info: KeyInfoResponse = read_json(&route, response).await?;
    let binding = bind_key(key_info)?;
    if let Some(user_id) = binding.user_id.as_deref() {
        let url = url_for("user/info", Some(("user_id", user_id)))?;
        let response: UserInfoResponse = get_json(client, url, key).await?;
        let result = result_from_user(&binding, user_id, response)?;
        if !include_model_activity {
            return Ok(result);
        }
        // Upstream 0.67.0: optional history must never fail the budget fetch.
        let rows = match url_for("user/daily/activity", None) {
            Ok(endpoint) => {
                model_activity::fetch(client, &endpoint, key, user_id, now.date_naive()).await
            }
            Err(_) => Vec::new(),
        };
        Ok(result.with_display_details(rows))
    } else if let Some(team_id) = binding.team_id.as_deref() {
        let url = url_for("team/info", Some(("team_id", team_id)))?;
        let response: TeamInfoResponse = get_json(client, url, key).await?;
        result_from_team(&binding, team_id, response)
    } else {
        Err(parse_error(MISSING_KEY_IDS))
    }
}

/// `/key/info` (or key report) statuses that mean the route is not available
/// to this key, so the next, more narrowly scoped source is tried.
fn route_unavailable(status: StatusCode) -> bool {
    matches!(
        status,
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN | StatusCode::NOT_FOUND
    )
}

async fn send(client: &Client, url: Url, key: &str) -> Result<Response, ProviderError> {
    Ok(client
        .get(url)
        .bearer_auth(key)
        .header("Accept", "application/json")
        .send()
        .await?)
}

/// Map a failed status to its error class. Only the route and the status are
/// named; response bodies are never echoed.
fn check_status(route: &str, status: StatusCode) -> Result<(), ProviderError> {
    if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
        return Err(ProviderError::AuthRequired);
    }
    if status == StatusCode::TOO_MANY_REQUESTS {
        return Err(ProviderError::Other(
            "LiteLLM rate limited the request (HTTP 429).".into(),
        ));
    }
    if !status.is_success() {
        return Err(ProviderError::Other(format!(
            "LiteLLM {route} returned status {status}"
        )));
    }
    Ok(())
}

async fn get_json<T: DeserializeOwned>(
    client: &Client,
    url: Url,
    key: &str,
) -> Result<T, ProviderError> {
    let route = url.path().to_string();
    let response = send(client, url, key).await?;
    read_json(&route, response).await
}

async fn read_json<T: DeserializeOwned>(
    route: &str,
    response: Response,
) -> Result<T, ProviderError> {
    check_status(route, response.status())?;
    let body = read_bounded_response(response, MAX_RESPONSE_BYTES)
        .await
        .map_err(|error| match error {
            BoundedBodyError::TooLarge => parse_error("response too large"),
            BoundedBodyError::Read(error) => ProviderError::Network(error),
        })?;
    serde_json::from_slice(&body).map_err(|e| parse_error(format!("{route}: {e}")))
}

fn resolve_base_and_key(ctx: &FetchContext) -> Result<(String, String), ProviderError> {
    if let Some(base) = ctx
        .workspace_id
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        let key = ctx.api_key.as_deref().ok_or(ProviderError::AuthRequired)?;
        return Ok((base.to_string(), key.to_string()));
    }

    let key = crate::providers::resolve_api_key(
        ctx.api_key.as_deref(),
        CREDENTIAL_TARGET,
        &["LITELLM_API_KEY"],
    )?;
    let base = std::env::var("LITELLM_BASE_URL")
        .ok()
        .or_else(|| std::env::var("LITELLM_API_BASE").ok())
        .ok_or_else(|| {
            ProviderError::NotInstalled(
                "LiteLLM base URL not found. Set it in provider extras or LITELLM_BASE_URL.".into(),
            )
        })?;
    Ok((base, key))
}
