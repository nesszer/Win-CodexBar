//! Lenient lookups over the expanded Alibaba console JSON.

use chrono::{DateTime, NaiveDate, NaiveDateTime, TimeZone, Utc};
use serde_json::Value;

pub(super) fn expand_json_strings(value: Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(values.into_iter().map(expand_json_strings).collect()),
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(key, value)| (key, expand_json_strings(value)))
                .collect(),
        ),
        Value::String(text) => serde_json::from_str::<Value>(&text)
            .ok()
            .filter(|nested| nested.is_object() || nested.is_array())
            .map(expand_json_strings)
            .unwrap_or(Value::String(text)),
        other => other,
    }
}

pub(super) fn percentage_points(ratio: Option<f64>) -> Option<f64> {
    let ratio = ratio.filter(|v| v.is_finite())?;
    Some((ratio.clamp(0.0, 1.0) * 100.0).clamp(0.0, 100.0))
}

pub(super) fn number_field(value: &Value, key: &str) -> Option<f64> {
    value.as_object().and_then(|map| parse_f64(map.get(key)))
}

pub(super) fn date_field(value: &Value, key: &str) -> Option<DateTime<Utc>> {
    value.as_object().and_then(|map| parse_date(map.get(key)))
}

/// Depth-first search: `probe` runs on each node before its children, and
/// the first `Some` wins. Probes return `None` for arrays and scalars.
pub(super) fn deep_find<'a, T>(
    value: &'a Value,
    probe: &impl Fn(&'a Value) -> Option<T>,
) -> Option<T> {
    if let Some(found) = probe(value) {
        return Some(found);
    }
    match value {
        Value::Object(map) => map.values().find_map(|nested| deep_find(nested, probe)),
        Value::Array(values) => values.iter().find_map(|nested| deep_find(nested, probe)),
        _ => None,
    }
}

pub(super) fn find_object_containing_any_of(value: &Value, keys: &[&str]) -> Option<Value> {
    deep_find(value, &|node| {
        let map = node.as_object()?;
        keys.iter()
            .any(|key| map.contains_key(*key))
            .then(|| node.clone())
    })
}

pub(super) const PLAN_NAME_KEYS: &[&str] = &[
    "planName",
    "plan_name",
    "packageName",
    "package_name",
    "commodityName",
    "commodity_name",
    "instanceName",
    "instance_name",
    "displayName",
    "display_name",
    "name",
    "title",
    "planType",
    "plan_type",
    "ProductName",
    "productName",
];
pub(super) const USED_QUOTA_KEYS: &[&str] = &[
    "usedQuota",
    "used_quota",
    "usedCredits",
    "usedCredit",
    "consumedCredits",
    "usage",
    "used",
    "usedAmount",
    "consumeAmount",
    "usedValue",
    "UsedValue",
    "consumedValue",
    "ConsumedValue",
];
pub(super) const TOTAL_QUOTA_KEYS: &[&str] = &[
    "totalQuota",
    "total_quota",
    "totalCredits",
    "totalCredit",
    "quota",
    "creditLimit",
    "creditsTotal",
    "monthlyTotalQuota",
    "amount",
    "totalValue",
    "TotalValue",
    "totalCount",
    "TotalCount",
    "subscriptionTotalNumber",
    "SubscriptionTotalNumber",
];
pub(super) const REMAINING_QUOTA_KEYS: &[&str] = &[
    "remainingQuota",
    "remainQuota",
    "remainingCredits",
    "remainingCredit",
    "availableCredits",
    "balance",
    "remaining",
    "availableAmount",
    "remainAmount",
    "totalSurplusValue",
    "TotalSurplusValue",
    "surplusValue",
    "SurplusValue",
];
pub(super) const RESET_DATE_KEYS: &[&str] = &[
    "nextRefreshTime",
    "resetTime",
    "periodEndTime",
    "billingCycleEnd",
    "billCycleEndTime",
    "expireTime",
    "expirationTime",
    "endTime",
    "validEndTime",
    "instanceEndTime",
    "nearestExpireDate",
    "NearestExpireDate",
];

pub(super) fn find_token_plan_instance(value: &Value) -> Option<Value> {
    find_first_object(
        value,
        &[
            "tokenPlanInstanceInfo",
            "token_plan_instance_info",
            "instanceInfo",
            "instance_info",
        ],
    )
    .or_else(|| {
        find_first_array(
            value,
            &[
                "tokenPlanInstanceInfos",
                "token_plan_instance_infos",
                "instanceInfos",
                "instances",
                "Data",
                "data",
                "successResponse",
            ],
        )
        .and_then(|values| {
            values
                .into_iter()
                .filter(Value::is_object)
                .max_by_key(active_signal_score)
        })
    })
}

pub(super) fn find_plan_name(value: &Value) -> Option<String> {
    first_string(value, PLAN_NAME_KEYS).or_else(|| find_first_string(value, PLAN_NAME_KEYS))
}

pub(super) fn find_quota_info(value: &Value) -> Option<Value> {
    find_first_object(
        value,
        &[
            "quotaInfo",
            "quota_info",
            "tokenPlanQuotaInfo",
            "token_plan_quota_info",
        ],
    )
    .or_else(|| {
        find_object_containing_any_of(
            value,
            &[USED_QUOTA_KEYS, TOTAL_QUOTA_KEYS, REMAINING_QUOTA_KEYS].concat(),
        )
    })
}

