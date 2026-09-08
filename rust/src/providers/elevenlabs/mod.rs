//! ElevenLabs provider implementation.
//!
//! Fetches subscription credit usage from ElevenLabs' API.

use async_trait::async_trait;
use chrono::{TimeZone, Utc};
use reqwest::Client;
use serde::Deserialize;

use crate::core::{
    FetchContext, Provider, ProviderError, ProviderFetchResult, ProviderId, ProviderMetadata,
    RateWindow, SourceMode, UsageSnapshot,
};

const ELEVENLABS_SUBSCRIPTION_URL: &str = "https://api.elevenlabs.io/v1/user/subscription";
const ELEVENLABS_CREDENTIAL_TARGET: &str = "codexbar-elevenlabs";
const MAX_AUTH_ERROR_BODY_BYTES: usize = 8 * 1024;

#[derive(Debug, Deserialize)]
struct ElevenLabsSubscriptionResponse {
    tier: Option<String>,
    character_count: u64,
    character_limit: u64,
    voice_slots_used: Option<u64>,
    professional_voice_slots_used: Option<u64>,
    voice_limit: Option<u64>,
    professional_voice_limit: Option<u64>,
    status: Option<String>,
    next_character_count_reset_unix: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct ElevenLabsApiErrorResponse {
    detail: Option<ElevenLabsApiErrorDetail>,
}

#[derive(Debug, Deserialize)]
struct ElevenLabsApiErrorDetail {
    code: Option<String>,
    status: Option<String>,
}

pub struct ElevenLabsProvider {
    metadata: ProviderMetadata,
    client: Client,
}

impl ElevenLabsProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: ProviderId::ElevenLabs,
                display_name: "ElevenLabs",
                session_label: "Credits",
                weekly_label: "Voices",
                supports_opus: false,
                supports_credits: true,
                default_enabled: false,
                is_primary: false,
                dashboard_url: Some("https://elevenlabs.io/app/settings/api-keys"),
                status_page_url: Some("https://status.elevenlabs.io"),
            },
            client: crate::core::credentialed_http_client_builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }

    async fn fetch_api(&self, api_key: &str) -> Result<UsageSnapshot, ProviderError> {
        let response = self
            .client
            .get(ELEVENLABS_SUBSCRIPTION_URL)
            .header("xi-api-key", api_key)
            .header("Accept", "application/json")
            .send()
            .await?;

        let status = response.status();
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            let body = read_bounded_error_body(response).await;
            return Err(auth_error_from_response(status, &body));
        }
        if !status.is_success() {
            return Err(ProviderError::Other(format!(
                "ElevenLabs API returned status {}",
                status
            )));
        }

        let subscription: ElevenLabsSubscriptionResponse = response.json().await.map_err(|e| {
            ProviderError::Parse(format!("Failed to parse ElevenLabs subscription: {e}"))
        })?;
        Ok(snapshot_from_subscription(&subscription))
    }
}

async fn read_bounded_error_body(mut response: reqwest::Response) -> Vec<u8> {
    let mut body = Vec::new();
    while body.len() < MAX_AUTH_ERROR_BODY_BYTES {
        match response.chunk().await {
            Ok(Some(chunk)) => {
                let remaining = MAX_AUTH_ERROR_BODY_BYTES - body.len();
                body.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
            }
            Ok(None) | Err(_) => break,
        }
    }
    body
}

fn auth_error_from_response(status: reqwest::StatusCode, body: &[u8]) -> ProviderError {
    if let Ok(response) = serde_json::from_slice::<ElevenLabsApiErrorResponse>(body)
        && let Some(detail) = response.detail
    {
        if let Some(error) = auth_error_from_value(detail.code.as_deref()) {
            return error;
        }
        if let Some(error) = auth_error_from_value(detail.status.as_deref()) {
            return error;
        }
    }

    if status == reqwest::StatusCode::FORBIDDEN {
        ProviderError::Other(
            "ElevenLabs denied access for the selected API key. Check its endpoint permissions and IP allowlist."
                .into(),
        )
    } else {
        ProviderError::Other(
            "ElevenLabs could not authenticate the selected API key. Check the key and its permissions."
                .into(),
        )
    }
}

fn auth_error_from_value(value: Option<&str>) -> Option<ProviderError> {
    match value?.trim().to_ascii_lowercase().as_str() {
        "invalid_api_key" => Some(ProviderError::Other(
            "ElevenLabs rejected the selected API key. Check that it is valid and has not been revoked."
                .into(),
        )),
        "missing_permissions" | "insufficient_permissions" => Some(ProviderError::Other(
            "ElevenLabs API key is missing the user_read permission required to fetch subscription usage."
                .into(),
        )),
        _ => None,
    }
}

