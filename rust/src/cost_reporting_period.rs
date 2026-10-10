//! Calendar selection shared by local cost surfaces (upstream 0.67.0).
//!
//! A [`CostReportingPeriod`] is a *semantic* choice (rolling days, month to
//! date, or all available history). Every operation resolves it again into
//! inclusive local-day bounds at its own boundary, so a saved selection never
//! goes stale across midnight or a month rollover. All arithmetic runs on
//! calendar dates, never on 24-hour multiples, so leap years and 23/25-hour
//! daylight-saving days need no special cases.
//!
//! Days are bucketed in one pinned zone ([`cost_bucket_zone`]) so history
//! keeps its day boundaries when the machine's zone changes.

use std::fmt;
use std::sync::{PoisonError, RwLock};

use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, TimeZone, Utc};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Largest rolling window a user can select.
pub const MAX_ROLLING_DAYS: u32 = 365;
/// Rolling window used when nothing is saved.
pub const DEFAULT_ROLLING_DAYS: u32 = 30;
/// Sanity ceiling for a resolved day count handed to a scanner (100 years).
///
/// Rolling selections stop at [`MAX_ROLLING_DAYS`], but *all available* history
/// resolves to many more days. The ceiling only keeps date arithmetic far from
/// `NaiveDate` overflow when a caller passes an arbitrary `u32`.
pub const MAX_WINDOW_DAYS: u32 = 36_500;

/// First day considered by an all-available window when no earliest source day
/// is known. Nothing this app reads predates it.
const ALL_AVAILABLE_FLOOR: NaiveDate = match NaiveDate::from_ymd_opt(1970, 1, 1) {
    Some(date) => date,
    None => panic!("valid epoch date"),
};

/// Clamp a resolved day count into `1..=MAX_WINDOW_DAYS`.
pub fn clamp_window_days(days: u32) -> u32 {
    days.clamp(1, MAX_WINDOW_DAYS)
}

/// Zone whose midnights bound a reporting day.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CostTimeZone {
    /// The machine's zone, used while no zone is pinned.
    Local,
    /// A pinned IANA zone.
    Named(chrono_tz::Tz),
}

/// Pinned zone that local cost history is bucketed in. `None` buckets in the
/// machine zone.
static COST_BUCKET_ZONE: RwLock<Option<chrono_tz::Tz>> = RwLock::new(None);

/// The zone local cost history is bucketed in (upstream's pinned bucket
/// calendar, `tokenCostUsageBucketTimeZone`).
///
/// The saved `Settings::cost_usage_bucket_time_zone` is stored but not yet
/// applied at runtime: nothing in the desktop app or the CLI calls
/// [`set_cost_bucket_zone`] (the wiring PRs #653 and #665 were closed). Until
/// it is wired, history is bucketed in the machine zone.
pub fn cost_bucket_zone() -> CostTimeZone {
    let pinned = *COST_BUCKET_ZONE
        .read()
        .unwrap_or_else(PoisonError::into_inner);
    pinned.map_or(CostTimeZone::Local, CostTimeZone::Named)
}

/// Bucket local cost history in the zone `identifier` names, or in the
/// machine zone when it is empty or not an IANA zone. Returns the zone now in
/// effect.
pub fn set_cost_bucket_zone(identifier: &str) -> CostTimeZone {
    let zone = CostTimeZone::from_identifier(identifier);
    let pinned = match zone {
        CostTimeZone::Named(tz) => Some(tz),
        CostTimeZone::Local => None,
    };
    *COST_BUCKET_ZONE
        .write()
        .unwrap_or_else(PoisonError::into_inner) = pinned;
    zone
}

impl CostTimeZone {
    /// Resolve a saved identifier (upstream
    /// `CostUsageBucketTimeZone.timeZone(identifier:)`): a trimmed IANA name
    /// selects that zone; an empty or unknown name selects the machine zone.
    pub fn from_identifier(identifier: &str) -> Self {
        identifier
            .trim()
            .parse::<chrono_tz::Tz>()
            .map_or(Self::Local, Self::Named)
    }

    /// Whether `identifier` names an IANA zone (upstream `isValidIdentifier`).
    pub fn is_valid_identifier(identifier: &str) -> bool {
        identifier.parse::<chrono_tz::Tz>().is_ok()
    }

