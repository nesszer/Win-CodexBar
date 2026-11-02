//! Codex quota-window history projected from persisted local request evidence.
//!
//! This module is intentionally separate from [`RateWindow`].  A live rate
//! window describes the account now; these rows describe local historical
//! evidence for a future display/transport surface.

use chrono::{DateTime, Duration, Local, NaiveDate, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;

use crate::core::{
    CodexPriorityOverlay, CodexSourceRowCache, CodexSourceUsageRow, CostUsageCache,
    CostUsagePricing, RateWindow, row_priced_model,
};
use crate::cost_reporting_period::cost_bucket_zone;

const NOMINAL_WEEK_MINUTES: i64 = 7 * 24 * 60;
const RESET_TOLERANCE_SECONDS: i64 = 2 * 60;
const MIN_WEEK_MINUTES: u32 = 6 * 24 * 60;
const MAX_WEEK_MINUTES: u32 = 8 * 24 * 60;
const MAX_WINDOW_COUNT: usize = 8;

/// One historical Codex quota window, newest first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexQuotaWindow {
    /// Zero is the current live-aligned window; older windows increase from there.
    pub offset: usize,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    /// Known subtotal. `None` means no valid evidence for this metric.
    pub total_tokens: Option<u64>,
    /// Known priced subtotal. `None` means no priced evidence for this metric.
    pub total_cost_usd: Option<f64>,
    pub entry_count: usize,
    /// Complete within the scanned local source, not account-wide completeness.
    pub tokens_are_complete: bool,
    pub cost_is_complete: bool,
    /// True when an edge was reconstructed from an unconfirmed forecast.
    pub boundaries_are_estimated: bool,
}

#[derive(Debug, Clone, Copy)]
struct Slice {
    start: DateTime<Utc>,
    end: Option<DateTime<Utc>>,
    tokens: Option<u64>,
    cost_usd: Option<f64>,
    tokens_are_complete: bool,
    cost_is_complete: bool,
}

#[derive(Debug, Clone, Copy, Default)]
struct Totals {
    tokens: u64,
    saw_tokens: bool,
    tokens_valid: bool,
    tokens_complete: bool,
    cost_usd: f64,
    saw_cost: bool,
    cost_valid: bool,
    cost_complete: bool,
    entry_count: usize,
}

impl Totals {
    fn new() -> Self {
        Self {
            tokens_valid: true,
            tokens_complete: true,
            cost_valid: true,
            cost_complete: true,
            ..Self::default()
        }
    }

    fn add(&mut self, slice: Slice) {
        self.entry_count = self.entry_count.saturating_add(1);
        self.tokens_complete &= slice.tokens_are_complete;
        self.cost_complete &= slice.cost_is_complete;
        if let Some(tokens) = slice.tokens {
            let (sum, overflowed) = self.tokens.overflowing_add(tokens);
            self.tokens_valid &= !overflowed;
            if !overflowed {
                self.tokens = sum;
                self.saw_tokens = true;
            }
        } else {
            self.tokens_complete = false;
        }
        if let Some(cost) = slice.cost_usd {
            let sum = self.cost_usd + cost;
            self.cost_valid &= cost.is_finite() && cost >= 0.0 && sum.is_finite();
            if self.cost_valid {
                self.cost_usd = sum;
                self.saw_cost = true;
            }
        } else {
            self.cost_complete = false;
        }
    }

    fn finish(self, source_complete: bool) -> (Option<u64>, Option<f64>, bool, bool) {
        let tokens = self
            .tokens_valid
            .then_some(self.tokens)
            .filter(|_| self.saw_tokens);
        let cost = self
            .cost_valid
            .then_some(self.cost_usd)
            .filter(|_| self.saw_cost);
        (
            tokens,
            cost,
            self.tokens_complete && tokens.is_some() && source_complete,
            self.cost_complete && cost.is_some() && source_complete,
        )
    }
}

