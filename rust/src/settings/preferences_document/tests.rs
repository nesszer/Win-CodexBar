use super::*;
use crate::settings::{
    Language, MenuBarIconStyle, ProviderConfig, ProviderQuotaWarnings, QuotaWarningWindow,
    ThemePreference, TrayIconMode,
};
use serde_json::json;
use std::collections::BTreeMap;

fn document(preferences: Value) -> String {
    json!({ "version": 1, "preferences": preferences }).to_string()
}

fn parse(preferences: Value) -> Result<PreferencesDocument, PreferencesError> {
    PreferencesDocument::from_json(&document(preferences))
}

fn max_document_bytes() -> usize {
    usize::try_from(MAX_PREFERENCES_DOCUMENT_BYTES).expect("limit fits in usize")
}

/// Settings as JSON with the set-valued `enabled_providers` in stable order.
fn snapshot(settings: &Settings) -> Value {
    let mut value = serde_json::to_value(settings).expect("serialize settings");
    if let Some(Value::Array(ids)) = value.get_mut("enabled_providers") {
        ids.sort_by(|a, b| a.as_str().cmp(&b.as_str()));
    }
    value
}

fn customized_settings() -> Settings {
    let mut settings = Settings {
        refresh_interval_secs: 900,
        adaptive_refresh: true,
        refresh_all_providers_on_menu_open: true,
        status_checks_enabled: false,
        low_power_mode_preference: crate::settings::LowPowerModePreference::Automatic,
        show_notifications: false,
        session_quota_notifications_enabled: false,
        sound_enabled: false,
        high_usage_threshold: 55.5,
        critical_usage_threshold: 80.0,
        predictive_pace_warning_enabled: true,
        limit_reset_notifications_enabled: true,
        show_pace: false,
        show_as_used: false,
        reset_time_relative: false,
        show_reset_when_exhausted: true,
        hide_personal_info: true,
        menu_bar_shows_highest_usage: true,
        menu_bar_shows_percent: true,
        menu_bar_icon_style: crate::settings::MenuBarIconStyle::Percent,
        menu_bar_color_by_provider: true,
        merged_overview_selected_providers: vec!["claude".to_string(), "codex".to_string()],
        quota_warning_markers_visible: false,
        workday_tick_style: crate::settings::WorkdayTickStyle::HighContrast,
        tray_icon_mode: TrayIconMode::PerProvider,
        switcher_shows_icons: false,
        providers_sorted_alphabetically: true,
        overview_layout: "detailed".to_string(),
        menu_bar_display_mode: "minimal".to_string(),
        // Loading normalizes the order to the full canonical list.
        provider_order: crate::settings::normalize_provider_order(&[
            "codex".to_string(),
            "claude".to_string(),
        ]),
        theme: ThemePreference::Dark,
        ui_language: Language::Japanese,
        ui_language_follows_system: false,
        window_scale_percent: 150,
        tray_scale_percent: 120,
        float_bar_opacity: 60,
        float_bar_scale: 120,
        float_bar_orientation: "vertical".to_string(),
        float_bar_style: "taskbar".to_string(),
        float_bar_dark_text: true,
        float_bar_show_reset_inline: true,
        float_bar_show_cost: true,
        cost_reporting_period: CostReportingPeriod::MonthToDate,
        cost_usage_enabled: false,
        spend_dashboard_period: CostReportingPeriod::Rolling(90),
        spend_dashboard_hidden_source_ids: vec!["claude".to_string(), "pi".to_string()],
        preferred_currency_code: "EUR".to_string(),
        switcher_shortcuts: BTreeMap::from([
            ("next".to_string(), "none".to_string()),
            ("previous".to_string(), "alt+left".to_string()),
        ]),
        ..Settings::default()
    };
    settings.enabled_providers = ["claude", "codex", "zai"].map(String::from).into();
    settings.provider_metrics.insert(
        "claude".to_string(),
        crate::settings::MetricPreference::Weekly,
    );
    settings
}

