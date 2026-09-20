//! Windsurf provider implementation.
//!
//! Reads Windsurf's cached plan information from its VS Code-style SQLite
//! state database. On Windows this defaults to
//! `%APPDATA%\Windsurf\User\globalStorage\state.vscdb`.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OpenFlags, types::Value as SqlValue};
use serde::Deserialize;
use std::path::PathBuf;

use crate::core::{
    FetchContext, Provider, ProviderError, ProviderFetchResult, ProviderId, ProviderMetadata,
    RateWindow, SourceMode, UsageSnapshot,
};

const STATE_KEY: &str = "windsurf.settings.cachedPlanInfo";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CachedPlanInfo {
    plan_name: Option<String>,
    end_timestamp: Option<i64>,
    usage: Option<WindsurfUsage>,
    quota_usage: Option<QuotaUsage>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WindsurfUsage {
    messages: Option<i64>,
    used_messages: Option<i64>,
    remaining_messages: Option<i64>,
    flow_actions: Option<i64>,
    used_flow_actions: Option<i64>,
    remaining_flow_actions: Option<i64>,
    flex_credits: Option<i64>,
    used_flex_credits: Option<i64>,
    remaining_flex_credits: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct QuotaUsage {
    daily_remaining_percent: Option<f64>,
    weekly_remaining_percent: Option<f64>,
    daily_reset_at_unix: Option<i64>,
    weekly_reset_at_unix: Option<i64>,
}

pub struct WindsurfProvider {
    metadata: ProviderMetadata,
}

impl WindsurfProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: ProviderId::Windsurf,
                display_name: "Windsurf",
                session_label: "Daily",
                weekly_label: "Weekly",
                supports_opus: false,
                supports_credits: false,
                default_enabled: false,
                is_primary: false,
                dashboard_url: Some("https://windsurf.com/subscription"),
                status_page_url: Some("https://status.windsurf.com"),
                tertiary_label_key: None,
            },
        }
    }

    fn default_db_path() -> Option<PathBuf> {
        if let Ok(path) = std::env::var("WINDSURF_STATE_DB")
            && !path.trim().is_empty()
        {
            return Some(PathBuf::from(path));
        }
        dirs::data_dir().map(|base| {
            base.join("Windsurf")
                .join("User")
                .join("globalStorage")
                .join("state.vscdb")
        })
    }

    fn read_cached_plan(db_path: PathBuf) -> Result<CachedPlanInfo, ProviderError> {
        if !db_path.exists() {
            return Err(ProviderError::NotInstalled(format!(
                "Windsurf database not found at {}. Open Windsurf and sign in first.",
                db_path.display()
            )));
        }

        let conn = Connection::open_with_flags(&db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|e| ProviderError::Other(format!("Failed to open Windsurf database: {e}")))?;
        conn.busy_timeout(std::time::Duration::from_millis(250))
            .map_err(|e| {
                ProviderError::Other(format!("Failed to configure SQLite timeout: {e}"))
            })?;

        let value: SqlValue = conn
            .query_row(
                "SELECT value FROM ItemTable WHERE key = ?1 LIMIT 1",
                [STATE_KEY],
                |row| row.get(0),
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => ProviderError::AuthRequired,
                other => {
                    ProviderError::Other(format!("Failed to read Windsurf plan cache: {other}"))
                }
            })?;

        let json_text = decode_json_value(value).ok_or_else(|| {
            ProviderError::Parse("Windsurf plan cache is not valid UTF-8/UTF-16 JSON".to_string())
        })?;

        serde_json::from_str::<CachedPlanInfo>(&json_text)
            .map_err(|e| ProviderError::Parse(format!("Failed to parse Windsurf plan cache: {e}")))
    }

    fn snapshot_from_plan(plan: CachedPlanInfo) -> UsageSnapshot {
        let mut primary = None;
        let mut secondary = None;

        if let Some(quota) = plan.quota_usage {
            if let Some(daily) = quota.daily_remaining_percent {
                primary = Some(window_from_remaining_percent(
                    daily,
                    quota.daily_reset_at_unix,
                ));
            }
            if let Some(weekly) = quota.weekly_remaining_percent {
                secondary = Some(window_from_remaining_percent(
                    weekly,
                    quota.weekly_reset_at_unix,
                ));
            }
        }

        if let Some(usage) = plan.usage {
            if primary.is_none() {
                primary = usage_window(
                    usage.used_messages,
                    usage.remaining_messages,
                    usage.messages,
                    "messages",
                );
            }
            if secondary.is_none() {
                secondary = usage_window(
                    usage.used_flow_actions,
                    usage.remaining_flow_actions,
                    usage.flow_actions,
                    "flow actions",
                );
            }
            if let Some(flex) = usage_window(
                usage.used_flex_credits,
                usage.remaining_flex_credits,
                usage.flex_credits,
                "flex credits",
            ) {
                let had_primary = primary.is_some();
                let mut snapshot = UsageSnapshot::new(primary.unwrap_or_else(|| flex.clone()));
                if let Some(secondary) = secondary {
                    snapshot = snapshot.with_secondary(secondary);
                }
                if had_primary {
                    snapshot = snapshot.with_extra_rate_window("flex", "Flex Credits", flex);
                }
                return with_identity(snapshot, plan.plan_name, plan.end_timestamp);
            }
        }

        let mut snapshot = UsageSnapshot::new(primary.unwrap_or_else(|| {
            let mut window = RateWindow::new(0.0);
            window.reset_description = Some("No cached Windsurf quota details".to_string());
            window
        }));
        if let Some(secondary) = secondary {
            snapshot = snapshot.with_secondary(secondary);
        }
        with_identity(snapshot, plan.plan_name, plan.end_timestamp)
    }
}

