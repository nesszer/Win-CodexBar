//! Canonical single-metric selection shared by native and webview surfaces.

use std::borrow::Cow;
use std::cmp::Ordering;

use codexbar::core::{IconLane, ProviderId};
use codexbar::settings::{MetricPreference, Settings};

use crate::commands::{ProviderUsageSnapshot, RateWindowSnapshot};

pub(crate) fn selected_usage_window(
    snapshot: &ProviderUsageSnapshot,
    settings: &Settings,
) -> RateWindowSnapshot {
    select_window(&with_icon_fallbacks(snapshot), settings)
}

fn select_window(snapshot: &ProviderUsageSnapshot, settings: &Settings) -> RateWindowSnapshot {
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

/// Let provider-declared extra windows stand in for an absent core lane.
///
/// A lane is absent when the primary is informational or the secondary is
/// missing; the provider's own `icon_fallback` hint names the extra window
/// that fills it. Only this selection view changes: the snapshot the UI
/// renders keeps its lanes as reported, and a lane with no hint stays absent.
fn with_icon_fallbacks(snapshot: &ProviderUsageSnapshot) -> Cow<'_, ProviderUsageSnapshot> {
    let primary = snapshot
        .primary
        .is_informational
        .then(|| icon_fallback_window(snapshot, IconLane::Primary))
        .flatten();
    let secondary = snapshot
        .secondary
        .is_none()
        .then(|| icon_fallback_window(snapshot, IconLane::Secondary))
        .flatten();
    if primary.is_none() && secondary.is_none() {
        return Cow::Borrowed(snapshot);
    }
    let mut resolved = snapshot.clone();
    if let Some(primary) = primary {
        resolved.primary = primary.clone();
    }
    if let Some(secondary) = secondary {
        resolved.secondary = Some(secondary.clone());
    }
    Cow::Owned(resolved)
}

/// Provider-declared extra window that stands in for an absent core lane.
pub(crate) fn icon_fallback_window(
    snapshot: &ProviderUsageSnapshot,
    lane: IconLane,
) -> Option<&RateWindowSnapshot> {
    snapshot
        .extra_rate_windows
        .iter()
        .find(|extra| extra.icon_fallback == Some(lane) && !extra.window.is_informational)
        .map(|extra| &extra.window)
}

/// Whether the provider maps extra windows onto the tray icon lanes. Such a
/// provider's icon is its two lanes; its other extra windows (team, monthly)
/// never take part in Automatic selection.
fn declares_icon_lanes(snapshot: &ProviderUsageSnapshot) -> bool {
    snapshot
        .extra_rate_windows
        .iter()
        .any(|extra| extra.icon_fallback.is_some())
}

/// Select the primary tray metric and, when there are multiple meaningful core
/// quotas, one distinct companion lane. Keeping this policy beside canonical
/// metric selection prevents tray rendering from duplicating the selected lane.
pub(crate) fn selected_usage_icon_windows(
    snapshot: &ProviderUsageSnapshot,
    settings: &Settings,
) -> (RateWindowSnapshot, Option<RateWindowSnapshot>) {
    let snapshot = with_icon_fallbacks(snapshot);
    if declares_icon_lanes(&snapshot) && icon_metric_is_automatic(&snapshot, settings) {
        return (
            snapshot.primary.clone(),
            snapshot
                .secondary
                .clone()
                .filter(|window| !window.is_informational),
        );
    }
    let selected = select_window(&snapshot, settings);
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

fn icon_metric_is_automatic(snapshot: &ProviderUsageSnapshot, settings: &Settings) -> bool {
    ProviderId::from_cli_name(&snapshot.provider_id)
        .map(|id| settings.get_provider_metric(id))
        .unwrap_or_default()
        == MetricPreference::Automatic
}

fn same_window(left: &RateWindowSnapshot, right: &RateWindowSnapshot) -> bool {
    left.used_percent.to_bits() == right.used_percent.to_bits()
        && left.window_minutes == right.window_minutes
        && left.resets_at == right.resets_at
        && left.reset_description == right.reset_description
        && left.description_is_detail == right.description_is_detail
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
        // Upstream 0.70.0 (#4072): the provider's plan allowance. A missing
        // or unknown plan falls back to the primary (included API) allowance,
        // never to a spend window. Providers without a plan window fall
        // through to Automatic.
        MetricPreference::MonthlyPlan => {
            let window_id = provider.and_then(monthly_plan_window_id)?;
            plan_window(snapshot, window_id)
                .or_else(|| non_informational(Some(&snapshot.primary)))
                .cloned()
        }
    }
}

