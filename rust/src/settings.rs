//! Settings management for CodexBar
//!
//! Handles persistent configuration including:
//! - Enabled/disabled providers
//! - Refresh interval
//! - Manual cookies
//! - Other user preferences

#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use crate::core::ProviderId;

/// UI language for the application
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    /// English (default)
    #[default]
    English,
    /// Chinese (Simplified)
    Chinese,
}

impl Language {
    /// Get the display name for this language
    pub fn display_name(&self) -> &'static str {
        match self {
            Language::English => "English",
            Language::Chinese => "中文",
        }
    }

    /// Get all available languages
    pub fn all() -> &'static [Language] {
        &[Language::English, Language::Chinese]
    }
}

/// UI theme preference (Phase 12).
///
/// `Auto` resolves at runtime via `prefers-color-scheme` in the frontend;
/// `Light` and `Dark` are explicit overrides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ThemePreference {
    #[default]
    Auto,
    Light,
    Dark,
}

impl ThemePreference {
    pub fn all() -> &'static [ThemePreference] {
        &[
            ThemePreference::Auto,
            ThemePreference::Light,
            ThemePreference::Dark,
        ]
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            ThemePreference::Auto => "Auto",
            ThemePreference::Light => "Light",
            ThemePreference::Dark => "Dark",
        }
    }
}

/// Update channel for receiving updates
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum UpdateChannel {
    #[default]
    Stable,
    Beta,
}

impl UpdateChannel {
    /// Get the display name for this channel
    pub fn display_name(&self) -> &'static str {
        match self {
            UpdateChannel::Stable => "Stable",
            UpdateChannel::Beta => "Beta",
        }
    }

    /// Get a description for this channel
    pub fn description(&self) -> &'static str {
        match self {
            UpdateChannel::Stable => "Receive stable, tested releases",
            UpdateChannel::Beta => "Get early access to new features",
        }
    }
}

/// Tray icon display mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum TrayIconMode {
    /// Single tray icon showing the primary provider or merged view
    #[default]
    Single,
    /// One tray icon per enabled provider
    PerProvider,
}

impl TrayIconMode {
    /// Get the display name for this mode
    pub fn display_name(&self) -> &'static str {
        match self {
            TrayIconMode::Single => "Single Icon",
            TrayIconMode::PerProvider => "Per Provider",
        }
    }

    /// Get a description for this mode
    pub fn description(&self) -> &'static str {
        match self {
            TrayIconMode::Single => "Show one tray icon for all providers",
            TrayIconMode::PerProvider => "Show a separate tray icon for each enabled provider",
        }
    }
}

/// Metric preference for display in tray and UI
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum MetricPreference {
    #[default]
    Automatic,
    Session,
    Weekly,
    Model,
    Tertiary,
    Credits,
    #[serde(rename = "extraUsage", alias = "extrausage")]
    ExtraUsage,
    Average,
}

impl MetricPreference {
    /// Get all available metric preferences
    pub fn all() -> &'static [MetricPreference] {
        &[
            MetricPreference::Automatic,
            MetricPreference::Session,
            MetricPreference::Weekly,
            MetricPreference::Model,
            MetricPreference::Tertiary,
            MetricPreference::Credits,
            MetricPreference::ExtraUsage,
            MetricPreference::Average,
        ]
    }

    /// Get the display name for this metric
    pub fn display_name(&self) -> &'static str {
        match self {
            MetricPreference::Automatic => "Automatic",
            MetricPreference::Session => "Session",
            MetricPreference::Weekly => "Weekly",
            MetricPreference::Model => "Model",
            MetricPreference::Tertiary => "Tertiary",
            MetricPreference::Credits => "Credits",
            MetricPreference::ExtraUsage => "Extra usage",
            MetricPreference::Average => "Average",
        }
    }

    /// Get a description for this metric
    pub fn description(&self) -> &'static str {
        match self {
            MetricPreference::Automatic => "Automatically select the best metric",
            MetricPreference::Session => "Current session usage",
            MetricPreference::Weekly => "Weekly usage limit",
            MetricPreference::Model => "Model-specific limit",
            MetricPreference::Tertiary => "Tertiary usage limit",
            MetricPreference::Credits => "Credit balance",
            MetricPreference::ExtraUsage => "On-demand or extra usage budget",
            MetricPreference::Average => "Average across metrics",
        }
    }
}

/// Per-provider configuration values.
///
/// All fields are optional / falsy-default so unused providers serialize as
/// empty objects (or skip serialization entirely). Defaults are applied via
/// the accessor methods on [`Settings`] (e.g. cookie source defaults to
/// `"auto"`, region defaults are provider-specific).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProviderConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cookie_source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage_source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_region: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manual_cookie_header: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ide_base_path: Option<String>,
    /// Codex-only: opt out of OpenAI web "extras" surfaces.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub openai_web_extras: Option<bool>,
    /// Codex-only: enable historical usage tracking in UI.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub historical_tracking: bool,
    /// Claude-only: avoid keychain prompts when reading credentials.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub avoid_keychain_prompts: bool,
}

/// Application settings
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(from = "RawSettings", default)]
pub struct Settings {
    /// Enabled provider IDs (by CLI name)
    pub enabled_providers: HashSet<String>,

    /// Refresh interval in seconds (0 = manual only)
    pub refresh_interval_secs: u64,

    /// Whether to start minimized
    pub start_minimized: bool,

    /// Whether to start at login
    pub start_at_login: bool,

    /// Whether to show notifications
    pub show_notifications: bool,

    /// Whether to play sound effects for threshold alerts
    pub sound_enabled: bool,

    /// Sound volume for alerts (0-100)
    pub sound_volume: u8,

    /// High usage threshold for warnings (percentage)
    pub high_usage_threshold: f64,

    /// Critical usage threshold for alerts (percentage)
    pub critical_usage_threshold: f64,

    /// Merge mode: show all enabled providers in a single tray icon
    pub merge_tray_icons: bool,

    /// Tray icon display mode: single icon or per-provider icons
    #[serde(default)]
    pub tray_icon_mode: TrayIconMode,

    /// Show provider icons in the merged switcher UI
    #[serde(default = "default_true")]
    pub switcher_shows_icons: bool,

    /// Prefer the provider closest to its limit in merged menu bar display
    #[serde(default)]
    pub menu_bar_shows_highest_usage: bool,

    /// Replace bar-only tray display with provider branding plus percent text where supported
    #[serde(default)]
    pub menu_bar_shows_percent: bool,

    /// Show usage bars as "used" (true) or "remaining" (false)
    pub show_as_used: bool,

    /// Enable random "surprise" animations (blinks, wiggles)
    pub surprise_animations: bool,

    /// Enable UI animations (chart entrances, transitions)
    pub enable_animations: bool,

    /// Show reset times as relative (e.g., "2h 30m" instead of "3:00 PM")
    pub reset_time_relative: bool,

    /// Menu bar display mode: "minimal", "compact", or "detailed"
    pub menu_bar_display_mode: String,

    /// Show credits and extra usage information in the UI
    pub show_credits_extra_usage: bool,

    /// Show all token accounts in provider menus instead of collapsing behind switchers
    #[serde(default)]
    pub show_all_token_accounts_in_menu: bool,

    /// Per-provider configuration map (cookie/usage source, region, manual
    /// headers, API tokens, etc). Replaces the legacy flat per-provider
    /// fields; legacy `settings.json` files are migrated via [`RawSettings`].
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub provider_configs: HashMap<ProviderId, ProviderConfig>,

    /// Show debug-oriented settings and troubleshooting surfaces
    #[serde(default)]
    pub show_debug_settings: bool,

    /// Disable credential/keychain-style reads where supported
    #[serde(default)]
    pub disable_keychain_access: bool,

    /// Hide personal info (emails, account names) for streaming/sharing
    pub hide_personal_info: bool,

    /// Update channel for receiving updates (Stable or Beta)
    pub update_channel: UpdateChannel,

    /// Per-provider metric preference for tray display
    #[serde(default)]
    pub provider_metrics: HashMap<String, MetricPreference>,

    /// Preferred display order of provider IDs (CLI names).
    ///
    /// An empty list means "fall back to the canonical `ProviderId::all()`
    /// order". Unknown or duplicated ids are filtered out on load; new
    /// providers are appended in their canonical order.
    #[serde(default)]
    pub provider_order: Vec<String>,

    /// Global keyboard shortcut to open the menu (e.g., "Ctrl+Shift+U")
    #[serde(default = "default_global_shortcut")]
    pub global_shortcut: String,

    /// Automatically download updates in the background
    #[serde(default)]
    pub auto_download_updates: bool,

    /// Install pending updates when quitting the application
    #[serde(default)]
    pub install_updates_on_quit: bool,

    /// UI language for the application (English default for backward compatibility)
    #[serde(default)]
    pub ui_language: Language,

    /// UI theme preference (Phase 12). Defaults to Auto (prefers-color-scheme).
    #[serde(default)]
    pub theme: ThemePreference,

    /// Show the always-on-top floating capacity bar.
    #[serde(default)]
    pub float_bar_enabled: bool,

    /// Opacity of the floating bar window, in the inclusive range 30..=100.
    /// Stored as `u8` so the on-disk format remains stable.
    #[serde(default = "default_float_bar_opacity")]
    pub float_bar_opacity: u8,

    /// Floating-bar orientation: "horizontal" (default) or "vertical".
    #[serde(default = "default_float_bar_orientation")]
    pub float_bar_orientation: String,

    /// When true the floating bar is fully click-through (overlay mode).
    #[serde(default)]
    pub float_bar_click_through: bool,

    /// Provider CLI names to display in the floating bar. Empty = all enabled.
    #[serde(default)]
    pub float_bar_provider_ids: Vec<String>,

    /// When true, the floating bar uses a dark-on-light palette so it
    /// stays legible on light desktop backgrounds. Defaults to false
    /// (light-on-dark, the original look).
    #[serde(default)]
    pub float_bar_dark_text: bool,
}

fn default_float_bar_opacity() -> u8 {
    80
}

fn default_float_bar_orientation() -> String {
    "horizontal".to_string()
}

/// Clamp the floating-bar opacity to the supported range.
///
/// Opacity values below 30% would make the bar effectively invisible, so we
/// pin the lower bound; the upper bound is the natural 100%.
pub fn clamp_float_bar_opacity(value: u8) -> u8 {
    value.clamp(30, 100)
}

/// Normalize a floating-bar orientation string. Unknown values fall back to
/// the default ("horizontal") so a corrupt settings file can't put the
/// renderer into an undefined state.
pub fn normalize_float_bar_orientation(value: &str) -> String {
    match value {
        "vertical" => "vertical".to_string(),
        _ => "horizontal".to_string(),
    }
}

