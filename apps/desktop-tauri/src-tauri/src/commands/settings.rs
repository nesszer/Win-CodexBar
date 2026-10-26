use super::*;

// ── Settings mutation ─────────────────────────────────────────────────

/// Partial settings update — every field is optional so the frontend can
/// send only what changed.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SettingsUpdate {
    pub preferred_currency_code: Option<String>,
    pub enabled_providers: Option<Vec<String>>,
    pub refresh_interval_secs: Option<u64>,
    pub adaptive_refresh: Option<bool>,
    pub refresh_all_providers_on_menu_open: Option<bool>,
    pub status_checks_enabled: Option<bool>,
    pub low_power_mode: Option<bool>,
    pub low_power_mode_preference: Option<String>,
    pub start_at_login: Option<bool>,
    pub start_minimized: Option<bool>,
    pub show_notifications: Option<bool>,
    pub sound_enabled: Option<bool>,
    pub notification_sound_theme: Option<codexbar::settings::NotificationSoundTheme>,
    pub notification_sound_paths: Option<codexbar::settings::NotificationSoundPaths>,
    pub high_usage_threshold: Option<f64>,
    pub critical_usage_threshold: Option<f64>,
    /// Map of provider CLI name → that provider's full quota warning settings
    /// (replaces the stored value; an empty object means Global).
    pub provider_quota_warnings:
        Option<std::collections::HashMap<String, codexbar::settings::ProviderQuotaWarnings>>,
    pub predictive_pace_warning_enabled: Option<bool>,
    pub predictive_pace_warning_lead_hours: Option<u16>,
    pub limit_reset_notifications_enabled: Option<bool>,
    pub credential_expiry_notifications_enabled: Option<bool>,
    pub session_quota_notifications_enabled: Option<bool>,
    pub show_pace: Option<bool>,
    pub tray_icon_mode: Option<String>,
    pub stacked_tray_top_provider: Option<String>,
    pub stacked_tray_bottom_provider: Option<String>,
    pub switcher_shows_icons: Option<bool>,
    pub menu_bar_shows_highest_usage: Option<bool>,
    pub menu_bar_shows_percent: Option<bool>,
    pub menu_bar_icon_style: Option<codexbar::settings::MenuBarIconStyle>,
    pub merged_overview_selected_providers: Option<Vec<String>>,
    pub merged_overview_all_accounts: Option<bool>,
    pub quota_warning_markers_visible: Option<bool>,
    pub highlight_menu_cards_on_hover: Option<bool>,
    pub workday_tick_style: Option<codexbar::settings::WorkdayTickStyle>,
    pub menu_bar_color_pace: Option<bool>,
    pub menu_bar_color_by_provider: Option<bool>,
    pub menu_bar_pace_reserve_positive: Option<bool>,
    pub providers_sorted_alphabetically: Option<bool>,
    pub providers_enabled_only: Option<bool>,
    pub show_as_used: Option<bool>,
    pub show_all_token_accounts_in_menu: Option<bool>,
    pub enable_animations: Option<bool>,
    pub reset_time_relative: Option<bool>,
    pub show_reset_when_exhausted: Option<bool>,
    pub menu_bar_display_mode: Option<String>,
    pub overview_layout: Option<String>,
    pub hide_personal_info: Option<bool>,
    pub update_channel: Option<String>,
    pub auto_download_updates: Option<bool>,
    pub install_updates_on_quit: Option<bool>,
    pub global_shortcut: Option<String>,
    /// Provider-switcher shortcut overrides; replaces the stored overrides.
    pub switcher_shortcuts: Option<std::collections::BTreeMap<String, String>>,
    pub codex_custom_sessions_dirs: Option<Vec<String>>,
    /// Extra Gemini profile homes scanned for Antigravity local history.
    pub antigravity_additional_profile_homes: Option<Vec<String>>,
    pub agent_sessions_enabled: Option<bool>,
    pub agent_sessions_local_only: Option<bool>,
    pub stay_awake_enabled: Option<bool>,
    pub agent_session_ssh_hosts: Option<Vec<String>>,
    pub hooks_enabled: Option<bool>,
    pub http_proxy_enabled: Option<bool>,
    pub http_proxy_url: Option<String>,
    pub http_proxy_username: Option<String>,
    pub http_proxy_password: Option<String>,
    pub ui_language: Option<String>,
    pub ui_language_follows_system: Option<bool>,
    pub theme: Option<String>,
    pub window_scale_percent: Option<u16>,
    pub tray_scale_percent: Option<u16>,
    pub tray_panel_always_on_top: Option<bool>,
    pub powertoys_status_pipe_enabled: Option<bool>,
    pub share_usage_with_cli: Option<bool>,
    pub claude_avoid_keychain_prompts: Option<bool>,
    pub claude_allow_reading_claude_code_credentials: Option<bool>,
    pub claude_swap_enabled: Option<bool>,
    pub claude_swap_executable_path: Option<String>,
    pub claude_swap_source: Option<String>,
    pub claude_desktop_cookies_enabled: Option<bool>,
    pub codex_spark_usage_visible: Option<bool>,
    pub codex_openai_web_extras: Option<bool>,
    pub codex_historical_tracking: Option<bool>,
    pub codex_external_oauth_sources_allowed: Option<bool>,
    pub codex_local_session_cost_ledger_enabled: Option<bool>,
    pub codex_openai_web_battery_saver_enabled: Option<bool>,
    /// OpenAI API: fetch the last hour of Tokens/min on each refresh (upstream #4495).
    #[serde(rename = "openAiLiveUsageEnabled")]
    pub openai_live_usage_enabled: Option<bool>,
    /// OpenAI API: manual per-model organization limits; replaces the stored list, and an
    /// empty list clears it.
    #[serde(rename = "openAiModelRateLimits")]
    pub openai_model_rate_limits: Option<Vec<ModelRateLimitSnapshot>>,
    pub disable_keychain_access: Option<bool>,
    /// Map of provider CLI name → metric preference label.
    pub provider_metrics: Option<std::collections::HashMap<String, String>>,
    /// Map of provider CLI name → stable raw usage-item IDs hidden in the UI.
    pub provider_hidden_usage_item_ids: Option<std::collections::HashMap<String, Vec<String>>>,
    pub float_bar_enabled: Option<bool>,
    pub float_bar_opacity: Option<u8>,
    pub float_bar_scale: Option<u8>,
    pub float_bar_orientation: Option<String>,
    pub float_bar_style: Option<String>,
    pub float_bar_click_through: Option<bool>,
    pub float_bar_provider_ids: Option<Vec<String>>,
    pub float_bar_dark_text: Option<bool>,
    pub float_bar_show_reset_inline: Option<bool>,
    pub float_bar_show_cost: Option<bool>,
    pub promote_tray_icon: Option<bool>,
    pub claude_daily_routines_usage_visible: Option<bool>,
    pub alibaba_token_plan_region: Option<String>,
    /// Optional user-entered Copilot seat AI-credit allowance; `null` clears it.
    #[serde(default, deserialize_with = "deserialize_double_option")]
    pub copilot_seat_credit_entitlement: Option<Option<f64>>,
    /// Work-week length (2..=7); `null` or an out-of-range value selects Automatic.
    #[serde(default, deserialize_with = "deserialize_double_option")]
    pub weekly_progress_work_days: Option<Option<u8>>,
    pub cost_summary_display_style: Option<String>,
    pub open_codex_usage_logs_enabled: Option<bool>,
    pub hide_native_codex_cost_when_open_codex_present: Option<bool>,
    /// History window: `rolling:N` (1..=365), `month-to-date`, or `all`.
    pub cost_reporting_period: Option<String>,
    /// "Track costs" in Usage & Spend.
    pub cost_usage_enabled: Option<bool>,
    /// Usage & Spend time range, same format as `cost_reporting_period`.
    pub spend_dashboard_period: Option<String>,
    /// Usage & Spend sources left out of the dashboard; stored sorted and unique.
    pub spend_dashboard_hidden_source_ids: Option<Vec<String>>,
}

