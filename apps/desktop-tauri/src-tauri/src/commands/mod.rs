use std::collections::HashSet;
use std::sync::Mutex;

use codexbar::core::{
    FetchContext, ProviderAccountData, ProviderFetchResult, ProviderId, ProviderMetadata,
    RateWindow, SourceMode, TokenAccountOverride, TokenAccountStore, instantiate_provider,
};
use codexbar::locale;
use codexbar::secure_file::{self, SecureFileStatus};
use codexbar::settings::{
    ApiKeys, Language, ManualCookies, MetricPreference, Settings, ThemePreference, TrayIconMode,
    UpdateChannel,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tauri::{Emitter, Manager};

use crate::events;
use crate::proof_harness::{self, ProofCommand, ProofStatePayload};
use crate::state::AppState;
use crate::surface::SurfaceMode;
use crate::surface_target::SurfaceTarget;

mod chart;
mod diagnostics;
mod tokens;
mod updater;

pub use chart::*;
pub use diagnostics::*;
pub use tokens::*;
pub use updater::*;

const PROVIDER_CACHE_STALE_AFTER: std::time::Duration = std::time::Duration::from_secs(30);
const MAX_API_KEY_LEN: usize = 16 * 1024;
const MAX_COOKIE_HEADER_LEN: usize = 64 * 1024;
const MAX_LABEL_LEN: usize = 80;

fn parse_provider_arg(provider_id: &str) -> Result<ProviderId, String> {
    let trimmed = provider_id.trim();
    if trimmed.is_empty() {
        return Err("Provider id is empty".to_string());
    }
    if trimmed.len() > 64 || trimmed.chars().any(char::is_control) {
        return Err("Provider id is invalid".to_string());
    }
    ProviderId::from_cli_name(trimmed).ok_or_else(|| format!("Unknown provider: {trimmed}"))
}

fn canonical_provider_arg(provider_id: &str) -> Result<String, String> {
    Ok(parse_provider_arg(provider_id)?.cli_name().to_string())
}

fn validate_single_line_secret(value: &str, field: &str, max_len: usize) -> Result<(), String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(format!("{field} is empty"));
    }
    if trimmed.len() > max_len {
        return Err(format!("{field} is too long"));
    }
    if trimmed.contains('\r') || trimmed.contains('\n') {
        return Err(format!("{field} must be a single line"));
    }
    Ok(())
}