fn default_global_shortcut() -> String {
    "Ctrl+Shift+U".to_string()
}

fn default_true() -> bool {
    true
}

/// Default cookie source value for browser-authenticated providers.
///
/// Browser cookie extraction reads browser profile databases and decrypts
/// Chromium cookies via Windows DPAPI, which can trigger behavior-based AV
/// engines. Keep that path explicit opt-in by default.
const DEFAULT_COOKIE_SOURCE: &str = "manual";

/// Default usage source value for any provider.
const DEFAULT_PROVIDER_SOURCE: &str = "auto";

/// Default API region for providers that expose one.
fn default_api_region(id: ProviderId) -> &'static str {
    match id {
        ProviderId::Alibaba => "intl",
        ProviderId::Zai | ProviderId::MiniMax => "global",
        _ => "",
    }
}

/// Default for the codex `openai_web_extras` boolean (true = show extras).
const DEFAULT_CODEX_OPENAI_WEB_EXTRAS: bool = true;

/// Raw on-disk shape of [`Settings`] used purely for deserialization.
///
/// It mirrors the canonical `Settings` fields but ALSO accepts the legacy
/// flat per-provider fields (`codex_cookie_source`, `alibaba_api_region`,
/// `claude_avoid_keychain_prompts`, …) so existing `settings.json` files keep
/// loading. The `From<RawSettings> for Settings` impl folds any present
/// legacy field into the unified [`provider_configs`](Settings::provider_configs)
/// map.
///
/// Saves go through `Settings`'s derived `Serialize`, which writes only the
/// new format (no legacy flat fields).
#[derive(Debug, Deserialize)]
#[serde(default)]
struct RawSettings {
    enabled_providers: HashSet<String>,
    refresh_interval_secs: u64,
    start_minimized: bool,
    start_at_login: bool,
    show_notifications: bool,
    sound_enabled: bool,
    sound_volume: u8,
    high_usage_threshold: f64,
    critical_usage_threshold: f64,
    merge_tray_icons: bool,
    tray_icon_mode: TrayIconMode,
    #[serde(default = "default_true")]
    switcher_shows_icons: bool,
    menu_bar_shows_highest_usage: bool,
    menu_bar_shows_percent: bool,
    show_as_used: bool,
    surprise_animations: bool,
    enable_animations: bool,
    reset_time_relative: bool,
    menu_bar_display_mode: String,
    show_credits_extra_usage: bool,
    show_all_token_accounts_in_menu: bool,

    // ── New unified per-provider map ─────────────────────────────────
    provider_configs: HashMap<ProviderId, ProviderConfig>,

    // ── Legacy flat per-provider fields (migrated on load) ───────────
    #[serde(default)]
    claude_usage_source: Option<String>,
    #[serde(default)]
    codex_usage_source: Option<String>,
    #[serde(default)]
    codex_cookie_source: Option<String>,
    #[serde(default)]
    codex_historical_tracking: Option<bool>,
    #[serde(default)]
    codex_openai_web_extras: Option<bool>,
    #[serde(default)]
    claude_cookie_source: Option<String>,
    #[serde(default)]
    cursor_cookie_source: Option<String>,
    #[serde(default)]
    opencode_cookie_source: Option<String>,
    #[serde(default)]
    opencode_workspace_id: Option<String>,
    #[serde(default)]
    factory_cookie_source: Option<String>,
    #[serde(default)]
    alibaba_cookie_source: Option<String>,
    #[serde(default)]
    alibaba_cookie_header: Option<String>,
    #[serde(default)]
    alibaba_api_region: Option<String>,
    #[serde(default)]
    kimi_cookie_source: Option<String>,
    #[serde(default)]
    kimi_manual_cookie_header: Option<String>,
    #[serde(default)]
    minimax_cookie_source: Option<String>,
    #[serde(default)]
    augment_cookie_source: Option<String>,
    #[serde(default)]
    augment_cookie_header: Option<String>,
    #[serde(default)]
    amp_cookie_source: Option<String>,
    #[serde(default)]
    amp_cookie_header: Option<String>,
    #[serde(default)]
    ollama_cookie_source: Option<String>,
    #[serde(default)]
    ollama_cookie_header: Option<String>,
    #[serde(default)]
    zai_api_region: Option<String>,
    #[serde(default)]
    jetbrains_ide_base_path: Option<String>,
    #[serde(default)]
    minimax_cookie_header: Option<String>,
    #[serde(default)]
    minimax_api_token: Option<String>,
    #[serde(default)]
    minimax_api_region: Option<String>,
    #[serde(default)]
    claude_avoid_keychain_prompts: Option<bool>,

    show_debug_settings: bool,
    disable_keychain_access: bool,
    hide_personal_info: bool,
    update_channel: UpdateChannel,
    provider_metrics: HashMap<String, MetricPreference>,
    provider_order: Vec<String>,
    #[serde(default = "default_global_shortcut")]
    global_shortcut: String,
    auto_download_updates: bool,
    install_updates_on_quit: bool,
    ui_language: Language,
    theme: ThemePreference,

    #[serde(default)]
    float_bar_enabled: bool,
    #[serde(default = "default_float_bar_opacity")]
    float_bar_opacity: u8,
    #[serde(default = "default_float_bar_orientation")]
    float_bar_orientation: String,
    #[serde(default)]
    float_bar_click_through: bool,
    #[serde(default)]
    float_bar_provider_ids: Vec<String>,
    #[serde(default)]
    float_bar_dark_text: bool,
}

impl Default for RawSettings {
    fn default() -> Self {
        let s = Settings::default();
        Self {
            enabled_providers: s.enabled_providers,
            refresh_interval_secs: s.refresh_interval_secs,
            start_minimized: s.start_minimized,
            start_at_login: s.start_at_login,
            show_notifications: s.show_notifications,
            sound_enabled: s.sound_enabled,
            sound_volume: s.sound_volume,
            high_usage_threshold: s.high_usage_threshold,
            critical_usage_threshold: s.critical_usage_threshold,
            merge_tray_icons: s.merge_tray_icons,
            tray_icon_mode: s.tray_icon_mode,
            switcher_shows_icons: s.switcher_shows_icons,
            menu_bar_shows_highest_usage: s.menu_bar_shows_highest_usage,
            menu_bar_shows_percent: s.menu_bar_shows_percent,
            show_as_used: s.show_as_used,
            surprise_animations: s.surprise_animations,
            enable_animations: s.enable_animations,
            reset_time_relative: s.reset_time_relative,
            menu_bar_display_mode: s.menu_bar_display_mode,
            show_credits_extra_usage: s.show_credits_extra_usage,
            show_all_token_accounts_in_menu: s.show_all_token_accounts_in_menu,
            provider_configs: s.provider_configs,
            claude_usage_source: None,
            codex_usage_source: None,
            codex_cookie_source: None,
            codex_historical_tracking: None,
            codex_openai_web_extras: None,
            claude_cookie_source: None,
            cursor_cookie_source: None,
            opencode_cookie_source: None,
            opencode_workspace_id: None,
            factory_cookie_source: None,
            alibaba_cookie_source: None,
            alibaba_cookie_header: None,
            alibaba_api_region: None,
            kimi_cookie_source: None,
            kimi_manual_cookie_header: None,
            minimax_cookie_source: None,
            augment_cookie_source: None,
            augment_cookie_header: None,
            amp_cookie_source: None,
            amp_cookie_header: None,
            ollama_cookie_source: None,
            ollama_cookie_header: None,
            zai_api_region: None,
            jetbrains_ide_base_path: None,
            minimax_cookie_header: None,
            minimax_api_token: None,
            minimax_api_region: None,
            claude_avoid_keychain_prompts: None,
            show_debug_settings: s.show_debug_settings,
            disable_keychain_access: s.disable_keychain_access,
            hide_personal_info: s.hide_personal_info,
            update_channel: s.update_channel,
            provider_metrics: s.provider_metrics,
            provider_order: s.provider_order,
            global_shortcut: s.global_shortcut,
            auto_download_updates: s.auto_download_updates,
            install_updates_on_quit: s.install_updates_on_quit,
            ui_language: s.ui_language,
            theme: s.theme,
            float_bar_enabled: s.float_bar_enabled,
            float_bar_opacity: s.float_bar_opacity,
            float_bar_orientation: s.float_bar_orientation,
            float_bar_click_through: s.float_bar_click_through,
            float_bar_provider_ids: s.float_bar_provider_ids,
            float_bar_dark_text: s.float_bar_dark_text,
        }
    }
}

