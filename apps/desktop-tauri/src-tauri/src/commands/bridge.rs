mod openai_live;
#[cfg(test)]
mod openai_live_tests;
mod openai_usage;
#[cfg(test)]
mod openai_usage_tests;
pub(crate) mod pace;
mod quota_block;
mod status;
mod throughput;
#[cfg(test)]
mod throughput_tests;
pub(crate) use openai_live::{ModelRateLimitSnapshot, OrgRateLimitsSnapshot};
pub(crate) use openai_usage::OpenAiApiUsageSnapshot;
pub use quota_block::MonthlyLimitBlockSnapshot;
pub(crate) use status::{compact_tray_status_label, friendly_provider_error};
pub(crate) use throughput::ProviderThroughputSnapshot;

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
    /// The reset is the next quota regeneration; the card says "Regenerates in".
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub reset_regenerates: bool,
    /// The window feeds the tray and float bar only; the card draws no row for it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hidden_on_card: bool,
    #[serde(default)]
    pub reserve_percent: Option<f64>,
    #[serde(default)]
    pub reserve_description: Option<String>,
    #[serde(default)]
    pub reserve_will_last_to_reset: bool,
    #[serde(default)]
    pub reserve_eta_seconds: Option<f64>,
    #[serde(default)]
    pub pace: Option<pace::WindowPaceSnapshot>,
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

