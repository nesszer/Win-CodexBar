use super::*;

#[test]
fn preferred_currency_patch_accepts_supported_codes_and_rejects_unknown_codes() {
    let mut settings = Settings::default();
    SettingsUpdate {
        preferred_currency_code: Some("try".to_string()),
        ..SettingsUpdate::default()
    }
    .apply_to(&mut settings)
    .expect("TRY is supported");
    assert_eq!(settings.preferred_currency_code, "TRY");

    let result = SettingsUpdate {
        preferred_currency_code: Some("BTC".to_string()),
        ..SettingsUpdate::default()
    }
    .apply_to(&mut settings);
    assert!(matches!(result, Err(error) if error.contains("Unsupported preferred currency")));
}

#[test]
fn provider_metrics_patch_keeps_only_declared_named_extras() {
    let mut settings = Settings::default();
    SettingsUpdate {
        provider_metrics: Some(
            [
                ("cursor", "extra:cursor-grok-bot"),
                ("codex", "extra:cursor-grok-bot"),
                ("claude", "extra:retired"),
                ("notaprovider", "extra:cursor-grok-bot"),
                ("gemini", "weekly"),
            ]
            .into_iter()
            .map(|(provider, metric)| (provider.to_string(), metric.to_string()))
            .collect(),
        ),
        ..SettingsUpdate::default()
    }
    .apply_to(&mut settings)
    .expect("metric patch applies");

    assert_eq!(
        settings.provider_metrics.get("cursor"),
        Some(&MetricPreference::NamedExtra("cursor-grok-bot".to_string()))
    );
    assert_eq!(
        settings.provider_metrics.get("gemini"),
        Some(&MetricPreference::Weekly)
    );
    assert!(!settings.provider_metrics.contains_key("codex"));
    assert!(!settings.provider_metrics.contains_key("claude"));
    assert!(!settings.provider_metrics.contains_key("notaprovider"));
}

#[test]
fn only_data_affecting_settings_refresh_providers() {
    assert!(
        SettingsUpdate {
            enabled_providers: Some(vec!["codex".to_string()]),
            ..Default::default()
        }
        .refreshes_provider_data()
    );
    assert!(
        SettingsUpdate {
            claude_allow_reading_claude_code_credentials: Some(true),
            ..Default::default()
        }
        .refreshes_provider_data()
    );
    assert!(
        !SettingsUpdate {
            provider_metrics: Some(Default::default()),
            tray_icon_mode: Some("single".to_string()),
            ..Default::default()
        }
        .refreshes_provider_data()
    );
    assert!(
        !SettingsUpdate {
            provider_hidden_usage_item_ids: Some(
                [("codex".to_string(), Vec::new())].into_iter().collect(),
            ),
            ..Default::default()
        }
        .refreshes_provider_data()
    );
    assert!(
        !SettingsUpdate {
            claude_daily_routines_usage_visible: Some(false),
            ..Default::default()
        }
        .refreshes_provider_data()
    );
}

#[test]
fn codex_options_and_quota_warnings_round_trip_through_the_bridge() {
    let patch: SettingsUpdate = serde_json::from_value(serde_json::json!({
        "codexOpenaiWebExtras": false,
        "codexHistoricalTracking": true,
        "codexExternalOauthSourcesAllowed": true,
        "codexLocalSessionCostLedgerEnabled": true,
        "codexOpencodeLocalHistoryEnabled": true,
        "codexOpenaiWebBatterySaverEnabled": true,
        "providerQuotaWarnings": {
            "codex": {
                "session": { "high": 60.0, "critical": 85.0 },
                "weekly": { "enabled": false }
            },
            "not-a-provider": { "session": { "high": 10.0 } }
        }
    }))
    .expect("bridge patch deserializes");
    assert!(patch.notifies_float_bar());
    assert!(patch.refreshes_tray_presentation());
    assert!(patch.refreshes_provider_data());

    let mut settings = Settings::default();
    assert!(
        settings.codex_openai_web_extras(),
        "Windows default stays on"
    );
    patch.apply_to(&mut settings).expect("apply");

    let snapshot = serde_json::to_value(crate::commands::bridge::SettingsSnapshot::from(
        settings.clone(),
    ))
    .expect("snapshot serializes");
    assert_eq!(snapshot["codexOpenaiWebExtras"], false);
    assert_eq!(snapshot["codexHistoricalTracking"], true);
    assert_eq!(snapshot["codexExternalOauthSourcesAllowed"], true);
    assert_eq!(snapshot["codexLocalSessionCostLedgerEnabled"], true);
    assert_eq!(snapshot["codexOpencodeLocalHistoryEnabled"], true);
    assert_eq!(snapshot["codexOpenaiWebBatterySaverEnabled"], true);
    assert_eq!(
        snapshot["providerQuotaWarnings"],
        serde_json::json!({
            "codex": {
                "session": { "high": 60.0, "critical": 85.0 },
                "weekly": { "enabled": false }
            }
        })
    );
    assert!(snapshot.get("providerUsageThresholds").is_none());
    assert!(!settings.quota_warning_enabled(codexbar::core::ProviderId::Codex, "weekly"));

    // An empty object returns the provider to Global.
    let reset: SettingsUpdate = serde_json::from_value(serde_json::json!({
        "providerQuotaWarnings": { "codex": {} }
    }))
    .expect("reset patch");
    reset.apply_to(&mut settings).expect("apply reset");
    assert!(
        settings
            .quota_warnings(codexbar::core::ProviderId::Codex)
            .is_empty()
    );
}

