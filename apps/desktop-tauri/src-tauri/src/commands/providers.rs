use super::credential_alerts::{self, FetchAttempt};
use super::provider_refresh::{
    ProviderRefreshCompletion, ProviderRefreshReservation, complete_provider_refresh,
    reserve_provider_refresh,
};
use super::warning_identity::WarningIdentity;
use super::*;
use chrono::{Local, Utc};
use codexbar::core::HookUsageWindow;
use codexbar::notifications::WarningScope;
use serde::Serialize;
use std::sync::Arc;

mod reset_backfill;

const MAX_CONCURRENT_PROVIDER_FETCHES: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RefreshScope {
    AllEnabled,
    AutoResume,
}

impl RefreshScope {
    fn provider_ids(self, settings: &Settings, enabled_ids: &[ProviderId]) -> Vec<ProviderId> {
        match self {
            Self::AllEnabled => enabled_ids.to_vec(),
            Self::AutoResume => crate::auto_resume::enabled_provider_ids(settings),
        }
    }

    fn refresh_account_lanes(self) -> bool {
        matches!(self, Self::AllEnabled)
    }
}

/// Account changes supersede the old identity's cache and any in-flight batch.
pub(crate) fn invalidate_account_usage(
    state: &mut AppState,
    id: ProviderId,
) -> ProviderUsageSnapshot {
    state.provider_refresh_generation = state.provider_refresh_generation.wrapping_add(1);
    state.is_refreshing = false;
    state.provider_refresh_started_at = None;
    state.transient_provider_failure_counts.remove(&id);
    state.last_good_owners.remove(&id);
    state.auto_resume.clear_provider(id);
    state
        .provider_cache
        .retain(|snapshot| snapshot.provider_id != id.cli_name());
    let pending = ProviderUsageSnapshot::from_error(
        id,
        instantiate_provider(id).metadata(),
        format!("Account changed. Refreshing {} usage…", id.display_name()),
        codexbar::core::ProviderStateKind::Unknown,
    );
    state.provider_cache.push(pending.clone());
    pending
}

// ── Provider refresh commands ────────────────────────────────────────