impl SettingsUpdate {
    fn refreshes_provider_data(&self) -> bool {
        self.enabled_providers.is_some()
            || self.claude_allow_reading_claude_code_credentials.is_some()
            || self.codex_openai_web_extras.is_some()
            || self.openai_live_usage_enabled.is_some()
            || self.codex_external_oauth_sources_allowed.is_some()
            || self.alibaba_token_plan_region.is_some()
            || self.copilot_seat_credit_entitlement.is_some()
            || self.weekly_progress_work_days.is_some()
    }

    fn notifies_float_bar(&self) -> bool {
        self.enabled_providers.is_some()
            || self.refresh_interval_secs.is_some()
            || self.low_power_mode.is_some()
            || self.low_power_mode_preference.is_some()
            || self.adaptive_refresh.is_some()
            || self.codex_custom_sessions_dirs.is_some()
            || self.cost_reporting_period.is_some()
            || self.cost_usage_enabled.is_some()
            || self.high_usage_threshold.is_some()
            || self.critical_usage_threshold.is_some()
            || self.provider_quota_warnings.is_some()
            || self.show_as_used.is_some()
            || self.reset_time_relative.is_some()
            || self.show_reset_when_exhausted.is_some()
    }

    fn rebuilds_tray_menu(&self) -> bool {
        self.float_bar_enabled.is_some()
            || self.ui_language.is_some()
            || self.ui_language_follows_system.is_some()
    }

