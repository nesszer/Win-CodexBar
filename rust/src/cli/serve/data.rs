//! `/usage` and `/cost` data route handlers.
//!
//! Moved verbatim from the pre-0.48.0 serve module; the only 0.48.0 change is
//! the additive `daily` field on `/cost` — the web dashboard's daily spend bar
//! charts ride this array (upstream #2722 fetches `/cost` for the same data).
//!
//! `--request-timeout` (upstream parity) bounds both routes. Each provider gets
//! 0.8 of the request timeout: `/usage` fetches providers concurrently against
//! one deadline counted from the request start, and `/cost` scans them one
//! after another, each with a fresh budget capped at the request deadline. A
//! provider out of budget becomes a "timed out" row while its work carries on
//! in [`ServeOperations`], so the next request joins it rather than starting
//! another fetch. Upstream also serves the late result from a response cache;
//! this port has none, which is why the timeout is off by default.
//!
//! `/usage` error rows carry `errorKind` (and `signInUrl` for a browser
//! sign-in), like `usage --json`; see [`crate::cli::error_kind`]. `/cost`
//! timeout and failure rows carry `errorKind` too.

use std::time::Duration;

use futures::future::join_all;
use serde_json::{Value, json};
use tokio::time::Instant;

use crate::cli::cost_period::{cost_totals_json, rolling_window_days, stamp_period, window_days};
use crate::cli::error_kind::{
    ERROR_KIND_TIMEOUT, ERROR_KIND_UNKNOWN, error_row, provider_error_row,
};
use crate::cli::fetch_context::populate_api_region_from_settings;
use crate::cli::usage::ProviderSelection;
use crate::core::{CostScanOptions, FetchContext, ProviderId, instantiate_provider};
use crate::cost_reporting_period::CostReportingPeriod;
use crate::cost_scanner::{self, CostScanner};
use crate::settings::Settings;

use super::json_response;
use super::operations::{OperationMiss, ServeOperations};

/// Share of the request timeout each provider gets (upstream
/// `serveProviderTimeout`), so provider rows time out before the request
/// answers 504.
const PROVIDER_TIMEOUT_SHARE: f64 = 0.8;

/// Web fetch timeout, in seconds, while `--request-timeout` is off.
const DEFAULT_WEB_TIMEOUT_SECS: u64 = 60;

/// Deadlines for one `/usage` or `/cost` request (upstream
/// `serveRequestDeadline`, `serveProviderTimeout` and
/// `serveCostProviderDeadline`).
#[derive(Debug, Clone, Copy)]
pub(super) struct RequestBudget {
    started: Instant,
    timeout: Option<Duration>,
}

impl RequestBudget {
    /// Start the clock for one request; `None` disables every deadline.
    pub(super) fn start(timeout: Option<Duration>) -> Self {
        Self {
            started: Instant::now(),
            timeout,
        }
    }

    /// When the whole response is due; past it the route answers 504.
    pub(super) fn request_deadline(&self) -> Option<Instant> {
        self.timeout.map(|timeout| self.started + timeout)
    }

    fn provider_timeout(&self) -> Option<Duration> {
        self.timeout
            .map(|timeout| timeout.mul_f64(PROVIDER_TIMEOUT_SHARE))
    }

    /// `/usage` provider deadline, counted from the request start.
    fn usage_provider_deadline(&self) -> Option<Instant> {
        self.provider_timeout()
            .map(|timeout| self.started + timeout)
    }

    /// `/cost` deadline for a scan that starts at `now`: a full provider
    /// budget, never past the request deadline.
    fn cost_provider_deadline(&self, now: Instant) -> Option<Instant> {
        let provider = now + self.provider_timeout()?;
        Some(
            self.request_deadline()
                .map_or(provider, |request| provider.min(request)),
        )
    }

    /// Provider web fetch timeout: the provider budget (upstream passes it as
    /// `webTimeout`), or 60 s while the request timeout is off.
    fn web_timeout_secs(&self) -> u64 {
        self.provider_timeout()
            .map_or(DEFAULT_WEB_TIMEOUT_SECS, |timeout| timeout.as_secs().max(1))
    }
}

