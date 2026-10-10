mod openai_usage;
#[cfg(test)]
mod openai_usage_tests;
pub(crate) mod pace;
mod quota_block;
mod settings_snapshot;
mod status;
pub(crate) use openai_usage::OpenAiApiUsageSnapshot;
pub use quota_block::MonthlyLimitBlockSnapshot;
pub use settings_snapshot::*;
pub(crate) use status::{compact_tray_status_label, friendly_provider_error};

use super::*;
use codexbar::core::BlockedWindows;

// ── Bridge snapshot types ────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RateWindowSnapshot {
    pub used_percent: f64,
    /// Defaults to `100.0` when absent in JSON (e.g. proof-seed files).
    #[serde(default = "default_full_remaining")]
    pub remaining_percent: f64,
    #[serde(default)]
    pub window_minutes: Option<u32>,
    #[serde(default)]
    pub resets_at: Option<String>,
    #[serde(default)]
    pub reset_description: Option<String>,
    #[serde(default)]
    pub is_exhausted: bool,
    #[serde(default)]
    pub is_informational: bool,
    /// `reset_description` is a detail line (for example spend amounts), not reset wording.
    #[serde(default)]
    pub description_is_detail: bool,
    #[serde(default)]
    pub reserve_percent: Option<f64>,
    #[serde(default)]
    pub reserve_description: Option<String>,
    #[serde(default)]
    pub reserve_will_last_to_reset: bool,
    #[serde(default)]
    pub reserve_eta_seconds: Option<f64>,
    /// Set while a longer exhausted pool (Kimi's monthly membership) blocks
    /// this window; presentation only, the raw percentages above stay as is.
    #[serde(default)]
    pub monthly_limit_block: Option<MonthlyLimitBlockSnapshot>,
}

/// Serde default for [`RateWindowSnapshot::remaining_percent`] — the common
/// case for a fresh window (0 %% used → 100 %% remaining).
fn default_full_remaining() -> f64 {
    100.0
}

impl Default for RateWindowSnapshot {
    fn default() -> Self {
        Self::from_rate_window(&RateWindow::new(0.0))
    }
}

/// Parse an RFC 3339 bridge timestamp into UTC.
pub(crate) fn parse_utc(value: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    chrono::DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|dt| dt.with_timezone(&chrono::Utc))
}

impl RateWindowSnapshot {
    pub(crate) fn resets_at_utc(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        self.resets_at.as_deref().and_then(parse_utc)
    }

    pub(crate) fn to_rate_window(&self) -> RateWindow {
        RateWindow::with_details(
            self.used_percent,
            self.window_minutes,
            self.resets_at_utc(),
            self.reset_description.clone(),
        )
    }

    pub(super) fn from_rate_window(rw: &RateWindow) -> Self {
        Self {
            used_percent: rw.used_percent,
            remaining_percent: rw.remaining_percent(),
            window_minutes: rw.window_minutes,
            resets_at: rw.resets_at.map(|dt| dt.to_rfc3339()),
            reset_description: rw.reset_description.clone(),
            is_exhausted: rw.is_exhausted(),
            is_informational: rw.is_informational,
            reserve_percent: None,
            reserve_description: None,
            reserve_will_last_to_reset: false,
            reserve_eta_seconds: None,
            monthly_limit_block: None,
            description_is_detail: rw.description_is_detail,
        }
    }

