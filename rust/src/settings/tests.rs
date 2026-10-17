use super::*;

#[test]
fn test_settings_default() {
    let settings = Settings::default();
    assert_eq!(settings.preferred_currency_code, "AUTO");
    assert!(settings.enabled_providers.contains("claude"));
    assert!(settings.enabled_providers.contains("codex"));
    assert_eq!(settings.refresh_interval_secs, 300);
    assert!(settings.show_notifications);
    assert_eq!(
        settings.notification_sound_paths,
        NotificationSoundPaths::default()
    );
    assert_eq!(
        settings.notification_sound_theme,
        NotificationSoundTheme::Windows
    );
    assert_eq!(settings.high_usage_threshold, 70.0);
    assert_eq!(settings.critical_usage_threshold, 90.0);
    assert!(!settings.show_reset_when_exhausted);
    assert!(!settings.predictive_pace_warning_enabled);
    assert!(!settings.credential_expiry_notifications_enabled);
    assert!(settings.session_quota_notifications_enabled);
    assert!(settings.adaptive_refresh);
    assert!(!settings.float_bar_show_cost);
    assert!(!settings.tray_panel_always_on_top);
    assert_eq!(settings.overview_layout, "detailed");
    assert!(settings.promote_tray_icon);
    assert!(settings.claude_daily_routines_usage_visible);
    assert!(!settings.claude_allow_reading_claude_code_credentials);
    assert_eq!(
        settings.low_power_mode_preference,
        LowPowerModePreference::Off
    );
}

#[test]
fn kimi_cookie_source_defaults_to_automatic_discovery() {
    let settings = Settings::default();
    assert_eq!(settings.cookie_source(ProviderId::Kimi), "auto");
    assert_eq!(settings.cookie_source(ProviderId::Claude), "manual");
}

#[test]
fn hyper_cookie_source_defaults_to_automatic_session_import() {
    // Upstream resolves `hyperCookieSource` with an `.auto` fallback.
    let mut settings = Settings::default();
    assert_eq!(settings.cookie_source(ProviderId::Hyper), "auto");
    settings.set_cookie_source(ProviderId::Hyper, "off");
    assert_eq!(settings.cookie_source(ProviderId::Hyper), "off");
}

#[test]
fn groq_cookie_source_defaults_to_automatic_session_import() {
    // Upstream resolves `groqCookieSource` with an `.auto` fallback.
    let mut settings = Settings::default();
    assert_eq!(settings.cookie_source(ProviderId::Groq), "auto");
    settings.set_cookie_source(ProviderId::Groq, "manual");
    assert_eq!(settings.cookie_source(ProviderId::Groq), "manual");
}

#[test]
fn langdock_cookie_source_defaults_to_automatic_session_import() {
    // Upstream has no Langdock source picker (it reads one Edge profile), so
    // Windows defaults to automatic browser import.
    let mut settings = Settings::default();
    assert_eq!(settings.cookie_source(ProviderId::Langdock), "auto");
    settings.set_cookie_source(ProviderId::Langdock, "off");
    assert_eq!(settings.cookie_source(ProviderId::Langdock), "off");
}

#[test]
fn lithosai_cookie_source_defaults_to_automatic_session_import() {
    // Upstream's LithosAI cookie source falls back to `.auto`.
    let mut settings = Settings::default();
    assert_eq!(settings.cookie_source(ProviderId::LithosAI), "auto");
    settings.set_cookie_source(ProviderId::LithosAI, "off");
    assert_eq!(settings.cookie_source(ProviderId::LithosAI), "off");
}

#[test]
fn museai_cookie_source_defaults_to_automatic_session_import() {
    // Upstream's Muse (muse.ai) cookie source falls back to `.auto`.
    let mut settings = Settings::default();
    assert_eq!(settings.cookie_source(ProviderId::MuseAI), "auto");
    settings.set_cookie_source(ProviderId::MuseAI, "off");
    assert_eq!(settings.cookie_source(ProviderId::MuseAI), "off");
}

#[test]
fn workbuddy_cookie_source_defaults_to_automatic_session_import() {
    // Upstream's WorkBuddy cookie source falls back to `.auto`.
    let mut settings = Settings::default();
    assert_eq!(settings.cookie_source(ProviderId::WorkBuddy), "auto");
    settings.set_cookie_source(ProviderId::WorkBuddy, "off");
    assert_eq!(settings.cookie_source(ProviderId::WorkBuddy), "off");
}

#[test]
fn tinyapi_cookie_source_defaults_to_automatic_session_import() {
    // Upstream's TinyApi cookie source falls back to `.auto`.
    let mut settings = Settings::default();
    assert_eq!(settings.cookie_source(ProviderId::TinyApi), "auto");
    settings.set_cookie_source(ProviderId::TinyApi, "off");
    assert_eq!(settings.cookie_source(ProviderId::TinyApi), "off");
}

#[test]
fn xapi_cookie_source_defaults_to_automatic_session_import() {
    // X API cookie source falls back to `.auto`.
    let mut settings = Settings::default();
    assert_eq!(settings.cookie_source(ProviderId::Xapi), "auto");
    settings.set_cookie_source(ProviderId::Xapi, "off");
    assert_eq!(settings.cookie_source(ProviderId::Xapi), "off");
}

#[test]
fn parallel_cookie_source_defaults_to_automatic_session_import() {
    // Parallel's browser session falls back to `.auto` like X API.
    let mut settings = Settings::default();
    assert_eq!(settings.cookie_source(ProviderId::Parallel), "auto");
    settings.set_cookie_source(ProviderId::Parallel, "off");
    assert_eq!(settings.cookie_source(ProviderId::Parallel), "off");
}

