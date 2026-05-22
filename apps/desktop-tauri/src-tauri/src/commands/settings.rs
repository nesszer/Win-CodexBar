use super::*;

// ── Settings mutation ─────────────────────────────────────────────────

/// Partial settings update — every field is optional so the frontend can
/// send only what changed.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SettingsUpdate {
    pub enabled_providers: Option<Vec<String>>,
    pub refresh_interval_secs: Option<u64>,
    pub start_at_login: Option<bool>,
    pub start_minimized: Option<bool>,
    pub show_notifications: Option<bool>,
    pub sound_enabled: Option<bool>,
    pub sound_volume: Option<u8>,
    pub high_usage_threshold: Option<f64>,
    pub critical_usage_threshold: Option<f64>,
    pub tray_icon_mode: Option<String>,
    pub switcher_shows_icons: Option<bool>,
    pub menu_bar_shows_highest_usage: Option<bool>,
    pub menu_bar_shows_percent: Option<bool>,
    pub show_as_used: Option<bool>,
    pub show_credits_extra_usage: Option<bool>,
    pub show_all_token_accounts_in_menu: Option<bool>,
    pub surprise_animations: Option<bool>,
    pub enable_animations: Option<bool>,
    pub reset_time_relative: Option<bool>,
    pub menu_bar_display_mode: Option<String>,
    pub hide_personal_info: Option<bool>,
    pub update_channel: Option<String>,
    pub auto_download_updates: Option<bool>,
    pub install_updates_on_quit: Option<bool>,
    pub global_shortcut: Option<String>,
    pub ui_language: Option<String>,
    pub theme: Option<String>,
    pub claude_avoid_keychain_prompts: Option<bool>,
    pub disable_keychain_access: Option<bool>,
    pub show_debug_settings: Option<bool>,
    /// Map of provider CLI name → metric preference label.
    pub provider_metrics: Option<std::collections::HashMap<String, String>>,
    pub float_bar_enabled: Option<bool>,
    pub float_bar_opacity: Option<u8>,
    pub float_bar_orientation: Option<String>,
    pub float_bar_click_through: Option<bool>,
    pub float_bar_provider_ids: Option<Vec<String>>,
    pub float_bar_dark_text: Option<bool>,
}

fn parse_tray_icon_mode(s: &str) -> Option<TrayIconMode> {
    match s {
        "single" => Some(TrayIconMode::Single),
        "perProvider" => Some(TrayIconMode::PerProvider),
        _ => None,
    }
}

fn parse_update_channel(s: &str) -> Option<UpdateChannel> {
    match s {
        "stable" => Some(UpdateChannel::Stable),
        "beta" => Some(UpdateChannel::Beta),
        _ => None,
    }
}

fn parse_language(s: &str) -> Option<Language> {
    match s {
        "english" => Some(Language::English),
        "chinese" => Some(Language::Chinese),
        _ => None,
    }
}

