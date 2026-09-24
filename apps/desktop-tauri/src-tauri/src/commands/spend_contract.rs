//! Upstream 0.53 Usage & Spend accounting bridge.

use std::sync::Mutex;

use codexbar::cost_scanner::CostScanner;
use codexbar::settings::Settings;
use codexbar::spend_contract::{SpendContract, build_local_spend_contract_from_summary};
use tauri::State;

use crate::state::AppState;

#[tauri::command]
pub async fn get_spend_contract(
    state: State<'_, Mutex<AppState>>,
    provider_id: String,
    history_days: Option<u32>,
    include_open_codex: Option<bool>,
) -> Result<SpendContract, String> {
    let containment_proof = state
        .lock()
        .map_err(|_| "app state lock is poisoned".to_string())?
        .is_containment_proof();
    if containment_proof {
        return Err("spend contract is unavailable during containment proof".to_string());
    }

    let provider = provider_id.trim().to_ascii_lowercase();
    if !matches!(provider.as_str(), "codex" | "claude" | "pi" | "opencodego") {
        return Err(format!(
            "Spend contract is unavailable for provider: {provider}"
        ));
    }
    let days = history_days.unwrap_or(30);
    let include_import = include_open_codex.unwrap_or(false) && provider == "codex";
    tauri::async_runtime::spawn_blocking(move || {
        let history_days = if days == 0 { 365 } else { days.clamp(1, 365) };
        let scanner = CostScanner::new(history_days);
        let summary = match provider.as_str() {
            "codex" => scanner.scan_codex(),
            "claude" => scanner.scan_claude(),
            "pi" => scanner.scan_pi(),
            "opencodego" => scanner.scan_opencodego_with_cancel(None),
            _ => unreachable!(),
        };
        let settings = Settings::load();
        build_local_spend_contract_from_summary(
            &provider,
            history_days,
            include_import,
            settings.hide_native_codex_cost_when_open_codex_present && provider == "codex",
            settings.hide_personal_info,
            summary,
        )
    })
    .await
    .map_err(|error| format!("spend contract worker failed: {error}"))
}
