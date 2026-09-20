//! Warp provider implementation
//!
//! Fetches usage data from Warp's GraphQL API
//! Requires API key for authentication

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::json;

use crate::core::{
    FetchContext, Provider, ProviderError, ProviderFetchResult, ProviderId, ProviderMetadata,
    RateWindow, SourceMode, UsageSnapshot,
};

/// Warp GraphQL API endpoint
const WARP_API_URL: &str = "https://app.warp.dev/graphql/v2?op=GetRequestLimitInfo";

/// Windows Credential Manager target for Warp API token
const WARP_CREDENTIAL_TARGET: &str = "codexbar-warp";

/// GraphQL query for fetching request limit info
const GRAPHQL_QUERY: &str = r#"query GetRequestLimitInfo($requestContext: RequestContext!) {
  user(requestContext: $requestContext) {
    __typename
    ... on UserOutput {
      user {
        requestLimitInfo {
          isUnlimited
          nextRefreshTime
          requestLimit
          requestsUsedSinceLastRefresh
        }
        bonusGrants {
          requestCreditsGranted
          requestCreditsRemaining
          expiration
        }
        workspaces {
          bonusGrantsInfo {
            grants {
              requestCreditsGranted
              requestCreditsRemaining
              expiration
            }
          }
        }
      }
    }
  }
}"#;

/// Warp GraphQL response structures
#[derive(Debug, Deserialize)]
struct GraphQLResponse {
    data: Option<GraphQLData>,
    errors: Option<Vec<GraphQLError>>,
}

