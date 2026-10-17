use super::*;
use codexbar::core::{ProviderId, ProviderStateKind};
use codexbar::tray::render_stacked_bars_icon_rgba_with_pace;

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
        pace: None,
        monthly_limit_block: None,
        description_is_detail: false,
        reset_regenerates: false,
        hidden_on_card: false,
    };

    ProviderUsageSnapshot {
        provider_id: id.into(),
        display_name: display_name.into(),
        primary: window(used_percent),
        primary_label: None,
        secondary: secondary_percent.map(window),
        secondary_label: None,
        secondary_label_key: None,
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
            folded_into_primary: false,
            title_key: None,
        }),
        plan_name: None,
        account_email: None,
        account_label: None,
        subscription: None,
        source_label: String::new(),
        has_successful_claude_cli_quota: false,
        updated_at: "2025-01-01T00:00:00Z".into(),
        error: None,
        error_state: ProviderStateKind::Ready,
        pace: None,
        pace_authoritative: true,
        account_organization: None,
        open_ai_api_usage: None,
        throughput: None,
        tray_status_label: None,
        fetch_duration_ms: None,
        wayfinder_usage: None,
        session_equivalent_forecast: None,
        quota_burndown: None,
    }
}

#[test]
fn single_plan_uses_highest_provider_for_icon_and_summary() {
    let settings = Settings {
        tray_icon_mode: TrayIconMode::Single,
        menu_bar_shows_highest_usage: true,
        show_as_used: true,
        ..Settings::default()
    };
    let snapshots = vec![
        fake_snapshot("codex", "Codex", 30.0),
        fake_snapshot("claude", "Claude", 72.0),
    ];

    let plan = TrayPresentationPlan::resolve(&settings, &snapshots, None);

    assert_eq!(
        plan.icon,
        TrayIconPlan::Bars {
            primary_percent: 72.0,
            secondary_percent: None,
            has_error: false,
            provider_tint: None,
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
    let plan = TrayPresentationPlan::resolve(&settings, &snapshots, None);

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
        TrayPresentationPlan::resolve(&settings, &snapshots, None).status_labels(Language::English);

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
        show_as_used: true,
        ..Settings::default()
    };
    let snapshots = vec![
        fake_snapshot("codex", "Codex", 30.0),
        fake_snapshot("claude", "Claude", 72.0),
        fake_snapshot("gemini", "Gemini", 44.0),
    ];

    let plan = TrayPresentationPlan::resolve(&settings, &snapshots, None);

    assert_eq!(
        plan.icon,
        TrayIconPlan::Stacked {
            top_percent: 72.0,
            bottom_percent: 30.0,
            has_error: false,
            top_tint: None,
            bottom_tint: None,
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
    let plan = TrayPresentationPlan::resolve(&settings, &snapshots, None);

    assert!(std::ptr::eq(plan.status_rows[0].snapshot, &snapshots[1]));
    assert!(std::ptr::eq(plan.status_rows[1].snapshot, &snapshots[0]));
}

#[test]
fn stacked_plan_falls_back_around_stale_and_duplicate_preferences() {
    let settings = Settings {
        tray_icon_mode: TrayIconMode::Stacked,
        stacked_tray_top_provider: Some("missing".to_string()),
        stacked_tray_bottom_provider: Some("claude".to_string()),
        show_as_used: true,
        ..Settings::default()
    };
    let snapshots = vec![
        fake_snapshot("codex", "Codex", 30.0),
        fake_snapshot("claude", "Claude", 72.0),
    ];

    let plan = TrayPresentationPlan::resolve(&settings, &snapshots, None);

    assert_eq!(
        plan.icon,
        TrayIconPlan::Stacked {
            top_percent: 30.0,
            bottom_percent: 72.0,
            has_error: false,
            top_tint: None,
            bottom_tint: None,
        }
    );
    assert_eq!(plan.status_rows[0].snapshot.provider_id, "codex");
    assert_eq!(plan.status_rows[1].snapshot.provider_id, "claude");
}

#[test]
fn one_provider_stacked_mode_falls_back_to_single_provider_bars() {
    let settings = Settings {
        tray_icon_mode: TrayIconMode::Stacked,
        show_as_used: true,
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

    let plan = TrayPresentationPlan::resolve(&settings, &snapshots, None);

    assert_eq!(
        plan.icon,
        TrayIconPlan::Bars {
            primary_percent: 65.0,
            secondary_percent: Some(30.0),
            has_error: false,
            provider_tint: None,
        }
    );
    assert_eq!(plan.status_rows.len(), 1);
}

#[test]
fn one_healthy_provider_never_uses_stacked_renderer() {
    let settings = Settings {
        tray_icon_mode: TrayIconMode::Stacked,
        menu_bar_icon_style: codexbar::settings::MenuBarIconStyle::Percent,
        show_as_used: true,
        ..Settings::default()
    };
    let healthy = fake_snapshot("codex", "Codex", 30.0);
    let mut failed = fake_snapshot("claude", "Claude", 72.0);
    failed.error = Some("offline".to_string());
    let snapshots = vec![healthy, failed];

    let plan = TrayPresentationPlan::resolve(&settings, &snapshots, None);

    assert_eq!(
        plan.icon,
        TrayIconPlan::Percent {
            percent: 30.0,
            has_error: false,
            provider_tint: None,
        }
    );
    assert_eq!(plan.status_rows.len(), 1);
    assert_eq!(plan.status_rows[0].snapshot.provider_id, "codex");
}

#[test]
fn all_errors_produce_error_styled_zero_percent_plan() {
    let settings = Settings {
        menu_bar_icon_style: codexbar::settings::MenuBarIconStyle::Percent,
        ..Settings::default()
    };
    let mut snapshot = fake_snapshot("codex", "Codex", 30.0);
    snapshot.error = Some("offline".to_string());
    let snapshots = vec![snapshot];

    let plan = TrayPresentationPlan::resolve(&settings, &snapshots, None);

    assert_eq!(
        plan.icon,
        TrayIconPlan::Percent {
            percent: 0.0,
            has_error: true,
            provider_tint: None,
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

    let plan = TrayPresentationPlan::resolve(&settings, &snapshots, None);

    assert_eq!(
        plan.icon,
        TrayIconPlan::Bars {
            primary_percent: 85.0,
            secondary_percent: Some(80.0),
            has_error: false,
            provider_tint: None,
        }
    );
}

#[test]
fn render_icon_delegates_to_resolved_stacked_renderer() {
    let settings = Settings {
        tray_icon_mode: TrayIconMode::Stacked,
        stacked_tray_top_provider: Some("claude".to_string()),
        stacked_tray_bottom_provider: Some("codex".to_string()),
        show_as_used: true,
        ..Settings::default()
    };
    let snapshots = vec![
        fake_snapshot("codex", "Codex", 40.0),
        fake_snapshot("claude", "Claude", 72.0),
    ];
    let plan = TrayPresentationPlan::resolve(&settings, &snapshots, None);

    assert_eq!(
        plan.render_icon(),
        render_stacked_bar_icon_rgba_tinted(72.0, 40.0, false, None, None)
    );
}

#[test]
fn codex_headline_skips_informational_primary() {
    let mut snapshot = fake_snapshot_with("codex", "Codex", 0.0, Some(25.0), Some(30.0), None);
    snapshot.primary.is_informational = true;

    assert_eq!(codex_lane_headline_window(&snapshot).used_percent, 25.0);
}
fn used_view_settings() -> Settings {
    Settings {
        show_as_used: true,
        ..Settings::default()
    }
}

fn fake_extra_window(percent: f64) -> crate::commands::NamedRateWindowSnapshot {
    crate::commands::NamedRateWindowSnapshot {
        id: "additional_budget".to_string(),
        title: "Additional Budget".to_string(),
        fallback_lane: false,
        icon_fallback: None,
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
            pace: None,
            monthly_limit_block: None,
            description_is_detail: false,
            reset_regenerates: false,
            hidden_on_card: false,
        },
    }
}

#[test]
fn selected_tray_percent_uses_cursor_extra_usage_cost() {
    let mut settings = used_view_settings();
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
    let mut settings = used_view_settings();
    settings.set_provider_metric(ProviderId::Copilot, MetricPreference::ExtraUsage);
    let mut snapshot = fake_snapshot("copilot", "Copilot", 20.0);
    snapshot.extra_rate_windows.push(fake_extra_window(42.0));

    let (primary, secondary) = selected_tray_percents(&snapshot, &settings);

    assert_eq!(primary, 42.0);
    assert_eq!(secondary, None);
}

#[test]
fn copilot_automatic_tracks_highest_extra_rate_window() {
    let settings = used_view_settings();
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
    let mut settings = used_view_settings();
    settings.set_provider_metric(ProviderId::Cursor, MetricPreference::ExtraUsage);
    let snapshot = fake_snapshot_with("cursor", "Cursor", 10.0, Some(72.0), None, None);

    let (primary, _) = selected_tray_percents(&snapshot, &settings);

    assert_eq!(primary, 72.0);
}

#[test]
fn single_meaningful_secondary_quota_uses_full_single_meter() {
    let settings = used_view_settings();
    let mut snapshot = fake_snapshot_with("claude", "Claude", 0.0, Some(42.0), None, None);
    snapshot.primary.is_informational = true;

    let (primary, secondary) = selected_tray_percents(&snapshot, &settings);

    assert_eq!(primary, 42.0);
    assert_eq!(secondary, None);
}

#[test]
fn selected_secondary_quota_is_not_duplicated_when_tertiary_is_meaningful() {
    let settings = used_view_settings();
    let mut snapshot = fake_snapshot_with("claude", "Claude", 0.0, Some(42.0), Some(30.0), None);
    snapshot.primary.is_informational = true;

    let (primary, secondary) = selected_tray_percents(&snapshot, &settings);

    assert_eq!(primary, 42.0);
    assert_eq!(secondary, Some(30.0));
}

#[test]
fn two_meaningful_quotas_keep_two_meter_layout() {
    let mut settings = used_view_settings();
    settings.set_provider_metric(ProviderId::Cursor, MetricPreference::Session);
    let snapshot = fake_snapshot_with("cursor", "Cursor", 15.0, Some(40.0), None, None);

    let (primary, secondary) = selected_tray_percents(&snapshot, &settings);

    assert_eq!(primary, 15.0);
    assert_eq!(secondary, Some(40.0));
}

#[test]
fn informational_primary_skips_session_and_automatic_phantom_zero() {
    let mut settings = used_view_settings();
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
    let settings = used_view_settings();
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
        pace: None,
        monthly_limit_block: None,
        description_is_detail: false,
        reset_regenerates: false,
        hidden_on_card: false,
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
    let settings = used_view_settings();
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
    let settings = used_view_settings();
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
        pace: None,
        monthly_limit_block: None,
        description_is_detail: false,
        reset_regenerates: false,
        hidden_on_card: false,
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

fn pace_now() -> chrono::DateTime<chrono::Utc> {
    "2026-10-12T12:00:00Z".parse().expect("fixed now")
}

/// A `minutes`-long window that resets `reset_in` minutes after `pace_now`.
fn paced_window(used: f64, minutes: Option<u32>, reset_in: i64) -> RateWindowSnapshot {
    let mut window = fake_snapshot("claude", "Claude", used).primary;
    window.window_minutes = minutes;
    window.resets_at = Some((pace_now() + chrono::Duration::minutes(reset_in)).to_rfc3339());
    window
}

fn paced_snapshot(id: &str, primary: RateWindowSnapshot) -> ProviderUsageSnapshot {
    let mut snapshot = fake_snapshot(id, id, primary.used_percent);
    snapshot.primary = primary;
    snapshot
}

/// Work days stay off: work-day progress follows the machine time zone.
fn tint_settings() -> Settings {
    Settings {
        menu_bar_color_pace: true,
        weekly_progress_work_days: None,
        ..Settings::default()
    }
}

fn tint(settings: &Settings, snapshots: &[ProviderUsageSnapshot]) -> Option<TrayPaceColor> {
    TrayPresentationPlan::resolve_at(settings, snapshots, None, pace_now()).icon_pace
}

fn render_at(settings: &Settings, snapshots: &[ProviderUsageSnapshot]) -> (Vec<u8>, u32, u32) {
    TrayPresentationPlan::resolve_at(settings, snapshots, None, pace_now()).render_icon()
}

/// Half way through a five-hour window the expected use is 50%.
#[test]
fn tray_pace_tint_is_opt_in_and_follows_the_drawn_window_stage() {
    let behind = vec![paced_snapshot("claude", paced_window(30.0, Some(300), 150))];
    let ahead = vec![paced_snapshot("claude", paced_window(70.0, Some(300), 150))];
    let on_track = vec![paced_snapshot("claude", paced_window(51.0, Some(300), 150))];
    let colored = tint_settings();
    let plain = Settings {
        menu_bar_color_pace: false,
        ..tint_settings()
    };

    assert_eq!(tint(&colored, &behind), Some(TrayPaceColor::Behind));
    assert_eq!(tint(&colored, &ahead), Some(TrayPaceColor::Ahead));
    assert_eq!(tint(&colored, &on_track), None);
    assert_eq!(tint(&plain, &ahead), None);

    for style in [
        codexbar::settings::MenuBarIconStyle::Bars,
        codexbar::settings::MenuBarIconStyle::Percent,
    ] {
        let colored = Settings {
            menu_bar_icon_style: style,
            ..tint_settings()
        };
        let plain = Settings {
            menu_bar_color_pace: false,
            ..colored.clone()
        };
        let plain_behind = render_at(&plain, &behind);
        let tinted_behind = render_at(&colored, &behind);
        // An on-track 51% icon differs from a 30% one only by usage, so
        // compare tinted and untinted icons for the same usage.
        assert_eq!(render_at(&plain, &on_track), render_at(&colored, &on_track));
        assert_ne!(plain_behind, tinted_behind);
        assert_ne!(
            render_at(&colored, &ahead),
            render_at(&plain, &ahead),
            "{style:?} ahead"
        );
    }
}

/// Upstream `menuBarPaceWindow` paces the selected metric window
/// (`percentWindow`), not the session lane.
#[test]
fn tray_pace_tint_follows_the_selected_metric_lane() {
    let mut snapshot = paced_snapshot("claude", paced_window(30.0, Some(300), 150));
    snapshot.secondary = Some(paced_window(70.0, Some(10_080), 5_040));
    let snapshots = vec![snapshot];

    let mut weekly = tint_settings();
    weekly.set_provider_metric(ProviderId::Claude, MetricPreference::Weekly);
    assert_eq!(tint(&weekly, &snapshots), Some(TrayPaceColor::Ahead));

    let mut session = tint_settings();
    session.set_provider_metric(ProviderId::Claude, MetricPreference::Session);
    assert_eq!(tint(&session, &snapshots), Some(TrayPaceColor::Behind));

    // Claude's Automatic metric draws the fuller lane, the weekly one here.
    assert_eq!(
        tint(&tint_settings(), &snapshots),
        Some(TrayPaceColor::Ahead)
    );
}

/// Upstream `menuBarPaceWindow` paces Codex on its weekly lane whatever
/// metric the menu bar shows; no weekly lane means no pace.
#[test]
fn codex_tray_pace_tint_uses_its_weekly_lane() {
    let mut snapshot = paced_snapshot("codex", paced_window(30.0, Some(300), 150));
    snapshot.secondary = Some(paced_window(70.0, Some(10_080), 5_040));
    let mut session = tint_settings();
    session.set_provider_metric(ProviderId::Codex, MetricPreference::Session);
    assert_eq!(tint(&session, &[snapshot]), Some(TrayPaceColor::Ahead));

    let session_only = paced_snapshot("codex", paced_window(70.0, Some(300), 150));
    assert_eq!(tint(&session, &[session_only]), None);
}

/// Upstream `UsageStore.weeklyPace` (UsageStore+HistoricalPace.swift:8-64):
/// no pace without a length, without pace authority, with no quota left, or
/// when it expects less than 3% use.
#[test]
fn tray_pace_tint_follows_the_mac_weekly_pace_guards() {
    let colored = tint_settings();

    let lengthless = vec![paced_snapshot("copilot", paced_window(70.0, None, 150))];
    assert_eq!(tint(&colored, &lengthless), None);

    let mut estimated = paced_snapshot("claude", paced_window(70.0, Some(300), 150));
    estimated.pace_authoritative = false;
    assert_eq!(tint(&colored, &[estimated]), None);

    let exhausted = vec![paced_snapshot(
        "claude",
        paced_window(100.0, Some(300), 150),
    )];
    assert_eq!(tint(&colored, &exhausted), None);

    // 5 of 300 minutes gone: 1.7% expected. 10 gone: 3.3% expected.
    let too_early = vec![paced_snapshot("claude", paced_window(40.0, Some(300), 295))];
    assert_eq!(tint(&colored, &too_early), None);
    let early = vec![paced_snapshot("claude", paced_window(40.0, Some(300), 290))];
    assert_eq!(tint(&colored, &early), Some(TrayPaceColor::Ahead));
}

/// A provider-declared icon lane (Doubao's Agent Plan) is the drawn window,
/// so it drives the tint.
#[test]
fn tray_pace_tint_uses_the_icon_fallback_window() {
    let mut snapshot = paced_snapshot("doubao", paced_window(0.0, None, 150));
    snapshot.primary.is_informational = true;
    snapshot
        .extra_rate_windows
        .push(crate::commands::NamedRateWindowSnapshot {
            id: "doubao-agent-plan".to_string(),
            title: "Agent Plan".to_string(),
            window: paced_window(70.0, Some(300), 150),
            fallback_lane: false,
            icon_fallback: Some(codexbar::core::IconLane::Primary),
        });
    assert_eq!(
        tint(&tint_settings(), &[snapshot]),
        Some(TrayPaceColor::Ahead)
    );
}

/// The Mac menu bar paces a regenerating window that has a length; only the
/// card swaps its pace for the regen line.
#[test]
fn tray_pace_tint_scores_a_regenerating_window_with_a_length() {
    let mut window = paced_window(70.0, Some(300), 150);
    window.reset_regenerates = true;
    let snapshots = vec![paced_snapshot("synthetic", window)];
    assert_eq!(
        tint(&tint_settings(), &snapshots),
        Some(TrayPaceColor::Ahead)
    );
}

#[test]
fn stacked_tray_icon_keeps_usage_colours_when_pace_tint_is_enabled() {
    let mut codex = paced_snapshot("codex", paced_window(50.0, Some(300), 150));
    codex.secondary = Some(paced_window(90.0, Some(10_080), 5_040));
    let claude = paced_snapshot("claude", paced_window(80.0, Some(300), 150));
    let snapshots = vec![codex, claude];
    // Both providers are ahead of pace, so the single icon would tint.
    assert_eq!(
        tint(&tint_settings(), &snapshots),
        Some(TrayPaceColor::Ahead)
    );
    let stacked = |color_pace: bool| Settings {
        tray_icon_mode: TrayIconMode::Stacked,
        menu_bar_color_pace: color_pace,
        ..tint_settings()
    };

    let stacked_settings = stacked(true);
    let plan = TrayPresentationPlan::resolve_at(&stacked_settings, &snapshots, None, pace_now());
    assert!(matches!(plan.icon, TrayIconPlan::Stacked { .. }));
    assert_eq!(plan.icon_pace, None);
    assert_eq!(plan.render_icon(), render_at(&stacked(false), &snapshots));
}

#[test]
fn icon_style_not_the_legacy_percent_flag_picks_the_renderer() {
    let snapshots = vec![fake_snapshot("codex", "Codex", 30.0)];
    let bars = Settings {
        menu_bar_icon_style: codexbar::settings::MenuBarIconStyle::Bars,
        menu_bar_shows_percent: true,
        show_as_used: true,
        ..Settings::default()
    };
    assert!(matches!(
        TrayPresentationPlan::resolve(&bars, &snapshots, None).icon,
        TrayIconPlan::Bars { .. }
    ));
    let percent = Settings {
        menu_bar_icon_style: codexbar::settings::MenuBarIconStyle::Percent,
        menu_bar_shows_percent: false,
        show_as_used: true,
        ..Settings::default()
    };
    assert_eq!(
        TrayPresentationPlan::resolve(&percent, &snapshots, None).icon,
        TrayIconPlan::Percent {
            percent: 30.0,
            has_error: false,
            provider_tint: None,
        }
    );

    // The two styles draw different tray pixels for the same usage.
    let (bar_pixels, bar_w, bar_h) =
        TrayPresentationPlan::resolve(&bars, &snapshots, None).render_icon();
    let (percent_pixels, percent_w, percent_h) =
        TrayPresentationPlan::resolve(&percent, &snapshots, None).render_icon();
    assert_eq!((bar_w, bar_h), (percent_w, percent_h));
    let changed = bar_pixels
        .as_chunks::<4>()
        .0
        .iter()
        .zip(percent_pixels.as_chunks::<4>().0.iter())
        .filter(|(a, b)| a != b)
        .count();
    assert!(
        changed > 0,
        "bars and percent icons rendered identical pixels"
    );
}

#[test]
fn retired_minimal_display_mode_no_longer_forces_most_used_provider() {
    // After the one-shot fold, turning "Show most-used provider" off must win.
    let settings = Settings {
        tray_icon_mode: TrayIconMode::Single,
        menu_bar_display_mode: "minimal".to_string(),
        menu_bar_shows_highest_usage: false,
        show_as_used: true,
        ..Settings::default()
    };
    let snapshots = vec![
        fake_snapshot("codex", "Codex", 30.0),
        fake_snapshot("claude", "Claude", 72.0),
    ];
    let plan = TrayPresentationPlan::resolve(&settings, &snapshots, None);
    assert_eq!(plan.status_rows[0].snapshot.provider_id, "codex");
}

#[test]
fn stacked_bars_style_plans_the_same_windows_as_bars_as_equal_rows() {
    let snapshots = vec![fake_snapshot_with(
        "codex",
        "Codex",
        30.0,
        Some(65.0),
        None,
        None,
    )];
    let bars_settings = Settings {
        menu_bar_icon_style: codexbar::settings::MenuBarIconStyle::Bars,
        show_as_used: true,
        ..Settings::default()
    };
    let stacked_settings = Settings {
        menu_bar_icon_style: codexbar::settings::MenuBarIconStyle::StackedBars,
        show_as_used: true,
        ..Settings::default()
    };

    let TrayIconPlan::Bars {
        primary_percent,
        secondary_percent,
        has_error,
        provider_tint,
    } = TrayPresentationPlan::resolve(&bars_settings, &snapshots, None).icon
    else {
        panic!("bars style should plan Bars");
    };
    let stacked = TrayPresentationPlan::resolve(&stacked_settings, &snapshots, None);
    assert_eq!(
        stacked.icon,
        TrayIconPlan::StackedBars {
            primary_percent,
            secondary_percent,
            has_error,
            provider_tint,
        }
    );
    assert_eq!(
        stacked.render_icon(),
        render_stacked_bars_icon_rgba_with_pace(
            primary_percent,
            secondary_percent,
            has_error,
            None
        )
    );
    assert_ne!(
        stacked.render_icon(),
        TrayPresentationPlan::resolve(&bars_settings, &snapshots, None).render_icon()
    );
}

#[test]
fn stacked_bars_style_with_one_window_draws_the_bars_fallback() {
    let snapshots = vec![fake_snapshot("codex", "Codex", 30.0)];
    let bars_settings = Settings {
        menu_bar_icon_style: codexbar::settings::MenuBarIconStyle::Bars,
        show_as_used: true,
        ..Settings::default()
    };
    let stacked_settings = Settings {
        menu_bar_icon_style: codexbar::settings::MenuBarIconStyle::StackedBars,
        ..bars_settings.clone()
    };
    let bars = TrayPresentationPlan::resolve(&bars_settings, &snapshots, None);
    let stacked = TrayPresentationPlan::resolve(&stacked_settings, &snapshots, None);
    assert!(matches!(
        stacked.icon,
        TrayIconPlan::StackedBars {
            secondary_percent: None,
            ..
        }
    ));
    assert_eq!(stacked.render_icon(), bars.render_icon());
}

#[test]
fn stacked_bars_style_keeps_the_pace_tint_on_both_rows() {
    let settings = Settings {
        menu_bar_icon_style: codexbar::settings::MenuBarIconStyle::StackedBars,
        ..tint_settings()
    };
    let snapshots = vec![paced_snapshot(
        "synthetic",
        paced_window(70.0, Some(300), 150),
    )];
    let plan = TrayPresentationPlan::resolve_at(&settings, &snapshots, None, pace_now());
    assert_eq!(plan.icon_pace, Some(TrayPaceColor::Ahead));

    let TrayIconPlan::StackedBars {
        primary_percent,
        secondary_percent,
        has_error,
        provider_tint: None,
    } = plan.icon
    else {
        panic!("stacked bars style should plan StackedBars");
    };
    assert_eq!(
        plan.render_icon(),
        render_stacked_bars_icon_rgba_with_pace(
            primary_percent,
            secondary_percent,
            has_error,
            Some(TrayPaceColor::Ahead),
        )
    );
}

const CODEX_BRAND: (u8, u8, u8) = (0x49, 0xA3, 0xB0);
const CLAUDE_BRAND: (u8, u8, u8) = (0xCC, 0x7C, 0x5E);

fn color_by_provider_settings(style: codexbar::settings::MenuBarIconStyle) -> Settings {
    Settings {
        menu_bar_color_by_provider: true,
        tray_icon_mode: TrayIconMode::Single,
        menu_bar_icon_style: style,
        show_as_used: true,
        ..Settings::default()
    }
}

fn pixel_at(rgba: &[u8], x: u32, y: u32) -> [u8; 4] {
    let index = ((y * codexbar::tray::TRAY_ICON_SIZE + x) * 4) as usize;
    [
        rgba[index],
        rgba[index + 1],
        rgba[index + 2],
        rgba[index + 3],
    ]
}

#[test]
fn color_by_provider_defaults_off_and_keeps_usage_colors() {
    assert!(!Settings::default().menu_bar_color_by_provider);
    let snapshots = vec![fake_snapshot("codex", "Codex", 30.0)];
    let plain = Settings {
        tray_icon_mode: TrayIconMode::Single,
        show_as_used: true,
        ..Settings::default()
    };
    let plan = TrayPresentationPlan::resolve(&plain, &snapshots, None);
    assert!(matches!(
        plan.icon,
        TrayIconPlan::Bars {
            provider_tint: None,
            ..
        }
    ));
}

#[test]
fn color_by_provider_tints_single_meter_with_brand_color() {
    let snapshots = vec![fake_snapshot("codex", "Codex", 30.0)];
    let settings = color_by_provider_settings(codexbar::settings::MenuBarIconStyle::Bars);
    let plan = TrayPresentationPlan::resolve(&settings, &snapshots, None);
    assert!(matches!(
        plan.icon,
        TrayIconPlan::Bars {
            provider_tint: Some(CODEX_BRAND),
            ..
        }
    ));
    // 30% of the single meter fills x 4..11; x=8 sits in the fill.
    let (rgba, _, _) = plan.render_icon();
    assert_eq!(
        pixel_at(&rgba, 8, 16),
        [CODEX_BRAND.0, CODEX_BRAND.1, CODEX_BRAND.2, 255]
    );
}

#[test]
fn color_by_provider_tints_percent_icon_text() {
    let snapshots = vec![fake_snapshot("codex", "Codex", 30.0)];
    let settings = color_by_provider_settings(codexbar::settings::MenuBarIconStyle::Percent);
    let plan = TrayPresentationPlan::resolve(&settings, &snapshots, None);
    assert!(matches!(
        plan.icon,
        TrayIconPlan::Percent {
            provider_tint: Some(CODEX_BRAND),
            ..
        }
    ));
    let (rgba, _, _) = plan.render_icon();
    let tinted = rgba
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|px| px[..3] == [CODEX_BRAND.0, CODEX_BRAND.1, CODEX_BRAND.2])
        .count();
    assert!(tinted > 0, "percent glyphs should use the brand color");
}

#[test]
fn color_by_provider_uses_the_effective_accent_override() {
    let snapshots = vec![fake_snapshot("codex", "Codex", 30.0)];
    let mut settings = color_by_provider_settings(codexbar::settings::MenuBarIconStyle::Bars);
    settings.set_accent_color(ProviderId::Codex, Some("#FF0000"));
    let plan = TrayPresentationPlan::resolve(&settings, &snapshots, None);
    assert!(matches!(
        plan.icon,
        TrayIconPlan::Bars {
            provider_tint: Some((255, 0, 0)),
            ..
        }
    ));
}

#[test]
fn color_by_provider_malformed_override_gives_no_tint() {
    let snapshots = vec![fake_snapshot("codex", "Codex", 30.0)];
    let mut settings = color_by_provider_settings(codexbar::settings::MenuBarIconStyle::Bars);
    settings.set_accent_color(ProviderId::Codex, Some("not-a-color"));
    let plan = TrayPresentationPlan::resolve(&settings, &snapshots, None);
    assert!(matches!(
        plan.icon,
        TrayIconPlan::Bars {
            provider_tint: None,
            ..
        }
    ));
}

#[test]
fn color_by_provider_colors_each_stacked_row_by_its_provider() {
    let snapshots = vec![
        fake_snapshot("codex", "Codex", 30.0),
        fake_snapshot("claude", "Claude", 72.0),
    ];
    let settings = Settings {
        menu_bar_color_by_provider: true,
        tray_icon_mode: TrayIconMode::Stacked,
        stacked_tray_top_provider: Some("codex".to_string()),
        stacked_tray_bottom_provider: Some("claude".to_string()),
        show_as_used: true,
        ..Settings::default()
    };
    let plan = TrayPresentationPlan::resolve(&settings, &snapshots, None);
    assert!(matches!(
        plan.icon,
        TrayIconPlan::Stacked {
            top_tint: Some(CODEX_BRAND),
            bottom_tint: Some(CLAUDE_BRAND),
            ..
        }
    ));
    let (rgba, width, _) = plan.render_icon();
    let pixel = |x: u32, y: u32| {
        let index = ((y * width + x) * 4) as usize;
        [
            rgba[index],
            rgba[index + 1],
            rgba[index + 2],
            rgba[index + 3],
        ]
    };
    // Top row covers y 6..14, bottom row y 18..26 of the stacked icon.
    assert_eq!(
        pixel(8, 8),
        [CODEX_BRAND.0, CODEX_BRAND.1, CODEX_BRAND.2, 255]
    );
    assert_eq!(
        pixel(8, 20),
        [CLAUDE_BRAND.0, CLAUDE_BRAND.1, CLAUDE_BRAND.2, 255]
    );
}

#[test]
fn parse_hex_rgb_accepts_only_six_digit_hex() {
    assert_eq!(parse_hex_rgb("#49A3B0"), Some(CODEX_BRAND));
    assert_eq!(parse_hex_rgb("49a3b0"), Some(CODEX_BRAND));
    assert_eq!(parse_hex_rgb(" #FF5733 "), Some((255, 87, 51)));
    assert_eq!(parse_hex_rgb("#49A3"), None);
    assert_eq!(parse_hex_rgb("#GG0000"), None);
    assert_eq!(parse_hex_rgb(""), None);
}

#[test]
fn color_by_provider_tints_both_stacked_bars_rows() {
    let snapshots = vec![fake_snapshot_with(
        "codex",
        "Codex",
        30.0,
        Some(65.0),
        None,
        None,
    )];
    let settings = color_by_provider_settings(codexbar::settings::MenuBarIconStyle::StackedBars);
    let plan = TrayPresentationPlan::resolve(&settings, &snapshots, None);
    assert!(matches!(
        plan.icon,
        TrayIconPlan::StackedBars {
            provider_tint: Some(CODEX_BRAND),
            ..
        }
    ));
    let (rgba, _, _) = plan.render_icon();
    assert_eq!(
        pixel_at(&rgba, 8, 8),
        [CODEX_BRAND.0, CODEX_BRAND.1, CODEX_BRAND.2, 255]
    );
    assert_eq!(
        pixel_at(&rgba, 8, 20),
        [CODEX_BRAND.0, CODEX_BRAND.1, CODEX_BRAND.2, 255]
    );
}
