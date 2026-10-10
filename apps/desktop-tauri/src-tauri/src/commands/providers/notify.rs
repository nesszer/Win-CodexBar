use super::*;

pub(super) fn update_tray_and_notifications(
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
    let resets_at = lane.resets_at_utc();
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
pub(in crate::commands) fn quota_notification_account_identity(
    snapshot: &ProviderUsageSnapshot,
    token_account_id: Option<uuid::Uuid>,
) -> String {
    ProviderId::from_cli_name(&snapshot.provider_id)
        .map(|provider| {
            WarningIdentity::new(
                provider,
                &snapshot.source_label,
                snapshot.account_email.as_deref(),
                snapshot.account_organization.as_deref(),
                token_account_id,
            )
            .threshold_key()
        })
        .unwrap_or_default()
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
    let observed_at = parse_utc(&snapshot.updated_at);

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
        let rate_window = window.to_rate_window();
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