#[tauri::command]
pub async fn update_settings(
    app: tauri::AppHandle,
    patch: SettingsUpdate,
) -> Result<SettingsSnapshot, String> {
    let mut settings = Settings::load();
    let notify_float_bar = patch.enabled_providers.is_some()
        || patch.refresh_interval_secs.is_some()
        || patch.high_usage_threshold.is_some()
        || patch.critical_usage_threshold.is_some();
    let rebuild_tray_menu = patch.float_bar_enabled.is_some();

    // If the shortcut is changing, validate and re-register before persisting.
    if let Some(ref new_shortcut) = patch.global_shortcut
        && *new_shortcut != settings.global_shortcut
    {
        crate::shortcut_bridge::reregister_shortcut(&app, &settings.global_shortcut, new_shortcut)?;
    }

    if let Some(providers) = patch.enabled_providers {
        settings.enabled_providers = providers.into_iter().collect::<HashSet<_>>();
    }
    if let Some(v) = patch.refresh_interval_secs {
        settings.refresh_interval_secs = v;
    }
    if let Some(v) = patch.start_at_login {
        settings.set_start_at_login(v).map_err(|e| e.to_string())?;
    }
    if let Some(v) = patch.show_notifications {
        settings.show_notifications = v;
    }
    if let Some(ref s) = patch.tray_icon_mode
        && let Some(mode) = parse_tray_icon_mode(s)
    {
        settings.tray_icon_mode = mode;
    }
    if let Some(v) = patch.show_as_used {
        settings.show_as_used = v;
    }
    if let Some(v) = patch.surprise_animations {
        settings.surprise_animations = v;
    }
    if let Some(v) = patch.enable_animations {
        settings.enable_animations = v;
    }
    if let Some(v) = patch.reset_time_relative {
        settings.reset_time_relative = v;
    }
    if let Some(v) = patch.menu_bar_display_mode {
        settings.menu_bar_display_mode = v;
    }
    if let Some(v) = patch.hide_personal_info {
        settings.hide_personal_info = v;
    }
    if let Some(ref s) = patch.update_channel
        && let Some(ch) = parse_update_channel(s)
    {
        settings.update_channel = ch;
    }
    if let Some(v) = patch.global_shortcut {
        settings.global_shortcut = v;
    }
    if let Some(ref s) = patch.ui_language
        && let Some(lang) = parse_language(s)
        && settings.ui_language != lang
    {
        settings.ui_language = lang;
        let _ = app.emit(events::LOCALE_CHANGED, language_label(lang));
    }
    if let Some(ref s) = patch.theme
        && let Some(theme) = parse_theme(s)
    {
        settings.theme = theme;
    }
    if let Some(v) = patch.start_minimized {
        settings.start_minimized = v;
    }
    if let Some(v) = patch.sound_enabled {
        settings.sound_enabled = v;
    }
    if let Some(v) = patch.sound_volume {
        settings.sound_volume = v;
    }
    if let Some(v) = patch.high_usage_threshold {
        settings.high_usage_threshold = v.clamp(0.0, 100.0);
    }
    if let Some(v) = patch.critical_usage_threshold {
        settings.critical_usage_threshold = v.clamp(0.0, 100.0);
    }
    if let Some(v) = patch.switcher_shows_icons {
        settings.switcher_shows_icons = v;
    }
    if let Some(v) = patch.menu_bar_shows_highest_usage {
        settings.menu_bar_shows_highest_usage = v;
    }
    if let Some(v) = patch.menu_bar_shows_percent {
        settings.menu_bar_shows_percent = v;
    }
    if let Some(v) = patch.show_credits_extra_usage {
        settings.show_credits_extra_usage = v;
    }
    if let Some(v) = patch.show_all_token_accounts_in_menu {
        settings.show_all_token_accounts_in_menu = v;
    }
    if let Some(v) = patch.auto_download_updates {
        settings.auto_download_updates = v;
    }
    if let Some(v) = patch.install_updates_on_quit {
        settings.install_updates_on_quit = v;
    }
    if let Some(v) = patch.claude_avoid_keychain_prompts {
        settings.set_claude_avoid_keychain_prompts(v);
    }
    if let Some(v) = patch.disable_keychain_access {
        settings.disable_keychain_access = v;
        if v {
            settings.set_claude_avoid_keychain_prompts(true);
        }
    }
    if let Some(v) = patch.show_debug_settings {
        settings.show_debug_settings = v;
    }
    if let Some(metrics_map) = patch.provider_metrics {
        for (provider, label) in metrics_map {
            if let Some(pref) = parse_metric_preference(&label) {
                settings.provider_metrics.insert(provider, pref);
            }
        }
    }
    let float_bar_patch = crate::floatbar::SettingsPatch {
        enabled: patch.float_bar_enabled,
        opacity: patch.float_bar_opacity,
        orientation: patch.float_bar_orientation,
        click_through: patch.float_bar_click_through,
        provider_ids: patch.float_bar_provider_ids,
        dark_text: patch.float_bar_dark_text,
    };
    float_bar_patch.apply(&mut settings);

    settings.save().map_err(|e| e.to_string())?;

    crate::floatbar::after_settings_saved(&app, &float_bar_patch, &settings, notify_float_bar);
    if rebuild_tray_menu {
        crate::tray_bridge::rebuild_tray_menu(&app);
    }

    Ok(SettingsSnapshot::from(settings))
}