/// Settings whose secret and machine-specific fields are all populated.
fn secret_settings() -> Settings {
    let mut settings = customized_settings();
    settings.provider_configs.insert(
        ProviderId::Claude,
        ProviderConfig {
            api_token: Some("SECRET-API-TOKEN".to_string()),
            manual_cookie_header: Some("sessionKey=SECRET-COOKIE".to_string()),
            management_api_token: Some("SECRET-MGMT".to_string()),
            ..ProviderConfig::default()
        },
    );
    settings.http_proxy_enabled = true;
    settings.http_proxy_url = "http://proxy.invalid:8080".to_string();
    settings.http_proxy_username = "SECRET-PROXY-USER".to_string();
    settings.http_proxy_password = "SECRET-PROXY-PASSWORD".to_string();
    settings.codex_custom_sessions_dirs = vec!["C:\\secret\\sessions".to_string()];
    settings.antigravity_additional_profile_homes = vec!["C:\\secret\\gemini-home".to_string()];
    settings.agent_session_ssh_hosts = vec!["secret-host.invalid".to_string()];
    settings.notification_sound_paths.high_usage = Some("C:\\secret\\ding.wav".to_string());
    settings.cost_usage_bucket_time_zone = "Asia/Tokyo".to_string();
    settings.stay_awake_enabled = true;
    settings.credential_expiry_notifications_enabled = true;
    settings.menu_bar_color_pace = true;
    settings.menu_bar_pace_reserve_positive = true;
    settings.stacked_tray_top_provider = Some("claude".to_string());
    settings.stacked_tray_bottom_provider = Some("codex".to_string());
    settings
}

#[test]
fn round_trips_every_allowlisted_preference() {
    let source = customized_settings();
    let exported = PreferencesDocument::from_settings(&source).expect("export");
    assert_eq!(
        exported.len(),
        ALLOWED_PREFERENCES.len() + PROVIDER_PREFERENCES.len()
    );

    let reparsed = PreferencesDocument::from_json(&exported.to_json()).expect("reimport");
    assert_eq!(reparsed, exported);

    let mut target = Settings {
        start_at_login: true,
        global_shortcut: "Ctrl+Alt+K".to_string(),
        ..Settings::default()
    };
    let applied = reparsed.apply_to(&mut target).expect("apply");
    assert_eq!(
        applied,
        ALLOWED_PREFERENCES.len() + PROVIDER_PREFERENCES.len()
    );

    let again = PreferencesDocument::from_settings(&target).expect("re-export");
    assert_eq!(again, exported);
    assert_eq!(target.window_scale_percent, 150);
    assert_eq!(target.ui_language, Language::Japanese);
    assert!(!target.status_checks_enabled);
    assert!(again.to_json().contains("\"status_checks_enabled\": false"));
    // Portable like the Mac PreferencesDocument: depleted/restored alerts travel.
    assert!(!target.session_quota_notifications_enabled);
    assert_eq!(
        target.cost_reporting_period,
        CostReportingPeriod::MonthToDate
    );
    assert!(!target.cost_usage_enabled);
    assert_eq!(
        target.spend_dashboard_period,
        CostReportingPeriod::Rolling(90)
    );
    assert_eq!(target.spend_dashboard_hidden_source_ids, ["claude", "pi"]);
    assert_eq!(target.preferred_currency_code, "EUR");
    assert_eq!(
        target
            .switcher_shortcuts
            .get("previous")
            .map(String::as_str),
        Some("alt+left")
    );
    assert_eq!(
        target.enabled_providers,
        ["claude", "codex", "zai"].map(String::from).into()
    );
    // Non-portable state on the receiving machine is untouched.
    assert!(target.start_at_login);
    assert_eq!(target.global_shortcut, "Ctrl+Alt+K");
}

#[test]
fn menu_bar_icon_style_import_accepts_stacked_bars_and_rejects_junk() {
    let document =
        parse(json!({ "menu_bar_icon_style": "stackedBars" })).expect("stackedBars imports");
    let mut target = Settings::default();
    document.apply_to(&mut target).expect("apply");
    assert_eq!(target.menu_bar_icon_style, MenuBarIconStyle::StackedBars);

    let rejected = parse(json!({ "menu_bar_icon_style": "stacked_bars" }));
    assert!(
        rejected.is_err(),
        "unknown style must be rejected on import"
    );
}

