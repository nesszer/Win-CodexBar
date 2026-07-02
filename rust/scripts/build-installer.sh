#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RUST_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
REPO_ROOT="$(cd "$RUST_DIR/.." && pwd)"
VERSION="${1:-$(sed -n 's/^version = "\(.*\)"/\1/p' "$RUST_DIR/Cargo.toml" | head -n 1)}"
TARGET_TRIPLE="${INSTALLER_TARGET_TRIPLE:-x86_64-pc-windows-gnu}"
TARGET_BIN_DIR="$RUST_DIR/target/$TARGET_TRIPLE/release"
OUTPUT_DIR="$RUST_DIR/target/installer"
INSTALLER_DEPS_DIR="$RUST_DIR/target/installer-deps"
INSTALLER_PATH="$OUTPUT_DIR/CodexBar-${VERSION}-Setup.exe"
INNO_IMAGE="${INNO_SETUP_IMAGE:-amake/innosetup}"
CONTAINER_NAME="codexbar-inno-${VERSION//./-}-$$"
VC_REDIST_URL="${VC_REDIST_URL:-https://aka.ms/vc14/vc_redist.x64.exe}"
VC_REDIST_PATH="$INSTALLER_DEPS_DIR/vc_redist.x64.exe"
VC_REDIST_SHA256="${VC_REDIST_SHA256:-}"
WEBVIEW2_BOOTSTRAPPER_URL="${WEBVIEW2_BOOTSTRAPPER_URL:-https://go.microsoft.com/fwlink/p/?LinkId=2124703}"
WEBVIEW2_BOOTSTRAPPER_PATH="$INSTALLER_DEPS_DIR/MicrosoftEdgeWebview2Setup.exe"
WEBVIEW2_BOOTSTRAPPER_SHA256="${WEBVIEW2_BOOTSTRAPPER_SHA256:-}"

for required_file in "$TARGET_BIN_DIR/codexbar.exe" "$TARGET_BIN_DIR/codexbar-cli.exe" "$TARGET_BIN_DIR/codexbar-desktop.exe" "$RUST_DIR/icons/icon.ico"; do
  if [[ ! -f "$required_file" ]]; then
    echo "Missing required build artifact: $required_file" >&2
    echo "Build the Windows target first, then rerun this script." >&2
    exit 1
  fi
done

mkdir -p "$OUTPUT_DIR"
mkdir -p "$INSTALLER_DEPS_DIR"

verify_sha256() {
  local file_path="$1"
  local expected_sha256="$2"
  local label="$3"
  local actual_sha256

  expected_sha256="$(printf '%s' "$expected_sha256" | tr '[:upper:]' '[:lower:]')"
  if command -v sha256sum >/dev/null 2>&1; then
    actual_sha256="$(sha256sum "$file_path" | awk '{print $1}')"
  elif command -v shasum >/dev/null 2>&1; then
    actual_sha256="$(shasum -a 256 "$file_path" | awk '{print $1}')"
  else
    echo "Missing SHA-256 tool (sha256sum or shasum) required for dependency verification." >&2
    exit 1
  fi

  if [[ "$actual_sha256" != "$expected_sha256" ]]; then
    echo "$label checksum mismatch." >&2
    echo "Expected: $expected_sha256" >&2
    echo "Actual:   $actual_sha256" >&2
    exit 1
  fi
}

if [[ -z "$VC_REDIST_SHA256" ]]; then
  cat >&2 <<'EOF'
VC_REDIST_SHA256 is required to verify vc_redist.x64.exe.
Set VC_REDIST_SHA256 to the expected SHA-256 hash from a trusted source.
EOF
  exit 1
fi

if [[ -z "$WEBVIEW2_BOOTSTRAPPER_SHA256" ]]; then
  cat >&2 <<'EOF'
WEBVIEW2_BOOTSTRAPPER_SHA256 is required to verify MicrosoftEdgeWebview2Setup.exe.
Set WEBVIEW2_BOOTSTRAPPER_SHA256 to the expected SHA-256 hash from a trusted source.
EOF
  exit 1
fi

curl -L "$VC_REDIST_URL" -o "$VC_REDIST_PATH"
verify_sha256 "$VC_REDIST_PATH" "$VC_REDIST_SHA256" "vc_redist.x64.exe"

curl -L "$WEBVIEW2_BOOTSTRAPPER_URL" -o "$WEBVIEW2_BOOTSTRAPPER_PATH"
verify_sha256 \
  "$WEBVIEW2_BOOTSTRAPPER_PATH" \
  "$WEBVIEW2_BOOTSTRAPPER_SHA256" \
  "MicrosoftEdgeWebview2Setup.exe"

cleanup() {
  docker rm -f "$CONTAINER_NAME" >/dev/null 2>&1 || true
}
trap cleanup EXIT

docker create \
  --name "$CONTAINER_NAME" \
  -v "$REPO_ROOT:/work" \
  -w /work/rust/installer \
  --entrypoint /bin/sh \
  "$INNO_IMAGE" \
  -lc "/opt/bin/iscc /Qp /DAppVersion=$VERSION /DTargetBinDir=..\\\\target\\\\$TARGET_TRIPLE\\\\release /DVCRedistPath=..\\\\target\\\\installer-deps\\\\vc_redist.x64.exe /DWebView2BootstrapperPath=..\\\\target\\\\installer-deps\\\\MicrosoftEdgeWebview2Setup.exe /DOutputDir=C:\\\\inno-out /DOutputBaseFilename=CodexBar-$VERSION-Setup codexbar.iss" \
  >/dev/null

docker start -a "$CONTAINER_NAME" >/dev/null
docker cp \
  "$CONTAINER_NAME:/home/xclient/.wine/drive_c/inno-out/CodexBar-$VERSION-Setup.exe" \
  "$INSTALLER_PATH"

if [[ ! -f "$INSTALLER_PATH" ]]; then
  echo "Expected installer was not created: $INSTALLER_PATH" >&2
  exit 1
fi

printf '%s\n' "$INSTALLER_PATH"
