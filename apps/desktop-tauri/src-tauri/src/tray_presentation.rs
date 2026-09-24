//! Pure tray presentation policy shared by the native tray surfaces.

use crate::commands::{ProviderUsageSnapshot, RateWindowSnapshot};
use codexbar::settings::{Language, MetricPreference, Settings, TrayIconMode};
use codexbar::tray::{
    render_bar_icon_rgba, render_percent_icon_rgba, render_stacked_bar_icon_rgba,
};

#[derive(Debug, Clone, Copy, PartialEq)]
enum TrayIconPlan {
    Bars {
        primary_percent: f64,
        secondary_percent: Option<f64>,
        has_error: bool,
    },
    Percent {
        percent: f64,
        has_error: bool,
    },
    Stacked {
        top_percent: f64,
        bottom_percent: f64,
        has_error: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TrayStatusKey {
    Summary,
    Provider,
}

#[derive(Debug, Clone, Copy)]
struct TrayStatusRow<'a> {
    key: TrayStatusKey,
    snapshot: &'a ProviderUsageSnapshot,
}

/// Fully resolved tray presentation, independent of Tauri and operating-system state.
///
/// The plan is the single policy boundary for provider ordering, mode-specific
/// selection, metric selection, status rows, and icon renderer choice.
pub(crate) struct TrayPresentationPlan<'a> {
    settings: &'a Settings,
    icon: TrayIconPlan,
    status_rows: Vec<TrayStatusRow<'a>>,
}

impl<'a> TrayPresentationPlan<'a> {
    pub(crate) fn resolve(settings: &'a Settings, snapshots: &'a [ProviderUsageSnapshot]) -> Self {
        let ordered = ordered_snapshot_refs(settings, snapshots);
        let healthy = ordered
            .into_iter()
            .filter(|snapshot| snapshot.error.is_none())
            .collect::<Vec<_>>();
        let has_error = healthy.is_empty() && !snapshots.is_empty();
        let prefer_highest =
            settings.menu_bar_shows_highest_usage || settings.menu_bar_display_mode == "minimal";
        let selected = pick_tray_provider(&healthy, prefer_highest);

        let (icon, status_rows) = match settings.tray_icon_mode {
            TrayIconMode::Stacked => {
                if let Some((top, bottom)) = pick_stacked_tray_providers(&healthy, settings) {
                    (
                        TrayIconPlan::Stacked {
                            top_percent: selected_tray_percents(top, settings).0,
                            bottom_percent: selected_tray_percents(bottom, settings).0,
                            has_error,
                        },
                        vec![
                            TrayStatusRow {
                                key: TrayStatusKey::Provider,
                                snapshot: top,
                            },
                            TrayStatusRow {
                                key: TrayStatusKey::Provider,
                                snapshot: bottom,
                            },
                        ],
                    )
                } else {
                    let percents = selected
                        .map(|snapshot| selected_tray_percents(snapshot, settings))
                        .unwrap_or((0.0, None));
                    let rows = healthy
                        .first()
                        .map(|snapshot| TrayStatusRow {
                            key: TrayStatusKey::Provider,
                            snapshot,
                        })
                        .into_iter()
                        .collect();
                    (
                        resolve_single_provider_icon_plan(
                            settings, percents.0, percents.1, has_error,
                        ),
                        rows,
                    )
                }
            }
            TrayIconMode::PerProvider => {
                let percents = selected
                    .map(|snapshot| selected_tray_percents(snapshot, settings))
                    .unwrap_or_else(|| fallback_percents(&healthy, settings));
                let rows = healthy
                    .iter()
                    .copied()
                    .map(|snapshot| TrayStatusRow {
                        key: TrayStatusKey::Provider,
                        snapshot,
                    })
                    .collect();
                (
                    resolve_single_provider_icon_plan(settings, percents.0, percents.1, has_error),
                    rows,
                )
            }
            TrayIconMode::Single => {
                let percents = selected
                    .map(|snapshot| selected_tray_percents(snapshot, settings))
                    .unwrap_or_else(|| fallback_percents(&healthy, settings));
                let rows = selected
                    .map(|snapshot| TrayStatusRow {
                        key: TrayStatusKey::Summary,
                        snapshot,
                    })
                    .into_iter()
                    .collect();
                (
                    resolve_single_provider_icon_plan(settings, percents.0, percents.1, has_error),
                    rows,
                )
            }
        };

        Self {
            settings,
            icon,
            status_rows,
        }
    }

    pub(crate) fn render_icon(&self) -> (Vec<u8>, u32, u32) {
        match self.icon {
            TrayIconPlan::Bars {
                primary_percent,
                secondary_percent,
                has_error,
            } => render_bar_icon_rgba(primary_percent, secondary_percent, has_error),
            TrayIconPlan::Percent { percent, has_error } => {
                render_percent_icon_rgba(percent, has_error)
            }
            TrayIconPlan::Stacked {
                top_percent,
                bottom_percent,
                has_error,
            } => render_stacked_bar_icon_rgba(top_percent, bottom_percent, has_error),
        }
    }

    pub(crate) fn status_labels(&self, language: Language) -> Vec<(String, String)> {
        self.status_rows
            .iter()
            .map(|row| {
                let (_, label) = provider_status_label(row.snapshot, self.settings, language);
                let key = match row.key {
                    TrayStatusKey::Summary => "status_summary".to_string(),
                    TrayStatusKey::Provider => row.snapshot.provider_id.clone(),
                };
                (key, label)
            })
            .collect()
    }
}

fn resolve_single_provider_icon_plan(
    settings: &Settings,
    primary_percent: f64,
    secondary_percent: Option<f64>,
    has_error: bool,
) -> TrayIconPlan {
    if settings.menu_bar_shows_percent {
        TrayIconPlan::Percent {
            percent: primary_percent,
            has_error,
        }
    } else {
        TrayIconPlan::Bars {
            primary_percent,
            secondary_percent,
            has_error,
        }
    }
}

