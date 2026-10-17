//! Claude plan renewal / paid-access expiration dates from the authenticated
//! `GET /api/organizations/{org}/subscription_details` claude.ai response
//! (upstream CodexBar #4324).
//!
//! These dates are billing metadata, independent of quota-window resets. A
//! missing, unavailable, or unrecognized response never invents a date and
//! never fails the usage fetch.

use chrono::{DateTime, NaiveDate, Utc};
use serde_json::{Map, Value};

use crate::core::SubscriptionMetadata;

/// The four date keys must all be present (each may be `null`); anything
/// else is an unrecognized schema.
const REQUIRED_KEYS: [&str; 4] = [
    "next_charge_at",
    "next_charge_date",
    "plan_ending_at",
    "plan_ending_before",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BillingDate {
    date: DateTime<Utc>,
    is_date_only: bool,
}

/// Parse a subscription_details body. `None` means unrecognized; `Some` with
/// no dates is an authoritative "nothing to show".
pub(super) fn parse(body: &str) -> Option<SubscriptionMetadata> {
    let Value::Object(fields) = serde_json::from_str::<Value>(body).ok()? else {
        return None;
    };
    let status = fields.get("status")?.as_str()?;
    if !matches!(status, "active" | "trialing" | "canceled") {
        return None;
    }
    if !REQUIRED_KEYS.iter().all(|key| fields.contains_key(*key)) {
        return None;
    }

    let end = first_date(&fields, "plan_ending_at", "plan_ending_before")?;
    // A scheduled ending takes precedence over a next-charge value left in
    // the same response.
    let renewal = if end.is_none() && matches!(status, "active" | "trialing") {
        first_date(&fields, "next_charge_at", "next_charge_date")?
    } else {
        None
    };

    Some(
        SubscriptionMetadata::new(None, end.map(|d| d.date), renewal.map(|d| d.date))
            .with_date_only(
                renewal.is_some_and(|d| d.is_date_only),
                end.is_some_and(|d| d.is_date_only),
            ),
    )
}

/// The first non-null of two keys. The fallback is only read when the
/// preferred key is null. Outer `None` = malformed value.
fn first_date(
    fields: &Map<String, Value>,
    preferred: &str,
    fallback: &str,
) -> Option<Option<BillingDate>> {
    match date(fields, preferred)? {
        Some(found) => Some(Some(found)),
        None => date(fields, fallback),
    }
}

fn date(fields: &Map<String, Value>, key: &str) -> Option<Option<BillingDate>> {
    let value = match fields.get(key) {
        None | Some(Value::Null) => return Some(None),
        Some(value) => value,
    };
    let raw = value.as_str()?;
    if raw.len() == 10 {
        let day = NaiveDate::parse_from_str(raw, "%Y-%m-%d").ok()?;
        // Reject lenient forms such as "2026-1-005" that re-format differently.
        if day.format("%Y-%m-%d").to_string() != raw {
            return None;
        }
        return Some(Some(BillingDate {
            date: day.and_hms_opt(0, 0, 0)?.and_utc(),
            is_date_only: true,
        }));
    }
    let parsed = DateTime::parse_from_rfc3339(raw).ok()?;
    Some(Some(BillingDate {
        date: parsed.with_timezone(&Utc),
        is_date_only: false,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn body(
        status: &str,
        next_at: &str,
        next_date: &str,
        end_at: &str,
        end_before: &str,
    ) -> String {
        format!(
            r#"{{"status":"{status}","next_charge_at":{next_at},"next_charge_date":{next_date},"plan_ending_at":{end_at},"plan_ending_before":{end_before}}}"#
        )
    }

    #[test]
    fn active_subscription_reports_timestamp_renewal() {
        let metadata = parse(&body(
            "active",
            r#""2026-11-03T08:15:00Z""#,
            "null",
            "null",
            "null",
        ))
        .expect("recognized");
        assert_eq!(
            metadata.renews_at,
            Some(Utc.with_ymd_and_hms(2026, 11, 3, 8, 15, 0).unwrap())
        );
        assert!(!metadata.renews_at_is_date_only);
        assert_eq!(metadata.expires_at, None);
    }

    #[test]
    fn calendar_only_renewal_keeps_date_precision() {
        let metadata = parse(&body("trialing", "null", r#""2026-12-01""#, "null", "null"))
            .expect("recognized");
        assert_eq!(
            metadata.renews_at,
            Some(Utc.with_ymd_and_hms(2026, 12, 1, 0, 0, 0).unwrap())
        );
        assert!(metadata.renews_at_is_date_only);
    }

    #[test]
    fn scheduled_ending_takes_precedence_over_next_charge() {
        let metadata = parse(&body(
            "active",
            r#""2026-11-03T08:15:00Z""#,
            "null",
            "null",
            r#""2026-11-20""#,
        ))
        .expect("recognized");
        assert_eq!(metadata.renews_at, None);
        assert_eq!(
            metadata.expires_at,
            Some(Utc.with_ymd_and_hms(2026, 11, 20, 0, 0, 0).unwrap())
        );
        assert!(metadata.expires_at_is_date_only);
        assert!(!metadata.renews_at_is_date_only);
    }

    #[test]
    fn canceled_subscription_never_reports_renewal() {
        let metadata = parse(&body(
            "canceled",
            r#""2026-11-03T08:15:00Z""#,
            "null",
            "null",
            "null",
        ))
        .expect("recognized");
        assert!(metadata.is_empty());
    }

    #[test]
    fn unrecognized_payloads_are_rejected() {
        // Unknown status.
        assert!(parse(&body("past_due", "null", "null", "null", "null")).is_none());
        // Missing required key.
        assert!(parse(r#"{"status":"active","next_charge_at":null}"#).is_none());
        // Non-string date.
        assert!(parse(&body("active", "12345", "null", "null", "null")).is_none());
        // Lenient or invalid calendar dates.
        assert!(parse(&body("active", "null", r#""2026-02-30""#, "null", "null")).is_none());
        assert!(parse(&body("active", "null", r#""2026/12/01""#, "null", "null")).is_none());
        // Unparseable timestamp.
        assert!(parse(&body("active", r#""tomorrow""#, "null", "null", "null")).is_none());
        assert!(parse("[]").is_none());
        assert!(parse("not json").is_none());
    }

    #[test]
    fn preferred_value_wins_without_reading_fallback() {
        // A malformed fallback is irrelevant when the preferred key has a date.
        let metadata = parse(&body(
            "active",
            r#""2026-11-03T08:15:00+02:00""#,
            "42",
            "null",
            "null",
        ))
        .expect("recognized");
        assert_eq!(
            metadata.renews_at,
            Some(Utc.with_ymd_and_hms(2026, 11, 3, 6, 15, 0).unwrap())
        );
    }

    #[test]
    fn date_only_flags_round_trip_through_json() {
        let metadata =
            SubscriptionMetadata::new(None, None, Some(Utc::now())).with_date_only(true, false);
        let json = serde_json::to_value(&metadata).unwrap();
        assert_eq!(json["renewsAtIsDateOnly"], true);
        assert!(json.get("expiresAtIsDateOnly").is_none());
        let restored: SubscriptionMetadata = serde_json::from_value(json).unwrap();
        assert_eq!(restored, metadata);
        let legacy: SubscriptionMetadata = serde_json::from_str("{}").unwrap();
        assert!(!legacy.renews_at_is_date_only && !legacy.expires_at_is_date_only);
    }
}