    /// Enrich with raw reserve info derived from pace analysis.
    /// delta_percent = actual - expected; negative means ahead (in reserve).
    /// Only meaningful for longer windows (weekly); skip if reserve rounds to 0.
    /// Localization happens at render time so cached snapshots stay language-neutral.
    fn with_pace_reserve(mut self, pace: &codexbar::core::UsagePace) -> Self {
        let reserve = pace.delta_percent.abs().round();
        if pace.delta_percent < 0.0 && reserve > 0.0 {
            self.reserve_percent = Some(reserve);
            self.reserve_will_last_to_reset = pace.will_last_to_reset;
            self.reserve_eta_seconds = pace.eta_seconds;
        }
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CostDailyPointBridge {
    pub day: String,
    pub amount: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CostSnapshotBridge {
    pub used: f64,
    #[serde(default)]
    pub limit: Option<f64>,
    #[serde(default)]
    pub remaining: Option<f64>,
    #[serde(default = "default_currency")]
    pub currency_code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub currency_symbol: Option<String>,
    #[serde(default = "default_cost_period")]
    pub period: String,
    #[serde(default)]
    pub resets_at: Option<String>,
    /// Defaults to `format!("${:.2}", used)` when absent (filled by
    /// [`parse_seed_usage_snapshot`](crate::proof_harness::parse_seed_usage_snapshot)).
    #[serde(default)]
    pub formatted_used: String,
    #[serde(default)]
    pub formatted_limit: Option<String>,
    #[serde(default)]
    pub balance: Option<f64>,
    #[serde(default)]
    pub balance_updated_at: Option<String>,
    #[serde(default)]
    pub account_id: Option<String>,
    #[serde(default)]
    pub formatted_balance: Option<String>,
    #[serde(default)]
    pub daily: Vec<CostDailyPointBridge>,
    #[serde(default)]
    pub always_visible: bool,
}

fn default_currency() -> String {
    "USD".to_string()
}

fn default_cost_period() -> String {
    "month".to_string()
}

/// Format a cost amount using the snapshot's currency symbol when available,
/// otherwise falling back to the currency-code prefix. Used by tray surfaces
/// that render a spend amount without a rate-window percent (MonthlyPlan).
pub(crate) fn format_cost_amount(
    cost: &CostSnapshotBridge,
    rates_cache: Option<&CurrencyRateCache>,
) -> String {
    if let Some((amount, currency)) =
        crate::commands::convert_preferred_amount(rates_cache, cost.used, &cost.currency_code)
    {
        // The canonical core symbol table (shared with CLI/tray formatting).
        return codexbar::core::format_currency(amount, &currency);
    }
    if !cost.formatted_used.is_empty() {
        return cost.formatted_used.clone();
    }
    if let Some(ref symbol) = cost.currency_symbol {
        format!("{}{:.2}", symbol, cost.used)
    } else {
        format!("{:.2} {}", cost.used, cost.currency_code)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NamedRateWindowSnapshot {
    pub id: String,
    pub title: String,
    pub window: RateWindowSnapshot,
    /// Whether this lane is a provider-declared fallback that only fills in
    /// when the provider reports no real core quota window.
    #[serde(default)]
    pub fallback_lane: bool,
    /// Provider-declared tray-icon lane this window stands in for when the
    /// snapshot has no real core window in that lane.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon_fallback: Option<codexbar::core::IconLane>,
}

/// Pace prediction snapshot for tray/bridge display.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaceSnapshot {
    pub stage: String,
    pub delta_percent: f64,
    #[serde(default)]
    pub will_last_to_reset: bool,
    #[serde(default)]
    pub eta_seconds: Option<f64>,
    #[serde(default)]
    pub expected_used_percent: f64,
    #[serde(default)]
    pub actual_used_percent: f64,
    /// Block of the window this pace comes from (upstream hides its pace).
    #[serde(default)]
    pub monthly_limit_block: Option<MonthlyLimitBlockSnapshot>,
}

/// One burndown chart point (RFC 3339 capture time + remaining percent).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaBurndownPointSnapshot {
    pub captured_at: String,
    pub remaining_percent: f64,
}

/// Recorded remaining-quota burndown for one series (session / weekly),
/// upstream 0.70.0 #4085. `captured_at` of the last sample drives the
/// capture-age caption; the chart is empty when there is no current window.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaBurndownSnapshot {
    /// `session` or `weekly`.
    pub series: String,
    pub window_minutes: u32,
    pub start: String,
    pub reset: String,
    pub samples: Vec<QuotaBurndownPointSnapshot>,
    pub ideal: [QuotaBurndownPointSnapshot; 2],
}

/// Session-equivalent weekly forecast for Claude/Codex menu secondary line.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionEquivalentForecastSnapshot {
    pub estimated_windows_to_exhaust_weekly: f64,
    pub windows_until_reset: i64,
    pub available_windows_until_reset: f64,
    pub sample_count: usize,
    pub weekly_resets_at: String,
    pub weekly_used_percent: f64,
}

/// Subscription dates from an authenticated OpenAI dashboard/API response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionMetadataSnapshot {
    pub starts_at: Option<String>,
    pub expires_at: Option<String>,
    pub renews_at: Option<String>,
}

