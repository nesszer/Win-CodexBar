//! Pure tray presentation policy shared by the native tray surfaces.

use crate::commands::{ProviderUsageSnapshot, RateWindowSnapshot};
use chrono::{DateTime, Utc};
use codexbar::core::{IconLane, ProviderId, UsagePace};
use codexbar::settings::{Language, MetricPreference, Settings, TrayIconMode};
use codexbar::tray::{
    TrayPaceColor, render_bar_icon_rgba_tinted, render_percent_icon_rgba_tinted,
    render_stacked_bar_icon_rgba_tinted, render_stacked_bars_icon_rgba_tinted,
};

/// Opt-in provider accent for a tray meter (`menu_bar_color_by_provider`).
type ProviderRgb = (u8, u8, u8);

#[derive(Debug, Clone, Copy, PartialEq)]
enum TrayIconPlan {
    Bars {
        primary_percent: f64,
        secondary_percent: Option<f64>,
        has_error: bool,
        provider_tint: Option<ProviderRgb>,
    },
    /// The same session and weekly windows as `Bars`, drawn as equal rows.
    StackedBars {
        primary_percent: f64,
        secondary_percent: Option<f64>,
        has_error: bool,
        provider_tint: Option<ProviderRgb>,
    },
    Percent {
        percent: f64,
        has_error: bool,
        provider_tint: Option<ProviderRgb>,
    },
    Stacked {
        top_percent: f64,
        bottom_percent: f64,
        has_error: bool,
        top_tint: Option<ProviderRgb>,
        bottom_tint: Option<ProviderRgb>,
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
    /// Optional pace tint for the single-provider bar and percent icons
    /// (upstream 0.59.0 pace colors). Stacked icons keep usage-level colours.
    icon_pace: Option<TrayPaceColor>,
    status_rows: Vec<TrayStatusRow<'a>>,
    rates_cache: Option<&'a crate::commands::CurrencyRateCache>,
}

impl<'a> TrayPresentationPlan<'a> {
    pub(crate) fn resolve(
        settings: &'a Settings,
        snapshots: &'a [ProviderUsageSnapshot],
        rates_cache: Option<&'a crate::commands::CurrencyRateCache>,
    ) -> Self {
        Self::resolve_at(settings, snapshots, rates_cache, Utc::now())
    }

