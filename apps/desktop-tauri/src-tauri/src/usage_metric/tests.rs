use super::*;
use codexbar::providers::copilot::SEAT_CREDIT_WINDOW_ID;

fn window(used_percent: f64) -> RateWindowSnapshot {
    derived_window(used_percent, None)
}

fn snapshot() -> ProviderUsageSnapshot {
    ProviderUsageSnapshot {
        provider_id: "codex".to_string(),
        display_name: "Codex".to_string(),
        primary: window(20.0),
        secondary: Some(window(60.0)),
        source_label: "test".to_string(),
        updated_at: "2026-08-16T00:00:00Z".to_string(),
        ..Default::default()
    }
}

#[test]
fn weekly_preference_selects_the_weekly_window() {
    let snapshot = snapshot();
    let mut settings = Settings::default();
    settings.set_provider_metric(ProviderId::Codex, MetricPreference::Weekly);

    assert_eq!(
        selected_usage_window(&snapshot, &settings).used_percent,
        60.0
    );
}

#[test]
fn missing_selected_session_falls_back_to_a_real_window() {
    let mut snapshot = snapshot();
    snapshot.primary.is_informational = true;
    snapshot.primary.used_percent = 0.0;
    let mut settings = Settings::default();
    settings.set_provider_metric(ProviderId::Codex, MetricPreference::Session);

    assert_eq!(
        selected_usage_window(&snapshot, &settings).used_percent,
        60.0
    );
}

#[test]
fn automatic_selects_the_highest_real_window() {
    let snapshot = snapshot();

    assert_eq!(
        selected_usage_window(&snapshot, &Settings::default()).used_percent,
        60.0
    );
}

#[test]
fn copilot_automatic_uses_seat_credit_progress_without_metered_quota() {
    let mut snapshot = snapshot();
    snapshot.provider_id = "copilot".to_string();
    snapshot.primary = RateWindowSnapshot {
        is_informational: true,
        ..window(0.0)
    };
    snapshot.secondary = None;
    snapshot.extra_rate_windows = vec![crate::commands::NamedRateWindowSnapshot {
        id: SEAT_CREDIT_WINDOW_ID.to_string(),
        title: "Credits used".to_string(),
        window: window(35.0),
        fallback_lane: true,
        icon_fallback: None,
    }];

    assert_eq!(
        selected_usage_window(&snapshot, &Settings::default()).used_percent,
        35.0
    );
}

#[test]
fn copilot_automatic_keeps_metered_quota_authoritative_over_seat_credits() {
    let mut snapshot = snapshot();
    snapshot.provider_id = "copilot".to_string();
    snapshot.primary = window(20.0);
    snapshot.secondary = None;
    snapshot.extra_rate_windows = vec![crate::commands::NamedRateWindowSnapshot {
        id: SEAT_CREDIT_WINDOW_ID.to_string(),
        title: "Credits used".to_string(),
        window: window(90.0),
        fallback_lane: true,
        icon_fallback: None,
    }];

    assert_eq!(
        selected_usage_window(&snapshot, &Settings::default()).used_percent,
        20.0
    );
}

#[test]
fn copilot_explicit_session_does_not_fall_back_to_seat_credits() {
    let mut snapshot = snapshot();
    snapshot.provider_id = "copilot".to_string();
    snapshot.primary = RateWindowSnapshot {
        is_informational: true,
        ..window(0.0)
    };
    snapshot.secondary = None;
    snapshot.extra_rate_windows = vec![crate::commands::NamedRateWindowSnapshot {
        id: SEAT_CREDIT_WINDOW_ID.to_string(),
        title: "Credits used".to_string(),
        window: window(35.0),
        fallback_lane: true,
        icon_fallback: None,
    }];
    let mut settings = Settings::default();
    let provider = ProviderId::from_cli_name(&snapshot.provider_id).expect("copilot provider");
    settings.set_provider_metric(provider, MetricPreference::Session);

    let selected = selected_usage_window(&snapshot, &settings);
    assert!(selected.is_informational);
    assert_eq!(selected.used_percent, 0.0);
}

#[test]
fn cursor_automatic_uses_semantic_monthly_lane_and_keeps_grok_bot_explicit() {
    let mut snapshot = snapshot();
    snapshot.provider_id = "cursor".to_string();
    snapshot.primary = window(85.0);
    snapshot.secondary = Some(window(20.0));
    snapshot.extra_rate_windows = vec![crate::commands::NamedRateWindowSnapshot {
        id: "cursor-grok-bot".to_string(),
        title: "Grok Bot".to_string(),
        window: window(95.0),
        fallback_lane: false,
        icon_fallback: None,
    }];

    assert_eq!(
        selected_usage_window(&snapshot, &Settings::default()).used_percent,
        20.0
    );

    let mut settings = Settings::default();
    settings.set_provider_metric(ProviderId::Cursor, MetricPreference::ExtraUsage);
    assert_eq!(
        selected_usage_window(&snapshot, &settings).used_percent,
        95.0
    );
}

