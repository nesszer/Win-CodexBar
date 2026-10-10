use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::PathBuf;

use chrono::{DateTime, Duration, NaiveDate, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::core::{
    CostUsagePricing, ModelsDevPricingSnapshot, ModelsDevPricingTarget, models_dev_pricing_targets,
};

use super::{
    ActivityHistogram, CostCoverageCounts, CostProvenance, CustomPricing, CustomRates,
    ImportedSpendSource, SpendDailyPoint, SpendModelRow, SpendTokenMix,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OpenCodexEntry {
    request_id: String,
    timestamp: DateTime<Utc>,
    provider: String,
    model: String,
    usage_status: String,
    conversation_id: Option<String>,
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    cache_read_tokens: Option<u64>,
    cache_creation_tokens: Option<u64>,
    reasoning_tokens: Option<u64>,
    total_tokens: Option<u64>,
}

mod cache;
mod nous;
#[cfg(test)]
mod nous_tests;
#[cfg(test)]
mod pricing_tests;

#[derive(Default)]
struct ModelAccumulator {
    input: u64,
    output: u64,
    cache_read: u64,
    cache_creation: u64,
    total: Option<u64>,
    cost: Option<f64>,
    custom_pricing: bool,
}

#[derive(Default)]
struct DailyAccumulator {
    cost: f64,
    saw_cost: bool,
    total_tokens: u64,
    saw_tokens: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RouteTarget {
    Subscription(&'static str),
    TokenOnly,
    Unknown,
}

fn route_provider(provider: &str) -> RouteTarget {
    match provider.trim().to_ascii_lowercase().as_str() {
        "openai" => RouteTarget::Subscription("codex"),
        "opencode-go" => RouteTarget::Subscription("opencodego"),
        "kimi-coding" | "kimi-for-coding" => RouteTarget::Subscription("kimi"),
        "deepseek" => RouteTarget::Subscription("deepseek"),
        "nous" => RouteTarget::Subscription(nous::SUBSCRIPTION_ID),
        "opencode-free" | "opencode" => RouteTarget::TokenOnly,
        _ => RouteTarget::Unknown,
    }
}

fn route_model(model: &str) -> RouteTarget {
    let trimmed = model.trim();
    let Some((prefix, _)) = trimmed.split_once('/') else {
        return RouteTarget::Subscription("codex");
    };
    if prefix.is_empty() {
        RouteTarget::Unknown
    } else {
        route_provider(prefix)
    }
}

fn route_entry(entry: &OpenCodexEntry) -> RouteTarget {
    // OpenCodex normally records the billing provider separately from the model
    // namespace. Only the historical OpenAI transport format used the model
    // prefix as an explicit route; never let another provider's namespace
    // override its recorded provider.
    if entry.provider.trim().eq_ignore_ascii_case("openai") && entry.model.trim().contains('/') {
        let routed = route_model(&entry.model);
        if routed != RouteTarget::Unknown {
            return routed;
        }
    }
    route_provider(&entry.provider)
}

pub(super) fn load_for_subscription(
    provider_id: &str,
    history_days: u32,
    custom: &CustomPricing,
) -> Option<ImportedSpendSource> {
    let source_path = usage_path()?;
    let entries = cache::load_entries(&source_path)?;
    let entries = entries
        .into_iter()
        .filter(|entry| matches!(route_entry(entry), RouteTarget::Subscription(id) if id == provider_id))
        .collect();
    aggregate(entries, Utc::now(), history_days.clamp(1, 365), custom)
}

/// Refreshes the models.dev catalog when a ledger row that a Usage & Spend
/// build may import needs a price it lacks (upstream 0.60.4
/// `OpenCodexUsageStore.refreshPricingIfNeeded`).
pub(super) async fn refresh_pricing_if_needed() {
    let targets = tokio::task::spawn_blocking(|| {
        let entries = cache::load_entries(&usage_path()?)?;
        Some(pricing_targets(&entries, Utc::now()))
    })
    .await
    .ok()
    .flatten()
    .unwrap_or_default();
    crate::core::refresh_exact_pricing_targets_if_needed(&targets).await;
}

/// The exact models.dev identities the cached catalog must price for the
/// rows a build may show. Windows imports only rows routed to a
/// subscription. A direct OpenAI model with bundled Codex rates never reads
/// the catalog, and a model-less row is never priced, so neither refreshes.
fn pricing_targets(entries: &[OpenCodexEntry], now: DateTime<Utc>) -> Vec<ModelsDevPricingTarget> {
    let mut targets = BTreeSet::new();
    for entry in entries {
        if entry.timestamp > now
            || !is_priceable_status(entry)
            || !matches!(route_entry(entry), RouteTarget::Subscription(_))
        {
            continue;
        }
        let resolved = models_dev_pricing_targets(&pricing_provider(entry), &entry.model);
        match resolved.first() {
            Some(first) if is_codex_target(first) => {
                if !CostUsagePricing::has_bundled_codex_pricing(&first.model_id)
                    && !CostUsagePricing::is_codex_unattributed_model(&first.model_id)
                {
                    targets.insert(first.clone());
                }
            }
            _ => targets.extend(resolved),
        }
    }
    targets.into_iter().collect()
}

fn aggregate(
    entries: Vec<OpenCodexEntry>,
    now: DateTime<Utc>,
    history_days: u32,
    custom: &CustomPricing,
) -> Option<ImportedSpendSource> {
    aggregate_with_pricing(entries, now, history_days, custom, None)
}

/// `pricing_snapshot` pins the models.dev catalog; `None` reads the cached one.
fn aggregate_with_pricing(
    entries: Vec<OpenCodexEntry>,
    now: DateTime<Utc>,
    history_days: u32,
    custom: &CustomPricing,
    pricing_snapshot: Option<&ModelsDevPricingSnapshot>,
) -> Option<ImportedSpendSource> {
    let zone = crate::cost_reporting_period::cost_bucket_zone();
    let first_day = zone.date(now) - Duration::days(i64::from(history_days.saturating_sub(1)));

    // requestId is authoritative: a later row replaces an earlier row with the same id.
    let mut unique: HashMap<String, OpenCodexEntry> = HashMap::new();
    for entry in entries {
        unique.insert(entry.request_id.clone(), entry);
    }
    let mut entries: Vec<_> = unique
        .into_values()
        .filter(|entry| entry.timestamp <= now && zone.date(entry.timestamp) >= first_day)
        .collect();
    entries.sort_by(|left, right| {
        left.timestamp
            .cmp(&right.timestamp)
            .then_with(|| left.request_id.cmp(&right.request_id))
    });
    if entries.is_empty() {
        return None;
    }

    let mut conversations = HashSet::new();
    let mut token_mix = SpendTokenMix::default();
    let mut token_total: Option<u64> = None;
    let mut coverage = CostCoverageCounts::default();
    let mut activity = ActivityHistogram::default();
    let mut models: HashMap<String, ModelAccumulator> = HashMap::new();
    let mut daily: BTreeMap<String, DailyAccumulator> = BTreeMap::new();
    let mut known_cost = 0.0;
    let mut saw_known_cost = false;
    let mut saw_vendor_provenance = false;
    let mut saw_list_provenance = false;
    let mut saw_metered_cost = false;
    // Upstream 0.55.0 #3136: resolve the dynamic pricing catalog once per
    // aggregate instead of re-checking its cache metadata for every usage row.
    let pricing_snapshot = pricing_snapshot
        .cloned()
        .unwrap_or_else(crate::core::pricing_snapshot);

    for entry in &entries {
        // A row without a conversationId is its own session (upstream 0.68.0).
        let session = entry.conversation_id.as_ref().unwrap_or(&entry.request_id);
        conversations.insert(session.clone());
        token_mix.input_tokens = add_optional(token_mix.input_tokens, entry.input_tokens);
        token_mix.output_tokens = add_optional(token_mix.output_tokens, entry.output_tokens);
        token_mix.cache_read_tokens =
            add_optional(token_mix.cache_read_tokens, entry.cache_read_tokens);
        token_mix.cache_creation_tokens =
            add_optional(token_mix.cache_creation_tokens, entry.cache_creation_tokens);
        token_mix.reasoning_tokens =
            add_optional(token_mix.reasoning_tokens, entry.reasoning_tokens);
        // Same per-entry basis and saturation as the daily and model totals below.
        if let Some(total) = entry.resolved_total_tokens() {
            token_total = Some(token_total.unwrap_or(0).saturating_add(total));
        }

        let pricing = RowPricing::resolve(entry, custom);
        let cost = pricing.cost(entry, &pricing_snapshot);
        match entry.usage_status.as_str() {
            "reported" => saw_vendor_provenance = true,
            "estimated" => saw_list_provenance = true,
            _ => {}
        }
        match entry.usage_status.as_str() {
            "reported" if cost.is_some() => coverage.priced = coverage.priced.saturating_add(1),
            "estimated" if cost.is_some() => {
                coverage.estimated = coverage.estimated.saturating_add(1)
            }
            "unsupported" => coverage.unmetered = coverage.unmetered.saturating_add(1),
            _ => coverage.unpriced = coverage.unpriced.saturating_add(1),
        }
        if let Some(cost) = cost {
            known_cost += cost;
            saw_known_cost = true;
            if entry.usage_status == "reported" {
                saw_metered_cost = true;
            }
        }

        activity.add_local(entry.timestamp);

        let day = daily
            .entry(zone.date(entry.timestamp).format("%Y-%m-%d").to_string())
            .or_default();
        if let Some(cost) = cost {
            day.cost += cost;
            day.saw_cost = true;
        }
        if let Some(total) = entry.resolved_total_tokens() {
            day.total_tokens = day.total_tokens.saturating_add(total);
            day.saw_tokens = true;
        }

        let model = models.entry(entry.model.clone()).or_default();
        model.input = model.input.saturating_add(entry.input_tokens.unwrap_or(0));
        model.output = model
            .output
            .saturating_add(entry.output_tokens.unwrap_or(0));
        model.cache_read = model
            .cache_read
            .saturating_add(entry.cache_read_tokens.unwrap_or(0));
        model.cache_creation = model
            .cache_creation
            .saturating_add(entry.cache_creation_tokens.unwrap_or(0));
        if let Some(total) = entry.resolved_total_tokens() {
            model.total = Some(model.total.unwrap_or(0).saturating_add(total));
        }
        if let Some(cost) = cost {
            model.cost = Some(model.cost.unwrap_or(0.0) + cost);
        }
        model.custom_pricing |= pricing.custom.is_some();
    }

    let mut model_rows: Vec<_> = models
        .into_iter()
        .map(|(model, acc)| SpendModelRow {
            model,
            cost_usd: acc.cost,
            input_tokens: acc.input,
            output_tokens: acc.output,
            cache_read_tokens: acc.cache_read,
            total_tokens: acc.total.unwrap_or_else(|| {
                acc.input
                    .saturating_add(acc.output)
                    .saturating_add(acc.cache_creation)
            }),
            custom_pricing: acc.custom_pricing,
        })
        .collect();
    model_rows.sort_by(|left, right| match (left.cost_usd, right.cost_usd) {
        (Some(a), Some(b)) => b
            .partial_cmp(&a)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.model.cmp(&right.model)),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => left.model.cmp(&right.model),
    });

    // Counts are clamped to u32::MAX before casting.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "clamped to u32::MAX before casting"
    )]
    let request_count = entries.len().min(u32::MAX as usize) as u32;
    #[allow(
        clippy::cast_possible_truncation,
        reason = "clamped to u32::MAX before casting"
    )]
    let conversation_count = conversations.len().min(u32::MAX as usize) as u32;

    let snapshot_provenance =
        CostProvenance::from_source_kinds(saw_vendor_provenance, saw_list_provenance);
    let provenance =
        CostProvenance::for_window(snapshot_provenance, saw_known_cost, saw_metered_cost);

    Some(ImportedSpendSource {
        source_id: "opencodex".to_string(),
        display_name: "OpenCodex".to_string(),
        request_count,
        conversation_count,
        known_cost_usd: saw_known_cost.then_some(known_cost),
        provenance,
        token_mix,
        token_total,
        coverage,
        models: model_rows,
        daily: daily
            .into_iter()
            .map(|(day, acc)| SpendDailyPoint {
                day,
                cost_usd: acc.saw_cost.then_some(acc.cost),
                total_tokens: acc.saw_tokens.then_some(acc.total_tokens),
            })
            .collect(),
        hourly_activity: activity.into_cells(),
    })
}

