use super::super::weekly_reset;
use super::CodexApi;
use super::credentials::CodexCredentials;
use crate::core::{ProviderError, RateWindow, UsageSnapshot};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tokio::sync::Mutex as AsyncMutex;

pub(super) const RESET_CREDITS_PATH: &str = "/wham/rate-limit-reset-credits";
pub(super) const RESET_CREDITS_CACHE_TTL: Duration = Duration::from_secs(600);

static RESET_CREDITS_CACHE: OnceLock<Mutex<HashMap<String, Arc<AsyncMutex<ResetCreditsCache>>>>> =
    OnceLock::new();

#[derive(Default)]
pub(super) struct ResetCreditsCache {
    pub(super) loaded_at: Option<Instant>,
    value: Option<ResetCredits>,
    confirmation_failure_at: Option<Instant>,
}

impl ResetCreditsCache {
    fn confirmation_failed_recently(&self) -> bool {
        self.confirmation_failure_at
            .is_some_and(|failed| failed.elapsed() < RESET_CREDITS_CACHE_TTL)
    }
}

impl CodexApi {
    pub(super) fn reset_credits_cache_slot(
        &self,
        creds: &CodexCredentials,
        base_url: &str,
    ) -> Arc<AsyncMutex<ResetCreditsCache>> {
        // The Codex home is part of the scope: two homes never share an
        // observation, even when they hold the same account and token.
        let auth_path = self.get_auth_path();
        let home = weekly_reset::scope_key(None, &auth_path);
        let account = weekly_reset::scope_key(creds.account_id.as_deref(), &auth_path);
        let token = weekly_reset::scope_key(Some(&creds.access_token), &auth_path);
        let key = format!(
            "{}|{home}|{account}|{token}",
            base_url.trim_end_matches('/')
        );
        let cache = RESET_CREDITS_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
        let mut cache = cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        Arc::clone(
            cache
                .entry(key)
                .or_insert_with(|| Arc::new(AsyncMutex::new(ResetCreditsCache::default()))),
        )
    }

    pub(super) async fn fetch_rate_limit_reset_credits_cached(
        &self,
        creds: &CodexCredentials,
        base_url: &str,
    ) -> Option<ResetCredits> {
        let slot = self.reset_credits_cache_slot(creds, base_url);
        let mut cache = slot.lock().await;
        if cache
            .loaded_at
            .is_some_and(|loaded| loaded.elapsed() < RESET_CREDITS_CACHE_TTL)
        {
            return cache.value.clone();
        }
        cache.value = self
            .fetch_rate_limit_reset_credits(creds, base_url)
            .await
            .ok();
        cache.loaded_at = Some(Instant::now());
        cache.confirmation_failure_at = None;
        cache.value.clone()
    }

    /// Reset credits for the initial weekly-reset decision. A pending delayed
    /// candidate is revalidated against the current inventory, which must be
    /// observed after the candidate was stored: the ten-minute cache can still
    /// hold the very observation that created it, so that case reads fresh.
    pub(super) async fn initial_reset_credits(
        &self,
        creds: &CodexCredentials,
        base_url: &str,
        started: Instant,
        observed: Option<ResetCredits>,
        candidate_pending: bool,
    ) -> Option<ResetCredits> {
        if !candidate_pending {
            return observed;
        }
        self.fresh_reset_credits_for_confirmation(creds, base_url, started)
            .await
    }

    pub(super) async fn fresh_reset_credits_for_confirmation(
        &self,
        creds: &CodexCredentials,
        base_url: &str,
        started: Instant,
    ) -> Option<ResetCredits> {
        let slot = self.reset_credits_cache_slot(creds, base_url);
        let mut cache = slot.lock().await;
        cache.value.as_ref()?;
        if cache.confirmation_failed_recently() {
            return None;
        }
        if cache.loaded_at.is_some_and(|loaded| loaded >= started) {
            return cache.value.clone();
        }
        self.refetch_reset_credits_into(&mut cache, creds, base_url)
            .await
    }