/// Build a `FetchContext` for a provider using persisted cookies/keys.
pub(crate) fn build_fetch_context(
    id: ProviderId,
    settings: &Settings,
    cookies: &ManualCookies,
    api_keys: &ApiKeys,
    token_accounts: &HashMap<ProviderId, ProviderAccountData>,
) -> FetchContext {
    let provider = instantiate_provider(id);
    let cookie_source = settings.cookie_source(id);
    let stored_cookie = cookies.get(id.cli_name()).map(|s| s.to_string());
    let stored_api_key = api_keys.get(id.cli_name()).map(|s| s.to_string());
    let token_override = token_accounts
        .get(&id)
        .and_then(|data| data.active_account())
        .cloned()
        .map(|account| TokenAccountOverride::from_account(id, account));
    let active_token_cookie = token_override
        .as_ref()
        .and_then(|override_data| override_data.cookie_header.clone());
    let defer_provider_browser_cookie_lookup = provider.owns_browser_cookie_resolution()
        && active_token_cookie.is_none()
        && stored_cookie.is_none();
    let active_token_env = token_override
        .as_ref()
        .and_then(|override_data| override_data.env_override.as_ref());
    let active_token_api_key = active_token_env.and_then(|env| env.values().next().cloned());
    let usage_source = SourceMode::parse(settings.usage_source(id)).unwrap_or_default();
    let token_account_kind = token_override.as_ref().map(|account| account.kind);
    // Selected token-account key overrides a stored provider apiKey (upstream #2271 / #1183).
    let api_key = active_token_api_key.clone().or(stored_api_key);
    let has_kimi_code_api_key =
        id == ProviderId::Kimi && api_key.as_deref().is_some_and(|key| !key.trim().is_empty());
    let has_opencodego_api_key = id == ProviderId::OpenCodeGo
        && api_key.as_deref().is_some_and(|key| !key.trim().is_empty());

    // Providers whose cookies only enrich an API result keep the configured
    // usage source. Off and the default manual-without-cookie state never read
    // a browser; only Automatic imports one (`browser_cookie_import`).
    let cookies_only_enrich_usage = provider.cookies_only_enrich_usage();
    let browser_cookie_import =
        cookies_only_enrich_usage && matches!(cookie_source, "auto" | "browser" | "web");
    let (mut source_mode, mut cookie_header, fails_closed_without_cookie) = if id
        .cookie_domain()
        .is_none()
    {
        // The #725 wallet rule (#725 keeps Auto for wallet providers) folds into
        // the account-source decision; other no-cookie providers still honor
        // cookie_source below (the account-source pack's fix for the #619 merge).
        let keeps_auto =
            usage_source == SourceMode::Auto && provider.token_account_preserves_auto_source();
        // A wallet provider keeps its saved usage source even with a selected
        // token account: rewriting it to OAuth would skip the wallet read
        // (#725 / upstream base-source resolver).
        if keeps_auto || active_token_env.is_none() {
            // A wallet provider keeps its saved usage source even with a
            // selected token account: rewriting it to OAuth would skip the
            // wallet read (#725 / upstream base-source resolver).
            if active_token_env.is_some() {
                (usage_source, None, false)
            } else {
                match cookie_source {
                    // #433: an explicitly selected, non-empty Claude manual cookie is
                    // authoritative. Do not let an active OAuth token account silently
                    // replace it; this keeps tray refresh behavior aligned with diagnose,
                    // whose Claude Auto path tries the supplied Web cookie before OAuth.
                    "manual"
                        if provider.manual_cookie_precedes_token_account()
                            && stored_cookie
                                .as_deref()
                                .is_some_and(|cookie| !cookie.trim().is_empty()) =>
                    {
                        (SourceMode::Web, stored_cookie.clone(), false)
                    }
                    "auto" | "browser" | "web" => (usage_source, None, false),
                    _ => (usage_source, None, false),
                }
            }
        } else {
            match cookie_source {
                // Opt-in web providers keep their default credential lane
                // unless the usage source is explicitly Web; a stored or
                // browser cookie must not turn Auto into Web.
                _ if provider.web_is_opt_in() && usage_source != SourceMode::Web => {
                    (usage_source, None, false)
                }
                // #433: an explicitly selected, non-empty Claude manual cookie is
                // authoritative. Do not let an active OAuth token account silently
                // replace it; this keeps tray refresh behavior aligned with diagnose,
                // whose Claude Auto path tries the supplied Web cookie before OAuth.
                "manual"
                    if provider.manual_cookie_precedes_token_account()
                        && stored_cookie
                            .as_deref()
                            .is_some_and(|cookie| !cookie.trim().is_empty()) =>
                {
                    (SourceMode::Web, stored_cookie.clone(), false)
                }
                _ if active_token_env.is_some() => (SourceMode::OAuth, None, false),
                // Charm Hyper: the cookie source only picks the session, and
                // the usage source keeps routing. Off and an empty Manual
                // source never import a browser session, while Auto keeps its
                // API-key fallback.
                "off" | "manual" if provider.cookie_source_scopes_session_only() => {
                    let cookie_header = if cookie_source == "manual" {
                        active_token_cookie.clone().or(stored_cookie)
                    } else {
                        None
                    };
                    let source_mode = if provider.available_sources().contains(&usage_source) {
                        usage_source
                    } else {
                        SourceMode::Auto
                    };
                    let cookie_missing = cookie_header.is_none();
                    (source_mode, cookie_header, cookie_missing)
                }
                "off" if provider_uses_oauth_without_cookies(id, usage_source) => {
                    (SourceMode::OAuth, None, false)
                }
                "off"
                    if (has_kimi_code_api_key || has_opencodego_api_key)
                        && usage_source == SourceMode::Auto =>
                {
                    (SourceMode::Auto, None, false)
                }
                // Droid/Factory: cookie-off must never scrape browser cookies. Map to
                // Cli (API-only in the provider) so Auto does not fall through to web.
                "off" if id == ProviderId::Factory => (SourceMode::Cli, None, false),
                "off" => (SourceMode::Cli, None, false),
                "manual" => {
                    let cookie_header = active_token_cookie.clone().or(stored_cookie);
                    let fails_closed_without_cookie = cookie_header
                        .as_deref()
                        .is_none_or(|header| header.trim().is_empty())
                        && provider.manual_empty_cookie_policy()
                            == ManualEmptyCookiePolicy::FailClosedWeb;
                    let source_mode = if (has_kimi_code_api_key || has_opencodego_api_key)
                        && usage_source == SourceMode::Auto
                    {
                        SourceMode::Auto
                    } else if let Some(mode) = grok_source_mode_for_manual_cookie(id, usage_source)
                    {
                        // Grok Switch writes ~/.grok/auth.json. Leftover grok.com
                        // cookies must not force Web, or Weekly/notifications keep
                        // showing the previous browser account.
                        mode
                    } else if cookie_header.is_some() {
                        SourceMode::Web
                    } else if fails_closed_without_cookie {
                        // The provider owns this policy; Web with no header means
                        // it fails closed instead of importing a browser account
                        // the user did not select.
                        SourceMode::Web
                    } else if provider_uses_oauth_without_cookies(id, usage_source) {
                        SourceMode::OAuth
                    } else {
                        SourceMode::Cli
                    };
                    (source_mode, cookie_header, fails_closed_without_cookie)
                }
                // `browser` is accepted as a legacy alias from older settings.
                "auto" | "browser" | "web" => {
                    // Claude resolves its cached cookie and browser fallback inside
                    // the provider; other providers retain the shell fallback.
                    let cookie_header =
                        active_token_cookie.clone().or(stored_cookie).or_else(|| {
                            if defer_provider_browser_cookie_lookup {
                                None
                            } else {
                                provider_cookie_domain(id, settings).and_then(|domain| {
                                    codexbar::browser::cookies::get_cookie_header(domain)
                                        .ok()
                                        .filter(|h| !h.is_empty())
                                })
                            }
                        });
                    (usage_source, cookie_header, false)
                }
                _ => (usage_source, stored_cookie, false),
            }
        }
    } else if cookies_only_enrich_usage {
        let cookie_header = (cookie_source == "manual")
            .then(|| active_token_cookie.clone().or(stored_cookie))
            .flatten();
        (usage_source, cookie_header, false)
    } else {
        match cookie_source {
            // #433: an explicitly selected, non-empty Claude manual cookie is
            // authoritative. Do not let an active OAuth token account silently
            // replace it; this keeps tray refresh behavior aligned with diagnose,
            // whose Claude Auto path tries the supplied Web cookie before OAuth.
            "manual"
                if provider.manual_cookie_precedes_token_account()
                    && stored_cookie
                        .as_deref()
                        .is_some_and(|cookie| !cookie.trim().is_empty()) =>
            {
                (SourceMode::Web, stored_cookie.clone(), false)
            }
            _ if active_token_env.is_some() => (SourceMode::OAuth, None, false),
            // Opt-in web providers keep their default credential lane
            // unless the usage source is explicitly Web; a stored or
            // browser cookie must not turn Auto into Web.
            _ if provider.web_is_opt_in() && usage_source != SourceMode::Web => {
                (usage_source, None, false)
            }
            // Charm Hyper: the cookie source only picks the session, and
            // the usage source keeps routing. Off and an empty Manual
            // source never import a browser session, while Auto keeps its
            // API-key fallback.
            "off" | "manual" if provider.cookie_source_scopes_session_only() => {
                let cookie_header = if cookie_source == "manual" {
                    active_token_cookie.clone().or(stored_cookie)
                } else {
                    None
                };
                let source_mode = if provider.available_sources().contains(&usage_source) {
                    usage_source
                } else {
                    SourceMode::Auto
                };
                let cookie_missing = cookie_header.is_none();
                (source_mode, cookie_header, cookie_missing)
            }
            "off" if provider_uses_oauth_without_cookies(id, usage_source) => {
                (SourceMode::OAuth, None, false)
            }
            "off"
                if (has_kimi_code_api_key || has_opencodego_api_key)
                    && usage_source == SourceMode::Auto =>
            {
                (SourceMode::Auto, None, false)
            }
            // Droid/Factory: cookie-off must never scrape browser cookies. Map to
            // Cli (API-only in the provider) so Auto does not fall through to web.
            "off" if id == ProviderId::Factory => (SourceMode::Cli, None, false),
            "off" => (SourceMode::Cli, None, false),
            "manual" => {
                let cookie_header = active_token_cookie.clone().or(stored_cookie);
                let fails_closed_without_cookie = cookie_header
                    .as_deref()
                    .is_none_or(|header| header.trim().is_empty())
                    && provider.manual_empty_cookie_policy()
                        == ManualEmptyCookiePolicy::FailClosedWeb;
                let source_mode = if (has_kimi_code_api_key || has_opencodego_api_key)
                    && usage_source == SourceMode::Auto
                {
                    SourceMode::Auto
                } else if let Some(mode) = grok_source_mode_for_manual_cookie(id, usage_source) {
                    // Grok Switch writes ~/.grok/auth.json. Leftover grok.com
                    // cookies must not force Web, or Weekly/notifications keep
                    // showing the previous browser account.
                    mode
                } else if cookie_header.is_some() {
                    SourceMode::Web
                } else if fails_closed_without_cookie {
                    // The provider owns this policy; Web with no header means
                    // it fails closed instead of importing a browser account
                    // the user did not select.
                    SourceMode::Web
                } else if provider_uses_oauth_without_cookies(id, usage_source) {
                    SourceMode::OAuth
                } else {
                    SourceMode::Cli
                };
                (source_mode, cookie_header, fails_closed_without_cookie)
            }
            // `browser` is accepted as a legacy alias from older settings.
            "auto" | "browser" | "web" => {
                // Claude resolves its cached cookie and browser fallback inside
                // the provider; other providers retain the shell fallback.
                let cookie_header = active_token_cookie.clone().or(stored_cookie).or_else(|| {
                    if defer_provider_browser_cookie_lookup {
                        None
                    } else {
                        provider_cookie_domain(id, settings).and_then(|domain| {
                            codexbar::browser::cookies::get_cookie_header(domain)
                                .ok()
                                .filter(|h| !h.is_empty())
                        })
                    }
                });
                (usage_source, cookie_header, false)
            }
            _ => (usage_source, stored_cookie, false),
        }
    };

    // Cookie-web providers (Cursor, OpenCode, …) reject SourceMode::Cli. The shell
    // historically mapped "manual + no cookie" to Cli, which surfaces as
    // "Source mode 'Cli' not supported". Remap to Web and try browser cookies
    // unless the user explicitly disabled cookies ("off"). Providers whose
    // cookie source only scopes the session (Charm Hyper) or only enriches an
    // API result (Muse browser team quota) own this contract in the provider,
    // so the shell must not remap their source mode.
    if source_mode == SourceMode::Cli
        && cookie_source != "off"
        && !provider.supports_cli()
        && !provider.cookie_source_scopes_session_only()
        && !cookies_only_enrich_usage
    {
        if cookie_header
            .as_deref()
            .map(str::trim)
            .is_none_or(|s| s.is_empty())
        {
            cookie_header = provider_cookie_domain(id, settings).and_then(|domain| {
                codexbar::browser::cookies::get_cookie_header(domain)
                    .ok()
                    .filter(|h| !h.is_empty())
            });
        }
        source_mode = SourceMode::Web;
    }

    // The inverse case: some providers never fetch over the web and reject
    // SourceMode::Web outright (Codex since the 0.54 port only does OAuth/PAT/CLI).
    // The default cookie source is "manual", so a pasted chatgpt.com cookie flipped
    // Codex into Web and every refresh failed with
    // "Source mode 'Web' not supported for this provider". Fall back to the
    // configured usage source (or Auto) instead of handing the provider a mode it
    // advertises as unsupported.
    let available_sources = provider.available_sources();
    if source_mode == SourceMode::Web && !available_sources.contains(&SourceMode::Web) {
        source_mode = if available_sources.contains(&usage_source) {
            usage_source
        } else {
            SourceMode::Auto
        };
    }

    let workspace_id = settings.workspace_id(id).trim().to_string();
    let api_region = settings.api_region(id).trim().to_string();
    // Every gateway-style provider (Wayfinder, Bifrost, Aixy) stores its base
    // URL here; providers without one report an empty string.
    let gateway_url = Some(settings.gateway_url(id))
        .filter(|url| !url.is_empty())
        .map(str::to_owned);
    // Local-first Auto providers (OpenCode Go) flip to web-first when a
    // token account or manual cookie source scopes the session to web creds.
    let auto_prefer_web = token_override.is_some() || cookie_source == "manual";

    // These upstream account types are explicit identity selections. Keep the
    // provider's saved region/source settings intact, but project the selected
    // credential into the route required by that account.
    let (cookie_header, api_key) = match (id, token_account_kind, usage_source) {
        (ProviderId::Kimi, Some(_), _) => (active_token_cookie.clone(), None),
        (ProviderId::Doubao, Some(_), _) => (None, active_token_api_key.clone()),
        (
            ProviderId::OpenCodeGo,
            Some(codexbar::core::TokenAccountKind::ApiKey),
            SourceMode::Auto,
        ) => (None, active_token_api_key.clone()),
        (ProviderId::OpenCodeGo, Some(codexbar::core::TokenAccountKind::ApiKey), _) => {
            (cookie_header, api_key)
        }
        (
            ProviderId::OpenCodeGo,
            Some(codexbar::core::TokenAccountKind::Cookie),
            SourceMode::Auto,
        ) => (active_token_cookie.clone(), api_key),
        _ => (cookie_header, api_key),
    };
    let source_mode = token_override
        .as_ref()
        .and_then(|account| account.effective_source_mode(usage_source))
        .unwrap_or(source_mode);
    let token_account_isolated = token_override.is_some()
        && matches!(
            id,
            ProviderId::Kimi | ProviderId::Doubao | ProviderId::OpenCodeGo
        );

    FetchContext {
        source_mode,
        manual_cookie_header: cookie_header,
        manual_cookie_missing: fails_closed_without_cookie,
        api_key,
        token_account_kind,
        token_account_isolated,
        workspace_id: (!workspace_id.is_empty()).then_some(workspace_id),
        seat_credit_entitlement: settings.seat_credit_entitlement(id),
        api_region: (!api_region.is_empty()).then_some(api_region),
        gateway_url,
        auto_prefer_web: auto_prefer_web
            && !(id == ProviderId::OpenCodeGo
                && token_account_kind == Some(codexbar::core::TokenAccountKind::ApiKey)),
        browser_cookie_import,
        optional_details_enabled: settings.optional_details_enabled(id),
        ..FetchContext::default()
    }
}

