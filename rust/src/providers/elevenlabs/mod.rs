//! ElevenLabs provider implementation.
//!
//! Fetches subscription credit usage from ElevenLabs' API.

use async_trait::async_trait;
use chrono::{TimeZone, Utc};
use reqwest::{Client, Url};
use serde::Deserialize;

use crate::core::{
    FetchContext, Provider, ProviderError, ProviderFetchResult, ProviderId, RateWindow, SourceMode,
    UsageSnapshot,
};

const ELEVENLABS_API_BASE_URL: &str = "https://api.elevenlabs.io";
const ELEVENLABS_API_URL_ENV: &str = "ELEVENLABS_API_URL";
const INVALID_ENDPOINT_OVERRIDE: &str =
    "ElevenLabs endpoint override ELEVENLABS_API_URL must use HTTPS or a bare host.";
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
    client: Client,
}

impl ElevenLabsProvider {
    pub fn new() -> Self {
        Self {
            client: crate::core::credentialed_http_client_builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }

    async fn fetch_api(&self, api_key: &str) -> Result<UsageSnapshot, ProviderError> {
        let endpoint_override = std::env::var(ELEVENLABS_API_URL_ENV).ok();
        let endpoint = subscription_url(endpoint_override.as_deref())
            .map_err(|message| ProviderError::Other(message.to_string()))?;
        let response = self
            .client
            .get(endpoint)
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

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        match ctx.source_mode {
            SourceMode::Auto | SourceMode::OAuth => {
                let api_key = resolve_api_key(
                    ctx.api_key.as_deref(),
                    ELEVENLABS_CREDENTIAL_TARGET,
                    &["ELEVENLABS_API_KEY", "XI_API_KEY"],
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

    let mut snapshot = UsageSnapshot::new(primary);
    if let Some(login_method) = display_tier(subscription) {
        snapshot = snapshot.with_login_method(login_method);
    }

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

fn display_tier(subscription: &ElevenLabsSubscriptionResponse) -> Option<String> {
    let tier = subscription
        .tier
        .as_deref()
        .map(str::trim)
        .filter(|tier| !tier.is_empty());
    let Some(tier) = tier else {
        return subscription
            .status
            .as_deref()
            .filter(|status| !status.is_empty())
            .map(str::to_string);
    };

    let tier = title_case_tier(tier);
    match subscription
        .status
        .as_deref()
        .filter(|status| !status.is_empty() && !status.eq_ignore_ascii_case("active"))
    {
        Some(status) => Some(format!("{tier} · {status}")),
        None => Some(tier),
    }
}

fn title_case_tier(tier: &str) -> String {
    let normalized = tier.replace('_', " ").to_lowercase();
    let mut title = String::with_capacity(normalized.len());
    let mut previous_is_word = false;
    for character in normalized.chars() {
        let is_word = character.is_ascii_alphanumeric() || character == '_';
        if is_word && !previous_is_word {
            title.push(character.to_ascii_uppercase());
        } else {
            title.push(character);
        }
        previous_is_word = is_word;
    }
    title
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
    if let Some(key) = resolve_env_api_key(env_names, |name| std::env::var(name).ok()) {
        return Ok(key);
    }
    Err(ProviderError::NotInstalled(format!(
        "API key not found. Set {} in Preferences or environment.",
        env_names.join(" / ")
    )))
}

fn resolve_env_api_key<F>(env_names: &[&str], mut lookup: F) -> Option<String>
where
    F: FnMut(&str) -> Option<String>,
{
    env_names
        .iter()
        .find_map(|name| lookup(name).and_then(|value| cleaned(&value)))
}

fn subscription_url(raw_override: Option<&str>) -> Result<String, &'static str> {
    let base = match raw_override.and_then(cleaned) {
        Some(raw) => normalized_https_url(&raw).ok_or(INVALID_ENDPOINT_OVERRIDE)?,
        None => Url::parse(ELEVENLABS_API_BASE_URL).expect("default API base URL is valid"),
    };
    endpoint_from_base(base).map(|url| url.to_string())
}

fn endpoint_from_base(mut base: Url) -> Result<Url, &'static str> {
    let has_v1_suffix = base.path().trim_end_matches('/').rsplit('/').next() == Some("v1");
    let mut path = base
        .path_segments_mut()
        .map_err(|_| INVALID_ENDPOINT_OVERRIDE)?;
    path.pop_if_empty();
    if has_v1_suffix {
        path.extend(["user", "subscription"]);
    } else {
        path.extend(["v1", "user", "subscription"]);
    }
    drop(path);
    Ok(base)
}

fn normalized_https_url(raw: &str) -> Option<Url> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let candidate = if has_explicit_scheme(raw) {
        // Foundation's URL(string:) yields no host unless "//" directly follows the scheme
        // colon, so upstream rejects "https:host" even when "://" appears later in the value.
        if !raw
            .split_once(':')
            .is_some_and(|(_, rest)| rest.starts_with("//"))
        {
            return None;
        }
        raw.to_string()
    } else {
        format!("https://{raw}")
    };
    let url = Url::parse(&candidate).ok()?;
    if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some() {
        return None;
    }
    let host = url.host_str()?;
    if host.is_empty()
        || host.contains('%')
        || host
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
    {
        return None;
    }
    let authority = candidate
        .split_once("://")?
        .1
        .split(['/', '?', '#'])
        .next()?;
    if authority.is_empty()
        || authority.contains('@')
        || authority.contains('%')
        || authority.contains('\\')
        || authority
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
    {
        return None;
    }
    let is_bracketed_ipv6 = authority.starts_with('[') && host.contains(':');
    if !is_bracketed_ipv6
        && host
            .chars()
            .any(|character| matches!(character, '/' | '\\' | '?' | '#' | '@' | ':'))
    {
        return None;
    }
    Some(url)
}

/// Mirrors upstream `ProviderEndpointOverrideValidator.hasExplicitURLScheme`.
fn has_explicit_scheme(raw: &str) -> bool {
    let Some(colon) = raw.find(':') else {
        return false;
    };
    if raw[colon..].starts_with("://") {
        return true;
    }
    if raw.find(['/', '?', '#']).is_some_and(|end| colon > end) {
        return false;
    }
    let after_colon = &raw[colon + 1..];
    if after_colon.is_empty() {
        return true;
    }
    let suffix = after_colon
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default();
    if !suffix.is_empty() && suffix.chars().all(char::is_numeric) {
        return false;
    }
    let mut scheme = raw[..colon].chars();
    scheme.next().is_some_and(char::is_alphabetic)
        && scheme
            .all(|character| character.is_alphanumeric() || matches!(character, '+' | '-' | '.'))
}

fn cleaned(raw: &str) -> Option<String> {
    let mut value = raw.trim();
    if value.is_empty() {
        return None;
    }
    if value.len() >= 2
        && ((value.starts_with('"') && value.ends_with('"'))
            || (value.starts_with('\'') && value.ends_with('\'')))
    {
        value = value[1..value.len() - 1].trim();
    }
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
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
        assert_eq!(snapshot.login_method.as_deref(), Some("Creator"));
    }

    fn subscription(tier: Option<&str>, status: Option<&str>) -> ElevenLabsSubscriptionResponse {
        ElevenLabsSubscriptionResponse {
            tier: tier.map(str::to_string),
            character_count: 0,
            character_limit: 0,
            voice_slots_used: None,
            professional_voice_slots_used: None,
            voice_limit: None,
            professional_voice_limit: None,
            status: status.map(str::to_string),
            next_character_count_reset_unix: None,
        }
    }

    #[test]
    fn subscription_endpoint_matches_upstream_base_url_rules() {
        let cases = [
            (
                "https://elevenlabs.test",
                "https://elevenlabs.test/v1/user/subscription",
            ),
            (
                "https://elevenlabs.test/v1/",
                "https://elevenlabs.test/v1/user/subscription",
            ),
            ("myproxy:8443", "https://myproxy:8443/v1/user/subscription"),
            ("proxy:8443/v1", "https://proxy:8443/v1/user/subscription"),
            (
                "mock-elevenlabs:8443",
                "https://mock-elevenlabs:8443/v1/user/subscription",
            ),
            (
                "elevenlabs.test/proxy",
                "https://elevenlabs.test/proxy/v1/user/subscription",
            ),
            (
                "https://elevenlabs.test/v1/?fixture=1",
                "https://elevenlabs.test/v1/user/subscription?fixture=1",
            ),
            (
                "https://[::1]:8443/v1",
                "https://[::1]:8443/v1/user/subscription",
            ),
            (
                "elevenlabs.test:8443/proxy",
                "https://elevenlabs.test:8443/proxy/v1/user/subscription",
            ),
        ];
        for (input, expected) in cases {
            assert_eq!(subscription_url(Some(input)).unwrap(), expected, "{input}");
        }
        assert_eq!(
            subscription_url(None).unwrap(),
            "https://api.elevenlabs.io/v1/user/subscription"
        );
        assert_eq!(
            subscription_url(Some("  ")).unwrap(),
            "https://api.elevenlabs.io/v1/user/subscription"
        );
        assert_eq!(
            subscription_url(Some(" 'https://elevenlabs.test/v1/' ")).unwrap(),
            "https://elevenlabs.test/v1/user/subscription"
        );
    }

    #[test]
    fn invalid_endpoint_overrides_are_rejected_with_the_upstream_message() {
        for input in [
            "http://attacker.test/v1",
            "ftp://attacker.test/v1",
            "https://user:password@elevenlabs.test",
            "https://@elevenlabs.test",
            "https://elevenlabs%2etest",
            "https://elevenlabs.test\\@attacker.test",
            "https:elevenlabs.test",
            "https:/elevenlabs.test",
            "https:",
            "https:elevenlabs.test/v1?next=https://x",
            "https:/elevenlabs.test/x://y",
            "https:elevenlabs.test#://frag",
        ] {
            assert_eq!(
                subscription_url(Some(input)),
                Err(INVALID_ENDPOINT_OVERRIDE),
                "{input}"
            );
        }
    }

    #[test]
    fn environment_api_key_lookup_prefers_primary_and_skips_blank_values() {
        use std::collections::HashMap;

        let both = HashMap::from([
            ("ELEVENLABS_API_KEY".to_string(), " primary ".to_string()),
            ("XI_API_KEY".to_string(), "alias".to_string()),
        ]);
        assert_eq!(
            resolve_env_api_key(&["ELEVENLABS_API_KEY", "XI_API_KEY"], |name| {
                both.get(name).cloned()
            })
            .as_deref(),
            Some("primary")
        );

        let alias_only = HashMap::from([("XI_API_KEY".to_string(), " 'alias-key' ".to_string())]);
        assert_eq!(
            resolve_env_api_key(&["ELEVENLABS_API_KEY", "XI_API_KEY"], |name| {
                alias_only.get(name).cloned()
            })
            .as_deref(),
            Some("alias-key")
        );

        let blank_primary = HashMap::from([
            ("ELEVENLABS_API_KEY".to_string(), "   ".to_string()),
            ("XI_API_KEY".to_string(), "alias-key".to_string()),
        ]);
        assert_eq!(
            resolve_env_api_key(&["ELEVENLABS_API_KEY", "XI_API_KEY"], |name| {
                blank_primary.get(name).cloned()
            })
            .as_deref(),
            Some("alias-key")
        );
    }

    #[test]
    fn tier_and_status_display_matches_upstream_casing_and_suffix() {
        let cases = [
            (Some("creator"), Some("active"), Some("Creator")),
            (
                Some(" growing_business "),
                Some("past_due"),
                Some("Growing Business · past_due"),
            ),
            (Some("PRO"), Some("ACTIVE"), Some("Pro")),
            (Some(""), Some("trialing"), Some("trialing")),
            (Some("starter"), Some(""), Some("Starter")),
            (None, None, None),
        ];
        for (tier, status, expected) in cases {
            assert_eq!(
                display_tier(&subscription(tier, status)).as_deref(),
                expected,
                "tier={tier:?}, status={status:?}"
            );
        }
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