/// Display-only provider inventory. Redemption identifiers never cross the
/// bridge.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInventoryItemSnapshot {
    pub id: String,
    pub title: String,
    pub available_count: u32,
    pub next_expires_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderDisplayProgressSnapshot {
    pub used: f64,
    pub total: f64,
}

/// Display-only provider detail row. It never participates in quota math or
/// core persistence and contains values validated by the provider carrier.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderDisplayDetailSnapshot {
    pub id: String,
    #[serde(default)]
    pub section_title: Option<String>,
    pub title: String,
    pub value: String,
    pub secondary_value: Option<String>,
    pub progress: Option<ProviderDisplayProgressSnapshot>,
}

/// A frontend-friendly snapshot of one provider's usage data.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderUsageSnapshot {
    #[serde(default)]
    pub tertiary_label: Option<String>,
    pub provider_id: String,
    #[serde(default = "default_display_name")]
    pub display_name: String,
    pub primary: RateWindowSnapshot,
    #[serde(default)]
    pub primary_label: Option<String>,
    #[serde(default)]
    pub secondary: Option<RateWindowSnapshot>,
    #[serde(default)]
    pub secondary_label: Option<String>,
    #[serde(default)]
    pub model_specific: Option<RateWindowSnapshot>,
    #[serde(default)]
    pub tertiary: Option<RateWindowSnapshot>,
    #[serde(default)]
    pub extra_rate_windows: Vec<NamedRateWindowSnapshot>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inventory: Vec<ProviderInventoryItemSnapshot>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub display_details: Vec<ProviderDisplayDetailSnapshot>,
    #[serde(default)]
    pub cost: Option<CostSnapshotBridge>,
    #[serde(default)]
    pub plan_name: Option<String>,
    #[serde(default)]
    pub account_email: Option<String>,
    #[serde(default)]
    pub subscription: Option<SubscriptionMetadataSnapshot>,
    #[serde(default = "default_source_label")]
    pub source_label: String,
    #[serde(default)]
    pub has_successful_claude_cli_quota: bool,
    /// Defaults to launch time when absent so the card renders as fresh.
    #[serde(default)]
    pub updated_at: String,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default = "default_error_state")]
    pub error_state: codexbar::core::ProviderStateKind,
    #[serde(default)]
    pub pace: Option<PaceSnapshot>,
    #[serde(default)]
    pub account_organization: Option<String>,
    #[serde(default)]
    pub tray_status_label: Option<String>,
    #[serde(default)]
    pub fetch_duration_ms: Option<u128>,
    #[serde(default)]
    pub wayfinder_usage: Option<codexbar::core::WayfinderUsageSnapshot>,
    /// Per-day OpenAI Admin API history for the daily usage chart.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open_ai_api_usage: Option<OpenAiApiUsageSnapshot>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub session_equivalent_forecast: Option<SessionEquivalentForecastSnapshot>,
    /// Recorded remaining-quota burndown for the selected window
    /// (upstream 0.70.0 #4085); Codex and Claude only.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub quota_burndown: Option<QuotaBurndownSnapshot>,
}

fn default_display_name() -> String {
    "Codex".to_string()
}

fn default_source_label() -> String {
    "seed".to_string()
}

fn default_error_state() -> codexbar::core::ProviderStateKind {
    codexbar::core::ProviderStateKind::Unknown
}

/// Provider payload after applying settings-driven cross-surface presentation.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderUsagePresentationSnapshot {
    #[serde(flatten)]
    pub snapshot: ProviderUsageSnapshot,
    pub selected_metric: RateWindowSnapshot,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub hidden_usage_item_ids: Vec<String>,
}

impl ProviderUsagePresentationSnapshot {
    pub(crate) fn new(snapshot: ProviderUsageSnapshot, settings: &Settings) -> Self {
        let selected_metric = crate::usage_metric::selected_usage_window(&snapshot, settings);
        let hidden_usage_item_ids = snapshot
            .provider_id
            .trim()
            .parse::<String>()
            .ok()
            .and_then(|id| ProviderId::from_cli_name(&id))
            .map(|id| settings.hidden_usage_item_ids(id))
            .unwrap_or_default();
        Self {
            snapshot,
            selected_metric,
            hidden_usage_item_ids,
        }
    }
}

