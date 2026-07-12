use std::sync::OnceLock;
use std::time::{Duration, Instant};

use codexbar::settings::Settings;

const AUTO_REFRESH_POLL_INTERVAL: Duration = Duration::from_secs(15);

pub fn install(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut schedule: Option<(Duration, Instant)> = None;
        loop {
            let interval = refresh_interval(Settings::load().refresh_interval_secs);
            match interval {
                None => schedule = None,
                Some(interval) => {
                    let now = Instant::now();
                    let scheduled_at = schedule
                        .filter(|(scheduled_interval, _)| *scheduled_interval == interval)
                        .map(|(_, scheduled_at)| scheduled_at)
                        .unwrap_or(now);
                    if now >= scheduled_at {
                        let _ = crate::commands::do_refresh_providers_if_stale(&app).await;
                        schedule = Some((
                            interval,
                            next_fixed_tick(scheduled_at, Instant::now(), interval),
                        ));
                    }
                }
            }
            tokio::time::sleep(AUTO_REFRESH_POLL_INTERVAL).await;
        }
    });
}

fn next_fixed_tick(
    previous_scheduled_at: Instant,
    completed_at: Instant,
    interval: Duration,
) -> Instant {
    let mut scheduled_at = previous_scheduled_at + interval;
    while scheduled_at <= completed_at {
        scheduled_at += interval;
    }
    scheduled_at
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

pub(crate) fn schedule_refresh_enrichment(settings: &Settings) {
    let provider_ids = powertoys_local_usage_provider_ids(settings);
    if provider_ids.is_empty() {
        return;
    }
    tauri::async_runtime::spawn(async move {
        static ENRICHMENT: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
        let _guard = ENRICHMENT
            .get_or_init(|| tokio::sync::Mutex::new(()))
            .lock()
            .await;
        crate::commands::refresh_provider_local_usage_cache(provider_ids).await;
    });
}

fn refresh_interval(seconds: u64) -> Option<Duration> {
    (seconds > 0).then(|| Duration::from_secs(seconds))
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
    fn fixed_cadence_advances_from_the_scheduled_tick() {
        let start = Instant::now();
        let interval = Duration::from_secs(100);
        let first_tick = start + interval;

        assert_eq!(
            next_fixed_tick(first_tick, first_tick + Duration::from_secs(60), interval),
            start + Duration::from_secs(200)
        );
        assert_eq!(
            next_fixed_tick(first_tick, first_tick + Duration::from_secs(260), interval),
            start + Duration::from_secs(400)
        );
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