fn grok_bot_snapshot(used_percent: Option<f64>) -> ProviderUsageSnapshot {
    let mut snapshot = snapshot();
    snapshot.provider_id = "cursor".to_string();
    snapshot.primary = window(85.0);
    snapshot.secondary = Some(window(20.0));
    snapshot.extra_rate_windows = used_percent
        .map(|used| crate::commands::NamedRateWindowSnapshot {
            id: "cursor-grok-bot".to_string(),
            title: "Grok Bot".to_string(),
            window: window(used),
            fallback_lane: false,
            icon_fallback: None,
        })
        .into_iter()
        .collect();
    snapshot
}

fn grok_bot_settings() -> Settings {
    let mut settings = Settings::default();
    settings.set_provider_metric(
        ProviderId::Cursor,
        MetricPreference::NamedExtra("cursor-grok-bot".to_string()),
    );
    settings
}

#[test]
fn named_extra_preference_selects_its_window_including_a_real_zero() {
    let settings = grok_bot_settings();
    for used in [0.0, 42.0] {
        let selected = selected_usage_window(&grok_bot_snapshot(Some(used)), &settings);
        assert!(!selected.is_informational, "{used}");
        assert_eq!(selected.used_percent, used);
    }
}

#[test]
fn named_extra_without_a_reading_stays_unknown_instead_of_borrowing_a_lane() {
    let selected = selected_usage_window(&grok_bot_snapshot(None), &grok_bot_settings());
    assert!(selected.is_informational);
    assert_eq!(selected.used_percent, 0.0);
}

#[test]
fn saved_grok_bot_reads_the_promoted_weekly_pool() {
    let mut weekly = grok_bot_snapshot(None);
    weekly.primary = window(10.0);
    weekly.secondary = None;
    weekly.primary_label = Some("Weekly".to_string());
    let selected = selected_usage_window(&weekly, &grok_bot_settings());
    assert!(!selected.is_informational);
    assert_eq!(selected.used_percent, 10.0);

    // Without the weekly layout, a missing Grok Bot reading stays unknown.
    let mut monthly = weekly.clone();
    monthly.primary_label = Some("Total".to_string());
    assert!(selected_usage_window(&monthly, &grok_bot_settings()).is_informational);
}

#[test]
fn undeclared_named_extra_falls_back_to_automatic() {
    let mut settings = Settings::default();
    settings.set_provider_metric(
        ProviderId::Cursor,
        MetricPreference::NamedExtra("cursor-retired-window".to_string()),
    );
    // Cursor's Automatic lane is the semantic monthly (secondary) lane.
    assert_eq!(
        selected_usage_window(&grok_bot_snapshot(Some(95.0)), &settings).used_percent,
        20.0
    );

    // Another provider cannot select Cursor's allowance by id.
    let mut codex = grok_bot_snapshot(Some(95.0));
    codex.provider_id = "codex".to_string();
    let mut settings = Settings::default();
    settings.set_provider_metric(
        ProviderId::Codex,
        MetricPreference::NamedExtra("cursor-grok-bot".to_string()),
    );
    assert_eq!(
        selected_usage_window(&codex, &settings).used_percent,
        selected_usage_window(&codex, &Settings::default()).used_percent
    );
}

#[test]
fn kimi_monthly_only_snapshot_selects_the_total_usage_lane() {
    // Upstream 0.60.5 #3694: a Code API response may report only the
    // monthly Total usage pool. The weekly lane is then an informational
    // placeholder, and every metric preference lands on the monthly lane.
    let mut snapshot = snapshot();
    snapshot.provider_id = "kimi".to_string();
    snapshot.primary = RateWindowSnapshot {
        is_informational: true,
        ..window(0.0)
    };
    snapshot.secondary = None;
    snapshot.extra_rate_windows = vec![crate::commands::NamedRateWindowSnapshot {
        id: "kimi-monthly".to_string(),
        title: "Total usage".to_string(),
        window: window(100.0),
        fallback_lane: false,
        icon_fallback: None,
    }];

    for preference in [
        MetricPreference::Automatic,
        MetricPreference::Session,
        MetricPreference::Weekly,
    ] {
        let mut settings = Settings::default();
        settings.set_provider_metric(ProviderId::Kimi, preference.clone());
        let selected = selected_usage_window(&snapshot, &settings);
        assert!(!selected.is_informational, "{preference:?}");
        assert_eq!(selected.used_percent, 100.0, "{preference:?}");
    }
}