/// The provider-declared monthly plan allowance, when the snapshot carries a
/// known value for it. Providers without a plan window return `None`.
#[cfg(test)]
pub(crate) fn monthly_plan_window(
    snapshot: &ProviderUsageSnapshot,
    provider: Option<ProviderId>,
) -> Option<&RateWindowSnapshot> {
    plan_window(snapshot, provider.and_then(monthly_plan_window_id)?)
}

fn monthly_plan_window_id(provider: ProviderId) -> Option<&'static str> {
    codexbar::core::instantiate_provider(provider).monthly_plan_window_id()
}

fn plan_window<'a>(
    snapshot: &'a ProviderUsageSnapshot,
    window_id: &str,
) -> Option<&'a RateWindowSnapshot> {
    snapshot
        .extra_rate_windows
        .iter()
        .find(|extra| extra.id == window_id)
        .map(|extra| &extra.window)
        .filter(|window| !window.is_informational)
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

    if policy.prefers_secondary_window {
        let primary = non_informational(Some(&snapshot.primary));
        let secondary = non_informational(snapshot.secondary.as_ref());
        let preferred = primary
            .into_iter()
            .chain(secondary)
            .find(|window| automatic_window_is_exhausted(window))
            .or(secondary)
            .or(primary);
        if let Some(window) = preferred {
            return Some(window.clone());
        }
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
    if policy.uses_extra_windows && !declares_icon_lanes(snapshot) {
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
    /// Whether the secondary lane represents the provider unless a core lane
    /// is exhausted (upstream's LiteLLM team-budget resolver).
    prefers_secondary_window: bool,
}

fn automatic_metric_policy(provider: Option<ProviderId>) -> AutomaticMetricPolicy {
    let prioritizes = |id: ProviderId| {
        codexbar::core::instantiate_provider(id).automatic_metric_prioritizes_exhausted_window()
    };
    let missing_core_is_terminal = |id: ProviderId| {
        codexbar::core::instantiate_provider(id).automatic_metric_missing_core_is_terminal()
    };
    let prefers_secondary = |id: ProviderId| {
        codexbar::core::instantiate_provider(id).automatic_metric_prefers_secondary_window()
    };
    match provider {
        Some(ProviderId::Antigravity) => AutomaticMetricPolicy {
            prefers_available_window: true,
            prioritizes_exhausted_window: false,
            uses_extra_windows: false,
            missing_core_is_terminal: true,
            prefers_secondary_window: false,
        },
        // Cursor's monthly Auto lane is the semantic weekly pace. Grok Bot is
        // a named extra allowance and must stay available through the explicit
        // ExtraUsage preference without changing the automatic bar.
        Some(ProviderId::Cursor) => AutomaticMetricPolicy {
            prefers_available_window: false,
            prioritizes_exhausted_window: prioritizes(ProviderId::Cursor),
            uses_extra_windows: false,
            missing_core_is_terminal: true,
            prefers_secondary_window: false,
        },
        Some(id) => AutomaticMetricPolicy {
            prefers_available_window: false,
            prioritizes_exhausted_window: prioritizes(id),
            uses_extra_windows: true,
            missing_core_is_terminal: missing_core_is_terminal(id),
            prefers_secondary_window: prefers_secondary(id),
        },
        None => AutomaticMetricPolicy {
            prefers_available_window: false,
            prioritizes_exhausted_window: true,
            uses_extra_windows: true,
            missing_core_is_terminal: false,
            prefers_secondary_window: false,
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
        pace: None,
        monthly_limit_block: None,
        description_is_detail: false,
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
            display_details: Vec::new(),
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
            quota_burndown: None,
            open_ai_api_usage: None,
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
            settings.set_provider_metric(ProviderId::Kimi, preference);
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
        let presentation = crate::commands::ProviderUsagePresentationSnapshot::new(
            snapshot(),
            &Settings::default(),
        );
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

        let baseline = crate::commands::ProviderUsagePresentationSnapshot::new(
            raw.clone(),
            &Settings::default(),
        );
        assert_eq!(baseline.selected_metric.used_percent, 95.0);

        let mut settings = Settings::default();
        settings.set_hidden_usage_item_ids(
            ProviderId::Claude,
            vec![codexbar::settings::CLAUDE_DAILY_ROUTINES_USAGE_ITEM_ID.to_string()],
        );
        let hidden =
            crate::commands::ProviderUsagePresentationSnapshot::new(raw.clone(), &settings);

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

        for (has_session, has_weekly) in
            [(false, false), (false, true), (true, false), (true, true)]
        {
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
        let presentation = crate::commands::ProviderUsagePresentationSnapshot::new(
            raw.clone(),
            &Settings::default(),
        );

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
                .map(|(id, percent, minutes)| {
                    agent_extra(id, agent_window(*percent, *minutes), None)
                })
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
}