impl ProviderUsageSnapshot {
    pub(super) fn from_fetch_result(
        id: ProviderId,
        metadata: &ProviderMetadata,
        result: &ProviderFetchResult,
        token_account_id: Option<uuid::Uuid>,
    ) -> Self {
        let usage = &result.usage;
        let allows_pace = result.pace_authoritative;

        // A missing session is represented by an informational primary so the
        // weekly lane keeps its canonical role. Use that weekly lane for the
        // provider-level pace summary instead of returning no pace at all.
        let primary_pace_window = if usage.primary.is_informational {
            usage.secondary.as_ref()
        } else {
            Some(&usage.primary)
        };
        let primary_pace = allows_pace.then(|| {
            primary_pace_window
                .and_then(|window| codexbar::core::UsagePace::weekly(window, None, 10080))
        });
        let primary_pace = primary_pace.flatten();

        let blocked = BlockedWindows::evaluate(id, usage, chrono::Utc::now());
        let pace = primary_pace.as_ref().map(|p| PaceSnapshot {
            stage: pace::stage_str(p.stage).to_string(),
            delta_percent: p.delta_percent,
            will_last_to_reset: p.will_last_to_reset,
            eta_seconds: p.eta_seconds,
            expected_used_percent: p.expected_used_percent,
            actual_used_percent: p.actual_used_percent,
            monthly_limit_block: MonthlyLimitBlockSnapshot::for_pace(usage, &blocked),
        });

        // Compute pace for secondary window (weekly) to derive reserve info
        let secondary_pace = allows_pace.then(|| {
            usage
                .secondary
                .as_ref()
                .and_then(|sw| codexbar::core::UsagePace::weekly(sw, None, 10080))
        });
        let secondary_pace = secondary_pace.flatten();

        let primary_snap = RateWindowSnapshot::from_rate_window(&usage.primary)
            .with_quota_block(blocked.primary, &blocked);

        let secondary_snap = usage.secondary.as_ref().map(|sw| {
            let mut s = RateWindowSnapshot::from_rate_window(sw)
                .with_quota_block(blocked.secondary, &blocked);
            if let Some(ref p) = secondary_pace {
                s = s.with_pace_reserve(p);
            }
            s
        });

        // Scope forecast history to the signed-in account so switching accounts on one
        // provider does not blend burn samples across plans. Codex publishes no email or
        // organization (ADR 0003 ambient/managed lanes), so its discriminator is the
        // managed token-account id.
        let account_key = forecast_account_key(usage, token_account_id);
        let session_equivalent_forecast = session_equivalent_forecast_for(
            id,
            account_key.as_deref(),
            &usage.primary,
            usage.secondary.as_ref(),
        );
        let quota_burndown = quota_burndown_for(
            id,
            account_key.as_deref(),
            &usage.primary,
            usage.secondary.as_ref(),
        );

        Self {
            provider_id: id.cli_name().to_string(),
            display_name: id.display_name().to_string(),
            primary: primary_snap,
            primary_label: Some(
                usage
                    .primary_label
                    .clone()
                    .unwrap_or_else(|| metadata.session_label.to_string()),
            ),
            secondary: secondary_snap,
            secondary_label: usage.secondary.as_ref().map(|_| {
                usage
                    .secondary_label
                    .clone()
                    .unwrap_or_else(|| metadata.weekly_label.to_string())
            }),
            model_specific: usage.model_specific.as_ref().map(|w| {
                RateWindowSnapshot::from_rate_window(w)
                    .with_quota_block(blocked.model_specific, &blocked)
            }),
            tertiary: usage.tertiary.as_ref().map(|w| {
                RateWindowSnapshot::from_rate_window(w).with_quota_block(blocked.tertiary, &blocked)
            }),
            // F5 (upstream 0.48.0): label the tertiary lane by its duration cadence
            // so surfaces (MenuCard, CLI, tray) can show "Monthly" instead of the
            // generic "DetailWindowTertiary" slot key.
            tertiary_label: usage.tertiary.as_ref().map(|w| {
                match codexbar::core::RateWindowCadence::from_minutes(w.window_minutes.unwrap_or(0))
                    .label_key()
                {
                    "monthly" => "monthly".to_string(),
                    other => other.to_string(),
                }
            }),
            extra_rate_windows: usage
                .extra_rate_windows
                .iter()
                .zip(&blocked.extra)
                .map(|(extra, &is_blocked)| NamedRateWindowSnapshot {
                    id: extra.id.clone(),
                    title: extra.title.clone(),
                    window: RateWindowSnapshot::from_rate_window(&extra.window)
                        .with_quota_block(is_blocked, &blocked),
                    fallback_lane: extra.fallback_lane,
                    icon_fallback: extra.icon_fallback,
                })
                .collect(),
            inventory: result
                .inventory
                .iter()
                .map(|item| ProviderInventoryItemSnapshot {
                    id: item.id.clone(),
                    title: item.title.clone(),
                    available_count: item.available_count,
                    next_expires_at: item.next_expires_at.map(|date| date.to_rfc3339()),
                })
                .collect(),
            display_details: result
                .display_details()
                .iter()
                .map(|detail| ProviderDisplayDetailSnapshot {
                    id: detail.id().to_string(),
                    section_title: detail.section_title().map(ToOwned::to_owned),
                    title: detail.title().to_string(),
                    value: detail.value().to_string(),
                    secondary_value: detail.secondary_value().map(ToOwned::to_owned),
                    progress: detail
                        .progress()
                        .map(|progress| ProviderDisplayProgressSnapshot {
                            used: progress.used(),
                            total: progress.total(),
                        }),
                })
                .collect(),
            cost: result.cost.as_ref().map(|c| CostSnapshotBridge {
                used: c.used,
                limit: c.limit,
                remaining: c.remaining(),
                currency_code: c.currency_code.clone(),
                currency_symbol: c.currency_symbol.clone(),
                period: c.period.clone(),
                resets_at: c.resets_at.map(|dt| dt.to_rfc3339()),
                formatted_used: c.format_used(),
                formatted_limit: c.format_limit(),
                balance: c.balance,
                balance_updated_at: c.balance_updated_at.map(|dt| dt.to_rfc3339()),
                account_id: c.account_id.clone(),
                formatted_balance: c.format_balance(),
                daily: c
                    .daily
                    .iter()
                    .map(|point| CostDailyPointBridge {
                        day: point.day.clone(),
                        amount: point.amount,
                    })
                    .collect(),
                always_visible: c.always_visible,
            }),
            plan_name: usage.login_method.clone(),
            account_email: usage.account_email.clone(),
            subscription: usage.subscription.as_ref().map(|subscription| {
                SubscriptionMetadataSnapshot {
                    starts_at: subscription.starts_at.map(|date| date.to_rfc3339()),
                    expires_at: subscription.expires_at.map(|date| date.to_rfc3339()),
                    renews_at: subscription.renews_at.map(|date| date.to_rfc3339()),
                }
            }),
            source_label: result.source_label.clone(),
            has_successful_claude_cli_quota: result.has_successful_claude_cli_quota,
            updated_at: usage.updated_at.to_rfc3339(),
            error: None,
            error_state: codexbar::core::ProviderStateKind::Ready,
            pace,
            account_organization: usage.account_organization.clone(),
            tray_status_label: None,
            fetch_duration_ms: None,
            wayfinder_usage: result.wayfinder_usage.clone(),
            open_ai_api_usage: result.open_ai_api_usage.as_ref().map(Into::into),
            session_equivalent_forecast,
            quota_burndown,
        }
    }

