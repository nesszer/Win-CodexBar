use super::*;

// ── Bridge snapshot types ────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RateWindowSnapshot {
    pub used_percent: f64,
    pub remaining_percent: f64,
    pub window_minutes: Option<u32>,
    pub resets_at: Option<String>,
    pub reset_description: Option<String>,
    pub is_exhausted: bool,
    pub reserve_percent: Option<f64>,
    pub reserve_description: Option<String>,
}

impl RateWindowSnapshot {
    pub(super) fn from_rate_window(rw: &RateWindow) -> Self {
        Self {
            used_percent: rw.used_percent,
            remaining_percent: rw.remaining_percent(),
            window_minutes: rw.window_minutes,
            resets_at: rw.resets_at.map(|dt| dt.to_rfc3339()),
            reset_description: rw.reset_description.clone(),
            is_exhausted: rw.is_exhausted(),
            reserve_percent: None,
            reserve_description: None,
        }
    }

    /// Enrich with reserve info derived from pace analysis.
    /// delta_percent = actual - expected; negative means ahead (in reserve).
    /// Only meaningful for longer windows (weekly); skip if reserve rounds to 0.
    fn with_pace_reserve(mut self, pace: &codexbar::core::UsagePace) -> Self {
        let reserve = pace.delta_percent.abs().round();
        if pace.delta_percent < 0.0 && reserve > 0.0 {
            self.reserve_percent = Some(reserve);
            self.reserve_description = if pace.will_last_to_reset {
                Some("Lasts until reset".to_string())
            } else {
                pace.eta_seconds.map(|s| {
                    let h = (s / 3600.0) as u64;
                    if h >= 24 {
                        format!("Runs out in {}d {}h", h / 24, h % 24)
                    } else {
                        format!("Runs out in {}h", h)
                    }
                })
            };
        }
        self
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CostSnapshotBridge {
    pub used: f64,
    pub limit: Option<f64>,
    pub remaining: Option<f64>,
    pub currency_code: String,
    pub period: String,
    pub resets_at: Option<String>,
    pub formatted_used: String,
    pub formatted_limit: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NamedRateWindowSnapshot {
    pub id: String,
    pub title: String,
    pub window: RateWindowSnapshot,
}

/// Pace prediction snapshot for tray/bridge display.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaceSnapshot {
    pub stage: &'static str,
    pub delta_percent: f64,
    pub will_last_to_reset: bool,
    pub eta_seconds: Option<f64>,
    pub expected_used_percent: f64,
    pub actual_used_percent: f64,
}

/// A frontend-friendly snapshot of one provider's usage data.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderUsageSnapshot {
    pub provider_id: String,
    pub display_name: String,
    pub primary: RateWindowSnapshot,
    pub primary_label: Option<String>,
    pub secondary: Option<RateWindowSnapshot>,
    pub secondary_label: Option<String>,
    pub model_specific: Option<RateWindowSnapshot>,
    pub tertiary: Option<RateWindowSnapshot>,
    pub extra_rate_windows: Vec<NamedRateWindowSnapshot>,
    pub cost: Option<CostSnapshotBridge>,
    pub plan_name: Option<String>,
    pub account_email: Option<String>,
    pub source_label: String,
    pub updated_at: String,
    pub error: Option<String>,
    pub pace: Option<PaceSnapshot>,
    pub account_organization: Option<String>,
    pub tray_status_label: Option<String>,
    pub fetch_duration_ms: Option<u128>,
}

pub(crate) fn pace_stage_str(stage: codexbar::core::PaceStage) -> &'static str {
    use codexbar::core::PaceStage;
    match stage {
        PaceStage::OnTrack => "on_track",
        PaceStage::SlightlyAhead => "slightly_ahead",
        PaceStage::Ahead => "ahead",
        PaceStage::FarAhead => "far_ahead",
        PaceStage::SlightlyBehind => "slightly_behind",
        PaceStage::Behind => "behind",
        PaceStage::FarBehind => "far_behind",
    }
}

