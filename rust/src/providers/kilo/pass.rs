//! Kilo Pass decoding for the `kiloPass.getState` tRPC payload.
//!
//! Mirrors upstream `KiloUsageFetcher.passFields` / `planName`
//! (`Sources/CodexBarCore/Providers/Kilo/KiloUsageFetcher.swift`): the live
//! payload nests the pass under `subscription`; a flat payload carrying the
//! same `currentPeriod*` / `tier` keys is treated as the subscription itself;
//! anything else falls back to a scan of generic usage keys.

use chrono::{DateTime, TimeZone, Utc};
use serde_json::{Map, Value};

type Object = Map<String, Value>;

/// How deep [`dictionary_contexts`] descends into nested objects (Swift `maxDepth`).
const MAX_CONTEXT_DEPTH: usize = 2;

#[derive(Debug, Default, Clone, PartialEq)]
pub(super) struct PassFields {
    pub used: Option<f64>,
    pub total: Option<f64>,
    pub remaining: Option<f64>,
    pub bonus: Option<f64>,
    pub resets_at: Option<DateTime<Utc>>,
}

pub(super) fn pass_fields(payload: Option<&Value>) -> PassFields {
    let Some(subscription) = subscription_data(payload) else {
        return fallback_pass_fields(payload);
    };

    let used = double_from(subscription.get("currentPeriodUsageUsd")).map(|v| v.max(0.0));
    let base = double_from(subscription.get("currentPeriodBaseCreditsUsd")).map(|v| v.max(0.0));
    let bonus = double_from(subscription.get("currentPeriodBonusCreditsUsd"))
        .unwrap_or(0.0)
        .max(0.0);
    let total = base.map(|base| base + bonus);
    let remaining = match (total, used) {
        (Some(total), Some(used)) => Some((total - used).max(0.0)),
        _ => None,
    };
    let resets_at = ["nextBillingAt", "nextRenewalAt", "renewsAt", "renewAt"]
        .iter()
        .find_map(|key| date_from(subscription.get(*key)));

    PassFields {
        used,
        total,
        remaining,
        bonus: (bonus > 0.0).then_some(bonus),
        resets_at,
    }
}

pub(super) fn plan_name(payload: Option<&Value>) -> Option<String> {
    if let Some(subscription) = subscription_data(payload) {
        if let Some(tier) = subscription.get("tier").and_then(Value::as_str) {
            let tier = tier.trim();
            if !tier.is_empty() {
                return Some(plan_name_for_tier(tier).to_string());
            }
        }
        return Some("Kilo Pass".to_string());
    }

    let contexts = dictionary_contexts(payload);
    let candidates = [
        first_string(
            &[
                "planName",
                "tier",
                "tierName",
                "passName",
                "subscriptionName",
            ],
            &contexts,
        ),
        string_value(&["plan", "name"], &contexts),
        string_value(&["subscription", "plan", "name"], &contexts),
        string_value(&["subscription", "name"], &contexts),
        string_value(&["pass", "name"], &contexts),
        string_value(&["state", "name"], &contexts),
        string_value(&["state"], &contexts),
    ];
    for candidate in candidates.into_iter().flatten() {
        let trimmed = candidate.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }

    first_string(&["name"], &contexts)
        .filter(|name| name.to_lowercase().contains("pass"))
        .map(str::to_string)
}

/// The subscription object: nested under `subscription`, or the payload itself
/// when it already has the subscription shape. An explicit `subscription: null`
/// means no pass.
fn subscription_data(payload: Option<&Value>) -> Option<&Object> {
    let object = payload?.as_object()?;
    match object.get("subscription") {
        Some(Value::Object(subscription)) => return Some(subscription),
        Some(Value::Null) => return None,
        _ => {}
    }

    let has_subscription_shape = [
        "currentPeriodUsageUsd",
        "currentPeriodBaseCreditsUsd",
        "currentPeriodBonusCreditsUsd",
        "tier",
    ]
    .iter()
    .any(|key| object.contains_key(*key));
    has_subscription_shape.then_some(object)
}

fn plan_name_for_tier(tier: &str) -> &str {
    match tier {
        "tier_19" => "Starter",
        "tier_49" => "Pro",
        "tier_199" => "Expert",
        other => other,
    }
}