#[derive(Debug, Deserialize)]
struct GraphQLError {
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GraphQLData {
    user: Option<UserWrapper>,
}

#[derive(Debug, Deserialize)]
struct UserWrapper {
    #[serde(rename = "__typename")]
    type_name: Option<String>,
    user: Option<UserData>,
}

#[derive(Debug, Deserialize)]
struct UserData {
    #[serde(rename = "requestLimitInfo")]
    request_limit_info: Option<RequestLimitInfo>,
    #[serde(rename = "bonusGrants")]
    bonus_grants: Option<Vec<BonusGrant>>,
    workspaces: Option<Vec<Workspace>>,
}

#[derive(Debug, Deserialize)]
struct RequestLimitInfo {
    #[serde(rename = "isUnlimited")]
    is_unlimited: Option<bool>,
    #[serde(rename = "nextRefreshTime")]
    next_refresh_time: Option<String>,
    #[serde(rename = "requestLimit")]
    request_limit: Option<i64>,
    #[serde(rename = "requestsUsedSinceLastRefresh")]
    requests_used: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct BonusGrant {
    #[serde(rename = "requestCreditsGranted")]
    request_credits_granted: Option<i64>,
    #[serde(rename = "requestCreditsRemaining")]
    request_credits_remaining: Option<i64>,
    expiration: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Workspace {
    #[serde(rename = "bonusGrantsInfo")]
    bonus_grants_info: Option<BonusGrantsInfo>,
}

#[derive(Debug, Deserialize)]
struct BonusGrantsInfo {
    grants: Option<Vec<BonusGrant>>,
}

/// Warp provider
pub struct WarpProvider {
    metadata: ProviderMetadata,
}

impl WarpProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: ProviderId::Warp,
                display_name: "Warp",
                session_label: "Credits",
                weekly_label: "Add-on credits",
                supports_opus: false,
                supports_credits: false,
                default_enabled: false,
                is_primary: false,
                dashboard_url: Some("https://docs.warp.dev/reference/cli/api-keys"),
                status_page_url: None,
                tertiary_label_key: None,
            },
        }
    }

    /// Get API token from ctx, Windows Credential Manager, or env
    fn get_api_token(api_key: Option<&str>) -> Result<String, ProviderError> {
        if let Some(key) = api_key
            && !key.is_empty()
        {
            return Ok(key.to_string());
        }

        match keyring::Entry::new(WARP_CREDENTIAL_TARGET, "api_token") {
            Ok(entry) => match entry.get_password() {
                Ok(token) => Ok(token),
                Err(_) => std::env::var("WARP_API_KEY").map_err(|_| {
                    ProviderError::NotInstalled(
                        "Warp API key not found. Set in Preferences → Providers or WARP_API_KEY environment variable.".to_string(),
                    )
                }),
            },
            Err(_) => std::env::var("WARP_API_KEY").map_err(|_| {
                ProviderError::NotInstalled(
                    "Warp API key not found. Set in Preferences → Providers or WARP_API_KEY environment variable.".to_string(),
                )
            }),
        }
    }

    /// Fetch usage from Warp GraphQL API
    async fn fetch_usage_api(&self, ctx: &FetchContext) -> Result<UsageSnapshot, ProviderError> {
        let api_key = Self::get_api_token(ctx.api_key.as_deref())?;
        let gql_response = Self::request_limit_info(&api_key).await?;
        let user = Self::extract_user_data(gql_response)?;

        Self::build_usage_snapshot(user)
    }

    async fn request_limit_info(api_key: &str) -> Result<GraphQLResponse, ProviderError> {
        let client = crate::core::credentialed_http_client_builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .map_err(|e| ProviderError::Other(e.to_string()))?;
        let response = Self::send_graphql_request(&client, api_key).await?;

        if !response.status().is_success() {
            return Err(ProviderError::Other(format!(
                "Warp API returned status {}",
                response.status()
            )));
        }

        response
            .json()
            .await
            .map_err(|e| ProviderError::Parse(e.to_string()))
    }

    async fn send_graphql_request(
        client: &reqwest::Client,
        api_key: &str,
    ) -> Result<reqwest::Response, ProviderError> {
        let os_version = "10.0";
        let body = Self::request_body(os_version);

        Ok(client
            .post(WARP_API_URL)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .header("x-warp-client-id", "warp-app")
            .header("x-warp-os-category", "Windows")
            .header("x-warp-os-name", "Windows")
            .header("x-warp-os-version", os_version)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("User-Agent", "Warp/1.0")
            .json(&body)
            .send()
            .await?)
    }

    fn request_body(os_version: &str) -> serde_json::Value {
        json!({
            "query": GRAPHQL_QUERY,
            "variables": {
                "requestContext": {
                    "clientContext": {},
                    "osContext": {
                        "category": "Windows",
                        "name": "Windows",
                        "version": os_version
                    }
                }
            },
            "operationName": "GetRequestLimitInfo"
        })
    }

    fn extract_user_data(gql_response: GraphQLResponse) -> Result<UserData, ProviderError> {
        if let Some(errors) = &gql_response.errors
            && !errors.is_empty()
        {
            return Err(ProviderError::Other(Self::graphql_error_summary(errors)));
        }

        let data = gql_response
            .data
            .ok_or_else(|| ProviderError::Parse("Missing data in response".to_string()))?;
        let user_wrapper = data
            .user
            .ok_or_else(|| ProviderError::Parse("Missing data.user in response".to_string()))?;
        user_wrapper
            .user
            .ok_or_else(|| ProviderError::Parse("Missing user data in response".to_string()))
    }

    fn graphql_error_summary(errors: &[GraphQLError]) -> String {
        let messages: Vec<String> = errors.iter().filter_map(|e| e.message.clone()).collect();
        if messages.is_empty() {
            "GraphQL request failed".to_string()
        } else {
            messages.join(" | ")
        }
    }

    fn build_usage_snapshot(user: UserData) -> Result<UsageSnapshot, ProviderError> {
        let UserData {
            request_limit_info,
            bonus_grants,
            workspaces,
        } = user;

        let limit_info = request_limit_info.ok_or_else(|| {
            ProviderError::Parse("Missing requestLimitInfo in response".to_string())
        })?;
        let primary = Self::primary_window(&limit_info);

        let mut usage = UsageSnapshot::new(primary).with_login_method("Warp API");
        if let Some(secondary) = Self::bonus_window(Self::all_bonus_grants(
            bonus_grants.as_ref(),
            workspaces.as_ref(),
        )) {
            usage = usage.with_secondary(secondary);
        }

        Ok(usage)
    }

    fn primary_window(limit_info: &RequestLimitInfo) -> RateWindow {
        let is_unlimited = limit_info.is_unlimited.unwrap_or(false);
        let request_limit = limit_info.request_limit.unwrap_or(0);
        let requests_used = limit_info.requests_used.unwrap_or(0);
        let used_percent = Self::primary_used_percent(is_unlimited, requests_used, request_limit);
        let reset_description = if is_unlimited {
            "Unlimited".to_string()
        } else {
            format!("{requests_used}/{request_limit} credits")
        };

        let resets_at = (!is_unlimited)
            .then(|| Self::parse_timestamp(limit_info.next_refresh_time.as_deref()))
            .flatten();
        RateWindow::with_details(used_percent, None, resets_at, Some(reset_description))
    }

    fn primary_used_percent(is_unlimited: bool, requests_used: i64, request_limit: i64) -> f64 {
        if is_unlimited || request_limit <= 0 {
            return 0.0;
        }

        ((requests_used as f64) / (request_limit as f64) * 100.0).clamp(0.0, 100.0)
    }

    fn all_bonus_grants<'a>(
        bonus_grants: Option<&'a Vec<BonusGrant>>,
        workspaces: Option<&'a Vec<Workspace>>,
    ) -> Vec<&'a BonusGrant> {
        let mut all_grants = Vec::new();
        if let Some(grants) = bonus_grants {
            all_grants.extend(grants.iter());
        }

        if let Some(workspaces) = workspaces {
            all_grants.extend(workspaces.iter().flat_map(|workspace| {
                workspace
                    .bonus_grants_info
                    .as_ref()
                    .and_then(|info| info.grants.as_ref())
                    .into_iter()
                    .flatten()
            }));
        }

        all_grants
    }

    fn bonus_window(all_grants: Vec<&BonusGrant>) -> Option<RateWindow> {
        let mut bonus_remaining = 0_i64;
        let mut bonus_total = 0_i64;
        let mut next_expiration: Option<(DateTime<Utc>, i64)> = None;

        for grant in all_grants {
            let remaining = grant.request_credits_remaining.unwrap_or(0);
            bonus_remaining = bonus_remaining.saturating_add(remaining);
            bonus_total = bonus_total.saturating_add(grant.request_credits_granted.unwrap_or(0));

            if remaining > 0
                && let Some(expiration) = Self::parse_timestamp(grant.expiration.as_deref())
            {
                match next_expiration.as_mut() {
                    Some((current, current_remaining)) if expiration < *current => {
                        *current = expiration;
                        *current_remaining = remaining;
                    }
                    Some((current, current_remaining)) if expiration == *current => {
                        *current_remaining = current_remaining.saturating_add(remaining);
                    }
                    None => next_expiration = Some((expiration, remaining)),
                    _ => {}
                }
            }
        }

        if bonus_total <= 0 && bonus_remaining <= 0 {
            return None;
        }

        let reset_description = next_expiration.map(|(expiration, remaining)| {
            format!(
                "{} credits expire on {}",
                remaining,
                expiration.format("%Y-%m-%d %H:%M UTC")
            )
        });
        Some(RateWindow::with_details(
            Self::bonus_used_percent(bonus_total, bonus_remaining),
            None,
            None,
            reset_description,
        ))
    }

    fn bonus_used_percent(bonus_total: i64, bonus_remaining: i64) -> f64 {
        if bonus_total > 0 {
            let bonus_used = bonus_total - bonus_remaining;
            ((bonus_used as f64) / (bonus_total as f64) * 100.0).clamp(0.0, 100.0)
        } else if bonus_remaining > 0 {
            0.0
        } else {
            100.0
        }
    }

    fn parse_timestamp(value: Option<&str>) -> Option<DateTime<Utc>> {
        value
            .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
            .map(|value| value.with_timezone(&Utc))
    }
}