fn provider_uses_oauth_without_cookies(id: ProviderId, usage_source: SourceMode) -> bool {
    match id {
        ProviderId::Claude => usage_source != SourceMode::Cli,
        ProviderId::Grok => matches!(usage_source, SourceMode::Auto | SourceMode::OAuth),
        _ => false,
    }
}

/// Keep Grok Auto/OAuth/Cli on the switched login instead of rewriting a
/// leftover manual cookie as Web. Auto remains Auto even when its manual
/// cookie is empty; the provider uses that context to skip browser refresh.
fn grok_source_mode_for_manual_cookie(
    id: ProviderId,
    usage_source: SourceMode,
) -> Option<SourceMode> {
    if id != ProviderId::Grok {
        return None;
    }
    match usage_source {
        SourceMode::Cli => Some(SourceMode::Cli),
        SourceMode::Auto => Some(SourceMode::Auto),
        SourceMode::OAuth => Some(SourceMode::OAuth),
        _ => None,
    }
}

pub(crate) fn provider_cookie_domain(id: ProviderId, settings: &Settings) -> Option<&'static str> {
    if id == ProviderId::MiniMax {
        return Some(
            codexbar::providers::MiniMaxProvider::cookie_domain_for_region(Some(
                settings.api_region(id),
            )),
        );
    }
    if id == ProviderId::Alibaba {
        return Some(
            codexbar::providers::AlibabaProvider::cookie_domain_for_region(Some(
                settings.api_region(id),
            )),
        );
    }
    id.cookie_domain()
}

const DEFAULT_PROVIDER_FETCH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(35);
const SLOW_PROVIDER_FETCH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(75);
const OPTIONAL_LITELLM_FETCH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(40);
const MAX_CONTEXT_FETCH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(65);

pub(crate) fn provider_fetch_timeout(id: ProviderId, ctx: &FetchContext) -> std::time::Duration {
    let provider_timeout = match id {
        ProviderId::Claude | ProviderId::Codex | ProviderId::Copilot => SLOW_PROVIDER_FETCH_TIMEOUT,
        ProviderId::LiteLLM if ctx.optional_details_enabled => OPTIONAL_LITELLM_FETCH_TIMEOUT,
        _ => DEFAULT_PROVIDER_FETCH_TIMEOUT,
    };
    let context_timeout = std::time::Duration::from_secs(ctx.web_timeout.saturating_add(5));
    provider_timeout.max(context_timeout.min(MAX_CONTEXT_FETCH_TIMEOUT))
}

pub(crate) fn upsert_provider_cache(
    cache: &mut Vec<ProviderUsageSnapshot>,
    snapshot: ProviderUsageSnapshot,
) {
    if let Some(existing) = cache
        .iter_mut()
        .find(|existing| existing.provider_id == snapshot.provider_id)
    {
        *existing = snapshot;
    } else {
        cache.push(snapshot);
    }
}

/// Drop cached snapshots for providers that are no longer enabled.
pub(crate) fn prune_provider_cache_to_enabled(
    cache: &mut Vec<ProviderUsageSnapshot>,
    enabled_ids: &[ProviderId],
) {
    cache.retain(|snapshot| {
        enabled_ids
            .iter()
            .any(|id| id.cli_name() == snapshot.provider_id)
    });
}

/// Invalidate in-flight publish work and remove disabled providers from cache.
///
/// Also clears the refresh lock so a follow-up force refresh can start immediately
/// (otherwise `begin_provider_refresh` no-ops while a superseded batch still holds
/// `is_refreshing`, and newly enabled providers never get a replacement run).
pub(crate) fn invalidate_provider_refresh_and_prune_disabled(
    state: &tauri::State<'_, Mutex<AppState>>,
    enabled_ids: &[ProviderId],
) -> Result<(), String> {
    let mut guard = state.lock().map_err(|e| e.to_string())?;
    guard.provider_refresh_generation = guard.provider_refresh_generation.wrapping_add(1);
    guard.is_refreshing = false;
    guard.provider_refresh_started_at = None;
    prune_provider_cache_to_enabled(&mut guard.provider_cache, enabled_ids);
    // Drop transient-failure counters for providers that left the enabled set.
    guard
        .transient_provider_failure_counts
        .retain(|id, _| enabled_ids.contains(id));
    guard
        .last_good_owners
        .retain(|id, _| enabled_ids.contains(id));
    guard
        .provider_cache_updated_at_by_provider
        .retain(|id, _| enabled_ids.contains(id));
    guard.auto_resume.clear_disabled(enabled_ids);
    Ok(())
}

