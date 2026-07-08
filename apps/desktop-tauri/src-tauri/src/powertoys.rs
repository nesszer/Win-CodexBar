use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use chrono::Utc;
use serde::Serialize;
use tauri::Manager;

use crate::commands::{
    ProviderLocalUsageSummary, ProviderUsageSnapshot, RateWindowSnapshot,
    load_provider_local_usage_summary,
};
use crate::state::AppState;

pub const STATUS_PIPE_NAME: &str = r"\\.\pipe\WinCodexBar.Status";
const LOCAL_USAGE_TTL: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PowerToysSnapshot {
    version: u32,
    updated_at: String,
    providers: Vec<PowerToysProviderSnapshot>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PowerToysProviderSnapshot {
    id: String,
    name: String,
    status_text: String,
    subtitle: Option<String>,
    primary_label: Option<String>,
    primary: RateWindowSnapshot,
    secondary_label: Option<String>,
    secondary: Option<RateWindowSnapshot>,
    today_cost: Option<f64>,
    thirty_day_cost: Option<f64>,
    latest_tokens: Option<u64>,
    thirty_day_tokens: Option<u64>,
    top_model: Option<String>,
    plan_name: Option<String>,
    account_email: Option<String>,
    updated_at: String,
    error: Option<String>,
}

#[cfg(not(windows))]
pub fn install(_app: tauri::AppHandle) {}

#[cfg(windows)]
pub fn install(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        run_status_pipe(app).await;
    });
}

pub fn snapshot(app: &tauri::AppHandle) -> PowerToysSnapshot {
    let providers = app
        .state::<Mutex<AppState>>()
        .lock()
        .map(|guard| guard.provider_cache.clone())
        .unwrap_or_default()
        .into_iter()
        .map(provider_snapshot)
        .collect();

    PowerToysSnapshot {
        version: 1,
        updated_at: Utc::now().to_rfc3339(),
        providers,
    }
}

fn provider_snapshot(provider: ProviderUsageSnapshot) -> PowerToysProviderSnapshot {
    let local_usage = cached_local_usage(&provider.provider_id);
    let status_text = if provider.error.is_some() {
        "error".to_string()
    } else {
        format!("{}%", provider.primary.used_percent.round())
    };
    let subtitle = provider_subtitle(&provider, local_usage.as_ref());

    PowerToysProviderSnapshot {
        id: provider.provider_id,
        name: provider.display_name,
        status_text,
        subtitle,
        primary_label: provider.primary_label,
        primary: provider.primary,
        secondary_label: provider.secondary_label,
        secondary: provider.secondary,
        today_cost: local_usage.as_ref().and_then(|summary| summary.today_cost),
        thirty_day_cost: local_usage
            .as_ref()
            .and_then(|summary| summary.thirty_day_cost),
        latest_tokens: local_usage.as_ref().and_then(|summary| summary.latest_tokens),
        thirty_day_tokens: local_usage
            .as_ref()
            .and_then(|summary| summary.thirty_day_tokens),
        top_model: local_usage.and_then(|summary| summary.top_model),
        plan_name: provider.plan_name,
        account_email: provider.account_email,
        updated_at: provider.updated_at,
        error: provider.error,
    }
}

fn provider_subtitle(
    provider: &ProviderUsageSnapshot,
    local_usage: Option<&ProviderLocalUsageSummary>,
) -> Option<String> {
    let mut parts = Vec::new();
    if let (Some(label), Some(secondary)) = (&provider.secondary_label, &provider.secondary) {
        parts.push(format!("{} {}%", label, secondary.used_percent.round()));
    }
    if let Some(cost) = local_usage.and_then(|summary| summary.today_cost) {
        parts.push(format!("Today ${cost:.2}"));
    }
    if parts.is_empty() {
        provider
            .primary
            .reset_description
            .as_ref()
            .map(|reset| reset.to_string())
    } else {
        Some(parts.join(" · "))
    }
}

struct CachedLocalUsage {
    loaded_at: Instant,
    summary: Option<ProviderLocalUsageSummary>,
}

fn local_usage_cache() -> &'static Mutex<HashMap<String, CachedLocalUsage>> {
    static CACHE: OnceLock<Mutex<HashMap<String, CachedLocalUsage>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn cached_local_usage(provider_id: &str) -> Option<ProviderLocalUsageSummary> {
    let cache = local_usage_cache();
    if let Ok(guard) = cache.lock()
        && let Some(entry) = guard.get(provider_id)
        && entry.loaded_at.elapsed() <= LOCAL_USAGE_TTL
    {
        return entry.summary.clone();
    }

    let summary = load_provider_local_usage_summary(provider_id);
    if let Ok(mut guard) = cache.lock() {
        guard.insert(
            provider_id.to_string(),
            CachedLocalUsage {
                loaded_at: Instant::now(),
                summary: summary.clone(),
            },
        );
    }
    summary
}

#[cfg(windows)]
async fn run_status_pipe(app: tauri::AppHandle) {
    use tokio::io::AsyncWriteExt;
    use tokio::net::windows::named_pipe::ServerOptions;

    loop {
        let server = match ServerOptions::new().create(STATUS_PIPE_NAME) {
            Ok(server) => server,
            Err(err) => {
                tracing::warn!("failed to create PowerToys status pipe: {err}");
                tokio::time::sleep(Duration::from_secs(2)).await;
                continue;
            }
        };

        if let Err(err) = server.connect().await {
            tracing::warn!("PowerToys status pipe connection failed: {err}");
            continue;
        }

        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            let mut server = server;
            let payload = serde_json::to_vec(&snapshot(&app)).unwrap_or_else(|err| {
                tracing::warn!("failed to serialize PowerToys snapshot: {err}");
                b"{\"version\":1,\"providers\":[]}".to_vec()
            });
            let _ = server.write_all(&payload).await;
            let _ = server.write_all(b"\n").await;
            let _ = server.flush().await;
        });
    }
}
