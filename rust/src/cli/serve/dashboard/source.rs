//! Production dashboard snapshot producer: collects provider usage (bounded,
//! concurrent), local cost-scan data (off-runtime), and Claude token-account
//! rows, then projects them through the pure [`build_snapshot`] mapping.
//!
//! F9/#2717 parity notes (0.48.0): each provider fetch is individually bounded
//! so one slow provider cannot bar the snapshot; a timed-out/failed provider
//! becomes an error ROW, the build still completes, and every waiter receives
//! the finished (late) result via the coordinator — never a discarded build.

use std::collections::{BTreeSet, HashMap};
use std::pin::Pin;
use std::time::Duration;

use chrono::Utc;
use futures::StreamExt;
use tokio::time::Instant;

use crate::cli::fetch_context::populate_api_region_from_settings;
use crate::core::{
    CostScanOptions, FetchContext, ProviderFetchResult, ProviderId, TokenAccount,
    TokenAccountSupport, build_fetch_context, instantiate_provider,
    token_account_session_is_passive,
};
use crate::cost_scanner::{self, CostScanner};
use crate::settings::{ApiKeys, ManualCookies, Settings};
use crate::spend_contract::build_local_spend_contract_from_summary;

use crate::cli::serve::collection::{
    AccountFetchEnvelope, ClaudeAccountsInput, ProviderFetchEnvelope, RawCostPayload,
    SnapshotCollection,
};
use crate::cli::serve::dashboard::coordinator::{BoxSnapshotArtifactsFuture, SnapshotArtifacts};
use crate::cli::serve::metrics::MetricsSnapshot;

use super::snapshot::{DashboardIdentity, SnapshotInput, SnapshotPayload, build_snapshot};

pub type BoxSnapshotFuture = Pin<Box<dyn Future<Output = Result<SnapshotPayload, String>> + Send>>;

/// Hard bound per provider fetch inside a dashboard build. Existing serve
/// `web_timeout` is 60 s; builds add a 75 s outer envelope (provider-internal
/// bounds stay authoritative, matching upstream's 0.8x-below-the-deadline rule
/// of thumb in spirit: anything genuinely stuck becomes an error row, and the
/// snapshot still completes).
const PROVIDER_FETCH_TIMEOUT: Duration = Duration::from_secs(75);
/// Same bound for one Claude account fetch.
const ACCOUNT_FETCH_TIMEOUT: Duration = Duration::from_secs(75);
/// Most token-account fetches in flight for one provider that declares no
/// minimum delay between account refreshes. Same bound as the provider
/// fan-out of `codexbar usage` (`MAX_CONCURRENT_PROVIDER_FETCHES`).
const MAX_CONCURRENT_ACCOUNT_FETCHES: usize = 3;

/// Collects the stable dashboard-v1 payload independently of its transport
/// (upstream `DashboardSnapshotProducer.live` analog): `codexbar serve` wraps
/// it in the authenticated + cached route, `codexbar dashboard` runs it once.
#[derive(Clone, Debug)]
pub struct SnapshotProducer {
    pub refresh_seconds: u32,
    pub identity: Option<DashboardIdentity>,
    pub version: String,
    /// Outer per-provider fetch envelope; `None` relies on provider-internal
    /// `web_timeout` alone (`--timeout 0` in the dashboard command).
    pub fetch_timeout: Option<Duration>,
    /// `serve --all-accounts`: add every configured token account as account
    /// rows, private by default (upstream merge a379b25de).
    pub all_accounts: bool,
}

impl SnapshotProducer {
    pub fn new(refresh_seconds: u32, identity: Option<DashboardIdentity>) -> Self {
        Self {
            refresh_seconds,
            identity,
            version: env!("CARGO_PKG_VERSION").to_string(),
            fetch_timeout: Some(PROVIDER_FETCH_TIMEOUT),
            all_accounts: false,
        }
    }

    pub fn with_all_accounts(mut self, all_accounts: bool) -> Self {
        self.all_accounts = all_accounts;
        self
    }

    pub fn with_fetch_timeout(mut self, timeout: Option<Duration>) -> Self {
        self.fetch_timeout = timeout;
        self
    }

    pub fn collect(&self) -> BoxSnapshotFuture {
        let this = self.clone();
        Box::pin(async move { Ok(this.collect_artifacts_inner().await?.dashboard) })
    }

    pub(crate) fn collect_artifacts(&self) -> BoxSnapshotArtifactsFuture<MetricsSnapshot> {
        let this = self.clone();
        Box::pin(async move { this.collect_artifacts_inner().await })
    }