impl From<RawSettings> for Settings {
    fn from(raw: RawSettings) -> Self {
        let mut provider_configs = raw.provider_configs;

        // Helper closures to lazily insert per-provider configs from legacy
        // flat fields. Existing `provider_configs` entries take precedence.
        fn set_cookie_source(
            map: &mut HashMap<ProviderId, ProviderConfig>,
            id: ProviderId,
            value: Option<String>,
        ) {
            if let Some(v) = value {
                let entry = map.entry(id).or_default();
                if entry.cookie_source.is_none() {
                    entry.cookie_source = Some(v);
                }
            }
        }
        fn set_usage_source(
            map: &mut HashMap<ProviderId, ProviderConfig>,
            id: ProviderId,
            value: Option<String>,
        ) {
            if let Some(v) = value {
                let entry = map.entry(id).or_default();
                if entry.usage_source.is_none() {
                    entry.usage_source = Some(v);
                }
            }
        }
        fn set_region(
            map: &mut HashMap<ProviderId, ProviderConfig>,
            id: ProviderId,
            value: Option<String>,
        ) {
            if let Some(v) = value {
                let entry = map.entry(id).or_default();
                if entry.api_region.is_none() {
                    entry.api_region = Some(v);
                }
            }
        }
        fn set_header(
            map: &mut HashMap<ProviderId, ProviderConfig>,
            id: ProviderId,
            value: Option<String>,
        ) {
            if let Some(v) = value {
                let entry = map.entry(id).or_default();
                if entry.manual_cookie_header.is_none() {
                    entry.manual_cookie_header = Some(v);
                }
            }
        }

        set_cookie_source(
            &mut provider_configs,
            ProviderId::Codex,
            raw.codex_cookie_source,
        );
        set_cookie_source(
            &mut provider_configs,
            ProviderId::Claude,
            raw.claude_cookie_source,
        );
        set_cookie_source(
            &mut provider_configs,
            ProviderId::Cursor,
            raw.cursor_cookie_source,
        );
        set_cookie_source(
            &mut provider_configs,
            ProviderId::OpenCode,
            raw.opencode_cookie_source,
        );
        set_cookie_source(
            &mut provider_configs,
            ProviderId::Factory,
            raw.factory_cookie_source,
        );
        set_cookie_source(
            &mut provider_configs,
            ProviderId::Alibaba,
            raw.alibaba_cookie_source,
        );
        set_cookie_source(
            &mut provider_configs,
            ProviderId::Kimi,
            raw.kimi_cookie_source,
        );
        set_cookie_source(
            &mut provider_configs,
            ProviderId::MiniMax,
            raw.minimax_cookie_source,
        );
        set_cookie_source(
            &mut provider_configs,
            ProviderId::Augment,
            raw.augment_cookie_source,
        );
        set_cookie_source(
            &mut provider_configs,
            ProviderId::Amp,
            raw.amp_cookie_source,
        );
        set_cookie_source(
            &mut provider_configs,
            ProviderId::Ollama,
            raw.ollama_cookie_source,
        );

        set_usage_source(
            &mut provider_configs,
            ProviderId::Claude,
            raw.claude_usage_source,
        );
        set_usage_source(
            &mut provider_configs,
            ProviderId::Codex,
            raw.codex_usage_source,
        );

        set_region(
            &mut provider_configs,
            ProviderId::Alibaba,
            raw.alibaba_api_region,
        );
        set_region(&mut provider_configs, ProviderId::Zai, raw.zai_api_region);
        set_region(
            &mut provider_configs,
            ProviderId::MiniMax,
            raw.minimax_api_region,
        );

        set_header(
            &mut provider_configs,
            ProviderId::Alibaba,
            raw.alibaba_cookie_header,
        );
        set_header(
            &mut provider_configs,
            ProviderId::Kimi,
            raw.kimi_manual_cookie_header,
        );
        set_header(
            &mut provider_configs,
            ProviderId::Augment,
            raw.augment_cookie_header,
        );
        set_header(
            &mut provider_configs,
            ProviderId::Amp,
            raw.amp_cookie_header,
        );
        set_header(
            &mut provider_configs,
            ProviderId::Ollama,
            raw.ollama_cookie_header,
        );
        set_header(
            &mut provider_configs,
            ProviderId::MiniMax,
            raw.minimax_cookie_header,
        );

        if let Some(v) = raw.opencode_workspace_id {
            let entry = provider_configs.entry(ProviderId::OpenCode).or_default();
            if entry.workspace_id.is_none() {
                entry.workspace_id = Some(v);
            }
        }
        if let Some(v) = raw.minimax_api_token {
            let entry = provider_configs.entry(ProviderId::MiniMax).or_default();
            if entry.api_token.is_none() {
                entry.api_token = Some(v);
            }
        }
        if let Some(v) = raw.jetbrains_ide_base_path {
            let entry = provider_configs.entry(ProviderId::JetBrains).or_default();
            if entry.ide_base_path.is_none() {
                entry.ide_base_path = Some(v);
            }
        }
        if let Some(v) = raw.codex_openai_web_extras {
            let entry = provider_configs.entry(ProviderId::Codex).or_default();
            if entry.openai_web_extras.is_none() {
                entry.openai_web_extras = Some(v);
            }
        }
        if let Some(v) = raw.codex_historical_tracking
            && v
        {
            provider_configs
                .entry(ProviderId::Codex)
                .or_default()
                .historical_tracking = true;
        }
        if let Some(v) = raw.claude_avoid_keychain_prompts
            && v
        {
            provider_configs
                .entry(ProviderId::Claude)
                .or_default()
                .avoid_keychain_prompts = true;
        }

        Settings {
            enabled_providers: raw.enabled_providers,
            refresh_interval_secs: raw.refresh_interval_secs,
            start_minimized: raw.start_minimized,
            start_at_login: raw.start_at_login,
            show_notifications: raw.show_notifications,
            sound_enabled: raw.sound_enabled,
            sound_volume: raw.sound_volume,
            high_usage_threshold: raw.high_usage_threshold,
            critical_usage_threshold: raw.critical_usage_threshold,
            merge_tray_icons: raw.merge_tray_icons,
            tray_icon_mode: raw.tray_icon_mode,
            switcher_shows_icons: raw.switcher_shows_icons,
            menu_bar_shows_highest_usage: raw.menu_bar_shows_highest_usage,
            menu_bar_shows_percent: raw.menu_bar_shows_percent,
            show_as_used: raw.show_as_used,
            surprise_animations: raw.surprise_animations,
            enable_animations: raw.enable_animations,
            reset_time_relative: raw.reset_time_relative,
            menu_bar_display_mode: raw.menu_bar_display_mode,
            show_credits_extra_usage: raw.show_credits_extra_usage,
            show_all_token_accounts_in_menu: raw.show_all_token_accounts_in_menu,
            provider_configs,
            show_debug_settings: raw.show_debug_settings,
            disable_keychain_access: raw.disable_keychain_access,
            hide_personal_info: raw.hide_personal_info,
            update_channel: raw.update_channel,
            provider_metrics: raw.provider_metrics,
            provider_order: raw.provider_order,
            global_shortcut: raw.global_shortcut,
            auto_download_updates: raw.auto_download_updates,
            install_updates_on_quit: raw.install_updates_on_quit,
            ui_language: raw.ui_language,
            theme: raw.theme,
            float_bar_enabled: raw.float_bar_enabled,
            float_bar_opacity: clamp_float_bar_opacity(raw.float_bar_opacity),
            float_bar_orientation: normalize_float_bar_orientation(&raw.float_bar_orientation),
            float_bar_click_through: raw.float_bar_click_through,
            float_bar_provider_ids: raw.float_bar_provider_ids,
            float_bar_dark_text: raw.float_bar_dark_text,
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        let mut enabled = HashSet::new();
        // Default enabled providers
        enabled.insert("claude".to_string());
        enabled.insert("codex".to_string());

        Self {
            enabled_providers: enabled,
            refresh_interval_secs: 300, // 5 minutes
            start_minimized: false,
            start_at_login: false,
            show_notifications: true,
            sound_enabled: true,
            sound_volume: 100,
            high_usage_threshold: 70.0,
            critical_usage_threshold: 90.0,
            merge_tray_icons: false, // Show single provider by default
            tray_icon_mode: TrayIconMode::default(), // Single icon by default
            switcher_shows_icons: true,
            menu_bar_shows_highest_usage: false,
            menu_bar_shows_percent: false,
            show_as_used: true,         // Show as "used" by default
            surprise_animations: false, // Off by default
            enable_animations: true,    // Animations enabled by default
            reset_time_relative: true,  // Show relative times by default
            menu_bar_display_mode: "detailed".to_string(), // Detailed mode by default
            show_credits_extra_usage: true, // Show credits + extra usage by default
            show_all_token_accounts_in_menu: false,
            provider_configs: HashMap::new(),
            show_debug_settings: false,
            disable_keychain_access: false,
            hide_personal_info: false, // Show personal info by default
            update_channel: UpdateChannel::default(), // Stable by default
            provider_metrics: HashMap::new(), // Empty = use Automatic for all
            provider_order: Vec::new(), // Empty = canonical ProviderId::all() order
            global_shortcut: default_global_shortcut(), // Ctrl+Shift+U by default
            auto_download_updates: false, // Require explicit opt-in for background downloads
            install_updates_on_quit: false, // Don't auto-install on quit by default
            ui_language: Language::default(), // English by default
            theme: ThemePreference::default(), // Auto (follows prefers-color-scheme)
            float_bar_enabled: false,
            float_bar_opacity: default_float_bar_opacity(),
            float_bar_orientation: default_float_bar_orientation(),
            float_bar_click_through: false,
            float_bar_provider_ids: Vec::new(),
            float_bar_dark_text: false,
        }
    }
}

impl Settings {
    /// Get the settings file path
    pub fn settings_path() -> Option<PathBuf> {
        dirs::config_dir().map(|p| p.join("CodexBar").join("settings.json"))
    }

    /// Load settings from disk
    pub fn load() -> Self {
        #[allow(unused_mut)]
        let mut settings = match Self::settings_path() {
            Some(path) if path.exists() => match crate::secure_file::read_string(&path) {
                Ok(content) => {
                    serde_json::from_str(content.trim_start_matches('\u{feff}')).unwrap_or_default()
                }
                Err(_) => Self::default(),
            },
            _ => Self::default(),
        };

        // Sync autostart toggle with actual registry state
        #[cfg(target_os = "windows")]
        {
            settings.start_at_login = Self::is_start_at_login_enabled();
        }

        settings
    }

    /// Save settings to disk
    pub fn save(&self) -> anyhow::Result<()> {
        let path = Self::settings_path()
            .ok_or_else(|| anyhow::anyhow!("Could not determine settings path"))?;

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let json = serde_json::to_string_pretty(self)?;
        crate::secure_file::write_string(&path, &json)?;

        Ok(())
    }

    fn start_at_login_command(exe_path: &std::path::Path) -> String {
        format!("\"{}\"", exe_path.display())
    }

    #[cfg(target_os = "windows")]
    pub fn apply_start_at_login_registry(enabled: bool) -> anyhow::Result<()> {
        use winreg::RegKey;
        use winreg::enums::*;

        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let run_key = hkcu.open_subkey_with_flags(
            r"Software\Microsoft\Windows\CurrentVersion\Run",
            KEY_READ | KEY_WRITE,
        )?;

        if enabled {
            let exe_path = std::env::current_exe()?;
            let command = Self::start_at_login_command(&exe_path);
            run_key.set_value("CodexBar", &command)?;
        } else {
            let _ = run_key.delete_value("CodexBar");
        }

        Ok(())
    }

    #[cfg(not(target_os = "windows"))]
    pub fn apply_start_at_login_registry(_enabled: bool) -> anyhow::Result<()> {
        Ok(())
    }

    /// Set start at login (updates Windows registry)
    pub fn set_start_at_login(&mut self, enabled: bool) -> anyhow::Result<()> {
        self.start_at_login = enabled;
        Self::apply_start_at_login_registry(enabled)?;
        Ok(())
    }

    /// Check if start at login is actually enabled in registry
    #[cfg(target_os = "windows")]
    pub fn is_start_at_login_enabled() -> bool {
        use winreg::RegKey;
        use winreg::enums::*;

        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        if let Ok(run_key) = hkcu.open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Run") {
            run_key.get_value::<String, _>("CodexBar").is_ok()
        } else {
            false
        }
    }

    #[cfg(not(target_os = "windows"))]
    pub fn is_start_at_login_enabled() -> bool {
        false
    }

