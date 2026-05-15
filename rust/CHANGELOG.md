# Changelog

All notable changes to Win-CodexBar will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/).

---

## [Unreleased]

---

## [0.26.0] — 2026-05-15

### Added
- Ported upstream CodexBar 0.26 AWS Bedrock monthly spend tracking to the Windows/Tauri Rust backend using AWS Cost Explorer and SigV4 request signing.
- Added Bedrock provider IDs, CLI aliases, metadata, Settings provider rows, API-key configuration support, credential metadata, and provider icon registry entries.
- Added OpenRouter daily, weekly, and monthly API-key spend windows from the `/api/v1/auth/key` endpoint.
- Added Moonshot / Kimi API balance parsing for international and China API regions, voucher balance, cash balance, and deficit state.

### Changed
- Updated app/package versions and provider catalog metadata for the 0.26.0 Windows release.

### Notes
- Upstream 0.26 also includes macOS/Swift-only menu, Sparkle, localization, and native settings changes that do not map to the Windows/Tauri shell.

---

## [0.25.1] — 2026-05-11

### Changed
- Aligned the Windows/Tauri package version with upstream CodexBar 0.25.1 after reviewing the upstream patch set.
- Updated app, CLI, package, Tauri, and release metadata for the 0.25.1 Windows artifact release.

### Notes
- Upstream 0.25.1 fixes macOS SwiftPM localization bundle lookup, macOS Keychain cache prompt churn, Pi session cost cache migration, Swift concurrency annotations, and standalone Swift CLI archive version fallback. The Windows/Tauri port does not use those Swift/macOS code paths, so this release keeps runtime behavior unchanged while providing a verified Windows 0.25.1 build.

---

## [0.25.0] — 2026-05-11

### Added
- Ported upstream CodexBar 0.25 provider support for Manus, Xiaomi MiMo, Doubao, Command Code, Crof, StepFun, Venice, and OpenAI API balance to the Windows/Tauri Rust backend.
- Added new provider IDs, CLI aliases, metadata, Settings provider rows, API-key configuration entries, cookie-domain mappings, token-account support for cookie-backed providers, and provider icon registry entries.
- Added provider fetchers for credit balances, request quotas, refresh-credit windows, token-plan usage, purchased credits, DIEM/USD balances, and OpenAI API credit grants.

### Changed
- Updated app/package versions and README provider catalog metadata for the 0.25.0 Windows release.

---

## [0.24.1] — 2026-05-11

### Fixed
- Bundle Microsoft's Evergreen WebView2 bootstrapper in the Inno installer and install WebView2 Runtime silently on clean Windows machines before launching CodexBar.

---

## [0.24.0] — 2026-05-10

### Added
- Ported upstream CodexBar 0.24 provider support for Codebuff, DeepSeek, and Windsurf to the Windows/Tauri Rust backend.
- Added Codebuff and DeepSeek API-key provider configuration, CLI aliases, credential metadata, icons, and chart colors.
- Added Windsurf local cached-plan usage support using the platform application-data `state.vscdb` cache.

### Changed
- Updated app/package versions and provider catalog metadata for the 0.24.0 Windows release.

---

## [0.23.11] — 2026-05-10

### Fixed
- Handle Claude Web usage payloads that include overlapping design or routines alias fields without failing with a duplicate-field parse error.
- Keep Claude Web parse diagnostics useful without exposing raw response bodies in user-facing errors or logs.

---

## [0.23.10] — 2026-05-06

### Fixed
- Route active Claude OAuth token accounts through OAuth mode and pass the selected token directly into the Claude OAuth fetcher.
- Keep Claude `sessionKey` token accounts on the web/cookie path instead of confusing them with OAuth tokens.
- Report OAuth, Web, and CLI failures together in Claude Auto mode so a final CLI parse error no longer hides earlier token or cookie failures.

---

## [0.23.7] — 2026-05-03