impl ProviderUsageSnapshot {
    pub(super) fn from_fetch_result(
        id: ProviderId,
        metadata: &ProviderMetadata,
        result: &ProviderFetchResult,
    ) -> Self {
        let usage = &result.usage;

        let primary_pace = codexbar::core::UsagePace::weekly(&usage.primary, None, 10080);

        let pace = primary_pace.as_ref().map(|p| PaceSnapshot {
            stage: pace_stage_str(p.stage),
            delta_percent: p.delta_percent,
            will_last_to_reset: p.will_last_to_reset,
            eta_seconds: p.eta_seconds,
            expected_used_percent: p.expected_used_percent,
            actual_used_percent: p.actual_used_percent,
        });

        // Compute pace for secondary window (weekly) to derive reserve info
        let secondary_pace = usage
            .secondary
            .as_ref()
            .and_then(|sw| codexbar::core::UsagePace::weekly(sw, None, 10080));

        let primary_snap = RateWindowSnapshot::from_rate_window(&usage.primary);

        let secondary_snap = usage.secondary.as_ref().map(|sw| {
            let mut s = RateWindowSnapshot::from_rate_window(sw);
            if let Some(ref p) = secondary_pace {
                s = s.with_pace_reserve(p);
            }
            s
        });

        let tray_status_label = Some(compact_tray_status_label(
            &usage.primary,
            usage.primary.used_percent,
        ));

        Self {
            provider_id: id.cli_name().to_string(),
            display_name: id.display_name().to_string(),
            primary: primary_snap,
            primary_label: Some(metadata.session_label.to_string()),
            secondary: secondary_snap,
            secondary_label: usage
                .secondary
                .as_ref()
                .map(|_| metadata.weekly_label.to_string()),
            model_specific: usage
                .model_specific
                .as_ref()
                .map(RateWindowSnapshot::from_rate_window),
            tertiary: usage
                .tertiary
                .as_ref()
                .map(RateWindowSnapshot::from_rate_window),
            extra_rate_windows: usage
                .extra_rate_windows
                .iter()
                .map(|extra| NamedRateWindowSnapshot {
                    id: extra.id.clone(),
                    title: extra.title.clone(),
                    window: RateWindowSnapshot::from_rate_window(&extra.window),
                })
                .collect(),
            cost: result.cost.as_ref().map(|c| CostSnapshotBridge {
                used: c.used,
                limit: c.limit,
                remaining: c.remaining(),
                currency_code: c.currency_code.clone(),
                period: c.period.clone(),
                resets_at: c.resets_at.map(|dt| dt.to_rfc3339()),
                formatted_used: c.format_used(),
                formatted_limit: c.format_limit(),
            }),
            plan_name: usage.login_method.clone(),
            account_email: usage.account_email.clone(),
            source_label: result.source_label.clone(),
            updated_at: usage.updated_at.to_rfc3339(),
            error: None,
            pace,
            account_organization: usage.account_organization.clone(),
            tray_status_label,
            fetch_duration_ms: None,
        }
    }

    pub(super) fn from_error(id: ProviderId, metadata: &ProviderMetadata, error: String) -> Self {
        let error = friendly_provider_error(id, &error);
        Self {
            provider_id: id.cli_name().to_string(),
            display_name: id.display_name().to_string(),
            primary: RateWindowSnapshot {
                used_percent: 0.0,
                remaining_percent: 100.0,
                window_minutes: None,
                resets_at: None,
                reset_description: None,
                is_exhausted: false,
                reserve_percent: None,
                reserve_description: None,
            },
            primary_label: Some(metadata.session_label.to_string()),
            secondary: None,
            secondary_label: None,
            model_specific: None,
            tertiary: None,
            extra_rate_windows: Vec::new(),
            cost: None,
            plan_name: None,
            account_email: None,
            source_label: String::new(),
            updated_at: chrono::Utc::now().to_rfc3339(),
            error: Some(error),
            pace: None,
            account_organization: None,
            tray_status_label: None,
            fetch_duration_ms: None,
        }
    }
}

fn compact_tray_status_label(window: &RateWindow, used_percent: f64) -> String {
    let pct = format!("{used_percent:.0}%");
    if let Some(reset) = compact_reset_description(window) {
        format!("{pct} • {reset}")
    } else {
        pct
    }
}

fn compact_reset_description(window: &RateWindow) -> Option<String> {
    if let Some(resets_at) = window.resets_at {
        return Some(format_compact_reset_countdown(resets_at));
    }

    window
        .reset_description
        .as_deref()
        .map(normalize_reset_description)
        .filter(|desc| !desc.is_empty())
}

fn format_compact_reset_countdown(resets_at: chrono::DateTime<chrono::Utc>) -> String {
    let now = chrono::Utc::now();
    if resets_at <= now {
        return "resets now".to_string();
    }

    let total_minutes = (resets_at - now).num_minutes().max(0);
    let days = total_minutes / 1440;
    let hours = (total_minutes % 1440) / 60;
    let minutes = total_minutes % 60;

    if days > 0 {
        format!("resets in {days}d {hours}h")
    } else {
        format!("resets in {hours}h {minutes:02}m")
    }
}