impl Default for ElevenLabsProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for ElevenLabsProvider {
    fn id(&self) -> ProviderId {
        ProviderId::ElevenLabs
    }

    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        match ctx.source_mode {
            SourceMode::Auto | SourceMode::OAuth => {
                let api_key = resolve_api_key(
                    ctx.api_key.as_deref(),
                    ELEVENLABS_CREDENTIAL_TARGET,
                    &["ELEVENLABS_API_KEY"],
                )?;
                Ok(ProviderFetchResult::new(
                    self.fetch_api(&api_key).await?,
                    "api",
                ))
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

fn snapshot_from_subscription(subscription: &ElevenLabsSubscriptionResponse) -> UsageSnapshot {
    let used_percent = if subscription.character_limit > 0 {
        subscription.character_count as f64 / subscription.character_limit as f64 * 100.0
    } else {
        0.0
    };

    let mut primary = RateWindow::new(used_percent);
    primary.reset_description = Some(format!(
        "{} / {} credits",
        format_count(subscription.character_count),
        format_count(subscription.character_limit)
    ));
    primary.resets_at = subscription
        .next_character_count_reset_unix
        .and_then(|timestamp| Utc.timestamp_opt(timestamp, 0).single());

    let mut snapshot = UsageSnapshot::new(primary).with_login_method(display_tier(subscription));

    if let (Some(used), Some(limit)) = (subscription.voice_slots_used, subscription.voice_limit)
        && limit > 0
    {
        snapshot = snapshot.with_extra_rate_window(
            "voice-slots",
            "Voice slots",
            RateWindow::with_details(
                used as f64 / limit as f64 * 100.0,
                None,
                None,
                Some(format!("{used} / {limit}")),
            ),
        );
    }

    if let (Some(used), Some(limit)) = (
        subscription.professional_voice_slots_used,
        subscription.professional_voice_limit,
    ) && limit > 0
    {
        snapshot = snapshot.with_extra_rate_window(
            "professional-voices",
            "Professional voices",
            RateWindow::with_details(
                used as f64 / limit as f64 * 100.0,
                None,
                None,
                Some(format!("{used} / {limit}")),
            ),
        );
    }

    snapshot
}

fn display_tier(subscription: &ElevenLabsSubscriptionResponse) -> String {
    let tier = subscription
        .tier
        .as_deref()
        .map(|tier| tier.replace('_', " "))
        .filter(|tier| !tier.trim().is_empty())
        .unwrap_or_else(|| "Subscription".to_string());
    match subscription.status.as_deref() {
        Some(status) if !status.is_empty() && !status.eq_ignore_ascii_case("active") => {
            format!("{tier} - {status}")
        }
        _ => tier,
    }
}

fn format_count(value: u64) -> String {
    let raw = value.to_string();
    let mut out = String::with_capacity(raw.len() + raw.len() / 3);
    for (idx, ch) in raw.chars().rev().enumerate() {
        if idx > 0 && idx % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out.chars().rev().collect()
}

fn resolve_api_key(
    explicit: Option<&str>,
    credential_target: &str,
    env_names: &[&str],
) -> Result<String, ProviderError> {
    if let Some(key) = explicit
        && !key.trim().is_empty()
    {
        return Ok(key.trim().to_string());
    }
    if let Ok(entry) = keyring::Entry::new(credential_target, "api_key")
        && let Ok(key) = entry.get_password()
        && !key.trim().is_empty()
    {
        return Ok(key);
    }
    for env in env_names {
        if let Ok(key) = std::env::var(env)
            && !key.trim().is_empty()
        {
            return Ok(key);
        }
    }
    Err(ProviderError::NotInstalled(format!(
        "API key not found. Set {} in Preferences or environment.",
        env_names.join(" / ")
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_surfaces_credit_and_voice_usage() {
        let snapshot = snapshot_from_subscription(&ElevenLabsSubscriptionResponse {
            tier: Some("creator".into()),
            character_count: 25_000,
            character_limit: 100_000,
            voice_slots_used: Some(2),
            professional_voice_slots_used: Some(1),
            voice_limit: Some(5),
            professional_voice_limit: Some(2),
            status: Some("active".into()),
            next_character_count_reset_unix: None,
        });

        assert_eq!(snapshot.primary.used_percent, 25.0);
        assert_eq!(
            snapshot.primary.reset_description.as_deref(),
            Some("25,000 / 100,000 credits")
        );
        assert_eq!(snapshot.extra_rate_windows.len(), 2);
        assert_eq!(snapshot.login_method.as_deref(), Some("creator"));
    }

    #[test]
    fn auth_error_checks_code_before_status_and_uses_status_as_fallback() {
        let code_wins = auth_error_from_response(
            reqwest::StatusCode::UNAUTHORIZED,
            br#"{"detail":{"code":" INVALID_API_KEY ","status":"missing_permissions"}}"#,
        );
        assert_eq!(
            code_wins.to_string(),
            "ElevenLabs rejected the selected API key. Check that it is valid and has not been revoked."
        );

        let status_used = auth_error_from_response(
            reqwest::StatusCode::UNAUTHORIZED,
            br#"{"detail":{"code":"unknown","status":" INSUFFICIENT_PERMISSIONS "}}"#,
        );
        assert_eq!(
            status_used.to_string(),
            "ElevenLabs API key is missing the user_read permission required to fetch subscription usage."
        );
    }

    #[test]
    fn auth_error_malformed_or_empty_body_uses_status_fallback() {
        assert_eq!(
            auth_error_from_response(reqwest::StatusCode::UNAUTHORIZED, b"").to_string(),
            "ElevenLabs could not authenticate the selected API key. Check the key and its permissions."
        );
        assert_eq!(
            auth_error_from_response(reqwest::StatusCode::FORBIDDEN, b"not JSON").to_string(),
            "ElevenLabs denied access for the selected API key. Check its endpoint permissions and IP allowlist."
        );
    }

    #[test]
    fn auth_error_messages_do_not_expose_response_body() {
        let marker = b"sensitive-response-marker-api-key";
        for status in [
            reqwest::StatusCode::UNAUTHORIZED,
            reqwest::StatusCode::FORBIDDEN,
        ] {
            let error = auth_error_from_response(
                status,
                br#"{"detail":{"code":"unknown","status":"unknown","message":"sensitive-response-marker-api-key"}}"#,
            );
            assert!(
                !error
                    .to_string()
                    .contains(std::str::from_utf8(marker).unwrap())
            );
        }
    }
}