    /// Check if a provider is enabled
    pub fn is_provider_enabled(&self, id: ProviderId) -> bool {
        self.enabled_providers.contains(id.cli_name())
    }

    /// Enable a provider
    pub fn enable_provider(&mut self, id: ProviderId) {
        self.enabled_providers.insert(id.cli_name().to_string());
    }

    /// Disable a provider
    pub fn disable_provider(&mut self, id: ProviderId) {
        self.enabled_providers.remove(id.cli_name());
    }

    /// Toggle a provider's enabled state
    pub fn toggle_provider(&mut self, id: ProviderId) -> bool {
        let name = id.cli_name().to_string();
        if self.enabled_providers.contains(&name) {
            self.enabled_providers.remove(&name);
            false
        } else {
            self.enabled_providers.insert(name);
            true
        }
    }

    /// Get list of enabled provider IDs
    pub fn get_enabled_provider_ids(&self) -> Vec<ProviderId> {
        ProviderId::all()
            .iter()
            .filter(|id| self.is_provider_enabled(**id))
            .copied()
            .collect()
    }

    /// Get all available providers with their enabled status
    pub fn get_all_providers_status(&self) -> Vec<ProviderStatus> {
        ProviderId::all()
            .iter()
            .map(|id| ProviderStatus {
                id: id.cli_name().to_string(),
                name: id.display_name().to_string(),
                enabled: self.is_provider_enabled(*id),
            })
            .collect()
    }

    /// Get the metric preference for a provider
    pub fn get_provider_metric(&self, id: ProviderId) -> MetricPreference {
        self.provider_metrics
            .get(id.cli_name())
            .copied()
            .unwrap_or_default()
    }

    /// Set the metric preference for a provider
    pub fn set_provider_metric(&mut self, id: ProviderId, metric: MetricPreference) {
        self.provider_metrics
            .insert(id.cli_name().to_string(), metric);
    }

    // ── Per-provider configuration accessors ─────────────────────────
    //
    // These thin wrappers around `provider_configs` apply provider-specific
    // defaults (e.g. cookie/usage source defaults to `"auto"`) so callers
    // never have to reach into the raw `Option<String>` fields. The
    // `*_str` / boolean / setter pairs intentionally mirror the names of
    // the legacy flat fields so call-site migration is mechanical.

    /// Read-only access to a provider's stored config, if any.
    pub fn provider_config(&self, id: ProviderId) -> Option<&ProviderConfig> {
        self.provider_configs.get(&id)
    }

    /// Mutable access to a provider's config, lazily creating an empty
    /// entry if none exists.
    pub fn provider_config_mut(&mut self, id: ProviderId) -> &mut ProviderConfig {
        self.provider_configs.entry(id).or_default()
    }

    /// Cookie source for `id`, or the default `"manual"` if unset.
    pub fn cookie_source(&self, id: ProviderId) -> &str {
        self.provider_configs
            .get(&id)
            .and_then(|c| c.cookie_source.as_deref())
            .unwrap_or(DEFAULT_COOKIE_SOURCE)
    }

    pub fn set_cookie_source(&mut self, id: ProviderId, source: impl Into<String>) {
        self.provider_config_mut(id).cookie_source = Some(source.into());
    }

    /// Usage source for `id`, or the default `"auto"` if unset.
    pub fn usage_source(&self, id: ProviderId) -> &str {
        self.provider_configs
            .get(&id)
            .and_then(|c| c.usage_source.as_deref())
            .unwrap_or(DEFAULT_PROVIDER_SOURCE)
    }

    pub fn set_usage_source(&mut self, id: ProviderId, source: impl Into<String>) {
        self.provider_config_mut(id).usage_source = Some(source.into());
    }

    /// API region for `id`, or the provider-specific default if unset.
    pub fn api_region(&self, id: ProviderId) -> &str {
        self.provider_configs
            .get(&id)
            .and_then(|c| c.api_region.as_deref())
            .unwrap_or_else(|| default_api_region(id))
    }

    pub fn set_api_region(&mut self, id: ProviderId, region: impl Into<String>) {
        self.provider_config_mut(id).api_region = Some(region.into());
    }

    /// Manual cookie header for `id`, or `""` if unset.
    pub fn manual_cookie_header(&self, id: ProviderId) -> &str {
        self.provider_configs
            .get(&id)
            .and_then(|c| c.manual_cookie_header.as_deref())
            .unwrap_or("")
    }

    pub fn set_manual_cookie_header(&mut self, id: ProviderId, header: impl Into<String>) {
        self.provider_config_mut(id).manual_cookie_header = Some(header.into());
    }

    /// API token for `id`, or `""` if unset.
    pub fn api_token(&self, id: ProviderId) -> &str {
        self.provider_configs
            .get(&id)
            .and_then(|c| c.api_token.as_deref())
            .unwrap_or("")
    }

    pub fn set_api_token(&mut self, id: ProviderId, token: impl Into<String>) {
        self.provider_config_mut(id).api_token = Some(token.into());
    }

    /// Workspace ID override for `id`, or `""` if unset.
    pub fn workspace_id(&self, id: ProviderId) -> &str {
        self.provider_configs
            .get(&id)
            .and_then(|c| c.workspace_id.as_deref())
            .unwrap_or("")
    }

    pub fn set_workspace_id(&mut self, id: ProviderId, value: impl Into<String>) {
        self.provider_config_mut(id).workspace_id = Some(value.into());
    }

    /// IDE base path override for `id`, or `""` if unset.
    pub fn ide_base_path(&self, id: ProviderId) -> &str {
        self.provider_configs
            .get(&id)
            .and_then(|c| c.ide_base_path.as_deref())
            .unwrap_or("")
    }

    pub fn set_ide_base_path(&mut self, id: ProviderId, value: impl Into<String>) {
        self.provider_config_mut(id).ide_base_path = Some(value.into());
    }

    /// Codex `openai_web_extras` toggle, default `true`.
    pub fn openai_web_extras(&self, id: ProviderId) -> bool {
        self.provider_configs
            .get(&id)
            .and_then(|c| c.openai_web_extras)
            .unwrap_or(DEFAULT_CODEX_OPENAI_WEB_EXTRAS)
    }

    pub fn set_openai_web_extras(&mut self, id: ProviderId, value: bool) {
        self.provider_config_mut(id).openai_web_extras = Some(value);
    }

    /// Per-provider historical-tracking toggle (currently codex-only).
    pub fn historical_tracking(&self, id: ProviderId) -> bool {
        self.provider_configs
            .get(&id)
            .map(|c| c.historical_tracking)
            .unwrap_or(false)
    }

    pub fn set_historical_tracking(&mut self, id: ProviderId, value: bool) {
        self.provider_config_mut(id).historical_tracking = value;
    }

    /// Per-provider "avoid keychain prompts" toggle (currently claude-only).
    pub fn avoid_keychain_prompts(&self, id: ProviderId) -> bool {
        self.provider_configs
            .get(&id)
            .map(|c| c.avoid_keychain_prompts)
            .unwrap_or(false)
    }

    pub fn set_avoid_keychain_prompts(&mut self, id: ProviderId, value: bool) {
        self.provider_config_mut(id).avoid_keychain_prompts = value;
    }

    // ── Legacy field-name aliases ────────────────────────────────────
    //
    // Keep the names of the old flat per-provider fields available as
    // accessor methods so existing call sites only need a `()` (read) or
    // `set_` prefix (write). New code should prefer the typed accessors
    // above.

    pub fn codex_cookie_source(&self) -> &str {
        self.cookie_source(ProviderId::Codex)
    }
    pub fn set_codex_cookie_source(&mut self, v: impl Into<String>) {
        self.set_cookie_source(ProviderId::Codex, v)
    }
    pub fn claude_cookie_source(&self) -> &str {
        self.cookie_source(ProviderId::Claude)
    }
    pub fn set_claude_cookie_source(&mut self, v: impl Into<String>) {
        self.set_cookie_source(ProviderId::Claude, v)
    }
    pub fn cursor_cookie_source(&self) -> &str {
        self.cookie_source(ProviderId::Cursor)
    }
    pub fn set_cursor_cookie_source(&mut self, v: impl Into<String>) {
        self.set_cookie_source(ProviderId::Cursor, v)
    }
    pub fn opencode_cookie_source(&self) -> &str {
        self.cookie_source(ProviderId::OpenCode)
    }
    pub fn set_opencode_cookie_source(&mut self, v: impl Into<String>) {
        self.set_cookie_source(ProviderId::OpenCode, v)
    }
    pub fn factory_cookie_source(&self) -> &str {
        self.cookie_source(ProviderId::Factory)
    }
    pub fn set_factory_cookie_source(&mut self, v: impl Into<String>) {
        self.set_cookie_source(ProviderId::Factory, v)
    }
    pub fn alibaba_cookie_source(&self) -> &str {
        self.cookie_source(ProviderId::Alibaba)
    }
    pub fn set_alibaba_cookie_source(&mut self, v: impl Into<String>) {
        self.set_cookie_source(ProviderId::Alibaba, v)
    }
    pub fn kimi_cookie_source(&self) -> &str {
        self.cookie_source(ProviderId::Kimi)
    }
    pub fn set_kimi_cookie_source(&mut self, v: impl Into<String>) {
        self.set_cookie_source(ProviderId::Kimi, v)
    }
    pub fn minimax_cookie_source(&self) -> &str {
        self.cookie_source(ProviderId::MiniMax)
    }
    pub fn set_minimax_cookie_source(&mut self, v: impl Into<String>) {
        self.set_cookie_source(ProviderId::MiniMax, v)
    }
    pub fn augment_cookie_source(&self) -> &str {
        self.cookie_source(ProviderId::Augment)
    }
    pub fn set_augment_cookie_source(&mut self, v: impl Into<String>) {
        self.set_cookie_source(ProviderId::Augment, v)
    }
    pub fn amp_cookie_source(&self) -> &str {
        self.cookie_source(ProviderId::Amp)
    }
    pub fn set_amp_cookie_source(&mut self, v: impl Into<String>) {
        self.set_cookie_source(ProviderId::Amp, v)
    }
    pub fn ollama_cookie_source(&self) -> &str {
        self.cookie_source(ProviderId::Ollama)
    }
    pub fn set_ollama_cookie_source(&mut self, v: impl Into<String>) {
        self.set_cookie_source(ProviderId::Ollama, v)
    }

