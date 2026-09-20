use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use reqwest::Client;
use serde_json::Value;
use std::collections::BTreeMap;

use crate::core::{
    CostDailyPoint, CostSnapshot, FetchContext, Provider, ProviderError, ProviderFetchResult,
    ProviderId, ProviderMetadata, RateWindow, SourceMode, UsageSnapshot,
};

const POE_BALANCE_URL: &str = "https://api.poe.com/usage/current_balance";
const POE_HISTORY_URL: &str = "https://api.poe.com/usage/points_history";
const CREDENTIAL_TARGET: &str = "codexbar-poe";
const POE_HISTORY_PAGE_LIMIT: usize = 100;
const POE_HISTORY_MAX_PAGES: usize = 5;

#[derive(Debug, Clone, PartialEq)]
struct PoeHistoryEntry {
    date: DateTime<Utc>,
    points: f64,
}

#[derive(Debug, Clone, PartialEq)]
struct PoeHistorySummary {
    total_points: f64,
    daily: Vec<CostDailyPoint>,
}

pub struct PoeProvider {
    metadata: ProviderMetadata,
    client: Client,
}

impl PoeProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: ProviderId::Poe,
                display_name: "Poe",
                session_label: "Balance",
                weekly_label: "Points",
                supports_opus: false,
                supports_credits: true,
                default_enabled: false,
                is_primary: false,
                dashboard_url: Some("https://poe.com/settings/subscription"),
                status_page_url: None,
                tertiary_label_key: None,
            },
            client: crate::core::credentialed_http_client_builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }
}

impl Default for PoeProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for PoeProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Poe
    }

    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        match ctx.source_mode {
            SourceMode::Auto | SourceMode::OAuth => {
                let key = crate::providers::resolve_api_key(
                    ctx.api_key.as_deref(),
                    CREDENTIAL_TARGET,
                    &["POE_API_KEY"],
                )?;
                // Capture one refresh instant for the complete balance/history snapshot.
                let refresh_now = Utc::now();
                let response = self
                    .client
                    .get(POE_BALANCE_URL)
                    .bearer_auth(&key)
                    .header("Accept", "application/json")
                    .send()
                    .await?;
                if response.status() == reqwest::StatusCode::UNAUTHORIZED
                    || response.status() == reqwest::StatusCode::FORBIDDEN
                {
                    return Err(ProviderError::AuthRequired);
                }
                if !response.status().is_success() {
                    return Err(ProviderError::Other(format!(
                        "Poe usage returned status {}",
                        response.status()
                    )));
                }
                let value: Value = response.json().await.map_err(|e| {
                    ProviderError::Parse(format!("Failed to parse Poe balance: {e}"))
                })?;
                let balance = first_number(
                    &value,
                    &[
                        "current_point_balance",
                        "currentPointBalance",
                        "balance",
                        "points",
                    ],
                );
                let mut result =
                    ProviderFetchResult::new(snapshot_from_balance_at(&value, refresh_now), "api");
                if ctx.include_credits
                    && let Ok(entries) = fetch_points_history(&self.client, &key, refresh_now).await
                    && let Some(cost) = cost_snapshot_from_history(&entries, balance, refresh_now)
                {
                    result = result.with_cost(cost);
                }
                Ok(result)
            }
            SourceMode::Web | SourceMode::Cli => {
                Err(ProviderError::UnsupportedSource(ctx.source_mode))
            }
        }
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::OAuth]
    }
}

