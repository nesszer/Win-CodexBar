use super::*;
use codexbar::core::{ProviderId, ProviderStateKind};

fn fake_snapshot(id: &str, display_name: &str, used_percent: f64) -> ProviderUsageSnapshot {
    fake_snapshot_with(id, display_name, used_percent, None, None, None)
}

fn fake_snapshot_with(
    id: &str,
    display_name: &str,
    used_percent: f64,
    secondary_percent: Option<f64>,
    tertiary_percent: Option<f64>,
    cost: Option<(f64, f64)>,
) -> ProviderUsageSnapshot {
    let window = |percent: f64| RateWindowSnapshot {
        used_percent: percent,
        remaining_percent: 100.0 - percent,
        window_minutes: None,
        resets_at: None,
        reset_description: None,
        is_exhausted: false,
        is_informational: false,
        reserve_percent: None,
        reserve_description: None,
        reserve_will_last_to_reset: false,
        reserve_eta_seconds: None,
    };

    ProviderUsageSnapshot {
        provider_id: id.into(),
        display_name: display_name.into(),
        primary: window(used_percent),
        primary_label: None,
        secondary: secondary_percent.map(window),
        secondary_label: None,
        model_specific: None,
        tertiary: tertiary_percent.map(window),
        tertiary_label: None,
        extra_rate_windows: Vec::new(),
        inventory: Vec::new(),
        display_details: Vec::new(),
        cost: cost.map(|(used, limit)| crate::commands::CostSnapshotBridge {
            used,
            limit: Some(limit),
            remaining: Some((limit - used).max(0.0)),
            currency_code: "USD".to_string(),
            currency_symbol: None,
            period: "monthly".to_string(),
            resets_at: None,
            formatted_used: format!("${used:.2}"),
            formatted_limit: Some(format!("${limit:.2}")),
            balance: None,
            formatted_balance: None,
            balance_updated_at: None,
            account_id: None,
            daily: Vec::new(),
            always_visible: false,
        }),
        plan_name: None,
        account_email: None,
        subscription: None,
        source_label: String::new(),
        has_successful_claude_cli_quota: false,
        updated_at: "2025-01-01T00:00:00Z".into(),
        error: None,
        error_state: ProviderStateKind::Ready,
        pace: None,
        account_organization: None,
        tray_status_label: None,
        fetch_duration_ms: None,
        wayfinder_usage: None,
        session_equivalent_forecast: None,
    }
}

#[test]
fn single_plan_uses_highest_provider_for_icon_and_summary() {
    let settings = Settings {
        tray_icon_mode: TrayIconMode::Single,
        menu_bar_shows_highest_usage: true,
        ..Settings::default()
    };
    let snapshots = vec![
        fake_snapshot("codex", "Codex", 30.0),
        fake_snapshot("claude", "Claude", 72.0),
    ];

    let plan = TrayPresentationPlan::resolve(&settings, &snapshots);

    assert_eq!(
        plan.icon,
        TrayIconPlan::Bars {
            primary_percent: 72.0,
            secondary_percent: None,
            has_error: false,
        }
    );
    assert_eq!(
        plan.status_labels(Language::English),
        vec![("status_summary".to_string(), "Claude 72%".to_string())]
    );
}

#[test]
fn single_plan_borrows_selected_snapshot_from_stable_input() {
    let settings = Settings {
        tray_icon_mode: TrayIconMode::Single,
        menu_bar_shows_highest_usage: true,
        ..Settings::default()
    };
    let snapshots = vec![
        fake_snapshot("codex", "Codex", 30.0),
        fake_snapshot("claude", "Claude", 72.0),
    ];

    // `resolve` drops its temporary ordered/healthy vectors before returning.
    let plan = TrayPresentationPlan::resolve(&settings, &snapshots);

    assert!(std::ptr::eq(plan.status_rows[0].snapshot, &snapshots[1]));
}

#[test]
fn per_provider_plan_preserves_configured_order_for_status_rows() {
    let settings = Settings {
        tray_icon_mode: TrayIconMode::PerProvider,
        provider_order: codexbar::settings::normalize_provider_order(&[
            "claude".to_string(),
            "codex".to_string(),
        ]),
        ..Settings::default()
    };
    let snapshots = vec![
        fake_snapshot("codex", "Codex", 30.0),
        fake_snapshot("claude", "Claude", 72.0),
    ];

    let labels =
        TrayPresentationPlan::resolve(&settings, &snapshots).status_labels(Language::English);

    assert_eq!(
        labels,
        vec![
            ("claude".to_string(), "Claude 72%".to_string()),
            ("codex".to_string(), "Codex 30%".to_string()),
        ]
    );
}

