use super::*;

mod language_theme;
mod metric_preference;
mod provider_configs;
mod unreadable_file;

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
    assert_eq!(settings.predictive_pace_warning_lead_hours, 0);
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
fn predictive_pace_warning_lead_hours_round_trips_and_is_capped_at_a_week() {
    let defaulted = defaulted();
    assert_eq!(defaulted.predictive_pace_warning_lead_hours, 0);
    assert!(defaulted.predictive_pace_warning_lead_seconds().is_none());

    let within_a_day = Settings {
        predictive_pace_warning_lead_hours: 24,
        ..Settings::default()
    };
    let (json, loaded) = round_trip(&within_a_day);
    assert!(json.contains(r#""predictive_pace_warning_lead_hours":24"#));
    assert_eq!(loaded.predictive_pace_warning_lead_hours, 24);
    assert_eq!(
        loaded.predictive_pace_warning_lead_seconds(),
        Some(86_400.0)
    );

    let too_long = Settings {
        predictive_pace_warning_lead_hours: 500,
        ..Settings::default()
    };
    let (_, loaded) = round_trip(&too_long);
    assert_eq!(loaded.predictive_pace_warning_lead_hours, 168);
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
fn agent_sessions_local_only_defaults_off_and_round_trips() {
    // Off keeps the existing remote discovery behavior for every install.
    assert!(!defaulted().agent_sessions_local_only);

    let local_only = Settings {
        agent_sessions_enabled: true,
        agent_sessions_local_only: true,
        ..Settings::default()
    };
    let (json, loaded) = round_trip(&local_only);
    assert!(json.contains(r#""agent_sessions_local_only":true"#));
    assert!(loaded.agent_sessions_enabled);
    assert!(loaded.agent_sessions_local_only);
}

#[test]
fn agent_sessions_remote_discovery_requires_enabled_and_not_local_only() {
    // Fail closed: the default (and any unreadable file) never allows remote discovery.
    assert!(!defaulted().agent_sessions_remote_allowed());

    let enabled = Settings {
        agent_sessions_enabled: true,
        ..Settings::default()
    };
    assert!(enabled.agent_sessions_remote_allowed());

    let local_only = Settings {
        agent_sessions_enabled: true,
        agent_sessions_local_only: true,
        ..Settings::default()
    };
    assert!(!local_only.agent_sessions_remote_allowed());

    let local_only_while_disabled = Settings {
        agent_sessions_local_only: true,
        ..Settings::default()
    };
    assert!(!local_only_while_disabled.agent_sessions_remote_allowed());
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
    assert!(!loaded.limit_reset_notifications_enabled);
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
fn provider_default_order_ignores_saved_order_and_hides_unlisted_providers() {
    let listed_in_canonical_order: Vec<String> = ProviderId::all()
        .iter()
        .filter(|provider| !provider.is_deprecated())
        .map(|provider| provider.cli_name().to_string())
        .collect();

    let defaults = Settings::default();
    assert_eq!(
        defaults.provider_default_order_names(),
        listed_in_canonical_order
    );

    let customized = Settings {
        provider_order: normalize_provider_order(&["gemini".to_string(), "claude".to_string()]),
        ..Settings::default()
    };
    assert_eq!(
        customized.provider_default_order_names(),
        listed_in_canonical_order
    );

    let with_deprecated_enabled = Settings {
        enabled_providers: [ProviderId::KimiK2.cli_name().to_string()]
            .into_iter()
            .collect(),
        ..Settings::default()
    };
    assert!(
        with_deprecated_enabled
            .provider_default_order_names()
            .contains(&ProviderId::KimiK2.cli_name().to_string())
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

/// Characterization pin for the parse errors `codexbar config validate` shows: the message and the
/// line/column must not change when the deserializer is restructured.
#[test]
fn settings_parse_errors_are_pinned() {
    let cases = [
        // Wrongly typed canonical field.
        "{\n  \"refresh_interval_secs\": \"soon\"\n}",
        // Wrongly typed legacy flat field.
        "{\n  \"codex_cookie_source\": 5\n}",
        // Wrong type inside provider_configs.
        "{\n  \"provider_configs\": { \"codex\": { \"cookie_source\": 5 } }\n}",
        // Canonical field error after legacy and unknown keys.
        "{\n  \"claude_cookie_source\": \"auto\",\n  \"not_a_setting\": 1,\n  \"theme\": 7\n}",
        // Truncated document.
        "{\n  \"theme\": \"dark\",",
    ];
    let errors: Vec<String> = cases
        .iter()
        .map(|json| {
            serde_json::from_str::<Settings>(json)
                .unwrap_err()
                .to_string()
        })
        .collect();
    assert_eq!(
        errors,
        [
            "invalid type: string \"soon\", expected u64 at line 2 column 33",
            "invalid type: integer `5`, expected a string at line 2 column 26",
            "invalid type: integer `5`, expected a string at line 2 column 53",
            "expected value at line 4 column 12",
            "EOF while parsing a value at line 2 column 18",
        ]
    );
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
fn claude_platform_cookie_source_defaults_to_automatic_session_import() {
    let mut settings = Settings::default();
    assert_eq!(settings.cookie_source(ProviderId::ClaudePlatform), "auto");
    settings.set_cookie_source(ProviderId::ClaudePlatform, "manual");
    assert_eq!(settings.cookie_source(ProviderId::ClaudePlatform), "manual");
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
fn merged_overview_all_accounts_defaults_off_and_round_trips() {
    assert!(!Settings::default().merged_overview_all_accounts);
    let legacy: Settings =
        serde_json::from_str(r#"{ "enabled_providers": [] }"#).expect("legacy file");
    assert!(!legacy.merged_overview_all_accounts);

    let on = Settings {
        merged_overview_all_accounts: true,
        ..Settings::default()
    };
    let json = serde_json::to_string(&on).expect("serialize overview accounts");
    assert!(json.contains(r#""merged_overview_all_accounts":true"#));
    let loaded: Settings = serde_json::from_str(&json).expect("deserialize overview accounts");
    assert!(loaded.merged_overview_all_accounts);
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