/// Providers a legacy OpenAI-transport row may name in its model prefix as
/// the billing route (upstream `CostUsagePricing.codexModelsDevProviderIDs`).
const ROUTED_MODEL_PROVIDER_IDS: [&str; 7] = [
    "deepseek",
    "kimi-coding",
    "kimi-for-coding",
    "openai",
    "opencode",
    "opencode-free",
    "opencode-go",
];

/// The provider whose catalog prices `entry` (upstream
/// `OpenCodexUsagePricing.providerID(for:)`): the recorded provider, except
/// that a legacy OpenAI-transport row names a known route in its model prefix.
fn pricing_provider(entry: &OpenCodexEntry) -> String {
    let provider = entry.provider.trim().to_ascii_lowercase();
    if provider == "openai"
        && let Some((prefix, _)) = entry.model.trim().split_once('/')
    {
        let prefix = prefix.to_ascii_lowercase();
        if ROUTED_MODEL_PROVIDER_IDS.contains(&prefix.as_str()) {
            return prefix;
        }
    }
    provider
}

fn is_priceable_status(entry: &OpenCodexEntry) -> bool {
    matches!(entry.usage_status.as_str(), "reported" | "estimated")
}

/// A direct OpenAI model keeps the Codex pricing convention.
fn is_codex_target(target: &ModelsDevPricingTarget) -> bool {
    target.provider_id == "openai" && !target.model_id.contains('/')
}

