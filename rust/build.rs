//! Build script for CodexBar
//!
//! Injects build metadata (git commit) at compile time.

use std::path::Path;
use std::process::Command;

/// Resolves a git path (e.g. `HEAD`, `refs/heads/main`, `packed-refs`) through git itself,
/// so linked worktrees (per-worktree HEAD, common-dir refs) and detached HEAD are handled.
/// Build scripts run with cwd = `rust/`, which is not the repository root.
fn git_path(name: &str) -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--path-format=absolute", "--git-path", name])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let path = String::from_utf8(output.stdout).ok()?;
    let path = path.trim();
    if path.is_empty() {
        None
    } else {
        Some(path.to_string())
    }
}

fn main() {
    // Get git commit hash
    let git_commit = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|output| {
            if output.status.success() {
                String::from_utf8(output.stdout).ok()
            } else {
                None
            }
        })
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    println!("cargo:rustc-env=GIT_COMMIT={}", git_commit);

    // Re-run if git HEAD or the checked-out branch ref changes. Only paths that exist are
    // emitted: cargo treats a missing rerun-if-changed path as always dirty.
    let mut watched: Vec<String> = Vec::new();
    if let Some(head_path) = git_path("HEAD") {
        let head = std::fs::read_to_string(&head_path).unwrap_or_default();
        let reference = head.trim().strip_prefix("ref: ").map(str::trim);
        if let Some(reference) = reference.filter(|r| !r.is_empty()) {
            watched.extend(git_path(reference));
            watched.extend(git_path("packed-refs"));
        }
        watched.push(head_path);
    }

    let mut emitted = false;
    for path in watched.iter().filter(|p| Path::new(p).exists()) {
        println!("cargo:rerun-if-changed={}", path);
        emitted = true;
    }
    if !emitted {
        println!("cargo:rerun-if-changed=build.rs");
    }
}