#[test]
fn export_is_deterministic_and_sorted() {
    let first = PreferencesDocument::from_settings(&customized_settings())
        .expect("export")
        .to_json();
    let second = PreferencesDocument::from_settings(&customized_settings())
        .expect("export")
        .to_json();
    assert_eq!(first, second);
    assert!(first.ends_with("}\n"));
    let claude = first.find("\"claude\"").expect("claude listed");
    let codex = first.find("\"codex\"").expect("codex listed");
    let zai = first.find("\"zai\"").expect("zai listed");
    assert!(claude < codex && codex < zai);
}

#[test]
fn export_skips_provider_preferences_unknown_to_this_build() {
    let mut settings = customized_settings();
    settings
        .enabled_providers
        .insert("future-provider".to_string());
    settings.provider_metrics.insert(
        "future-provider".to_string(),
        crate::settings::MetricPreference::Weekly,
    );

    let exported = PreferencesDocument::from_settings(&settings)
        .expect("stale provider IDs do not prevent export");
    let json: Value = serde_json::from_str(&exported.to_json()).expect("valid JSON");
    let preferences = json.get("preferences").expect("preferences object");

    assert!(
        !preferences["enabled_providers"]
            .as_array()
            .expect("provider list")
            .iter()
            .any(|id| id == "future-provider")
    );
    assert!(
        preferences["provider_metrics"]
            .get("future-provider")
            .is_none()
    );
    assert!(preferences["provider_metrics"].get("claude").is_some());
    // Per-provider quota warnings live in `provider_configs`, which is not
    // portable, so the legacy threshold key is no longer exported.
    assert!(preferences.get("provider_usage_thresholds").is_none());
}

#[test]
fn importing_legacy_threshold_overrides_migrates_them_to_quota_warnings() {
    let mut settings = Settings::default();
    parse(json!({
        "provider_usage_thresholds": {
            "codex:session": { "high": 55 },
            "claude": { "critical": 97 }
        }
    }))
    .expect("legacy key still validates")
    .apply_to(&mut settings)
    .expect("apply");

    let codex = settings.quota_warnings(crate::core::ProviderId::Codex);
    assert_eq!(codex.session.high, Some(55.0));
    assert!(codex.weekly.is_global());
    let claude = settings.quota_warnings(crate::core::ProviderId::Claude);
    assert_eq!(claude.session.critical, Some(97.0));
    assert_eq!(claude.weekly.critical, Some(97.0));
    assert_eq!(claude.weekly.high, None);
}

fn quota_window(
    enabled: Option<bool>,
    high: Option<f64>,
    critical: Option<f64>,
) -> QuotaWarningWindow {
    QuotaWarningWindow {
        enabled,
        high,
        critical,
    }
}