fn normalize_reset_description(desc: &str) -> String {
    let trimmed = desc.trim();
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("resets in ") || lower.starts_with("reset in ") {
        trimmed.to_string()
    } else if lower.starts_with("in ") {
        format!("resets {trimmed}")
    } else {
        format!("resets in {trimmed}")
    }
}

pub(crate) fn friendly_provider_error(id: ProviderId, error: &str) -> String {
    if id != ProviderId::Claude {
        return error.to_string();
    }

    let trimmed = error.trim();
    let lower = trimmed.to_lowercase();

    if lower.contains("swift.cancellationerror")
        || lower.contains("the operation couldn't be completed")
        || lower.contains("the operation could not be completed")
    {
        return "Claude usage fetch was cancelled before usage data was returned. Refresh Claude, or re-authenticate with Claude Code and try again.".to_string();
    }

    if lower.contains("claude oauth credentials not found") {
        return "Claude sign-in was not found. Run `claude` once to authenticate, then refresh Claude in Win-CodexBar.".to_string();
    }

    if lower.contains("oauth token expired") || lower.contains("token invalid or expired") {
        return "Claude sign-in expired. Run `claude` to refresh your Claude Code login, then refresh Claude in Win-CodexBar.".to_string();
    }

    if trimmed == "Authentication required" {
        return "Claude needs sign-in before Win-CodexBar can read usage. Run `claude` once, or add Claude cookies in Provider settings.".to_string();
    }

    if lower.starts_with("claude usage failed from all configured sources.") {
        return trimmed
            .replace(
                "OAuth: OAuth error: Claude OAuth credentials not found. Run `claude` to authenticate.",
                "OAuth: sign-in not found",
            )
            .replace(
                "Web: No cookies available for web API",
                "Web: no Claude cookies available",
            )
            .replace(
                "CLI: Provider not installed:",
                "CLI: not installed:",
            );
    }

    trimmed.to_string()
}

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
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsSnapshot {
    enabled_providers: Vec<String>,
    provider_order: Vec<String>,
    refresh_interval_secs: u64,
    start_at_login: bool,
    start_minimized: bool,
    show_notifications: bool,
    sound_enabled: bool,
    sound_volume: u8,
    high_usage_threshold: f64,
    critical_usage_threshold: f64,
    tray_icon_mode: &'static str,
    switcher_shows_icons: bool,
    menu_bar_shows_highest_usage: bool,
    menu_bar_shows_percent: bool,
    show_as_used: bool,
    show_all_token_accounts_in_menu: bool,
    enable_animations: bool,
    reset_time_relative: bool,
    menu_bar_display_mode: String,
    hide_personal_info: bool,
    update_channel: &'static str,
    auto_download_updates: bool,
    install_updates_on_quit: bool,
    global_shortcut: String,
    ui_language: &'static str,
    theme: &'static str,
    claude_avoid_keychain_prompts: bool,
    disable_keychain_access: bool,
    provider_metrics: std::collections::HashMap<String, &'static str>,
    float_bar_enabled: bool,
    float_bar_opacity: u8,
    float_bar_scale: u8,
    float_bar_orientation: String,
    float_bar_style: String,
    float_bar_click_through: bool,
    float_bar_provider_ids: Vec<String>,
    float_bar_dark_text: bool,
    float_bar_show_reset_inline: bool,
}