    /// Picking a language stops following the system; picking "System"
    /// (`ui_language_follows_system: true`) resolves to `system`.
    fn apply_language_with(&self, settings: &mut Settings, system: Language) {
        if let Some(language) = self.ui_language.as_deref().and_then(parse_language) {
            settings.ui_language = language;
            settings.ui_language_follows_system = false;
        }
        if let Some(follow) = self.ui_language_follows_system {
            settings.ui_language_follows_system = follow;
        }
        settings.resolve_system_language_with(system);
    }

    pub fn changes_tray_promotion(&self) -> bool {
        self.promote_tray_icon.is_some()
    }

    fn refreshes_tray_presentation(&self) -> bool {
        self.tray_icon_mode.is_some()
            || self.stacked_tray_top_provider.is_some()
            || self.stacked_tray_bottom_provider.is_some()
            || self.switcher_shows_icons.is_some()
            || self.menu_bar_shows_highest_usage.is_some()
            || self.menu_bar_shows_percent.is_some()
            || self.menu_bar_icon_style.is_some()
            || self.menu_bar_color_pace.is_some()
            || self.menu_bar_color_by_provider.is_some()
            || self.show_as_used.is_some()
            || self.reset_time_relative.is_some()
            || self.menu_bar_display_mode.is_some()
            || self.overview_layout.is_some()
            || self.provider_metrics.is_some()
            || self.provider_hidden_usage_item_ids.is_some()
            || self.provider_quota_warnings.is_some()
            || self.preferred_currency_code.is_some()
            || self.codex_spark_usage_visible.is_some()
            || self.copilot_seat_credit_entitlement.is_some()
            || self.cost_reporting_period.is_some()
            || self.enabled_providers.is_some()
            || self.ui_language.is_some()
            || self.ui_language_follows_system.is_some()
            || self.status_checks_enabled.is_some()
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

    /// OpenAI API Live Usage switches. Limits are validated strictly: a bad row is
    /// reported and nothing is saved.
    pub(super) fn apply_openai_live_settings(
        self,
        settings: &mut Settings,
    ) -> Result<Self, String> {
        if let Some(v) = self.openai_live_usage_enabled {
            settings.set_live_usage_enabled(ProviderId::OpenAIApi, v);
        }
        if let Some(rows) = self.openai_model_rate_limits.as_deref() {
            let limits = super::openai_live::validated_model_rate_limits(rows)?;
            settings.set_model_rate_limits(ProviderId::OpenAIApi, limits);
        }
        Ok(self)
    }

    fn apply_provider_settings(self, settings: &mut Settings) -> Self {
        if let Some(providers) = self.enabled_providers.clone() {
            settings.enabled_providers = providers.into_iter().collect::<HashSet<_>>();
        }
        if let Some(v) = self.refresh_interval_secs {
            settings.refresh_interval_secs = v;
        }
        if let Some(v) = self.adaptive_refresh {
            settings.adaptive_refresh = v;
        }
        if let Some(v) = self.refresh_all_providers_on_menu_open {
            settings.refresh_all_providers_on_menu_open = v;
        }
        if let Some(v) = self.status_checks_enabled {
            settings.status_checks_enabled = v;
        }
        if let Some(v) = self
            .cost_reporting_period
            .as_deref()
            .and_then(codexbar::cost_reporting_period::CostReportingPeriod::parse)
        {
            settings.cost_reporting_period = v;
        }
        if let Some(v) = self
            .spend_dashboard_period
            .as_deref()
            .and_then(codexbar::cost_reporting_period::CostReportingPeriod::parse)
        {
            settings.spend_dashboard_period = v;
        }
        if let Some(ids) = self.spend_dashboard_hidden_source_ids.clone() {
            settings.spend_dashboard_hidden_source_ids =
                codexbar::settings::normalize_spend_dashboard_hidden_source_ids(ids);
        }
        if let Some(v) = self.cost_usage_enabled {
            if v && !settings.cost_usage_enabled {
                // Mac pins the bucket zone when cost tracking is turned on.
                settings.pin_cost_usage_bucket_time_zone();
            }
            settings.cost_usage_enabled = v;
        }
        if let Some(v) = self.open_codex_usage_logs_enabled {
            settings.open_codex_usage_logs_enabled = v;
        }
        if let Some(v) = self.hide_native_codex_cost_when_open_codex_present {
            settings.hide_native_codex_cost_when_open_codex_present = v;
        }
        if let Some(v) = self.low_power_mode {
            settings.low_power_mode_preference = if v {
                codexbar::settings::LowPowerModePreference::On
            } else {
                codexbar::settings::LowPowerModePreference::Off
            };
        }
        if let Some(value) = self.low_power_mode_preference.as_deref()
            && let Some(preference) = codexbar::settings::LowPowerModePreference::parse(value)
        {
            settings.low_power_mode_preference = preference;
        }
        if let Some(ref s) = self.tray_icon_mode
            && let Some(mode) = parse_tray_icon_mode(s)
        {
            settings.tray_icon_mode = mode;
        }
        if let Some(provider) = self.stacked_tray_top_provider.clone() {
            settings.stacked_tray_top_provider = normalize_optional_provider_id(provider);
        }
        if let Some(provider) = self.stacked_tray_bottom_provider.clone() {
            settings.stacked_tray_bottom_provider = normalize_optional_provider_id(provider);
        }
        if let Some(v) = self.provider_metrics.clone() {
            apply_provider_metrics(settings, v);
        }
        if let Some(values) = self.provider_hidden_usage_item_ids.clone() {
            apply_provider_hidden_usage_item_ids(settings, values);
        }
        self
    }

    fn apply_general_settings(self, settings: &mut Settings) -> Result<Self, String> {
        if let Some(value) = self.preferred_currency_code.as_deref() {
            let normalized = codexbar::currency::normalize_preferred_currency(value);
            if !value.trim().eq_ignore_ascii_case("AUTO") && normalized == "AUTO" {
                return Err(format!("Unsupported preferred currency: {value}"));
            }
            settings.preferred_currency_code = normalized;
        }
        if let Some(v) = self.start_at_login {
            settings.set_start_at_login(v).map_err(|e| e.to_string())?;
        }
        if let Some(v) = self.start_minimized {
            settings.start_minimized = v;
        }
        if let Some(v) = self.global_shortcut.clone() {
            settings.global_shortcut = v;
        }
        // Read the registry only when "System" is being picked.
        let system = if self.ui_language_follows_system == Some(true) {
            Language::system()
        } else {
            settings.ui_language
        };
        self.apply_language_with(settings, system);
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
        if let Some(v) = self.show_reset_when_exhausted {
            settings.show_reset_when_exhausted = v;
        }
        if let Some(v) = self.menu_bar_display_mode.clone() {
            settings.menu_bar_display_mode = v;
        }
        if let Some(v) = self.overview_layout.as_deref()
            && !v.trim().is_empty()
        {
            // Shared normalizer: trims/case-folds known values, falls back to
            // "compact" for anything unknown (same tolerance as settings load).
            settings.overview_layout = codexbar::settings::normalize_overview_layout(v);
        }
        if let Some(v) = self.window_scale_percent {
            settings.window_scale_percent = codexbar::settings::clamp_window_scale_percent(v);
        }
        if let Some(v) = self.tray_scale_percent {
            settings.tray_scale_percent = codexbar::settings::clamp_tray_scale_percent(v);
        }
        if let Some(v) = self.tray_panel_always_on_top {
            settings.tray_panel_always_on_top = v;
        }
        if let Some(v) = self.switcher_shows_icons {
            settings.switcher_shows_icons = v;
        }
        if let Some(v) = self.menu_bar_shows_highest_usage {
            settings.menu_bar_shows_highest_usage = v;
        }
        if let Some(v) = self.menu_bar_shows_percent {
            settings.menu_bar_shows_percent = v;
            // Legacy callers that still send the flag drive the new key too.
            if self.menu_bar_icon_style.is_none() {
                settings.menu_bar_icon_style = if v {
                    codexbar::settings::MenuBarIconStyle::Percent
                } else {
                    codexbar::settings::MenuBarIconStyle::Bars
                };
            }
        }
        if let Some(v) = self.menu_bar_icon_style {
            settings.menu_bar_icon_style = v;
            // Keep the legacy flag in step for one release so a downgrade keeps the look.
            settings.menu_bar_shows_percent = v == codexbar::settings::MenuBarIconStyle::Percent;
        }
        if let Some(ids) = self.merged_overview_selected_providers.clone() {
            settings.set_merged_overview_selected_providers(ids);
        }
        if let Some(v) = self.merged_overview_all_accounts {
            settings.merged_overview_all_accounts = v;
        }
        if let Some(v) = self.quota_warning_markers_visible {
            settings.quota_warning_markers_visible = v;
        }
        if let Some(v) = self.highlight_menu_cards_on_hover {
            settings.highlight_menu_cards_on_hover = v;
        }
        if let Some(v) = self.workday_tick_style {
            settings.workday_tick_style = v;
        }
        if let Some(v) = self.menu_bar_color_pace {
            settings.menu_bar_color_pace = v;
        }
        if let Some(v) = self.menu_bar_color_by_provider {
            settings.menu_bar_color_by_provider = v;
        }
        if let Some(v) = self.menu_bar_pace_reserve_positive {
            settings.menu_bar_pace_reserve_positive = v;
        }
        if let Some(v) = self.providers_sorted_alphabetically {
            settings.providers_sorted_alphabetically = v;
        }
        if let Some(v) = self.providers_enabled_only {
            settings.providers_enabled_only = v;
        }
        if let Some(v) = self.show_all_token_accounts_in_menu {
            settings.show_all_token_accounts_in_menu = v;
        }
        if let Some(v) = self.promote_tray_icon {
            settings.promote_tray_icon = v;
        }
        self
    }

    fn apply_notification_settings(self, settings: &mut Settings) -> Result<Self, String> {
        if let Some(v) = self.show_notifications {
            settings.show_notifications = v;
        }
        if let Some(v) = self.sound_enabled {
            settings.sound_enabled = v;
        }
        if let Some(v) = self.notification_sound_theme {
            settings.notification_sound_theme = v;
        }
        if let Some(v) = self.notification_sound_paths.clone() {
            codexbar::sound::validate_custom_sound_path_updates(
                &settings.notification_sound_paths,
                &v,
            )
            .map_err(|error| error.to_string())?;
            settings.notification_sound_paths = v;
        }
        if let Some(v) = self.high_usage_threshold {
            settings.high_usage_threshold = v.clamp(0.0, 100.0);
        }
        if let Some(v) = self.critical_usage_threshold {
            settings.critical_usage_threshold = v.clamp(0.0, 100.0);
        }
        if let Some(values) = self.provider_quota_warnings.clone() {
            apply_provider_quota_warnings(settings, values);
        }
        if let Some(v) = self.predictive_pace_warning_enabled {
            settings.predictive_pace_warning_enabled = v;
        }
        if let Some(v) = self.predictive_pace_warning_lead_hours {
            settings.predictive_pace_warning_lead_hours =
                codexbar::settings::clamp_predictive_pace_warning_lead_hours(v);
        }
        if let Some(v) = self.limit_reset_notifications_enabled {
            settings.limit_reset_notifications_enabled = v;
        }
        if let Some(v) = self.credential_expiry_notifications_enabled {
            settings.credential_expiry_notifications_enabled = v;
        }
        if let Some(v) = self.session_quota_notifications_enabled {
            settings.session_quota_notifications_enabled = v;
        }
        if let Some(v) = self.show_pace {
            settings.show_pace = v;
        }
        Ok(self)
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
        if let Some(v) = self.antigravity_additional_profile_homes.clone() {
            settings.antigravity_additional_profile_homes = normalize_custom_sessions_dirs(v);
        }
        if let Some(v) = self.agent_sessions_enabled {
            settings.agent_sessions_enabled = v;
        }
        if let Some(v) = self.agent_sessions_local_only {
            settings.agent_sessions_local_only = v;
        }
        if let Some(v) = self.stay_awake_enabled {
            settings.stay_awake_enabled = v;
        }
        if let Some(v) = self.agent_session_ssh_hosts.clone() {
            settings.agent_session_ssh_hosts =
                codexbar::agent_sessions::RemoteSessionFetcher::sanitized_hosts(&v);
        }
        if let Some(v) = self.hooks_enabled {
            settings.hooks_enabled = v;
        }
        if let Some(v) = self.http_proxy_enabled {
            settings.http_proxy_enabled = v;
        }
        if let Some(v) = self.http_proxy_url.clone() {
            settings.http_proxy_url = v.trim().to_string();
        }
        if let Some(v) = self.http_proxy_username.clone() {
            settings.http_proxy_username = v.trim().to_string();
        }
        if let Some(v) = self.http_proxy_password.clone() {
            settings.http_proxy_password = v;
        }
        if let Some(v) = self.install_updates_on_quit {
            settings.install_updates_on_quit = v;
        }
        if let Some(v) = self.powertoys_status_pipe_enabled {
            settings.powertoys_status_pipe_enabled = v;
        }
        if let Some(v) = self.share_usage_with_cli {
            settings.share_usage_with_cli = v;
        }
        if let Some(v) = self.claude_avoid_keychain_prompts {
            settings.set_claude_avoid_keychain_prompts(v);
        }
        if let Some(v) = self.claude_allow_reading_claude_code_credentials {
            settings.claude_allow_reading_claude_code_credentials = v;
        }
        if let Some(v) = self.claude_swap_enabled {
            settings.set_claude_swap_enabled(v);
        }
        if let Some(v) = self.claude_desktop_cookies_enabled {
            settings.set_claude_desktop_cookies_enabled(v);
        }
        if let Some(v) = self.claude_swap_executable_path.clone() {
            settings.set_claude_swap_executable_path(v);
        }
        if let Some(source) = self
            .claude_swap_source
            .as_deref()
            .and_then(codexbar::providers::claude::claude_swap::ClaudeSwapAccountSource::parse)
        {
            settings.set_claude_swap_source(source);
        }
        if let Some(v) = self.codex_spark_usage_visible {
            settings.set_codex_spark_usage_visible(v);
        }
        if let Some(v) = self.codex_openai_web_extras {
            settings.set_openai_web_extras(codexbar::core::ProviderId::Codex, v);
        }
        if let Some(v) = self.codex_historical_tracking {
            settings.set_historical_tracking(codexbar::core::ProviderId::Codex, v);
        }
        if let Some(v) = self.codex_external_oauth_sources_allowed {
            settings.codex_external_oauth_sources_allowed = v;
        }
        if let Some(v) = self.codex_local_session_cost_ledger_enabled {
            settings.set_local_session_cost_ledger_enabled(codexbar::core::ProviderId::Codex, v);
        }
        if let Some(v) = self.codex_openai_web_battery_saver_enabled {
            settings.set_openai_web_battery_saver_enabled(codexbar::core::ProviderId::Codex, v);
        }
        if let Some(v) = self.disable_keychain_access {
            settings.disable_keychain_access = v;
            if v {
                settings.set_claude_avoid_keychain_prompts(true);
            }
        }
        if let Some(v) = self.claude_daily_routines_usage_visible {
            settings.set_claude_daily_routines_usage_visible(v);
        }
        if let Some(v) = self.alibaba_token_plan_region.as_deref() {
            let region = codexbar::providers::AlibabaTokenPlanRegion::from_settings_value(Some(v));
            settings.set_api_region(
                codexbar::core::ProviderId::AlibabaTokenPlan,
                region.as_str(),
            );
        }
        if let Some(v) = self.weekly_progress_work_days {
            settings.weekly_progress_work_days =
                codexbar::settings::normalize_weekly_progress_work_days(v);
        }
        if let Some(v) = self
            .cost_summary_display_style
            .as_deref()
            .and_then(crate::commands::bridge::parse_cost_summary_display_style)
        {
            settings.cost_summary_display_style = v;
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
            show_cost: self.float_bar_show_cost,
        }
    }

    fn apply_to(self, settings: &mut Settings) -> Result<crate::floatbar::SettingsPatch, String> {
        if let Some(value) = self.low_power_mode_preference.as_deref()
            && codexbar::settings::LowPowerModePreference::parse(value).is_none()
        {
            return Err(format!("Invalid low power mode preference: {value}"));
        }
        if let Some(value) = self.cost_reporting_period.as_deref()
            && codexbar::cost_reporting_period::CostReportingPeriod::parse(value).is_none()
        {
            return Err(format!("Invalid cost reporting period: {value}"));
        }
        if let Some(value) = self.spend_dashboard_period.as_deref()
            && codexbar::cost_reporting_period::CostReportingPeriod::parse(value).is_none()
        {
            return Err(format!("Invalid spend dashboard period: {value}"));
        }
        if let Some(overrides) = &self.switcher_shortcuts {
            settings.switcher_shortcuts =
                codexbar::switcher_shortcuts::normalize_overrides(overrides)
                    .map_err(|error| error.to_string())?;
        }
        if let Some(value) = self.copilot_seat_credit_entitlement {
            settings.set_seat_credit_entitlement(codexbar::core::ProviderId::Copilot, value)?;
        }
        let float_bar_patch = self.float_bar_patch();
        self.apply_openai_live_settings(settings)?
            .apply_provider_settings(settings)
            .apply_general_settings(settings)?
            .apply_display_settings(settings)
            .apply_notification_settings(settings)?
            .apply_advanced_settings(settings);
        float_bar_patch.apply(settings);
        Ok(float_bar_patch)
    }
}

/// Present key → `Some(value)`, explicit `null` → `Some(None)`; a missing key
/// stays `None` through `#[serde(default)]`.
fn deserialize_double_option<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Ok(Some(Option::<T>::deserialize(deserializer)?))
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
        let Some(pref) = parse_metric_preference(&label) else {
            continue;
        };
        let declared = match codexbar::core::ProviderId::from_cli_name(&provider) {
            Some(id) => pref.is_declared_for(id),
            None => pref.named_extra_id().is_none(),
        };
        if !declared {
            tracing::warn!(provider = %provider, metric = %label, "ignoring undeclared named extra metric");
            continue;
        }
        settings.provider_metrics.insert(provider, pref);
    }
}