/// #827: a preferences export carries each provider's quota-warning settings
/// and an import restores them, without any other `provider_configs` field.
#[test]
fn provider_quota_warnings_round_trip_with_literal_values() {
    let mut source = secret_settings();
    source.set_quota_warnings(
        ProviderId::Codex,
        ProviderQuotaWarnings {
            session: quota_window(Some(true), Some(55.0), Some(85.0)),
            weekly: QuotaWarningWindow::default(),
        },
    );
    source.set_quota_warnings(
        ProviderId::Claude,
        ProviderQuotaWarnings {
            session: QuotaWarningWindow::default(),
            weekly: quota_window(Some(false), Some(60.0), Some(90.0)),
        },
    );

    let text = PreferencesDocument::from_settings(&source)
        .expect("export")
        .to_json();
    let json: Value = serde_json::from_str(&text).expect("valid JSON");
    assert_eq!(
        json["preferences"]["provider_quota_warnings"],
        json!({
            "claude": { "weekly": { "enabled": false, "high": 60.0, "critical": 90.0 } },
            "codex": { "session": { "enabled": true, "high": 55.0, "critical": 85.0 } }
        })
    );
    // Only the quota warnings leave `provider_configs`; its secrets stay home.
    assert!(!text.contains("SECRET"));
    assert!(!text.contains("provider_configs"));

    // The receiving machine has its own Claude token and a Gemini override
    // that the document does not list.
    let mut target = Settings::default();
    target.provider_config_mut(ProviderId::Claude).api_token = Some("TARGET-TOKEN".to_string());
    target.set_quota_warnings(
        ProviderId::Gemini,
        ProviderQuotaWarnings {
            session: quota_window(None, Some(40.0), None),
            weekly: QuotaWarningWindow::default(),
        },
    );
    PreferencesDocument::from_json(&text)
        .expect("reimport")
        .apply_to(&mut target)
        .expect("apply");

    let codex = target.quota_warnings(ProviderId::Codex);
    assert_eq!(
        codex.session,
        quota_window(Some(true), Some(55.0), Some(85.0))
    );
    assert!(codex.weekly.is_global());
    let claude = target.quota_warnings(ProviderId::Claude);
    assert!(claude.session.is_global());
    assert_eq!(
        claude.weekly,
        quota_window(Some(false), Some(60.0), Some(90.0))
    );
    // The document is the full picture: an unlisted provider goes back to global.
    assert!(target.quota_warnings(ProviderId::Gemini).is_empty());
    // Other provider config on the receiving machine is untouched.
    assert_eq!(
        target
            .provider_config(ProviderId::Claude)
            .and_then(|config| config.api_token.as_deref()),
        Some("TARGET-TOKEN")
    );
    // Re-exporting gives the same document.
    assert_eq!(
        PreferencesDocument::from_settings(&target)
            .expect("re-export")
            .to_json(),
        text
    );
}

#[test]
fn provider_quota_warnings_null_resets_and_wins_over_the_legacy_key() {
    let codex_off = ProviderQuotaWarnings {
        session: quota_window(Some(false), None, None),
        weekly: QuotaWarningWindow::default(),
    };
    let mut settings = Settings::default();
    settings.set_quota_warnings(ProviderId::Codex, codex_off);
    parse(json!({ "provider_quota_warnings": null }))
        .expect("null")
        .apply_to(&mut settings)
        .expect("apply");
    assert!(settings.quota_warnings(ProviderId::Codex).is_empty());

    // A document without the key leaves the warnings alone.
    settings.set_quota_warnings(ProviderId::Codex, codex_off);
    parse(json!({ "theme": "dark" }))
        .expect("theme")
        .apply_to(&mut settings)
        .expect("apply");
    assert_eq!(settings.quota_warnings(ProviderId::Codex), codex_off);

    // With both keys, the current one wins over the legacy one.
    parse(json!({
        "provider_usage_thresholds": { "codex:session": { "high": 10 } },
        "provider_quota_warnings": { "codex": { "session": { "high": 70 } } }
    }))
    .expect("both")
    .apply_to(&mut settings)
    .expect("apply");
    assert_eq!(
        settings.quota_warnings(ProviderId::Codex).session,
        quota_window(None, Some(70.0), None)
    );

    // The document key is not a Settings field.
    let fields = serde_json::to_value(Settings::default()).expect("serialize");
    for (key, _) in PROVIDER_PREFERENCES {
        assert!(
            fields.get(*key).is_none(),
            "`{key}` shadows a Settings field"
        );
    }
}