/// In-flight `/usage` fetches and `/cost` scans, shared by every request.
#[derive(Debug, Clone, Default)]
pub(super) struct DataOperations {
    usage: ServeOperations<Value>,
    cost: ServeOperations<Value>,
}

/// `/usage` provider selection. Upstream `serveProviderSelection`: an absent
/// or empty `provider` query follows the enabled providers, like a plain
/// `codexbar usage`; `enabled` runs only in that case.
pub(super) fn usage_selection(
    provider: Option<&str>,
    enabled: impl FnOnce() -> Vec<ProviderId>,
) -> anyhow::Result<ProviderSelection> {
    ProviderSelection::from_arg_or_enabled(provider.filter(|raw| !raw.is_empty()), enabled)
}

pub(super) async fn usage_response(
    provider: Option<&str>,
    budget: RequestBudget,
    operations: &DataOperations,
) -> String {
    let settings = Settings::load();
    let selection = match usage_selection(provider, || settings.get_enabled_provider_ids()) {
        Ok(selection) => selection,
        Err(error) => {
            return json_response(400, json!({ "error": error.to_string() }));
        }
    };
    let ctx = FetchContext {
        web_timeout: budget.web_timeout_secs(),
        // Serve `/usage` is a background poll read: keep the short optional-
        // join grace (upstream #2583), unlike `codexbar usage` which blocks
        // for the full completeness window.
        requires_optional_usage_completeness: false,
        ..FetchContext::default()
    };

    let rows = collect_usage_rows(
        selection.as_list(),
        budget.usage_provider_deadline(),
        &operations.usage,
        |provider_id| {
            let mut provider_ctx = ctx.clone();
            populate_api_region_from_settings(provider_id, &settings, &mut provider_ctx);
            fetch_usage_row(provider_id, provider_ctx)
        },
    )
    .await;
    json_response(200, Value::Array(rows))
}

/// One provider's `/usage` row.
async fn fetch_usage_row(provider_id: ProviderId, ctx: FetchContext) -> Value {
    let provider = instantiate_provider(provider_id);
    match provider.fetch_usage(&ctx).await {
        Ok(result) => json!({
            "provider": provider_id.cli_name(),
            "source": result.source_label,
            "usage": result.usage,
            "cost": result.cost,
        }),
        Err(error) => provider_error_row(provider.as_ref(), &error),
    }
}

/// `/usage` rows in `providers` order (upstream `serveCollectUsageOutputs`).
/// Providers are fetched concurrently; one still running at `deadline`
/// becomes a timeout row, and its fetch carries on for a later request to
/// join.
async fn collect_usage_rows<F, Fut>(
    providers: Vec<ProviderId>,
    deadline: Option<Instant>,
    operations: &ServeOperations<Value>,
    fetch: F,
) -> Vec<Value>
where
    F: Fn(ProviderId) -> Fut,
    Fut: Future<Output = Value> + Send + 'static,
{
    let fetch = &fetch;
    join_all(providers.into_iter().map(|provider_id| async move {
        operations
            .value(provider_id.cli_name(), deadline, || fetch(provider_id))
            .await
            .unwrap_or_else(|miss| usage_miss_row(provider_id, miss))
    }))
    .await
}

/// Row for a provider whose `/usage` fetch gave no value in time.
fn usage_miss_row(provider_id: ProviderId, miss: OperationMiss) -> Value {
    let name = provider_id.cli_name();
    let (error, kind) = match miss {
        OperationMiss::TimedOut => (format!("{name} usage timed out"), ERROR_KIND_TIMEOUT),
        OperationMiss::Failed => (format!("{name} usage failed"), ERROR_KIND_UNKNOWN),
    };
    error_row(provider_id, &error, kind, None)
}

pub(super) async fn cost_response(
    provider: Option<&str>,
    budget: RequestBudget,
    operations: &DataOperations,
) -> String {
    let selection = match ProviderSelection::from_arg(provider) {
        Ok(selection) => selection,
        Err(error) => {
            return json_response(400, json!({ "error": error.to_string() }));
        }
    };
    // The saved selection is read per request, so a change in Settings (or a
    // month rollover for month to date) applies without restarting `serve`.
    // There is no `/cost` response cache; the scanners' own caches are keyed by
    // the resolved day range, so one period never reuses another's entries.
    let period = Settings::load().cost_reporting_period;
    let rows = collect_cost_rows(
        selection.as_list(),
        &period.raw(),
        budget,
        &operations.cost,
        |provider_id| scan_cost_row(provider_id, period),
    )
    .await;
    json_response(200, Value::Array(rows))
}