fn fallback_percents(
    healthy: &[&ProviderUsageSnapshot],
    settings: &Settings,
) -> (f64, Option<f64>) {
    (
        healthy
            .iter()
            .map(|snapshot| selected_tray_percents(snapshot, settings).0)
            .fold(0.0_f64, f64::max),
        None,
    )
}

fn ordered_snapshot_refs<'a>(
    settings: &Settings,
    snapshots: &'a [ProviderUsageSnapshot],
) -> Vec<&'a ProviderUsageSnapshot> {
    let order = settings
        .provider_display_order_names()
        .into_iter()
        .enumerate()
        .map(|(index, provider_id)| (provider_id, index))
        .collect::<std::collections::HashMap<_, _>>();
    let mut ordered = snapshots.iter().collect::<Vec<_>>();
    ordered.sort_by(|a, b| {
        let a_order = order.get(&a.provider_id);
        let b_order = order.get(&b.provider_id);
        match (a_order, b_order) {
            (Some(a_order), Some(b_order)) if a_order != b_order => a_order.cmp(b_order),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            _ => a.display_name.cmp(&b.display_name),
        }
    });
    ordered
}

fn provider_status_label(
    snapshot: &ProviderUsageSnapshot,
    settings: &Settings,
    language: Language,
) -> (String, String) {
    let provider = codexbar::core::ProviderId::from_cli_name(&snapshot.provider_id);
    let preference = provider
        .map(|id| settings.get_provider_metric(id))
        .unwrap_or_default();
    if preference == MetricPreference::MonthlyPlan
        && let Some(cost) = snapshot.cost.as_ref()
    {
        let amount = if !cost.formatted_used.is_empty() {
            cost.formatted_used.clone()
        } else {
            crate::commands::format_cost_amount(cost)
        };
        return (
            snapshot.provider_id.clone(),
            format!("{} {}", snapshot.display_name, amount),
        );
    }

    let label = crate::commands::compact_tray_status_label(headline_window(snapshot), language);
    (
        snapshot.provider_id.clone(),
        format!("{} {}", snapshot.display_name, label),
    )
}

/// Window that headline tray surfaces should label for a provider.
pub(crate) fn headline_window(snapshot: &ProviderUsageSnapshot) -> &RateWindowSnapshot {
    if snapshot.provider_id == "codex" {
        codex_lane_headline_window(snapshot)
    } else {
        &snapshot.primary
    }
}

/// Pick the first non-informational Codex lane in session, weekly, monthly order.
pub(crate) fn codex_lane_headline_window(snapshot: &ProviderUsageSnapshot) -> &RateWindowSnapshot {
    if !snapshot.primary.is_informational {
        return &snapshot.primary;
    }
    if let Some(ref secondary) = snapshot.secondary
        && !secondary.is_informational
    {
        return secondary;
    }
    if let Some(ref tertiary) = snapshot.tertiary
        && !tertiary.is_informational
    {
        return tertiary;
    }
    &snapshot.primary
}

/// Resolve a stable top/bottom pair while retaining stale saved preferences.
fn pick_stacked_tray_providers<'a>(
    healthy: &[&'a ProviderUsageSnapshot],
    settings: &Settings,
) -> Option<(&'a ProviderUsageSnapshot, &'a ProviderUsageSnapshot)> {
    if healthy.len() < 2 {
        return None;
    }

    let preferred = |provider_id: Option<&str>| {
        provider_id.and_then(|id| {
            healthy
                .iter()
                .copied()
                .find(|snapshot| snapshot.provider_id == id)
        })
    };
    let preferred_bottom = preferred(settings.stacked_tray_bottom_provider.as_deref());
    let top = preferred(settings.stacked_tray_top_provider.as_deref()).or_else(|| {
        healthy.iter().copied().find(|snapshot| {
            preferred_bottom.map(|bottom| bottom.provider_id.as_str())
                != Some(snapshot.provider_id.as_str())
        })
    })?;
    let bottom = preferred_bottom
        .filter(|snapshot| snapshot.provider_id != top.provider_id)
        .or_else(|| {
            healthy
                .iter()
                .copied()
                .find(|snapshot| snapshot.provider_id != top.provider_id)
        })?;

    Some((top, bottom))
}

fn pick_tray_provider<'a>(
    healthy: &[&'a ProviderUsageSnapshot],
    prefer_highest: bool,
) -> Option<&'a ProviderUsageSnapshot> {
    if prefer_highest {
        healthy.iter().copied().max_by(|a, b| {
            a.primary
                .used_percent
                .partial_cmp(&b.primary.used_percent)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    } else {
        healthy.first().copied()
    }
}

fn selected_tray_percents(
    snapshot: &ProviderUsageSnapshot,
    settings: &Settings,
) -> (f64, Option<f64>) {
    let (selected, companion) =
        crate::usage_metric::selected_usage_icon_windows(snapshot, settings);
    (
        display_metric_percent(&selected, settings.show_as_used),
        companion
            .as_ref()
            .map(|window| display_metric_percent(window, settings.show_as_used)),
    )
}

fn display_metric_percent(window: &RateWindowSnapshot, show_as_used: bool) -> f64 {
    if window.is_informational {
        return 0.0;
    }
    if window.is_exhausted || window.used_percent >= 100.0 {
        return if show_as_used { 100.0 } else { 0.0 };
    }

    let used = window.used_percent.clamp(0.0, 100.0);
    if show_as_used { used } else { 100.0 - used }
}

#[cfg(test)]
#[path = "tray_presentation_tests.rs"]
mod tests;