fn apply_provider_quota_warnings(
    settings: &mut Settings,
    values: std::collections::HashMap<String, codexbar::settings::ProviderQuotaWarnings>,
) {
    for (provider, warnings) in values {
        if let Ok(provider_id) = super::parse_provider_arg(&provider) {
            settings.set_quota_warnings(provider_id, warnings);
        }
    }
}

fn apply_provider_hidden_usage_item_ids(
    settings: &mut Settings,
    values: std::collections::HashMap<String, Vec<String>>,
) {
    for (provider, ids) in values {
        if let Ok(provider_id) = super::parse_provider_arg(&provider) {
            settings.set_hidden_usage_item_ids(provider_id, ids);
        }
    }
}

fn parse_tray_icon_mode(s: &str) -> Option<TrayIconMode> {
    match s {
        "single" => Some(TrayIconMode::Single),
        "perProvider" => Some(TrayIconMode::PerProvider),
        "stacked" => Some(TrayIconMode::Stacked),
        _ => None,
    }
}

fn normalize_optional_provider_id(value: String) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
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

/// Serializes the load -> patch -> save of `update_settings`. Commands run
/// concurrently, so two overlapping patches would otherwise both load the same
/// file and the later save would drop the earlier change.
pub(super) static UPDATE_SETTINGS_LOCK: Mutex<()> = Mutex::new(());