fn fallback_pass_fields(payload: Option<&Value>) -> PassFields {
    let contexts = dictionary_contexts(payload);
    if contexts.is_empty() {
        return PassFields::default();
    }

    let mut total = money_amount(
        &[
            "amountCents",
            "totalCents",
            "planAmountCents",
            "monthlyAmountCents",
            "limitCents",
            "includedCents",
            "valueCents",
        ],
        &[
            "amount_mUsd",
            "total_mUsd",
            "planAmount_mUsd",
            "limit_mUsd",
            "included_mUsd",
            "value_mUsd",
        ],
        &[
            "amount",
            "total",
            "limit",
            "included",
            "value",
            "creditsTotal",
            "totalCredits",
            "planAmount",
        ],
        &contexts,
    );
    let mut used = money_amount(
        &[
            "usedCents",
            "spentCents",
            "consumedCents",
            "usedAmountCents",
            "consumedAmountCents",
        ],
        &[
            "used_mUsd",
            "spent_mUsd",
            "consumed_mUsd",
            "usedAmount_mUsd",
        ],
        &[
            "used",
            "spent",
            "consumed",
            "usage",
            "creditsUsed",
            "usedAmount",
            "consumedAmount",
        ],
        &contexts,
    );
    let mut remaining = money_amount(
        &[
            "remainingCents",
            "remainingAmountCents",
            "availableCents",
            "leftCents",
            "balanceCents",
        ],
        &[
            "remaining_mUsd",
            "available_mUsd",
            "left_mUsd",
            "balance_mUsd",
        ],
        &[
            "remaining",
            "available",
            "left",
            "balance",
            "creditsRemaining",
            "remainingAmount",
            "availableAmount",
        ],
        &contexts,
    );
    let bonus = money_amount(
        &[
            "bonusCents",
            "bonusAmountCents",
            "includedBonusCents",
            "bonusRemainingCents",
        ],
        &["bonus_mUsd", "bonusAmount_mUsd"],
        &["bonus", "bonusAmount", "bonusCredits", "includedBonus"],
        &contexts,
    );
    let resets_at = first_date(
        &[
            "resetAt",
            "resetsAt",
            "nextResetAt",
            "renewAt",
            "renewsAt",
            "nextRenewalAt",
            "currentPeriodEnd",
            "periodEndsAt",
            "expiresAt",
            "expiryAt",
        ],
        &contexts,
    );

    if total.is_none()
        && let (Some(used), Some(remaining)) = (used, remaining)
    {
        total = Some(used + remaining);
    }
    if used.is_none()
        && let (Some(total), Some(remaining)) = (total, remaining)
    {
        used = Some((total - remaining).max(0.0));
    }
    if remaining.is_none()
        && let (Some(total), Some(used)) = (total, used)
    {
        remaining = Some((total - used).max(0.0));
    }

    PassFields {
        used,
        total,
        remaining,
        bonus,
        resets_at,
    }
}

/// Breadth-first list of the payload object and its nested objects (including
/// objects inside arrays), down to [`MAX_CONTEXT_DEPTH`].
fn dictionary_contexts(payload: Option<&Value>) -> Vec<&Object> {
    let Some(root) = payload.and_then(Value::as_object) else {
        return Vec::new();
    };

    let mut contexts = Vec::new();
    let mut queue = std::collections::VecDeque::from([(root, 0_usize)]);
    while let Some((current, depth)) = queue.pop_front() {
        contexts.push(current);
        if depth >= MAX_CONTEXT_DEPTH {
            continue;
        }
        for value in current.values() {
            match value {
                Value::Object(nested) => queue.push_back((nested, depth + 1)),
                Value::Array(items) => {
                    for nested in items.iter().filter_map(Value::as_object) {
                        queue.push_back((nested, depth + 1));
                    }
                }
                _ => {}
            }
        }
    }
    contexts
}

fn first_double(keys: &[&str], contexts: &[&Object]) -> Option<f64> {
    contexts
        .iter()
        .find_map(|context| keys.iter().find_map(|key| double_from(context.get(*key))))
}

fn first_string<'a>(keys: &[&str], contexts: &[&'a Object]) -> Option<&'a str> {
    contexts
        .iter()
        .find_map(|context| keys.iter().find_map(|key| context.get(*key)?.as_str()))
}

fn first_date(keys: &[&str], contexts: &[&Object]) -> Option<DateTime<Utc>> {
    contexts
        .iter()
        .find_map(|context| keys.iter().find_map(|key| date_from(context.get(*key))))
}

fn string_value<'a>(path: &[&str], contexts: &[&'a Object]) -> Option<&'a str> {
    contexts.iter().find_map(|context| {
        let (first, rest) = path.split_first()?;
        let mut cursor = context.get(*first)?;
        for key in rest {
            cursor = cursor.get(*key)?;
        }
        cursor.as_str()
    })
}

fn money_amount(
    cents_keys: &[&str],
    micro_usd_keys: &[&str],
    plain_keys: &[&str],
    contexts: &[&Object],
) -> Option<f64> {
    if let Some(cents) = first_double(cents_keys, contexts) {
        return Some(cents / 100.0);
    }
    if let Some(micro_usd) = first_double(micro_usd_keys, contexts) {
        return Some(micro_usd / 1_000_000.0);
    }
    first_double(plain_keys, contexts)
}

fn double_from(raw: Option<&Value>) -> Option<f64> {
    match raw? {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.trim().parse::<f64>().ok(),
        _ => None,
    }
}

