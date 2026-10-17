//! Per-source spend series for the Usage & Spend explorer (upstream 0.73.0
//! `SpendDashboardModel.dailyPoints` / `hourlyPoints`).
//!
//! The backend only publishes recorded points; bucketing into weeks or months,
//! drill-down windows and legend isolation are presentation and live in the
//! frontend model. Hourly days carry their real local hour slots so 23- and
//! 25-hour daylight-saving days render without any clock arithmetic in the UI.

use chrono::{DateTime, Duration, NaiveDate, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};

use crate::cost_reporting_period::{CostDayBounds, CostTimeZone};
use crate::cost_scanner::HourlyCostPoint;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpendTrendSource {
    pub id: String,
    pub provider_id: String,
    pub display_name: String,
    /// The whole window is a covered, priced zero for this source.
    pub known_zero: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpendTrendDailyPoint {
    pub source_id: String,
    pub day: String,
    /// `None` is unknown (unscanned or unpriced); `Some(0.0)` is a covered zero.
    pub cost_usd: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpendTrendHourlyPoint {
    pub source_id: String,
    /// RFC 3339 UTC instant the local hour began.
    pub hour: String,
    pub cost_usd: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpendTrendHourSlot {
    pub start: String,
    pub offset_minutes: i32,
}

/// One local calendar day with hourly data, as its real hour slots.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpendTrendHourlyDay {
    pub day: String,
    pub start: String,
    pub end: String,
    pub hours: Vec<SpendTrendHourSlot>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpendTrend {
    /// First reporting day of the window, inclusive.
    pub start_day: String,
    /// Last reporting day of the window, inclusive.
    pub end_day: String,
    pub time_zone: String,
    pub sources: Vec<SpendTrendSource>,
    pub daily: Vec<SpendTrendDailyPoint>,
    pub hourly: Vec<SpendTrendHourlyPoint>,
    pub hourly_days: Vec<SpendTrendHourlyDay>,
}

fn day_key(day: NaiveDate) -> String {
    day.format("%Y-%m-%d").to_string()
}

fn instant(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(SecondsFormat::Secs, true)
}

/// Collects sources for one window and bucket zone.
pub struct SpendTrendBuilder {
    bounds: CostDayBounds,
    zone: CostTimeZone,
    trend: SpendTrend,
    hourly_days: std::collections::BTreeSet<NaiveDate>,
}

impl SpendTrendBuilder {
    pub fn new(bounds: CostDayBounds, zone: CostTimeZone) -> Self {
        Self {
            bounds,
            zone,
            trend: SpendTrend {
                start_day: day_key(bounds.start),
                end_day: day_key(bounds.end),
                time_zone: zone.identifier(),
                sources: Vec::new(),
                daily: Vec::new(),
                hourly: Vec::new(),
                hourly_days: Vec::new(),
            },
            hourly_days: std::collections::BTreeSet::new(),
        }
    }

    /// Adds one source. Days outside the window and non-finite or negative
    /// costs are dropped (a bad cost reads as unknown).
    pub fn add_source(
        &mut self,
        source: SpendTrendSource,
        daily: impl IntoIterator<Item = (String, Option<f64>)>,
        hourly: &[HourlyCostPoint],
    ) {
        let valid = |cost: Option<f64>| cost.filter(|cost| cost.is_finite() && *cost >= 0.0);
        let mut points: Vec<SpendTrendDailyPoint> = daily
            .into_iter()
            .filter(|(day, _)| self.bounds.contains_day_key(day))
            .map(|(day, cost)| SpendTrendDailyPoint {
                source_id: source.id.clone(),
                day,
                cost_usd: valid(cost),
            })
            .collect();
        points.sort_by(|a, b| a.day.cmp(&b.day));
        points.dedup_by(|a, b| a.day == b.day);
        self.trend.daily.extend(points);
        for point in hourly {
            let day = self.zone.date(point.hour_start);
            if day < self.bounds.start || day > self.bounds.end {
                continue;
            }
            self.hourly_days.insert(day);
            self.trend.hourly.push(SpendTrendHourlyPoint {
                source_id: source.id.clone(),
                hour: instant(point.hour_start),
                cost_usd: valid(point.cost_usd),
            });
        }
        self.trend.sources.push(source);
    }

    pub fn finish(mut self) -> SpendTrend {
        self.trend.sources.sort_by(|a, b| a.id.cmp(&b.id));
        self.trend
            .hourly
            .sort_by(|a, b| a.hour.cmp(&b.hour).then(a.source_id.cmp(&b.source_id)));
        self.trend.hourly_days = self
            .hourly_days
            .iter()
            .map(|day| hourly_day(self.zone, *day))
            .collect();
        self.trend
    }
}

/// The local hour slots of `day` in `zone`, from its first instant to the
/// next day's first instant.
pub fn hourly_day(zone: CostTimeZone, day: NaiveDate) -> SpendTrendHourlyDay {
    let start = zone.start_of_day_utc(day);
    let end = day.succ_opt().map_or(start + Duration::hours(24), |next| {
        zone.start_of_day_utc(next)
    });
    let mut hours = Vec::new();
    let mut cursor = start;
    while cursor < end && hours.len() < 26 {
        let (slot, offset_minutes) = zone.hour_start(cursor);
        hours.push(SpendTrendHourSlot {
            start: instant(slot.max(start)),
            offset_minutes,
        });
        cursor = slot + Duration::hours(1);
    }
    SpendTrendHourlyDay {
        day: day_key(day),
        start: instant(start),
        end: instant(end),
        hours,
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

    fn day(value: &str) -> NaiveDate {
        NaiveDate::parse_from_str(value, "%Y-%m-%d").unwrap()
    }

    const BERLIN: CostTimeZone = CostTimeZone::Named(chrono_tz::Europe::Berlin);

    fn source(id: &str) -> SpendTrendSource {
        SpendTrendSource {
            id: id.to_string(),
            provider_id: id.to_string(),
            display_name: id.to_string(),
            known_zero: false,
        }
    }

    #[test]
    fn dst_days_publish_their_real_hour_slots_and_offsets() {
        let regular = hourly_day(BERLIN, day("2026-01-10"));
        assert_eq!(regular.hours.len(), 24);
        assert_eq!(regular.start, "2026-01-09T23:00:00Z");
        assert_eq!(regular.end, "2026-01-10T23:00:00Z");

        let spring = hourly_day(BERLIN, day("2026-03-29"));
        assert_eq!(spring.hours.len(), 23);
        assert_eq!(spring.hours.first().unwrap().offset_minutes, 60);
        assert_eq!(spring.hours.last().unwrap().offset_minutes, 120);
        assert_eq!(spring.end, "2026-03-29T22:00:00Z");

        let fall = hourly_day(BERLIN, day("2026-10-25"));
        assert_eq!(fall.hours.len(), 25);
        assert_eq!(fall.hours[2].start, "2026-10-25T00:00:00Z");
        assert_eq!(fall.hours[2].offset_minutes, 120);
        assert_eq!(fall.hours[3].start, "2026-10-25T01:00:00Z");
        assert_eq!(fall.hours[3].offset_minutes, 60);
    }

    #[test]
    fn sources_keep_only_window_days_and_valid_costs() {
        let bounds = CostDayBounds {
            start: day("2026-03-28"),
            end: day("2026-03-30"),
        };
        let mut builder = SpendTrendBuilder::new(bounds, BERLIN);
        builder.add_source(
            source("claude"),
            vec![
                ("2026-03-27".to_string(), Some(9.0)),
                ("2026-03-28".to_string(), Some(1.0)),
                ("2026-03-29".to_string(), Some(f64::NAN)),
                ("2026-03-30".to_string(), Some(0.0)),
            ],
            &[
                HourlyCostPoint {
                    hour_start: at("2026-03-27T10:00:00Z"),
                    offset_minutes: 60,
                    cost_usd: Some(4.0),
                },
                HourlyCostPoint {
                    hour_start: at("2026-03-29T09:00:00Z"),
                    offset_minutes: 120,
                    cost_usd: Some(2.0),
                },
            ],
        );
        builder.add_source(source("codex"), Vec::new(), &[]);
        let trend = builder.finish();
        assert_eq!(trend.start_day, "2026-03-28");
        assert_eq!(trend.end_day, "2026-03-30");
        assert_eq!(trend.time_zone, "Europe/Berlin");
        assert_eq!(
            trend
                .sources
                .iter()
                .map(|s| s.id.as_str())
                .collect::<Vec<_>>(),
            ["claude", "codex"]
        );
        assert_eq!(
            trend
                .daily
                .iter()
                .map(|p| (p.day.as_str(), p.cost_usd))
                .collect::<Vec<_>>(),
            [
                ("2026-03-28", Some(1.0)),
                ("2026-03-29", None),
                ("2026-03-30", Some(0.0))
            ]
        );
        assert_eq!(trend.hourly.len(), 1);
        assert_eq!(trend.hourly[0].hour, "2026-03-29T09:00:00Z");
        assert_eq!(trend.hourly_days.len(), 1);
        assert_eq!(trend.hourly_days[0].day, "2026-03-29");
        assert_eq!(trend.hourly_days[0].hours.len(), 23);
    }

    #[test]
    fn serializes_camel_case_for_the_bridge() {
        let bounds = CostDayBounds {
            start: day("2026-01-01"),
            end: day("2026-01-01"),
        };
        let mut builder = SpendTrendBuilder::new(bounds, CostTimeZone::UTC);
        builder.add_source(
            source("claude"),
            vec![("2026-01-01".to_string(), Some(1.0))],
            &[],
        );
        let json = serde_json::to_value(builder.finish()).unwrap();
        assert_eq!(json["startDay"], "2026-01-01");
        assert_eq!(json["sources"][0]["providerId"], "claude");
        assert_eq!(json["sources"][0]["knownZero"], false);
        assert_eq!(json["daily"][0]["sourceId"], "claude");
        assert_eq!(json["daily"][0]["costUsd"], 1.0);
        assert!(json["hourlyDays"].as_array().unwrap().is_empty());
    }
}
