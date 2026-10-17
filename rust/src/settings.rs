//! Settings management for CodexBar
//!
//! Handles persistent configuration including:
//! - Enabled/disabled providers
//! - Refresh interval
//! - Manual cookies
//! - Other user preferences

#![allow(
    dead_code,
    reason = "settings types mirror the full config schema; some fields are not yet consumed"
)]

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::core::ProviderId;
use crate::cost_reporting_period::CostReportingPeriod;

/// Stable namespace used by the desktop bridge for quota metric rows.
pub const USAGE_ITEM_METRIC_PREFIX: &str = "metric:";
/// Stable namespace for provider detail sections (upstream 0.62.0 #3638).
pub const USAGE_ITEM_DETAIL_SECTION_PREFIX: &str = "detailSection:";
pub const CODEX_SPARK_USAGE_ITEM_IDS: [&str; 2] = [
    "metric:extra-codex-spark",
    "metric:extra-codex-spark-weekly",
];
pub const CLAUDE_DAILY_ROUTINES_USAGE_ITEM_ID: &str = "metric:extra-claude-routines";
/// Namespace for non-metric card sections with their own visibility choice
/// (upstream 0.75.0 #4484). The raw IDs match upstream's.
pub const USAGE_ITEM_SECTION_PREFIX: &str = "section:";
/// Claude saved limit-reset credits (the `reset-credits` inventory row).
pub const CLAUDE_RESET_CREDITS_USAGE_ITEM_ID: &str = "section:claude-reset-credits";
/// Claude Extra usage / prepaid balance block. Admin API spend is excluded.
pub const CLAUDE_EXTRA_USAGE_USAGE_ITEM_ID: &str = "section:claude-extra-usage";
/// Plan renewal / expiry dates shown beside the quota rows.
pub const SUBSCRIPTION_USAGE_ITEM_ID: &str = "section:subscription";

mod api_keys;
mod config_path;
mod cost_time_zone;
mod manual_cookies;
mod model_rate_limits;
mod optional_details;
mod preferences_document;
mod provider_workspace;
mod raw;
mod spend_dashboard;
mod status;
mod system_language;
mod types;

pub use api_keys::*;
pub use config_path::{
    CONFIG_PATH_ENV, apply_config_path_env, config_store_dir, resolve_config_path,
    settings_file_override,
};
pub use cost_time_zone::*;
pub use manual_cookies::*;
pub use model_rate_limits::{ModelRateLimit, rate_limit_number};
pub use optional_details::provider_has_optional_details;
pub use preferences_document::*;
pub use provider_workspace::*;
use raw::RawSettings;
pub use spend_dashboard::*;
pub use status::*;
pub use types::*;

#[cfg(test)]
mod tests;

/// Application settings
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum LowPowerModePreference {
    #[default]
    Off,
    On,
    Automatic,
}