    pub(super) fn from_error(
        id: ProviderId,
        metadata: &ProviderMetadata,
        error: String,
        state_kind: codexbar::core::ProviderStateKind,
    ) -> Self {
        let error = friendly_provider_error(id, &error);
        Self {
            provider_id: id.cli_name().to_string(),
            display_name: id.display_name().to_string(),
            primary_label: Some(metadata.session_label.to_string()),
            updated_at: chrono::Utc::now().to_rfc3339(),
            error: Some(error),
            error_state: state_kind,
            ..Default::default()
        }
    }
}

/// Account discriminator that forecast history is scoped to.
///
/// Deliberately mirrors `quota_notification_account_identity` precedence
/// (token account -> email -> organization) so a single account is never seen as two
/// different identities by the notification and forecast subsystems. Kept as a separate
/// function because that one consumes an already-built `ProviderUsageSnapshot`, while the
/// forecast needs the key *while* the snapshot is being built.
///
/// `providers::tests::forecast_account_key_matches_notification_identity` pins them
/// together.
pub(super) fn forecast_account_key(
    usage: &codexbar::core::UsageSnapshot,
    token_account_id: Option<uuid::Uuid>,
) -> Option<String> {
    if let Some(id) = token_account_id {
        return Some(format!("token-account:{}", id.as_hyphenated()));
    }
    if let Some(email) = usage
        .account_email
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return Some(email.to_ascii_lowercase());
    }
    if let Some(org) = usage
        .account_organization
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return Some(format!("org:{}", org.to_ascii_lowercase()));
    }
    None
}

