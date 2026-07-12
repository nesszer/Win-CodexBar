# Token-Cost Freshness Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Port upstream v0.42.0 token-cost freshness, fast retry, and bounded unknown-model pricing refresh behavior into the existing Rust/Tauri cost pipeline.

**Architecture:** Keep local token scanning and its in-memory cache in `commands/chart.rs`; add a scan timestamp and a narrow failure-retry policy without coupling it to `AppState.provider_cache_updated_at`. Add the models.dev catalog, disk cache, dynamic pricing lookup, and cache-path keyed refresh coordinator under the shared Rust `core` pricing boundary. The Tauri chart refresh performs one scan, refreshes only when that scan reports unknown models, and rescans once when pricing becomes available.

**Tech Stack:** Rust 2024, Tokio already present in both crates, existing `reqwest` async client, `serde`/`serde_json`, Tauri commands, existing Vitest frontend tests.

## Global Constraints

- Port behavior, not Swift structure, from upstream steipete/CodexBar v0.41.0-v0.42.0.
- Token-cost age is independent from provider quota freshness.
- Fast cost-fetch failures clear token-cost suppression; timed-out scans retain the normal TTL.
- Unknown-model refreshes are bounded, coalesced by cache path, and retry the lookup at most once.
- Do not add a second scheduler or dependencies.
- Preserve all existing filesystem paths.
- Keep provider-specific pricing and refresh logic inside the shared cost/pricing boundary.
- Do not log raw tokens, cookies, API keys, or model-pricing response contents.
- Use TDD: write each focused failing test before its implementation.

---

### Task 1: Track scan age and cost-fetch retry policy

**Files:**
- Modify: `rust/src/cost_scanner.rs:23-48` to expose unavailable model IDs.
- Modify: `apps/desktop-tauri/src-tauri/src/commands/chart.rs:19-285` to carry scan age, separate cache age from quota age, and classify failures.
- Test: `apps/desktop-tauri/src-tauri/src/commands/chart.rs` in a new `#[cfg(test)]` module.

**Interfaces:**
- `CostSummary::unknown_models: HashSet<String>`.
- `ProviderLocalUsageSummary::token_cost_updated_at_ms: i64`, serialized as `tokenCostUpdatedAtMs`.
- `fn token_cost_cache_is_fresh(loaded_at: Option<Instant>, now: Instant, ttl: Duration) -> bool`.
- `fn cost_fetch_failure_allows_early_retry(failure: CostFetchFailure) -> bool`, false only for `TimedOut`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn token_cost_age_does_not_use_provider_quota_age() {
    let token_loaded = Instant::now() - Duration::from_secs(31);
    let provider_updated = Instant::now();
    assert!(!token_cost_cache_is_fresh(
        Some(token_loaded), Instant::now(), Duration::from_secs(30)
    ));
    assert!(is_provider_cache_fresh(Some(provider_updated), Duration::from_secs(30)));
}