#[test]
fn rejection_matrix_names_the_offending_key() {
    let cases = [
        (
            "unknown key",
            json!({ "not_a_preference": true }),
            "not_a_preference",
        ),
        (
            "provider_configs smuggled in",
            json!({ "provider_configs": { "claude": { "api_token": "x" } } }),
            "provider_configs",
        ),
        (
            "http proxy password",
            json!({ "http_proxy_password": "x" }),
            "http_proxy_password",
        ),
        (
            "start at login",
            json!({ "start_at_login": true }),
            "start_at_login",
        ),
        ("bad enum", json!({ "theme": "sepia" }), "theme"),
        (
            "bad language",
            json!({ "ui_language": "klingon" }),
            "ui_language",
        ),
        (
            "bad layout",
            json!({ "overview_layout": "huge" }),
            "overview_layout",
        ),
        (
            "bad menu bar icon style",
            json!({ "menu_bar_icon_style": "critters" }),
            "menu_bar_icon_style",
        ),
        (
            "bad workday tick style",
            json!({ "workday_tick_style": "highContrast" }),
            "workday_tick_style",
        ),
        (
            "overview selection over six",
            json!({ "merged_overview_selected_providers":
                ["codex", "claude", "gemini", "cursor", "copilot", "zai", "kimi"] }),
            "merged_overview_selected_providers",
        ),
        (
            "overview selection unknown provider",
            json!({ "merged_overview_selected_providers": ["codex", "nope"] }),
            "merged_overview_selected_providers",
        ),
        (
            "threshold over 100",
            json!({ "high_usage_threshold": 100.5 }),
            "high_usage_threshold",
        ),
        (
            "negative threshold",
            json!({ "critical_usage_threshold": -1 }),
            "critical_usage_threshold",
        ),
        (
            "threshold as string",
            json!({ "high_usage_threshold": "70" }),
            "high_usage_threshold",
        ),
        (
            "bool as string",
            json!({ "show_pace": "true" }),
            "show_pace",
        ),
        ("bool as number", json!({ "show_pace": 1 }), "show_pace"),
        (
            "scale too small",
            json!({ "window_scale_percent": 99 }),
            "window_scale_percent",
        ),
        (
            "scale too large",
            json!({ "tray_scale_percent": 201 }),
            "tray_scale_percent",
        ),
        (
            "scale fractional",
            json!({ "window_scale_percent": 100.5 }),
            "window_scale_percent",
        ),
        (
            "interval below a minute",
            json!({ "refresh_interval_secs": 5 }),
            "refresh_interval_secs",
        ),
        (
            "interval above a day",
            json!({ "refresh_interval_secs": 86_401 }),
            "refresh_interval_secs",
        ),
        (
            "unknown provider id",
            json!({ "enabled_providers": ["nope"] }),
            "enabled_providers",
        ),
        (
            "duplicate provider id",
            json!({ "provider_order": ["codex", "codex"] }),
            "provider_order",
        ),
        (
            "provider alias",
            json!({ "enabled_providers": ["openai"] }),
            "enabled_providers",
        ),
        (
            "metric value",
            json!({ "provider_metrics": { "codex": "bogus" } }),
            "provider_metrics",
        ),
        (
            "metric provider",
            json!({ "provider_metrics": { "nope": "weekly" } }),
            "provider_metrics",
        ),
        (
            "threshold window",
            json!({ "provider_usage_thresholds": { "codex:monthly": { "high": 50 } } }),
            "provider_usage_thresholds",
        ),
        (
            "threshold field",
            json!({ "provider_usage_thresholds": { "codex": { "low": 50 } } }),
            "provider_usage_thresholds",
        ),
        (
            "threshold range",
            json!({ "provider_usage_thresholds": { "codex": { "high": 150 } } }),
            "provider_usage_thresholds",
        ),
        (
            "stay awake",
            json!({ "stay_awake_enabled": true }),
            "stay_awake_enabled",
        ),
        (
            "pinned cost zone",
            json!({ "cost_usage_bucket_time_zone": "UTC" }),
            "cost_usage_bucket_time_zone",
        ),
        (
            "zero-day period",
            json!({ "cost_reporting_period": "rolling:0" }),
            "cost_reporting_period",
        ),
        (
            "period over a year",
            json!({ "cost_reporting_period": "rolling:400" }),
            "cost_reporting_period",
        ),
        (
            "unknown period",
            json!({ "cost_reporting_period": "fortnight" }),
            "cost_reporting_period",
        ),
        (
            "unknown dashboard period",
            json!({ "spend_dashboard_period": "fortnight" }),
            "spend_dashboard_period",
        ),
        (
            "track costs as text",
            json!({ "cost_usage_enabled": "yes" }),
            "cost_usage_enabled",
        ),
        (
            "unsorted hidden sources",
            json!({ "spend_dashboard_hidden_source_ids": ["codex", "claude"] }),
            "spend_dashboard_hidden_source_ids",
        ),
        (
            "duplicate hidden sources",
            json!({ "spend_dashboard_hidden_source_ids": ["codex", "codex"] }),
            "spend_dashboard_hidden_source_ids",
        ),
        (
            "hidden sources not a list",
            json!({ "spend_dashboard_hidden_source_ids": "codex" }),
            "spend_dashboard_hidden_source_ids",
        ),
        (
            "unsupported currency",
            json!({ "preferred_currency_code": "BTC" }),
            "preferred_currency_code",
        ),
        (
            "lowercase currency",
            json!({ "preferred_currency_code": "eur" }),
            "preferred_currency_code",
        ),
        (
            "reserved switcher shortcut",
            json!({ "switcher_shortcuts": { "next": "ctrl+r" } }),
            "switcher_shortcuts",
        ),
        (
            "duplicate switcher shortcut",
            json!({ "switcher_shortcuts": { "previous": "right" } }),
            "switcher_shortcuts",
        ),
        (
            "unknown switcher action",
            json!({ "switcher_shortcuts": { "jump": "ctrl+j" } }),
            "switcher_shortcuts",
        ),
        (
            "switcher shortcut as number",
            json!({ "switcher_shortcuts": { "next": 3 } }),
            "switcher_shortcuts",
        ),
        (
            "quota warnings for an unknown provider",
            json!({ "provider_quota_warnings": { "future-provider": { "session": { "high": 50 } } } }),
            "provider_quota_warnings",
        ),
        (
            "quota warnings for an unknown window",
            json!({ "provider_quota_warnings": { "codex": { "monthly": { "high": 50 } } } }),
            "provider_quota_warnings",
        ),
        (
            "quota warning above 100 percent",
            json!({ "provider_quota_warnings": { "codex": { "session": { "critical": 101 } } } }),
            "provider_quota_warnings",
        ),
        (
            "quota warning secret smuggled in",
            json!({ "provider_quota_warnings": { "claude": { "session": { "api_token": "x" } } } }),
            "provider_quota_warnings",
        ),
        (
            "quota warning enabled as string",
            json!({ "provider_quota_warnings": { "codex": { "weekly": { "enabled": "no" } } } }),
            "provider_quota_warnings",
        ),
    ];
    for (name, preferences, key) in cases {
        assert_eq!(
            parse(preferences).expect_err(name),
            PreferencesError::InvalidPreference(key.to_string()),
            "{name}"
        );
    }
}

