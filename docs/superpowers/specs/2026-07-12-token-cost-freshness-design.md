# Token-Cost Freshness Design

## Goal

Port the relevant CodexBar v0.42.0 behavior into the active Rust/Tauri
architecture:

- token-cost age is independent from provider quota freshness;
- fast cost-fetch failures can retry on the next scheduled pass;
- unknown-model pricing refreshes are bounded and coalesced.

The change remains one focused PR. It does not add a second scheduler,
dependencies, or filesystem paths.

## Architecture

The existing Tauri local-usage cache in `commands/chart.rs` remains the
token-cost refresh boundary. Its cached summary gains an explicit token-cost
timestamp sourced from the local scan result. Consumers use this timestamp for
token-cost age; `AppState.provider_cache_updated_at` continues to describe
provider quota data only.

The shared Rust pricing boundary in `rust/src/core/cost_pricing.rs` owns
unknown-model refresh coordination. A cache-path keyed in-flight guard
coalesces concurrent refreshes. A bounded refresh-attempt timestamp prevents
repeated misses from creating a loop. A miss may trigger at most one refresh
and one retry before the existing fallback behavior is returned.

All existing cache and log paths remain unchanged. Refresh work uses the
existing execution paths and does not become part of provider quota refresh.

## Data flow

1. A local token-cost request checks the token-cost timestamp and its existing
   local usage TTL.
2. If stale, the existing refresh worker performs the scan and stores the
   scan timestamp with the summary.
3. On a non-timeout fetch failure, the token-cost suppression timestamp is
   cleared so a later scheduled/manual pass can retry promptly. Timeout
   failures retain suppression until the normal TTL expires.
4. When pricing lookup cannot find a model, one request starts a refresh for
   the relevant pricing cache path. Concurrent requests await that same
   refresh. After the refresh, the original lookup retries once; if it still
   misses, the existing fallback is used without recursion.

## Error handling

Refresh failures are surfaced through the existing cost-fetch error/fallback
behavior. They do not silently manufacture fresh timestamps or recursively
retry. In-flight state is cleared on both success and failure, and bounded
attempt state is updated only when a refresh is actually attempted.

## Tests

Tests are written before implementation and cover:

- token-cost timestamp and quota freshness remain independent;
- fast cost-fetch failures clear suppression while timeouts retain it;
- concurrent unknown-model misses result in one refresh;
- an unknown model does not loop after the bounded refresh and retry;
- refresh failure releases coalescing state for a later bounded attempt.

Validation includes both Cargo format checks, shared and Tauri Rust tests,
shared and Tauri Clippy with `-D warnings`, frontend checks when touched, and
`git diff --check`.

## Upstream mapping

This ports behavior, not Swift structure:

- upstream `CostUsageFetcher` token snapshots preserve the underlying scan
  timestamp rather than treating every cache read as fresh;
- upstream `UsageStore` clears token-fetch TTL suppression for fast failures
  but keeps it for timed-out scans;
- upstream `ModelsDevPricing` coordinates refreshes by cache path, limits
  repeated attempts, and retries an unknown-model lookup once.

