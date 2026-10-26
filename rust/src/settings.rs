//! Settings management for CodexBar
//!
//! Handles persistent configuration including:
//! - Enabled/disabled providers
//! - Refresh interval
//! - Manual cookies
//! - Other user preferences

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
mod http_server;
mod manual_cookies;
mod model_rate_limits;
mod optional_details;
mod persistence;
mod preferences_document;
mod provider_accessors;
mod provider_workspace;
mod raw;
mod spend_dashboard;
mod start_at_login;
mod system_language;
mod types;

pub use api_keys::*;
pub use config_path::{
    CONFIG_PATH_ENV, apply_config_path_env, config_store_dir, resolve_config_path,
    settings_file_override,
};
pub use cost_time_zone::*;
pub use http_server::*;
pub use manual_cookies::*;
pub use model_rate_limits::{ModelRateLimit, rate_limit_number};
pub use optional_details::provider_has_optional_details;
pub use preferences_document::*;
pub use provider_workspace::*;
use raw::RawSettings;
pub use spend_dashboard::*;
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

    /// Expand each selected Overview provider into one row per cached account
    /// (claude-swap/tcr, managed Codex, token accounts). Off by default; adds no fetches.
    #[serde(default)]
    pub merged_overview_all_accounts: bool,

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

    /// Only warn once the estimated run-out is this many hours away or less.
    /// `0` warns as soon as the pace predicts running out before the reset.
    #[serde(default)]
    pub predictive_pace_warning_lead_hours: u16,

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

    /// Limit Agent Sessions to this PC: no Tailscale probe and no SSH polling.
    /// Off by default, so remote discovery keeps running as before.
    #[serde(default)]
    pub agent_sessions_local_only: bool,

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

    /// Write a usage cache that the `codexbar` CLI falls back to when it
    /// cannot decrypt its own credentials (upstream `shareUsageWithCLI`).
    /// Off by default; machine-local.
    #[serde(default)]
    pub share_usage_with_cli: bool,

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

    /// Whether these settings may be written back to `settings.json`.
    /// Public only so other crates can build `Settings { .., ..Settings::default() }`.
    #[serde(skip)]
    pub load_state: SettingsLoadState,
}

/// Where a [`Settings`] value came from, as far as saving is concerned.
///
/// A `settings.json` that exists but cannot be read or parsed (a DPAPI
/// decrypt failure, a truncated write, a hand edit) still holds the user's
/// API keys, cookies and accounts. The app runs on in-memory defaults then,
/// and [`Settings::save`] refuses to replace the file until a later load
/// reads it successfully. The field is private so only the loader can set
/// the blocked state.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SettingsLoadState {
    unreadable_file: bool,
}

impl SettingsLoadState {
    pub(crate) const UNREADABLE_FILE: Self = Self {
        unreadable_file: true,
    };

    /// True when the settings file exists but could not be loaded, so saving
    /// these defaults would destroy it.
    pub fn is_unreadable_file(self) -> bool {
        self.unreadable_file
    }
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

/// Longest lead a pace warning can wait for: a full weekly window.
pub const MAX_PREDICTIVE_PACE_WARNING_LEAD_HOURS: u16 = 168;

pub fn clamp_predictive_pace_warning_lead_hours(value: u16) -> u16 {
    value.min(MAX_PREDICTIVE_PACE_WARNING_LEAD_HOURS)
}

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
            merged_overview_all_accounts: false,
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
            predictive_pace_warning_lead_hours: 0,
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
            agent_sessions_local_only: false,
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
            share_usage_with_cli: false,
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
            load_state: SettingsLoadState::default(),
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
    /// Check if a provider is enabled
    /// How close the estimated run-out must be before a pace warning shows,
    /// in seconds; `None` warns as soon as the pace predicts running out.
    pub fn predictive_pace_warning_lead_seconds(&self) -> Option<f64> {
        match self.predictive_pace_warning_lead_hours {
            0 => None,
            hours => Some(f64::from(hours) * 3600.0),
        }
    }

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

    /// Built-in order of the providers the Settings sidebar lists. It is the
    /// target of "Restore default order" and decides whether that action is
    /// available.
    pub fn provider_default_order_names(&self) -> Vec<String> {
        ProviderId::all()
            .iter()
            .filter(|provider| self.is_provider_listed(**provider))
            .map(|provider| provider.cli_name().to_string())
            .collect()
    }

    /// Get the metric preference for a provider
    pub fn get_provider_metric(&self, id: ProviderId) -> MetricPreference {
        self.provider_metrics
            .get(id.cli_name())
            .cloned()
            .unwrap_or_default()
    }

    /// Set the metric preference for a provider
    pub fn set_provider_metric(&mut self, id: ProviderId, metric: MetricPreference) {
        self.provider_metrics
            .insert(id.cli_name().to_string(), metric);
    }
}