#[test]
fn rejects_bad_envelopes() {
    for text in [
        r#"{"version":2,"preferences":{}}"#,
        r#"{"version":"1","preferences":{}}"#,
        r#"{"preferences":{}}"#,
    ] {
        assert_eq!(
            PreferencesDocument::from_json(text),
            Err(PreferencesError::UnsupportedVersion),
            "{text}"
        );
    }
    for text in [
        "not json",
        "[]",
        r#"{"version":1}"#,
        r#"{"version":1,"preferences":[]}"#,
        r#"{"version":1,"preferences":{},"extra":true}"#,
    ] {
        assert_eq!(
            PreferencesDocument::from_json(text),
            Err(PreferencesError::Malformed),
            "{text}"
        );
    }
    let oversized = format!(
        r#"{{"version":1,"preferences":{{}},"pad":"{}"}}"#,
        "x".repeat(max_document_bytes())
    );
    assert_eq!(
        PreferencesDocument::from_json(&oversized),
        Err(PreferencesError::TooLarge)
    );
}

#[test]
fn errors_never_echo_values() {
    let error = parse(json!({ "theme": "TOP-SECRET-VALUE" })).expect_err("bad theme");
    assert!(!error.to_string().contains("TOP-SECRET-VALUE"));
    let error = PreferencesDocument::from_json("TOP-SECRET-VALUE").expect_err("not json");
    assert!(!error.to_string().contains("TOP-SECRET-VALUE"));
}

#[test]
fn missing_keys_leave_settings_unchanged() {
    let empty = parse(json!({})).expect("empty document");
    let mut settings = customized_settings();
    // The first apply normalizes state the way a settings load does.
    empty.apply_to(&mut settings).expect("apply");
    let before = snapshot(&settings);
    assert_eq!(empty.apply_to(&mut settings).expect("apply"), 0);
    assert_eq!(snapshot(&settings), before);

    parse(json!({ "theme": "light" }))
        .expect("theme")
        .apply_to(&mut settings)
        .expect("apply");
    assert_eq!(settings.theme, ThemePreference::Light);
    assert_eq!(settings.window_scale_percent, 150);
}

