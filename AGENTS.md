# Repository Guidelines

## Project Overview

- Windows desktop tray app for AI-provider usage and limits (Win-CodexBar port of CodexBar).
- Default product surface: Tauri 2 desktop shell in `apps/desktop-tauri`, not the CLI.
- Shared domain/backend and CLI live in the `rust/` crate `codexbar`.
- Material under `docs/` that describes the upstream macOS/Swift project is historical unless the task is explicitly about upstream parity.
- When repo docs conflict, trust active sources: `apps/desktop-tauri` plus `rust/src`.

## Architecture & Data Flow

- Cargo workspace (root `Cargo.toml`): members `rust`, `apps/desktop-tauri/src-tauri`; **default-member** is the Tauri crate.
- Path dependency: `codexbar-desktop-tauri` → `codexbar = { path = "../../../rust" }`.
- Frontend: React 18 + Vite in `apps/desktop-tauri/src/`. Typed invoke bridge in `src/lib/tauri.ts`; DTOs in `src/types/bridge.ts`.
- Surfaces: the hidden `main` webview routes by window label / surface mode — TrayPanel, Settings, FloatBar. Settings, float bar, and the tray-panel flyout use detached windows. The flyout's TrayPanel is the only dashboard layout; the legacy PopOut layout is retired (`SurfaceMode::PopOut` remains only as a data key).
- **Provider refresh**: `codexbar::core::instantiate_provider` (`rust/src/core/provider_factory.rs`) → `Provider::fetch_usage` → shell `commands/providers.rs` (semaphore + timeout) → `AppState.provider_cache` → events → React `useProviders`.
- **Settings**: `%config%/CodexBar/settings.json` via `Settings::load` / `save` and `secure_file` (DPAPI-capable on Windows). Frontend `updateSettings` patch → save → `codexbar:settings-updated` / float-bar config events.
- **Tray**: `tray_bridge` + `tray_menu`. Icon pixels from shared `codexbar::tray::{render_bar_icon_rgba, render_percent_icon_rgba}`.
- **Float bar**: `floatbar/` owns the auxiliary always-on-top window. The builder must pin `.theme(Some(tauri::Theme::Dark))` — WebView2 resolves `prefers-color-scheme` on a shared process profile; an unpinned window flips other webviews under theme `auto`.
- **Proof harness**: env `CODEXBAR_PROOF_MODE` (e.g. `settings:menu`) opens a target surface and suppresses blur-dismiss for automation / CUA capture. `CODEXBAR_SEED_USAGE_JSON=<abs-path>` seeds one synthetic bridge-shaped Codex `ProviderUsageSnapshot` into the provider cache at launch (pinned against refresh eviction; malformed files are warned about and skipped).

## Key Directories

- `apps/desktop-tauri/src/` — React UI (surfaces, hooks, i18n, bridge types)
- `apps/desktop-tauri/src-tauri/src/` — Tauri shell (`main`, tray, floatbar, shell windows, commands, proof_harness)
- `rust/src/core/` — `ProviderId`, `Provider` trait, `instantiate_provider`, fetch context
- `rust/src/providers/` — one module per provider (fetch/parse/auth)
- `rust/src/settings/` — settings model and load/save
- `rust/src/browser/` — Windows browser detection + cookie extraction
- `rust/src/tray/` — shared tray-icon renderer
- `rust/src/cli/` — CLI subcommands (`codexbar` binary)
- `scripts/` — `dev.ps1`, `local-check.ps1`, release and smoke scripts
- `docs/` — Windows port docs (`ARCHITECTURE`, `CLI`, `CONFIGURATION`, `PROVIDERS`, `BUILDING`, `COOKIES`, `WINDOWS_PROOF`, ADRs). Upstream macOS docs are read-only reference only.
- `.circleci/config.yml` — primary hosted Windows PR/push gate; `.github/workflows/pr-check.yml` — manual Blacksmith Windows reserve; `.github/workflows/interaction-guard.yml` — lightweight GitHub-hosted policy guard.

## Development Commands