/// Hold [`UPDATE_SETTINGS_LOCK`] around a settings load -> change -> save made
/// outside `update_settings`. Drop it before follow-up work that may save again.
pub(crate) fn lock_settings_writes() -> std::sync::MutexGuard<'static, ()> {
    UPDATE_SETTINGS_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[tauri::command]
pub async fn update_settings(
    app: tauri::AppHandle,
    patch: SettingsUpdate,
) -> Result<SettingsSnapshot, String> {
    let write_guard = UPDATE_SETTINGS_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut settings = Settings::load();
    let notify_float_bar = patch.notifies_float_bar();
    let refresh_provider_data = patch.refreshes_provider_data();
    let clear_local_usage_cache = patch.codex_custom_sessions_dirs.is_some()
        || patch.antigravity_additional_profile_homes.is_some()
        || patch.cost_reporting_period.is_some()
        || patch.cost_usage_enabled.is_some();
    let rebuild_tray_menu = patch.rebuilds_tray_menu();
    let refresh_tray_presentation = patch.refreshes_tray_presentation();
    let stay_awake_changed = patch.stay_awake_enabled.is_some();
    let tray_promotion_changed = patch.changes_tray_promotion();
    let tray_panel_always_on_top_changed = patch.tray_panel_always_on_top.is_some();
    let share_usage_with_cli_changed = patch.share_usage_with_cli.is_some();
    let overview_all_accounts_turned_on = patch.merged_overview_all_accounts == Some(true);
    let previous_promoted = settings.promote_tray_icon;
    let previous_language = settings.ui_language;
    let previous_limit_reset = settings.limit_reset_notifications_enabled;
    let status_targets_changed =
        patch.status_checks_enabled.is_some() || patch.enabled_providers.is_some();

    patch.validate_shortcut_change(&app, &settings.global_shortcut)?;
    let float_bar_patch = patch.apply_to(&mut settings)?;

    settings.save().map_err(|e| e.to_string())?;
    drop(write_guard);
    if settings.ui_language != previous_language {
        let _ = app.emit(events::LOCALE_CHANGED, language_label(settings.ui_language));
    }
    if previous_limit_reset && !settings.limit_reset_notifications_enabled {
        // The next refresh batch also clears it; doing it here forgets the
        // saved reset state even if the app quits before that refresh.
        let removal = app
            .state::<Mutex<AppState>>()
            .lock()
            .ok()
            .and_then(|mut guard| guard.notification_manager.clear_limit_reset_state());
        if let Some(removal) = removal {
            removal.apply();
        }
    }
    if clear_local_usage_cache {
        crate::commands::clear_provider_local_usage_cache();
    }
    if share_usage_with_cli_changed {
        crate::commands::cli_usage_cache::publish(
            &app.state::<Mutex<AppState>>(),
            settings.share_usage_with_cli,
        );
    }

    if overview_all_accounts_turned_on {
        crate::commands::spawn_background_pool_read(&app, &settings, true);
    }

    if refresh_provider_data {
        // Invalidate any in-flight publish work and drop disabled providers from
        // the live cache before a follow-up refresh starts.
        let enabled_ids = settings.get_enabled_provider_ids();
        let state = app.state::<Mutex<AppState>>();
        let _ =
            crate::commands::invalidate_provider_refresh_and_prune_disabled(&state, &enabled_ids);
    }

    crate::floatbar::after_settings_saved(&app, &float_bar_patch, &settings, notify_float_bar);
    if stay_awake_changed {
        crate::stay_awake::settings_changed(&app, settings.stay_awake_enabled);
    }
    if rebuild_tray_menu {
        crate::tray_bridge::rebuild_tray_menu(&app);
    }
    if status_targets_changed {
        crate::status_poller::settings_changed(&app, &settings, !refresh_provider_data);
    }
    if refresh_tray_presentation {
        crate::tray_bridge::refresh_tray_presentation(&app);
    }
    if tray_promotion_changed {
        let new_promoted = settings.promote_tray_icon;
        if new_promoted
            || crate::tray_visibility::should_write_demotion(previous_promoted, new_promoted)
        {
            crate::tray_visibility::apply_promotion(new_promoted);
        }
    }
    if tray_panel_always_on_top_changed {
        crate::shell::flyout_window::apply_always_on_top(&app, &settings);
    }

    // Notify other windows (PopOut dashboard, tray, float bar) so they re-read
    // settings live — e.g. the Display tab's window-scale slider takes effect
    // immediately instead of only after the PopOut is reopened.
    events::emit_settings_changed(&app);
    if refresh_provider_data {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            let _ = crate::commands::do_refresh_providers(&app).await;
        });
    }

    Ok(SettingsSnapshot::from(settings))
}

#[cfg(test)]
mod tests;
