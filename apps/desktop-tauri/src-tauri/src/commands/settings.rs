use super::*;

// ── Settings mutation ─────────────────────────────────────────────────

/// Partial settings update — every field is optional so the frontend can
/// send only what changed.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SettingsUpdate {
    pub enabled_providers: Option<Vec<String>>,
    pub refresh_interval_secs: Option<u64>,
    pub refresh_all_providers_on_menu_open: Option<bool>,
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
    pub show_all_token_accounts_in_menu: Option<bool>,
    pub enable_animations: Option<bool>,
    pub reset_time_relative: Option<bool>,
    pub menu_bar_display_mode: Option<String>,
    pub hide_personal_info: Option<bool>,
    pub update_channel: Option<String>,
    pub auto_download_updates: Option<bool>,
    pub install_updates_on_quit: Option<bool>,
    pub global_shortcut: Option<String>,
    pub codex_custom_sessions_dirs: Option<Vec<String>>,
    pub ui_language: Option<String>,
    pub theme: Option<String>,
    pub window_scale_percent: Option<u16>,
    pub tray_scale_percent: Option<u16>,
    pub claude_avoid_keychain_prompts: Option<bool>,
    pub disable_keychain_access: Option<bool>,
    /// Map of provider CLI name → metric preference label.
    pub provider_metrics: Option<std::collections::HashMap<String, String>>,
    pub float_bar_enabled: Option<bool>,
    pub float_bar_opacity: Option<u8>,
    pub float_bar_scale: Option<u8>,
    pub float_bar_orientation: Option<String>,
    pub float_bar_style: Option<String>,
    pub float_bar_click_through: Option<bool>,
    pub float_bar_provider_ids: Option<Vec<String>>,
    pub float_bar_dark_text: Option<bool>,
    pub float_bar_show_reset_inline: Option<bool>,
}

impl SettingsUpdate {
    fn notifies_float_bar(&self) -> bool {
        self.enabled_providers.is_some()
            || self.refresh_interval_secs.is_some()
            || self.high_usage_threshold.is_some()
            || self.critical_usage_threshold.is_some()
            || self.show_as_used.is_some()
            || self.reset_time_relative.is_some()
    }

    fn rebuilds_tray_menu(&self) -> bool {
        self.float_bar_enabled.is_some() || self.ui_language.is_some()
    }

    fn refreshes_tray_presentation(&self) -> bool {
        self.tray_icon_mode.is_some()
            || self.switcher_shows_icons.is_some()
            || self.menu_bar_shows_highest_usage.is_some()
            || self.menu_bar_shows_percent.is_some()
            || self.show_as_used.is_some()
            || self.reset_time_relative.is_some()
            || self.menu_bar_display_mode.is_some()
            || self.provider_metrics.is_some()
            || self.enabled_providers.is_some()
            || self.ui_language.is_some()
    }

    fn validate_shortcut_change(
        &self,
        app: &tauri::AppHandle,
        current_shortcut: &str,
    ) -> Result<(), String> {
        let Some(new_shortcut) = &self.global_shortcut else {
            return Ok(());
        };

        if new_shortcut.trim().is_empty() {
            crate::shortcut_bridge::unregister_shortcut(app, current_shortcut)?;
        } else if new_shortcut != current_shortcut {
            crate::shortcut_bridge::reregister_shortcut(app, current_shortcut, new_shortcut)?;
        }

        Ok(())
    }

    fn apply_provider_settings(self, settings: &mut Settings) -> Self {
        if let Some(providers) = self.enabled_providers.clone() {
            settings.enabled_providers = providers.into_iter().collect::<HashSet<_>>();
        }
        if let Some(v) = self.refresh_interval_secs {
            settings.refresh_interval_secs = v;
        }
        if let Some(v) = self.refresh_all_providers_on_menu_open {
            settings.refresh_all_providers_on_menu_open = v;
        }
        if let Some(ref s) = self.tray_icon_mode
            && let Some(mode) = parse_tray_icon_mode(s)
        {
            settings.tray_icon_mode = mode;
        }
        if let Some(v) = self.provider_metrics.clone() {
            apply_provider_metrics(settings, v);
        }
        self
    }