    /// [`Self::resolve`] with the pace tint scored at `now`.
    pub(crate) fn resolve_at(
        settings: &'a Settings,
        snapshots: &'a [ProviderUsageSnapshot],
        rates_cache: Option<&'a crate::commands::CurrencyRateCache>,
        now: DateTime<Utc>,
    ) -> Self {
        let ordered = ordered_snapshot_refs(settings, snapshots);
        let healthy = ordered
            .into_iter()
            .filter(|snapshot| snapshot.error.is_none())
            .collect::<Vec<_>>();
        let has_error = healthy.is_empty() && !snapshots.is_empty();
        // The retired Display mode "minimal" is folded into this flag once on load.
        let prefer_highest = settings.menu_bar_shows_highest_usage;
        let selected = pick_tray_provider(&healthy, prefer_highest);
        let icon_pace = tray_pace_color(settings, selected, now);
        let single_tint = provider_tint(settings, selected);

        let (icon, status_rows) = match settings.tray_icon_mode {
            TrayIconMode::Stacked => {
                if let Some((top, bottom)) = pick_stacked_tray_providers(&healthy, settings) {
                    (
                        TrayIconPlan::Stacked {
                            top_percent: selected_tray_percents(top, settings).0,
                            bottom_percent: selected_tray_percents(bottom, settings).0,
                            has_error,
                            top_tint: provider_tint(settings, Some(top)),
                            bottom_tint: provider_tint(settings, Some(bottom)),
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
                            settings,
                            percents.0,
                            percents.1,
                            has_error,
                            single_tint,
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
                    resolve_single_provider_icon_plan(
                        settings,
                        percents.0,
                        percents.1,
                        has_error,
                        single_tint,
                    ),
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
                    resolve_single_provider_icon_plan(
                        settings,
                        percents.0,
                        percents.1,
                        has_error,
                        single_tint,
                    ),
                    rows,
                )
            }
        };

        let icon_pace = match icon {
            TrayIconPlan::Stacked { .. } => None,
            TrayIconPlan::Bars { .. }
            | TrayIconPlan::StackedBars { .. }
            | TrayIconPlan::Percent { .. } => icon_pace,
        };

        Self {
            settings,
            icon,
            icon_pace,
            status_rows,
            rates_cache,
        }
    }

    pub(crate) fn render_icon(&self) -> (Vec<u8>, u32, u32) {
        match self.icon {
            TrayIconPlan::Bars {
                primary_percent,
                secondary_percent,
                has_error,
                provider_tint,
            } => render_bar_icon_rgba_tinted(
                primary_percent,
                secondary_percent,
                has_error,
                self.icon_pace,
                provider_tint,
            ),
            TrayIconPlan::Percent {
                percent,
                has_error,
                provider_tint,
            } => render_percent_icon_rgba_tinted(percent, has_error, self.icon_pace, provider_tint),
            TrayIconPlan::StackedBars {
                primary_percent,
                secondary_percent,
                has_error,
                provider_tint,
            } => render_stacked_bars_icon_rgba_tinted(
                primary_percent,
                secondary_percent,
                has_error,
                self.icon_pace,
                provider_tint,
            ),
            TrayIconPlan::Stacked {
                top_percent,
                bottom_percent,
                has_error,
                top_tint,
                bottom_tint,
            } => render_stacked_bar_icon_rgba_tinted(
                top_percent,
                bottom_percent,
                has_error,
                top_tint,
                bottom_tint,
            ),
        }
    }

    pub(crate) fn status_labels(&self, language: Language) -> Vec<(String, String)> {
        self.status_rows
            .iter()
            .map(|row| {
                let (_, label) =
                    provider_status_label(row.snapshot, self.settings, language, self.rates_cache);
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
    provider_tint: Option<ProviderRgb>,
) -> TrayIconPlan {
    use codexbar::settings::MenuBarIconStyle;
    match settings.menu_bar_icon_style {
        MenuBarIconStyle::Percent => TrayIconPlan::Percent {
            percent: primary_percent,
            has_error,
            provider_tint,
        },
        MenuBarIconStyle::StackedBars => TrayIconPlan::StackedBars {
            primary_percent,
            secondary_percent,
            has_error,
            provider_tint,
        },
        MenuBarIconStyle::Bars => TrayIconPlan::Bars {
            primary_percent,
            secondary_percent,
            has_error,
            provider_tint,
        },
    }
}

/// Accent tint for the meter of `snapshot` (upstream "Color by provider").
///
/// Opt-in through `menu_bar_color_by_provider`. Uses the effective accent, so
/// a per-provider override applies too. A malformed override yields no tint.
/// Legibility against the tray canvas is checked by the renderer.
fn provider_tint(
    settings: &Settings,
    snapshot: Option<&ProviderUsageSnapshot>,
) -> Option<ProviderRgb> {
    if !settings.menu_bar_color_by_provider {
        return None;
    }
    let id = ProviderId::from_cli_name(&snapshot?.provider_id)?;
    parse_hex_rgb(&settings.effective_accent_color(id))
}

/// Parse `#RRGGBB` (the leading `#` is optional and case is ignored).
fn parse_hex_rgb(value: &str) -> Option<ProviderRgb> {
    let hex = value.trim().strip_prefix('#').unwrap_or(value.trim());
    if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let channel = |range: std::ops::Range<usize>| u8::from_str_radix(&hex[range], 16).ok();
    Some((channel(0..2)?, channel(2..4)?, channel(4..6)?))
}

/// Pace tint for the provider that drives a single-provider tray icon.
///
/// Opt-in through `menu_bar_color_pace`; on-track and unknown stages keep the
/// normal usage-level colour. The pace comes from the menu bar pace window
/// (upstream `menuBarPaceWindow`, StatusItemController+Animation.swift
/// :1207-1225): the lane the provider declares, or else the window the icon
/// draws for the selected metric.
fn tray_pace_color(
    settings: &Settings,
    selected: Option<&ProviderUsageSnapshot>,
    now: DateTime<Utc>,
) -> Option<TrayPaceColor> {
    if !settings.menu_bar_color_pace {
        return None;
    }
    let snapshot = selected?;
    let window = menu_bar_pace_window(snapshot, settings)?;
    let pace = menu_bar_pace(snapshot, &window, settings.weekly_progress_work_days, now)?;
    TrayPaceColor::from_stage(crate::commands::pace::stage_str(pace.stage))
}

/// Window the menu bar pace is scored on. A provider-declared lane wins over
/// the selected metric; a missing declared lane gives no pace.
fn menu_bar_pace_window(
    snapshot: &ProviderUsageSnapshot,
    settings: &Settings,
) -> Option<RateWindowSnapshot> {
    let declared = ProviderId::from_cli_name(&snapshot.provider_id)
        .and_then(|id| codexbar::core::instantiate_provider(id).menu_bar_pace_lane());
    match declared {
        Some(IconLane::Primary) => Some(snapshot.primary.clone()),
        Some(IconLane::Secondary) => snapshot.secondary.clone(),
        None => Some(crate::usage_metric::selected_usage_icon_windows(snapshot, settings).0),
    }
}

/// Generic branch of upstream `UsageStore.weeklyPace`
/// (UsageStore+HistoricalPace.swift:8-64): the fetch must allow pace and the
/// window must have quota left and its own length (no 7-day fallback). A
/// monthly sentinel resolves to the real calendar cycle first
/// (`paceWindowForElapsedEligibility`, :118-124), and a pace expecting less
/// than 3% use is dropped.
fn menu_bar_pace(
    snapshot: &ProviderUsageSnapshot,
    window: &RateWindowSnapshot,
    work_days: Option<u8>,
    now: DateTime<Utc>,
) -> Option<UsagePace> {
    const MINIMUM_EXPECTED_PERCENT: f64 = 3.0;
    if !snapshot.pace_authoritative || window.is_informational {
        return None;
    }
    window.window_minutes?;
    let rate_window = window.to_rate_window();
    if rate_window.remaining_percent() <= 0.0 {
        return None;
    }
    let pace_window = match ProviderId::from_cli_name(&snapshot.provider_id) {
        Some(id) => id
            .pace_capability()
            .resolved_reset_window_for_pace(&rate_window),
        None => rate_window,
    };
    let pace = UsagePace::weekly(
        &pace_window,
        Some(now),
        crate::commands::pace::WEEKLY_FALLBACK_MINUTES,
        work_days,
    )?;
    (pace.expected_used_percent >= MINIMUM_EXPECTED_PERCENT).then_some(pace)
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
    rates_cache: Option<&crate::commands::CurrencyRateCache>,
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
            crate::commands::format_cost_amount(cost, rates_cache)
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
    } else if snapshot.primary.is_informational {
        // Provider-declared icon lanes (Doubao Agent Plan) stand in for an
        // absent core session, matching the tray icon.
        crate::usage_metric::icon_fallback_window(snapshot, codexbar::core::IconLane::Primary)
            .unwrap_or(&snapshot.primary)
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

pub(crate) fn pick_tray_provider<'a>(
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

pub(crate) fn selected_tray_percents(
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
