//! Locale module for UI internationalization
//!
//! Provides localized strings for the application UI surfaces.
//! The locale is determined by the user's language setting in Settings.

use crate::settings::Language;
use crate::settings::Settings;

/// Get the localized string for a given key in the specified language
pub fn get_text(lang: Language, key: LocaleKey) -> &'static str {
    match lang {
        Language::English => key.english(),
        Language::Chinese => key.chinese(),
    }
}

/// Get the current UI language from settings
pub fn current_language() -> Language {
    Settings::load().ui_language
}

/// Get the localized tooltip for single-tray usage display
/// Format: "Provider: Session X% | Weekly Y%"
pub fn tray_tooltip(provider_name: &str, session_percent: f64, weekly_percent: f64) -> String {
    let lang = current_language();
    let session_label = get_text(lang, LocaleKey::TraySessionPercent);
    let weekly_label = get_text(lang, LocaleKey::TrayWeeklyPercent);
    format!(
        "{}: {} | {}",
        provider_name,
        session_label.replace("{}", &format!("{}", session_percent as i32)),
        weekly_label.replace("{}", &format!("{}", weekly_percent as i32))
    )
}

/// Get the localized tooltip for single-tray usage display with status overlay
/// Format: "Provider: Session X% | Weekly Y% (Status)"
pub fn tray_tooltip_with_status(
    provider_name: &str,
    session_percent: f64,
    weekly_percent: f64,
    status: Option<IconOverlayStatus>,
) -> String {
    let lang = current_language();
    let session_label = get_text(lang, LocaleKey::TraySessionPercent);
    let weekly_label = get_text(lang, LocaleKey::TrayWeeklyPercent);
    let status_suffix = match status {
        None => "",
        Some(IconOverlayStatus::Error) => get_text(lang, LocaleKey::TrayStatusError),
        Some(IconOverlayStatus::Stale) => get_text(lang, LocaleKey::TrayStatusStale),
        Some(IconOverlayStatus::Incident) => get_text(lang, LocaleKey::TrayStatusIncident),
        Some(IconOverlayStatus::Partial) => get_text(lang, LocaleKey::TrayStatusPartial),
    };
    format!(
        "{}: {} | {}{}",
        provider_name,
        session_label.replace("{}", &format!("{}", session_percent as i32)),
        weekly_label.replace("{}", &format!("{}", weekly_percent as i32)),
        status_suffix
    )
}

/// Status overlay types for tray tooltips
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconOverlayStatus {
    Error,
    Stale,
    Incident,
    Partial,
}

/// Get the localized tooltip for credits mode
/// Format: "Provider: Weekly quota exhausted | Credits remaining X%"
pub fn tray_tooltip_credits(provider_name: &str, credits_percent: f64) -> String {
    let lang = current_language();
    let exhausted = get_text(lang, LocaleKey::TrayWeeklyExhausted);
    let credits = get_text(lang, LocaleKey::TrayCreditsRemaining);
    format!(
        "{}: {} | {}",
        provider_name,
        exhausted,
        credits.replace("{}", &format!("{:.0}", credits_percent))
    )
}

/// Locale keys for app-owned UI strings
#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
pub enum LocaleKey {
    // Tab names (Preferences)
    TabGeneral,
    TabProviders,
    TabDisplay,
    TabApiKeys,
    TabCookies,
    TabAdvanced,
    TabAbout,
    TabShortcuts,

    // General settings (Preferences)
    InterfaceLanguage,
    StartupSettings,
    StartAtLogin,
    StartMinimized,
    StartAtLoginHelper,
    StartMinimizedHelper,

    // Notification settings (Preferences)
    ShowNotificationsHelper,
    SoundEnabledHelper,
    HighUsageThresholdHelper,
    CriticalUsageThresholdHelper,

    // Notification settings (Preferences)
    ShowNotifications,
    SoundEnabled,
    SoundVolume,
    HighUsageThreshold,
    HighUsageAlert,
    CriticalUsageThreshold,
    CriticalUsageAlert,

    // Display settings (Preferences)
    UsageDisplay,
    ShowUsageAsUsed,
    ShowUsageAsUsedHelper,
    ResetTimeRelative,
    ResetTimeRelativeHelper,
    ShowCreditsExtra,
    ShowCreditsExtraHelper,
    TrayIcon,
    MergeTrayIcons,
    MergeTrayIconsHelper,
    PerProviderTrayIcons,
    PerProviderTrayIconsHelper,

    // Provider settings (Preferences)
    ProviderEnabled,
    ProviderDisabled,
    ProviderInfo,
    ProviderUsage,
    AuthType,
    DataSource,
    ProviderNotDetected,
    ProviderLastFetchFailed,
    ProviderUsageNotFetchedYet,
    ProviderNotFetchedYetTitle,
    ProviderDisabledNoRecentData,
    ProviderSourceAutoShort,
    ProviderSourceWebShort,
    ProviderSourceCliShort,
    ProviderSourceOauthShort,
    ProviderSourceApiShort,
    ProviderSourceGithubApiShort,
    ProviderSourceLocalShort,
    ProviderSourceKiroEnvShort,
    TrackingItem,
    MainWindowLiveUsageData,
    StartTrackingUsage,
    ClickTrayIconForMetrics,

    // Browser cookie import (Preferences)
    BrowserCookieImport,
    ImportFromBrowser,
    NoCookiesFoundInBrowser,
    SelectBrowser,
    ImportCookies,
    ImportSuccess,
    ImportFailed,
    SaveFailed,
    CookiesAutoImport,
    QuickActions,
    OpenProviderDashboard,
    OllamaNoDashboard,

    // API Keys tab (Preferences)
    ApiKeysTitle,
    ApiKeysDescription,
    AddKey,
    KeySet,
    KeyRequired,
    Remove,
    GetKey,

    // Cookies tab (Preferences)
    SavedCookies,
    AddManualCookie,
    CookieHeader,
    PasteHere,
    DeleteCookie,
    CookieSaved,
    CookieDeleted,

    // Advanced tab (Preferences)
    RefreshSettings,
    Animations,
    MenuBar,
    Fun,
    GlobalShortcut,
    Privacy,
    Updates,
    UpdateChannel,
    UpdateChannelStable,
    UpdateChannelBeta,
    Never,
    LastUpdated,
    NeverUpdated,
    MinutesAgo,
    HoursAgo,
    DaysAgo,
    BuiltWithRust,
    OriginalMacOSVersion,
    Links,
    BuildInfo,
    EnabledProviders,
    Appearance,
    ThemeSelection,
    LightMode,
    DarkMode,

    // About (Preferences)
    AboutTitle,
    Version,

    // Main popup - Header actions
    ActionRefreshAll,
    ActionSettings,
    ActionClose,

    // Main popup - Provider section
    ProviderAccount,
    ProviderSession,
    ProviderWeekly,
    ProviderMonthly,
    ProviderModel,
    ProviderPlan,
    ProviderNextReset,
    ProviderNoRecentUsage,
    ProviderNotSignedIn,
    SummaryTab,

    // Main popup - Loading/Empty/Error states (non-happy-path)
    StateLoadingProviders,
    StateNoProviderData,
    StateNoProviderSelected,
    StateSummaryRefreshPending,
    StateError,
    StateRetry,
    StateDownload,
    StateRestartAndUpdate,

    // Main popup - Credits
    CreditsTitle,

    // Main popup - Update banner (non-happy-path)
    UpdateRestartAndUpdate,
    UpdateRetry,
    UpdateDownload,
    UpdateDownloading,
    UpdateReady,
    UpdateFailed,

    // Main popup - Settings button
    ButtonOpenProviderSettings,

    // Main popup - Bottom menu (Actions)
    MenuSettings,
    MenuAbout,
    MenuQuit,

    // Main popup - Status strings
    StatusJustUpdated,
    StatusUnableToGetUsage,

    // Main popup - Provider detail actions
    ActionRefresh,
    ActionSwitchAccount,
    ActionUsageDashboard,
    ActionStatusPage,
    ActionCopyError,
    ActionBuyCredits,

    // Main popup - Pace status
    PaceOnTrack,
    PaceBehind,

    // Main popup - Reset prefix
    MetricResetsIn,

    // Main popup - Section titles
    SectionUsageBreakdown,
    SectionCost,

    // Main popup - Usage/reset labels
    ResetInProgress,
    TomorrowAt,
    UsedPercent,
    RemainingPercent,
    RemainingAmount,
    Tokens1K,
    TodayCost,
    Last30DaysCost,
    StatusLabel,

    // Tray - Single icon mode
    TrayOpenCodexBar,
    TrayPopOutDashboard,
    TrayRefreshAll,
    TrayProviders,
    TraySettings,
    TrayCheckForUpdates,
    TrayQuit,
    TrayLoading,
    TrayNoProviders,
    TraySessionPercent,
    TrayWeeklyPercent,
    TrayStatusError,
    TrayStatusStale,
    TrayStatusIncident,
    TrayStatusPartial,
    TrayWeeklyExhausted,
    TrayCreditsRemaining,
    TrayStatusRowLoading,
    TrayStatusRowError,
    TrayCreditsRow,

    // Tray - Per-provider mode
    TrayProviderPopOut,
    TrayProviderRefresh,
    TrayProviderSettings,
    TrayProviderQuit,

    // Provider settings - Live renderer specific
    State,
    Source,
    Updated,
    UpdatedJustNow,
    UpdatedMinutesAgo,
    UpdatedHoursAgo,
    UpdatedDaysAgo,
    Status,
    AllSystemsOperational,
    Plan,
    Account,

    // Provider detail - Usage section
    ProviderSessionLabel,
    ProviderWeeklyLabel,
    ProviderCodeReviewLabel,
    ResetsInShort,
    ResetsInDaysHours,
    ResetsInHoursMinutes,

    // Provider detail - Tray Display
    TrayDisplayTitle,
    ShowInTray,

    // Provider detail - Credits
    CreditsLabel,
    CreditsLeft,

    // Provider detail - Cost
    CostTitle,
    TodayCostFull,
    Last30DaysCostFull,

    // Provider detail - Settings section
    ProviderSettingsTitle,
    ProviderAccountsTitle,
    ProviderOptionsTitle,
    MenuBarMetric,
    MenuBarMetricHelper,
    UsageSource,
    ProviderNoCodexAccountsDetected,
    ProviderCodexAutoImportHelp,
    ProviderCodexHistoryHelp,
    ProviderOpenAiCookies,
    ProviderHistoricalTracking,
    ProviderOpenAiWebExtras,
    ProviderOpenAiWebExtrasHelp,
    ProviderCodexCreditsUnavailable,
    ProviderCodexLastFetchFailedTitle,
    ProviderCodexNotRunningHelp,
    ProviderCookieSource,
    CookieSourceManual,
    ProviderRegion,
    ProviderClaudeCookies,
    ProviderClaudeCookiesHelp,
    ProviderClaudeAvoidKeychainPrompts,
    ProviderClaudeAvoidKeychainPromptsHelp,
    ProviderCursorCookieSourceHelp,
    ProviderCursorCreditsHelp,
    AutoFallbackHelp,
    ProviderSourceOauthWeb,
    Automatic,
    Average,
    ExtraUsage,
    OAuth,
    Api,
    Web,

    // General tab sections
    PrivacyTitle,
    HidePersonalInfo,
    HidePersonalInfoHelper,
    UpdatesTitle,
    UpdateChannelChoice,
    UpdateChannelChoiceHelper,
    AutoDownloadUpdates,
    AutoDownloadUpdatesHelper,
    InstallUpdatesOnQuit,
    InstallUpdatesOnQuitHelper,

    // Keyboard shortcuts
    KeyboardShortcutsTitle,
    GlobalShortcutLabel,
    GlobalShortcutHelper,
    ShortcutFormatHint,
    Saved,
    InvalidFormat,
    ShortcutHintPlaceholder,

    // Display/Preferences helpers
    SurpriseAnimationsHelper,
    SelectProvider,

    // Refresh interval labels
    RefreshInterval30Sec,
    RefreshInterval1Min,
    RefreshInterval5Min,
    RefreshInterval10Min,

    // Cookies tab
    BrowserCookiesTitle,
    CookieImport,
    Provider,
    SelectPlaceholder,
    AutoRefreshInterval,

    // About tab - render_about_tab
    AboutDescription,
    AboutDescriptionLine2,
    ViewOnGitHub,
    SubmitIssue,
    MaintainedBy,
    CommitLabel,
    BuildDateLabel,

    // Shared form controls
    Save,
    Cancel,
    Label,
    Token,
    AddAccount,
    AccountAdded,
    AccountRemoved,
    AccountSwitched,
    AccountLabelHint,
    EnterApiKeyFor,
    PasteApiKeyHere,
    ApiKeySaved,
    ApiKeyRemoved,
    EnvironmentVariable,
    CookieSavedForProvider,
    CookieRemovedForProvider,

    // Usage helper functions
    ShowUsedPercent,
    ShowRemainingPercent,

    // Main popup - Update banner messages (non-happy-path)
    UpdateAvailableMessage,
    UpdateReadyMessage,
    UpdateFailedMessage,
    UpdateDownloadingMessage,

    // Tauri desktop shell — Settings section headings
    TabTokenAccounts,
    SectionRefresh,
    SectionNotifications,
    SectionUsageThresholds,
    SectionKeyboard,
    SectionUsageRendering,
    SectionTime,
    SectionLanguage,
    SectionCredentialsSecurity,
    SectionDebug,
    SectionApiKeys,
    SectionSavedCookies,
    SectionImportFromBrowser,
    SectionAddCookieManually,
    SectionTokenAccounts,
    SectionSavedAccounts,
    SectionAddAccount,

    // Tauri desktop shell — General tab fields
    RefreshIntervalLabel,
    RefreshIntervalHelper,
    SoundVolumeHelper,
    HighUsageWarningHelper,
    CriticalUsageWarningHelper,
    GlobalShortcutFieldLabel,
    GlobalShortcutToggleHelper,
    ShortcutRecordButton,
    ShortcutRecordingLabel,
    ShortcutRecordingHint,
    ShortcutClearButton,
    ShortcutEmptyPlaceholder,
    NotificationTestSound,
    NotificationTestSoundPlaying,

    // Tauri desktop shell — Display tab fields
    TrayIconModeLabel,
    TrayIconModeHelper,
    TrayIconModeSingle,
    TrayIconModePerProvider,
    ShowProviderIcons,
    ShowProviderIconsHelper,
    PreferHighestUsage,
    PreferHighestUsageHelper,
    ShowPercentInTray,
    ShowPercentInTrayHelper,
    DisplayModeLabel,
    DisplayModeHelper,
    DisplayModeDetailed,
    DisplayModeCompact,
    DisplayModeMinimal,
    ShowAsUsedLabel,
    ShowAsUsedHelper,
    ShowAllTokenAccountsLabel,
    ShowAllTokenAccountsHelper,
    EnableAnimationsLabel,
    EnableAnimationsHelper,
    SurpriseAnimationsLabel,

    // Tauri desktop shell — Advanced tab fields
    UpdateChannelStableOption,
    UpdateChannelBetaOption,
    AvoidKeychainPromptsLabel,
    AvoidKeychainPromptsHelper,
    DisableAllKeychainLabel,
    DisableAllKeychainHelper,
    ShowDebugSettingsLabel,
    ShowDebugSettingsHelper,
    LanguageEnglishOption,
    LanguageChineseOption,

    // Tauri desktop shell — Theme (Phase 12)
    SectionTheme,
    ThemeLabel,
    ThemeHelper,
    ThemeAutoOption,
    ThemeLightOption,
    ThemeDarkOption,

    // Tauri desktop shell — settings status / common
    SettingsStatusSaving,
    ApiKeysTabHint,

    // Tauri desktop shell — tray / popout
    FetchingProviderData,
    NoProvidersConfigured,
    EnableProvidersHint,
    OpenSettingsButton,
    TooltipRefresh,
    TooltipSettings,
    TooltipPopOut,
    TooltipBackToTray,
    TrayCardErrorBadge,
    SummaryProvidersLabel,
    SummaryRefreshing,
    SummaryFailed,
    SummaryWithErrors,

    // Tauri desktop shell — provider detail
    DetailBackButton,
    DetailWindowPrimary,
    DetailWindowSecondary,
    DetailWindowModelSpecific,
    DetailWindowTertiary,
    DetailWindowMinutesSuffix,
    DetailWindowExhausted,
    DetailPaceTitle,
    DetailPaceOnTrack,
    DetailPaceSlightlyAhead,
    DetailPaceAhead,
    DetailPaceFarAhead,
    DetailPaceSlightlyBehind,
    DetailPaceBehind,
    DetailPaceFarBehind,
    DetailPaceRunsOutIn,
    DetailPaceWillLastToReset,
    DetailCostTitle,
    DetailCostUsed,
    DetailCostLimit,
    DetailCostRemaining,
    DetailCostResets,
    DetailChartCost,
    DetailChartCredits,
    DetailChartUsageBreakdown,
    DetailChartEmpty,
    DetailUpdatedPrefix,

    // Tauri desktop shell — update banner
    BannerCheckingForUpdates,
    BannerUpdateAvailablePrefix,
    BannerDownloadButton,
    BannerViewRelease,
    BannerDismiss,
    BannerDownloadingPrefix,
    BannerReadyToInstallSuffix,
    BannerInstallRestart,
    BannerUpdateFailedPrefix,
    BannerRetry,

