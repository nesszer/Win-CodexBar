# Contributing

Thanks for helping improve Win-CodexBar. This repo is the Windows/Tauri port, so prefer the active Tauri and Rust codepaths over historical upstream macOS material.

## Active project layout

- `apps\desktop-tauri\` is the default desktop app.
- `apps\desktop-tauri\src\` contains the React frontend.
- `apps\desktop-tauri\src-tauri\src\` contains the Tauri shell, tray bridge, commands, and desktop integration.
- `rust\src\` contains shared backend/domain logic and the standalone `codexbar` CLI.
- `rust\src\providers\` contains provider-specific fetch, auth, and parsing logic.
- `docs\` may include upstream or historical macOS notes. Do not treat those as authoritative for Windows/Tauri work unless the issue is explicitly about upstream parity.

## Before filing an issue

Search existing open and closed issues first. When filing a bug or feature request, use the GitHub issue template and include the affected surfaces, such as tray panel, Settings UI, config file, CLI, installer, or provider-specific behavior.

Do not paste secrets, API keys, cookies, OAuth tokens, private account details, or raw credential files into issues.

## Before opening a PR

Keep PRs focused on one behavior change or fix. Link the issue when one exists, and explain what changed in user-facing terms.

For UI, tray, Settings, or visual behavior changes, use CUA Driver for visual proof. If CUA Driver cannot be used, explain why in the PR and include equivalent manual proof. For provider/parser changes, include deterministic samples or tests where practical.

Before submitting a PR, run a thermo-nuclear code quality review against the final diff and address any structural maintainability issues it finds: https://github.com/cursor/plugins/blob/main/cursor-team-kit/skills/thermo-nuclear-code-quality-review/SKILL.md

## Build and test

Run checks that match the code you changed and list the exact commands in the PR.

Common commands from the repo root:

```powershell
cargo test --manifest-path rust\Cargo.toml
cargo test --manifest-path apps\desktop-tauri\src-tauri\Cargo.toml
cargo fmt --all
cargo clippy --manifest-path rust\Cargo.toml --all-targets -- -D warnings
cargo clippy --manifest-path apps\desktop-tauri\src-tauri\Cargo.toml --all-targets -- -D warnings
pnpm --dir apps\desktop-tauri test
pnpm --dir apps\desktop-tauri run build
```

Build the desktop shell through Tauri:

```powershell
pnpm --dir apps\desktop-tauri tauri:build
```

Raw `cargo build --release` for the Tauri crate is not the preferred desktop build because it can still point at the dev URL.

## Coding expectations

- Keep provider-specific logic inside the provider module when possible.
- Route new provider construction through `codexbar::core::instantiate_provider`.
- Keep settings and ordering behavior in shared Rust when it is cross-surface state.
- Use existing helpers instead of adding near-duplicate logic.
- Avoid logging or storing raw secrets.
- Add focused tests near the changed code.
- Keep UI behavior accessible; do not rely only on brittle drag/drop when a button or keyboard path is needed.
- For UI/tray flows, use CUA Driver proof instead of eyeballing only, especially when interaction reliability matters.

## Release and packaging changes

Installer, Winget, and release workflow changes need extra care. Verify release URLs, installer hashes, and packaging behavior before opening a PR that changes release artifacts or manifests.
