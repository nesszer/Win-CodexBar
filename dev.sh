#!/usr/bin/env bash
# Build and run CodexBar on Linux / WSL.
#
# Usage:
#   ./dev.sh                 # debug build + run
#   ./dev.sh --release       # optimised build
#   ./dev.sh --skip-build    # run last build
#   ./dev.sh --verbose       # debug build + run with verbose logging
#   ./dev.sh --cli           # run CLI usage command instead of menubar

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")" && pwd)"
RUST_DIR="$REPO_ROOT/rust"

# ── Parse arguments ──────────────────────────────────────────────────────────

RELEASE=0
SKIP_BUILD=0
VERBOSE=0
CLI_MODE=0

for arg in "$@"; do
    case "$arg" in
        --release)   RELEASE=1 ;;
        --skip-build) SKIP_BUILD=1 ;;
        --verbose)   VERBOSE=1 ;;
        --cli)       CLI_MODE=1 ;;
        -h|--help)
            echo "Usage: $0 [--release] [--skip-build] [--verbose] [--cli]"
            echo ""
            echo "Options:"
            echo "  --release      Optimised build"
            echo "  --skip-build   Run last build without rebuilding"
            echo "  --verbose      Enable debug logging"
            echo "  --cli          Run CLI usage command instead of menubar GUI"
            exit 0
            ;;
        *)
            echo "Unknown option: $arg"
            echo "Run '$0 --help' for usage."
            exit 1
            ;;
    esac
done

# ── Check prerequisites ──────────────────────────────────────────────────────

if ! command -v cargo &>/dev/null; then
    echo "ERROR: cargo (Rust) not found."
    echo "Install Rust: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
    exit 1
fi

# ── WSL detection ─────────────────────────────────────────────────────────────

IS_WSL=0
if grep -qi microsoft /proc/version 2>/dev/null || [ -n "${WSL_DISTRO_NAME:-}" ]; then
    IS_WSL=1
    echo "Detected WSL environment ($WSL_DISTRO_NAME)"

    # Check for GUI support
    if [ -z "${DISPLAY:-}" ] && [ -z "${WAYLAND_DISPLAY:-}" ]; then
        echo ""
        echo "⚠  No display server detected."
        echo "   GUI mode requires WSLg (Windows 11) or an X server."
        echo "   Use --cli to run CLI commands instead."
        echo ""
        if [ "$CLI_MODE" -eq 0 ]; then
            echo "Tip: 'codexbar usage -p claude' works without a display."
            echo ""
            CLI_MODE=1
            echo "Auto-switching to CLI mode."
        fi
    fi
fi

# ── Build ─────────────────────────────────────────────────────────────────────

if [ "$SKIP_BUILD" -eq 0 ]; then
    cd "$RUST_DIR"

    if [ "$RELEASE" -eq 1 ]; then
        echo "Building CodexBar (release)..."
        cargo build --bin codexbar --release
    else
        echo "Building CodexBar (debug)..."
        cargo build --bin codexbar
    fi

    cd "$REPO_ROOT"
fi

# ── Locate binary ─────────────────────────────────────────────────────────────

PROFILE="debug"
[ "$RELEASE" -eq 1 ] && PROFILE="release"

BINARY="$RUST_DIR/target/$PROFILE/codexbar"
if [ ! -f "$BINARY" ]; then
    echo "ERROR: Binary not found at $BINARY"
    echo "Run without --skip-build to build first."
    exit 1
fi

# ── Run ───────────────────────────────────────────────────────────────────────

echo ""
if [ "$CLI_MODE" -eq 1 ]; then
    echo "Running: codexbar usage -p all"
    ARGS="usage -p all"
else
    echo "Running: codexbar menubar"
    ARGS="menubar"
fi

if [ "$VERBOSE" -eq 1 ]; then
    "$BINARY" -v $ARGS
else
    "$BINARY" $ARGS
fi