    /// The machine zone's IANA name to pin (upstream `pinIdentifier()`).
    ///
    /// `None` when the system zone cannot be read safely or is not a known
    /// IANA name, so a fallback zone is never persisted as the pin.
    pub fn pin_identifier() -> Option<String> {
        crate::core::try_local_timezone_name().filter(|name| Self::is_valid_identifier(name))
    }
    /// UTC, the bucket zone of provider-reported daily costs.
    pub const UTC: Self = Self::Named(chrono_tz::UTC);

    /// Zone identifier used in cache identities.
    pub fn identifier(&self) -> String {
        match self {
            Self::Local => crate::core::local_timezone_name(),
            Self::Named(tz) => tz.name().to_string(),
        }
    }

    /// The calendar date `now` falls on in this zone.
    pub fn date(&self, now: DateTime<Utc>) -> NaiveDate {
        match self {
            Self::Local => now.with_timezone(&Local).date_naive(),
            Self::Named(tz) => now.with_timezone(tz).date_naive(),
        }
    }

    /// The instant a calendar date begins in this zone.
    ///
    /// A midnight skipped by a DST gap begins at the first valid instant of
    /// that date instead.
    pub fn start_of_day_utc(&self, date: NaiveDate) -> DateTime<Utc> {
        match self {
            Self::Local => Self::resolve(&Local, date),
            Self::Named(tz) => Self::resolve(tz, date),
        }
    }

    fn resolve<Tz: TimeZone>(tz: &Tz, date: NaiveDate) -> DateTime<Utc> {
        let midnight = date.and_time(chrono::NaiveTime::MIN);
        // A gap at midnight (a few zones spring forward at 00:00) has no local
        // midnight; the first valid instant is at most a few hours later.
        (0..=6)
            .find_map(|hours| {
                tz.from_local_datetime(&(midnight + Duration::hours(hours)))
                    .earliest()
            })
            .map_or_else(
                || Utc.from_utc_datetime(&midnight),
                |local| local.with_timezone(&Utc),
            )
    }
}

/// Inclusive local-day bounds resolved from a [`CostReportingPeriod`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CostDayBounds {
    pub start: NaiveDate,
    pub end: NaiveDate,
}

impl CostDayBounds {
    /// Number of calendar days covered, counting both ends.
    pub fn days(&self) -> u32 {
        u32::try_from((self.end - self.start).num_days() + 1).unwrap_or(u32::MAX)
    }

    /// Whether a `YYYY-MM-DD` day key falls inside the bounds.
    pub fn contains_day_key(&self, key: &str) -> bool {
        NaiveDate::parse_from_str(key, "%Y-%m-%d")
            .is_ok_and(|day| day >= self.start && day <= self.end)
    }
}

/// Which slice of local history a cost surface reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CostReportingPeriod {
    /// The last N calendar days including today.
    ///
    /// Parsing and [`Self::rolling`] keep N within `1..=365`; the scanner
    /// entry point [`crate::cost_scanner::CostScanner::new`] wraps whatever
    /// day count its existing callers pass.
    Rolling(u32),
    /// From midnight on the first of the current month through today.
    MonthToDate,
    /// Every day with available source history. Not clamped to a year.
    AllAvailable,
}

impl Default for CostReportingPeriod {
    fn default() -> Self {
        Self::Rolling(DEFAULT_ROLLING_DAYS)
    }
}

impl CostReportingPeriod {
    /// A rolling selection clamped to `1..=365`.
    pub fn rolling(days: u32) -> Self {
        Self::Rolling(days.clamp(1, MAX_ROLLING_DAYS))
    }

