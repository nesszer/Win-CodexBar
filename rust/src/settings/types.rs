use super::*;

/// UI language for the application
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    /// English (default)
    #[default]
    English,
    /// Chinese (Simplified)
    Chinese,
    /// Japanese
    Japanese,
    /// Spanish (Mexican)
    Spanish,
}

impl Language {
    /// Get the display name for this language
    pub fn display_name(&self) -> &'static str {
        match self {
            Language::English => "English",
            Language::Chinese => "中文",
            Language::Japanese => "日本語",
            Language::Spanish => "Español",
        }
    }

    /// Get all available languages
    pub fn all() -> &'static [Language] {
        &[
            Language::English,
            Language::Chinese,
            Language::Japanese,
            Language::Spanish,
        ]
    }

    /// Stable label used in bridge JSON and persisted settings
    /// (e.g. "english", "spanish").
    pub fn label(&self) -> &'static str {
        match self {
            Language::English => "english",
            Language::Chinese => "chinese",
            Language::Japanese => "japanese",
            Language::Spanish => "spanish",
        }
    }

    /// Accepted input aliases — short codes and native names (all lowercase).
    /// Used by resolve() for flexible language parsing.
    pub fn accepted_aliases(&self) -> &'static [&'static str] {
        match self {
            Language::English => &["en", "en-us"],
            Language::Chinese => &["zh", "zh-cn", "zh-hans", "中文"],
            Language::Japanese => &["ja", "ja-jp", "日本語"],
            Language::Spanish => &["es", "es-mx", "español"],
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
