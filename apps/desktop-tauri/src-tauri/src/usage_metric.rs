//! Canonical single-metric selection shared by native and webview surfaces.

use std::cmp::Ordering;

use codexbar::core::ProviderId;
use codexbar::settings::{MetricPreference, Settings};

use crate::commands::{ProviderUsageSnapshot, RateWindowSnapshot};

pub(crate) fn selected_usage_window(
    snapshot: &ProviderUsageSnapshot,
    settings: &Settings,
) -> RateWindowSnapshot {
    let provider = ProviderId::from_cli_name(&snapshot.provider_id);
    let preference = provider
        .map(|id| settings.get_provider_metric(id))
        .unwrap_or_default();

    if let Some(selected) = preferred_window(snapshot, provider, preference) {
        return selected;
    }
    // Providers with Automatic-only fallback lanes (seat credits) keep an
    // explicit metric choice authoritative when its corresponding lane is not
    // available instead of silently replacing it with fallback progress.
    if provider.is_some_and(|id| {
        !codexbar::core::instantiate_provider(id).explicit_preference_falls_through_to_automatic()
    }) {
        return snapshot.primary.clone();
    }
    automatic_window(snapshot, provider).unwrap_or_else(|| snapshot.primary.clone())
}

/// Select the primary tray metric and, when there are multiple meaningful core
/// quotas, one distinct companion lane. Keeping this policy beside canonical
/// metric selection prevents tray rendering from duplicating the selected lane.
pub(crate) fn selected_usage_icon_windows(
    snapshot: &ProviderUsageSnapshot,
    settings: &Settings,
) -> (RateWindowSnapshot, Option<RateWindowSnapshot>) {
    let selected = selected_usage_window(snapshot, settings);
    let meaningful_count = std::iter::once(&snapshot.primary)
        .chain(snapshot.secondary.iter())
        .chain(snapshot.tertiary.iter())
        .filter(|window| !window.is_informational)
        .count();
    if meaningful_count <= 1 {
        return (selected, None);
    }

    let companion = snapshot
        .secondary
        .iter()
        .chain(std::iter::once(&snapshot.primary))
        .chain(snapshot.tertiary.iter())
        .filter(|window| !window.is_informational)
        .find(|window| !same_window(window, &selected))
        .cloned();
    (selected, companion)
}

fn same_window(left: &RateWindowSnapshot, right: &RateWindowSnapshot) -> bool {
    left.used_percent.to_bits() == right.used_percent.to_bits()
        && left.window_minutes == right.window_minutes
        && left.resets_at == right.resets_at
        && left.reset_description == right.reset_description
        && left.is_informational == right.is_informational
}

fn preferred_window(
    snapshot: &ProviderUsageSnapshot,
    provider: Option<ProviderId>,
    preference: MetricPreference,
) -> Option<RateWindowSnapshot> {
    match preference {
        MetricPreference::Automatic => automatic_window(snapshot, provider),
        // A missing session is represented by an informational zero-percent
        // placeholder. Fall through to Automatic instead of displaying it.
        MetricPreference::Session if snapshot.primary.is_informational => None,
        MetricPreference::Session => Some(snapshot.primary.clone()),
        MetricPreference::Weekly => non_informational(snapshot.secondary.as_ref())
            .or_else(|| non_informational(Some(&snapshot.primary)))
            .cloned(),
        MetricPreference::Model => snapshot
            .model_specific
            .clone()
            .or_else(|| non_informational(Some(&snapshot.primary)).cloned()),
        MetricPreference::Tertiary => snapshot
            .tertiary
            .clone()
            .or_else(|| snapshot.secondary.clone())
            .or_else(|| non_informational(Some(&snapshot.primary)).cloned()),
        MetricPreference::Credits => cost_window(snapshot),
        MetricPreference::ExtraUsage => {
            extra_usage_window(snapshot).or_else(|| cost_window(snapshot))
        }
        MetricPreference::Average => average_window(snapshot),
        MetricPreference::MonthlyPlan => cost_window(snapshot),
    }
}

