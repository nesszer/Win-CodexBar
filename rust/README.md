# CodexBar for Windows

A Windows port of [CodexBar](https://github.com/steipete/CodexBar) - a system tray application for monitoring AI provider usage limits.

![CodexBar Windows](screenshots/tray-icon.png)

## Features

- **System Tray Icon** - Color-coded usage indicator with incident badges
- **Multiple Providers** - Support for 40 AI providers:
  - Codex, Claude, Cursor, Factory, Gemini, Copilot, Antigravity, z.ai
  - MiniMax, Kiro, Vertex AI, Augment, OpenCode, OpenCode Go, Kimi, Kimi K2
  - Amp, Warp, Ollama, OpenRouter, JetBrains AI, Alibaba
  - NanoGPT, Infini, Perplexity, Abacus AI, Mistral, Kilo, AWS Bedrock, Codebuff, DeepSeek, Windsurf
  - Manus, Xiaomi MiMo, Doubao, Command Code, Crof, StepFun, Venice, OpenAI API
- **Usage Notifications** - Windows toast alerts when usage hits thresholds
- **Settings Panel** - Enable/disable providers, configure refresh intervals
- **Manual Cookie Input** - Fallback for when automatic cookie extraction fails
- **Status Page Polling** - Shows provider incidents with visual badges
- **CLI Tool** - Command-line interface for scripts and automation

## Installation

### From Release

Download the latest release from the [Releases](https://github.com/Finesssee/Win-CodexBar/releases) page.

- Recommended: `CodexBar-<version>-Setup.exe`
  - This installer now installs the required Microsoft Visual C++ runtime and Microsoft Edge WebView2 Runtime on clean Windows machines before launching CodexBar.
- Portable: `CodexBar-<version>-portable.exe`
  - Run it directly without installing.
  - Release builds statically link the WebView2 loader, so no `WebView2Loader.dll` sidecar is required.
  - Best for machines that already have the Microsoft Visual C++ runtime and Microsoft Edge WebView2 Runtime installed.
- Linux CLI: `codexbar-v<version>-linux-x86_64.tar.gz` or `codexbar-v<version>-linux-aarch64.tar.gz`
  - Includes the standalone `codexbar` CLI binary plus README and license files.

### Build from Source

Requirements:
- Rust 1.70+ (install from [rustup.rs](https://rustup.rs))
- Windows 10/11

```powershell
# Clone the repository
git clone https://github.com/Finesssee/Win-CodexBar.git
cd Win-CodexBar/rust

# Build release version
cargo build --release

# Run the CLI
./target/release/codexbar.exe --help

# Build the Tauri desktop shell
cd ../apps/desktop-tauri
npm install
npm run tauri:build
```

## Usage

### GUI Mode (System Tray)

```powershell
codexbar.exe
```

This launches the system tray application:
- Click the tray icon to show the usage panel
- Use Settings to pick English or Chinese in `General`
- Use `General -> Updates` to choose the release channel, background auto-download behavior, and whether a ready installer should run automatically when you quit CodexBar
- Use the Providers tab to re-enable providers and review auth/account state
- Use the Cookies, API Keys, or provider account sections to recover credentials if status stops updating
- Use the About button for version info

On Windows Remote Desktop sessions, CodexBar now exits with a direct error dialog instead of crashing with renderer errors. Use the local Windows desktop session for the native UI, or run installed CLI commands like `codexbar-cli usage -p claude` while connected over RDP.

### CLI Mode

```powershell
# Show usage for all enabled providers
codexbar

# Show usage for specific provider
codexbar -p claude

# Show usage for all providers
codexbar -p all

# Output as JSON
codexbar --json --pretty

# Include provider status
codexbar --status

# Show local cost usage (Claude + Codex logs)
codexbar cost

# Enable/disable auto-start on Windows boot
codexbar autostart enable
codexbar autostart disable
codexbar autostart status
```

### Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | Unexpected failure |
| 2 | Provider not installed |
| 3 | Parse error |
| 4 | Timeout |

## Configuration

Settings are stored in `%APPDATA%\CodexBar\settings.json`:

```json
{
  "enabled_providers": ["claude", "codex"],
  "refresh_interval_secs": 300,
  "show_notifications": true,
  "high_usage_threshold": 70.0,
  "critical_usage_threshold": 90.0
}
```

Manual cookies, API keys, token accounts, and settings are stored under `%APPDATA%\CodexBar`.
On Windows, new saves are wrapped with DPAPI protection. Existing plaintext files remain readable
and are rewritten protected on the next save.

## Provider Authentication

Each provider has different authentication methods:

| Provider | Auth Method |
|----------|-------------|
| Claude | Browser cookies (Chrome/Edge), OAuth, session env vars |
| Codex | Local CLI, Browser cookies |
| Cursor | Browser cookies |
| Gemini | gcloud CLI credentials |
| Copilot | GitHub device flow, gh CLI auth, legacy token fallback |
| Antigravity | Local language server |
| Windsurf | Local cache |
| Zai | Local config |
| Kiro | AWS credentials |
| Vertex AI | gcloud OAuth |
| Augment | VS Code extension |
| MiniMax | API key |
| OpenCode / OpenCode Go | Browser cookies |
| Kimi / Kimi K2 | Browser cookies / API cookies |
| Amp | Sourcegraph token |
| Warp | API key |
| Ollama | Browser cookies |
| OpenRouter | API key |
| JetBrains AI | Local IDE config |
| Alibaba | API key / browser cookies |
| NanoGPT | API key |
| Infini | API key |
| Perplexity | Browser cookies |
| Abacus AI | Browser cookies |
| Mistral | Browser cookies |
| Kilo | API key / local CLI config |
| Codebuff | API key / local Manicode config |
| DeepSeek | API key |

### Claude session env vars

When browser cookie extraction is unreliable, Claude can also read a session key from the environment:

```powershell
$env:CLAUDE_AI_SESSION_KEY = "sk-ant-..."
codexbar -p claude
```

`CLAUDE_AI_SESSION_KEY` and `CLAUDE_WEB_SESSION_KEY` both work. Each accepts either the raw token or the full cookie-style value, for example `sessionKey=sk-ant-...`.

## Screenshots

### System Tray
![Tray Icon](screenshots/tray-icon.png)

The tray icon color indicates overall usage:
- Green: 0-50% used
- Yellow: 50-80% used
- Orange: 80-95% used
- Red: 95-100% used
- Gray: Unknown/loading

A badge appears in the corner for status issues:
- Yellow badge: Degraded performance
- Red badge: Major outage

### Main Panel
![Main Panel](screenshots/main-panel.png)

### Settings
![Settings](screenshots/settings.png)

### About
![About](screenshots/about.png)

## Development

### Running Tests

```powershell
cargo test
```

Plain `cargo test` now follows the host target by default:
- Windows hosts build and test the native Windows target
- Linux hosts build and test the native Linux target

If you explicitly want a cross-target pass, provide the target yourself:

```bash
cargo test --target x86_64-unknown-linux-gnu
```

### Project Structure

```
rust/
├── src/
│   ├── main.rs           # Entry point
│   ├── cli/              # CLI commands
│   ├── core/             # Core data models
│   ├── providers/        # Provider implementations
│   ├── browser/          # Cookie extraction
│   ├── tauri_app/        # Tauri GUI
│   ├── tray/             # Tray icon types
│   ├── settings.rs       # Settings management
│   ├── status.rs         # Status page polling
│   └── notifications.rs  # Windows notifications
├── ui/                   # HTML/CSS/JS for GUI
│   ├── index.html        # Main panel
│   ├── settings.html     # Settings page
│   ├── about.html        # About dialog
│   └── cookies.html      # Cookie input
└── Cargo.toml
```

## Credits

This is a Windows port of [CodexBar](https://github.com/steipete/CodexBar) by [Peter Steinberger](https://twitter.com/steipete).

## License

MIT