pub(crate) fn is_current_provider_refresh_generation(guard: &AppState, generation: u64) -> bool {
    guard.provider_refresh_generation == generation
}

/// Core refresh logic, usable from both the Tauri command and tray menu actions.
pub(crate) async fn do_refresh_providers(app: &tauri::AppHandle) -> Result<(), String> {
    do_refresh_providers_with_outcome(app).await.map(|_| ())
}

pub(crate) async fn do_refresh_providers_with_outcome(
    app: &tauri::AppHandle,
) -> Result<ProviderRefreshOutcome, String> {
    do_refresh_providers_with_policy(app, true, RefreshScope::AllEnabled).await
}

pub(crate) async fn do_refresh_providers_if_stale(app: &tauri::AppHandle) -> Result<(), String> {
    do_refresh_providers_with_policy(app, false, RefreshScope::AllEnabled)
        .await
        .map(|_| ())
}

/// Refresh only enabled providers with the opt-in exact-session watcher. This
/// keeps a 60-second resume probe from shortening or repeating unrelated
/// provider work.
pub(crate) async fn do_refresh_auto_resume_providers_if_stale(
    app: &tauri::AppHandle,
) -> Result<(), String> {
    do_refresh_providers_with_policy(app, false, RefreshScope::AutoResume)
        .await
        .map(|_| ())
}

async fn do_refresh_providers_with_policy(
    app: &tauri::AppHandle,
    force: bool,
    scope: RefreshScope,
) -> Result<ProviderRefreshOutcome, String> {
    let state = app.state::<Mutex<AppState>>();
    let expected_generation = state
        .lock()
        .map_err(|e| e.to_string())?
        .provider_refresh_generation;
    let settings = Settings::load();
    let enabled_ids = settings.get_enabled_provider_ids();
    let refresh_ids = scope.provider_ids(&settings, &enabled_ids);
    if let Ok(mut guard) = state.lock() {
        guard
            .notification_manager
            .retire_credential_episodes_except(&enabled_ids);
    }
    if refresh_ids.is_empty() {
        return Ok(ProviderRefreshOutcome::Skipped {
            reason: ProviderRefreshSkipReason::NoEnabledProviders,
        });
    }

    let inputs = ProviderRefreshInputs::load(settings, enabled_ids);
    let generation = match begin_provider_refresh(&state, force, &refresh_ids, expected_generation)?
    {
        ProviderRefreshReservation::Reserved { generation } => generation,
        ProviderRefreshReservation::Skipped(reason) => {
            // Settings or account identity changed while inputs were loading,
            // another batch owns the refresh, or the cache is already fresh.
            return Ok(ProviderRefreshOutcome::Skipped { reason });
        }
    };

    // Ensure cache only contains currently enabled providers for this generation.
    if scope == RefreshScope::AllEnabled
        && let Ok(mut guard) = state.lock()
        && is_current_provider_refresh_generation(&guard, generation)
    {
        prune_provider_cache_to_enabled(&mut guard.provider_cache, &inputs.enabled_ids);
    }

    events::emit_refresh_started(
        app,
        refresh_ids
            .iter()
            .map(|id| id.cli_name().to_string())
            .collect(),
    );
    let enabled_count = refresh_ids.len();

    let handles = spawn_provider_refreshes(
        app,
        &inputs,
        &refresh_ids,
        generation,
        scope.refresh_account_lanes(),
    );
    await_provider_refreshes(handles).await;

    let error_count = match finish_provider_refresh(&state, generation)? {
        ProviderRefreshCompletion::Published { error_count } => error_count,
        ProviderRefreshCompletion::Superseded { current_generation } => {
            // Superseded by a newer generation (or invalidate). Do not clear UI
            // "refreshing" for dead work or stamp tray from incomplete work.
            return Ok(ProviderRefreshOutcome::Superseded {
                generation,
                current_generation,
            });
        }
    };
    update_tray_and_notifications(app, &state, &inputs.settings, &inputs.token_accounts)?;

    events::emit_refresh_complete(app, enabled_count, error_count);
    crate::auto_refresh::schedule_refresh_enrichment(&inputs.settings);

    Ok(ProviderRefreshOutcome::Published { generation })
}

fn begin_provider_refresh(
    state: &tauri::State<'_, Mutex<AppState>>,
    force: bool,
    provider_ids: &[ProviderId],
    expected_generation: u64,
) -> Result<ProviderRefreshReservation, String> {
    let mut guard = state.lock().map_err(|e| e.to_string())?;
    Ok(reserve_provider_refresh(
        &mut guard,
        force,
        provider_ids,
        expected_generation,
    ))
}

struct ProviderRefreshInputs {
    settings: Settings,
    enabled_ids: Vec<ProviderId>,
    manual_cookies: ManualCookies,
    api_keys: ApiKeys,
    token_accounts: HashMap<ProviderId, ProviderAccountData>,
}

impl ProviderRefreshInputs {
    fn load(settings: Settings, enabled_ids: Vec<ProviderId>) -> Self {
        let manual_cookies = ManualCookies::load();
        let api_keys = ApiKeys::load();
        let token_accounts = TokenAccountStore::new().load().unwrap_or_else(|e| {
            tracing::warn!("failed to load token accounts for provider refresh: {e}");
            HashMap::new()
        });

        Self {
            settings,
            enabled_ids,
            manual_cookies,
            api_keys,
            token_accounts,
        }
    }
}

fn spawn_provider_refreshes(
    app: &tauri::AppHandle,
    inputs: &ProviderRefreshInputs,
    provider_ids: &[ProviderId],
    generation: u64,
    refresh_account_lanes: bool,
) -> Vec<tokio::task::JoinHandle<()>> {
    let mut handles = Vec::with_capacity(provider_ids.len());
    let fetch_permits = Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_PROVIDER_FETCHES));

    for id in provider_ids {
        let id = *id;
        let app_handle = app.clone();
        let fetch_permits = Arc::clone(&fetch_permits);
        let ctx = build_fetch_context(
            id,
            &inputs.settings,
            &inputs.manual_cookies,
            &inputs.api_keys,
            &inputs.token_accounts,
        );
        // Resolved here rather than inside the fetch: forecast history is keyed by
        // account, and the managed-account id is the only discriminator Codex exposes.
        let token_account_id = inputs
            .token_accounts
            .get(&id)
            .and_then(ProviderAccountData::active_account)
            .map(|account| account.id);
        let hooks_enabled = inputs.settings.hooks_enabled;

        handles.push(tokio::spawn(async move {
            let Ok(_permit) = fetch_permits.acquire_owned().await else {
                return;
            };
            refresh_provider(
                app_handle,
                id,
                ctx,
                generation,
                token_account_id,
                hooks_enabled,
            )
            .await;
        }));
    }

    // ADR 0003 multi-account lanes: when Codex is enabled, refresh every
    // account snapshot (ambient + managed) on the same cycle, bounded by the
    // shared fetch semaphore. The ambient account still publishes the single
    // "codex" provider snapshot used by tray/menu; the lanes fill the account
    // snapshot store consumed by the Settings accounts panel.
    if refresh_account_lanes && provider_ids.contains(&ProviderId::Codex) {
        let app_handle = app.clone();
        let fetch_permits = Arc::clone(&fetch_permits);
        handles.push(tokio::spawn(async move {
            super::codex_accounts::refresh_codex_account_lanes(
                app_handle,
                fetch_permits,
                generation,
            )
            .await;
        }));
    }

    if refresh_account_lanes && provider_ids.contains(&ProviderId::Claude) {
        let app_handle = app.clone();
        let permits = Arc::clone(&fetch_permits);
        handles.push(tokio::spawn(async move {
            super::refresh_claude_account_lanes(app_handle, permits, generation).await;
        }));
    }

    handles
}

