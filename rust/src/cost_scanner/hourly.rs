//! Per-hour cost buckets for the Usage & Spend hourly chart (upstream 0.73.0
//! `CostUsageHourlyEntry`). Hours are local clock hours of the cost bucket
//! zone, keyed by the instant each hour began.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};

use crate::cost_reporting_period::CostTimeZone;

/// Cost recorded in one local clock hour.
#[derive(Debug, Clone, PartialEq)]
pub struct HourlyCostPoint {
    /// The instant the local hour began.
    pub hour_start: DateTime<Utc>,
    /// Minutes east of UTC during that hour.
    pub offset_minutes: i32,
    /// `None` when any record in the hour is unpriced or the sum overflowed.
    pub cost_usd: Option<f64>,
}

#[derive(Debug, Default)]
pub(super) struct HourlyCostAccumulator {
    slots: BTreeMap<DateTime<Utc>, (i32, Option<f64>)>,
}

impl HourlyCostAccumulator {
    pub(super) fn add(&mut self, zone: CostTimeZone, at: DateTime<Utc>, cost: Option<f64>) {
        let (start, offset) = zone.hour_start(at);
        let slot = self.slots.entry(start).or_insert((offset, Some(0.0)));
        slot.1 = match (slot.1, cost) {
            (Some(total), Some(cost)) if cost.is_finite() && cost >= 0.0 => {
                Some(total + cost).filter(|sum| sum.is_finite())
            }
            _ => None,
        };
    }

    pub(super) fn into_points(self) -> Vec<HourlyCostPoint> {
        self.slots
            .into_iter()
            .map(|(hour_start, (offset_minutes, cost_usd))| HourlyCostPoint {
                hour_start,
                offset_minutes,
                cost_usd,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(value: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(value)
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn records_sum_into_their_local_hour_and_unpriced_hours_stay_unknown() {
        let zone = CostTimeZone::Named(chrono_tz::Europe::Berlin);
        let mut hourly = HourlyCostAccumulator::default();
        hourly.add(zone, at("2026-10-25T00:10:00Z"), Some(1.0));
        hourly.add(zone, at("2026-10-25T00:50:00Z"), Some(0.5));
        // The repeated 02:00 local hour after fall-back is its own bucket.
        hourly.add(zone, at("2026-10-25T01:10:00Z"), Some(2.0));
        hourly.add(zone, at("2026-10-25T05:00:00Z"), Some(1.0));
        hourly.add(zone, at("2026-10-25T05:30:00Z"), None);

        let points = hourly.into_points();
        assert_eq!(
            points,
            vec![
                HourlyCostPoint {
                    hour_start: at("2026-10-25T00:00:00Z"),
                    offset_minutes: 120,
                    cost_usd: Some(1.5),
                },
                HourlyCostPoint {
                    hour_start: at("2026-10-25T01:00:00Z"),
                    offset_minutes: 60,
                    cost_usd: Some(2.0),
                },
                HourlyCostPoint {
                    hour_start: at("2026-10-25T05:00:00Z"),
                    offset_minutes: 60,
                    cost_usd: None,
                },
            ]
        );
    }
}