#[test]
fn stacked_plan_resolves_distinct_preferences_once() {
    let settings = Settings {
        tray_icon_mode: TrayIconMode::Stacked,
        stacked_tray_top_provider: Some("claude".to_string()),
        stacked_tray_bottom_provider: Some("codex".to_string()),
        ..Settings::default()
    };
    let snapshots = vec![
        fake_snapshot("codex", "Codex", 30.0),
        fake_snapshot("claude", "Claude", 72.0),
        fake_snapshot("gemini", "Gemini", 44.0),
    ];

    let plan = TrayPresentationPlan::resolve(&settings, &snapshots);

    assert_eq!(
        plan.icon,
        TrayIconPlan::Stacked {
            top_percent: 72.0,
            bottom_percent: 30.0,
            has_error: false,
        }
    );
    assert_eq!(
        plan.status_labels(Language::English),
        vec![
            ("claude".to_string(), "Claude 72%".to_string()),
            ("codex".to_string(), "Codex 30%".to_string()),
        ]
    );
}

#[test]
fn stacked_plan_borrows_both_snapshots_from_stable_input() {
    let settings = Settings {
        tray_icon_mode: TrayIconMode::Stacked,
        stacked_tray_top_provider: Some("claude".to_string()),
        stacked_tray_bottom_provider: Some("codex".to_string()),
        ..Settings::default()
    };
    let snapshots = vec![
        fake_snapshot("codex", "Codex", 30.0),
        fake_snapshot("claude", "Claude", 72.0),
    ];

    // The plan retains references to the caller-owned snapshots, not the
    // temporary vector of references used during selection.
    let plan = TrayPresentationPlan::resolve(&settings, &snapshots);

    assert!(std::ptr::eq(plan.status_rows[0].snapshot, &snapshots[1]));
    assert!(std::ptr::eq(plan.status_rows[1].snapshot, &snapshots[0]));
}

#[test]
fn stacked_plan_falls_back_around_stale_and_duplicate_preferences() {
    let settings = Settings {
        tray_icon_mode: TrayIconMode::Stacked,
        stacked_tray_top_provider: Some("missing".to_string()),
        stacked_tray_bottom_provider: Some("claude".to_string()),
        ..Settings::default()
    };
    let snapshots = vec![
        fake_snapshot("codex", "Codex", 30.0),
        fake_snapshot("claude", "Claude", 72.0),
    ];

    let plan = TrayPresentationPlan::resolve(&settings, &snapshots);

    assert_eq!(
        plan.icon,
        TrayIconPlan::Stacked {
            top_percent: 30.0,
            bottom_percent: 72.0,
            has_error: false,
        }
    );
    assert_eq!(plan.status_rows[0].snapshot.provider_id, "codex");
    assert_eq!(plan.status_rows[1].snapshot.provider_id, "claude");
}

#[test]
fn one_provider_stacked_mode_falls_back_to_single_provider_bars() {
    let settings = Settings {
        tray_icon_mode: TrayIconMode::Stacked,
        ..Settings::default()
    };
    let snapshots = vec![fake_snapshot_with(
        "codex",
        "Codex",
        30.0,
        Some(65.0),
        None,
        None,
    )];

    let plan = TrayPresentationPlan::resolve(&settings, &snapshots);

    assert_eq!(
        plan.icon,
        TrayIconPlan::Bars {
            primary_percent: 65.0,
            secondary_percent: Some(30.0),
            has_error: false,
        }
    );
    assert_eq!(plan.status_rows.len(), 1);
}

#[test]
fn one_healthy_provider_never_uses_stacked_renderer() {
    let settings = Settings {
        tray_icon_mode: TrayIconMode::Stacked,
        menu_bar_shows_percent: true,
        ..Settings::default()
    };
    let healthy = fake_snapshot("codex", "Codex", 30.0);
    let mut failed = fake_snapshot("claude", "Claude", 72.0);
    failed.error = Some("offline".to_string());
    let snapshots = vec![healthy, failed];

    let plan = TrayPresentationPlan::resolve(&settings, &snapshots);

    assert_eq!(
        plan.icon,
        TrayIconPlan::Percent {
            percent: 30.0,
            has_error: false,
        }
    );
    assert_eq!(plan.status_rows.len(), 1);
    assert_eq!(plan.status_rows[0].snapshot.provider_id, "codex");
}