#[test]
fn preferred_currency_defaults_validates_and_round_trips() {
    let legacy: Settings = serde_json::from_str(r#"{"enabled_providers": []}"#)
        .expect("legacy settings without a preferred currency remain valid");
    assert_eq!(legacy.preferred_currency_code, "AUTO");

    let selected: Settings =
        serde_json::from_str(r#"{"enabled_providers": [], "preferred_currency_code": "try"}"#)
            .expect("supported currency loads");
    assert_eq!(selected.preferred_currency_code, "TRY");
    let encoded = serde_json::to_string(&selected).expect("serialize selected currency");
    assert!(encoded.contains(r#""preferred_currency_code":"TRY""#));

    let invalid: Settings =
        serde_json::from_str(r#"{"enabled_providers": [], "preferred_currency_code": "BTC"}"#)
            .expect("unknown currency is normalized safely");
    assert_eq!(invalid.preferred_currency_code, "AUTO");
}

#[test]
fn overview_layout_defaults_to_detailed_and_round_trips() {
    let defaulted: Settings = serde_json::from_str(r#"{ "enabled_providers": [] }"#)
        .expect("missing overview layout defaults to detailed");
    assert_eq!(defaulted.overview_layout, "detailed");

    let compact = Settings {
        overview_layout: "compact".to_string(),
        ..Settings::default()
    };
    let json = serde_json::to_string(&compact).expect("serialize overview layout");
    let loaded: Settings = serde_json::from_str(&json).expect("deserialize overview layout");
    assert_eq!(loaded.overview_layout, "compact");

    let unknown: Settings =
        serde_json::from_str(r#"{ "enabled_providers": [], "overview_layout": "unsupported" }"#)
            .expect("unknown overview layout is accepted and normalized");
    assert_eq!(unknown.overview_layout, "detailed");
}

#[test]
fn tray_panel_always_on_top_defaults_off_and_round_trips() {
    let defaulted: Settings = serde_json::from_str(r#"{ "enabled_providers": [] }"#)
        .expect("missing tray panel topmost field defaults off");
    assert!(!defaulted.tray_panel_always_on_top);

    let enabled = Settings {
        tray_panel_always_on_top: true,
        ..Settings::default()
    };
    let json = serde_json::to_string(&enabled).expect("serialize tray panel topmost setting");
    assert!(json.contains(r#""tray_panel_always_on_top":true"#));

    let loaded: Settings =
        serde_json::from_str(&json).expect("deserialize tray panel topmost setting");
    assert!(loaded.tray_panel_always_on_top);
}

#[test]
fn low_power_mode_migrates_legacy_boolean_and_round_trips_preference() {
    let defaulted: Settings = serde_json::from_str(r#"{ "enabled_providers": [] }"#)
        .expect("missing low power fields defaults off");
    assert_eq!(
        defaulted.low_power_mode_preference,
        LowPowerModePreference::Off
    );

    let legacy: Settings =
        serde_json::from_str(r#"{ "enabled_providers": [], "low_power_mode": true }"#)
            .expect("legacy low_power_mode migrates");
    assert_eq!(legacy.low_power_mode_preference, LowPowerModePreference::On);

    let automatic = Settings {
        low_power_mode_preference: LowPowerModePreference::Automatic,
        ..Settings::default()
    };
    let json = serde_json::to_string(&automatic).expect("serialize low power preference");
    assert!(json.contains(r#""low_power_mode_preference":"automatic""#));
    let loaded: Settings = serde_json::from_str(&json).expect("deserialize low power preference");
    assert_eq!(
        loaded.low_power_mode_preference,
        LowPowerModePreference::Automatic
    );
}

#[test]
fn open_codex_usage_logs_default_off_and_round_trip() {
    let defaulted: Settings = serde_json::from_str(r#"{ "enabled_providers": [] }"#)
        .expect("missing open_codex_usage_logs_enabled defaults false");
    assert!(!defaulted.open_codex_usage_logs_enabled);

    let enabled = Settings {
        open_codex_usage_logs_enabled: true,
        hide_native_codex_cost_when_open_codex_present: true,
        ..Settings::default()
    };
    let json = serde_json::to_string(&enabled).expect("serialize OpenCodex usage opt-in");
    assert!(json.contains(r#""open_codex_usage_logs_enabled":true"#));

    let loaded: Settings = serde_json::from_str(&json).expect("deserialize OpenCodex usage opt-in");
    assert!(loaded.open_codex_usage_logs_enabled);
    assert!(loaded.hide_native_codex_cost_when_open_codex_present);
}

#[test]
fn tray_pace_color_defaults_off_and_round_trips() {
    let defaulted: Settings = serde_json::from_str(r#"{ "enabled_providers": [] }"#)
        .expect("missing tray pace color defaults false");
    assert!(!defaulted.menu_bar_color_pace);

    let enabled = Settings {
        menu_bar_color_pace: true,
        ..Settings::default()
    };
    let json = serde_json::to_string(&enabled).expect("serialize tray pace color");
    assert!(json.contains(r#""menu_bar_color_pace":true"#));

    let loaded: Settings = serde_json::from_str(&json).expect("deserialize tray pace color");
    assert!(loaded.menu_bar_color_pace);
}

#[test]
fn color_by_provider_defaults_off_and_round_trips() {
    // Old settings files carry no key and must load with the toggle off.
    let legacy: Settings = serde_json::from_str(r#"{ "enabled_providers": [] }"#)
        .expect("legacy settings without color by provider load");
    assert!(!legacy.menu_bar_color_by_provider);
    assert!(!Settings::default().menu_bar_color_by_provider);

    let enabled = Settings {
        menu_bar_color_by_provider: true,
        ..Settings::default()
    };
    let json = serde_json::to_string(&enabled).expect("serialize color by provider");
    assert!(json.contains(r#""menu_bar_color_by_provider":true"#));

    let loaded: Settings = serde_json::from_str(&json).expect("deserialize color by provider");
    assert!(loaded.menu_bar_color_by_provider);
}

#[test]
fn pace_reserve_positive_defaults_off_and_round_trips() {
    let defaulted: Settings = serde_json::from_str(r#"{ "enabled_providers": [] }"#)
        .expect("missing pace sign defaults to ahead-of-pace positive");
    assert!(!defaulted.menu_bar_pace_reserve_positive);
    assert!(!Settings::default().menu_bar_pace_reserve_positive);

    let enabled = Settings {
        menu_bar_pace_reserve_positive: true,
        ..Settings::default()
    };
    let json = serde_json::to_string(&enabled).expect("serialize pace sign");
    assert!(json.contains(r#""menu_bar_pace_reserve_positive":true"#));

    let loaded: Settings = serde_json::from_str(&json).expect("deserialize pace sign");
    assert!(loaded.menu_bar_pace_reserve_positive);
}

#[test]
fn providers_sorted_alphabetically_defaults_off_and_round_trips() {
    assert!(!Settings::default().providers_sorted_alphabetically);

    let legacy: Settings = serde_json::from_str(r#"{ "enabled_providers": [] }"#)
        .expect("legacy file without the sidebar sort key");
    assert!(!legacy.providers_sorted_alphabetically);

    let sorted = Settings {
        providers_sorted_alphabetically: true,
        ..Settings::default()
    };
    let json = serde_json::to_string(&sorted).expect("serialize sidebar sort");
    assert!(json.contains(r#""providers_sorted_alphabetically":true"#));

    let loaded: Settings = serde_json::from_str(&json).expect("deserialize sidebar sort");
    assert!(loaded.providers_sorted_alphabetically);
}

#[test]
fn providers_enabled_only_defaults_off_and_round_trips() {
    assert!(!Settings::default().providers_enabled_only);

    let legacy: Settings = serde_json::from_str(r#"{ "enabled_providers": [] }"#)
        .expect("legacy file without the sidebar enabled-only key");
    assert!(!legacy.providers_enabled_only);

    let filtered = Settings {
        providers_enabled_only: true,
        ..Settings::default()
    };
    let json = serde_json::to_string(&filtered).expect("serialize enabled-only filter");
    assert!(json.contains(r#""providers_enabled_only":true"#));

    let loaded: Settings = serde_json::from_str(&json).expect("deserialize enabled-only filter");
    assert!(loaded.providers_enabled_only);
}

#[test]
fn merge_visible_provider_order_keeps_hidden_slots_in_place() {
    let full: Vec<String> = ["codex", "kimik2", "claude", "gemini"]
        .into_iter()
        .map(str::to_string)
        .collect();
    let visible: Vec<String> = ["gemini", "codex", "claude"]
        .into_iter()
        .map(str::to_string)
        .collect();

    // kimik2 is hidden from the sidebar; it keeps index 1.
    assert_eq!(
        merge_visible_provider_order(&full, &visible),
        Some(
            ["gemini", "kimik2", "codex", "claude"]
                .into_iter()
                .map(str::to_string)
                .collect()
        )
    );
}

#[test]
fn merge_visible_provider_order_rejects_unknown_or_repeated_ids() {
    let full: Vec<String> = ["codex", "claude"]
        .into_iter()
        .map(str::to_string)
        .collect();
    let unknown: Vec<String> = ["codex", "nope"].into_iter().map(str::to_string).collect();
    let repeated: Vec<String> = ["codex", "codex"].into_iter().map(str::to_string).collect();

    assert_eq!(merge_visible_provider_order(&full, &unknown), None);
    assert_eq!(merge_visible_provider_order(&full, &repeated), None);
    assert_eq!(merge_visible_provider_order(&full, &[]), Some(full.clone()));
}

#[test]
fn cost_reporting_period_defaults_to_thirty_days_and_round_trips() {
    let defaulted: Settings = serde_json::from_str(r#"{ "enabled_providers": [] }"#)
        .expect("missing cost_reporting_period defaults");
    assert_eq!(
        defaulted.cost_reporting_period,
        CostReportingPeriod::Rolling(30)
    );

    for period in [
        CostReportingPeriod::Rolling(90),
        CostReportingPeriod::MonthToDate,
        CostReportingPeriod::AllAvailable,
    ] {
        let settings = Settings {
            cost_reporting_period: period,
            ..Settings::default()
        };
        let json = serde_json::to_string(&settings).expect("serialize cost period");
        assert!(json.contains(&format!(r#""cost_reporting_period":"{}""#, period.raw())));
        let loaded: Settings = serde_json::from_str(&json).expect("deserialize cost period");
        assert_eq!(loaded.cost_reporting_period, period);
    }
}

#[test]
fn unreadable_cost_reporting_period_loads_as_the_default() {
    let loaded: Settings =
        serde_json::from_str(r#"{ "enabled_providers": [], "cost_reporting_period": "weekly" }"#)
            .expect("an unreadable period must not fail the whole settings file");
    assert_eq!(
        loaded.cost_reporting_period,
        CostReportingPeriod::Rolling(30)
    );
}

#[test]
fn notification_sound_paths_round_trip_and_default_for_existing_settings() {
    let settings = Settings {
        notification_sound_theme: NotificationSoundTheme::CodexBar,
        notification_sound_paths: NotificationSoundPaths {
            critical_usage: Some(r"C:\sounds\critical.wav".to_string()),
            ..NotificationSoundPaths::default()
        },
        ..Settings::default()
    };
    let json = serde_json::to_string(&settings).expect("serialize notification sound paths");
    assert!(json.contains("\"criticalUsage\":\"C:\\\\sounds\\\\critical.wav\""));

    let loaded: Settings =
        serde_json::from_str(&json).expect("deserialize notification sound paths");
    assert_eq!(
        loaded.notification_sound_paths,
        settings.notification_sound_paths
    );
    assert_eq!(
        loaded.notification_sound_theme,
        NotificationSoundTheme::CodexBar
    );

    let legacy: Settings = serde_json::from_str(r#"{ "enabled_providers": [] }"#)
        .expect("deserialize settings without notification sound paths");
    assert_eq!(
        legacy.notification_sound_paths,
        NotificationSoundPaths::default()
    );
    assert_eq!(
        legacy.notification_sound_theme,
        NotificationSoundTheme::Windows
    );
}

#[test]
fn promote_tray_icon_defaults_on_when_missing_from_disk() {
    let loaded: Settings = serde_json::from_str(
        r#"{
            "enabled_providers": ["claude", "codex"],
            "refresh_interval_secs": 300
        }"#,
    )
    .expect("parse settings without promote_tray_icon");
    assert!(loaded.promote_tray_icon);
}

#[test]
fn promote_tray_default_migration_flips_old_false_once() {
    assert!(Settings::should_migrate_promote_tray_default(false, false));
    assert!(!Settings::should_migrate_promote_tray_default(true, false));
    assert!(!Settings::should_migrate_promote_tray_default(false, true));
    assert!(!Settings::should_migrate_promote_tray_default(true, true));
}

#[test]
fn new_warning_and_reset_settings_are_backward_compatible() {
    let loaded: Settings = serde_json::from_str(
        r#"{
            "enabled_providers": ["claude", "codex"],
            "refresh_interval_secs": 300
        }"#,
    )
    .expect("parse legacy settings");

    assert!(!loaded.show_reset_when_exhausted);
    assert!(!loaded.predictive_pace_warning_enabled);
    assert!(!loaded.credential_expiry_notifications_enabled);
    assert!(!loaded.limit_reset_notifications_enabled);
}

#[test]
fn limit_reset_notifications_default_off_and_round_trip_when_enabled() {
    assert!(!Settings::default().limit_reset_notifications_enabled);

    let settings = Settings {
        limit_reset_notifications_enabled: true,
        ..Settings::default()
    };
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    settings.save_to_path(&path).unwrap();
    assert!(Settings::load_from_path(Some(&path)).limit_reset_notifications_enabled);
}

fn load_settings_json(json: &str) -> Settings {
    serde_json::from_str(json).expect("parse settings")
}

fn custom(high: Option<f64>, critical: Option<f64>) -> QuotaWarningWindow {
    QuotaWarningWindow {
        enabled: None,
        high,
        critical,
    }
}

#[test]
fn custom_quota_warnings_win_over_global_thresholds() {
    let mut settings = Settings::default();
    settings.set_quota_warnings(
        ProviderId::Codex,
        ProviderQuotaWarnings {
            session: QuotaWarningWindow::default(),
            weekly: custom(Some(75.0), Some(95.0)),
        },
    );

    assert_eq!(
        settings.usage_thresholds(ProviderId::Codex, "weekly"),
        UsageThresholds {
            high: 75.0,
            critical: 95.0,
        }
    );
    // Global window and another provider keep the global values.
    assert_eq!(
        settings.usage_thresholds(ProviderId::Codex, "session"),
        UsageThresholds {
            high: 70.0,
            critical: 90.0,
        }
    );
    assert_eq!(
        settings.usage_thresholds(ProviderId::Claude, "session"),
        UsageThresholds {
            high: 70.0,
            critical: 90.0,
        }
    );
    // A partial Custom value inherits the missing one from global.
    settings.set_quota_warnings(
        ProviderId::Claude,
        ProviderQuotaWarnings {
            session: custom(None, Some(98.0)),
            weekly: QuotaWarningWindow::default(),
        },
    );
    assert_eq!(
        settings.usage_thresholds(ProviderId::Claude, "session"),
        UsageThresholds {
            high: 70.0,
            critical: 98.0,
        }
    );
}

#[test]
fn quota_warning_off_is_per_window_and_keeps_saved_values() {
    let mut settings = Settings::default();
    assert!(settings.quota_warning_enabled(ProviderId::Codex, "session"));
    settings.set_quota_warnings(
        ProviderId::Codex,
        ProviderQuotaWarnings {
            session: QuotaWarningWindow {
                enabled: Some(false),
                high: Some(60.0),
                critical: Some(80.0),
            },
            weekly: QuotaWarningWindow::default(),
        },
    );
    assert!(!settings.quota_warning_enabled(ProviderId::Codex, "session"));
    assert!(settings.quota_warning_enabled(ProviderId::Codex, "weekly"));
    assert!(settings.quota_warning_enabled(ProviderId::Claude, "session"));

    let reloaded = load_settings_json(&serde_json::to_string(&settings).expect("serialize"));
    let session = reloaded.quota_warnings(ProviderId::Codex).session;
    assert_eq!(session.enabled, Some(false));
    assert_eq!(session.high, Some(60.0));
    assert_eq!(session.critical, Some(80.0));
}

#[test]
fn global_quota_warnings_are_not_serialized() {
    let mut settings = Settings::default();
    settings.set_quota_warnings(ProviderId::Codex, ProviderQuotaWarnings::default());
    let value = serde_json::to_value(&settings).expect("serialize");
    assert!(
        value["provider_configs"]["codex"]
            .get("quota_warnings")
            .is_none()
    );
}

#[test]
fn migration_moves_a_window_key_into_quota_warnings() {
    let loaded = load_settings_json(
        r#"{ "provider_usage_thresholds": { "codex:session": { "high": 80.0 } } }"#,
    );
    let codex = loaded.quota_warnings(ProviderId::Codex);
    assert_eq!(codex.session, custom(Some(80.0), None));
    assert!(codex.weekly.is_global());
    assert_eq!(
        loaded.usage_thresholds(ProviderId::Codex, "session"),
        UsageThresholds {
            high: 80.0,
            critical: 90.0,
        }
    );
    // Only the new key is written back.
    let saved = serde_json::to_value(&loaded).expect("serialize");
    assert!(saved.get("provider_usage_thresholds").is_none());
    assert_eq!(
        saved["provider_configs"]["codex"]["quota_warnings"]["session"]["high"],
        serde_json::json!(80.0)
    );
}

#[test]
fn migration_spreads_a_provider_key_over_both_windows() {
    let loaded = load_settings_json(
        r#"{ "provider_usage_thresholds": { "claude": { "high": 65.0, "critical": 85.0 } } }"#,
    );
    let claude = loaded.quota_warnings(ProviderId::Claude);
    assert_eq!(claude.session, custom(Some(65.0), Some(85.0)));
    assert_eq!(claude.weekly, custom(Some(65.0), Some(85.0)));
}

#[test]
fn migration_merges_window_and_provider_keys_per_field() {
    let loaded = load_settings_json(
        r#"{
            "provider_usage_thresholds": {
                "codex": { "high": 75.0 },
                "codex:weekly": { "critical": 95.0 }
            }
        }"#,
    );
    let codex = loaded.quota_warnings(ProviderId::Codex);
    assert_eq!(codex.weekly, custom(Some(75.0), Some(95.0)));
    assert_eq!(codex.session, custom(Some(75.0), None));
}

#[test]
fn migration_without_legacy_keys_leaves_every_window_global() {
    let loaded = load_settings_json(r#"{ "provider_usage_thresholds": {} }"#);
    assert!(
        loaded
            .provider_configs
            .values()
            .all(|config| config.quota_warnings.is_empty())
    );
    let loaded = load_settings_json(r#"{ "refresh_interval_secs": 300 }"#);
    assert!(loaded.quota_warnings(ProviderId::Codex).is_empty());
}

#[test]
fn migration_keeps_existing_quota_warnings() {
    let loaded = load_settings_json(
        r#"{
            "provider_usage_thresholds": { "codex": { "high": 50.0 } },
            "provider_configs": {
                "codex": { "quota_warnings": { "session": { "enabled": false } } }
            }
        }"#,
    );
    let codex = loaded.quota_warnings(ProviderId::Codex);
    assert_eq!(
        codex.session,
        QuotaWarningWindow {
            enabled: Some(false),
            high: None,
            critical: None,
        }
    );
    assert_eq!(codex.weekly, custom(Some(50.0), None));
}

#[test]
fn migration_normalizes_out_of_range_and_unknown_entries() {
    let loaded = load_settings_json(
        r#"{
            "provider_usage_thresholds": {
                "codex": { "high": 120.0 },
                "claude": {},
                "codex:weekly": { "critical": -10.0 },
                "codex:monthly": { "high": 10.0 },
                "future-provider": { "high": 10.0 }
            },
            "provider_configs": {
                "zai": { "quota_warnings": { "weekly": { "high": 140.0 } } }
            }
        }"#,
    );
    let codex = loaded.quota_warnings(ProviderId::Codex);
    assert_eq!(codex.session, custom(Some(100.0), None));
    assert_eq!(codex.weekly, custom(Some(100.0), Some(0.0)));
    assert!(loaded.quota_warnings(ProviderId::Claude).is_empty());
    assert_eq!(
        loaded.quota_warnings(ProviderId::Zai).weekly,
        custom(Some(100.0), None)
    );
}

#[test]
fn codex_option_toggles_round_trip_through_settings_json() {
    let mut settings = Settings::default();
    settings.set_local_session_cost_ledger_enabled(ProviderId::Codex, true);
    settings.set_openai_web_battery_saver_enabled(ProviderId::Codex, true);
    let reloaded = load_settings_json(&serde_json::to_string(&settings).expect("serialize"));
    assert!(reloaded.local_session_cost_ledger_enabled(ProviderId::Codex));
    assert!(reloaded.openai_web_battery_saver_enabled(ProviderId::Codex));
    assert!(!Settings::default().local_session_cost_ledger_enabled(ProviderId::Codex));
    assert!(!Settings::default().openai_web_battery_saver_enabled(ProviderId::Codex));
}

#[test]
fn float_bar_defaults_are_safe() {
    let settings = Settings::default();
    assert!(!settings.float_bar_enabled);
    assert_eq!(settings.float_bar_opacity, 80);
    assert_eq!(settings.float_bar_scale, 100);
    assert_eq!(settings.float_bar_orientation, "horizontal");
    assert_eq!(settings.float_bar_style, "floating");
    assert!(!settings.float_bar_click_through);
    assert!(settings.float_bar_provider_ids.is_empty());
    assert!(!settings.float_bar_dark_text);
    assert!(!settings.float_bar_show_reset_inline);
    assert!(!settings.float_bar_show_cost);
}

#[test]
fn main_window_scale_defaults_to_100_percent() {
    let settings = Settings::default();
    assert_eq!(settings.window_scale_percent, 100);
}

#[test]
fn main_window_scale_clamp_pins_to_supported_range() {
    assert_eq!(clamp_window_scale_percent(0), 100);
    assert_eq!(clamp_window_scale_percent(99), 100);
    assert_eq!(clamp_window_scale_percent(100), 100);
    assert_eq!(clamp_window_scale_percent(125), 125);
    assert_eq!(clamp_window_scale_percent(180), 180);
    assert_eq!(clamp_window_scale_percent(250), 250);
    assert_eq!(clamp_window_scale_percent(251), 250);
}

#[test]
fn raw_settings_clamps_main_window_scale_on_load() {
    let json = r#"{
            "enabled_providers": ["claude", "codex"],
            "refresh_interval_secs": 300,
            "window_scale_percent": 300
        }"#;
    let loaded: Settings = serde_json::from_str(json).expect("parse settings");
    assert_eq!(loaded.window_scale_percent, 250);
}

#[test]
fn tray_scale_defaults_to_100_percent() {
    let settings = Settings::default();
    assert_eq!(settings.tray_scale_percent, 100);
}

#[test]
fn tray_scale_clamp_pins_to_supported_range() {
    assert_eq!(clamp_tray_scale_percent(0), 100);
    assert_eq!(clamp_tray_scale_percent(99), 100);
    assert_eq!(clamp_tray_scale_percent(100), 100);
    assert_eq!(clamp_tray_scale_percent(125), 125);
    assert_eq!(clamp_tray_scale_percent(180), 180);
    assert_eq!(clamp_tray_scale_percent(200), 200);
    assert_eq!(clamp_tray_scale_percent(201), 200);
}

#[test]
fn raw_settings_clamps_tray_scale_on_load() {
    let json = r#"{
            "enabled_providers": ["claude", "codex"],
            "refresh_interval_secs": 300,
            "tray_scale_percent": 300
        }"#;
    let loaded: Settings = serde_json::from_str(json).expect("parse settings");
    assert_eq!(loaded.tray_scale_percent, 200);
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
fn float_bar_scale_clamp_pins_to_supported_range() {
    assert_eq!(clamp_float_bar_scale(0), 75);
    assert_eq!(clamp_float_bar_scale(74), 75);
    assert_eq!(clamp_float_bar_scale(100), 100);
    assert_eq!(clamp_float_bar_scale(150), 150);
    assert_eq!(clamp_float_bar_scale(250), 200);
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
fn float_bar_style_normalization_rejects_unknown_values() {
    assert_eq!(normalize_float_bar_style("floating"), "floating");
    assert_eq!(normalize_float_bar_style("taskbar"), "taskbar");
    assert_eq!(normalize_float_bar_style(""), "floating");
    assert_eq!(normalize_float_bar_style("TASKBAR"), "floating");
    assert_eq!(normalize_float_bar_style("glass"), "floating");
}

#[test]
fn float_bar_settings_round_trip_through_raw() {
    // Serialize a Settings with custom float-bar values then deserialize
    // through the `from = "RawSettings"` path — values must survive intact
    // (after clamping/normalization).
    let s = Settings {
        float_bar_enabled: true,
        float_bar_opacity: 65,
        float_bar_scale: 140,
        float_bar_orientation: "vertical".to_string(),
        float_bar_style: "taskbar".to_string(),
        float_bar_click_through: true,
        float_bar_provider_ids: vec!["claude".into(), "codex".into()],
        float_bar_dark_text: true,
        float_bar_show_reset_inline: true,
        float_bar_show_cost: true,
        ..Settings::default()
    };

    let json = serde_json::to_string(&s).expect("serialize");
    let back: Settings = serde_json::from_str(&json).expect("deserialize");
    assert!(back.float_bar_enabled);
    assert_eq!(back.float_bar_opacity, 65);
    assert_eq!(back.float_bar_scale, 140);
    assert_eq!(back.float_bar_orientation, "vertical");
    assert_eq!(back.float_bar_style, "taskbar");
    assert!(back.float_bar_click_through);
    assert_eq!(back.float_bar_provider_ids, vec!["claude", "codex"]);
    assert!(back.float_bar_dark_text);
    assert!(back.float_bar_show_reset_inline);
    assert!(back.float_bar_show_cost);
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
            "high_usage_threshold": 70.0,
            "critical_usage_threshold": 90.0,
            "merge_tray_icons": false,
            "show_as_used": true,
            "enable_animations": true,
            "reset_time_relative": true,
            "menu_bar_display_mode": "detailed",
            "disable_keychain_access": false,
            "hide_personal_info": false,
            "float_bar_opacity": 250,
            "float_bar_scale": 250,
            "float_bar_orientation": "diagonal",
            "float_bar_style": "glass"
        }"#;
    let loaded: Settings = serde_json::from_str(json).expect("parse");
    assert_eq!(loaded.float_bar_opacity, 100);
    assert_eq!(loaded.float_bar_scale, 200);
    assert_eq!(loaded.float_bar_orientation, "horizontal");
    assert_eq!(loaded.float_bar_style, "floating");
}

#[test]
fn provider_listing_hides_retired_providers_unless_enabled() {
    let mut settings = Settings::default();
    settings.enabled_providers.clear();
    assert!(!settings.is_provider_listed(ProviderId::KimiK2));
    settings.enable_provider(ProviderId::KimiK2);
    assert!(settings.is_provider_listed(ProviderId::KimiK2));
    assert!(settings.is_provider_listed(ProviderId::Codex));
}

#[test]
fn test_settings_provider_enabled() {
    let settings = Settings::default();
    assert!(settings.is_provider_enabled(ProviderId::Claude));
    assert!(settings.is_provider_enabled(ProviderId::Codex));
    assert!(!settings.is_provider_enabled(ProviderId::Gemini));
    assert!(!settings.is_provider_enabled(ProviderId::Wayfinder));
    assert_eq!(
        settings.gateway_url(ProviderId::Wayfinder),
        "http://127.0.0.1:8088"
    );
}

#[test]
fn wayfinder_gateway_round_trips_without_changing_settings_paths() {
    let mut settings = Settings::default();
    settings.set_gateway_url(
        ProviderId::Wayfinder,
        "https://gateway.example.test/wayfinder/",
    );

    let json = serde_json::to_string(&settings).expect("serialize settings");
    let loaded: Settings = serde_json::from_str(&json).expect("deserialize settings");
    assert_eq!(
        loaded.gateway_url(ProviderId::Wayfinder),
        "https://gateway.example.test/wayfinder/"
    );
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
fn provider_order_dedupes_unknowns_and_appends_canonical_ids() {
    let order = normalize_provider_order(&[
        "gemini".to_string(),
        "not-a-provider".to_string(),
        "claude".to_string(),
        "gemini".to_string(),
    ]);

    assert_eq!(order[0], "gemini");
    assert_eq!(order[1], "claude");
    assert!(!order.iter().any(|id| id == "not-a-provider"));
    assert_eq!(order.len(), ProviderId::all().len());
}

#[test]
fn enabled_provider_ids_follow_custom_provider_order() {
    let settings = Settings {
        enabled_providers: ["claude", "codex", "gemini"]
            .into_iter()
            .map(str::to_string)
            .collect(),
        provider_order: normalize_provider_order(&[
            "gemini".to_string(),
            "claude".to_string(),
            "codex".to_string(),
        ]),
        ..Settings::default()
    };

    assert_eq!(
        settings.get_enabled_provider_ids(),
        vec![ProviderId::Gemini, ProviderId::Claude, ProviderId::Codex]
    );
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
        ProviderId::DeepInfra,
        ProviderId::HuggingFace,
        ProviderId::AiAnd,
        ProviderId::ElevenLabs,
        ProviderId::Deepgram,
        ProviderId::Grok,
        ProviderId::Groq,
        ProviderId::LLMProxy,
        ProviderId::Xai,
        ProviderId::Meta,
    ] {
        assert!(
            providers.iter().any(|provider| provider.id == id),
            "{id} should be configurable from the API Keys UI"
        );
    }
}

#[test]
fn groq_api_key_help_matches_upstream_copy() {
    let info = get_api_key_providers()
        .into_iter()
        .find(|info| info.id == ProviderId::Groq)
        .expect("Groq api key metadata");
    assert_eq!(
        info.api_key_help,
        Some(
            "Usage & spend come from your console.groq.com browser session automatically. An API key is optional and only adds Enterprise Prometheus metrics."
        )
    );
}

#[test]
fn openrouter_api_key_help_explains_management_keys() {
    let info = get_api_key_providers()
        .into_iter()
        .find(|info| info.id == ProviderId::OpenRouter)
        .expect("OpenRouter api key metadata");
    assert_eq!(
        info.api_key_help,
        Some(
            "Required. Enter a regular API key or a Management API key here. Management keys also enable account Activity on the official OpenRouter API."
        )
    );
}

#[test]
fn test_t3_chat_is_cookie_configured_not_api_key_configured() {
    let providers = get_api_key_providers();
    assert!(
        !providers
            .iter()
            .any(|provider| provider.id == ProviderId::T3Chat),
        "T3 Chat fetches usage from browser cookies or pasted cURL, not API keys"
    );
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
fn api_key_display_mask_is_utf8_safe() {
    let mut keys = ApiKeys::default();
    keys.set("openrouter", "🔑🔒漢字abcdefgh🔐", Some("unicode"));

    let display = keys.get_all_for_display();

    assert_eq!(display.len(), 1);
    assert_eq!(display[0].masked_key, "🔑🔒漢字...fgh🔐");
}

#[test]
fn test_start_at_login_command_uses_only_the_executable_path() {
    let path = std::path::PathBuf::from(r"C:\Program Files\CodexBar\codexbar-desktop-tauri.exe");
    let command = Settings::start_at_login_command(&path);
    assert_eq!(
        command,
        "\"C:\\Program Files\\CodexBar\\codexbar-desktop-tauri.exe\""
    );
    assert!(!command.contains("menubar"));
}

#[test]
fn test_start_at_login_prefers_desktop_sibling_when_called_from_cli() {
    let temp = tempfile::tempdir().expect("temp dir");
    let cli_path = temp.path().join("codexbar-cli.exe");
    let desktop_path = temp.path().join("codexbar.exe");
    std::fs::write(&cli_path, b"cli").expect("write cli");
    std::fs::write(&desktop_path, b"desktop").expect("write desktop");

    let command = Settings::start_at_login_command(&cli_path);

    assert_eq!(command, format!("\"{}\"", desktop_path.display()));
}

#[test]
fn test_start_at_login_keeps_current_exe_when_desktop_sibling_missing() {
    let temp = tempfile::tempdir().expect("temp dir");
    let cli_path = temp.path().join("codexbar-cli.exe");
    std::fs::write(&cli_path, b"cli").expect("write cli");

    let command = Settings::start_at_login_command(&cli_path);

    assert_eq!(command, format!("\"{}\"", cli_path.display()));
}

#[test]
fn test_start_at_login_repairs_stale_cli_command_after_update() {
    let temp = tempfile::tempdir().expect("temp dir");
    let cli_path = temp.path().join("codexbar-cli.exe");
    let desktop_path = temp.path().join("codexbar.exe");
    std::fs::write(&cli_path, b"cli").expect("write cli");
    std::fs::write(&desktop_path, b"desktop").expect("write desktop");
    let stale_command = format!("\"{}\"", cli_path.display());

    assert!(Settings::start_at_login_command_needs_repair(
        &stale_command,
        &desktop_path
    ));
}

#[test]
fn test_start_at_login_keeps_current_desktop_command_after_update() {
    let temp = tempfile::tempdir().expect("temp dir");
    let desktop_path = temp.path().join("codexbar.exe");
    std::fs::write(&desktop_path, b"desktop").expect("write desktop");
    let current_command = format!("\"{}\"", desktop_path.display());

    assert!(!Settings::start_at_login_command_needs_repair(
        &current_command,
        &desktop_path
    ));
}

#[test]
fn test_start_at_login_repairs_legacy_desktop_command_after_update() {
    let temp = tempfile::tempdir().expect("temp dir");
    let desktop_path = temp.path().join("codexbar.exe");
    let legacy_desktop_path = temp.path().join("codexbar-desktop.exe");
    std::fs::write(&desktop_path, b"desktop").expect("write desktop");
    std::fs::write(&legacy_desktop_path, b"legacy desktop").expect("write legacy desktop");
    let stale_command = format!("\"{}\"", legacy_desktop_path.display());

    assert!(Settings::start_at_login_command_needs_repair(
        &stale_command,
        &legacy_desktop_path
    ));
}

#[test]
fn test_language_defaults_to_english() {
    let settings = Settings::default();
    assert_eq!(settings.ui_language, Language::English);
}

#[test]
fn new_install_language_follows_the_system() {
    // No settings file: a new install, which follows the Windows display language.
    let settings = Settings::load_from_path(None);
    assert!(settings.ui_language_follows_system);

    let dir = tempfile::tempdir().expect("tempdir");
    let missing = dir.path().join("settings.json");
    assert!(Settings::load_from_path(Some(&missing)).ui_language_follows_system);
}

#[test]
fn legacy_settings_file_keeps_its_chosen_language() {
    // A file written before "System" existed keeps the language it chose.
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("settings.json");
    std::fs::write(&path, r#"{ "ui_language": "japanese" }"#).expect("write");
    let mut settings = Settings::load_from_path(Some(&path));
    assert!(!settings.ui_language_follows_system);
    assert!(!settings.resolve_system_language_with(Language::Korean));
    assert_eq!(settings.ui_language, Language::Japanese);

    std::fs::write(&path, "{}").expect("write");
    let settings = Settings::load_from_path(Some(&path));
    assert!(!settings.ui_language_follows_system);
    assert_eq!(settings.ui_language, Language::English);
}

#[test]
fn full_legacy_settings_file_keeps_its_chosen_language() {
    // A complete file as the previous release wrote it: every field, but no
    // `ui_language_follows_system` key.
    let mut legacy = serde_json::to_value(Settings::default()).expect("json");
    let object = legacy.as_object_mut().expect("settings object");
    assert!(object.remove("ui_language_follows_system").is_some());
    object.insert("ui_language".into(), serde_json::json!("japanese"));
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("settings.json");
    std::fs::write(&path, serde_json::to_string_pretty(&legacy).expect("json")).expect("write");

    let mut settings = Settings::load_from_path(Some(&path));
    assert!(!settings.ui_language_follows_system);
    assert!(!settings.resolve_system_language_with(Language::Turkish));
    assert_eq!(settings.ui_language, Language::Japanese);
}

#[test]
fn following_the_system_round_trips_through_the_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("settings.json");
    let following = Settings::default();
    std::fs::write(&path, serde_json::to_string(&following).expect("json")).expect("write");
    let mut loaded = Settings::load_from_path(Some(&path));
    assert!(loaded.ui_language_follows_system);
    assert!(loaded.resolve_system_language_with(Language::Turkish));
    assert_eq!(loaded.ui_language, Language::Turkish);
}

#[test]
fn test_language_all_variants_available() {
    let languages = Language::all();
    assert_eq!(languages.len(), 10);
    assert!(languages.contains(&Language::English));
    assert!(languages.contains(&Language::Chinese));
    assert!(languages.contains(&Language::ChineseTraditional));
    assert!(languages.contains(&Language::Japanese));
    assert!(languages.contains(&Language::Korean));
    assert!(languages.contains(&Language::Spanish));
    assert!(languages.contains(&Language::PortugueseBrazil));
    assert!(languages.contains(&Language::Russian));
    assert!(languages.contains(&Language::Turkish));
    assert!(languages.contains(&Language::Ukrainian));
}

#[test]
fn test_language_display_names() {
    assert_eq!(Language::English.display_name(), "English");
    assert_eq!(Language::Chinese.display_name(), "中文");
    assert_eq!(Language::ChineseTraditional.display_name(), "繁體中文");
    assert_eq!(Language::Japanese.display_name(), "日本語");
    assert_eq!(Language::Russian.display_name(), "Русский");
    assert_eq!(Language::Turkish.display_name(), "Türkçe");
    assert_eq!(Language::Ukrainian.display_name(), "Українська");
    assert_eq!(
        Language::PortugueseBrazil.display_name(),
        "Português (Brasil)"
    );
}

#[test]
fn test_language_resolves_brazilian_portuguese_aliases() {
    for alias in [
        "portuguesebrazil",
        "pt",
        "pt-BR",
        "Portuguese",
        "Português",
        "portugues",
        "Português (Brasil)",
    ] {
        assert_eq!(
            Language::resolve(alias),
            Some(Language::PortugueseBrazil),
            "failed to resolve {alias}"
        );
    }
}

#[test]
fn test_language_resolves_russian_aliases() {
    assert_eq!(Language::resolve("russian"), Some(Language::Russian));
    assert_eq!(Language::resolve("ru-RU"), Some(Language::Russian));
    assert_eq!(Language::resolve("Русский"), Some(Language::Russian));
}

#[test]
fn test_language_resolves_turkish_aliases() {
    assert_eq!(Language::resolve("turkish"), Some(Language::Turkish));
    assert_eq!(Language::resolve("tr-TR"), Some(Language::Turkish));
    assert_eq!(Language::resolve("Türkçe"), Some(Language::Turkish));
    assert_eq!(Language::resolve("turkce"), Some(Language::Turkish));
}

#[test]
fn test_language_resolves_ukrainian_aliases() {
    assert_eq!(Language::resolve("ukrainian"), Some(Language::Ukrainian));
    assert_eq!(Language::resolve("uk"), Some(Language::Ukrainian));
    assert_eq!(Language::resolve("uk-UA"), Some(Language::Ukrainian));
    assert_eq!(Language::resolve("Українська"), Some(Language::Ukrainian));
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
    let loaded: Settings = serde_json::from_str(&content).expect("Failed to deserialize settings");

    assert_eq!(loaded.ui_language, Language::Chinese);
}

#[test]
fn test_settings_with_utf8_bom_parses_perprovider_tray_mode() {
    let json = "\u{feff}{\n            \"enabled_providers\": [\"claude\", \"codex\"],\n            \"refresh_interval_secs\": 300,\n            \"tray_icon_mode\": \"perprovider\"\n        }";

    let settings: Settings = serde_json::from_str(json.trim_start_matches('\u{feff}')).unwrap();

    assert_eq!(settings.tray_icon_mode, TrayIconMode::PerProvider);
}

#[test]
fn stacked_tray_mode_preserves_provider_preferences() {
    let json = r#"{
        "tray_icon_mode": "stacked",
        "stacked_tray_top_provider": "claude",
        "stacked_tray_bottom_provider": "codex"
    }"#;

    let settings: Settings = serde_json::from_str(json).unwrap();

    assert_eq!(settings.tray_icon_mode, TrayIconMode::Stacked);
    assert_eq!(
        settings.stacked_tray_top_provider.as_deref(),
        Some("claude")
    );
    assert_eq!(
        settings.stacked_tray_bottom_provider.as_deref(),
        Some("codex")
    );

    let saved = serde_json::to_string(&settings).unwrap();
    let reloaded: Settings = serde_json::from_str(&saved).unwrap();
    assert_eq!(
        reloaded.stacked_tray_top_provider.as_deref(),
        Some("claude")
    );
    assert_eq!(
        reloaded.stacked_tray_bottom_provider.as_deref(),
        Some("codex")
    );
}

#[test]
fn test_language_serde_serialization() {
    // Test that Language serializes to lowercase string
    let english = Language::English;
    let chinese = Language::Chinese;
    let chinese_traditional = Language::ChineseTraditional;

    let english_json = serde_json::to_string(&english).unwrap();
    let chinese_json = serde_json::to_string(&chinese).unwrap();
    let chinese_traditional_json = serde_json::to_string(&chinese_traditional).unwrap();

    assert_eq!(english_json, "\"english\"");
    assert_eq!(chinese_json, "\"chinese\"");
    assert_eq!(chinese_traditional_json, "\"chinesetraditional\"");
}

#[test]
fn test_language_serde_deserialization() {
    // Test that lowercase strings deserialize correctly
    let english: Language = serde_json::from_str("\"english\"").unwrap();
    let chinese: Language = serde_json::from_str("\"chinese\"").unwrap();
    let chinese_traditional: Language = serde_json::from_str("\"chinesetraditional\"").unwrap();

    assert_eq!(english, Language::English);
    assert_eq!(chinese, Language::Chinese);
    assert_eq!(chinese_traditional, Language::ChineseTraditional);
}

#[test]
fn test_language_resolves_traditional_chinese_aliases() {
    assert_eq!(
        Language::resolve("chinesetraditional"),
        Some(Language::ChineseTraditional)
    );
    assert_eq!(
        Language::resolve("zh-tw"),
        Some(Language::ChineseTraditional)
    );
    assert_eq!(
        Language::resolve("zh-hant-tw"),
        Some(Language::ChineseTraditional)
    );
    assert_eq!(
        Language::resolve("繁體中文"),
        Some(Language::ChineseTraditional)
    );
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
    settings.set_auto_resume_after_quota_reset(ProviderId::Codex, true);
    settings
        .set_seat_credit_entitlement(ProviderId::Copilot, Some(300.0))
        .expect("valid seat credit entitlement");

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
    assert!(loaded.auto_resume_after_quota_reset(ProviderId::Codex));
    assert_eq!(
        loaded.seat_credit_entitlement(ProviderId::Copilot),
        Some(300.0)
    );
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

#[test]
fn retired_provider_config_is_ignored_until_explicit_save() {
    let original = r#"{
            "enabled_providers": ["codex", "crof"],
            "refresh_interval_secs": 300,
            "provider_metrics": { "codex": "weekly", "crof": "session" },
            "float_bar_provider_ids": ["codex", "crof"],
            "stacked_tray_top_provider": "crof",
            "stacked_tray_bottom_provider": "crof",
            "provider_configs": {
                "crof": { "api_token": "retired-fixture-key" },
                "codex": { "cookie_source": "manual", "openai_web_extras": false },
                "alibaba": { "api_region": "cn", "manual_cookie_header": "ali=PLACEHOLDER" }
            }
        }"#;
    let original_bytes = original.as_bytes().to_vec();

    let settings: Settings =
        serde_json::from_str(original).expect("load settings with retired key");

    assert_eq!(original.as_bytes(), original_bytes);
    assert_eq!(settings.cookie_source(ProviderId::Codex), "manual");
    assert!(!settings.openai_web_extras(ProviderId::Codex));
    assert_eq!(
        settings.enabled_providers,
        HashSet::from(["codex".to_string()])
    );
    assert_eq!(settings.provider_metrics.len(), 1);
    assert_eq!(settings.float_bar_provider_ids, ["codex"]);
    assert_eq!(settings.stacked_tray_top_provider, None);
    assert_eq!(settings.stacked_tray_bottom_provider, None);
    assert_eq!(settings.api_region(ProviderId::Alibaba), "cn");
    assert_eq!(
        settings.manual_cookie_header(ProviderId::Alibaba),
        "ali=PLACEHOLDER"
    );

    let saved = serde_json::to_string(&settings).expect("serialize sanitized settings");
    let saved_value: serde_json::Value = serde_json::from_str(&saved).unwrap();
    let saved_configs = saved_value["provider_configs"].as_object().unwrap();
    assert!(!saved_configs.contains_key("crof"));
    assert!(saved_configs.contains_key("codex"));
    assert!(saved_configs.contains_key("alibaba"));
    assert!(
        !saved.contains("\"crof\""),
        "saved settings retained Crof: {saved}"
    );
}

#[test]
fn provider_aliases_are_canonicalized_at_the_load_boundary() {
    let settings: Settings = serde_json::from_str(
        r#"{
            "enabled_providers": ["openai", "ClAuDe", "not-a-provider"],
            "provider_metrics": {
                "openai": "weekly",
                "CoDeX": "session",
                "not-a-provider": "weekly"
            },
            "float_bar_provider_ids": ["OPENAI", "codex", "ClAuDe", "unknown"],
            "stacked_tray_top_provider": "OPENAI",
            "stacked_tray_bottom_provider": "ClAuDe"
        }"#,
    )
    .expect("load settings containing provider aliases");

    assert_eq!(
        settings.enabled_providers,
        HashSet::from(["claude".to_string(), "codex".to_string()])
    );
    assert_eq!(
        settings.provider_metrics.get("codex"),
        Some(&MetricPreference::Session)
    );
    assert_eq!(settings.provider_metrics.len(), 1);
    assert_eq!(settings.float_bar_provider_ids, ["codex", "claude"]);
    assert_eq!(settings.stacked_tray_top_provider.as_deref(), Some("codex"));
    assert_eq!(
        settings.stacked_tray_bottom_provider.as_deref(),
        Some("claude")
    );
}

#[test]
fn stacked_preferences_preserve_known_disabled_providers() {
    let settings: Settings = serde_json::from_str(
        r#"{
            "enabled_providers": ["claude"],
            "stacked_tray_top_provider": "OPENAI",
            "stacked_tray_bottom_provider": "not-a-provider"
        }"#,
    )
    .expect("load stacked preferences independently of enablement");

    assert_eq!(settings.stacked_tray_top_provider.as_deref(), Some("codex"));
    assert_eq!(settings.stacked_tray_bottom_provider, None);
    assert_eq!(
        settings.enabled_providers,
        HashSet::from(["claude".to_string()])
    );
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
    assert_eq!(settings.api_region(ProviderId::Alibaba), "singapore");
    assert_eq!(settings.api_region(ProviderId::Zai), "global");
    assert_eq!(settings.api_region(ProviderId::MiniMax), "global");
    assert!(settings.openai_web_extras(ProviderId::Codex));
    assert!(!settings.historical_tracking(ProviderId::Codex));
    assert!(!settings.avoid_keychain_prompts(ProviderId::Claude));
    assert!(!settings.auto_resume_after_quota_reset(ProviderId::Codex));
    assert!(!settings.auto_resume_after_quota_reset(ProviderId::Claude));
}

#[test]
fn codex_spark_usage_visibility_defaults_to_visible_and_roundtrips() {
    let mut settings = Settings::default();
    assert!(settings.codex_spark_usage_visible());

    settings.set_codex_spark_usage_visible(false);
    let serialized = serde_json::to_string(&settings).unwrap();
    let loaded: Settings = serde_json::from_str(&serialized).unwrap();

    assert!(!loaded.codex_spark_usage_visible());
}

#[test]
fn migrate_legacy_visibility_flags_materializes_hidden_usage_item_ids() {
    let mut settings = Settings::default();
    settings.set_spark_usage_visible(ProviderId::Codex, false);
    settings.claude_daily_routines_usage_visible = false;

    settings.migrate_legacy_usage_item_flags();

    assert_eq!(
        settings.hidden_usage_item_ids(ProviderId::Codex),
        CODEX_SPARK_USAGE_ITEM_IDS
            .iter()
            .map(|id| (*id).to_string())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        settings.hidden_usage_item_ids(ProviderId::Claude),
        vec![CLAUDE_DAILY_ROUTINES_USAGE_ITEM_ID.to_string()]
    );
}

#[test]
fn generic_claude_visibility_writes_only_the_usage_item_list() {
    let mut settings = Settings::default();

    settings.set_hidden_usage_item_ids(
        ProviderId::Claude,
        vec![CLAUDE_DAILY_ROUTINES_USAGE_ITEM_ID.to_string()],
    );

    assert!(settings.claude_daily_routines_usage_visible);
    assert_eq!(
        settings.hidden_usage_item_ids(ProviderId::Claude),
        vec![CLAUDE_DAILY_ROUTINES_USAGE_ITEM_ID.to_string()]
    );

    settings.set_hidden_usage_item_ids(ProviderId::Claude, Vec::new());

    assert!(settings.claude_daily_routines_usage_visible);
    assert!(
        settings
            .hidden_usage_item_ids(ProviderId::Claude)
            .is_empty()
    );
}

#[test]
fn explicit_hidden_usage_item_ids_roundtrip_and_restore_defaults() {
    let mut settings = Settings::default();
    settings.set_hidden_usage_item_ids(
        ProviderId::Codex,
        vec![
            "metric:secondary".to_string(),
            "metric:secondary".to_string(),
            "not-a-metric".to_string(),
        ],
    );

    assert_eq!(
        settings.hidden_usage_item_ids(ProviderId::Codex),
        vec!["metric:secondary".to_string()]
    );
    assert!(settings.codex_spark_usage_visible());

    let serialized = serde_json::to_string(&settings).unwrap();
    let loaded: Settings = serde_json::from_str(&serialized).unwrap();
    assert_eq!(
        loaded.hidden_usage_item_ids(ProviderId::Codex),
        vec!["metric:secondary".to_string()]
    );

    settings.set_hidden_usage_item_ids(ProviderId::Codex, Vec::new());
    settings.set_hidden_usage_item_ids(ProviderId::Claude, Vec::new());
    assert!(settings.hidden_usage_item_ids(ProviderId::Codex).is_empty());
    assert!(settings.codex_spark_usage_visible());
}

#[test]
fn legacy_visibility_setters_preserve_other_explicit_hidden_items() {
    let mut settings = Settings::default();
    settings.set_hidden_usage_item_ids(ProviderId::Claude, vec!["metric:secondary".to_string()]);

    settings.toggle_hidden_items(
        ProviderId::Claude,
        &[CLAUDE_DAILY_ROUTINES_USAGE_ITEM_ID],
        false,
    );
    assert_eq!(
        settings.hidden_usage_item_ids(ProviderId::Claude),
        vec![
            "metric:extra-claude-routines".to_string(),
            "metric:secondary".to_string(),
        ]
    );

    settings.toggle_hidden_items(
        ProviderId::Claude,
        &[CLAUDE_DAILY_ROUTINES_USAGE_ITEM_ID],
        true,
    );
    assert_eq!(
        settings.hidden_usage_item_ids(ProviderId::Claude),
        vec!["metric:secondary".to_string()]
    );
}

/// Windows never had upstream's global optional-usage gate, so Claude reset
/// credits, Extra usage and plan dates were all visible before the
/// independent choices existed. Upgrading must keep them visible and leave
/// existing choices untouched.
#[test]
fn upgrade_keeps_claude_optional_rows_visible_and_preserves_existing_choices() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    std::fs::write(
        &path,
        r#"{ "enabled_providers": ["claude"],
             "provider_configs": { "claude": { "hidden_usage_item_ids": ["metric:secondary"] } } }"#,
    )
    .unwrap();

    let mut loaded = Settings::load_from_path(Some(&path));
    loaded.migrate_legacy_usage_item_flags();

    assert_eq!(
        loaded.hidden_usage_item_ids(ProviderId::Claude),
        vec!["metric:secondary".to_string()]
    );
    for id in [
        CLAUDE_RESET_CREDITS_USAGE_ITEM_ID,
        CLAUDE_EXTRA_USAGE_USAGE_ITEM_ID,
        SUBSCRIPTION_USAGE_ITEM_ID,
    ] {
        assert!(
            !loaded
                .hidden_usage_item_ids(ProviderId::Claude)
                .contains(&id.to_string())
        );
    }

    let fresh = Settings::load_from_path(Some(&dir.path().join("missing.json")));
    assert!(fresh.hidden_usage_item_ids(ProviderId::Claude).is_empty());
}

#[test]
fn claude_section_choices_round_trip_independently_through_persistence() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let mut settings = Settings::default();
    settings.set_hidden_usage_item_ids(
        ProviderId::Claude,
        vec![
            SUBSCRIPTION_USAGE_ITEM_ID.to_string(),
            CLAUDE_RESET_CREDITS_USAGE_ITEM_ID.to_string(),
            "section:unknown-section".to_string(),
            "bogus:claude-extra-usage".to_string(),
        ],
    );
    settings.save_to_path(&path).unwrap();

    let loaded = Settings::load_from_path(Some(&path));
    assert_eq!(
        loaded.hidden_usage_item_ids(ProviderId::Claude),
        vec![
            CLAUDE_RESET_CREDITS_USAGE_ITEM_ID.to_string(),
            SUBSCRIPTION_USAGE_ITEM_ID.to_string(),
            "section:unknown-section".to_string(),
        ]
    );
    assert!(
        !loaded
            .hidden_usage_item_ids(ProviderId::Claude)
            .contains(&CLAUDE_EXTRA_USAGE_USAGE_ITEM_ID.to_string())
    );
}

/// Cookie-denial settings must survive the same path-based persistence used by
/// `Settings::save` and `Settings::load`, including the secure-file wrapper
/// shared by desktop and CLI settings.
#[test]
fn cookie_denial_round_trips_through_settings_persistence() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let mut settings = Settings::default();
    settings.set_cookie_source(ProviderId::Codex, "off");
    settings.set_openai_web_extras(ProviderId::Codex, false);

    settings.save_to_path(&path).unwrap();
    let loaded = Settings::load_from_path(Some(&path));

    assert_eq!(loaded.cookie_source(ProviderId::Codex), "off");
    assert!(!loaded.openai_web_extras(ProviderId::Codex));
}

#[test]
fn show_as_used_defaults_to_left_without_a_saved_value() {
    let dir = tempfile::tempdir().unwrap();
    let new_install = dir.path().join("missing.json");
    let without_key = dir.path().join("settings.json");
    std::fs::write(&without_key, r#"{ "enabled_providers": ["claude"] }"#).unwrap();

    assert!(!Settings::load_from_path(Some(&new_install)).show_as_used);
    assert!(!Settings::load_from_path(Some(&without_key)).show_as_used);
}

#[test]
fn show_as_used_keeps_a_saved_used_preference() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    std::fs::write(
        &path,
        r#"{ "enabled_providers": ["claude"], "show_as_used": true }"#,
    )
    .unwrap();

    assert!(Settings::load_from_path(Some(&path)).show_as_used);
}

/// Migration #1: a file written before the session-quota key existed keeps the
/// behaviour it had, when depleted/restored alerts followed `show_notifications`.
#[test]
fn session_quota_notifications_inherit_show_notifications_from_legacy_files() {
    let off: Settings =
        serde_json::from_str(r#"{ "enabled_providers": [], "show_notifications": false }"#)
            .expect("deserialize legacy settings with notifications off");
    assert!(!off.session_quota_notifications_enabled);

    let on: Settings =
        serde_json::from_str(r#"{ "enabled_providers": [], "show_notifications": true }"#)
            .expect("deserialize legacy settings with notifications on");
    assert!(on.session_quota_notifications_enabled);

    // Once the key is stored it wins over `show_notifications` in both directions.
    let explicit_on: Settings = serde_json::from_str(
        r#"{ "enabled_providers": [], "show_notifications": false,
             "session_quota_notifications_enabled": true }"#,
    )
    .expect("deserialize explicit session quota opt-in");
    assert!(explicit_on.session_quota_notifications_enabled);
    assert!(!explicit_on.show_notifications);

    let explicit_off: Settings = serde_json::from_str(
        r#"{ "enabled_providers": [], "show_notifications": true,
             "session_quota_notifications_enabled": false }"#,
    )
    .expect("deserialize explicit session quota opt-out");
    assert!(!explicit_off.session_quota_notifications_enabled);

    let round_trip: Settings =
        serde_json::from_str(&serde_json::to_string(&explicit_on).expect("serialize"))
            .expect("deserialize round trip");
    assert!(round_trip.session_quota_notifications_enabled);
    assert!(!round_trip.show_notifications);
}

/// Migration #6: adaptive refresh is the default for new installs only.
#[test]
fn adaptive_refresh_defaults_on_for_new_installs_only() {
    // No settings file: a new install.
    assert!(Settings::load_from_path(None).adaptive_refresh);
    let dir = tempfile::tempdir().expect("temp dir");
    let missing = dir.path().join("settings.json");
    assert!(Settings::load_from_path(Some(&missing)).adaptive_refresh);

    // An existing file without the key keeps the fixed interval it ran with.
    std::fs::write(&missing, "{}").expect("write legacy settings");
    let legacy = Settings::load_from_path(Some(&missing));
    assert!(!legacy.adaptive_refresh);
    assert_eq!(legacy.refresh_interval_secs, 300);

    let explicit: Settings = serde_json::from_str(r#"{ "adaptive_refresh": true }"#)
        .expect("deserialize explicit adaptive refresh");
    assert!(explicit.adaptive_refresh);
    let explicit_off: Settings = serde_json::from_str(r#"{ "adaptive_refresh": false }"#)
        .expect("deserialize explicit fixed refresh");
    assert!(!explicit_off.adaptive_refresh);
}

/// Upstream `SettingsStore.swift:737`: `statusChecksEnabled ?? true`.
#[test]
fn status_checks_default_on_for_new_and_legacy_settings() {
    assert!(Settings::default().status_checks_enabled);
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("settings.json");
    std::fs::write(&path, r#"{ "refresh_interval_secs": 600 }"#).expect("write legacy settings");
    let legacy = Settings::load_from_path(Some(&path));
    assert!(legacy.status_checks_enabled);
    assert_eq!(legacy.refresh_interval_secs, 600);

    std::fs::write(&path, r#"{ "status_checks_enabled": false }"#).expect("write explicit off");
    assert!(!Settings::load_from_path(Some(&path)).status_checks_enabled);
}

// ── Mac parity PR 3: Menu Bar / Menu pane keys ──────────────────────────

#[test]
fn menu_bar_icon_style_defaults_to_bars_for_new_installs() {
    assert_eq!(
        Settings::default().menu_bar_icon_style,
        MenuBarIconStyle::Bars
    );
}

#[test]
fn menu_bar_icon_style_migrates_from_legacy_percent_flag() {
    let legacy_percent: Settings =
        serde_json::from_str(r#"{ "enabled_providers": [], "menu_bar_shows_percent": true }"#)
            .expect("legacy percent file");
    assert_eq!(
        legacy_percent.menu_bar_icon_style,
        MenuBarIconStyle::Percent
    );

    let legacy_bars: Settings =
        serde_json::from_str(r#"{ "enabled_providers": [], "menu_bar_shows_percent": false }"#)
            .expect("legacy bars file");
    assert_eq!(legacy_bars.menu_bar_icon_style, MenuBarIconStyle::Bars);

    let legacy_missing: Settings = serde_json::from_str(r#"{ "enabled_providers": [] }"#)
        .expect("legacy file without either key");
    assert_eq!(legacy_missing.menu_bar_icon_style, MenuBarIconStyle::Bars);
}

#[test]
fn stored_menu_bar_icon_style_wins_over_the_legacy_flag() {
    let bars: Settings = serde_json::from_str(
        r#"{ "enabled_providers": [], "menu_bar_shows_percent": true, "menu_bar_icon_style": "bars" }"#,
    )
    .expect("stored bars");
    assert_eq!(bars.menu_bar_icon_style, MenuBarIconStyle::Bars);

    let percent: Settings = serde_json::from_str(
        r#"{ "enabled_providers": [], "menu_bar_shows_percent": false, "menu_bar_icon_style": "percent" }"#,
    )
    .expect("stored percent");
    assert_eq!(percent.menu_bar_icon_style, MenuBarIconStyle::Percent);

    let unknown: Settings = serde_json::from_str(
        r#"{ "enabled_providers": [], "menu_bar_shows_percent": true, "menu_bar_icon_style": "sparkles" }"#,
    )
    .expect("an unknown style must not fail the whole file");
    assert_eq!(unknown.menu_bar_icon_style, MenuBarIconStyle::Percent);
}

#[test]
fn menu_bar_icon_style_round_trips() {
    for style in [
        MenuBarIconStyle::Bars,
        MenuBarIconStyle::Percent,
        MenuBarIconStyle::StackedBars,
    ] {
        let settings = Settings {
            menu_bar_icon_style: style,
            // The legacy flag disagrees on purpose: the stored key must win.
            menu_bar_shows_percent: style == MenuBarIconStyle::Bars,
            ..Settings::default()
        };
        let json = serde_json::to_string(&settings).expect("serialize icon style");
        assert!(json.contains(&format!(r#""menu_bar_icon_style":"{}""#, style.as_str())));
        let loaded: Settings = serde_json::from_str(&json).expect("deserialize icon style");
        assert_eq!(loaded.menu_bar_icon_style, style);
    }
}

#[test]
fn stacked_bars_icon_style_loads_from_a_settings_file() {
    assert_eq!(
        MenuBarIconStyle::StackedBars.as_str(),
        "stackedBars",
        "the id is the camelCase string the bridge and the file both use"
    );
    assert_eq!(
        MenuBarIconStyle::parse("stackedBars"),
        Some(MenuBarIconStyle::StackedBars)
    );
    assert_eq!(
        MenuBarIconStyle::parse("STACKEDBARS"),
        Some(MenuBarIconStyle::StackedBars)
    );
    assert_eq!(MenuBarIconStyle::parse("stacked_bars"), None);

    let stored: Settings = serde_json::from_str(
        r#"{ "enabled_providers": [], "menu_bar_shows_percent": false, "menu_bar_icon_style": "stackedBars" }"#,
    )
    .expect("stackedBars file loads");
    assert_eq!(stored.menu_bar_icon_style, MenuBarIconStyle::StackedBars);

    // Unknown ids keep the legacy fallback, so a bad value never breaks the file.
    let unknown: Settings = serde_json::from_str(
        r#"{ "enabled_providers": [], "menu_bar_shows_percent": false, "menu_bar_icon_style": "stacked_bars" }"#,
    )
    .expect("unknown style still loads");
    assert_eq!(unknown.menu_bar_icon_style, MenuBarIconStyle::Bars);
}

#[test]
fn merged_overview_selection_defaults_empty_and_round_trips() {
    assert!(
        Settings::default()
            .merged_overview_selected_providers
            .is_empty()
    );
    let legacy: Settings =
        serde_json::from_str(r#"{ "enabled_providers": [] }"#).expect("legacy file");
    assert!(legacy.merged_overview_selected_providers.is_empty());

    let settings = Settings {
        merged_overview_selected_providers: vec!["claude".into(), "codex".into()],
        ..Settings::default()
    };
    let json = serde_json::to_string(&settings).expect("serialize overview selection");
    assert!(json.contains(r#""merged_overview_selected_providers":["claude","codex"]"#));
    let loaded: Settings = serde_json::from_str(&json).expect("deserialize overview selection");
    assert_eq!(
        loaded.merged_overview_selected_providers,
        vec!["claude".to_string(), "codex".to_string()]
    );
}

#[test]
fn merged_overview_selection_is_canonical_deduped_and_capped_at_six() {
    let loaded: Settings = serde_json::from_str(
        r#"{ "enabled_providers": [], "merged_overview_selected_providers":
            ["Codex", "codex", "not-a-provider", "claude", "gemini", "cursor", "copilot", "zai", "kimi"] }"#,
    )
    .expect("parse overview selection");
    assert_eq!(
        loaded.merged_overview_selected_providers,
        ["codex", "claude", "gemini", "cursor", "copilot", "zai"]
            .map(String::from)
            .to_vec()
    );

    let mut settings = Settings::default();
    settings.set_merged_overview_selected_providers(
        [
            "codex", "claude", "gemini", "cursor", "copilot", "zai", "kimi",
        ]
        .map(String::from)
        .to_vec(),
    );
    assert_eq!(
        settings.merged_overview_selected_providers.len(),
        MERGED_OVERVIEW_PROVIDER_LIMIT
    );
}

#[test]
fn quota_warning_markers_default_on_and_round_trip() {
    assert!(Settings::default().quota_warning_markers_visible);
    let legacy: Settings =
        serde_json::from_str(r#"{ "enabled_providers": [] }"#).expect("legacy file");
    assert!(legacy.quota_warning_markers_visible);

    let hidden = Settings {
        quota_warning_markers_visible: false,
        ..Settings::default()
    };
    let json = serde_json::to_string(&hidden).expect("serialize markers");
    assert!(json.contains(r#""quota_warning_markers_visible":false"#));
    let loaded: Settings = serde_json::from_str(&json).expect("deserialize markers");
    assert!(!loaded.quota_warning_markers_visible);
}

#[test]
fn highlight_menu_cards_on_hover_defaults_on_and_round_trips() {
    assert!(Settings::default().highlight_menu_cards_on_hover);
    let legacy: Settings =
        serde_json::from_str(r#"{ "enabled_providers": [] }"#).expect("legacy file");
    assert!(legacy.highlight_menu_cards_on_hover);

    let off = Settings {
        highlight_menu_cards_on_hover: false,
        ..Settings::default()
    };
    let json = serde_json::to_string(&off).expect("serialize hover highlight");
    assert!(json.contains(r#""highlight_menu_cards_on_hover":false"#));
    let loaded: Settings = serde_json::from_str(&json).expect("deserialize hover highlight");
    assert!(!loaded.highlight_menu_cards_on_hover);
}

#[test]
fn workday_tick_style_defaults_subtle_and_round_trips() {
    assert_eq!(
        Settings::default().workday_tick_style,
        WorkdayTickStyle::Subtle
    );
    let legacy: Settings =
        serde_json::from_str(r#"{ "enabled_providers": [] }"#).expect("legacy file");
    assert_eq!(legacy.workday_tick_style, WorkdayTickStyle::Subtle);

    for style in [
        WorkdayTickStyle::Hidden,
        WorkdayTickStyle::Subtle,
        WorkdayTickStyle::HighContrast,
    ] {
        let settings = Settings {
            workday_tick_style: style,
            ..Settings::default()
        };
        let json = serde_json::to_string(&settings).expect("serialize tick style");
        assert!(json.contains(&format!(r#""workday_tick_style":"{}""#, style.as_str())));
        let loaded: Settings = serde_json::from_str(&json).expect("deserialize tick style");
        assert_eq!(loaded.workday_tick_style, style);
    }

    let unknown: Settings =
        serde_json::from_str(r#"{ "enabled_providers": [], "workday_tick_style": "neon" }"#)
            .expect("an unknown tick style must not fail the whole file");
    assert_eq!(unknown.workday_tick_style, WorkdayTickStyle::Subtle);
}

#[test]
fn weekly_progress_work_days_accepts_two_through_seven() {
    for days in 2..=7u8 {
        let loaded: Settings = serde_json::from_str(&format!(
            r#"{{ "enabled_providers": [], "weekly_progress_work_days": {days} }}"#
        ))
        .expect("parse work days");
        assert_eq!(loaded.weekly_progress_work_days, Some(days));
    }
    for days in [0u8, 1, 8, 200] {
        let loaded: Settings = serde_json::from_str(&format!(
            r#"{{ "enabled_providers": [], "weekly_progress_work_days": {days} }}"#
        ))
        .expect("parse out-of-range work days");
        assert_eq!(
            loaded.weekly_progress_work_days, None,
            "{days} is out of range"
        );
    }
}

#[test]
fn minimal_display_mode_folds_into_show_most_used_once() {
    let dir = tempfile::tempdir().expect("tempdir");
    let marker = dir.path().join(".display-mode-fold-v1");
    let mut settings: Settings =
        serde_json::from_str(r#"{ "enabled_providers": [], "menu_bar_display_mode": "minimal" }"#)
            .expect("legacy minimal file");
    assert!(!settings.menu_bar_shows_highest_usage);

    let mut persisted = 0;
    settings.apply_display_mode_fold_with_marker(&marker, |_| {
        persisted += 1;
        Ok(())
    });
    assert!(settings.menu_bar_shows_highest_usage);
    assert_eq!(persisted, 1, "the fold is saved once");
    assert!(marker.exists(), "the marker is written");
    // The field stays readable for one release.
    assert_eq!(settings.menu_bar_display_mode, "minimal");

    // The user turns "Show most-used provider" off afterwards: it stays off.
    settings.menu_bar_shows_highest_usage = false;
    settings.apply_display_mode_fold_with_marker(&marker, |_| {
        persisted += 1;
        Ok(())
    });
    assert!(!settings.menu_bar_shows_highest_usage);
    assert_eq!(persisted, 1, "no second save once the marker exists");
}

#[test]
fn display_mode_fold_leaves_other_modes_alone_but_writes_the_marker() {
    let dir = tempfile::tempdir().expect("tempdir");
    let marker = dir.path().join(".display-mode-fold-v1");
    let mut settings: Settings =
        serde_json::from_str(r#"{ "enabled_providers": [], "menu_bar_display_mode": "detailed" }"#)
            .expect("detailed file");
    let mut persisted = 0;
    settings.apply_display_mode_fold_with_marker(&marker, |_| {
        persisted += 1;
        Ok(())
    });
    assert!(!settings.menu_bar_shows_highest_usage);
    assert_eq!(persisted, 0);
    assert!(marker.exists());

    // A file switched to minimal after the marker exists is not folded.
    settings.menu_bar_display_mode = "minimal".into();
    settings.apply_display_mode_fold_with_marker(&marker, |_| {
        persisted += 1;
        Ok(())
    });
    assert!(!settings.menu_bar_shows_highest_usage);
    assert_eq!(persisted, 0);
}

#[test]
fn display_mode_fold_retries_when_the_save_fails() {
    let dir = tempfile::tempdir().expect("tempdir");
    let marker = dir.path().join(".display-mode-fold-v1");
    let mut settings: Settings =
        serde_json::from_str(r#"{ "enabled_providers": [], "menu_bar_display_mode": "minimal" }"#)
            .expect("legacy minimal file");

    settings.apply_display_mode_fold_with_marker(&marker, |_| anyhow::bail!("disk full"));
    assert!(!marker.exists(), "no marker after a failed save");

    let mut reloaded: Settings =
        serde_json::from_str(r#"{ "enabled_providers": [], "menu_bar_display_mode": "minimal" }"#)
            .expect("legacy minimal file");
    let mut persisted = 0;
    reloaded.apply_display_mode_fold_with_marker(&marker, |_| {
        persisted += 1;
        Ok(())
    });
    assert!(reloaded.menu_bar_shows_highest_usage);
    assert_eq!(persisted, 1, "the next launch saves the fold");
    assert!(marker.exists());
}

#[test]
fn stacked_tray_mode_survives_load_with_the_new_keys() {
    let loaded: Settings = serde_json::from_str(
        r#"{ "enabled_providers": ["codex", "claude"], "tray_icon_mode": "stacked",
             "menu_bar_icon_style": "percent", "workday_tick_style": "high_contrast" }"#,
    )
    .expect("stacked file");
    assert_eq!(loaded.tray_icon_mode, TrayIconMode::Stacked);
    let json = serde_json::to_string(&loaded).expect("serialize");
    let reloaded: Settings = serde_json::from_str(&json).expect("reload");
    assert_eq!(reloaded.tray_icon_mode, TrayIconMode::Stacked);
    assert_eq!(reloaded.menu_bar_icon_style, MenuBarIconStyle::Percent);
    assert_eq!(reloaded.workday_tick_style, WorkdayTickStyle::HighContrast);
}

/// PR 4 / Q10: "Track costs" is on for new installs, and a file written before
/// the key existed keeps scanning cost logs as it always did.
#[test]
fn cost_usage_enabled_defaults_on_for_new_installs_and_legacy_files() {
    let dir = tempfile::tempdir().unwrap();
    let new_install = dir.path().join("missing.json");
    let without_key = dir.path().join("settings.json");
    std::fs::write(&without_key, r#"{ "enabled_providers": ["codex"] }"#).unwrap();

    assert!(Settings::default().cost_usage_enabled);
    assert!(Settings::load_from_path(Some(&new_install)).cost_usage_enabled);
    assert!(Settings::load_from_path(Some(&without_key)).cost_usage_enabled);
}

#[test]
fn cost_usage_enabled_off_round_trips_through_settings_persistence() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let settings = Settings {
        cost_usage_enabled: false,
        ..Settings::default()
    };
    settings.save_to_path(&path).unwrap();
    assert!(!Settings::load_from_path(Some(&path)).cost_usage_enabled);
}

/// A file without `spend_dashboard_period` copies the menu period, so the
/// dashboard keeps showing the range it showed before the split.
#[test]
fn spend_dashboard_period_copies_cost_reporting_period_when_missing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    std::fs::write(
        &path,
        r#"{ "enabled_providers": [], "cost_reporting_period": "month-to-date" }"#,
    )
    .unwrap();
    let loaded = Settings::load_from_path(Some(&path));
    assert_eq!(
        loaded.spend_dashboard_period,
        CostReportingPeriod::MonthToDate
    );
    assert_eq!(
        loaded.cost_reporting_period,
        CostReportingPeriod::MonthToDate
    );

    let null_key: Settings = serde_json::from_str(
        r#"{ "cost_reporting_period": "all", "spend_dashboard_period": null }"#,
    )
    .unwrap();
    assert_eq!(
        null_key.spend_dashboard_period,
        CostReportingPeriod::AllAvailable
    );

    assert_eq!(
        Settings::default().spend_dashboard_period,
        CostReportingPeriod::Rolling(30)
    );
}

#[test]
fn spend_dashboard_period_round_trips_apart_from_cost_reporting_period() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    for period in [
        CostReportingPeriod::Rolling(7),
        CostReportingPeriod::Rolling(90),
        CostReportingPeriod::MonthToDate,
        CostReportingPeriod::AllAvailable,
    ] {
        let settings = Settings {
            cost_reporting_period: CostReportingPeriod::Rolling(30),
            spend_dashboard_period: period,
            ..Settings::default()
        };
        settings.save_to_path(&path).unwrap();
        let loaded = Settings::load_from_path(Some(&path));
        assert_eq!(loaded.spend_dashboard_period, period);
        assert_eq!(
            loaded.cost_reporting_period,
            CostReportingPeriod::Rolling(30)
        );
    }
}

/// An unreadable saved dashboard period reads as the default (`rolling:30`),
/// like `cost_reporting_period`; it does not copy the menu period.
#[test]
fn unreadable_spend_dashboard_period_loads_as_the_default() {
    let loaded: Settings = serde_json::from_str(
        r#"{ "cost_reporting_period": "all", "spend_dashboard_period": "weekly" }"#,
    )
    .expect("an unreadable period must not fail the whole settings file");
    assert_eq!(
        loaded.spend_dashboard_period,
        CostReportingPeriod::Rolling(30)
    );
}

#[test]
fn spend_dashboard_hidden_sources_default_empty_normalize_and_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    std::fs::write(&path, r#"{ "enabled_providers": [] }"#).unwrap();
    assert!(
        Settings::load_from_path(Some(&path))
            .spend_dashboard_hidden_source_ids
            .is_empty()
    );
    assert!(
        Settings::default()
            .spend_dashboard_hidden_source_ids
            .is_empty()
    );

    std::fs::write(
        &path,
        r#"{ "spend_dashboard_hidden_source_ids": ["pi", " Codex", "", "pi"] }"#,
    )
    .unwrap();
    let loaded = Settings::load_from_path(Some(&path));
    assert_eq!(loaded.spend_dashboard_hidden_source_ids, ["codex", "pi"]);

    loaded.save_to_path(&path).unwrap();
    assert_eq!(
        Settings::load_from_path(Some(&path)).spend_dashboard_hidden_source_ids,
        ["codex", "pi"]
    );
}

#[test]
fn quota_warning_migration_saves_a_legacy_file_once_without_the_old_key() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("settings.json");
    std::fs::write(
        &path,
        r#"{ "provider_usage_thresholds": { "codex:session": { "high": 80.0 } } }"#,
    )
    .expect("seed legacy file");
    let mut settings = Settings::load_from_path(Some(&path));
    assert!(settings.legacy_usage_thresholds_pending);

    let mut saves = 0;
    settings.apply_quota_warning_migration_with(|s| {
        saves += 1;
        s.save_to_path(&path)
    });
    assert_eq!(saves, 1);
    assert!(!settings.legacy_usage_thresholds_pending);
    let saved: serde_json::Value =
        serde_json::from_str(&crate::secure_file::read_string(&path).expect("read back"))
            .expect("saved json");
    assert!(saved.get("provider_usage_thresholds").is_none());
    assert_eq!(
        saved["provider_configs"]["codex"]["quota_warnings"]["session"]["high"],
        serde_json::json!(80.0)
    );

    // The rewritten file no longer asks for a save.
    let mut reloaded = Settings::load_from_path(Some(&path));
    assert!(!reloaded.legacy_usage_thresholds_pending);
    reloaded.apply_quota_warning_migration_with(|_| {
        saves += 1;
        Ok(())
    });
    assert_eq!(saves, 1);
}

#[test]
fn quota_warning_migration_retries_after_a_failed_save() {
    let mut settings = load_settings_json(r#"{ "provider_usage_thresholds": { "claude": {} } }"#);
    assert!(settings.legacy_usage_thresholds_pending);
    settings.apply_quota_warning_migration_with(|_| Err(anyhow::anyhow!("disk full")));
    assert!(settings.legacy_usage_thresholds_pending);
    assert!(!Settings::default().legacy_usage_thresholds_pending);
}

#[test]
fn custom_quota_warnings_with_explicit_enabled_round_trip() {
    let mut settings = Settings::default();
    let window = QuotaWarningWindow {
        enabled: Some(true),
        high: Some(55.0),
        critical: Some(85.0),
    };
    settings.set_quota_warnings(
        ProviderId::Codex,
        ProviderQuotaWarnings {
            session: window,
            weekly: QuotaWarningWindow::default(),
        },
    );
    let reloaded = load_settings_json(&serde_json::to_string(&settings).expect("serialize"));
    assert_eq!(reloaded.quota_warnings(ProviderId::Codex).session, window);
    assert!(reloaded.quota_warning_enabled(ProviderId::Codex, "session"));
    assert_eq!(
        reloaded.usage_thresholds(ProviderId::Codex, "session"),
        UsageThresholds {
            high: 55.0,
            critical: 85.0,
        }
    );
}

#[test]
fn antigravity_additional_profile_homes_default_empty_and_round_trip() {
    let defaulted: Settings = serde_json::from_str(r#"{ "enabled_providers": [] }"#)
        .expect("missing additional Gemini homes default to empty");
    assert!(defaulted.antigravity_additional_profile_homes.is_empty());

    let json = serde_json::to_string(&Settings::default()).expect("serialize default settings");
    assert!(!json.contains("antigravity_additional_profile_homes"));

    let configured = Settings {
        antigravity_additional_profile_homes: vec!["D:\\gemini-work".to_string()],
        ..Settings::default()
    };
    let json = serde_json::to_string(&configured).expect("serialize additional Gemini homes");
    assert!(json.contains(r#""antigravity_additional_profile_homes":["D:\\gemini-work"]"#));

    let loaded: Settings =
        serde_json::from_str(&json).expect("deserialize additional Gemini homes");
    assert_eq!(
        loaded.antigravity_additional_profile_homes,
        vec!["D:\\gemini-work"]
    );
}