    async fn collect_artifacts_inner(&self) -> Result<SnapshotArtifacts<MetricsSnapshot>, String> {
        let settings = Settings::load();
        let identity = resolve_identity(
            self.identity,
            settings.hide_personal_info,
            self.all_accounts,
        );
        let provider_ids: Vec<ProviderId> = settings.get_enabled_provider_ids();
        let deadline = self.fetch_timeout.map(BuildDeadline::starting_now);

        let mut account_plans = if self.all_accounts {
            plan_token_accounts(&provider_ids, &settings)
        } else {
            HashMap::new()
        };
        let mut provider_accounts: HashMap<String, ClaudeAccountsInput> = HashMap::new();

        // Every provider, and every expanded provider's account list, runs
        // concurrently under the same build deadline; order is restored by index.
        let mut set = tokio::task::JoinSet::new();
        let mut account_tasks: HashMap<tokio::task::Id, (usize, ProviderId)> = HashMap::new();
        for (index, provider_id) in provider_ids.iter().enumerate() {
            let provider_id = *provider_id;
            match account_plans.remove(&provider_id) {
                Some(Ok(plan)) if !plan.passive => {
                    let handle = set.spawn(async move {
                        let (envelope, section) = fetch_account_plan(plan, deadline).await;
                        (index, envelope, Some(section))
                    });
                    account_tasks.insert(handle.id(), (index, provider_id));
                }
                planned => {
                    let settings_ctx = match planned {
                        Some(Err(error)) => {
                            provider_accounts.insert(
                                provider_id.cli_name().to_string(),
                                ClaudeAccountsInput {
                                    accounts: Err(error),
                                    incomplete: false,
                                },
                            );
                            None
                        }
                        Some(Ok(mut plan)) => {
                            provider_accounts.insert(
                                provider_id.cli_name().to_string(),
                                ClaudeAccountsInput {
                                    accounts: Ok(passive_account_rows(&plan)),
                                    incomplete: false,
                                },
                            );
                            plan.provider_ctx.take()
                        }
                        None => None,
                    };
                    let fetch_timeout = self.fetch_timeout;
                    let ctx = settings_ctx.unwrap_or_else(|| {
                        let mut ctx = FetchContext::default();
                        populate_api_region_from_settings(provider_id, &settings, &mut ctx);
                        ctx
                    });
                    set.spawn(async move {
                        (
                            index,
                            fetch_provider_envelope(provider_id, ctx, fetch_timeout).await,
                            None,
                        )
                    });
                }
            }
        }

        let (costs, claude_accounts) = tokio::join!(
            collect_costs(provider_ids.contains(&ProviderId::Pi)),
            collect_claude_accounts(provider_ids.contains(&ProviderId::Claude), &settings),
        );

        let mut indexed = Vec::with_capacity(provider_ids.len());
        while let Some(joined) = set.join_next().await {
            match joined {
                Ok((index, envelope, section)) => {
                    if let Some(section) = section {
                        provider_accounts.insert(envelope.id.clone(), section);
                    }
                    indexed.push((index, envelope));
                }
                Err(join_error) => {
                    let Some((index, provider_id)) = account_tasks.get(&join_error.id()).copied()
                    else {
                        return Err(format!("dashboard build task failed: {join_error}"));
                    };
                    tracing::warn!(
                        provider = provider_id.cli_name(),
                        is_panic = join_error.is_panic(),
                        "dashboard account task failed"
                    );
                    provider_accounts.insert(
                        provider_id.cli_name().to_string(),
                        ClaudeAccountsInput {
                            accounts: Ok(Vec::new()),
                            incomplete: true,
                        },
                    );
                    indexed.push((
                        index,
                        provider_envelope(
                            provider_id,
                            Err("account fetch task failed".to_string()),
                        ),
                    ));
                }
            }
        }
        indexed.sort_by_key(|(index, _)| *index);
        let providers: Vec<ProviderFetchEnvelope> =
            indexed.into_iter().map(|(_, envelope)| envelope).collect();

        let order: Vec<String> = provider_ids
            .iter()
            .map(|id| id.cli_name().to_string())
            .collect();
        let enabled: BTreeSet<String> = order.iter().cloned().collect();
        let collection = SnapshotCollection {
            providers,
            costs,
            claude_accounts,
            provider_accounts,
            generated_at: Utc::now(),
            refresh_seconds: self.refresh_seconds,
            order,
            enabled,
        };
        let metrics = MetricsSnapshot::from_collection(&collection);
        Ok(SnapshotArtifacts {
            dashboard: build_snapshot(&SnapshotInput {
                collection,
                identity,
                version: Some(self.version.clone()),
                usage_bars_show_used: Some(settings.show_as_used),
                weekly_work_days: settings.weekly_progress_work_days,
                all_accounts: self.all_accounts,
            }),
            sidecar: Some(metrics),
        })
    }
}

/// Identity for one snapshot build: an explicit `--identity` wins; expanded
/// `--all-accounts` snapshots are private without it; ordinary snapshots
/// follow the app's hide_personal_info setting (upstream 0.50.1 #2960,
/// `resolveDashboardIdentityMode`).
fn resolve_identity(
    configured: Option<DashboardIdentity>,
    hide_personal_info: bool,
    all_accounts: bool,
) -> DashboardIdentity {
    if let Some(configured) = configured {
        return configured;
    }
    if all_accounts {
        DashboardIdentity::None
    } else if hide_personal_info {
        DashboardIdentity::Redacted
    } else {
        DashboardIdentity::Full
    }
}

