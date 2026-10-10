//! Codex API client for fetching usage information
//!
//! Uses OAuth tokens stored by the Codex CLI in ~/.codex/auth.json

use super::{pat, weekly_reset};
use crate::core::{CostSnapshot, ProviderError, UsageSnapshot};
use chrono::Utc;
use std::path::PathBuf;
use std::time::Instant;

mod credentials;
mod parse;
mod reset_credits;

use credentials::CodexCredentials;
use parse::format_plan_type;
use reset_credits::apply_reset_credits_window;
pub(super) use reset_credits::{ResetCredit, ResetCredits, next_available_reset_credit_expiry};

#[path = "subscription.rs"]
mod subscription;

const DEFAULT_BASE_URL: &str = "https://chatgpt.com/backend-api";
const USAGE_PATH: &str = "/wham/usage";

/// Codex API client
pub struct CodexApi {
    client: reqwest::Client,
    home_dir: PathBuf,
    /// When set, overrides CODEX_HOME / ~/.codex for auth.json + config.toml (tests).
    codex_home_override: Option<PathBuf>,
}

impl CodexApi {
    pub fn new() -> Self {
        // Build client with proper TLS settings
        let client = crate::core::credentialed_http_client_builder()
            .use_rustls_tls()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Self {
            client,
            home_dir: dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")),
            codex_home_override: None,
        }
    }

    /// Point the client at a specific Codex home directory (contains auth.json / config.toml).
    pub fn with_codex_home(mut self, codex_home: impl Into<PathBuf>) -> Self {
        self.codex_home_override = Some(codex_home.into());
        self
    }

    fn codex_dir(&self) -> PathBuf {
        if let Some(override_dir) = &self.codex_home_override {
            return override_dir.clone();
        }
        if let Ok(codex_home) = std::env::var("CODEX_HOME") {
            let trimmed = codex_home.trim();
            if !trimmed.is_empty() {
                return PathBuf::from(trimmed);
            }
        }
        self.home_dir.join(".codex")
    }

    pub(super) fn has_pat_credentials(&self) -> bool {
        pat::load_token(&self.get_auth_path()).is_ok()
    }

    pub(super) async fn fetch_usage_pat(
        &self,
        cli_version: Option<&str>,
    ) -> Result<(UsageSnapshot, Option<CostSnapshot>, Option<String>), ProviderError> {
        let token = pat::load_token(&self.get_auth_path())?;
        let (json, whoami) =
            pat::fetch_usage(&self.client, &self.resolve_base_url(), &token, cli_version).await?;
        let account_id = whoami.account_id.clone();
        // Email is display metadata, not a stable credential/account binding.
        // A PAT without the provider's account id must fail closed for any
        // operation that could reopen a local session.
        let account_identity = account_id.clone();
        let (mut usage, cost) = self.build_result_from_json(&json)?;
        if let Some(email) = whoami.email {
            usage = usage.with_email(email);
        }
        if usage.login_method.is_none()
            && let Some(plan_type) = whoami.plan_type
        {
            usage = usage.with_login_method(format_plan_type(&plan_type));
        }
        let usage = subscription::enrich_subscription_metadata(
            self,
            &self.resolve_base_url(),
            &token,
            account_id.as_deref(),
            usage,
        )
        .await;
        Ok((usage, cost, account_identity))
    }

    /// Fetch usage information from Codex API.
    ///
    /// v0.55.1 weekly-reset publication is account-scoped and persistent: a
    /// suspicious early drop to <=1% is confirmed before it can replace the
    /// last published weekly window, and reset-credit inventory is evidence
    /// only. The app never redeems or decrements credits on observation.
    pub async fn fetch_usage(
        &self,
    ) -> Result<(UsageSnapshot, Option<CostSnapshot>, Option<String>), ProviderError> {
        let (usage, cost, account, _) = self.fetch_usage_with_reset_credits().await?;
        Ok((usage, cost, account))
    }