    /// Parse the persisted form: `rolling:N`, `month-to-date`, or `all`.
    ///
    /// `rolling:0` and non-numeric counts are rejected; counts above 365 clamp.
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim() {
            "month-to-date" => Some(Self::MonthToDate),
            "all" => Some(Self::AllAvailable),
            other => {
                let days = other.strip_prefix("rolling:")?.parse::<u32>().ok()?;
                (days > 0).then(|| Self::rolling(days))
            }
        }
    }

    /// The persisted form, the inverse of [`Self::parse`].
    pub fn raw(&self) -> String {
        match self {
            Self::Rolling(days) => format!("rolling:{days}"),
            Self::MonthToDate => "month-to-date".to_string(),
            Self::AllAvailable => "all".to_string(),
        }
    }

    /// Human label shown next to totals: "Today", "Last N days", "Month to
    /// date" or "All".
    pub fn label(&self) -> String {
        match self {
            Self::Rolling(1) => "Today".to_string(),
            Self::Rolling(days) => format!("Last {days} days"),
            Self::MonthToDate => "Month to date".to_string(),
            Self::AllAvailable => "All".to_string(),
        }
    }

    /// Resolve a saved selection, falling back to a legacy day count and then
    /// to the 30-day default. An unreadable saved value migrates like a
    /// missing one.
    pub fn migrated(raw: Option<&str>, legacy_days: Option<u32>) -> Self {
        raw.and_then(Self::parse)
            .unwrap_or_else(|| Self::rolling(legacy_days.unwrap_or(DEFAULT_ROLLING_DAYS)))
    }

    /// Inclusive day bounds for `now` in `tz`.
    ///
    /// `earliest` only affects [`Self::AllAvailable`]: it is the first day
    /// with source data. Without it the window opens at 1970-01-01, which
    /// callers that walk day by day must avoid by supplying the real first
    /// day. The start never passes today.
    pub fn bounds(
        &self,
        now: DateTime<Utc>,
        tz: CostTimeZone,
        earliest: Option<NaiveDate>,
    ) -> CostDayBounds {
        let end = tz.date(now);
        let start = match self {
            Self::Rolling(days) => end - Duration::days(i64::from((*days).max(1) - 1)),
            Self::MonthToDate => end.with_day(1).unwrap_or(end),
            Self::AllAvailable => earliest.unwrap_or(ALL_AVAILABLE_FLOOR),
        };
        CostDayBounds {
            start: start.min(end),
            end,
        }
    }

    /// Number of days in the resolved window.
    pub fn days(&self, now: DateTime<Utc>, tz: CostTimeZone, earliest: Option<NaiveDate>) -> u32 {
        self.bounds(now, tz, earliest).days()
    }

    /// Keep the entries whose day key lies inside the resolved window.
    pub fn entries<T>(
        &self,
        entries: Vec<T>,
        day_key: impl Fn(&T) -> &str,
        now: DateTime<Utc>,
        tz: CostTimeZone,
    ) -> Vec<T> {
        let bounds = self.bounds(now, tz, None);
        entries
            .into_iter()
            .filter(|entry| bounds.contains_day_key(day_key(entry)))
            .collect()
    }

    /// Cache identity `raw|zone|start|end`.
    ///
    /// It changes with the selection, the pinned zone, and the resolved dates,
    /// so a month rollover or a zone change never reuses a previous window.
    pub fn identity(&self, now: DateTime<Utc>, tz: CostTimeZone) -> String {
        let bounds = self.bounds(now, tz, None);
        format!(
            "{}|{}|{}|{}",
            self.raw(),
            tz.identifier(),
            bounds.start.format("%Y-%m-%d"),
            bounds.end.format("%Y-%m-%d"),
        )
    }

    /// Day count for sources that scan a trailing day count, resolved in the
    /// local cost zone. All available history is not clamped to a year.
    pub fn scan_days(&self, now: DateTime<Utc>) -> u32 {
        clamp_window_days(self.days(now, CostTimeZone::Local, None))
    }

    /// Day count for sidecars that only support rolling windows of
    /// `1..=365` days (Codex workspaces, OpenCodex imports). Month to date
    /// maps to the days elapsed this month; all available history caps at a year.
    pub fn sidecar_days(&self, now: DateTime<Utc>) -> u32 {
        self.scan_days(now).clamp(1, MAX_ROLLING_DAYS)
    }

    /// Pick the period a request runs with: a valid raw `period` wins,
    /// otherwise `saved`.
    pub fn resolve_request(period: Option<&str>, saved: Self) -> Self {
        period.and_then(Self::parse).unwrap_or(saved)
    }
}

impl fmt::Display for CostReportingPeriod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.raw())
    }
}