fn provider_envelope(
    provider_id: ProviderId,
    fetch: Result<ProviderFetchResult, String>,
) -> ProviderFetchEnvelope {
    let provider = instantiate_provider(provider_id);
    let metadata = provider.metadata();
    ProviderFetchEnvelope {
        id: provider_id.cli_name().to_string(),
        display_name: metadata.display_name.to_string(),
        session_label: metadata.session_label.to_string(),
        weekly_label: metadata.weekly_label.to_string(),
        fetch,
    }
}

/// Fetch one provider with a hard outer bound; the error row carries the
/// failure instead of failing the whole snapshot (F9 semantics).
async fn fetch_provider_envelope(
    provider_id: ProviderId,
    ctx: FetchContext,
    fetch_timeout: Option<Duration>,
) -> ProviderFetchEnvelope {
    let fetch = bounded_fetch(
        provider_id,
        ctx,
        None,
        fetch_timeout.map(FetchBudget::whole),
    )
    .await;
    provider_envelope(provider_id, fetch)
}

/// How long one fetch may run, and the budget its timeout error reports.
/// An account fetch runs for the build's remaining time but reports the
/// build's total, so every timeout row in a snapshot reads the same.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FetchBudget {
    remaining: Duration,
    total: Duration,
}

impl FetchBudget {
    fn whole(total: Duration) -> Self {
        Self {
            remaining: total,
            total,
        }
    }
}

async fn bounded_fetch(
    provider_id: ProviderId,
    ctx: FetchContext,
    label: Option<&str>,
    budget: Option<FetchBudget>,
) -> Result<ProviderFetchResult, String> {
    let provider = instantiate_provider(provider_id);
    let Some(budget) = budget else {
        return provider
            .fetch_usage(&ctx)
            .await
            .map_err(|error| match label {
                Some(label) => format!("{label}: {error}"),
                None => error.to_string(),
            });
    };
    match tokio::time::timeout(budget.remaining, provider.fetch_usage(&ctx)).await {
        Ok(Ok(result)) => Ok(result),
        Ok(Err(error)) => Err(match label {
            Some(label) => format!("{label}: {error}"),
            None => error.to_string(),
        }),
        Err(_) => Err(timeout_error(label, budget)),
    }
}

fn timeout_error(label: Option<&str>, budget: FetchBudget) -> String {
    let secs = budget.total.as_secs();
    match label {
        Some(label) => format!("{label}: fetch timed out after {secs}s"),
        None => format!("fetch timed out after {secs}s"),
    }
}

/// Local cost data for the scanned providers, computed off the async
/// runtime so a large corpus cannot stall dashboard builds.
async fn collect_costs(pi_selected: bool) -> HashMap<String, RawCostPayload> {
    let result = tokio::task::spawn_blocking(move || {
        let mut scan_options = CostScanOptions::app_driven();
        scan_options.include_pi_sessions = !pi_selected;
        let scanner = CostScanner::new(30).with_options(scan_options);
        let codex = scanner.scan_codex_with_cancel(None);
        let claude = scanner.scan_claude_with_cancel_and_pi_sessions(None, !pi_selected);
        let pi = scanner.scan_pi_with_cancel(None);
        let pi_contract =
            build_local_spend_contract_from_summary("pi", 30, false, false, false, pi);
        let today = crate::cost_reporting_period::cost_bucket_zone()
            .date(Utc::now())
            .format("%Y-%m-%d")
            .to_string();
        let today_of = |provider: &str| {
            cost_scanner::get_daily_cost_history(provider, 30)
                .into_iter()
                .find(|(day, _)| day == &today)
                .and_then(|(_, cost)| cost)
        };
        let mut costs = HashMap::new();
        costs.insert(
            "codex".to_string(),
            RawCostPayload {
                today_usd: today_of("codex"),
                last_30_days_usd: Some(codex.total_cost_usd),
            },
        );
        costs.insert(
            "claude".to_string(),
            RawCostPayload {
                today_usd: today_of("claude"),
                last_30_days_usd: Some(claude.total_cost_usd),
            },
        );
        costs.insert(
            "pi".to_string(),
            RawCostPayload {
                today_usd: today_of("pi"),
                last_30_days_usd: pi_contract.known_cost_usd,
            },
        );
        costs
    })
    .await;
    match result {
        Ok(costs) => costs,
        Err(join_error) => {
            tracing::warn!(
                ?join_error,
                "dashboard cost scan task failed; continuing without costs"
            );
            HashMap::new()
        }
    }
}