    // Tauri desktop shell — providers sidebar (Phase 6a)
    ProviderSidebarReorderHint,
    ProviderStatusOk,
    ProviderStatusStale,
    ProviderStatusError,
    ProviderStatusLoading,
    ProviderStatusDisabled,
    ProviderDetailPlaceholder,

    // Tauri desktop shell — Phase 6d credential detection UIs
    CredentialsSectionTitle,
    CredsStatusAuthenticated,
    CredsStatusNotSignedIn,
    CredsStatusDetected,
    CredsStatusNotDetected,
    CredsStatusAvailable,
    CredsStatusUnavailable,
    CredsOpenFolderAction,
    CredsRefreshDetectionAction,
    CredsSavePathAction,
    CredsBrowseAction,
    CredsGeminiCliLabel,
    CredsGeminiCliHelperPrefix,
    CredsGeminiCliSetupAction,
    CredsGeminiCliSetupHelp,
    CredsVertexAiLabel,
    CredsVertexAiHelperPrefix,
    CredsVertexAiSetupAction,
    CredsVertexAiSetupHelp,
    CredsJetBrainsLabel,
    CredsJetBrainsHelperDetectedPrefix,
    CredsJetBrainsHelperCustomPrefix,
    CredsJetBrainsHelperMissing,
    CredsJetBrainsCustomPathLabel,
    CredsJetBrainsCustomPathPlaceholder,
    CredsJetBrainsSelectLabel,
    CredsJetBrainsAutoDetectOption,
    CredsKiroLabel,
    CredsKiroHelperAvailablePrefix,
    CredsKiroHelperMissing,
    CredsOpenAiHistoryHelp,

    // Tauri desktop shell — Token accounts (Phase 6e, review)
    TokenAccountActive,
    TokenAccountSetActive,
    TokenAccountRemove,
    TokenAccountAddButton,
    TokenAccountGithubLoginButton,
    TokenAccountEmpty,
    TokenAccountLabelPlaceholder,
    TokenAccountProviderLabel,
    TokenAccountProviderPlaceholder,
    TokenAccountAddedPrefix,
    TokenAccountUsedPrefix,
    TokenAccountTabHint,
    TokenAccountNoSupported,
    TokenAccountInlineSummary,

    // Phase 9 - Tray / pop-out pace badges + countdowns
    TrayPaceBadgeSlow,
    TrayPaceBadgeSteady,
    TrayPaceBadgeRacing,
    TrayPaceBadgeBurning,
    TrayResetsInLabel,
    TrayResetsDueNow,
}

