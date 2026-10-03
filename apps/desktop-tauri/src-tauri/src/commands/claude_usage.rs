use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use codexbar::core::{ProviderError, RateWindow, UsageSnapshot};
use codexbar::providers::claude::accounts::{self, ClaudeAccount};
use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager};

use crate::state::AppState;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeAccountWindow {
    pub used_percent: f64,
    pub resets_at: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeAccountUsage {
    pub five_hour: Option<ClaudeAccountWindow>,
    pub seven_day: Option<ClaudeAccountWindow>,
    pub updated_at: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeAccountUsageState {
    pub usage: Option<ClaudeAccountUsage>,
    pub usage_error: Option<String>,
    #[serde(default)]
    pub needs_authentication: bool,
}

#[derive(Serialize, Deserialize)]
pub struct ClaudeAccountState {
    #[serde(flatten)]
    pub account: ClaudeAccount,
    #[serde(flatten)]
    pub usage: ClaudeAccountUsageState,
}

#[tauri::command]
pub fn get_claude_accounts_state(
    state: tauri::State<'_, Mutex<AppState>>,
) -> Result<Vec<ClaudeAccountState>, String> {
    if state
        .lock()
        .map_err(|e| e.to_string())?
        .proof_config
        .is_some()
        && let Some(accounts) = crate::proof_harness::seed_claude_accounts_from_env()
    {
        return Ok(accounts);
    }
    let accounts = super::claude_accounts_list()?;
    let state = state.lock().map_err(|e| e.to_string())?;
    Ok(accounts
        .into_iter()
        .map(|account| {
            let mut usage = state
                .claude_account_usage
                .get(&account.id)
                .cloned()
                .unwrap_or_default();
            if !codexbar::settings::Settings::load().claude_allow_reading_claude_code_credentials {
                usage = ClaudeAccountUsageState { usage: None, usage_error: Some(
                    "Enable Allow reading Claude Code's credentials in Settings to check saved accounts.".into()
                ), needs_authentication: false };
            }
            ClaudeAccountState { account, usage }
        })
        .collect())
}

fn quota_window(window: &RateWindow) -> Option<ClaudeAccountWindow> {
    (!window.is_informational && window.used_percent.is_finite()).then(|| ClaudeAccountWindow {
        used_percent: window.used_percent.clamp(0.0, 100.0),
        resets_at: window.resets_at.map(|at| at.to_rfc3339()),
    })
}

fn update_usage(
    previous: &mut ClaudeAccountUsageState,
    result: Result<UsageSnapshot, ProviderError>,
) {
    match result {
        Ok(usage) => {
            previous.usage = Some(ClaudeAccountUsage {
                five_hour: quota_window(&usage.primary),
                seven_day: usage.secondary.as_ref().and_then(quota_window),
                updated_at: usage.updated_at.to_rfc3339(),
            });
            previous.usage_error = None;
            previous.needs_authentication = false;
        }
        Err(error) => {
            previous.needs_authentication = matches!(
                error,
                ProviderError::OAuthRevoked(_)
                    | ProviderError::OAuthExpired(_)
                    | ProviderError::AuthRequired
                    | ProviderError::NoCookies
            );
            // Never expose response bodies or credentials through this bridge.
            previous.usage_error = Some(
                if previous.needs_authentication {
                    "Sign in again to check this Claude account."
                } else {
                    "Claude usage check failed. Will retry automatically."
                }
                .into(),
            );
        }
    }
}

fn publish_results(
    state: &mut AppState,
    generation: u64,
    live_accounts: &[ClaudeAccount],
    results: Vec<(String, Result<UsageSnapshot, ProviderError>)>,
) -> bool {
    if state.provider_refresh_generation != generation {
        return false;
    }
    let live_ids: HashMap<_, _> = live_accounts.iter().map(|a| (a.id.as_str(), a)).collect();
    state
        .claude_account_usage
        .retain(|id, _| live_ids.contains_key(id.as_str()));
    for (id, result) in results {
        if live_ids.contains_key(id.as_str()) {
            update_usage(state.claude_account_usage.entry(id).or_default(), result);
        }
    }
    true
}

/// Share the provider refresh cadence and permits; each request uses only the
/// selected account's OAuth credentials and leaves the active CLI login intact.
pub(crate) async fn refresh_claude_account_lanes(
    app: tauri::AppHandle,
    permits: Arc<tokio::sync::Semaphore>,
    generation: u64,
) {
    if !codexbar::settings::Settings::load().claude_allow_reading_claude_code_credentials {
        return;
    }
    let Ok(accounts) = super::claude_accounts_list() else {
        return;
    };
    let mut workers = Vec::new();
    for account in accounts {
        let permits = Arc::clone(&permits);
        workers.push(tokio::spawn(async move {
            let Ok(_permit) = permits.acquire_owned().await else {
                return None;
            };
            // The worker owns the credential operation until renewal and
            // persistence finish. Refresh rotation is never cancelled by a UI.
            let result = accounts::fetch_usage(&account.id)
                .await
                .map(|result| result.usage);
            Some((account.id, result))
        }));
    }
    let mut results = Vec::new();
    for worker in workers {
        if let Ok(Some(result)) = worker.await {
            results.push(result);
        }
    }
    let Ok(accounts) = super::claude_accounts_list() else {
        return;
    };
    let state = app.state::<Mutex<AppState>>();
    let published = state
        .lock()
        .is_ok_and(|mut state| publish_results(&mut state, generation, &accounts, results));
    if published {
        let _ = app.emit("claude-accounts-updated", ());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generic_api_errors_do_not_request_sign_in() {
        let mut state = ClaudeAccountUsageState::default();
        update_usage(
            &mut state,
            Err(ProviderError::OAuth("API error 500".into())),
        );
        assert!(!state.usage_error.unwrap().contains("Sign in"));
        assert!(!state.needs_authentication);
    }

    #[test]
    fn failures_preserve_last_good_limits_and_recovery_clears_error() {
        let mut state = ClaudeAccountUsageState::default();
        let usage = UsageSnapshot::new(RateWindow::new(23.0));
        update_usage(&mut state, Ok(usage.clone()));
        update_usage(
            &mut state,
            Err(ProviderError::OAuthExpired("expired".into())),
        );
        assert_eq!(
            state
                .usage
                .as_ref()
                .unwrap()
                .five_hour
                .as_ref()
                .unwrap()
                .used_percent,
            23.0
        );
        assert!(state.usage_error.as_ref().unwrap().contains("Sign in"));
        assert!(state.needs_authentication);
        update_usage(&mut state, Ok(usage));
        assert!(state.usage_error.is_none());
        assert!(!state.needs_authentication);
    }

    #[test]
    fn removed_accounts_and_superseded_generations_cannot_publish() {
        let mut state = AppState::new();
        let generation = state.provider_refresh_generation;
        state.provider_refresh_generation += 1;
        assert!(!publish_results(&mut state, generation, &[], vec![]));
        let generation = state.provider_refresh_generation;
        assert!(publish_results(
            &mut state,
            generation,
            &[],
            vec![(
                "removed".into(),
                Ok(UsageSnapshot::new(RateWindow::new(10.0)))
            )]
        ));
        assert!(state.claude_account_usage.is_empty());
    }

    #[test]
    fn missing_session_is_unavailable_instead_of_zero_usage() {
        assert!(quota_window(&RateWindow::no_active_session()).is_none());
    }
}