#[test]
fn opencodego_automatic_prefers_explicitly_exhausted_window_over_higher_percentage() {
    let mut snapshot = snapshot();
    snapshot.provider_id = "opencodego".to_string();
    snapshot.primary.is_exhausted = true;

    let selected = selected_usage_window(&snapshot, &Settings::default());

    assert_eq!(selected.used_percent, 20.0);
    assert!(selected.is_exhausted);
}

#[test]
fn claude_and_codex_automatic_keep_highest_used_window() {
    for provider_id in ["claude", "codex"] {
        let mut snapshot = snapshot();
        snapshot.provider_id = provider_id.to_string();
        snapshot.primary.is_exhausted = true;

        let selected = selected_usage_window(&snapshot, &Settings::default());

        assert_eq!(
            selected.used_percent, 60.0,
            "{provider_id} should keep highest-used automatic selection"
        );
        assert!(!selected.is_exhausted);
    }
}

#[test]
fn automatic_treats_a_full_window_as_exhausted_even_without_the_flag() {
    let mut snapshot = snapshot();
    let mut full = window(100.0);
    full.is_exhausted = false;
    snapshot.tertiary = Some(full);

    let selected = selected_usage_window(&snapshot, &Settings::default());

    assert_eq!(selected.used_percent, 100.0);
    assert!(!selected.is_exhausted);
}

#[test]
fn non_automatic_highest_window_keeps_percentage_order() {
    let healthy = window(80.0);
    let mut exhausted = window(20.0);
    exhausted.is_exhausted = true;

    let selected = highest_window([&healthy, &exhausted].into_iter()).expect("window");

    assert_eq!(selected.used_percent, 80.0);
}

#[test]
fn antigravity_automatic_prefers_active_core_quota_over_exhausted_extra_window() {
    let mut snapshot = snapshot();
    snapshot.provider_id = "antigravity".to_string();
    snapshot.primary = window(100.0);
    snapshot.primary.is_exhausted = true;
    snapshot.primary_label = Some("Gemini 5h".to_string());
    snapshot.secondary = Some(window(88.0));
    snapshot.secondary_label = Some("Gemini Weekly".to_string());
    snapshot.extra_rate_windows = vec![crate::commands::NamedRateWindowSnapshot {
        id: "antigravity-quota-summary-3p-weekly".to_string(),
        title: "Claude/GPT weekly".to_string(),
        window: window(100.0),
        fallback_lane: false,
        icon_fallback: None,
    }];

    let selected = selected_usage_window(&snapshot, &Settings::default());

    assert_eq!(selected.used_percent, 88.0);
    assert!(!selected.is_exhausted);
}

#[test]
fn antigravity_automatic_uses_core_slots_only() {
    let mut snapshot = snapshot();
    snapshot.provider_id = "antigravity".to_string();
    snapshot.primary = window(80.0);
    snapshot.secondary = Some(window(20.0));
    snapshot.model_specific = Some(window(90.0));
    snapshot.extra_rate_windows = vec![crate::commands::NamedRateWindowSnapshot {
        id: "legacy-other".to_string(),
        title: "Other".to_string(),
        window: window(100.0),
        fallback_lane: false,
        icon_fallback: None,
    }];

    let selected = selected_usage_window(&snapshot, &Settings::default());

    assert_eq!(selected.used_percent, 90.0);
}

#[test]
fn single_meaningful_quota_omits_the_companion_icon_lane() {
    let mut snapshot = snapshot();
    snapshot
        .secondary
        .as_mut()
        .expect("fixture has a secondary window")
        .is_informational = true;

    let (selected, companion) = selected_usage_icon_windows(&snapshot, &Settings::default());

    assert_eq!(selected.used_percent, 20.0);
    assert!(companion.is_none());
}

#[test]
fn average_preference_derives_the_combined_percentage() {
    let mut snapshot = snapshot();
    snapshot.provider_id = "gemini".to_string();
    let mut settings = Settings::default();
    settings.set_provider_metric(ProviderId::Gemini, MetricPreference::Average);

    let selected = selected_usage_window(&snapshot, &settings);
    assert_eq!(selected.used_percent, 40.0);
    assert_eq!(selected.remaining_percent, 60.0);
}

fn mistral_snapshot(plan: Option<RateWindowSnapshot>) -> ProviderUsageSnapshot {
    let mut snapshot = snapshot();
    snapshot.provider_id = "mistral".to_string();
    snapshot.display_name = "Mistral".to_string();
    snapshot.primary = window(2.0);
    snapshot.primary_label = Some("Included API".to_string());
    snapshot.secondary = None;
    snapshot.extra_rate_windows = plan
        .map(|window| crate::commands::NamedRateWindowSnapshot {
            id: codexbar::providers::mistral::MONTHLY_PLAN_WINDOW_ID.to_string(),
            title: "Monthly Plan".to_string(),
            window,
            fallback_lane: false,
            icon_fallback: None,
        })
        .into_iter()
        .collect();
    snapshot
}

