//! Dashboard snapshot v1 schema (`/dashboard/v1/snapshot`) and the pure,
//! transport-independent builder.
//!
//! Upstream 0.48.0 reference: `Sources/CodexBarCLI/DashboardPayloads.swift`
//! and `DashboardSnapshotBuilder.swift` at tag `v0.48.0`. JSON field names
//! (camelCase) and defaults (`schemaVersion: 1`,
//! `staleAfterSeconds = max(180, refresh * 3)`, identity redaction
//! `redacted@domain`, sort keys `index * 10` / fallback `10000 + index`) mirror
//! the pinned upstream contract. Dates serialize as ISO-8601 (the upstream web
//! UI parses them with `Date.parse`).
//!
//! Documented divergences (Win-CodexBar architecture):
//! - `credits` is always `null`: Win-CodexBar has no separate CreditsSnapshot
//!   pipeline (balances ride the cost snapshot / extra rate windows).
//! - `status` is always `null`: provider status-page polling is a separate
//!   `--fetch-status` path here; the HTML hides the status chip (#2723 parity).
//! - `display.accentColor` defaults to upstream's own fallback `#6E6E6E`:
//!   provider descriptors here carry no brand color.
//! - `accounts.pace` uses the 7-stage local [`PaceStage`] model (identical
//!   stage names to upstream's `UsagePace.Stage`).

use std::collections::{BTreeSet, HashMap};

use super::antigravity;
use super::window::make_window_with_idle;

use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::cli::serve::collection::SnapshotCollection;
pub use crate::cli::serve::collection::{
    AccountFetchEnvelope, ClaudeAccountsInput, ProviderFetchEnvelope, RawCostPayload,
};
#[cfg(test)]
use crate::core::ProviderFetchResult;
use crate::core::{RateWindow, UsagePace, UsageSnapshot};

/// How much account identity a snapshot exposes. Upstream 0.48.0 exposes two
/// CLI modes (`redacted` default, `full` opt-in); upstream's internal `none`
/// case is intentionally not a user-facing knob here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DashboardIdentity {
    Redacted,
    Full,
}