#[test]
fn all_errors_produce_error_styled_zero_percent_plan() {
    let settings = Settings {
        menu_bar_shows_percent: true,
        ..Settings::default()
    };
    let mut snapshot = fake_snapshot("codex", "Codex", 30.0);
    snapshot.error = Some("offline".to_string());
    let snapshots = vec![snapshot];

    let plan = TrayPresentationPlan::resolve(&settings, &snapshots);

    assert_eq!(
        plan.icon,
        TrayIconPlan::Percent {
            percent: 0.0,
            has_error: true,
        }
    );
    assert!(plan.status_rows.is_empty());
}

#[test]
fn plan_uses_selected_metric_and_remaining_display_mode() {
    let mut settings = Settings {
        show_as_used: false,
        ..Settings::default()
    };
    settings.set_provider_metric(ProviderId::Cursor, MetricPreference::ExtraUsage);
    let snapshots = vec![fake_snapshot_with(
        "cursor",
        "Cursor",
        10.0,
        Some(20.0),
        Some(72.0),
        Some((15.0, 100.0)),
    )];

    let plan = TrayPresentationPlan::resolve(&settings, &snapshots);

    assert_eq!(
        plan.icon,
        TrayIconPlan::Bars {
            primary_percent: 85.0,
            secondary_percent: Some(80.0),
            has_error: false,
        }
    );
}

#[test]
fn render_icon_delegates_to_resolved_stacked_renderer() {
    let settings = Settings {
        tray_icon_mode: TrayIconMode::Stacked,
        stacked_tray_top_provider: Some("claude".to_string()),
        stacked_tray_bottom_provider: Some("codex".to_string()),
        ..Settings::default()
    };
    let snapshots = vec![
        fake_snapshot("codex", "Codex", 40.0),
        fake_snapshot("claude", "Claude", 72.0),
    ];
    let plan = TrayPresentationPlan::resolve(&settings, &snapshots);

    assert_eq!(
        plan.render_icon(),
        render_stacked_bar_icon_rgba(72.0, 40.0, false)
    );
}

#[test]
fn codex_headline_skips_informational_primary() {
    let mut snapshot = fake_snapshot_with("codex", "Codex", 0.0, Some(25.0), Some(30.0), None);
    snapshot.primary.is_informational = true;

    assert_eq!(codex_lane_headline_window(&snapshot).used_percent, 25.0);
}
fn fake_extra_window(percent: f64) -> crate::commands::NamedRateWindowSnapshot {
    crate::commands::NamedRateWindowSnapshot {
        id: "additional_budget".to_string(),
        title: "Additional Budget".to_string(),
        fallback_lane: false,
        window: crate::commands::RateWindowSnapshot {
            used_percent: percent,
            remaining_percent: 100.0 - percent,
            window_minutes: None,
            resets_at: None,
            reset_description: None,
            is_exhausted: false,
            is_informational: false,
            reserve_percent: None,
            reserve_description: None,
            reserve_will_last_to_reset: false,
            reserve_eta_seconds: None,
        },
    }
}

#[test]
fn selected_tray_percent_uses_cursor_extra_usage_cost() {
    let mut settings = Settings::default();
    settings.set_provider_metric(ProviderId::Cursor, MetricPreference::ExtraUsage);
    let snapshot = fake_snapshot_with(
        "cursor",
        "Cursor",
        10.0,
        Some(20.0),
        Some(72.0),
        Some((15.0, 100.0)),
    );

    let (primary, secondary) = selected_tray_percents(&snapshot, &settings);

    assert_eq!(primary, 15.0);
    assert_eq!(secondary, Some(20.0));
}

#[test]
fn selected_tray_percent_tracks_extra_rate_window() {
    let mut settings = Settings::default();
    settings.set_provider_metric(ProviderId::Copilot, MetricPreference::ExtraUsage);
    let mut snapshot = fake_snapshot("copilot", "Copilot", 20.0);
    snapshot.extra_rate_windows.push(fake_extra_window(42.0));

    let (primary, secondary) = selected_tray_percents(&snapshot, &settings);

    assert_eq!(primary, 42.0);
    assert_eq!(secondary, None);
}

#[test]
fn copilot_automatic_tracks_highest_extra_rate_window() {
    let settings = Settings::default();
    let mut snapshot = fake_snapshot("copilot", "Copilot", 20.0);
    snapshot.extra_rate_windows.push(fake_extra_window(42.0));

    let (primary, _) = selected_tray_percents(&snapshot, &settings);

    assert_eq!(primary, 42.0);
}

#[test]
fn selected_tray_percent_respects_remaining_display_mode() {
    let mut settings = Settings {
        show_as_used: false,
        ..Settings::default()
    };
    settings.set_provider_metric(ProviderId::Cursor, MetricPreference::ExtraUsage);
    let snapshot = fake_snapshot_with(
        "cursor",
        "Cursor",
        10.0,
        Some(20.0),
        Some(72.0),
        Some((15.0, 100.0)),
    );

    let (primary, secondary) = selected_tray_percents(&snapshot, &settings);

    assert_eq!(primary, 85.0);
    assert_eq!(secondary, Some(80.0));
}