async fn refresh_provider(
    app: tauri::AppHandle,
    id: ProviderId,
    ctx: FetchContext,
    generation: u64,
    token_account_id: Option<uuid::Uuid>,
    hooks_enabled: bool,
) {
    let (snapshot, account_identity, retention) =
        fetch_provider_snapshot(id, ctx, token_account_id).await;
    let fresh_snapshot = snapshot.error.is_none();
    // Captured before last-good preservation can swap in a cached snapshot.
    let fetch_attempt = FetchAttempt::of(&snapshot);

    let state = app.state::<Mutex<AppState>>();
    let published = if let Ok(mut guard) = state.lock() {
        if !is_current_provider_refresh_generation(&guard, generation) {
            tracing::debug!(
                provider = id.cli_name(),
                generation,
                current = guard.provider_refresh_generation,
                "dropping superseded provider refresh result"
            );
            None
        } else {
            let snapshot = preserve_last_good_transient_failure_with_policy(
                &mut guard,
                id,
                snapshot,
                retention.policy,
                &retention.failure_ownership,
            );
            if fresh_snapshot {
                record_last_good_owner(&mut guard, id, retention.fresh_owner);
            }
            // F6 (upstream 0.48.0): backfill missing reset timestamps from the
            // cached snapshot before persisting and publishing.
            let cached = guard
                .provider_cache
                .iter()
                .find(|c| c.provider_id == snapshot.provider_id && c.error.is_none())
                .cloned();
            let mut snapshot = snapshot;
            reset_backfill::codex_reset_backfill(&mut snapshot, cached.as_ref());
            upsert_provider_cache(&mut guard.provider_cache, snapshot.clone());
            if fresh_snapshot {
                guard
                    .provider_cache_updated_at_by_provider
                    .insert(id, std::time::Instant::now());
            }
            // Read consent after the fetch completes, so a toggle change made
            // while the request was in flight takes effect for this outcome.
            let credential_alerts = match fetch_attempt {
                FetchAttempt::Failed(kind) if kind.needs_sign_in() => Some(
                    codexbar::notifications::CredentialAlertPolicy::from_settings(&Settings::load()),
                ),
                _ => None,
            };
            credential_alerts::observe_attempt(
                &mut guard.notification_manager,
                credential_alerts,
                id,
                token_account_id,
                fetch_attempt,
                &snapshot,
                cached.as_ref(),
            );
            Some(snapshot)
        }
    } else {
        None
    };

    if let Some(snapshot) = published {
        events::emit_provider_updated(&app, &snapshot);
        if fresh_snapshot {
            dispatch_usage_updated_hook(
                hooks_enabled,
                id,
                &snapshot,
                account_identity.as_deref(),
                token_account_id,
            );
            crate::auto_resume::observe_fresh_snapshot(
                &app,
                id,
                &snapshot,
                token_account_id,
                account_identity.as_deref(),
            )
            .await;
        }
    }
}

/// Publish the current successful snapshot to opt-in external hooks. The event
/// carries both quota windows in one payload and is rate-limited per provider
/// account so periodic refreshes cannot create a hook storm.
fn dispatch_usage_updated_hook(
    hooks_enabled: bool,
    provider: ProviderId,
    snapshot: &ProviderUsageSnapshot,
    account_identity: Option<&str>,
    token_account_id: Option<uuid::Uuid>,
) {
    if !hooks_enabled {
        return;
    }
    let settings = Settings::load();
    let account = if settings.hide_personal_info {
        None
    } else {
        account_identity
            .map(str::trim)
            .filter(|identity| !identity.is_empty())
    };
    let rate_limit_scope = quota_notification_account_identity(snapshot, token_account_id);
    let rate_limit_scope = if rate_limit_scope.is_empty() {
        None
    } else {
        Some(format!("provider-account:{rate_limit_scope}"))
    };
    let primary = HookUsageWindow {
        used_percent: snapshot.primary.used_percent,
        window_minutes: snapshot.primary.window_minutes,
        resets_at: snapshot.primary.resets_at.clone(),
        is_informational: snapshot.primary.is_informational,
    };
    let secondary = snapshot.secondary.as_ref().map(|window| HookUsageWindow {
        used_percent: window.used_percent,
        window_minutes: window.window_minutes,
        resets_at: window.resets_at.clone(),
        is_informational: window.is_informational,
    });
    codexbar::core::dispatch_usage_updated_hook(
        hooks_enabled,
        provider.cli_name(),
        &primary,
        secondary.as_ref(),
        account,
        rate_limit_scope,
    );
}

#[cfg(test)]
pub(super) fn preserve_last_good_transient_failure(
    guard: &mut AppState,
    id: ProviderId,
    snapshot: ProviderUsageSnapshot,
    error: &codexbar::core::ProviderError,
) -> ProviderUsageSnapshot {
    let policy = instantiate_provider(id).last_good_failure_policy_for_error(error);
    preserve_last_good_transient_failure_with_policy(
        guard,
        id,
        snapshot,
        Some(policy),
        &error.failure_ownership(),
    )
}

/// Remember which live session produced the fresh snapshot now cached for
/// `id`, or forget any earlier owner when the fresh snapshot has none.
pub(super) fn record_last_good_owner(
    guard: &mut AppState,
    id: ProviderId,
    owner: Option<codexbar::core::LastGoodOwner>,
) {
    match owner {
        Some(owner) => {
            guard.last_good_owners.insert(id, owner);
        }
        None => {
            guard.last_good_owners.remove(&id);
        }
    }
}

fn preserve_last_good_transient_failure_with_policy(
    guard: &mut AppState,
    id: ProviderId,
    snapshot: ProviderUsageSnapshot,
    policy: Option<codexbar::core::LastGoodFailurePolicy>,
    ownership: &codexbar::core::FailureOwnership,
) -> ProviderUsageSnapshot {
    let Some(error) = snapshot.error.as_deref() else {
        guard.transient_provider_failure_counts.remove(&id);
        return snapshot;
    };

    let policy = policy.unwrap_or(codexbar::core::LastGoodFailurePolicy::Replace);
    if policy == codexbar::core::LastGoodFailurePolicy::Replace {
        guard.transient_provider_failure_counts.remove(&id);
        return snapshot;
    }
    // A failure tied to a live session may keep only a snapshot that the same
    // session produced. Anything else shows the error.
    if !ownership.allows_retention(guard.last_good_owners.get(&id)) {
        guard.transient_provider_failure_counts.remove(&id);
        return snapshot;
    }

    let Some(mut previous) = guard
        .provider_cache
        .iter()
        .find(|cached| cached.provider_id == id.cli_name() && cached.error.is_none())
        .cloned()
    else {
        return snapshot;
    };
    // Preserved quota remains useful for display, but the failed current
    // attempt cannot prove that Claude CLI is available for account actions.
    previous.has_successful_claude_cli_quota = false;

    let count = guard
        .transient_provider_failure_counts
        .entry(id)
        .or_insert(0);
    match policy {
        codexbar::core::LastGoodFailurePolicy::Preserve => {
            tracing::warn!(
                provider = id.cli_name(),
                error,
                "preserving last good provider snapshot after transient failure"
            );
            previous
        }
        codexbar::core::LastGoodFailurePolicy::PreserveOnce if *count == 0 => {
            *count = 1;
            tracing::warn!(
                provider = id.cli_name(),
                error,
                "preserving last good provider snapshot after transient failure"
            );
            previous
        }
        codexbar::core::LastGoodFailurePolicy::PreserveOnce => {
            *count = count.saturating_add(1);
            snapshot
        }
        codexbar::core::LastGoodFailurePolicy::PreserveOnceThenSurface if *count == 0 => {
            *count = 1;
            tracing::warn!(
                provider = id.cli_name(),
                error,
                "preserving last good provider snapshot after transient failure"
            );
            previous
        }
        codexbar::core::LastGoodFailurePolicy::PreserveOnceThenSurface => {
            *count = count.saturating_add(1);
            let mut surfaced = previous;
            surfaced.error = snapshot.error;
            surfaced.error_state = snapshot.error_state;
            surfaced.fetch_duration_ms = snapshot.fetch_duration_ms;
            surfaced
        }
        codexbar::core::LastGoodFailurePolicy::Replace => snapshot,
    }
}