#[test]
fn fast_cost_failures_allow_the_next_pass_to_retry() {
    assert!(cost_fetch_failure_allows_early_retry(CostFetchFailure::Failed));
    assert!(!cost_fetch_failure_allows_early_retry(CostFetchFailure::TimedOut));
}
```

Also assert that serialized local usage contains `tokenCostUpdatedAtMs`, and
update existing Rust summary fixtures with the timestamp.

- [ ] **Step 2: Run focused tests and verify failure**

Run `cargo test --manifest-path apps/desktop-tauri/src-tauri/Cargo.toml token_cost_age_does_not_use_provider_quota_age fast_cost_failures_allow_the_next_pass_to_retry`.
Expected: the new field and functions do not compile yet.

- [ ] **Step 3: Implement the minimal cache/retry change**

Set the token timestamp from local scan completion, never from
`AppState.provider_cache_updated_at`. Keep the local cache `loaded_at` as a
separate `Instant`. Record unknown model IDs while preserving current fallback
costs. Clear only token-cost suppression for fast failures; retain it for
timeouts.

- [ ] **Step 4: Run the focused tests and verify success**

Run the command from Step 2. Expected: cache-age, retry-policy, and
serialization tests pass.

- [ ] **Step 5: Commit**

```powershell
git add rust/src/cost_scanner.rs apps/desktop-tauri/src-tauri/src/commands/chart.rs apps/desktop-tauri/src-tauri/src/commands/tests.rs
git commit -m "Separate token-cost age from quota freshness`n`nCo-authored-by: Copilot App <223556219+Copilot@users.noreply.github.com>"
```

### Task 2: Add models.dev pricing cache and coalesced refresh

**Files:**
- Create: `rust/src/core/models_dev_pricing.rs` for catalog decoding, cache artifacts, dynamic pricing, and refresh coordination.
- Modify: `rust/src/core/mod.rs:3-33` to register and re-export the pipeline.
- Modify: `rust/src/core/cost_pricing.rs:585-812` to consult refreshed rates after static lookup.
- Test: `rust/src/core/models_dev_pricing.rs` for parser, cache, and coordinator behavior.

**Interfaces:**
- `pub async fn refresh_unknown_models_if_needed(provider_id: &str, model_ids: &HashSet<String>) -> bool`.
- `pub fn lookup(provider_id: &str, model_id: &str) -> Option<DynamicModelPricing>`.
- A cache-path keyed coordinator with one in-flight operation and a 15-minute attempt gate.

- [ ] **Step 1: Write failing coalescing and bounded-attempt tests**

```rust
#[tokio::test]
async fn concurrent_refreshes_for_one_cache_path_share_one_operation() {
    let coordinator = ModelsDevRefreshCoordinator::default();
    let calls = Arc::new(AtomicUsize::new(0));
    let first_calls = Arc::clone(&calls);
    let first = coordinator.refresh(path.clone(), now, async move {
        first_calls.fetch_add(1, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(10)).await;
        true
    });
    let second = coordinator.refresh(path, now, async {
        panic!("the second caller must await the first operation");
    });
    assert!(tokio::join!(first, second).0);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn failed_refresh_is_not_retried_within_the_attempt_window() {
    let coordinator = ModelsDevRefreshCoordinator::default();
    assert!(!coordinator.refresh(path.clone(), now, async { false }).await);
    assert!(!coordinator.refresh(path, now + Duration::from_secs(60), async {
        panic!("the 15-minute bound must suppress this attempt");
    }).await);
}
```

Add JSON tests for both the top-level provider map and the `providers` envelope,
including conversion from USD per million tokens to USD per token.

- [ ] **Step 2: Run focused tests and verify failure**

Run `cargo test --manifest-path rust/Cargo.toml models_dev_pricing`.
Expected: coordinator, catalog, and dynamic lookup types are missing.

- [ ] **Step 3: Implement the cache and coordinator**

Decode both catalog envelopes; store a versioned artifact under the existing
per-user cache root without moving any existing path; use a 24-hour cache TTL;
require priceable OpenAI and Anthropic entries; merge still-priceable entries
from the previous catalog; and use the existing async `reqwest::Client` with a
20-second timeout. Key in-flight and last-attempt state by standardized cache
path. Return false on transport, status, decode, or implausible-catalog errors
without logging response contents.

- [ ] **Step 4: Implement dynamic lookup**

Add `DynamicModelPricing` with input, output, cache-read, cache-write, and
optional over-200K rates. Have static pricing win first, then consult the
refreshed provider/model catalog, then preserve the existing fallback.

- [ ] **Step 5: Run focused tests and commit**

Run `cargo test --manifest-path rust/Cargo.toml models_dev_pricing` and
`cargo test --manifest-path rust/Cargo.toml test_unknown_model_falls_back_to_sonnet`.
Then commit:

```powershell
git add rust/src/core/mod.rs rust/src/core/models_dev_pricing.rs rust/src/core/cost_pricing.rs
git commit -m "Coalesce unknown-model pricing refreshes`n`nCo-authored-by: Copilot App <223556219+Copilot@users.noreply.github.com>"
```

### Task 3: Wire one bounded unknown-pricing retry into Tauri

**Files:**
- Modify: `apps/desktop-tauri/src-tauri/src/commands/chart.rs:70-285` for one scan, refresh, and rescan.
- Modify: `rust/src/cost_scanner.rs` and `rust/src/codex_costs.rs` to record unknown IDs.
- Modify: `apps/desktop-tauri/src-tauri/src/commands/tests.rs` and `powertoys.rs` for the timestamp field.
- Modify: `apps/desktop-tauri/src/types/bridge.ts` and affected frontend fixtures.
- Test: `chart.rs` for one refresh/rescan and no recursion.

**Interfaces:**
- The local scan result carries the summary and `HashSet<String>` unknown IDs.
- The pricing pipeline is called once; a successful refresh permits one rescan
  with retry disabled, and an unsuccessful refresh returns the first result.
- Token-cost timestamp remains independent from provider snapshot `updated_at`.

- [ ] **Step 1: Write failing retry-bound tests**

Use a test-only callback seam:

```rust
#[test]
fn unknown_models_trigger_one_refresh_and_one_rescan() {
    let mut refresh_calls = 0;
    let mut scan_calls = 0;
    let outcome = retry_unknown_pricing_once(
        vec!["claude-future-1".to_string()],
        || { refresh_calls += 1; true },
        || { scan_calls += 1; vec![] },
    );
    assert_eq!(refresh_calls, 1);
    assert_eq!(scan_calls, 1);
    assert!(outcome.unknown_models.is_empty());
}

#[test]
fn unavailable_pricing_does_not_loop() {
    let outcome = retry_unknown_pricing_once(
        vec!["claude-future-1".to_string()],
        || false,
        || panic!("no second scan after an unsuccessful refresh"),
    );
    assert_eq!(outcome.unknown_models, ["claude-future-1"]);
}
```

- [ ] **Step 2: Run focused tests and verify failure**

Run `cargo test --manifest-path apps/desktop-tauri/src-tauri/Cargo.toml unknown_models_trigger_one_refresh_and_one_rescan unavailable_pricing_does_not_loop`.
Expected: the retry seam is not present.

- [ ] **Step 3: Wire the existing chart refresh**

After the first 30-day/today scan, collect unknown IDs and call the shared
async pricing pipeline through the existing Tauri runtime. If pricing becomes
available, rerun that scan exactly once with retry disabled. Otherwise retain
the first result and fallback costs. Keep `is_refreshing` and the existing
provider scheduler unchanged. Clear only token-cost suppression on fast worker
failure; retain it on timeout; do not let a late cancelled worker overwrite a
newer cache entry.

- [ ] **Step 4: Update fixtures and run focused checks**

Run:

```powershell
cargo test --manifest-path apps/desktop-tauri/src-tauri/Cargo.toml unknown_models_trigger_one_refresh_and_one_rescan unavailable_pricing_does_not_loop
cd apps/desktop-tauri
npm test -- --run src/types/bridge.test.ts src/components/MenuCard.test.tsx src/floatbar/FloatBar.test.tsx
```

- [ ] **Step 5: Commit**

```powershell
git add rust/src/cost_scanner.rs rust/src/codex_costs.rs apps/desktop-tauri/src-tauri/src/commands/chart.rs apps/desktop-tauri/src-tauri/src/commands/tests.rs apps/desktop-tauri/src-tauri/src/powertoys.rs apps/desktop-tauri/src/types/bridge.ts
git commit -m "Retry unknown token pricing once`n`nCo-authored-by: Copilot App <223556219+Copilot@users.noreply.github.com>"
```

### Task 4: Full validation, reviews, and focused PR

- [ ] **Step 1: Format and inspect**

Run `cargo fmt --all -- --check` and `git diff --check`.

- [ ] **Step 2: Validate shared Rust**

Run `cargo test --manifest-path rust/Cargo.toml` and
`cargo clippy --manifest-path rust/Cargo.toml --all-targets -- -D warnings`.

- [ ] **Step 3: Validate Tauri Rust**

Run `cargo test --manifest-path apps/desktop-tauri/src-tauri/Cargo.toml` and
`cargo clippy --manifest-path apps/desktop-tauri/src-tauri/Cargo.toml --all-targets -- -D warnings`.

- [ ] **Step 4: Validate frontend**

From `apps/desktop-tauri`, run `npm test -- --run`, `npm run lint`, and
`npm run build` because the bridge type changes.

- [ ] **Step 5: Run correctness and thermo-nuclear maintainability reviews**

Review the complete diff against `main` for quota/token age separation,
fast-versus-timeout retry behavior, one cache-path in-flight refresh, one
unknown-model rescan, no recursive loop, preserved paths, provider boundaries,
duplicate schedulers, oversized abstractions, duplicated normalization, broad
error swallowing, and unnecessary public APIs. Fix only findings caused by the
PR and rerun affected tests.

- [ ] **Step 6: Push and open, without merging**

Run `git diff main...HEAD --check`, verify the worktree and final diff, then
push `finesssee-port-cost-freshness` and open one PR into `main` documenting
the upstream v0.41.0-v0.42.0 mapping and the exact checks above.
