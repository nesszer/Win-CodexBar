use super::*;

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct UsageThresholdOverride {
    pub high: Option<f64>,
    pub critical: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UsageThresholds {
    pub high: f64,
    pub critical: f64,
}

/// WAV files assigned to individual notification events.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct NotificationSoundPaths {
    pub predictive_warning: Option<String>,
    pub high_usage: Option<String>,
    pub critical_usage: Option<String>,
    pub exhausted: Option<String>,
    pub status_issue: Option<String>,
    pub session_depleted: Option<String>,
    pub session_restored: Option<String>,
}

/// Sound theme used when an event has no custom WAV file.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NotificationSoundTheme {
    /// Use the existing Windows system-sound mapping.
    #[default]
    Windows,
    /// Use a distinct built-in CodexBar sound for each event.
    CodexBar,
}

pub fn normalize_usage_threshold_overrides(
    values: HashMap<String, UsageThresholdOverride>,
) -> HashMap<String, UsageThresholdOverride> {
    let known = crate::core::cli_name_map();
    values
        .into_iter()
        .filter_map(|(key, mut value)| {
            let (provider, window) = key
                .split_once(':')
                .map_or((key.as_str(), None), |(provider, window)| {
                    (provider, Some(window))
                });
            if !known.contains_key(provider)
                || window.is_some_and(|window| !matches!(window, "session" | "weekly"))
            {
                return None;
            }
            value.high = value.high.map(|number| number.clamp(0.0, 100.0));
            value.critical = value.critical.map(|number| number.clamp(0.0, 100.0));
            (value.high.is_some() || value.critical.is_some()).then_some((key, value))
        })
        .collect()
}

/// The two quota windows that carry per-provider warning settings.
pub const QUOTA_WARNING_WINDOWS: [&str; 2] = ["session", "weekly"];

/// One window's per-provider quota warning settings (Mac
/// `QuotaWarningWindowConfig`). Values are *used* percentages.
///
/// - Global: every field is absent; the global thresholds apply.
/// - Custom: `high` and/or `critical` is set; a missing value inherits global.
/// - Off: `enabled == Some(false)`; saved values are kept for a later Custom.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct QuotaWarningWindow {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub high: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub critical: Option<f64>,
}

impl QuotaWarningWindow {
    /// True when the window follows the global settings (nothing stored).
    pub fn is_global(&self) -> bool {
        self.enabled.is_none() && self.high.is_none() && self.critical.is_none()
    }

    /// Clamp stored values to 0..=100 and drop non-finite numbers.
    pub fn normalized(self) -> Self {
        let clamp = |value: Option<f64>| {
            value
                .filter(|number| number.is_finite())
                .map(|number| number.clamp(0.0, 100.0))
        };
        Self {
            enabled: self.enabled,
            high: clamp(self.high),
            critical: clamp(self.critical),
        }
    }
}

/// Per-provider quota warning settings for the session and weekly windows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProviderQuotaWarnings {
    #[serde(skip_serializing_if = "QuotaWarningWindow::is_global")]
    pub session: QuotaWarningWindow,
    #[serde(skip_serializing_if = "QuotaWarningWindow::is_global")]
    pub weekly: QuotaWarningWindow,
}

impl ProviderQuotaWarnings {
    pub fn is_empty(&self) -> bool {
        self.session.is_global() && self.weekly.is_global()
    }

    pub fn window(&self, window: &str) -> Option<&QuotaWarningWindow> {
        match window {
            "session" => Some(&self.session),
            "weekly" => Some(&self.weekly),
            _ => None,
        }
    }

    pub fn window_mut(&mut self, window: &str) -> Option<&mut QuotaWarningWindow> {
        match window {
            "session" => Some(&mut self.session),
            "weekly" => Some(&mut self.weekly),
            _ => None,
        }
    }

    pub fn normalized(self) -> Self {
        Self {
            session: self.session.normalized(),
            weekly: self.weekly.normalized(),
        }
    }
}

