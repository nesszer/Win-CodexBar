use super::*;
use std::sync::Arc;

const MAX_CONCURRENT_PROVIDER_FETCHES: usize = 8;

// ── Provider refresh commands ────────────────────────────────────────

/// Build a `FetchContext` for a provider using persisted cookies/keys.
pub(crate) fn build_fetch_context(
    id: ProviderId,
    settings: &Settings,
    cookies: &ManualCookies,
    api_keys: &ApiKeys,
    token_accounts: &HashMap<ProviderId, ProviderAccountData>,
) -> FetchContext {
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
    let active_token_env = token_override
        .as_ref()
        .and_then(|override_data| override_data.env_override.as_ref());
    let active_token_api_key = active_token_env.and_then(|env| env.values().next().cloned());
    let usage_source = SourceMode::parse(settings.usage_source(id)).unwrap_or_default();
    let api_key = stored_api_key.or(active_token_api_key);
    let has_kimi_code_api_key =
        id == ProviderId::Kimi && api_key.as_deref().is_some_and(|key| !key.trim().is_empty());

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
            "off" if id == ProviderId::Claude && usage_source != SourceMode::Cli => {
                (SourceMode::OAuth, None)
            }
            "off" if has_kimi_code_api_key && usage_source == SourceMode::Auto => {
                (SourceMode::Auto, None)
            }
            "off" => (SourceMode::Cli, None),
            "manual" => {
                let cookie_header = active_token_cookie.or(stored_cookie);
                let source_mode = if has_kimi_code_api_key && usage_source == SourceMode::Auto {
                    SourceMode::Auto
                } else if cookie_header.is_some() {
                    SourceMode::Web
                } else if id == ProviderId::Claude && usage_source != SourceMode::Cli {
                    SourceMode::OAuth
                } else {
                    SourceMode::Cli
                };
                (source_mode, cookie_header)
            }
            // `browser` is accepted as a legacy alias from older settings.
            "auto" | "browser" | "web" => {
                // Try browser cookie extraction as fallback when no manual cookie is set.
                // On non-Windows this is a harmless no-op that returns an error.
                let cookie_header = active_token_cookie.or(stored_cookie).or_else(|| {
                    provider_cookie_domain(id, settings).and_then(|domain| {
                        codexbar::browser::cookies::get_cookie_header(domain)
                            .ok()
                            .filter(|h| !h.is_empty())
                    })
                });
                (usage_source, cookie_header)
            }
            _ => (usage_source, stored_cookie),
        }
    };

    let workspace_id = settings.workspace_id(id).trim().to_string();
    let api_region = settings.api_region(id).trim().to_string();

    FetchContext {
        source_mode,
        manual_cookie_header: cookie_header,
        api_key,
        workspace_id: (!workspace_id.is_empty()).then_some(workspace_id),
        api_region: (!api_region.is_empty()).then_some(api_region),
        ..FetchContext::default()
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
const MAX_CONTEXT_FETCH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(65);

pub(crate) fn provider_fetch_timeout(id: ProviderId, ctx: &FetchContext) -> std::time::Duration {
    let provider_timeout = match id {
        ProviderId::Claude | ProviderId::Codex | ProviderId::Copilot => SLOW_PROVIDER_FETCH_TIMEOUT,
        _ => DEFAULT_PROVIDER_FETCH_TIMEOUT,
    };
    let context_timeout = std::time::Duration::from_secs(ctx.web_timeout.saturating_add(5));
    provider_timeout.max(context_timeout.min(MAX_CONTEXT_FETCH_TIMEOUT))
}

pub(crate) fn is_provider_cache_fresh(
    updated_at: Option<std::time::Instant>,
    stale_after: std::time::Duration,
) -> bool {
    updated_at
        .map(|updated| updated.elapsed() <= stale_after)
        .unwrap_or(false)
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

/// Core refresh logic, usable from both the Tauri command and tray menu actions.
pub(crate) async fn do_refresh_providers(app: &tauri::AppHandle) -> Result<(), String> {
    do_refresh_providers_with_policy(app, true).await
}

pub(crate) async fn do_refresh_providers_if_stale(app: &tauri::AppHandle) -> Result<(), String> {
    do_refresh_providers_with_policy(app, false).await
}

async fn do_refresh_providers_with_policy(
    app: &tauri::AppHandle,
    force: bool,
) -> Result<(), String> {
    let state = app.state::<Mutex<AppState>>();

    if !begin_provider_refresh(&state, force)? {
        return Ok(());
    }

    events::emit_refresh_started(app);

    let inputs = ProviderRefreshInputs::load();
    let enabled_count = inputs.enabled_ids.len();

    let handles = spawn_provider_refreshes(app, &inputs);
    await_provider_refreshes(handles).await;

    let error_count = finish_provider_refresh(&state)?;
    update_tray_and_notifications(app, &state, &inputs.settings)?;

    events::emit_refresh_complete(app, enabled_count, error_count);

    Ok(())
}

fn begin_provider_refresh(
    state: &tauri::State<'_, Mutex<AppState>>,
    force: bool,
) -> Result<bool, String> {
    let mut guard = state.lock().map_err(|e| e.to_string())?;
    if guard.is_refreshing {
        return Ok(false);
    }
    if provider_cache_can_skip_refresh(&guard, force) {
        return Ok(false);
    }

    guard.is_refreshing = true;
    guard.provider_refresh_started_at = Some(std::time::Instant::now());
    Ok(true)
}

fn provider_cache_can_skip_refresh(guard: &AppState, force: bool) -> bool {
    !force
        && !guard.provider_cache.is_empty()
        && is_provider_cache_fresh(guard.provider_cache_updated_at, PROVIDER_CACHE_STALE_AFTER)
}

struct ProviderRefreshInputs {
    settings: Settings,
    enabled_ids: Vec<ProviderId>,
    manual_cookies: ManualCookies,
    api_keys: ApiKeys,
    token_accounts: HashMap<ProviderId, ProviderAccountData>,
}

impl ProviderRefreshInputs {
    fn load() -> Self {
        let settings = Settings::load();
        let enabled_ids = settings.get_enabled_provider_ids();
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
) -> Vec<tokio::task::JoinHandle<()>> {
    let mut handles = Vec::with_capacity(inputs.enabled_ids.len());
    let fetch_permits = Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_PROVIDER_FETCHES));

    for id in &inputs.enabled_ids {
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

        handles.push(tokio::spawn(async move {
            let Ok(_permit) = fetch_permits.acquire_owned().await else {
                return;
            };
            refresh_provider(app_handle, id, ctx).await;
        }));
    }

    handles
}

async fn refresh_provider(app: tauri::AppHandle, id: ProviderId, ctx: FetchContext) {
    let snapshot = fetch_provider_snapshot(id, ctx).await;

    let state = app.state::<Mutex<AppState>>();
    if let Ok(mut guard) = state.lock() {
        let snapshot = preserve_last_good_transient_failure(&mut guard, id, snapshot);
        upsert_provider_cache(&mut guard.provider_cache, snapshot.clone());
        drop(guard);
        events::emit_provider_updated(&app, &snapshot);
    } else {
        events::emit_provider_updated(&app, &snapshot);
    }
}

pub(super) fn preserve_last_good_transient_failure(
    guard: &mut AppState,
    id: ProviderId,
    snapshot: ProviderUsageSnapshot,
) -> ProviderUsageSnapshot {
    if snapshot.error.is_none() {
        guard.transient_provider_failure_counts.remove(&id);
        return snapshot;
    }

    if id != ProviderId::Claude || !is_transient_claude_auth_error(snapshot.error.as_deref()) {
        guard.transient_provider_failure_counts.remove(&id);
        return snapshot;
    }

    let Some(previous) = guard
        .provider_cache
        .iter()
        .find(|cached| cached.provider_id == id.cli_name() && cached.error.is_none())
        .cloned()
    else {
        return snapshot;
    };

    let count = guard
        .transient_provider_failure_counts
        .entry(id)
        .or_insert(0);
    if *count == 0 {
        *count = 1;
        tracing::warn!(
            provider = id.cli_name(),
            "preserving last good provider snapshot after transient auth failure"
        );
        previous
    } else {
        *count = count.saturating_add(1);
        snapshot
    }
}

fn is_transient_claude_auth_error(error: Option<&str>) -> bool {
    let Some(error) = error else {
        return false;
    };
    let lower = error.to_ascii_lowercase();
    lower.contains("unauthorized")
        || lower.contains("authentication required")
        || lower.contains("auth required")
        || lower.contains("oauth")
}

async fn fetch_provider_snapshot(id: ProviderId, ctx: FetchContext) -> ProviderUsageSnapshot {
    let provider = instantiate_provider(id);
    let metadata = provider.metadata().clone();
    let started = std::time::Instant::now();

    let mut snapshot =
        match tokio::time::timeout(provider_fetch_timeout(id, &ctx), provider.fetch_usage(&ctx))
            .await
        {
            Ok(Ok(result)) => ProviderUsageSnapshot::from_fetch_result(id, &metadata, &result),
            Ok(Err(e)) => ProviderUsageSnapshot::from_error(
                id,
                &metadata,
                codexbar::logging::safe_error_message(e),
            ),
            Err(_) => ProviderUsageSnapshot::from_error(id, &metadata, "Timeout".to_string()),
        };

    record_provider_fetch_duration(id, &mut snapshot, started);
    snapshot
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

fn finish_provider_refresh(state: &tauri::State<'_, Mutex<AppState>>) -> Result<usize, String> {
    let mut guard = state.lock().map_err(|e| e.to_string())?;
    guard.is_refreshing = false;
    guard.provider_cache_updated_at = Some(std::time::Instant::now());
    guard.provider_refresh_started_at = None;
    Ok(guard
        .provider_cache
        .iter()
        .filter(|s| s.error.is_some())
        .count())
}

fn update_tray_and_notifications(
    app: &tauri::AppHandle,
    state: &tauri::State<'_, Mutex<AppState>>,
    settings: &Settings,
) -> Result<(), String> {
    let cached = {
        let guard = state.lock().map_err(|e| e.to_string())?;
        guard.provider_cache.clone()
    };
    crate::tray_bridge::update_tray_status_items(app, &cached);
    crate::tray_bridge::update_tray_icon_and_tooltip(app, &cached);
    notify_usage_thresholds(state, settings, &cached);
    Ok(())
}

fn notify_usage_thresholds(
    state: &tauri::State<'_, Mutex<AppState>>,
    settings: &Settings,
    cached: &[ProviderUsageSnapshot],
) {
    let cli_map = codexbar::core::cli_name_map();
    if let Ok(mut guard) = state.lock() {
        for snapshot in cached {
            if snapshot.error.is_none()
                && let Some(&provider) = cli_map.get(snapshot.provider_id.as_str())
            {
                guard.notification_manager.check_and_notify(
                    provider,
                    snapshot.primary.used_percent,
                    settings,
                );
                guard.notification_manager.check_session_transition(
                    provider,
                    snapshot.primary.used_percent,
                    settings,
                );
            }
        }
    }
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
) -> Vec<ProviderUsageSnapshot> {
    state
        .lock()
        .map(|guard| guard.provider_cache.clone())
        .unwrap_or_default()
}