### Fixed
- Parse Claude CLI's exhausted `You've hit your limit · resets ...` short form as full session usage instead of reporting `Claude CLI did not return usage data`.
- Make Claude CLI usage parsing more tolerant of compact labels, decimal percentages, and remaining/available wording.
- Keep weekly reset lines from being promoted into the session reset when the session section has no reset.

### Security
- Re-enable the Tauri content security policy and disable global Tauri injection.
- Narrow the default Tauri capability permissions to the event, window, and global shortcut APIs the frontend actually uses.
- Harden external URL opening by validating web URLs and avoiding `cmd /c start` on Windows.

---

## [0.23.6] — 2026-04-30

### Fixed
- Use active token accounts during provider refresh so Cursor, Ollama, and other cookie-token providers fetch via web cookies instead of falling back to unsupported CLI mode.

---

## [0.23.5] — 2026-04-29

### Added
- Added safe diagnostics and credential storage status reporting for the Tauri shell without exposing secret values.
- Added a Windows installer smoke-test script for silent install, installed-file, registry, shortcut, and uninstall validation.

### Changed
- Reused fresh provider refresh results during startup and panel opening to reduce avoidable provider fetches.

### Fixed
- Redacted secret-like values from provider refresh errors before they cross the Tauri bridge.
- Re-verify downloaded installer SHA-256 hashes immediately before applying an update.
- Hardened desktop command inputs for provider IDs, credential values, cookie source values, region values, token accounts, and filesystem paths.

---
## [1.2.12] — 2026-04-04

### Security
- Harden updater integrity checks with SHA256 digest validation (PR #24)
- Harden CLI binary resolution against CWD hijacking (PR #26)
- Harden Kiro CLI path resolution with PATH support and filename validation (PR #25)
- Verify VC++ runtime signature/checksum before packaging (PR #23)

### Added
- Add Infini AI Coding Plan provider support with 5-hour/7-day/30-day usage windows (PR #28)

### Fixed
- Fix Windows CLI visibility - `codexbar.exe usage` now produces output in PowerShell (Issue #22)
- Fix Claude `.local/bin` path detection for Windows native installs (Issue #22)
- Fix Claude auto-source error reporting to surface auth errors before CLI timeout (Issue #22)
- Fix Infini provider `with_base_url()` being ignored in `fetch_usage()` (PR #28 fix)
- Fix Kiro provider PATH lookup removal that regressed WSL/Linux support (PR #25 fix)

---
## [1.2.10] — 2026-04-03

### Added
- Added NanoGPT as a supported provider on current `main`, including daily/monthly usage parsing and the related Windows preferences wiring.

### Fixed
- Aligned the CLI help text with the actual supported provider set so `nanogpt` now appears in both top-level help and `codexbar usage --help`.

---
## [1.2.9] — 2026-04-03

### Changed
- Added a Summary view to the Windows popup so enabled providers can be scanned together without switching cards one by one, while keeping the native Windows title bar and resize behavior intact.
- Replaced the stale root Swift CI and Linux release workflows with Rust-native automation that validates and packages the `codexbar` binary from `rust/`.

### Fixed
- Claude web usage fetching now accepts `CLAUDE_AI_SESSION_KEY` and `CLAUDE_WEB_SESSION_KEY` values as either raw session tokens or `sessionKey=...`, and it sends the shared browser-style headers Claude expects on every API request.
- Localized the new Summary-view strings in both English and Chinese instead of shipping hardcoded English labels.

---

## [1.2.8] — 2026-03-31

### Changed
- The Windows installer now bundles Microsoft's Visual C++ redistributable and installs it before launching CodexBar on clean machines.

### Fixed
- Fixed clean-Windows installer runs failing to launch CodexBar because the required Visual C++ runtime was missing.

---

## [1.2.7] — 2026-03-31

### Changed
- Completed the Windows updater flow so update checks, background downloads, and quit-time install behavior use the same settings and release asset selection rules.

### Fixed
- Wired the existing `install_updates_on_quit` setting into the actual Windows quit path instead of leaving it as a dead preference.
- Added visible General -> Updates controls for auto-download and quit-time install behavior in both English and Chinese.
- Hardened pending installer detection so cached portable executables and stale/current-version installers are ignored, while the newest newer installer is preferred.
- Fixed manual Check for Updates to respect the same auto-download behavior as startup checks and added a guard to prevent overlapping installer downloads.
- Added coverage for pending-installer selection and beta-style installer names in the updater tests.

---

## [1.2.6] — 2026-03-31

### Changed
- Hardened the Windows release path by restoring the installer/portable asset flow and cleaning up repo-wide lint debt so the Rust quality gates stay green.

### Fixed
- Added a clear Windows Remote Desktop startup guard so the app shows a direct dialog instead of failing with opaque renderer errors.
- Completed the Windows localization/auth recovery cleanup from the outstanding PR work and shipped the Claude parsing/update-path fixes in the merged codebase.

---

## [1.2.5] — 2026-03-29

### Added
- Windows now loads common Simplified Chinese font fallbacks so localized UI text renders correctly instead of missing glyph boxes.

### Changed
- Localized major Windows UI surfaces to Simplified Chinese, including the main window, tray menus/tooltips, provider detail actions, and settings sections.

### Fixed
- Reset/usage/status strings now display correctly in the localized Windows UI instead of mixing untranslated English labels through key flows.

---

## [1.0.1] — 2026-01-19

### Added

#### Swift Feature Port (Wave 1)
Complete porting of Swift CodexBar features to Rust Windows version:

- **Icon Morphing**: "Unbraid" animation from ribbons to usage bars
- **Model-Level Cost Breakdowns**: Per-model cost tracking on chart hover
- **Augment Session Keepalive**: Background cookie refresh before expiry
- **VertexAI Token Refresher**: OAuth token refresh with caching
- **MiniMax LocalStorage Import**: Browser localStorage session extraction
- **Web Probe Watchdog**: Process watchdog for browser automation
- **Usage Pace Prediction**: On Track/Ahead/Behind quota calculation with ETA
- **Personal Info Redaction**: Email address privacy protection for streaming
- **Copilot Device Flow OAuth**: GitHub Device Flow authentication
- **Zai MCP Details Submenu**: Per-model usage breakdown
- **OpenAI Deep Scraper**: React Fiber inspection for dashboard scraping
- **Provider-Specific Icon Twists**: Unique visual styles per provider
- **Eye Blink System**: Micro-motion animations with per-provider state
- **Command Runner**: Process execution with timeout and stop conditions
- **Token Account Multi-Support**: Multi-account token management with parallel fetching
- **Credential Migration System**: Windows credential format upgrades with version tracking
- **OpenAI Friendly Errors**: Human-readable Cloudflare/login/rate-limit detection
- **OpenCode Advanced Scraper**: Workspace ID resolution from JSON/HTML
- **Kiro CLI Version Detection**: Semver parsing with compatibility checks
- **Weekly Indicator Bars**: 4px progress bars in provider switcher tabs
- **Smart Menu Invalidation**: Version-based tracking prevents unnecessary rebuilds
- **Eye Blink Animation**: Random blinks with 18% double-blink probability
- **Icon Twist System**: Provider-specific visual styles (Claude crab, Gemini sparkle, etc.)
- **Provider Status Indicators**: Health overlays with Statuspage.io integration
- **Session Quota Notifications**: Depleted/restored state tracking with alerts
- **Cost Usage Pricing**: Model-specific token pricing (GPT-5, Claude Opus/Sonnet/Haiku)
- **JSONL Scanner**: Incremental log file parsing with file-level caching for Codex/Claude sessions
- **OpenAI Dashboard Models**: Usage breakdown and credits data structures
- **Cookie Header Cache**: Cookie normalization and caching with staleness tracking
- **Provider Fetch Plan**: Orchestrated fetching with strategy pipelines and fallback logic
- **Widget Snapshot**: Data export structures for external widget integrations
- **TTY Command Runner**: Windows-optimized command execution with ConPTY-style features

#### New Providers
- **Amp Provider**: Sourcegraph/Cody with API token support
- **Synthetic Provider**: Usage tracking support

#### UI Enhancements
- **API Keys Tab**: Provider access token configuration UI
- **Tab Icons**: Emoji icons in preference tabs
- **Tilt Animation**: New surprise animation
- **Unbraid Animation**: New loading animation pattern
- Preferences window now resizable
- Console window hides automatically in GUI mode

#### New Modules
`keepalive`, `token_refresher`, `local_storage`, `watchdog`, `usage_pace`, `redactor`, `icon_twist`, `blink`, `device_flow`, `mcp_details`, `scraper`, `command_runner`, `token_accounts`, `credential_migration`, `friendly_errors`, `version`, `weekly_indicator`, `menu_invalidation`, `indicators`, `session_quota`, `cost_pricing`, `jsonl_scanner`, `openai_dashboard`, `cookie_cache`, `fetch_plan`, `widget_snapshot`, `tty_runner`

### Changed
- Renamed "Zed AI" to "Zai" across entire codebase (display names, docs, comments)
- Build now uses GNU toolchain (`x86_64-pc-windows-gnu`) to avoid MSVC linker PATH conflicts
- Provider count increased from 15 to 18
- Refactored `status.rs` into `status/mod.rs` + `status/indicators.rs`

### Fixed
- Fixed 80 compiler warnings with targeted `#[allow(...)]` attributes