#[test]
fn reset_notification_toggle_travels_in_both_directions() {
    let mut source = Settings {
        limit_reset_notifications_enabled: true,
        ..Settings::default()
    };
    let exported = PreferencesDocument::from_json(
        &PreferencesDocument::from_settings(&source)
            .expect("export")
            .to_json(),
    )
    .expect("reimport");
    assert!(
        exported
            .to_json()
            .contains("\"limit_reset_notifications_enabled\": true")
    );

    let mut target = Settings::default();
    exported.apply_to(&mut target).expect("apply");
    assert!(target.limit_reset_notifications_enabled);

    // A document without the key keeps the receiving machine's choice.
    parse(json!({}))
        .expect("empty")
        .apply_to(&mut target)
        .expect("apply");
    assert!(target.limit_reset_notifications_enabled);

    source.limit_reset_notifications_enabled = false;
    PreferencesDocument::from_settings(&source)
        .expect("export")
        .apply_to(&mut target)
        .expect("apply");
    assert!(!target.limit_reset_notifications_enabled);
}

#[test]
fn null_restores_the_default() {
    let mut settings = customized_settings();
    let applied = parse(json!({
        "window_scale_percent": null,
        "theme": null,
        "enabled_providers": null,
        "provider_usage_thresholds": null,
        "provider_metrics": null,
        "provider_order": null,
        "show_pace": null,
        "cost_reporting_period": null,
        "preferred_currency_code": null,
        "switcher_shortcuts": null,
    }))
    .expect("nulls are valid")
    .apply_to(&mut settings)
    .expect("apply");
    assert_eq!(applied, 10);

    let defaults = Settings::default();
    assert_eq!(settings.window_scale_percent, defaults.window_scale_percent);
    assert_eq!(settings.theme, defaults.theme);
    assert_eq!(settings.enabled_providers, defaults.enabled_providers);
    assert!(settings.provider_metrics.is_empty());
    assert!(settings.provider_order.is_empty());
    assert!(settings.show_pace);
    assert_eq!(
        settings.cost_reporting_period,
        defaults.cost_reporting_period
    );
    assert_eq!(settings.preferred_currency_code, "AUTO");
    // Omitted from settings.json when empty, so null clears the overrides.
    assert!(settings.switcher_shortcuts.is_empty());
    // Untouched preferences keep their customized values.
    assert_eq!(settings.tray_scale_percent, 120);
}

#[test]
fn allowed_and_excluded_keys_classify_every_settings_field() {
    let Value::Object(serialized) = snapshot(&secret_settings()) else {
        panic!("settings must serialize to an object");
    };
    let allowed: HashSet<&str> = ALLOWED_PREFERENCES.iter().map(|(key, _)| *key).collect();
    let excluded: HashSet<&str> = EXCLUDED_KEYS.iter().copied().collect();

    assert_eq!(
        allowed.len(),
        ALLOWED_PREFERENCES.len(),
        "duplicate allowlist key"
    );
    assert_eq!(
        excluded.len(),
        EXCLUDED_KEYS.len(),
        "duplicate excluded key"
    );
    assert!(
        allowed.is_disjoint(&excluded),
        "a key is both allowed and excluded"
    );

    for key in serialized.keys() {
        assert!(
            allowed.contains(key.as_str()) || excluded.contains(key.as_str()),
            "Settings field `{key}` is not classified: add it to ALLOWED_PREFERENCES \
             (portable) or EXCLUDED_KEYS (never exported)"
        );
    }
    for key in allowed.iter().chain(excluded.iter()) {
        assert!(
            serialized.contains_key(*key),
            "`{key}` is classified but is not a Settings field"
        );
    }
}