impl Serialize for CostReportingPeriod {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.raw())
    }
}

/// Settings files must keep loading, so an unreadable value reads as the default.
impl<'de> Deserialize<'de> for CostReportingPeriod {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum SavedPeriod {
            Text(String),
            Unreadable(serde::de::IgnoredAny),
        }

        let raw = match SavedPeriod::deserialize(deserializer)? {
            SavedPeriod::Text(raw) => Some(raw),
            SavedPeriod::Unreadable(_) => None,
        };
        Ok(Self::migrated(raw.as_deref(), None))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono_tz::{America::Los_Angeles, Asia::Tokyo};

    const LA: CostTimeZone = CostTimeZone::Named(Los_Angeles);
    const UTC: CostTimeZone = CostTimeZone::UTC;

    fn at(value: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(value)
            .expect("fixture instant")
            .with_timezone(&Utc)
    }

    fn day(value: &str) -> NaiveDate {
        NaiveDate::parse_from_str(value, "%Y-%m-%d").expect("fixture day")
    }

    #[test]
    fn month_starts_and_day_counts_follow_the_zone_calendar() {
        // (now, days into the month, month start): copied in spirit from
        // upstream CostReportingPeriodTests (Los Angeles bucket calendar).
        let cases = [
            ("2026-01-31T20:00:00Z", 31, "2026-01-01"),
            ("2026-02-01T08:00:00Z", 1, "2026-02-01"),
            ("2026-02-28T20:00:00Z", 28, "2026-02-01"),
            ("2024-02-29T20:00:00Z", 29, "2024-02-01"),
            // 23-hour spring-forward day (2026-03-08) earlier in the month.
            ("2026-03-09T07:00:00Z", 9, "2026-03-01"),
            // 25-hour fall-back day (2026-11-01) earlier in the month.
            ("2026-11-02T08:00:00Z", 2, "2026-11-01"),
        ];
        for (now, days, start) in cases {
            let now = at(now);
            let period = CostReportingPeriod::MonthToDate;
            assert_eq!(period.days(now, LA, None), days, "{now}");
            assert_eq!(period.bounds(now, LA, None).start, day(start), "{now}");
            assert_eq!(period.bounds(now, LA, None).end, LA.date(now), "{now}");
        }
    }

    #[test]
    fn labels_match_upstream_wording() {
        assert_eq!(CostReportingPeriod::Rolling(1).label(), "Today");
        assert_eq!(CostReportingPeriod::Rolling(30).label(), "Last 30 days");
        assert_eq!(CostReportingPeriod::MonthToDate.label(), "Month to date");
        assert_eq!(CostReportingPeriod::AllAvailable.label(), "All");
    }

    #[test]
    fn identity_distinguishes_periods_rollover_and_zones() {
        let january = at("2026-02-01T07:59:59Z");
        let february = january + Duration::seconds(1);
        let month = CostReportingPeriod::MonthToDate;
        assert_ne!(month.identity(january, LA), month.identity(february, LA));
        assert_ne!(
            month.identity(february, LA),
            CostReportingPeriod::Rolling(1).identity(february, LA)
        );
        assert_eq!(month.days(january, LA, None), 31);
        assert_eq!(month.days(january, UTC, None), 1);
        assert_ne!(month.identity(january, LA), month.identity(january, UTC));
        assert_eq!(
            month.identity(january, LA),
            "month-to-date|America/Los_Angeles|2026-01-01|2026-01-31"
        );
    }

    #[test]
    fn rolling_bounds_include_today_and_never_use_24_hour_math() {
        let now = at("2026-03-09T07:00:00Z");
        // Seven days back from Mar 9 crosses the 2026-03-08 spring-forward day.
        let bounds = CostReportingPeriod::Rolling(7).bounds(now, LA, None);
        assert_eq!(
            (bounds.start, bounds.end),
            (day("2026-03-03"), day("2026-03-09"))
        );
        assert_eq!(bounds.days(), 7);
        let today = CostReportingPeriod::Rolling(1).bounds(now, LA, None);
        assert_eq!(
            (today.start, today.end),
            (day("2026-03-09"), day("2026-03-09"))
        );
        // A degenerate zero-day rolling window still covers today.
        assert_eq!(CostReportingPeriod::Rolling(0).days(now, LA, None), 1);
    }