fn automatic_window(
    snapshot: &ProviderUsageSnapshot,
    provider: Option<ProviderId>,
) -> Option<RateWindowSnapshot> {
    // Cursor's Auto usage is the monthly included allowance, surfaced by the
    // provider in the semantic secondary slot. Do not let a higher percentage
    // in the aggregate or API slot change which quota Automatic represents.
    if provider == Some(ProviderId::Cursor)
        && let Some(semantic_monthly) = non_informational(snapshot.secondary.as_ref())
    {
        return Some(semantic_monthly.clone());
    }

    if provider == Some(ProviderId::Claude) {
        let weekly = non_informational(snapshot.secondary.as_ref());
        if let (Some(model), Some(weekly)) = (snapshot.model_specific.as_ref(), weekly) {
            let model_exhausted = model.is_exhausted || model.used_percent >= 100.0;
            let weekly_has_remaining = !weekly.is_exhausted && weekly.used_percent < 100.0;
            if model_exhausted && weekly_has_remaining {
                return Some(weekly.clone());
            }
        }
        if snapshot.primary.is_informational
            && let Some(weekly) = weekly
        {
            return Some(weekly.clone());
        }
    }

    let policy = automatic_metric_policy(provider);

    if snapshot.primary.is_informational
        && policy.missing_core_is_terminal
        && snapshot.secondary.is_none()
    {
        return None;
    }

    let mut windows = Vec::with_capacity(4 + snapshot.extra_rate_windows.len());
    windows.push(&snapshot.primary);
    windows.extend(snapshot.secondary.iter());
    windows.extend(snapshot.model_specific.iter());
    windows.extend(snapshot.tertiary.iter());
    let has_core_window = std::iter::once(&snapshot.primary)
        .chain(snapshot.secondary.iter())
        .chain(snapshot.model_specific.iter())
        .chain(snapshot.tertiary.iter())
        .any(|window| !window.is_informational);
    if policy.uses_extra_windows {
        windows.extend(
            snapshot
                .extra_rate_windows
                .iter()
                // Fallback lanes (e.g. a seat-credit allowance) only fill in
                // when the provider reports no real core quota window.
                .filter(|extra| !extra.fallback_lane || !has_core_window)
                .map(|extra| &extra.window),
        );
    }
    let windows = windows
        .into_iter()
        .filter(|window| !window.is_informational);
    let selected = if policy.prefers_available_window {
        highest_available_window(windows)
    } else if policy.prioritizes_exhausted_window {
        highest_automatic_window(windows)
    } else {
        highest_window(windows)
    };

    selected.cloned()
}

#[derive(Clone, Copy)]
struct AutomaticMetricPolicy {
    prefers_available_window: bool,
    prioritizes_exhausted_window: bool,
    uses_extra_windows: bool,
    /// Whether a snapshot with an informational primary and no secondary lane
    /// is a dead end for Automatic selection. False for providers whose
    /// fallback lanes (seat credits) should still be considered.
    missing_core_is_terminal: bool,
}

fn automatic_metric_policy(provider: Option<ProviderId>) -> AutomaticMetricPolicy {
    let prioritizes = |id: ProviderId| {
        codexbar::core::instantiate_provider(id).automatic_metric_prioritizes_exhausted_window()
    };
    let missing_core_is_terminal = |id: ProviderId| {
        codexbar::core::instantiate_provider(id).automatic_metric_missing_core_is_terminal()
    };
    match provider {
        Some(ProviderId::Antigravity) => AutomaticMetricPolicy {
            prefers_available_window: true,
            prioritizes_exhausted_window: false,
            uses_extra_windows: false,
            missing_core_is_terminal: true,
        },
        // Cursor's monthly Auto lane is the semantic weekly pace. Grok Bot is
        // a named extra allowance and must stay available through the explicit
        // ExtraUsage preference without changing the automatic bar.
        Some(ProviderId::Cursor) => AutomaticMetricPolicy {
            prefers_available_window: false,
            prioritizes_exhausted_window: prioritizes(ProviderId::Cursor),
            uses_extra_windows: false,
            missing_core_is_terminal: true,
        },
        Some(id) => AutomaticMetricPolicy {
            prefers_available_window: false,
            prioritizes_exhausted_window: prioritizes(id),
            uses_extra_windows: true,
            missing_core_is_terminal: missing_core_is_terminal(id),
        },
        None => AutomaticMetricPolicy {
            prefers_available_window: false,
            prioritizes_exhausted_window: true,
            uses_extra_windows: true,
            missing_core_is_terminal: false,
        },
    }
}

