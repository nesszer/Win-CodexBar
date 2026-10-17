//! Claude promotional cloud-session credits (`iguana_necktie`).
//!
//! The OAuth usage response and the Web usage response can carry a
//! promotional dollar balance under `iguana_necktie`: an allowance, the used
//! or remaining amount, an optional expiry (`resets_at`), and an optional
//! lock reason. These dollars are display-only. They never enter quota
//! windows, pacing, spend totals, or the prepaid Extra usage balance.
//!
//! The block becomes one transient [`ProviderDisplayDetail`] row, so it
//! reaches the tray card, the Visible usage items toggle, and CLI text through
//! the generic detail path. A missing or malformed block yields no row and
//! leaves the usage windows untouched.

use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::core::{ProviderDisplayDetail, format_currency};

/// Row title. The Visible usage items toggle keys on this title, so the
/// detail-section ID is `detailSection:Cloud credits`.
pub(super) const DETAIL_TITLE: &str = "Cloud credits";
const DETAIL_ROW_ID: &str = "claude-cloud-credits";
const EXPIRED_VALUE: &str = "Expired";
const UNAVAILABLE_VALUE: &str = "Unavailable";

/// Build the display row from the raw `iguana_necktie` value.
///
/// Returns `None` when the block is absent or malformed: a missing,
/// non-finite, or non-positive allowance; a used or remaining amount outside
/// `0..=limit`; neither amount present; or an unreadable `resets_at`.
pub(super) fn detail_from_block(
    block: &serde_json::Value,
    now: DateTime<Utc>,
) -> Option<ProviderDisplayDetail> {
    CloudCreditsBlock::deserialize(block)
        .ok()?
        .validate()?
        .detail(now)
}

/// Raw `iguana_necktie` block. Amounts are USD, not minor units.
#[derive(Debug, Deserialize)]
struct CloudCreditsBlock {
    limit_dollars: f64,
    #[serde(default)]
    used_dollars: Option<f64>,
    #[serde(default)]
    remaining_dollars: Option<f64>,
    #[serde(default)]
    resets_at: Option<String>,
    #[serde(default)]
    locked_reason: Option<String>,
}

/// Validated credits. `remaining` is the reported amount when present,
/// otherwise the allowance minus the reported used amount.
#[derive(Debug, Clone, PartialEq)]
struct CloudCredits {
    limit: f64,
    remaining: f64,
    expires_at: Option<DateTime<Utc>>,
    locked: bool,
}

impl CloudCreditsBlock {
    fn validate(self) -> Option<CloudCredits> {
        let limit = self.limit_dollars;
        if !(limit.is_finite() && limit > 0.0) {
            return None;
        }
        let in_range = |amount: f64| amount.is_finite() && (0.0..=limit).contains(&amount);
        if self.used_dollars.is_some_and(|used| !in_range(used)) {
            return None;
        }
        if self
            .remaining_dollars
            .is_some_and(|remaining| !in_range(remaining))
        {
            return None;
        }
        // The reported remaining amount wins, so the balance and the progress
        // bar always agree.
        let remaining = self
            .remaining_dollars
            .or_else(|| self.used_dollars.map(|used| limit - used))?;
        let expires_at = match self.resets_at.as_deref() {
            Some(raw) => Some(DateTime::parse_from_rfc3339(raw).ok()?.with_timezone(&Utc)),
            None => None,
        };
        let locked = self
            .locked_reason
            .as_deref()
            .is_some_and(|reason| !reason.trim().is_empty());
        Some(CloudCredits {
            limit,
            remaining,
            expires_at,
            locked,
        })
    }
}