```text
# Local CI slice (mirrors hosted PR check)
.\scripts\local-check.ps1

# Rust backend / CLI
cargo test --manifest-path rust/Cargo.toml
cargo clippy --manifest-path rust/Cargo.toml --all-targets -- -D warnings
cargo build -p codexbar
cargo run -p codexbar -- --help

# Tauri shell crate
cargo test --manifest-path apps/desktop-tauri/src-tauri/Cargo.toml
cargo clippy --manifest-path apps/desktop-tauri/src-tauri/Cargo.toml --all-targets -- -D warnings

# Frontend (cwd apps/desktop-tauri) — use pnpm, not npm
pnpm install
pnpm test
pnpm run build
pnpm run tauri:dev
pnpm run tauri:build:debug
pnpm run tauri:build

# Dev launch helpers (repo root)
.\scripts\dev.ps1
.\scripts\dev.ps1 -SkipBuild
./dev.sh
```

- Raw `cargo build --release` on the Tauri crate can still embed the **dev URL**. Prefer `pnpm run tauri:build` / `tauri:build:debug` or `scripts/dev.ps1`.
- Binaries: `codexbar.exe` (CLI), `codexbar-desktop-tauri.exe` (desktop).
- Default desktop work runs from the repo root (default-member). Use `cd rust` only for CLI/backend-only focus.
- There is no active root `Scripts/` (capital S) pipeline — use `scripts/`.
- Format before handoff when Rust changed: `cargo fmt --all`. Clippy both manifests with `-D warnings` (or explain skips).

## Code Conventions & Common Patterns

- Prefer small, typed structs/enums and focused modules; keep changes local.
- Provider-specific logic stays inside `rust/src/providers/<name>/` (or that module). Do not add cross-provider branching in shared paths.
- **New provider**: (1) `ProviderId` variant + metadata methods (`cli_name`, `display_name`, …), (2) provider module implementing `Provider`, (3) match arm in `core/provider_factory.rs::instantiate`. The factory is exhaustive — missing arms fail to compile. Never duplicate factories in the shell or CLI.
- Errors: `thiserror` (`ProviderError`) and `anyhow` where already used; keep user-facing messages friendly.
- Logging: `tracing` only. Never log secrets, cookies, tokens, or raw API keys.
- Frontend tests are co-located `*.test.ts` / `*.test.tsx`. Bridge types in `types/bridge.ts` must stay aligned with Rust command payloads (including settings tab ids).
- **Settings tab ids** (case-sensitive; backend whitelist in `surface_target.rs` must mirror frontend `SettingsTabId` / `TAB_META`): `general`, `providers`, `notifications`, `menuBar`, `menu`, `usageSpend`, `advanced`, `about`. Unknown ids fall back to General in the UI. Old ids `display` / `apiKeys` / `cookies` are not valid settings tabs.
- Cookie import UX uses **explicit browser selection** in Preferences — do not assume Chrome-only.
- Claude CLI output is user-configurable; do not treat a customizable status line as the usage source of truth.
- Keep provider data siloed: never show identity / plan / email from provider A in provider B UI.
- Secrets (manual cookies, API keys, token accounts): use existing redaction, `secure_file`, and keyring helpers.
- Do not add dependencies or tooling without confirmation.
- Do not open issues or PRs against upstream `steipete/CodexBar` unless the user explicitly asks. This repo is Win-CodexBar only.
- **GitHub write safety:** for any mutating `gh` command (comment, review, merge, close, edit, create, label, release, etc.), always pass the explicit `--repo owner/repo`; never rely on the current remote or a PR/issue number alone.
- Before any GitHub mutation, perform a read-back against that same explicit repo and verify the returned owner/repo or URL exactly matches the intended target. Abort on any mismatch.
- Treat `steipete/CodexBar` as **read-only by default**. A GitHub write to upstream requires explicit user authorization in the current turn; prior permission never carries forward.

## Important Files