impl Default for WarpProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for WarpProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Warp
    }

    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        tracing::debug!("Fetching Warp usage");

        match ctx.source_mode {
            SourceMode::Auto | SourceMode::OAuth => {
                let usage = self.fetch_usage_api(ctx).await?;
                Ok(ProviderFetchResult::new(usage, "api"))
            }
            SourceMode::Web | SourceMode::Cli => {
                Err(ProviderError::UnsupportedSource(ctx.source_mode))
            }
        }
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::OAuth]
    }

    fn supports_web(&self) -> bool {
        false
    }

    fn supports_cli(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primary_usage_is_zero_for_unlimited_accounts() {
        assert_eq!(WarpProvider::primary_used_percent(true, 75, 100), 0.0);
    }

    #[test]
    fn primary_usage_clamps_to_percent_range() {
        assert_eq!(WarpProvider::primary_used_percent(false, 25, 100), 25.0);
        assert_eq!(WarpProvider::primary_used_percent(false, 150, 100), 100.0);
        assert_eq!(WarpProvider::primary_used_percent(false, 25, 0), 0.0);
    }

    #[test]
    fn bonus_usage_handles_total_and_remaining_only_cases() {
        assert_eq!(WarpProvider::bonus_used_percent(100, 25), 75.0);
        assert_eq!(WarpProvider::bonus_used_percent(100, -25), 100.0);
        assert_eq!(WarpProvider::bonus_used_percent(0, 25), 0.0);
        assert_eq!(WarpProvider::bonus_used_percent(0, 0), 100.0);
    }

    #[test]
    fn all_bonus_grants_includes_user_and_workspace_grants() {
        let user_grants = vec![BonusGrant {
            request_credits_granted: Some(100),
            request_credits_remaining: Some(50),
            expiration: None,
        }];
        let workspaces = vec![Workspace {
            bonus_grants_info: Some(BonusGrantsInfo {
                grants: Some(vec![BonusGrant {
                    request_credits_granted: Some(25),
                    request_credits_remaining: Some(5),
                    expiration: None,
                }]),
            }),
        }];

        let grants = WarpProvider::all_bonus_grants(Some(&user_grants), Some(&workspaces));
        assert_eq!(grants.len(), 2);
    }

    #[test]
    fn primary_window_preserves_refresh_timestamp() {
        let window = WarpProvider::primary_window(&RequestLimitInfo {
            is_unlimited: Some(false),
            next_refresh_time: Some("2026-09-20T12:34:56Z".to_string()),
            request_limit: Some(100),
            requests_used: Some(25),
        });

        assert_eq!(window.used_percent, 25.0);
        assert_eq!(
            window.resets_at.map(|value| value.to_rfc3339()),
            Some("2026-09-20T12:34:56+00:00".to_string())
        );
        assert_eq!(window.reset_description.as_deref(), Some("25/100 credits"));
    }

    #[test]
    fn bonus_window_describes_earliest_expiring_remaining_grant() {
        let later = BonusGrant {
            request_credits_granted: Some(100),
            request_credits_remaining: Some(80),
            expiration: Some("2026-10-01T00:00:00Z".to_string()),
        };
        let earlier = BonusGrant {
            request_credits_granted: Some(50),
            request_credits_remaining: Some(10),
            expiration: Some("2026-09-22T00:00:00Z".to_string()),
        };
        let same_earlier = BonusGrant {
            request_credits_granted: Some(50),
            request_credits_remaining: Some(10),
            expiration: Some("2026-09-22T00:00:00Z".to_string()),
        };

        let window = WarpProvider::bonus_window(vec![&later, &earlier, &same_earlier])
            .expect("bonus window");
        assert_eq!(window.used_percent, 50.0);
        assert_eq!(
            window.reset_description.as_deref(),
            Some("20 credits expire on 2026-09-22 00:00 UTC")
        );
    }
}