impl LowPowerModePreference {
    pub fn resolve(self, system_battery_saver_enabled: bool) -> bool {
        match self {
            Self::Off => false,
            Self::On => true,
            Self::Automatic => system_battery_saver_enabled,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::On => "on",
            Self::Automatic => "automatic",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "off" => Some(Self::Off),
            "on" => Some(Self::On),
            "automatic" => Some(Self::Automatic),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(from = "RawSettings", default)]
pub struct Settings {
    /// Preferred display currency for supported spend amounts; AUTO keeps provider currencies.
    #[serde(default = "default_preferred_currency_code")]
    pub preferred_currency_code: String,

    /// Enabled provider IDs (by CLI name)
    pub enabled_providers: HashSet<String>,

    /// Refresh interval in seconds (0 = manual only).
    /// Ignored when [`Self::adaptive_refresh`] is true.
    pub refresh_interval_secs: u64,

    /// When true, ignore the fixed interval and use adaptive refresh delays.
    #[serde(default)]
    pub adaptive_refresh: bool,

    /// Force-refresh enabled providers whenever the tray/menu surface opens.
    #[serde(default)]
    pub refresh_all_providers_on_menu_open: bool,

    /// Poll provider status pages and surface incidents (upstream
    /// `statusChecksEnabled`, default on).
    #[serde(default = "default_true")]
    pub status_checks_enabled: bool,

    /// Off/On/Automatic background-work power preference (upstream 0.53).
    #[serde(default)]
    pub low_power_mode_preference: LowPowerModePreference,

    /// Whether to start minimized
    pub start_minimized: bool,

    /// Whether to start at login
    pub start_at_login: bool,

    /// Whether to show notifications
    pub show_notifications: bool,

    /// Whether to play sound effects for threshold alerts
    pub sound_enabled: bool,

    /// Per-notification WAV files. Unassigned events use the selected sound theme.
    #[serde(default)]
    pub notification_sound_paths: NotificationSoundPaths,

    /// Sound theme used when an event has no custom WAV file.
    #[serde(default)]
    pub notification_sound_theme: NotificationSoundTheme,

    /// High usage threshold for warnings (percentage)
    pub high_usage_threshold: f64,

    /// Critical usage threshold for alerts (percentage)
    pub critical_usage_threshold: f64,

    /// Merge mode: show all enabled providers in a single tray icon
    pub merge_tray_icons: bool,

    /// Tray icon display mode: single icon or per-provider icons
    #[serde(default)]
    pub tray_icon_mode: TrayIconMode,

    /// Optional preferred provider for the upper row of a stacked tray icon.
    /// Stale or disabled values are retained and ignored until eligible again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stacked_tray_top_provider: Option<String>,

    /// Optional preferred provider for the lower row of a stacked tray icon.
    /// Stale or duplicate values fall back to the next eligible provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stacked_tray_bottom_provider: Option<String>,

    /// Show provider icons in the merged switcher UI
    #[serde(default = "default_true")]
    pub switcher_shows_icons: bool,

    /// Prefer the provider closest to its limit in merged menu bar display
    #[serde(default)]
    pub menu_bar_shows_highest_usage: bool,

    /// Legacy "Show percent in tray" flag. No longer read: it only seeds
    /// `menu_bar_icon_style` for files written before that key existed.
    /// Kept (and still written) for one release so a downgrade keeps the style.
    #[serde(default)]
    pub menu_bar_shows_percent: bool,

    /// How the single tray icon is drawn: meter bars or icon + percent.
    /// Legacy files without the key follow `menu_bar_shows_percent`.
    #[serde(default)]
    pub menu_bar_icon_style: MenuBarIconStyle,

    /// Providers shown in the merged Overview, in provider order, at most
    /// [`MERGED_OVERVIEW_PROVIDER_LIMIT`]. Empty = the first enabled ones.
    #[serde(default)]
    pub merged_overview_selected_providers: Vec<String>,

    /// Draw quota-warning threshold ticks on usage bars.
    #[serde(default = "default_true")]
    pub quota_warning_markers_visible: bool,

    /// Tray panel Refresh row highlights under the pointer. Keyboard focus
    /// stays highlighted either way.
    #[serde(default = "default_true")]
    pub highlight_menu_cards_on_hover: bool,

    /// How workday boundaries appear on weekly usage bars.
    #[serde(default)]
    pub workday_tick_style: WorkdayTickStyle,

    /// Colour the Windows tray usage indicator by pace when enabled.
    #[serde(default)]
    pub menu_bar_color_pace: bool,

    /// Tint the Windows tray usage indicator with each shown provider's accent
    /// color (upstream "Color by provider", #4321). Defaults off.
    #[serde(default)]
    pub menu_bar_color_by_provider: bool,

    /// Show reserve as `+` and deficit as `-` in signed pace text (upstream
    /// `menuBarPaceReservePositive`). Display only: the stored pace delta stays
    /// usage-ahead, so pace colours and stages keep their meaning.
    #[serde(default)]
    pub menu_bar_pace_reserve_positive: bool,

    /// Settings sidebar lists providers alphabetically (enabled first)
    /// instead of in `provider_order`. Display only; never rewrites the order.
    #[serde(default)]
    pub providers_sorted_alphabetically: bool,

    /// Settings sidebar lists only enabled providers (upstream `providersEnabledOnly`).
    /// Local browsing filter: not part of the portable preferences document.
    #[serde(default)]
    pub providers_enabled_only: bool,

    /// Show usage bars as "used" (true) or "remaining" (false)
    pub show_as_used: bool,

    /// Enable UI animations (chart entrances, transitions)
    pub enable_animations: bool,

    /// Show reset times as relative (e.g., "2h 30m" instead of "3:00 PM")
    pub reset_time_relative: bool,

    /// Replace exhausted quota text with its concrete future reset time.
    #[serde(default)]
    pub show_reset_when_exhausted: bool,

    /// Warn when Codex or Claude pace predicts exhaustion before reset.
    #[serde(default)]
    pub predictive_pace_warning_enabled: bool,

    /// Announce when a session or weekly limit resets (opt-in).
    #[serde(default)]
    pub limit_reset_notifications_enabled: bool,

    /// Notify once per failure episode when a provider account needs sign-in again.
    #[serde(default)]
    pub credential_expiry_notifications_enabled: bool,

    /// Notify when a 5-hour session quota is depleted and when it becomes
    /// available again. Legacy files without the key inherit `show_notifications`.
    #[serde(default = "default_true")]
    pub session_quota_notifications_enabled: bool,

    /// Show pace visualizations and forecast text in provider menu cards.
    #[serde(default = "default_true")]
    pub show_pace: bool,

    /// Retired "Display mode" row ("minimal", "compact", "detailed"). Kept
    /// readable for one release; "minimal" is folded into
    /// `menu_bar_shows_highest_usage` once on load (`.display-mode-fold-v1`).
    pub menu_bar_display_mode: String,

    /// Overview card layout: "detailed" or "compact".
    #[serde(default = "default_overview_layout")]
    pub overview_layout: String,

    /// Show all token accounts in provider menus instead of collapsing behind switchers
    #[serde(default)]
    pub show_all_token_accounts_in_menu: bool,

    /// Per-provider configuration map (cookie/usage source, region, manual
    /// headers, API tokens, etc). Replaces the legacy flat per-provider
    /// fields; legacy `settings.json` files are migrated via [`RawSettings`].
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub provider_configs: HashMap<ProviderId, ProviderConfig>,

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

    /// Provider-switcher shortcut overrides (action -> shortcut, normalized).
    /// Empty means every action uses its default; see
    /// [`crate::switcher_shortcuts`].
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub switcher_shortcuts: BTreeMap<String, String>,

    /// Additional Codex home or sessions directories to include in local cost scans.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub codex_custom_sessions_dirs: Vec<String>,

    /// Extra Gemini home directories (each one's `.gemini`) whose Antigravity
    /// local token history is combined with the primary home. Empty keeps the
    /// single-home default. Affects local history only, not sign-in or quotas.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub antigravity_additional_profile_homes: Vec<String>,

    /// Discover local and configured SSH Codex/Claude sessions.
    #[serde(default)]
    pub agent_sessions_enabled: bool,

    /// Keep the system awake (no idle sleep) while a local agent session is live.
    #[serde(default)]
    pub stay_awake_enabled: bool,

    /// SSH targets queried for remote agent sessions.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub agent_session_ssh_hosts: Vec<String>,

    /// Master switch for external hooks (`hooks.json` next to settings).
    #[serde(default)]
    pub hooks_enabled: bool,

    /// Route provider/app HTTPS through a user-configured HTTP(S) proxy (#235).
    #[serde(default)]
    pub http_proxy_enabled: bool,

    /// Proxy base URL, e.g. `http://127.0.0.1:7890`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub http_proxy_url: String,

    /// Optional proxy basic-auth username.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub http_proxy_username: String,

    /// Optional proxy basic-auth password (stored in local settings.json).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub http_proxy_password: String,

    /// Automatically download updates in the background
    #[serde(default)]
    pub auto_download_updates: bool,

    /// Install pending updates when quitting the application
    #[serde(default)]
    pub install_updates_on_quit: bool,

    /// UI language for the application (English default for backward compatibility)
    #[serde(default)]
    pub ui_language: Language,

    /// Language "System": `ui_language` follows the Windows display
    /// language (Mac `AppLanguage.system`). True for new installs; false
    /// for files written before this setting existed, so their chosen
    /// language stays put.
    #[serde(default)]
    pub ui_language_follows_system: bool,

    /// UI theme preference (Phase 12). Defaults to Auto (prefers-color-scheme).
    #[serde(default)]
    pub theme: ThemePreference,

    /// Main PopOut window display scale, in the inclusive range 100..=250.
    /// 100 % is normal size; higher values enlarge the window content.
    #[serde(default = "default_window_scale_percent")]
    pub window_scale_percent: u16,

    /// Tray flyout display scale, in the inclusive range 100..=200.
    /// 100 % is normal size; higher values enlarge the flyout content.
    #[serde(default = "default_tray_scale_percent")]
    pub tray_scale_percent: u16,

    /// Keep the tray flyout above other windows after it loses focus.
    /// Disabled by default so the flyout retains normal z-order behavior.
    #[serde(default)]
    pub tray_panel_always_on_top: bool,

    /// Enable the local PowerToys Command Palette status pipe.
    #[serde(default)]
    pub powertoys_status_pipe_enabled: bool,

    /// Show the always-on-top floating capacity bar.
    #[serde(default)]
    pub float_bar_enabled: bool,

    /// Opacity of the floating bar window, in the inclusive range 30..=100.
    /// Stored as `u8` so the on-disk format remains stable.
    #[serde(default = "default_float_bar_opacity")]
    pub float_bar_opacity: u8,

    /// Floating-bar visual scale, in the inclusive range 75..=200.
    #[serde(default = "default_float_bar_scale")]
    pub float_bar_scale: u8,

    /// Floating-bar orientation: "horizontal" (default) or "vertical".
    #[serde(default = "default_float_bar_orientation")]
    pub float_bar_orientation: String,

    /// Floating-bar visual style: "floating" (default) or "taskbar".
    #[serde(default = "default_float_bar_style")]
    pub float_bar_style: String,

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

    /// When true, show the primary window's next reset inline in each pill.
    #[serde(default)]
    pub float_bar_show_reset_inline: bool,

    /// When true, show local cost summaries in the floating bar.
    #[serde(default)]
    pub float_bar_show_cost: bool,

    /// Promote the tray icon out of the Windows hidden-icons overflow area.
    /// Only has effect on Windows 11 (build ≥ 22000); silently ignored elsewhere.
    /// Defaults on so upgrades keep the icon pinned to the taskbar notification area.
    #[serde(default = "default_true")]
    pub promote_tray_icon: bool,

    /// When true, show Claude Daily Routines usage in extras.
    /// Defaults on to match upstream visibility.
    #[serde(default = "default_true")]
    pub claude_daily_routines_usage_visible: bool,

    /// Explicit consent to read (and refresh) Claude Code's own credentials
    /// (`~/.claude/.credentials.json` / Credential Manager). Default OFF —
    /// upstream #2634: without consent the OAuth source stays closed and Auto
    /// falls back to labeled reduced-fidelity CLI usage; refreshed tokens are
    /// never rotated into Claude Code's storage without consent (#2745).
    #[serde(default)]
    pub claude_allow_reading_claude_code_credentials: bool,

    /// Optional work-week length [2,7] for session-equivalent weekly forecast.
    /// `None` uses wall-clock time until weekly reset.
    #[serde(default)]
    pub weekly_progress_work_days: Option<u8>,

    /// Alibaba Token Plan API region: "cn" | "intl" | "cn-personal" | "intl-personal".
    #[serde(default = "default_alibaba_token_plan_region")]
    pub alibaba_token_plan_region: String,

    /// Opt-in: allow read-only Codex usage reads from the CLI-owned OAuth
    /// credential source. Win-CodexBar never refreshes or writes `auth.json`.
    /// Default OFF — when disabled, the source must contain refresh
    /// provenance; JWT expiry is checked when available (upstream 0.50.1
    /// #2944).
    #[serde(default)]
    pub codex_external_oauth_sources_allowed: bool,

    /// How cost is rendered on provider MenuCards (#2976).
    #[serde(default)]
    pub cost_summary_display_style: CostSummaryDisplayStyle,

    /// Opt-in read-only import of OpenCodex usage.jsonl into Usage & Spend / CLI cost output.
    #[serde(default)]
    pub open_codex_usage_logs_enabled: bool,

    /// Hide native Codex spend rows when an OpenCodex import is present.
    #[serde(default)]
    pub hide_native_codex_cost_when_open_codex_present: bool,

    /// History window for local cost surfaces: `rolling:N`, `month-to-date`, or
    /// `all` (upstream 0.67.0). Missing or unreadable values read as `rolling:30`.
    #[serde(default)]
    pub cost_reporting_period: CostReportingPeriod,

    /// IANA zone local cost history is bucketed in; empty means the machine
    /// zone (upstream 0.67.0 `tokenCostUsageBucketTimeZone`).
    #[serde(default)]
    pub cost_usage_bucket_time_zone: String,

    /// "Track costs" in Usage & Spend (Mac `costUsageEnabled`). Off stops the
    /// local cost-log scan, which blanks cost on the dashboard, cards and float
    /// bar. True for new installs and for files written before the key existed.
    #[serde(default = "default_true")]
    pub cost_usage_enabled: bool,

    /// Time range of the Usage & Spend dashboard, kept apart from
    /// `cost_reporting_period` (menu, float bar, share exports). A file without
    /// the key copies `cost_reporting_period`.
    #[serde(default)]
    pub spend_dashboard_period: CostReportingPeriod,

    /// Usage & Spend sources (provider ids) left out of the stat strip, charts,
    /// Copy JSON and Export JSON. Sorted and unique.
    #[serde(default)]
    pub spend_dashboard_hidden_source_ids: Vec<String>,

    /// Set when the loaded file still carried legacy `provider_usage_thresholds`;
    /// the desktop shell saves once at startup so only the new key stays on disk.
    /// Public only so other crates can build `Settings { .., ..Settings::default() }`.
    #[serde(skip)]
    pub legacy_usage_thresholds_pending: bool,
}

fn default_window_scale_percent() -> u16 {
    100
}

fn default_alibaba_token_plan_region() -> String {
    "cn".to_string()
}

pub fn clamp_window_scale_percent(value: u16) -> u16 {
    value.clamp(100, 250)
}

fn default_tray_scale_percent() -> u16 {
    100
}

/// Smallest Panel scale; `apps/desktop-tauri/src/lib/trayScale.ts` mirrors it.
pub const TRAY_SCALE_PERCENT_MIN: u16 = 100;
/// Largest Panel scale; `apps/desktop-tauri/src/lib/trayScale.ts` mirrors it.
pub const TRAY_SCALE_PERCENT_MAX: u16 = 200;

pub fn clamp_tray_scale_percent(value: u16) -> u16 {
    value.clamp(TRAY_SCALE_PERCENT_MIN, TRAY_SCALE_PERCENT_MAX)
}

fn default_float_bar_opacity() -> u8 {
    80
}

fn default_float_bar_scale() -> u8 {
    100
}

fn default_float_bar_orientation() -> String {
    "horizontal".to_string()
}

fn default_float_bar_style() -> String {
    "floating".to_string()
}

/// Clamp the floating-bar opacity to the supported range.
///
/// Opacity values below 30% would make the bar effectively invisible, so we
/// pin the lower bound; the upper bound is the natural 100%.
pub fn clamp_float_bar_opacity(value: u8) -> u8 {
    value.clamp(30, 100)
}

/// Clamp the floating-bar visual scale to the supported range.
pub fn clamp_float_bar_scale(value: u8) -> u8 {
    value.clamp(75, 200)
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

/// Normalize a floating-bar style string. Unknown values fall back to the
/// original floating style so existing settings keep their previous look.
pub fn normalize_float_bar_style(value: &str) -> String {
    match value {
        "taskbar" => "taskbar".to_string(),
        _ => "floating".to_string(),
    }
}

/// Canonicalize a requested provider display order.
///
/// Keeps requested provider IDs that map to a real [`ProviderId`], drops
/// duplicates, and appends omitted providers in canonical order. An empty
/// request intentionally returns the full canonical order so display callers
/// can use one path for default and customized ordering.
pub fn normalize_provider_order(requested: &[String]) -> Vec<String> {
    let canonical = ProviderId::all()
        .iter()
        .map(|provider| provider.cli_name().to_string())
        .collect::<Vec<_>>();
    let valid = canonical.iter().map(String::as_str).collect::<HashSet<_>>();
    let mut seen = HashSet::new();
    let mut out = Vec::with_capacity(canonical.len());

    for provider_id in requested {
        if valid.contains(provider_id.as_str()) && seen.insert(provider_id.clone()) {
            out.push(provider_id.clone());
        }
    }
    for provider_id in canonical {
        if seen.insert(provider_id.clone()) {
            out.push(provider_id);
        }
    }

    out
}

/// Replace only the slots held by `visible` inside `full`, so every other
/// provider (for example a soft-removed one) keeps its position. Returns
/// `None` unless `full` and `visible` have no repeats and every visible id is
/// present in `full`.
pub fn merge_visible_provider_order(full: &[String], visible: &[String]) -> Option<Vec<String>> {
    let full_set: HashSet<&str> = full.iter().map(String::as_str).collect();
    let visible_set: HashSet<&str> = visible.iter().map(String::as_str).collect();
    if full_set.len() != full.len()
        || visible_set.len() != visible.len()
        || !visible_set.is_subset(&full_set)
    {
        return None;
    }
    let mut next = visible.iter();
    Some(
        full.iter()
            .map(|id| {
                if visible_set.contains(id.as_str()) {
                    next.next().cloned().unwrap_or_else(|| id.clone())
                } else {
                    id.clone()
                }
            })
            .collect(),
    )
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
        ProviderId::Alibaba => crate::providers::AlibabaRegion::Singapore.settings_value(),
        ProviderId::AlibabaTokenPlan => "cn",
        ProviderId::Zai | ProviderId::MiniMax => "global",
        _ => "",
    }
}

/// Default for the codex `openai_web_extras` boolean (true = show extras).
const DEFAULT_CODEX_OPENAI_WEB_EXTRAS: bool = true;
const DEFAULT_CODEX_SPARK_USAGE_VISIBLE: bool = true;

fn normalize_hidden_usage_item_ids(ids: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut normalized = ids
        .into_iter()
        .map(|id| id.trim().to_string())
        .filter(|id| {
            !id.is_empty()
                && id.len() <= 128
                && !id.chars().any(char::is_control)
                && (id.starts_with(USAGE_ITEM_METRIC_PREFIX)
                    || id.starts_with(USAGE_ITEM_DETAIL_SECTION_PREFIX)
                    || id.starts_with(USAGE_ITEM_SECTION_PREFIX))
        })
        .filter(|id| seen.insert(id.clone()))
        .collect::<Vec<_>>();
    normalized.sort_unstable();
    normalized
}

impl Default for Settings {
    fn default() -> Self {
        let mut enabled = HashSet::new();
        // Default enabled providers
        enabled.insert("claude".to_string());
        enabled.insert("codex".to_string());

        Self {
            preferred_currency_code: "AUTO".to_string(),
            enabled_providers: enabled,
            refresh_interval_secs: 300, // 5 minutes
            // New installs use adaptive refresh; legacy files without the key
            // keep the fixed interval through the field default in `RawSettings`.
            adaptive_refresh: true,
            refresh_all_providers_on_menu_open: false,
            status_checks_enabled: true,
            low_power_mode_preference: LowPowerModePreference::Off,
            start_minimized: false,
            start_at_login: false,
            show_notifications: true,
            sound_enabled: true,
            notification_sound_paths: NotificationSoundPaths::default(),
            notification_sound_theme: NotificationSoundTheme::default(),
            high_usage_threshold: 70.0,
            critical_usage_threshold: 90.0,
            merge_tray_icons: false, // Show single provider by default
            tray_icon_mode: TrayIconMode::default(), // Single icon by default
            stacked_tray_top_provider: None,
            stacked_tray_bottom_provider: None,
            switcher_shows_icons: true,
            menu_bar_shows_highest_usage: false,
            menu_bar_shows_percent: false,
            menu_bar_icon_style: MenuBarIconStyle::Bars,
            merged_overview_selected_providers: Vec::new(),
            quota_warning_markers_visible: true,
            highlight_menu_cards_on_hover: true,
            workday_tick_style: WorkdayTickStyle::Subtle,
            menu_bar_color_pace: false,
            menu_bar_color_by_provider: false,
            menu_bar_pace_reserve_positive: false,
            providers_sorted_alphabetically: false,
            providers_enabled_only: false,
            show_as_used: false,
            enable_animations: true,   // Animations enabled by default
            reset_time_relative: true, // Show relative times by default
            show_reset_when_exhausted: false,
            predictive_pace_warning_enabled: false,
            limit_reset_notifications_enabled: false,
            credential_expiry_notifications_enabled: false,
            session_quota_notifications_enabled: true,
            show_pace: true,
            menu_bar_display_mode: "detailed".to_string(), // Detailed mode by default
            overview_layout: default_overview_layout(),
            show_all_token_accounts_in_menu: false,
            provider_configs: HashMap::new(),
            disable_keychain_access: false,
            hide_personal_info: false, // Show personal info by default
            update_channel: UpdateChannel::default(), // Stable by default
            provider_metrics: HashMap::new(), // Empty = use Automatic for all
            provider_order: Vec::new(), // Empty = canonical ProviderId::all() order
            global_shortcut: default_global_shortcut(), // Ctrl+Shift+U by default
            switcher_shortcuts: BTreeMap::new(),
            codex_custom_sessions_dirs: Vec::new(),
            antigravity_additional_profile_homes: Vec::new(),
            agent_sessions_enabled: false,
            stay_awake_enabled: false,
            agent_session_ssh_hosts: Vec::new(),
            hooks_enabled: false,
            http_proxy_enabled: false,
            http_proxy_url: String::new(),
            http_proxy_username: String::new(),
            http_proxy_password: String::new(),
            auto_download_updates: false, // Require explicit opt-in for background downloads
            install_updates_on_quit: false, // Don't auto-install on quit by default
            ui_language: Language::default(), // English until the system language resolves
            ui_language_follows_system: true,
            theme: ThemePreference::default(), // Auto (follows prefers-color-scheme)
            window_scale_percent: default_window_scale_percent(),
            tray_scale_percent: default_tray_scale_percent(),
            tray_panel_always_on_top: false,
            powertoys_status_pipe_enabled: false,
            float_bar_enabled: false,
            float_bar_opacity: default_float_bar_opacity(),
            float_bar_scale: default_float_bar_scale(),
            float_bar_orientation: default_float_bar_orientation(),
            float_bar_style: default_float_bar_style(),
            float_bar_click_through: false,
            float_bar_provider_ids: Vec::new(),
            float_bar_dark_text: false,
            float_bar_show_reset_inline: false,
            float_bar_show_cost: false,
            promote_tray_icon: true,
            claude_daily_routines_usage_visible: true,
            claude_allow_reading_claude_code_credentials: false,
            weekly_progress_work_days: None,
            alibaba_token_plan_region: default_alibaba_token_plan_region(),
            codex_external_oauth_sources_allowed: false,
            legacy_usage_thresholds_pending: false,
            cost_summary_display_style: CostSummaryDisplayStyle::default(),
            open_codex_usage_logs_enabled: false,
            hide_native_codex_cost_when_open_codex_present: false,
            cost_reporting_period: CostReportingPeriod::default(),
            cost_usage_bucket_time_zone: String::new(),
            cost_usage_enabled: true,
            spend_dashboard_period: CostReportingPeriod::default(),
            spend_dashboard_hidden_source_ids: Vec::new(),
        }
    }
}

fn default_preferred_currency_code() -> String {
    "AUTO".to_string()
}

fn default_overview_layout() -> String {
    "detailed".to_string()
}

pub fn normalize_overview_layout(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "compact" => "compact".to_string(),
        "detailed" => "detailed".to_string(),
        _ => default_overview_layout(),
    }
}

impl Settings {
    /// Get the settings file path (`CODEXBAR_CONFIG` when the CLI applied it).
    pub fn settings_path() -> Option<PathBuf> {
        config_path::settings_file_for(settings_file_override(), crate::logging::config_root())
    }