- `apps/desktop-tauri/src-tauri/src/main.rs` — shell entry, command registration, setup
- `apps/desktop-tauri/src/App.tsx` — surface routing by window label
- `apps/desktop-tauri/src/lib/tauri.ts` — frontend invoke bridge
- `apps/desktop-tauri/src/types/bridge.ts` — DTOs + `SettingsTabId`
- `rust/src/core/provider_factory.rs` — sole provider factory
- `rust/src/core/provider/` — `ProviderId` (`id.rs`), spec table (`spec.rs`), `Provider` trait (`traits.rs`)
- `apps/desktop-tauri/src-tauri/src/commands/providers.rs` — refresh engine
- `apps/desktop-tauri/src-tauri/src/tray_bridge.rs` — tray icon and menu
- `apps/desktop-tauri/src-tauri/src/floatbar/window.rs` — float bar window builder
- `apps/desktop-tauri/src-tauri/src/surface_target.rs` — proof / settings tab whitelist
- `apps/desktop-tauri/src-tauri/tauri.conf.json` — active Tauri config
- `scripts/local-check.ps1` — local CI slice
- `.circleci/config.yml` — primary hosted PR/push gate; `.github/workflows/pr-check.yml` — manual Blacksmith reserve
- `CONTEXT.md` — CI budget glossary (Blacksmith pool / `CI_BUDGET_MODE`)

## Runtime/Tooling Preferences

