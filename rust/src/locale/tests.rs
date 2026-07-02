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
fn test_locale_key_japanese() {
    assert_eq!(get_text(Language::Japanese, LocaleKey::TabGeneral), "一般");
    assert_eq!(
        get_text(Language::Japanese, LocaleKey::InterfaceLanguage),
        "表示言語"
    );
    assert_eq!(
        get_text(Language::Japanese, LocaleKey::StartAtLogin),
        "ログイン時に起動"
    );
}

#[test]
fn test_locale_key_spanish() {
    assert_eq!(
        get_text(Language::Spanish, LocaleKey::TabGeneral),
        "General"
    );
    assert_eq!(
        get_text(Language::Spanish, LocaleKey::InterfaceLanguage),
        "Idioma de la interfaz"
    );
    assert_eq!(
        get_text(Language::Spanish, LocaleKey::StartAtLogin),
        "Iniciar al arrancar"
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

    // Test that Japanese language returns Japanese strings
    let lang = Language::Japanese;
    assert_eq!(get_text(lang, LocaleKey::TabAbout), "情報");

    // Test that Spanish language returns Spanish strings
    let lang = Language::Spanish;
    assert_eq!(get_text(lang, LocaleKey::TabAbout), "Acerca de");
}

#[test]
fn test_all_locale_keys_have_all_languages() {
    // Verify all sampled variants have English, Chinese, Japanese, and Spanish coverage.
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

        // Japanese should not be empty
        let japanese = key.japanese();
        assert!(
            !japanese.is_empty(),
            "Japanese string for {:?} is empty",
            key
        );

        // Spanish should not be empty
        let spanish = key.spanish();
        assert!(!spanish.is_empty(), "Spanish string for {:?} is empty", key);
    }
}
