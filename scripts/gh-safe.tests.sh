#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
test_root="$(mktemp -d)"
trap 'rm -rf "$test_root"' EXIT
mkdir -p "$test_root/bin"
log="$test_root/gh.log"

cat > "$test_root/bin/gh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf '%q ' "$@" >> "${GH_SAFE_TEST_LOG:?}"
printf '\n' >> "${GH_SAFE_TEST_LOG:?}"
if [[ "${FAKE_GH_MODE:-ok}" == cross ]]; then
  printf '%s\n' 'steipete/CodexBar|https://github.com/steipete/CodexBar'
  exit 0
fi
if [[ "${1:-}" == repo && "${2:-}" == view ]]; then
  printf '%s\n' "${3:?}|https://github.com/${3:?}"
elif [[ "${1:-}" == pr && "${2:-}" == view && " $* " == *' --json id '* ]]; then
  printf '%s\n' 'PR_kwTEST361'
elif [[ "${1:-}" == pr && "${2:-}" == view ]]; then
  printf '%s\n' 'https://github.com/nesszer/Win-CodexBar/pull/361'
elif [[ "${1:-}" == api && "${2:-}" == graphql ]]; then
  if [[ "${FAKE_GH_MODE:-ok}" == wrong-entry ]]; then
    printf '%s\n' 'https://github.com/nesszer/Win-CodexBar/pull/362|QUEUED|1'
  else
    printf '%s\n' 'https://github.com/nesszer/Win-CodexBar/pull/361|QUEUED|1'
  fi
elif [[ "${1:-}" == issue && "${2:-}" == view ]]; then
  printf '%s\n' 'https://github.com/nesszer/Win-CodexBar/issues/123'
elif [[ "${1:-}" == api ]]; then
  if [[ "${2:-}" == repos/nesszer/Win-CodexBar/releases/tags/v1.2.3 ]]; then
    printf '%s\n' 'https://github.com/nesszer/Win-CodexBar/releases/tag/v1.2.3'
  fi
fi
EOF
chmod +x "$test_root/bin/gh"
export PATH="$test_root/bin:$PATH"
export GH_SAFE_TEST_LOG="$log"

expect_fail() {
  if "$@" >/dev/null 2>&1; then
    echo "Expected failure: $*" >&2
    exit 1
  fi
}

bash -n "$repo_root/scripts/gh-safe.sh"

bash "$repo_root/scripts/gh-safe.sh" \
  --repo nesszer/Win-CodexBar --verify-kind repo --what-if -- \
  pr create --title test --body test >/dev/null

expect_fail bash "$repo_root/scripts/gh-safe.sh" \
  --repo steipete/CodexBar --verify-kind repo --what-if -- \
  pr create --title test --body test

expect_fail bash "$repo_root/scripts/gh-safe.sh" \
  --repo other/repo --verify-kind repo --what-if -- \
  pr create --title test --body test

bash "$repo_root/scripts/gh-safe.sh" \
  --repo microsoft/winget-pkgs --verify-kind repo --what-if -- \
  pr create --head Finesssee:test --title test --body test >/dev/null

bash "$repo_root/scripts/gh-safe.sh" \
  --repo Finesssee/winget-pkgs --verify-kind repo --what-if -- \
  api repos/Finesssee/winget-pkgs/merge-upstream -X POST -f branch=master >/dev/null

expect_fail bash "$repo_root/scripts/gh-safe.sh" \
  --repo Finesssee/winget-pkgs --verify-kind repo --what-if -- \
  api repos/microsoft/winget-pkgs/git/refs -X POST -f ref=refs/heads/test

expect_fail bash "$repo_root/scripts/gh-safe.sh" \
  --repo other/winget-pkgs --verify-kind repo --what-if -- \
  pr create --title test --body test

expect_fail bash "$repo_root/scripts/gh-safe.sh" \
  --repo nesszer/Win-CodexBar --verify-kind pr --target 361 --what-if -- \
  pr comment 362 --body test

expect_fail bash "$repo_root/scripts/gh-safe.sh" \
  --repo nesszer/Win-CodexBar --verify-kind pr --target 361 --what-if -- \
  pr comment 999 --comment 361