    /// Load settings from disk
    pub fn load() -> Self {
        let path = Self::settings_path();
        #[allow(
            unused_mut,
            reason = "mutability is needed for conditional initialization paths that the compiler cannot prove"
        )]
        let mut settings = Self::load_from_path(path.as_deref());

        // Sync autostart toggle with actual registry state and repair stale commands from older builds.
        // A CLI run on a `CODEXBAR_CONFIG` file skips these desktop integrations, so it touches
        // neither the Run key nor the default config root.
        #[cfg(target_os = "windows")]
        if settings_file_override().is_none() {
            settings.start_at_login = Self::sync_start_at_login_registry();
            settings.apply_promote_tray_default_migration();
        }

        // One-shot migration: materialize the persisted hidden usage-item list
        // from the pre-0.62 per-provider visibility flags. After this the list
        // is the sole source of truth and the flags stay untouched.
        settings.migrate_legacy_usage_item_flags();

        settings.resolve_system_language();

        settings
    }

    fn load_from_path(path: Option<&Path>) -> Self {
        match path {
            Some(path) if path.exists() => match crate::secure_file::read_string(path) {
                Ok(content) => {
                    serde_json::from_str(content.trim_start_matches('\u{feff}')).unwrap_or_default()
                }
                Err(_) => Self::default(),
            },
            _ => Self::default(),
        }
    }

    /// Materialize `hidden_usage_item_ids` from the pre-0.62 per-provider
    /// visibility flags where the list was never persisted. Idempotent: a
    /// provider with an explicit list is left alone.
    fn migrate_legacy_usage_item_flags(&mut self) {
        if self
            .provider_config(ProviderId::Codex)
            .and_then(|config| config.hidden_usage_item_ids.as_ref())
            .is_none()
            && !self.spark_usage_visible(ProviderId::Codex)
        {
            self.toggle_hidden_items(ProviderId::Codex, &CODEX_SPARK_USAGE_ITEM_IDS, false);
        }
        if self
            .provider_config(ProviderId::Claude)
            .and_then(|config| config.hidden_usage_item_ids.as_ref())
            .is_none()
            && !self.claude_daily_routines_usage_visible
        {
            self.toggle_hidden_items(
                ProviderId::Claude,
                &[CLAUDE_DAILY_ROUTINES_USAGE_ITEM_ID],
                false,
            );
        }
    }

    /// Marker written after the one-shot "pin tray by default" migration (issue #237).
    fn promote_tray_default_marker_path() -> Option<PathBuf> {
        crate::logging::config_root().map(|p| p.join(".tray-pin-default-v1"))
    }

    /// Old builds defaulted `promote_tray_icon` to false and persisted that on any
    /// settings save. Flip those installs to the new default once; later opt-outs
    /// are preserved because the marker file remains.
    fn should_migrate_promote_tray_default(
        promote_tray_icon: bool,
        already_migrated: bool,
    ) -> bool {
        !already_migrated && !promote_tray_icon
    }

    fn apply_promote_tray_default_migration(&mut self) {
        let Some(marker) = Self::promote_tray_default_marker_path() else {
            return;
        };
        let already_migrated = marker.exists();
        if Self::should_migrate_promote_tray_default(self.promote_tray_icon, already_migrated) {
            self.promote_tray_icon = true;
            if let Err(error) = self.save() {
                tracing::warn!("Failed to persist promote_tray_icon default migration: {error}");
            }
        }
        if !already_migrated && let Some(parent) = marker.parent() {
            // Best-effort marker dir creation; the write below reports failure.
            let _created_dir = std::fs::create_dir_all(parent);
            if let Err(error) = std::fs::write(&marker, b"1") {
                tracing::warn!("Failed to write promote_tray_icon migration marker: {error}");
            }
        }
    }

    /// Marker written after the one-shot Display mode "minimal" fold (Mac parity PR 3).
    fn display_mode_fold_marker_path() -> Option<PathBuf> {
        crate::logging::config_root().map(|p| p.join(".display-mode-fold-v1"))
    }

    /// The Display mode row is gone. "Minimal" used to force the most-used
    /// provider into the tray, so carry that into `menu_bar_shows_highest_usage`
    /// once. Returns true when the settings changed. Later opt-outs stick
    /// because the marker remains.
    fn fold_minimal_display_mode(&mut self, already_migrated: bool) -> bool {
        if already_migrated
            || self.menu_bar_shows_highest_usage
            || !self
                .menu_bar_display_mode
                .trim()
                .eq_ignore_ascii_case("minimal")
        {
            return false;
        }
        self.menu_bar_shows_highest_usage = true;
        true
    }

    /// Run the one-shot Display mode fold against the default config root.
    /// The desktop shell calls this once at startup rather than from
    /// [`Settings::load`], so unit tests and helpers that load settings never
    /// write the marker into the real config dir. A `CODEXBAR_CONFIG` run skips it.
    pub fn apply_display_mode_fold_migration(&mut self) {
        if settings_file_override().is_some() {
            return;
        }
        let Some(marker) = Self::display_mode_fold_marker_path() else {
            return;
        };
        self.apply_display_mode_fold_with_marker(&marker, Self::save);
    }

    fn apply_display_mode_fold_with_marker(
        &mut self,
        marker: &Path,
        persist: impl FnOnce(&Self) -> anyhow::Result<()>,
    ) {
        let already_migrated = marker.exists();
        if self.fold_minimal_display_mode(already_migrated)
            && let Err(error) = persist(self)
        {
            // No marker: the fold retries on the next launch instead of being lost.
            tracing::warn!("Failed to persist display mode fold migration: {error}");
            return;
        }
        if !already_migrated && let Some(parent) = marker.parent() {
            // Best-effort marker dir creation; the write below reports failure.
            let _created_dir = std::fs::create_dir_all(parent);
            if let Err(error) = std::fs::write(marker, b"1") {
                tracing::warn!("Failed to write display mode fold marker: {error}");
            }
        }
    }

    /// Migration #5 write-back: when the loaded file still had legacy
    /// `provider_usage_thresholds` (already folded into `quota_warnings` on
    /// read), save once so the file holds only the new key. The desktop shell
    /// calls this at startup; tests and helpers that only load never write.
    pub fn apply_quota_warning_migration(&mut self) {
        self.apply_quota_warning_migration_with(Self::save);
    }

    fn apply_quota_warning_migration_with(
        &mut self,
        persist: impl FnOnce(&Self) -> anyhow::Result<()>,
    ) {
        if !self.legacy_usage_thresholds_pending {
            return;
        }
        match persist(self) {
            Ok(()) => self.legacy_usage_thresholds_pending = false,
            // The legacy key stays readable, so the write retries next launch.
            Err(error) => tracing::warn!("Failed to persist quota warning migration: {error}"),
        }
    }

    /// Store the merged Overview selection: canonical ids, deduplicated,
    /// at most [`MERGED_OVERVIEW_PROVIDER_LIMIT`].
    pub fn set_merged_overview_selected_providers(&mut self, ids: Vec<String>) {
        self.merged_overview_selected_providers = raw::normalize_merged_overview_selection(ids);
    }

    /// Save settings to disk
    pub fn save(&self) -> anyhow::Result<()> {
        let path = Self::settings_path()
            .ok_or_else(|| anyhow::anyhow!("Could not determine settings path"))?;

        self.save_to_path(&path)
    }

    fn save_to_path(&self, path: &Path) -> anyhow::Result<()> {
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let json = serde_json::to_string_pretty(self)?;
        crate::secure_file::write_string(path, &json)?;

        Ok(())
    }

    fn start_at_login_exe_path(current_exe: &std::path::Path) -> std::path::PathBuf {
        let file_name = current_exe.file_name().and_then(|name| name.to_str());
        if file_name.is_some_and(|name| {
            name.eq_ignore_ascii_case("codexbar-cli.exe")
                || name.eq_ignore_ascii_case("codexbar-desktop.exe")
        }) && let Some(desktop_exe) = current_exe
            .parent()
            .map(|dir| dir.join("codexbar.exe"))
            .filter(|path| path.exists())
        {
            return desktop_exe;
        }

        current_exe.to_path_buf()
    }

    fn start_at_login_command(current_exe: &std::path::Path) -> String {
        let exe_path = Self::start_at_login_exe_path(current_exe);
        format!("\"{}\"", exe_path.display())
    }

    fn start_at_login_command_needs_repair(existing: &str, current_exe: &std::path::Path) -> bool {
        existing != Self::start_at_login_command(current_exe)
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
            // Best-effort removal; a missing value means the desired state already.
            let _removed_value = run_key.delete_value("CodexBar");
        }

        Ok(())
    }

    #[cfg(target_os = "windows")]
    fn sync_start_at_login_registry() -> bool {
        use winreg::RegKey;
        use winreg::enums::*;

        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let Ok(run_key) = hkcu.open_subkey_with_flags(
            r"Software\Microsoft\Windows\CurrentVersion\Run",
            KEY_READ | KEY_WRITE,
        ) else {
            return false;
        };

        let Ok(existing) = run_key.get_value::<String, _>("CodexBar") else {
            return false;
        };

        match std::env::current_exe() {
            Ok(exe_path) if Self::start_at_login_command_needs_repair(&existing, &exe_path) => {
                let command = Self::start_at_login_command(&exe_path);
                if let Err(error) = run_key.set_value("CodexBar", &command) {
                    tracing::warn!("Failed to repair CodexBar start-at-login command: {error}");
                }
            }
            Err(error) => {
                tracing::warn!(
                    "Failed to resolve current executable for start-at-login sync: {error}"
                );
            }
            _ => {}
        }

        true
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

    /// Whether a provider belongs in user-facing listings: retired
    /// (deprecated) providers are shown only while enabled.
    pub fn is_provider_listed(&self, id: ProviderId) -> bool {
        !id.is_deprecated() || self.is_provider_enabled(id)
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
        self.provider_display_order()
            .into_iter()
            .filter(|id| self.is_provider_enabled(*id))
            .collect()
    }

    /// Get all available providers with their enabled status
    pub fn get_all_providers_status(&self) -> Vec<ProviderStatus> {
        self.provider_display_order()
            .into_iter()
            .map(|id| ProviderStatus {
                id: id.cli_name().to_string(),
                name: id.display_name().to_string(),
                enabled: self.is_provider_enabled(id),
            })
            .collect()
    }

    /// Provider display order as typed IDs, falling back to canonical order
    /// when no custom order has been persisted.
    pub fn provider_display_order(&self) -> Vec<ProviderId> {
        normalize_provider_order(&self.provider_order)
            .into_iter()
            .filter_map(|provider_id| ProviderId::from_cli_name(&provider_id))
            .collect()
    }

    /// Provider display order as CLI-name strings.
    pub fn provider_display_order_names(&self) -> Vec<String> {
        normalize_provider_order(&self.provider_order)
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

    /// Cookie source for `id`. Kimi, Charm Hyper and Groq follow upstream's
    /// automatic default; providers with no specific default retain the
    /// legacy manual default.
    pub fn cookie_source(&self, id: ProviderId) -> &str {
        self.provider_configs
            .get(&id)
            .and_then(|c| c.cookie_source.as_deref())
            .unwrap_or(match id {
                ProviderId::Kimi
                | ProviderId::Hyper
                | ProviderId::Groq
                | ProviderId::Langdock
                | ProviderId::LithosAI
                | ProviderId::MuseAI
                | ProviderId::WorkBuddy
                | ProviderId::TinyApi
                | ProviderId::Xapi
                | ProviderId::Parallel => "auto",
                _ => DEFAULT_COOKIE_SOURCE,
            })
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
        if id == ProviderId::AlibabaTokenPlan && !self.alibaba_token_plan_region.trim().is_empty() {
            return self.alibaba_token_plan_region.as_str();
        }
        self.provider_configs
            .get(&id)
            .and_then(|c| c.api_region.as_deref())
            .unwrap_or_else(|| default_api_region(id))
    }

    pub fn set_api_region(&mut self, id: ProviderId, region: impl Into<String>) {
        let region = region.into();
        if id == ProviderId::AlibabaTokenPlan {
            self.alibaba_token_plan_region = region.clone();
        }
        self.provider_config_mut(id).api_region = Some(region);
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

    pub fn management_api_token(&self, id: ProviderId) -> Option<&str> {
        self.provider_configs
            .get(&id)
            .and_then(|config| config.management_api_token.as_deref())
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }

    pub fn set_management_api_token(&mut self, id: ProviderId, token: Option<String>) {
        let token = token
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        self.provider_config_mut(id).management_api_token = token;
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

    /// Optional user-entered allowance for Copilot seat AI credits.
    ///
    /// GitHub reports the absolute `credits_used` counter but does not expose
    /// a documented included-credit ceiling, so callers must keep an absent
    /// or non-positive value as unknown rather than inventing a denominator.
    ///
    /// This setter is the single owner of the positive-finite invariant:
    /// invalid values are rejected instead of silently dropped, while the
    /// getter keeps defensively filtering values persisted by older builds.
    pub fn seat_credit_entitlement(&self, id: ProviderId) -> Option<f64> {
        self.provider_configs
            .get(&id)
            .and_then(|config| config.seat_credit_entitlement)
            .filter(|value| value.is_finite() && *value > 0.0)
    }

    pub fn set_seat_credit_entitlement(
        &mut self,
        id: ProviderId,
        value: Option<f64>,
    ) -> Result<(), String> {
        if let Some(value) = value
            && (!value.is_finite() || value <= 0.0)
        {
            return Err(
                "Copilot seat AI-credit allowance must be a finite number greater than zero"
                    .to_string(),
            );
        }
        self.provider_config_mut(id).seat_credit_entitlement = value;
        Ok(())
    }

    /// Wayfinder gateway URL, defaulting to the local loopback gateway.
    pub fn gateway_url(&self, id: ProviderId) -> &str {
        self.provider_configs
            .get(&id)
            .and_then(|c| c.gateway_url.as_deref())
            .unwrap_or_else(|| {
                if id == ProviderId::Wayfinder {
                    crate::providers::wayfinder::DEFAULT_GATEWAY_URL
                } else {
                    ""
                }
            })
    }

    pub fn set_gateway_url(&mut self, id: ProviderId, value: impl Into<String>) {
        self.provider_config_mut(id).gateway_url = Some(value.into());
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

    /// Codex Spark rows are visible by default.
    pub fn spark_usage_visible(&self, id: ProviderId) -> bool {
        self.provider_configs
            .get(&id)
            .and_then(|c| c.spark_usage_visible)
            .unwrap_or(DEFAULT_CODEX_SPARK_USAGE_VISIBLE)
    }

    pub fn set_spark_usage_visible(&mut self, id: ProviderId, value: bool) {
        self.provider_config_mut(id).spark_usage_visible = Some(value);
    }

    /// Return the persisted hidden usage-item IDs for `id`.
    pub fn hidden_usage_item_ids(&self, id: ProviderId) -> Vec<String> {
        self.provider_configs
            .get(&id)
            .and_then(|config| config.hidden_usage_item_ids.as_ref())
            .map_or_else(Vec::new, |ids| normalize_hidden_usage_item_ids(ids.clone()))
    }

    /// Persist an explicit presentation-only usage-item visibility list.
    pub fn set_hidden_usage_item_ids(&mut self, id: ProviderId, ids: Vec<String>) {
        let hidden = normalize_hidden_usage_item_ids(ids);
        self.provider_config_mut(id).hidden_usage_item_ids = Some(hidden);
    }

    /// Add or remove `items` from the provider's hidden usage-item list.
    ///
    /// `visible = false` hides the items; `visible = true` un-hides them. This
    /// is the single write path for usage-item visibility; it only touches the
    /// presentation list and never the legacy per-provider boolean flags.
    pub fn toggle_hidden_items(&mut self, id: ProviderId, items: &[&str], visible: bool) {
        let mut hidden = self.hidden_usage_item_ids(id);
        if visible {
            hidden.retain(|item| !items.contains(&item.as_str()));
        } else {
            hidden.extend(items.iter().map(|item| (*item).to_string()));
        }
        self.set_hidden_usage_item_ids(id, hidden);
    }

    /// Update the old Claude Daily Routines flag without touching the
    /// usage-item list; the flag is presentation-only and kept for callers
    /// that still read the boolean directly.
    pub fn set_claude_daily_routines_usage_visible(&mut self, value: bool) {
        self.claude_daily_routines_usage_visible = value;
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

    /// Codex "Local session cost estimates" option (persisted only).
    pub fn local_session_cost_ledger_enabled(&self, id: ProviderId) -> bool {
        self.provider_configs
            .get(&id)
            .is_some_and(|c| c.local_session_cost_ledger_enabled)
    }

    pub fn set_local_session_cost_ledger_enabled(&mut self, id: ProviderId, value: bool) {
        self.provider_config_mut(id)
            .local_session_cost_ledger_enabled = value;
    }

    /// Codex OpenAI web battery saver option (persisted only).
    pub fn openai_web_battery_saver_enabled(&self, id: ProviderId) -> bool {
        self.provider_configs
            .get(&id)
            .is_some_and(|c| c.openai_web_battery_saver_enabled)
    }

    pub fn set_openai_web_battery_saver_enabled(&mut self, id: ProviderId, value: bool) {
        self.provider_config_mut(id)
            .openai_web_battery_saver_enabled = value;
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

    /// Whether the desktop shell may reopen a captured CLI session after its
    /// provider quota becomes available again. This is intentionally opt-in.
    pub fn auto_resume_after_quota_reset(&self, id: ProviderId) -> bool {
        self.provider_configs
            .get(&id)
            .map(|config| config.auto_resume_after_quota_reset)
            .unwrap_or(false)
    }

    pub fn set_auto_resume_after_quota_reset(&mut self, id: ProviderId, value: bool) {
        self.provider_config_mut(id).auto_resume_after_quota_reset = value;
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
    pub fn codex_spark_usage_visible(&self) -> bool {
        self.spark_usage_visible(ProviderId::Codex)
    }
    pub fn set_codex_spark_usage_visible(&mut self, v: bool) {
        self.set_spark_usage_visible(ProviderId::Codex, v)
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

    /// Claude-only: whether the external claude-swap (`cswap`) adapter is
    /// enabled. Disabled by default.
    pub fn claude_swap_enabled(&self) -> bool {
        self.provider_configs
            .get(&ProviderId::Claude)
            .map(|config| config.claude_swap_enabled)
            .unwrap_or(false)
    }

    pub fn set_claude_swap_enabled(&mut self, value: bool) {
        self.provider_config_mut(ProviderId::Claude)
            .claude_swap_enabled = value;
    }

    /// Claude-only: configured claude-swap executable path, or `""` when unset.
    pub fn claude_swap_executable_path(&self) -> &str {
        self.provider_configs
            .get(&ProviderId::Claude)
            .and_then(|config| config.claude_swap_executable_path.as_deref())
            .unwrap_or("")
    }

    pub fn set_claude_swap_executable_path(&mut self, value: impl Into<String>) {
        let trimmed = value.into().trim().to_string();
        let config = self.provider_config_mut(ProviderId::Claude);
        config.claude_swap_executable_path = if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    }

    // ── Per-provider accent color override (#2972) ──────────────────

    /// The user-overridden accent color for `id`, or `None` to use the
    /// shipped brand color.
    pub fn accent_color(&self, id: ProviderId) -> Option<&str> {
        self.provider_configs
            .get(&id)
            .and_then(|c| c.accent_color.as_deref())
            .filter(|s| !s.trim().is_empty())
    }

    /// Set the accent color override for `id`. Pass an empty string or
    /// `None` to clear the override and revert to the shipped brand color.
    pub fn set_accent_color(&mut self, id: ProviderId, color: Option<impl Into<String>>) {
        let entry = self.provider_config_mut(id);
        entry.accent_color = color
            .map(Into::into)
            .filter(|s: &String| !s.trim().is_empty());
    }

    /// Resolve the effective accent color for `id`: the user override if
    /// set, otherwise the shipped brand color from the provider registry.
    pub fn effective_accent_color(&self, id: ProviderId) -> String {
        if let Some(override_color) = self.accent_color(id) {
            return override_color.trim().to_string();
        }
        crate::core::brand_color(id).to_string()
    }
}
