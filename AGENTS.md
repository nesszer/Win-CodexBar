# Repository Guidelines

Win-CodexBar is the Windows port of CodexBar: a tray app that shows AI-provider usage and limits. This file holds the rules for working in this repo. Long reference material lives in `docs/`; the [docs index](#docs-index) says where.

## How to use this file

- Read the code before you rely on memory or on a doc. The sources of truth are `apps/desktop-tauri` and `rust/src`. When a doc and the code disagree, the code wins; fix the doc.
- Precedence: the user's instructions in the current task come first, then this file, then other docs. Upstream macOS docs under `docs/` are historical unless the task is about upstream parity.
- Do the reversible work first (reading, editing, local builds and tests, local commits). Ask the user before anything in [Ask first](#ask-first).

## Project overview

- The product is the Tauri 2 desktop app in `apps/desktop-tauri`, not the CLI.
- The shared backend and the CLI are the `rust/` crate `codexbar`.
- The root `Cargo.toml` is a workspace with members `rust` and `apps/desktop-tauri/src-tauri`. The Tauri crate is the default member, so plain `cargo` commands from the repo root target the desktop app. Use `cd rust` only for CLI or backend-only work.
- The Tauri crate depends on `codexbar = { path = "../../../rust" }`.
- The frontend is React 18 and Vite in `apps/desktop-tauri/src/`. The typed invoke bridge is `src/lib/tauri.ts`; its DTOs are in `src/types/bridge.ts`.

## Architecture and data flow

[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) has the full picture. The rules that matter on every change:

- **Windows.** The hidden `main` webview routes by window label. Settings, the float bar and the tray-panel flyout are separate windows. The flyout's TrayPanel is the only dashboard layout. The old PopOut layout is retired; `SurfaceMode::PopOut` remains only as a data key.
- **Provider refresh.** `codexbar::core::instantiate_provider` (`rust/src/core/provider_factory.rs`) builds the provider, then `Provider::fetch_usage` runs from the shell's `commands/providers.rs` under a semaphore and a timeout. Results go to `AppState.provider_cache`, then to events, then to the React hook `useProviders`.
- **Settings.** `%APPDATA%\CodexBar\settings.json` is read and written by `Settings::load` / `save` through `secure_file`, which wraps it with DPAPI on Windows. The frontend sends an `updateSettings` patch; the shell saves it and the UI reloads on `codexbar:settings-updated` and the float-bar config events.
- **Tray.** `tray_bridge` and `tray_menu` own the icon and the menu. The icon pixels come from the shared `codexbar::tray::{render_bar_icon_rgba, render_percent_icon_rgba}`.
- **Float bar.** `floatbar/` owns the always-on-top window. Its builder must pin `.theme(Some(tauri::Theme::Dark))`. WebView2 resolves `prefers-color-scheme` on a process profile shared by all windows, so an unpinned float bar flips the other windows when the theme is `auto`. Call this failure the **theme flip**.
- **Proof harness.** `CODEXBAR_PROOF_MODE` (for example `settings:menu`) opens one surface and turns off blur-dismiss for automation. `CODEXBAR_SEED_USAGE_JSON=<abs-path>` seeds synthetic bridge-shaped `ProviderUsageSnapshot` data into the provider cache at launch, pinned against refresh eviction. A malformed seed file logs a warning and is skipped. See [docs/WINDOWS_PROOF.md](docs/WINDOWS_PROOF.md#proof-mode).

## Key directories

| Path | Contents |
| --- | --- |
| `apps/desktop-tauri/src/` | React UI: surfaces, hooks, i18n, bridge types |
| `apps/desktop-tauri/src-tauri/src/` | Tauri shell: `main`, tray, floatbar, shell windows, commands, proof_harness |
| `rust/src/core/` | `ProviderId`, the `Provider` trait, `instantiate_provider`, fetch context |
| `rust/src/providers/` | One module per provider: fetch, parse, auth |
| `rust/src/settings/` | Settings model, load and save |
| `rust/src/browser/` | Windows browser detection and cookie extraction |
| `rust/src/tray/` | Shared tray-icon renderer |
| `rust/src/cli/` | CLI subcommands (`codexbar` binary) |
| `scripts/` | `dev.ps1`, `local-check.ps1`, release, smoke and safety scripts. There is no root `Scripts/` (capital S) pipeline. |
| `docs/` | Windows port docs and ADRs. Upstream macOS docs there are read-only reference. |
| `.circleci/config.yml` | Primary hosted Windows PR and push gate |
| `.github/workflows/pr-check.yml` | Manual Blacksmith Windows reserve |
| `.github/workflows/interaction-guard.yml` | Lightweight GitHub-hosted policy guard |

## Important files

| File | Role |
| --- | --- |
| `apps/desktop-tauri/src-tauri/src/main.rs` | Shell entry, command registration, setup |
| `apps/desktop-tauri/src/App.tsx` | Surface routing by window label |
| `apps/desktop-tauri/src/lib/tauri.ts` | Frontend invoke bridge |
| `apps/desktop-tauri/src/types/bridge.ts` | DTOs and `SettingsTabId` |
| `rust/src/core/provider_factory.rs` | The only provider factory, and `pace_capability` |
| `rust/src/core/provider.rs` | `ProviderId` and the `Provider` trait |
| `apps/desktop-tauri/src-tauri/src/commands/providers.rs` | Refresh engine |
| `apps/desktop-tauri/src-tauri/src/tray_bridge.rs` | Tray icon and menu |
| `apps/desktop-tauri/src-tauri/src/floatbar/window.rs` | Float bar window builder |
| `apps/desktop-tauri/src-tauri/src/surface_target.rs` | Proof and settings tab whitelist |
| `apps/desktop-tauri/src-tauri/tauri.conf.json` | Active Tauri config |
| `scripts/local-check.ps1` | Local CI slice |
| `.circleci/config.yml` | Primary hosted PR and push gate (`.github/workflows/pr-check.yml` is the manual Blacksmith reserve) |
| `CONTEXT.md` | CI roles and budget glossary (Blacksmith pool, `CI_BUDGET_MODE`) |

## Commands

Run from the repo root unless a line says otherwise.

```powershell
# Local CI slice; -Slice ci mirrors the hosted PR check
.\scripts\local-check.ps1
.\scripts\local-check.ps1 -Slice ci

# Rust backend and CLI
cargo test --manifest-path rust/Cargo.toml
cargo clippy --manifest-path rust/Cargo.toml --all-targets -- -D warnings
cargo build -p codexbar
cargo run -p codexbar -- --help

# Tauri shell crate
cargo test --manifest-path apps/desktop-tauri/src-tauri/Cargo.toml
cargo clippy --manifest-path apps/desktop-tauri/src-tauri/Cargo.toml --all-targets -- -D warnings

# Frontend (from apps/desktop-tauri); use pnpm, not npm
pnpm install
pnpm test
pnpm run build
pnpm run tauri:dev
pnpm run tauri:build:debug
pnpm run tauri:build

# Dev launch helpers
.\scripts\dev.ps1
.\scripts\dev.ps1 -SkipBuild
./scripts/dev.sh
```

- A raw `cargo build --release` of the Tauri crate can embed the **dev URL** instead of the built frontend. Build with `pnpm run tauri:build`, `tauri:build:debug` or `scripts/dev.ps1`.
- Cargo builds `codexbar.exe` (CLI) and `codexbar-desktop-tauri.exe` (desktop). Release builds rename them: the installer ships `codexbar.exe` (desktop), `codexbar-cli.exe` (CLI) and `codexbar-desktop.exe` (compatibility alias). See [docs/BUILDING.md](docs/BUILDING.md#where-the-binaries-land).
- When Rust changed, run `cargo fmt --all` before handoff and clippy on both manifests with `-D warnings`. If you skip one, say why.

## Code conventions

- Prefer small typed structs and enums in focused modules. Keep changes local.
- Keep provider-specific logic inside `rust/src/providers/<name>/`. Shared paths have no cross-provider branching; when shared code needs a provider-specific answer, add a typed field or a trait method with a default.
- **New provider:** (1) a `ProviderId` variant with its metadata methods (`cli_name`, `display_name`, ...), (2) a provider module that implements `Provider`, (3) a match arm in `instantiate` and in `pace_capability` in `core/provider_factory.rs`. Both matches are exhaustive, so a missing arm doesn't compile. Never add a second factory in the shell or the CLI. See [docs/PROVIDERS.md](docs/PROVIDERS.md#add-a-provider).
- Errors: `thiserror` (`ProviderError`), and `anyhow` where it is already used. Keep user-facing messages friendly.
- Logging: `tracing` only. Never log secrets, cookies, tokens or raw API keys.
- Frontend tests sit next to the code as `*.test.ts` / `*.test.tsx`. `types/bridge.ts` must match the Rust command payloads, including the settings tab ids.
- **Settings tab ids** are case-sensitive: `general`, `providers`, `notifications`, `menuBar`, `menu`, `usageSpend`, `advanced`, `hooks`, `about`. The backend whitelist in `surface_target.rs` must match the frontend `SettingsTabId` and `TAB_META`. Unknown ids fall back to General in the UI. The old ids `display`, `apiKeys` and `cookies` are not settings tabs.
- Cookie import uses **explicit browser selection** in Settings. Don't assume Chrome.
- Claude CLI output is user-configurable. Don't treat a customizable status line as the usage source of truth.
- Keep provider data siloed: never show identity, plan or email from provider A in provider B's UI.
- Secrets (manual cookies, API keys, token accounts) go through the existing redaction, `secure_file` and keyring helpers.

## Ask first

These need the user's confirmation, because they are hard to undo or affect shared systems:

- Adding a dependency or new tooling.
- Any GitHub write to upstream `steipete/CodexBar`. It is read-only by default. A write needs explicit authorization from the user in the current turn; earlier permission never carries over. Don't open issues or PRs there unless the user asks.
- Opening issues or PRs against trycua/cua. Use it as tooling only.

## GitHub write safety

- Route every GitHub mutation (comment, review, merge, close, edit, create, label, release and so on) through `scripts/gh-safe.sh`. It is the only supported mutation wrapper. It binds the repo, reads the target back, blocks repo overrides and fails closed on a mismatch.
- Always pass the explicit `--repo owner/repo`. Never rely on the current remote or on a PR or issue number alone.
- Before any mutation, read the target back from that same repo and check that the returned owner/repo or URL matches exactly. Abort on any mismatch.
- For object mutations (PR, issue, release), bind both the exact `owner/repo` and the exact number or tag. Use repo-only verification only for creates, where the object doesn't exist yet.
- `gh-safe.sh` allows three repos: `nesszer/Win-CodexBar`, `Finesssee/winget-pkgs` (the fork that receives the manifest branch) and `microsoft/winget-pkgs` (where the Winget PR opens).

## Runtime and tooling

- **pnpm** only. The exact version is pinned only by `packageManager` in `apps/desktop-tauri/package.json`, and the lockfile reflects it. Don't add npm or yarn lockfiles.
- **Node:** CircleCI pins Node 24.18.0. There is no `.nvmrc`. Use 24.18.0 locally to match CI.
- **Rust:** edition 2024, stable, target `x86_64-pc-windows-msvc`. CI pins the version in `scripts/circleci-pinned-rust.txt`. There is no committed `rust-toolchain.toml`, `rustfmt.toml` or `clippy.toml`; use the defaults plus the CI flags (`clippy -- -D warnings`).
- Test tray, DPAPI and browser-cookie behavior on a native Windows host. WSL and Linux can't exercise those paths.

### Worktree storage policy

- Use Git worktrees to isolate branches that are edited at the same time. Read-only review can use an existing checkout, `git show` or `git diff` without a new worktree.
- Load `scripts/worktree-env.ps1` before local Cargo builds. It sets a process-local `CARGO_TARGET_DIR` outside every registered worktree. Set `WCB_CARGO_TARGET_ROOT` for a temporary machine-local cache root, or `WCB_CARGO_TARGET_DIR` for an explicit per-process target.
- Don't commit a shared writable `target-dir` in `.cargo/config.toml`, set a global `CARGO_TARGET_DIR`, or share one exact target directory between concurrent builds. The source worktree stays isolated; only reproducible build output moves.
- `scripts/worktree-storage.ps1` is read-only. It reports free disk, registered worktrees, worktree-local `target` and `node_modules`, and the configured external Cargo target. Its warnings are advisory. It never deletes files, cleans Cargo output, switches branches or prunes Git metadata.
- `target`, Cargo incremental output, frontend build output and reinstallable `node_modules` are disposable. Source edits, commits, branches, PR history and review evidence are not. Before you retire a worktree, check that it is clean and its commit is preserved. Removing a worktree doesn't delete its branch.
- Warning levels: under 60 GiB free, over 1 GiB for one worktree target, or over 5 GiB for all worktree targets together. Review cleanup at once under 35 GiB free, or over 5 GiB for one target or 10 GiB in total. CircleCI keeps its normal runner-local cache and skips this audit.

## Testing & QA

- Rust: prefer focused `#[cfg(test)]` unit tests next to the changed module. After a Rust change, run the tests for both manifests.
- Frontend: Vitest 3 with jsdom and Testing Library. From `apps/desktop-tauri`: `pnpm test` (`src/**/*.{test,spec}.{ts,tsx}`).
- Parser and fetcher changes: add deterministic samples or fixtures where practical.
- Run the checks that fit the change. There are no coverage thresholds; don't invent any.
- **Hosted PR check.** CircleCI Windows is the primary gate. It runs `scripts/local-check.ps1 -Slice ci` for same-repository ordinary and canonical PRs and for `main`, and never for fork PRs. Micro PRs that target `port/upstream-*` skip hosted Windows compute on purpose; a micro-named PR that targets `main` does not. `.github/workflows/pr-check.yml` is a manual Blacksmith Windows reserve only. Budget details are in `CONTEXT.md`, `.github/CI.md` and `docs/adr/`.
- **Local mirror.** `.\scripts\local-check.ps1 -Slice ci` mirrors the hosted check. The default developer slice, with no parameters, doesn't run the full installer or smoke test unless asked.
- **Fork PRs and reviews.** CircleCI never runs on fork PRs, and this project adds no CircleCI API token. Read the full diff. Then run the `-Slice ci` steps from main's copy of `scripts/local-check.ps1` against the fork head (`pull/<number>/head`) in a disposable, credential-free Windows environment (Windows Sandbox or a throwaway VM; a worktree is not isolation), and post the result (see `.github/CI.md`).
- **CodeRabbit** (`.coderabbit.yaml`) reviews every PR, drafts included, against these rules. Fix or answer its findings before merging.

### UI validation with CUA ([trycua/cua](https://github.com/trycua/cua))

Unit tests and `local-check` don't prove tray, Settings, float bar, theme or WebView2 behavior. After any UI, tray, Settings, float-bar or visual change, retest on a real Windows desktop build. Vitest, jsdom and `cargo test` alone are never enough for the tray icon, DWM, the WebView2 theme, float bar z-order or Settings chrome.

1. **Rebuild** a desktop binary that includes the change: `pnpm --dir apps/desktop-tauri run tauri:build:debug` or `.\scripts\dev.ps1`. Proof from an exe built before the change (a **stale exe**) proves nothing.
2. **Close** any running CodexBar. The single-instance plugin can hand a new launch to the old process.
3. **Launch** the new binary in proof mode, for example `$env:CODEXBAR_PROOF_MODE = 'settings:menu'`. The float bar section is on the `menu` pane. For CDP, also set `$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = '--remote-debugging-port=<port>'`, one port per parallel lane.
4. **Drive in the background only.** Never use `bring_to_front`, foreground mode, `hotkey` or `type_text`; they take the user's focus or keyboard. Use CDP for webview controls: attach to the window's own page target by URL, never the first target, click with `Input.dispatchMouseEvent`, and assert the DOM state and the persisted value through the bridge (`get_settings_snapshot`). Use cua-driver 0.31+ with `element_token` from `get_window_state`, and pass one `session` label on the state read and every action that uses its tokens.
5. **Prove with CUA every time:** `verify_state` for window existence and bounds, real-pixel screenshots through `get_window_state` with `--screenshot-out-file`, and native facts CDP can't see (DWM dark under `auto`, float bar topmost and layered, window frame). CDP screenshots are not proof.
6. **Attach the proof** to the PR: CDP assertions and CUA screenshots. List what isn't covered, such as tray icon or tray menu pixels. If CUA can't run, say why and attach equivalent manual proof (PR template).

**Reviewing snapshots.** When you ask Astra or Fable to review snapshots of the app, ask it to collage them into one contact sheet first. It then reads one image instead of one per snapshot, which saves a lot of context.

[docs/WINDOWS_PROOF.md](docs/WINDOWS_PROOF.md) has the cua-driver install command (driver usually at `%LOCALAPPDATA%\Programs\Cua\cua-driver\bin\cua-driver.exe`), the CDP target URLs, the cua-driver session rules and the post-install checklist.

## Commits and PRs

- Short imperative commit messages, for example `Fix Claude CLI parser` or `Improve cookie import errors`. One change per commit.
- A PR includes: a summary of behavior changes, the commands you ran (`cargo test`, `pnpm test`, `.\scripts\local-check.ps1` ...), Windows screenshots or GIFs for UI changes, and the linked issue or reference.
- `ci/circleci: pr-check` is the primary Windows gate; `.github/workflows/pr-check.yml` is the manual Blacksmith backup. Porting micro PRs that target `port/upstream-*` use focused local evidence and skip automatic hosted Windows CI on purpose. Same-repository PRs to `main` always get the hosted gate; fork PRs never do (see Fork PRs and reviews).
- UI, tray, Settings, float-bar and visual PRs need hybrid proof by default: CDP for actions and state, CUA for real pixels and native checks, after a fresh local rebuild. See [UI validation with CUA](#ui-validation-with-cua-trycuacua).
- Before a non-trivial merge, run the thermo-nuclear structure review when the project process requires it.

## Release and Winget

- Update Winget as a normal release step, after the GitHub release artifacts are stable.
- Every version needs its own immutable manifest folder in `microsoft/winget-pkgs`, for example `manifests/f/Finesssee/Win-CodexBar/0.23.6/`. Winget doesn't track "latest".
- For a routine bump, copy the previous approved folder and change only `PackageVersion`, `InstallerUrl`, `InstallerSha256`, `DisplayName`, `DisplayVersion`, `ReleaseNotes` and `ReleaseNotesUrl`.
- Keep `PackageIdentifier`, `InstallerType`, `Scope`, `ProductCode`, `Publisher`, the package URLs and the silent install behavior unchanged unless there is a real packaging reason.
- Before you open a Winget PR, check that the installer URL resolves and recompute the SHA-256 from the downloaded file. On Windows, run `winget validate` when it is available.
- The first submission was approved in `microsoft/winget-pkgs#366653` and the v0.23.5 update in `microsoft/winget-pkgs#366794`. Later updates still go through Microsoft validation and review.

## Docs index

| Need | File |
| --- | --- |
| Windows, data flow, events, pace | `docs/ARCHITECTURE.md` |
| Settings panes, keys, files, portable preferences, hooks | `docs/CONFIGURATION.md` |
| Adding a provider, windows, pace rules, provider notes | `docs/PROVIDERS.md` |
| CLI commands and flags | `docs/CLI.md` |
| Build, checks, release builds, binary names | `docs/BUILDING.md` |
| Browser cookie import | `docs/COOKIES.md` |
| Proof mode, seeding, CDP, cua-driver | `docs/WINDOWS_PROOF.md` |
| CI roles and budget terms | `CONTEXT.md`, `.github/CI.md`, `docs/adr/` |
| Porting from upstream | `docs/PORTING.md` |