    fn apply_general_settings(self, settings: &mut Settings) -> Result<Self, String> {
        if let Some(v) = self.start_at_login {
            settings.set_start_at_login(v).map_err(|e| e.to_string())?;
        }
        if let Some(v) = self.start_minimized {
            settings.start_minimized = v;
        }
        if let Some(v) = self.global_shortcut.clone() {
            settings.global_shortcut = v;
        }
        if let Some(v) = self.ui_language.as_deref().and_then(parse_language)
            && settings.ui_language != v
        {
            settings.ui_language = v;
        }
        if let Some(v) = self.theme.as_deref().and_then(parse_theme) {
            settings.theme = v;
        }
        Ok(self)
    }

    fn apply_display_settings(self, settings: &mut Settings) -> Self {
        if let Some(v) = self.show_as_used {
            settings.show_as_used = v;
        }
        if let Some(v) = self.reset_time_relative {
            settings.reset_time_relative = v;
        }
        if let Some(v) = self.menu_bar_display_mode.clone() {
            settings.menu_bar_display_mode = v;
        }
        if let Some(v) = self.window_scale_percent {
            settings.window_scale_percent = codexbar::settings::clamp_window_scale_percent(v);
        }
        if let Some(v) = self.tray_scale_percent {
            settings.tray_scale_percent = codexbar::settings::clamp_tray_scale_percent(v);
        }
        if let Some(v) = self.switcher_shows_icons {
            settings.switcher_shows_icons = v;
        }
        if let Some(v) = self.menu_bar_shows_highest_usage {
            settings.menu_bar_shows_highest_usage = v;
        }
        if let Some(v) = self.menu_bar_shows_percent {
            settings.menu_bar_shows_percent = v;
        }
        if let Some(v) = self.show_all_token_accounts_in_menu {
            settings.show_all_token_accounts_in_menu = v;
        }
        self
    }

    fn apply_notification_settings(self, settings: &mut Settings) -> Self {
        if let Some(v) = self.show_notifications {
            settings.show_notifications = v;
        }
        if let Some(v) = self.sound_enabled {
            settings.sound_enabled = v;
        }
        if let Some(v) = self.sound_volume {
            settings.sound_volume = v;
        }
        if let Some(v) = self.high_usage_threshold {
            settings.high_usage_threshold = v.clamp(0.0, 100.0);
        }
        if let Some(v) = self.critical_usage_threshold {
            settings.critical_usage_threshold = v.clamp(0.0, 100.0);
        }
        self
    }

    fn apply_advanced_settings(self, settings: &mut Settings) -> Self {
        if let Some(v) = self.enable_animations {
            settings.enable_animations = v;
        }
        if let Some(v) = self.hide_personal_info {
            settings.hide_personal_info = v;
        }
        if let Some(v) = self
            .update_channel
            .as_deref()
            .and_then(parse_update_channel)
        {
            settings.update_channel = v;
        }
        if let Some(v) = self.auto_download_updates {
            settings.auto_download_updates = v;
        }
        if let Some(v) = self.codex_custom_sessions_dirs.clone() {
            settings.codex_custom_sessions_dirs = normalize_custom_sessions_dirs(v);
        }
        if let Some(v) = self.install_updates_on_quit {
            settings.install_updates_on_quit = v;
        }
        if let Some(v) = self.claude_avoid_keychain_prompts {
            settings.set_claude_avoid_keychain_prompts(v);
        }
        if let Some(v) = self.disable_keychain_access {
            settings.disable_keychain_access = v;
            if v {
                settings.set_claude_avoid_keychain_prompts(true);
            }
        }
        self
    }

    fn float_bar_patch(&self) -> crate::floatbar::SettingsPatch {
        crate::floatbar::SettingsPatch {
            enabled: self.float_bar_enabled,
            opacity: self.float_bar_opacity,
            scale: self.float_bar_scale,
            orientation: self.float_bar_orientation.clone(),
            style: self.float_bar_style.clone(),
            click_through: self.float_bar_click_through,
            provider_ids: self.float_bar_provider_ids.clone(),
            dark_text: self.float_bar_dark_text,
            show_reset_inline: self.float_bar_show_reset_inline,
        }
    }