/// Migration #5: fold legacy `provider_usage_thresholds` entries
/// (`<provider>` and `<provider>:<window>`) into
/// `provider_configs.<provider>.quota_warnings`.
///
/// Per field, the window key wins over the provider key; a value neither key
/// sets stays absent and keeps inheriting the global threshold. A window that
/// already has new-format settings is left untouched.
pub(super) fn migrate_legacy_usage_thresholds(
    provider_configs: &mut HashMap<ProviderId, ProviderConfig>,
    legacy: HashMap<String, UsageThresholdOverride>,
) {
    let known = crate::core::cli_name_map();
    let legacy = normalize_usage_threshold_overrides(legacy);
    let mut providers: Vec<(&str, ProviderId)> = legacy
        .keys()
        .filter_map(|key| {
            let provider = key.split_once(':').map_or(key.as_str(), |(p, _)| p);
            known.get(provider).map(|id| (provider, *id))
        })
        .collect();
    providers.sort_unstable_by_key(|(provider, _)| *provider);
    providers.dedup();
    for (provider_key, provider_id) in providers {
        let provider_override = legacy.get(provider_key);
        for window in QUOTA_WARNING_WINDOWS {
            let window_override = legacy.get(&format!("{provider_key}:{window}"));
            let high = window_override
                .and_then(|value| value.high)
                .or_else(|| provider_override.and_then(|value| value.high));
            let critical = window_override
                .and_then(|value| value.critical)
                .or_else(|| provider_override.and_then(|value| value.critical));
            if high.is_none() && critical.is_none() {
                continue;
            }
            let config = provider_configs.entry(provider_id).or_default();
            let Some(target) = config.quota_warnings.window_mut(window) else {
                continue;
            };
            if target.is_global() {
                target.high = high;
                target.critical = critical;
            }
        }
    }
}

impl Settings {
    fn quota_warning_window(&self, provider: ProviderId, window: &str) -> QuotaWarningWindow {
        self.provider_configs
            .get(&provider)
            .and_then(|config| config.quota_warnings.window(window))
            .copied()
            .unwrap_or_default()
    }

    /// Effective warning thresholds (*used* %) for one provider window:
    /// the provider's Custom value, else the global threshold.
    pub fn usage_thresholds(&self, provider: ProviderId, window: &str) -> UsageThresholds {
        let custom = self.quota_warning_window(provider, window);
        UsageThresholds {
            high: custom.high.unwrap_or(self.high_usage_threshold),
            critical: custom.critical.unwrap_or(self.critical_usage_threshold),
        }
    }

    /// False when the provider window's quota warnings are set to Off. Off
    /// suppresses threshold notifications and bar markers. Threshold hooks
    /// still fire at the window's thresholds, as on macOS, where quota hooks
    /// are independent of the notification preferences.
    pub fn quota_warning_enabled(&self, provider: ProviderId, window: &str) -> bool {
        self.quota_warning_window(provider, window)
            .enabled
            .unwrap_or(true)
    }

    pub fn quota_warnings(&self, provider: ProviderId) -> ProviderQuotaWarnings {
        self.provider_configs
            .get(&provider)
            .map(|config| config.quota_warnings)
            .unwrap_or_default()
    }

    pub fn set_quota_warnings(&mut self, provider: ProviderId, value: ProviderQuotaWarnings) {
        self.provider_config_mut(provider).quota_warnings = value.normalized();
    }
}

/// UI language for the application
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    /// English (default)
    #[default]
    English,
    /// Chinese (Simplified)
    Chinese,
    /// Chinese (Traditional, Taiwan)
    ChineseTraditional,
    /// Japanese
    Japanese,
    /// Korean
    Korean,
    /// Spanish (Mexican)
    Spanish,
    /// Portuguese (Brazil)
    PortugueseBrazil,
    /// Russian
    Russian,
    /// Turkish
    Turkish,
    /// Ukrainian
    Ukrainian,
}