fn sanitize_optional_label(label: Option<String>) -> Result<Option<String>, String> {
    let Some(label) = label else {
        return Ok(None);
    };
    let trimmed = label.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if trimmed.len() > MAX_LABEL_LEN || trimmed.chars().any(char::is_control) {
        return Err("Label is invalid".to_string());
    }
    Ok(Some(trimmed.to_string()))
}

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
    fn from_rate_window(rw: &RateWindow) -> Self {
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

fn pace_stage_str(stage: codexbar::core::PaceStage) -> &'static str {
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
    fn from_fetch_result(
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

        let tray_status_label = {
            let pct = format!("{:.0}%", usage.primary.used_percent);
            if let Some(desc) = &usage.primary.reset_description {
                Some(format!("{pct} • resets in {desc}"))
            } else {
                Some(pct)
            }
        };

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

    fn from_error(id: ProviderId, metadata: &ProviderMetadata, error: String) -> Self {
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapState {
    contract_version: &'static str,
    surface_modes: Vec<SurfaceModeDescriptor>,
    commands: Vec<BridgeCommandDescriptor>,
    events: Vec<BridgeEventDescriptor>,
    providers: Vec<ProviderCatalogEntry>,
    settings: SettingsSnapshot,
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
    id: &'static str,
    description: &'static str,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeEventDescriptor {
    id: &'static str,
    description: &'static str,
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
}

#[tauri::command]
pub fn get_bootstrap_state() -> BootstrapState {
    BootstrapState {
        contract_version: "v1",
        surface_modes: surface_modes(),
        commands: bridge_commands(),
        events: bridge_events(),
        providers: provider_catalog(),
        settings: SettingsSnapshot::from(Settings::load()),
    }
}

#[tauri::command]
pub fn get_provider_catalog() -> Vec<ProviderCatalogEntry> {
    provider_catalog()
}

#[tauri::command]
pub fn get_settings_snapshot() -> SettingsSnapshot {
    SettingsSnapshot::from(Settings::load())
}

impl From<Settings> for SettingsSnapshot {
    fn from(settings: Settings) -> Self {
        let avoid_keychain_prompts = settings.claude_avoid_keychain_prompts();

        let mut enabled_providers = settings.enabled_providers.into_iter().collect::<Vec<_>>();
        enabled_providers.sort();

        let provider_metrics = settings
            .provider_metrics
            .into_iter()
            .map(|(k, v)| (k, metric_preference_label(v)))
            .collect();

        Self {
            enabled_providers,
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
        }
    }
}

fn provider_catalog() -> Vec<ProviderCatalogEntry> {
    ProviderId::all()
        .iter()
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

fn bridge_commands() -> Vec<BridgeCommandDescriptor> {
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

fn bridge_events() -> Vec<BridgeEventDescriptor> {
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

fn update_channel_label(channel: UpdateChannel) -> &'static str {
    match channel {
        UpdateChannel::Stable => "stable",
        UpdateChannel::Beta => "beta",
    }
}

fn language_label(language: Language) -> &'static str {
    match language {
        Language::English => "english",
        Language::Chinese => "chinese",
    }
}

fn theme_label(theme: ThemePreference) -> &'static str {
    match theme {
        ThemePreference::Auto => "auto",
        ThemePreference::Light => "light",
        ThemePreference::Dark => "dark",
    }
}

fn parse_theme(s: &str) -> Option<ThemePreference> {
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

fn parse_metric_preference(s: &str) -> Option<MetricPreference> {
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
pub fn update_settings(
    app: tauri::AppHandle,
    patch: SettingsUpdate,
) -> Result<SettingsSnapshot, String> {
    let mut settings = Settings::load();

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

    settings.save().map_err(|e| e.to_string())?;

    Ok(SettingsSnapshot::from(settings))
}

// ── Surface-mode commands ────────────────────────────────────────────

#[tauri::command]
pub fn set_surface_mode(
    mode: String,
    target: SurfaceTarget,
    window: tauri::WebviewWindow,
) -> Result<String, String> {
    let mode = SurfaceMode::parse(&mode).ok_or_else(|| format!("unknown surface mode: {mode}"))?;
    let target = validate_surface_target(mode, target)?;

    crate::shell::transition_to_target(window.app_handle(), mode, target, None)
        .map(|mode| mode.as_str().to_string())
}

/// Open (or focus) a detached Settings/About window.
///
/// Unlike `set_surface_mode`, this spawns a *separate* window so the tray
/// panel stays open.  On Windows, `WebviewWindowBuilder::build` deadlocks
/// inside synchronous Tauri commands, so this must be `async`.
#[tauri::command]
pub async fn open_settings_window(app: tauri::AppHandle, tab: String) -> Result<(), String> {
    crate::shell::settings_window::open_or_focus(&app, &tab)
}

#[tauri::command]
pub fn close_settings_window(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
) -> Result<(), String> {
    crate::shell::settings_window::dismiss(&app, &window)
}

#[tauri::command]
pub fn get_current_surface_mode(state: tauri::State<'_, Mutex<AppState>>) -> String {
    state
        .lock()
        .unwrap()
        .surface_machine
        .current()
        .as_str()
        .to_string()
}

#[tauri::command]
pub fn get_current_surface_state(state: tauri::State<'_, Mutex<AppState>>) -> CurrentSurfaceState {
    let guard = state.lock().unwrap();
    CurrentSurfaceState {
        mode: guard.surface_machine.current().as_str().to_string(),
        target: guard.current_target.clone(),
    }
}

#[tauri::command]
pub fn get_proof_state(app: tauri::AppHandle) -> Result<ProofStatePayload, String> {
    proof_harness::ensure_proof_mode(&app)?;
    proof_harness::capture_state(&app)
}

#[tauri::command]
pub fn run_proof_command(
    app: tauri::AppHandle,
    command: String,
) -> Result<ProofStatePayload, String> {
    let command =
        ProofCommand::parse(&command).ok_or_else(|| format!("unknown proof command: {command}"))?;
    proof_harness::run_command(&app, command)
}

fn validate_surface_target(
    mode: SurfaceMode,
    target: SurfaceTarget,
) -> Result<SurfaceTarget, String> {
    if mode == SurfaceMode::Hidden {
        return Err("set_surface_mode only supports visible surfaces".into());
    }

    if target.mode() != mode {
        return Err(format!(
            "surface target '{}' is not valid for mode '{}'",
            target_label(&target),
            mode.as_str()
        ));
    }

    Ok(target)
}

fn target_label(target: &SurfaceTarget) -> String {
    match target {
        SurfaceTarget::Summary => "summary".into(),
        SurfaceTarget::Dashboard => "dashboard".into(),
        SurfaceTarget::Provider { provider_id } => format!("provider:{provider_id}"),
        SurfaceTarget::Settings { tab } => format!("settings:{tab}"),
    }
}

// ── Provider refresh commands ────────────────────────────────────────

/// Build a `FetchContext` for a provider using persisted cookies/keys.
fn build_fetch_context(
    id: ProviderId,
    settings: &Settings,
    cookies: &ManualCookies,
    api_keys: &ApiKeys,
    token_accounts: &HashMap<ProviderId, ProviderAccountData>,
) -> FetchContext {
    let cookie_source = settings.cookie_source(id);
    let stored_cookie = cookies.get(id.cli_name()).map(|s| s.to_string());
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

    let (source_mode, cookie_header) = match cookie_source {
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
        // `browser` is accepted as a legacy alias from older settings.
        "auto" | "browser" | "web" => {
            // Try browser cookie extraction as fallback when no manual cookie is set.
            // On non-Windows this is a harmless no-op that returns an error.
            let cookie_header = active_token_cookie.or(stored_cookie).or_else(|| {
                id.cookie_domain().and_then(|domain| {
                    codexbar::browser::cookies::get_cookie_header(domain)
                        .ok()
                        .filter(|h| !h.is_empty())
                })
            });
            (usage_source, cookie_header)
        }
        _ => (usage_source, stored_cookie),
    };

    let api_key = api_keys
        .get(id.cli_name())
        .map(|s| s.to_string())
        .or(active_token_api_key);

    FetchContext {
        source_mode,
        manual_cookie_header: cookie_header,
        api_key,
        ..FetchContext::default()
    }
}

const PROVIDER_FETCH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

fn is_provider_cache_fresh(
    updated_at: Option<std::time::Instant>,
    stale_after: std::time::Duration,
) -> bool {
    updated_at
        .map(|updated| updated.elapsed() <= stale_after)
        .unwrap_or(false)
}

fn upsert_provider_cache(cache: &mut Vec<ProviderUsageSnapshot>, snapshot: ProviderUsageSnapshot) {
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

    {
        let mut guard = state.lock().map_err(|e| e.to_string())?;
        if guard.is_refreshing {
            return Ok(());
        }
        if !force
            && !guard.provider_cache.is_empty()
            && is_provider_cache_fresh(guard.provider_cache_updated_at, PROVIDER_CACHE_STALE_AFTER)
        {
            return Ok(());
        }
        guard.is_refreshing = true;
        guard.provider_refresh_started_at = Some(std::time::Instant::now());
    }

    events::emit_refresh_started(app);

    // Load settings and credential stores once, outside the hot loop.
    let settings = Settings::load();
    let enabled_ids = settings.get_enabled_provider_ids();
    let manual_cookies = ManualCookies::load();
    let api_keys = ApiKeys::load();
    let token_accounts = TokenAccountStore::new().load().unwrap_or_else(|e| {
        tracing::warn!("failed to load token accounts for provider refresh: {e}");
        HashMap::new()
    });

    // Spawn one task per enabled provider.
    let mut handles = Vec::with_capacity(enabled_ids.len());

    for id in &enabled_ids {
        let id = *id;
        let app_handle = app.clone();
        let ctx = build_fetch_context(id, &settings, &manual_cookies, &api_keys, &token_accounts);

        handles.push(tokio::spawn(async move {
            let provider = instantiate_provider(id);
            let metadata = provider.metadata().clone();
            let started = std::time::Instant::now();

            let mut snapshot = match tokio::time::timeout(
                PROVIDER_FETCH_TIMEOUT,
                provider.fetch_usage(&ctx),
            )
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
            let fetch_duration_ms = started.elapsed().as_millis();
            snapshot.fetch_duration_ms = Some(fetch_duration_ms);
            if fetch_duration_ms > 5_000 {
                tracing::warn!(
                    provider = id.cli_name(),
                    fetch_duration_ms,
                    "slow provider refresh"
                );
            }

            // Emit per-provider update event.
            events::emit_provider_updated(&app_handle, &snapshot);

            // Append to the cache.
            let st = app_handle.state::<Mutex<AppState>>();
            if let Ok(mut guard) = st.lock() {
                upsert_provider_cache(&mut guard.provider_cache, snapshot);
            }
        }));
    }

    // Await all tasks.
    for handle in handles {
        let _ = handle.await;
    }

    // Finalise.
    let error_count = {
        let mut guard = state.lock().map_err(|e| e.to_string())?;
        guard.is_refreshing = false;
        guard.provider_cache_updated_at = Some(std::time::Instant::now());
        guard.provider_refresh_started_at = None;
        guard
            .provider_cache
            .iter()
            .filter(|s| s.error.is_some())
            .count()
    };

    // Update tray menu labels, icon, and tooltip once after the full refresh cycle.
    {
        let cached = {
            let guard = state.lock().map_err(|e| e.to_string())?;
            guard.provider_cache.clone()
        };
        crate::tray_bridge::update_tray_status_items(app, &cached);
        crate::tray_bridge::update_tray_icon_and_tooltip(app, &cached);

        // Fire OS notifications for any usage-threshold crossings.
        let cli_map = codexbar::core::cli_name_map();
        if let Ok(mut guard) = state.lock() {
            for snapshot in &cached {
                if snapshot.error.is_none()
                    && let Some(&provider) = cli_map.get(snapshot.provider_id.as_str())
                {
                    guard.notification_manager.check_and_notify(
                        provider,
                        snapshot.primary.used_percent,
                        &settings,
                    );
                    guard.notification_manager.check_session_transition(
                        provider,
                        snapshot.primary.used_percent,
                        &settings,
                    );
                }
            }
        }
    }

    events::emit_refresh_complete(app, enabled_ids.len(), error_count);

    Ok(())
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

// ── Credential store commands ─────────────────────────────────────────

/// Bridge-friendly API key info (secrets masked).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeyInfoBridge {
    pub provider_id: String,
    pub provider: String,
    pub masked_key: String,
    pub saved_at: String,
    pub label: Option<String>,
}

/// Bridge-friendly saved cookie info.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CookieInfoBridge {
    pub provider_id: String,
    pub provider: String,
    pub saved_at: String,
}

/// Bridge-friendly provider config info for the API keys tab.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeyProviderInfoBridge {
    pub id: String,
    pub display_name: String,
    pub env_var: Option<String>,
    pub help: Option<String>,
    pub dashboard_url: Option<String>,
}

/// App metadata for the About tab.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfoBridge {
    pub name: String,
    pub version: String,
    pub build_number: String,
    pub update_channel: String,
    pub tagline: String,
}

#[tauri::command]
pub fn get_api_keys() -> Vec<ApiKeyInfoBridge> {
    let keys = ApiKeys::load();
    keys.get_all_for_display()
        .into_iter()
        .map(|info| ApiKeyInfoBridge {
            provider_id: info.provider_id,
            provider: info.provider,
            masked_key: info.masked_key,
            saved_at: info.saved_at,
            label: info.label,
        })
        .collect()
}

#[tauri::command]
pub fn get_api_key_providers() -> Vec<ApiKeyProviderInfoBridge> {
    codexbar::settings::get_api_key_providers()
        .into_iter()
        .map(|p| ApiKeyProviderInfoBridge {
            id: p.id.cli_name().to_string(),
            display_name: p.name.to_string(),
            env_var: p.api_key_env_var.map(|s| s.to_string()),
            help: p.api_key_help.map(|s| s.to_string()),
            dashboard_url: p.dashboard_url.map(|s| s.to_string()),
        })
        .collect()
}

#[tauri::command]
pub fn set_api_key(
    provider_id: String,
    api_key: String,
    label: Option<String>,
) -> Result<Vec<ApiKeyInfoBridge>, String> {
    let canonical_provider = canonical_provider_arg(&provider_id)?;
    if !codexbar::settings::get_api_key_providers()
        .iter()
        .any(|p| p.id.cli_name() == canonical_provider)
    {
        return Err(format!(
            "Provider '{canonical_provider}' does not support API-key storage"
        ));
    }
    validate_single_line_secret(&api_key, "API key", MAX_API_KEY_LEN)?;
    let label = sanitize_optional_label(label)?;

    let mut keys = ApiKeys::load();
    keys.set(&canonical_provider, api_key.trim(), label.as_deref());
    keys.save().map_err(|e| e.to_string())?;
    Ok(get_api_keys())
}

#[tauri::command]
pub fn remove_api_key(provider_id: String) -> Result<Vec<ApiKeyInfoBridge>, String> {
    let canonical_provider = canonical_provider_arg(&provider_id)?;
    let mut keys = ApiKeys::load();
    keys.remove(&canonical_provider);
    keys.save().map_err(|e| e.to_string())?;
    Ok(get_api_keys())
}

#[tauri::command]
pub fn get_manual_cookies() -> Vec<CookieInfoBridge> {
    let cookies = ManualCookies::load();
    cookies
        .get_all_for_display()
        .into_iter()
        .map(|info| CookieInfoBridge {
            provider_id: info.provider_id,
            provider: info.provider,
            saved_at: info.saved_at,
        })
        .collect()
}

#[tauri::command]
pub fn set_manual_cookie(
    provider_id: String,
    cookie_header: String,
) -> Result<Vec<CookieInfoBridge>, String> {
    let id = parse_provider_arg(&provider_id)?;
    if id.cookie_domain().is_none() {
        return Err(format!(
            "Provider '{}' does not support manual cookie storage",
            id.cli_name()
        ));
    }
    validate_single_line_secret(&cookie_header, "Cookie header", MAX_COOKIE_HEADER_LEN)?;

    let mut cookies = ManualCookies::load();
    cookies.set(id.cli_name(), cookie_header.trim());
    cookies.save().map_err(|e| e.to_string())?;
    Ok(get_manual_cookies())
}

#[tauri::command]
pub fn remove_manual_cookie(provider_id: String) -> Result<Vec<CookieInfoBridge>, String> {
    let canonical_provider = canonical_provider_arg(&provider_id)?;
    let mut cookies = ManualCookies::load();
    cookies.remove(&canonical_provider);
    cookies.save().map_err(|e| e.to_string())?;
    Ok(get_manual_cookies())
}

// ── Browser cookie import commands ────────────────────────────────────

/// Bridge-friendly detected browser entry.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectedBrowserBridge {
    /// Stable key used when calling `import_browser_cookies`.
    pub browser_type: String,
    pub display_name: String,
    pub profile_count: usize,
}

/// List all browsers detected on this machine that CodexBar can read cookies from.
///
/// On non-Windows platforms (e.g. Linux CI) this returns an empty list because
/// DPAPI is unavailable; the UI should hide/disable the import button in that case.
#[tauri::command]
pub fn list_detected_browsers() -> Vec<DetectedBrowserBridge> {
    use codexbar::browser::detection::BrowserDetector;

    BrowserDetector::detect_all()
        .into_iter()
        .map(|b| DetectedBrowserBridge {
            browser_type: browser_type_key(b.browser_type).to_string(),
            display_name: b.browser_type.display_name().to_string(),
            profile_count: b.profiles.len(),
        })
        .collect()
}

/// Import cookies for `provider_id` from the named browser and persist them as
/// a manual-cookie override, replacing any existing entry for that provider.
///
/// `browser_type` must be one of the keys returned by `list_detected_browsers`
/// (e.g. `"chrome"`, `"edge"`, `"brave"`).
///
/// Returns the updated manual-cookies list on success.
#[tauri::command]
pub fn import_browser_cookies(
    provider_id: String,
    browser_type: String,
) -> Result<Vec<CookieInfoBridge>, String> {
    use codexbar::browser::cookies::{CookieError, CookieExtractor};
    use codexbar::browser::detection::BrowserDetector;

    // Resolve the provider to get its cookie domain.
    let pid = parse_provider_arg(&provider_id)?;

    let domain = pid
        .cookie_domain()
        .ok_or_else(|| format!("Provider '{provider_id}' does not use cookie authentication"))?;

    // Find the requested browser.
    let browsers = BrowserDetector::detect_all();
    let browser = browsers
        .into_iter()
        .find(|b| browser_type_key(b.browser_type) == browser_type.as_str())
        .ok_or_else(|| format!("Browser '{browser_type}' not found or not installed"))?;

    // Extract the cookie header.
    let cookies = CookieExtractor::extract_for_domain(&browser, domain).map_err(|e| match e {
        CookieError::Dpapi(msg) => format!("DPAPI error: {msg}"),
        other => other.to_string(),
    })?;

    if cookies.is_empty() {
        return Err(format!(
            "No cookies found for {domain} in {}. Make sure you are signed in to that site in the browser.",
            browser.browser_type.display_name()
        ));
    }

    let cookie_header = CookieExtractor::build_cookie_header(&cookies);

    // Persist as manual cookie.
    let mut manual = ManualCookies::load();
    manual.set(pid.cli_name(), &cookie_header);
    manual.save().map_err(|e| e.to_string())?;

    Ok(get_manual_cookies())
}

/// Map `BrowserType` to a stable lowercase string key used in the IPC bridge.
fn browser_type_key(bt: codexbar::browser::detection::BrowserType) -> &'static str {
    use codexbar::browser::detection::BrowserType;
    match bt {
        BrowserType::Chrome => "chrome",
        BrowserType::Edge => "edge",
        BrowserType::Brave => "brave",
        BrowserType::Arc => "arc",
        BrowserType::Firefox => "firefox",
        BrowserType::Chromium => "chromium",
    }
}

