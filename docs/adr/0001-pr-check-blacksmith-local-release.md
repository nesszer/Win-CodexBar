# ADR 0001: PR check on Blacksmith Windows, release stays local

Date: 2026-07-24
Status: Superseded. ADR 0005 moved the hosted PR check to CircleCI Windows. ADR 0004 moved releases to GitHub Actions.

## Context

Win-CodexBar had no hosted CI. Contributors ran `scripts/local-check.ps1`
before opening a PR. Releases were built with `scripts/windows-release-build.ps1`
on a local Windows server. We wanted hosted feedback on PRs without paying for
release-grade CI.

## Decision

Add one PR check workflow, `.github/workflows/pr-check.yml`, that runs on
**Blacksmith Windows** (`blacksmith-4vcpu-windows-2025`). It runs only the
local-check slice:

- `cargo fmt --all --check` (workspace)
- `cargo clippy --all-targets -- -D warnings` on both `rust` and
  `apps/desktop-tauri/src-tauri`
- `cargo test` on both crates
- `pnpm test` and `pnpm run build` (which runs `tsc --noEmit`) in
  `apps/desktop-tauri`

Releases stay **local**. There is no release workflow on Blacksmith. The
operator builds the release with `tauri:build`, packages the installer, runs
the smoke install and uploads to GitHub Releases with
`scripts/windows-release-build.ps1` on their own Windows server, as
`docs/BUILDING.md` describes.

## Consequences

- PRs get hosted fmt, clippy and test results on the real Windows target.
- Releases cost no CI time. A release is a deliberate step that the operator runs.
- The Blacksmith Windows runner has VS Build Tools, so the Rust and Tauri
  toolchain works without extra setup.

## Later changes

Added 2026-10-12. The decision above is kept as history.

- ADR 0005 (2026-08-30) moved the automatic hosted PR check to CircleCI
  Windows. `pr-check.yml` still exists, but it now runs only by manual
  dispatch, as a Blacksmith reserve.
- The local release path no longer applies. ADR 0004 records that
  `.github/workflows/release.yml` builds, signs and drafts releases on
  GitHub-hosted Windows when a `vX.Y.Z` tag is pushed.
- The check slice has grown. The current list of steps is in `CONTEXT.md`
  under "Local check slice".