fn average_window(snapshot: &ProviderUsageSnapshot) -> Option<RateWindowSnapshot> {
    if snapshot.primary.is_informational {
        return snapshot.secondary.clone();
    }
    let secondary = snapshot.secondary.as_ref()?;
    Some(derived_window(
        (snapshot.primary.used_percent + secondary.used_percent) / 2.0,
        None,
    ))
}

fn cost_window(snapshot: &ProviderUsageSnapshot) -> Option<RateWindowSnapshot> {
    let cost = snapshot.cost.as_ref()?;
    let limit = cost.limit?;
    if limit <= 0.0 {
        return None;
    }
    Some(derived_window(
        (cost.used / limit) * 100.0,
        cost.resets_at.clone(),
    ))
}

fn extra_usage_window(snapshot: &ProviderUsageSnapshot) -> Option<RateWindowSnapshot> {
    highest_window(
        snapshot
            .extra_rate_windows
            .iter()
            .map(|extra| &extra.window),
    )
    .cloned()
}

fn derived_window(used_percent: f64, resets_at: Option<String>) -> RateWindowSnapshot {
    let used_percent = used_percent.clamp(0.0, 100.0);
    RateWindowSnapshot {
        used_percent,
        remaining_percent: 100.0 - used_percent,
        window_minutes: None,
        resets_at,
        reset_description: None,
        is_exhausted: used_percent >= 100.0,
        is_informational: false,
        reserve_percent: None,
        reserve_description: None,
        reserve_will_last_to_reset: false,
        reserve_eta_seconds: None,
    }
}

fn non_informational(window: Option<&RateWindowSnapshot>) -> Option<&RateWindowSnapshot> {
    window.filter(|window| !window.is_informational)
}

fn highest_window<'a>(
    windows: impl Iterator<Item = &'a RateWindowSnapshot>,
) -> Option<&'a RateWindowSnapshot> {
    windows.max_by(|a, b| {
        a.used_percent
            .partial_cmp(&b.used_percent)
            .unwrap_or(Ordering::Equal)
    })
}

fn highest_available_window<'a>(
    windows: impl Iterator<Item = &'a RateWindowSnapshot>,
) -> Option<&'a RateWindowSnapshot> {
    let windows = windows.collect::<Vec<_>>();
    highest_window(
        windows
            .iter()
            .copied()
            .filter(|window| !automatic_window_is_exhausted(window)),
    )
    .or_else(|| highest_window(windows.into_iter()))
}

fn highest_automatic_window<'a>(
    windows: impl Iterator<Item = &'a RateWindowSnapshot>,
) -> Option<&'a RateWindowSnapshot> {
    windows.max_by(|a, b| {
        automatic_window_is_exhausted(a)
            .cmp(&automatic_window_is_exhausted(b))
            .then_with(|| {
                a.used_percent
                    .partial_cmp(&b.used_percent)
                    .unwrap_or(Ordering::Equal)
            })
    })
}

fn automatic_window_is_exhausted(window: &RateWindowSnapshot) -> bool {
    window.is_exhausted || window.used_percent >= 100.0
}

#[cfg(test)]
mod tests {
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
            primary_label: None,
            secondary: Some(window(60.0)),
            secondary_label: None,
            model_specific: None,
            tertiary: None,
            tertiary_label: None,
            extra_rate_windows: Vec::new(),
            inventory: Vec::new(),
            cost: None,
            plan_name: None,
            account_email: None,
            subscription: None,
            source_label: "test".to_string(),
            has_successful_claude_cli_quota: false,
            updated_at: "2026-08-16T00:00:00Z".to_string(),
            error: None,
            error_state: codexbar::core::ProviderStateKind::Ready,
            pace: None,
            account_organization: None,
            tray_status_label: None,
            fetch_duration_ms: None,
            wayfinder_usage: None,
            session_equivalent_forecast: None,
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

    #[test]
    fn presentation_payload_flattens_the_snapshot_and_selected_metric() {
        let presentation = crate::commands::ProviderUsagePresentationSnapshot::new(
            snapshot(),
            &Settings::default(),
        );
        let value = serde_json::to_value(presentation).expect("serialize presentation");

        assert_eq!(value["providerId"], "codex");
        assert_eq!(value["selectedMetric"]["usedPercent"], 60.0);
        assert!(value.get("snapshot").is_none());
    }
}