#[tauri::command]
pub fn get_app_info() -> AppInfoBridge {
    let settings = Settings::load();
    AppInfoBridge {
        name: "CodexBar".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        build_number: option_env!("BUILD_NUMBER").unwrap_or("dev").to_string(),
        update_channel: update_channel_label(settings.update_channel).to_string(),
        tagline: "May your tokens never run out—keep agent limits in view.".to_string(),
    }
}

pub(super) fn open_url_in_browser(url: &str) -> Result<(), String> {
    let url = validate_external_url(url)?;
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new(windows_system_binary("rundll32.exe"))
            .arg("url.dll,FileProtocolHandler")
            .arg(url)
            .spawn()
            .map_err(|e| format!("Failed to open URL: {e}"))?;
    }
    #[cfg(not(target_os = "windows"))]
    {
        let opener = if cfg!(target_os = "macos") {
            "open"
        } else {
            "xdg-open"
        };
        std::process::Command::new(opener)
            .arg(url)
            .spawn()
            .map_err(|e| format!("Failed to open URL: {e}"))?;
    }
    Ok(())
}

fn validate_external_url(url: &str) -> Result<&str, String> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Err("URL is empty".to_string());
    }
    if trimmed.len() > 2048 || trimmed.chars().any(char::is_control) {
        return Err("URL is invalid".to_string());
    }
    if !(trimmed.starts_with("https://") || trimmed.starts_with("http://")) {
        return Err("Only http and https URLs can be opened".to_string());
    }
    Ok(trimmed)
}

#[cfg(target_os = "windows")]
fn windows_system_binary(name: &str) -> std::path::PathBuf {
    std::env::var_os("SystemRoot")
        .map(std::path::PathBuf::from)
        .map(|root| root.join("System32").join(name))
        .filter(|path| path.exists())
        .unwrap_or_else(|| std::path::PathBuf::from(name))
}

// ════════════════════════════════════════════════════════════════════════════════
// PHASE 4 — Provider ordering, cookie source, region, credential detection,
// global shortcut capture, session/environment introspection, quick actions.
// ════════════════════════════════════════════════════════════════════════════════

// ── Provider summaries + ordering ─────────────────────────────────────

/// Lightweight provider entry returned to the UI after a reorder.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSummary {
    pub id: String,
    pub display_name: String,
    pub enabled: bool,
    pub order: u32,
}

/// Canonicalise a requested provider order: keep requested ids that match a
/// real `ProviderId`, drop duplicates, and append any canonical ids that were
/// omitted (preserving their canonical order).
fn apply_provider_order(requested: &[String]) -> Vec<String> {
    let canonical: Vec<String> = ProviderId::all()
        .iter()
        .map(|p| p.cli_name().to_string())
        .collect();
    let valid: std::collections::HashSet<&str> = canonical.iter().map(|s| s.as_str()).collect();

    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut out: Vec<String> = Vec::with_capacity(canonical.len());

    for id in requested {
        if valid.contains(id.as_str()) && seen.insert(id.clone()) {
            out.push(id.clone());
        }
    }
    for id in &canonical {
        if seen.insert(id.clone()) {
            out.push(id.clone());
        }
    }

    out
}

/// Build `ProviderSummary` list honouring the persisted `provider_order`.
fn build_provider_summaries(settings: &Settings) -> Vec<ProviderSummary> {
    let order = if settings.provider_order.is_empty() {
        apply_provider_order(&[])
    } else {
        apply_provider_order(&settings.provider_order)
    };

    let by_id: std::collections::HashMap<String, &ProviderId> = ProviderId::all()
        .iter()
        .map(|p| (p.cli_name().to_string(), p))
        .collect();

    order
        .iter()
        .enumerate()
        .filter_map(|(idx, id)| {
            by_id.get(id).map(|p| ProviderSummary {
                id: id.clone(),
                display_name: p.display_name().to_string(),
                enabled: settings.enabled_providers.contains(id),
                order: idx as u32,
            })
        })
        .collect()
}

#[tauri::command]
pub fn reorder_providers(ids: Vec<String>) -> Result<Vec<ProviderSummary>, String> {
    let mut settings = Settings::load();
    settings.provider_order = apply_provider_order(&ids);
    settings.save().map_err(|e| e.to_string())?;
    Ok(build_provider_summaries(&settings))
}

// ── Per-provider cookie source + region ───────────────────────────────

/// Map a CLI-name string to a `ProviderId` whose cookie source is exposed in
/// the UI. Returns `None` for providers without a user-facing cookie source.
fn cookie_source_provider(provider_id: &str) -> Option<codexbar::core::ProviderId> {
    use codexbar::core::ProviderId;
    Some(match provider_id {
        "codex" => ProviderId::Codex,
        "claude" => ProviderId::Claude,
        "cursor" => ProviderId::Cursor,
        "opencode" => ProviderId::OpenCode,
        "factory" => ProviderId::Factory,
        "alibaba" => ProviderId::Alibaba,
        "kimi" | "kimik2" => ProviderId::Kimi,
        "minimax" => ProviderId::MiniMax,
        "augment" => ProviderId::Augment,
        "amp" => ProviderId::Amp,
        "ollama" => ProviderId::Ollama,
        "mistral" => ProviderId::Mistral,
        _ => return None,
    })
}

fn provider_cookie_source_lookup(settings: &Settings, provider_id: &str) -> Option<String> {
    cookie_source_provider(provider_id).map(|id| settings.cookie_source(id).to_string())
}

fn provider_cookie_source_set(
    settings: &mut Settings,
    provider_id: &str,
    source: String,
) -> Result<(), String> {
    let id = cookie_source_provider(provider_id)
        .ok_or_else(|| format!("Provider '{provider_id}' does not expose a cookie source"))?;
    settings.set_cookie_source(id, source);
    Ok(())
}