impl RateWindowSnapshot {
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
            pace: None,
            monthly_limit_block: None,
            description_is_detail: rw.description_is_detail,
            reset_regenerates: rw.reset_regenerates,
            hidden_on_card: rw.hidden_on_card,
        }
    }

    /// The core window this snapshot was built from, as far as pace needs it:
    /// usage, length, reset time and reset description.
    pub(crate) fn to_rate_window(&self) -> RateWindow {
        let resets_at = self
            .resets_at
            .as_deref()
            .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
            .map(|value| value.with_timezone(&chrono::Utc));
        RateWindow::with_details(
            self.used_percent,
            self.window_minutes,
            resets_at,
            self.reset_description.clone(),
        )
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
    /// The primary meter already shows this cap, so the cost block omits its bar.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub folded_into_primary: bool,
    /// Provider-owned cost-block title as a locale key name; absent keeps the shared title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title_key: Option<String>,
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
    /// Date-only values are UTC midnight and must be formatted in UTC.
    #[serde(default)]
    pub renews_at_is_date_only: bool,
    #[serde(default)]
    pub expires_at_is_date_only: bool,
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
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    /// Locale key that names the secondary lane instead of `secondary_label`
    /// (provider-owned, see `UsageSnapshot::secondary_label_key`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secondary_label_key: Option<String>,
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
    /// Header-only label of the active token account (upstream shows it when
    /// the provider reports no email). Never used for history or warning keys.
    #[serde(default)]
    pub account_label: Option<String>,
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
    /// Whether the fetch allows pace (upstream `allowsPace(dataConfidence:)`).
    /// The tray scores its pace tint at presentation time, so it needs this
    /// beside the windows. Tray-only; not sent to the frontend.
    #[serde(default = "default_pace_authoritative", skip_serializing)]
    pub pace_authoritative: bool,
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
    /// Last-hour Tokens/min grid (OpenAI Admin API 1-minute buckets).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub throughput: Option<ProviderThroughputSnapshot>,
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

fn default_pace_authoritative() -> bool {
    true
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
        pace_work_days: Option<u8>,
    ) -> Self {
        Self::from_fetch_result_at(
            id,
            metadata,
            result,
            token_account_id,
            pace_work_days,
            chrono::Utc::now(),
        )
    }

    pub(super) fn from_fetch_result_at(
        id: ProviderId,
        metadata: &ProviderMetadata,
        result: &ProviderFetchResult,
        token_account_id: Option<uuid::Uuid>,
        pace_work_days: Option<u8>,
        now: chrono::DateTime<chrono::Utc>,
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
        // Upstream `UsageStore.weeklyPace`: a window with a length expands a
        // monthly sentinel to the real calendar cycle before scoring, so the
        // tray agrees with the card.
        let primary_pace = allows_pace.then(|| {
            primary_pace_window.and_then(|window| {
                let pace_window = if window.window_minutes.is_some() {
                    id.pace_capability().resolved_reset_window_for_pace(window)
                } else {
                    window.clone()
                };
                codexbar::core::UsagePace::weekly(&pace_window, Some(now), 10080, pace_work_days)
            })
        });
        let primary_pace = primary_pace.flatten();

        let blocked = BlockedWindows::evaluate(id, usage, now);
        let pace = primary_pace.as_ref().map(|p| PaceSnapshot {
            stage: pace::stage_str(p.stage).to_string(),
            delta_percent: p.delta_percent,
            will_last_to_reset: p.will_last_to_reset,
            eta_seconds: p.eta_seconds,
            expected_used_percent: p.expected_used_percent,
            actual_used_percent: p.actual_used_percent,
            monthly_limit_block: MonthlyLimitBlockSnapshot::for_pace(usage, &blocked),
        });

        // Pace of the secondary lane, which also derives the row's reserve.
        let secondary_pace = allows_pace.then(|| {
            usage.secondary.as_ref().and_then(|sw| {
                pace::lane_usage_pace(
                    id,
                    pace::PaceLane::Card,
                    sw,
                    pace::WEEKLY_FALLBACK_MINUTES,
                    now,
                    pace_work_days,
                )
                .map(|(pace, _)| pace)
            })
        });
        let secondary_pace = secondary_pace.flatten();

        let lane = |window: &RateWindow,
                    is_blocked: bool,
                    default_minutes: u32,
                    pace_lane: pace::PaceLane| RateWindowSnapshot {
            pace: allows_pace
                .then(|| {
                    pace::WindowPaceSnapshot::for_lane(
                        id,
                        pace_lane,
                        window,
                        default_minutes,
                        now,
                        pace_work_days,
                    )
                })
                .flatten(),
            ..RateWindowSnapshot::from_rate_window(window).with_quota_block(is_blocked, &blocked)
        };
        let primary_snap = lane(
            &usage.primary,
            blocked.primary,
            pace::SESSION_FALLBACK_MINUTES,
            pace::PaceLane::Card,
        );

        let secondary_snap = usage.secondary.as_ref().map(|sw| {
            let mut s = lane(
                sw,
                blocked.secondary,
                pace::WEEKLY_FALLBACK_MINUTES,
                pace::PaceLane::Card,
            );
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
            secondary_label_key: usage
                .secondary
                .as_ref()
                .and(usage.secondary_label_key)
                .map(|key| key.name().to_string()),
            model_specific: usage.model_specific.as_ref().map(|w| {
                lane(
                    w,
                    blocked.model_specific,
                    pace::WEEKLY_FALLBACK_MINUTES,
                    pace::PaceLane::Card,
                )
            }),
            tertiary: usage.tertiary.as_ref().map(|w| {
                lane(
                    w,
                    blocked.tertiary,
                    pace::WEEKLY_FALLBACK_MINUTES,
                    pace::PaceLane::Card,
                )
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
                    window: lane(
                        &extra.window,
                        is_blocked,
                        pace::WEEKLY_FALLBACK_MINUTES,
                        pace::PaceLane::Extra,
                    ),
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
                folded_into_primary: c.folded_into_primary,
                title_key: c.card_title_key.map(|key| key.name().to_string()),
            }),
            plan_name: usage.login_method.clone(),
            account_email: usage.account_email.clone(),
            account_label: None,
            subscription: usage.subscription.as_ref().map(|subscription| {
                SubscriptionMetadataSnapshot {
                    starts_at: subscription.starts_at.map(|date| date.to_rfc3339()),
                    expires_at: subscription.expires_at.map(|date| date.to_rfc3339()),
                    renews_at: subscription.renews_at.map(|date| date.to_rfc3339()),
                    renews_at_is_date_only: subscription.renews_at_is_date_only,
                    expires_at_is_date_only: subscription.expires_at_is_date_only,
                }
            }),
            source_label: result.source_label.clone(),
            has_successful_claude_cli_quota: result.has_successful_claude_cli_quota,
            updated_at: usage.updated_at.to_rfc3339(),
            error: None,
            error_state: codexbar::core::ProviderStateKind::Ready,
            pace,
            pace_authoritative: allows_pace,
            account_organization: usage.account_organization.clone(),
            tray_status_label: None,
            fetch_duration_ms: None,
            wayfinder_usage: result.wayfinder_usage.clone(),
            open_ai_api_usage: result.open_ai_api_usage.as_ref().map(Into::into),
            throughput: result.throughput.as_ref().map(Into::into),
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
            primary: RateWindowSnapshot {
                used_percent: 0.0,
                remaining_percent: 100.0,
                window_minutes: None,
                resets_at: None,
                reset_description: None,
                is_exhausted: false,
                is_informational: false,
                reserve_percent: None,
                reserve_description: None,
                reserve_will_last_to_reset: false,
                reserve_eta_seconds: None,
                pace: None,
                monthly_limit_block: None,
                description_is_detail: false,
                reset_regenerates: false,
                hidden_on_card: false,
            },
            primary_label: Some(metadata.session_label.to_string()),
            secondary: None,
            secondary_label: None,
            secondary_label_key: None,
            model_specific: None,
            tertiary: None,
            tertiary_label: None,
            extra_rate_windows: Vec::new(),
            inventory: Vec::new(),
            display_details: Vec::new(),
            cost: None,
            plan_name: None,
            account_email: None,
            account_label: None,
            subscription: None,
            source_label: String::new(),
            has_successful_claude_cli_quota: false,
            updated_at: chrono::Utc::now().to_rfc3339(),
            error: Some(error),
            error_state: state_kind,
            pace: None,
            pace_authoritative: true,
            account_organization: None,
            tray_status_label: None,
            fetch_duration_ms: None,
            wayfinder_usage: None,
            open_ai_api_usage: None,
            throughput: None,
            session_equivalent_forecast: None,
            quota_burndown: None,
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapState {
    pub(crate) contract_version: &'static str,
    pub(crate) providers: Vec<ProviderCatalogEntry>,
    pub(crate) settings: SettingsSnapshot,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrentSurfaceState {
    pub mode: String,
    pub target: SurfaceTarget,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCatalogEntry {
    pub(crate) id: String,
    pub(crate) display_name: String,
    pub(crate) cookie_domain: Option<String>,
    /// CLI aliases the Settings search also matches (see `ProviderId::cli_aliases`).
    pub(crate) cli_aliases: Vec<String>,
}

/// One provider's service status for the sidebar dot and the tray panel.
/// `status` is `None` until the first successful poll.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderStatusEntry {
    pub provider_id: String,
    pub status: Option<ProviderServiceStatus>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderServiceStatus {
    /// Upstream raw indicator: none, minor, major, critical, maintenance, unknown.
    pub indicator: &'static str,
    pub description: Option<String>,
    /// Unix milliseconds of the feed's last update.
    pub updated_at: Option<i64>,
}

impl From<&codexbar::status::ProviderStatus> for ProviderServiceStatus {
    fn from(status: &codexbar::status::ProviderStatus) -> Self {
        Self {
            indicator: status.indicator.as_raw(),
            description: status.description.clone(),
            updated_at: status.updated_at.map(|at| at.timestamp_millis()),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsSnapshot {
    preferred_currency_code: String,
    enabled_providers: Vec<String>,
    provider_order: Vec<String>,
    refresh_interval_secs: u64,
    adaptive_refresh: bool,
    refresh_all_providers_on_menu_open: bool,
    status_checks_enabled: bool,
    low_power_mode: bool,
    low_power_mode_preference: &'static str,
    start_at_login: bool,
    start_minimized: bool,
    show_notifications: bool,
    sound_enabled: bool,
    notification_sound_theme: codexbar::settings::NotificationSoundTheme,
    notification_sound_paths: codexbar::settings::NotificationSoundPaths,
    high_usage_threshold: f64,
    critical_usage_threshold: f64,
    /// Per-provider quota warnings keyed by CLI name; Global providers are absent.
    provider_quota_warnings:
        std::collections::HashMap<String, codexbar::settings::ProviderQuotaWarnings>,
    predictive_pace_warning_enabled: bool,
    limit_reset_notifications_enabled: bool,
    credential_expiry_notifications_enabled: bool,
    session_quota_notifications_enabled: bool,
    show_pace: bool,
    tray_icon_mode: &'static str,
    stacked_tray_top_provider: Option<String>,
    stacked_tray_bottom_provider: Option<String>,
    switcher_shows_icons: bool,
    menu_bar_shows_highest_usage: bool,
    menu_bar_shows_percent: bool,
    menu_bar_icon_style: codexbar::settings::MenuBarIconStyle,
    merged_overview_selected_providers: Vec<String>,
    quota_warning_markers_visible: bool,
    highlight_menu_cards_on_hover: bool,
    workday_tick_style: codexbar::settings::WorkdayTickStyle,
    menu_bar_color_pace: bool,
    menu_bar_color_by_provider: bool,
    menu_bar_pace_reserve_positive: bool,
    providers_sorted_alphabetically: bool,
    providers_enabled_only: bool,
    show_as_used: bool,
    show_all_token_accounts_in_menu: bool,
    enable_animations: bool,
    reset_time_relative: bool,
    show_reset_when_exhausted: bool,
    menu_bar_display_mode: String,
    overview_layout: String,
    hide_personal_info: bool,
    update_channel: &'static str,
    auto_download_updates: bool,
    install_updates_on_quit: bool,
    global_shortcut: String,
    switcher_shortcuts: std::collections::BTreeMap<String, String>,
    codex_custom_sessions_dirs: Vec<String>,
    antigravity_additional_profile_homes: Vec<String>,
    agent_sessions_enabled: bool,
    stay_awake_enabled: bool,
    agent_session_ssh_hosts: Vec<String>,
    hooks_enabled: bool,
    http_proxy_enabled: bool,
    http_proxy_url: String,
    http_proxy_username: String,
    http_proxy_password: String,
    ui_language: &'static str,
    ui_language_follows_system: bool,
    theme: &'static str,
    window_scale_percent: u16,
    tray_scale_percent: u16,
    tray_panel_always_on_top: bool,
    powertoys_status_pipe_enabled: bool,
    claude_avoid_keychain_prompts: bool,
    claude_swap_enabled: bool,
    claude_swap_executable_path: String,
    codex_spark_usage_visible: bool,
    codex_openai_web_extras: bool,
    codex_historical_tracking: bool,
    codex_external_oauth_sources_allowed: bool,
    codex_local_session_cost_ledger_enabled: bool,
    codex_openai_web_battery_saver_enabled: bool,
    /// OpenAI API Live Usage (upstream #4495): fetch Tokens/min on each refresh.
    #[serde(rename = "openAiLiveUsageEnabled")]
    openai_live_usage_enabled: bool,
    /// Manual per-model organization limits, read by the card's limit line and the editor.
    #[serde(rename = "openAiModelRateLimits")]
    openai_model_rate_limits: Vec<ModelRateLimitSnapshot>,
    disable_keychain_access: bool,
    wayfinder_gateway_url: String,
    provider_metrics: std::collections::HashMap<String, &'static str>,
    float_bar_enabled: bool,
    float_bar_opacity: u8,
    float_bar_scale: u8,
    float_bar_orientation: String,
    float_bar_style: String,
    float_bar_click_through: bool,
    float_bar_provider_ids: Vec<String>,
    float_bar_dark_text: bool,
    float_bar_show_reset_inline: bool,
    float_bar_show_cost: bool,
    promote_tray_icon: bool,
    claude_daily_routines_usage_visible: bool,
    claude_allow_reading_claude_code_credentials: bool,
    alibaba_token_plan_region: String,
    copilot_seat_credit_entitlement: Option<f64>,
    weekly_progress_work_days: Option<u8>,
    cost_summary_display_style: &'static str,
    open_codex_usage_logs_enabled: bool,
    hide_native_codex_cost_when_open_codex_present: bool,
    /// History window as its persisted raw form (`rolling:N`,
    /// `month-to-date`, `all`).
    cost_reporting_period: String,
    cost_usage_enabled: bool,
    /// Usage & Spend time range, same raw form as `cost_reporting_period`.
    spend_dashboard_period: String,
    spend_dashboard_hidden_source_ids: Vec<String>,
    provider_accent_colors: std::collections::HashMap<String, String>,
}

#[tauri::command]
pub fn get_bootstrap_state() -> BootstrapState {
    bootstrap_state_for(Settings::load())
}

pub(crate) fn bootstrap_state_for(settings: Settings) -> BootstrapState {
    BootstrapState {
        contract_version: "v1",
        providers: provider_catalog_for(&settings),
        settings: SettingsSnapshot::from(settings),
    }
}

#[tauri::command]
pub fn get_provider_catalog() -> Vec<ProviderCatalogEntry> {
    provider_catalog_for(&Settings::load())
}

#[tauri::command]
pub fn get_settings_snapshot() -> SettingsSnapshot {
    SettingsSnapshot::from(Settings::load())
}

impl From<Settings> for SettingsSnapshot {
    fn from(settings: Settings) -> Self {
        let avoid_keychain_prompts = settings.claude_avoid_keychain_prompts();
        let claude_swap_enabled = settings.claude_swap_enabled();
        let claude_swap_executable_path = settings.claude_swap_executable_path().to_string();
        let codex_spark_usage_visible = settings.codex_spark_usage_visible();
        let openai_live_usage_enabled = settings.live_usage_enabled(ProviderId::OpenAIApi);
        let openai_model_rate_limits = settings
            .model_rate_limits(ProviderId::OpenAIApi)
            .iter()
            .map(ModelRateLimitSnapshot::from)
            .collect();
        let codex_openai_web_extras = settings.codex_openai_web_extras();
        let codex_historical_tracking = settings.codex_historical_tracking();
        let codex_local_session_cost_ledger_enabled =
            settings.local_session_cost_ledger_enabled(ProviderId::Codex);
        let codex_openai_web_battery_saver_enabled =
            settings.openai_web_battery_saver_enabled(ProviderId::Codex);
        let provider_quota_warnings = settings
            .provider_configs
            .iter()
            .filter(|(_, config)| !config.quota_warnings.is_empty())
            .map(|(id, config)| (id.cli_name().to_string(), config.quota_warnings))
            .collect();
        let wayfinder_gateway_url = settings.gateway_url(ProviderId::Wayfinder).to_string();

        let provider_order = settings.provider_display_order_names();
        let enabled_providers = provider_order
            .iter()
            .filter(|provider_id| settings.enabled_providers.contains(*provider_id))
            .cloned()
            .collect();

        let copilot_seat_credit_entitlement = settings.seat_credit_entitlement(ProviderId::Copilot);

        let provider_metrics = settings
            .provider_metrics
            .into_iter()
            .map(|(k, v)| (k, metric_preference_label(v)))
            .collect();

        Self {
            preferred_currency_code: settings.preferred_currency_code,
            enabled_providers,
            provider_order,
            refresh_interval_secs: settings.refresh_interval_secs,
            adaptive_refresh: settings.adaptive_refresh,
            refresh_all_providers_on_menu_open: settings.refresh_all_providers_on_menu_open,
            status_checks_enabled: settings.status_checks_enabled,
            low_power_mode: settings.low_power_mode_preference
                == codexbar::settings::LowPowerModePreference::On,
            low_power_mode_preference: settings.low_power_mode_preference.as_str(),
            start_at_login: settings.start_at_login,
            start_minimized: settings.start_minimized,
            show_notifications: settings.show_notifications,
            sound_enabled: settings.sound_enabled,
            notification_sound_theme: settings.notification_sound_theme,
            notification_sound_paths: settings.notification_sound_paths,
            high_usage_threshold: settings.high_usage_threshold,
            critical_usage_threshold: settings.critical_usage_threshold,
            provider_quota_warnings,
            predictive_pace_warning_enabled: settings.predictive_pace_warning_enabled,
            limit_reset_notifications_enabled: settings.limit_reset_notifications_enabled,
            credential_expiry_notifications_enabled: settings
                .credential_expiry_notifications_enabled,
            session_quota_notifications_enabled: settings.session_quota_notifications_enabled,
            show_pace: settings.show_pace,
            tray_icon_mode: tray_icon_mode_label(settings.tray_icon_mode),
            stacked_tray_top_provider: settings.stacked_tray_top_provider,
            stacked_tray_bottom_provider: settings.stacked_tray_bottom_provider,
            switcher_shows_icons: settings.switcher_shows_icons,
            menu_bar_shows_highest_usage: settings.menu_bar_shows_highest_usage,
            menu_bar_shows_percent: settings.menu_bar_shows_percent,
            menu_bar_icon_style: settings.menu_bar_icon_style,
            merged_overview_selected_providers: settings.merged_overview_selected_providers,
            quota_warning_markers_visible: settings.quota_warning_markers_visible,
            highlight_menu_cards_on_hover: settings.highlight_menu_cards_on_hover,
            workday_tick_style: settings.workday_tick_style,
            menu_bar_color_pace: settings.menu_bar_color_pace,
            menu_bar_color_by_provider: settings.menu_bar_color_by_provider,
            menu_bar_pace_reserve_positive: settings.menu_bar_pace_reserve_positive,
            providers_sorted_alphabetically: settings.providers_sorted_alphabetically,
            providers_enabled_only: settings.providers_enabled_only,
            show_as_used: settings.show_as_used,
            show_all_token_accounts_in_menu: settings.show_all_token_accounts_in_menu,
            enable_animations: settings.enable_animations,
            reset_time_relative: settings.reset_time_relative,
            show_reset_when_exhausted: settings.show_reset_when_exhausted,
            menu_bar_display_mode: settings.menu_bar_display_mode,
            overview_layout: settings.overview_layout,
            hide_personal_info: settings.hide_personal_info,
            update_channel: update_channel_label(settings.update_channel),
            auto_download_updates: settings.auto_download_updates,
            install_updates_on_quit: settings.install_updates_on_quit,
            switcher_shortcuts: codexbar::switcher_shortcuts::resolve_or_default(
                &settings.switcher_shortcuts,
            ),
            global_shortcut: settings.global_shortcut,
            codex_custom_sessions_dirs: settings.codex_custom_sessions_dirs,
            antigravity_additional_profile_homes: settings.antigravity_additional_profile_homes,
            agent_sessions_enabled: settings.agent_sessions_enabled,
            stay_awake_enabled: settings.stay_awake_enabled,
            agent_session_ssh_hosts: settings.agent_session_ssh_hosts,
            hooks_enabled: settings.hooks_enabled,
            openai_live_usage_enabled,
            openai_model_rate_limits,
            http_proxy_enabled: settings.http_proxy_enabled,
            http_proxy_url: settings.http_proxy_url,
            http_proxy_username: settings.http_proxy_username,
            http_proxy_password: settings.http_proxy_password,
            ui_language: language_label(settings.ui_language),
            ui_language_follows_system: settings.ui_language_follows_system,
            theme: theme_label(settings.theme),
            window_scale_percent: settings.window_scale_percent,
            tray_scale_percent: settings.tray_scale_percent,
            tray_panel_always_on_top: settings.tray_panel_always_on_top,
            powertoys_status_pipe_enabled: settings.powertoys_status_pipe_enabled,
            claude_avoid_keychain_prompts: avoid_keychain_prompts,
            claude_swap_enabled,
            claude_swap_executable_path,
            codex_spark_usage_visible,
            codex_openai_web_extras,
            codex_historical_tracking,
            codex_external_oauth_sources_allowed: settings.codex_external_oauth_sources_allowed,
            codex_local_session_cost_ledger_enabled,
            codex_openai_web_battery_saver_enabled,
            disable_keychain_access: settings.disable_keychain_access,
            wayfinder_gateway_url,
            provider_metrics,
            float_bar_enabled: settings.float_bar_enabled,
            float_bar_opacity: settings.float_bar_opacity,
            float_bar_scale: settings.float_bar_scale,
            float_bar_orientation: settings.float_bar_orientation,
            float_bar_style: settings.float_bar_style,
            float_bar_click_through: settings.float_bar_click_through,
            float_bar_provider_ids: settings.float_bar_provider_ids,
            float_bar_dark_text: settings.float_bar_dark_text,
            float_bar_show_reset_inline: settings.float_bar_show_reset_inline,
            float_bar_show_cost: settings.float_bar_show_cost,
            promote_tray_icon: settings.promote_tray_icon,
            claude_daily_routines_usage_visible: settings.claude_daily_routines_usage_visible,
            claude_allow_reading_claude_code_credentials: settings
                .claude_allow_reading_claude_code_credentials,
            alibaba_token_plan_region: settings.alibaba_token_plan_region,
            copilot_seat_credit_entitlement,
            weekly_progress_work_days: settings.weekly_progress_work_days,
            cost_summary_display_style: cost_summary_display_style_label(
                settings.cost_summary_display_style,
            ),
            open_codex_usage_logs_enabled: settings.open_codex_usage_logs_enabled,
            hide_native_codex_cost_when_open_codex_present: settings
                .hide_native_codex_cost_when_open_codex_present,
            cost_reporting_period: settings.cost_reporting_period.raw(),
            cost_usage_enabled: settings.cost_usage_enabled,
            spend_dashboard_period: settings.spend_dashboard_period.raw(),
            spend_dashboard_hidden_source_ids: settings.spend_dashboard_hidden_source_ids,
            provider_accent_colors: settings
                .provider_configs
                .iter()
                .filter_map(|(id, config)| {
                    config
                        .accent_color
                        .as_ref()
                        .map(|color| (id.cli_name().to_string(), color.clone()))
                })
                .collect(),
        }
    }
}

pub(crate) fn provider_catalog_for(settings: &Settings) -> Vec<ProviderCatalogEntry> {
    // Soft-removed providers (upstream #2254) stay hidden in Settings unless already enabled.
    settings
        .provider_display_order()
        .into_iter()
        .filter(|provider| settings.is_provider_listed(*provider))
        .map(|provider| ProviderCatalogEntry {
            id: provider.cli_name().to_string(),
            display_name: provider.display_name().to_string(),
            cookie_domain: provider.cookie_domain().map(ToString::to_string),
            cli_aliases: provider
                .cli_aliases()
                .iter()
                .map(ToString::to_string)
                .collect(),
        })
        .collect()
}

fn tray_icon_mode_label(mode: TrayIconMode) -> &'static str {
    match mode {
        TrayIconMode::Single => "single",
        TrayIconMode::PerProvider => "perProvider",
        TrayIconMode::Stacked => "stacked",
    }
}

pub(super) fn update_channel_label(channel: UpdateChannel) -> &'static str {
    match channel {
        UpdateChannel::Stable => "stable",
        UpdateChannel::Beta => "beta",
    }
}

pub(super) fn language_label(language: Language) -> &'static str {
    language.label()
}

fn theme_label(theme: ThemePreference) -> &'static str {
    match theme {
        ThemePreference::Auto => "auto",
        ThemePreference::Light => "light",
        ThemePreference::Dark => "dark",
    }
}

fn cost_summary_display_style_label(
    style: codexbar::settings::CostSummaryDisplayStyle,
) -> &'static str {
    match style {
        codexbar::settings::CostSummaryDisplayStyle::Compact => "compact",
        codexbar::settings::CostSummaryDisplayStyle::Detailed => "detailed",
        codexbar::settings::CostSummaryDisplayStyle::Hidden => "hidden",
    }
}

pub(crate) fn parse_cost_summary_display_style(
    s: &str,
) -> Option<codexbar::settings::CostSummaryDisplayStyle> {
    use codexbar::settings::CostSummaryDisplayStyle;
    match s {
        "compact" => Some(CostSummaryDisplayStyle::Compact),
        "detailed" => Some(CostSummaryDisplayStyle::Detailed),
        "hidden" => Some(CostSummaryDisplayStyle::Hidden),
        _ => None,
    }
}

pub(super) fn parse_theme(s: &str) -> Option<ThemePreference> {
    match s {
        "auto" => Some(ThemePreference::Auto),
        "light" => Some(ThemePreference::Light),
        "dark" => Some(ThemePreference::Dark),
        _ => None,
    }
}

fn metric_preference_label(pref: MetricPreference) -> &'static str {
    match pref {
        MetricPreference::Automatic => "automatic",
        MetricPreference::Session => "session",
        MetricPreference::Weekly => "weekly",
        MetricPreference::Model => "model",
        MetricPreference::Tertiary => "tertiary",
        MetricPreference::Credits => "credits",
        MetricPreference::ExtraUsage => "extraUsage",
        MetricPreference::MonthlyPlan => "monthlyPlan",
        MetricPreference::Average => "average",
    }
}

pub(super) fn parse_metric_preference(s: &str) -> Option<MetricPreference> {
    match s {
        "automatic" => Some(MetricPreference::Automatic),
        "session" => Some(MetricPreference::Session),
        "weekly" => Some(MetricPreference::Weekly),
        "model" => Some(MetricPreference::Model),
        "tertiary" => Some(MetricPreference::Tertiary),
        "credits" => Some(MetricPreference::Credits),
        "extraUsage" | "extrausage" => Some(MetricPreference::ExtraUsage),
        "monthlyPlan" | "monthlyplan" => Some(MetricPreference::MonthlyPlan),
        "average" => Some(MetricPreference::Average),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot_window_with(
        used_percent: f64,
        window_minutes: Option<u32>,
        resets_at: Option<chrono::DateTime<chrono::Utc>>,
        reset_description: Option<String>,
    ) -> RateWindowSnapshot {
        RateWindowSnapshot {
            used_percent,
            remaining_percent: 100.0 - used_percent,
            window_minutes,
            resets_at: resets_at.map(|dt| dt.to_rfc3339()),
            reset_description,
            is_exhausted: false,
            is_informational: false,
            reserve_percent: None,
            reserve_description: None,
            reserve_will_last_to_reset: false,
            reserve_eta_seconds: None,
            pace: None,
            monthly_limit_block: None,
            description_is_detail: false,
            reset_regenerates: false,
            hidden_on_card: false,
        }
    }

    /// Notion names its billing lane with the "Monthly" app string
    /// (upstream `NotionProviderDescriptor.swift:32`); a provider that only
    /// sets the raw "Monthly" label sends no key.
    #[test]
    fn secondary_label_key_crosses_the_bridge_only_when_the_provider_sets_it() {
        let now = chrono::Utc::now();
        let monthly = || {
            RateWindow::with_details(
                25.0,
                Some(43_200),
                Some(now + chrono::Duration::days(9)),
                None,
            )
        };
        let bridge = |id: ProviderId, usage: codexbar::core::UsageSnapshot| {
            let metadata = codexbar::core::instantiate_provider(id).metadata().clone();
            let result = ProviderFetchResult::new(usage, "web");
            serde_json::to_value(ProviderUsageSnapshot::from_fetch_result(
                id, &metadata, &result, None, None,
            ))
            .unwrap()
        };

        let notion = bridge(
            ProviderId::Notion,
            codexbar::core::UsageSnapshot::new(monthly())
                .with_secondary(monthly())
                .with_secondary_label_key(codexbar::locale::LocaleKey::ProviderLabelMonthly),
        );
        assert_eq!(notion["secondaryLabelKey"], "ProviderLabelMonthly");
        assert_eq!(notion["secondaryLabel"], "Monthly");

        let augment = bridge(
            ProviderId::Augment,
            codexbar::core::UsageSnapshot::new(monthly()).with_secondary(monthly()),
        );
        assert!(augment.get("secondaryLabelKey").is_none());
        assert_eq!(augment["secondaryLabel"], "Monthly");
    }

    #[test]
    fn tray_status_prefers_relative_reset_countdown() {
        let window = snapshot_window_with(
            13.0,
            Some(300),
            Some(chrono::Utc::now() + chrono::Duration::minutes(125)),
            Some("Jun 10 at 3:00PM".to_string()),
        );

        let label = compact_tray_status_label(&window, Language::English);

        assert!(label.starts_with("13% • Resets in 2h "));
        assert!(label.ends_with('m'));
        assert!(!label.contains("Jun 10"));
    }

    #[test]
    fn tray_status_normalizes_fallback_reset_description() {
        let window = snapshot_window_with(8.0, Some(300), None, Some("2h 05m".to_string()));

        assert_eq!(
            compact_tray_status_label(&window, Language::English),
            "8% • Resets in 2h 05m"
        );
    }

    #[test]
    fn credit_balance_detail_crosses_the_bridge_and_is_not_a_reset_phrase() {
        let rw = RateWindow::with_details(25.0, None, None, Some("750 / 1000 credits left".into()))
            .with_description_as_detail();
        let window = RateWindowSnapshot::from_rate_window(&rw);

        assert!(window.description_is_detail);
        assert_eq!(
            window.reset_description.as_deref(),
            Some("750 / 1000 credits left")
        );
        let json = serde_json::to_value(&window).unwrap();
        assert_eq!(json["descriptionIsDetail"], true);
        assert_eq!(compact_tray_status_label(&window, Language::English), "25%");

        let plain = RateWindowSnapshot::from_rate_window(&RateWindow::new(10.0));
        let json = serde_json::to_value(&plain).unwrap();
        assert_eq!(json["descriptionIsDetail"], false);
    }

    #[test]
    fn japanese_tray_status_label_has_no_english_reset_text() {
        use codexbar::settings::Language;

        let window = snapshot_window_with(
            13.0,
            Some(300),
            Some(chrono::Utc::now() + chrono::Duration::minutes(125)),
            None,
        );

        let label = compact_tray_status_label(&window, Language::Japanese);

        assert!(label.contains("リセットまで"), "{label}");
        assert!(!label.to_ascii_lowercase().contains("resets in"), "{label}");
        assert!(label.contains("13%"), "{label}");
    }

    #[test]
    fn japanese_tray_status_strips_english_fallback_reset_prefix() {
        use codexbar::settings::Language;

        let window =
            snapshot_window_with(8.0, Some(300), None, Some("Resets in 2h 05m".to_string()));

        let label = compact_tray_status_label(&window, Language::Japanese);

        assert!(label.contains("リセットまで"), "{label}");
        assert!(!label.to_ascii_lowercase().contains("resets in"), "{label}");
        assert!(label.contains("2時間 05分"), "{label}");
    }

    #[test]
    fn tray_status_label_relocalizes_without_refetch() {
        let window = snapshot_window_with(
            13.0,
            Some(300),
            Some(chrono::Utc::now() + chrono::Duration::minutes(125)),
            None,
        );

        let english = compact_tray_status_label(&window, Language::English);
        let japanese = compact_tray_status_label(&window, Language::Japanese);

        assert!(english.contains("Resets in"), "{english}");
        assert!(japanese.contains("リセットまで"), "{japanese}");
        assert!(
            !japanese.to_ascii_lowercase().contains("resets in"),
            "{japanese}"
        );
    }

    /// Provider-declared card wording (Synthetic's regen countdown, LithosAI's
    /// card-hidden headline and cost titles) reaches the card.
    #[test]
    fn provider_card_wording_crosses_the_bridge() {
        let regen =
            RateWindowSnapshot::from_rate_window(&RateWindow::new(10.0).with_regenerating_reset());
        let hidden = RateWindowSnapshot::from_rate_window(
            &RateWindow::informational("Balance: $1.00").hidden_on_card(),
        );
        let regen_json = serde_json::to_value(&regen).unwrap();
        let hidden_json = serde_json::to_value(&hidden).unwrap();
        assert_eq!(regen_json["resetRegenerates"], true);
        assert!(regen_json.get("hiddenOnCard").is_none());
        assert_eq!(hidden_json["hiddenOnCard"], true);
        assert!(hidden_json.get("resetRegenerates").is_none());

        let metadata = codexbar::core::instantiate_provider(ProviderId::LithosAI)
            .metadata()
            .clone();
        let cost = codexbar::core::CostSnapshot::new(0.0, "USD", "Prepaid credits")
            .with_balance(4.0)
            .with_card_title(codexbar::locale::LocaleKey::CreditsTitle);
        let result = codexbar::core::ProviderFetchResult::new(
            codexbar::core::UsageSnapshot::new(RateWindow::new(0.0)),
            "web",
        )
        .with_cost(cost);
        let snapshot = ProviderUsageSnapshot::from_fetch_result(
            ProviderId::LithosAI,
            &metadata,
            &result,
            None,
            None,
        );
        let json = serde_json::to_value(&snapshot).unwrap();
        assert_eq!(json["cost"]["titleKey"], "CreditsTitle");

        let plain = codexbar::core::ProviderFetchResult::new(
            codexbar::core::UsageSnapshot::new(RateWindow::new(0.0)),
            "web",
        )
        .with_cost(codexbar::core::CostSnapshot::new(1.0, "USD", "This month"));
        let json = serde_json::to_value(ProviderUsageSnapshot::from_fetch_result(
            ProviderId::LithosAI,
            &metadata,
            &plain,
            None,
            None,
        ))
        .unwrap();
        assert!(json["cost"].get("titleKey").is_none());
    }
}
