# ADR 0005: Hosted PR check moves to CircleCI Windows

Date: 2026-08-30
Status: Accepted. Supersedes the hosted-runner decision of ADR 0001 and the
Blacksmith gate description in ADR 0002.

## Context

The shared Blacksmith free-tier minute pool is used up. The Blacksmith GitHub
Actions runners behind ADR 0001's PR check (`.github/workflows/pr-check.yml`)
can no longer run scheduled PR checks reliably.

CircleCI is already this repo's hosted Windows platform for releases (ADR 0004
as it stood on this date), with a working `circleci/windows@5.0`
configuration. The repository is public, so its PR builds use CircleCI's Free
Plan open-source allowance, not the personal credit block.

## Decision

Move the hosted PR and push check to CircleCI, as a new `pr-check` job in
`.circleci/config.yml` (workflow `pr-check`).

### What the job runs

- The job uses the `win/default` executor with `size: medium`.
- The config doesn't repeat the check steps. It calls scripts, and the check
  itself is `scripts/local-check.ps1 -Slice ci`. This opt-in slice runs the
  same steps as `.github/workflows/pr-check.yml`: `cargo fmt --all --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace`, `pnpm install --frozen-lockfile`, `pnpm test`,
  `pnpm run build`, and the interaction-guard script tests. Running the script
  with no parameters still does what it did before.
- `scripts/circleci-pr-gates.ps1` owns the three skip decisions that used to
  be inline in the config:
  1. the budget mode is `off`;
  2. the pipeline is a branch push other than `main`, `master` or a
     `gh-readonly-queue/` merge-queue push;
  3. the PR changes only docs.

  Each real skip calls `circleci-agent step halt`.
- `scripts/run-circleci-pr-check.ps1` installs the toolchain and verifies each
  download against its official checksum:
  - It downloads `rustup-init.exe` from static.rust-lang.org and checks it
    against the official `.sha256` file next to it.
  - It checks the Node 24.18.0 x64 MSI against the official `SHASUMS256.txt`
    entry before `msiexec` runs. The MSI installs into its own per-version
    `INSTALLDIR`, because an upgrade of the same product silently does nothing.
  - corepack activates pnpm from the exact `packageManager` pin in
    `apps/desktop-tauri/package.json`.
- `scripts/circleci-pr-common.ps1` holds the pure checksum and gate logic.
  `scripts/circleci-pr.tests.ps1` tests it offline.
- `scripts/circleci-pinned-rust.txt` holds the pinned Rust version. The Cargo
  target cache keys include `{{ checksum "scripts/circleci-pinned-rust.txt" }}`,
  so bumping the Rust pin invalidates the target cache. No pipeline parameter
  or config edit is needed.
- The config uses two compile-time PR values from CircleCI's GitHub App
  integration: `pipeline.event.context.github.pr_url` and
  `pipeline.event.github.pull_request.base.sha`. They are set only on pull
  request events and are empty on push pipelines. Because they come from the
  GitHub App, CircleCI never builds fork PRs. External contributors' changes
  need a manual fallback on a same-repo branch.

### Triggers and skips

- The workflow filter stays wide (`branches: only: /.*/`), because CircleCI
  delivers same-repo PR builds as branch pipelines. Early gates inside the job
  give the same result as the GitHub workflow: PRs plus pushes to `main` and
  `master`, skipping `docs/**`, `**/*.md`, `CONTEXT.md` and `.github/CI.md`.
- Each real skip logs its reason and calls `circleci-agent step halt`. This
  stops the whole job before the cache restore and the provision-and-run step,
  so a skip spends no cache or toolchain minutes. A non-zero exit from
  `circleci-agent` throws, so a failed halt can't pass silently.
- The docs-only check diffs the base commit against the head with a two-dot
  tree diff. This works when the base is fetched at depth 1.
  - The base SHA comes from `pipeline.event.github.pull_request.base.sha`.
  - If that value is empty but the pipeline is still tied to a PR, the gate
    looks up the base SHA in the public GitHub pulls API, using
    `CIRCLE_PROJECT_USERNAME` and `CIRCLE_PROJECT_REPONAME`, and fetches it
    with `--depth=1`.
  - Only PR pipelines get the docs-only check. Every `main` or `master` push
    and every `gh-readonly-queue/` merge-queue push runs the full check.
  - If resolving, fetching or diffing fails on a PR pipeline, the gate fails
    open. It ends only the gate step, and the job runs the checks. The gate
    never skips on an unknown base.
- The job reads `CI_BUDGET_MODE` as a CircleCI project environment variable
  (unset or empty means `normal`) and halts with `circleci-agent step halt`
  when it is `off`.

### The GitHub workflows

`.github/workflows/pr-check.yml` stays as a manual fallback. Its `on:` block
now holds only `workflow_dispatch`, with the push and `pull_request` triggers
removed. It no longer runs on its own, and a maintainer can run it by hand for
Blacksmith diagnostics. The job body is unchanged. The interaction guard
(`.github/workflows/interaction-guard.yml`) stays a GitHub Actions workflow
and doesn't change.

## Consequences

- Auto-cancel of superseded pushes is a CircleCI **project setting**
  ("Auto-cancel redundant workflows", Project Settings > Advanced). It is keyed
  by branch, like the GitHub workflow's `concurrency` group was. The `pr-check`
  job has no `concurrency` YAML.
- Hosted PR results still come from the real Windows target, with no
  dependency on Blacksmith.
- `CI_BUDGET_MODE` must now be set in two places to control both services:
  CircleCI project environment variables (read by the `pr-check` gate) and
  GitHub Actions repository variables (read by `vars.CI_BUDGET_MODE`). The
  glossary in `CONTEXT.md` documents both.
- CircleCI Windows credits become the recurring PR cost, charged to the
  open-source allowance. Organization credit alerts should cover the PR check
  as well as releases.
- ADRs 0001 and 0002 stay as unchanged history of the Blacksmith period. This
  ADR supersedes them only on where the hosted PR check runs.

## Later changes

Added 2026-10-12. The decision above is kept as history.

- Releases left CircleCI. ADR 0004 was rewritten on 2026-09-16: GitHub Actions
  (`.github/workflows/release.yml`) now builds, signs and drafts releases.
  CircleCI ignores release tags, so its credits cover only the PR check.
- The CircleCI workflow no longer uses a `branches: only: /.*/` filter. It has
  a `when:` condition that runs on pushes and on PRs, except PRs whose base
  branch starts with `port/upstream-`. The job ignores `vX.Y.Z` tags.
- The docs-only check matches each changed path against
  `^(docs/.*|.*\.md)$`, which also covers `CONTEXT.md` and `.github/CI.md`.
- There is no same-repo branch fallback for fork PRs. Maintainers validate a
  fork PR locally in a disposable, credential-free environment and merge it
  with an admin override. `.github/CI.md` under "Fork pull requests" has the
  steps.
- The `-Slice ci` check now has 11 steps, including oxlint and the anti-slop
  checks. `CONTEXT.md` under "Local check slice" lists them.
