# Building from source

This page shows how to build and test Win-CodexBar on Windows. The desktop app is the Tauri shell in `apps/desktop-tauri`. The CLI and the shared backend are the `codexbar` crate in `rust/`.

## Prerequisites

- Rust stable with the `x86_64-pc-windows-msvc` target. CI pins the version in `scripts/circleci-pinned-rust.txt` (1.98.0).
- Visual Studio Build Tools with the **Desktop development with C++** workload.
- Node.js. CI uses Node 24.18.0, so use the same version locally.
- pnpm, at the exact version in the `packageManager` field of `apps/desktop-tauri/package.json` (`pnpm@11.25.0`). Corepack can activate it. Don't use npm or yarn.

Install the tools yourself with rustup, winget or corepack, or with a tool manager such as mise. The repo has no setup script that installs them.

## Build the desktop app

Run these from the repo root:

```powershell
pnpm --dir apps/desktop-tauri install --frozen-lockfile
pnpm --dir apps/desktop-tauri run tauri:build
```

`tauri:build` runs `tauri build --no-bundle`. It builds the frontend and the release desktop binary, `codexbar-desktop-tauri.exe`, and makes no installer.

For a faster debug build:

```powershell
pnpm --dir apps/desktop-tauri run tauri:build:debug
```

A raw `cargo build --release` of the Tauri crate can embed the dev server URL instead of the built frontend. Use the pnpm scripts or `scripts/dev.ps1` instead.

## Build the CLI only

```powershell
cargo build -p codexbar --release
```

The binary is `codexbar.exe`.

## Where the binaries land

Without extra setup, Cargo writes to `target\` in the repo root, for example `target\release\codexbar-desktop-tauri.exe` and `target\release\codexbar.exe`.

In a Git worktree, load `scripts/worktree-env.ps1` first. It sets `CARGO_TARGET_DIR` for the current PowerShell process only, to a folder outside every registered worktree:

```powershell
. .\scripts\worktree-env.ps1
```

The default folder is `%LOCALAPPDATA%\Win-CodexBar\cargo-target\repo-<id>\worktree-<id>`. The script prints the path it chose. Set `WCB_CARGO_TARGET_ROOT` to change the root folder, or `WCB_CARGO_TARGET_DIR` to set the exact folder. `scripts/local-check.ps1` loads this script itself.

## Run the app while you develop

`scripts/dev.ps1` builds the desktop app and starts it:

```powershell
.\scripts\dev.ps1
.\scripts\dev.ps1 -Release
.\scripts\dev.ps1 -SkipBuild
.\scripts\dev.ps1 -Verbose
```

- With no switch, it makes a debug build with `tauri:build:debug` and starts it.
- `-Release` builds with `tauri:build` instead.
- `-SkipBuild` starts the last build without building again.
- `-Verbose` sets `RUST_LOG=debug`.

The script installs nothing and doesn't run `pnpm install`. It checks that `cargo` and `pnpm.cmd` are on `PATH`, and it checks for `dlltool` on GNU toolchain hosts only. Run `pnpm install` yourself first.

For hot reload of the frontend, run Tauri dev mode:

```powershell
pnpm --dir apps/desktop-tauri run tauri:dev
```

On macOS, Linux or WSL, `scripts/dev.sh` takes `--release`, `--skip-build`, `--verbose` and `--cli`. In WSL with no display server, it switches to CLI mode.

## Run the checks

Run the same check as the hosted PR check:

```powershell
.\scripts\local-check.ps1 -Slice ci
```

`CONTEXT.md` lists its 11 steps. With no parameters, `local-check.ps1` runs a shorter developer check: the worktree storage report, the `gh-safe.sh` tests, both Rust test suites, and the frontend tests and build. Add `-Format`, `-Clippy`, `-ReleaseDoctor` or `-All` for more.

To run single steps:

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
pnpm --dir apps/desktop-tauri test
pnpm --dir apps/desktop-tauri run lint
pnpm --dir apps/desktop-tauri run build
```

`pnpm run build` runs `check-locale` first, then `tsc --noEmit` and `vite build`. `check-locale` fails when the Rust and TypeScript locale keys differ.

Tests and builds don't prove tray, settings, float bar or WebView2 behavior. See [WINDOWS_PROOF.md](./WINDOWS_PROOF.md).

## Build a release on Windows

GitHub Actions builds, signs and drafts the published releases when a `vX.Y.Z` tag is pushed (`.github/workflows/release.yml`). [release/ci-cd.md](./release/ci-cd.md) describes that flow.

To build release assets on your own Windows PC, use the cached release builder:

```powershell
.\scripts\windows-release-build.ps1 -Ref vX.Y.Z
```

It builds from a clean checkout under `-WorkRoot` (default `C:\code\Win-CodexBar-release`):