    pub fn claude_usage_source(&self) -> &str {
        self.usage_source(ProviderId::Claude)
    }
    pub fn set_claude_usage_source(&mut self, v: impl Into<String>) {
        self.set_usage_source(ProviderId::Claude, v)
    }
    pub fn codex_usage_source(&self) -> &str {
        self.usage_source(ProviderId::Codex)
    }
    pub fn set_codex_usage_source(&mut self, v: impl Into<String>) {
        self.set_usage_source(ProviderId::Codex, v)
    }

    pub fn alibaba_api_region(&self) -> &str {
        self.api_region(ProviderId::Alibaba)
    }
    pub fn set_alibaba_api_region(&mut self, v: impl Into<String>) {
        self.set_api_region(ProviderId::Alibaba, v)
    }
    pub fn zai_api_region(&self) -> &str {
        self.api_region(ProviderId::Zai)
    }
    pub fn set_zai_api_region(&mut self, v: impl Into<String>) {
        self.set_api_region(ProviderId::Zai, v)
    }
    pub fn minimax_api_region(&self) -> &str {
        self.api_region(ProviderId::MiniMax)
    }
    pub fn set_minimax_api_region(&mut self, v: impl Into<String>) {
        self.set_api_region(ProviderId::MiniMax, v)
    }

    pub fn alibaba_cookie_header(&self) -> &str {
        self.manual_cookie_header(ProviderId::Alibaba)
    }
    pub fn set_alibaba_cookie_header(&mut self, v: impl Into<String>) {
        self.set_manual_cookie_header(ProviderId::Alibaba, v)
    }
    pub fn kimi_manual_cookie_header(&self) -> &str {
        self.manual_cookie_header(ProviderId::Kimi)
    }
    pub fn set_kimi_manual_cookie_header(&mut self, v: impl Into<String>) {
        self.set_manual_cookie_header(ProviderId::Kimi, v)
    }
    pub fn augment_cookie_header(&self) -> &str {
        self.manual_cookie_header(ProviderId::Augment)
    }
    pub fn set_augment_cookie_header(&mut self, v: impl Into<String>) {
        self.set_manual_cookie_header(ProviderId::Augment, v)
    }
    pub fn amp_cookie_header(&self) -> &str {
        self.manual_cookie_header(ProviderId::Amp)
    }
    pub fn set_amp_cookie_header(&mut self, v: impl Into<String>) {
        self.set_manual_cookie_header(ProviderId::Amp, v)
    }
    pub fn ollama_cookie_header(&self) -> &str {
        self.manual_cookie_header(ProviderId::Ollama)
    }
    pub fn set_ollama_cookie_header(&mut self, v: impl Into<String>) {
        self.set_manual_cookie_header(ProviderId::Ollama, v)
    }
    pub fn minimax_cookie_header(&self) -> &str {
        self.manual_cookie_header(ProviderId::MiniMax)
    }
    pub fn set_minimax_cookie_header(&mut self, v: impl Into<String>) {
        self.set_manual_cookie_header(ProviderId::MiniMax, v)
    }

    pub fn opencode_workspace_id(&self) -> &str {
        self.workspace_id(ProviderId::OpenCode)
    }
    pub fn set_opencode_workspace_id(&mut self, v: impl Into<String>) {
        self.set_workspace_id(ProviderId::OpenCode, v)
    }
    pub fn minimax_api_token(&self) -> &str {
        self.api_token(ProviderId::MiniMax)
    }
    pub fn set_minimax_api_token(&mut self, v: impl Into<String>) {
        self.set_api_token(ProviderId::MiniMax, v)
    }
    pub fn jetbrains_ide_base_path(&self) -> &str {
        self.ide_base_path(ProviderId::JetBrains)
    }
    pub fn set_jetbrains_ide_base_path(&mut self, v: impl Into<String>) {
        self.set_ide_base_path(ProviderId::JetBrains, v)
    }

    pub fn codex_openai_web_extras(&self) -> bool {
        self.openai_web_extras(ProviderId::Codex)
    }
    pub fn set_codex_openai_web_extras(&mut self, v: bool) {
        self.set_openai_web_extras(ProviderId::Codex, v)
    }
    pub fn codex_historical_tracking(&self) -> bool {
        self.historical_tracking(ProviderId::Codex)
    }
    pub fn set_codex_historical_tracking(&mut self, v: bool) {
        self.set_historical_tracking(ProviderId::Codex, v)
    }
    pub fn claude_avoid_keychain_prompts(&self) -> bool {
        self.avoid_keychain_prompts(ProviderId::Claude)
    }
    pub fn set_claude_avoid_keychain_prompts(&mut self, v: bool) {
        self.set_avoid_keychain_prompts(ProviderId::Claude, v)
    }
}

/// Provider status for settings UI
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderStatus {
    pub id: String,
    pub name: String,
    pub enabled: bool,
}

/// Refresh interval options
#[derive(Debug, Clone, Serialize)]
pub struct RefreshIntervalOption {
    pub value: u64,
    pub label: String,
}

/// Get available refresh interval options
pub fn get_refresh_interval_options() -> Vec<RefreshIntervalOption> {
    vec![
        RefreshIntervalOption {
            value: 60,
            label: "1 minute".to_string(),
        },
        RefreshIntervalOption {
            value: 120,
            label: "2 minutes".to_string(),
        },
        RefreshIntervalOption {
            value: 300,
            label: "5 minutes".to_string(),
        },
        RefreshIntervalOption {
            value: 600,
            label: "10 minutes".to_string(),
        },
        RefreshIntervalOption {
            value: 900,
            label: "15 minutes".to_string(),
        },
        RefreshIntervalOption {
            value: 1800,
            label: "30 minutes".to_string(),
        },
    ]
}

/// Manual cookie storage
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ManualCookies {
    /// Provider ID -> cookie header mapping
    pub cookies: HashMap<String, ManualCookieEntry>,
}

/// A single manual cookie entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManualCookieEntry {
    pub cookie_header: String,
    pub saved_at: String,
}

impl ManualCookies {
    /// Get the cookies file path
    pub fn cookies_path() -> Option<PathBuf> {
        dirs::config_dir().map(|p| p.join("CodexBar").join("manual_cookies.json"))
    }

    /// Load manual cookies from disk
    pub fn load() -> Self {
        if let Some(path) = Self::cookies_path()
            && path.exists()
            && let Ok(content) = crate::secure_file::read_string(&path)
        {
            return serde_json::from_str(&content).unwrap_or_default();
        }
        Self::default()
    }

    /// Save manual cookies to disk
    pub fn save(&self) -> anyhow::Result<()> {
        let path = Self::cookies_path()
            .ok_or_else(|| anyhow::anyhow!("Could not determine cookies path"))?;

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let json = serde_json::to_string_pretty(self)?;
        crate::secure_file::write_string(&path, &json)?;

        Ok(())
    }

    /// Get cookie for a provider
    pub fn get(&self, provider_id: &str) -> Option<&str> {
        self.cookies
            .get(provider_id)
            .map(|e| e.cookie_header.as_str())
    }

    /// Set cookie for a provider
    pub fn set(&mut self, provider_id: &str, cookie_header: &str) {
        let now = chrono::Utc::now().format("%Y-%m-%d %H:%M").to_string();
        self.cookies.insert(
            provider_id.to_string(),
            ManualCookieEntry {
                cookie_header: cookie_header.to_string(),
                saved_at: now,
            },
        );
    }

    /// Remove cookie for a provider
    pub fn remove(&mut self, provider_id: &str) {
        self.cookies.remove(provider_id);
    }

    /// Get all saved cookies for UI display
    pub fn get_all_for_display(&self) -> Vec<SavedCookieInfo> {
        self.cookies
            .iter()
            .map(|(id, entry)| {
                let provider_name = ProviderId::from_cli_name(id)
                    .map(|p| p.display_name().to_string())
                    .unwrap_or_else(|| id.clone());

                SavedCookieInfo {
                    provider_id: id.clone(),
                    provider: provider_name,
                    saved_at: entry.saved_at.clone(),
                }
            })
            .collect()
    }
}

/// Info about a saved cookie for UI display
#[derive(Debug, Clone, Serialize)]
pub struct SavedCookieInfo {
    pub provider_id: String,
    pub provider: String,
    pub saved_at: String,
}

/// API key storage for providers that need tokens
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ApiKeys {
    /// Provider ID -> API key mapping
    pub keys: HashMap<String, ApiKeyEntry>,
}

/// A single API key entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyEntry {
    pub api_key: String,
    pub saved_at: String,
    /// Optional label for the key (e.g., "Personal", "Work")
    #[serde(default)]
    pub label: Option<String>,
}

impl ApiKeys {
    /// Get the API keys file path
    pub fn keys_path() -> Option<PathBuf> {
        dirs::config_dir().map(|p| p.join("CodexBar").join("api_keys.json"))
    }

    /// Load API keys from disk
    pub fn load() -> Self {
        if let Some(path) = Self::keys_path()
            && path.exists()
            && let Ok(content) = crate::secure_file::read_string(&path)
        {
            return serde_json::from_str(&content).unwrap_or_default();
        }
        Self::default()
    }

    /// Save API keys to disk
    pub fn save(&self) -> anyhow::Result<()> {
        let path = Self::keys_path()
            .ok_or_else(|| anyhow::anyhow!("Could not determine API keys path"))?;

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let json = serde_json::to_string_pretty(self)?;
        crate::secure_file::write_string(&path, &json)?;

        Ok(())
    }

    /// Get API key for a provider
    pub fn get(&self, provider_id: &str) -> Option<&str> {
        self.keys.get(provider_id).map(|e| e.api_key.as_str())
    }

    /// Set API key for a provider
    pub fn set(&mut self, provider_id: &str, api_key: &str, label: Option<&str>) {
        let now = chrono::Utc::now().format("%Y-%m-%d %H:%M").to_string();
        self.keys.insert(
            provider_id.to_string(),
            ApiKeyEntry {
                api_key: api_key.to_string(),
                saved_at: now,
                label: label.map(|s| s.to_string()),
            },
        );
    }

    /// Remove API key for a provider
    pub fn remove(&mut self, provider_id: &str) {
        self.keys.remove(provider_id);
    }

    /// Check if a provider has an API key configured
    pub fn has_key(&self, provider_id: &str) -> bool {
        self.keys
            .get(provider_id)
            .map(|e| !e.api_key.is_empty())
            .unwrap_or(false)
    }