impl Language {
    /// Get the display name for this language
    pub fn display_name(&self) -> &'static str {
        match self {
            Language::English => "English",
            Language::Chinese => "中文",
            Language::ChineseTraditional => "繁體中文",
            Language::Japanese => "日本語",
            Language::Korean => "한국어",
            Language::Spanish => "Español",
            Language::PortugueseBrazil => "Português (Brasil)",
            Language::Russian => "Русский",
            Language::Turkish => "Türkçe",
            Language::Ukrainian => "Українська",
        }
    }

    /// Get all available languages
    pub fn all() -> &'static [Language] {
        &[
            Language::English,
            Language::Chinese,
            Language::ChineseTraditional,
            Language::Japanese,
            Language::Korean,
            Language::Spanish,
            Language::PortugueseBrazil,
            Language::Russian,
            Language::Turkish,
            Language::Ukrainian,
        ]
    }

    /// Stable label used in bridge JSON and persisted settings
    /// (e.g. "english", "spanish").
    pub fn label(&self) -> &'static str {
        match self {
            Language::English => "english",
            Language::Chinese => "chinese",
            Language::ChineseTraditional => "chinesetraditional",
            Language::Japanese => "japanese",
            Language::Korean => "korean",
            Language::Spanish => "spanish",
            Language::PortugueseBrazil => "portuguesebrazil",
            Language::Russian => "russian",
            Language::Turkish => "turkish",
            Language::Ukrainian => "ukrainian",
        }
    }

    /// Accepted input aliases — short codes and native names (all lowercase).
    /// Used by resolve() for flexible language parsing.
    pub fn accepted_aliases(&self) -> &'static [&'static str] {
        match self {
            Language::English => &["en", "en-us"],
            Language::Chinese => &["zh", "zh-cn", "zh-hans", "中文"],
            Language::ChineseTraditional => &["zh-tw", "zh-hant", "zh-hant-tw", "繁體中文"],
            Language::Japanese => &["ja", "ja-jp", "日本語"],
            Language::Korean => &["ko", "ko-kr", "한국어"],
            Language::Spanish => &["es", "es-mx", "español"],
            Language::PortugueseBrazil => &[
                "pt",
                "pt-br",
                "portuguese",
                "português",
                "portugues",
                "português (brasil)",
            ],
            Language::Russian => &["ru", "ru-ru", "русский"],
            Language::Turkish => &["tr", "tr-tr", "türkçe", "turkce"],
            Language::Ukrainian => &["uk", "uk-ua", "українська"],
        }
    }

    /// Resolve a language from any recognized input string.
    /// Matches against label() and all accepted_aliases().
    /// Case-insensitive via Unicode-aware to_lowercase().
    pub fn resolve(raw: &str) -> Option<Language> {
        let normalized = raw.trim().to_lowercase();
        for lang in Self::all() {
            if normalized == lang.label() {
                return Some(*lang);
            }
            for alias in lang.accepted_aliases() {
                if normalized == *alias {
                    return Some(*lang);
                }
            }
        }
        None
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
    /// One tray icon with the selected metrics for two providers stacked vertically
    Stacked,
}

impl TrayIconMode {
    /// Get the display name for this mode
    pub fn display_name(&self) -> &'static str {
        match self {
            TrayIconMode::Single => "Single Icon",
            TrayIconMode::PerProvider => "Per Provider",
            TrayIconMode::Stacked => "Stacked Providers",
        }
    }

    /// Get a description for this mode
    pub fn description(&self) -> &'static str {
        match self {
            TrayIconMode::Single => "Show one tray icon for all providers",
            TrayIconMode::PerProvider => "Show a separate tray icon for each enabled provider",
            TrayIconMode::Stacked => "Show two providers as stacked usage meters",
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
    /// The provider's monthly plan allowance window, such as Mistral's Vibe
    /// plan (upstream 0.70.0 #4072).
    MonthlyPlan,
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
            MetricPreference::MonthlyPlan,
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
            MetricPreference::MonthlyPlan => "Monthly plan",
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
            MetricPreference::MonthlyPlan => "Monthly plan allowance",
            MetricPreference::Average => "Average across metrics",
        }
    }
}

/// How cost is rendered on provider MenuCards (#2976).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum CostSummaryDisplayStyle {
    #[default]
    Compact,
    Detailed,
    Hidden,
}

impl CostSummaryDisplayStyle {
    pub fn all() -> &'static [CostSummaryDisplayStyle] {
        &[
            CostSummaryDisplayStyle::Compact,
            CostSummaryDisplayStyle::Detailed,
            CostSummaryDisplayStyle::Hidden,
        ]
    }
}