async fn fetch_points_history(
    client: &Client,
    key: &str,
    refresh_now: DateTime<Utc>,
) -> Result<Vec<PoeHistoryEntry>, ProviderError> {
    let cutoff = refresh_now - Duration::days(30);
    let mut cursor = None;
    let mut entries = Vec::new();

    for _ in 0..POE_HISTORY_MAX_PAGES {
        let mut url = reqwest::Url::parse(POE_HISTORY_URL)
            .map_err(|e| ProviderError::Other(e.to_string()))?;
        {
            let mut query = url.query_pairs_mut();
            query.append_pair("limit", &POE_HISTORY_PAGE_LIMIT.to_string());
            if let Some(cursor) = cursor.as_deref() {
                query.append_pair("starting_after", cursor);
            }
        }

        let response = client
            .get(url)
            .bearer_auth(key)
            .header("Accept", "application/json")
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(ProviderError::Other(format!(
                "Poe history returned status {}",
                response.status()
            )));
        }
        let root: Value = response
            .json()
            .await
            .map_err(|e| ProviderError::Parse(format!("Failed to parse Poe history: {e}")))?;
        let rows = history_rows(&root);
        for row in &rows {
            if let Some(entry) = parse_history_entry(row)
                && entry.date >= cutoff
            {
                entries.push(entry);
            }
        }

        let last_date = rows
            .iter()
            .rev()
            .find_map(|row| history_timestamp(row).and_then(parse_entry_date));
        if last_date.is_some_and(|date| date < cutoff) {
            break;
        }
        cursor = history_cursor(&root, &rows);
        if cursor.is_none() {
            break;
        }
    }
    Ok(entries)
}

fn snapshot_from_balance_at(value: &Value, updated_at: DateTime<Utc>) -> UsageSnapshot {
    let balance = first_number(
        value,
        &[
            "current_point_balance",
            "currentPointBalance",
            "balance",
            "points",
        ],
    );
    let mut primary = RateWindow::new(0.0);
    primary.reset_description = balance.map(|v| format!("Balance: {v:.0} points"));
    let label = primary
        .reset_description
        .clone()
        .unwrap_or_else(|| "Poe API".into());
    let mut snapshot = UsageSnapshot::new(primary).with_login_method(label);
    snapshot.updated_at = updated_at;
    snapshot
}

fn history_rows(root: &Value) -> Vec<&Value> {
    ["data", "items", "results"]
        .iter()
        .find_map(|key| root.get(*key).and_then(Value::as_array))
        .map(|rows| rows.iter().collect())
        .unwrap_or_default()
}

fn history_timestamp(row: &Value) -> Option<&Value> {
    ["creation_time", "timestamp", "created_at"]
        .iter()
        .find_map(|key| row.get(*key))
}

fn parse_entry_date(value: &Value) -> Option<DateTime<Utc>> {
    match value {
        Value::Number(number) => number.as_f64().and_then(timestamp_to_date),
        Value::String(raw) => {
            let raw = raw.trim();
            if raw.is_empty() {
                return None;
            }
            raw.parse::<f64>()
                .ok()
                .and_then(timestamp_to_date)
                .or_else(|| {
                    DateTime::parse_from_rfc3339(raw)
                        .ok()
                        .map(|date| date.with_timezone(&Utc))
                })
        }
        _ => None,
    }
}

fn timestamp_to_date(value: f64) -> Option<DateTime<Utc>> {
    if !value.is_finite() {
        return None;
    }
    let millis = if value > 100_000_000_000_000.0 {
        value / 1_000.0
    } else if value > 1_000_000_000_000.0 {
        value
    } else {
        value * 1_000.0
    };
    if !millis.is_finite() || millis < i64::MIN as f64 || millis > i64::MAX as f64 {
        return None;
    }
    #[allow(
        clippy::cast_possible_truncation,
        reason = "millis is finite and bounds-checked immediately above"
    )]
    DateTime::from_timestamp_millis(millis.round() as i64)
}

fn parse_history_entry(row: &Value) -> Option<PoeHistoryEntry> {
    let date = history_timestamp(row).and_then(parse_entry_date)?;
    let points = direct_number(row, &["cost_points", "points", "point_cost"])?;
    points.is_finite().then_some(PoeHistoryEntry {
        date,
        points: points.max(0.0),
    })
}

fn history_cursor(root: &Value, rows: &[&Value]) -> Option<String> {
    root.get("next_cursor")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|cursor| !cursor.is_empty())
        .map(str::to_owned)
        .or_else(|| {
            if root.get("has_more").and_then(Value::as_bool) == Some(true) {
                rows.last()
                    .and_then(|row| row.get("query_id"))
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|cursor| !cursor.is_empty())
                    .map(str::to_owned)
            } else {
                None
            }
        })
}