/// Project persisted Codex local history onto the live weekly quota boundary.
///
/// The function is pure with respect to the live snapshot: it reads the
/// supplied [`RateWindow`] only for its reset metadata and returns display
/// data. It never changes the window, cache, readiness state, or notifications.
pub fn codex_quota_windows_from_cache(
    cache: &CostUsageCache,
    weekly_window: Option<&RateWindow>,
    observed_next_resets: &[DateTime<Utc>],
    now: DateTime<Utc>,
    week_count: usize,
) -> Vec<CodexQuotaWindow> {
    let Some(weekly_window) = weekly_window else {
        return Vec::new();
    };
    let Some(reset_at) = weekly_window.resets_at else {
        return Vec::new();
    };
    let duration = quota_duration(weekly_window.window_minutes);
    let current_end = current_window_end(reset_at, duration, now);
    let count = week_count.clamp(1, MAX_WINDOW_COUNT);
    let boundaries = quota_boundaries(observed_next_resets, current_end, duration, count, now);
    let slices = cache_slices(cache);
    let source_complete = !cache.codex_scan_incomplete
        && cache.codex_pending_paths.is_empty()
        && cache.scan_since_key.is_some()
        && cache.scan_until_key.is_some();
    let history_start = cache
        .scan_since_key
        .as_deref()
        .and_then(bucket_day_start)
        .unwrap_or_else(|| {
            let count = i32::try_from(count).expect("quota window count is capped");
            current_end - duration * (count + 1)
        });

    let mut windows = Vec::with_capacity(count);
    for offset in 0..count {
        let Some(end) = boundaries
            .get(boundaries.len().saturating_sub(1 + offset))
            .copied()
        else {
            break;
        };
        let Some(start) = boundaries
            .get(boundaries.len().saturating_sub(2 + offset))
            .copied()
        else {
            break;
        };
        // Keep the live current window even if the requested local scan starts
        // part-way through it; older windows would be falsely complete.
        if offset > 0 && start < history_start {
            break;
        }
        let mut totals = Totals::new();
        for slice in slices.iter().copied() {
            let overlaps = match slice.end {
                Some(slice_end) => slice.start < end && slice_end > start,
                None => slice.start >= start && slice.start < end,
            };
            if !overlaps {
                continue;
            }
            let contained = match slice.end {
                Some(slice_end) => slice.start >= start && slice_end <= end,
                None => slice.start >= start && slice.start < end,
            };
            if contained {
                totals.add(slice);
            } else {
                // A coarse day interval crossing a reset cannot be assigned to
                // either side. A zero value remains harmless and complete.
                let zero_tokens = slice.tokens == Some(0);
                let zero_cost = slice.cost_usd == Some(0.0);
                totals.entry_count = totals.entry_count.saturating_add(1);
                totals.tokens_complete &= zero_tokens && slice.tokens_are_complete;
                totals.cost_complete &= zero_cost && slice.cost_is_complete;
            }
        }
        let (total_tokens, total_cost_usd, tokens_are_complete, cost_is_complete) =
            totals.finish(source_complete);
        windows.push(CodexQuotaWindow {
            offset,
            start,
            end,
            total_tokens,
            total_cost_usd,
            entry_count: totals.entry_count,
            tokens_are_complete,
            cost_is_complete,
            boundaries_are_estimated: !boundary_is_confirmed(observed_next_resets, start, now)
                || (offset > 0 && !boundary_is_confirmed(observed_next_resets, end, now))
                || reset_at <= now,
        });
    }
    windows
}

fn quota_duration(window_minutes: Option<u32>) -> Duration {
    let minutes = window_minutes
        .filter(|minutes| (MIN_WEEK_MINUTES..=MAX_WEEK_MINUTES).contains(minutes))
        .map(i64::from)
        .unwrap_or(NOMINAL_WEEK_MINUTES);
    Duration::minutes(minutes)
}

fn current_window_end(
    reset_at: DateTime<Utc>,
    duration: Duration,
    now: DateTime<Utc>,
) -> DateTime<Utc> {
    if reset_at > now {
        return reset_at;
    }
    let elapsed = now.signed_duration_since(reset_at);
    let periods = elapsed
        .num_milliseconds()
        .div_euclid(duration.num_milliseconds())
        + 1;
    reset_at + duration * i32::try_from(periods).unwrap_or(i32::MAX)
}

