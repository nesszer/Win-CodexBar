use super::*;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapState {
    pub(crate) contract_version: &'static str,
    pub(crate) providers: Vec<ProviderCatalogEntry>,
    pub(crate) settings: SettingsSnapshot,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrentSurfaceState {
    pub mode: String,
    pub target: SurfaceTarget,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCatalogEntry {
    pub(crate) id: String,
    pub(crate) display_name: String,
    pub(crate) cookie_domain: Option<String>,
    /// CLI aliases the Settings search also matches (see `ProviderId::cli_aliases`).
    pub(crate) cli_aliases: Vec<String>,
}

/// One provider's service status for the sidebar dot and the tray panel.
/// `status` is `None` until the first successful poll.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderStatusEntry {
    pub provider_id: String,
    pub status: Option<ProviderServiceStatus>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderServiceStatus {
    /// Upstream raw indicator: none, minor, major, critical, maintenance, unknown.
    pub indicator: &'static str,
    pub description: Option<String>,
    /// Unix milliseconds of the feed's last update.
    pub updated_at: Option<i64>,
}

impl From<&codexbar::status::ProviderStatus> for ProviderServiceStatus {
    fn from(status: &codexbar::status::ProviderStatus) -> Self {
        Self {
            indicator: status.indicator.as_raw(),
            description: status.description.clone(),
            updated_at: status.updated_at.map(|at| at.timestamp_millis()),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsSnapshot {
    preferred_currency_code: String,
    enabled_providers: Vec<String>,
    provider_order: Vec<String>,
    refresh_interval_secs: u64,
    adaptive_refresh: bool,
    refresh_all_providers_on_menu_open: bool,
    status_checks_enabled: bool,
    low_power_mode: bool,
    low_power_mode_preference: &'static str,
    start_at_login: bool,
    start_minimized: bool,
    show_notifications: bool,
    sound_enabled: bool,
    notification_sound_theme: codexbar::settings::NotificationSoundTheme,
    notification_sound_paths: codexbar::settings::NotificationSoundPaths,
    high_usage_threshold: f64,
    critical_usage_threshold: f64,
    /// Per-provider quota warnings keyed by CLI name; Global providers are absent.
    provider_quota_warnings:
        std::collections::HashMap<String, codexbar::settings::ProviderQuotaWarnings>,
    predictive_pace_warning_enabled: bool,
    predictive_pace_warning_lead_hours: u16,
    limit_reset_notifications_enabled: bool,
    credential_expiry_notifications_enabled: bool,
    session_quota_notifications_enabled: bool,
    show_pace: bool,
    tray_icon_mode: &'static str,
    stacked_tray_top_provider: Option<String>,
    stacked_tray_bottom_provider: Option<String>,
    switcher_shows_icons: bool,
    menu_bar_shows_highest_usage: bool,
    menu_bar_shows_percent: bool,
    menu_bar_icon_style: codexbar::settings::MenuBarIconStyle,
    merged_overview_selected_providers: Vec<String>,
    merged_overview_all_accounts: bool,
    quota_warning_markers_visible: bool,
    highlight_menu_cards_on_hover: bool,
    workday_tick_style: codexbar::settings::WorkdayTickStyle,
    menu_bar_color_pace: bool,
    menu_bar_color_by_provider: bool,
    menu_bar_pace_reserve_positive: bool,
    providers_sorted_alphabetically: bool,
    providers_enabled_only: bool,
    provider_default_order: Vec<String>,
    show_as_used: bool,
    show_all_token_accounts_in_menu: bool,
    enable_animations: bool,
    reset_time_relative: bool,
    show_reset_when_exhausted: bool,
    menu_bar_display_mode: String,
    overview_layout: String,
    hide_personal_info: bool,
    update_channel: &'static str,
    auto_download_updates: bool,
    install_updates_on_quit: bool,
    global_shortcut: String,
    switcher_shortcuts: std::collections::BTreeMap<String, String>,
    codex_custom_sessions_dirs: Vec<String>,
    antigravity_additional_profile_homes: Vec<String>,
    agent_sessions_enabled: bool,
    agent_sessions_local_only: bool,
    stay_awake_enabled: bool,
    agent_session_ssh_hosts: Vec<String>,
    hooks_enabled: bool,
    http_proxy_enabled: bool,
    http_proxy_url: String,
    http_proxy_username: String,
    http_proxy_password: String,
    ui_language: &'static str,
    ui_language_follows_system: bool,
    theme: &'static str,
    window_scale_percent: u16,
    tray_scale_percent: u16,
    tray_panel_always_on_top: bool,
    powertoys_status_pipe_enabled: bool,
    share_usage_with_cli: bool,
    claude_avoid_keychain_prompts: bool,
    claude_swap_enabled: bool,
    claude_swap_executable_path: String,
    claude_swap_source: codexbar::providers::claude::claude_swap::ClaudeSwapAccountSource,
    claude_desktop_cookies_enabled: bool,
    codex_spark_usage_visible: bool,
    codex_openai_web_extras: bool,
    codex_historical_tracking: bool,
    codex_external_oauth_sources_allowed: bool,
    codex_local_session_cost_ledger_enabled: bool,
    codex_openai_web_battery_saver_enabled: bool,
    /// OpenAI API Live Usage (upstream #4495): fetch Tokens/min on each refresh.
    #[serde(rename = "openAiLiveUsageEnabled")]
    openai_live_usage_enabled: bool,
    /// Manual per-model organization limits, read by the card's limit line and the editor.
    #[serde(rename = "openAiModelRateLimits")]
    openai_model_rate_limits: Vec<ModelRateLimitSnapshot>,
    disable_keychain_access: bool,
    wayfinder_gateway_url: String,
    provider_metrics: std::collections::HashMap<String, String>,
    float_bar_enabled: bool,
    float_bar_opacity: u8,
    float_bar_scale: u8,
    float_bar_orientation: String,
    float_bar_style: String,
    float_bar_click_through: bool,
    float_bar_provider_ids: Vec<String>,
    float_bar_dark_text: bool,
    float_bar_show_reset_inline: bool,
    float_bar_show_cost: bool,
    promote_tray_icon: bool,
    claude_daily_routines_usage_visible: bool,
    claude_allow_reading_claude_code_credentials: bool,
    alibaba_token_plan_region: String,
    copilot_seat_credit_entitlement: Option<f64>,
    weekly_progress_work_days: Option<u8>,
    cost_summary_display_style: &'static str,
    open_codex_usage_logs_enabled: bool,
    hide_native_codex_cost_when_open_codex_present: bool,
    /// History window as its persisted raw form (`rolling:N`,
    /// `month-to-date`, `all`).
    cost_reporting_period: String,
    cost_usage_enabled: bool,
    /// Usage & Spend time range, same raw form as `cost_reporting_period`.
    spend_dashboard_period: String,
    spend_dashboard_hidden_source_ids: Vec<String>,
    provider_accent_colors: std::collections::HashMap<String, String>,
}

#[tauri::command]
pub fn get_bootstrap_state() -> BootstrapState {
    bootstrap_state_for(Settings::load())
}

pub(crate) fn bootstrap_state_for(settings: Settings) -> BootstrapState {
    BootstrapState {
        contract_version: "v1",
        providers: provider_catalog_for(&settings),
        settings: SettingsSnapshot::from(settings),
    }
}

#[tauri::command]
pub fn get_provider_catalog() -> Vec<ProviderCatalogEntry> {
    provider_catalog_for(&Settings::load())
}

#[tauri::command]
pub fn get_settings_snapshot() -> SettingsSnapshot {
    SettingsSnapshot::from(Settings::load())
}

impl From<Settings> for SettingsSnapshot {
    fn from(settings: Settings) -> Self {
        let avoid_keychain_prompts = settings.claude_avoid_keychain_prompts();
        let claude_swap_enabled = settings.claude_swap_enabled();
        let claude_swap_executable_path = settings.claude_swap_executable_path().to_string();
        let claude_swap_source = settings.claude_swap_source();
        let claude_desktop_cookies_enabled = settings.claude_desktop_cookies_enabled();
        let codex_spark_usage_visible = settings.codex_spark_usage_visible();
        let openai_live_usage_enabled = settings.live_usage_enabled(ProviderId::OpenAIApi);
        let openai_model_rate_limits = settings
            .model_rate_limits(ProviderId::OpenAIApi)
            .iter()
            .map(ModelRateLimitSnapshot::from)
            .collect();
        let codex_openai_web_extras = settings.codex_openai_web_extras();
        let codex_historical_tracking = settings.codex_historical_tracking();
        let codex_local_session_cost_ledger_enabled =
            settings.local_session_cost_ledger_enabled(ProviderId::Codex);
        let codex_openai_web_battery_saver_enabled =
            settings.openai_web_battery_saver_enabled(ProviderId::Codex);
        let provider_quota_warnings = settings
            .provider_configs
            .iter()
            .filter(|(_, config)| !config.quota_warnings.is_empty())
            .map(|(id, config)| (id.cli_name().to_string(), config.quota_warnings))
            .collect();
        let wayfinder_gateway_url = settings.gateway_url(ProviderId::Wayfinder).to_string();

        let provider_order = settings.provider_display_order_names();
        let provider_default_order = settings.provider_default_order_names();
        let enabled_providers = provider_order
            .iter()
            .filter(|provider_id| settings.enabled_providers.contains(*provider_id))
            .cloned()
            .collect();

        let copilot_seat_credit_entitlement = settings.seat_credit_entitlement(ProviderId::Copilot);

        let provider_metrics = settings
            .provider_metrics
            .into_iter()
            .map(|(k, v)| (k, metric_preference_label(&v)))
            .collect();

        Self {
            preferred_currency_code: settings.preferred_currency_code,
            enabled_providers,
            provider_order,
            refresh_interval_secs: settings.refresh_interval_secs,
            adaptive_refresh: settings.adaptive_refresh,
            refresh_all_providers_on_menu_open: settings.refresh_all_providers_on_menu_open,
            status_checks_enabled: settings.status_checks_enabled,
            low_power_mode: settings.low_power_mode_preference
                == codexbar::settings::LowPowerModePreference::On,
            low_power_mode_preference: settings.low_power_mode_preference.as_str(),
            start_at_login: settings.start_at_login,
            start_minimized: settings.start_minimized,
            show_notifications: settings.show_notifications,
            sound_enabled: settings.sound_enabled,
            notification_sound_theme: settings.notification_sound_theme,
            notification_sound_paths: settings.notification_sound_paths,
            high_usage_threshold: settings.high_usage_threshold,
            critical_usage_threshold: settings.critical_usage_threshold,
            provider_quota_warnings,
            predictive_pace_warning_enabled: settings.predictive_pace_warning_enabled,
            predictive_pace_warning_lead_hours: settings.predictive_pace_warning_lead_hours,
            limit_reset_notifications_enabled: settings.limit_reset_notifications_enabled,
            credential_expiry_notifications_enabled: settings
                .credential_expiry_notifications_enabled,
            session_quota_notifications_enabled: settings.session_quota_notifications_enabled,
            show_pace: settings.show_pace,
            tray_icon_mode: tray_icon_mode_label(settings.tray_icon_mode),
            stacked_tray_top_provider: settings.stacked_tray_top_provider,
            stacked_tray_bottom_provider: settings.stacked_tray_bottom_provider,
            switcher_shows_icons: settings.switcher_shows_icons,
            menu_bar_shows_highest_usage: settings.menu_bar_shows_highest_usage,
            menu_bar_shows_percent: settings.menu_bar_shows_percent,
            menu_bar_icon_style: settings.menu_bar_icon_style,
            merged_overview_selected_providers: settings.merged_overview_selected_providers,
            merged_overview_all_accounts: settings.merged_overview_all_accounts,
            quota_warning_markers_visible: settings.quota_warning_markers_visible,
            highlight_menu_cards_on_hover: settings.highlight_menu_cards_on_hover,
            workday_tick_style: settings.workday_tick_style,
            menu_bar_color_pace: settings.menu_bar_color_pace,
            menu_bar_color_by_provider: settings.menu_bar_color_by_provider,
            menu_bar_pace_reserve_positive: settings.menu_bar_pace_reserve_positive,
            providers_sorted_alphabetically: settings.providers_sorted_alphabetically,
            providers_enabled_only: settings.providers_enabled_only,
            provider_default_order,
            show_as_used: settings.show_as_used,
            show_all_token_accounts_in_menu: settings.show_all_token_accounts_in_menu,
            enable_animations: settings.enable_animations,
            reset_time_relative: settings.reset_time_relative,
            show_reset_when_exhausted: settings.show_reset_when_exhausted,
            menu_bar_display_mode: settings.menu_bar_display_mode,
            overview_layout: settings.overview_layout,
            hide_personal_info: settings.hide_personal_info,
            update_channel: update_channel_label(settings.update_channel),
            auto_download_updates: settings.auto_download_updates,
            install_updates_on_quit: settings.install_updates_on_quit,
            switcher_shortcuts: codexbar::switcher_shortcuts::resolve_or_default(
                &settings.switcher_shortcuts,
            ),
            global_shortcut: settings.global_shortcut,
            codex_custom_sessions_dirs: settings.codex_custom_sessions_dirs,
            antigravity_additional_profile_homes: settings.antigravity_additional_profile_homes,
            agent_sessions_enabled: settings.agent_sessions_enabled,
            agent_sessions_local_only: settings.agent_sessions_local_only,
            stay_awake_enabled: settings.stay_awake_enabled,
            agent_session_ssh_hosts: settings.agent_session_ssh_hosts,
            hooks_enabled: settings.hooks_enabled,
            openai_live_usage_enabled,
            openai_model_rate_limits,
            http_proxy_enabled: settings.http_proxy_enabled,
            http_proxy_url: settings.http_proxy_url,
            http_proxy_username: settings.http_proxy_username,
            http_proxy_password: settings.http_proxy_password,
            ui_language: language_label(settings.ui_language),
            ui_language_follows_system: settings.ui_language_follows_system,
            theme: theme_label(settings.theme),
            window_scale_percent: settings.window_scale_percent,
            tray_scale_percent: settings.tray_scale_percent,
            tray_panel_always_on_top: settings.tray_panel_always_on_top,
            powertoys_status_pipe_enabled: settings.powertoys_status_pipe_enabled,
            share_usage_with_cli: settings.share_usage_with_cli,
            claude_avoid_keychain_prompts: avoid_keychain_prompts,
            claude_swap_enabled,
            claude_swap_executable_path,
            claude_swap_source,
            claude_desktop_cookies_enabled,
            codex_spark_usage_visible,
            codex_openai_web_extras,
            codex_historical_tracking,
            codex_external_oauth_sources_allowed: settings.codex_external_oauth_sources_allowed,
            codex_local_session_cost_ledger_enabled,
            codex_openai_web_battery_saver_enabled,
            disable_keychain_access: settings.disable_keychain_access,
            wayfinder_gateway_url,
            provider_metrics,
            float_bar_enabled: settings.float_bar_enabled,
            float_bar_opacity: settings.float_bar_opacity,
            float_bar_scale: settings.float_bar_scale,
            float_bar_orientation: settings.float_bar_orientation,
            float_bar_style: settings.float_bar_style,
            float_bar_click_through: settings.float_bar_click_through,
            float_bar_provider_ids: settings.float_bar_provider_ids,
            float_bar_dark_text: settings.float_bar_dark_text,
            float_bar_show_reset_inline: settings.float_bar_show_reset_inline,
            float_bar_show_cost: settings.float_bar_show_cost,
            promote_tray_icon: settings.promote_tray_icon,
            claude_daily_routines_usage_visible: settings.claude_daily_routines_usage_visible,
            claude_allow_reading_claude_code_credentials: settings
                .claude_allow_reading_claude_code_credentials,
            alibaba_token_plan_region: settings.alibaba_token_plan_region,
            copilot_seat_credit_entitlement,
            weekly_progress_work_days: settings.weekly_progress_work_days,
            cost_summary_display_style: cost_summary_display_style_label(
                settings.cost_summary_display_style,
            ),
            open_codex_usage_logs_enabled: settings.open_codex_usage_logs_enabled,
            hide_native_codex_cost_when_open_codex_present: settings
                .hide_native_codex_cost_when_open_codex_present,
            cost_reporting_period: settings.cost_reporting_period.raw(),
            cost_usage_enabled: settings.cost_usage_enabled,
            spend_dashboard_period: settings.spend_dashboard_period.raw(),
            spend_dashboard_hidden_source_ids: settings.spend_dashboard_hidden_source_ids,
            provider_accent_colors: settings
                .provider_configs
                .iter()
                .filter_map(|(id, config)| {
                    config
                        .accent_color
                        .as_ref()
                        .map(|color| (id.cli_name().to_string(), color.clone()))
                })
                .collect(),
        }
    }
}

pub(crate) fn provider_catalog_for(settings: &Settings) -> Vec<ProviderCatalogEntry> {
    // Soft-removed providers (upstream #2254) stay hidden in Settings unless already enabled.
    settings
        .provider_display_order()
        .into_iter()
        .filter(|provider| settings.is_provider_listed(*provider))
        .map(|provider| ProviderCatalogEntry {
            id: provider.cli_name().to_string(),
            display_name: provider.display_name().to_string(),
            cookie_domain: provider.cookie_domain().map(ToString::to_string),
            cli_aliases: provider
                .cli_aliases()
                .iter()
                .map(ToString::to_string)
                .collect(),
        })
        .collect()
}

fn tray_icon_mode_label(mode: TrayIconMode) -> &'static str {
    match mode {
        TrayIconMode::Single => "single",
        TrayIconMode::PerProvider => "perProvider",
        TrayIconMode::Stacked => "stacked",
    }
}

pub(in crate::commands) fn update_channel_label(channel: UpdateChannel) -> &'static str {
    match channel {
        UpdateChannel::Stable => "stable",
        UpdateChannel::Beta => "beta",
    }
}

pub(in crate::commands) fn language_label(language: Language) -> &'static str {
    language.label()
}

fn theme_label(theme: ThemePreference) -> &'static str {
    match theme {
        ThemePreference::Auto => "auto",
        ThemePreference::Light => "light",
        ThemePreference::Dark => "dark",
    }
}

fn cost_summary_display_style_label(
    style: codexbar::settings::CostSummaryDisplayStyle,
) -> &'static str {
    match style {
        codexbar::settings::CostSummaryDisplayStyle::Compact => "compact",
        codexbar::settings::CostSummaryDisplayStyle::Detailed => "detailed",
        codexbar::settings::CostSummaryDisplayStyle::Hidden => "hidden",
    }
}