impl CloudCredits {
    fn detail(&self, now: DateTime<Utc>) -> Option<ProviderDisplayDetail> {
        // `resets_at` marks expiry, not a recurring reset. An expiry at or
        // before `now` means the credits are gone.
        let expired = self.expires_at.is_some_and(|expiry| expiry <= now);
        let (value, used) = if expired {
            (EXPIRED_VALUE.to_string(), None)
        } else if self.locked {
            (UNAVAILABLE_VALUE.to_string(), None)
        } else {
            (
                format!(
                    "{} of {} remaining",
                    format_currency(self.remaining, "USD"),
                    format_currency(self.limit, "USD"),
                ),
                Some(self.limit - self.remaining),
            )
        };
        let mut row = ProviderDisplayDetail::new(DETAIL_ROW_ID, DETAIL_TITLE, value)?;
        if let Some(used) = used {
            row = row.with_progress(used, self.limit)?;
        }
        // The expired state omits the timestamp: its label is already
        // localized, and an English "Expired <time>" fragment would not be.
        if let Some(expiry) = self.expires_at.filter(|_| !expired) {
            row = row.with_secondary_value(format!("Expires {}", expiry.format("%Y-%m-%d")))?;
        }
        Some(row)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use serde_json::json;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 3, 12, 0, 0).unwrap()
    }

    fn detail(block: serde_json::Value) -> Option<ProviderDisplayDetail> {
        detail_from_block(&block, now())
    }

    #[test]
    fn reported_remaining_becomes_value_progress_and_expiry() {
        let row = detail(json!({
            "limit_dollars": 20.0,
            "used_dollars": 8.0,
            "remaining_dollars": 12.0,
            "resets_at": "2099-01-01T00:00:00Z"
        }))
        .expect("valid block");

        assert_eq!(row.id(), "claude-cloud-credits");
        assert_eq!(row.title(), "Cloud credits");
        assert_eq!(row.section_title(), None);
        assert_eq!(row.value(), "$12.00 of $20.00 remaining");
        assert_eq!(row.secondary_value(), Some("Expires 2099-01-01"));
        let progress = row.progress().expect("progress");
        assert_eq!(progress.used(), 8.0);
        assert_eq!(progress.total(), 20.0);
    }

    #[test]
    fn remaining_is_derived_from_used_when_absent() {
        let row = detail(json!({"limit_dollars": 20.0, "used_dollars": 5.0})).expect("valid");

        assert_eq!(row.value(), "$15.00 of $20.00 remaining");
        assert_eq!(row.progress().unwrap().used(), 5.0);
        assert_eq!(row.secondary_value(), None);
    }

    #[test]
    fn reported_remaining_wins_over_used() {
        let row = detail(json!({
            "limit_dollars": 10.0,
            "used_dollars": 5.0,
            "remaining_dollars": 3.0
        }))
        .expect("valid");

        assert_eq!(row.value(), "$3.00 of $10.00 remaining");
        assert_eq!(row.progress().unwrap().used(), 7.0);
    }

    #[test]
    fn exhausted_balance_keeps_a_zero_remaining_row() {
        let row = detail(json!({"limit_dollars": 20.0, "remaining_dollars": 0.0})).expect("valid");

        assert_eq!(row.value(), "$0.00 of $20.00 remaining");
        assert_eq!(row.progress().unwrap().used(), 20.0);
    }

    #[test]
    fn expiry_at_or_before_now_reports_expired_without_progress() {
        let past = detail(json!({
            "limit_dollars": 20.0,
            "remaining_dollars": 12.0,
            "resets_at": "2026-10-03T11:59:59Z"
        }))
        .expect("valid");
        assert_eq!(past.value(), "Expired");
        assert!(past.progress().is_none());
        assert_eq!(past.secondary_value(), None);

        let at_now = detail(json!({
            "limit_dollars": 20.0,
            "remaining_dollars": 12.0,
            "resets_at": "2026-10-03T12:00:00Z"
        }))
        .expect("valid");
        assert_eq!(at_now.value(), "Expired");
    }

    #[test]
    fn lock_reason_reports_unavailable_and_keeps_expiry() {
        let row = detail(json!({
            "limit_dollars": 20.0,
            "remaining_dollars": 12.0,
            "resets_at": "2099-01-01T00:00:00Z",
            "locked_reason": "plan_restricted"
        }))
        .expect("valid");

        assert_eq!(row.value(), "Unavailable");
        assert!(row.progress().is_none());
        assert_eq!(row.secondary_value(), Some("Expires 2099-01-01"));
    }

    #[test]
    fn blank_lock_reason_is_not_a_lock() {
        let row = detail(json!({
            "limit_dollars": 20.0,
            "remaining_dollars": 12.0,
            "locked_reason": "   "
        }))
        .expect("valid");

        assert_eq!(row.value(), "$12.00 of $20.00 remaining");
    }

    #[test]
    fn malformed_blocks_yield_no_row() {
        let cases = [
            json!(null),
            json!("not an object"),
            json!({"remaining_dollars": 12.0}),
            json!({"limit_dollars": "20"}),
            json!({"limit_dollars": 0.0, "remaining_dollars": 0.0}),
            json!({"limit_dollars": -5.0, "remaining_dollars": 1.0}),
            json!({"limit_dollars": 20.0}),
            json!({"limit_dollars": 20.0, "remaining_dollars": 21.0}),
            json!({"limit_dollars": 20.0, "remaining_dollars": -1.0}),
            json!({"limit_dollars": 20.0, "used_dollars": 21.0}),
            json!({"limit_dollars": 20.0, "used_dollars": -1.0}),
            json!({"limit_dollars": 20.0, "remaining_dollars": 12.0, "resets_at": "soon"}),
            json!({"limit_dollars": 20.0, "remaining_dollars": 12.0, "resets_at": ""}),
        ];
        for block in cases {
            assert!(detail(block.clone()).is_none(), "accepted {block}");
        }
    }
}