fn quota_boundaries(
    observed_next_resets: &[DateTime<Utc>],
    current_end: DateTime<Utc>,
    duration: Duration,
    count: usize,
    now: DateTime<Utc>,
) -> Vec<DateTime<Utc>> {
    let count_i32 = i32::try_from(count).expect("quota window count is capped");
    let earliest = current_end - duration * (count_i32 + 2);
    let latest = current_end + Duration::seconds(RESET_TOLERANCE_SECONDS);
    let mut dates = vec![current_end, current_end - duration];
    let mut observations: Vec<_> = observed_next_resets
        .iter()
        .copied()
        .filter(|next| *next > DateTime::<Utc>::UNIX_EPOCH)
        .collect();
    observations.sort();
    for next in observations {
        if next < earliest - duration || next > latest + duration {
            continue;
        }
        // A future reset is a forecast, not evidence that its inferred start
        // was a real reset. In an idle window the forecast may move on every
        // refresh; projecting each one backwards creates minute-long rows.
        if next <= now {
            dates.push(next);
        }
    }
    dates.sort();
    dates = unique_dates(dates);
    dates.retain(|date| *date >= earliest && *date <= latest);
    dates.retain(|date| (*date - current_end).abs() > Duration::seconds(RESET_TOLERANCE_SECONDS));
    dates.push(current_end);
    dates.sort();

    // Fill gaps so sparse observations do not collapse several ordinary weeks
    // into one historical row.
    let mut filled = Vec::with_capacity(dates.len());
    for date in dates {
        if let Some(mut cursor) = filled.last().copied() {
            loop {
                let next = cursor + duration;
                // Insert only a missing whole window. Splitting an observed
                // gap slightly longer than a week would invent a short tail.
                if next + duration > date || next <= cursor {
                    break;
                }
                filled.push(next);
                cursor = next;
            }
        }
        filled.push(date);
    }
    filled = unique_dates(filled);
    while filled.len() < count + 1 {
        let Some(oldest) = filled.first().copied() else {
            break;
        };
        filled.insert(0, oldest - duration);
    }
    filled.sort();
    filled
}

fn unique_dates(mut dates: Vec<DateTime<Utc>>) -> Vec<DateTime<Utc>> {
    dates.sort();
    let tolerance = Duration::seconds(RESET_TOLERANCE_SECONDS);
    let mut unique: Vec<DateTime<Utc>> = Vec::with_capacity(dates.len());
    for date in dates {
        if let Some(last) = unique.last_mut()
            && (date - *last).abs() <= tolerance
        {
            // Keep the latest member of a consecutive jitter chain. Comparing
            // against the first retained value split one-minute updates into
            // a new boundary every two minutes.
            *last = date;
        } else {
            unique.push(date);
        }
    }
    unique
}

fn boundary_is_confirmed(
    observed_next_resets: &[DateTime<Utc>],
    instant: DateTime<Utc>,
    now: DateTime<Utc>,
) -> bool {
    observed_next_resets.iter().any(|reset| {
        *reset <= now && (*reset - instant).abs() <= Duration::seconds(RESET_TOLERANCE_SECONDS)
    })
}

fn cache_slices(cache: &CostUsageCache) -> Vec<Slice> {
    if cache.codex_source_rows.is_empty() {
        return legacy_day_slices(cache);
    }
    let mut sources: Vec<(&String, &CodexSourceRowCache)> =
        cache.codex_source_rows.iter().collect();
    sources.sort_by(|left, right| {
        left.1
            .file_identity
            .cmp(&right.1.file_identity)
            .then_with(|| left.0.cmp(right.0))
    });
    let mut identities = HashSet::new();
    let mut slices = Vec::new();
    let cursor = cache.codex_priority_turns_cursor.as_ref();
    for (path, source) in sources {
        let overlay = cursor.and_then(|cursor| cursor.overlay_for_file(Path::new(path)));
        let identity = if source.file_identity.is_empty() {
            path.as_str()
        } else {
            source.file_identity.as_str()
        };
        if !identities.insert(identity.to_string()) {
            continue;
        }
        for row in &source.rows {
            slices.push(slice_from_row(row, overlay.as_ref()));
        }
    }
    slices.sort_by_key(|slice| (slice.start, slice.end));
    slices
}