    #[test]
    fn month_to_date_on_the_first_covers_only_today() {
        let now = at("2026-02-01T08:00:00Z");
        let bounds = CostReportingPeriod::MonthToDate.bounds(now, LA, None);
        assert_eq!(bounds.start, bounds.end);
        assert_eq!(bounds.start, day("2026-02-01"));
    }

    #[test]
    fn month_start_follows_the_pinned_zone_across_the_date_line() {
        let now = at("2026-02-28T16:00:00Z");
        // Tokyo is already on March 1; Los Angeles is still on February 28.
        assert_eq!(
            CostReportingPeriod::MonthToDate
                .bounds(now, CostTimeZone::Named(Tokyo), None)
                .start,
            day("2026-03-01")
        );
        assert_eq!(
            CostReportingPeriod::MonthToDate.bounds(now, LA, None).start,
            day("2026-02-01")
        );
    }

    #[test]
    fn all_available_uses_the_earliest_day_and_is_not_clamped_to_a_year() {
        let now = at("2026-07-15T12:00:00Z");
        let earliest = day("2023-05-04");
        let bounds = CostReportingPeriod::AllAvailable.bounds(now, UTC, Some(earliest));
        assert_eq!((bounds.start, bounds.end), (earliest, day("2026-07-15")));
        assert!(bounds.days() > MAX_ROLLING_DAYS);
        // Without a known earliest day the window opens at the epoch floor.
        assert_eq!(
            CostReportingPeriod::AllAvailable
                .bounds(now, UTC, None)
                .start,
            day("1970-01-01")
        );
        // A future earliest day cannot push the start past today.
        let future = CostReportingPeriod::AllAvailable.bounds(now, UTC, Some(day("2030-01-01")));
        assert_eq!(future.start, future.end);
    }

    #[test]
    fn raw_forms_round_trip_and_reject_invalid_input() {
        for (raw, period) in [
            ("rolling:30", CostReportingPeriod::Rolling(30)),
            ("rolling:1", CostReportingPeriod::Rolling(1)),
            ("month-to-date", CostReportingPeriod::MonthToDate),
            ("all", CostReportingPeriod::AllAvailable),
        ] {
            assert_eq!(CostReportingPeriod::parse(raw), Some(period));
            assert_eq!(period.raw(), raw);
        }
        assert_eq!(
            CostReportingPeriod::parse("rolling:999"),
            Some(CostReportingPeriod::Rolling(365))
        );
        for invalid in [
            "",
            "rolling:0",
            "rolling:",
            "rolling:-3",
            "rolling:x",
            "week",
            "ALL",
        ] {
            assert_eq!(CostReportingPeriod::parse(invalid), None, "{invalid:?}");
        }
    }

    #[test]
    fn scan_and_sidecar_days_follow_the_selection() {
        let now = Utc::now();
        let elapsed = CostReportingPeriod::MonthToDate.scan_days(now);
        assert_eq!(CostReportingPeriod::Rolling(7).scan_days(now), 7);
        assert_eq!(CostReportingPeriod::MonthToDate.sidecar_days(now), elapsed);
        assert!(CostReportingPeriod::AllAvailable.scan_days(now) > MAX_ROLLING_DAYS);
        assert_eq!(
            CostReportingPeriod::AllAvailable.sidecar_days(now),
            MAX_ROLLING_DAYS
        );
    }

    #[test]
    fn request_resolution_prefers_explicit_then_saved() {
        let saved = CostReportingPeriod::MonthToDate;
        let resolve = |period| CostReportingPeriod::resolve_request(period, saved);
        assert_eq!(resolve(Some("all")), CostReportingPeriod::AllAvailable);
        assert_eq!(
            resolve(Some("rolling:90")),
            CostReportingPeriod::Rolling(90)
        );
        assert_eq!(resolve(None), saved);
        assert_eq!(resolve(Some("bogus")), saved);
    }