/// What a refresh tells the shell about keeping or replacing the last good
/// snapshot.
#[derive(Default)]
struct RefreshRetention {
    /// Failure handling when a prior good snapshot exists.
    policy: Option<codexbar::core::LastGoodFailurePolicy>,
    /// Session the failed request belonged to, when the provider can tell.
    failure_ownership: codexbar::core::FailureOwnership,
    /// Session that produced a fresh snapshot.
    fresh_owner: Option<codexbar::core::LastGoodOwner>,
}

async fn fetch_provider_snapshot(
    id: ProviderId,
    ctx: FetchContext,
    token_account_id: Option<uuid::Uuid>,
) -> (ProviderUsageSnapshot, Option<String>, RefreshRetention) {
    let provider = instantiate_provider(id);
    let metadata = provider.metadata().clone();
    let started = std::time::Instant::now();

    let (mut snapshot, account_identity, retention) =
        match tokio::time::timeout(provider_fetch_timeout(id, &ctx), provider.fetch_usage(&ctx))
            .await
        {
            Ok(Ok(result)) => {
                let account_identity = result.account_identity().map(ToOwned::to_owned);
                (
                    ProviderUsageSnapshot::from_fetch_result(
                        id,
                        &metadata,
                        &result,
                        token_account_id,
                    ),
                    account_identity,
                    RefreshRetention {
                        fresh_owner: result.last_good_owner.clone(),
                        ..RefreshRetention::default()
                    },
                )
            }
            Ok(Err(e)) => {
                let policy = provider.last_good_failure_policy_for_error(&e);
                (
                    ProviderUsageSnapshot::from_error(
                        id,
                        &metadata,
                        codexbar::logging::safe_error_message(&e),
                        provider.error_state_kind(&e),
                    ),
                    None,
                    RefreshRetention {
                        policy: Some(policy),
                        failure_ownership: e.failure_ownership(),
                        fresh_owner: None,
                    },
                )
            }
            Err(_) => {
                let error = codexbar::core::ProviderError::Timeout;
                let policy = provider.last_good_failure_policy_for_error(&error);
                (
                    ProviderUsageSnapshot::from_error(
                        id,
                        &metadata,
                        "Timeout".to_string(),
                        provider.error_state_kind(&error),
                    ),
                    None,
                    RefreshRetention {
                        policy: Some(policy),
                        failure_ownership: error.failure_ownership(),
                        fresh_owner: None,
                    },
                )
            }
        };

    record_provider_fetch_duration(id, &mut snapshot, started);
    (snapshot, account_identity, retention)
}

fn record_provider_fetch_duration(
    id: ProviderId,
    snapshot: &mut ProviderUsageSnapshot,
    started: std::time::Instant,
) {
    let fetch_duration_ms = started.elapsed().as_millis();
    snapshot.fetch_duration_ms = Some(fetch_duration_ms);
    if fetch_duration_ms > 5_000 {
        tracing::warn!(
            provider = id.cli_name(),
            fetch_duration_ms,
            "slow provider refresh"
        );
    }
}

async fn await_provider_refreshes(handles: Vec<tokio::task::JoinHandle<()>>) {
    for handle in handles {
        let _ = handle.await;
    }
}

/// Finish a refresh batch. Returns `None` when `generation` was superseded
/// (do not emit complete / tray updates for dead work). Returns `Some(error_count)`
/// when this batch still owns the generation and the lock was released.
fn finish_provider_refresh(
    state: &tauri::State<'_, Mutex<AppState>>,
    generation: u64,
) -> Result<ProviderRefreshCompletion, String> {
    let mut guard = state.lock().map_err(|e| e.to_string())?;
    Ok(complete_provider_refresh(&mut guard, generation))
}

fn update_tray_and_notifications(
    app: &tauri::AppHandle,
    state: &tauri::State<'_, Mutex<AppState>>,
    settings: &Settings,
    token_accounts: &HashMap<ProviderId, ProviderAccountData>,
) -> Result<(), String> {
    let cached = {
        let guard = state.lock().map_err(|e| e.to_string())?;
        guard.provider_cache.clone()
    };
    crate::tray_bridge::update_tray_status_items(app, &cached);
    crate::tray_bridge::update_tray_icon_and_tooltip(app, &cached);
    notify_usage_thresholds(state, settings, token_accounts, &cached);
    Ok(())
}

fn notify_usage_thresholds(
    state: &tauri::State<'_, Mutex<AppState>>,
    settings: &Settings,
    token_accounts: &HashMap<ProviderId, ProviderAccountData>,
    cached: &[ProviderUsageSnapshot],
) {
    let cli_map = codexbar::core::cli_name_map();
    if let Ok(mut guard) = state.lock() {
        for snapshot in cached {
            if snapshot.error.is_none()
                && let Some(&provider) = cli_map.get(snapshot.provider_id.as_str())
            {
                let token_account_id = token_accounts
                    .get(&provider)
                    .and_then(ProviderAccountData::active_account)
                    .map(|account| account.id);
                let warning_identity = WarningIdentity::new(
                    provider,
                    &snapshot.source_label,
                    snapshot.account_email.as_deref(),
                    snapshot.account_organization.as_deref(),
                    token_account_id,
                );
                // Hooks keep their own per-source baselines (edge-triggered, first sample
                // never fires), so they stay on the source key; only toast dedupe below
                // bridges account-identity gaps.
                let account = warning_identity.threshold_key();
                let scope = warning_identity.gap_scope();
                // Skip all session consumers for synthetic/no-session
                // placeholders (e.g. Claude OAuth five_hour: null).
                let session_account = resolve_toast_account(
                    &mut guard.notification_manager,
                    provider,
                    &scope,
                    "session",
                    &snapshot.primary,
                    settings,
                );
                if guard.notification_manager.check_session_lane(
                    provider,
                    &session_account,
                    snapshot.primary.used_percent,
                    snapshot.primary.is_informational,
                    settings,
                ) {
                    dispatch_quota_hooks(
                        settings,
                        provider,
                        &account,
                        "session",
                        snapshot.primary.used_percent,
                    );
                }
                if let Some(weekly) = &snapshot.secondary
                    && !weekly.is_informational
                {
                    let weekly_account = resolve_toast_account(
                        &mut guard.notification_manager,
                        provider,
                        &scope,
                        "weekly",
                        weekly,
                        settings,
                    );
                    guard.notification_manager.check_and_notify(
                        provider,
                        &weekly_account,
                        "weekly",
                        weekly.used_percent,
                        settings,
                    );
                    dispatch_quota_hooks(
                        settings,
                        provider,
                        &account,
                        "weekly",
                        weekly.used_percent,
                    );
                }
                notify_predictive_pace(
                    &mut guard.notification_manager,
                    provider,
                    snapshot,
                    token_accounts,
                    settings,
                );
            }
        }
    }
}

