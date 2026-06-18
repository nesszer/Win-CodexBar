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
    pub(crate) surface_modes: Vec<SurfaceModeDescriptor>,
    pub(crate) commands: Vec<BridgeCommandDescriptor>,
    pub(crate) events: Vec<BridgeEventDescriptor>,
    pub(crate) providers: Vec<ProviderCatalogEntry>,
    pub(crate) settings: SettingsSnapshot,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SurfaceModeDescriptor {
    id: &'static str,
    label: &'static str,
    description: &'static str,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeCommandDescriptor {
    pub(crate) id: &'static str,
    pub(crate) description: &'static str,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeEventDescriptor {
    pub(crate) id: &'static str,
    pub(crate) description: &'static str,
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
    show_credits_extra_usage: bool,
    show_all_token_accounts_in_menu: bool,
    surprise_animations: bool,
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
    show_debug_settings: bool,
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
        surface_modes: surface_modes(),
        commands: bridge_commands(),
        events: bridge_events(),
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
            show_credits_extra_usage: settings.show_credits_extra_usage,
            show_all_token_accounts_in_menu: settings.show_all_token_accounts_in_menu,
            surprise_animations: settings.surprise_animations,
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
            show_debug_settings: settings.show_debug_settings,
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

fn surface_modes() -> Vec<SurfaceModeDescriptor> {
    vec![
        SurfaceModeDescriptor {
            id: "hidden",
            label: "Hidden",
            description: "No window is visible; the tray icon remains active.",
        },
        SurfaceModeDescriptor {
            id: "trayPanel",
            label: "Tray panel",
            description: "Borderless anchored panel opened from a tray left click.",
        },
        SurfaceModeDescriptor {
            id: "popOut",
            label: "Pop out",
            description: "Decorated window for a richer, persistent provider view.",
        },
        SurfaceModeDescriptor {
            id: "settings",
            label: "Settings",
            description: "Dedicated settings surface for provider and shell configuration.",
        },
    ]
}

pub(crate) fn bridge_commands() -> Vec<BridgeCommandDescriptor> {
    vec![
        BridgeCommandDescriptor {
            id: "get_bootstrap_state",
            description: "Load the shell contract, provider catalog, and persisted settings snapshot.",
        },
        BridgeCommandDescriptor {
            id: "get_provider_catalog",
            description: "List providers available to the desktop shell from the shared Rust engine.",
        },
        BridgeCommandDescriptor {
            id: "get_settings_snapshot",
            description: "Read persisted settings from the existing Rust settings file format.",
        },
        BridgeCommandDescriptor {
            id: "refresh_providers",
            description: "Async refresh of provider usage snapshots with per-provider event updates.",
        },
        BridgeCommandDescriptor {
            id: "refresh_providers_if_stale",
            description: "Refresh provider usage only when the in-memory cache is stale.",
        },
        BridgeCommandDescriptor {
            id: "get_cached_providers",
            description: "Return the most recent provider usage snapshots from the in-memory cache.",
        },
        BridgeCommandDescriptor {
            id: "get_safe_diagnostics",
            description: "Return a redacted diagnostics snapshot for support/debugging.",
        },
        BridgeCommandDescriptor {
            id: "update_settings",
            description: "Persist a partial settings update through the shared Rust settings facade.",
        },
        BridgeCommandDescriptor {
            id: "set_surface_mode",
            description: "Switch the shell to a visible surface using a required typed target.",
        },
        BridgeCommandDescriptor {
            id: "dismiss_tray_panel",
            description: "Hide the tray panel without affecting other visible surfaces.",
        },
        BridgeCommandDescriptor {
            id: "close_settings_window",
            description: "Dismiss Settings without exiting the tray application.",
        },
        BridgeCommandDescriptor {
            id: "get_current_surface_mode",
            description: "Read the current coarse shell surface mode.",
        },
        BridgeCommandDescriptor {
            id: "get_current_surface_state",
            description: "Read the current coarse shell mode together with its typed target.",
        },
        BridgeCommandDescriptor {
            id: "get_proof_state",
            description: "Dump proof-harness state including surface target, window rect, tray anchor, and work-area evidence.",
        },
        BridgeCommandDescriptor {
            id: "run_proof_command",
            description: "Drive deterministic proof-harness transitions such as tray, native menu, dashboard, provider, settings, about, and hide.",
        },
        BridgeCommandDescriptor {
            id: "get_update_state",
            description: "Get the current app-update lifecycle state.",
        },
        BridgeCommandDescriptor {
            id: "check_for_updates",
            description: "Trigger an update check against the configured channel.",
        },
        BridgeCommandDescriptor {
            id: "download_update",
            description: "Download an available update in the background with progress events.",
        },
        BridgeCommandDescriptor {
            id: "apply_update",
            description: "Launch the downloaded installer and exit the application.",
        },
        BridgeCommandDescriptor {
            id: "dismiss_update",
            description: "Dismiss the current update notification and reset to idle.",
        },
        BridgeCommandDescriptor {
            id: "open_release_page",
            description: "Open the release page for the available update in the default browser.",
        },
        BridgeCommandDescriptor {
            id: "open_external_url",
            description: "Open a validated external http(s) URL in the default browser.",
        },
        BridgeCommandDescriptor {
            id: "get_api_keys",
            description: "List stored API keys for configured providers.",
        },
        BridgeCommandDescriptor {
            id: "get_api_key_providers",
            description: "List providers that support API-key authentication and related help metadata.",
        },
        BridgeCommandDescriptor {
            id: "set_api_key",
            description: "Store or replace an API key for a provider.",
        },
        BridgeCommandDescriptor {
            id: "remove_api_key",
            description: "Delete a stored API key for a provider.",
        },
        BridgeCommandDescriptor {
            id: "get_manual_cookies",
            description: "List manually stored provider cookies.",
        },
        BridgeCommandDescriptor {
            id: "set_manual_cookie",
            description: "Store or replace a manual provider cookie value.",
        },
        BridgeCommandDescriptor {
            id: "remove_manual_cookie",
            description: "Delete a stored manual provider cookie.",
        },
        BridgeCommandDescriptor {
            id: "list_detected_browsers",
            description: "Return browsers detected on this machine that CodexBar can import cookies from.",
        },
        BridgeCommandDescriptor {
            id: "import_browser_cookies",
            description: "Extract and persist cookies for a provider from a detected browser.",
        },
        BridgeCommandDescriptor {
            id: "get_app_info",
            description: "Read app metadata displayed in the shell About surface.",
        },
        BridgeCommandDescriptor {
            id: "get_provider_chart_data",
            description: "Return cost history, credits history, and usage breakdown chart data for a provider.",
        },
        BridgeCommandDescriptor {
            id: "get_token_account_providers",
            description: "List providers that support token accounts (multi-account session/API tokens).",
        },
        BridgeCommandDescriptor {
            id: "get_token_accounts",
            description: "Load token accounts for a provider.",
        },
        BridgeCommandDescriptor {
            id: "add_token_account",
            description: "Add a token account for a provider.",
        },
        BridgeCommandDescriptor {
            id: "remove_token_account",
            description: "Remove a token account by UUID.",
        },
        BridgeCommandDescriptor {
            id: "set_active_token_account",
            description: "Set the active token account for a provider.",
        },
        BridgeCommandDescriptor {
            id: "reorder_providers",
            description: "Persist a new provider display order and return refreshed summaries.",
        },
        BridgeCommandDescriptor {
            id: "set_provider_cookie_source",
            description: "Set the preferred cookie/credential source for a provider.",
        },
        BridgeCommandDescriptor {
            id: "get_provider_cookie_source",
            description: "Read the preferred cookie/credential source for a provider.",
        },
        BridgeCommandDescriptor {
            id: "set_provider_region",
            description: "Set the preferred API region for a provider (Alibaba, Z.ai, MiniMax).",
        },
        BridgeCommandDescriptor {
            id: "get_provider_region",
            description: "Read the preferred API region for a provider.",
        },
        BridgeCommandDescriptor {
            id: "get_provider_cookie_source_options",
            description: "List supported cookie/credential source options for a provider.",
        },
        BridgeCommandDescriptor {
            id: "get_provider_region_options",
            description: "List supported API region options for a provider (empty if none).",
        },
        BridgeCommandDescriptor {
            id: "set_provider_workspace_id",
            description: "Set an optional provider workspace/project scope.",
        },
        BridgeCommandDescriptor {
            id: "get_provider_workspace_id",
            description: "Read an optional provider workspace/project scope.",
        },
        BridgeCommandDescriptor {
            id: "get_gemini_cli_signed_in",
            description: "Detect whether the Gemini CLI is signed in locally.",
        },
        BridgeCommandDescriptor {
            id: "get_vertexai_status",
            description: "Detect VertexAI application default credentials.",
        },
        BridgeCommandDescriptor {
            id: "list_jetbrains_detected_ides",
            description: "List detected JetBrains/Google IDE config directories.",
        },
        BridgeCommandDescriptor {
            id: "set_jetbrains_ide_path",
            description: "Persist an explicit JetBrains IDE config path override.",
        },
        BridgeCommandDescriptor {
            id: "get_kiro_status",
            description: "Detect availability of the Kiro CLI.",
        },
        BridgeCommandDescriptor {
            id: "register_global_shortcut",
            description: "Register a global keyboard shortcut that emits `global-shortcut-triggered` events.",
        },
        BridgeCommandDescriptor {
            id: "unregister_global_shortcut",
            description: "Unregister the currently-captured global shortcut.",
        },
        BridgeCommandDescriptor {
            id: "is_remote_session",
            description: "Return true when running inside an SSH or Windows Remote Desktop session.",
        },
        BridgeCommandDescriptor {
            id: "get_launch_block_reason",
            description: "Return a user-facing reason when the native shell should not launch (SSH/RDP).",
        },
        BridgeCommandDescriptor {
            id: "get_work_area_rect",
            description: "Return the current monitor's work area in physical pixels (excludes the taskbar / Dock / panel).",
        },
        BridgeCommandDescriptor {
            id: "play_notification_sound",
            description: "Play the short notification chime used after refreshes.",
        },
        BridgeCommandDescriptor {
            id: "open_provider_dashboard",
            description: "Open a provider's external dashboard URL in the default browser.",
        },
        BridgeCommandDescriptor {
            id: "open_provider_status_page",
            description: "Open a provider's external status page URL in the default browser.",
        },
        BridgeCommandDescriptor {
            id: "get_provider_detail",
            description: "Return the aggregated identity/usage/pace/cost snapshot backing the Settings provider detail pane.",
        },
        BridgeCommandDescriptor {
            id: "trigger_provider_login",
            description: "Trigger a provider's login flow (CLI-based where available).",
        },
        BridgeCommandDescriptor {
            id: "revoke_provider_credentials",
            description: "Revoke or remove stored credentials (API keys, manual cookies, and token accounts) for a provider.",
        },
        BridgeCommandDescriptor {
            id: "get_credential_storage_status",
            description: "Return non-secret credential file protection status labels.",
        },
        BridgeCommandDescriptor {
            id: "get_locale_strings",
            description: "Return every localized UI string for the requested language (or current language when None).",
        },
        BridgeCommandDescriptor {
            id: "set_ui_language",
            description: "Persist the UI language and emit `locale-changed` so frontends can refetch strings.",
        },
        BridgeCommandDescriptor {
            id: "open_path",
            description: "Open a filesystem path (file or folder) in the OS file manager.",
        },
    ]
}

pub(crate) fn bridge_events() -> Vec<BridgeEventDescriptor> {
    vec![
        BridgeEventDescriptor {
            id: "surface-mode-changed",
            description: "Emitted when the shell changes coarse mode or typed target.",
        },
        BridgeEventDescriptor {
            id: "provider-updated",
            description: "Emitted as provider usage snapshots refresh in the shared backend.",
        },
        BridgeEventDescriptor {
            id: "refresh-started",
            description: "Emitted when a provider refresh cycle begins.",
        },
        BridgeEventDescriptor {
            id: "refresh-complete",
            description: "Emitted when a provider refresh cycle completes.",
        },
        BridgeEventDescriptor {
            id: "update-state-changed",
            description: "Emitted when updater state changes in the backend.",
        },
        BridgeEventDescriptor {
            id: "login-phase-changed",
            description: "Emitted when a provider login flow advances between phases.",
        },
        BridgeEventDescriptor {
            id: "proof-state-changed",
            description: "Emitted when the proof harness updates menu evidence or visible shell state for parity capture.",
        },
        BridgeEventDescriptor {
            id: "global-shortcut-triggered",
            description: "Emitted when the user-registered global shortcut (via register_global_shortcut) fires.",
        },
        BridgeEventDescriptor {
            id: "locale-changed",
            description: "Emitted when the persisted UI language changes. Payload: serialized language label.",
        },
    ]
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
    match language {
        Language::English => "english",
        Language::Chinese => "chinese",
        Language::Japanese => "japanese",
    }
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