/// `/cost` rows in `providers` order (upstream `serveCollectCostPayloads`).
/// Scans run one after another; each gets its own provider budget from when
/// it starts, capped at the request deadline, and a scan still running then
/// becomes a timeout row.
async fn collect_cost_rows<F, Fut>(
    providers: Vec<ProviderId>,
    period_key: &str,
    budget: RequestBudget,
    operations: &ServeOperations<Value>,
    scan: F,
) -> Vec<Value>
where
    F: Fn(ProviderId) -> Fut,
    Fut: Future<Output = Value> + Send + 'static,
{
    let mut rows = Vec::with_capacity(providers.len());
    for provider_id in providers {
        let deadline = budget.cost_provider_deadline(Instant::now());
        let key = format!("{}|{period_key}", provider_id.cli_name());
        let row = operations
            .value(&key, deadline, || scan(provider_id))
            .await
            .unwrap_or_else(|miss| cost_miss_row(provider_id, miss));
        rows.push(row);
    }
    rows
}

/// Row for a provider whose `/cost` scan gave no value in time.
fn cost_miss_row(provider_id: ProviderId, miss: OperationMiss) -> Value {
    let name = provider_id.cli_name();
    let (error, kind) = match miss {
        OperationMiss::TimedOut => (format!("{name} cost refresh timed out"), ERROR_KIND_TIMEOUT),
        OperationMiss::Failed => (format!("{name} cost refresh failed"), ERROR_KIND_UNKNOWN),
    };
    error_row(provider_id, &error, kind, None)
}

/// One provider's `/cost` row. The scans read local history synchronously,
/// so they run on the blocking pool, where a request that stops waiting
/// cannot stall the server.
async fn scan_cost_row(provider_id: ProviderId, period: CostReportingPeriod) -> Value {
    tokio::task::spawn_blocking(move || cost_row(provider_id, period))
        .await
        .unwrap_or_else(|_| cost_miss_row(provider_id, OperationMiss::Failed))
}

/// Scan one provider's local cost history for `period` (blocking).
fn cost_row(provider_id: ProviderId, period: CostReportingPeriod) -> Value {
    let days = window_days(period);
    if provider_id == ProviderId::Antigravity {
        use crate::providers::antigravity::local_sessions;
        let history = local_sessions::summarize(days);
        // Upstream 0.64 `serve` refreshes unknown-model pricing in the
        // background; a later `/cost` read picks up the new prices. Blocking
        // pool threads run inside the runtime, so the refresh can be spawned.
        if let Some(refresh) = local_sessions::background_pricing_refresh(&history) {
            tokio::spawn(refresh);
        }
        let mut payload =
            crate::spend_contract::local_token_history_json("antigravity", &history, days);
        stamp_period(&mut payload, period);
        return payload;
    }
    if provider_id == ProviderId::Muse {
        let report = crate::providers::muse::local_usage::scan(days, None);
        let history: crate::spend_contract::LocalTokenHistorySummary = report.into();
        let mut payload = crate::spend_contract::local_token_history_json("muse", &history, days);
        stamp_period(&mut payload, period);
        return payload;
    }
    let scanner = CostScanner::for_period(period).with_options(CostScanOptions::app_driven());
    let (summary, daily) = match provider_id {
        ProviderId::Codex => (scanner.scan_codex(), None),
        ProviderId::Claude => {
            let snapshot = scanner.scan_claude_chart_snapshot_with_cancel(None);
            (
                snapshot.summary,
                Some((snapshot.daily_cost, snapshot.daily_incomplete)),
            )
        }
        ProviderId::Pi => (scanner.scan_pi(), None),
        _ => {
            return json!({
                "provider": provider_id.cli_name(),
                "supported": false,
                "error": "Local cost scanning not available for this provider"
            });
        }
    };
    // Claude's snapshot derives the summary, chart rows and incomplete markers
    // in one transcript walk. Other providers use the shared daily-history
    // path, capped at one year for All.
    let (daily_cost, daily_incomplete) = daily.unwrap_or_else(|| {
        cost_scanner::get_daily_cost_and_incomplete_history(
            provider_id.cli_name(),
            rolling_window_days(period),
        )
    });
    let mut payload = json!({
        "provider": provider_id.cli_name(),
        "supported": true,
        "days_scanned": days,
        "totals": cost_totals_json(provider_id.cli_name(), &summary),
        "cost": {
            "total_usd": summary.total_cost_usd,
            "currency": "USD"
        },
        "daily": daily_json_with_incomplete(daily_cost, &daily_incomplete),
        "tokens": {
            "input": summary.input_tokens,
            "output": summary.output_tokens,
            "cached": summary.cached_tokens
        },
        "sessions_count": summary.sessions_count,
        "by_model": summary.by_model,
    });
    // Only emitted when a scan excluded preliminary Claude proxy rows.
    if summary.incomplete_request_count > 0
        && let Some(object) = payload.as_object_mut()
    {
        object.insert(
            "incompleteRequestCount".to_string(),
            json!(summary.incomplete_request_count),
        );
    }
    stamp_period(&mut payload, period);
    payload
}