#[test]
fn export_never_contains_secrets_or_machine_state() {
    let text = PreferencesDocument::from_settings(&secret_settings())
        .expect("export")
        .to_json();
    for secret in [
        "SECRET",
        "proxy.invalid",
        "secret-host",
        "secret\\\\",
        "sessionKey",
        "provider_configs",
        "http_proxy",
        "start_at_login",
        "global_shortcut",
        "stay_awake",
        "cost_usage_bucket_time_zone",
        "Asia/Tokyo",
        "stacked_tray",
        "credential_expiry",
        "menu_bar_color_pace",
        "menu_bar_pace_reserve_positive",
        "highlight_menu_cards_on_hover",
    ] {
        assert!(!text.contains(secret), "export leaked `{secret}`");
    }
}

#[test]
fn applying_keeps_everything_else_on_the_receiving_machine() {
    let empty = parse(json!({})).expect("empty");
    let mut target = secret_settings();
    empty.apply_to(&mut target).expect("apply");
    let before = snapshot(&target);
    // The whole-struct JSON round trip used by apply is lossless once normalized.
    empty.apply_to(&mut target).expect("apply");
    assert_eq!(snapshot(&target), before);

    parse(json!({ "theme": "light" }))
        .expect("theme")
        .apply_to(&mut target)
        .expect("apply");
    let config = target
        .provider_configs
        .get(&ProviderId::Claude)
        .expect("config");
    assert_eq!(config.api_token.as_deref(), Some("SECRET-API-TOKEN"));
    assert_eq!(target.http_proxy_password, "SECRET-PROXY-PASSWORD");
}

#[test]
fn file_round_trip_and_size_bound() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("preferences.json");
    let exported = PreferencesDocument::from_settings(&customized_settings()).expect("export");
    exported.write_file(&path).expect("write");
    assert_eq!(
        PreferencesDocument::read_file(&path).expect("read"),
        exported
    );

    let big = dir.path().join("big.json");
    std::fs::write(&big, vec![b' '; max_document_bytes() + 10]).expect("write");
    assert_eq!(
        PreferencesDocument::read_file(&big),
        Err(PreferencesError::TooLarge)
    );

    assert!(matches!(
        PreferencesDocument::read_file(&dir.path().join("missing.json")),
        Err(PreferencesError::Io(_))
    ));
}

#[test]
fn tray_scale_document_bounds_match_the_settings_clamp() {
    use crate::settings::{
        TRAY_SCALE_PERCENT_MAX, TRAY_SCALE_PERCENT_MIN, clamp_tray_scale_percent,
    };
    assert_eq!(clamp_tray_scale_percent(0), TRAY_SCALE_PERCENT_MIN);
    assert_eq!(clamp_tray_scale_percent(u16::MAX), TRAY_SCALE_PERCENT_MAX);
    for accepted in [TRAY_SCALE_PERCENT_MIN, TRAY_SCALE_PERCENT_MAX] {
        assert!(
            parse(json!({ "tray_scale_percent": accepted })).is_ok(),
            "{accepted} should be accepted"
        );
    }
    for rejected in [TRAY_SCALE_PERCENT_MIN - 1, TRAY_SCALE_PERCENT_MAX + 1] {
        assert!(
            parse(json!({ "tray_scale_percent": rejected })).is_err(),
            "{rejected} should be rejected"
        );
    }
}

#[test]
fn a_document_without_the_follow_flag_keeps_its_chosen_language() {
    // Exported before "System" existed: a language but no follow flag.
    let document = parse(json!({ "ui_language": "japanese" })).expect("parse");
    let mut target = Settings::default();
    assert!(target.ui_language_follows_system);
    document.apply_to(&mut target).expect("apply");
    assert!(!target.ui_language_follows_system);
    assert_eq!(target.ui_language, Language::Japanese);
    assert!(!target.resolve_system_language_with(Language::Korean));
    assert_eq!(target.ui_language, Language::Japanese);

    // A document that carries the flag keeps following the system.
    let document = parse(json!({
        "ui_language": "japanese",
        "ui_language_follows_system": true,
    }))
    .expect("parse");
    let mut target = Settings::default();
    document.apply_to(&mut target).expect("apply");
    assert!(target.ui_language_follows_system);
    assert!(target.resolve_system_language_with(Language::Korean));
    assert_eq!(target.ui_language, Language::Korean);
}
