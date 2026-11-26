#!/usr/bin/env bash
set -euo pipefail

repo=""
verify_kind=""
target=""
allow_upstream_write=0
what_if=0

usage() {
  cat <<'EOF'
Usage:
  bash scripts/gh-safe.sh --repo owner/repo --verify-kind repo|pr|issue|release [--target id-or-tag] [--allow-upstream-write] [--what-if] -- <gh args...>

Examples:
  bash scripts/gh-safe.sh --repo nesszer/Win-CodexBar --verify-kind pr --target 361 --what-if -- pr comment 361 --body-file .review/comment.md
  bash scripts/gh-safe.sh --repo nesszer/Win-CodexBar --verify-kind repo --what-if -- pr create --title "..." --body-file body.md
  bash scripts/gh-safe.sh --repo nesszer/Win-CodexBar --verify-kind pr --target 361 --what-if -- pr enqueue 361

`pr enqueue <n>` is handled by this wrapper, not gh: it adds the verified PR to the
base branch's merge queue (GraphQL enqueuePullRequest) and checks that the queue entry
belongs to the same PR. gh pr merge can't do this while repository auto-merge is off.
  bash scripts/gh-safe.sh --repo microsoft/winget-pkgs --verify-kind repo --what-if -- pr create --head Finesssee:branch --title "..." --body-file body.md
EOF
}

while (($#)); do
  case "$1" in
    --repo) repo="${2:-}"; shift 2 ;;
    --verify-kind) verify_kind="${2:-}"; shift 2 ;;
    --target) target="${2:-}"; shift 2 ;;
    --allow-upstream-write) allow_upstream_write=1; shift ;;
    --what-if) what_if=1; shift ;;
    --) shift; break ;;
    -h|--help) usage; exit 0 ;;
    *) echo "Unknown wrapper argument: $1" >&2; usage >&2; exit 2 ;;
  esac
done

gh_args=("$@")

[[ "$repo" =~ ^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$ ]] || { echo "Invalid --repo '$repo'; expected owner/repo." >&2; exit 2; }
case "$verify_kind" in repo|pr|issue|release) ;; *) echo "Invalid --verify-kind '$verify_kind'." >&2; exit 2 ;; esac
((${#gh_args[@]} > 0)) || { echo 'No gh command supplied after --.' >&2; exit 2; }

if [[ "${repo,,}" == "steipete/codexbar" ]]; then
  ((allow_upstream_write == 1)) || { echo 'Writes to steipete/CodexBar are blocked by default. Explicit current-turn authorization is required.' >&2; exit 3; }
else
  # Winget release step: the fork receives the manifest branch, and the PR opens on microsoft/winget-pkgs.
  case "${repo,,}" in
    nesszer/win-codexbar|finesssee/winget-pkgs|microsoft/winget-pkgs) ;;
    *) echo "GitHub writes are not allowlisted for '$repo'. Expected nesszer/Win-CodexBar, Finesssee/winget-pkgs or microsoft/winget-pkgs." >&2; exit 3 ;;
  esac
fi

for arg in "${gh_args[@]}"; do
  case "$arg" in --repo|-R|--repo=*) echo 'Forwarded gh args may not override the repository.' >&2; exit 3 ;; esac
done