/// Per-provider configuration values.
///
/// All fields are optional / falsy-default so unused providers serialize as
/// empty objects (or skip serialization entirely). Defaults are applied via
/// the accessor methods on [`Settings`] (e.g. cookie source defaults to
/// `"auto"`, region defaults are provider-specific).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
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
    /// Optional secondary management credential for providers that expose one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub management_api_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    /// Wayfinder gateway URL override.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gateway_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ide_base_path: Option<String>,
    /// Codex-only: opt out of OpenAI web "extras" surfaces.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub openai_web_extras: Option<bool>,
    /// Codex-only: show or hide Codex Spark quota rows in presentation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spark_usage_visible: Option<bool>,
    /// Presentation-only usage rows hidden by the user. `None` preserves the
    /// legacy visibility flags until the new visibility controls are used.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hidden_usage_item_ids: Option<Vec<String>>,
    /// Codex-only: enable historical usage tracking in UI.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub historical_tracking: bool,
    /// Codex-only: keep a local session cost ledger (Mac Options parity;
    /// persisted only, no Windows consumer yet).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub local_session_cost_ledger_enabled: bool,
    /// Codex-only: OpenAI web battery saver, shown while web extras is on
    /// (Mac Options parity; persisted only, no Windows consumer yet).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub openai_web_battery_saver_enabled: bool,
    /// Per-window quota warning settings (Global, Custom or Off).
    #[serde(default, skip_serializing_if = "ProviderQuotaWarnings::is_empty")]
    pub quota_warnings: ProviderQuotaWarnings,
    /// Claude-only: avoid keychain prompts when reading credentials.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub avoid_keychain_prompts: bool,
    /// Claude-only: opt in to reading external accounts from the claude-swap
    /// (`cswap`) executable. Disabled by default; CodexBar never reads or stores
    /// claude-swap credentials (issue #477).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub claude_swap_enabled: bool,
    /// Claude-only: path to the claude-swap (`cswap`) executable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claude_swap_executable_path: Option<String>,
    /// Codex/Claude-only: reopen the captured CLI session after its quota resets.
    /// Disabled by default; the desktop shell never injects a prompt.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub auto_resume_after_quota_reset: bool,
    /// LiteLLM/Claude-only: fetch one optional extra breakdown (LiteLLM model
    /// activity, Claude workspace spend). Disabled by default; each provider
    /// has exactly one such breakdown.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub optional_details_enabled: bool,
    /// Per-provider accent color override (hex, e.g. "#FF5733"). `None`
    /// means the shipped brand color is used (#2972).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accent_color: Option<String>,
    /// Optional user-entered Copilot seat AI-credit allowance. GitHub does not
    /// publish this denominator; it is only used when a valid value is set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seat_credit_entitlement: Option<f64>,
    /// OpenAI API-only: `Some(false)` opts out of the per-refresh Tokens/min fetch.
    /// `None` keeps upstream's default of on.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub live_usage_enabled: Option<bool>,
    /// OpenAI API-only: manual per-model organization rate limits for Live Usage
    /// headroom. Validated on load; `None` when no valid row is stored.
    #[serde(
        default,
        deserialize_with = "super::model_rate_limits::deserialize_model_rate_limits",
        skip_serializing_if = "Option::is_none"
    )]
    pub model_rate_limits: Option<Vec<super::ModelRateLimit>>,
}

/// How the single tray icon is drawn (Mac "Menu bar style").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MenuBarIconStyle {
    /// Two-row usage meter.
    #[default]
    Bars,
    /// Provider icon plus the percent text.
    Percent,
    /// Session and weekly meters drawn as two equal-height rows (upstream
    /// "Progress bars stacked", #4436). Older builds read it as `Bars`.
    #[serde(rename = "stackedBars")]
    StackedBars,
}

impl MenuBarIconStyle {
    /// Lenient parse; unknown values return `None` so callers pick a fallback.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "bars" => Some(Self::Bars),
            "percent" => Some(Self::Percent),
            "stackedbars" => Some(Self::StackedBars),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Bars => "bars",
            Self::Percent => "percent",
            Self::StackedBars => "stackedBars",
        }
    }
}

/// How workday boundaries appear on weekly usage bars.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum WorkdayTickStyle {
    Hidden,
    #[default]
    Subtle,
    HighContrast,
}

impl WorkdayTickStyle {
    /// Lenient parse; accepts `high_contrast` and `highContrast`.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().replace('-', "_").as_str() {
            "hidden" => Some(Self::Hidden),
            "subtle" => Some(Self::Subtle),
            "high_contrast" | "highcontrast" => Some(Self::HighContrast),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Hidden => "hidden",
            Self::Subtle => "subtle",
            Self::HighContrast => "high_contrast",
        }
    }
}

/// Most providers the merged Overview can show (Mac `overviewProviderLimit`).
pub const MERGED_OVERVIEW_PROVIDER_LIMIT: usize = 6;

/// Highest accepted `weekly_progress_work_days` value.
pub const WEEKLY_PROGRESS_WORK_DAYS_MAX: u8 = 7;

/// Normalise a work-week length: `Some(2..=7)` passes, anything else is
/// `None` (Automatic, wall-clock pacing).
pub fn normalize_weekly_progress_work_days(value: Option<u8>) -> Option<u8> {
    value.filter(|days| (2..=WEEKLY_PROGRESS_WORK_DAYS_MAX).contains(days))
}
