use super::*;

impl LocaleKey {
    pub(super) fn english(self) -> &'static str {
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
            LocaleKey::LanguageJapaneseOption => "日本語",

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
            LocaleKey::DetailPaceRunsOutIn => "Runs out in",
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
            LocaleKey::ProviderSidebarSearch => "Search",
            LocaleKey::ProviderSidebarClearSearch => "Clear provider search",
            LocaleKey::ProviderSidebarNoMatches => "No matching providers",
            LocaleKey::ProviderSidebarReorderHint => "Drag to reorder",
            LocaleKey::ProviderSidebarMoveUp => "Move up",
            LocaleKey::ProviderSidebarMoveDown => "Move down",
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
}
