use std::sync::Mutex;
use std::time::Duration;
#[cfg(test)]
use std::time::Instant;

use codexbar::settings::Settings;
use tauri::Manager;

use crate::state::AppState;

const AUTO_REFRESH_POLL_INTERVAL: Duration = Duration::from_secs(15);

pub fn install(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            if should_refresh(&app) {
                if crate::commands::do_refresh_providers_if_stale(&app)
                    .await
                    .is_ok()
                {
                    refresh_powertoys_local_usage_cache().await;
                }
            }
            tokio::time::sleep(AUTO_REFRESH_POLL_INTERVAL).await;
        }
    });
}

async fn refresh_powertoys_local_usage_cache() {
    let settings = Settings::load();
    let provider_ids = powertoys_local_usage_provider_ids(&settings);
    crate::commands::refresh_provider_local_usage_cache(provider_ids).await;
}

fn powertoys_local_usage_provider_ids(settings: &Settings) -> Vec<String> {
    if !settings.powertoys_status_pipe_enabled {
        return Vec::new();
    }

    settings
        .get_enabled_provider_ids()
        .into_iter()
        .map(|provider| provider.cli_name().to_string())
        .filter(|provider_id| matches!(provider_id.as_str(), "codex" | "claude"))
        .collect()
}

fn should_refresh(app: &tauri::AppHandle) -> bool {
    let settings = Settings::load();
    let Some(interval) = refresh_interval(settings.refresh_interval_secs) else {
        return false;
    };

    let state = app.state::<Mutex<AppState>>();
    state
        .lock()
        .map(|guard| should_refresh_from_state(&guard, interval))
        .unwrap_or(false)
}

fn refresh_interval(seconds: u64) -> Option<Duration> {
    (seconds > 0).then(|| Duration::from_secs(seconds))
}

fn should_refresh_from_state(state: &AppState, interval: Duration) -> bool {
    if state.is_refreshing {
        return false;
    }
    match state.provider_cache_updated_at {
        Some(updated_at) => updated_at.elapsed() >= interval,
        None => true,
    }
}

#[cfg(test)]
pub(crate) fn should_refresh_from_values(
    is_refreshing: bool,
    updated_at: Option<Instant>,
    interval_secs: u64,
) -> bool {
    let Some(interval) = refresh_interval(interval_secs) else {
        return false;
    };
    if is_refreshing {
        return false;
    }
    updated_at
        .map(|updated| updated.elapsed() >= interval)
        .unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_refresh_setting_disables_background_refresh() {
        assert!(!should_refresh_from_values(false, None, 0));
    }

    #[test]
    fn missing_cache_triggers_background_refresh() {
        assert!(should_refresh_from_values(false, None, 300));
    }

    #[test]
    fn fresh_cache_does_not_refresh_before_interval() {
        assert!(!should_refresh_from_values(
            false,
            Some(Instant::now() - Duration::from_secs(299)),
            300,
        ));
    }

    #[test]
    fn stale_cache_refreshes_after_configured_interval() {
        assert!(should_refresh_from_values(
            false,
            Some(Instant::now() - Duration::from_secs(300)),
            300,
        ));
    }

    #[test]
    fn active_refresh_blocks_overlapping_background_refresh() {
        assert!(!should_refresh_from_values(true, None, 300));
    }

    #[test]
    fn powertoys_local_usage_refresh_only_includes_supported_enabled_providers() {
        let mut settings = Settings::default();
        assert!(powertoys_local_usage_provider_ids(&settings).is_empty());

        settings.powertoys_status_pipe_enabled = true;
        settings.enabled_providers = ["codex".to_string(), "cursor".to_string()]
            .into_iter()
            .collect();

        assert_eq!(
            powertoys_local_usage_provider_ids(&settings),
            vec!["codex".to_string()]
        );
    }
}