fn metric_settings(provider: ProviderId, preference: MetricPreference) -> Settings {
    let mut settings = Settings::default();
    settings.set_provider_metric(provider, preference);
    settings
}

#[test]
fn mistral_monthly_plan_selects_the_vibe_plan_window() {
    let mut snapshot = mistral_snapshot(Some(window(42.0)));
    snapshot.primary = window(80.0);

    let monthly_plan = metric_settings(ProviderId::Mistral, MetricPreference::MonthlyPlan);
    assert_eq!(
        selected_usage_window(&snapshot, &monthly_plan).used_percent,
        42.0
    );
    // Included API and Automatic stay separate choices (upstream #4072).
    let included_api = metric_settings(ProviderId::Mistral, MetricPreference::Session);
    assert_eq!(
        selected_usage_window(&snapshot, &included_api).used_percent,
        80.0
    );
    assert_eq!(
        selected_usage_window(&snapshot, &Settings::default()).used_percent,
        80.0
    );
    let presentation =
        crate::commands::ProviderUsagePresentationSnapshot::new(snapshot, &monthly_plan);
    assert_eq!(presentation.selected_metric.used_percent, 42.0);
}

#[test]
fn mistral_monthly_plan_falls_back_to_included_api_without_a_known_plan() {
    let settings = metric_settings(ProviderId::Mistral, MetricPreference::MonthlyPlan);
    let unknown_plan = RateWindowSnapshot {
        is_informational: true,
        ..window(0.0)
    };

    for snapshot in [mistral_snapshot(None), mistral_snapshot(Some(unknown_plan))] {
        let selected = selected_usage_window(&snapshot, &settings);
        assert_eq!(selected.used_percent, 2.0);
        assert!(!selected.is_informational);
    }
}

#[test]
fn mistral_monthly_plan_never_selects_a_spend_window() {
    let mut snapshot = mistral_snapshot(None);
    snapshot.cost = Some(crate::commands::CostSnapshotBridge {
        used: 45.0,
        limit: Some(50.0),
        remaining: Some(5.0),
        currency_code: "EUR".to_string(),
        currency_symbol: Some("€".to_string()),
        period: "month".to_string(),
        resets_at: None,
        formatted_used: "€45.00".to_string(),
        formatted_limit: Some("€50.00".to_string()),
        balance: None,
        balance_updated_at: None,
        account_id: None,
        formatted_balance: None,
        daily: Vec::new(),
        always_visible: false,
        folded_into_primary: false,
        pay_as_you_go: false,
        title_key: None,
    });
    let settings = metric_settings(ProviderId::Mistral, MetricPreference::MonthlyPlan);

    assert_eq!(
        selected_usage_window(&snapshot, &settings).used_percent,
        2.0
    );
}

#[test]
fn monthly_plan_without_a_provider_plan_window_falls_through_to_automatic() {
    let mut snapshot = snapshot();
    snapshot.extra_rate_windows = vec![crate::commands::NamedRateWindowSnapshot {
        id: codexbar::providers::mistral::MONTHLY_PLAN_WINDOW_ID.to_string(),
        title: "Monthly Plan".to_string(),
        window: window(90.0),
        fallback_lane: false,
        icon_fallback: None,
    }];
    let settings = metric_settings(ProviderId::Codex, MetricPreference::MonthlyPlan);

    // Codex declares no plan window, so a same-named lane is not special
    // and the choice resolves like Automatic (highest real window).
    assert_eq!(
        selected_usage_window(&snapshot, &settings).used_percent,
        selected_usage_window(&snapshot, &Settings::default()).used_percent
    );
    assert!(monthly_plan_window(&snapshot, Some(ProviderId::Codex)).is_none());
}

#[test]
fn presentation_payload_flattens_the_snapshot_and_selected_metric() {
    let presentation =
        crate::commands::ProviderUsagePresentationSnapshot::new(snapshot(), &Settings::default());
    let value = serde_json::to_value(presentation).expect("serialize presentation");

    assert_eq!(value["providerId"], "codex");
    assert_eq!(value["selectedMetric"]["usedPercent"], 60.0);
    assert!(value.get("snapshot").is_none());
}

#[test]
fn hidden_usage_items_are_presentation_metadata_and_do_not_change_selected_metric() {
    let mut settings = Settings::default();
    settings.set_hidden_usage_item_ids(ProviderId::Codex, vec!["metric:secondary".to_string()]);

    let presentation =
        crate::commands::ProviderUsagePresentationSnapshot::new(snapshot(), &settings);

    assert!(presentation.snapshot.secondary.is_some());
    assert_eq!(presentation.selected_metric.used_percent, 60.0);
    assert_eq!(
        presentation.hidden_usage_item_ids,
        vec!["metric:secondary".to_string()]
    );
    let value = serde_json::to_value(presentation).expect("serialize presentation");
    assert_eq!(
        value["hiddenUsageItemIds"],
        serde_json::json!(["metric:secondary"])
    );
}