/// Dashboard-charts shape for one provider's daily spend: [{date, totalCost}].
/// Days with incomplete Claude requests also carry `incompleteRequestCount`.
#[cfg(test)]
fn daily_json(daily: Vec<(String, Option<f64>)>) -> serde_json::Value {
    daily_json_with_incomplete(daily, &[])
}

fn daily_json_with_incomplete(
    daily: Vec<(String, Option<f64>)>,
    incomplete: &[(String, u32)],
) -> serde_json::Value {
    serde_json::Value::Array(
        daily
            .into_iter()
            .map(|(date, cost_usd)| {
                let count = incomplete
                    .iter()
                    .find(|(day, _)| *day == date)
                    .map(|(_, count)| *count)
                    .filter(|count| *count > 0);
                let mut row = json!({ "date": date, "totalCost": cost_usd });
                if let (Some(count), Some(object)) = (count, row.as_object_mut()) {
                    object.insert("incompleteRequestCount".to_string(), json!(count));
                }
                row
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn budget(timeout: Option<Duration>) -> RequestBudget {
        RequestBudget::start(timeout)
    }

    /// Never resolves on its own; the paused test clock jumps to the deadline.
    async fn stuck_row() -> Value {
        std::future::pending::<Value>().await
    }

    fn ready_row(provider_id: ProviderId) -> Value {
        json!({ "provider": provider_id.cli_name(), "usage": {} })
    }

    /// The paused clock lands on whole-millisecond timer ticks.
    fn assert_elapsed(started: Instant, expected: Duration) {
        let elapsed = started.elapsed();
        assert!(
            elapsed >= expected && elapsed < expected + Duration::from_millis(5),
            "elapsed {elapsed:?}, expected {expected:?}"
        );
    }

    #[test]
    fn budget_off_sets_no_deadlines() {
        let off = budget(None);
        assert_eq!(off.request_deadline(), None);
        assert_eq!(off.usage_provider_deadline(), None);
        assert_eq!(off.cost_provider_deadline(Instant::now()), None);
        assert_eq!(off.web_timeout_secs(), 60);
    }

    #[test]
    fn providers_get_four_fifths_of_the_request_budget() {
        let budget = budget(Some(Duration::from_secs(10)));
        let started = budget.started;
        assert_eq!(
            budget.request_deadline(),
            Some(started + Duration::from_secs(10))
        );
        assert_eq!(
            budget.usage_provider_deadline(),
            Some(started + Duration::from_secs(8))
        );
        // A cost scan gets a full provider budget from its own start ...
        assert_eq!(
            budget.cost_provider_deadline(started + Duration::from_secs(1)),
            Some(started + Duration::from_secs(9))
        );
        // ... but never runs past the request deadline.
        assert_eq!(
            budget.cost_provider_deadline(started + Duration::from_secs(5)),
            Some(started + Duration::from_secs(10))
        );
        assert_eq!(budget.web_timeout_secs(), 8);
        let short = RequestBudget::start(Some(Duration::from_millis(500)));
        assert_eq!(
            short.web_timeout_secs(),
            1,
            "web timeouts stay at least 1 s"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn usage_rows_keep_order_and_time_out_only_the_slow_provider() {
        let operations = ServeOperations::<Value>::default();
        let budget = budget(Some(Duration::from_secs(10)));
        let started = Instant::now();
        let rows = collect_usage_rows(
            vec![ProviderId::Claude, ProviderId::Codex, ProviderId::Pi],
            budget.usage_provider_deadline(),
            &operations,
            |provider_id| async move {
                if provider_id == ProviderId::Claude {
                    stuck_row().await
                } else {
                    ready_row(provider_id)
                }
            },
        )
        .await;
        assert_elapsed(started, Duration::from_secs(8));
        assert_eq!(
            rows,
            vec![
                json!({
                    "provider": "claude",
                    "error": "claude usage timed out",
                    "errorKind": "timeout",
                }),
                json!({ "provider": "codex", "usage": {} }),
                json!({ "provider": "pi", "usage": {} }),
            ]
        );
        assert!(
            operations.is_running("claude"),
            "the slow fetch keeps running for the next request"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn usage_rows_wait_for_every_provider_without_a_deadline() {
        let operations = ServeOperations::<Value>::default();
        let rows = collect_usage_rows(
            vec![ProviderId::Codex, ProviderId::Claude],
            None,
            &operations,
            |provider_id| async move {
                tokio::time::sleep(Duration::from_secs(120)).await;
                ready_row(provider_id)
            },
        )
        .await;
        assert_eq!(
            rows,
            vec![
                json!({ "provider": "codex", "usage": {} }),
                json!({ "provider": "claude", "usage": {} }),
            ]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn cost_rows_scan_in_order_with_a_fresh_capped_budget_each() {
        let operations = ServeOperations::<Value>::default();
        let budget = budget(Some(Duration::from_secs(10)));
        let started = Instant::now();
        let rows = collect_cost_rows(
            vec![ProviderId::Codex, ProviderId::Claude, ProviderId::Pi],
            "rolling:30",
            budget,
            &operations,
            |provider_id| async move {
                match provider_id {
                    // Codex outlives its 8 s budget; Claude then has the 2 s
                    // left before the request deadline and needs 1 s; Pi
                    // finishes at once inside the last second.
                    ProviderId::Codex => stuck_row().await,
                    ProviderId::Claude => {
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        ready_row(provider_id)
                    }
                    _ => ready_row(provider_id),
                }
            },
        )
        .await;
        assert_elapsed(started, Duration::from_secs(9));
        assert_eq!(
            rows,
            vec![
                json!({
                    "provider": "codex",
                    "error": "codex cost refresh timed out",
                    "errorKind": "timeout",
                }),
                json!({ "provider": "claude", "usage": {} }),
                json!({ "provider": "pi", "usage": {} }),
            ]
        );
        assert!(operations.is_running("codex|rolling:30"));
    }

    #[tokio::test(start_paused = true)]
    async fn cost_scans_past_the_request_deadline_never_start() {
        use std::sync::{Arc, Mutex};

        let operations = ServeOperations::<Value>::default();
        let budget = budget(Some(Duration::from_secs(10)));
        let started = Instant::now();
        let scans = Arc::new(Mutex::new(Vec::new()));
        let rows = collect_cost_rows(
            vec![ProviderId::Codex, ProviderId::Claude, ProviderId::Pi],
            "all",
            budget,
            &operations,
            |provider_id| {
                let scans = Arc::clone(&scans);
                async move {
                    scans.lock().unwrap().push(provider_id.cli_name());
                    // Outlives whatever is left of the request budget.
                    tokio::time::sleep(Duration::from_secs(60)).await;
                    ready_row(provider_id)
                }
            },
        )
        .await;
        // Codex times out at 8 s, Claude at the 10 s request deadline, and Pi
        // starts with no budget left.
        assert_elapsed(started, Duration::from_secs(10));
        assert_eq!(
            rows,
            vec![
                json!({
                    "provider": "codex",
                    "error": "codex cost refresh timed out",
                    "errorKind": "timeout",
                }),
                json!({
                    "provider": "claude",
                    "error": "claude cost refresh timed out",
                    "errorKind": "timeout",
                }),
                json!({
                    "provider": "pi",
                    "error": "pi cost refresh timed out",
                    "errorKind": "timeout",
                }),
            ]
        );
        assert_eq!(*scans.lock().unwrap(), vec!["codex", "claude"]);
        assert!(!operations.is_running("pi|all"), "Pi's scan never started");
    }

    #[test]
    fn miss_rows_name_the_provider_and_the_reason() {
        assert_eq!(
            usage_miss_row(ProviderId::Codex, OperationMiss::Failed),
            json!({ "provider": "codex", "error": "codex usage failed", "errorKind": "unknown" })
        );
        assert_eq!(
            cost_miss_row(ProviderId::Claude, OperationMiss::Failed),
            json!({
                "provider": "claude",
                "error": "claude cost refresh failed",
                "errorKind": "unknown",
            })
        );
    }

    #[test]
    fn daily_array_shape_matches_dashboard_charts_contract() {
        let daily = daily_json_with_incomplete(
            vec![
                ("2026-08-07".to_string(), Some(0.0)),
                ("2026-08-08".to_string(), Some(4.25)),
                ("2026-08-09".to_string(), None),
            ],
            &[("2026-08-09".to_string(), 2)],
        );
        let rows = daily.as_array().unwrap();
        assert!(rows[0].get("incompleteRequestCount").is_none());
        assert_eq!(rows[2]["incompleteRequestCount"], 2);
        assert_eq!(rows[0]["date"], "2026-08-07");
        assert_eq!(rows[1]["totalCost"], 4.25);
        assert_eq!(rows[0]["totalCost"], 0.0);
        assert!(rows[2]["totalCost"].is_null());
    }

    #[test]
    fn antigravity_cost_payload_is_token_only_and_preserves_partial_unknown() {
        use crate::spend_contract::{LocalHistoryCoverage, LocalTokenHistorySummary};
        let complete = crate::spend_contract::local_token_history_json(
            "antigravity",
            &LocalTokenHistorySummary {
                total_tokens: 42,
                session_count: 1,
                coverage: LocalHistoryCoverage::Complete,
                cost_estimate: Default::default(),
                ..Default::default()
            },
            30,
        );
        assert!(complete["cost"]["total_usd"].is_null());
        assert_eq!(complete["tokens"]["total"], 42);
        assert_eq!(complete["historyCoverage"], "complete");

        let partial = crate::spend_contract::local_token_history_json(
            "antigravity",
            &LocalTokenHistorySummary {
                total_tokens: 42,
                session_count: 1,
                coverage: LocalHistoryCoverage::Partial,
                cost_estimate: Default::default(),
                ..Default::default()
            },
            30,
        );
        assert!(partial["tokens"]["total"].is_null());
        assert_eq!(partial["historyCoverage"], "partial");
    }
    #[test]
    fn daily_rows_use_upstream_total_cost_key_only() {
        let daily = daily_json(vec![
            ("2026-08-07".to_string(), Some(0.0)),
            ("2026-08-08".to_string(), Some(4.25)),
            ("2026-08-09".to_string(), None),
        ]);
        let serialized = daily.to_string();
        assert!(
            serialized.contains("\"totalCost\""),
            "wire key is totalCost"
        );
        assert!(
            !serialized.contains("cost_usd") && !serialized.contains("costUSD"),
            "no stale daily cost keys may leak to the wire"
        );
    }

    #[test]
    fn daily_empty_array_has_no_rows() {
        let daily = daily_json(vec![]);
        assert_eq!(daily.as_array().unwrap().len(), 0);
    }

    #[test]
    fn daily_zero_values_are_preserved_not_filtered() {
        let daily = daily_json(vec![("2026-08-07".to_string(), Some(0.0))]);
        let rows = daily.as_array().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["totalCost"], 0.0);
    }
}
