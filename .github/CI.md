# CI — Win-CodexBar

Win-CodexBar separates continuous validation from release signing.

- CircleCI Windows is the primary PR and protected-branch validation path.
  Its pr-check job runs scripts/local-check.ps1 -Slice ci.
- GitHub Actions Windows is the sole canonical tag-release producer. Its
  release workflow builds, signs through SignPath, verifies the signed files,
  and creates a draft GitHub Release.
- Blacksmith Windows remains a manual reserve workflow in
  .github/workflows/pr-check.yml.
- interaction-guard.yml is a lightweight GitHub-hosted policy guard.

## CircleCI validation

.circleci/config.yml contains the pr-check workflow only. Canonical release
tags are ignored by that workflow so CircleCI cannot create a competing
release build or publisher. The validation job has no GitHub release-write
credential.

The hosted PR check delegates to scripts/local-check.ps1 -Slice ci:

~~~powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
pnpm --dir apps/desktop-tauri install --frozen-lockfile
pnpm --dir apps/desktop-tauri run lint
pnpm --dir apps/desktop-tauri run test:anti-slop
pnpm --dir apps/desktop-tauri test
pnpm --dir apps/desktop-tauri run build
node --test .github/scripts/interaction-guard.test.mjs
~~~

CircleCI's GitHub App trigger and auto-cancel settings live outside the
repository. Keep PR, default-branch, and budget rules there; do not add a
second tag trigger.

## Fork pull requests

CircleCI GitHub App pipelines never fire for pull requests from forks. To
build one:

1. Read the fork PR's diff. The build runs the contributor's code on CircleCI
   with this project's environment variables.
2. Add the `ci:run` label. .github/workflows/fork-ci-mirror.yml pushes
   exactly that head commit to ci/pr-<number> in this repository.
3. The same workflow starts a CircleCI pipeline through the trigger API,
   with the config from main and the code from ci/pr-<number>. A plain push
   is not enough: CircleCI ignores a branch push that brings no new commits,
   and the fork head is already in this repository through its pull ref.
   The pipeline's fork_pr parameter lets it through the trigger gate in
   scripts/circleci-pr-common.ps1, and the docs-only gate finds the PR base
   from the number. The check lands on the same commit, so it shows on the
   fork PR.

One-time setup for step 3:

- Actions secret CIRCLECI_TOKEN: a CircleCI personal API token (User
  Settings -> Personal API Tokens) from a user with access to this project.
- Actions variable CIRCLECI_PROJECT_SLUG: Project Settings -> Overview, for
  example circleci/<org-id>/<project-id>.
- Actions variable CIRCLECI_PIPELINE_DEFINITION_ID: Project Settings ->
  Pipelines, the id of the pipeline that runs .circleci/config.yml.

Any new push to the fork PR removes the label; re-add it after reviewing the
new commits. Closing the PR removes the mirror branch. GITHUB_TOKEN cannot
push commits that change .github/workflows/**, so the mirror fails for those
PRs; review them by hand and use the manual Blacksmith workflow if needed.

## Code review

CodeRabbit reviews every PR, drafts included, with the settings in
.coderabbit.yaml. It uses AGENTS.md as its guidelines and runs blocking
pre-merge checks for provider data siloing, secret handling and new
dependencies. It approves a PR once its comments are resolved; maintainers
still merge. It does not build or test anything, and it does not replace UI
proof on a fresh Windows build.

## GitHub Actions release path

.github/workflows/release.yml runs only for canonical vX.Y.Z tag pushes on a
GitHub-hosted Windows runner. It:

1. freezes the tag commit SHA and runs release preflight;
2. requires the SignPath credentials before building;
3. builds the three release artifacts;
4. uploads exactly the installer, portable executable, and CLI ZIP to GitHub
   Actions for SignPath;
5. waits for release-signing to complete;
6. verifies Authenticode on both executables and the CLI inside the ZIP;
7. emits the final six assets and manifest from signed bytes; and
8. creates or updates a draft release with the hash-safe publisher.

A signing failure stops the workflow. It cannot publish unsigned assets. The
manual signpath-test.yml workflow uses test-signing, retains its final bundle
as an Actions artifact, and never publishes a release.

The production workflow requires SIGNPATH_API_TOKEN as its only SignPath
repository secret. The organization ID, project slug, release-signing and
test-signing policy names, and codexbar-installer artifact configuration are
reviewed workflow values. Set the nonsecret Actions variable
SIGNPATH_RELEASE_CERT_THUMBPRINT after the production certificate is issued;
the finalizer rejects any signed file whose thumbprint differs.

GitHub Actions must have actions: read and contents: write for the production
workflow. The test workflow has contents: read and actions: read. SignPath
origin verification also requires the GitHub.com trusted build system to be
linked to the project and the SignPath GitHub App to have repository access.

## Budget and safety boundary

CircleCI remains the recurring Windows validation cost. The tag release path is
in GitHub Actions because SignPath verifies GitHub-hosted build provenance.
There is no parallel CircleCI tag build.

Keep the existing CircleCI budget gates, cache reuse, and auto-cancel settings.
Use the manual Blacksmith workflow only when an independent Windows result is
needed. Protect main and the canonical vX.Y.Z tag namespace.

v0.60.3 is an immutable unsigned release from before the SignPath cutover.
Do not replace its assets. The first signed release is the next normal version
after SignPath production onboarding is complete.

## Local checks

~~~powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts\release-pipeline.tests.ps1
powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts\install-release-prerequisites.ps1 -AssertOnly
~~~