/// The token lanes of a priceable row. A row without both input and output
/// is unpriced (upstream `listPriceUSD`); a missing cache lane is zero.
#[derive(Debug, Clone, Copy)]
struct RowTokens {
    input: u64,
    output: u64,
    cache_read: u64,
    cache_write: u64,
}

impl RowTokens {
    fn of(entry: &OpenCodexEntry) -> Option<Self> {
        if !is_priceable_status(entry) {
            return None;
        }
        Some(Self {
            input: entry.input_tokens?,
            output: entry.output_tokens?,
            cache_read: entry.cache_read_tokens.unwrap_or(0),
            cache_write: entry.cache_creation_tokens.unwrap_or(0),
        })
    }
}

/// How one ledger row is priced (upstream 0.60.4 `OpenCodexUsagePricing`):
/// by a custom override, or by the models.dev catalog of its recorded
/// billing route. Another vendor's namespace in the model id never borrows
/// that vendor's rates.
struct RowPricing<'a> {
    /// The override that prices the row. It also marks the model row as
    /// custom-priced, even when its rates leave the cost unknown.
    custom: Option<&'a CustomRates>,
    targets: Vec<ModelsDevPricingTarget>,
}

impl<'a> RowPricing<'a> {
    /// Overrides resolve in upstream order: the recorded provider and model,
    /// then (for a resolvable row) the billing route, then each catalog
    /// identity. The first match wins whole; a rate it lacks is never filled
    /// from a later override or from the catalog.
    fn resolve(entry: &OpenCodexEntry, custom: &'a CustomPricing) -> Self {
        let provider = pricing_provider(entry);
        let targets = models_dev_pricing_targets(&provider, &entry.model);
        let custom = custom
            .overlay_rates(&entry.provider, &entry.model)
            .or_else(|| {
                targets
                    .first()
                    .and_then(|_| custom.overlay_rates(&provider, &entry.model))
            })
            .or_else(|| {
                targets
                    .iter()
                    .find_map(|target| custom.overlay_rates(&target.provider_id, &target.model_id))
            });
        Self { custom, targets }
    }