- `source\` holds the checkout.
- `cache\` keeps the pnpm store, the Cargo output and the installer downloads between runs.
- `assets\` receives the release files.

The script renames the binaries for release. The desktop app becomes `codexbar.exe`, with `codexbar-desktop.exe` as a second name for older shortcuts. The CLI becomes `codexbar-cli.exe`. It writes these assets, each with a `.sha256` file:

| Asset | Contents |
| --- | --- |
| `CodexBar-<version>-Setup.exe` | Inno Setup installer for `codexbar.exe`, `codexbar-cli.exe` and `codexbar-desktop.exe` |
| `CodexBar-<version>-portable.exe` | The desktop app as one file |
| `CodexBarCLI-v<version>-windows-x64.zip` | `codexbar-cli.exe` |

Keep the `.sha256` files. They are the source for Winget's `InstallerSha256`.

Other switches:

| Switch | What it does |
| --- | --- |
| `-WarmCacheOnly` | Builds the desktop binary to fill the caches, then stops before installer packaging |
| `-SmokeInstall` | Installs and uninstalls the new installer to test it |
| `-RefreshInstallerDependencies` | Downloads the installer bootstrappers again instead of reusing the cached copies |
| `-RepoUrl` | The repo to clone when the managed checkout doesn't exist yet (default `https://github.com/nesszer/Win-CodexBar.git`) |

The script only builds and tests. It has no upload switch.

To check release assets:

```powershell
.\scripts\release-doctor.ps1 -Version X.Y.Z
```

`release-doctor.ps1` also takes `-AssetsDir` and `-SkipGitHub`.

To test an installer on its own:

```powershell
.\scripts\windows-smoke-install.ps1 -InstallerPath <path-to-Setup.exe> -ExpectedVersion X.Y.Z
```

The smoke test checks the silent install switches (`/VERYSILENT /SUPPRESSMSGBOXES /NORESTART`), the installed files, the CLI `--help` and `--version` output, the Start Menu shortcut, the uninstall registry entry and the silent uninstall. Add `-LeaveInstalled` to keep the app installed for manual checks, or `-InstallDir` to choose the install folder.

## Cross-build from macOS

For a compile check from a Mac, use the cross-build wrapper:

```bash
./scripts/macos-windows-cross-build.sh
```

Or run the package script:

```bash
pnpm --dir apps/desktop-tauri run tauri:build:windows-cross
```

This uses `cargo-xwin` with Homebrew `llvm` and `lld`, and builds `target/x86_64-pc-windows-msvc/release/codexbar-desktop-tauri.exe`. It catches frontend, Tauri and Windows-target Rust compile errors. It doesn't replace a Windows build: installer packaging, tray behavior, WebView2, DPAPI, startup and smoke install still need a real Windows PC.

## Project layout

```
Win-CodexBar/
├── apps/desktop-tauri/          Tauri desktop shell
│   ├── src/                     React frontend (TypeScript)
│   └── src-tauri/src/
│       ├── commands/            Tauri commands called by the frontend
│       ├── shell/               Windows: activation, DWM, flyout and Settings windows
│       ├── floatbar/            Float bar window
│       ├── tray_bridge.rs       Tray icon and menu
│       └── main.rs              Desktop entry point
├── rust/                        Shared backend crate and CLI
│   └── src/
│       ├── core/                Provider ids, the Provider trait, the provider factory
│       ├── providers/           One module per provider
│       ├── settings/            Settings model, load and save
│       ├── browser/             Browser detection and cookie import
│       ├── tray/                Tray icon renderer
│       ├── cli/                 CLI subcommands
│       └── main.rs              CLI entry point
├── docs/                        Documentation
└── scripts/                     Dev, check and release scripts
```

## Documentation map

| Doc | Contents |
| --- | --- |
| [ARCHITECTURE.md](./ARCHITECTURE.md) | Modules, entry points and data flow |
| [CLI.md](./CLI.md) | `codexbar` CLI commands |
| [CONFIGURATION.md](./CONFIGURATION.md) | Settings, config files and the Settings window |
| [PROVIDERS.md](./PROVIDERS.md) | Providers, sign-in sources and usage windows |
| [COOKIES.md](./COOKIES.md) | Browser cookie import |
| [WSL.md](./WSL.md) | WSL limits |
| [WINDOWS_PROOF.md](./WINDOWS_PROOF.md) | Proving UI changes on Windows |
| [release/ci-cd.md](./release/ci-cd.md) | Hosted PR check and the release workflow |
| [../AGENTS.md](../AGENTS.md) | Rules for agents and contributors |

The upstream macOS docs (`steipete/CodexBar`) are read-only reference. Don't copy Swift, Keychain or Sparkle steps here without rewriting them for Windows.

## Stored process environments

A process environment carries API keys, tokens and cookies. Every struct, union or enum-variant field that stores one uses `codexbar::process_environment::ProcessEnvironment`. This includes optional maps and `(OsString, OsString)` pair lists.

- Its `Debug` output is `ProcessEnvironment(N entries; redacted)`. So `{:?}`, `dbg!`, `tracing` field captures and `assert_eq!` failures never print variable names or values.
- Wrapping an `Option` keeps `None` apart from an empty map. Equality still compares the original contents.
- Deref, iteration, `get` and `insert` still return the original values for the child process. Never log what they return.
- Function parameters and locals are short-lived and stay unwrapped.

The `process_environment::storage_guard` test scans `rust/src` and `apps/desktop-tauri/src-tauri/src` for fields with an environment-like name whose type is a string map or a string pair list. It looks through `Option`, `Box`, `Arc`, references, slices and local `type` aliases, and fails on any field that is not wrapped. Each entry in its `REVIEWED_EXCEPTIONS` list must match exactly one declaration, so stale or duplicate exceptions fail too.

The guard matches text. It doesn't check types. Generic parameters, fields with other names and explicit logging of an unwrapped map still need code review.

```bash
cargo test -p codexbar process_environment
```