#[test]
fn claude_routines_hide_and_restore_preserve_raw_data_and_selected_metric() {
    let mut raw = snapshot();
    raw.provider_id = "claude".to_string();
    raw.extra_rate_windows = vec![crate::commands::NamedRateWindowSnapshot {
        id: "claude-routines".to_string(),
        title: "Daily Routines".to_string(),
        window: window(95.0),
        fallback_lane: false,
        icon_fallback: None,
    }];

    let baseline =
        crate::commands::ProviderUsagePresentationSnapshot::new(raw.clone(), &Settings::default());
    assert_eq!(baseline.selected_metric.used_percent, 95.0);

    let mut settings = Settings::default();
    settings.set_hidden_usage_item_ids(
        ProviderId::Claude,
        vec![codexbar::settings::CLAUDE_DAILY_ROUTINES_USAGE_ITEM_ID.to_string()],
    );
    let hidden = crate::commands::ProviderUsagePresentationSnapshot::new(raw.clone(), &settings);

    assert_eq!(hidden.selected_metric.used_percent, 95.0);
    assert_eq!(hidden.snapshot.extra_rate_windows[0].id, "claude-routines");
    assert_eq!(
        hidden.snapshot.extra_rate_windows[0].window.used_percent,
        95.0
    );

    settings.set_hidden_usage_item_ids(ProviderId::Claude, Vec::new());
    let restored = crate::commands::ProviderUsagePresentationSnapshot::new(raw, &settings);

    assert_eq!(restored.selected_metric.used_percent, 95.0);
    assert_eq!(
        restored.snapshot.extra_rate_windows[0].id,
        "claude-routines"
    );
    assert!(restored.hidden_usage_item_ids.is_empty());
}

fn litellm_budgets(personal: f64, team: Option<f64>) -> ProviderUsageSnapshot {
    let mut snapshot = snapshot();
    snapshot.provider_id = "litellm".to_string();
    snapshot.primary = window(personal);
    snapshot.secondary = team.map(window);
    snapshot
}

const AGENT_RESET: &str = "2026-08-16T05:00:00Z";

fn agent_window(used_percent: f64, minutes: u32) -> RateWindowSnapshot {
    RateWindowSnapshot {
        window_minutes: Some(minutes),
        resets_at: Some(AGENT_RESET.to_string()),
        ..window(used_percent)
    }
}

fn agent_extra(
    id: &str,
    window: RateWindowSnapshot,
    icon_fallback: Option<IconLane>,
) -> crate::commands::NamedRateWindowSnapshot {
    crate::commands::NamedRateWindowSnapshot {
        id: id.to_string(),
        title: id.to_string(),
        window,
        fallback_lane: false,
        icon_fallback,
    }
}

/// Bridge shape of a Doubao Coding Plan snapshot that also reports Agent
/// Plan lanes (`doubao-agent-*`), with optional Coding Plan session/weekly.
fn doubao_snapshot(
    coding_session: Option<f64>,
    coding_weekly: Option<f64>,
) -> ProviderUsageSnapshot {
    let mut snapshot = snapshot();
    snapshot.provider_id = "doubao".to_string();
    snapshot.primary = coding_session.map_or_else(
        || RateWindowSnapshot {
            window_minutes: Some(300),
            is_informational: true,
            ..window(0.0)
        },
        window,
    );
    snapshot.secondary = coding_weekly.map(window);
    snapshot.extra_rate_windows = vec![
        agent_extra(
            "doubao-agent-session",
            agent_window(0.0, 300),
            Some(IconLane::Primary),
        ),
        agent_extra(
            "doubao-agent-weekly",
            agent_window(31.0, 10_080),
            Some(IconLane::Secondary),
        ),
        agent_extra("doubao-agent-monthly", agent_window(72.0, 43_200), None),
    ];
    snapshot
}

#[test]
fn litellm_automatic_prefers_the_team_budget_over_a_fuller_personal_budget() {
    let snapshot = litellm_budgets(60.0, Some(7.0));

    assert_eq!(
        selected_usage_window(&snapshot, &Settings::default()).used_percent,
        7.0
    );
    let (selected, companion) = selected_usage_icon_windows(&snapshot, &Settings::default());
    assert_eq!(selected.used_percent, 7.0);
    assert_eq!(companion.map(|window| window.used_percent), Some(60.0));
}

#[test]
fn litellm_automatic_shows_an_exhausted_budget_first() {
    let personal_exhausted = litellm_budgets(100.0, Some(7.0));
    assert_eq!(
        selected_usage_window(&personal_exhausted, &Settings::default()).used_percent,
        100.0
    );

    let team_exhausted = litellm_budgets(40.0, Some(100.0));
    assert_eq!(
        selected_usage_window(&team_exhausted, &Settings::default()).used_percent,
        100.0
    );
}