expect_fail bash "$repo_root/scripts/gh-safe.sh" \
  --repo nesszer/Win-CodexBar --verify-kind pr --target 361 --what-if -- \
  pr close

expect_fail bash "$repo_root/scripts/gh-safe.sh" \
  --repo nesszer/Win-CodexBar --verify-kind pr --target 361 --what-if -- \
  pr comment 361 --repo steipete/CodexBar --body test

FAKE_GH_MODE=cross expect_fail bash "$repo_root/scripts/gh-safe.sh" \
  --repo nesszer/Win-CodexBar --verify-kind repo --what-if -- \
  pr create --title test --body test
expect_fail bash "$repo_root/scripts/gh-safe.sh" \
  --repo nesszer/Win-CodexBar --verify-kind repo --what-if -- \
  api repos/steipete/CodexBar/branches/main/protection -X PUT -f test=true

expect_fail bash "$repo_root/scripts/gh-safe.sh" \
  --repo nesszer/Win-CodexBar --verify-kind repo --what-if -- \
  api graphql -f query=test

: > "$log"
bash "$repo_root/scripts/gh-safe.sh" \
  --repo nesszer/Win-CodexBar --verify-kind pr --target 361 -- \
  pr comment 361 --body test >/dev/null

grep -Fq 'pr comment 361 --body test --repo nesszer/Win-CodexBar' "$log" || {
  echo 'Safe wrapper did not bind the canonical repo on mutation.' >&2
  cat "$log" >&2
  exit 1
}
: > "$log"
bash "$repo_root/scripts/gh-safe.sh" \
  --repo nesszer/Win-CodexBar --verify-kind repo -- \
  api repos/nesszer/Win-CodexBar/branches/main/protection -X PUT -f test=true >/dev/null

grep -Fq 'api repos/nesszer/Win-CodexBar/branches/main/protection -X PUT -f test=true' "$log" || {
  echo 'Safe wrapper did not preserve the verified repository-scoped API path.' >&2
  cat "$log" >&2
  exit 1
}
if grep -Fq -- '--repo nesszer/Win-CodexBar' "$log"; then
  echo 'Safe wrapper incorrectly appended --repo to gh api.' >&2
  cat "$log" >&2
  exit 1
fi
: > "$log"
bash "$repo_root/scripts/gh-safe.sh" \
  --repo nesszer/Win-CodexBar --verify-kind issue --target 123 --what-if -- \
  issue close 123 >/dev/null

: > "$log"
bash "$repo_root/scripts/gh-safe.sh" \
  --repo nesszer/Win-CodexBar --verify-kind release --target v1.2.3 --what-if -- \
  release upload v1.2.3 dist/app.zip >/dev/null

# pr enqueue: wrapper-handled merge-queue entry, bound to the verified PR.
: > "$log"
bash "$repo_root/scripts/gh-safe.sh" \
  --repo nesszer/Win-CodexBar --verify-kind pr --target 361 --what-if -- \
  pr enqueue 361 >/dev/null
if grep -Fq 'graphql' "$log"; then
  echo 'pr enqueue --what-if must not call the enqueue mutation.' >&2
  cat "$log" >&2
  exit 1
fi

: > "$log"
bash "$repo_root/scripts/gh-safe.sh" \
  --repo nesszer/Win-CodexBar --verify-kind pr --target 361 -- \
  pr enqueue 361 >/dev/null
grep -Fq 'enqueuePullRequest' "$log" && grep -Fq 'id=PR_kwTEST361' "$log" || {
  echo 'pr enqueue did not send the mutation for the verified PR node id.' >&2
  cat "$log" >&2
  exit 1
}

FAKE_GH_MODE=wrong-entry expect_fail bash "$repo_root/scripts/gh-safe.sh" \
  --repo nesszer/Win-CodexBar --verify-kind pr --target 361 -- \
  pr enqueue 361

expect_fail bash "$repo_root/scripts/gh-safe.sh" \
  --repo nesszer/Win-CodexBar --verify-kind pr --target 361 --what-if -- \
  pr enqueue 362

expect_fail bash "$repo_root/scripts/gh-safe.sh" \
  --repo nesszer/Win-CodexBar --verify-kind pr --target 361 --what-if -- \
  pr enqueue 361 --admin

echo 'GitHub write-safety shell tests passed.'
