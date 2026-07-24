# ADR 0002: CI budget modes

Date: 2026-07-24
Status: Accepted

## Context

Win-CodexBar and its sister repo (`linear-cli`) share Blacksmith CI minute
budget. We need a single, coarse control to keep spend within the 60%
intent share and to stop runaway spend fast. Per-workflow toggles are too
fine-grained for an emergency stop.

## Decision

Introduce a repository variable `CI_BUDGET_MODE` with three modes:
`normal`, `thin`, and `off`. Unset is treated as `normal`.

- **PR check** (`pr-check.yml`): runs when mode is unset, `normal`, or `thin`;
  skips only when `off`.
  Gate: `if: vars.CI_BUDGET_MODE != 'off'`
- **Interaction guard** (`interaction-guard.yml`): same gate. Keeps its
  existing `blacksmith-2vcpu-ubuntu-2404` runner.
- **Release**: unaffected — release stays local regardless of mode.

`thin` is a no-op for Win-CodexBar's PR check because it is already a single
job (no cross-OS matrix to trim). The mode exists for parity with the sister
repo and as a future dial if a second job is added.

## Consequences

- One variable controls both workflows; `off` is an immediate, full stop.
- Operators set `CI_BUDGET_MODE` in repo Settings → Variables, not in code.
- The glossary in `CONTEXT.md` is the shared definition across both repos.