impl Default for WindsurfProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for WindsurfProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Windsurf
    }

    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        match ctx.source_mode {
            SourceMode::Auto | SourceMode::Cli => {
                let path = Self::default_db_path().ok_or_else(|| {
                    ProviderError::NotInstalled(
                        "Could not resolve Windsurf application data path.".to_string(),
                    )
                })?;
                let plan = Self::read_cached_plan(path)?;
                Ok(ProviderFetchResult::new(
                    Self::snapshot_from_plan(plan),
                    "local cache",
                ))
            }
            SourceMode::OAuth | SourceMode::Web => {
                Err(ProviderError::UnsupportedSource(ctx.source_mode))
            }
        }
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::Cli]
    }

    fn supports_cli(&self) -> bool {
        true
    }
}

fn decode_json_value(value: SqlValue) -> Option<String> {
    match value {
        SqlValue::Text(text) => valid_json_text(&text),
        SqlValue::Blob(bytes) => decode_json_blob(&bytes),
        _ => None,
    }
}

fn decode_json_blob(value: &[u8]) -> Option<String> {
    if let Ok(text) = std::str::from_utf8(value)
        && let Some(json) = valid_json_text(text)
    {
        return Some(json);
    }

    if value.len().is_multiple_of(2) {
        let utf16: Vec<u16> = value
            .as_chunks::<2>()
            .0
            .iter()
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .collect();
        if let Ok(text) = String::from_utf16(&utf16)
            && let Some(json) = valid_json_text(&text)
        {
            return Some(json);
        }
    }

    None
}

fn valid_json_text(text: &str) -> Option<String> {
    let trimmed = text.trim_matches(char::from(0)).trim();
    if serde_json::from_str::<serde_json::Value>(trimmed).is_ok() {
        return Some(trimmed.to_string());
    }
    None
}

fn window_from_remaining_percent(remaining: f64, reset_unix: Option<i64>) -> RateWindow {
    let resets_at = reset_unix.and_then(|ts| DateTime::<Utc>::from_timestamp(ts, 0));
    // Reset countdowns are localized at render time; keep cached snapshots language-neutral.
    RateWindow::with_details((100.0 - remaining).clamp(0.0, 100.0), None, resets_at, None)
}

fn usage_window(
    raw_used: Option<i64>,
    raw_remaining: Option<i64>,
    raw_total: Option<i64>,
    unit: &str,
) -> Option<RateWindow> {
    let total = raw_total?;
    if total <= 0 {
        return None;
    }
    let used = raw_used.or_else(|| raw_remaining.map(|remaining| total - remaining))?;
    let clamped_used = used.clamp(0, total);
    let percent = clamped_used as f64 / total as f64 * 100.0;
    let mut window = RateWindow::new(percent);
    window.reset_description = Some(format!("{clamped_used} / {total} {unit}"));
    Some(window)
}

fn with_identity(
    mut snapshot: UsageSnapshot,
    plan_name: Option<String>,
    end_timestamp: Option<i64>,
) -> UsageSnapshot {
    if let Some(plan) = plan_name.filter(|v| !v.trim().is_empty()) {
        snapshot = snapshot.with_login_method(plan);
    }
    if let Some(end) = end_timestamp.and_then(DateTime::<Utc>::from_timestamp_millis) {
        snapshot = snapshot.with_organization(format!("Expires {}", end.format("%Y-%m-%d")));
    }
    snapshot
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_snapshot_from_quota_usage() {
        let snapshot = WindsurfProvider::snapshot_from_plan(CachedPlanInfo {
            plan_name: Some("Pro".into()),
            end_timestamp: Some(1_800_000_000_000),
            usage: None,
            quota_usage: Some(QuotaUsage {
                daily_remaining_percent: Some(80.0),
                weekly_remaining_percent: Some(25.0),
                daily_reset_at_unix: None,
                weekly_reset_at_unix: None,
            }),
        });

        assert_eq!(snapshot.primary.used_percent, 20.0);
        assert_eq!(
            snapshot.secondary.as_ref().map(|w| w.used_percent),
            Some(75.0)
        );
        assert_eq!(snapshot.login_method.as_deref(), Some("Pro"));
    }

    #[test]
    fn decodes_utf16_state_value() {
        let json = r#"{"planName":"Free"}"#;
        let bytes: Vec<u8> = json.encode_utf16().flat_map(|c| c.to_le_bytes()).collect();

        assert_eq!(decode_json_blob(&bytes).as_deref(), Some(json));
    }

    #[test]
    fn decodes_sql_text_state_value() {
        let json = r#"{"planName":"Teams"}"#;

        assert_eq!(
            decode_json_value(SqlValue::Text(json.to_string())).as_deref(),
            Some(json)
        );
    }

    #[test]
    fn quota_window_with_resets_at_has_no_pre_localized_description() {
        let future = Utc::now() + chrono::Duration::hours(4);
        let window = window_from_remaining_percent(80.0, Some(future.timestamp()));

        assert!(window.resets_at.is_some());
        assert!(
            window.reset_description.is_none(),
            "reset_description should be None when resets_at is present, got {:?}",
            window.reset_description
        );
    }

    #[test]
    fn usage_window_keeps_raw_unit_description() {
        let window = usage_window(Some(5), Some(5), Some(10), "messages").unwrap();

        assert_eq!(window.reset_description.as_deref(), Some("5 / 10 messages"));
    }
}