### Technical
- Added `ApiKeys` storage system
- Enhanced release profile (`opt-level=3`, `panic=abort`)
- All 156 tests passing

---

## [1.0.0] — 2025-01-17

### Added
- Initial Windows port of CodexBar using Rust + egui
- System tray integration with animated icons
- Support for 15 AI providers: Claude, Codex, Cursor, Gemini, Copilot, Antigravity, Windsurf, Zai, MiniMax, Kiro, Vertex AI, Augment, OpenCode, Kimi, Kimi K2
- Native Windows notifications via toast
- Browser cookie extraction (Chrome, Edge, Firefox, Brave)
- Keyboard shortcuts via global-hotkey
- Cost history charts with egui_plot
- CLI commands: `usage`, `cost`, `menubar`, `autostart`
- Windows installer via Inno Setup
- Auto-update checker
- Loading animations: Knight Rider, Cylon, OutsideIn, Race, Pulse
- Surprise animations: Blink, Wiggle, Pulse, Rainbow
- Provider status page integration
- Manual cookie paste support
- Preferences window with provider toggles

---

[Unreleased]: https://github.com/Finesssee/Win-CodexBar/compare/v1.2.10...HEAD
[1.2.10]: https://github.com/Finesssee/Win-CodexBar/compare/v1.2.9...v1.2.10
[1.2.9]: https://github.com/Finesssee/Win-CodexBar/compare/v1.2.8...v1.2.9
[1.2.8]: https://github.com/Finesssee/Win-CodexBar/compare/v1.2.7...v1.2.8
[1.0.1]: https://github.com/Finesssee/Win-CodexBar/compare/v1.0.0...v1.0.1
[1.0.0]: https://github.com/Finesssee/Win-CodexBar/releases/tag/v1.0.0
