# Win-CodexBar — Windows & WSL Port of CodexBar

A Windows (and WSL) port of [CodexBar](https://github.com/steipete/CodexBar) — the tiny menu bar app that keeps your AI provider usage limits visible.

> **This is an unofficial port.** The original CodexBar is a macOS Swift app by [Peter Steinberger](https://github.com/steipete). This port is built with Rust + egui for native Windows and WSL support.

## Features

- **15 AI Providers**: Codex, Claude, Cursor, Gemini, Copilot, Antigravity, Windsurf, Zai, Kiro, Vertex AI, Augment, MiniMax, OpenCode, Kimi, Kimi K2
- **System Tray Icon**: Dynamic two-bar meter showing session + weekly usage
- **Native Windows UI**: Built with egui - no web runtime required
- **Browser Cookie Extraction**: Automatic extraction from Chrome, Edge, Brave, Firefox (DPAPI encrypted)
- **CLI Commands**: `codexbar usage` and `codexbar cost` for scripting
- **Preferences Window**: Enable/disable providers, set refresh intervals, manage cookies

## Screenshots

### Main Window
![CodexBar Main Window](docs/images/main-window.png)

### Settings
![CodexBar Settings](docs/images/settings.png)

### Overview
![CodexBar Overview](docs/images/overview.png)

## Getting Started

### Quick Start — Windows

```powershell
# Clone and run — prerequisites are installed automatically
git clone https://github.com/Finesssee/Win-CodexBar.git
cd Win-CodexBar
.\dev.ps1
```

This will:
1. Check for Rust and MinGW-w64, install them if missing
2. Build CodexBar in debug mode
3. Launch the system tray app

Other options:
```powershell
.\dev.ps1 -Release         # optimised build
.\dev.ps1 -Verbose         # debug logging
.\dev.ps1 -SkipBuild       # run last build without rebuilding
```

### Quick Start — WSL (Ubuntu)

CodexBar runs natively inside WSL. The CLI works out of the box; the GUI
requires [WSLg](https://github.com/microsoft/wslg) (Windows 11, build 22000+).

```bash
# Clone and build
git clone https://github.com/Finesssee/Win-CodexBar.git
cd Win-CodexBar
./dev.sh
```

This will:
1. Detect your WSL environment
2. Build CodexBar as a native Linux binary
3. Launch the GUI (WSLg) or CLI (no display server detected)

CLI-only mode (no display server needed):
```bash
./dev.sh --cli              # codexbar usage -p all
./dev.sh --release          # optimised build
```

#### How WSL Support Works

When running inside WSL, CodexBar:

- **Browser cookies**: Reads Windows browser data from `/mnt/c/Users/<you>/AppData/...`.
  Chromium cookies encrypted with DPAPI cannot be decrypted from WSL automatically.
  Use manual cookies (Settings → Cookies) or CLI-based provider authentication instead.
- **Provider CLIs**: Works with `codex`, `claude`, `gemini` etc. installed inside WSL natively.
- **GUI**: Requires WSLg (Windows 11) or an X server. Falls back to CLI mode automatically.
- **Notifications**: Uses `notify-send` in WSL, or Windows toast via `powershell.exe` as fallback.

#### WSL Authentication Tips

| Provider | WSL Auth Strategy |
|----------|-------------------|
| Codex | `npm i -g @openai/codex` inside WSL, then `codex login` |
| Claude | `npm i -g @anthropic-ai/claude-code` inside WSL, then `claude login` |
| Gemini | `npm i -g @anthropic-ai/claude-code` inside WSL |
| Cursor / Kimi | Manual cookies — copy from browser DevTools (F12 → Network → Cookie header) |
| Copilot | GitHub Device Flow works natively in WSL |

### Download

Download the latest release from [GitHub Releases](https://github.com/Finesssee/Win-CodexBar/releases).

### Manual Build

Prerequisites: Rust 1.70+ with `x86_64-pc-windows-gnu` target, MinGW-w64.
Install them automatically with:

```powershell
.\scripts\setup-windows.ps1
```

Then build:
```powershell
cd rust
cargo build --release
# Binary at: target/release/codexbar.exe
```

## Usage

### GUI (System Tray)
```powershell
codexbar menubar
```

### CLI
```bash
# Check usage for a provider
codexbar usage -p claude
codexbar usage -p codex
codexbar usage -p all

# Check local cost usage (from JSONL logs)
codexbar cost -p codex
codexbar cost -p claude
```

## Providers

| Provider | Auth Method | What's Tracked |
|----------|-------------|----------------|
| Codex | OAuth / CLI | Session, Weekly, Credits |
| Claude | OAuth / Cookies / CLI | Session (5h), Weekly |
| Cursor | Browser Cookies | Plan, Usage, Billing |
| Gemini | OAuth (gcloud) | Quota |
| Copilot | GitHub Device Flow | Usage |
| Antigravity | Local Language Server | Usage |
| Windsurf | Local Config | Usage |
| Zai | API Token | Quota |
| Kiro | CLI | Monthly Credits |
| Vertex AI | gcloud OAuth | Cost Tracking |
| Augment | Browser Cookies | Credits |
| MiniMax | API | Usage |
| OpenCode | Local Config | Usage |
| Kimi | Browser Cookies | 5-Hour Rate, Weekly |
| Kimi K2 | API Key | Credits |

## First Run

1. Run `codexbar menubar` to start the app
2. Click **Settings** in the menu
3. Go to **Providers** tab and enable the providers you use
4. Make sure you're logged into the provider CLIs (e.g., `codex`, `claude`, `gemini`)

## Browser Cookie Extraction

Win-CodexBar automatically extracts cookies from:
- **Chrome** (DPAPI + AES-256-GCM)
- **Edge** (DPAPI + AES-256-GCM)
- **Brave** (DPAPI + AES-256-GCM)
- **Firefox** (unencrypted SQLite)

For providers that need web authentication (Claude, Cursor, Kimi), cookies are extracted automatically when you're logged into the web interface.

> **WSL note**: Chromium cookies are encrypted with Windows DPAPI, which is not accessible
> from WSL. Automatic extraction from Chrome/Edge/Brave only works when running CodexBar
> natively on Windows. In WSL, use manual cookies or CLI-based provider authentication.

### Manual Cookies

If automatic extraction fails, you can add cookies manually:
1. Go to **Settings** → **Cookies** tab
2. Select the provider
3. Paste the cookie header from browser DevTools (F12 → Network → Request Headers → Cookie)

## Differences from macOS Version

| Feature | macOS | Windows | WSL |
|---------|-------|---------|-----|
| UI Framework | SwiftUI | egui (Rust) | egui (via WSLg) |
| System Tray | NSStatusItem | tray-icon crate | tray-icon (WSLg) |
| Cookie Decryption | Keychain | DPAPI | Manual cookies |
| Widget | WidgetKit | Not available | Not available |
| Auto-update | Sparkle | Manual | Manual |
| Notifications | macOS native | PowerShell toast | notify-send |

## Privacy

- **No disk scanning**: Only reads known config locations and browser cookies
- **On-device only**: No data sent to external servers (except provider APIs)
- **Cookies are opt-in**: Browser cookie extraction only happens for enabled providers

## Credits

- **Original CodexBar**: [steipete/CodexBar](https://github.com/steipete/CodexBar) by Peter Steinberger (MIT)
- **Inspired by**: [ccusage](https://github.com/ryoppippi/ccusage) for cost tracking

## License

MIT - Same as original CodexBar

---

*This is an unofficial Windows port. For the official macOS version, visit [steipete/CodexBar](https://github.com/steipete/CodexBar).*
