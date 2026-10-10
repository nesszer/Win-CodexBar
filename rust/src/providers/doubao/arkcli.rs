//! `arkcli usage plan` fallback for Doubao Coding Plan usage (upstream 0.45 #2221).

use chrono::{DateTime, TimeZone, Utc};
use serde::Deserialize;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use super::{CodingPlanQuota, CodingPlanResult, coding_plan_snapshot};
use crate::core::{ProviderError, UsageSnapshot};

#[derive(Debug, Deserialize)]
struct ArkcliUsageResponse {
    #[serde(default)]
    viewer: Option<ArkcliViewer>,
    #[serde(default)]
    items: Vec<ArkcliUsageItem>,
}

#[derive(Debug, Deserialize)]
struct ArkcliViewer {
    #[serde(default, rename = "auth_method")]
    auth_method: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ArkcliUsageItem {
    product: String,
    #[serde(default)]
    subscribed: Option<bool>,
    #[serde(default)]
    periods: Option<Vec<ArkcliPeriod>>,
    #[serde(default, rename = "updated_at")]
    updated_at: Option<f64>,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ArkcliPeriod {
    label: String,
    percent: f64,
    #[serde(default, rename = "reset_at")]
    reset_at: Option<String>,
}

pub(super) fn resolve_arkcli_binary() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("ARKCLI_PATH") {
        let p = PathBuf::from(path.trim());
        if p.is_file() {
            return Some(p);
        }
    }
    which::which("arkcli").ok()
}

fn is_arkcli_auth_error(message: &str) -> bool {
    let n = message.to_ascii_lowercase();
    [
        "not logged in",
        "not authenticated",
        "authentication required",
        "login required",
        "please login",
        "please log in",
    ]
    .iter()
    .any(|s| n.contains(s))
}

fn run_arkcli_usage_plan() -> Result<Vec<u8>, ProviderError> {
    let bin = resolve_arkcli_binary().ok_or_else(|| {
        ProviderError::NotInstalled(
            "arkcli was not found. Install arkcli, run 'arkcli auth login', or configure Doubao API credentials."
                .into(),
        )
    })?;
    let mut command = Command::new(&bin);
    command
        .args(["usage", "plan", "--format", "json"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    // Runs during background refreshes: keep the CLI's console window hidden
    // so it does not flash up or take focus.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = command
        .spawn()
        .map_err(|e| ProviderError::Other(format!("Failed to launch arkcli: {e}")))?;

    // ponytail: 15s wall-clock via join timeout isn't available on std Command;
    // kill after wait timeout via a simple timed poll loop.
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if std::time::Instant::now() >= deadline => {
                // Best-effort teardown of the timed-out child; the outcome is already
                // reported as timed out.
                let _killed = child.kill();
                let _reaped = child.wait();
                return Err(ProviderError::Other(
                    "arkcli usage timed out. Check arkcli authentication and try again.".into(),
                ));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => {
                return Err(ProviderError::Other(format!("arkcli wait failed: {e}")));
            }
        }
    }
    let output = child
        .wait_with_output()
        .map_err(|e| ProviderError::Other(format!("arkcli wait failed: {e}")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let message = stderr.split_whitespace().collect::<Vec<_>>().join(" ");
        if is_arkcli_auth_error(&message) {
            return Err(ProviderError::AuthRequired);
        }
        let code = output.status.code().unwrap_or(-1);
        return Err(ProviderError::Other(format!(
            "arkcli usage failed ({code}): {}",
            if message.is_empty() {
                "unknown error"
            } else {
                &message
            }
        )));
    }
    if output.stdout.len() > 256 * 1024 {
        return Err(ProviderError::Other(
            "arkcli returned too much output. Update arkcli and try again.".into(),
        ));
    }
    Ok(output.stdout)
}

pub(super) fn decode_arkcli_usage(bytes: &[u8]) -> Result<CodingPlanResult, ProviderError> {
    let response: ArkcliUsageResponse = serde_json::from_slice(bytes)
        .map_err(|e| ProviderError::Parse(format!("Failed to parse arkcli usage: {e}")))?;

    if let Some(method) = response
        .viewer
        .as_ref()
        .and_then(|v| v.auth_method.as_deref())
        .map(str::trim)
        && method.eq_ignore_ascii_case("none")
    {
        return Err(ProviderError::AuthRequired);
    }

    let mut quotas = Vec::new();
    let mut update_ts: Option<f64> = None;
    let mut status = response
        .viewer
        .and_then(|v| v.auth_method)
        .filter(|s| !s.trim().is_empty());

    for item in response.items {
        let product = item.product.to_ascii_lowercase();
        let level_prefix = match product.as_str() {
            "agent-plan" => "agent_",
            "coding-plan" => "",
            "agent-plan-team" => "agent_team_",
            "coding-plan-team" => "coding_team_",
            _ => continue,
        };
        if item.subscribed == Some(false) {
            continue;
        }
        let periods = item.periods.unwrap_or_default();
        if !periods.is_empty()
            && let Some(updated_at) = item.updated_at.filter(|v| *v > 0.0)
        {
            // arkcli may emit ms or seconds; 1e11 is the unit threshold.
            let seconds = if updated_at >= 1e11 {
                updated_at / 1000.0
            } else {
                updated_at
            };
            if update_ts.map(|t| seconds > t).unwrap_or(true) {
                update_ts = Some(seconds);
            }
        }
        for period in periods {
            let level = format!("{level_prefix}{}", period.label);
            let reset_timestamp = period
                .reset_at
                .as_deref()
                .and_then(|raw| DateTime::parse_from_rfc3339(raw.trim()).ok())
                .map(|d| d.timestamp() as f64);
            quotas.push(CodingPlanQuota {
                level,
                percent: period.percent,
                reset_timestamp,
            });
        }
        if status.is_none() {
            status = Some(product);
        }
    }

    if quotas.is_empty() {
        return Err(ProviderError::Parse(
            "arkcli returned no active Coding or Agent Plan usage.".into(),
        ));
    }

    Ok(CodingPlanResult {
        status,
        update_timestamp: update_ts,
        quota_usage: quotas,
    })
}

pub(super) fn fetch_arkcli_usage() -> Result<UsageSnapshot, ProviderError> {
    let stdout = run_arkcli_usage_plan()?;
    let usage = decode_arkcli_usage(&stdout)?;
    Ok(coding_plan_snapshot(usage))
}

pub(super) fn datetime_from_epoch(timestamp: f64) -> Option<DateTime<Utc>> {
    if !timestamp.is_finite() || timestamp <= 0.0 {
        return None;
    }
    // Epoch seconds guarded finite and positive; the sub-second fraction is
    // below timestamp resolution.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "epoch seconds; sub-second fraction below timestamp resolution"
    )]
    let whole_seconds = timestamp as i64;
    Utc.timestamp_opt(whole_seconds, 0).single()
}