fn slice_from_row(row: &CodexSourceUsageRow, overlay: Option<&CodexPriorityOverlay<'_>>) -> Slice {
    let timestamp = row.timestamp.or_else(|| local_day_start(&row.day_key));
    let end = row.timestamp.map(|_| None).unwrap_or_else(|| {
        bucket_day_start(&row.day_key).and_then(|start| start.checked_add_signed(Duration::days(1)))
    });
    let input = u64::try_from(row.input.max(0)).unwrap_or(0);
    let output = u64::try_from(row.output.max(0)).unwrap_or(0);
    let tokens = Some(input.saturating_add(output));
    let cost_usd = row_priced_model(row, overlay).and_then(|model| {
        let date = timestamp
            .map(|value| cost_bucket_zone().date(value))
            .or_else(|| timestamp.map(|value| value.with_timezone(&Local).date_naive()))?;
        let cached = u64::try_from(row.cached.max(0)).unwrap_or(0).min(input);
        if model.ends_with("-priority") {
            CostUsagePricing::codex_fast_cost_usd_at_date(&model, input, cached, output, date)
        } else {
            CostUsagePricing::codex_cost_usd_at_date(&model, input, cached, output, date)
        }
    });
    Slice {
        start: timestamp.unwrap_or(DateTime::<Utc>::UNIX_EPOCH),
        end,
        tokens,
        cost_usd,
        tokens_are_complete: timestamp.is_some(),
        cost_is_complete: timestamp.is_some() && cost_usd.is_some(),
    }
}

fn legacy_day_slices(cache: &CostUsageCache) -> Vec<Slice> {
    let mut slices = Vec::new();
    let mut days: Vec<_> = cache.days.iter().collect();
    days.sort_by_key(|(day, _)| *day);
    for (day, models) in days {
        let Some(start) = bucket_day_start(day) else {
            continue;
        };
        let Some(end) = start.checked_add_signed(Duration::days(1)) else {
            continue;
        };
        let mut models: Vec<_> = models.iter().collect();
        models.sort_by_key(|(model, _)| *model);
        for (model, packed) in models {
            let Some(input) = packed.first().copied() else {
                continue;
            };
            let Some(output) = packed.get(2).copied() else {
                continue;
            };
            let input = u64::try_from(input.max(0)).unwrap_or(0);
            let output = u64::try_from(output.max(0)).unwrap_or(0);
            let cached = u64::try_from(packed.get(1).copied().unwrap_or(0).max(0))
                .unwrap_or(0)
                .min(input);
            let cost_usd = CostUsagePricing::codex_day_aggregate_cost_usd(
                model,
                input,
                cached,
                output,
                Some(
                    NaiveDate::parse_from_str(day, "%Y-%m-%d")
                        .ok()
                        .unwrap_or_else(|| cost_bucket_zone().date(start)),
                ),
            );
            slices.push(Slice {
                start,
                end: Some(end),
                tokens: Some(input.saturating_add(output)),
                cost_usd,
                tokens_are_complete: true,
                cost_is_complete: cost_usd.is_some(),
            });
        }
    }
    slices
}

/// The instant a `YYYY-MM-DD` day key begins in the machine's local zone.
fn local_day_start(day: &str) -> Option<DateTime<Utc>> {
    let date = NaiveDate::parse_from_str(day, "%Y-%m-%d").ok()?;
    let naive = date.and_hms_opt(0, 0, 0)?;
    Local
        .from_local_datetime(&naive)
        .single()
        .or_else(|| Local.from_local_datetime(&naive).earliest())
        .or_else(|| Local.from_local_datetime(&naive).latest())
        .map(|value| value.with_timezone(&Utc))
}

/// The instant a `YYYY-MM-DD` day key begins in the pinned bucket zone.
fn bucket_day_start(day: &str) -> Option<DateTime<Utc>> {
    let date = NaiveDate::parse_from_str(day, "%Y-%m-%d").ok()?;
    Some(cost_bucket_zone().start_of_day_utc(date))
}

#[cfg(test)]
mod tests;
