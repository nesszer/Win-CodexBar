//! Command Palette integration snapshot endpoint.
//!
//! This module is intentionally read-only. It exposes a small, stable JSON
//! contract for external shells such as PowerToys Command Palette without
//! leaking account identifiers or credential material.

use std::collections::{HashMap, HashSet};

use anyhow::Context;
use chrono::Utc;
use clap::{Args, Subcommand};
use serde::Serialize;

use crate::core::{
    FetchContext, ProviderAccountData, ProviderFetchResult, ProviderId, ProviderMetadata,
    RateWindow, SourceMode, TokenAccountOverride, TokenAccountStore, instantiate_provider,
};
use crate::settings::{ApiKeys, ManualCookies, Settings};

const CONTRACT_VERSION: &str = "cmdpal.snapshot.v1";
const PROVIDER_FETCH_TIMEOUT_SECS: u64 = 30;

#[derive(Args, Debug)]
pub struct CmdPalArgs {
    #[command(subcommand)]
    pub command: CmdPalCommand,
}

#[derive(Subcommand, Debug)]
pub enum CmdPalCommand {
    /// Emit a safe, read-only JSON snapshot for Command Palette.
    Snapshot(SnapshotArgs),
}

#[derive(Args, Debug)]
pub struct SnapshotArgs {
    /// Emit JSON. Kept explicit so the CLI contract is self-documenting.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CmdPalSnapshotPayload {
    pub contract_version: &'static str,
    pub generated_at: String,
    pub refresh_interval_secs: u64,
    pub providers: Vec<CmdPalProviderSnapshot>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CmdPalProviderSnapshot {
    pub provider_id: String,
    pub display_name: String,
    pub primary_label: Option<String>,
    pub primary: Option<CmdPalRateWindow>,
    pub secondary_label: Option<String>,
    pub secondary: Option<CmdPalRateWindow>,
    pub source: Option<String>,
    pub updated_at: Option<String>,
    pub error: Option<String>,
    pub dashboard_url: Option<String>,
    pub status_page_url: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CmdPalRateWindow {
    pub used_percent: f64,
    pub remaining_percent: f64,
    pub window_minutes: Option<u32>,
    pub resets_at: Option<String>,
    pub reset_description: Option<String>,
    pub is_exhausted: bool,
}

impl CmdPalRateWindow {
    fn from_rate_window(window: &RateWindow) -> Self {
        Self {
            used_percent: window.used_percent,
            remaining_percent: window.remaining_percent(),
            window_minutes: window.window_minutes,
            resets_at: window.resets_at.map(|dt| dt.to_rfc3339()),
            reset_description: window.reset_description.clone(),
            is_exhausted: window.is_exhausted(),
        }
    }
}

impl CmdPalProviderSnapshot {
    fn from_fetch_result(
        id: ProviderId,
        metadata: &ProviderMetadata,
        result: &ProviderFetchResult,
    ) -> Self {
        let usage = &result.usage;

        Self {
            provider_id: id.cli_name().to_string(),
            display_name: id.display_name().to_string(),
            primary_label: Some(metadata.session_label.to_string()),
            primary: Some(CmdPalRateWindow::from_rate_window(&usage.primary)),
            secondary_label: usage
                .secondary
                .as_ref()
                .map(|_| metadata.weekly_label.to_string()),
            secondary: usage
                .secondary
                .as_ref()
                .map(CmdPalRateWindow::from_rate_window),
            source: Some(result.source_label.clone()),
            updated_at: Some(usage.updated_at.to_rfc3339()),
            error: None,
            dashboard_url: metadata.dashboard_url.map(ToString::to_string),
            status_page_url: metadata.status_page_url.map(ToString::to_string),
        }
    }

    fn from_error(id: ProviderId, metadata: &ProviderMetadata, error: String) -> Self {
        Self {
            provider_id: id.cli_name().to_string(),
            display_name: id.display_name().to_string(),
            primary_label: Some(metadata.session_label.to_string()),
            primary: None,
            secondary_label: None,
            secondary: None,
            source: None,
            updated_at: Some(Utc::now().to_rfc3339()),
            error: Some(error),
            dashboard_url: metadata.dashboard_url.map(ToString::to_string),
            status_page_url: metadata.status_page_url.map(ToString::to_string),
        }
    }
}

pub async fn run(args: CmdPalArgs) -> anyhow::Result<()> {
    match args.command {
        CmdPalCommand::Snapshot(snapshot_args) => snapshot(snapshot_args).await,
    }
}

async fn snapshot(_args: SnapshotArgs) -> anyhow::Result<()> {
    let settings = Settings::load();
    let payload = build_snapshot_payload(settings).await;
    let json =
        serde_json::to_string_pretty(&payload).context("serialize Command Palette snapshot")?;
    println!("{json}");
    Ok(())
}

async fn build_snapshot_payload(settings: Settings) -> CmdPalSnapshotPayload {
    let enabled_ids = ordered_enabled_provider_ids(&settings);
    let manual_cookies = ManualCookies::load();
    let api_keys = ApiKeys::load();
    let token_accounts = TokenAccountStore::new().load().unwrap_or_else(|e| {
        tracing::warn!("failed to load token accounts for Command Palette snapshot: {e}");
        HashMap::new()
    });

    let mut handles = Vec::with_capacity(enabled_ids.len());

    for id in enabled_ids {
        let ctx = build_fetch_context(id, &settings, &manual_cookies, &api_keys, &token_accounts);
        handles.push(tokio::spawn(async move {
            let provider = instantiate_provider(id);
            let metadata = provider.metadata().clone();
            match tokio::time::timeout(
                std::time::Duration::from_secs(PROVIDER_FETCH_TIMEOUT_SECS),
                provider.fetch_usage(&ctx),
            )
            .await
            {
                Ok(Ok(result)) => CmdPalProviderSnapshot::from_fetch_result(id, &metadata, &result),
                Ok(Err(e)) => CmdPalProviderSnapshot::from_error(
                    id,
                    &metadata,
                    crate::logging::safe_error_message(e),
                ),
                Err(_) => CmdPalProviderSnapshot::from_error(
                    id,
                    &metadata,
                    format!("Timeout after {PROVIDER_FETCH_TIMEOUT_SECS}s"),
                ),
            }
        }));
    }

    let mut providers = Vec::with_capacity(handles.len());
    for handle in handles {
        if let Ok(provider) = handle.await {
            providers.push(provider);
        }
    }

    CmdPalSnapshotPayload {
        contract_version: CONTRACT_VERSION,
        generated_at: Utc::now().to_rfc3339(),
        refresh_interval_secs: settings.refresh_interval_secs,
        providers,
    }
}

fn ordered_enabled_provider_ids(settings: &Settings) -> Vec<ProviderId> {
    let enabled: HashSet<ProviderId> = settings
        .enabled_providers
        .iter()
        .filter_map(|name| ProviderId::from_cli_name(name))
        .collect();
    if enabled.is_empty() {
        return Vec::new();
    }

    let mut ordered = Vec::with_capacity(enabled.len());
    for name in &settings.provider_order {
        if let Some(id) = ProviderId::from_cli_name(name)
            && enabled.contains(&id)
            && !ordered.contains(&id)
        {
            ordered.push(id);
        }
    }

    for &id in ProviderId::all() {
        if enabled.contains(&id) && !ordered.contains(&id) {
            ordered.push(id);
        }
    }

    ordered
}

fn build_fetch_context(
    id: ProviderId,
    settings: &Settings,
    cookies: &ManualCookies,
    api_keys: &ApiKeys,
    token_accounts: &HashMap<ProviderId, ProviderAccountData>,
) -> FetchContext {
    let cookie_source = settings.cookie_source(id);
    let stored_cookie = cookies.get(id.cli_name()).map(ToString::to_string);
    let token_override = token_accounts
        .get(&id)
        .and_then(|data| data.active_account())
        .cloned()
        .map(|account| TokenAccountOverride::from_account(id, account));
    let active_token_cookie = token_override
        .as_ref()
        .and_then(|override_data| override_data.cookie_header.clone());
    let active_token_env = token_override
        .as_ref()
        .and_then(|override_data| override_data.env_override.as_ref());
    let active_token_api_key = active_token_env.and_then(|env| env.values().next().cloned());
    let usage_source = SourceMode::parse(settings.usage_source(id)).unwrap_or_default();

    let (source_mode, cookie_header) = if id.cookie_domain().is_none() {
        let source_mode = if active_token_env.is_some() {
            SourceMode::OAuth
        } else {
            usage_source
        };
        (source_mode, None)
    } else {
        match cookie_source {
            _ if active_token_env.is_some() => (SourceMode::OAuth, None),
            "off" => (SourceMode::Cli, None),
            "manual" => {
                let cookie_header = active_token_cookie.or(stored_cookie);
                let source_mode = if cookie_header.is_some() {
                    SourceMode::Web
                } else {
                    SourceMode::Cli
                };
                (source_mode, cookie_header)
            }
            "auto" | "browser" | "web" => {
                let cookie_header = active_token_cookie.or(stored_cookie).or_else(|| {
                    id.cookie_domain().and_then(|domain| {
                        crate::browser::cookies::get_cookie_header(domain)
                            .ok()
                            .filter(|h| !h.is_empty())
                    })
                });
                (usage_source, cookie_header)
            }
            _ => (usage_source, stored_cookie),
        }
    };

    let api_key = api_keys
        .get(id.cli_name())
        .map(ToString::to_string)
        .or(active_token_api_key);

    FetchContext {
        source_mode,
        include_credits: false,
        web_timeout: PROVIDER_FETCH_TIMEOUT_SECS,
        verbose: false,
        manual_cookie_header: cookie_header,
        api_key,
    }
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use super::*;
    use crate::core::{ProviderFetchResult, RateWindow, UsageSnapshot};

    #[test]
    fn ordered_enabled_provider_ids_filters_unknown_and_honors_provider_order() {
        let mut settings = Settings::default();
        settings.enabled_providers = [
            "cursor".to_string(),
            "claude".to_string(),
            "unknown".to_string(),
            "codex".to_string(),
        ]
        .into_iter()
        .collect();
        settings.provider_order = vec![
            "cursor".to_string(),
            "missing".to_string(),
            "claude".to_string(),
            "cursor".to_string(),
        ];

        assert_eq!(
            ordered_enabled_provider_ids(&settings),
            vec![ProviderId::Cursor, ProviderId::Claude, ProviderId::Codex]
        );
    }

    #[test]
    fn rate_window_snapshot_preserves_reset_fields() {
        let resets_at = Utc.with_ymd_and_hms(2026, 5, 18, 12, 0, 0).unwrap();
        let window =
            RateWindow::with_details(72.4, Some(300), Some(resets_at), Some("2h 10m".to_string()));

        let snapshot = CmdPalRateWindow::from_rate_window(&window);

        assert_eq!(snapshot.used_percent, 72.4);
        assert!((snapshot.remaining_percent - 27.6).abs() < 1e-9);
        assert_eq!(snapshot.window_minutes, Some(300));
        assert_eq!(
            snapshot.resets_at.as_deref(),
            Some("2026-05-18T12:00:00+00:00")
        );
        assert_eq!(snapshot.reset_description.as_deref(), Some("2h 10m"));
        assert!(!snapshot.is_exhausted);
    }

    #[test]
    fn provider_snapshot_json_excludes_personal_identity_fields() {
        let provider = instantiate_provider(ProviderId::Codex);
        let metadata = provider.metadata().clone();
        let usage = UsageSnapshot::new(RateWindow::new(41.0))
            .with_email("person@example.com")
            .with_organization("Example Org")
            .with_login_method("Team");
        let result = ProviderFetchResult::new(usage, "oauth");

        let snapshot =
            CmdPalProviderSnapshot::from_fetch_result(ProviderId::Codex, &metadata, &result);
        let json = serde_json::to_string(&snapshot).unwrap();

        assert!(!json.contains("person@example.com"));
        assert!(!json.contains("Example Org"));
        assert!(!json.contains("account"));
        assert!(!json.contains("organization"));
        assert!(!json.contains("email"));
        assert!(!json.contains("token"));
        assert!(!json.contains("apiKey"));
        assert!(!json.contains("cookie"));
    }
}
