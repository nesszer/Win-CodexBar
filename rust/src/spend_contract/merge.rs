//! Activity histograms and the helpers that merge native and imported spend.

use std::collections::BTreeMap;

use chrono::{DateTime, Datelike, Local, Timelike, Utc};

use super::{CostCoverageCounts, SpendActivityCell, SpendDailyPoint, SpendModelRow, SpendTokenMix};
use crate::codex_workspaces::SessionUsage;

/// Conversation counts per local (weekday, hour) cell.
#[derive(Default)]
pub(super) struct ActivityHistogram(pub(super) BTreeMap<(u8, u8), u32>);

impl ActivityHistogram {
    pub(super) fn add_local(&mut self, timestamp: DateTime<Utc>) {
        let local = timestamp.with_timezone(&Local);
        #[allow(
            clippy::cast_possible_truncation,
            reason = "weekday (0-6) and hour (0-23) fit u8"
        )]
        let key = (
            local.weekday().num_days_from_monday() as u8,
            local.hour() as u8,
        );
        self.add(key, 1);
    }

    pub(super) fn add(&mut self, key: (u8, u8), conversations: u32) {
        let count = self.0.entry(key).or_insert(0);
        *count = count.saturating_add(conversations);
    }

    pub(super) fn into_cells(self) -> Vec<SpendActivityCell> {
        self.0
            .into_iter()
            .map(|((weekday, hour), conversations)| SpendActivityCell {
                weekday,
                hour,
                conversations,
            })
            .collect()
    }
}

pub(super) fn activity_from_sessions(sessions: &[SessionUsage]) -> Vec<SpendActivityCell> {
    let mut activity = ActivityHistogram::default();
    for session in sessions {
        if let Some(timestamp) = session.latest_activity.or(session.started_at) {
            activity.add_local(timestamp);
        }
    }
    activity.into_cells()
}
pub(super) fn sum_optional_cost(left: Option<f64>, right: Option<f64>) -> Option<f64> {
    let valid = |value: f64| value.is_finite() && value >= 0.0;
    match (left, right) {
        (Some(left), Some(right)) if valid(left) && valid(right) => {
            let total = left + right;
            valid(total).then_some(total)
        }
        (Some(value), None) | (None, Some(value)) if valid(value) => Some(value),
        (None, None) => None,
        _ => None,
    }
}

pub(super) fn merge_coverage(
    left: CostCoverageCounts,
    right: &CostCoverageCounts,
) -> (CostCoverageCounts, bool) {
    let priced = left.priced.checked_add(right.priced);
    let unpriced = left.unpriced.checked_add(right.unpriced);
    let unmetered = left.unmetered.checked_add(right.unmetered);
    let estimated = left.estimated.checked_add(right.estimated);
    let exact =
        priced.is_some() && unpriced.is_some() && unmetered.is_some() && estimated.is_some();
    let merged = CostCoverageCounts {
        priced: priced.unwrap_or(u32::MAX),
        unpriced: unpriced.unwrap_or(u32::MAX),
        unmetered: unmetered.unwrap_or(u32::MAX),
        estimated: estimated.unwrap_or(u32::MAX),
    };
    let exact = exact && merged.checked_total().is_some();
    (merged, exact)
}

pub(super) fn merge_token_mix(mut left: SpendTokenMix, right: &SpendTokenMix) -> SpendTokenMix {
    let mut overflowed_classes = left.overflowed_classes | right.overflowed_classes;
    for (bit, merged, incoming) in [
        (0, &mut left.input_tokens, right.input_tokens),
        (1, &mut left.output_tokens, right.output_tokens),
        (2, &mut left.cache_read_tokens, right.cache_read_tokens),
        (
            3,
            &mut left.cache_creation_tokens,
            right.cache_creation_tokens,
        ),
        (4, &mut left.reasoning_tokens, right.reasoning_tokens),
    ] {
        *merged = merge_token_class(*merged, incoming, &mut overflowed_classes, 1 << bit);
    }
    left.overflowed_classes = overflowed_classes;
    left
}

pub(super) fn merge_token_class(
    left: Option<u64>,
    right: Option<u64>,
    overflowed_classes: &mut u8,
    bit: u8,
) -> Option<u64> {
    if *overflowed_classes & bit != 0 {
        return None;
    }
    match (left, right) {
        (Some(left), Some(right)) => match left.checked_add(right) {
            Some(total) => Some(total),
            None => {
                *overflowed_classes |= bit;
                None
            }
        },
        (Some(left), None) => Some(left),
        (None, Some(right)) => Some(right),
        (None, None) => None,
    }
}

pub(super) fn add_optional(left: Option<u64>, right: Option<u64>) -> Option<u64> {
    match (left, right) {
        (Some(left), Some(right)) => left.checked_add(right),
        (Some(left), None) => Some(left),
        (None, Some(right)) => Some(right),
        (None, None) => None,
    }
}

pub(super) fn merge_models(
    mut left: Vec<SpendModelRow>,
    right: &[SpendModelRow],
) -> Vec<SpendModelRow> {
    for incoming in right {
        if let Some(existing) = left.iter_mut().find(|row| row.model == incoming.model) {
            existing.cost_usd = sum_optional_cost(existing.cost_usd, incoming.cost_usd);
            existing.input_tokens = existing.input_tokens.saturating_add(incoming.input_tokens);
            existing.output_tokens = existing
                .output_tokens
                .saturating_add(incoming.output_tokens);
            existing.cache_read_tokens = existing
                .cache_read_tokens
                .saturating_add(incoming.cache_read_tokens);
            existing.total_tokens = existing.total_tokens.saturating_add(incoming.total_tokens);
            existing.custom_pricing |= incoming.custom_pricing;
        } else {
            left.push(incoming.clone());
        }
    }
    left.sort_by(|a, b| {
        b.cost_usd
            .partial_cmp(&a.cost_usd)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.model.cmp(&b.model))
    });
    left
}

pub(super) fn merge_daily(
    left: Vec<SpendDailyPoint>,
    right: &[SpendDailyPoint],
) -> Vec<SpendDailyPoint> {
    let mut days: BTreeMap<String, SpendDailyPoint> = left
        .into_iter()
        .map(|point| (point.day.clone(), point))
        .collect();
    for incoming in right {
        let entry = days
            .entry(incoming.day.clone())
            .or_insert_with(|| SpendDailyPoint {
                day: incoming.day.clone(),
                cost_usd: None,
                total_tokens: None,
            });
        entry.cost_usd = sum_optional_cost(entry.cost_usd, incoming.cost_usd);
        entry.total_tokens = add_optional(entry.total_tokens, incoming.total_tokens);
    }
    days.into_values().collect()
}

pub(super) fn merge_activity(
    left: Vec<SpendActivityCell>,
    right: &[SpendActivityCell],
) -> Vec<SpendActivityCell> {
    // Collected, not added: a duplicate cell on the left keeps the last count.
    let mut activity = ActivityHistogram(
        left.into_iter()
            .map(|cell| ((cell.weekday, cell.hour), cell.conversations))
            .collect(),
    );
    for cell in right {
        activity.add((cell.weekday, cell.hour), cell.conversations);
    }
    activity.into_cells()
}