#[test]
fn exhausted_automatic_window_never_renders_as_remaining_progress() {
    let mut settings = Settings {
        show_as_used: false,
        ..Settings::default()
    };
    let mut snapshot = fake_snapshot_with(
        "opencodego",
        "OpenCode Go",
        20.0,
        Some(60.0),
        Some(40.0),
        None,
    );
    snapshot
        .tertiary
        .as_mut()
        .expect("monthly quota")
        .is_exhausted = true;

    let (remaining, _) = selected_tray_percents(&snapshot, &settings);
    assert_eq!(remaining, 0.0);

    settings.show_as_used = true;
    let (used, _) = selected_tray_percents(&snapshot, &settings);
    assert_eq!(used, 100.0);
}

#[test]
fn full_automatic_window_without_exhausted_flag_has_zero_remaining_progress() {
    let mut settings = Settings {
        show_as_used: false,
        ..Settings::default()
    };
    let mut snapshot = fake_snapshot_with(
        "opencodego",
        "OpenCode Go",
        20.0,
        Some(60.0),
        Some(100.0),
        None,
    );
    snapshot
        .tertiary
        .as_mut()
        .expect("monthly quota")
        .is_exhausted = false;

    let (remaining, _) = selected_tray_percents(&snapshot, &settings);
    assert_eq!(remaining, 0.0);

    settings.show_as_used = true;
    let (used, _) = selected_tray_percents(&snapshot, &settings);
    assert_eq!(used, 100.0);
}

#[test]
fn missing_automatic_window_does_not_look_like_available_remaining_progress() {
    let settings = Settings {
        show_as_used: false,
        ..Settings::default()
    };
    let mut snapshot = fake_snapshot_with("opencodego", "OpenCode Go", 0.0, None, None, None);
    snapshot.primary.is_informational = true;

    let (remaining, _) = selected_tray_percents(&snapshot, &settings);

    assert_eq!(remaining, 0.0);
}

#[test]
fn selected_tray_percent_falls_back_when_extra_usage_missing() {
    let mut settings = Settings::default();
    settings.set_provider_metric(ProviderId::Cursor, MetricPreference::ExtraUsage);
    let snapshot = fake_snapshot_with("cursor", "Cursor", 10.0, Some(72.0), None, None);

    let (primary, _) = selected_tray_percents(&snapshot, &settings);

    assert_eq!(primary, 72.0);
}

#[test]
fn single_meaningful_secondary_quota_uses_full_single_meter() {
    let settings = Settings::default();
    let mut snapshot = fake_snapshot_with("claude", "Claude", 0.0, Some(42.0), None, None);
    snapshot.primary.is_informational = true;

    let (primary, secondary) = selected_tray_percents(&snapshot, &settings);

    assert_eq!(primary, 42.0);
    assert_eq!(secondary, None);
}

#[test]
fn selected_secondary_quota_is_not_duplicated_when_tertiary_is_meaningful() {
    let settings = Settings::default();
    let mut snapshot = fake_snapshot_with("claude", "Claude", 0.0, Some(42.0), Some(30.0), None);
    snapshot.primary.is_informational = true;

    let (primary, secondary) = selected_tray_percents(&snapshot, &settings);

    assert_eq!(primary, 42.0);
    assert_eq!(secondary, Some(30.0));
}

#[test]
fn two_meaningful_quotas_keep_two_meter_layout() {
    let mut settings = Settings::default();
    settings.set_provider_metric(ProviderId::Cursor, MetricPreference::Session);
    let snapshot = fake_snapshot_with("cursor", "Cursor", 15.0, Some(40.0), None, None);

    let (primary, secondary) = selected_tray_percents(&snapshot, &settings);

    assert_eq!(primary, 15.0);
    assert_eq!(secondary, Some(40.0));
}

#[test]
fn informational_primary_skips_session_and_automatic_phantom_zero() {
    let mut settings = Settings::default();
    settings.set_provider_metric(ProviderId::Claude, MetricPreference::Session);
    let mut snapshot = fake_snapshot_with("claude", "Claude", 0.0, Some(42.0), None, None);
    snapshot.primary.is_informational = true;

    // Session preference must not paint the synthetic 0% primary;
    // it falls through to Automatic which prefers weekly (42%).
    let (primary, _) = selected_tray_percents(&snapshot, &settings);
    assert_eq!(primary, 42.0);
    assert_ne!(primary, 0.0);

    // Automatic also prefers weekly over informational primary.
    settings.set_provider_metric(ProviderId::Claude, MetricPreference::Automatic);
    let (primary, _) = selected_tray_percents(&snapshot, &settings);
    assert_eq!(primary, 42.0);
}