    fn cost(&self, entry: &OpenCodexEntry, snapshot: &ModelsDevPricingSnapshot) -> Option<f64> {
        let tokens = RowTokens::of(entry)?;
        let Some(rates) = self.custom else {
            return self.catalog_cost(tokens, entry.timestamp.date_naive(), snapshot);
        };
        // Custom rates keep the historical convention that input includes
        // cache reads and writes. Nous rows record input without them and
        // bill each lane on its own (upstream 0.68.0 Nous fixtures).
        let input = if route_entry(entry) == RouteTarget::Subscription(nous::SUBSCRIPTION_ID) {
            tokens.input
        } else {
            tokens
                .input
                .saturating_sub(tokens.cache_read)
                .saturating_sub(tokens.cache_write)
        };
        rates.lane_cost(input, tokens.output, tokens.cache_read, tokens.cache_write)
    }

    /// Upstream `providerCostUSD`. A direct OpenAI model keeps the Codex
    /// convention: inclusive input, request-day rates, bundled rates before
    /// the catalog. Every other identity needs an exact catalog entry and
    /// bills independent token lanes; a consumed cache lane without its own
    /// catalog rate leaves the row unpriced instead of borrowing the input
    /// rate.
    fn catalog_cost(
        &self,
        tokens: RowTokens,
        day: NaiveDate,
        snapshot: &ModelsDevPricingSnapshot,
    ) -> Option<f64> {
        let first = self.targets.first()?;
        if is_codex_target(first) {
            return CostUsagePricing::codex_cost_usd_at_date_with_cache_write_and_pricing_snapshot(
                &first.model_id,
                tokens.input,
                tokens.cache_read,
                tokens.cache_write,
                tokens.output,
                day,
                Some(snapshot),
            );
        }
        let pricing = self
            .targets
            .iter()
            .find_map(|target| snapshot.lookup_exact(&target.provider_id, &target.model_id))?;
        if (tokens.cache_read > 0 && pricing.cache_read_input_cost_per_token.is_none())
            || (tokens.cache_write > 0 && pricing.cache_write_input_cost_per_token.is_none())
        {
            return None;
        }
        let inclusive_input = tokens
            .input
            .checked_add(tokens.cache_read)?
            .checked_add(tokens.cache_write)?;
        Some(CostUsagePricing::models_dev_cost_usd(
            &pricing,
            inclusive_input,
            tokens.cache_read,
            tokens.cache_write,
            tokens.output,
        ))
    }
}