#[tauri::command]
pub fn get_bootstrap_state() -> BootstrapState {
    let settings = Settings::load();
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

        let provider_order = settings.provider_display_order_names();
        let enabled_providers = provider_order
            .iter()
            .filter(|provider_id| settings.enabled_providers.contains(*provider_id))
            .cloned()
            .collect();

        let provider_metrics = settings
            .provider_metrics
            .into_iter()
            .map(|(k, v)| (k, metric_preference_label(v)))
            .collect();

        Self {
            enabled_providers,
            provider_order,
            refresh_interval_secs: settings.refresh_interval_secs,
            start_at_login: settings.start_at_login,
            start_minimized: settings.start_minimized,
            show_notifications: settings.show_notifications,
            sound_enabled: settings.sound_enabled,
            sound_volume: settings.sound_volume,
            high_usage_threshold: settings.high_usage_threshold,
            critical_usage_threshold: settings.critical_usage_threshold,
            tray_icon_mode: tray_icon_mode_label(settings.tray_icon_mode),
            switcher_shows_icons: settings.switcher_shows_icons,
            menu_bar_shows_highest_usage: settings.menu_bar_shows_highest_usage,
            menu_bar_shows_percent: settings.menu_bar_shows_percent,
            show_as_used: settings.show_as_used,
            show_all_token_accounts_in_menu: settings.show_all_token_accounts_in_menu,
            enable_animations: settings.enable_animations,
            reset_time_relative: settings.reset_time_relative,
            menu_bar_display_mode: settings.menu_bar_display_mode,
            hide_personal_info: settings.hide_personal_info,
            update_channel: update_channel_label(settings.update_channel),
            auto_download_updates: settings.auto_download_updates,
            install_updates_on_quit: settings.install_updates_on_quit,
            global_shortcut: settings.global_shortcut,
            ui_language: language_label(settings.ui_language),
            theme: theme_label(settings.theme),
            claude_avoid_keychain_prompts: avoid_keychain_prompts,
            disable_keychain_access: settings.disable_keychain_access,
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
        }
    }
}

pub(crate) fn provider_catalog_for(settings: &Settings) -> Vec<ProviderCatalogEntry> {
    settings
        .provider_display_order()
        .into_iter()
        .map(|provider| ProviderCatalogEntry {
            id: provider.cli_name().to_string(),
            display_name: provider.display_name().to_string(),
            cookie_domain: provider.cookie_domain().map(ToString::to_string),
        })
        .collect()
}

fn tray_icon_mode_label(mode: TrayIconMode) -> &'static str {
    match mode {
        TrayIconMode::Single => "single",
        TrayIconMode::PerProvider => "perProvider",
    }
}

pub(super) fn update_channel_label(channel: UpdateChannel) -> &'static str {
    match channel {
        UpdateChannel::Stable => "stable",
        UpdateChannel::Beta => "beta",
    }
}

pub(super) fn language_label(language: Language) -> &'static str {
    language.label()
}

fn theme_label(theme: ThemePreference) -> &'static str {
    match theme {
        ThemePreference::Auto => "auto",
        ThemePreference::Light => "light",
        ThemePreference::Dark => "dark",
    }
}

pub(super) fn parse_theme(s: &str) -> Option<ThemePreference> {
    match s {
        "auto" => Some(ThemePreference::Auto),
        "light" => Some(ThemePreference::Light),
        "dark" => Some(ThemePreference::Dark),
        _ => None,
    }
}

fn metric_preference_label(pref: MetricPreference) -> &'static str {
    match pref {
        MetricPreference::Automatic => "automatic",
        MetricPreference::Session => "session",
        MetricPreference::Weekly => "weekly",
        MetricPreference::Model => "model",
        MetricPreference::Tertiary => "tertiary",
        MetricPreference::Credits => "credits",
        MetricPreference::ExtraUsage => "extraUsage",
        MetricPreference::Average => "average",
    }
}

pub(super) fn parse_metric_preference(s: &str) -> Option<MetricPreference> {
    match s {
        "automatic" => Some(MetricPreference::Automatic),
        "session" => Some(MetricPreference::Session),
        "weekly" => Some(MetricPreference::Weekly),
        "model" => Some(MetricPreference::Model),
        "tertiary" => Some(MetricPreference::Tertiary),
        "credits" => Some(MetricPreference::Credits),
        "extraUsage" | "extrausage" => Some(MetricPreference::ExtraUsage),
        "average" => Some(MetricPreference::Average),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tray_status_prefers_relative_reset_countdown() {
        let window = RateWindow::with_details(
            13.0,
            Some(300),
            Some(chrono::Utc::now() + chrono::Duration::minutes(125)),
            Some("Jun 10 at 3:00PM".to_string()),
        );

        let label = compact_tray_status_label(&window, window.used_percent);

        assert!(label.starts_with("13% • resets in 2h "));
        assert!(label.ends_with('m'));
        assert!(!label.contains("Jun 10"));
    }

    #[test]
    fn tray_status_normalizes_fallback_reset_description() {
        let window = RateWindow::with_details(8.0, Some(300), None, Some("2h 05m".to_string()));

        assert_eq!(
            compact_tray_status_label(&window, window.used_percent),
            "8% • resets in 2h 05m"
        );
    }
}
