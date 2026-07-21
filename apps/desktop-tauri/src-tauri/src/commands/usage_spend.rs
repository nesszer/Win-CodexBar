//! Usage & Spend settings tab: 7-day / 30-day local cost aggregates.

use codexbar::cost_scanner::CostScanner;
use serde::Serialize;
use tauri::State;

use super::ProviderUsageSnapshot;
use crate::state::AppState;
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageSpendRow {
    pub provider_id: String,
    pub display_name: String,
    pub seven_day: Option<f64>,
    pub thirty_day: Option<f64>,
    pub currency: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageSpendSummary {
    pub rows: Vec<UsageSpendRow>,
}

#[tauri::command]
pub async fn get_usage_spend_summary(
    state: State<'_, Mutex<AppState>>,
) -> Result<UsageSpendSummary, String> {
    let cached = {
        let guard = state.lock().map_err(|e| e.to_string())?;
        guard.provider_cache.clone()
    };

    tauri::async_runtime::spawn_blocking(move || build_usage_spend_summary(&cached))
        .await
        .map_err(|e| format!("usage spend worker failed: {e}"))
}

fn build_usage_spend_summary(cached: &[ProviderUsageSnapshot]) -> UsageSpendSummary {
    let mut rows = Vec::new();

    // Local JSONL scanners for Codex / Claude (primary spend sources).
    let codex_7 = CostScanner::new(7).scan_codex().total_cost_usd;
    let codex_30 = CostScanner::new(30).scan_codex().total_cost_usd;
    rows.push(UsageSpendRow {
        provider_id: "codex".into(),
        display_name: "Codex".into(),
        seven_day: Some(codex_7),
        thirty_day: Some(codex_30),
        currency: "USD".into(),
        source: "local logs".into(),
    });

    let claude_7 = CostScanner::new(7).scan_claude().total_cost_usd;
    let claude_30 = CostScanner::new(30).scan_claude().total_cost_usd;
    rows.push(UsageSpendRow {
        provider_id: "claude".into(),
        display_name: "Claude".into(),
        seven_day: Some(claude_7),
        thirty_day: Some(claude_30),
        currency: "USD".into(),
        source: "local logs".into(),
    });

    // Surface any other provider cost snapshots from the last refresh (period
    // costs, not calendar 7d/30d — shown under thirty_day only).
    for snapshot in cached {
        if snapshot.provider_id == "codex" || snapshot.provider_id == "claude" {
            continue;
        }
        let Some(cost) = &snapshot.cost else {
            continue;
        };
        rows.push(UsageSpendRow {
            provider_id: snapshot.provider_id.clone(),
            display_name: if snapshot.display_name.is_empty() {
                snapshot.provider_id.clone()
            } else {
                snapshot.display_name.clone()
            },
            seven_day: None,
            thirty_day: Some(cost.used),
            currency: cost.currency_code.clone(),
            source: format!("period ({})", cost.period),
        });
    }

    UsageSpendSummary { rows }
}