#[cfg(test)]
fn entry_cost(
    entry: &OpenCodexEntry,
    custom: &CustomPricing,
    snapshot: &ModelsDevPricingSnapshot,
) -> Option<f64> {
    RowPricing::resolve(entry, custom).cost(entry, snapshot)
}

fn usage_path() -> Option<PathBuf> {
    if let Ok(home) = std::env::var("OPENCODEX_HOME") {
        let trimmed = home.trim();
        if !trimmed.is_empty() {
            return Some(PathBuf::from(trimmed).join("usage.jsonl"));
        }
    }
    dirs::home_dir().map(|home| home.join(".opencodex").join("usage.jsonl"))
}

fn parse_line(line: &str) -> Option<OpenCodexEntry> {
    let value: Value = serde_json::from_str(line.trim()).ok()?;
    let request_id = value.get("requestId")?.as_str()?.trim().to_string();
    let model = value.get("model")?.as_str()?.trim().to_string();
    if request_id.is_empty() || model.is_empty() {
        return None;
    }
    let provider = value
        .get("provider")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("openai")
        .to_string();
    let timestamp = parse_timestamp(value.get("timestamp")?)?;
    let usage = value.get("usage").and_then(Value::as_object);
    Some(OpenCodexEntry {
        request_id,
        timestamp,
        provider,
        model,
        usage_status: value
            .get("usageStatus")
            .and_then(Value::as_str)
            .unwrap_or("unreported")
            .trim()
            .to_ascii_lowercase(),
        conversation_id: value
            .get("conversationId")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned),
        input_tokens: usage.and_then(|object| nonnegative_u64(object.get("inputTokens"))),
        output_tokens: usage.and_then(|object| nonnegative_u64(object.get("outputTokens"))),
        cache_read_tokens: usage.and_then(|object| {
            nonnegative_u64(object.get("cacheReadInputTokens"))
                .or_else(|| nonnegative_u64(object.get("cachedInputTokens")))
        }),
        cache_creation_tokens: usage
            .and_then(|object| nonnegative_u64(object.get("cacheCreationInputTokens"))),
        reasoning_tokens: usage
            .and_then(|object| nonnegative_u64(object.get("reasoningOutputTokens"))),
        // The Hermes extractor reports the total inside `usage`.
        total_tokens: nonnegative_u64(value.get("totalTokens"))
            .or_else(|| usage.and_then(|object| nonnegative_u64(object.get("totalTokens")))),
    })
}