pub(crate) fn parse_cost_summary_display_style(
    s: &str,
) -> Option<codexbar::settings::CostSummaryDisplayStyle> {
    use codexbar::settings::CostSummaryDisplayStyle;
    match s {
        "compact" => Some(CostSummaryDisplayStyle::Compact),
        "detailed" => Some(CostSummaryDisplayStyle::Detailed),
        "hidden" => Some(CostSummaryDisplayStyle::Hidden),
        _ => None,
    }
}

pub(in crate::commands) fn parse_theme(s: &str) -> Option<ThemePreference> {
    match s {
        "auto" => Some(ThemePreference::Auto),
        "light" => Some(ThemePreference::Light),
        "dark" => Some(ThemePreference::Dark),
        _ => None,
    }
}

fn metric_preference_label(pref: &MetricPreference) -> String {
    pref.key().into_owned()
}

pub(in crate::commands) fn parse_metric_preference(s: &str) -> Option<MetricPreference> {
    MetricPreference::from_key(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overview_all_accounts_reaches_the_bridge_as_camel_case() {
        let off = serde_json::to_value(SettingsSnapshot::from(Settings::default()))
            .expect("serialize snapshot");
        assert_eq!(off["mergedOverviewAllAccounts"], serde_json::json!(false));

        let on = Settings {
            merged_overview_all_accounts: true,
            ..Settings::default()
        };
        let json = serde_json::to_value(SettingsSnapshot::from(on)).expect("serialize snapshot");
        assert_eq!(json["mergedOverviewAllAccounts"], serde_json::json!(true));
        assert!(json.get("merged_overview_all_accounts").is_none());
    }
}