#[test]
fn claude_automatic_prefers_weekly_when_model_exhausted() {
    let settings = Settings::default();
    let mut snapshot = fake_snapshot_with("claude", "Claude", 40.0, Some(22.0), None, None);
    snapshot.model_specific = Some(crate::commands::RateWindowSnapshot {
        used_percent: 100.0,
        remaining_percent: 0.0,
        window_minutes: Some(10080),
        resets_at: None,
        reset_description: None,
        is_exhausted: true,
        is_informational: false,
        reserve_percent: None,
        reserve_description: None,
        reserve_will_last_to_reset: false,
        reserve_eta_seconds: None,
    });

    let (primary, _) = selected_tray_percents(&snapshot, &settings);
    assert_eq!(primary, 22.0);

    // Explicit model override is untouched.
    let mut overridden = settings.clone();
    overridden.set_provider_metric(ProviderId::Claude, MetricPreference::Model);
    let (primary, _) = selected_tray_percents(&snapshot, &overridden);
    assert_eq!(primary, 100.0);
}

#[test]
fn automatic_prefers_exhausted_weekly_over_low_session() {
    let settings = Settings::default();
    let snapshot = fake_snapshot_with("codex", "Codex", 20.0, Some(100.0), None, None);

    let (primary, _) = selected_tray_percents(&snapshot, &settings);
    assert_eq!(primary, 100.0);

    // Explicit session override still wins.
    let mut overridden = settings.clone();
    overridden.set_provider_metric(ProviderId::Codex, MetricPreference::Session);
    let (primary, _) = selected_tray_percents(&snapshot, &overridden);
    assert_eq!(primary, 20.0);
}

#[test]
fn automatic_picks_highest_among_model_and_extra_windows() {
    let settings = Settings::default();
    let mut snapshot = fake_snapshot_with("gemini", "Gemini", 10.0, Some(30.0), Some(40.0), None);
    snapshot.model_specific = Some(crate::commands::RateWindowSnapshot {
        used_percent: 55.0,
        remaining_percent: 45.0,
        window_minutes: None,
        resets_at: None,
        reset_description: None,
        is_exhausted: false,
        is_informational: false,
        reserve_percent: None,
        reserve_description: None,
        reserve_will_last_to_reset: false,
        reserve_eta_seconds: None,
    });
    snapshot.extra_rate_windows.push(fake_extra_window(90.0));

    let (primary, _) = selected_tray_percents(&snapshot, &settings);
    assert_eq!(primary, 90.0);
}

#[test]
fn f5_headline_prefers_non_informational_primary() {
    let snapshot = fake_snapshot_with("codex", "Codex", 50.0, Some(20.0), Some(30.0), None);
    let headline = codex_lane_headline_window(&snapshot);
    assert!((headline.used_percent - 50.0).abs() < f64::EPSILON);
}

#[test]
fn f5_headline_falls_back_to_secondary_when_primary_informational() {
    let mut snapshot = fake_snapshot_with("codex", "Codex", 0.0, Some(25.0), Some(30.0), None);
    snapshot.primary.is_informational = true;
    let headline = codex_lane_headline_window(&snapshot);
    assert!((headline.used_percent - 25.0).abs() < f64::EPSILON);
}

#[test]
fn f5_headline_falls_back_to_tertiary_when_primary_and_secondary_informational() {
    let mut snapshot = fake_snapshot_with("codex", "Codex", 0.0, Some(0.0), Some(35.0), None);
    snapshot.primary.is_informational = true;
    snapshot.secondary.as_mut().unwrap().is_informational = true;
    let headline = codex_lane_headline_window(&snapshot);
    assert!((headline.used_percent - 35.0).abs() < f64::EPSILON);
}

#[test]
fn f5_headline_returns_primary_when_all_informational() {
    let mut snapshot = fake_snapshot_with("codex", "Codex", 0.0, Some(0.0), Some(0.0), None);
    snapshot.primary.is_informational = true;
    if let Some(sec) = &mut snapshot.secondary {
        sec.is_informational = true;
    }
    if let Some(ter) = &mut snapshot.tertiary {
        ter.is_informational = true;
    }
    let headline = codex_lane_headline_window(&snapshot);
    // Falls back to primary (the placeholder) when all are informational.
    assert!(headline.is_informational);
}