    pub(super) async fn fetch_usage_with_reset_credits(
        &self,
    ) -> Result<
        (
            UsageSnapshot,
            Option<CostSnapshot>,
            Option<String>,
            Option<ResetCredits>,
        ),
        ProviderError,
    > {
        let creds = self.load_credentials().await?;
        let base_url = self.resolve_base_url();
        let auth_path = self.get_auth_path();
        let scope = weekly_reset::scope_key(creds.account_id.as_deref(), &auth_path);
        let exact_oauth = creds.is_external_oauth;
        let mut state = weekly_reset::load(&scope);

        let first_fetch_started = Instant::now();
        let (first_usage, first_cost, first_credits) =
            self.fetch_usage_once(&creds, &base_url).await?;
        let first_credits = self
            .initial_reset_credits(
                &creds,
                &base_url,
                first_fetch_started,
                first_credits,
                state.has_delayed_candidate(),
            )
            .await;
        let mut displayed_credits = first_credits.clone();
        let observed_at = Utc::now();
        let first_inventory = weekly_reset::inventory(first_credits.as_ref(), observed_at);
        let (usage, cost) = match weekly_reset::initial_decision(
            &mut state,
            &first_usage,
            first_inventory.as_ref(),
            exact_oauth,
            observed_at,
        ) {
            weekly_reset::InitialDecision::Publish => {
                weekly_reset::commit_publication(&mut state, &first_usage, first_inventory);
                weekly_reset::save(&scope, &state);
                (first_usage, first_cost)
            }
            weekly_reset::InitialDecision::Preserve => {
                let usage = weekly_reset::preserve_weekly(&state, first_usage);
                weekly_reset::save(&scope, &state);
                (usage, first_cost)
            }
            weekly_reset::InitialDecision::RequiresConfirmation => {
                // Confirmation must compare independent inventory observations,
                // even when the ordinary 10-minute cache is still fresh.
                let initial_credits = self
                    .fresh_reset_credits_for_confirmation(&creds, &base_url, first_fetch_started)
                    .await;
                let confirmation = self.fetch_usage_once(&creds, &base_url).await;
                let (confirmation_usage, confirmation_cost, _) = match confirmation {
                    Ok(value) => value,
                    Err(error) => {
                        tracing::debug!(
                            %error,
                            "Codex weekly reset confirmation failed; preserving first successful usage"
                        );
                        let usage = weekly_reset::preserve_weekly(&state, first_usage);
                        weekly_reset::save(&scope, &state);
                        let usage = subscription::enrich_subscription_metadata(
                            self,
                            &base_url,
                            &creds.access_token,
                            creds.account_id.as_deref(),
                            usage,
                        )
                        .await;
                        return Ok((
                            usage,
                            first_cost,
                            creds.account_id.clone(),
                            displayed_credits,
                        ));
                    }
                };
                let confirmation_credits = if initial_credits.is_some() {
                    self.fetch_rate_limit_reset_credits_fresh(&creds, &base_url)
                        .await
                } else {
                    None
                };
                displayed_credits = confirmation_credits
                    .clone()
                    .or(initial_credits.clone())
                    .or(displayed_credits);
                let confirmation_inventory =
                    weekly_reset::inventory(confirmation_credits.as_ref(), Utc::now());
                let initial_inventory =
                    weekly_reset::inventory(initial_credits.as_ref(), observed_at);
                match weekly_reset::confirmation_decision(
                    &mut state,
                    &first_usage,
                    initial_inventory.as_ref(),
                    &confirmation_usage,
                    confirmation_inventory.as_ref(),
                    exact_oauth,
                    observed_at,
                ) {
                    weekly_reset::ConfirmationDecision::Publish => {
                        weekly_reset::commit_publication(
                            &mut state,
                            &confirmation_usage,
                            confirmation_inventory,
                        );
                        weekly_reset::save(&scope, &state);
                        (confirmation_usage, confirmation_cost)
                    }
                    weekly_reset::ConfirmationDecision::Preserve => {
                        let usage = weekly_reset::preserve_weekly(&state, first_usage);
                        weekly_reset::save(&scope, &state);
                        (usage, first_cost)
                    }
                }
            }
        };
        let usage = subscription::enrich_subscription_metadata(
            self,
            &base_url,
            &creds.access_token,
            creds.account_id.as_deref(),
            usage,
        )
        .await;
        let usage = apply_reset_credits_window(usage, displayed_credits.as_ref());
        Ok((usage, cost, creds.account_id.clone(), displayed_credits))
    }