- Package manager: **pnpm**, with the exact version pinned only by `packageManager` in `apps/desktop-tauri/package.json` (and reflected by the lockfile). Do not introduce npm or yarn lockfiles.
- Node: CircleCI pins **Node 24.18.0**; no `.nvmrc` in repo. Prefer Node 24.18.0 locally for hosted parity.
- Rust: edition **2024**, stable toolchain; CI target `x86_64-pc-windows-msvc`. No committed `rust-toolchain.toml` / `rustfmt.toml` / `clippy.toml` — defaults plus CI flags (`clippy -- -D warnings`).
- Tray / DPAPI / browser-cookie behavior: validate on **Windows-native** hosts. WSL/Linux is insufficient for those paths.
- **CUA (computer-use) for UI proof** — see [Testing & QA](#testing--qa). Project: [trycua/cua](https://github.com/trycua/cua). On this machine the Windows driver is typically `%LOCALAPPDATA%\Programs\Cua\cua-driver\bin\cua-driver.exe`.

### Worktree storage policy

- Keep source isolation in Git worktrees when branches are edited concurrently. Read-only issue review can use an existing checkout, `git show`, or `git diff` without creating another worktree.
- Local Cargo builds should load `scripts/worktree-env.ps1`. It sets a process-local `CARGO_TARGET_DIR` outside every registered worktree. Use `WCB_CARGO_TARGET_ROOT` for a temporary machine-local cache root or `WCB_CARGO_TARGET_DIR` for an explicit per-process override.
- Do not commit a shared writable `target-dir` in `.cargo/config.toml`, set a global `CARGO_TARGET_DIR`, or share one exact target directory between concurrent builds. The source worktree remains isolated; only reproducible build output is redirected.
- `scripts/worktree-storage.ps1` is read-only. It reports free disk, registered worktrees, worktree-local `target`, `node_modules`, and the configured external Cargo target. Storage warnings are advisory and never delete files, clean Cargo output, switch branches, or prune Git metadata.
- Treat `target`, Cargo incremental artifacts, frontend build output, and reinstallable `node_modules` as disposable. Preserve source edits, commits, branches, PR history, and review evidence. Before retiring a worktree, verify it is clean and its commit is preserved; removing a worktree does not delete its branch.
- Use these local warning guides: below 60 GiB free, above 1 GiB per worktree target, or above 5 GiB aggregate worktree targets. Treat below 35 GiB free or above 5 GiB for one target / 10 GiB aggregate as an immediate cleanup review. CircleCI keeps its normal runner-local cache and skips this workstation audit.


## Testing & QA

- Rust: prefer focused `#[cfg(test)]` unit tests near the changed module. Run both manifests after Rust changes.
- Frontend: Vitest 3 + jsdom + Testing Library. From `apps/desktop-tauri`: `pnpm test` (`src/**/*.{test,spec}.{ts,tsx}`).
- **Hosted PR check**: CircleCI Windows is primary and runs `scripts/local-check.ps1 -Slice ci` for same-repository ordinary/canonical PRs and `main` (never for fork PRs); micro PRs targeting `port/upstream-*` intentionally skip hosted Windows compute; a micro-named PR targeting `main` does not. `.github/workflows/pr-check.yml` is manual Blacksmith Windows reserve only. Budget details: `CONTEXT.md`, `.github/CI.md`, and ADRs under `docs/adr/`.
- **Fork PRs and reviews**: CircleCI never runs on fork PRs, and this project adds no CircleCI API token. Read the full diff, then run the `-Slice ci` steps from main's copy of `scripts/local-check.ps1` against the fork head (`pull/<number>/head`) in a disposable, credential-free Windows environment (Windows Sandbox or a throwaway VM; a worktree is not isolation), and post the result (see `.github/CI.md`). CodeRabbit (`.coderabbit.yaml`) reviews every PR, drafts included, against these rules; fix or answer its findings before merging.
- **Hosted mirror**: `.\scripts\local-check.ps1 -Slice ci`. The default no-parameter developer slice remains available and does not run full installer/smoke unless requested.
- Parser / fetcher changes: add deterministic samples or fixtures where practical.
- No coverage thresholds are configured — do not invent any.

### UI validation with CUA ([trycua/cua](https://github.com/trycua/cua))

Unit tests and `local-check` do **not** prove tray, settings, float bar, theme, or WebView2 behavior. For UI / tray / settings / float-bar / visual changes, agents **must** retest on a real Windows desktop build using **Cua Drivers** (background computer-use: click, type, screenshot, UIA) from the open-source [trycua/cua](https://github.com/trycua/cua) project. Docs: [cua.ai/docs](https://cua.ai/docs), driver install: [install guide](https://cua.ai/docs/how-to-guides/driver/install), CLI reference: [cua-driver CLI](https://cua.ai/docs/reference/cua-driver/cli-reference).

**Install (Windows PowerShell, from upstream README):**

```powershell
irm https://cua.ai/driver/install.ps1 | iex
```

Then follow post-install instructions (permissions / accessibility as prompted).

**Typical layout after install:**

- Driver binary: `%LOCALAPPDATA%\Programs\Cua\cua-driver\bin\cua-driver.exe`
- Long-lived daemon: `cua-driver serve` (often over a named pipe). One-shot tools: `cua-driver call <tool> '<json>'` (e.g. `list_windows`, `get_window_state`, `click`, screenshots via `--screenshot-out-file`).

**Required retest loop after UI-affecting code changes:**

1. **Rebuild** a local desktop binary that includes the change (`pnpm --dir apps/desktop-tauri run tauri:build:debug` or `.\scripts\dev.ps1`). Do not validate against a stale pre-change exe.
2. **Close** any already-running CodexBar instance (single-instance plugin may hand off to the old process).
3. **Launch** the new binary. For stable automation (no blur-dismiss), set proof mode, e.g.  
   `$env:CODEXBAR_PROOF_MODE = 'settings:menu'`  
   (settings tab ids: `general`, `providers`, `notifications`, `menuBar`, `menu`, `usageSpend`, `advanced`, `about` — float bar section is on **`menu`**).  
   To use CDP, also set `$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = '--remote-debugging-port=<port>'` (one port per parallel lane).
4. **Drive in the background only** — never `bring_to_front`, foreground mode, `hotkey`, or `type_text`; they take the user's focus or keyboard.
   - **CDP (preferred for webview controls):** read `http://127.0.0.1:<port>/json/list` and attach to the window's own page target by URL — main / tray panel / Settings: `http://tauri.localhost/`; float bar: `…?window=floatbar…`. Never the first target: tools that attach to it or open `about:blank` (e.g. browser-use) see a blank page. Click with `Input.dispatchMouseEvent` (does not move the OS cursor or take focus); assert DOM state and the persisted value via the app bridge (`get_settings_snapshot`).
   - **cua-driver 0.31+:** actions take `element_token` from `get_window_state` (`element_index` / `snapshot_id` are rejected); pixel clicks need a screenshot from the same session first. Pass the same `session` label on the state read and every action that uses its tokens (or keep one `serve` connection); separate one-shot `call`s without it retire the snapshot, so the token goes stale.
5. **Prove with CUA every time:** `verify_state` (`timeout_ms`, `stable_samples`) for window existence/bounds, real-pixel screenshots via `get_window_state` + `--screenshot-out-file`, and native facts CDP can't see (DWM dark under `auto`, float bar topmost/layered, window frame). CDP screenshots are not proof.
6. **Attach proof** to the PR: CDP assertions + CUA screenshots; list anything not covered (e.g. tray icon / tray menu pixels) explicitly. If CUA cannot run, say why and attach equivalent manual proof (PR template).

**Do not** treat Vitest/jsdom or `cargo test` alone as sufficient for tray icon, DWM, WebView2 theme, float bar z-order, or settings chrome. **Do not** open issues/PRs against trycua/cua unless the user explicitly asks; use it as tooling.


## Commit & PR Guidelines

- Short imperative commit messages (e.g. `Fix Claude CLI parser`, `Improve cookie import errors`).
- Keep commits scoped to one change.
- In PRs / patches include:
  - Summary of behavior changes
  - Commands run (`cargo test`, `pnpm test`, `.\scripts\local-check.ps1`, etc.)
  - Screenshots / GIFs for UI changes (Windows)
  - Linked issue / reference when relevant
- Hosted PR check exists: `ci/circleci: pr-check` is the primary Windows gate; `.github/workflows/pr-check.yml` is manual Blacksmith backup. Porting micro PRs targeting `port/upstream-*` use focused local evidence and intentionally skip automatic hosted Windows CI; same-repository `main`-bound PRs always get the hosted gate; fork PRs never do (see Fork PRs and reviews).
- UI / tray / settings / float-bar / visual PRs: **hybrid proof is the default** — CDP for actions and state assertions, CUA Driver ([trycua/cua](https://github.com/trycua/cua)) for real-pixel screenshots and native checks — after a **fresh local rebuild** — see [UI validation with CUA](#ui-validation-with-cua-trycuacua). If CUA cannot be used, explain why and attach equivalent manual proof (PR template checkboxes).
- Before non-trivial merge: thermo-nuclear structure review when the project process requires it.


## Release & Winget Notes

- Treat Winget updates as a normal release step after GitHub release artifacts are stable.
- Winget does not track "latest" GitHub releases; every version needs its own immutable manifest folder in `microsoft/winget-pkgs`, for example `manifests/f/Finesssee/Win-CodexBar/0.23.6/`.
- For routine version bumps, copy the previous approved manifest folder and change only version-specific fields: `PackageVersion`, `InstallerUrl`, `InstallerSha256`, `DisplayName`, `DisplayVersion`, `ReleaseNotes`, and `ReleaseNotesUrl`.
- Keep stable package identity and installer behavior unchanged unless there is a real packaging reason: `PackageIdentifier`, `InstallerType`, `Scope`, `ProductCode`, `Publisher`, package URLs, and silent install behavior.
- Before opening a Winget PR, verify the release installer URL resolves and recompute the SHA-256 from the downloaded asset. On Windows, run `winget validate` when available.
- The first Winget package submission was approved in `microsoft/winget-pkgs#366653`; the v0.23.5 update was approved in `microsoft/winget-pkgs#366794`. Future updates should be faster, but still expect Microsoft validation/review.
- Agents must route GitHub mutations through `scripts/gh-safe.sh` instead of calling mutating `gh` subcommands directly. This is the sole supported mutation wrapper. It owns repo binding, performs read-back verification, blocks repo overrides, and fails closed on target mismatch.
- `gh-safe.sh` allowlists three repos: `nesszer/Win-CodexBar`, plus `Finesssee/winget-pkgs` (the fork that receives the manifest branch) and `microsoft/winget-pkgs` (where the winget PR opens).
- For object mutations (PR/issue/release), bind both the exact `owner/repo` and exact object number/tag. Use repo-only verification only for creates where the target object does not exist yet.