    #[test]
    fn legacy_windows_migrate_unchanged() {
        let migrated = CostReportingPeriod::migrated;
        assert_eq!(migrated(None, Some(90)), CostReportingPeriod::Rolling(90));
        assert_eq!(migrated(None, None), CostReportingPeriod::Rolling(30));
        assert_eq!(
            migrated(Some("invalid"), Some(999)),
            CostReportingPeriod::Rolling(365)
        );
        assert_eq!(
            migrated(Some("month-to-date"), Some(90)),
            CostReportingPeriod::MonthToDate
        );
    }

    #[test]
    fn entries_filter_by_the_resolved_window() {
        let now = at("2026-02-02T20:00:00Z");
        let rows = vec![
            ("2026-01-31", 100),
            ("2026-02-01", 3),
            ("2026-02-02", 4),
            ("2026-02-03", 9),
            ("not-a-day", 1),
        ];
        let kept = CostReportingPeriod::MonthToDate.entries(rows, |row| row.0, now, LA);
        assert_eq!(kept.iter().map(|row| row.1).sum::<i32>(), 7);
    }

    #[test]
    fn start_of_day_is_the_local_midnight_instant() {
        assert_eq!(
            LA.start_of_day_utc(day("2026-02-01")),
            at("2026-02-01T08:00:00Z")
        );
        // Daylight time: the first of November 2026 is still PDT (UTC-7).
        assert_eq!(
            LA.start_of_day_utc(day("2026-11-01")),
            at("2026-11-01T07:00:00Z")
        );
        assert_eq!(
            UTC.start_of_day_utc(day("2026-02-01")),
            at("2026-02-01T00:00:00Z")
        );
    }

    #[test]
    fn serde_uses_the_raw_string_and_tolerates_unreadable_values() {
        assert_eq!(
            serde_json::to_string(&CostReportingPeriod::MonthToDate).unwrap(),
            "\"month-to-date\""
        );
        let read = |json: &str| serde_json::from_str::<CostReportingPeriod>(json).unwrap();
        assert_eq!(read("\"all\""), CostReportingPeriod::AllAvailable);
        assert_eq!(read("\"nonsense\""), CostReportingPeriod::Rolling(30));
        assert_eq!(read("null"), CostReportingPeriod::Rolling(30));
        assert_eq!(read("42"), CostReportingPeriod::Rolling(30));
        assert_eq!(read(r#"{"a":1}"#), CostReportingPeriod::Rolling(30));
    }

    #[test]
    fn saved_zone_identifiers_resolve_like_upstream() {
        assert_eq!(
            CostTimeZone::from_identifier(" Asia/Tokyo\n"),
            CostTimeZone::Named(Tokyo)
        );
        for fallback in ["", "   ", "Mars/Olympus", "local"] {
            assert_eq!(
                CostTimeZone::from_identifier(fallback),
                CostTimeZone::Local,
                "{fallback:?}"
            );
        }
        assert!(CostTimeZone::is_valid_identifier("America/Los_Angeles"));
        assert!(CostTimeZone::is_valid_identifier("UTC"));
        assert!(!CostTimeZone::is_valid_identifier(""));
        assert!(!CostTimeZone::is_valid_identifier("Mars/Olympus"));
    }

    #[test]
    fn pin_identifier_is_the_machine_zone_or_nothing() {
        if let Some(name) = CostTimeZone::pin_identifier() {
            assert!(CostTimeZone::is_valid_identifier(&name), "{name}");
            assert_eq!(name, crate::core::local_timezone_name());
        }
    }

    #[test]
    fn the_bucket_zone_applies_a_saved_identifier_process_wide() {
        // Other tests bucket through `cost_bucket_zone()` in parallel, so this
        // only ever applies the machine's own zone, which keeps their days.
        let Some(machine) = CostTimeZone::pin_identifier() else {
            return;
        };
        let applied = set_cost_bucket_zone(&machine);
        assert_eq!(applied, CostTimeZone::from_identifier(&machine));
        assert_eq!(cost_bucket_zone(), applied);
        assert_eq!(cost_bucket_zone().identifier(), machine);
        assert_eq!(set_cost_bucket_zone("Mars/Olympus"), CostTimeZone::Local);
        assert_eq!(cost_bucket_zone(), CostTimeZone::Local);
        assert_eq!(set_cost_bucket_zone(""), CostTimeZone::Local);
    }
}