/// Account key a toast lane is deduped under (see `NotificationManager::resolve_warning_account`).
/// Informational placeholders are not observed, matching `check_session_lane`.
fn resolve_toast_account(
    manager: &mut codexbar::notifications::NotificationManager,
    provider: ProviderId,
    scope: &WarningScope,
    window: &str,
    lane: &RateWindowSnapshot,
    settings: &Settings,
) -> String {
    if lane.is_informational {
        return scope.key().to_string();
    }
    let resets_at = lane
        .resets_at
        .as_deref()
        .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
        .map(|date| date.with_timezone(&chrono::Utc));
    manager.resolve_warning_account(
        provider,
        scope,
        window,
        lane.used_percent,
        resets_at,
        settings,
    )
}

fn dispatch_quota_hooks(
    settings: &Settings,
    provider: ProviderId,
    account: &str,
    window: &str,
    used_percent: f64,
) {
    if !settings.hooks_enabled {
        return;
    }
    let thresholds = settings.usage_thresholds(provider, window);
    let account = if settings.hide_personal_info || account.is_empty() {
        None
    } else {
        Some(account)
    };
    codexbar::core::emit_quota_threshold_hooks(
        true,
        provider.cli_name(),
        window,
        used_percent,
        thresholds.high,
        thresholds.critical,
        account,
    );
}

/// Stable account discriminator for threshold/session toast dedupe.
/// Prefer token-account id, then email, org, plan; empty for single-account lanes.
pub(super) fn quota_notification_account_identity(
    snapshot: &ProviderUsageSnapshot,
    token_account_id: Option<uuid::Uuid>,
) -> String {
    ProviderId::from_cli_name(&snapshot.provider_id)
        .map(|provider| {
            quota_notification_account_identity_for(
                provider,
                &snapshot.source_label,
                snapshot.account_email.as_deref(),
                snapshot.account_organization.as_deref(),
                token_account_id,
            )
        })
        .unwrap_or_default()
}

pub(super) fn quota_notification_account_identity_for(
    provider: ProviderId,
    source_label: &str,
    account_email: Option<&str>,
    account_organization: Option<&str>,
    token_account_id: Option<uuid::Uuid>,
) -> String {
    WarningIdentity::new(
        provider,
        source_label,
        account_email,
        account_organization,
        token_account_id,
    )
    .threshold_key()
}