    fn apply_to(self, settings: &mut Settings) -> Result<crate::floatbar::SettingsPatch, String> {
        let float_bar_patch = self.float_bar_patch();
        self.apply_provider_settings(settings)
            .apply_general_settings(settings)?
            .apply_display_settings(settings)
            .apply_notification_settings(settings)
            .apply_advanced_settings(settings);
        float_bar_patch.apply(settings);
        Ok(float_bar_patch)
    }
}

fn normalize_custom_sessions_dirs(dirs: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();

    for dir in dirs {
        let trimmed = dir.trim();
        if trimmed.is_empty() {
            continue;
        }
        let key = trimmed.replace('/', "\\").to_ascii_lowercase();
        if seen.insert(key) {
            out.push(trimmed.to_string());
        }
    }

    out
}

fn apply_provider_metrics(
    settings: &mut Settings,
    metrics_map: std::collections::HashMap<String, String>,
) {
    for (provider, label) in metrics_map {
        if let Some(pref) = parse_metric_preference(&label) {
            settings.provider_metrics.insert(provider, pref);
        }
    }
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
    Language::resolve(s)
}

#[tauri::command]
pub async fn update_settings(
    app: tauri::AppHandle,
    patch: SettingsUpdate,
) -> Result<SettingsSnapshot, String> {
    let mut settings = Settings::load();
    let notify_float_bar = patch.notifies_float_bar();
    let rebuild_tray_menu = patch.rebuilds_tray_menu();
    let refresh_tray_presentation = patch.refreshes_tray_presentation();
    let previous_language = settings.ui_language;

    patch.validate_shortcut_change(&app, &settings.global_shortcut)?;
    let float_bar_patch = patch.apply_to(&mut settings)?;

    if settings.ui_language != previous_language {
        let _ = app.emit(events::LOCALE_CHANGED, language_label(settings.ui_language));
    }

    settings.save().map_err(|e| e.to_string())?;

    crate::floatbar::after_settings_saved(&app, &float_bar_patch, &settings, notify_float_bar);
    if rebuild_tray_menu {
        crate::tray_bridge::rebuild_tray_menu(&app);
    }
    if refresh_tray_presentation {
        crate::tray_bridge::refresh_tray_presentation(&app);
    }

    // Notify other windows (PopOut dashboard, tray, float bar) so they re-read
    // settings live — e.g. the Display tab's window-scale slider takes effect
    // immediately instead of only after the PopOut is reopened.
    events::emit_settings_changed(&app);

    Ok(SettingsSnapshot::from(settings))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_settings_that_affect_tray_trigger_presentation_refresh() {
        assert!(
            SettingsUpdate {
                switcher_shows_icons: Some(false),
                ..Default::default()
            }
            .refreshes_tray_presentation()
        );
        assert!(
            SettingsUpdate {
                reset_time_relative: Some(false),
                ..Default::default()
            }
            .refreshes_tray_presentation()
        );
    }

    #[test]
    fn ui_language_change_refreshes_tray_presentation() {
        assert!(
            SettingsUpdate {
                ui_language: Some("japanese".to_string()),
                ..Default::default()
            }
            .refreshes_tray_presentation()
        );
    }

    #[test]
    fn apply_display_settings_clamps_window_scale_percent() {
        let mut settings = Settings::default();

        SettingsUpdate {
            window_scale_percent: Some(300),
            ..Default::default()
        }
        .apply_display_settings(&mut settings);
        assert_eq!(settings.window_scale_percent, 250);

        SettingsUpdate {
            window_scale_percent: Some(50),
            ..Default::default()
        }
        .apply_display_settings(&mut settings);
        assert_eq!(settings.window_scale_percent, 100);
    }

    #[test]
    fn apply_display_settings_clamps_tray_scale_percent() {
        let mut settings = Settings::default();

        SettingsUpdate {
            tray_scale_percent: Some(300),
            ..Default::default()
        }
        .apply_display_settings(&mut settings);
        assert_eq!(settings.tray_scale_percent, 200);

        SettingsUpdate {
            tray_scale_percent: Some(50),
            ..Default::default()
        }
        .apply_display_settings(&mut settings);
        assert_eq!(settings.tray_scale_percent, 100);
    }
}