#[test]
fn apply_advanced_settings_sets_claude_code_credentials_consent() {
    let mut settings = Settings::default();
    assert!(!settings.claude_allow_reading_claude_code_credentials);

    SettingsUpdate {
        claude_allow_reading_claude_code_credentials: Some(true),
        ..Default::default()
    }
    .apply_advanced_settings(&mut settings);
    assert!(settings.claude_allow_reading_claude_code_credentials);

    SettingsUpdate {
        claude_allow_reading_claude_code_credentials: Some(false),
        ..Default::default()
    }
    .apply_advanced_settings(&mut settings);
    assert!(!settings.claude_allow_reading_claude_code_credentials);
}

#[test]
fn apply_provider_usage_item_visibility_persists() {
    let mut settings = Settings::default();
    SettingsUpdate {
        provider_hidden_usage_item_ids: Some(
            [("codex".to_string(), vec!["metric:secondary".to_string()])]
                .into_iter()
                .collect(),
        ),
        ..Default::default()
    }
    .apply_provider_settings(&mut settings);

    assert_eq!(
        settings.hidden_usage_item_ids(ProviderId::Codex),
        vec!["metric:secondary".to_string()]
    );
    assert!(settings.codex_spark_usage_visible());
}

#[test]
fn copilot_seat_credit_update_distinguishes_missing_clear_and_value() {
    let missing: SettingsUpdate = serde_json::from_str("{}").unwrap();
    assert_eq!(missing.copilot_seat_credit_entitlement, None);

    let clear: SettingsUpdate =
        serde_json::from_str(r#"{"copilotSeatCreditEntitlement":null}"#).unwrap();
    assert_eq!(clear.copilot_seat_credit_entitlement, Some(None));

    let value: SettingsUpdate =
        serde_json::from_str(r#"{"copilotSeatCreditEntitlement":300}"#).unwrap();
    assert_eq!(value.copilot_seat_credit_entitlement, Some(Some(300.0)));
}

#[test]
fn invalid_copilot_seat_credit_update_is_rejected_by_the_settings_setter() {
    let mut settings = Settings::default();

    let update: SettingsUpdate =
        serde_json::from_str(r#"{"copilotSeatCreditEntitlement":-5}"#).unwrap();
    let error = update
        .apply_to(&mut settings)
        .expect_err("invalid allowance must be rejected");
    assert_eq!(
        error,
        "Copilot seat AI-credit allowance must be a finite number greater than zero"
    );
    assert_eq!(settings.seat_credit_entitlement(ProviderId::Copilot), None);
}

#[test]
fn cost_reporting_period_update_is_validated_and_applied() {
    use codexbar::cost_reporting_period::CostReportingPeriod;

    let mut settings = Settings::default();
    let bad: SettingsUpdate =
        serde_json::from_str(r#"{"costReportingPeriod":"rolling:0"}"#).unwrap();
    let error = bad
        .apply_to(&mut settings)
        .expect_err("zero-day window must be rejected");
    assert_eq!(error, "Invalid cost reporting period: rolling:0");

    let good: SettingsUpdate =
        serde_json::from_str(r#"{"costReportingPeriod":"month-to-date"}"#).unwrap();
    good.apply_to(&mut settings).expect("valid period applies");
    assert_eq!(
        settings.cost_reporting_period,
        CostReportingPeriod::MonthToDate
    );
}

#[test]
fn spend_dashboard_updates_are_validated_and_applied() {
    use codexbar::cost_reporting_period::CostReportingPeriod;

    let mut settings = Settings::default();
    let bad: SettingsUpdate =
        serde_json::from_str(r#"{"spendDashboardPeriod":"fortnight"}"#).unwrap();
    let error = bad
        .apply_to(&mut settings)
        .expect_err("unknown dashboard period must be rejected");
    assert_eq!(error, "Invalid spend dashboard period: fortnight");

    let good: SettingsUpdate = serde_json::from_str(
        r#"{"spendDashboardPeriod":"rolling:90","spendDashboardHiddenSourceIds":["pi"," Codex","pi"]}"#,
    )
    .unwrap();
    good.apply_to(&mut settings)
        .expect("valid dashboard patch applies");
    assert_eq!(
        settings.spend_dashboard_period,
        CostReportingPeriod::Rolling(90)
    );
    // The menu / float-bar period is a separate key.
    assert_eq!(
        settings.cost_reporting_period,
        CostReportingPeriod::Rolling(30)
    );
    assert_eq!(settings.spend_dashboard_hidden_source_ids, ["codex", "pi"]);
}

#[test]
fn turning_cost_tracking_on_pins_the_bucket_zone_and_off_keeps_it() {
    let mut settings = Settings {
        cost_usage_enabled: false,
        cost_usage_bucket_time_zone: String::new(),
        ..Settings::default()
    };
    let on: SettingsUpdate = serde_json::from_str(r#"{"costUsageEnabled":true}"#).unwrap();
    assert!(on.notifies_float_bar());
    on.apply_to(&mut settings).expect("enable applies");
    assert!(settings.cost_usage_enabled);
    let mut expected = Settings {
        cost_usage_bucket_time_zone: String::new(),
        ..Settings::default()
    };
    expected.pin_cost_usage_bucket_time_zone();
    assert_eq!(
        settings.cost_usage_bucket_time_zone,
        expected.cost_usage_bucket_time_zone
    );

    let off: SettingsUpdate = serde_json::from_str(r#"{"costUsageEnabled":false}"#).unwrap();
    off.apply_to(&mut settings).expect("disable applies");
    assert!(!settings.cost_usage_enabled);
    assert_eq!(
        settings.cost_usage_bucket_time_zone,
        expected.cost_usage_bucket_time_zone
    );
}

#[test]
fn display_settings_that_affect_tray_trigger_presentation_refresh() {
    assert!(
        SettingsUpdate {
            switcher_shows_icons: Some(false),
            ..Default::default()
        }
        .refreshes_tray_presentation()
    );
    assert!(
        SettingsUpdate {
            reset_time_relative: Some(false),
            ..Default::default()
        }
        .refreshes_tray_presentation()
    );
    assert!(
        SettingsUpdate {
            menu_bar_color_pace: Some(true),
            ..Default::default()
        }
        .refreshes_tray_presentation()
    );
    assert!(
        SettingsUpdate {
            stacked_tray_top_provider: Some("claude".to_string()),
            ..Default::default()
        }
        .refreshes_tray_presentation()
    );
}

#[test]
fn apply_display_settings_updates_tray_pace_color() {
    let mut settings = Settings::default();
    assert!(!settings.menu_bar_color_pace);

    SettingsUpdate {
        menu_bar_color_pace: Some(true),
        ..Default::default()
    }
    .apply_display_settings(&mut settings);
    assert!(settings.menu_bar_color_pace);

    SettingsUpdate {
        menu_bar_color_pace: Some(false),
        ..Default::default()
    }
    .apply_display_settings(&mut settings);
    assert!(!settings.menu_bar_color_pace);
}

#[test]
fn apply_display_settings_updates_color_by_provider() {
    let mut settings = Settings::default();
    assert!(!settings.menu_bar_color_by_provider);

    SettingsUpdate {
        menu_bar_color_by_provider: Some(true),
        ..Default::default()
    }
    .apply_display_settings(&mut settings);
    assert!(settings.menu_bar_color_by_provider);

    SettingsUpdate {
        menu_bar_color_by_provider: Some(false),
        ..Default::default()
    }
    .apply_display_settings(&mut settings);
    assert!(!settings.menu_bar_color_by_provider);
}

#[test]
fn apply_display_settings_updates_pace_sign() {
    let mut settings = Settings::default();
    assert!(!settings.menu_bar_pace_reserve_positive);

    let update: SettingsUpdate =
        serde_json::from_str(r#"{"menuBarPaceReservePositive":true}"#).unwrap();
    update.apply_display_settings(&mut settings);
    assert!(settings.menu_bar_pace_reserve_positive);

    SettingsUpdate {
        menu_bar_pace_reserve_positive: Some(false),
        ..Default::default()
    }
    .apply_display_settings(&mut settings);
    assert!(!settings.menu_bar_pace_reserve_positive);
}

#[test]
fn menu_bar_icon_style_update_repaints_tray_and_mirrors_legacy_flag() {
    let mut settings = Settings::default();
    let update: SettingsUpdate = serde_json::from_str(r#"{"menuBarIconStyle":"percent"}"#).unwrap();
    assert!(update.refreshes_tray_presentation());
    update.apply_display_settings(&mut settings);
    assert_eq!(
        settings.menu_bar_icon_style,
        codexbar::settings::MenuBarIconStyle::Percent
    );
    assert!(settings.menu_bar_shows_percent);

    let update: SettingsUpdate = serde_json::from_str(r#"{"menuBarIconStyle":"bars"}"#).unwrap();
    update.apply_display_settings(&mut settings);
    assert_eq!(
        settings.menu_bar_icon_style,
        codexbar::settings::MenuBarIconStyle::Bars
    );
    assert!(!settings.menu_bar_shows_percent);

    // Progress bars stacked is not the percent style, so the legacy flag stays off.
    let update: SettingsUpdate =
        serde_json::from_str(r#"{"menuBarIconStyle":"stackedBars"}"#).unwrap();
    assert!(update.refreshes_tray_presentation());
    update.apply_display_settings(&mut settings);
    assert_eq!(
        settings.menu_bar_icon_style,
        codexbar::settings::MenuBarIconStyle::StackedBars
    );
    assert!(!settings.menu_bar_shows_percent);
}

#[test]
fn menu_pane_keys_apply_from_camel_case_patch() {
    let mut settings = Settings::default();
    let update: SettingsUpdate = serde_json::from_str(
        r#"{"quotaWarningMarkersVisible":false,"workdayTickStyle":"high_contrast",
            "highlightMenuCardsOnHover":false,
            "mergedOverviewSelectedProviders":["claude","Codex","claude","gemini","cursor","copilot","zai","kimi"],
            "weeklyProgressWorkDays":7}"#,
    )
    .unwrap();
    update
        .apply_display_settings(&mut settings)
        .apply_advanced_settings(&mut settings);
    assert!(!settings.quota_warning_markers_visible);
    assert!(!settings.highlight_menu_cards_on_hover);
    assert_eq!(
        settings.workday_tick_style,
        codexbar::settings::WorkdayTickStyle::HighContrast
    );
    assert_eq!(
        settings.merged_overview_selected_providers,
        ["claude", "codex", "gemini", "cursor", "copilot", "zai"]
            .map(String::from)
            .to_vec()
    );
    assert_eq!(settings.weekly_progress_work_days, Some(7));

    let update: SettingsUpdate = serde_json::from_str(r#"{"weeklyProgressWorkDays":8}"#).unwrap();
    update
        .apply_display_settings(&mut settings)
        .apply_advanced_settings(&mut settings);
    assert_eq!(settings.weekly_progress_work_days, None);

    settings.weekly_progress_work_days = Some(5);
    let update: SettingsUpdate =
        serde_json::from_str(r#"{"weeklyProgressWorkDays":null}"#).unwrap();
    assert!(update.refreshes_provider_data());
    update
        .apply_display_settings(&mut settings)
        .apply_advanced_settings(&mut settings);
    assert_eq!(
        settings.weekly_progress_work_days, None,
        "null selects Automatic"
    );
}

#[test]
fn settings_snapshot_emits_snake_case_style_values() {
    let settings = Settings {
        menu_bar_icon_style: codexbar::settings::MenuBarIconStyle::Percent,
        workday_tick_style: codexbar::settings::WorkdayTickStyle::HighContrast,
        ..Settings::default()
    };
    let json = serde_json::to_value(crate::commands::SettingsSnapshot::from(settings)).unwrap();
    assert_eq!(json["menuBarIconStyle"], "percent");
    assert_eq!(json["workdayTickStyle"], "high_contrast");
    assert_eq!(json["quotaWarningMarkersVisible"], true);
    assert_eq!(json["highlightMenuCardsOnHover"], true);
    assert_eq!(
        json["mergedOverviewSelectedProviders"],
        serde_json::json!([])
    );
}

#[test]
fn providers_sidebar_sort_applies_without_touching_order_or_tray() {
    let mut settings = Settings::default();
    let order = settings.provider_order.clone();
    let update: SettingsUpdate =
        serde_json::from_str(r#"{"providersSortedAlphabetically":true}"#).unwrap();
    assert!(!update.refreshes_tray_presentation());
    update.apply_display_settings(&mut settings);
    assert!(settings.providers_sorted_alphabetically);
    assert_eq!(settings.provider_order, order);
}

#[test]
fn stacked_tray_update_accepts_mode_and_clears_automatic_provider() {
    let mut settings = Settings {
        stacked_tray_top_provider: Some("codex".to_string()),
        ..Settings::default()
    };

    SettingsUpdate {
        tray_icon_mode: Some("stacked".to_string()),
        stacked_tray_top_provider: Some(String::new()),
        stacked_tray_bottom_provider: Some("claude".to_string()),
        ..Default::default()
    }
    .apply_provider_settings(&mut settings);

    assert_eq!(settings.tray_icon_mode, TrayIconMode::Stacked);
    assert_eq!(settings.stacked_tray_top_provider, None);
    assert_eq!(
        settings.stacked_tray_bottom_provider.as_deref(),
        Some("claude")
    );
}

#[test]
fn choosing_system_language_follows_the_system() {
    let mut settings = Settings {
        ui_language: Language::Spanish,
        ui_language_follows_system: false,
        ..Settings::default()
    };
    let patch = SettingsUpdate {
        ui_language_follows_system: Some(true),
        ..Default::default()
    };
    assert!(patch.rebuilds_tray_menu());
    assert!(patch.refreshes_tray_presentation());
    patch.apply_language_with(&mut settings, Language::Korean);
    assert!(settings.ui_language_follows_system);
    assert_eq!(settings.ui_language, Language::Korean);
}

#[test]
fn choosing_a_specific_language_stops_following_the_system() {
    let mut settings = Settings {
        ui_language: Language::Korean,
        ui_language_follows_system: true,
        ..Settings::default()
    };
    SettingsUpdate {
        ui_language: Some("japanese".to_string()),
        ..Default::default()
    }
    .apply_language_with(&mut settings, Language::Korean);
    assert!(!settings.ui_language_follows_system);
    assert_eq!(settings.ui_language, Language::Japanese);

    // An unrelated patch leaves the choice alone.
    SettingsUpdate::default().apply_language_with(&mut settings, Language::Korean);
    assert!(!settings.ui_language_follows_system);
    assert_eq!(settings.ui_language, Language::Japanese);
}

#[test]
fn ui_language_change_refreshes_tray_presentation() {
    assert!(
        SettingsUpdate {
            ui_language: Some("japanese".to_string()),
            ..Default::default()
        }
        .refreshes_tray_presentation()
    );
}

#[test]
fn apply_display_settings_clamps_window_scale_percent() {
    let mut settings = Settings::default();

    SettingsUpdate {
        window_scale_percent: Some(300),
        ..Default::default()
    }
    .apply_display_settings(&mut settings);
    assert_eq!(settings.window_scale_percent, 250);

    SettingsUpdate {
        window_scale_percent: Some(50),
        ..Default::default()
    }
    .apply_display_settings(&mut settings);
    assert_eq!(settings.window_scale_percent, 100);
}

#[test]
fn apply_display_settings_clamps_tray_scale_percent() {
    let mut settings = Settings::default();

    SettingsUpdate {
        tray_scale_percent: Some(300),
        ..Default::default()
    }
    .apply_display_settings(&mut settings);
    assert_eq!(settings.tray_scale_percent, 200);

    SettingsUpdate {
        tray_scale_percent: Some(50),
        ..Default::default()
    }
    .apply_display_settings(&mut settings);
    assert_eq!(settings.tray_scale_percent, 100);
}

#[test]
fn apply_display_settings_updates_tray_panel_always_on_top() {
    let mut settings = Settings::default();
    assert!(!settings.tray_panel_always_on_top);

    SettingsUpdate {
        tray_panel_always_on_top: Some(true),
        ..Default::default()
    }
    .apply_display_settings(&mut settings);
    assert!(settings.tray_panel_always_on_top);

    SettingsUpdate {
        tray_panel_always_on_top: Some(false),
        ..Default::default()
    }
    .apply_display_settings(&mut settings);
    assert!(!settings.tray_panel_always_on_top);
}

#[test]
fn apply_notification_settings_updates_sound() {
    let mut settings = Settings::default();

    SettingsUpdate {
        notification_sound_theme: Some(codexbar::settings::NotificationSoundTheme::CodexBar),
        ..Default::default()
    }
    .apply_notification_settings(&mut settings)
    .expect("apply sound theme");

    assert_eq!(
        settings.notification_sound_theme,
        codexbar::settings::NotificationSoundTheme::CodexBar
    );
}

#[test]
fn apply_notification_settings_rejects_invalid_custom_sound() {
    let mut settings = Settings::default();
    let result = SettingsUpdate {
        notification_sound_paths: Some(codexbar::settings::NotificationSoundPaths {
            high_usage: Some("relative.wav".to_string()),
            ..Default::default()
        }),
        ..Default::default()
    }
    .apply_notification_settings(&mut settings);

    assert!(result.is_err());
    assert_eq!(
        settings.notification_sound_paths,
        codexbar::settings::NotificationSoundPaths::default()
    );
}

fn switcher_patch(json: &str) -> SettingsUpdate {
    serde_json::from_str(&format!(r#"{{"switcherShortcuts":{json}}}"#)).unwrap()
}

#[test]
fn switcher_shortcuts_patch_stores_normalized_non_default_overrides() {
    let mut settings = Settings::default();
    switcher_patch(r#"{"select2":"Alt+Cmd+2","previous":"left","next":"none"}"#)
        .apply_to(&mut settings)
        .expect("valid overrides are stored");

    assert_eq!(
        settings.switcher_shortcuts,
        std::collections::BTreeMap::from([
            ("select2".to_string(), "ctrl+alt+2".to_string()),
            ("next".to_string(), "none".to_string()),
        ])
    );
}

#[test]
fn switcher_shortcuts_empty_patch_restores_defaults() {
    let mut settings = Settings::default();
    switcher_patch(r#"{"next":"shift+right"}"#)
        .apply_to(&mut settings)
        .unwrap();
    switcher_patch("{}").apply_to(&mut settings).unwrap();

    assert!(settings.switcher_shortcuts.is_empty());
}

#[test]
fn switcher_shortcuts_patch_rejects_invalid_maps_and_keeps_stored_value() {
    let mut settings = Settings::default();
    switcher_patch(r#"{"next":"shift+right"}"#)
        .apply_to(&mut settings)
        .unwrap();

    for (json, message) in [
        (
            r#"{"bogus":"ctrl+1"}"#,
            "Unknown switcher shortcut action: bogus",
        ),
        (
            r#"{"next":"left"}"#,
            "Each switcher shortcut can be assigned to only one action",
        ),
        (
            r#"{"next":"ctrl+r"}"#,
            "ctrl+r is reserved and cannot be used as a switcher shortcut",
        ),
        (r#"{"next":"f1"}"#, "f1 is not a valid switcher shortcut"),
    ] {
        let error = switcher_patch(json)
            .apply_to(&mut settings)
            .expect_err(json);
        assert_eq!(error, message);
    }
    assert_eq!(
        settings.switcher_shortcuts.get("next").map(String::as_str),
        Some("shift+right")
    );
}

#[test]
fn switcher_shortcuts_snapshot_exposes_the_fully_resolved_map() {
    let settings = Settings {
        switcher_shortcuts: std::collections::BTreeMap::from([(
            "next".to_string(),
            "none".to_string(),
        )]),
        ..Settings::default()
    };
    let value =
        serde_json::to_value(super::super::bridge::SettingsSnapshot::from(settings)).unwrap();
    let map = &value["switcherShortcuts"];

    assert_eq!(map["next"], "none");
    assert_eq!(map["previous"], "left");
    assert_eq!(map["select9"], "ctrl+9");
    assert_eq!(map.as_object().unwrap().len(), 11);
}

#[test]
fn limit_reset_toggle_travels_through_patch_and_snapshot() {
    let snapshot = |settings: &Settings| {
        serde_json::to_value(super::super::bridge::SettingsSnapshot::from(
            settings.clone(),
        ))
        .unwrap()["limitResetNotificationsEnabled"]
            .clone()
    };
    let mut settings = Settings::default();
    assert_eq!(snapshot(&settings), serde_json::Value::Bool(false));

    serde_json::from_str::<SettingsUpdate>(r#"{"limitResetNotificationsEnabled":true}"#)
        .unwrap()
        .apply_to(&mut settings)
        .unwrap();
    assert!(settings.limit_reset_notifications_enabled);
    assert_eq!(snapshot(&settings), serde_json::Value::Bool(true));
}