#[tauri::command]
pub fn set_provider_cookie_source(provider_id: String, source: String) -> Result<(), String> {
    let source = source.trim();
    if source.is_empty()
        || !cookie_source_options_for(&provider_id, Language::English)
            .iter()
            .any(|option| option.value == source)
    {
        return Err(format!(
            "Invalid cookie source '{source}' for provider '{provider_id}'"
        ));
    }
    let mut settings = Settings::load();
    provider_cookie_source_set(&mut settings, &provider_id, source.to_string())?;
    settings.save().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_provider_cookie_source(provider_id: String) -> Result<Option<String>, String> {
    Ok(provider_cookie_source_lookup(
        &Settings::load(),
        &provider_id,
    ))
}

fn region_provider(provider_id: &str) -> Option<codexbar::core::ProviderId> {
    use codexbar::core::ProviderId;
    Some(match provider_id {
        "alibaba" => ProviderId::Alibaba,
        "zai" => ProviderId::Zai,
        "minimax" => ProviderId::MiniMax,
        _ => return None,
    })
}

fn provider_region_lookup(settings: &Settings, provider_id: &str) -> Option<String> {
    region_provider(provider_id).map(|id| settings.api_region(id).to_string())
}

fn provider_region_set(
    settings: &mut Settings,
    provider_id: &str,
    region: String,
) -> Result<(), String> {
    let id = region_provider(provider_id)
        .ok_or_else(|| format!("Provider '{provider_id}' does not have a region picker"))?;
    settings.set_api_region(id, region);
    Ok(())
}

#[tauri::command]
pub fn set_provider_region(provider_id: String, region: String) -> Result<(), String> {
    let region = region.trim();
    if region.is_empty()
        || !region_options_for(&provider_id)
            .iter()
            .any(|option| option.value == region)
    {
        return Err(format!(
            "Invalid region '{region}' for provider '{provider_id}'"
        ));
    }
    let mut settings = Settings::load();
    provider_region_set(&mut settings, &provider_id, region.to_string())?;
    settings.save().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_provider_region(provider_id: String) -> Result<Option<String>, String> {
    Ok(provider_region_lookup(&Settings::load(), &provider_id))
}

// ── Phase 6c — cookie source & region option catalogs ────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CookieSourceOption {
    pub value: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RegionOption {
    pub value: String,
    pub label: String,
}

fn cookie_option(
    lang: Language,
    value: &str,
    auto_desc: &str,
    manual_desc: &str,
    off_desc: Option<&str>,
) -> CookieSourceOption {
    let (label, description) = match value {
        "auto" => (
            locale::get_text(lang, locale::LocaleKey::Automatic).to_string(),
            auto_desc.to_string(),
        ),
        "manual" => (
            locale::get_text(lang, locale::LocaleKey::CookieSourceManual).to_string(),
            manual_desc.to_string(),
        ),
        "off" => (
            locale::get_text(lang, locale::LocaleKey::ProviderDisabled).to_string(),
            off_desc.unwrap_or("").to_string(),
        ),
        other => (other.to_string(), String::new()),
    };
    CookieSourceOption {
        value: value.to_string(),
        label,
        description: if description.is_empty() {
            None
        } else {
            Some(description)
        },
    }
}

/// Returns the catalog of cookie source options for a given provider,
/// mirroring the `egui` ComboBox choices in `preferences.rs`.
/// Empty vec means the provider does not expose a cookie-source picker.
pub fn cookie_source_options_for(provider_id: &str, lang: Language) -> Vec<CookieSourceOption> {
    match provider_id {
        "codex" => vec![
            cookie_option(
                lang,
                "auto",
                locale::get_text(lang, locale::LocaleKey::ProviderCodexAutoImportHelp),
                "Paste a Cookie header from a chatgpt.com request.",
                Some("Disable OpenAI dashboard cookie usage."),
            ),
            cookie_option(
                lang,
                "manual",
                "",
                "Paste a Cookie header from a chatgpt.com request.",
                None,
            ),
            cookie_option(
                lang,
                "off",
                "",
                "",
                Some("Disable OpenAI dashboard cookie usage."),
            ),
        ],
        "claude" => vec![
            cookie_option(
                lang,
                "auto",
                locale::get_text(lang, locale::LocaleKey::ProviderClaudeCookiesHelp),
                "",
                None,
            ),
            cookie_option(
                lang,
                "manual",
                "",
                locale::get_text(lang, locale::LocaleKey::ProviderClaudeCookiesHelp),
                None,
            ),
        ],
        "cursor" => vec![
            cookie_option(
                lang,
                "auto",
                locale::get_text(lang, locale::LocaleKey::ProviderCursorCookieSourceHelp),
                "",
                None,
            ),
            cookie_option(
                lang,
                "manual",
                "",
                "Paste a Cookie header from a cursor.com request.",
                None,
            ),
        ],
        "opencode" => vec![
            cookie_option(
                lang,
                "auto",
                "Automatic imports browser cookies from opencode.ai.",
                "",
                None,
            ),
            cookie_option(
                lang,
                "manual",
                "",
                "Paste a Cookie header from the billing page.",
                None,
            ),
        ],
        "factory" => vec![
            cookie_option(
                lang,
                "auto",
                "Automatic imports browser cookies and WorkOS sessions.",
                "",
                None,
            ),
            cookie_option(
                lang,
                "manual",
                "",
                "Paste a Cookie header from Factory.",
                None,
            ),
        ],
        "alibaba" => vec![
            cookie_option(
                lang,
                "auto",
                "Automatic imports browser cookies from Model Studio / Bailian.",
                "",
                None,
            ),
            cookie_option(
                lang,
                "manual",
                "",
                "Paste a Cookie header from Model Studio or Bailian.",
                None,
            ),
        ],
        "kimi" | "kimik2" => vec![
            cookie_option(lang, "auto", "Automatic imports browser cookies.", "", None),
            cookie_option(
                lang,
                "manual",
                "",
                "Paste a cookie header or the kimi-auth token value.",
                None,
            ),
            cookie_option(lang, "off", "", "", Some("Kimi cookies are disabled.")),
        ],
        "minimax" => vec![
            cookie_option(
                lang,
                "auto",
                "Automatic imports browser cookies and Coding Plan tokens.",
                "",
                None,
            ),
            cookie_option(
                lang,
                "manual",
                "",
                "Paste a Cookie header from the Coding Plan page.",
                None,
            ),
        ],
        "augment" => vec![
            cookie_option(lang, "auto", "Automatic imports browser cookies.", "", None),
            cookie_option(
                lang,
                "manual",
                "",
                "Paste a Cookie header from the Augment dashboard.",
                None,
            ),
        ],
        "amp" => vec![
            cookie_option(lang, "auto", "Automatic imports browser cookies.", "", None),
            cookie_option(
                lang,
                "manual",
                "",
                "Paste a Cookie header from Amp settings.",
                None,
            ),
        ],
        "ollama" => vec![
            cookie_option(lang, "auto", "Automatic imports browser cookies.", "", None),
            cookie_option(
                lang,
                "manual",
                "",
                "Paste a Cookie header from Ollama settings.",
                None,
            ),
        ],
        "mistral" => vec![
            cookie_option(
                lang,
                "auto",
                "Automatic imports browser cookies from Mistral Admin.",
                "",
                None,
            ),
            cookie_option(
                lang,
                "manual",
                "",
                "Paste a Cookie header from admin.mistral.ai.",
                None,
            ),
        ],
        _ => Vec::new(),
    }
}

/// Returns the API region options for a given provider.
/// Empty vec means the provider has no region picker.
pub fn region_options_for(provider_id: &str) -> Vec<RegionOption> {
    match provider_id {
        "alibaba" => vec![
            RegionOption {
                value: "intl".to_string(),
                label: "International (Model Studio)".to_string(),
            },
            RegionOption {
                value: "cn".to_string(),
                label: "China Mainland (Bailian)".to_string(),
            },
        ],
        "zai" => vec![
            RegionOption {
                value: "global".to_string(),
                label: "Global".to_string(),
            },
            RegionOption {
                value: "china".to_string(),
                label: "China Mainland (BigModel)".to_string(),
            },
        ],
        "minimax" => vec![
            RegionOption {
                value: "global".to_string(),
                label: "Global (.io)".to_string(),
            },
            RegionOption {
                value: "china".to_string(),
                label: "China Mainland (.com)".to_string(),
            },
        ],
        _ => Vec::new(),
    }
}

#[tauri::command]
pub fn get_provider_cookie_source_options(
    provider_id: String,
) -> Result<Vec<CookieSourceOption>, String> {
    let lang = Settings::load().ui_language;
    Ok(cookie_source_options_for(&provider_id, lang))
}

#[tauri::command]
pub fn get_provider_region_options(provider_id: String) -> Result<Vec<RegionOption>, String> {
    Ok(region_options_for(&provider_id))
}

// ── Credential detection ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiCliStatus {
    pub signed_in: bool,
    pub credentials_path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VertexAiStatus {
    pub has_credentials: bool,
    pub credentials_path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JetbrainsIde {
    pub id: String,
    pub display_name: String,
    pub path: String,
    pub detected: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KiroStatus {
    pub available: bool,
    pub hint: Option<String>,
}

fn gemini_cli_credentials_path() -> Option<std::path::PathBuf> {
    codexbar::host::session::gemini_cli_credentials_path()
}

fn vertexai_credentials_path_raw() -> Option<std::path::PathBuf> {
    codexbar::host::session::vertexai_credentials_path()
}

fn jetbrains_detected_ide_paths() -> Vec<std::path::PathBuf> {
    codexbar::host::session::jetbrains_detected_ide_paths()
}

#[tauri::command]
pub fn get_gemini_cli_signed_in() -> Result<GeminiCliStatus, String> {
    let path = gemini_cli_credentials_path();
    let signed_in = path.as_ref().map(|p| p.exists()).unwrap_or(false);
    Ok(GeminiCliStatus {
        signed_in,
        credentials_path: path.map(|p| p.to_string_lossy().into_owned()),
    })
}

#[tauri::command]
pub fn get_vertexai_status() -> Result<VertexAiStatus, String> {
    let path = vertexai_credentials_path_raw();
    let has = path.as_ref().map(|p| p.exists()).unwrap_or(false);
    Ok(VertexAiStatus {
        has_credentials: has,
        credentials_path: path.map(|p| p.to_string_lossy().into_owned()),
    })
}

#[tauri::command]
pub fn list_jetbrains_detected_ides() -> Result<Vec<JetbrainsIde>, String> {
    let settings = Settings::load();
    let override_path = settings.jetbrains_ide_base_path().to_string();

    let mut entries: Vec<JetbrainsIde> = jetbrains_detected_ide_paths()
        .into_iter()
        .map(|p| {
            let display = p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| p.display().to_string());
            JetbrainsIde {
                id: display.to_lowercase(),
                display_name: display,
                path: p.to_string_lossy().into_owned(),
                detected: true,
            }
        })
        .collect();

    // If the user has an override that isn't already in the detected list,
    // surface it explicitly with `detected: false`.
    if !override_path.is_empty() && !entries.iter().any(|e| e.path == override_path) {
        let path_buf = std::path::PathBuf::from(&override_path);
        let display = path_buf
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| override_path.clone());
        entries.push(JetbrainsIde {
            id: format!("override::{display}").to_lowercase(),
            display_name: display,
            path: override_path,
            detected: false,
        });
    }

    Ok(entries)
}

#[tauri::command]
pub fn set_jetbrains_ide_path(path: String) -> Result<(), String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err("JetBrains IDE path is empty".to_string());
    }
    let pb = std::path::PathBuf::from(trimmed);
    if !pb.is_absolute() {
        return Err("JetBrains IDE path must be absolute".to_string());
    }
    if !pb.is_dir() {
        return Err(format!("JetBrains IDE path is not a directory: {trimmed}"));
    }
    let mut settings = Settings::load();
    settings.set_jetbrains_ide_base_path(pb.to_string_lossy().into_owned());
    settings.save().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_kiro_status() -> Result<KiroStatus, String> {
    if let Some(path) = codexbar::providers::kiro::find_kiro_cli() {
        Ok(KiroStatus {
            available: true,
            hint: Some(path.to_string_lossy().into_owned()),
        })
    } else {
        Ok(KiroStatus {
            available: false,
            hint: Some("kiro-cli: not found on PATH or known install locations".into()),
        })
    }
}

/// Open a filesystem path in the OS file manager (Finder / Explorer /
/// xdg-open). Non-existent paths are rejected so the UI gets immediate
/// feedback instead of a silent no-op shell launch.
#[tauri::command]
pub fn open_path(path: String) -> Result<(), String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err("Path is empty".into());
    }
    let pb = std::path::PathBuf::from(trimmed);
    if !pb.is_absolute() {
        return Err("Path must be absolute".into());
    }
    if !pb.exists() {
        return Err(format!("Path not found: {trimmed}"));
    }
    // When given a file, open its parent directory so the file is highlighted
    // in a useful way across platforms without needing per-OS --select flags.
    let target = if pb.is_file() {
        pb.parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| pb.clone())
    } else {
        pb.clone()
    };
    let target_str = target.to_string_lossy().into_owned();

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new(windows_system_binary("explorer.exe"))
            .arg(&target_str)
            .spawn()
            .map_err(|e| format!("Failed to open path: {e}"))?;
    }
    #[cfg(not(target_os = "windows"))]
    {
        let opener = if cfg!(target_os = "macos") {
            "open"
        } else {
            "xdg-open"
        };
        std::process::Command::new(opener)
            .arg(&target_str)
            .spawn()
            .map_err(|e| format!("Failed to open path: {e}"))?;
    }
    Ok(())
}

// ── Global shortcut capture (user-driven, emits events) ───────────────

#[tauri::command]
pub fn register_global_shortcut(app: tauri::AppHandle, accelerator: String) -> Result<(), String> {
    use tauri::Emitter;
    use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

    let shortcut = crate::shortcut_bridge::parse_shortcut(&accelerator)
        .ok_or_else(|| format!("Invalid shortcut \"{accelerator}\". Use e.g. Ctrl+Shift+U."))?;

    // Best-effort cleanup of any prior capture registration.
    let _ = app.global_shortcut().unregister(shortcut);

    let accel_emit = accelerator.clone();
    app.global_shortcut()
        .on_shortcut(shortcut, move |app, _sc, event| {
            if event.state == ShortcutState::Pressed {
                let _ = app.emit("global-shortcut-triggered", accel_emit.clone());
            }
        })
        .map_err(|e| format!("Failed to register shortcut \"{accelerator}\": {e}"))?;

    Ok(())
}