    /// Get all saved API keys for UI display (with masked values)
    pub fn get_all_for_display(&self) -> Vec<SavedApiKeyInfo> {
        self.keys
            .iter()
            .map(|(id, entry)| {
                let provider_name = ProviderId::from_cli_name(id)
                    .map(|p| p.display_name().to_string())
                    .unwrap_or_else(|| id.clone());

                // Mask the key for display (show first 4 and last 4 chars)
                let masked = if entry.api_key.len() > 12 {
                    format!(
                        "{}...{}",
                        &entry.api_key[..4],
                        &entry.api_key[entry.api_key.len() - 4..]
                    )
                } else if entry.api_key.len() > 4 {
                    format!("{}...", &entry.api_key[..4])
                } else {
                    "****".to_string()
                };

                SavedApiKeyInfo {
                    provider_id: id.clone(),
                    provider: provider_name,
                    masked_key: masked,
                    saved_at: entry.saved_at.clone(),
                    label: entry.label.clone(),
                }
            })
            .collect()
    }
}

/// Info about a saved API key for UI display
#[derive(Debug, Clone, Serialize)]
pub struct SavedApiKeyInfo {
    pub provider_id: String,
    pub provider: String,
    pub masked_key: String,
    pub saved_at: String,
    pub label: Option<String>,
}

/// Provider configuration info
#[derive(Debug, Clone)]
pub struct ProviderConfigInfo {
    pub id: ProviderId,
    pub name: &'static str,
    pub requires_api_key: bool,
    pub api_key_env_var: Option<&'static str>,
    pub api_key_help: Option<&'static str>,
    pub config_file_path: Option<&'static str>,
    pub dashboard_url: Option<&'static str>,
}

