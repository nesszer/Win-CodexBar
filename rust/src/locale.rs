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
        Language::Japanese => key.japanese(),
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
    LanguageJapaneseOption,

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
    ProviderSidebarSearch,
    ProviderSidebarClearSearch,
    ProviderSidebarNoMatches,
    ProviderSidebarReorderHint,
    ProviderSidebarMoveUp,
    ProviderSidebarMoveDown,
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

mod chinese;
mod english;
mod japanese;
mod keys;

#[cfg(test)]
mod tests;