#[test]
fn litellm_automatic_uses_the_only_budget_and_explicit_choices_still_win() {
    let personal_only = litellm_budgets(25.0, None);
    assert_eq!(
        selected_usage_window(&personal_only, &Settings::default()).used_percent,
        25.0
    );

    let mut settings = Settings::default();
    settings.set_provider_metric(ProviderId::LiteLLM, MetricPreference::Session);
    assert_eq!(
        selected_usage_window(&litellm_budgets(60.0, Some(7.0)), &settings).used_percent,
        60.0
    );
}

#[test]
fn doubao_icon_windows_fall_back_to_agent_plan_lanes() {
    let mut settings = Settings::default();
    settings.set_provider_metric(ProviderId::Doubao, MetricPreference::Session);

    for (has_session, has_weekly) in [(false, false), (false, true), (true, false), (true, true)] {
        let snapshot = doubao_snapshot(has_session.then_some(10.0), has_weekly.then_some(20.0));
        let (primary, secondary) = selected_usage_icon_windows(&snapshot, &settings);
        let secondary = secondary.expect("secondary icon lane");

        assert_eq!(primary.used_percent, if has_session { 10.0 } else { 0.0 });
        assert_eq!(secondary.used_percent, if has_weekly { 20.0 } else { 31.0 });
        assert!(!primary.is_informational);
        assert_eq!(
            primary.window_minutes,
            if has_session { None } else { Some(300) }
        );
        assert_eq!(
            secondary.window_minutes,
            if has_weekly { None } else { Some(10_080) }
        );
        assert_eq!(
            primary.resets_at.as_deref(),
            (!has_session).then_some(AGENT_RESET)
        );
        assert_eq!(
            secondary.resets_at.as_deref(),
            (!has_weekly).then_some(AGENT_RESET)
        );
    }
}

#[test]
fn doubao_icon_fallback_does_not_change_the_reported_snapshot() {
    let raw = doubao_snapshot(None, None);
    let presentation =
        crate::commands::ProviderUsagePresentationSnapshot::new(raw.clone(), &Settings::default());

    assert!(presentation.snapshot.primary.is_informational);
    assert!(presentation.snapshot.secondary.is_none());
    assert!(presentation.snapshot.tertiary.is_none());
    assert_eq!(
        presentation
            .snapshot
            .extra_rate_windows
            .iter()
            .map(|extra| extra.id.as_str())
            .collect::<Vec<_>>(),
        [
            "doubao-agent-session",
            "doubao-agent-weekly",
            "doubao-agent-monthly"
        ]
    );
    assert!(!presentation.selected_metric.is_informational);
}

#[test]
fn doubao_icon_windows_preserve_missing_lanes() {
    let no_hints = |levels: &[(&str, f64, u32)]| {
        let mut snapshot = doubao_snapshot(None, None);
        snapshot.extra_rate_windows = levels
            .iter()
            .map(|(id, percent, minutes)| agent_extra(id, agent_window(*percent, *minutes), None))
            .collect();
        snapshot
    };
    // Monthly and team buckets never stand in for the session/weekly lanes.
    for id in ["doubao-agent-monthly", "doubao-agent-team-session"] {
        let snapshot = no_hints(&[(id, 25.0, 300)]);
        let resolved = with_icon_fallbacks(&snapshot);
        assert!(matches!(resolved, Cow::Borrowed(_)), "{id}");
        assert!(resolved.primary.is_informational, "{id}");
        assert!(resolved.secondary.is_none(), "{id}");
    }

    // Agent weekly alone fills only the weekly lane; no session is invented.
    let mut snapshot = doubao_snapshot(None, None);
    snapshot
        .extra_rate_windows
        .retain(|extra| extra.id == "doubao-agent-weekly");
    let resolved = with_icon_fallbacks(&snapshot);
    assert!(resolved.primary.is_informational);
    assert_eq!(resolved.secondary.as_ref().unwrap().used_percent, 31.0);

    // Coding session alone (agent lanes absent): unchanged, nothing invented.
    let mut snapshot = doubao_snapshot(Some(25.0), None);
    snapshot.extra_rate_windows.clear();
    let resolved = with_icon_fallbacks(&snapshot);
    assert!(matches!(resolved, Cow::Borrowed(_)));
    let (primary, companion) = selected_usage_icon_windows(&snapshot, &Settings::default());
    assert_eq!(primary.used_percent, 25.0);
    assert!(companion.is_none());
}

