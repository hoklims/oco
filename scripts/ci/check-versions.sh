#!/usr/bin/env bash
set -euo pipefail

# Verify all package versions are in sync with Cargo.toml workspace version.
# Run: bash scripts/ci/check-versions.sh

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"

# Source of truth: Cargo.toml workspace version
CARGO_VERSION=$(grep -m1 '^version' "$ROOT/Cargo.toml" | sed 's/.*"\(.*\)".*/\1/')

if [ -z "$CARGO_VERSION" ]; then
  echo "FAIL: Could not read version from Cargo.toml"
  exit 1
fi

echo "Reference version (Cargo.toml): $CARGO_VERSION"

ERRORS=0

check_json_version() {
  local file="$1"
  local rel="${file#"$ROOT"/}"
  if [ ! -f "$file" ]; then
    return
  fi
  local ver
  ver=$(grep -m1 '"version"' "$file" | sed 's/.*"\([0-9][^"]*\)".*/\1/')
  if [ "$ver" != "$CARGO_VERSION" ]; then
    echo "MISMATCH: $rel has version $ver (expected $CARGO_VERSION)"
    ERRORS=$((ERRORS + 1))
  else
    echo "      OK: $rel ($ver)"
  fi
}

check_json_version "$ROOT/package.json"
check_json_version "$ROOT/apps/vscode-extension/package.json"

echo ""
if [ "$ERRORS" -gt 0 ]; then
  echo "FAIL: $ERRORS version mismatch(es) found. Update to $CARGO_VERSION."
  exit 1
else
  echo "All versions in sync."
fi