/// Claude token-account rows (upstream "claude-swap" analog): per-account
/// cookie-override fetches, active flag from the store, errors as rows.
async fn collect_claude_accounts(
    claude_enabled: bool,
    settings: &Settings,
) -> Option<ClaudeAccountsInput> {
    if !claude_enabled {
        return None;
    }
    let data = match crate::core::TokenAccountStore::new().load_provider(ProviderId::Claude) {
        Ok(data) => data,
        Err(error) => {
            return Some(ClaudeAccountsInput {
                accounts: Err(format!("claude token accounts unavailable: {error}")),
                incomplete: false,
            });
        }
    };
    let active_index = data.active_account().map(|active| active.id);
    let mut set = tokio::task::JoinSet::new();
    for (index, account) in data.accounts.iter().cloned().enumerate() {
        let header = crate::core::TokenAccountSupport::normalized_cookie_header(
            ProviderId::Claude,
            &account.token,
        );
        let mut ctx = FetchContext {
            manual_cookie_header: Some(header),
            ..FetchContext::default()
        };
        populate_api_region_from_settings(ProviderId::Claude, settings, &mut ctx);
        set.spawn(async move {
            let fetch = bounded_fetch(
                ProviderId::Claude,
                ctx,
                Some(&account.label),
                Some(FetchBudget::whole(ACCOUNT_FETCH_TIMEOUT)),
            )
            .await;
            (
                index,
                AccountFetchEnvelope {
                    id: account.id.to_string(),
                    label: account.label.clone(),
                    active: active_index == Some(account.id),
                    fetch,
                },
            )
        });
    }
    let mut indexed = Vec::new();
    let mut incomplete = false;
    while let Some(joined) = set.join_next().await {
        match joined {
            Ok(item) => indexed.push(item),
            Err(_) => incomplete = true,
        }
    }
    indexed.sort_by_key(|(index, _)| *index);
    let accounts: Vec<AccountFetchEnvelope> = indexed.into_iter().map(|(_, row)| row).collect();
    if accounts.is_empty() && !incomplete {
        // No accounts: absent section (same as upstream when the adapter is off).
        return None;
    }
    Some(ClaudeAccountsInput {
        accounts: Ok(accounts),
        incomplete,
    })
}

/// The wall-clock budget shared by every fetch in one snapshot build.
#[derive(Clone, Copy, Debug)]
struct BuildDeadline {
    at: Instant,
    total: Duration,
}

impl BuildDeadline {
    fn starting_now(total: Duration) -> Self {
        Self {
            at: Instant::now() + total,
            total,
        }
    }

    /// Time left for the next fetch, or `None` once the build is out of time.
    fn remaining(&self) -> Option<Duration> {
        let left = self.at.saturating_duration_since(Instant::now());
        (!left.is_zero()).then_some(left)
    }

    fn next_budget(&self) -> Option<FetchBudget> {
        self.remaining().map(|remaining| FetchBudget {
            remaining,
            total: self.total,
        })
    }

    fn is_expired(&self) -> bool {
        self.remaining().is_none()
    }

    fn expired_error(&self) -> String {
        format!("fetch timed out after {}s", self.total.as_secs())
    }
}

/// One provider's configured token accounts for an expanded snapshot, with
/// each account's fetch context already resolved.
struct AccountPlan {
    provider_id: ProviderId,
    rows: Vec<(TokenAccount, FetchContext)>,
    active_id: Option<uuid::Uuid>,
    minimum_delay: Option<Duration>,
    /// The saved sessions are inactive for this route, so the rows are listed
    /// as unavailable instead of fetched (see [`passive_account_rows`]).
    passive: bool,
    /// For a passive plan, the provider row's context built from settings with
    /// no selected account, so configured API keys and the cookie source still
    /// apply (an empty context would ignore them).
    provider_ctx: Option<FetchContext>,
}

const PASSIVE_SESSION_ROW_ERROR: &str =
    "Saved session is not used here; it needs the Manual cookie source and a non-API usage source.";

/// Providers whose configured token accounts `--all-accounts` expands.
/// Claude keeps its own section ([`collect_claude_accounts`]).
fn token_account_providers(provider_ids: &[ProviderId]) -> Vec<ProviderId> {
    provider_ids
        .iter()
        .copied()
        .filter(|id| *id != ProviderId::Claude && TokenAccountSupport::is_supported(*id))
        .collect()
}

/// Load the token accounts of every expandable provider. Providers without
/// accounts are absent; a store that cannot be read yields an error section.
fn plan_token_accounts(
    provider_ids: &[ProviderId],
    settings: &Settings,
) -> HashMap<ProviderId, Result<AccountPlan, String>> {
    let providers = token_account_providers(provider_ids);
    if providers.is_empty() {
        return HashMap::new();
    }
    let store = crate::core::TokenAccountStore::new();
    let cookies = ManualCookies::load();
    let api_keys = ApiKeys::load();
    let mut plans = HashMap::new();
    for provider_id in providers {
        let data = match store.load_provider(provider_id) {
            Ok(data) => data,
            Err(error) => {
                plans.insert(
                    provider_id,
                    Err(format!("token accounts unavailable: {error}")),
                );
                continue;
            }
        };
        if data.accounts.is_empty() {
            continue;
        }
        let passive = token_account_session_is_passive(provider_id, settings);
        let rows = data
            .accounts
            .iter()
            .map(|account| {
                // Passive rows are never fetched, and building their context
                // could read a browser, so they keep an empty context.
                let ctx = if passive {
                    FetchContext::default()
                } else {
                    account_fetch_context(provider_id, settings, &cookies, &api_keys, account)
                };
                (account.clone(), ctx)
            })
            .collect();
        plans.insert(
            provider_id,
            Ok(AccountPlan {
                provider_id,
                rows,
                active_id: data.active_account().map(|active| active.id),
                minimum_delay: TokenAccountSupport::for_provider(provider_id)
                    .and_then(|support| support.minimum_delay_between_account_refreshes),
                passive,
                provider_ctx: passive
                    .then(|| build_fetch_context(provider_id, settings, &cookies, &api_keys, None)),
            }),
        );
    }
    plans
}