#[test]
fn doubao_automatic_icon_uses_agent_lanes_not_team_windows() {
    let mut snapshot = doubao_snapshot(None, None);
    snapshot.extra_rate_windows = vec![
        agent_extra(
            "doubao-agent-session",
            agent_window(42.0, 300),
            Some(IconLane::Primary),
        ),
        agent_extra(
            "doubao-agent-weekly",
            agent_window(67.0, 10_080),
            Some(IconLane::Secondary),
        ),
        agent_extra("doubao-agent-team-session", agent_window(91.0, 300), None),
        agent_extra("doubao-agent-team-weekly", agent_window(88.0, 10_080), None),
    ];
    let settings = Settings::default();

    let (top, bottom) = selected_usage_icon_windows(&snapshot, &settings);
    assert_eq!(top.used_percent, 42.0);
    assert_eq!(bottom.expect("bottom lane").used_percent, 67.0);
    assert_eq!(
        selected_usage_window(&snapshot, &settings).used_percent,
        67.0
    );
    assert_eq!(
        crate::tray_presentation::headline_window(&snapshot).used_percent,
        42.0
    );
}

#[test]
fn icon_fallback_hints_are_ignored_while_the_core_lane_is_present() {
    let snapshot = doubao_snapshot(Some(10.0), Some(20.0));
    let resolved = with_icon_fallbacks(&snapshot);

    assert!(matches!(resolved, Cow::Borrowed(_)));
    assert_eq!(resolved.primary.used_percent, 10.0);
    assert_eq!(resolved.secondary.as_ref().unwrap().used_percent, 20.0);
}

fn claude_extra(id: &str, used_percent: f64) -> crate::commands::NamedRateWindowSnapshot {
    crate::commands::NamedRateWindowSnapshot {
        id: id.to_string(),
        title: id.to_string(),
        window: window(used_percent),
        fallback_lane: false,
        icon_fallback: None,
    }
}

/// Shape of a Claude account that reports only model-scoped weekly limits
/// (upstream #4126): a session placeholder, no weekly lane, scoped extras.
fn claude_scoped_only_snapshot() -> ProviderUsageSnapshot {
    let mut snapshot = snapshot();
    snapshot.provider_id = "claude".to_string();
    snapshot.primary = RateWindowSnapshot {
        is_informational: true,
        ..window(0.0)
    };
    snapshot.secondary = None;
    snapshot.extra_rate_windows = vec![
        claude_extra("claude-weekly-scoped-fable", 12.0),
        claude_extra("claude-weekly-scoped-sonnet", 40.0),
        claude_extra("claude-routines", 70.0),
    ];
    snapshot
}

#[test]
fn claude_automatic_selects_the_most_constrained_scoped_weekly_without_core_lanes() {
    let snapshot = claude_scoped_only_snapshot();
    let settings = Settings::default();

    let selected = selected_usage_window(&snapshot, &settings);
    assert!(!selected.is_informational);
    assert_eq!(selected.used_percent, 40.0);
    let (top, bottom) = selected_usage_icon_windows(&snapshot, &settings);
    assert_eq!(top.used_percent, 40.0);
    assert!(bottom.is_none());
}

#[test]
fn claude_session_and_weekly_preferences_fall_back_to_the_scoped_weekly() {
    let snapshot = claude_scoped_only_snapshot();
    for preference in [MetricPreference::Session, MetricPreference::Weekly] {
        let mut settings = Settings::default();
        settings.set_provider_metric(ProviderId::Claude, preference.clone());

        assert_eq!(
            selected_usage_window(&snapshot, &settings).used_percent,
            40.0,
            "{preference:?}"
        );
    }
}

#[test]
fn claude_scoped_weekly_does_not_replace_a_reported_weekly_lane() {
    let mut snapshot = claude_scoped_only_snapshot();
    snapshot.secondary = Some(window(30.0));

    assert_eq!(
        selected_usage_window(&snapshot, &Settings::default()).used_percent,
        30.0
    );
}

#[test]
fn scoped_weekly_fallback_is_claude_owned() {
    let mut snapshot = claude_scoped_only_snapshot();
    snapshot.provider_id = "codex".to_string();

    assert!(selected_usage_window(&snapshot, &Settings::default()).is_informational);
}

fn depletion_now() -> DateTime<Utc> {
    use chrono::TimeZone;
    Utc.with_ymd_and_hms(2026, 8, 16, 12, 0, 0).unwrap()
}

/// A window of `minutes` at `used_percent` that resets `remaining_minutes`
/// after [`depletion_now`].
fn timed_window(used_percent: f64, minutes: u32, remaining_minutes: i64) -> RateWindowSnapshot {
    let resets_at = depletion_now() + chrono::Duration::minutes(remaining_minutes);
    RateWindowSnapshot {
        window_minutes: Some(minutes),
        ..derived_window(used_percent, Some(resets_at.to_rfc3339()))
    }
}

