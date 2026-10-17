//! Quota model shared by the quota XML reader and the idea.log reader.

use chrono::{DateTime, Utc};

/// The IDE stores 1.00 displayed credit as 100,000 quota units
/// (10.00 monthly credits = 1,000,000 units).
const UNITS_PER_CREDIT: f64 = 100_000.0;

/// Monthly tariff usage plus the optional purchased top-up balance.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct QuotaInfo {
    /// Monthly tariff units used.
    pub used: f64,
    /// Monthly tariff units granted.
    pub maximum: f64,
    /// Purchased top-up balance, kept apart from the monthly percentage.
    pub top_up: Option<TopUpQuota>,
}

impl QuotaInfo {
    /// Monthly used percentage, clamped to 0-100. Zero when the maximum is not
    /// a positive finite number, so a broken record never shows false usage.
    pub fn used_percent(&self) -> f64 {
        if !(self.maximum.is_finite() && self.maximum > 0.0) || !self.used.is_finite() {
            return 0.0;
        }
        ((self.used / self.maximum) * 100.0).clamp(0.0, 100.0)
    }
}

/// Purchased top-up credits. JetBrains spends them only after the monthly
/// quota is used up, so they are a balance rather than a usage window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct TopUpQuota {
    pub maximum: f64,
    pub available: f64,
}

impl TopUpQuota {
    /// Accept only finite, nonnegative balances with a positive maximum.
    /// A missing or invalid balance is never reconstructed from usage.
    pub fn new(maximum: Option<f64>, available: Option<f64>) -> Option<Self> {
        let (maximum, available) = (maximum?, available?);
        let valid =
            maximum.is_finite() && maximum > 0.0 && available.is_finite() && available >= 0.0;
        valid.then(|| Self {
            maximum,
            available: available.min(maximum),
        })
    }

    /// Remaining purchased credits in displayed credit units.
    pub fn available_credits(&self) -> f64 {
        self.available / UNITS_PER_CREDIT
    }
}

/// Next monthly refill. Only the reset instant is shown.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct RefillInfo {
    pub next: Option<DateTime<Utc>>,
}

/// Parse an ISO-8601 / RFC 3339 instant with an explicit offset.
pub(super) fn parse_timestamp(raw: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|parsed| parsed.with_timezone(&Utc))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn used_percent_is_monthly_only_and_clamped() {
        let quota = QuotaInfo {
            used: 68_433.145,
            maximum: 1_000_000.0,
            top_up: None,
        };
        assert!((quota.used_percent() - 6.8433145).abs() < 1e-9);

        let over = QuotaInfo {
            used: 2_000.0,
            maximum: 1_000.0,
            top_up: None,
        };
        assert_eq!(over.used_percent(), 100.0);

        let no_maximum = QuotaInfo {
            used: 5.0,
            maximum: 0.0,
            top_up: None,
        };
        assert_eq!(no_maximum.used_percent(), 0.0);
    }

    #[test]
    fn top_up_requires_finite_nonnegative_balance_and_positive_maximum() {
        assert!(TopUpQuota::new(None, Some(1.0)).is_none());
        assert!(TopUpQuota::new(Some(10.0), None).is_none());
        assert!(TopUpQuota::new(Some(0.0), Some(0.0)).is_none());
        assert!(TopUpQuota::new(Some(10.0), Some(-1.0)).is_none());
        assert!(TopUpQuota::new(Some(10.0), Some(f64::NAN)).is_none());
        assert!(TopUpQuota::new(Some(f64::INFINITY), Some(1.0)).is_none());

        let zero = TopUpQuota::new(Some(5_489_986.397), Some(0.0)).expect("exhausted is valid");
        assert_eq!(zero.available_credits(), 0.0);
    }

    #[test]
    fn top_up_available_is_capped_at_maximum_and_converted_to_credits() {
        let capped = TopUpQuota::new(Some(100_000.0), Some(250_000.0)).expect("valid");
        assert_eq!(capped.available, 100_000.0);
        assert_eq!(capped.available_credits(), 1.0);

        let balance = TopUpQuota::new(Some(5_489_986.397), Some(5_489_986.397)).expect("valid");
        assert!((balance.available_credits() - 54.89986397).abs() < 1e-9);
    }

    #[test]
    fn parse_timestamp_requires_explicit_offset() {
        assert!(parse_timestamp("2028-09-22T21:00:00Z").is_some());
        assert!(parse_timestamp("2028-09-22T21:00:00").is_none());
        assert!(parse_timestamp("not a date").is_none());
    }
}
