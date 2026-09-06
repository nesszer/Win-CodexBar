# Providers (Windows)

Windows rewrite of the *role* of upstream `docs/providers.md`: how providers are registered and fetched in **this** repo.
Do **not** treat upstream’s full strategy table as authoritative for Win-CodexBar without checking code — IDs and auto-order drift.

## Single factory

All shells and the CLI construct providers through:

```text
codexbar::core::instantiate_provider  →  rust/src/core/provider_factory.rs
```

`ProviderId` lives in `rust/src/core/provider.rs`. The factory match is **exhaustive** (missing arm = compile error). Tests ensure every id instantiates.

**Never** duplicate provider factories in the Tauri shell or ad-hoc commands.

## Adding a provider

1. Add a `ProviderId` variant + `cli_name` / `display_name` / cookie domain / `from_cli_name` metadata as required.
2. Implement `Provider` in `rust/src/providers/<name>/` (or module).
3. Add the match arm in `provider_factory.rs::instantiate`.
4. Keep provider-specific parsing and auth **inside** that module — no cross-provider branching in shared UI paths.
5. Keep identity / plan / email **siloed** per provider in the UI.

## Fetch strategies (concept)

Same vocabulary as upstream, implemented in Rust:

| Source label | Meaning (typical) |
|--------------|-------------------|
| `auto` | Provider-specific fallback order |
| `web` | Cookie / dashboard HTTP |
| `cli` | Local CLI / PTY / RPC helpers |
| `oauth` | OAuth-backed flows where supported |

CLI: `codexbar usage --source auto|web|cli|oauth`.

Auth resolution helpers in `rust/src/providers/` commonly try: explicit settings → keyring/entry → environment variables (exact order is provider-specific).

## Cookie-backed providers

Windows browser import: Chrome, Edge, Brave (DPAPI + AES-GCM), Firefox (SQLite).  
Settings → **Providers** → provider detail → choose browser → Import.  
Manual cookie header paste is the fallback (required under WSL for Chromium DPAPI).  
Details: [COOKIES.md](./COOKIES.md).

## Ollama: browser-priority cookie starvation (fixed, PR #430)

`resolve_browser_cookie_header` (`rust/src/providers/ollama/cookies.rs`) used to call the shared `browser_cookies_for_domain` helper, which returns the **first installed browser** that has *any* cookies for `ollama.com` — even stale/irrelevant ones (analytics, consent) with no session cookie. On a machine with multiple Chromium browsers, an earlier-priority browser (Chrome, Edge) with only junk cookies silently starved out a later one (often Brave) that actually held the logged-in session, surfacing `No cookies available for web API` despite a valid session existing on disk.

Fixed by walking every detected browser and using the first one whose cookies contain a recognized Ollama session cookie name, instead of stopping at the first non-empty result. See `first_recognized_cookie_header` and its regression test.

## MiniMax: client-rendered console pages (fixed, PR #431)

MiniMax redesigned `platform.minimax.io`: `/console/usage` and `/console/plan` are Next.js pages loaded via `next/dynamic(..., { ssr: false })`. The server never emits real quota numbers in the initial HTML — `__NEXT_DATA__.props.pageProps.userConfig` is always `null` — regardless of cookie validity. Every cookie/HTML-scraping path is therefore structurally unable to read usage on the current site; it silently fell back to `probe_cli()`'s hardcoded 0% "configured" stub.

The coding-plan `remains` endpoint (`/v1/api/openplatform/coding_plan/remains`) that the HTML scraper already falls back to also accepts a plain `Authorization: Bearer <api_key>` with **no cookie at all**, returning the same `model_remains` JSON the existing parser (`coding_plan.rs`) already handles. `fetch_via_web` now tries this Bearer path first (key from Settings or `MINIMAX_API_KEY`) before the legacy `group_id`+`api_key` billing endpoint. MiniMax is now also registered in `get_api_key_providers()` so a plain API key can be set via Settings / `codexbar config set-api-key minimax` — previously only a paired `group_id`+`api_key` (env vars or a local `minimax`-CLI-style config file) was supported.

## Codex: external OAuth staleness gate

Codex reads the CLI-owned OAuth session from `~/.codex/auth.json` (`rust/src/providers/codex/api.rs`). This file is not managed by CodexBar — it is written and refreshed by the `codex` CLI itself.

Upstream 0.50.1 #2944 added a fail-closed gate for this case: when `auth.json` has a `refresh_token` (i.e. it is a CLI-owned "external OAuth" credential, not an API key) and `last_refresh` is **older than `EXTERNAL_OAUTH_STALENESS_WINDOW`** (8 days, `codex/api.rs`), CodexBar refuses to use it and reports `AuthRequired` instead of silently trusting a possibly-stale/compromised token. `Settings.codex_external_oauth_sources_allowed` (default `false`) bypasses this gate when explicitly enabled.

Symptom: Codex shows "Authentication required" / `codexbar diagnose -p codex` reports `error.category: "auth"` even though `auth.json` exists and looks otherwise valid — the fix is not a CodexBar config change, it's refreshing the CLI's own session:

```powershell
codex doctor          # does a real reachability handshake; updates last_refresh
# or
codex login status    # lighter, but only checks local state — does not always refresh
```

Any Codex CLI use that reaches the network (interactive session, `codex exec`, `codex doctor`) updates `last_refresh` and clears the gate on CodexBar's next poll. No CodexBar restart is required.

## Listing what is enabled

```powershell
codexbar config providers
codexbar config enable -p cursor
codexbar config disable -p cursor
```

Desktop: Settings → Providers (sidebar reorder, per-provider credential UI).

## Status pages

Optional status polling (provider status pages) is available via CLI `--status` and Settings advanced toggles where wired. Mapping of Statuspage vs Google incidents is provider metadata in code — see provider modules rather than upstream-only URLs if they disagree.

## Usage & Spend

Desktop tab id: `usageSpend`. The desktop and Overview consume one shared spend catalog. Codex and Claude local logs are first-class; routed OpenCodex usage enriches the matching Codex, OpenCode Go, Kimi, or DeepSeek subscription instead of appearing as a second fake provider. xAI and OpenRouter can publish exact provider-metered daily USD spend when their management credentials are configured, while Grok local sessions contribute tokens only. Missing spend sources remain unknown rather than becoming a false `$0`. Do not invent cross-currency totals.

Custom pricing overlays are exact-match overrides used only where the local spend contract has matching provider/model token evidence. Explicit zero rates mean free; omitted rate fields stay unknown. The Usage & Spend surface keeps provenance/coverage visible, preserves cost-only model rows when token coverage is partial, and can Copy JSON or save the same JSON contract through the native file picker.

## Upstream doc warning

Upstream `docs/providers.md` is a large auto-strategy matrix (60+ providers) for the macOS app. Use it as **inspiration** when porting a provider. For runtime truth on Windows:

1. `rust/src/core/provider.rs` (`ProviderId`)
2. `rust/src/providers/<id>/`
3. `codexbar usage -p <id> -v` / desktop provider detail errors

## Related

- [ARCHITECTURE.md](./ARCHITECTURE.md)
- [CLI.md](./CLI.md)
- [CONFIGURATION.md](./CONFIGURATION.md)
- [COOKIES.md](./COOKIES.md)
