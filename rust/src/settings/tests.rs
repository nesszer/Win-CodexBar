use super::*;

mod language_theme;
mod provider_configs;

/// Settings parsed from a document that omits every optional field.
fn defaulted() -> Settings {
    serde_json::from_str(r#"{ "enabled_providers": [] }"#).expect("minimal settings parse")
}

/// Serialize then parse back; the JSON is returned for field-name checks.
fn round_trip(settings: &Settings) -> (String, Settings) {
    let json = serde_json::to_string(settings).expect("serialize settings");
    let loaded = serde_json::from_str(&json).expect("deserialize settings");
    (json, loaded)
}

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
fn preferred_currency_defaults_validates_and_round_trips() {
    let legacy = defaulted();
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
    let defaulted = defaulted();
    assert_eq!(defaulted.overview_layout, "detailed");

    let compact = Settings {
        overview_layout: "compact".to_string(),
        ..Settings::default()
    };
    let (_, loaded) = round_trip(&compact);
    assert_eq!(loaded.overview_layout, "compact");

    let unknown: Settings =
        serde_json::from_str(r#"{ "enabled_providers": [], "overview_layout": "unsupported" }"#)
            .expect("unknown overview layout is accepted and normalized");
    assert_eq!(unknown.overview_layout, "detailed");
}

#[test]
fn tray_panel_always_on_top_defaults_off_and_round_trips() {
    let defaulted = defaulted();
    assert!(!defaulted.tray_panel_always_on_top);

    let enabled = Settings {
        tray_panel_always_on_top: true,
        ..Settings::default()
    };
    let (json, loaded) = round_trip(&enabled);
    assert!(json.contains(r#""tray_panel_always_on_top":true"#));

    assert!(loaded.tray_panel_always_on_top);
}

#[test]
fn low_power_mode_migrates_legacy_boolean_and_round_trips_preference() {
    let defaulted = defaulted();
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
    let (json, loaded) = round_trip(&automatic);
    assert!(json.contains(r#""low_power_mode_preference":"automatic""#));
    assert_eq!(
        loaded.low_power_mode_preference,
        LowPowerModePreference::Automatic
    );
}

#[test]
fn open_codex_usage_logs_default_off_and_round_trip() {
    let defaulted = defaulted();
    assert!(!defaulted.open_codex_usage_logs_enabled);

    let enabled = Settings {
        open_codex_usage_logs_enabled: true,
        hide_native_codex_cost_when_open_codex_present: true,
        ..Settings::default()
    };
    let (json, loaded) = round_trip(&enabled);
    assert!(json.contains(r#""open_codex_usage_logs_enabled":true"#));

    assert!(loaded.open_codex_usage_logs_enabled);
    assert!(loaded.hide_native_codex_cost_when_open_codex_present);
}

#[test]
fn tray_pace_color_defaults_off_and_round_trips() {
    let defaulted = defaulted();
    assert!(!defaulted.menu_bar_color_pace);

    let enabled = Settings {
        menu_bar_color_pace: true,
        ..Settings::default()
    };
    let (json, loaded) = round_trip(&enabled);
    assert!(json.contains(r#""menu_bar_color_pace":true"#));

    assert!(loaded.menu_bar_color_pace);
}

#[test]
fn cost_reporting_period_defaults_to_thirty_days_and_round_trips() {
    let defaulted = defaulted();
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
        let (json, loaded) = round_trip(&settings);
        assert!(json.contains(&format!(r#""cost_reporting_period":"{}""#, period.raw())));
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
    let (json, loaded) = round_trip(&settings);
    assert!(json.contains("\"criticalUsage\":\"C:\\\\sounds\\\\critical.wav\""));

    assert_eq!(
        loaded.notification_sound_paths,
        settings.notification_sound_paths
    );
    assert_eq!(
        loaded.notification_sound_theme,
        NotificationSoundTheme::CodexBar
    );

    let legacy = defaulted();
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
}

#[test]
fn usage_thresholds_inherit_from_window_provider_and_global_levels() {
    let mut settings = Settings::default();
    settings.provider_usage_thresholds.insert(
        "codex".into(),
        UsageThresholdOverride {
            high: Some(75.0),
            critical: None,
        },
    );
    settings.provider_usage_thresholds.insert(
        "codex:weekly".into(),
        UsageThresholdOverride {
            high: None,
            critical: Some(95.0),
        },
    );

    assert_eq!(
        settings.usage_thresholds(ProviderId::Codex, "weekly"),
        UsageThresholds {
            high: 75.0,
            critical: 95.0,
        }
    );
    assert_eq!(
        settings.usage_thresholds(ProviderId::Claude, "session"),
        UsageThresholds {
            high: 70.0,
            critical: 90.0,
        }
    );
}

#[test]
fn empty_and_out_of_range_threshold_overrides_are_normalized_on_load() {
    let loaded: Settings = serde_json::from_str(
        r#"{
            "provider_usage_thresholds": {
                "codex": {"high": 120.0},
                "claude": {},
                "codex:weekly": {"critical": -10.0}
            }
        }"#,
    )
    .expect("parse settings");

    assert_eq!(loaded.provider_usage_thresholds.len(), 2);
    assert_eq!(loaded.provider_usage_thresholds["codex"].high, Some(100.0));
    assert_eq!(
        loaded.provider_usage_thresholds["codex:weekly"].critical,
        Some(0.0)
    );
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
fn clamp_helpers_pin_to_supported_ranges() {
    // (input, expected) per helper: below range lifts to the floor, in range
    // passes through, above range drops to the ceiling.
    for (input, expected) in [
        (0, 100),
        (99, 100),
        (100, 100),
        (125, 125),
        (180, 180),
        (250, 250),
        (251, 250),
    ] {
        assert_eq!(
            clamp_window_scale_percent(input),
            expected,
            "window scale {input}"
        );
    }
    for (input, expected) in [
        (0, 100),
        (99, 100),
        (100, 100),
        (125, 125),
        (180, 180),
        (200, 200),
        (201, 200),
    ] {
        assert_eq!(
            clamp_tray_scale_percent(input),
            expected,
            "tray scale {input}"
        );
    }
    // Opacity floors at 30 so the bar isn't accidentally invisible.
    for (input, expected) in [
        (0, 30),
        (29, 30),
        (45, 45),
        (80, 80),
        (150, 100),
        (255, 100),
    ] {
        assert_eq!(
            clamp_float_bar_opacity(input),
            expected,
            "float bar opacity {input}"
        );
    }
    for (input, expected) in [(0, 75), (74, 75), (100, 100), (150, 150), (250, 200)] {
        assert_eq!(
            clamp_float_bar_scale(input),
            expected,
            "float bar scale {input}"
        );
    }
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

    let (_, back) = round_trip(&s);
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

    let (_, loaded) = round_trip(&settings);
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