/// Rows for a passive plan: every account is listed as unavailable with no
/// fetch, so no row reports the ambient credential under another account's name.
/// The provider row itself takes the ordinary (non-account) fetch.
fn passive_account_rows(plan: &AccountPlan) -> Vec<AccountFetchEnvelope> {
    let results = plan
        .rows
        .iter()
        .map(|_| Err(PASSIVE_SESSION_ROW_ERROR.to_string()))
        .collect();
    account_rows(
        plan.rows.iter().map(|(account, _)| account),
        plan.active_id,
        results,
    )
    .1
}

/// The context the desktop refresh would use with `account` selected, marked
/// isolated: each row describes that one account, so providers must not fall
/// back to an ambient login (Grok's ~/.grok switch) or another credential.
fn account_fetch_context(
    provider_id: ProviderId,
    settings: &Settings,
    cookies: &ManualCookies,
    api_keys: &ApiKeys,
    account: &TokenAccount,
) -> FetchContext {
    let mut ctx = build_fetch_context(provider_id, settings, cookies, api_keys, Some(account));
    ctx.token_account_isolated = true;
    ctx
}

/// Fetch every account of one provider; the provider's top-level row is the
/// active account's result, so that account is fetched only once (upstream
/// `DashboardSnapshotBuilder.selectedPayloads`).
async fn fetch_account_plan(
    plan: AccountPlan,
    deadline: Option<BuildDeadline>,
) -> (ProviderFetchEnvelope, ClaudeAccountsInput) {
    let provider_id = plan.provider_id;
    let rows = &plan.rows;
    let results = collect_account_results(
        rows.len(),
        plan.minimum_delay,
        deadline,
        |index, budget| bounded_fetch(provider_id, rows[index].1.clone(), None, budget),
        tokio::time::sleep,
    )
    .await;

    let (top_level, accounts) = account_rows(
        rows.iter().map(|(account, _)| account),
        plan.active_id,
        results,
    );
    (
        provider_envelope(provider_id, top_level),
        ClaudeAccountsInput {
            accounts: Ok(accounts),
            incomplete: false,
        },
    )
}

/// Pair each account with its result. The provider's top-level row is the
/// active account's result, or the first account's when none is active.
fn account_rows<'a>(
    accounts: impl IntoIterator<Item = &'a TokenAccount>,
    active_id: Option<uuid::Uuid>,
    results: Vec<Result<ProviderFetchResult, String>>,
) -> (
    Result<ProviderFetchResult, String>,
    Vec<AccountFetchEnvelope>,
) {
    let rows: Vec<AccountFetchEnvelope> = accounts
        .into_iter()
        .zip(results)
        .map(|(account, fetch)| AccountFetchEnvelope {
            id: format!("token:{}", account.id),
            label: account.label.clone(),
            active: active_id == Some(account.id),
            fetch,
        })
        .collect();
    let top_level = rows
        .iter()
        .find(|row| row.active)
        .or_else(|| rows.first())
        .map(|row| row.fetch.clone())
        .unwrap_or_else(|| Err("no token accounts".to_string()));
    (top_level, rows)
}