fn summarize_history(entries: &[PoeHistoryEntry], refresh_now: DateTime<Utc>) -> PoeHistorySummary {
    let cutoff = refresh_now - Duration::days(30);
    let mut daily = BTreeMap::<String, f64>::new();
    let mut total_points = 0.0;

    for entry in entries.iter().filter(|entry| entry.date >= cutoff) {
        let day = entry.date.format("%Y-%m-%d").to_string();
        *daily.entry(day.clone()).or_default() += entry.points;
        total_points += entry.points;
    }

    PoeHistorySummary {
        total_points,
        daily: daily
            .into_iter()
            .map(|(day, amount)| CostDailyPoint { day, amount })
            .collect(),
    }
}

fn cost_snapshot_from_history(
    entries: &[PoeHistoryEntry],
    balance: Option<f64>,
    refresh_now: DateTime<Utc>,
) -> Option<CostSnapshot> {
    let summary = summarize_history(entries, refresh_now);
    if summary.daily.is_empty() {
        return None;
    }
    let mut cost =
        CostSnapshot::new(summary.total_points, "points", "Last 30 days").with_daily(summary.daily);
    if let Some(balance) = balance {
        cost = cost.with_balance(balance);
    }
    cost.updated_at = refresh_now;
    Some(cost)
}

fn first_number(value: &Value, keys: &[&str]) -> Option<f64> {
    match value {
        Value::Object(map) => keys
            .iter()
            .find_map(|key| map.get(*key).and_then(value_as_number))
            .or_else(|| map.values().find_map(|value| first_number(value, keys))),
        Value::Array(items) => items.iter().find_map(|value| first_number(value, keys)),
        _ => None,
    }
}

fn direct_number(value: &Value, keys: &[&str]) -> Option<f64> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(value_as_number))
}

fn value_as_number(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str()?.trim().parse::<f64>().ok())
        .filter(|value| value.is_finite())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn parses_balance_label() {
        let snapshot = snapshot_from_balance_at(
            &serde_json::json!({"current_point_balance": 1234}),
            Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap(),
        );
        assert_eq!(
            snapshot.login_method.as_deref(),
            Some("Balance: 1234 points")
        );
    }

    #[test]
    fn history_uses_one_refresh_clock_for_midnight_bucket_and_retention() {
        let refresh_now = Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 30).unwrap();
        let entries = vec![
            PoeHistoryEntry {
                date: Utc.with_ymd_and_hms(2026, 8, 31, 23, 59, 59).unwrap(),
                points: 5.0,
            },
            PoeHistoryEntry {
                date: Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap(),
                points: 3.0,
            },
            PoeHistoryEntry {
                date: Utc.with_ymd_and_hms(2026, 8, 2, 0, 0, 29).unwrap(),
                points: 99.0,
            },
        ];

        let summary = summarize_history(&entries, refresh_now);
        let balance_snapshot = snapshot_from_balance_at(
            &serde_json::json!({"current_point_balance": 321}),
            refresh_now,
        );
        let cost = cost_snapshot_from_history(&entries, Some(321.0), refresh_now).unwrap();

        assert_eq!(balance_snapshot.updated_at, refresh_now);
        assert_eq!(cost.updated_at, refresh_now);
        assert_eq!(summary.total_points, 8.0);
        let today = refresh_now.format("%Y-%m-%d").to_string();
        assert_eq!(
            summary
                .daily
                .iter()
                .find(|point| point.day == today)
                .map(|point| point.amount),
            Some(3.0)
        );
        assert_eq!(
            summary.daily,
            vec![
                CostDailyPoint {
                    day: "2026-08-31".to_string(),
                    amount: 5.0,
                },
                CostDailyPoint {
                    day: "2026-09-01".to_string(),
                    amount: 3.0,
                },
            ]
        );
    }
}