#[tauri::command]
pub fn unregister_global_shortcut(app: tauri::AppHandle) -> Result<(), String> {
    use tauri_plugin_global_shortcut::GlobalShortcutExt;

    // We don't know which accelerator was registered — unregister_all is a
    // too-wide hammer (it would also drop the persistent tray-toggle binding),
    // so re-register that afterwards.
    app.global_shortcut()
        .unregister_all()
        .map_err(|e| format!("Failed to clear shortcuts: {e}"))?;
    crate::shortcut_bridge::register(&app);
    Ok(())
}

// ── Session / environment ─────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkAreaRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[tauri::command]
pub fn is_remote_session() -> Result<bool, String> {
    Ok(codexbar::host::session::is_ssh_session() || codexbar::host::session::is_remote_session())
}

#[tauri::command]
pub fn get_launch_block_reason() -> Result<Option<String>, String> {
    Ok(codexbar::host::session::current_launch_block_reason().map(|s| s.to_string()))
}

#[tauri::command]
pub fn get_work_area_rect(app: tauri::AppHandle) -> Result<WorkAreaRect, String> {
    use tauri::Manager;

    // Prefer the OS-native probe on Windows because it reliably excludes the
    // taskbar; Tauri's monitor API forwards to the same APIs but we keep the
    // direct path to preserve parity with the egui build.
    if let Some(area) = codexbar::host::session::primary_work_area_pixels() {
        return Ok(WorkAreaRect {
            x: area.x,
            y: area.y,
            width: area.width,
            height: area.height,
        });
    }

    // Cross-platform fallback (macOS: NSScreen.visibleFrame; Linux: GTK /
    // X11 work-area) via Tauri's monitor wrapper. Require a window so tao's
    // screen backend is initialised.
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "Main window is not available".to_string())?;

    let monitor = window
        .current_monitor()
        .map_err(|e| e.to_string())?
        .or_else(|| window.primary_monitor().ok().flatten())
        .ok_or_else(|| "No monitor detected".to_string())?;

    let work_area = monitor.work_area();
    Ok(WorkAreaRect {
        x: work_area.position.x,
        y: work_area.position.y,
        width: work_area.size.width as i32,
        height: work_area.size.height as i32,
    })
}

// ── Misc UX ────────────────────────────────────────────────────────────

#[tauri::command]
pub fn play_notification_sound() -> Result<(), String> {
    // Use the shared sound helper, honouring the user's `sound_enabled` flag.
    let settings = Settings::load();
    codexbar::sound::play_alert(codexbar::sound::AlertSound::Success, &settings);
    Ok(())
}

/// Reposition the tray panel so its bottom-right corner stays anchored to
/// the system-tray area. Called from the frontend after dynamic resize.
#[tauri::command]
pub fn reanchor_tray_panel(app: tauri::AppHandle) -> Result<(), String> {
    use crate::window_positioner::{PanelSize, Rect};
    use tauri::Manager;

    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "main window unavailable".to_string())?;
    let scale = window.scale_factor().unwrap_or(1.0).max(1.0);

    // Use the window's current logical size (after JS resize).
    let outer = window.outer_size().map_err(|e| e.to_string())?;
    let panel_size = PanelSize {
        width: (outer.width as f64 / scale).round() as u32,
        height: (outer.height as f64 / scale).round() as u32,
    };

    // Prefer the saved tray anchor from a real click; fall back to
    // bottom-right of the primary work area.
    let monitor = window
        .primary_monitor()
        .ok()
        .flatten()
        .or_else(|| window.current_monitor().ok().flatten())
        .ok_or_else(|| "no monitor".to_string())?;

    let work_area = Rect {
        x: monitor.work_area().position.x,
        y: monitor.work_area().position.y,
        width: monitor.work_area().size.width,
        height: monitor.work_area().size.height,
    };

    let (x, y) = {
        let st = app.try_state::<std::sync::Mutex<crate::state::AppState>>();
        let anchor = st.and_then(|s| s.lock().ok()?.tray_anchor);
        if let Some(a) = anchor {
            crate::window_positioner::calculate_panel_position(
                &Rect {
                    x: a.x,
                    y: a.y,
                    width: a.width,
                    height: a.height,
                },
                &work_area,
                &panel_size,
                scale,
            )
        } else {
            // Bottom-right fallback
            crate::window_positioner::calculate_popout_position(
                None,
                &work_area,
                &panel_size,
                scale,
            )
        }
    };

    // Pass physical coordinates directly — tao converts PhysicalPosition
    // to OS logical internally by dividing by the window's scale factor.
    let pos = tauri::PhysicalPosition::new(x, y);
    tracing::debug!(
        "reanchor_tray_panel: panel={}x{} => ({},{})",
        panel_size.width,
        panel_size.height,
        pos.x,
        pos.y
    );
    let _ = window.set_position(pos);
    Ok(())
}

#[tauri::command]
pub fn quit_app(app: tauri::AppHandle) {
    app.exit(0);
}

fn dashboard_url_for_provider(provider_id: &str) -> Option<String> {
    codexbar::settings::get_api_key_providers()
        .into_iter()
        .find(|p| p.id.cli_name() == provider_id)
        .and_then(|p| p.dashboard_url.map(|s| s.to_string()))
}

fn status_page_url_for_provider(provider_id: &str) -> Option<String> {
    let id = ProviderId::from_cli_name(provider_id)?;
    let provider = instantiate_provider(id);
    provider.metadata().status_page_url.map(|s| s.to_string())
}

#[tauri::command]
pub fn open_provider_dashboard(provider_id: String) -> Result<(), String> {
    let provider_id = canonical_provider_arg(&provider_id)?;
    let url = dashboard_url_for_provider(&provider_id)
        .ok_or_else(|| format!("No dashboard URL registered for provider '{provider_id}'"))?;
    open_url_in_browser(&url)
}

#[tauri::command]
pub fn open_provider_status_page(provider_id: String) -> Result<(), String> {
    let provider_id = canonical_provider_arg(&provider_id)?;
    let url = status_page_url_for_provider(&provider_id)
        .ok_or_else(|| format!("No status page URL registered for provider '{provider_id}'"))?;
    open_url_in_browser(&url)
}

#[tauri::command]
pub fn trigger_provider_login(provider_id: String) -> Result<(), String> {
    let provider_id = canonical_provider_arg(&provider_id)?;
    // TODO(6b): replace fallthrough once LoginPhase events land. The login
    // runners live in `codexbar::login` but are async-oriented and tightly
    // coupled to the egui UI's phase callbacks. For the Tauri shell we
    // currently surface the dashboard URL.
    if let Some(url) = dashboard_url_for_provider(&provider_id) {
        return open_url_in_browser(&url);
    }
    Err(format!(
        "Login flow for '{provider_id}' is not yet wired through the Tauri shell"
    ))
}

// ── Provider detail pane (Phase 6b) ──────────────────────────────────

/// DTO for the provider detail pane in the Settings Providers tab.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderDetail {
    pub id: String,
    pub display_name: String,
    pub enabled: bool,

    // Identity
    pub email: Option<String>,
    pub plan: Option<String>,
    pub auth_type: Option<String>,
    pub source_label: Option<String>,
    pub organization: Option<String>,
    pub last_updated: Option<String>,

    // Usage windows — reuse existing RateWindowSnapshot shape.
    pub session: Option<RateWindowSnapshot>,
    pub weekly: Option<RateWindowSnapshot>,
    pub model_specific: Option<RateWindowSnapshot>,
    pub tertiary: Option<RateWindowSnapshot>,
    pub extra_rate_windows: Vec<NamedRateWindowSnapshot>,

    // Cost / pace.
    pub cost: Option<CostSnapshotBridge>,
    pub pace: Option<PaceSnapshot>,

    // Error / state.
    pub last_error: Option<String>,

    // URLs for quick-actions (button visibility).
    pub dashboard_url: Option<String>,
    pub status_page_url: Option<String>,
    pub buy_credits_url: Option<String>,

    // True if the shared backend has produced any snapshot yet.
    pub has_snapshot: bool,

    // Phase 6c — currently-persisted cookie source & region for round-tripping
    // into the settings UI pickers. `None` for providers that do not support
    // one of the pickers.
    pub cookie_source: Option<String>,
    pub region: Option<String>,
}

fn build_provider_detail(provider_id: &str) -> Result<ProviderDetail, String> {
    let id = parse_provider_arg(provider_id)?;

    let settings = Settings::load();
    let enabled = settings
        .enabled_providers
        .iter()
        .any(|p| p == id.cli_name());

    let provider = instantiate_provider(id);
    let metadata = provider.metadata();

    Ok(ProviderDetail {
        id: id.cli_name().to_string(),
        display_name: id.display_name().to_string(),
        enabled,
        email: None,
        plan: None,
        auth_type: None,
        source_label: None,
        organization: None,
        last_updated: None,
        session: None,
        weekly: None,
        model_specific: None,
        tertiary: None,
        extra_rate_windows: Vec::new(),
        cost: None,
        pace: None,
        last_error: None,
        dashboard_url: metadata.dashboard_url.map(|s| s.to_string()),
        status_page_url: metadata.status_page_url.map(|s| s.to_string()),
        // Buy-credits currently mirrors the dashboard URL for providers that
        // support credit top-ups; refine once a dedicated URL lands upstream.
        buy_credits_url: if metadata.supports_credits {
            metadata.dashboard_url.map(|s| s.to_string())
        } else {
            None
        },
        has_snapshot: false,
        cookie_source: provider_cookie_source_lookup(&settings, id.cli_name()),
        region: provider_region_lookup(&settings, id.cli_name()),
    })
}

#[tauri::command]
pub fn get_provider_detail(
    app: tauri::AppHandle,
    provider_id: String,
) -> Result<ProviderDetail, String> {
    let mut detail = build_provider_detail(&provider_id)?;

    // Merge the latest cached snapshot, if any.
    let state = app.state::<Mutex<AppState>>();
    if let Ok(guard) = state.lock()
        && let Some(snap) = guard
            .provider_cache
            .iter()
            .find(|s| s.provider_id == detail.id)
    {
        detail.email = snap.account_email.clone();
        detail.plan = snap.plan_name.clone();
        detail.organization = snap.account_organization.clone();
        detail.source_label = if snap.source_label.is_empty() {
            None
        } else {
            Some(snap.source_label.clone())
        };
        detail.last_updated = Some(snap.updated_at.clone());
        if snap.error.is_none() {
            detail.session = Some(snap.primary.clone());
            detail.weekly = snap.secondary.clone();
            detail.model_specific = snap.model_specific.clone();
            detail.tertiary = snap.tertiary.clone();
            detail.extra_rate_windows = snap.extra_rate_windows.clone();
            detail.cost = snap.cost.clone();
            detail.pace = snap.pace.clone();
        }
        detail.last_error = snap.error.clone();
        detail.has_snapshot = true;
    }

    Ok(detail)
}