/// Run `fetch` for accounts `0..count` and return the results in account
/// order (upstream `collectAccountUsage`). A provider with a minimum delay
/// is fetched one account at a time with `sleep(delay)` between accounts;
/// others run up to [`MAX_CONCURRENT_ACCOUNT_FETCHES`] at once, and a slow
/// account does not hold back the ones queued after it. Each fetch gets the
/// build's remaining time, and an account reached after the deadline becomes
/// a timeout row without being fetched.
async fn collect_account_results<F, Fut, S, SFut>(
    count: usize,
    minimum_delay: Option<Duration>,
    deadline: Option<BuildDeadline>,
    fetch: F,
    sleep: S,
) -> Vec<Result<ProviderFetchResult, String>>
where
    F: Fn(usize, Option<FetchBudget>) -> Fut,
    Fut: Future<Output = Result<ProviderFetchResult, String>>,
    S: Fn(Duration) -> SFut,
    SFut: Future<Output = ()>,
{
    let run_one = |index: usize| {
        let fetch = match deadline {
            Some(deadline) => deadline
                .next_budget()
                .map(|budget| fetch(index, Some(budget))),
            None => Some(fetch(index, None)),
        };
        async move {
            let result = match fetch {
                Some(fetch) => fetch.await,
                None => Err(deadline.map(|d| d.expired_error()).unwrap_or_default()),
            };
            (index, result)
        }
    };

    match minimum_delay {
        Some(delay) => {
            let mut results = Vec::with_capacity(count);
            for index in 0..count {
                if let Some(deadline) = deadline
                    && deadline.is_expired()
                {
                    // Out of time: the rest become timeout rows, with no
                    // further pacing sleeps.
                    results.resize_with(count, || Err(deadline.expired_error()));
                    break;
                }
                if index > 0 {
                    sleep(delay).await;
                }
                results.push(run_one(index).await.1);
            }
            results
        }
        None => {
            let mut indexed: Vec<(usize, Result<ProviderFetchResult, String>)> =
                futures::stream::iter(0..count)
                    .map(run_one)
                    .buffer_unordered(MAX_CONCURRENT_ACCOUNT_FETCHES)
                    .collect()
                    .await;
            indexed.sort_by_key(|(index, _)| *index);
            indexed.into_iter().map(|(_, result)| result).collect()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn producer_defaults_to_redacted_identity() {
        let producer = SnapshotProducer::new(60, Some(DashboardIdentity::Redacted));
        assert_eq!(producer.identity, Some(DashboardIdentity::Redacted));
        assert_eq!(producer.refresh_seconds, 60);
        assert!(!producer.version.is_empty());
    }

    #[test]
    fn producer_none_identity_follows_settings() {
        let producer = SnapshotProducer::new(60, None);
        assert_eq!(producer.identity, None);
        assert_eq!(producer.refresh_seconds, 60);
        assert!(!producer.all_accounts);
        assert!(producer.with_all_accounts(true).all_accounts);
    }

    #[test]
    fn identity_resolution_matches_upstream_order() {
        use DashboardIdentity::{Full, None as Private, Redacted};
        assert_eq!(resolve_identity(Some(Full), true, true), Full);
        assert_eq!(resolve_identity(Some(Redacted), false, true), Redacted);
        assert_eq!(resolve_identity(None, false, true), Private);
        assert_eq!(resolve_identity(None, true, false), Redacted);
        assert_eq!(resolve_identity(None, false, false), Full);
    }

    #[test]
    fn token_account_expansion_skips_claude_and_unsupported_providers() {
        assert!(TokenAccountSupport::is_supported(ProviderId::OpenRouter));
        assert!(!TokenAccountSupport::is_supported(ProviderId::Codex));
        assert_eq!(
            token_account_providers(&[
                ProviderId::Claude,
                ProviderId::Codex,
                ProviderId::OpenRouter
            ]),
            vec![ProviderId::OpenRouter]
        );
    }

    #[test]
    fn only_neuralwatt_declares_a_minimum_account_refresh_delay() {
        for id in ProviderId::all().iter().copied() {
            let delay = TokenAccountSupport::for_provider(id)
                .and_then(|support| support.minimum_delay_between_account_refreshes);
            let expected = (id == ProviderId::Neuralwatt).then_some(Duration::from_secs(1));
            assert_eq!(delay, expected, "{id:?}");
        }
    }

    #[test]
    fn openrouter_account_context_uses_the_account_key() {
        let mut api_keys = ApiKeys::default();
        api_keys.set("openrouter", "sk-or-v1-stored", None);
        let account = TokenAccount::new("Work", "sk-or-v1-work");
        let ctx = account_fetch_context(
            ProviderId::OpenRouter,
            &Settings::default(),
            &ManualCookies::default(),
            &api_keys,
            &account,
        );
        assert_eq!(ctx.api_key.as_deref(), Some("sk-or-v1-work"));
        assert_eq!(
            ctx.token_account_kind,
            Some(crate::core::TokenAccountKind::ApiKey)
        );
        assert!(ctx.token_account_isolated);
    }

    #[test]
    fn xai_account_context_carries_the_saved_team_id() {
        let mut settings = Settings::default();
        settings.set_workspace_id(ProviderId::Xai, "team-123");
        let account = TokenAccount::new("Team", "xai-management-key");
        let ctx = account_fetch_context(
            ProviderId::Xai,
            &settings,
            &ManualCookies::default(),
            &ApiKeys::default(),
            &account,
        );
        assert_eq!(ctx.workspace_id.as_deref(), Some("team-123"));
        assert_eq!(ctx.api_key.as_deref(), Some("xai-management-key"));
    }

    #[test]
    fn grok_bearer_account_context_routes_to_the_account_token() {
        let account = TokenAccount::new("Second", "grok-bearer-token");
        let ctx = account_fetch_context(
            ProviderId::Grok,
            &Settings::default(),
            &ManualCookies::default(),
            &ApiKeys::default(),
            &account,
        );
        assert_eq!(ctx.source_mode, crate::core::SourceMode::OAuth);
        assert_eq!(ctx.api_key.as_deref(), Some("grok-bearer-token"));
        assert!(ctx.token_account_isolated);
    }

    #[test]
    fn only_manual_cookies_with_a_non_api_source_keep_command_code_sessions_active() {
        let id = ProviderId::CommandCode;
        let mut settings = Settings::default();
        for (cookie_source, usage_source, passive) in [
            ("manual", "auto", false),
            ("manual", "web", false),
            ("manual", "oauth", true),
            ("auto", "auto", true),
            ("auto", "web", true),
            ("off", "auto", true),
        ] {
            settings.set_cookie_source(id, cookie_source);
            settings.set_usage_source(id, usage_source);
            assert_eq!(
                token_account_session_is_passive(id, &settings),
                passive,
                "{cookie_source}/{usage_source}"
            );
        }
        // Providers without an owned credential lane never go passive here.
        assert!(!token_account_session_is_passive(
            ProviderId::OpenRouter,
            &settings
        ));
    }

    #[test]
    fn passive_account_rows_are_listed_unavailable_without_a_fetch() {
        let work = TokenAccount::new("Work", "work-session");
        let plan = AccountPlan {
            provider_id: ProviderId::CommandCode,
            rows: vec![
                (
                    TokenAccount::new("Personal", "personal-session"),
                    FetchContext::default(),
                ),
                (work.clone(), FetchContext::default()),
            ],
            active_id: Some(work.id),
            minimum_delay: None,
            passive: true,
            provider_ctx: None,
        };

        let rows = passive_account_rows(&plan);

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].label, "Personal");
        assert!(!rows[0].active);
        assert_eq!(rows[1].label, "Work");
        assert!(rows[1].active);
        assert!(rows.iter().all(|row| row.fetch.is_err()));
    }

    fn ok_result(label: &str) -> Result<ProviderFetchResult, String> {
        Ok(ProviderFetchResult::new(
            crate::core::UsageSnapshot::new(crate::core::RateWindow::new(10.0)),
            label,
        ))
    }

    #[tokio::test]
    async fn delayed_provider_fetches_accounts_one_at_a_time_with_the_delay_between() {
        use std::sync::{Arc, Mutex};
        let log = Arc::new(Mutex::new(Vec::<String>::new()));
        let fetch_log = Arc::clone(&log);
        let sleep_log = Arc::clone(&log);
        let results = collect_account_results(
            3,
            Some(Duration::from_secs(1)),
            None,
            move |index, _budget| {
                let log = Arc::clone(&fetch_log);
                async move {
                    log.lock().unwrap().push(format!("start {index}"));
                    tokio::task::yield_now().await;
                    log.lock().unwrap().push(format!("end {index}"));
                    ok_result(&format!("account {index}"))
                }
            },
            move |delay| {
                sleep_log
                    .lock()
                    .unwrap()
                    .push(format!("sleep {}ms", delay.as_millis()));
                std::future::ready(())
            },
        )
        .await;
        assert_eq!(
            *log.lock().unwrap(),
            vec![
                "start 0",
                "end 0",
                "sleep 1000ms",
                "start 1",
                "end 1",
                "sleep 1000ms",
                "start 2",
                "end 2",
            ]
        );
        let labels: Vec<String> = results
            .into_iter()
            .map(|result| result.unwrap().source_label)
            .collect();
        assert_eq!(labels, vec!["account 0", "account 1", "account 2"]);
    }

    #[tokio::test]
    async fn undelayed_provider_caps_account_fetch_concurrency_and_keeps_order() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};
        let in_flight = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let slept = Arc::new(AtomicUsize::new(0));
        let (fetch_in_flight, fetch_peak, sleep_count) = (
            Arc::clone(&in_flight),
            Arc::clone(&peak),
            Arc::clone(&slept),
        );
        let results = collect_account_results(
            7,
            None,
            None,
            move |index, _budget| {
                let in_flight = Arc::clone(&fetch_in_flight);
                let peak = Arc::clone(&fetch_peak);
                async move {
                    let now = in_flight.fetch_add(1, Ordering::SeqCst) + 1;
                    peak.fetch_max(now, Ordering::SeqCst);
                    // Later accounts finish first; order must still hold.
                    for _ in 0..(7 - index) {
                        tokio::task::yield_now().await;
                    }
                    in_flight.fetch_sub(1, Ordering::SeqCst);
                    ok_result(&format!("account {index}"))
                }
            },
            move |_delay| {
                sleep_count.fetch_add(1, Ordering::SeqCst);
                std::future::ready(())
            },
        )
        .await;
        assert_eq!(peak.load(Ordering::SeqCst), MAX_CONCURRENT_ACCOUNT_FETCHES);
        assert_eq!(slept.load(Ordering::SeqCst), 0);
        let labels: Vec<String> = results
            .into_iter()
            .map(|result| result.unwrap().source_label)
            .collect();
        let expected: Vec<String> = (0..7).map(|index| format!("account {index}")).collect();
        assert_eq!(labels, expected);
    }

    #[tokio::test]
    async fn accounts_reached_after_the_deadline_become_timeout_rows() {
        let deadline = BuildDeadline {
            at: Instant::now(),
            total: Duration::from_secs(75),
        };
        let fetched = std::sync::atomic::AtomicUsize::new(0);
        let slept = std::sync::atomic::AtomicUsize::new(0);
        let results = collect_account_results(
            2,
            Some(Duration::from_secs(1)),
            Some(deadline),
            |_index, _budget| {
                fetched.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                std::future::ready(ok_result("account"))
            },
            |_delay| {
                slept.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                std::future::ready(())
            },
        )
        .await;
        assert_eq!(fetched.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert_eq!(slept.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert_eq!(
            results
                .into_iter()
                .map(|result| result.unwrap_err())
                .collect::<Vec<_>>(),
            vec!["fetch timed out after 75s", "fetch timed out after 75s"]
        );
    }

    #[tokio::test]
    async fn each_account_fetch_gets_the_remaining_build_time() {
        let deadline = BuildDeadline::starting_now(Duration::from_secs(75));
        let results = collect_account_results(
            1,
            None,
            Some(deadline),
            |_index, budget| async move {
                let budget = budget.expect("a deadline yields a budget");
                assert!(budget.remaining <= Duration::from_secs(75));
                assert!(budget.remaining > Duration::from_secs(70));
                assert_eq!(budget.total, Duration::from_secs(75));
                ok_result("account")
            },
            |_delay| std::future::ready(()),
        )
        .await;
        assert!(results[0].is_ok());
    }

    #[tokio::test]
    async fn delayed_provider_stops_sleeping_once_the_deadline_passes() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let deadline = BuildDeadline {
            at: Instant::now() + Duration::from_millis(50),
            total: Duration::from_secs(75),
        };
        let fetched = AtomicUsize::new(0);
        let slept = AtomicUsize::new(0);
        let results = collect_account_results(
            3,
            Some(Duration::from_secs(1)),
            Some(deadline),
            |_index, budget| {
                fetched.fetch_add(1, Ordering::SeqCst);
                let remaining = budget.expect("first account has a budget").remaining;
                async move {
                    // The first account uses up the whole build budget.
                    tokio::time::sleep(remaining + Duration::from_millis(10)).await;
                    ok_result("account 0")
                }
            },
            |_delay| {
                slept.fetch_add(1, Ordering::SeqCst);
                std::future::ready(())
            },
        )
        .await;
        assert_eq!(fetched.load(Ordering::SeqCst), 1);
        assert_eq!(slept.load(Ordering::SeqCst), 0);
        assert!(results[0].is_ok());
        assert_eq!(
            results[1..]
                .iter()
                .map(|result| result.clone().unwrap_err())
                .collect::<Vec<_>>(),
            vec!["fetch timed out after 75s", "fetch timed out after 75s"]
        );
    }

    #[tokio::test]
    async fn slow_first_account_does_not_hold_back_later_accounts() {
        use std::sync::Mutex;
        let count = MAX_CONCURRENT_ACCOUNT_FETCHES + 4;
        // Account 0 only finishes once the last account has run, which needs
        // every account after it to start while account 0 is still pending.
        let (release, released) = tokio::sync::oneshot::channel::<()>();
        let release = Mutex::new(Some(release));
        let released = Mutex::new(Some(released));
        let results = tokio::time::timeout(
            Duration::from_secs(5),
            collect_account_results(
                count,
                None,
                None,
                |index, _budget| {
                    let wait = (index == 0).then(|| released.lock().unwrap().take().unwrap());
                    let signal =
                        (index == count - 1).then(|| release.lock().unwrap().take().unwrap());
                    async move {
                        if let Some(wait) = wait {
                            wait.await.expect("last account releases the first");
                        }
                        if let Some(signal) = signal {
                            signal.send(()).expect("first account is still waiting");
                        }
                        ok_result(&format!("account {index}"))
                    }
                },
                |_delay| std::future::ready(()),
            ),
        )
        .await
        .expect("a pending first account must not block later accounts");
        let labels: Vec<String> = results
            .into_iter()
            .map(|result| result.unwrap().source_label)
            .collect();
        let expected: Vec<String> = (0..count).map(|index| format!("account {index}")).collect();
        assert_eq!(labels, expected);
    }

    fn labelled_results(count: usize) -> Vec<Result<ProviderFetchResult, String>> {
        (0..count)
            .map(|index| ok_result(&format!("account {index}")))
            .collect()
    }

    #[test]
    fn top_level_row_is_the_active_accounts_result() {
        let accounts = [
            TokenAccount::new("First", "key-1"),
            TokenAccount::new("Second", "key-2"),
            TokenAccount::new("Third", "key-3"),
        ];
        let (top_level, rows) =
            account_rows(accounts.iter(), Some(accounts[1].id), labelled_results(3));
        assert_eq!(top_level.unwrap().source_label, "account 1");
        assert_eq!(
            rows.iter().map(|row| row.active).collect::<Vec<_>>(),
            vec![false, true, false]
        );
        assert_eq!(rows[2].id, format!("token:{}", accounts[2].id));
    }

    #[test]
    fn top_level_row_falls_back_to_the_first_account_without_an_active_one() {
        let accounts = [
            TokenAccount::new("First", "key-1"),
            TokenAccount::new("Second", "key-2"),
        ];
        let (top_level, rows) = account_rows(accounts.iter(), None, labelled_results(2));
        assert_eq!(top_level.unwrap().source_label, "account 0");
        assert!(rows.iter().all(|row| !row.active));
        let (empty, none) = account_rows(std::iter::empty(), None, Vec::new());
        assert!(none.is_empty());
        assert_eq!(empty.unwrap_err(), "no token accounts");
    }

    #[test]
    fn account_timeouts_report_the_build_total_not_the_remaining_time() {
        let deadline = BuildDeadline::starting_now(Duration::from_secs(75));
        let budget = FetchBudget {
            remaining: Duration::from_secs(3),
            total: deadline.total,
        };
        assert_eq!(timeout_error(None, budget), deadline.expired_error());
        assert_eq!(
            timeout_error(Some("Work"), budget),
            "Work: fetch timed out after 75s"
        );
    }
}