if [[ "$verify_kind" != repo ]]; then
  [[ -n "$target" ]] || { echo "--target is required for verify kind '$verify_kind'." >&2; exit 2; }
  [[ "${gh_args[0]}" == "$verify_kind" ]] || { echo "Forwarded command must target '$verify_kind'." >&2; exit 3; }
  ((${#gh_args[@]} >= 3)) || { echo "Forwarded command must carry its object as the third token (gh <object> <verb> <number>)." >&2; exit 3; }
  [[ "${gh_args[2]}" == "$target" ]] || { echo "Forwarded command object '${gh_args[2]:-}' is not the verified target '$target'." >&2; exit 3; }
fi
enqueue=0
if [[ "$verify_kind" == pr && "${gh_args[0]}" == pr && "${gh_args[1]}" == enqueue ]]; then
  ((${#gh_args[@]} == 3)) || { echo 'pr enqueue takes only the PR number.' >&2; exit 3; }
  enqueue=1
fi
api_passthrough=0
if [[ "$verify_kind" == repo && "${gh_args[0]}" == api ]]; then
  ((${#gh_args[@]} >= 2)) || { echo 'gh api requires a repository-scoped REST path.' >&2; exit 3; }
  api_path="${gh_args[1]}"
  expected_api_prefix="repos/$repo/"
  shopt -s nocasematch
  [[ "$api_path" == "$expected_api_prefix"* ]] || {
    shopt -u nocasematch
    echo "Repository API write path '$api_path' is outside '$expected_api_prefix'." >&2
    exit 3
  }
  shopt -u nocasematch
  [[ "$api_path" != *'..'* ]] || { echo "Repository API write path may not contain '..'." >&2; exit 3; }
  api_passthrough=1
fi

case "$verify_kind" in
  repo)
    readback="$(gh repo view "$repo" --json url,nameWithOwner --jq '.nameWithOwner + "|" + .url')"
    readback_repo="${readback%%|*}"
    verified_url="${readback#*|}/"
    [[ "${readback_repo,,}" == "${repo,,}" ]] || { echo "Repository read-back mismatch: '$readback_repo' != '$repo'." >&2; exit 4; }
    ;;
  pr) verified_url="$(gh pr view "$target" --repo "$repo" --json url --jq .url)" ;;
  issue) verified_url="$(gh issue view "$target" --repo "$repo" --json url --jq .url)" ;;
  release) verified_url="$(gh api "repos/$repo/releases/tags/$target" --jq .html_url)" ;;
esac

expected_prefix="https://github.com/$repo/"
shopt -s nocasematch
[[ "$verified_url" == "$expected_prefix"* ]] || { echo "GitHub target mismatch: '$verified_url' is not under '$expected_prefix'." >&2; exit 4; }
case "$verify_kind" in
  pr) [[ "$verified_url" == *"/pull/$target" ]] || { echo "GitHub target mismatch: '$verified_url' does not end with '/pull/$target'." >&2; exit 4; } ;;
  issue) [[ "$verified_url" == *"/issues/$target" ]] || { echo "GitHub target mismatch: '$verified_url' does not end with '/issues/$target'." >&2; exit 4; } ;;
  release) [[ "$verified_url" == *"/releases/tag/$target" ]] || { echo "GitHub target mismatch: '$verified_url' does not end with '/releases/tag/$target'." >&2; exit 4; } ;;
esac
shopt -u nocasematch

echo "Verified GitHub write target: $verified_url"
if ((enqueue == 1)); then
  pr_id="$(gh pr view "$target" --repo "$repo" --json id --jq .id)"
  [[ "$pr_id" =~ ^PR_[A-Za-z0-9_-]+$ ]] || { echo "Unexpected pull request node id '$pr_id'." >&2; exit 4; }
  if ((what_if == 1)); then
    echo "WhatIf: enqueue $verified_url ($pr_id) in its merge queue"
    exit 0
  fi
  entry="$(gh api graphql \
    -f query='mutation($id: ID!) { enqueuePullRequest(input: {pullRequestId: $id}) { mergeQueueEntry { state position pullRequest { url } } } }' \
    -f id="$pr_id" \
    --jq '.data.enqueuePullRequest.mergeQueueEntry | "\(.pullRequest.url)|\(.state)|\(.position)"')"
  [[ "${entry%%|*}" == "$verified_url" ]] || { echo "Merge queue entry '$entry' does not belong to $verified_url." >&2; exit 4; }
  echo "Enqueued: $entry"
  exit 0
fi
if ((what_if == 1)); then
  printf 'WhatIf: gh'
  printf ' %q' "${gh_args[@]}"
  if ((api_passthrough == 0)); then
    printf ' --repo %q' "$repo"
  fi
  printf '\n'
  exit 0
fi

if ((api_passthrough == 1)); then
  gh "${gh_args[@]}"
else
  gh "${gh_args[@]}" --repo "$repo"
fi
