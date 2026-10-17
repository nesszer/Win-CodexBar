//! Codex Auto-review included usage (upstream #4452, #4453).
//!
//! OpenAI stopped drawing `codex-auto-review` checks from ChatGPT plan usage on
//! 2026-10-06. A request at or after that instant, in a rollout whose leaf
//! `session_meta` names a ChatGPT creator account, is priced at an exact $0:
//! its tokens stay in totals and the model is not listed as unpriced. Every
//! other request keeps the unpriced treatment, and an explicit custom rate for
//! the model still prices all of its tokens at report time.
//!
//! The rule is applied once per parsed rollout by keying each included request
//! under [`INCLUDED_MODEL_KEY`] instead of the model name. The day aggregates
//! then keep included and unpriced requests apart without per-request storage.

use chrono::{DateTime, Utc};

use super::{CodexUsageRecord, CostUsagePricing};

/// The Codex model name that Auto-review requests are billed under.
pub(crate) const AUTO_REVIEW_MODEL: &str = "codex-auto-review";

/// 2026-10-06T07:00:00Z, midnight US Pacific (PDT).
pub(crate) const AUTO_REVIEW_INCLUDED_SINCE_UNIX_MS: i64 = 1_791_270_000_000;

/// Day-map model key for included Auto-review requests. Aggregation maps it
/// back to [`AUTO_REVIEW_MODEL`] for display and prices it at $0 without
/// marking the breakdown partial.
pub(crate) const INCLUDED_MODEL_KEY: &str = "codex-auto-review-included";

/// Whether `model` is the day-map key of included requests.
pub(crate) fn is_included_model_key(model: &str) -> bool {
    model == INCLUDED_MODEL_KEY
}

/// Whether one request is included in plan usage. A missing timestamp or a
/// rollout without a ChatGPT creator keeps the request unpriced, so a gap
/// over-reports cost rather than hiding it.
pub(crate) fn is_included_request(
    model: &str,
    timestamp: Option<DateTime<Utc>>,
    has_creator_account: bool,
) -> bool {
    has_creator_account
        && timestamp.is_some_and(|at| at.timestamp_millis() >= AUTO_REVIEW_INCLUDED_SINCE_UNIX_MS)
        && CostUsagePricing::normalize_codex_model(model) == AUTO_REVIEW_MODEL
}

/// Key the included requests of one rollout. `has_creator_account` is the
/// rollout's leaf `session_meta` decision; records of other rollouts are left
/// unchanged.
pub(crate) fn mark_included_records(
    records: &mut [(CodexUsageRecord, i64)],
    has_creator_account: bool,
) {
    if !has_creator_account {
        return;
    }
    for (record, _) in records {
        if is_included_request(&record.model, record.timestamp, true) {
            record.model = INCLUDED_MODEL_KEY.to_string();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn record(model: &str, timestamp: Option<DateTime<Utc>>) -> (CodexUsageRecord, i64) {
        (
            CodexUsageRecord {
                day_key: "2026-10-06".to_string(),
                timestamp,
                model: model.to_string(),
                input: 100,
                cached: 0,
                output: 10,
                reasoning: None,
                turn_id: None,
            },
            0,
        )
    }

    fn at_ms(ms: i64) -> Option<DateTime<Utc>> {
        DateTime::<Utc>::from_timestamp_millis(ms)
    }

    #[test]
    fn cutoff_is_midnight_pacific_on_2026_10_06() {
        assert_eq!(
            AUTO_REVIEW_INCLUDED_SINCE_UNIX_MS,
            Utc.with_ymd_and_hms(2026, 10, 6, 7, 0, 0)
                .unwrap()
                .timestamp_millis()
        );
    }

    #[test]
    fn request_is_included_only_at_or_after_the_cutoff() {
        let cutoff = AUTO_REVIEW_INCLUDED_SINCE_UNIX_MS;
        assert!(!is_included_request(
            AUTO_REVIEW_MODEL,
            at_ms(cutoff - 1),
            true
        ));
        assert!(is_included_request(AUTO_REVIEW_MODEL, at_ms(cutoff), true));
        assert!(is_included_request(
            AUTO_REVIEW_MODEL,
            at_ms(cutoff + 1),
            true
        ));
    }

    #[test]
    fn request_needs_creator_account_timestamp_and_auto_review_model() {
        let after = at_ms(AUTO_REVIEW_INCLUDED_SINCE_UNIX_MS + 1_000);
        assert!(!is_included_request(AUTO_REVIEW_MODEL, after, false));
        assert!(!is_included_request(AUTO_REVIEW_MODEL, None, true));
        assert!(!is_included_request("gpt-5.6-sol", after, true));
        assert!(is_included_request("openai/codex-auto-review", after, true));
    }

    #[test]
    fn included_key_is_free_on_every_pricing_path_and_other_auto_review_is_not() {
        let day = chrono::NaiveDate::from_ymd_opt(2026, 10, 6);
        assert_eq!(
            CostUsagePricing::codex_day_aggregate_cost_usd(
                INCLUDED_MODEL_KEY,
                1_000_000,
                200_000,
                1_000_000,
                day
            ),
            Some(0.0)
        );
        assert_eq!(
            CostUsagePricing::codex_cost_usd_at_date(
                INCLUDED_MODEL_KEY,
                1_000,
                0,
                1_000,
                chrono::NaiveDate::from_ymd_opt(2026, 10, 6).unwrap()
            ),
            Some(0.0)
        );
        assert_eq!(
            CostUsagePricing::codex_day_aggregate_cost_usd(AUTO_REVIEW_MODEL, 1_000, 0, 1_000, day),
            None,
            "pre-cutoff or non-ChatGPT Auto-review stays unpriced"
        );
    }

    #[test]
    fn mark_keys_only_eligible_records_of_a_creator_rollout() {
        let after = AUTO_REVIEW_INCLUDED_SINCE_UNIX_MS + 1_000;
        let mut records = vec![
            record(AUTO_REVIEW_MODEL, at_ms(after)),
            record(
                AUTO_REVIEW_MODEL,
                at_ms(AUTO_REVIEW_INCLUDED_SINCE_UNIX_MS - 1_000),
            ),
            record("gpt-5.6-sol", at_ms(after)),
        ];
        mark_included_records(&mut records, true);
        assert_eq!(records[0].0.model, INCLUDED_MODEL_KEY);
        assert_eq!(records[1].0.model, AUTO_REVIEW_MODEL);
        assert_eq!(records[2].0.model, "gpt-5.6-sol");

        let mut api_key = vec![record(AUTO_REVIEW_MODEL, at_ms(after))];
        mark_included_records(&mut api_key, false);
        assert_eq!(api_key[0].0.model, AUTO_REVIEW_MODEL);
    }
}