#[tauri::command]
pub fn revoke_provider_credentials(provider_id: String) -> Result<(), String> {
    // Best-effort: drop every app-managed credential for this provider so the
    // caller can follow up with a fresh login or import. Missing entries are
    // silently ignored; only I/O errors propagate.
    let id = parse_provider_arg(&provider_id)?;
    let provider_id = id.cli_name();

    let mut keys = ApiKeys::load();
    keys.remove(provider_id);
    keys.save().map_err(|e| e.to_string())?;

    let mut cookies = ManualCookies::load();
    cookies.remove(provider_id);
    cookies.save().map_err(|e| e.to_string())?;

    let token_store = TokenAccountStore::new();
    let mut token_accounts = token_store.load().map_err(|e| e.to_string())?;
    if token_accounts.remove(&id).is_some() {
        token_store
            .save(&token_accounts)
            .map_err(|e| e.to_string())?;
    }

    Ok(())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialStorageStatusBridge {
    pub manual_cookies: String,
    pub api_keys: String,
    pub token_accounts: String,
}

fn credential_file_status_label(status: SecureFileStatus) -> String {
    match status {
        SecureFileStatus::Missing => "missing".to_string(),
        SecureFileStatus::Plaintext => "plaintext".to_string(),
        SecureFileStatus::Protected(protection) => format!("protected:{protection}"),
        SecureFileStatus::Unreadable(_) => "unreadable".to_string(),
    }
}

fn optional_credential_status(path: Option<std::path::PathBuf>) -> String {
    path.map(|path| credential_file_status_label(secure_file::status(&path)))
        .unwrap_or_else(|| "unavailable".to_string())
}

#[tauri::command]
pub fn get_credential_storage_status() -> CredentialStorageStatusBridge {
    CredentialStorageStatusBridge {
        manual_cookies: optional_credential_status(ManualCookies::cookies_path()),
        api_keys: optional_credential_status(ApiKeys::keys_path()),
        token_accounts: credential_file_status_label(secure_file::status(
            &TokenAccountStore::default_path(),
        )),
    }
}

// ── Locale / i18n commands ───────────────────────────────────────────

/// Snapshot of every localized UI string in a given language.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocaleStrings {
    /// Serialized language code (`"english"` or `"chinese"`).
    pub language: &'static str,
    /// Map of serialized `LocaleKey` variant name → localized text.
    pub entries: HashMap<&'static str, &'static str>,
}

fn locale_strings_for(lang: Language) -> LocaleStrings {
    let mut entries = HashMap::with_capacity(locale::LocaleKey::ALL.len());
    for (key, name) in locale::LocaleKey::ALL {
        entries.insert(*name, locale::get_text(lang, *key));
    }
    LocaleStrings {
        language: language_label(lang),
        entries,
    }
}

/// Return every UI string for the requested language.
///
/// When `language` is `None`, the user's current persisted language is used.
/// The `language` argument accepts either the short code (`"en"`, `"zh"`),
/// the persisted label (`"english"`, `"chinese"`), or the full name
/// (`"English"`, `"Chinese"`, `"中文"`).
#[tauri::command]
pub fn get_locale_strings(language: Option<String>) -> Result<LocaleStrings, String> {
    let lang = match language.as_deref() {
        None => locale::current_language(),
        Some(raw) => {
            parse_locale_language(raw).ok_or_else(|| format!("unknown language code: {raw}"))?
        }
    };
    Ok(locale_strings_for(lang))
}

fn parse_locale_language(raw: &str) -> Option<Language> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "en" | "en-us" | "english" => Some(Language::English),
        "zh" | "zh-cn" | "zh-hans" | "chinese" | "中文" => Some(Language::Chinese),
        _ => None,
    }
}

/// Persist the UI language and emit a `locale-changed` event so the
/// frontend can refetch its locale table without a restart.
#[tauri::command]
pub fn set_ui_language(app: tauri::AppHandle, language: String) -> Result<(), String> {
    let lang =
        parse_locale_language(&language).ok_or_else(|| format!("unknown language: {language}"))?;
    let mut settings = Settings::load();
    if settings.ui_language == lang {
        return Ok(());
    }
    settings.ui_language = lang;
    settings.save().map_err(|e| e.to_string())?;
    let _ = app.emit(events::LOCALE_CHANGED, language_label(lang));
    Ok(())
}

#[cfg(test)]
mod locale_tests {
    use super::*;

    #[test]
    fn locale_strings_roundtrip_english() {
        let bundle = locale_strings_for(Language::English);
        assert_eq!(bundle.language, "english");
        assert_eq!(
            bundle.entries.get("TabGeneral").copied(),
            Some("General"),
            "TabGeneral should resolve to English"
        );
        assert_eq!(bundle.entries.len(), locale::LocaleKey::ALL.len());
    }

    #[test]
    fn locale_strings_roundtrip_chinese() {
        let bundle = locale_strings_for(Language::Chinese);
        assert_eq!(bundle.language, "chinese");
        assert_eq!(bundle.entries.get("TabGeneral").copied(), Some("通用"));
        assert_eq!(bundle.entries.len(), locale::LocaleKey::ALL.len());
    }

    #[test]
    fn locale_strings_contains_every_variant() {
        let bundle = locale_strings_for(Language::English);
        for (_, name) in locale::LocaleKey::ALL {
            assert!(
                bundle.entries.contains_key(name),
                "missing key in locale bundle: {name}"
            );
        }
    }