fn session_equivalent_forecast_for(
    id: ProviderId,
    account_key: Option<&str>,
    session: &RateWindow,
    weekly: Option<&RateWindow>,
) -> Option<SessionEquivalentForecastSnapshot> {
    if !matches!(id, ProviderId::Claude | ProviderId::Codex) {
        return None;
    }
    let weekly = weekly?;
    let now = chrono::Utc::now();
    let provider_id = id.cli_name();
    codexbar::core::record_provider_windows(provider_id, account_key, session, Some(weekly), now);
    let work_days = Settings::load().weekly_progress_work_days;
    let forecast = codexbar::core::forecast_for_provider(
        provider_id,
        account_key,
        session,
        weekly,
        now,
        work_days,
    )?;
    Some(SessionEquivalentForecastSnapshot {
        estimated_windows_to_exhaust_weekly: forecast.estimated_windows_to_exhaust_weekly,
        windows_until_reset: forecast.windows_until_reset,
        available_windows_until_reset: forecast.available_windows_until_reset,
        sample_count: forecast.sample_count,
        weekly_resets_at: forecast.weekly_resets_at.to_rfc3339(),
        weekly_used_percent: forecast.weekly_used_percent,
    })
}

/// Build the recorded burndown for the live window (session or weekly lane)
/// from the persisted history; `None` when the window is expired or unknown.
/// Codex and Claude only, mirroring the forecast gate.
fn quota_burndown_for(
    id: ProviderId,
    account_key: Option<&str>,
    session: &RateWindow,
    weekly: Option<&RateWindow>,
) -> Option<QuotaBurndownSnapshot> {
    if !matches!(id, ProviderId::Claude | ProviderId::Codex) {
        return None;
    }
    let now = chrono::Utc::now();
    let provider_id = id.cli_name();
    // Persisted history feeds the chart; loading merges it into the
    // process-local store once per scope.
    let histories = codexbar::core::load_persisted_history(provider_id, account_key);

    let live = |series: &str, window: &RateWindow| -> Option<QuotaBurndownSnapshot> {
        let model = codexbar::core::QuotaBurndownModel::build(
            &series_entries(&histories, series),
            window,
            now,
        )?;
        Some(QuotaBurndownSnapshot {
            series: series.to_string(),
            window_minutes: window.window_minutes?,
            start: model.start.to_rfc3339(),
            reset: model.reset.to_rfc3339(),
            samples: model
                .samples
                .iter()
                .map(|sample| QuotaBurndownPointSnapshot {
                    captured_at: sample.captured_at.to_rfc3339(),
                    remaining_percent: sample.remaining_percent,
                })
                .collect(),
            ideal: [
                QuotaBurndownPointSnapshot {
                    captured_at: model.ideal[0].captured_at.to_rfc3339(),
                    remaining_percent: model.ideal[0].remaining_percent,
                },
                QuotaBurndownPointSnapshot {
                    captured_at: model.ideal[1].captured_at.to_rfc3339(),
                    remaining_percent: model.ideal[1].remaining_percent,
                },
            ],
        })
    };

    // Upstream prefers the shortest current window (session over weekly).
    live("session", session).or_else(|| weekly.and_then(|w| live("weekly", w)))
}

fn series_entries(
    histories: &[codexbar::core::PlanUtilizationSeriesHistory],
    series: &str,
) -> Vec<codexbar::core::PlanUtilizationHistoryEntry> {
    histories
        .iter()
        .find(|history| match series {
            "session" => history.name == codexbar::core::PlanUtilizationSeriesName::Session,
            _ => history.name == codexbar::core::PlanUtilizationSeriesName::Weekly,
        })
        .map(|history| history.entries.clone())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests;
