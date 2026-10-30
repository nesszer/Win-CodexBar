# Win-CodexBar

[English](./README.md) | [简体中文](./README.zh-CN.md) | [繁體中文（臺灣）](./README.zh-TW.md) | [日本語](./README.ja-JP.md) | [한국어](./README.ko-KR.md) | [Español mexicano](./README.es-MX.md) | [Português (Brasil)](./README.pt-BR.md) | [Türkçe](./README.tr-TR.md)

Win-CodexBar is a Windows tray app that shows how much of your AI coding-tool quota you have used, without opening each provider's dashboard. It is a Windows port of [CodexBar](https://github.com/steipete/CodexBar) for macOS. The app is a Tauri 2 and React shell on top of shared Rust provider code.

<table>
  <tr>
    <td width="36%" align="center">
      <img src="docs/images/tray-panel.png" alt="Win-CodexBar tray panel showing provider usage cards"/>
    </td>
    <td width="64%" align="center">
      <img src="docs/images/settings-providers.png" alt="Win-CodexBar Providers settings page"/>
    </td>
  </tr>
</table>

## Install

Install with the Windows Package Manager:

```powershell
winget install Finesssee.Win-CodexBar
```

Or download a build from [GitHub Releases](https://github.com/nesszer/Win-CodexBar/releases):

- Installer: `CodexBar-<version>-Setup.exe`
- Portable: `CodexBar-<version>-portable.exe`
- CLI only: `CodexBarCLI-v<version>-windows-x64.zip`
- Each file has a `.sha256` checksum next to it.

The installer installs the WebView2 runtime if it is missing. Release builds link the C runtime statically, so they don't need the VC++ redistributable.

Winget packages come from [microsoft/winget-pkgs](https://github.com/microsoft/winget-pkgs/tree/master/manifests/f/Finesssee/Win-CodexBar). Each Winget version pins one release URL and installer hash, so a new version can take a while to appear there.

### Code signing

Releases are not signed yet. SignPath.io is set up in the GitHub Actions release workflow, but signing stays off until SignPath issues the Release certificate 2026 and the `release-signing` policy is valid. Until then, check each download against its `.sha256` file. See [docs/CODE_SIGNING.md](docs/CODE_SIGNING.md).

## First run

1. Start **CodexBar** from the Start menu, or run the portable executable.
2. Click the tray icon to open the tray panel.
3. Open **Settings > Providers** from the tray menu.
4. Turn on the providers you use. A new install turns on Claude and Codex.
5. Add the credential each provider needs: an OAuth or device login, an API key, browser cookies, a local CLI login or a token account.

For Claude, browser cookies (the `sessionKey`) are preferred, because they match the usage on Claude's settings page. OAuth and the CLI are fallbacks. For CLI-based providers such as Codex and Gemini, sign in with that provider's CLI first.

## What the app does

- **Tray panel.** Left-click the tray icon, or press `Ctrl+Shift+U`, to open a light panel in the Mac style. It shows one card per provider. Each card has a metric row per usage window, with a bar, the reset time and, when the provider supports it, a pace line such as "10% in deficit · Runs out in 9d 8h".
- **Tray icon.** The icon shows bars or a percentage for one provider, or two providers stacked. With **Color Pace Indicator** on, it turns green when you are behind pace and red when you are ahead.
- **Float bar.** An optional always-on-top strip with the same usage.
- **Providers.** More than 100 providers, including Codex, Claude, Copilot, OpenRouter, Cursor, Gemini, DeepSeek, MiniMax, Kiro, Antigravity, Groq, Qoder and Sakana AI. Each provider has its own source, credentials, cookie import, token accounts, API keys and region settings.
- **Settings.** A window with a sidebar: General, Usage & Spend, Notifications, Menu Bar, Menu, Advanced, Hooks and About, then the Providers section with one page per provider. Each provider row has a status dot.
- **Provider status pages.** With **Check provider status** on (the default), the app polls the status pages of providers such as OpenAI, Claude, Copilot, Cursor and Gemini. An incident colors that provider's dot in Settings and adds a line to the tray panel and the tray tooltip.
- **Per-provider quota warnings.** Each provider can use the global warning thresholds, its own thresholds, or none.
- **Usage & Spend.** Local cost estimates from Codex and Claude logs and from provider APIs. Turn off **Track costs** to hide them.
- **Hooks.** Run a local program when a quota runs low, runs out or resets, or when a provider fails or recovers.
- **Hide personal information.** Removes email addresses and replaces account names with "Account 1", "Account 2" and so on.
- **Portable preferences.** Export your display and behavior choices to a JSON file and import them on another machine. Secrets are never exported.
- **CLI.** A console tool for scripts: usage, cost, config, diagnostics and a loopback dashboard server.
- **Languages.** The app follows the Windows display language by default.

[docs/CONFIGURATION.md](docs/CONFIGURATION.md) describes every setting.

## Supported providers

<details>
<summary>Provider matrix (a selection; the app supports more)</summary>

| Provider | Auth | Tracks |
|---|---|---|
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
| v0 | API Key | Billing quota, API rate limits, on-demand balance |
| Vercel AI Gateway | API Key (+ optional access token) | Team credit balance, lifetime spend; opt-in personal budget |
| Augment | Cookies | Credits |
| OpenCode | Local Config | Usage |
| Kimi | Cookies | 5h Rate, Weekly |
| Kimi K2 (deprecated) | API Key | Credits |
| Amp | Cookies | Usage |
| Warp | Local Config | Usage |
| Ollama | Cookies / API Key | Usage, Cloud Models, Pace windows |
| Azure OpenAI | API Key | Deployment |
| T3 Chat | Cookies / cURL | Base, Overage |
| TinyApi | Browser cookies / manual Cookie header | Available credits |
| TypeSafe | Browser cookies / manual Cookie header | Billing-cycle spend, balance, expiring credits |
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
| Aerostack | API Key (Account JWT) | Monthly AI tokens used/allowance |
| Cosmic AI | API Key | Input tokens used/allowance, Output tokens used/allowance |
| StepFun | Oasis Token | 5h, Weekly, Token refresh |
| Venice | API Key / Web session | USD / DIEM Balance, Bundled credits (web session expires in about 60 s) |
| OpenAI | Admin API / API Key | Usage, Requests, Project-scoped cost, Credit Balance |
| Grok | Cookies / auth.json | Billing |
| Helmcode (also NaN Builders) | Browser cookies / manual Cookie header | Per-model token quotas, reset windows, Helmcode prepaid balance |
| Raycast | Cookies (browser auto or manual) | Monthly AI credits, plan, renewal |
| Replicate | Cookies / token accounts | Monthly spend, credit balance |
| Aixy | API Key / token accounts | Applicable budget balances, 7-day key usage |
| ClawRouter | API Key | Monthly budget, requests, tokens, routed-provider spend |
| ElevenLabs | API Key | Subscription Credits, Voice Slots |
| Deepgram | API Key | Project Usage |
| Groq | API Key | Enterprise Metrics |
| LLM Proxy | API Key | Quota Stats |
| llmman | Local daemon / optional API Key | Memory in use, loaded and stored models |
| DevPass | API Key | Plan credits, Premium weekly, API-key spend |
| IBM Bob | API Key / token accounts | Monthly Bobcoins, per-team budgets |
| xKiro | API Key | Daily free tokens |
| Synthetic | API Key | Five-hour quota, Weekly tokens, Hourly search, Plan |
| Sail Research | API Key | Organization credit balance, spend for the reported range (up to 30 days) |
| Linkup | API Key | Account credit balance |
| Sofya | API Key | Available, plan and purchased credits, Monthly reset (free tier) |
| Tavily | API Key | Account plan usage, API key usage, Pay-as-you-go credits |
| Exa | API Key + Scope | Selected API key monthly spend (UTC) |
| Langdock | Browser cookies / manual Cookie header | Session (5h), Weekly |
| LithosAI | Browser cookies / manual Cookie header | Prepaid balance (Credits), UTC spend today and this month |
| Muse (muse.ai) | Browser cookies / manual Cookie header | Weekly usage, tokens left, top-up tokens |
| WorkBuddy | Browser cookies / manual Cookie header | Monthly credits left, plan, cycle reset |
| X API | Browser cookies / manual Cookie header | Prepaid balance, purchased and free credits |
| Parallel | Browser cookies / manual Cookie header | Prepaid account balance, pending debit |
| fal | API Key | Account credit balance |
| FLORA | API Key + workspace ID | Workspace available balance, member limit, month-to-date spend |
| Future AGI | API Key + secret key | Billing-period AI credits and gateway requests against free allowances, estimated cost |
| Munito | Admin API Key | Today's and this month's organization spend against spend limits |
| Soniox | Organization API key | Prepaid USD balance, this month's spend against the monthly budget, billing method |
| Claude Platform | Browser cookies / manual Cookie header | Console API credit and purchased credit, other grants, month-to-date spend |
| Trae CN | Browser cookies / manual Cookie header | Credit allowance used against total, remaining credits, plan name |
| Chatbox AI | License key | Remaining plan, reward, top-up and image quotas, plan name, plan reset and expiry |
| abliteration.ai | API Key | Prepaid USD credits: remaining balance, used against total |

</details>

[docs/PROVIDERS.md](docs/PROVIDERS.md) explains how providers fetch data, which windows get pace, and the details of individual providers.

## Supported languages

The app ships these 10 languages:

- English
- 简体中文
- 繁體中文（臺灣）
- 日本語
- 한국어
- Español mexicano
- Português (Brasil)
- Русский
- Türkçe
- Українська

A new install follows the Windows display language. Pick **System** or a language in **Settings > General**.

## Build from source

You need Rust (stable, `x86_64-pc-windows-msvc`), the Visual Studio C++ build tools, Node.js and pnpm. `scripts/dev.ps1` checks that `cargo` and `pnpm.cmd` are on `PATH` (and `dlltool` on GNU hosts) but installs nothing. Clone from the folder that should hold the repo; the remaining commands run from the cloned repo root:

```powershell
git clone https://github.com/nesszer/Win-CodexBar.git
cd Win-CodexBar
pnpm --dir apps/desktop-tauri install --frozen-lockfile
.\scripts\dev.ps1
```

Other `dev.ps1` options:

```powershell
.\scripts\dev.ps1 -Release
.\scripts\dev.ps1 -SkipBuild
```

`-Release` builds an optimized binary. `-SkipBuild` starts the last build again.

[docs/BUILDING.md](docs/BUILDING.md) covers the checks, the CLI build and release builds.

## Use the CLI

The installer puts the console CLI in `codexbar-cli.exe`. `codexbar.exe` is the tray app, and `codexbar-desktop.exe` is a copy kept for older shortcuts and autostart entries. Start menu shortcuts start the tray app.

```powershell
codexbar-cli --help
codexbar-cli diagnose --pretty
codexbar-cli usage -p claude
codexbar-cli usage -p all
codexbar-cli cost -p codex
```

A source build names the CLI `codexbar.exe`. See [docs/CLI.md](docs/CLI.md).

## Release builds

To build a release locally on Windows, use the cached release builder:

```powershell
.\scripts\windows-release-build.ps1 -Ref v0.70.0 -SmokeInstall
```

It builds the Tauri release binary and the console CLI, checks the installer dependencies, packages the installer with Inno Setup, writes the installer and portable files with SHA-256 checksums, and with `-SmokeInstall` runs a silent install and uninstall test. See [docs/BUILDING.md](docs/BUILDING.md#build-a-release-on-windows) and [docs/release/ci-cd.md](docs/release/ci-cd.md).

## Privacy

- **Local by default.** The app reads provider data from known local paths or from provider APIs you set up.
- **Cookies are opt-in.** The app reads browser cookies only for providers you turn on.
- **Secrets are protected.** API keys, pasted cookies, token accounts and `settings.json` are stored through the secure-file layer, which uses DPAPI for the current Windows user.
- **Diagnostics are safe to share.** They contain provider, source and status metadata only. They never contain cookies, API keys, bearer tokens or OAuth values.
- **Updates are verified.** An update download needs a SHA-256 digest from GitHub and is checked again right before it is applied.

See [docs/PRIVACY.md](docs/PRIVACY.md).

## Docs

| Topic | Link |
|---|---|
| Building from source | [docs/BUILDING.md](docs/BUILDING.md) |
| Settings and files | [docs/CONFIGURATION.md](docs/CONFIGURATION.md) |
| Providers, windows and pace | [docs/PROVIDERS.md](docs/PROVIDERS.md) |
| The CLI | [docs/CLI.md](docs/CLI.md) |
| Browser cookies | [docs/COOKIES.md](docs/COOKIES.md) |
| How the app is built | [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) |
| Proving UI changes | [docs/WINDOWS_PROOF.md](docs/WINDOWS_PROOF.md) |
| WSL setup and sign-in tips | [docs/WSL.md](docs/WSL.md) |
| Contributing | [CONTRIBUTING.md](CONTRIBUTING.md) |

See [CHANGELOG.md](CHANGELOG.md) for the release history. The current release is 0.70.0.

## Local integrations

- [AI Usage Limits](https://github.com/lenadweb/stream-deck-ai-limits): an Elgato Stream Deck plugin that reads the local `codexbar serve` dashboard and API to show provider, account, quota or payload metrics.
- [AI Monitor](https://github.com/tobymarks/esp32-ai-monitor): an ESP32 desk display (Cheap Yellow Display) with a Windows companion app (beta). The app calls the installed CLI (`codexbar-cli usage -p <provider> --json`) and sends Claude, Codex, Copilot, Cursor, Gemini or Antigravity limits to the display over USB serial.

## Credits

- Original macOS app: [steipete/CodexBar](https://github.com/steipete/CodexBar) by Peter Steinberger
- Cost tracking inspired by [ccusage](https://github.com/ryoppippi/ccusage)

## License

MIT, same as the original CodexBar.