fn date_from(raw: Option<&Value>) -> Option<DateTime<Utc>> {
    match raw? {
        Value::Number(number) => date_from_epoch(number.as_f64()?),
        Value::String(text) => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                return None;
            }
            if let Ok(numeric) = trimmed.parse::<f64>() {
                return date_from_epoch(numeric);
            }
            DateTime::parse_from_rfc3339(trimmed)
                .ok()
                .map(|date| date.with_timezone(&Utc))
        }
        _ => None,
    }
}

/// Epoch seconds, or milliseconds when the magnitude is too large for seconds.
fn date_from_epoch(value: f64) -> Option<DateTime<Utc>> {
    if !value.is_finite() {
        return None;
    }
    let seconds = if value.abs() > 10_000_000_000.0 {
        value / 1000.0
    } else {
        value
    };
    let millis = (seconds * 1000.0).round();
    if millis.abs() >= i64::MAX as f64 {
        return None;
    }
    #[expect(
        clippy::cast_possible_truncation,
        reason = "millis is rounded and range-checked against i64 above"
    )]
    let millis = millis as i64;
    Utc.timestamp_millis_opt(millis).single()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_subscription_parses_period_fields_and_tier() {
        let payload = serde_json::json!({
            "subscription": {
                "tier": "tier_19",
                "currentPeriodUsageUsd": 0,
                "currentPeriodBaseCreditsUsd": 19.0,
                "currentPeriodBonusCreditsUsd": 9.5,
                "nextBillingAt": "2026-03-28T04:00:00.000Z"
            }
        });
        let fields = pass_fields(Some(&payload));
        assert_eq!(fields.used, Some(0.0));
        assert_eq!(fields.total, Some(28.5));
        assert_eq!(fields.remaining, Some(28.5));
        assert_eq!(fields.bonus, Some(9.5));
        assert_eq!(
            fields.resets_at.map(|date| date.to_rfc3339()),
            Some("2026-03-28T04:00:00+00:00".to_string())
        );
        assert_eq!(plan_name(Some(&payload)).as_deref(), Some("Starter"));
    }

    #[test]
    fn known_tiers_map_and_tierless_subscription_defaults_to_kilo_pass() {
        let pro = serde_json::json!({ "subscription": { "tier": "tier_49" } });
        assert_eq!(plan_name(Some(&pro)).as_deref(), Some("Pro"));
        let expert = serde_json::json!({ "subscription": { "tier": "tier_199" } });
        assert_eq!(plan_name(Some(&expert)).as_deref(), Some("Expert"));
        let unknown = serde_json::json!({ "subscription": { "tier": " tier_x " } });
        assert_eq!(plan_name(Some(&unknown)).as_deref(), Some("tier_x"));

        let no_tier = serde_json::json!({
            "subscription": { "currentPeriodUsageUsd": 1.0, "currentPeriodBaseCreditsUsd": 19.0 }
        });
        assert_eq!(plan_name(Some(&no_tier)).as_deref(), Some("Kilo Pass"));
        let fields = pass_fields(Some(&no_tier));
        assert_eq!(fields.total, Some(19.0));
        assert_eq!(fields.bonus, None);
    }

    #[test]
    fn flat_subscription_shape_is_the_subscription() {
        let payload = serde_json::json!({
            "tier": "tier_49",
            "currentPeriodUsageUsd": "12.4",
            "currentPeriodBaseCreditsUsd": 49,
            "renewsAt": 1_790_000_000_000_i64
        });
        let fields = pass_fields(Some(&payload));
        assert_eq!(fields.used, Some(12.4));
        assert_eq!(fields.total, Some(49.0));
        assert_eq!(
            fields.resets_at.map(|date| date.timestamp()),
            Some(1_790_000_000)
        );
        assert_eq!(plan_name(Some(&payload)).as_deref(), Some("Pro"));
    }

    #[test]
    fn null_subscription_has_no_pass() {
        let payload = serde_json::json!({ "subscription": null });
        assert_eq!(pass_fields(Some(&payload)), PassFields::default());
        assert_eq!(plan_name(Some(&payload)), None);
        assert_eq!(pass_fields(None), PassFields::default());
    }

    #[test]
    fn fallback_fields_use_micro_dollar_scale() {
        let payload = serde_json::json!({
            "planName": "Starter",
            "amount_mUsd": 28_500_000,
            "used_mUsd": 3_500_000,
            "bonus_mUsd": 9_500_000,
            "nextRenewalAt": "2026-03-28T04:00:00.000Z"
        });
        let fields = pass_fields(Some(&payload));
        assert_eq!(fields.total, Some(28.5));
        assert_eq!(fields.used, Some(3.5));
        assert_eq!(fields.remaining, Some(25.0));
        assert_eq!(fields.bonus, Some(9.5));
        assert!(fields.resets_at.is_some());
        assert_eq!(plan_name(Some(&payload)).as_deref(), Some("Starter"));
    }

    #[test]
    fn fallback_plan_name_reads_nested_plan_name() {
        let payload = serde_json::json!({ "plan": { "name": "Kilo Pass Pro" } });
        assert_eq!(plan_name(Some(&payload)).as_deref(), Some("Kilo Pass Pro"));
    }
}
