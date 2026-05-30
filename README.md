# Win-CodexBar

[简体中文说明](./README.zh-CN.md)

The Windows port of [CodexBar](https://github.com/steipete/CodexBar) — a system tray app that keeps your AI coding-tool usage limits visible at a glance.

> Built with **Tauri + React** on a shared **Rust** backend. The original CodexBar is a macOS Swift app by [Peter Steinberger](https://github.com/steipete).

<p align="center">
  <img src="extra-docs/images/tray-panel.png" width="280" alt="Tray panel showing provider grid and Codex usage"/>
  &nbsp;&nbsp;
  <img src="extra-docs/images/settings-providers.png" width="480" alt="Settings — Providers tab"/>
</p>

## Features

- **49 AI providers** — Codex, Claude, Cursor, Factory, Gemini, Copilot, Antigravity, z.ai, MiniMax, Kiro, Vertex AI, Augment, OpenCode, Kimi, Kimi K2, Amp, Warp, Ollama, Azure OpenAI, T3 Chat, OpenRouter, Synthetic, JetBrains AI, Alibaba, Alibaba Token Plan, NanoGPT, Infini, Perplexity, Abacus AI, Mistral, OpenCode Go, Kilo, AWS Bedrock, Codebuff, DeepSeek, Windsurf, Manus, Xiaomi MiMo, Doubao, Command Code, Crof, StepFun, Venice, OpenAI, Grok, ElevenLabs, Deepgram, Groq, LLM Proxy
- **System tray icon** — dynamic two-bar meter showing session + weekly usage
- **Floating Bar** — optional always-on-top transparent capacity strip with orientation, opacity, and click-through controls
- **Browser cookie import** — Chrome, Edge, Brave, Firefox, with browser access kept opt-in
- **Per-provider credentials** — API keys, cookies, and OAuth all managed from the provider detail pane
- **Credential hardening** — local secret-bearing stores are protected with Windows DPAPI on save
- **Windows release packaging** — Inno Setup installer, standalone portable exe, WebView2 runtime bootstrap, VC++ runtime bootstrap, and SHA-256 checksum files
- **CLI** — `codexbar usage`, `codexbar cost`, `codexbar config`, and loopback `codexbar serve` for scripting and local integrations
- **WSL support** — CLI works out of the box; desktop shell via WSLg

## What's New in v0.31.1

- Fixed Antigravity usage on Windows when the local language server binds its API to a random listening port instead of a port near `--extension_server_port`.
- The app now checks the Antigravity language-server process's actual listening ports first, while keeping the older heuristic port probes as a fallback.

## v0.31.0

- Ported upstream CodexBar v0.31.0 provider fixes into Win-CodexBar.
- Added AWS Bedrock usage through named AWS CLI profiles, including SSO/assume-role profiles that the AWS CLI can resolve.
- Added Codex Spark 5-hour and weekly quota lanes when the Codex usage endpoint returns Spark-specific limits.
- Hid Claude's obsolete Design quota lane while keeping the remaining Claude usage windows intact.
- Made local Codex/Claude chart scans cancellation-aware so repeated refreshes stop obsolete JSONL scans sooner.

## v0.30.3

- Fixed DeepSeek balance display for accounts that only have CNY/RMB credit. A zero USD balance no longer hides a positive CNY balance or marks the provider exhausted.
- Verified the DeepSeek CNY fallback regression on Windows EC2 with the native Rust provider tests.
- Includes the v0.30.2 About tab link fix below.

## v0.30.2

- Fixed About tab external buttons so GitHub, Website, Original Project, and inline project links open through the Tauri shell on Windows.
- Verified the About tab link flow on a real Windows EC2 desktop with Cua Driver CLI against the native Tauri window.
- Includes the v0.30.1 Codex local usage fixes below.

## v0.30.1

- Fixed local Codex token usage parsing for current Codex session logs.
- Fixed cached input tokens being double-counted in local token totals.
- Routes Codex local cost scanning through the shared JSONL scanner so the tray, chart, and CLI paths stay aligned.
- Refreshes the tray layout after async local usage data loads without relying on a hidden global browser event.
- Includes the v0.30.0 provider updates below.

## v0.30.0

- Adds DeepSeek usage summaries on top of balance tracking: token totals, request counts, top model, category breakdowns, and current-month cost when the platform API exposes them.
- Scopes OpenAI Admin API usage by optional project ID from the provider detail pane, while leaving organization-wide usage as the default.
- Updates Alibaba Token Plan to the current Bailian subscription summary API and broadens parsing for the newer quota/reset field names.
- Refreshes expired StepFun Oasis tokens when a combined access/refresh token is available.
- Shows richer Ollama pacing windows and Antigravity per-model quota windows in the tray/settings UI.
- Keeps the v0.29 provider work: Alibaba Token Plan tracking, OpenCode renewal windows, Codex cost buckets, Azure OpenAI validation, T3 Chat quota tracking, and hardened OpenAI/MiniMax parsing.

## Quick Start

```powershell
# Prerequisites: Node.js + pnpm — Rust and MinGW are installed automatically
git clone https://github.com/Finesssee/Win-CodexBar.git
cd Win-CodexBar
.\dev.ps1
```

The script installs Rust/MinGW if needed, builds the Tauri desktop shell, and launches the app.

```powershell
.\dev.ps1 -Release          # optimised build
.\dev.ps1 -SkipBuild        # relaunch last build
```

## Download

Install with Windows Package Manager:

```powershell
winget install Finesssee.Win-CodexBar
```

Winget distribution is approved through [microsoft/winget-pkgs](https://github.com/microsoft/winget-pkgs/tree/master/manifests/f/Finesssee/Win-CodexBar). New releases may take a little time to appear in Winget after the GitHub release is published because each version is pinned to its own installer URL and SHA-256 hash.

You can also grab the latest build from [GitHub Releases](https://github.com/Finesssee/Win-CodexBar/releases).

- **Installer**: `CodexBar-<version>-Setup.exe`
- **Portable**: `CodexBar-<version>-portable.exe`
- **Checksums**: each release includes `.sha256` files for manual verification

The installer includes the desktop app, Microsoft's Evergreen WebView2 bootstrapper, app icon, Start Menu shortcut, uninstall metadata, and the Visual C++ runtime bootstrap needed on clean Windows machines. The portable exe is the same desktop app without installer integration; release builds statically link the WebView2 loader, so portable users only need the Microsoft Edge WebView2 Runtime installed on the machine.

## Fast Windows Release Builds

For local release builds on a Windows machine, use the cached release builder:

```powershell
.\scripts\windows-release-build.ps1 -Ref v0.31.1
```

Automated Windows release builds now run through CircleCI hosted Windows instead of GitHub Actions or AWS EC2. Cloudflare R2 can mirror verified artifacts after the Windows smoke install passes. See [docs/release/ci-cd.md](docs/release/ci-cd.md).

The script keeps a clean managed checkout under `C:\code\Win-CodexBar-release\source`, stores Rust build output in `C:\code\Win-CodexBar-release\cache\cargo-target`, stores pnpm packages in `C:\code\Win-CodexBar-release\cache\pnpm-store`, and reuses signed WebView2/VC++ bootstrapper downloads. It still builds the real release binary, verifies Microsoft signatures for installer dependencies, packages with Inno Setup, and writes the same four GitHub release assets under `C:\code\Win-CodexBar-release\assets`.

Useful release flags:

```powershell
.\scripts\windows-release-build.ps1 -Ref v0.31.1 -WarmCacheOnly
.\scripts\windows-release-build.ps1 -Ref v0.31.1 -WarmCliCache
.\scripts\windows-release-build.ps1 -Ref v0.31.1 -SmokeInstall
.\scripts\windows-release-build.ps1 -Ref v0.31.1 -UploadRelease v0.31.1
.\scripts\release-doctor.ps1 -Version 0.31.1
```

GitHub Actions are manual best-effort only for this project. CircleCI hosted Windows is the primary automated release path for installer and portable artifacts.

## First Run

1. Launch CodexBar — it sits in the system tray
2. Click the tray icon to open the usage panel
3. Open **Settings → Providers**, enable the services you use
4. For cookie-based providers, click the provider and use **Browser Cookies → Import**
5. For Claude, browser cookies/sessionKey are preferred because they match the settings-page usage numbers; OAuth and CLI stay as fallbacks
6. For CLI-based providers (`codex`, `gemini`), make sure you're logged in

## CLI

```bash
codexbar usage -p claude          # single provider
codexbar usage -p all             # all enabled providers
codexbar cost  -p codex           # local cost from JSONL logs
```

## Providers

| Provider | Auth | Tracks |
|----------|------|--------|
| Codex | OAuth / CLI | Session, Weekly, Credits |
| Claude | Cookies / OAuth fallback / CLI fallback | Session (5h), Weekly |
| Cursor | Cookies | Plan, Usage, Billing |
| Factory | Cookies | Usage |
| Gemini | gcloud OAuth | Quota |
| Copilot | GitHub Device Flow / gh CLI / legacy token | Plan usage, Chat |
| Antigravity | Local LSP | Usage, Per-model quotas |
| z.ai | API Token | Quota |
| MiniMax | API / Cookies | Usage, Billing Summary |
| Kiro | Cookies / CLI | Monthly Credits, Overage |
| Vertex AI | gcloud OAuth | Cost |
| Augment | Cookies | Credits |
| OpenCode | Local Config | Usage |
| Kimi | Cookies | 5h Rate, Weekly |
| Kimi K2 | API Key | Credits |
| Amp | Cookies | Usage |
| Warp | Local Config | Usage |
| Ollama | Cookies / API Key | Usage, Cloud Models, Pace windows |
| Azure OpenAI | API Key | Deployment |
| T3 Chat | Cookies / cURL | Base, Overage |
| OpenRouter | API Key | Credits |
| JetBrains AI | Local Config | Usage |
| Alibaba | Cookies | Usage |
| Alibaba Token Plan | Cookies | Token Plan Credits, Reset date |
| NanoGPT | API Key | Credits |
| Infini | API Key | Session, Weekly, Quota |
| Perplexity | Cookies | Credits, Plan |
| Abacus AI | Cookies | Credits |
| Mistral | Cookies | Billing, Usage |
| OpenCode Go | Cookies | Usage, Zen Balance |
| Kilo | API Key / CLI | Usage |
| Codebuff | API Key / Local Config | Credits, Weekly |
| DeepSeek | API Key | Balance, Usage summaries, Cost |
| Windsurf | Local Cache | Daily, Weekly |
| Manus | Cookies | Credits, Refresh Credits |
| Xiaomi MiMo | Cookies | Balance, Token Plan |
| Doubao | API Key | Request Limits |
| Command Code | Cookies | Monthly Credits, Purchased Credits |
| Crof | API Key | Credits, Request Quota |
| StepFun | Oasis Token | 5h, Weekly, Token refresh |
| Venice | API Key | USD / DIEM Balance |
| OpenAI | Admin API / API Key | Usage, Requests, Project-scoped cost, Credit Balance |
| Grok | Cookies / auth.json | Billing |
| ElevenLabs | API Key | Subscription Credits, Voice Slots |
| Deepgram | API Key | Project Usage |
| Groq | API Key | Enterprise Metrics |
| LLM Proxy | API Key | Quota Stats |

## Privacy

- **On-device only** — no data sent anywhere except provider APIs
- **No disk scanning** — only reads known config paths and browser cookies
- **Opt-in cookies** — extraction only runs for providers you enable
- **Protected credential stores** — app-managed API keys, manual cookies, and token accounts are written through the secure-file layer; on Windows this uses user-scoped DPAPI where available
- **Safe diagnostics** — diagnostic snapshots expose provider/source/status metadata only, never raw cookies, API keys, bearer tokens, or OAuth values
- **Verified updates** — automatic installer downloads require a GitHub SHA-256 digest and the installer is re-verified immediately before apply

## More Docs

| Topic | Link |
|-------|------|
| Building from source | [extra-docs/BUILDING.md](extra-docs/BUILDING.md) |
| WSL setup & auth tips | [extra-docs/WSL.md](extra-docs/WSL.md) |
| Browser cookie details | [extra-docs/COOKIES.md](extra-docs/COOKIES.md) |

## Credits

- **Original CodexBar**: [steipete/CodexBar](https://github.com/steipete/CodexBar) by Peter Steinberger
- **Inspired by**: [ccusage](https://github.com/ryoppippi/ccusage) for cost tracking

## License

MIT — same as the original CodexBar.

---

*For the macOS version, visit [steipete/CodexBar](https://github.com/steipete/CodexBar).*
