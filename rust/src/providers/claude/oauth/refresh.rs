//! OAuth token refresh HTTP call.
//!
//! POSTs `grant_type=refresh_token` to the OAuth token endpoint, mirroring the
//! Claude CLI's own refresh call, and builds the new credentials.

use chrono::Utc;
use reqwest::Client;
use serde::Deserialize;
use std::time::Duration;

use super::ClaudeOAuthCredentials;
use crate::core::ProviderError;

/// OAuth token endpoint + client id used to refresh an expired access token.
/// Mirrors the Claude CLI's own prod `TOKEN_URL` / `CLIENT_ID`.
const TOKEN_REFRESH_URL: &str = "https://platform.claude.com/v1/oauth/token";
const OAUTH_CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
const OAUTH_BETA_HEADER: &str = "oauth-2025-04-20";
/// Fallback access-token lifetime if a refresh response omits `expires_in`.
const DEFAULT_ACCESS_TTL_SECS: i64 = 3600;

/// Response from the OAuth token refresh endpoint (`grant_type=refresh_token`).
#[derive(Debug, Deserialize)]
struct RefreshTokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<i64>,
    #[serde(default)]
    scope: Option<String>,
}

/// POST `grant_type=refresh_token` to the OAuth token endpoint, mirroring the
/// Claude CLI's own refresh call, and build the new credentials.
pub(super) async fn refresh_access_token(
    client: &Client,
    refresh_token: &str,
    current: &ClaudeOAuthCredentials,
) -> Result<ClaudeOAuthCredentials, ProviderError> {
    let mut body = serde_json::json!({
        "grant_type": "refresh_token",
        "refresh_token": refresh_token,
        "client_id": OAUTH_CLIENT_ID,
    });
    if !current.scopes.is_empty() {
        body["scope"] = serde_json::Value::String(current.scopes.join(" "));
    }

    let response = client
        .post(TOKEN_REFRESH_URL)
        .header("anthropic-beta", OAUTH_BETA_HEADER)
        .header("Accept", "application/json")
        .json(&body)
        .timeout(Duration::from_secs(15))
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(ProviderError::OAuth(format!(
            "Token refresh failed ({}): {}",
            status,
            text.chars().take(200).collect::<String>()
        )));
    }

    let refreshed: RefreshTokenResponse = response
        .json()
        .await
        .map_err(|e| ProviderError::Parse(format!("Failed to parse refresh response: {e}")))?;

    let access_token = refreshed.access_token.trim().to_string();
    if access_token.is_empty() {
        return Err(ProviderError::OAuth(
            "Token refresh returned an empty access token".to_string(),
        ));
    }

    // The endpoint returns `expires_in`; if it is ever omitted, fall back to
    // a conservative TTL so the token is still treated as fresh for a bounded
    // window (and cached) instead of triggering a per-poll refresh storm.
    let ttl_secs = refreshed.expires_in.unwrap_or(DEFAULT_ACCESS_TTL_SECS);
    let expires_at = Some(Utc::now() + chrono::Duration::seconds(ttl_secs));

    let refresh_token = refreshed
        .refresh_token
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .or_else(|| current.refresh_token.clone());

    let scopes = refreshed
        .scope
        .map(|s| s.split_whitespace().map(str::to_string).collect::<Vec<_>>())
        .filter(|scopes| !scopes.is_empty())
        .unwrap_or_else(|| current.scopes.clone());

    Ok(ClaudeOAuthCredentials {
        access_token,
        refresh_token,
        expires_at,
        scopes,
        rate_limit_tier: current.rate_limit_tier.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::RefreshTokenResponse;

    #[test]
    fn parses_refresh_token_response() {
        let resp: RefreshTokenResponse = serde_json::from_str(
            r#"{
                "access_token": "new-access",
                "refresh_token": "new-refresh",
                "expires_in": 28800,
                "scope": "user:inference user:profile",
                "token_type": "Bearer"
            }"#,
        )
        .expect("refresh response should parse");

        assert_eq!(resp.access_token, "new-access");
        assert_eq!(resp.refresh_token.as_deref(), Some("new-refresh"));
        assert_eq!(resp.expires_in, Some(28800));
        assert_eq!(resp.scope.as_deref(), Some("user:inference user:profile"));
    }
}
