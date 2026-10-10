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
        resets_at,
        is_exhausted: used_percent >= 100.0,
        ..Default::default()
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
mod tests;