impl LocaleKey {
    fn english(self) -> &'static str {
        match self {
            // Tab names
            LocaleKey::TabGeneral => "General",
            LocaleKey::TabProviders => "Providers",
            LocaleKey::TabDisplay => "Display",
            LocaleKey::TabApiKeys => "API Keys",
            LocaleKey::TabCookies => "Cookies",
            LocaleKey::TabAdvanced => "Advanced",
            LocaleKey::TabAbout => "About",
            LocaleKey::TabShortcuts => "Shortcuts",

            // General settings
            LocaleKey::InterfaceLanguage => "Interface Language",
            LocaleKey::StartupSettings => "System",
            LocaleKey::StartAtLogin => "Start at Login",
            LocaleKey::StartMinimized => "Start Minimized",
            LocaleKey::StartAtLoginHelper => "Login automatically after system startup",
            LocaleKey::StartMinimizedHelper => "Start minimized to system tray",

            // Notification settings
            LocaleKey::ShowNotifications => "Show Notifications",
            LocaleKey::ShowNotificationsHelper => "Alert when usage thresholds are reached",
            LocaleKey::SoundEnabled => "Sound Alerts",
            LocaleKey::SoundEnabledHelper => "Play sound when thresholds are reached",
            LocaleKey::SoundVolume => "Alert Volume",
            LocaleKey::HighUsageThreshold => "High Usage Threshold",
            LocaleKey::HighUsageThresholdHelper => "Show warning at this usage level",
            LocaleKey::HighUsageAlert => "High Usage Alert",
            LocaleKey::CriticalUsageThreshold => "Critical Usage Threshold",
            LocaleKey::CriticalUsageThresholdHelper => "Show critical alert at this level",
            LocaleKey::CriticalUsageAlert => "Critical Alert",

            // Display settings
            LocaleKey::UsageDisplay => "Usage Display",
            LocaleKey::ShowUsageAsUsed => "Show Usage as Used",
            LocaleKey::ShowUsageAsUsedHelper => "Display as used percentage instead of remaining",
            LocaleKey::ResetTimeRelative => "Relative Reset Time",
            LocaleKey::ResetTimeRelativeHelper => "Show \"2h 30m\" instead of \"3:00 PM\"",
            LocaleKey::ShowCreditsExtra => "Show Credits & Extra Usage",
            LocaleKey::ShowCreditsExtraHelper => "Display credit balance and extra usage info",
            LocaleKey::TrayIcon => "Tray Icon",
            LocaleKey::MergeTrayIcons => "Merge Tray Icons",
            LocaleKey::MergeTrayIconsHelper => "Show all providers in a single tray icon",
            LocaleKey::PerProviderTrayIcons => "Per-Provider Icons",
            LocaleKey::PerProviderTrayIconsHelper => {
                "Show separate tray icon for each enabled provider"
            }

            // Provider settings
            LocaleKey::ProviderEnabled => "Enabled",
            LocaleKey::ProviderDisabled => "Disabled",
            LocaleKey::ProviderInfo => "Info",
            LocaleKey::ProviderUsage => "Usage",
            LocaleKey::AuthType => "Authentication",
            LocaleKey::DataSource => "Data Source",
            LocaleKey::ProviderNotDetected => "not detected",
            LocaleKey::ProviderLastFetchFailed => "last fetch failed",
            LocaleKey::ProviderUsageNotFetchedYet => "usage not fetched yet",
            LocaleKey::ProviderNotFetchedYetTitle => "Not fetched yet",
            LocaleKey::ProviderDisabledNoRecentData => "Disabled — no recent data",
            LocaleKey::ProviderSourceAutoShort => "auto",
            LocaleKey::ProviderSourceWebShort => "web",
            LocaleKey::ProviderSourceCliShort => "cli",
            LocaleKey::ProviderSourceOauthShort => "oauth",
            LocaleKey::ProviderSourceApiShort => "api",
            LocaleKey::ProviderSourceGithubApiShort => "github api",
            LocaleKey::ProviderSourceLocalShort => "local",
            LocaleKey::ProviderSourceKiroEnvShort => "kiro env",
            LocaleKey::TrackingItem => "Tracked Item",
            LocaleKey::MainWindowLiveUsageData => "Live usage data in main window",
            LocaleKey::StartTrackingUsage => "Enable to start tracking usage",
            LocaleKey::ClickTrayIconForMetrics => "Click tray icon for live metrics",

            // Browser cookie import
            LocaleKey::BrowserCookieImport => "Browser Cookie Import",
            LocaleKey::ImportFromBrowser => "Import {} cookies from browser",
            LocaleKey::NoCookiesFoundInBrowser => "No cookies found in {}. Please log in first.",
            LocaleKey::SelectBrowser => "Select browser...",
            LocaleKey::ImportCookies => "Import Cookies",
            LocaleKey::ImportSuccess => "Imported cookies for {}",
            LocaleKey::ImportFailed => "Import failed: {}",
            LocaleKey::SaveFailed => "Save failed: {}",
            LocaleKey::CookiesAutoImport => {
                "Cookies are automatically imported from Chrome, Edge, Brave and Firefox"
            }
            LocaleKey::QuickActions => "Quick Actions",
            LocaleKey::OpenProviderDashboard => "Open {} Dashboard",
            LocaleKey::OllamaNoDashboard => "Ollama runs locally, no dashboard",

            // API Keys tab
            LocaleKey::ApiKeysTitle => "API Keys",
            LocaleKey::ApiKeysDescription => {
                "Configure access tokens for providers that require authentication."
            }
            LocaleKey::AddKey => "+ Add Key",
            LocaleKey::KeySet => "Set",
            LocaleKey::KeyRequired => "Key Required",
            LocaleKey::Remove => "Remove",
            LocaleKey::GetKey => "Get key →",

            // Cookies tab
            LocaleKey::SavedCookies => "Saved Cookies",
            LocaleKey::AddManualCookie => "Add Manual Cookie",
            LocaleKey::CookieHeader => "Cookie Header",
            LocaleKey::PasteHere => "Paste here...",
            LocaleKey::DeleteCookie => "Delete",
            LocaleKey::CookieSaved => "Saved {} cookies",
            LocaleKey::CookieDeleted => "Deleted cookies for {}",

            // Advanced tab
            LocaleKey::RefreshSettings => "Refresh",
            LocaleKey::Animations => "Animations",
            LocaleKey::MenuBar => "Menu Bar",
            LocaleKey::Fun => "Fun",
            LocaleKey::GlobalShortcut => "Global Shortcut",
            LocaleKey::Privacy => "Privacy",
            LocaleKey::Updates => "Updates",
            LocaleKey::UpdateChannel => "Update Channel",
            LocaleKey::UpdateChannelStable => "Stable",
            LocaleKey::UpdateChannelBeta => "Beta",
            LocaleKey::Never => "Never",
            LocaleKey::LastUpdated => "Updated",
            LocaleKey::MinutesAgo => "{} minutes ago",
            LocaleKey::HoursAgo => "{} hours ago",
            LocaleKey::DaysAgo => "{} days ago",
            LocaleKey::BuiltWithRust => "Built with Rust + egui",
            LocaleKey::OriginalMacOSVersion => "Original macOS version",
            LocaleKey::Links => "Links",
            LocaleKey::BuildInfo => "Build Info",
            LocaleKey::EnabledProviders => "Enabled Providers",
            LocaleKey::Appearance => "Appearance",
            LocaleKey::ThemeSelection => "Theme",
            LocaleKey::LightMode => "Light",
            LocaleKey::DarkMode => "Dark",

            // About
            LocaleKey::AboutTitle => "About CodexBar",
            LocaleKey::Version => "Version",

            // Main popup - Header actions
            LocaleKey::ActionRefreshAll => "Refresh All",
            LocaleKey::ActionSettings => "Settings",
            LocaleKey::ActionClose => "✕",

            // Main popup - Provider section
            LocaleKey::ProviderAccount => "Account",
            LocaleKey::ProviderSession => "Session",
            LocaleKey::ProviderWeekly => "Weekly",
            LocaleKey::ProviderMonthly => "30-Day",
            LocaleKey::ProviderModel => "Model",
            LocaleKey::ProviderPlan => "Plan",
            LocaleKey::ProviderNextReset => "Next Reset",
            LocaleKey::ProviderNoRecentUsage => "No recent usage",
            LocaleKey::ProviderNotSignedIn => "Not signed in",
            LocaleKey::SummaryTab => "Summary",

            // Main popup - Loading/Empty/Error states
            LocaleKey::StateLoadingProviders => "Loading providers...",
            LocaleKey::StateNoProviderData => "No provider data.",
            LocaleKey::StateNoProviderSelected => "No provider selected.",
            LocaleKey::StateSummaryRefreshPending => "Updating after all provider refreshes finish",
            LocaleKey::StateError => "Error",
            LocaleKey::StateRetry => "Retry",
            LocaleKey::StateDownload => "Download",
            LocaleKey::StateRestartAndUpdate => "Restart & Update",

            // Main popup - Credits
            LocaleKey::CreditsTitle => "Credits",

            // Main popup - Update banner (non-happy-path)
            LocaleKey::UpdateRestartAndUpdate => "Restart & Update",
            LocaleKey::UpdateRetry => "Retry",
            LocaleKey::UpdateDownload => "Download",
            LocaleKey::UpdateDownloading => "Downloading",
            LocaleKey::UpdateReady => "Ready to install",
            LocaleKey::UpdateFailed => "Update failed",

            // Main popup - Settings button
            LocaleKey::ButtonOpenProviderSettings => "Open provider settings",

            // Main popup - Bottom menu (Actions)
            LocaleKey::MenuSettings => "Settings...",
            LocaleKey::MenuAbout => "About CodexBar",
            LocaleKey::MenuQuit => "Quit",

            // Main popup - Status strings
            LocaleKey::StatusJustUpdated => "Just updated",
            LocaleKey::StatusUnableToGetUsage => "Unable to get usage",

            // Main popup - Provider detail actions
            LocaleKey::ActionRefresh => "Refresh",
            LocaleKey::ActionSwitchAccount => "Switch account...",
            LocaleKey::ActionUsageDashboard => "Usage dashboard",
            LocaleKey::ActionStatusPage => "Status page",
            LocaleKey::ActionCopyError => "Copy error",
            LocaleKey::ActionBuyCredits => "Buy credits...",

            // Main popup - Pace status
            LocaleKey::PaceOnTrack => "On track",
            LocaleKey::PaceBehind => "Behind",

            // Main popup - Reset prefix
            LocaleKey::MetricResetsIn => "Resets in",

            // Main popup - Section titles
            LocaleKey::SectionUsageBreakdown => "Usage Breakdown",
            LocaleKey::SectionCost => "Cost",

            // Tray - Single icon mode
            LocaleKey::TrayOpenCodexBar => "Pop Out Dashboard",
            LocaleKey::TrayPopOutDashboard => "Pop Out Dashboard",
            LocaleKey::TrayRefreshAll => "Refresh All",
            LocaleKey::TrayProviders => "Providers",
            LocaleKey::TraySettings => "Settings...",
            LocaleKey::TrayCheckForUpdates => "Check for Updates",
            LocaleKey::TrayQuit => "Quit",
            LocaleKey::TrayLoading => "CodexBar - Loading...",
            LocaleKey::TrayNoProviders => "CodexBar - No providers available",
            LocaleKey::TraySessionPercent => "Session {}%",
            LocaleKey::TrayWeeklyPercent => "Weekly {}%",
            LocaleKey::TrayStatusError => " (Error)",
            LocaleKey::TrayStatusStale => " (Stale data)",
            LocaleKey::TrayStatusIncident => " (Incident)",
            LocaleKey::TrayStatusPartial => " (Partial outage)",
            LocaleKey::TrayWeeklyExhausted => "Weekly quota exhausted",
            LocaleKey::TrayCreditsRemaining => "Credits remaining {}%",
            LocaleKey::TrayStatusRowLoading => "Loading...",
            LocaleKey::TrayStatusRowError => "Error",
            LocaleKey::TrayCreditsRow => "Credits {}%",

            // Main popup - Usage/reset labels
            LocaleKey::ResetInProgress => "Resetting...",
            LocaleKey::TomorrowAt => "Tomorrow at {}",
            LocaleKey::UsedPercent => "{:.0}% used",
            LocaleKey::RemainingPercent => "{:.0}% remaining",
            LocaleKey::RemainingAmount => "{:.2} remaining",
            LocaleKey::Tokens1K => "1K tokens",
            LocaleKey::TodayCost => "Today: ${:.2}",
            LocaleKey::Last30DaysCost => "Last 30 days: ${:.2}",
            LocaleKey::StatusLabel => "Status: {}",

            // Main popup - Update banner messages
            LocaleKey::UpdateAvailableMessage => "Update available: {}",
            LocaleKey::UpdateReadyMessage => "{} ready to install",
            LocaleKey::UpdateFailedMessage => "Update failed: {}",
            LocaleKey::UpdateDownloadingMessage => "Downloading {} ({:.0}%)",

            // Tray - Per-provider mode
            LocaleKey::TrayProviderPopOut => "Pop Out Dashboard",
            LocaleKey::TrayProviderRefresh => "Refresh",
            LocaleKey::TrayProviderSettings => "Settings...",
            LocaleKey::TrayProviderQuit => "Quit",

            // Provider settings - Live renderer specific
            LocaleKey::State => "State",
            LocaleKey::Source => "Source",
            LocaleKey::Updated => "Updated",
            LocaleKey::NeverUpdated => "Never updated",
            LocaleKey::UpdatedJustNow => "Updated just now",
            LocaleKey::UpdatedMinutesAgo => "{} minutes ago",
            LocaleKey::UpdatedHoursAgo => "{} hours ago",
            LocaleKey::UpdatedDaysAgo => "{} days ago",
            LocaleKey::Status => "Status",
            LocaleKey::AllSystemsOperational => "All systems operational",
            LocaleKey::Plan => "Plan",
            LocaleKey::Account => "Account",

            // Provider detail - Usage section
            LocaleKey::ProviderSessionLabel => "Session",
            LocaleKey::ProviderWeeklyLabel => "Weekly",
            LocaleKey::ProviderCodeReviewLabel => "Code review",
            LocaleKey::ResetsInShort => "Resets in",
            LocaleKey::ResetsInDaysHours => "Resets in {}d {}h",
            LocaleKey::ResetsInHoursMinutes => "Resets in {}h {}m",

            // Provider detail - Tray Display
            LocaleKey::TrayDisplayTitle => "Tray Display",
            LocaleKey::ShowInTray => "Show in tray",

            // Provider detail - Credits
            LocaleKey::CreditsLabel => "Credits",
            LocaleKey::CreditsLeft => "{:.1} left",

            // Provider detail - Cost
            LocaleKey::CostTitle => "Cost",
            LocaleKey::TodayCostFull => "Today: ${:.2} • {} tokens",
            LocaleKey::Last30DaysCostFull => "Last 30 days: ${:.2} • {} tokens",

            // Provider detail - Settings section
            LocaleKey::ProviderSettingsTitle => "Settings",
            LocaleKey::ProviderAccountsTitle => "Accounts",
            LocaleKey::ProviderOptionsTitle => "Options",
            LocaleKey::MenuBarMetric => "Menu bar metric",
            LocaleKey::MenuBarMetricHelper => "Choose which window drives the menu bar percent.",
            LocaleKey::UsageSource => "Usage source",
            LocaleKey::ProviderNoCodexAccountsDetected => "No Codex accounts detected yet.",
            LocaleKey::ProviderCodexAutoImportHelp => {
                "Automatic imports browser cookies for dashboard extras."
            }
            LocaleKey::ProviderCodexHistoryHelp => {
                "Stores local Codex usage history (8 weeks) to personalize Pace predictions."
            }
            LocaleKey::ProviderOpenAiCookies => "OpenAI cookies",
            LocaleKey::ProviderHistoricalTracking => "Historical tracking",
            LocaleKey::ProviderOpenAiWebExtras => "OpenAI web extras",
            LocaleKey::ProviderOpenAiWebExtrasHelp => {
                "Show usage breakdown, credits history, and code review via chatgpt.com."
            }
            LocaleKey::ProviderCodexCreditsUnavailable => {
                "Credits unavailable; keep Codex running to refresh."
            }
            LocaleKey::ProviderCodexLastFetchFailedTitle => "Last Codex fetch failed:",
            LocaleKey::ProviderCodexNotRunningHelp => {
                "Codex not running. Try running a Codex command first."
            }
            LocaleKey::ProviderCookieSource => "Cookie source",
            LocaleKey::CookieSourceManual => "Manual",
            LocaleKey::ProviderRegion => "Region",
            LocaleKey::ProviderClaudeCookies => "Claude cookies",
            LocaleKey::ProviderClaudeCookiesHelp => {
                "Browser cookies/sessionKey are preferred because they match Claude's settings usage page."
            }
            LocaleKey::ProviderClaudeAvoidKeychainPrompts => "Avoid Keychain prompts",
            LocaleKey::ProviderClaudeAvoidKeychainPromptsHelp => {
                "Use /usr/bin/security to read Claude credentials and avoid CodexBar keychain prompts."
            }
            LocaleKey::ProviderCursorCookieSourceHelp => {
                "Automatic imports browser cookies or stored sessions."
            }
            LocaleKey::ProviderCursorCreditsHelp => "On-demand usage beyond included plan limits.",
            LocaleKey::AutoFallbackHelp => {
                "Auto falls back to the next source if the preferred one fails."
            }
            LocaleKey::ProviderSourceOauthWeb => "OAuth + Web",
            LocaleKey::Automatic => "Automatic",
            LocaleKey::Average => "Average",
            LocaleKey::ExtraUsage => "Extra usage",
            LocaleKey::OAuth => "OAuth",
            LocaleKey::Api => "API",
            LocaleKey::Web => "Web",

            // General tab sections
            LocaleKey::PrivacyTitle => "Privacy",
            LocaleKey::HidePersonalInfo => "Hide Personal Info",
            LocaleKey::HidePersonalInfoHelper => {
                "Mask emails and account names (good for streaming)"
            }
            LocaleKey::UpdatesTitle => "Updates",
            LocaleKey::UpdateChannelChoice => "Update Channel",
            LocaleKey::UpdateChannelChoiceHelper => {
                "Choose between stable and beta preview versions"
            }
            LocaleKey::AutoDownloadUpdates => "Check for updates automatically",
            LocaleKey::AutoDownloadUpdatesHelper => {
                "Download installer updates in the background when a new release is found"
            }
            LocaleKey::InstallUpdatesOnQuit => "Install updates on quit",
            LocaleKey::InstallUpdatesOnQuitHelper => {
                "Automatically launch a ready installer when you quit CodexBar"
            }

            // Keyboard shortcuts
            LocaleKey::KeyboardShortcutsTitle => "Keyboard Shortcuts",
            LocaleKey::GlobalShortcutLabel => "Global Shortcut",
            LocaleKey::GlobalShortcutHelper => "Press this shortcut to open CodexBar from anywhere",
            LocaleKey::ShortcutFormatHint => {
                "Format: Ctrl+Shift+Key, Alt+Ctrl+Key, etc. Restart required to apply changes."
            }
            LocaleKey::Saved => "Saved (restart to apply)",
            LocaleKey::InvalidFormat => "Invalid shortcut format",
            LocaleKey::ShortcutHintPlaceholder => "e.g., Ctrl+Shift+U",

            // Display/Preferences helpers
            LocaleKey::SurpriseAnimationsHelper => {
                "Show occasional fun animations in the tray icon"
            }
            LocaleKey::SelectProvider => "Select a provider",

            // Refresh interval labels
            LocaleKey::RefreshInterval30Sec => "30 sec",
            LocaleKey::RefreshInterval1Min => "1 min",
            LocaleKey::RefreshInterval5Min => "5 min",
            LocaleKey::RefreshInterval10Min => "10 min",

            // Cookies tab
            LocaleKey::BrowserCookiesTitle => "Browser Cookies",
            LocaleKey::CookieImport => "Cookie Import",
            LocaleKey::Provider => "Provider",
            LocaleKey::SelectPlaceholder => "Select...",
            LocaleKey::AutoRefreshInterval => "Auto-refresh interval",

            // About tab
            LocaleKey::AboutDescription => "A Windows port of the original macOS version.",
            LocaleKey::AboutDescriptionLine2 => "Track AI provider usage in your system tray.",
            LocaleKey::ViewOnGitHub => "→ View on GitHub",
            LocaleKey::SubmitIssue => "→ Submit an Issue",
            LocaleKey::MaintainedBy => "Maintained by CodexBar contributors",
            LocaleKey::CommitLabel => "Commit",
            LocaleKey::BuildDateLabel => "Built",

            // Shared form controls
            LocaleKey::Save => "Save",
            LocaleKey::Cancel => "Cancel",
            LocaleKey::Label => "Label",
            LocaleKey::Token => "Token",
            LocaleKey::AddAccount => "Add Account",
            LocaleKey::AccountAdded => "Account added",
            LocaleKey::AccountRemoved => "Account removed",
            LocaleKey::AccountSwitched => "Account switched",
            LocaleKey::AccountLabelHint => "e.g., Work Account, Personal...",
            LocaleKey::EnterApiKeyFor => "Enter API key for {}",
            LocaleKey::PasteApiKeyHere => "Paste your API key here...",
            LocaleKey::ApiKeySaved => "Saved API key for {}",
            LocaleKey::ApiKeyRemoved => "Removed API key for {}",
            LocaleKey::EnvironmentVariable => "Environment variable",
            LocaleKey::CookieSavedForProvider => "Saved cookies for {}",
            LocaleKey::CookieRemovedForProvider => "Removed cookies for {}",

            // Usage helper functions
            LocaleKey::ShowUsedPercent => "{:.0}% used",
            LocaleKey::ShowRemainingPercent => "{:.0}% remaining",

            // Tauri desktop shell — Settings section headings
            LocaleKey::TabTokenAccounts => "Tokens",
            LocaleKey::SectionRefresh => "Automation",
            LocaleKey::SectionNotifications => "Notifications",
            LocaleKey::SectionUsageThresholds => "Usage Thresholds",
            LocaleKey::SectionKeyboard => "Keyboard",
            LocaleKey::SectionUsageRendering => "Usage rendering",
            LocaleKey::SectionTime => "Time",
            LocaleKey::SectionLanguage => "Language",
            LocaleKey::SectionCredentialsSecurity => "Credentials & Security",
            LocaleKey::SectionDebug => "Debug",
            LocaleKey::SectionApiKeys => "API Keys",
            LocaleKey::SectionSavedCookies => "Saved Cookies",
            LocaleKey::SectionImportFromBrowser => "Import from Browser",
            LocaleKey::SectionAddCookieManually => "Add Cookie Manually",
            LocaleKey::SectionTokenAccounts => "Token Accounts",
            LocaleKey::SectionSavedAccounts => "Saved Accounts",
            LocaleKey::SectionAddAccount => "Add Account",

            // Tauri desktop shell — General tab fields
            LocaleKey::RefreshIntervalLabel => "Refresh interval",
            LocaleKey::RefreshIntervalHelper => {
                "Seconds between automatic provider refreshes (0 = manual)."
            }
            LocaleKey::SoundVolumeHelper => "Volume for threshold alert sounds (0–100).",
            LocaleKey::HighUsageWarningHelper => {
                "Show a warning when usage exceeds this percentage."
            }
            LocaleKey::CriticalUsageWarningHelper => {
                "Show a critical alert when usage exceeds this percentage."
            }
            LocaleKey::GlobalShortcutFieldLabel => "Global shortcut",
            LocaleKey::GlobalShortcutToggleHelper => "Key combination to toggle the tray panel.",
            // REVIEW-i18n: Phase 7 shortcut-capture + notification test labels.
            LocaleKey::ShortcutRecordButton => "Record",
            LocaleKey::ShortcutRecordingLabel => "Recording…",
            LocaleKey::ShortcutRecordingHint => {
                "Press modifiers + a key. Esc cancels, Backspace clears."
            }
            LocaleKey::ShortcutClearButton => "Clear",
            LocaleKey::ShortcutEmptyPlaceholder => "Not set",
            LocaleKey::NotificationTestSound => "Test sound",
            LocaleKey::NotificationTestSoundPlaying => "Playing…",

            // Tauri desktop shell — Display tab fields
            LocaleKey::TrayIconModeLabel => "Tray icon mode",
            LocaleKey::TrayIconModeHelper => {
                "Single unified icon or one icon per enabled provider."
            }
            LocaleKey::TrayIconModeSingle => "Single",
            LocaleKey::TrayIconModePerProvider => "Per provider",
            LocaleKey::ShowProviderIcons => "Show provider icons",
            LocaleKey::ShowProviderIconsHelper => "Display provider icons in the tray switcher.",
            LocaleKey::PreferHighestUsage => "Prefer highest usage",
            LocaleKey::PreferHighestUsageHelper => {
                "Show the provider closest to its limit in the merged tray display."
            }
            LocaleKey::ShowPercentInTray => "Show percent in tray",
            LocaleKey::ShowPercentInTrayHelper => {
                "Replace usage bar with provider branding + percentage text."
            }
            LocaleKey::DisplayModeLabel => "Display mode",
            LocaleKey::DisplayModeHelper => "Level of detail shown in the menu bar label.",
            LocaleKey::DisplayModeDetailed => "Detailed",
            LocaleKey::DisplayModeCompact => "Compact",
            LocaleKey::DisplayModeMinimal => "Minimal",
            LocaleKey::ShowAsUsedLabel => "Show as used",
            LocaleKey::ShowAsUsedHelper => "Display usage bars as consumed rather than remaining.",
            LocaleKey::ShowAllTokenAccountsLabel => "Show all token accounts",
            LocaleKey::ShowAllTokenAccountsHelper => {
                "List all token accounts in provider menus instead of collapsing them."
            }
            LocaleKey::EnableAnimationsLabel => "Enable animations",
            LocaleKey::EnableAnimationsHelper => "Smooth transitions and animated progress bars.",
            LocaleKey::SurpriseAnimationsLabel => "Surprise animations",

            // Tauri desktop shell — Advanced tab fields
            LocaleKey::UpdateChannelStableOption => "Stable",
            LocaleKey::UpdateChannelBetaOption => "Beta",
            LocaleKey::AvoidKeychainPromptsLabel => "Avoid keychain prompts (Claude)",
            LocaleKey::AvoidKeychainPromptsHelper => {
                "Skip keychain credential reads for Claude to prevent OS permission dialogs."
            }
            LocaleKey::DisableAllKeychainLabel => "Disable all keychain access",
            LocaleKey::DisableAllKeychainHelper => {
                "Turn off credential/keychain reads for all providers. Also enables the Claude option above."
            }
            LocaleKey::ShowDebugSettingsLabel => "Show debug settings",
            LocaleKey::ShowDebugSettingsHelper => {
                "Reveal troubleshooting and developer surfaces in the UI."
            }
            LocaleKey::LanguageEnglishOption => "English",
            LocaleKey::LanguageChineseOption => "中文",

            // Tauri desktop shell — Theme (Phase 12)
            LocaleKey::SectionTheme => "Appearance",
            LocaleKey::ThemeLabel => "Theme",
            LocaleKey::ThemeHelper => {
                "Auto follows your system color scheme. Light and Dark override it."
            }
            LocaleKey::ThemeAutoOption => "Auto (system)",
            LocaleKey::ThemeLightOption => "Light",
            LocaleKey::ThemeDarkOption => "Dark",

            // Tauri desktop shell — settings status / common
            LocaleKey::SettingsStatusSaving => "Saving…",
            LocaleKey::ApiKeysTabHint => {
                "Configure API keys for providers that use token-based authentication. Keys are stored locally and never transmitted."
            }

            // Tauri desktop shell — tray / popout
            LocaleKey::FetchingProviderData => "Fetching provider data…",
            LocaleKey::NoProvidersConfigured => "No providers configured.",
            LocaleKey::EnableProvidersHint => "Enable providers in Settings to see usage data.",
            LocaleKey::OpenSettingsButton => "Open Settings",
            LocaleKey::TooltipRefresh => "Refresh",
            LocaleKey::TooltipSettings => "Settings",
            LocaleKey::TooltipPopOut => "Pop out",
            LocaleKey::TooltipBackToTray => "Back to tray",
            LocaleKey::TrayCardErrorBadge => "Error",
            LocaleKey::SummaryProvidersLabel => "providers",
            LocaleKey::SummaryRefreshing => "refreshing…",
            LocaleKey::SummaryFailed => "failed",
            LocaleKey::SummaryWithErrors => "with errors",

            // Tauri desktop shell — provider detail
            LocaleKey::DetailBackButton => "Back",
            LocaleKey::DetailWindowPrimary => "Primary",
            LocaleKey::DetailWindowSecondary => "Secondary",
            LocaleKey::DetailWindowModelSpecific => "Model-specific",
            LocaleKey::DetailWindowTertiary => "Tertiary",
            LocaleKey::DetailWindowMinutesSuffix => "m window",
            LocaleKey::DetailWindowExhausted => "Exhausted",
            LocaleKey::DetailPaceTitle => "Pace",
            LocaleKey::DetailPaceOnTrack => "On track",
            LocaleKey::DetailPaceSlightlyAhead => "Slightly ahead",
            LocaleKey::DetailPaceAhead => "Ahead",
            LocaleKey::DetailPaceFarAhead => "Far ahead",
            LocaleKey::DetailPaceSlightlyBehind => "Slightly behind",
            LocaleKey::DetailPaceBehind => "Behind",
            LocaleKey::DetailPaceFarBehind => "Far behind",
            LocaleKey::DetailPaceRunsOutIn => "Runs out in ~{}h",
            LocaleKey::DetailPaceWillLastToReset => "Will last to reset",
            LocaleKey::DetailCostTitle => "Cost",
            LocaleKey::DetailCostUsed => "Used",
            LocaleKey::DetailCostLimit => "Limit",
            LocaleKey::DetailCostRemaining => "Remaining",
            LocaleKey::DetailCostResets => "Resets",
            LocaleKey::DetailChartCost => "Cost (30 days)",
            LocaleKey::DetailChartCredits => "Credits used (30 days)",
            LocaleKey::DetailChartUsageBreakdown => "Usage by service (30 days)",
            LocaleKey::DetailChartEmpty => "No chart data yet.",
            LocaleKey::DetailUpdatedPrefix => "Updated",

            // Tauri desktop shell — update banner
            LocaleKey::BannerCheckingForUpdates => "Checking for updates…",
            LocaleKey::BannerUpdateAvailablePrefix => "Update",
            LocaleKey::BannerDownloadButton => "Download",
            LocaleKey::BannerViewRelease => "View Release",
            LocaleKey::BannerDismiss => "Dismiss",
            LocaleKey::BannerDownloadingPrefix => "Downloading update",
            LocaleKey::BannerReadyToInstallSuffix => "ready to install",
            LocaleKey::BannerInstallRestart => "Install & Restart",
            LocaleKey::BannerUpdateFailedPrefix => "Update failed",
            LocaleKey::BannerRetry => "Retry",

            // Tauri desktop shell — providers sidebar (Phase 6a)
            LocaleKey::ProviderSidebarReorderHint => "Drag to reorder",
            LocaleKey::ProviderStatusOk => "Up to date",
            LocaleKey::ProviderStatusStale => "Stale",
            LocaleKey::ProviderStatusError => "Error",
            LocaleKey::ProviderStatusLoading => "Loading",
            LocaleKey::ProviderStatusDisabled => "Disabled",
            LocaleKey::ProviderDetailPlaceholder => "Detail pane arriving in Phase 6b",

            // Phase 6d — credential detection
            LocaleKey::CredentialsSectionTitle => "Credentials",
            LocaleKey::CredsStatusAuthenticated => "Authenticated",
            LocaleKey::CredsStatusNotSignedIn => "Not signed in",
            LocaleKey::CredsStatusDetected => "Detected",
            LocaleKey::CredsStatusNotDetected => "Not detected",
            LocaleKey::CredsStatusAvailable => "Available",
            LocaleKey::CredsStatusUnavailable => "Unavailable",
            LocaleKey::CredsOpenFolderAction => "Open credentials folder",
            LocaleKey::CredsRefreshDetectionAction => "Refresh detection",
            LocaleKey::CredsSavePathAction => "Save path",
            LocaleKey::CredsBrowseAction => "Browse…",
            LocaleKey::CredsGeminiCliLabel => "Gemini CLI",
            LocaleKey::CredsGeminiCliHelperPrefix => "Uses OAuth credentials from",
            LocaleKey::CredsGeminiCliSetupAction => "Setup Gemini CLI",
            LocaleKey::CredsGeminiCliSetupHelp => {
                "Install the Gemini CLI and run `gemini auth login` to sign in."
            }
            LocaleKey::CredsVertexAiLabel => "Google Cloud",
            LocaleKey::CredsVertexAiHelperPrefix => "Uses Google Cloud credentials from",
            LocaleKey::CredsVertexAiSetupAction => "Setup Google Cloud Auth",
            LocaleKey::CredsVertexAiSetupHelp => {
                "Run `gcloud auth application-default login` to create credentials."
            }
            LocaleKey::CredsJetBrainsLabel => "JetBrains IDE",
            LocaleKey::CredsJetBrainsHelperDetectedPrefix => "Using detected IDE config at",
            LocaleKey::CredsJetBrainsHelperCustomPrefix => "Using custom IDE base path",
            LocaleKey::CredsJetBrainsHelperMissing => {
                "Install a JetBrains IDE with AI Assistant enabled, then refresh CodexBar."
            }
            LocaleKey::CredsJetBrainsCustomPathLabel => "Custom path",
            LocaleKey::CredsJetBrainsCustomPathPlaceholder => "%APPDATA%/JetBrains/IntelliJIdea...",
            LocaleKey::CredsJetBrainsSelectLabel => "Select the JetBrains IDE to monitor.",
            LocaleKey::CredsJetBrainsAutoDetectOption => "Auto-detect",
            LocaleKey::CredsKiroLabel => "Kiro CLI",
            LocaleKey::CredsKiroHelperAvailablePrefix => "Detected at",
            LocaleKey::CredsKiroHelperMissing => {
                "kiro-cli: not found on PATH or known install locations."
            }
            LocaleKey::CredsOpenAiHistoryHelp => {
                "Enable historical tracking to see usage over time."
            }

            // Tauri desktop shell — Token accounts (Phase 6e, review)
            LocaleKey::TokenAccountActive => "Active",
            LocaleKey::TokenAccountSetActive => "Set Active",
            LocaleKey::TokenAccountRemove => "Remove",
            LocaleKey::TokenAccountAddButton => "Add Account",
            LocaleKey::TokenAccountGithubLoginButton => "Sign in with GitHub",
            LocaleKey::TokenAccountEmpty => "No accounts saved for this provider.",
            LocaleKey::TokenAccountLabelPlaceholder => "Label (e.g. Work, Personal)…",
            LocaleKey::TokenAccountProviderLabel => "Provider",
            LocaleKey::TokenAccountProviderPlaceholder => "Select provider…",
            LocaleKey::TokenAccountAddedPrefix => "Added",
            LocaleKey::TokenAccountUsedPrefix => "Used",
            LocaleKey::TokenAccountTabHint => {
                "Manage multiple session tokens or API tokens per provider. The active account is used for all fetches. Only providers that require manual tokens appear here."
            }
            LocaleKey::TokenAccountNoSupported => "No providers currently support token accounts.",
            LocaleKey::TokenAccountInlineSummary => "Token accounts",

            // Phase 9 - Tray / pop-out pace badges + countdowns
            LocaleKey::TrayPaceBadgeSlow => "Slow",
            LocaleKey::TrayPaceBadgeSteady => "Steady",
            LocaleKey::TrayPaceBadgeRacing => "Racing",
            LocaleKey::TrayPaceBadgeBurning => "Burning",
            LocaleKey::TrayResetsInLabel => "Resets in {}",
            LocaleKey::TrayResetsDueNow => "Resetting…",
        }
    }

    fn chinese(self) -> &'static str {
        match self {
            // Tab names
            LocaleKey::TabGeneral => "通用",
            LocaleKey::TabProviders => "服务商",
            LocaleKey::TabDisplay => "显示",
            LocaleKey::TabApiKeys => "API 密钥",
            LocaleKey::TabCookies => "Cookie",
            LocaleKey::TabAdvanced => "高级",
            LocaleKey::TabAbout => "关于",
            LocaleKey::TabShortcuts => "快捷键",

            // General settings
            LocaleKey::InterfaceLanguage => "界面语言",
            LocaleKey::StartupSettings => "系统",
            LocaleKey::StartAtLogin => "开机启动",
            LocaleKey::StartMinimized => "最小化启动",
            LocaleKey::StartAtLoginHelper => "登录后自动启动 CodexBar",
            LocaleKey::StartMinimizedHelper => "启动后停留在系统托盘",

            // Notification settings
            LocaleKey::ShowNotifications => "显示通知",
            LocaleKey::ShowNotificationsHelper => "达到用量阈值时提醒",
            LocaleKey::SoundEnabled => "声音提示",
            LocaleKey::SoundEnabledHelper => "达到阈值时播放提示音",
            LocaleKey::SoundVolume => "提示音音量",
            LocaleKey::HighUsageThreshold => "高用量阈值",
            LocaleKey::HighUsageThresholdHelper => "在该用量水平显示预警",
            LocaleKey::HighUsageAlert => "高位预警",
            LocaleKey::CriticalUsageThreshold => "紧急用量阈值",
            LocaleKey::CriticalUsageThresholdHelper => "在该水平显示严重告警",
            LocaleKey::CriticalUsageAlert => "严重告警",

            // Display settings
            LocaleKey::ShowUsageAsUsed => "显示已用用量",
            LocaleKey::ShowUsageAsUsedHelper => "显示为已使用百分比（而非剩余）",
            LocaleKey::ResetTimeRelative => "相对重置时间",
            LocaleKey::ResetTimeRelativeHelper => "显示\"2h 30m\"而不是\"3:00 PM\"",
            LocaleKey::ShowCreditsExtra => "显示额度与扩展用量",
            LocaleKey::ShowCreditsExtraHelper => "显示额度余额和额外用量信息",
            LocaleKey::UsageDisplay => "用量显示",
            LocaleKey::TrayIcon => "托盘图标",
            LocaleKey::MergeTrayIcons => "合并托盘图标",
            LocaleKey::MergeTrayIconsHelper => "将所有服务商显示在一个托盘图标中",
            LocaleKey::PerProviderTrayIcons => "按服务商分图标",
            LocaleKey::PerProviderTrayIconsHelper => "每个启用的服务商显示独立托盘图标",

            // Provider settings
            LocaleKey::ProviderEnabled => "已启用",
            LocaleKey::ProviderDisabled => "已禁用",
            LocaleKey::ProviderInfo => "信息",
            LocaleKey::ProviderUsage => "用量",
            LocaleKey::AuthType => "认证方式",
            LocaleKey::DataSource => "数据来源",
            LocaleKey::ProviderNotDetected => "未检测到",
            LocaleKey::ProviderLastFetchFailed => "上次获取失败",
            LocaleKey::ProviderUsageNotFetchedYet => "尚未获取用量",
            LocaleKey::ProviderNotFetchedYetTitle => "尚未获取",
            LocaleKey::ProviderDisabledNoRecentData => "已禁用 — 没有最近数据",
            LocaleKey::ProviderSourceAutoShort => "自动",
            LocaleKey::ProviderSourceWebShort => "网页",
            LocaleKey::ProviderSourceCliShort => "CLI",
            LocaleKey::ProviderSourceOauthShort => "OAuth",
            LocaleKey::ProviderSourceApiShort => "API",
            LocaleKey::ProviderSourceGithubApiShort => "GitHub API",
            LocaleKey::ProviderSourceLocalShort => "本地",
            LocaleKey::ProviderSourceKiroEnvShort => "Kiro 环境",
            LocaleKey::TrackingItem => "追踪项",
            LocaleKey::MainWindowLiveUsageData => "主窗口实时用量数据",
            LocaleKey::StartTrackingUsage => "启用后开始追踪用量",
            LocaleKey::ClickTrayIconForMetrics => "点击托盘图标查看实时指标",

            // Browser cookie import
            LocaleKey::BrowserCookieImport => "浏览器 Cookie 导入",
            LocaleKey::ImportFromBrowser => "从浏览器导入 {} 的 Cookies",
            LocaleKey::NoCookiesFoundInBrowser => "在 {} 的 {} 中未找到 Cookies。请先确认已登录",
            LocaleKey::SelectBrowser => "请选择浏览器...",
            LocaleKey::ImportCookies => "导入 Cookies",
            LocaleKey::ImportSuccess => "已为 {} 导入 Cookies",
            LocaleKey::ImportFailed => "导入失败：{}",
            LocaleKey::SaveFailed => "保存失败：{}",
            LocaleKey::CookiesAutoImport => {
                "Cookies 会自动从 Chrome、Edge、Brave 和 Firefox 中提取"
            }
            LocaleKey::QuickActions => "快捷操作",
            LocaleKey::OpenProviderDashboard => "→ 打开 {} 仪表盘",
            LocaleKey::OllamaNoDashboard => "Ollama 在本地运行，无仪表盘",

            // API Keys tab
            LocaleKey::ApiKeysTitle => "API 密钥",
            LocaleKey::ApiKeysDescription => "为需要认证的服务商配置访问令牌。",
            LocaleKey::AddKey => "+ 添加密钥",
            LocaleKey::KeySet => "✓ 已设置",
            LocaleKey::KeyRequired => "需要密钥",
            LocaleKey::Remove => "移除",
            LocaleKey::GetKey => "获取密钥 →",

            // Cookies tab
            LocaleKey::SavedCookies => "已保存的 Cookies",
            LocaleKey::AddManualCookie => "添加手动 Cookie",
            LocaleKey::CookieHeader => "Cookie 头",
            LocaleKey::PasteHere => "在这里粘贴...",
            LocaleKey::DeleteCookie => "删除",
            LocaleKey::CookieSaved => "已保存 {} 个 Cookies",
            LocaleKey::CookieDeleted => "已删除 {} 的 Cookies",

            // Advanced tab
            LocaleKey::RefreshSettings => "刷新",
            LocaleKey::Animations => "动画",
            LocaleKey::MenuBar => "菜单栏",
            LocaleKey::Fun => "趣味",
            LocaleKey::GlobalShortcut => "全局快捷键",
            LocaleKey::Privacy => "隐私",
            LocaleKey::Updates => "更新",
            LocaleKey::UpdateChannel => "更新通道",
            LocaleKey::UpdateChannelStable => "稳定版",
            LocaleKey::UpdateChannelBeta => "测试预览版",
            LocaleKey::Never => "从不",
            LocaleKey::LastUpdated => "上次更新",
            LocaleKey::MinutesAgo => "{} 分钟前更新",
            LocaleKey::HoursAgo => "{} 小时前更新",
            LocaleKey::DaysAgo => "{} 天前更新",
            LocaleKey::BuiltWithRust => "基于 Rust + egui 构建",
            LocaleKey::OriginalMacOSVersion => "原始 macOS 版本",
            LocaleKey::Links => "链接",
            LocaleKey::BuildInfo => "构建信息",
            LocaleKey::EnabledProviders => "已启用服务商",
            LocaleKey::Appearance => "外观",
            LocaleKey::ThemeSelection => "主题",
            LocaleKey::LightMode => "浅色",
            LocaleKey::DarkMode => "深色",

            // About
            LocaleKey::AboutTitle => "关于 CodexBar",
            LocaleKey::Version => "版本",

            // Main popup - Header actions
            LocaleKey::ActionRefreshAll => "刷新全部",
            LocaleKey::ActionSettings => "设置",
            LocaleKey::ActionClose => "✕",

            // Main popup - Provider section
            LocaleKey::ProviderAccount => "账号",
            LocaleKey::ProviderSession => "本次会话",
            LocaleKey::ProviderWeekly => "本周",
            LocaleKey::ProviderMonthly => "30天",
            LocaleKey::ProviderModel => "模型",
            LocaleKey::ProviderPlan => "套餐",
            LocaleKey::ProviderNextReset => "下次重置",
            LocaleKey::ProviderNoRecentUsage => "暂无用量",
            LocaleKey::ProviderNotSignedIn => "未登录",
            LocaleKey::SummaryTab => "汇总",

            // Main popup - Loading/Empty/Error states
            LocaleKey::StateLoadingProviders => "正在加载服务商...",
            LocaleKey::StateNoProviderData => "暂无服务商数据。",
            LocaleKey::StateNoProviderSelected => "尚未选择服务商。",
            LocaleKey::StateSummaryRefreshPending => "将在全部服务商刷新完成后更新",
            LocaleKey::StateError => "错误",
            LocaleKey::StateRetry => "重试",
            LocaleKey::StateDownload => "下载",
            LocaleKey::StateRestartAndUpdate => "重启并更新",

            // Main popup - Credits
            LocaleKey::CreditsTitle => "额度",

            // Main popup - Update banner (non-happy-path)
            LocaleKey::UpdateRestartAndUpdate => "重启并更新",
            LocaleKey::UpdateRetry => "重试",
            LocaleKey::UpdateDownload => "下载",
            LocaleKey::UpdateDownloading => "下载中",
            LocaleKey::UpdateReady => "准备安装",
            LocaleKey::UpdateFailed => "更新失败",

            // Main popup - Settings button
            LocaleKey::ButtonOpenProviderSettings => "打开服务商设置",

            // Main popup - Bottom menu (Actions)
            LocaleKey::MenuSettings => "设置...",
            LocaleKey::MenuAbout => "关于 CodexBar",
            LocaleKey::MenuQuit => "退出",

            // Main popup - Status strings
            LocaleKey::StatusJustUpdated => "刚刚更新",
            LocaleKey::StatusUnableToGetUsage => "无法获取用量",

            // Main popup - Provider detail actions
            LocaleKey::ActionRefresh => "刷新",
            LocaleKey::ActionSwitchAccount => "切换账号...",
            LocaleKey::ActionUsageDashboard => "用量仪表盘",
            LocaleKey::ActionStatusPage => "状态页面",
            LocaleKey::ActionCopyError => "复制错误",
            LocaleKey::ActionBuyCredits => "购买额度...",

            // Main popup - Pace status
            LocaleKey::PaceOnTrack => "进度正常",
            LocaleKey::PaceBehind => "进度滞后",

            // Main popup - Reset prefix
            LocaleKey::MetricResetsIn => "重置于",

            // Main popup - Section titles
            LocaleKey::SectionUsageBreakdown => "用量明细",
            LocaleKey::SectionCost => "费用",

            // Tray - Single icon mode
            LocaleKey::TrayOpenCodexBar => "弹出仪表盘",
            LocaleKey::TrayPopOutDashboard => "弹出仪表盘",
            LocaleKey::TrayRefreshAll => "刷新全部",
            LocaleKey::TrayProviders => "服务商",
            LocaleKey::TraySettings => "设置...",
            LocaleKey::TrayCheckForUpdates => "检查更新",
            LocaleKey::TrayQuit => "退出",
            LocaleKey::TrayLoading => "CodexBar - 加载中...",
            LocaleKey::TrayNoProviders => "CodexBar - 无可用服务商",
            LocaleKey::TraySessionPercent => "本次会话 {}%",
            LocaleKey::TrayWeeklyPercent => "本周 {}%",
            LocaleKey::TrayStatusError => "（错误）",
            LocaleKey::TrayStatusStale => "（数据过期）",
            LocaleKey::TrayStatusIncident => "（故障）",
            LocaleKey::TrayStatusPartial => "（部分中断）",
            LocaleKey::TrayWeeklyExhausted => "周额度已用尽",
            LocaleKey::TrayCreditsRemaining => "剩余额度 {}%",
            LocaleKey::TrayStatusRowLoading => "加载中...",
            LocaleKey::TrayStatusRowError => "错误",
            LocaleKey::TrayCreditsRow => "额度 {}%",

            // Main popup - Usage/reset labels
            LocaleKey::ResetInProgress => "正在重置...",
            LocaleKey::TomorrowAt => "明天 {}",
            LocaleKey::UsedPercent => "已使用 {:.0}%",
            LocaleKey::RemainingPercent => "剩余 {:.0}%",
            LocaleKey::RemainingAmount => "剩余 {:.2}",
            LocaleKey::Tokens1K => "1K tokens",
            LocaleKey::TodayCost => "今日：${:.2}",
            LocaleKey::Last30DaysCost => "近 30 天：${:.2}",
            LocaleKey::StatusLabel => "状态：{}",

            // Main popup - Update banner messages
            LocaleKey::UpdateAvailableMessage => "有可用更新：{}",
            LocaleKey::UpdateReadyMessage => "{} 准备安装",
            LocaleKey::UpdateFailedMessage => "更新失败：{}",
            LocaleKey::UpdateDownloadingMessage => "正在下载 {} ({:.0}%)",

            // Tray - Per-provider mode
            LocaleKey::TrayProviderPopOut => "弹出仪表盘",
            LocaleKey::TrayProviderRefresh => "刷新",
            LocaleKey::TrayProviderSettings => "设置...",
            LocaleKey::TrayProviderQuit => "退出",

            // Provider settings - Live renderer specific
            LocaleKey::State => "状态",
            LocaleKey::Source => "来源",
            LocaleKey::Updated => "更新时间",
            LocaleKey::NeverUpdated => "从未更新",
            LocaleKey::UpdatedJustNow => "刚刚更新",
            LocaleKey::UpdatedMinutesAgo => "{} 分钟前更新",
            LocaleKey::UpdatedHoursAgo => "{} 小时前更新",
            LocaleKey::UpdatedDaysAgo => "{} 天前更新",
            LocaleKey::Status => "状态",
            LocaleKey::AllSystemsOperational => "系统运行正常",
            LocaleKey::Plan => "套餐",
            LocaleKey::Account => "账号",

            // Provider detail - Usage section
            LocaleKey::ProviderSessionLabel => "本次会话",
            LocaleKey::ProviderWeeklyLabel => "本周",
            LocaleKey::ProviderCodeReviewLabel => "代码审查",
            LocaleKey::ResetsInShort => "重置于",
            LocaleKey::ResetsInDaysHours => "{} 天 {} 小时后重置",
            LocaleKey::ResetsInHoursMinutes => "{} 小时 {} 分钟后重置",

            // Provider detail - Tray Display
            LocaleKey::TrayDisplayTitle => "托盘显示",
            LocaleKey::ShowInTray => "在托盘中显示",

            // Provider detail - Credits
            LocaleKey::CreditsLabel => "额度",
            LocaleKey::CreditsLeft => "剩余 {:.1}",

            // Provider detail - Cost
            LocaleKey::CostTitle => "费用",
            LocaleKey::TodayCostFull => "今日：${:.2} • {} tokens",
            LocaleKey::Last30DaysCostFull => "近 30 天：${:.2} • {} tokens",

            // Provider detail - Settings section
            LocaleKey::ProviderSettingsTitle => "设置",
            LocaleKey::ProviderAccountsTitle => "账号",
            LocaleKey::ProviderOptionsTitle => "选项",
            LocaleKey::MenuBarMetric => "菜单栏指标",
            LocaleKey::MenuBarMetricHelper => "选择由哪个窗口驱动菜单栏百分比。",
            LocaleKey::UsageSource => "用量来源",
            LocaleKey::ProviderNoCodexAccountsDetected => "尚未检测到 Codex 账号。",
            LocaleKey::ProviderCodexAutoImportHelp => "自动导入浏览器 Cookie 以补充仪表盘信息。",
            LocaleKey::ProviderCodexHistoryHelp => {
                "在本地保存 Codex 用量历史（8 周），用于个性化 Pace 预测。"
            }
            LocaleKey::ProviderOpenAiCookies => "OpenAI Cookie",
            LocaleKey::ProviderHistoricalTracking => "历史追踪",
            LocaleKey::ProviderOpenAiWebExtras => "OpenAI 网页扩展",
            LocaleKey::ProviderOpenAiWebExtrasHelp => {
                "通过 chatgpt.com 显示用量明细、额度历史和代码审查信息。"
            }
            LocaleKey::ProviderCodexCreditsUnavailable => {
                "额度暂不可用；保持 Codex 运行后会自动刷新。"
            }
            LocaleKey::ProviderCodexLastFetchFailedTitle => "上次 Codex 获取失败：",
            LocaleKey::ProviderCodexNotRunningHelp => "Codex 未运行。先运行一次 Codex 命令再试。",
            LocaleKey::ProviderCookieSource => "Cookie 来源",
            LocaleKey::CookieSourceManual => "手动",
            LocaleKey::ProviderRegion => "地区",
            LocaleKey::ProviderClaudeCookies => "Claude Cookie",
            LocaleKey::ProviderClaudeCookiesHelp => {
                "优先使用浏览器 Cookie/sessionKey，因为它与 Claude 设置页的用量一致。"
            }
            LocaleKey::ProviderClaudeAvoidKeychainPrompts => "避免钥匙串提示",
            LocaleKey::ProviderClaudeAvoidKeychainPromptsHelp => {
                "使用 /usr/bin/security 读取 Claude 凭据，避免 CodexBar 的钥匙串提示。"
            }
            LocaleKey::ProviderCursorCookieSourceHelp => "自动导入浏览器 Cookie 或已保存会话。",
            LocaleKey::ProviderCursorCreditsHelp => "包含计划额度之外的按量计费用量。",
            LocaleKey::AutoFallbackHelp => "当首选来源失败时自动回退到下一个来源。",
            LocaleKey::ProviderSourceOauthWeb => "OAuth + 网页",
            LocaleKey::Automatic => "自动",
            LocaleKey::Average => "平均",
            LocaleKey::ExtraUsage => "额外用量",
            LocaleKey::OAuth => "OAuth",
            LocaleKey::Api => "API",
            LocaleKey::Web => "网页",

            // General tab sections
            LocaleKey::PrivacyTitle => "隐私",
            LocaleKey::HidePersonalInfo => "隐藏个人信息",
            LocaleKey::HidePersonalInfoHelper => "遮蔽邮箱和账号名称（适合直播时使用）",
            LocaleKey::UpdatesTitle => "更新",
            LocaleKey::UpdateChannelChoice => "更新通道",
            LocaleKey::UpdateChannelChoiceHelper => "在稳定版与测试预览版之间选择",
            LocaleKey::AutoDownloadUpdates => "自动检查更新",
            LocaleKey::AutoDownloadUpdatesHelper => "发现新版本后在后台下载安装器更新",
            LocaleKey::InstallUpdatesOnQuit => "退出时安装更新",
            LocaleKey::InstallUpdatesOnQuitHelper => "退出 CodexBar 时自动启动已准备好的安装器",

            // Keyboard shortcuts
            LocaleKey::KeyboardShortcutsTitle => "快捷键",
            LocaleKey::GlobalShortcutLabel => "全局快捷键",
            LocaleKey::GlobalShortcutHelper => "按此快捷键可从任何位置打开 CodexBar",
            LocaleKey::ShortcutFormatHint => {
                "格式：Ctrl+Shift+Key、Alt+Ctrl+Key 等。需重启以应用更改。"
            }
            LocaleKey::Saved => "已保存（需重启以应用）",
            LocaleKey::InvalidFormat => "无效的快捷键格式",
            LocaleKey::ShortcutHintPlaceholder => "例如：Ctrl+Shift+U",

            // Display/Preferences helpers
            LocaleKey::SurpriseAnimationsHelper => "在托盘图标中偶尔显示趣味动画",
            LocaleKey::SelectProvider => "请选择服务商",

            // Refresh interval labels
            LocaleKey::RefreshInterval30Sec => "30 秒",
            LocaleKey::RefreshInterval1Min => "1 分钟",
            LocaleKey::RefreshInterval5Min => "5 分钟",
            LocaleKey::RefreshInterval10Min => "10 分钟",

            // Cookies tab
            LocaleKey::BrowserCookiesTitle => "浏览器 Cookie",
            LocaleKey::CookieImport => "Cookie 导入",
            LocaleKey::Provider => "服务商",
            LocaleKey::SelectPlaceholder => "请选择...",
            LocaleKey::AutoRefreshInterval => "自动刷新间隔",

            // About tab
            LocaleKey::AboutDescription => "CodexBar 的 Windows 移植版本。",
            LocaleKey::AboutDescriptionLine2 => "在系统托盘中追踪 AI 服务商用量。",
            LocaleKey::ViewOnGitHub => "→ 查看 GitHub",
            LocaleKey::SubmitIssue => "→ 提交问题",
            LocaleKey::MaintainedBy => "由 CodexBar 贡献者维护",
            LocaleKey::CommitLabel => "提交",
            LocaleKey::BuildDateLabel => "构建",

            // Shared form controls
            LocaleKey::Save => "保存",
            LocaleKey::Cancel => "取消",
            LocaleKey::Label => "标签",
            LocaleKey::Token => "令牌",
            LocaleKey::AddAccount => "添加账号",
            LocaleKey::AccountAdded => "账号已添加",
            LocaleKey::AccountRemoved => "账号已移除",
            LocaleKey::AccountSwitched => "账号已切换",
            LocaleKey::AccountLabelHint => "例如：工作账号、个人账号...",
            LocaleKey::EnterApiKeyFor => "为 {} 输入 API Key",
            LocaleKey::PasteApiKeyHere => "在这里粘贴 API key...",
            LocaleKey::ApiKeySaved => "已保存 {} 的 API key",
            LocaleKey::ApiKeyRemoved => "已移除 {} 的 API key",
            LocaleKey::EnvironmentVariable => "环境变量",
            LocaleKey::CookieSavedForProvider => "已保存 {} 的 Cookie",
            LocaleKey::CookieRemovedForProvider => "已移除 {} 的 Cookie",

            // Usage helper functions
            LocaleKey::ShowUsedPercent => "已使用 {:.0}%",
            LocaleKey::ShowRemainingPercent => "剩余 {:.0}%",

            // Tauri desktop shell — Settings section headings
            LocaleKey::TabTokenAccounts => "令牌",
            LocaleKey::SectionRefresh => "自动化",
            LocaleKey::SectionNotifications => "通知",
            LocaleKey::SectionUsageThresholds => "用量阈值",
            LocaleKey::SectionKeyboard => "键盘",
            LocaleKey::SectionUsageRendering => "用量展示",
            LocaleKey::SectionTime => "时间",
            LocaleKey::SectionLanguage => "语言",
            LocaleKey::SectionCredentialsSecurity => "凭据与安全",
            LocaleKey::SectionDebug => "调试",
            LocaleKey::SectionApiKeys => "API 密钥",
            LocaleKey::SectionSavedCookies => "已保存的 Cookies",
            LocaleKey::SectionImportFromBrowser => "从浏览器导入",
            LocaleKey::SectionAddCookieManually => "手动添加 Cookie",
            LocaleKey::SectionTokenAccounts => "令牌账户",
            LocaleKey::SectionSavedAccounts => "已保存账户",
            LocaleKey::SectionAddAccount => "添加账户",

            // Tauri desktop shell — General tab fields
            LocaleKey::RefreshIntervalLabel => "刷新间隔",
            LocaleKey::RefreshIntervalHelper => "两次自动刷新之间的秒数（0 = 手动）。",
            LocaleKey::SoundVolumeHelper => "阈值告警音量（0–100）。",
            LocaleKey::HighUsageWarningHelper => "当用量超过该百分比时显示预警。",
            LocaleKey::CriticalUsageWarningHelper => "当用量超过该百分比时显示严重告警。",
            LocaleKey::GlobalShortcutFieldLabel => "全局快捷键",
            LocaleKey::GlobalShortcutToggleHelper => "用于切换托盘面板的组合键。",
            // REVIEW-i18n: Phase 7 shortcut-capture + notification test labels.
            LocaleKey::ShortcutRecordButton => "录制",
            LocaleKey::ShortcutRecordingLabel => "录制中…",
            LocaleKey::ShortcutRecordingHint => "按下修饰键 + 任意键。Esc 取消，Backspace 清除。",
            LocaleKey::ShortcutClearButton => "清除",
            LocaleKey::ShortcutEmptyPlaceholder => "未设置",
            LocaleKey::NotificationTestSound => "测试声音",
            LocaleKey::NotificationTestSoundPlaying => "播放中…",

            // Tauri desktop shell — Display tab fields
            LocaleKey::TrayIconModeLabel => "托盘图标模式",
            LocaleKey::TrayIconModeHelper => "使用单一合并图标，或为每个已启用服务商显示独立图标。",
            LocaleKey::TrayIconModeSingle => "合并",
            LocaleKey::TrayIconModePerProvider => "按服务商",
            LocaleKey::ShowProviderIcons => "显示服务商图标",
            LocaleKey::ShowProviderIconsHelper => "在托盘切换器中显示服务商图标。",
            LocaleKey::PreferHighestUsage => "优先显示最高用量",
            LocaleKey::PreferHighestUsageHelper => "在合并托盘显示中优先展示最接近限额的服务商。",
            LocaleKey::ShowPercentInTray => "在托盘中显示百分比",
            LocaleKey::ShowPercentInTrayHelper => "使用服务商标识与百分比文字替代用量条。",
            LocaleKey::DisplayModeLabel => "显示模式",
            LocaleKey::DisplayModeHelper => "菜单栏标签显示的详细程度。",
            LocaleKey::DisplayModeDetailed => "详细",
            LocaleKey::DisplayModeCompact => "紧凑",
            LocaleKey::DisplayModeMinimal => "最简",
            LocaleKey::ShowAsUsedLabel => "显示为已用",
            LocaleKey::ShowAsUsedHelper => "以已使用百分比而非剩余显示用量条。",
            LocaleKey::ShowAllTokenAccountsLabel => "显示所有令牌账户",
            LocaleKey::ShowAllTokenAccountsHelper => {
                "在服务商菜单中列出所有令牌账户，而不是折叠显示。"
            }
            LocaleKey::EnableAnimationsLabel => "启用动画",
            LocaleKey::EnableAnimationsHelper => "平滑过渡与动画进度条。",
            LocaleKey::SurpriseAnimationsLabel => "惊喜动画",

            // Tauri desktop shell — Advanced tab fields
            LocaleKey::UpdateChannelStableOption => "稳定版",
            LocaleKey::UpdateChannelBetaOption => "测试预览版",
            LocaleKey::AvoidKeychainPromptsLabel => "避免钥匙串弹窗（Claude）",
            LocaleKey::AvoidKeychainPromptsHelper => {
                "跳过 Claude 的钥匙串凭据读取，避免系统权限弹窗。"
            }
            LocaleKey::DisableAllKeychainLabel => "禁用所有钥匙串访问",
            LocaleKey::DisableAllKeychainHelper => {
                "关闭所有服务商的凭据/钥匙串读取。同时启用上方的 Claude 选项。"
            }
            LocaleKey::ShowDebugSettingsLabel => "显示调试设置",
            LocaleKey::ShowDebugSettingsHelper => "在界面中显示故障排查和开发者相关选项。",
            LocaleKey::LanguageEnglishOption => "English",
            LocaleKey::LanguageChineseOption => "中文",

            // Tauri desktop shell — Theme (Phase 12)
            // REVIEW-i18n: ZH translations for Phase 12 theme labels.
            LocaleKey::SectionTheme => "外观",
            LocaleKey::ThemeLabel => "主题",
            LocaleKey::ThemeHelper => "自动跟随系统配色方案；浅色/深色可手动覆盖。",
            LocaleKey::ThemeAutoOption => "自动（跟随系统）",
            LocaleKey::ThemeLightOption => "浅色",
            LocaleKey::ThemeDarkOption => "深色",

            // Tauri desktop shell — settings status / common
            LocaleKey::SettingsStatusSaving => "保存中…",
            LocaleKey::ApiKeysTabHint => {
                "为使用令牌认证的服务商配置 API 密钥。密钥仅存储在本地，不会上传。"
            }

            // Tauri desktop shell — tray / popout
            LocaleKey::FetchingProviderData => "正在获取服务商数据…",
            LocaleKey::NoProvidersConfigured => "尚未配置任何服务商。",
            LocaleKey::EnableProvidersHint => "请在设置中启用服务商以查看用量数据。",
            LocaleKey::OpenSettingsButton => "打开设置",
            LocaleKey::TooltipRefresh => "刷新",
            LocaleKey::TooltipSettings => "设置",
            LocaleKey::TooltipPopOut => "弹出",
            LocaleKey::TooltipBackToTray => "返回托盘",
            LocaleKey::TrayCardErrorBadge => "错误",
            LocaleKey::SummaryProvidersLabel => "服务商",
            LocaleKey::SummaryRefreshing => "正在刷新…",
            LocaleKey::SummaryFailed => "失败",
            LocaleKey::SummaryWithErrors => "存在错误",

            // Tauri desktop shell — provider detail
            LocaleKey::DetailBackButton => "返回",
            LocaleKey::DetailWindowPrimary => "主要",
            LocaleKey::DetailWindowSecondary => "次要",
            LocaleKey::DetailWindowModelSpecific => "模型专属",
            LocaleKey::DetailWindowTertiary => "第三",
            LocaleKey::DetailWindowMinutesSuffix => "分钟窗口",
            LocaleKey::DetailWindowExhausted => "已用尽",
            LocaleKey::DetailPaceTitle => "进度",
            LocaleKey::DetailPaceOnTrack => "正常",
            LocaleKey::DetailPaceSlightlyAhead => "略超前",
            LocaleKey::DetailPaceAhead => "超前",
            LocaleKey::DetailPaceFarAhead => "远超前",
            LocaleKey::DetailPaceSlightlyBehind => "略落后",
            LocaleKey::DetailPaceBehind => "落后",
            LocaleKey::DetailPaceFarBehind => "远落后",
            LocaleKey::DetailPaceRunsOutIn => "约 {} 小时后耗尽",
            LocaleKey::DetailPaceWillLastToReset => "足以支撑到重置",
            LocaleKey::DetailCostTitle => "费用",
            LocaleKey::DetailCostUsed => "已用",
            LocaleKey::DetailCostLimit => "限额",
            LocaleKey::DetailCostRemaining => "剩余",
            LocaleKey::DetailCostResets => "重置",
            LocaleKey::DetailChartCost => "费用（30 天）",
            LocaleKey::DetailChartCredits => "已用额度（30 天）",
            LocaleKey::DetailChartUsageBreakdown => "按服务划分的用量（30 天）",
            // REVIEW-i18n
            LocaleKey::DetailChartEmpty => "暂无图表数据。",
            LocaleKey::DetailUpdatedPrefix => "更新于",

            // Tauri desktop shell — update banner
            LocaleKey::BannerCheckingForUpdates => "正在检查更新…",
            LocaleKey::BannerUpdateAvailablePrefix => "更新",
            LocaleKey::BannerDownloadButton => "下载",
            LocaleKey::BannerViewRelease => "查看发布",
            LocaleKey::BannerDismiss => "忽略",
            LocaleKey::BannerDownloadingPrefix => "正在下载更新",
            LocaleKey::BannerReadyToInstallSuffix => "已准备好安装",
            LocaleKey::BannerInstallRestart => "安装并重启",
            LocaleKey::BannerUpdateFailedPrefix => "更新失败",
            LocaleKey::BannerRetry => "重试",

            // Tauri desktop shell — providers sidebar (Phase 6a)
            LocaleKey::ProviderSidebarReorderHint => "拖动以重新排序",
            LocaleKey::ProviderStatusOk => "已更新",
            LocaleKey::ProviderStatusStale => "已过期",
            LocaleKey::ProviderStatusError => "错误",
            LocaleKey::ProviderStatusLoading => "加载中",
            LocaleKey::ProviderStatusDisabled => "已禁用",
            LocaleKey::ProviderDetailPlaceholder => "详细面板将在 6b 阶段推出",

            // Phase 6d — credential detection
            LocaleKey::CredentialsSectionTitle => "凭据",
            LocaleKey::CredsStatusAuthenticated => "已认证",
            LocaleKey::CredsStatusNotSignedIn => "未登录",
            LocaleKey::CredsStatusDetected => "已检测到",
            LocaleKey::CredsStatusNotDetected => "未检测到",
            LocaleKey::CredsStatusAvailable => "可用",
            LocaleKey::CredsStatusUnavailable => "不可用",
            LocaleKey::CredsOpenFolderAction => "打开凭据文件夹",
            LocaleKey::CredsRefreshDetectionAction => "刷新检测",
            LocaleKey::CredsSavePathAction => "保存路径",
            LocaleKey::CredsBrowseAction => "浏览…",
            LocaleKey::CredsGeminiCliLabel => "Gemini CLI",
            LocaleKey::CredsGeminiCliHelperPrefix => "使用的 OAuth 凭据来自",
            LocaleKey::CredsGeminiCliSetupAction => "安装 Gemini CLI",
            LocaleKey::CredsGeminiCliSetupHelp => {
                "安装 Gemini CLI 并运行 `gemini auth login` 进行登录。"
            }
            LocaleKey::CredsVertexAiLabel => "Google Cloud",
            LocaleKey::CredsVertexAiHelperPrefix => "使用的 Google Cloud 凭据来自",
            LocaleKey::CredsVertexAiSetupAction => "配置 Google Cloud 身份",
            LocaleKey::CredsVertexAiSetupHelp => {
                "运行 `gcloud auth application-default login` 创建凭据。"
            }
            LocaleKey::CredsJetBrainsLabel => "JetBrains IDE",
            LocaleKey::CredsJetBrainsHelperDetectedPrefix => "使用检测到的 IDE 配置：",
            LocaleKey::CredsJetBrainsHelperCustomPrefix => "使用自定义 IDE 基础路径：",
            LocaleKey::CredsJetBrainsHelperMissing => {
                "请安装启用了 AI Assistant 的 JetBrains IDE，然后刷新 CodexBar。"
            }
            LocaleKey::CredsJetBrainsCustomPathLabel => "自定义路径",
            LocaleKey::CredsJetBrainsCustomPathPlaceholder => "%APPDATA%/JetBrains/IntelliJIdea...",
            LocaleKey::CredsJetBrainsSelectLabel => "选择要监控的 JetBrains IDE。",
            LocaleKey::CredsJetBrainsAutoDetectOption => "自动检测",
            LocaleKey::CredsKiroLabel => "Kiro CLI",
            LocaleKey::CredsKiroHelperAvailablePrefix => "检测到于",
            LocaleKey::CredsKiroHelperMissing => "kiro-cli：未在 PATH 或常见安装位置找到。",
            LocaleKey::CredsOpenAiHistoryHelp => "启用历史跟踪以查看一段时间内的使用情况。",

            // Tauri desktop shell — Token accounts (Phase 6e, review)
            LocaleKey::TokenAccountActive => "活动",
            LocaleKey::TokenAccountSetActive => "设为活动",
            LocaleKey::TokenAccountRemove => "移除",
            LocaleKey::TokenAccountAddButton => "添加账户",
            LocaleKey::TokenAccountGithubLoginButton => "使用 GitHub 登录",
            LocaleKey::TokenAccountEmpty => "该服务商尚未保存任何账户。",
            LocaleKey::TokenAccountLabelPlaceholder => "标签（如工作、个人）…",
            LocaleKey::TokenAccountProviderLabel => "服务商",
            LocaleKey::TokenAccountProviderPlaceholder => "选择服务商…",
            LocaleKey::TokenAccountAddedPrefix => "添加于",
            LocaleKey::TokenAccountUsedPrefix => "上次使用",
            LocaleKey::TokenAccountTabHint => {
                "按服务商管理多个会话令牌或 API 令牌。所有数据拉取都会使用活动账户。仅需要手动令牌的服务商会显示在此处。"
            }
            LocaleKey::TokenAccountNoSupported => "当前没有支持令牌账户的服务商。",
            LocaleKey::TokenAccountInlineSummary => "令牌账户",

            // Phase 9 - Tray / pop-out pace badges + countdowns
            // REVIEW-i18n: short badge labels for usage pace categories
            LocaleKey::TrayPaceBadgeSlow => "缓慢",
            LocaleKey::TrayPaceBadgeSteady => "稳定",
            LocaleKey::TrayPaceBadgeRacing => "加速",
            LocaleKey::TrayPaceBadgeBurning => "超速",
            LocaleKey::TrayResetsInLabel => "{} 后重置",
            LocaleKey::TrayResetsDueNow => "正在重置…",
        }
    }
}