pub(super) fn find_reset_date(value: &Value) -> Option<DateTime<Utc>> {
    first_date(value, RESET_DATE_KEYS).or_else(|| find_first_date(value, RESET_DATE_KEYS))
}

pub(super) fn find_first_object(value: &Value, keys: &[&str]) -> Option<Value> {
    deep_find(value, &|node| {
        let map = node.as_object()?;
        keys.iter()
            .find_map(|key| map.get(*key).filter(|v| v.is_object()).cloned())
    })
}

pub(super) fn find_first_array(value: &Value, keys: &[&str]) -> Option<Vec<Value>> {
    deep_find(value, &|node| {
        let map = node.as_object()?;
        keys.iter()
            .find_map(|key| map.get(*key).and_then(Value::as_array).cloned())
    })
}

pub(super) fn first_string(value: &Value, keys: &[&str]) -> Option<String> {
    let map = value.as_object()?;
    keys.iter().find_map(|key| parse_string(map.get(*key)))
}

pub(super) fn find_first_string(value: &Value, keys: &[&str]) -> Option<String> {
    deep_find(value, &|node| first_string(node, keys))
}

pub(super) fn first_f64(value: &Value, keys: &[&str]) -> Option<f64> {
    let map = value.as_object()?;
    keys.iter().find_map(|key| parse_f64(map.get(*key)))
}

pub(super) fn find_first_i64(value: &Value, keys: &[&str]) -> Option<i64> {
    deep_find(value, &|node| {
        let map = node.as_object()?;
        keys.iter().find_map(|key| parse_i64(map.get(*key)))
    })
}

pub(super) fn first_date(value: &Value, keys: &[&str]) -> Option<DateTime<Utc>> {
    let map = value.as_object()?;
    keys.iter().find_map(|key| parse_date(map.get(*key)))
}

pub(super) fn find_first_date(value: &Value, keys: &[&str]) -> Option<DateTime<Utc>> {
    deep_find(value, &|node| first_date(node, keys))
}

pub(super) fn parse_string(value: Option<&Value>) -> Option<String> {
    value?
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

pub(super) fn parse_f64(value: Option<&Value>) -> Option<f64> {
    match value? {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.trim().replace(',', "").parse().ok(),
        _ => None,
    }
}

pub(super) fn parse_i64(value: Option<&Value>) -> Option<i64> {
    match value? {
        Value::Number(number) => number.as_i64().or_else(|| {
            // Quota/timestamp JSON floats are whole numbers; the fractional
            // part is rounding noise from the upstream API.
            let v = number.as_f64()?;
            #[expect(clippy::cast_possible_truncation, reason = "quota/timestamp JSON floats are whole numbers; fractional part is rounding noise")]
            let whole = v as i64;
            Some(whole)
        }),
        Value::String(text) => text.trim().replace(',', "").parse().ok(),
        _ => None,
    }
}

pub(super) fn parse_bool(value: Option<&Value>) -> Option<bool> {
    match value? {
        Value::Bool(flag) => Some(*flag),
        Value::Number(number) => number.as_i64().map(|v| v != 0),
        Value::String(text) => match text.trim().to_lowercase().as_str() {
            "true" | "1" | "yes" | "active" | "valid" | "normal" => Some(true),
            "false" | "0" | "no" | "inactive" | "invalid" | "expired" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

pub(super) fn parse_date(value: Option<&Value>) -> Option<DateTime<Utc>> {
    if let Some(raw) = parse_i64(value) {
        if raw > 1_000_000_000_000 {
            return Utc.timestamp_opt(raw / 1000, 0).single();
        }
        if raw > 1_000_000_000 {
            return Utc.timestamp_opt(raw, 0).single();
        }
    }
    let text = parse_string(value)?;
    if let Ok(date) = DateTime::parse_from_rfc3339(&text) {
        return Some(date.with_timezone(&Utc));
    }
    if let Ok(date) = NaiveDate::parse_from_str(&text, "%Y-%m-%d")
        && let Some(date_time) = date.and_hms_opt(0, 0, 0)
    {
        return Some(date_time.and_utc());
    }
    for format in ["%Y-%m-%d %H:%M", "%Y-%m-%d %H:%M:%S"] {
        if let Ok(date) = NaiveDateTime::parse_from_str(&text, format) {
            return Some(date.and_utc());
        }
    }
    None
}

pub(super) fn active_signal_score(value: &Value) -> i32 {
    let status = first_string(value, &["status", "instanceStatus", "state"])
        .unwrap_or_default()
        .to_uppercase();
    if ["VALID", "ACTIVE", "NORMAL"].contains(&status.as_str()) {
        return 3;
    }
    if [
        "EXPIRED",
        "INVALID",
        "INACTIVE",
        "DISABLED",
        "TERMINATED",
        "STOPPED",
    ]
    .contains(&status.as_str())
    {
        return -1;
    }
    parse_bool(
        value
            .as_object()
            .and_then(|map| map.get("isActive").or_else(|| map.get("active"))),
    )
    .map(|active| if active { 3 } else { -1 })
    .unwrap_or(0)
}