fn notify_predictive_pace(
    manager: &mut codexbar::notifications::NotificationManager,
    provider: ProviderId,
    snapshot: &ProviderUsageSnapshot,
    token_accounts: &HashMap<ProviderId, ProviderAccountData>,
    settings: &Settings,
) {
    let enabled = settings.show_notifications && settings.predictive_pace_warning_enabled;
    manager.set_predictive_warnings_enabled(provider, enabled);
    if !enabled || !matches!(provider, ProviderId::Claude | ProviderId::Codex) {
        return;
    }

    let token_account_id = token_accounts
        .get(&provider)
        .and_then(ProviderAccountData::active_account)
        .map(|account| account.id);
    let warning_identity = WarningIdentity::new(
        provider,
        &snapshot.source_label,
        snapshot.account_email.as_deref(),
        None,
        token_account_id,
    );
    let Some(identity) = warning_identity.predictive_key() else {
        return;
    };
    let observed_at = chrono::DateTime::parse_from_rfc3339(&snapshot.updated_at)
        .ok()
        .map(|date| date.with_timezone(&chrono::Utc));

    for (warning_window, window, default_window_minutes) in [
        (
            codexbar::notifications::PredictiveWarningWindow::Session,
            Some(&snapshot.primary),
            300,
        ),
        (
            codexbar::notifications::PredictiveWarningWindow::Weekly,
            snapshot.secondary.as_ref(),
            10080,
        ),
    ] {
        let Some(window) = window else {
            continue;
        };
        if window.is_informational {
            continue;
        }
        let rate_window = RateWindow::with_details(
            window.used_percent,
            window.window_minutes,
            window
                .resets_at
                .as_deref()
                .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
                .map(|date| date.with_timezone(&chrono::Utc)),
            window.reset_description.clone(),
        );
        let Some(pace) =
            codexbar::core::UsagePace::weekly(&rate_window, observed_at, default_window_minutes)
        else {
            continue;
        };
        manager.check_predictive_pace(
            provider,
            &identity,
            warning_window,
            &rate_window,
            &pace,
            settings,
        );
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeepSeekPricingStatus {
    pub period: &'static str,
    pub current_local_time: String,
    pub next_transition_local_time: Option<String>,
    pub effective_local_time: String,
}

#[tauri::command]
pub fn get_deepseek_pricing_status(
    state: tauri::State<'_, Mutex<AppState>>,
) -> Option<DeepSeekPricingStatus> {
    let settings = Settings::load();
    if !settings.enabled_providers.contains("deepseek") {
        return None;
    }
    let now = Utc::now();
    let schedule = codexbar::providers::deepseek::pricing::status_at(now);
    let period = match schedule.period {
        codexbar::providers::deepseek::pricing::PricingPeriod::Standard => "standard",
        codexbar::providers::deepseek::pricing::PricingPeriod::Peak => "peak",
        codexbar::providers::deepseek::pricing::PricingPeriod::OffPeak => "offPeak",
    };
    if let Ok(mut app_state) = state.lock() {
        app_state
            .notification_manager
            .notify_pricing_transition(period, &settings);
    }
    let local = |instant: chrono::DateTime<Utc>| {
        instant
            .with_timezone(&Local)
            .format("%Y-%m-%d %H:%M:%S %Z")
            .to_string()
    };
    Some(DeepSeekPricingStatus {
        period,
        current_local_time: Local::now().format("%Y-%m-%d %H:%M:%S %Z").to_string(),
        next_transition_local_time: schedule.next_transition.map(local),
        effective_local_time: local(codexbar::providers::deepseek::pricing::EFFECTIVE_AT),
    })
}

#[tauri::command]
pub async fn refresh_providers(app: tauri::AppHandle) -> Result<(), String> {
    do_refresh_providers(&app).await
}

#[tauri::command]
pub async fn refresh_providers_if_stale(app: tauri::AppHandle) -> Result<(), String> {
    do_refresh_providers_if_stale(&app).await
}

#[tauri::command]
pub fn get_cached_providers(
    state: tauri::State<'_, Mutex<AppState>>,
) -> Vec<ProviderUsagePresentationSnapshot> {
    let snapshots = state
        .lock()
        .map(|guard| guard.provider_cache.clone())
        .unwrap_or_default();
    let settings = Settings::load();

    snapshots
        .into_iter()
        .map(|snapshot| ProviderUsagePresentationSnapshot::new(snapshot, &settings))
        .collect()
}

#[cfg(test)]
mod predictive_warning_tests {
    use super::*;

    fn empty_snapshot() -> ProviderUsageSnapshot {
        let metadata = codexbar::core::instantiate_provider(ProviderId::Claude)
            .metadata()
            .clone();
        ProviderUsageSnapshot::from_error(
            ProviderId::Claude,
            &metadata,
            "unused".to_string(),
            codexbar::core::ProviderStateKind::Unknown,
        )
    }

    #[test]
    fn quota_notification_account_identity_prefers_token_then_email() {
        let account_id = uuid::Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa").unwrap();
        let mut snapshot = empty_snapshot();
        snapshot.account_email = Some("Person@Example.com".to_string());
        snapshot.account_organization = Some("Acme Org".to_string());
        snapshot.plan_name = Some("Pro".to_string());

        assert_eq!(
            quota_notification_account_identity(&snapshot, Some(account_id)),
            "token-account:aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa"
        );
        assert_eq!(
            quota_notification_account_identity(&snapshot, None),
            "person@example.com"
        );

        snapshot.account_email = None;
        assert_eq!(
            quota_notification_account_identity(&snapshot, None),
            "org:acme org"
        );

        snapshot.account_organization = None;
        snapshot.source_label = "oauth".to_string();
        assert_eq!(
            quota_notification_account_identity(&snapshot, None),
            "claude:oauth:unknown"
        );

        snapshot.plan_name = None;
        snapshot.source_label = "cli (reduced fidelity)".to_string();
        assert_eq!(
            quota_notification_account_identity(&snapshot, None),
            "claude:cli:unknown"
        );
    }

    /// The forecast scope key and the notification identity must never disagree.
    /// If they did, one account would be seen as two identities and its burn history
    /// would be split, silently halving the sample count behind every forecast.
    #[test]
    fn forecast_account_key_matches_notification_identity() {
        use crate::commands::bridge::forecast_account_key;

        let token = uuid::Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa").unwrap();
        let mut usage = codexbar::core::UsageSnapshot::new(codexbar::core::RateWindow::new(1.0));
        let mut snapshot = empty_snapshot();

        for (email, org) in [
            (Some("Person@Example.com"), Some("Acme Org")),
            (Some("Person@Example.com"), None),
            (None, Some("Acme Org")),
            (None, None),
        ] {
            usage.account_email = email.map(str::to_string);
            usage.account_organization = org.map(str::to_string);
            snapshot.account_email = usage.account_email.clone();
            snapshot.account_organization = usage.account_organization.clone();

            for tok in [Some(token), None] {
                assert_eq!(
                    forecast_account_key(&usage, tok).unwrap_or_default(),
                    quota_notification_account_identity(&snapshot, tok),
                    "identity drift for email={email:?} org={org:?} token={tok:?}"
                );
            }
        }
    }
}
#[cfg(test)]
mod reset_backfill_tests {
    use super::*;
    use crate::commands::bridge::{ProviderUsageSnapshot, RateWindowSnapshot};
    fn win(used: f64, resets_at: Option<&str>) -> RateWindowSnapshot {
        RateWindowSnapshot {
            used_percent: used,
            remaining_percent: 100.0 - used,
            window_minutes: Some(300),
            resets_at: resets_at.map(String::from),
            reset_description: None,
            is_exhausted: false,
            is_informational: false,
            reserve_percent: None,
            reserve_description: None,
            reserve_will_last_to_reset: false,
            reserve_eta_seconds: None,
            description_is_detail: false,
            monthly_limit_block: None,
        }
    }
    fn codex_snapshot(primary: RateWindowSnapshot) -> ProviderUsageSnapshot {
        ProviderUsageSnapshot {
            provider_id: "codex".into(),
            display_name: "Codex".into(),
            primary,
            primary_label: None,
            secondary: None,
            secondary_label: None,
            model_specific: None,
            tertiary: None,
            tertiary_label: None,
            extra_rate_windows: Vec::new(),
            inventory: Vec::new(),
            display_details: Vec::new(),
            cost: None,
            plan_name: None,
            account_email: None,
            subscription: None,
            source_label: String::new(),
            has_successful_claude_cli_quota: false,
            updated_at: "2026-01-01T00:00:00Z".into(),
            error: None,
            error_state: codexbar::core::ProviderStateKind::Ready,
            pace: None,
            account_organization: None,
            tray_status_label: None,
            fetch_duration_ms: None,
            wayfinder_usage: None,
            open_ai_api_usage: None,
            session_equivalent_forecast: None,
            quota_burndown: None,
        }
    }
    #[test]
    fn f6_backfills_future_cached_reset() {
        // Cached has a future resets_at; fresh has none → backfilled.
        let future = (chrono::Utc::now() + chrono::Duration::hours(2)).to_rfc3339();
        let cached = codex_snapshot(win(50.0, Some(&future)));
        let mut fresh = codex_snapshot(win(30.0, None));
        reset_backfill::codex_reset_backfill(&mut fresh, Some(&cached));
        assert_eq!(fresh.primary.resets_at.as_deref(), Some(future.as_str()));
        // used_percent is NOT overwritten.
        assert!((fresh.primary.used_percent - 30.0).abs() < f64::EPSILON);
    }
    #[test]
    fn f6_does_not_backfill_stale_cached_reset() {
        // Cached reset is in the past → not backfilled.
        let past = (chrono::Utc::now() - chrono::Duration::hours(2)).to_rfc3339();
        let cached = codex_snapshot(win(50.0, Some(&past)));
        let mut fresh = codex_snapshot(win(30.0, None));
        reset_backfill::codex_reset_backfill(&mut fresh, Some(&cached));
        assert!(
            fresh.primary.resets_at.is_none(),
            "stale reset not backfilled"
        );
    }
    #[test]
    fn f6_does_not_overwrite_existing_resets_at() {
        // Fresh already has resets_at → cached not applied.
        let future1 = (chrono::Utc::now() + chrono::Duration::hours(3)).to_rfc3339();
        let future2 = (chrono::Utc::now() + chrono::Duration::hours(5)).to_rfc3339();
        let cached = codex_snapshot(win(50.0, Some(&future2)));
        let mut fresh = codex_snapshot(win(30.0, Some(&future1)));
        reset_backfill::codex_reset_backfill(&mut fresh, Some(&cached));
        assert_eq!(fresh.primary.resets_at.as_deref(), Some(future1.as_str()));
    }
    #[test]
    fn f6_skips_non_codex_provider() {
        let future = (chrono::Utc::now() + chrono::Duration::hours(2)).to_rfc3339();
        let mut cached = codex_snapshot(win(50.0, Some(&future)));
        cached.provider_id = "claude".into();
        let mut fresh = codex_snapshot(win(30.0, None));
        fresh.provider_id = "claude".into();
        reset_backfill::codex_reset_backfill(&mut fresh, Some(&cached));
        assert!(fresh.primary.resets_at.is_none(), "non-codex skip");
    }
    #[test]
    fn zai_five_hour_backfill_rejects_impossible_cached_reset() {
        for (offset, should_backfill) in [
            (chrono::Duration::hours(1), true),
            (chrono::Duration::hours(10), false),
        ] {
            let future = (chrono::Utc::now() + offset).to_rfc3339();
            let mut cached = codex_snapshot(win(50.0, Some(&future)));
            cached.provider_id = "zai".into();
            let mut fresh = codex_snapshot(win(30.0, None));
            fresh.provider_id = "zai".into();
            fresh.primary.reset_description = Some("5-hour".into());
            reset_backfill::codex_reset_backfill(&mut fresh, Some(&cached));
            assert_eq!(fresh.primary.resets_at.is_some(), should_backfill);
            assert!((fresh.primary.used_percent - 30.0).abs() < f64::EPSILON);
        }
    }
    #[test]
    fn f6_skips_when_no_cached_snapshot() {
        let mut fresh = codex_snapshot(win(30.0, None));
        reset_backfill::codex_reset_backfill(&mut fresh, None);
        assert!(fresh.primary.resets_at.is_none());
    }
}