impl DashboardIdentity {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "redacted" => Some(Self::Redacted),
            "full" => Some(Self::Full),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotPayload {
    pub schema_version: u32,
    pub generated_at: DateTime<Utc>,
    pub stale_after_seconds: u32,
    pub host: HostPayload,
    pub providers: Vec<SnapshotProvider>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostPayload {
    pub codex_bar_version: Option<String>,
    pub refresh_interval_seconds: u32,
    /// Whether dashboard bars show used quota (true) or remaining quota.
    /// The page treats an absent legacy value as false.
    pub usage_bars_show_used: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotProvider {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub source: String,
    pub status: Option<StatusPayload>,
    pub identity: Option<IdentityPayload>,
    pub windows: Vec<WindowPayload>,
    pub credits: Option<CreditsPayload>,
    pub cost: Option<CostPayload>,
    pub display: DisplayPayload,
    pub error: Option<ProviderErrorPayload>,
    pub updated_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accounts: Option<Vec<AccountPayload>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accounts_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusPayload {
    pub level: String,
    pub label: String,
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentityPayload {
    pub account_email: Option<String>,
    pub plan: Option<String>,
}

pub use super::window::WindowPayload;

#[derive(Debug, Clone, Serialize)]
pub struct CreditsPayload {
    pub remaining: f64,
    pub unit: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CostPayload {
    #[serde(rename = "todayUSD")]
    pub today_usd: Option<f64>,
    #[serde(rename = "last30DaysUSD")]
    pub last_30_days_usd: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DisplayPayload {
    pub accent_color: String,
    pub sort_key: u32,
    pub priority: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderErrorPayload {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountPayload {
    pub id: String,
    pub label: String,
    pub active: bool,
    pub identity: Option<IdentityPayload>,
    pub windows: Vec<WindowPayload>,
    pub pace: Option<ProviderPacePayload>,
    pub error: Option<String>,
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderPacePayload {
    pub primary: Option<PacePayload>,
    pub secondary: Option<PacePayload>,
    pub tertiary: Option<PacePayload>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PacePayload {
    pub stage: String,
    pub delta_percent: f64,
    pub expected_used_percent: f64,
    pub will_last_to_reset: bool,
    pub eta_seconds: Option<f64>,
    /// Always absent upstream in CLI output; kept absent here too (schema parity).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_out_probability: Option<f64>,
    pub summary: String,
}

pub struct SnapshotInput {
    pub collection: SnapshotCollection,
    pub identity: DashboardIdentity,
    pub version: Option<String>,
    /// None represents a caller with no fill preference. Dashboard output
    /// defaults that case to remaining quota.
    pub usage_bars_show_used: Option<bool>,
}

/// Build the stable display-oriented snapshot (pure; no I/O).
#[allow(
    clippy::cast_possible_truncation,
    reason = "provider order is a short config list; index*10 fits u32"
)]
pub fn build_snapshot(input: &SnapshotInput) -> SnapshotPayload {
    let mut sort_keys: HashMap<&str, u32> = HashMap::new();
    for (index, id) in input.collection.order.iter().enumerate() {
        sort_keys
            .entry(id.as_str())
            .or_insert_with(|| index as u32 * 10);
    }

    let known_ids: BTreeSet<&str> = crate::core::cli_name_map().keys().copied().collect();
    let mut claude_attached = false;
    let providers = input
        .collection
        .providers
        .iter()
        .enumerate()
        .map(|(index, envelope)| {
            // Provider-specific by design (upstream parity): account data
            // belongs only on the FIRST claude row.
            let claude = if !claude_attached && envelope.id == "claude" {
                claude_attached = true;
                input.collection.claude_accounts.as_ref()
            } else {
                None
            };
            let sort_key = sort_keys
                .get(envelope.id.as_str())
                .copied()
                .unwrap_or(10_000 + index as u32);
            build_provider(
                envelope,
                &input.collection.costs,
                input,
                &known_ids,
                sort_key,
                claude,
            )
        })
        .collect();

    let refresh = input.collection.refresh_seconds;
    SnapshotPayload {
        schema_version: 1,
        generated_at: input.collection.generated_at,
        stale_after_seconds: (refresh.saturating_mul(3)).max(180),
        host: HostPayload {
            codex_bar_version: input.version.clone(),
            refresh_interval_seconds: refresh,
            usage_bars_show_used: input.usage_bars_show_used.unwrap_or(false),
        },
        providers,
    }
}

fn build_provider(
    envelope: &ProviderFetchEnvelope,
    costs: &HashMap<String, RawCostPayload>,
    input: &SnapshotInput,
    known_ids: &BTreeSet<&str>,
    sort_key: u32,
    claude: Option<&ClaudeAccountsInput>,
) -> SnapshotProvider {
    let cost = costs.get(&envelope.id).and_then(|raw| {
        (raw.today_usd.is_some() || raw.last_30_days_usd.is_some()).then_some(CostPayload {
            today_usd: raw.today_usd,
            last_30_days_usd: raw.last_30_days_usd,
        })
    });

    let (source, identity, windows, updated_at, error) = match &envelope.fetch {
        Ok(result) => {
            let source = dashboard_source(&result.source_label);
            let identity = make_identity(&result.usage, input.identity);
            let windows = make_windows(
                Some(&envelope.id),
                &envelope.session_label,
                &envelope.weekly_label,
                &result.usage,
            );
            (
                source,
                identity,
                windows,
                Some(result.usage.updated_at),
                None,
            )
        }
        Err(message) => (
            "unknown".to_string(),
            None,
            Vec::new(),
            Some(input.collection.generated_at),
            Some(ProviderErrorPayload {
                code: 1,
                message: message.clone(),
                kind: Some("provider".to_string()),
            }),
        ),
    };

    let (accounts, accounts_error) = match claude {
        Some(claude) => match &claude.accounts {
            Ok(accounts) => (
                Some(
                    accounts
                        .iter()
                        .map(|account| {
                            build_account(
                                account,
                                input.identity,
                                &envelope.session_label,
                                &envelope.weekly_label,
                                input.collection.generated_at,
                            )
                        })
                        .collect(),
                ),
                None,
            ),
            Err(adapter_error) => (None, Some(adapter_error.clone())),
        },
        None => (None, None),
    };

    SnapshotProvider {
        id: envelope.id.clone(),
        name: envelope.display_name.clone(),
        // Upstream: known provider ids report config membership; unrecognized
        // payloads stay enabled.
        enabled: !known_ids.contains(envelope.id.as_str())
            || input.collection.enabled.contains(&envelope.id),
        source,
        status: None,
        identity,
        windows,
        credits: None,
        cost,
        display: DisplayPayload {
            accent_color: "#6E6E6E".to_string(),
            sort_key,
            priority: "normal".to_string(),
        },
        error,
        updated_at,
        accounts,
        accounts_error,
    }
}

fn build_account(
    account: &AccountFetchEnvelope,
    identity_mode: DashboardIdentity,
    session_label: &str,
    weekly_label: &str,
    generated_at: DateTime<Utc>,
) -> AccountPayload {
    let (identity, windows, pace, error, updated_at) = match &account.fetch {
        Ok(result) => (
            make_identity(&result.usage, identity_mode),
            make_windows(None, session_label, weekly_label, &result.usage),
            make_pace(&result.usage),
            None,
            Some(result.usage.updated_at),
        ),
        Err(message) => (
            None,
            Vec::new(),
            None,
            Some(message.clone()),
            Some(generated_at),
        ),
    };
    AccountPayload {
        id: account.id.clone(),
        label: account.label.clone(),
        active: account.active,
        identity,
        windows,
        pace,
        error,
        updated_at,
    }
}

fn dashboard_source(source: &str) -> String {
    let trimmed = source.trim();
    if trimmed.is_empty() {
        "unknown".to_string()
    } else {
        trimmed.to_string()
    }
}

fn make_identity(usage: &UsageSnapshot, mode: DashboardIdentity) -> Option<IdentityPayload> {
    let account_email = dashboard_email(usage.account_email.as_deref(), mode);
    let plan = usage
        .login_method
        .as_deref()
        .map(str::trim)
        .filter(|raw| !raw.is_empty())
        .map(str::to_string);
    if account_email.is_none() && plan.is_none() {
        None
    } else {
        Some(IdentityPayload {
            account_email,
            plan,
        })
    }
}

/// Upstream redaction: `redacted@domain` (everything before the last `@`
/// replaced); bare values without `@` become just `redacted`.
fn dashboard_email(email: Option<&str>, mode: DashboardIdentity) -> Option<String> {
    let email = email?.trim();
    if email.is_empty() {
        return None;
    }
    match mode {
        DashboardIdentity::Full => Some(email.to_string()),
        DashboardIdentity::Redacted => match email.rfind('@') {
            Some(at) => Some(format!("redacted{}", &email[at..])),
            None => Some("redacted".to_string()),
        },
    }
}

fn make_windows(
    provider_id: Option<&str>,
    session_label: &str,
    weekly_label: &str,
    usage: &UsageSnapshot,
) -> Vec<WindowPayload> {
    if provider_id == Some("antigravity") {
        return antigravity::quota_summary_windows(usage, session_label, weekly_label);
    }

    standard_windows(usage, session_label, weekly_label)
}

fn standard_windows(
    usage: &UsageSnapshot,
    session_label: &str,
    weekly_label: &str,
) -> Vec<WindowPayload> {
    let mut windows = Vec::with_capacity(4 + usage.extra_rate_windows.len());
    windows.push(make_window(
        "session",
        usage.primary_label.as_deref().unwrap_or(session_label),
        &usage.primary,
    ));
    if let Some(secondary) = &usage.secondary {
        windows.push(make_window(
            "weekly",
            usage.secondary_label.as_deref().unwrap_or(weekly_label),
            secondary,
        ));
    }
    push_model_and_tertiary_windows(&mut windows, usage);
    for extra in &usage.extra_rate_windows {
        windows.push(make_window(&extra.id, &extra.title, &extra.window));
    }
    windows
}

/// Shared tail for window mapping: model-specific row plus tertiary row.
fn push_model_and_tertiary_windows(windows: &mut Vec<WindowPayload>, usage: &UsageSnapshot) {
    if let Some(model) = &usage.model_specific {
        windows.push(make_window("model", "Opus", model));
    }
    if let Some(tertiary) = &usage.tertiary {
        windows.push(make_window("tertiary", "Tertiary", tertiary));
    }
}

fn make_window(kind: &str, label: &str, window: &RateWindow) -> WindowPayload {
    make_window_with_idle(kind, label, window, false)
}

fn make_pace(usage: &UsageSnapshot) -> Option<ProviderPacePayload> {
    let payload = ProviderPacePayload {
        primary: None,
        secondary: usage
            .secondary
            .as_ref()
            .and_then(|window| UsagePace::weekly(window, None, 10080))
            .map(|pace| pace_payload(&pace)),
        tertiary: None,
    };
    (payload.secondary.is_some()).then_some(payload)
}

/// Upstream `PacePayload` mapping: rounded percents, camelCase stage names.
fn pace_payload(pace: &UsagePace) -> PacePayload {
    PacePayload {
        stage: pace_stage_name(pace.stage).to_string(),
        delta_percent: pace.delta_percent.round(),
        expected_used_percent: pace.expected_used_percent.round(),
        will_last_to_reset: pace.will_last_to_reset,
        eta_seconds: pace.eta_seconds.map(|eta| eta.round()),
        run_out_probability: None,
        summary: pace.format_status(),
    }
}

/// Local stage names match upstream `UsagePace.Stage` exactly (camelCase).
fn pace_stage_name(stage: crate::core::PaceStage) -> &'static str {
    match stage {
        crate::core::PaceStage::OnTrack => "onTrack",
        crate::core::PaceStage::SlightlyAhead => "slightlyAhead",
        crate::core::PaceStage::Ahead => "ahead",
        crate::core::PaceStage::FarAhead => "farAhead",
        crate::core::PaceStage::SlightlyBehind => "slightlyBehind",
        crate::core::PaceStage::Behind => "behind",
        crate::core::PaceStage::FarBehind => "farBehind",
    }
}

#[cfg(test)]
#[path = "snapshot_tests.rs"]
mod snapshot_tests;