/// Session at 60% one hour into five (runs out in 40 minutes) beside a
/// weekly at 90% one day into seven (runs out in about 2.7 hours).
fn depleting_session_snapshot() -> ProviderUsageSnapshot {
    ProviderUsageSnapshot {
        primary: timed_window(60.0, 300, 240),
        secondary: Some(timed_window(90.0, 10_080, 6 * 1_440)),
        ..snapshot()
    }
}

fn automatic_at(snapshot: &ProviderUsageSnapshot) -> f64 {
    selected_usage_window_at(snapshot, &Settings::default(), depletion_now()).used_percent
}

#[test]
fn automatic_prefers_the_window_that_runs_out_first_over_the_fuller_one() {
    assert_eq!(automatic_at(&depleting_session_snapshot()), 60.0);
}

#[test]
fn automatic_ranks_a_forecast_depletion_above_a_window_that_lasts() {
    let mut snapshot = depleting_session_snapshot();
    // 40% three days into seven lasts to its reset.
    snapshot.secondary = Some(timed_window(40.0, 10_080, 4 * 1_440));
    snapshot.primary = timed_window(30.0, 300, 240);

    assert_eq!(automatic_at(&snapshot), 30.0);
}

#[test]
fn automatic_ignores_an_immature_forecast() {
    let mut snapshot = depleting_session_snapshot();
    // Five minutes into a five-hour session expects under 3% use.
    snapshot.primary = timed_window(10.0, 300, 295);
    snapshot.secondary = Some(timed_window(40.0, 10_080, 4 * 1_440));

    assert_eq!(automatic_at(&snapshot), 40.0);
}

#[test]
fn automatic_keeps_an_exhausted_window_ahead_of_a_forecast() {
    let mut snapshot = depleting_session_snapshot();
    snapshot.secondary = Some(timed_window(100.0, 10_080, 6 * 1_440));

    assert_eq!(automatic_at(&snapshot), 100.0);
}

#[test]
fn automatic_does_not_forecast_without_authoritative_pace() {
    let mut snapshot = depleting_session_snapshot();
    snapshot.pace_authoritative = false;

    assert_eq!(automatic_at(&snapshot), 90.0);
}

#[test]
fn automatic_does_not_forecast_a_window_without_its_own_length() {
    let mut snapshot = depleting_session_snapshot();
    snapshot.primary.window_minutes = None;

    assert_eq!(automatic_at(&snapshot), 90.0);
}

#[test]
fn equal_automatic_ranks_keep_the_earlier_lane() {
    let mut snapshot = snapshot();
    snapshot.primary = window(50.0);
    snapshot.primary.reset_description = Some("session".to_string());
    snapshot.secondary = Some(window(50.0));

    assert_eq!(
        selected_usage_window(&snapshot, &Settings::default())
            .reset_description
            .as_deref(),
        Some("session")
    );
}

fn gemini_snapshot(pro: f64, flash: f64) -> ProviderUsageSnapshot {
    ProviderUsageSnapshot {
        provider_id: "gemini".to_string(),
        display_name: "Gemini".to_string(),
        primary: window(pro),
        secondary: None,
        model_specific: Some(window(flash)),
        ..snapshot()
    }
}

#[test]
fn gemini_automatic_keeps_the_pro_tier_over_a_fuller_flash_tier() {
    assert_eq!(automatic_at(&gemini_snapshot(30.0, 80.0)), 30.0);
}

#[test]
fn gemini_automatic_selects_an_exhausted_tier_first() {
    assert_eq!(automatic_at(&gemini_snapshot(30.0, 100.0)), 100.0);
}

#[test]
fn kiro_automatic_keeps_plan_credits_until_a_lane_is_exhausted() {
    let mut snapshot = snapshot();
    snapshot.provider_id = "kiro".to_string();
    snapshot.primary = window(20.0);
    snapshot.secondary = Some(window(90.0));
    assert_eq!(automatic_at(&snapshot), 20.0);

    snapshot.secondary = Some(window(100.0));
    assert_eq!(automatic_at(&snapshot), 100.0);
}

#[test]
fn highest_usage_rank_forecasts_only_an_automatic_metric() {
    let now = depletion_now();
    let depleting = depleting_session_snapshot();
    let mut fuller = snapshot();
    fuller.provider_id = "claude".to_string();
    fuller.primary = window(80.0);
    fuller.secondary = None;

    let automatic = Settings::default();
    assert!(
        highest_usage_rank(&depleting, &automatic, now)
            .outranks(&highest_usage_rank(&fuller, &automatic, now))
    );

    let mut explicit = Settings::default();
    explicit.set_provider_metric(ProviderId::Codex, MetricPreference::Session);
    assert!(
        highest_usage_rank(&fuller, &explicit, now)
            .outranks(&highest_usage_rank(&depleting, &explicit, now))
    );
}