impl OpenCodexEntry {
    fn resolved_total_tokens(&self) -> Option<u64> {
        self.total_tokens.or_else(|| {
            let mut saw = false;
            let mut total = 0u64;
            for value in [
                self.input_tokens,
                self.output_tokens,
                self.cache_creation_tokens,
            ]
            .into_iter()
            .flatten()
            {
                saw = true;
                total = total.saturating_add(value);
            }
            saw.then_some(total)
        })
    }
}

fn parse_timestamp(value: &Value) -> Option<DateTime<Utc>> {
    if let Some(raw) = value.as_str() {
        if let Ok(parsed) = DateTime::parse_from_rfc3339(raw.trim()) {
            return Some(parsed.with_timezone(&Utc));
        }
        if let Ok(number) = raw.trim().parse::<f64>() {
            return timestamp_from_epoch(number);
        }
    }
    value.as_f64().and_then(timestamp_from_epoch)
}

fn timestamp_from_epoch(value: f64) -> Option<DateTime<Utc>> {
    if !value.is_finite() || value <= 0.0 {
        return None;
    }
    let seconds = if value >= 1_000_000_000_000.0 {
        value / 1000.0
    } else {
        value
    };
    // Epoch timestamps fit i64 seconds; float-to-int casts saturate otherwise.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "epoch timestamps fit i64 seconds"
    )]
    let whole = seconds.trunc() as i64;
    // Fractional seconds in [0, 1e9) fit u32.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "fractional seconds in [0, 1e9) fit u32"
    )]
    let nanos = (seconds.fract().abs() * 1_000_000_000.0) as u32;
    Utc.timestamp_opt(whole, nanos).single()
}

fn nonnegative_u64(value: Option<&Value>) -> Option<u64> {
    let value = value?;
    if let Some(number) = value.as_u64() {
        return Some(number);
    }
    let number = value.as_f64()?;
    // `u64::MAX as f64` rounds to 2^64, so the exclusive bound rejects that
    // unrepresentable floating-point boundary before the saturating cast.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "finite non-negative floats below 2^64 fit the intended truncating cast"
    )]
    let parsed =
        (number.is_finite() && number >= 0.0 && number < u64::MAX as f64).then_some(number as u64);
    parsed
}

fn add_optional(left: Option<u64>, right: Option<u64>) -> Option<u64> {
    match (left, right) {
        (Some(left), Some(right)) => left.checked_add(right),
        (Some(left), None) => Some(left),
        (None, Some(right)) => Some(right),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests;
