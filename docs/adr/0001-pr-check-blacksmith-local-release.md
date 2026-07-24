# ADR 0001: PR check on Blacksmith Windows, release stays local

Date: 2026-07-24
Status: Accepted

## Context

Win-CodexBar had no hosted CI. Contributors ran `scripts/local-check.ps1`
before PRs, and the canonical release path was `scripts/windows-release-build.ps1`
on a local Windows server. We want hosted feedback on PRs without paying for
release-grade CI.

## Decision

Add a single PR check workflow (`.github/workflows/pr-check.yml`) that runs on
**Blacksmith Windows** (`blacksmith-4vcpu-windows-2025`). It mirrors only the
local-check slice:

- `cargo fmt --all --check` (workspace)
- `cargo clippy --all-targets -- -D warnings` on both `rust` and
  `apps/desktop-tauri/src-tauri`
- `cargo test` on both crates
- `pnpm test` and `pnpm run build` (which runs `tsc --noEmit`) in
  `apps/desktop-tauri`

Release stays **LOCAL**: no release workflow on Blacksmith. `tauri:build`
release, installer packaging, smoke install, and upload to GitHub Releases
continue to use `scripts/windows-release-build.ps1` on the operator's Windows
server, exactly as documented in `docs/BUILDING.md`.

## Consequences

- PRs get hosted fmt/clippy/test feedback on the real Windows target.
- No release CI spend. Release remains a deliberate, operator-driven step.
- The Blacksmith Windows runner has VS Build Tools available, so the Rust +
  Tauri toolchain works without extra provisioning.