    /// Bearer GET shared by the ChatGPT backend endpoints; an empty account id
    /// sends no `ChatGPT-Account-Id` header.
    pub(super) fn authed_get(
        &self,
        url: &str,
        access_token: &str,
        account_id: Option<&str>,
    ) -> reqwest::RequestBuilder {
        let request = self
            .client
            .get(url)
            .header("Authorization", format!("Bearer {access_token}"))
            .header("User-Agent", "CodexBar")
            .header("Accept", "application/json");
        match account_id.filter(|id| !id.is_empty()) {
            Some(account_id) => request.header("ChatGPT-Account-Id", account_id),
            None => request,
        }
    }

    async fn fetch_usage_once(
        &self,
        creds: &CodexCredentials,
        base_url: &str,
    ) -> Result<(UsageSnapshot, Option<CostSnapshot>, Option<ResetCredits>), ProviderError> {
        let url = format!("{}{}", base_url, USAGE_PATH);
        let response = self
            .authed_get(&url, &creds.access_token, creds.account_id.as_deref())
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(super::authenticated_http_error(response, "Codex API").await);
        }
        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| ProviderError::Parse(e.to_string()))?;
        let (mut usage, cost) = self.build_result_from_json(&json)?;
        let reset_credits = self
            .fetch_rate_limit_reset_credits_cached(creds, base_url)
            .await;
        usage = apply_reset_credits_window(usage, reset_credits.as_ref());
        Ok((usage, cost, reset_credits))
    }

    fn get_auth_path(&self) -> PathBuf {
        self.codex_dir().join("auth.json")
    }

    fn resolve_base_url(&self) -> String {
        let config_path = self.codex_dir().join("config.toml");

        if let Ok(content) = std::fs::read_to_string(&config_path)
            && let Some(base_url) = parse_chatgpt_base_url(&content)
        {
            let normalized = normalize_base_url(&base_url);
            // Only allow HTTPS URLs for custom base URLs to prevent token exfiltration
            if normalized.starts_with("https://")
                || normalized.starts_with("http://127.0.0.1")
                || normalized.starts_with("http://localhost")
            {
                return normalized;
            }
            tracing::warn!(
                "Ignoring insecure custom chatgpt_base_url (must be HTTPS): {}",
                normalized
            );
        }

        DEFAULT_BASE_URL.to_string()
    }

    /// Whether config.toml points the CLI at a backend that does not
    /// authenticate against ChatGPT (Bedrock / other custom providers).
    fn uses_custom_backend(&self) -> bool {
        let Ok(content) = std::fs::read_to_string(self.codex_dir().join("config.toml")) else {
            return false;
        };
        parse_chatgpt_base_url(&content).is_some() || config_uses_non_chatgpt_provider(&content)
    }
}

impl Default for CodexApi {
    fn default() -> Self {
        Self::new()
    }
}

/// Whether config.toml selects a non-ChatGPT model provider (e.g. Bedrock),
/// meaning the CLI never authenticates against ChatGPT.
fn config_uses_non_chatgpt_provider(config_content: &str) -> bool {
    config_content.lines().any(|line| {
        let Some((key, value)) = line.trim().split_once('=') else {
            return false;
        };
        if !key.trim().eq_ignore_ascii_case("model_provider") {
            return false;
        }
        let provider = value.trim().trim_matches('"').trim_matches('\'');
        !provider.is_empty() && !provider.eq_ignore_ascii_case("openai")
    })
}

fn parse_chatgpt_base_url(config_content: &str) -> Option<String> {
    for line in config_content.lines() {
        // Skip comments
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }

        // Look for chatgpt_base_url = "..."
        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim();
            if key == "chatgpt_base_url" {
                let mut value = value.trim();
                // Remove quotes
                if (value.starts_with('"') && value.ends_with('"'))
                    || (value.starts_with('\'') && value.ends_with('\''))
                {
                    value = &value[1..value.len() - 1];
                }
                return Some(value.trim().to_string());
            }
        }
    }
    None
}

fn normalize_base_url(url: &str) -> String {
    let mut trimmed = url.trim().to_string();
    if trimmed.is_empty() {
        return DEFAULT_BASE_URL.to_string();
    }

    // Remove trailing slashes
    while trimmed.ends_with('/') {
        trimmed.pop();
    }

    // Add /backend-api if needed
    if (trimmed.starts_with("https://chatgpt.com")
        || trimmed.starts_with("https://chat.openai.com"))
        && !trimmed.contains("/backend-api")
    {
        trimmed.push_str("/backend-api");
    }

    trimmed
}

#[cfg(test)]
mod credential_retry_tests;

#[cfg(test)]
mod tests;