    #[test]
    fn parse_locale_language_accepts_aliases() {
        assert!(matches!(
            parse_locale_language("en"),
            Some(Language::English)
        ));
        assert!(matches!(
            parse_locale_language("English"),
            Some(Language::English)
        ));
        assert!(matches!(
            parse_locale_language("zh"),
            Some(Language::Chinese)
        ));
        assert!(matches!(
            parse_locale_language("Chinese"),
            Some(Language::Chinese)
        ));
        assert!(matches!(
            parse_locale_language("中文"),
            Some(Language::Chinese)
        ));
        assert!(parse_locale_language("klingon").is_none());
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::{
        ProviderSummary, ProviderUsageSnapshot, apply_provider_order, bridge_commands,
        bridge_events, provider_cookie_source_lookup, provider_region_lookup,
        validate_surface_target,
    };
    use crate::surface::SurfaceMode;
    use crate::surface_target::SurfaceTarget;
    use codexbar::core::{
        ProviderAccountData, ProviderFetchResult, ProviderId, SourceMode, TokenAccount,
        instantiate_provider,
    };
    use codexbar::host::session::launch_block_reason;
    use codexbar::settings::{ApiKeys, Language, ManualCookies, Settings};

    #[test]
    fn validate_surface_target_accepts_matching_target() {
        let target = validate_surface_target(
            SurfaceMode::Settings,
            SurfaceTarget::Settings {
                tab: "apiKeys".into(),
            },
        )
        .unwrap();

        assert_eq!(
            target,
            SurfaceTarget::Settings {
                tab: "apiKeys".into()
            }
        );
    }

    #[test]
    fn validate_surface_target_rejects_mismatched_target() {
        let error = validate_surface_target(
            SurfaceMode::TrayPanel,
            SurfaceTarget::Settings {
                tab: "apiKeys".into(),
            },
        )
        .unwrap_err();

        assert!(error.contains("not valid for mode 'trayPanel'"));
    }

    #[test]
    fn validate_surface_target_rejects_hidden_mode() {
        let error =
            validate_surface_target(SurfaceMode::Hidden, SurfaceTarget::Summary).unwrap_err();

        assert!(error.contains("only supports visible surfaces"));
    }

    #[test]
    fn bootstrap_contract_lists_current_surface_commands() {
        let ids = bridge_commands()
            .into_iter()
            .map(|descriptor| descriptor.id)
            .collect::<Vec<_>>();

        assert!(ids.contains(&"set_surface_mode"));
        assert!(ids.contains(&"get_current_surface_mode"));
        assert!(ids.contains(&"get_current_surface_state"));
        assert!(ids.contains(&"get_app_info"));
        assert!(!ids.contains(&"get_proof_config"));
    }

    #[test]
    fn bootstrap_contract_lists_surface_mode_changed_event() {
        let ids = bridge_events()
            .into_iter()
            .map(|descriptor| descriptor.id)
            .collect::<Vec<_>>();

        assert!(ids.contains(&"surface-mode-changed"));
    }

    #[test]
    fn bootstrap_contract_lists_phase4_commands() {
        let ids = bridge_commands()
            .into_iter()
            .map(|descriptor| descriptor.id)
            .collect::<Vec<_>>();

        for expected in [
            "reorder_providers",
            "set_provider_cookie_source",
            "get_provider_cookie_source",
            "set_provider_region",
            "get_provider_region",
            "get_gemini_cli_signed_in",
            "get_vertexai_status",
            "list_jetbrains_detected_ides",
            "set_jetbrains_ide_path",
            "get_kiro_status",
            "register_global_shortcut",
            "unregister_global_shortcut",
            "is_remote_session",
            "get_launch_block_reason",
            "get_work_area_rect",
            "play_notification_sound",
            "open_provider_dashboard",
            "trigger_provider_login",
            "revoke_provider_credentials",
            "get_credential_storage_status",
        ] {
            assert!(ids.contains(&expected), "missing command id: {expected}");
        }
    }

    #[test]
    fn credential_status_labels_do_not_include_error_details() {
        assert_eq!(
            super::credential_file_status_label(codexbar::secure_file::SecureFileStatus::Missing),
            "missing"
        );
        assert_eq!(
            super::credential_file_status_label(codexbar::secure_file::SecureFileStatus::Plaintext),
            "plaintext"
        );
        assert_eq!(
            super::credential_file_status_label(
                codexbar::secure_file::SecureFileStatus::Protected(
                    "windows-dpapi-user".to_string(),
                )
            ),
            "protected:windows-dpapi-user"
        );
        assert_eq!(
            super::credential_file_status_label(
                codexbar::secure_file::SecureFileStatus::Unreadable(
                    "secret path / token".to_string(),
                )
            ),
            "unreadable"
        );
    }

    #[test]
    fn command_inputs_reject_invalid_provider_ids_before_storage_writes() {
        assert!(super::set_api_key("not-a-provider".into(), "sk-test".into(), None).is_err());
        assert!(super::set_manual_cookie("not-a-provider".into(), "a=b".into()).is_err());
        assert!(super::remove_api_key("bad\nprovider".into()).is_err());
        assert!(super::remove_manual_cookie("".into()).is_err());
    }

    #[test]
    fn command_inputs_reject_multiline_secrets() {
        assert!(super::set_api_key("openrouter".into(), "sk-test\nnext".into(), None).is_err());
        assert!(super::set_manual_cookie("codex".into(), "a=b\nc=d".into()).is_err());
    }

    #[test]
    fn command_inputs_reject_unknown_cookie_source_and_region_values() {
        assert!(super::set_provider_cookie_source("codex".into(), "browser".into()).is_err());
        assert!(super::set_provider_region("zai".into(), "moon".into()).is_err());
    }

    #[test]
    fn bootstrap_contract_lists_global_shortcut_event() {
        let ids = bridge_events()
            .into_iter()
            .map(|descriptor| descriptor.id)
            .collect::<Vec<_>>();

        assert!(ids.contains(&"global-shortcut-triggered"));
    }

    #[test]
    fn apply_provider_order_dedupes_and_appends_unknown_canonical() {
        // Request only "codex" and "claude" — remaining canonical ids should
        // be appended after, preserving canonical order.
        let order = apply_provider_order(&["codex".to_string(), "claude".to_string()]);
        assert_eq!(order[0], "codex");
        assert_eq!(order[1], "claude");
        // Every canonical id appears exactly once.
        let mut sorted = order.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), order.len());
        // Every canonical id is present.
        let canonical = codexbar::core::ProviderId::all()
            .iter()
            .map(|p| p.cli_name().to_string())
            .collect::<Vec<_>>();
        for id in &canonical {
            assert!(order.contains(id), "missing canonical id: {id}");
        }
    }

    #[test]
    fn apply_provider_order_ignores_unknown_ids() {
        let order = apply_provider_order(&["not-a-provider".to_string(), "codex".to_string()]);
        assert_eq!(order[0], "codex");
        assert!(!order.iter().any(|id| id == "not-a-provider"));
    }

    #[test]
    fn provider_summaries_reflect_settings_order() {
        let canonical_len = codexbar::core::ProviderId::all().len();
        let s = Settings::default();
        let summaries: Vec<ProviderSummary> = super::build_provider_summaries(&s);
        assert_eq!(summaries.len(), canonical_len);
        // Index is assigned in emission order.
        for (i, s) in summaries.iter().enumerate() {
            assert_eq!(s.order, i as u32);
        }
    }

    #[test]
    fn provider_cookie_source_lookup_roundtrips_known_providers() {
        let mut s = Settings::default();
        super::provider_cookie_source_set(&mut s, "codex", "cli-config".to_string()).unwrap();
        assert_eq!(
            provider_cookie_source_lookup(&s, "codex").as_deref(),
            Some("cli-config")
        );
        assert!(provider_cookie_source_lookup(&s, "unknown-provider").is_none());
    }

    #[test]
    fn provider_region_lookup_roundtrips_known_providers() {
        let mut s = Settings::default();
        super::provider_region_set(&mut s, "alibaba", "china".to_string()).unwrap();
        assert_eq!(
            provider_region_lookup(&s, "alibaba").as_deref(),
            Some("china")
        );
        // Non-regional providers return None.
        assert!(provider_region_lookup(&s, "claude").is_none());
    }

    #[test]
    fn provider_cookie_source_set_rejects_unknown_provider() {
        let mut s = Settings::default();
        let err = super::provider_cookie_source_set(&mut s, "nope", "x".into()).unwrap_err();
        assert!(err.contains("nope"));
    }

    #[test]
    fn fetch_context_defaults_to_manual_cookies_without_browser_import() {
        let settings = Settings::default();
        let cookies = ManualCookies::default();
        let api_keys = ApiKeys::default();
        let token_accounts = HashMap::new();

        let ctx = super::build_fetch_context(
            ProviderId::Cursor,
            &settings,
            &cookies,
            &api_keys,
            &token_accounts,
        );

        assert_eq!(ctx.source_mode, SourceMode::Cli);
        assert!(ctx.manual_cookie_header.is_none());
    }

    #[test]
    fn fetch_context_manual_cookie_uses_web_without_browser_import() {
        let settings = Settings::default();
        let mut cookies = ManualCookies::default();
        cookies.set("cursor", "session=abc123");
        let api_keys = ApiKeys::default();
        let token_accounts = HashMap::new();

        let ctx = super::build_fetch_context(
            ProviderId::Cursor,
            &settings,
            &cookies,
            &api_keys,
            &token_accounts,
        );

        assert_eq!(ctx.source_mode, SourceMode::Web);
        assert_eq!(ctx.manual_cookie_header.as_deref(), Some("session=abc123"));
    }

    #[test]
    fn fetch_context_token_account_uses_web_cookie_header() {
        let settings = Settings::default();
        let cookies = ManualCookies::default();
        let api_keys = ApiKeys::default();
        let mut token_accounts = HashMap::new();
        let mut data = ProviderAccountData::new();
        data.add_account(TokenAccount::new("Work", "abc123"));
        token_accounts.insert(ProviderId::Ollama, data);

        let ctx = super::build_fetch_context(
            ProviderId::Ollama,
            &settings,
            &cookies,
            &api_keys,
            &token_accounts,
        );

        assert_eq!(ctx.source_mode, SourceMode::Web);
        assert_eq!(
            ctx.manual_cookie_header.as_deref(),
            Some("__Secure-session=abc123")
        );
    }

    #[test]
    fn fetch_context_claude_oauth_token_account_uses_oauth() {
        let settings = Settings::default();
        let cookies = ManualCookies::default();
        let api_keys = ApiKeys::default();
        let mut token_accounts = HashMap::new();
        let mut data = ProviderAccountData::new();
        data.add_account(TokenAccount::new("Claude OAuth", "sk-ant-oat01-abc123"));
        token_accounts.insert(ProviderId::Claude, data);

        let ctx = super::build_fetch_context(
            ProviderId::Claude,
            &settings,
            &cookies,
            &api_keys,
            &token_accounts,
        );

        assert_eq!(ctx.source_mode, SourceMode::OAuth);
        assert!(ctx.manual_cookie_header.is_none());
        assert_eq!(ctx.api_key.as_deref(), Some("sk-ant-oat01-abc123"));
    }

    #[test]
    fn fetch_context_claude_session_token_account_uses_web_cookie() {
        let settings = Settings::default();
        let cookies = ManualCookies::default();
        let api_keys = ApiKeys::default();
        let mut token_accounts = HashMap::new();
        let mut data = ProviderAccountData::new();
        data.add_account(TokenAccount::new("Claude Web", "sessionKey=abc123"));
        token_accounts.insert(ProviderId::Claude, data);

        let ctx = super::build_fetch_context(
            ProviderId::Claude,
            &settings,
            &cookies,
            &api_keys,
            &token_accounts,
        );

        assert_eq!(ctx.source_mode, SourceMode::Web);
        assert_eq!(
            ctx.manual_cookie_header.as_deref(),
            Some("sessionKey=abc123")
        );
        assert!(ctx.api_key.is_none());
    }

    #[test]
    fn fetch_context_token_account_takes_precedence_over_manual_cookie() {
        let settings = Settings::default();
        let mut cookies = ManualCookies::default();
        cookies.set("cursor", "manual=old");
        let api_keys = ApiKeys::default();
        let mut token_accounts = HashMap::new();
        let mut data = ProviderAccountData::new();
        data.add_account(TokenAccount::new("Work", "WorkosCursorSessionToken=new"));
        token_accounts.insert(ProviderId::Cursor, data);

        let ctx = super::build_fetch_context(
            ProviderId::Cursor,
            &settings,
            &cookies,
            &api_keys,
            &token_accounts,
        );

        assert_eq!(ctx.source_mode, SourceMode::Web);
        assert_eq!(
            ctx.manual_cookie_header.as_deref(),
            Some("WorkosCursorSessionToken=new")
        );
    }

    #[test]
    fn provider_region_set_rejects_non_regional_provider() {
        let mut s = Settings::default();
        let err = super::provider_region_set(&mut s, "claude", "global".into()).unwrap_err();
        assert!(err.contains("claude"));
    }

    #[test]
    fn launch_block_reason_helper_returns_none_when_not_blocked() {
        assert!(launch_block_reason(false, false).is_none());
    }

    #[test]
    fn launch_block_reason_helper_prefers_ssh() {
        let msg = launch_block_reason(true, true).unwrap();
        assert!(msg.contains("SSH"));
    }

    // ── Phase 6b — provider detail pane ────────────────────────────

    #[test]
    fn build_provider_detail_populates_identity_urls() {
        let detail = super::build_provider_detail("claude").expect("known provider");
        assert_eq!(detail.id, "claude");
        assert_eq!(detail.display_name, "Claude");
        // Claude advertises a status page URL in its metadata.
        assert!(detail.status_page_url.is_some());
        // No snapshot yet — empty usage bars and no error.
        assert!(detail.session.is_none());
        assert!(detail.last_error.is_none());
        assert!(!detail.has_snapshot);
    }

    #[test]
    fn build_provider_detail_rejects_unknown_provider() {
        let err = super::build_provider_detail("not-a-provider").unwrap_err();
        assert!(err.contains("not-a-provider"));
    }

    #[test]
    fn provider_detail_roundtrips_through_serde() {
        let detail = super::build_provider_detail("codex").expect("known provider");
        let json = serde_json::to_string(&detail).expect("serialize");
        // camelCase rename survives the round-trip.
        assert!(json.contains("\"displayName\""));
        assert!(json.contains("\"hasSnapshot\""));
        assert!(json.contains("\"statusPageUrl\""));
    }

    #[test]
    fn pace_stage_serializes_to_snake_case_string() {
        use codexbar::core::PaceStage;
        assert_eq!(super::pace_stage_str(PaceStage::OnTrack), "on_track");
        assert_eq!(
            super::pace_stage_str(PaceStage::SlightlyAhead),
            "slightly_ahead"
        );
        assert_eq!(super::pace_stage_str(PaceStage::FarAhead), "far_ahead");
        assert_eq!(
            super::pace_stage_str(PaceStage::SlightlyBehind),
            "slightly_behind"
        );
        assert_eq!(super::pace_stage_str(PaceStage::Behind), "behind");
        assert_eq!(super::pace_stage_str(PaceStage::FarBehind), "far_behind");
    }

    #[test]
    fn bootstrap_contract_lists_phase6b_commands() {
        let ids = bridge_commands()
            .into_iter()
            .map(|descriptor| descriptor.id)
            .collect::<Vec<_>>();

        for expected in ["get_provider_detail", "open_provider_status_page"] {
            assert!(ids.contains(&expected), "missing command id: {expected}");
        }
    }

    #[test]
    fn bootstrap_contract_lists_chart_data_command() {
        let ids = bridge_commands()
            .into_iter()
            .map(|descriptor| descriptor.id)
            .collect::<Vec<_>>();
        assert!(
            ids.contains(&"get_provider_chart_data"),
            "get_provider_chart_data must be advertised to the bridge",
        );
    }

    #[test]
    fn bootstrap_contract_lists_stale_refresh_command() {
        let ids = bridge_commands()
            .into_iter()
            .map(|descriptor| descriptor.id)
            .collect::<Vec<_>>();
        assert!(
            ids.contains(&"refresh_providers_if_stale"),
            "refresh_providers_if_stale must be advertised to the bridge",
        );
    }

    #[test]
    fn provider_cache_is_fresh_inside_stale_window() {
        assert!(super::is_provider_cache_fresh(
            Some(std::time::Instant::now()),
            std::time::Duration::from_secs(30),
        ));
    }

    #[test]
    fn provider_cache_is_stale_when_missing_timestamp() {
        assert!(!super::is_provider_cache_fresh(
            None,
            std::time::Duration::from_secs(30),
        ));
    }

    #[test]
    fn provider_cache_is_stale_after_window() {
        assert!(!super::is_provider_cache_fresh(
            Some(std::time::Instant::now() - std::time::Duration::from_secs(31)),
            std::time::Duration::from_secs(30),
        ));
    }

    #[test]
    fn provider_cache_upsert_replaces_existing_provider() {
        let metadata = instantiate_provider(ProviderId::Codex).metadata().clone();
        let result = ProviderFetchResult {
            usage: codexbar::core::UsageSnapshot::new(codexbar::core::RateWindow::new(10.0)),
            cost: None,
            source_label: "CLI".to_string(),
        };
        let mut first =
            ProviderUsageSnapshot::from_fetch_result(ProviderId::Codex, &metadata, &result);
        let mut second = first.clone();
        first.error = Some("old".to_string());
        second.error = Some("new".to_string());

        let mut cache = vec![first];
        super::upsert_provider_cache(&mut cache, second);

        assert_eq!(cache.len(), 1);
        assert_eq!(cache[0].provider_id, "codex");
        assert_eq!(cache[0].error.as_deref(), Some("new"));
    }

    #[test]
    fn chart_data_serde_roundtrip_preserves_fields() {
        use super::{DailyCostPoint, DailyUsageBreakdown, ProviderChartData, ServiceUsagePoint};

        let original = ProviderChartData {
            provider_id: "codex".into(),
            cost_history: vec![
                DailyCostPoint {
                    date: "2025-01-01".into(),
                    value: 1.25,
                },
                DailyCostPoint {
                    date: "2025-01-02".into(),
                    value: 0.0,
                },
            ],
            credits_history: vec![DailyCostPoint {
                date: "2025-01-01".into(),
                value: 42.0,
            }],
            usage_breakdown: vec![DailyUsageBreakdown {
                day: "2025-01-01".into(),
                services: vec![
                    ServiceUsagePoint {
                        service: "gpt-4o".into(),
                        credits_used: 10.0,
                    },
                    ServiceUsagePoint {
                        service: "gpt-4o-mini".into(),
                        credits_used: 3.5,
                    },
                ],
                total_credits_used: 13.5,
            }],
        };

        let json = serde_json::to_string(&original).expect("serialize");
        assert!(
            json.contains("\"providerId\":\"codex\""),
            "camelCase providerId: {json}"
        );
        assert!(json.contains("\"costHistory\""));
        assert!(json.contains("\"creditsHistory\""));
        assert!(json.contains("\"usageBreakdown\""));
        assert!(json.contains("\"creditsUsed\":10.0"));
        assert!(json.contains("\"totalCreditsUsed\":13.5"));

        let back: ProviderChartData = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.provider_id, "codex");
        assert_eq!(back.cost_history.len(), 2);
        assert_eq!(back.cost_history[0].date, "2025-01-01");
        assert_eq!(back.credits_history[0].value, 42.0);
        assert_eq!(back.usage_breakdown[0].services.len(), 2);
        assert_eq!(back.usage_breakdown[0].total_credits_used, 13.5);
    }

    #[test]
    fn chart_data_for_unknown_provider_is_empty() {
        let data =
            super::get_provider_chart_data("this-provider-definitely-does-not-exist".into(), None);
        assert_eq!(data.provider_id, "this-provider-definitely-does-not-exist");
        assert!(data.credits_history.is_empty());
        assert!(data.usage_breakdown.is_empty());
    }

    #[test]
    fn chart_data_requires_account_email_for_codex() {
        let data = super::get_provider_chart_data("codex".into(), None);
        assert_eq!(data.provider_id, "codex");
        assert!(data.credits_history.is_empty());
        assert!(data.usage_breakdown.is_empty());
    }

    #[test]
    fn bootstrap_contract_lists_phase6c_commands() {
        let ids = bridge_commands()
            .into_iter()
            .map(|descriptor| descriptor.id)
            .collect::<Vec<_>>();
        for expected in [
            "get_provider_cookie_source_options",
            "get_provider_region_options",
        ] {
            assert!(ids.contains(&expected), "missing command id: {expected}");
        }
    }

    #[test]
    fn cookie_options_for_cookie_supporting_provider() {
        let opts = super::cookie_source_options_for("codex", Language::English);
        let values: Vec<_> = opts.iter().map(|o| o.value.as_str()).collect();
        assert_eq!(values, vec!["auto", "manual", "off"]);
        assert!(opts.iter().any(|o| o.label == "Automatic"));
        assert!(opts.iter().any(|o| o.label == "Manual"));
        assert!(opts.iter().any(|o| o.label == "Disabled"));
    }

    #[test]
    fn cookie_options_empty_for_providers_without_picker() {
        assert!(super::cookie_source_options_for("anthropic", Language::English).is_empty());
        assert!(super::cookie_source_options_for("unknown", Language::English).is_empty());
    }

    #[test]
    fn region_options_for_regional_provider() {
        let opts = super::region_options_for("alibaba");
        let values: Vec<_> = opts.iter().map(|o| o.value.as_str()).collect();
        assert_eq!(values, vec!["intl", "cn"]);
    }

    #[test]
    fn region_options_empty_for_non_regional_provider() {
        assert!(super::region_options_for("claude").is_empty());
        assert!(super::region_options_for("codex").is_empty());
    }

    #[test]
    fn cookie_source_option_roundtrips_serde() {
        let opt = super::CookieSourceOption {
            value: "auto".to_string(),
            label: "Automatic".to_string(),
            description: Some("Imports browser cookies.".to_string()),
        };
        let json = serde_json::to_string(&opt).unwrap();
        let back: super::CookieSourceOption = serde_json::from_str(&json).unwrap();
        assert_eq!(opt, back);
    }

    #[test]
    fn region_option_roundtrips_serde() {
        let opt = super::RegionOption {
            value: "intl".to_string(),
            label: "International".to_string(),
        };
        let json = serde_json::to_string(&opt).unwrap();
        let back: super::RegionOption = serde_json::from_str(&json).unwrap();
        assert_eq!(opt, back);
    }

    // ── Phase 6d — credential detection UIs ────────────────────────

    #[test]
    fn bootstrap_contract_lists_phase6d_open_path() {
        let ids = bridge_commands()
            .into_iter()
            .map(|descriptor| descriptor.id)
            .collect::<Vec<_>>();
        assert!(ids.contains(&"open_path"));
    }

    #[test]
    fn open_path_rejects_empty_path() {
        let err = super::open_path(String::new()).unwrap_err();
        assert!(err.to_lowercase().contains("empty"));
    }

    #[test]
    fn open_path_rejects_relative_path() {
        let err = super::open_path("relative/path".into()).unwrap_err();
        assert!(err.contains("absolute"));
    }

    #[test]
    fn open_path_rejects_missing_path() {
        let missing = std::env::temp_dir()
            .join(format!("codexbar-phase6d-missing-{}", std::process::id()))
            .join("does-not-exist");
        let err = super::open_path(missing.to_string_lossy().into_owned()).unwrap_err();
        assert!(err.contains("not found"));
    }

    #[test]
    fn external_url_validator_accepts_http_and_https() {
        assert_eq!(
            super::validate_external_url(" https://github.com/Finesssee/Win-CodexBar "),
            Ok("https://github.com/Finesssee/Win-CodexBar")
        );
        assert_eq!(
            super::validate_external_url("http://localhost:1420"),
            Ok("http://localhost:1420")
        );
    }

    #[test]
    fn external_url_validator_rejects_non_web_and_control_urls() {
        assert!(super::validate_external_url("file:///C:/Windows/win.ini").is_err());
        assert!(super::validate_external_url("javascript:alert(1)").is_err());
        assert!(super::validate_external_url("https://example.com/\nmalicious").is_err());
    }

    // ── Phase 13 — E2E IPC harness ─────────────────────────────────
    //
    // Build the full bootstrap payload and prove that every shared
    // `ProviderId` variant ends up in the provider catalog with a
    // non-empty id + display name. If a new provider is added to the
    // enum but never wired through the desktop catalog, this test will
    // fail with `missing provider in bootstrap catalog: <id>`.

    #[test]
    fn bootstrap_payload_exposes_every_provider_variant() {
        let payload = super::get_bootstrap_state();

        let catalog_ids: std::collections::HashSet<String> = payload
            .providers
            .iter()
            .map(|entry| entry.id.clone())
            .collect();

        for entry in &payload.providers {
            assert!(!entry.id.is_empty(), "provider entry has empty id");
            assert!(
                !entry.display_name.is_empty(),
                "provider {} has empty display_name",
                entry.id
            );
        }

        for provider in ProviderId::all() {
            let expected = provider.cli_name().to_string();
            assert!(
                catalog_ids.contains(&expected),
                "missing provider in bootstrap catalog: {expected}"
            );
        }

        assert_eq!(
            catalog_ids.len(),
            ProviderId::all().len(),
            "bootstrap catalog size drifted from ProviderId::all()"
        );

        // Sanity — payload must also round-trip through JSON cleanly so
        // the TypeScript bridge never sees a partially-populated record.
        let encoded = serde_json::to_string(&payload).expect("serialize bootstrap");
        assert!(encoded.contains("contractVersion"));
        assert!(encoded.contains("\"providers\""));
        assert!(encoded.contains("\"settings\""));
    }
}
