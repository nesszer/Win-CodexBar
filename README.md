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

- **48 AI providers** — Codex, Claude, Cursor, Factory, Gemini, Copilot, Antigravity, z.ai, MiniMax, Kiro, Vertex AI, Augment, OpenCode, Kimi, Kimi K2, Amp, Warp, Ollama, Azure OpenAI, T3 Chat, OpenRouter, Synthetic, JetBrains AI, Alibaba, NanoGPT, Infini, Perplexity, Abacus AI, Mistral, OpenCode Go, Kilo, AWS Bedrock, Codebuff, DeepSeek, Windsurf, Manus, Xiaomi MiMo, Doubao, Command Code, Crof, StepFun, Venice, OpenAI, Grok, ElevenLabs, Deepgram, Groq, LLM Proxy
- **System tray icon** — dynamic two-bar meter showing session + weekly usage
- **Floating Bar** — optional always-on-top transparent capacity strip with orientation, opacity, and click-through controls
- **Browser cookie import** — Chrome, Edge, Brave, Firefox, with browser access kept opt-in
- **Per-provider credentials** — API keys, cookies, and OAuth all managed from the provider detail pane
- **Credential hardening** — local secret-bearing stores are protected with Windows DPAPI on save
- **Windows release packaging** — Inno Setup installer, standalone portable exe, WebView2 runtime bootstrap, VC++ runtime bootstrap, and SHA-256 checksum files
- **CLI** — `codexbar usage`, `codexbar cost`, `codexbar config`, and loopback `codexbar serve` for scripting and local integrations
- **WSL support** — CLI works out of the box; desktop shell via WSLg

## What's New in v0.28.0

- Adds Azure OpenAI deployment validation from saved/env credentials.
- Adds T3 Chat quota tracking with browser cookies or a full copied cURL request.
- Adds Ollama API-key support while keeping browser-cookie usage as the Web source.
- Hardens OpenAI dashboard account scraping, MiniMax billing aggregation, and provider catalog/settings metadata.

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

For local release builds on a Windows server, use the cached release builder:

```powershell
.\scripts\windows-release-build.ps1 -Ref v0.28.0
```

The script keeps a clean managed checkout under `C:\code\Win-CodexBar-release\source`, stores Rust build output in `C:\code\Win-CodexBar-release\cache\cargo-target`, stores pnpm packages in `C:\code\Win-CodexBar-release\cache\pnpm-store`, and reuses signed WebView2/VC++ bootstrapper downloads. It still builds the real release binary, verifies Microsoft signatures for installer dependencies, packages with Inno Setup, and writes the same four GitHub release assets under `C:\code\Win-CodexBar-release\assets`.

Useful release flags:

```powershell
.\scripts\windows-release-build.ps1 -Ref v0.28.0 -WarmCacheOnly
.\scripts\windows-release-build.ps1 -Ref v0.28.0 -WarmCliCache
.\scripts\windows-release-build.ps1 -Ref v0.28.0 -SmokeInstall
.\scripts\windows-release-build.ps1 -Ref v0.28.0 -UploadRelease v0.28.0
.\scripts\release-doctor.ps1 -Version 0.28.0
```

GitHub Actions are best-effort only for this project. The Windows server script is the primary release path for installer and portable artifacts.

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
| Antigravity | Cookies / LSP | Usage |
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
| Ollama | Cookies / API Key | Usage, Cloud Models |
| Azure OpenAI | API Key | Deployment |
| T3 Chat | Cookies / cURL | Base, Overage |
| OpenRouter | API Key | Credits |
| JetBrains AI | Local Config | Usage |
| Alibaba | Cookies | Usage |
| NanoGPT | API Key | Credits |
| Infini | API Key | Session, Weekly, Quota |
| Perplexity | Cookies | Credits, Plan |
| Abacus AI | Cookies | Credits |
| Mistral | Cookies | Billing, Usage |
| OpenCode Go | Cookies | Usage, Zen Balance |
| Kilo | API Key / CLI | Usage |
| Codebuff | API Key / Local Config | Credits, Weekly |
| DeepSeek | API Key | Balance |
| Windsurf | Local Cache | Daily, Weekly |
| Manus | Cookies | Credits, Refresh Credits |
| Xiaomi MiMo | Cookies | Balance, Token Plan |
| Doubao | API Key | Request Limits |
| Command Code | Cookies | Monthly Credits, Purchased Credits |
| Crof | API Key | Credits, Request Quota |
| StepFun | Oasis Token | 5h, Weekly |
| Venice | API Key | USD / DIEM Balance |
| OpenAI | Admin API / API Key | Usage, Requests, Credit Balance |
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