impl LocaleKey {
    /// Every variant of `LocaleKey` paired with its serialized name.
    ///
    /// Kept in sync with the `LocaleKey` enum above; used by the Tauri
    /// desktop shell to expose the full set of localized strings to the
    /// frontend via `get_locale_strings`.
    pub const ALL: &'static [(LocaleKey, &'static str)] = &[
        (LocaleKey::TabGeneral, "TabGeneral"),
        (LocaleKey::TabProviders, "TabProviders"),
        (LocaleKey::TabDisplay, "TabDisplay"),
        (LocaleKey::TabApiKeys, "TabApiKeys"),
        (LocaleKey::TabCookies, "TabCookies"),
        (LocaleKey::TabAdvanced, "TabAdvanced"),
        (LocaleKey::TabAbout, "TabAbout"),
        (LocaleKey::TabShortcuts, "TabShortcuts"),
        (LocaleKey::InterfaceLanguage, "InterfaceLanguage"),
        (LocaleKey::StartupSettings, "StartupSettings"),
        (LocaleKey::StartAtLogin, "StartAtLogin"),
        (LocaleKey::StartMinimized, "StartMinimized"),
        (LocaleKey::StartAtLoginHelper, "StartAtLoginHelper"),
        (LocaleKey::StartMinimizedHelper, "StartMinimizedHelper"),
        (
            LocaleKey::ShowNotificationsHelper,
            "ShowNotificationsHelper",
        ),
        (LocaleKey::SoundEnabledHelper, "SoundEnabledHelper"),
        (
            LocaleKey::HighUsageThresholdHelper,
            "HighUsageThresholdHelper",
        ),
        (
            LocaleKey::CriticalUsageThresholdHelper,
            "CriticalUsageThresholdHelper",
        ),
        (LocaleKey::ShowNotifications, "ShowNotifications"),
        (LocaleKey::SoundEnabled, "SoundEnabled"),
        (LocaleKey::SoundVolume, "SoundVolume"),
        (LocaleKey::HighUsageThreshold, "HighUsageThreshold"),
        (LocaleKey::HighUsageAlert, "HighUsageAlert"),
        (LocaleKey::CriticalUsageThreshold, "CriticalUsageThreshold"),
        (LocaleKey::CriticalUsageAlert, "CriticalUsageAlert"),
        (LocaleKey::UsageDisplay, "UsageDisplay"),
        (LocaleKey::ShowUsageAsUsed, "ShowUsageAsUsed"),
        (LocaleKey::ShowUsageAsUsedHelper, "ShowUsageAsUsedHelper"),
        (LocaleKey::ResetTimeRelative, "ResetTimeRelative"),
        (
            LocaleKey::ResetTimeRelativeHelper,
            "ResetTimeRelativeHelper",
        ),
        (LocaleKey::ShowCreditsExtra, "ShowCreditsExtra"),
        (LocaleKey::ShowCreditsExtraHelper, "ShowCreditsExtraHelper"),
        (LocaleKey::TrayIcon, "TrayIcon"),
        (LocaleKey::MergeTrayIcons, "MergeTrayIcons"),
        (LocaleKey::MergeTrayIconsHelper, "MergeTrayIconsHelper"),
        (LocaleKey::PerProviderTrayIcons, "PerProviderTrayIcons"),
        (
            LocaleKey::PerProviderTrayIconsHelper,
            "PerProviderTrayIconsHelper",
        ),
        (LocaleKey::ProviderEnabled, "ProviderEnabled"),
        (LocaleKey::ProviderDisabled, "ProviderDisabled"),
        (LocaleKey::ProviderInfo, "ProviderInfo"),
        (LocaleKey::ProviderUsage, "ProviderUsage"),
        (LocaleKey::AuthType, "AuthType"),
        (LocaleKey::DataSource, "DataSource"),
        (LocaleKey::ProviderNotDetected, "ProviderNotDetected"),
        (
            LocaleKey::ProviderLastFetchFailed,
            "ProviderLastFetchFailed",
        ),
        (
            LocaleKey::ProviderUsageNotFetchedYet,
            "ProviderUsageNotFetchedYet",
        ),
        (
            LocaleKey::ProviderNotFetchedYetTitle,
            "ProviderNotFetchedYetTitle",
        ),
        (
            LocaleKey::ProviderDisabledNoRecentData,
            "ProviderDisabledNoRecentData",
        ),
        (
            LocaleKey::ProviderSourceAutoShort,
            "ProviderSourceAutoShort",
        ),
        (LocaleKey::ProviderSourceWebShort, "ProviderSourceWebShort"),
        (LocaleKey::ProviderSourceCliShort, "ProviderSourceCliShort"),
        (
            LocaleKey::ProviderSourceOauthShort,
            "ProviderSourceOauthShort",
        ),
        (LocaleKey::ProviderSourceApiShort, "ProviderSourceApiShort"),
        (
            LocaleKey::ProviderSourceGithubApiShort,
            "ProviderSourceGithubApiShort",
        ),
        (
            LocaleKey::ProviderSourceLocalShort,
            "ProviderSourceLocalShort",
        ),
        (
            LocaleKey::ProviderSourceKiroEnvShort,
            "ProviderSourceKiroEnvShort",
        ),
        (LocaleKey::TrackingItem, "TrackingItem"),
        (
            LocaleKey::MainWindowLiveUsageData,
            "MainWindowLiveUsageData",
        ),
        (LocaleKey::StartTrackingUsage, "StartTrackingUsage"),
        (
            LocaleKey::ClickTrayIconForMetrics,
            "ClickTrayIconForMetrics",
        ),
        (LocaleKey::BrowserCookieImport, "BrowserCookieImport"),
        (LocaleKey::ImportFromBrowser, "ImportFromBrowser"),
        (
            LocaleKey::NoCookiesFoundInBrowser,
            "NoCookiesFoundInBrowser",
        ),
        (LocaleKey::SelectBrowser, "SelectBrowser"),
        (LocaleKey::ImportCookies, "ImportCookies"),
        (LocaleKey::ImportSuccess, "ImportSuccess"),
        (LocaleKey::ImportFailed, "ImportFailed"),
        (LocaleKey::SaveFailed, "SaveFailed"),
        (LocaleKey::CookiesAutoImport, "CookiesAutoImport"),
        (LocaleKey::QuickActions, "QuickActions"),
        (LocaleKey::OpenProviderDashboard, "OpenProviderDashboard"),
        (LocaleKey::OllamaNoDashboard, "OllamaNoDashboard"),
        (LocaleKey::ApiKeysTitle, "ApiKeysTitle"),
        (LocaleKey::ApiKeysDescription, "ApiKeysDescription"),
        (LocaleKey::AddKey, "AddKey"),
        (LocaleKey::KeySet, "KeySet"),
        (LocaleKey::KeyRequired, "KeyRequired"),
        (LocaleKey::Remove, "Remove"),
        (LocaleKey::GetKey, "GetKey"),
        (LocaleKey::SavedCookies, "SavedCookies"),
        (LocaleKey::AddManualCookie, "AddManualCookie"),
        (LocaleKey::CookieHeader, "CookieHeader"),
        (LocaleKey::PasteHere, "PasteHere"),
        (LocaleKey::DeleteCookie, "DeleteCookie"),
        (LocaleKey::CookieSaved, "CookieSaved"),
        (LocaleKey::CookieDeleted, "CookieDeleted"),
        (LocaleKey::RefreshSettings, "RefreshSettings"),
        (LocaleKey::Animations, "Animations"),
        (LocaleKey::MenuBar, "MenuBar"),
        (LocaleKey::Fun, "Fun"),
        (LocaleKey::GlobalShortcut, "GlobalShortcut"),
        (LocaleKey::Privacy, "Privacy"),
        (LocaleKey::Updates, "Updates"),
        (LocaleKey::UpdateChannel, "UpdateChannel"),
        (LocaleKey::UpdateChannelStable, "UpdateChannelStable"),
        (LocaleKey::UpdateChannelBeta, "UpdateChannelBeta"),
        (LocaleKey::Never, "Never"),
        (LocaleKey::LastUpdated, "LastUpdated"),
        (LocaleKey::NeverUpdated, "NeverUpdated"),
        (LocaleKey::MinutesAgo, "MinutesAgo"),
        (LocaleKey::HoursAgo, "HoursAgo"),
        (LocaleKey::DaysAgo, "DaysAgo"),
        (LocaleKey::BuiltWithRust, "BuiltWithRust"),
        (LocaleKey::OriginalMacOSVersion, "OriginalMacOSVersion"),
        (LocaleKey::Links, "Links"),
        (LocaleKey::BuildInfo, "BuildInfo"),
        (LocaleKey::EnabledProviders, "EnabledProviders"),
        (LocaleKey::Appearance, "Appearance"),
        (LocaleKey::ThemeSelection, "ThemeSelection"),
        (LocaleKey::LightMode, "LightMode"),
        (LocaleKey::DarkMode, "DarkMode"),
        (LocaleKey::AboutTitle, "AboutTitle"),
        (LocaleKey::Version, "Version"),
        (LocaleKey::ActionRefreshAll, "ActionRefreshAll"),
        (LocaleKey::ActionSettings, "ActionSettings"),
        (LocaleKey::ActionClose, "ActionClose"),
        (LocaleKey::ProviderAccount, "ProviderAccount"),
        (LocaleKey::ProviderSession, "ProviderSession"),
        (LocaleKey::ProviderWeekly, "ProviderWeekly"),
        (LocaleKey::ProviderMonthly, "ProviderMonthly"),
        (LocaleKey::ProviderModel, "ProviderModel"),
        (LocaleKey::ProviderPlan, "ProviderPlan"),
        (LocaleKey::ProviderNextReset, "ProviderNextReset"),
        (LocaleKey::ProviderNoRecentUsage, "ProviderNoRecentUsage"),
        (LocaleKey::ProviderNotSignedIn, "ProviderNotSignedIn"),
        (LocaleKey::SummaryTab, "SummaryTab"),
        (LocaleKey::StateLoadingProviders, "StateLoadingProviders"),
        (LocaleKey::StateNoProviderData, "StateNoProviderData"),
        (
            LocaleKey::StateNoProviderSelected,
            "StateNoProviderSelected",
        ),
        (
            LocaleKey::StateSummaryRefreshPending,
            "StateSummaryRefreshPending",
        ),
        (LocaleKey::StateError, "StateError"),
        (LocaleKey::StateRetry, "StateRetry"),
        (LocaleKey::StateDownload, "StateDownload"),
        (LocaleKey::StateRestartAndUpdate, "StateRestartAndUpdate"),
        (LocaleKey::CreditsTitle, "CreditsTitle"),
        (LocaleKey::UpdateRestartAndUpdate, "UpdateRestartAndUpdate"),
        (LocaleKey::UpdateRetry, "UpdateRetry"),
        (LocaleKey::UpdateDownload, "UpdateDownload"),
        (LocaleKey::UpdateDownloading, "UpdateDownloading"),
        (LocaleKey::UpdateReady, "UpdateReady"),
        (LocaleKey::UpdateFailed, "UpdateFailed"),
        (
            LocaleKey::ButtonOpenProviderSettings,
            "ButtonOpenProviderSettings",
        ),
        (LocaleKey::MenuSettings, "MenuSettings"),
        (LocaleKey::MenuAbout, "MenuAbout"),
        (LocaleKey::MenuQuit, "MenuQuit"),
        (LocaleKey::StatusJustUpdated, "StatusJustUpdated"),
        (LocaleKey::StatusUnableToGetUsage, "StatusUnableToGetUsage"),
        (LocaleKey::ActionRefresh, "ActionRefresh"),
        (LocaleKey::ActionSwitchAccount, "ActionSwitchAccount"),
        (LocaleKey::ActionUsageDashboard, "ActionUsageDashboard"),
        (LocaleKey::ActionStatusPage, "ActionStatusPage"),
        (LocaleKey::ActionCopyError, "ActionCopyError"),
        (LocaleKey::ActionBuyCredits, "ActionBuyCredits"),
        (LocaleKey::PaceOnTrack, "PaceOnTrack"),
        (LocaleKey::PaceBehind, "PaceBehind"),
        (LocaleKey::MetricResetsIn, "MetricResetsIn"),
        (LocaleKey::SectionUsageBreakdown, "SectionUsageBreakdown"),
        (LocaleKey::SectionCost, "SectionCost"),
        (LocaleKey::ResetInProgress, "ResetInProgress"),
        (LocaleKey::TomorrowAt, "TomorrowAt"),
        (LocaleKey::UsedPercent, "UsedPercent"),
        (LocaleKey::RemainingPercent, "RemainingPercent"),
        (LocaleKey::RemainingAmount, "RemainingAmount"),
        (LocaleKey::Tokens1K, "Tokens1K"),
        (LocaleKey::TodayCost, "TodayCost"),
        (LocaleKey::Last30DaysCost, "Last30DaysCost"),
        (LocaleKey::StatusLabel, "StatusLabel"),
        (LocaleKey::TrayOpenCodexBar, "TrayOpenCodexBar"),
        (LocaleKey::TrayPopOutDashboard, "TrayPopOutDashboard"),
        (LocaleKey::TrayRefreshAll, "TrayRefreshAll"),
        (LocaleKey::TrayProviders, "TrayProviders"),
        (LocaleKey::TraySettings, "TraySettings"),
        (LocaleKey::TrayCheckForUpdates, "TrayCheckForUpdates"),
        (LocaleKey::TrayQuit, "TrayQuit"),
        (LocaleKey::TrayLoading, "TrayLoading"),
        (LocaleKey::TrayNoProviders, "TrayNoProviders"),
        (LocaleKey::TraySessionPercent, "TraySessionPercent"),
        (LocaleKey::TrayWeeklyPercent, "TrayWeeklyPercent"),
        (LocaleKey::TrayStatusError, "TrayStatusError"),
        (LocaleKey::TrayStatusStale, "TrayStatusStale"),
        (LocaleKey::TrayStatusIncident, "TrayStatusIncident"),
        (LocaleKey::TrayStatusPartial, "TrayStatusPartial"),
        (LocaleKey::TrayWeeklyExhausted, "TrayWeeklyExhausted"),
        (LocaleKey::TrayCreditsRemaining, "TrayCreditsRemaining"),
        (LocaleKey::TrayStatusRowLoading, "TrayStatusRowLoading"),
        (LocaleKey::TrayStatusRowError, "TrayStatusRowError"),
        (LocaleKey::TrayCreditsRow, "TrayCreditsRow"),
        (LocaleKey::TrayProviderPopOut, "TrayProviderPopOut"),
        (LocaleKey::TrayProviderRefresh, "TrayProviderRefresh"),
        (LocaleKey::TrayProviderSettings, "TrayProviderSettings"),
        (LocaleKey::TrayProviderQuit, "TrayProviderQuit"),
        (LocaleKey::State, "State"),
        (LocaleKey::Source, "Source"),
        (LocaleKey::Updated, "Updated"),
        (LocaleKey::UpdatedJustNow, "UpdatedJustNow"),
        (LocaleKey::UpdatedMinutesAgo, "UpdatedMinutesAgo"),
        (LocaleKey::UpdatedHoursAgo, "UpdatedHoursAgo"),
        (LocaleKey::UpdatedDaysAgo, "UpdatedDaysAgo"),
        (LocaleKey::Status, "Status"),
        (LocaleKey::AllSystemsOperational, "AllSystemsOperational"),
        (LocaleKey::Plan, "Plan"),
        (LocaleKey::Account, "Account"),
        (LocaleKey::ProviderSessionLabel, "ProviderSessionLabel"),
        (LocaleKey::ProviderWeeklyLabel, "ProviderWeeklyLabel"),
        (
            LocaleKey::ProviderCodeReviewLabel,
            "ProviderCodeReviewLabel",
        ),
        (LocaleKey::ResetsInShort, "ResetsInShort"),
        (LocaleKey::ResetsInDaysHours, "ResetsInDaysHours"),
        (LocaleKey::ResetsInHoursMinutes, "ResetsInHoursMinutes"),
        (LocaleKey::TrayDisplayTitle, "TrayDisplayTitle"),
        (LocaleKey::ShowInTray, "ShowInTray"),
        (LocaleKey::CreditsLabel, "CreditsLabel"),
        (LocaleKey::CreditsLeft, "CreditsLeft"),
        (LocaleKey::CostTitle, "CostTitle"),
        (LocaleKey::TodayCostFull, "TodayCostFull"),
        (LocaleKey::Last30DaysCostFull, "Last30DaysCostFull"),
        (LocaleKey::ProviderSettingsTitle, "ProviderSettingsTitle"),
        (LocaleKey::ProviderAccountsTitle, "ProviderAccountsTitle"),
        (LocaleKey::ProviderOptionsTitle, "ProviderOptionsTitle"),
        (LocaleKey::MenuBarMetric, "MenuBarMetric"),
        (LocaleKey::MenuBarMetricHelper, "MenuBarMetricHelper"),
        (LocaleKey::UsageSource, "UsageSource"),
        (
            LocaleKey::ProviderNoCodexAccountsDetected,
            "ProviderNoCodexAccountsDetected",
        ),
        (
            LocaleKey::ProviderCodexAutoImportHelp,
            "ProviderCodexAutoImportHelp",
        ),
        (
            LocaleKey::ProviderCodexHistoryHelp,
            "ProviderCodexHistoryHelp",
        ),
        (LocaleKey::ProviderOpenAiCookies, "ProviderOpenAiCookies"),
        (
            LocaleKey::ProviderHistoricalTracking,
            "ProviderHistoricalTracking",
        ),
        (
            LocaleKey::ProviderOpenAiWebExtras,
            "ProviderOpenAiWebExtras",
        ),
        (
            LocaleKey::ProviderOpenAiWebExtrasHelp,
            "ProviderOpenAiWebExtrasHelp",
        ),
        (
            LocaleKey::ProviderCodexCreditsUnavailable,
            "ProviderCodexCreditsUnavailable",
        ),
        (
            LocaleKey::ProviderCodexLastFetchFailedTitle,
            "ProviderCodexLastFetchFailedTitle",
        ),
        (
            LocaleKey::ProviderCodexNotRunningHelp,
            "ProviderCodexNotRunningHelp",
        ),
        (LocaleKey::ProviderCookieSource, "ProviderCookieSource"),
        (LocaleKey::CookieSourceManual, "CookieSourceManual"),
        (LocaleKey::ProviderRegion, "ProviderRegion"),
        (LocaleKey::ProviderClaudeCookies, "ProviderClaudeCookies"),
        (
            LocaleKey::ProviderClaudeCookiesHelp,
            "ProviderClaudeCookiesHelp",
        ),
        (
            LocaleKey::ProviderClaudeAvoidKeychainPrompts,
            "ProviderClaudeAvoidKeychainPrompts",
        ),
        (
            LocaleKey::ProviderClaudeAvoidKeychainPromptsHelp,
            "ProviderClaudeAvoidKeychainPromptsHelp",
        ),
        (
            LocaleKey::ProviderCursorCookieSourceHelp,
            "ProviderCursorCookieSourceHelp",
        ),
        (
            LocaleKey::ProviderCursorCreditsHelp,
            "ProviderCursorCreditsHelp",
        ),
        (LocaleKey::AutoFallbackHelp, "AutoFallbackHelp"),
        (LocaleKey::ProviderSourceOauthWeb, "ProviderSourceOauthWeb"),
        (LocaleKey::Automatic, "Automatic"),
        (LocaleKey::Average, "Average"),
        (LocaleKey::ExtraUsage, "ExtraUsage"),
        (LocaleKey::OAuth, "OAuth"),
        (LocaleKey::Api, "Api"),
        (LocaleKey::Web, "Web"),
        (LocaleKey::PrivacyTitle, "PrivacyTitle"),
        (LocaleKey::HidePersonalInfo, "HidePersonalInfo"),
        (LocaleKey::HidePersonalInfoHelper, "HidePersonalInfoHelper"),
        (LocaleKey::UpdatesTitle, "UpdatesTitle"),
        (LocaleKey::UpdateChannelChoice, "UpdateChannelChoice"),
        (
            LocaleKey::UpdateChannelChoiceHelper,
            "UpdateChannelChoiceHelper",
        ),
        (LocaleKey::AutoDownloadUpdates, "AutoDownloadUpdates"),
        (
            LocaleKey::AutoDownloadUpdatesHelper,
            "AutoDownloadUpdatesHelper",
        ),
        (LocaleKey::InstallUpdatesOnQuit, "InstallUpdatesOnQuit"),
        (
            LocaleKey::InstallUpdatesOnQuitHelper,
            "InstallUpdatesOnQuitHelper",
        ),
        (LocaleKey::KeyboardShortcutsTitle, "KeyboardShortcutsTitle"),
        (LocaleKey::GlobalShortcutLabel, "GlobalShortcutLabel"),
        (LocaleKey::GlobalShortcutHelper, "GlobalShortcutHelper"),
        (LocaleKey::ShortcutFormatHint, "ShortcutFormatHint"),
        (LocaleKey::Saved, "Saved"),
        (LocaleKey::InvalidFormat, "InvalidFormat"),
        (
            LocaleKey::ShortcutHintPlaceholder,
            "ShortcutHintPlaceholder",
        ),
        (
            LocaleKey::SurpriseAnimationsHelper,
            "SurpriseAnimationsHelper",
        ),
        (LocaleKey::SelectProvider, "SelectProvider"),
        (LocaleKey::RefreshInterval30Sec, "RefreshInterval30Sec"),
        (LocaleKey::RefreshInterval1Min, "RefreshInterval1Min"),
        (LocaleKey::RefreshInterval5Min, "RefreshInterval5Min"),
        (LocaleKey::RefreshInterval10Min, "RefreshInterval10Min"),
        (LocaleKey::BrowserCookiesTitle, "BrowserCookiesTitle"),
        (LocaleKey::CookieImport, "CookieImport"),
        (LocaleKey::Provider, "Provider"),
        (LocaleKey::SelectPlaceholder, "SelectPlaceholder"),
        (LocaleKey::AutoRefreshInterval, "AutoRefreshInterval"),
        (LocaleKey::AboutDescription, "AboutDescription"),
        (LocaleKey::AboutDescriptionLine2, "AboutDescriptionLine2"),
        (LocaleKey::ViewOnGitHub, "ViewOnGitHub"),
        (LocaleKey::SubmitIssue, "SubmitIssue"),
        (LocaleKey::MaintainedBy, "MaintainedBy"),
        (LocaleKey::CommitLabel, "CommitLabel"),
        (LocaleKey::BuildDateLabel, "BuildDateLabel"),
        (LocaleKey::Save, "Save"),
        (LocaleKey::Cancel, "Cancel"),
        (LocaleKey::Label, "Label"),
        (LocaleKey::Token, "Token"),
        (LocaleKey::AddAccount, "AddAccount"),
        (LocaleKey::AccountAdded, "AccountAdded"),
        (LocaleKey::AccountRemoved, "AccountRemoved"),
        (LocaleKey::AccountSwitched, "AccountSwitched"),
        (LocaleKey::AccountLabelHint, "AccountLabelHint"),
        (LocaleKey::EnterApiKeyFor, "EnterApiKeyFor"),
        (LocaleKey::PasteApiKeyHere, "PasteApiKeyHere"),
        (LocaleKey::ApiKeySaved, "ApiKeySaved"),
        (LocaleKey::ApiKeyRemoved, "ApiKeyRemoved"),
        (LocaleKey::EnvironmentVariable, "EnvironmentVariable"),
        (LocaleKey::CookieSavedForProvider, "CookieSavedForProvider"),
        (
            LocaleKey::CookieRemovedForProvider,
            "CookieRemovedForProvider",
        ),
        (LocaleKey::ShowUsedPercent, "ShowUsedPercent"),
        (LocaleKey::ShowRemainingPercent, "ShowRemainingPercent"),
        (LocaleKey::UpdateAvailableMessage, "UpdateAvailableMessage"),
        (LocaleKey::UpdateReadyMessage, "UpdateReadyMessage"),
        (LocaleKey::UpdateFailedMessage, "UpdateFailedMessage"),
        (
            LocaleKey::UpdateDownloadingMessage,
            "UpdateDownloadingMessage",
        ),
        (LocaleKey::TabTokenAccounts, "TabTokenAccounts"),
        (LocaleKey::SectionRefresh, "SectionRefresh"),
        (LocaleKey::SectionNotifications, "SectionNotifications"),
        (LocaleKey::SectionUsageThresholds, "SectionUsageThresholds"),
        (LocaleKey::SectionKeyboard, "SectionKeyboard"),
        (LocaleKey::SectionUsageRendering, "SectionUsageRendering"),
        (LocaleKey::SectionTime, "SectionTime"),
        (LocaleKey::SectionLanguage, "SectionLanguage"),
        (
            LocaleKey::SectionCredentialsSecurity,
            "SectionCredentialsSecurity",
        ),
        (LocaleKey::SectionDebug, "SectionDebug"),
        (LocaleKey::SectionApiKeys, "SectionApiKeys"),
        (LocaleKey::SectionSavedCookies, "SectionSavedCookies"),
        (
            LocaleKey::SectionImportFromBrowser,
            "SectionImportFromBrowser",
        ),
        (
            LocaleKey::SectionAddCookieManually,
            "SectionAddCookieManually",
        ),
        (LocaleKey::SectionTokenAccounts, "SectionTokenAccounts"),
        (LocaleKey::SectionSavedAccounts, "SectionSavedAccounts"),
        (LocaleKey::SectionAddAccount, "SectionAddAccount"),
        (LocaleKey::RefreshIntervalLabel, "RefreshIntervalLabel"),
        (LocaleKey::RefreshIntervalHelper, "RefreshIntervalHelper"),
        (LocaleKey::SoundVolumeHelper, "SoundVolumeHelper"),
        (LocaleKey::HighUsageWarningHelper, "HighUsageWarningHelper"),
        (
            LocaleKey::CriticalUsageWarningHelper,
            "CriticalUsageWarningHelper",
        ),
        (
            LocaleKey::GlobalShortcutFieldLabel,
            "GlobalShortcutFieldLabel",
        ),
        (
            LocaleKey::GlobalShortcutToggleHelper,
            "GlobalShortcutToggleHelper",
        ),
        (LocaleKey::ShortcutRecordButton, "ShortcutRecordButton"),
        (LocaleKey::ShortcutRecordingLabel, "ShortcutRecordingLabel"),
        (LocaleKey::ShortcutRecordingHint, "ShortcutRecordingHint"),
        (LocaleKey::ShortcutClearButton, "ShortcutClearButton"),
        (
            LocaleKey::ShortcutEmptyPlaceholder,
            "ShortcutEmptyPlaceholder",
        ),
        (LocaleKey::NotificationTestSound, "NotificationTestSound"),
        (
            LocaleKey::NotificationTestSoundPlaying,
            "NotificationTestSoundPlaying",
        ),
        (LocaleKey::TrayIconModeLabel, "TrayIconModeLabel"),
        (LocaleKey::TrayIconModeHelper, "TrayIconModeHelper"),
        (LocaleKey::TrayIconModeSingle, "TrayIconModeSingle"),
        (
            LocaleKey::TrayIconModePerProvider,
            "TrayIconModePerProvider",
        ),
        (LocaleKey::ShowProviderIcons, "ShowProviderIcons"),
        (
            LocaleKey::ShowProviderIconsHelper,
            "ShowProviderIconsHelper",
        ),
        (LocaleKey::PreferHighestUsage, "PreferHighestUsage"),
        (
            LocaleKey::PreferHighestUsageHelper,
            "PreferHighestUsageHelper",
        ),
        (LocaleKey::ShowPercentInTray, "ShowPercentInTray"),
        (
            LocaleKey::ShowPercentInTrayHelper,
            "ShowPercentInTrayHelper",
        ),
        (LocaleKey::DisplayModeLabel, "DisplayModeLabel"),
        (LocaleKey::DisplayModeHelper, "DisplayModeHelper"),
        (LocaleKey::DisplayModeDetailed, "DisplayModeDetailed"),
        (LocaleKey::DisplayModeCompact, "DisplayModeCompact"),
        (LocaleKey::DisplayModeMinimal, "DisplayModeMinimal"),
        (LocaleKey::ShowAsUsedLabel, "ShowAsUsedLabel"),
        (LocaleKey::ShowAsUsedHelper, "ShowAsUsedHelper"),
        (
            LocaleKey::ShowAllTokenAccountsLabel,
            "ShowAllTokenAccountsLabel",
        ),
        (
            LocaleKey::ShowAllTokenAccountsHelper,
            "ShowAllTokenAccountsHelper",
        ),
        (LocaleKey::EnableAnimationsLabel, "EnableAnimationsLabel"),
        (LocaleKey::EnableAnimationsHelper, "EnableAnimationsHelper"),
        (
            LocaleKey::SurpriseAnimationsLabel,
            "SurpriseAnimationsLabel",
        ),
        (
            LocaleKey::UpdateChannelStableOption,
            "UpdateChannelStableOption",
        ),
        (
            LocaleKey::UpdateChannelBetaOption,
            "UpdateChannelBetaOption",
        ),
        (
            LocaleKey::AvoidKeychainPromptsLabel,
            "AvoidKeychainPromptsLabel",
        ),
        (
            LocaleKey::AvoidKeychainPromptsHelper,
            "AvoidKeychainPromptsHelper",
        ),
        (
            LocaleKey::DisableAllKeychainLabel,
            "DisableAllKeychainLabel",
        ),
        (
            LocaleKey::DisableAllKeychainHelper,
            "DisableAllKeychainHelper",
        ),
        (LocaleKey::ShowDebugSettingsLabel, "ShowDebugSettingsLabel"),
        (
            LocaleKey::ShowDebugSettingsHelper,
            "ShowDebugSettingsHelper",
        ),
        (LocaleKey::LanguageEnglishOption, "LanguageEnglishOption"),
        (LocaleKey::LanguageChineseOption, "LanguageChineseOption"),
        (LocaleKey::SectionTheme, "SectionTheme"),
        (LocaleKey::ThemeLabel, "ThemeLabel"),
        (LocaleKey::ThemeHelper, "ThemeHelper"),
        (LocaleKey::ThemeAutoOption, "ThemeAutoOption"),
        (LocaleKey::ThemeLightOption, "ThemeLightOption"),
        (LocaleKey::ThemeDarkOption, "ThemeDarkOption"),
        (LocaleKey::SettingsStatusSaving, "SettingsStatusSaving"),
        (LocaleKey::ApiKeysTabHint, "ApiKeysTabHint"),
        (LocaleKey::FetchingProviderData, "FetchingProviderData"),
        (LocaleKey::NoProvidersConfigured, "NoProvidersConfigured"),
        (LocaleKey::EnableProvidersHint, "EnableProvidersHint"),
        (LocaleKey::OpenSettingsButton, "OpenSettingsButton"),
        (LocaleKey::TooltipRefresh, "TooltipRefresh"),
        (LocaleKey::TooltipSettings, "TooltipSettings"),
        (LocaleKey::TooltipPopOut, "TooltipPopOut"),
        (LocaleKey::TooltipBackToTray, "TooltipBackToTray"),
        (LocaleKey::TrayCardErrorBadge, "TrayCardErrorBadge"),
        (LocaleKey::SummaryProvidersLabel, "SummaryProvidersLabel"),
        (LocaleKey::SummaryRefreshing, "SummaryRefreshing"),
        (LocaleKey::SummaryFailed, "SummaryFailed"),
        (LocaleKey::SummaryWithErrors, "SummaryWithErrors"),
        (LocaleKey::DetailBackButton, "DetailBackButton"),
        (LocaleKey::DetailWindowPrimary, "DetailWindowPrimary"),
        (LocaleKey::DetailWindowSecondary, "DetailWindowSecondary"),
        (
            LocaleKey::DetailWindowModelSpecific,
            "DetailWindowModelSpecific",
        ),
        (LocaleKey::DetailWindowTertiary, "DetailWindowTertiary"),
        (
            LocaleKey::DetailWindowMinutesSuffix,
            "DetailWindowMinutesSuffix",
        ),
        (LocaleKey::DetailWindowExhausted, "DetailWindowExhausted"),
        (LocaleKey::DetailPaceTitle, "DetailPaceTitle"),
        (LocaleKey::DetailPaceOnTrack, "DetailPaceOnTrack"),
        (
            LocaleKey::DetailPaceSlightlyAhead,
            "DetailPaceSlightlyAhead",
        ),
        (LocaleKey::DetailPaceAhead, "DetailPaceAhead"),
        (LocaleKey::DetailPaceFarAhead, "DetailPaceFarAhead"),
        (
            LocaleKey::DetailPaceSlightlyBehind,
            "DetailPaceSlightlyBehind",
        ),
        (LocaleKey::DetailPaceBehind, "DetailPaceBehind"),
        (LocaleKey::DetailPaceFarBehind, "DetailPaceFarBehind"),
        (LocaleKey::DetailPaceRunsOutIn, "DetailPaceRunsOutIn"),
        (
            LocaleKey::DetailPaceWillLastToReset,
            "DetailPaceWillLastToReset",
        ),
        (LocaleKey::DetailCostTitle, "DetailCostTitle"),
        (LocaleKey::DetailCostUsed, "DetailCostUsed"),
        (LocaleKey::DetailCostLimit, "DetailCostLimit"),
        (LocaleKey::DetailCostRemaining, "DetailCostRemaining"),
        (LocaleKey::DetailCostResets, "DetailCostResets"),
        (LocaleKey::DetailChartCost, "DetailChartCost"),
        (LocaleKey::DetailChartCredits, "DetailChartCredits"),
        (
            LocaleKey::DetailChartUsageBreakdown,
            "DetailChartUsageBreakdown",
        ),
        (LocaleKey::DetailChartEmpty, "DetailChartEmpty"),
        (LocaleKey::DetailUpdatedPrefix, "DetailUpdatedPrefix"),
        (
            LocaleKey::BannerCheckingForUpdates,
            "BannerCheckingForUpdates",
        ),
        (
            LocaleKey::BannerUpdateAvailablePrefix,
            "BannerUpdateAvailablePrefix",
        ),
        (LocaleKey::BannerDownloadButton, "BannerDownloadButton"),
        (LocaleKey::BannerViewRelease, "BannerViewRelease"),
        (LocaleKey::BannerDismiss, "BannerDismiss"),
        (
            LocaleKey::BannerDownloadingPrefix,
            "BannerDownloadingPrefix",
        ),
        (
            LocaleKey::BannerReadyToInstallSuffix,
            "BannerReadyToInstallSuffix",
        ),
        (LocaleKey::BannerInstallRestart, "BannerInstallRestart"),
        (
            LocaleKey::BannerUpdateFailedPrefix,
            "BannerUpdateFailedPrefix",
        ),
        (LocaleKey::BannerRetry, "BannerRetry"),
        (
            LocaleKey::ProviderSidebarReorderHint,
            "ProviderSidebarReorderHint",
        ),
        (LocaleKey::ProviderStatusOk, "ProviderStatusOk"),
        (LocaleKey::ProviderStatusStale, "ProviderStatusStale"),
        (LocaleKey::ProviderStatusError, "ProviderStatusError"),
        (LocaleKey::ProviderStatusLoading, "ProviderStatusLoading"),
        (LocaleKey::ProviderStatusDisabled, "ProviderStatusDisabled"),
        (
            LocaleKey::ProviderDetailPlaceholder,
            "ProviderDetailPlaceholder",
        ),
        // Phase 6d — credential detection
        (
            LocaleKey::CredentialsSectionTitle,
            "CredentialsSectionTitle",
        ),
        (
            LocaleKey::CredsStatusAuthenticated,
            "CredsStatusAuthenticated",
        ),
        (LocaleKey::CredsStatusNotSignedIn, "CredsStatusNotSignedIn"),
        (LocaleKey::CredsStatusDetected, "CredsStatusDetected"),
        (LocaleKey::CredsStatusNotDetected, "CredsStatusNotDetected"),
        (LocaleKey::CredsStatusAvailable, "CredsStatusAvailable"),
        (LocaleKey::CredsStatusUnavailable, "CredsStatusUnavailable"),
        (LocaleKey::CredsOpenFolderAction, "CredsOpenFolderAction"),
        (
            LocaleKey::CredsRefreshDetectionAction,
            "CredsRefreshDetectionAction",
        ),
        (LocaleKey::CredsSavePathAction, "CredsSavePathAction"),
        (LocaleKey::CredsBrowseAction, "CredsBrowseAction"),
        (LocaleKey::CredsGeminiCliLabel, "CredsGeminiCliLabel"),
        (
            LocaleKey::CredsGeminiCliHelperPrefix,
            "CredsGeminiCliHelperPrefix",
        ),
        (
            LocaleKey::CredsGeminiCliSetupAction,
            "CredsGeminiCliSetupAction",
        ),
        (
            LocaleKey::CredsGeminiCliSetupHelp,
            "CredsGeminiCliSetupHelp",
        ),
        (LocaleKey::CredsVertexAiLabel, "CredsVertexAiLabel"),
        (
            LocaleKey::CredsVertexAiHelperPrefix,
            "CredsVertexAiHelperPrefix",
        ),
        (
            LocaleKey::CredsVertexAiSetupAction,
            "CredsVertexAiSetupAction",
        ),
        (LocaleKey::CredsVertexAiSetupHelp, "CredsVertexAiSetupHelp"),
        (LocaleKey::CredsJetBrainsLabel, "CredsJetBrainsLabel"),
        (
            LocaleKey::CredsJetBrainsHelperDetectedPrefix,
            "CredsJetBrainsHelperDetectedPrefix",
        ),
        (
            LocaleKey::CredsJetBrainsHelperCustomPrefix,
            "CredsJetBrainsHelperCustomPrefix",
        ),
        (
            LocaleKey::CredsJetBrainsHelperMissing,
            "CredsJetBrainsHelperMissing",
        ),
        (
            LocaleKey::CredsJetBrainsCustomPathLabel,
            "CredsJetBrainsCustomPathLabel",
        ),
        (
            LocaleKey::CredsJetBrainsCustomPathPlaceholder,
            "CredsJetBrainsCustomPathPlaceholder",
        ),
        (
            LocaleKey::CredsJetBrainsSelectLabel,
            "CredsJetBrainsSelectLabel",
        ),
        (
            LocaleKey::CredsJetBrainsAutoDetectOption,
            "CredsJetBrainsAutoDetectOption",
        ),
        (LocaleKey::CredsKiroLabel, "CredsKiroLabel"),
        (
            LocaleKey::CredsKiroHelperAvailablePrefix,
            "CredsKiroHelperAvailablePrefix",
        ),
        (LocaleKey::CredsKiroHelperMissing, "CredsKiroHelperMissing"),
        (LocaleKey::CredsOpenAiHistoryHelp, "CredsOpenAiHistoryHelp"),
        // Phase 6e — Token accounts (review)
        (LocaleKey::TokenAccountActive, "TokenAccountActive"),
        (LocaleKey::TokenAccountSetActive, "TokenAccountSetActive"),
        (LocaleKey::TokenAccountRemove, "TokenAccountRemove"),
        (LocaleKey::TokenAccountAddButton, "TokenAccountAddButton"),
        (
            LocaleKey::TokenAccountGithubLoginButton,
            "TokenAccountGithubLoginButton",
        ),
        (LocaleKey::TokenAccountEmpty, "TokenAccountEmpty"),
        (
            LocaleKey::TokenAccountLabelPlaceholder,
            "TokenAccountLabelPlaceholder",
        ),
        (
            LocaleKey::TokenAccountProviderLabel,
            "TokenAccountProviderLabel",
        ),
        (
            LocaleKey::TokenAccountProviderPlaceholder,
            "TokenAccountProviderPlaceholder",
        ),
        (
            LocaleKey::TokenAccountAddedPrefix,
            "TokenAccountAddedPrefix",
        ),
        (LocaleKey::TokenAccountUsedPrefix, "TokenAccountUsedPrefix"),
        (LocaleKey::TokenAccountTabHint, "TokenAccountTabHint"),
        (
            LocaleKey::TokenAccountNoSupported,
            "TokenAccountNoSupported",
        ),
        (
            LocaleKey::TokenAccountInlineSummary,
            "TokenAccountInlineSummary",
        ),
        // Phase 9 - Tray / pop-out pace badges + countdowns
        (LocaleKey::TrayPaceBadgeSlow, "TrayPaceBadgeSlow"),
        (LocaleKey::TrayPaceBadgeSteady, "TrayPaceBadgeSteady"),
        (LocaleKey::TrayPaceBadgeRacing, "TrayPaceBadgeRacing"),
        (LocaleKey::TrayPaceBadgeBurning, "TrayPaceBadgeBurning"),
        (LocaleKey::TrayResetsInLabel, "TrayResetsInLabel"),
        (LocaleKey::TrayResetsDueNow, "TrayResetsDueNow"),
    ];
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_locale_key_english() {
        assert_eq!(
            get_text(Language::English, LocaleKey::TabGeneral),
            "General"
        );
        assert_eq!(
            get_text(Language::English, LocaleKey::InterfaceLanguage),
            "Interface Language"
        );
        assert_eq!(
            get_text(Language::English, LocaleKey::StartAtLogin),
            "Start at Login"
        );
    }

    #[test]
    fn test_locale_key_chinese() {
        assert_eq!(get_text(Language::Chinese, LocaleKey::TabGeneral), "通用");
        assert_eq!(get_text(Language::Chinese, LocaleKey::TabCookies), "Cookie");
        assert_eq!(
            get_text(Language::Chinese, LocaleKey::InterfaceLanguage),
            "界面语言"
        );
        assert_eq!(
            get_text(Language::Chinese, LocaleKey::StartAtLogin),
            "开机启动"
        );
    }

    #[test]
    fn test_locale_respects_language_setting() {
        // Test that English language returns English strings
        let lang = Language::English;
        assert_eq!(get_text(lang, LocaleKey::TabAbout), "About");

        // Test that Chinese language returns Chinese strings
        let lang = Language::Chinese;
        assert_eq!(get_text(lang, LocaleKey::TabAbout), "关于");
    }

    #[test]
    fn test_all_locale_keys_have_both_languages() {
        // Verify all variants have both English and Chinese
        let keys = [
            // Tab names
            LocaleKey::TabGeneral,
            LocaleKey::TabProviders,
            LocaleKey::TabDisplay,
            LocaleKey::TabApiKeys,
            LocaleKey::TabCookies,
            LocaleKey::TabAdvanced,
            LocaleKey::TabAbout,
            LocaleKey::TabShortcuts,
            // General settings
            LocaleKey::InterfaceLanguage,
            LocaleKey::StartupSettings,
            LocaleKey::StartAtLogin,
            LocaleKey::StartMinimized,
            // Display settings
            LocaleKey::ShowNotifications,
            LocaleKey::HighUsageThreshold,
            LocaleKey::CriticalUsageThreshold,
            LocaleKey::ShowUsageAsUsed,
            // About
            LocaleKey::AboutTitle,
            LocaleKey::Version,
            // Main popup - Header actions
            LocaleKey::ActionRefreshAll,
            LocaleKey::ActionSettings,
            LocaleKey::ActionClose,
            // Main popup - Provider section
            LocaleKey::ProviderAccount,
            LocaleKey::ProviderSession,
            LocaleKey::ProviderWeekly,
            LocaleKey::ProviderModel,
            LocaleKey::ProviderPlan,
            LocaleKey::ProviderNextReset,
            LocaleKey::ProviderNoRecentUsage,
            LocaleKey::ProviderNotSignedIn,
            LocaleKey::SummaryTab,
            // Main popup - Loading/Empty/Error states
            LocaleKey::StateLoadingProviders,
            LocaleKey::StateNoProviderData,
            LocaleKey::StateNoProviderSelected,
            LocaleKey::StateSummaryRefreshPending,
            LocaleKey::StateError,
            LocaleKey::StateRetry,
            LocaleKey::StateDownload,
            LocaleKey::StateRestartAndUpdate,
            // Main popup - Credits
            LocaleKey::CreditsTitle,
            // Main popup - Update banner (non-happy-path)
            LocaleKey::UpdateRestartAndUpdate,
            LocaleKey::UpdateRetry,
            LocaleKey::UpdateDownload,
            LocaleKey::UpdateDownloading,
            LocaleKey::UpdateReady,
            LocaleKey::UpdateFailed,
            // Main popup - Settings button
            LocaleKey::ButtonOpenProviderSettings,
            // Main popup - Bottom menu (Actions)
            LocaleKey::MenuSettings,
            LocaleKey::MenuAbout,
            LocaleKey::MenuQuit,
            // Main popup - Status strings
            LocaleKey::StatusJustUpdated,
            LocaleKey::StatusUnableToGetUsage,
            // Main popup - Provider detail actions
            LocaleKey::ActionRefresh,
            LocaleKey::ActionSwitchAccount,
            LocaleKey::ActionUsageDashboard,
            LocaleKey::ActionStatusPage,
            LocaleKey::ActionCopyError,
            LocaleKey::ActionBuyCredits,
            // Main popup - Pace status
            LocaleKey::PaceOnTrack,
            LocaleKey::PaceBehind,
            // Main popup - Reset prefix
            LocaleKey::MetricResetsIn,
            // Main popup - Section titles
            LocaleKey::SectionUsageBreakdown,
            LocaleKey::SectionCost,
            // Main popup - Usage/reset labels
            LocaleKey::ResetInProgress,
            LocaleKey::TomorrowAt,
            LocaleKey::UsedPercent,
            LocaleKey::RemainingPercent,
            LocaleKey::RemainingAmount,
            LocaleKey::Tokens1K,
            LocaleKey::TodayCost,
            LocaleKey::Last30DaysCost,
            LocaleKey::StatusLabel,
            // Main popup - Update banner messages
            LocaleKey::UpdateAvailableMessage,
            LocaleKey::UpdateReadyMessage,
            LocaleKey::UpdateFailedMessage,
            LocaleKey::UpdateDownloadingMessage,
        ];

        for key in keys {
            // English should not be empty
            let english = key.english();
            assert!(!english.is_empty(), "English string for {:?} is empty", key);

            // Chinese should not be empty
            let chinese = key.chinese();
            assert!(!chinese.is_empty(), "Chinese string for {:?} is empty", key);
        }
    }
}