    pub(super) async fn fetch_rate_limit_reset_credits_fresh(
        &self,
        creds: &CodexCredentials,
        base_url: &str,
    ) -> Option<ResetCredits> {
        let slot = self.reset_credits_cache_slot(creds, base_url);
        let mut cache = slot.lock().await;
        if cache.confirmation_failed_recently() {
            return None;
        }
        self.refetch_reset_credits_into(&mut cache, creds, base_url)
            .await
    }

    /// A failed read keeps the cached inventory but blocks further
    /// confirmation reads for one cache TTL.
    async fn refetch_reset_credits_into(
        &self,
        cache: &mut ResetCreditsCache,
        creds: &CodexCredentials,
        base_url: &str,
    ) -> Option<ResetCredits> {
        let fresh = self
            .fetch_rate_limit_reset_credits(creds, base_url)
            .await
            .ok();
        if let Some(value) = fresh.as_ref() {
            cache.value = Some(value.clone());
            cache.loaded_at = Some(Instant::now());
            cache.confirmation_failure_at = None;
        } else {
            cache.confirmation_failure_at = Some(Instant::now());
        }
        fresh
    }

    async fn fetch_rate_limit_reset_credits(
        &self,
        creds: &CodexCredentials,
        base_url: &str,
    ) -> Result<ResetCredits, ProviderError> {
        let response = self
            .authed_get(
                &format!("{}{}", base_url, RESET_CREDITS_PATH),
                &creds.access_token,
                creds.account_id.as_deref(),
            )
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(
                super::super::authenticated_http_error(response, "Codex reset credits").await,
            );
        }
        decode_reset_credits(&response.bytes().await?)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub(in crate::providers::codex) struct ResetCredit {
    #[serde(default)]
    pub(in crate::providers::codex) id: Option<String>,
    #[serde(default, alias = "resetType")]
    pub(in crate::providers::codex) reset_type: Option<String>,
    #[serde(default)]
    pub(in crate::providers::codex) status: Option<String>,
    #[serde(default)]
    pub(in crate::providers::codex) expires_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(in crate::providers::codex) struct ResetCredits {
    #[serde(default)]
    pub(in crate::providers::codex) credits: Vec<ResetCredit>,
    pub(in crate::providers::codex) available_count: u32,
}

pub(super) fn decode_reset_credits(data: &[u8]) -> Result<ResetCredits, ProviderError> {
    serde_json::from_slice(data)
        .map_err(|e| ProviderError::Parse(format!("Failed to parse Codex reset credits: {e}")))
}

fn parse_credit_expiry(raw: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

fn is_available_credit(credit: &ResetCredit) -> bool {
    match credit.status.as_deref() {
        None | Some("") => true,
        Some(status) => status.eq_ignore_ascii_case("available"),
    }
}

pub(in crate::providers::codex) fn next_available_reset_credit_expiry(
    credits: &[ResetCredit],
    now: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    credits
        .iter()
        .filter(|credit| is_available_credit(credit))
        .filter_map(|credit| credit.expires_at.as_deref().and_then(parse_credit_expiry))
        .filter(|expires_at| *expires_at > now)
        .min()
}

pub(super) fn reset_credits_rate_window(reset: &ResetCredits, now: DateTime<Utc>) -> RateWindow {
    let description = format!(
        "{} reset credit{} available",
        reset.available_count,
        if reset.available_count == 1 { "" } else { "s" }
    );
    let mut window = RateWindow::informational(description);
    window.resets_at = next_available_reset_credit_expiry(&reset.credits, now);
    window
}

pub(super) fn apply_reset_credits_window(
    mut usage: UsageSnapshot,
    reset: Option<&ResetCredits>,
) -> UsageSnapshot {
    usage
        .extra_rate_windows
        .retain(|window| window.id != "reset-credits");
    if let Some(reset) = reset.filter(|reset| reset.available_count > 0) {
        let window = reset_credits_rate_window(reset, Utc::now());
        usage = usage.with_extra_rate_window("reset-credits", "Reset credits", window);
    }
    usage
}