/// Get configuration info for providers that need API keys
pub fn get_api_key_providers() -> Vec<ProviderConfigInfo> {
    vec![
        ProviderConfigInfo {
            id: ProviderId::Alibaba,
            name: "Alibaba Coding Plan",
            requires_api_key: true,
            api_key_env_var: Some("ALIBABA_CODING_PLAN_API_KEY"),
            api_key_help: Some("Get your Coding Plan API key from Alibaba Model Studio / Bailian"),
            config_file_path: Some("~/.codexbar/config.json"),
            dashboard_url: Some(
                "https://modelstudio.console.alibabacloud.com/ap-southeast-1/?tab=coding-plan#/efm/detail",
            ),
        },
        ProviderConfigInfo {
            id: ProviderId::Amp,
            name: "Amp (Sourcegraph)",
            requires_api_key: true,
            api_key_env_var: Some("SRC_ACCESS_TOKEN"),
            api_key_help: Some("Get your token from Sourcegraph → Settings → Access Tokens"),
            config_file_path: Some("~/.amp/config.json"),
            dashboard_url: Some("https://sourcegraph.com/cody/manage"),
        },
        ProviderConfigInfo {
            id: ProviderId::Synthetic,
            name: "Synthetic",
            requires_api_key: true,
            api_key_env_var: Some("SYNTHETIC_API_KEY"),
            api_key_help: Some("Get your API key from Synthetic → Account → API Keys"),
            config_file_path: Some("~/.synthetic/config.json"),
            dashboard_url: Some("https://synthetic.computer/account"),
        },
        ProviderConfigInfo {
            id: ProviderId::Copilot,
            name: "GitHub Copilot (legacy token)",
            requires_api_key: true,
            api_key_env_var: Some("GITHUB_TOKEN"),
            api_key_help: Some(
                "Optional fallback. Prefer Providers → Copilot → Sign in with GitHub.",
            ),
            config_file_path: None,
            dashboard_url: Some("https://github.com/settings/copilot"),
        },
        ProviderConfigInfo {
            id: ProviderId::Zai,
            name: "z.ai",
            requires_api_key: true,
            api_key_env_var: Some("ZAI_API_TOKEN"),
            api_key_help: Some("Get your API token from z.ai Dashboard → Settings"),
            config_file_path: None,
            dashboard_url: Some("https://z.ai/dashboard"),
        },
        ProviderConfigInfo {
            id: ProviderId::Warp,
            name: "Warp",
            requires_api_key: true,
            api_key_env_var: Some("WARP_API_KEY"),
            api_key_help: Some(
                "Get your API key from Warp → Settings → API Keys (docs.warp.dev/reference/cli/api-keys)",
            ),
            config_file_path: None,
            dashboard_url: Some("https://docs.warp.dev/reference/cli/api-keys"),
        },
        ProviderConfigInfo {
            id: ProviderId::OpenRouter,
            name: "OpenRouter",
            requires_api_key: true,
            api_key_env_var: Some("OPENROUTER_API_KEY"),
            api_key_help: Some("Get your API key from openrouter.ai/settings/keys"),
            config_file_path: None,
            dashboard_url: Some("https://openrouter.ai/settings/credits"),
        },
        ProviderConfigInfo {
            id: ProviderId::NanoGPT,
            name: "NanoGPT",
            requires_api_key: true,
            api_key_env_var: Some("NANOGPT_API_KEY"),
            api_key_help: Some("Get your API key from nano-gpt.com/api"),
            config_file_path: None,
            dashboard_url: Some("https://nano-gpt.com/api"),
        },
        ProviderConfigInfo {
            id: ProviderId::Infini,
            name: "Infini AI",
            requires_api_key: true,
            api_key_env_var: Some("INFINI_API_KEY"),
            api_key_help: Some("Get your API key from Infini Cloud → Settings → API Keys"),
            config_file_path: None,
            dashboard_url: Some("https://cloud.infini-ai.com"),
        },
        ProviderConfigInfo {
            id: ProviderId::Kilo,
            name: "Kilo",
            requires_api_key: true,
            api_key_env_var: Some("KILO_API_KEY"),
            api_key_help: Some("Get your API key from Kilo, or sign in with Kilo CLI."),
            config_file_path: Some("~/.local/share/kilo/auth.json"),
            dashboard_url: Some("https://app.kilo.ai/usage"),
        },
        ProviderConfigInfo {
            id: ProviderId::Bedrock,
            name: "AWS Bedrock",
            requires_api_key: true,
            api_key_env_var: Some("AWS_ACCESS_KEY_ID:AWS_SECRET_ACCESS_KEY[:AWS_SESSION_TOKEN]"),
            api_key_help: Some(
                "Paste access_key:secret_key[:session_token], JSON credentials, or use AWS env vars.",
            ),
            config_file_path: None,
            dashboard_url: Some("https://console.aws.amazon.com/bedrock"),
        },
        ProviderConfigInfo {
            id: ProviderId::Codebuff,
            name: "Codebuff",
            requires_api_key: true,
            api_key_env_var: Some("CODEBUFF_API_KEY"),
            api_key_help: Some(
                "Get your API key from Codebuff, or sign in with Codebuff/Manicode.",
            ),
            config_file_path: Some("~/.config/manicode/credentials.json"),
            dashboard_url: Some("https://www.codebuff.com/usage"),
        },
        ProviderConfigInfo {
            id: ProviderId::DeepSeek,
            name: "DeepSeek",
            requires_api_key: true,
            api_key_env_var: Some("DEEPSEEK_API_KEY"),
            api_key_help: Some("Get your API key from platform.deepseek.com."),
            config_file_path: None,
            dashboard_url: Some("https://platform.deepseek.com/usage"),
        },
        ProviderConfigInfo {
            id: ProviderId::Doubao,
            name: "Doubao / Volcengine Ark",
            requires_api_key: true,
            api_key_env_var: Some("ARK_API_KEY"),
            api_key_help: Some("Get your API key from Volcengine Ark."),
            config_file_path: None,
            dashboard_url: Some("https://console.volcengine.com/ark/region:ark+cn-beijing/usage"),
        },
        ProviderConfigInfo {
            id: ProviderId::Crof,
            name: "Crof",
            requires_api_key: true,
            api_key_env_var: Some("CROF_API_KEY"),
            api_key_help: Some("Get your API key from Crof."),
            config_file_path: None,
            dashboard_url: Some("https://crof.ai"),
        },
        ProviderConfigInfo {
            id: ProviderId::StepFun,
            name: "StepFun",
            requires_api_key: true,
            api_key_env_var: Some("STEPFUN_OASIS_TOKEN"),
            api_key_help: Some("Paste an existing Oasis-Token from StepFun login."),
            config_file_path: None,
            dashboard_url: Some("https://platform.stepfun.com/dashboard"),
        },
        ProviderConfigInfo {
            id: ProviderId::Venice,
            name: "Venice",
            requires_api_key: true,
            api_key_env_var: Some("VENICE_API_KEY"),
            api_key_help: Some("Get your API key from Venice settings."),
            config_file_path: None,
            dashboard_url: Some("https://venice.ai/settings/api"),
        },
        ProviderConfigInfo {
            id: ProviderId::OpenAIApi,
            name: "OpenAI",
            requires_api_key: true,
            api_key_env_var: Some("OPENAI_ADMIN_KEY / OPENAI_API_KEY"),
            api_key_help: Some(
                "Use an OpenAI Admin API key for usage, or a platform key for legacy billing balance.",
            ),
            config_file_path: None,
            dashboard_url: Some("https://platform.openai.com/usage"),
        },
        ProviderConfigInfo {
            id: ProviderId::Grok,
            name: "Grok",
            requires_api_key: false,
            api_key_env_var: None,
            api_key_help: Some("Uses Grok browser cookies or ~/.grok/auth.json."),
            config_file_path: Some("~/.grok/auth.json"),
            dashboard_url: Some("https://grok.com/settings/subscription"),
        },
        ProviderConfigInfo {
            id: ProviderId::ElevenLabs,
            name: "ElevenLabs",
            requires_api_key: true,
            api_key_env_var: Some("ELEVENLABS_API_KEY"),
            api_key_help: Some("Get your API key from ElevenLabs Settings > API Keys."),
            config_file_path: None,
            dashboard_url: Some("https://elevenlabs.io/app/settings/api-keys"),
        },
        ProviderConfigInfo {
            id: ProviderId::Deepgram,
            name: "Deepgram",
            requires_api_key: true,
            api_key_env_var: Some("DEEPGRAM_API_KEY"),
            api_key_help: Some("Use a Deepgram API key with Management API access."),
            config_file_path: None,
            dashboard_url: Some("https://console.deepgram.com/usage"),
        },
        ProviderConfigInfo {
            id: ProviderId::Groq,
            name: "Groq",
            requires_api_key: true,
            api_key_env_var: Some("GROQ_API_KEY"),
            api_key_help: Some("Groq metrics require Enterprise Prometheus metrics access."),
            config_file_path: None,
            dashboard_url: Some("https://console.groq.com/settings/metrics"),
        },
        ProviderConfigInfo {
            id: ProviderId::LLMProxy,
            name: "LLM Proxy",
            requires_api_key: true,
            api_key_env_var: Some("LLM_PROXY_API_KEY + LLM_PROXY_BASE_URL"),
            api_key_help: Some("Set an LLM Proxy API key and base URL for quota-stats."),
            config_file_path: None,
            dashboard_url: None,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_settings_default() {
        let settings = Settings::default();
        assert!(settings.enabled_providers.contains("claude"));
        assert!(settings.enabled_providers.contains("codex"));
        assert_eq!(settings.refresh_interval_secs, 300);
        assert!(settings.show_notifications);
        assert_eq!(settings.high_usage_threshold, 70.0);
        assert_eq!(settings.critical_usage_threshold, 90.0);
    }

    #[test]
    fn float_bar_defaults_are_safe() {
        let settings = Settings::default();
        assert!(!settings.float_bar_enabled);
        assert_eq!(settings.float_bar_opacity, 80);
        assert_eq!(settings.float_bar_orientation, "horizontal");
        assert!(!settings.float_bar_click_through);
        assert!(settings.float_bar_provider_ids.is_empty());
        assert!(!settings.float_bar_dark_text);
    }

    #[test]
    fn float_bar_opacity_clamp_pins_to_supported_range() {
        // Below 30 → 30 so the bar isn't accidentally invisible.
        assert_eq!(clamp_float_bar_opacity(0), 30);
        assert_eq!(clamp_float_bar_opacity(29), 30);
        // Within range → unchanged.
        assert_eq!(clamp_float_bar_opacity(45), 45);
        assert_eq!(clamp_float_bar_opacity(80), 80);
        // Above 100 → 100.
        assert_eq!(clamp_float_bar_opacity(150), 100);
        assert_eq!(clamp_float_bar_opacity(255), 100);
    }

    #[test]
    fn float_bar_orientation_normalization_rejects_unknown_values() {
        assert_eq!(normalize_float_bar_orientation("horizontal"), "horizontal");
        assert_eq!(normalize_float_bar_orientation("vertical"), "vertical");
        // Anything else collapses to horizontal so a corrupt settings file
        // can't poison the renderer with an unknown layout token.
        assert_eq!(normalize_float_bar_orientation(""), "horizontal");
        assert_eq!(normalize_float_bar_orientation("diagonal"), "horizontal");
        assert_eq!(normalize_float_bar_orientation("VERTICAL"), "horizontal");
    }

    #[test]
    fn float_bar_settings_round_trip_through_raw() {
        // Serialize a Settings with custom float-bar values then deserialize
        // through the `from = "RawSettings"` path — values must survive intact
        // (after clamping/normalization).
        let s = Settings {
            float_bar_enabled: true,
            float_bar_opacity: 65,
            float_bar_orientation: "vertical".to_string(),
            float_bar_click_through: true,
            float_bar_provider_ids: vec!["claude".into(), "codex".into()],
            float_bar_dark_text: true,
            ..Settings::default()
        };

        let json = serde_json::to_string(&s).expect("serialize");
        let back: Settings = serde_json::from_str(&json).expect("deserialize");
        assert!(back.float_bar_enabled);
        assert_eq!(back.float_bar_opacity, 65);
        assert_eq!(back.float_bar_orientation, "vertical");
        assert!(back.float_bar_click_through);
        assert_eq!(back.float_bar_provider_ids, vec!["claude", "codex"]);
        assert!(back.float_bar_dark_text);
    }

    #[test]
    fn float_bar_raw_clamps_out_of_range_opacity_on_load() {
        // Simulate an externally-edited settings.json with a wild opacity.
        let json = r#"{
            "enabled_providers": [],
            "refresh_interval_secs": 300,
            "start_minimized": false,
            "start_at_login": false,
            "show_notifications": true,
            "sound_enabled": true,
            "sound_volume": 100,
            "high_usage_threshold": 70.0,
            "critical_usage_threshold": 90.0,
            "merge_tray_icons": false,
            "show_as_used": true,
            "surprise_animations": false,
            "enable_animations": true,
            "reset_time_relative": true,
            "menu_bar_display_mode": "detailed",
            "show_credits_extra_usage": true,
            "show_debug_settings": false,
            "disable_keychain_access": false,
            "hide_personal_info": false,
            "float_bar_opacity": 250,
            "float_bar_orientation": "diagonal"
        }"#;
        let loaded: Settings = serde_json::from_str(json).expect("parse");
        assert_eq!(loaded.float_bar_opacity, 100);
        assert_eq!(loaded.float_bar_orientation, "horizontal");
    }

    #[test]
    fn test_settings_provider_enabled() {
        let settings = Settings::default();
        assert!(settings.is_provider_enabled(ProviderId::Claude));
        assert!(settings.is_provider_enabled(ProviderId::Codex));
        assert!(!settings.is_provider_enabled(ProviderId::Gemini));
    }

    #[test]
    fn test_settings_toggle_provider() {
        let mut settings = Settings::default();

        // Claude starts enabled
        assert!(settings.is_provider_enabled(ProviderId::Claude));

        // Toggle off
        let enabled = settings.toggle_provider(ProviderId::Claude);
        assert!(!enabled);
        assert!(!settings.is_provider_enabled(ProviderId::Claude));

        // Toggle back on
        let enabled = settings.toggle_provider(ProviderId::Claude);
        assert!(enabled);
        assert!(settings.is_provider_enabled(ProviderId::Claude));
    }

    #[test]
    fn test_settings_get_enabled_provider_ids() {
        let settings = Settings::default();
        let enabled = settings.get_enabled_provider_ids();
        assert!(enabled.contains(&ProviderId::Claude));
        assert!(enabled.contains(&ProviderId::Codex));
    }

    #[test]
    fn test_settings_get_all_providers_status() {
        let settings = Settings::default();
        let status = settings.get_all_providers_status();
        assert_eq!(status.len(), ProviderId::all().len());

        let claude_status = status.iter().find(|s| s.id == "claude").unwrap();
        assert_eq!(claude_status.name, "Claude");
        assert!(claude_status.enabled);

        let gemini_status = status.iter().find(|s| s.id == "gemini").unwrap();
        assert!(!gemini_status.enabled);
    }

    #[test]
    fn test_api_key_provider_catalog_includes_token_providers() {
        let providers = get_api_key_providers();
        for id in [
            ProviderId::Kilo,
            ProviderId::Bedrock,
            ProviderId::Codebuff,
            ProviderId::DeepSeek,
            ProviderId::ElevenLabs,
            ProviderId::Deepgram,
            ProviderId::Grok,
            ProviderId::Groq,
            ProviderId::LLMProxy,
        ] {
            assert!(
                providers.iter().any(|provider| provider.id == id),
                "{id} should be configurable from the API Keys UI"
            );
        }
    }

    #[test]
    fn test_refresh_interval_options() {
        let options = get_refresh_interval_options();
        assert!(!options.is_empty());
        assert!(options.iter().any(|o| o.value == 60));
        assert!(options.iter().any(|o| o.value == 300));
    }

    #[test]
    fn test_manual_cookies_default() {
        let cookies = ManualCookies::default();
        assert!(cookies.cookies.is_empty());
    }

    #[test]
    fn test_manual_cookies_set_get_remove() {
        let mut cookies = ManualCookies::default();

        // Set a cookie
        cookies.set("claude", "session=abc123");
        assert_eq!(cookies.get("claude"), Some("session=abc123"));

        // Remove it
        cookies.remove("claude");
        assert_eq!(cookies.get("claude"), None);
    }

    #[test]
    fn test_start_at_login_command_uses_only_the_executable_path() {
        let path =
            std::path::PathBuf::from(r"C:\Program Files\CodexBar\codexbar-desktop-tauri.exe");
        let command = Settings::start_at_login_command(&path);
        assert_eq!(
            command,
            "\"C:\\Program Files\\CodexBar\\codexbar-desktop-tauri.exe\""
        );
        assert!(!command.contains("menubar"));
    }

    #[test]
    fn test_language_defaults_to_english() {
        let settings = Settings::default();
        assert_eq!(settings.ui_language, Language::English);
    }

    #[test]
    fn test_language_all_variants_available() {
        let languages = Language::all();
        assert_eq!(languages.len(), 2);
        assert!(languages.contains(&Language::English));
        assert!(languages.contains(&Language::Chinese));
    }

    #[test]
    fn test_language_display_names() {
        assert_eq!(Language::English.display_name(), "English");
        assert_eq!(Language::Chinese.display_name(), "中文");
    }

    #[test]
    fn test_settings_load_missing_language_field_defaults_to_english() {
        // Simulate loading legacy settings JSON without ui_language field
        let legacy_json = r#"{
            "enabled_providers": ["claude", "codex"],
            "refresh_interval_secs": 300,
            "start_minimized": false,
            "ui_language": "english"
        }"#;

        let settings: Result<Settings, _> = serde_json::from_str(legacy_json);
        assert!(settings.is_ok());
        let settings = settings.unwrap();
        assert_eq!(settings.ui_language, Language::English);
    }

    #[test]
    fn test_settings_roundtrip_with_language() {
        use std::io::Write;
        use tempfile::NamedTempFile;

        // Create settings with Chinese language
        let settings = Settings {
            ui_language: Language::Chinese,
            ..Settings::default()
        };

        // Save to a temp file
        let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");
        let json = serde_json::to_string_pretty(&settings).expect("Failed to serialize settings");
        temp_file
            .write_all(json.as_bytes())
            .expect("Failed to write settings");
        let path = temp_file.path().to_path_buf();

        // Read back and verify
        let content = std::fs::read_to_string(&path).expect("Failed to read settings");
        let loaded: Settings =
            serde_json::from_str(&content).expect("Failed to deserialize settings");

        assert_eq!(loaded.ui_language, Language::Chinese);
    }

    #[test]
    fn test_settings_with_utf8_bom_parses_perprovider_tray_mode() {
        let json = "\u{feff}{\n            \"enabled_providers\": [\"claude\", \"codex\"],\n            \"refresh_interval_secs\": 300,\n            \"tray_icon_mode\": \"perprovider\"\n        }";

        let settings: Settings = serde_json::from_str(json.trim_start_matches('\u{feff}')).unwrap();

        assert_eq!(settings.tray_icon_mode, TrayIconMode::PerProvider);
    }

    #[test]
    fn test_language_serde_serialization() {
        // Test that Language serializes to lowercase string
        let english = Language::English;
        let chinese = Language::Chinese;

        let english_json = serde_json::to_string(&english).unwrap();
        let chinese_json = serde_json::to_string(&chinese).unwrap();

        assert_eq!(english_json, "\"english\"");
        assert_eq!(chinese_json, "\"chinese\"");
    }

    #[test]
    fn test_language_serde_deserialization() {
        // Test that lowercase strings deserialize correctly
        let english: Language = serde_json::from_str("\"english\"").unwrap();
        let chinese: Language = serde_json::from_str("\"chinese\"").unwrap();

        assert_eq!(english, Language::English);
        assert_eq!(chinese, Language::Chinese);
    }

    #[test]
    fn test_theme_defaults_to_auto() {
        let settings = Settings::default();
        assert_eq!(settings.theme, ThemePreference::Auto);
    }

    #[test]
    fn test_theme_all_variants_available() {
        let themes = ThemePreference::all();
        assert_eq!(themes.len(), 3);
        assert!(themes.contains(&ThemePreference::Auto));
        assert!(themes.contains(&ThemePreference::Light));
        assert!(themes.contains(&ThemePreference::Dark));
    }

    #[test]
    fn test_theme_serde_roundtrip() {
        for variant in [
            ThemePreference::Auto,
            ThemePreference::Light,
            ThemePreference::Dark,
        ] {
            let encoded = serde_json::to_string(&variant).unwrap();
            let decoded: ThemePreference = serde_json::from_str(&encoded).unwrap();
            assert_eq!(decoded, variant);
        }
        assert_eq!(
            serde_json::to_string(&ThemePreference::Light).unwrap(),
            "\"light\""
        );
        assert_eq!(
            serde_json::to_string(&ThemePreference::Dark).unwrap(),
            "\"dark\""
        );
        assert_eq!(
            serde_json::to_string(&ThemePreference::Auto).unwrap(),
            "\"auto\""
        );
    }

    #[test]
    fn test_settings_missing_theme_defaults_to_auto() {
        // Legacy settings JSON without the theme field should still parse.
        let legacy_json = r#"{
            "enabled_providers": ["claude", "codex"],
            "refresh_interval_secs": 300,
            "ui_language": "english"
        }"#;

        let settings: Settings = serde_json::from_str(legacy_json).unwrap();
        assert_eq!(settings.theme, ThemePreference::Auto);
    }

    #[test]
    fn test_settings_roundtrip_with_theme() {
        let settings = Settings {
            theme: ThemePreference::Dark,
            ..Settings::default()
        };
        let json = serde_json::to_string(&settings).unwrap();
        let loaded: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.theme, ThemePreference::Dark);
    }

    // ── Phase 3: provider_configs migration tests ───────────────────────

    /// Loading a legacy `settings.json` (with flat per-provider fields)
    /// must populate `provider_configs` and surface every value through the
    /// per-provider accessors.
    #[test]
    fn test_legacy_per_provider_fields_migrate_into_provider_configs() {
        // NOTE: placeholder values only — no real cookies/tokens.
        let legacy_json = r#"{
            "enabled_providers": ["claude", "codex"],
            "refresh_interval_secs": 300,
            "codex_cookie_source": "manual",
            "claude_cookie_source": "browser",
            "cursor_cookie_source": "manual",
            "alibaba_cookie_source": "manual",
            "alibaba_cookie_header": "ali=PLACEHOLDER",
            "alibaba_api_region": "cn",
            "zai_api_region": "cn",
            "minimax_api_region": "cn",
            "minimax_api_token": "TOK_PLACEHOLDER",
            "claude_usage_source": "ccusage",
            "codex_usage_source": "manual",
            "codex_openai_web_extras": false,
            "codex_historical_tracking": true,
            "claude_avoid_keychain_prompts": true,
            "opencode_workspace_id": "ws_placeholder",
            "jetbrains_ide_base_path": "C:/JB"
        }"#;

        let settings: Settings = serde_json::from_str(legacy_json).unwrap();

        // Cookie sources
        assert_eq!(settings.cookie_source(ProviderId::Codex), "manual");
        assert_eq!(settings.cookie_source(ProviderId::Claude), "browser");
        assert_eq!(settings.cookie_source(ProviderId::Cursor), "manual");
        assert_eq!(settings.cookie_source(ProviderId::Alibaba), "manual");
        // Untouched providers fall through to the default "manual" to avoid
        // background browser-cookie reads unless the user opts into Automatic.
        assert_eq!(settings.cookie_source(ProviderId::Amp), "manual");

        // Manual cookie headers + api regions
        assert_eq!(
            settings.manual_cookie_header(ProviderId::Alibaba),
            "ali=PLACEHOLDER"
        );
        assert_eq!(settings.api_region(ProviderId::Alibaba), "cn");
        assert_eq!(settings.api_region(ProviderId::Zai), "cn");
        assert_eq!(settings.api_region(ProviderId::MiniMax), "cn");

        // Usage sources
        assert_eq!(settings.usage_source(ProviderId::Claude), "ccusage");
        assert_eq!(settings.usage_source(ProviderId::Codex), "manual");

        // Codex booleans
        assert!(!settings.openai_web_extras(ProviderId::Codex));
        assert!(settings.historical_tracking(ProviderId::Codex));

        // Claude per-provider boolean
        assert!(settings.avoid_keychain_prompts(ProviderId::Claude));

        // Misc per-provider strings
        assert_eq!(
            settings.workspace_id(ProviderId::OpenCode),
            "ws_placeholder"
        );
        assert_eq!(settings.api_token(ProviderId::MiniMax), "TOK_PLACEHOLDER");
        assert_eq!(settings.ide_base_path(ProviderId::JetBrains), "C:/JB");

        // Legacy field-name aliases agree with typed accessors.
        assert_eq!(settings.codex_cookie_source(), "manual");
        assert_eq!(settings.alibaba_api_region(), "cn");
        assert!(settings.codex_historical_tracking());
        assert!(!settings.codex_openai_web_extras());
        assert!(settings.claude_avoid_keychain_prompts());
    }

    /// Round-trip: build a `Settings` programmatically via the new map +
    /// accessors, serialize, parse back, and assert equality of every
    /// per-provider field.
    #[test]
    fn test_provider_configs_roundtrip() {
        let mut settings = Settings::default();
        settings.set_cookie_source(ProviderId::Codex, "manual");
        settings.set_cookie_source(ProviderId::Claude, "browser");
        settings.set_usage_source(ProviderId::Claude, "ccusage");
        settings.set_api_region(ProviderId::Alibaba, "cn");
        settings.set_api_region(ProviderId::Zai, "cn");
        settings.set_manual_cookie_header(ProviderId::Amp, "amp=PLACEHOLDER");
        settings.set_api_token(ProviderId::MiniMax, "TOK_PLACEHOLDER");
        settings.set_workspace_id(ProviderId::OpenCode, "ws_placeholder");
        settings.set_ide_base_path(ProviderId::JetBrains, "C:/JB");
        settings.set_openai_web_extras(ProviderId::Codex, false);
        settings.set_historical_tracking(ProviderId::Codex, true);
        settings.set_avoid_keychain_prompts(ProviderId::Claude, true);

        let json = serde_json::to_string(&settings).unwrap();
        // The legacy flat fields must NOT appear in serialized output.
        assert!(!json.contains("\"codex_cookie_source\""), "json: {json}");
        assert!(!json.contains("\"alibaba_api_region\""), "json: {json}");
        assert!(
            !json.contains("\"claude_avoid_keychain_prompts\""),
            "json: {json}"
        );
        assert!(json.contains("\"provider_configs\""), "json: {json}");

        let loaded: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.cookie_source(ProviderId::Codex), "manual");
        assert_eq!(loaded.cookie_source(ProviderId::Claude), "browser");
        assert_eq!(loaded.usage_source(ProviderId::Claude), "ccusage");
        assert_eq!(loaded.api_region(ProviderId::Alibaba), "cn");
        assert_eq!(loaded.api_region(ProviderId::Zai), "cn");
        assert_eq!(
            loaded.manual_cookie_header(ProviderId::Amp),
            "amp=PLACEHOLDER"
        );
        assert_eq!(loaded.api_token(ProviderId::MiniMax), "TOK_PLACEHOLDER");
        assert_eq!(loaded.workspace_id(ProviderId::OpenCode), "ws_placeholder");
        assert_eq!(loaded.ide_base_path(ProviderId::JetBrains), "C:/JB");
        assert!(!loaded.openai_web_extras(ProviderId::Codex));
        assert!(loaded.historical_tracking(ProviderId::Codex));
        assert!(loaded.avoid_keychain_prompts(ProviderId::Claude));
        assert_eq!(
            loaded.provider_configs.get(&ProviderId::Codex),
            settings.provider_configs.get(&ProviderId::Codex)
        );
    }

    /// New-format files (no legacy flat fields, only `provider_configs`)
    /// must load identically.
    #[test]
    fn test_new_format_provider_configs_only() {
        let json = r#"{
            "enabled_providers": ["claude"],
            "refresh_interval_secs": 300,
            "provider_configs": {
                "codex": { "cookie_source": "manual", "openai_web_extras": false },
                "alibaba": { "api_region": "cn", "manual_cookie_header": "ali=PLACEHOLDER" }
            }
        }"#;

        let settings: Settings = serde_json::from_str(json).unwrap();
        assert_eq!(settings.cookie_source(ProviderId::Codex), "manual");
        assert!(!settings.openai_web_extras(ProviderId::Codex));
        assert_eq!(settings.api_region(ProviderId::Alibaba), "cn");
        assert_eq!(
            settings.manual_cookie_header(ProviderId::Alibaba),
            "ali=PLACEHOLDER"
        );
        // Untouched providers still get their defaults.
        assert_eq!(settings.cookie_source(ProviderId::Claude), "manual");
        assert_eq!(settings.api_region(ProviderId::Zai), "global");
    }

    /// Default `Settings` should serialize WITHOUT a `provider_configs`
    /// field (empty map skipped).
    #[test]
    fn test_default_settings_skip_empty_provider_configs() {
        let settings = Settings::default();
        let json = serde_json::to_string(&settings).unwrap();
        assert!(
            !json.contains("\"provider_configs\""),
            "empty map should be skipped, json: {json}"
        );
    }

    /// Per-provider defaults are applied even when the entry is absent.
    #[test]
    fn test_per_provider_defaults_applied() {
        let settings = Settings::default();
        assert_eq!(settings.cookie_source(ProviderId::Codex), "manual");
        assert_eq!(settings.usage_source(ProviderId::Codex), "auto");
        assert_eq!(settings.api_region(ProviderId::Alibaba), "intl");
        assert_eq!(settings.api_region(ProviderId::Zai), "global");
        assert_eq!(settings.api_region(ProviderId::MiniMax), "global");
        assert!(settings.openai_web_extras(ProviderId::Codex));
        assert!(!settings.historical_tracking(ProviderId::Codex));
        assert!(!settings.avoid_keychain_prompts(ProviderId::Claude));
    }
}
