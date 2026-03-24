#!/usr/bin/env bash
# OCO Hook: Stop
# Prevents premature completion on tasks that modified code.
# Requires verification where applicable.

set -o pipefail

INPUT=$(cat || true)

STOP_REASON=$(echo "$INPUT" | jq -r '.reason // "complete"' 2>/dev/null || echo "complete")

# Only enforce verification for completion stops
if [ "$STOP_REASON" != "complete" ] && [ "$STOP_REASON" != "" ]; then
  exit 0
fi

# Source shared session init (secure state dir, canonical workspace)
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=session-init.sh
source "${SCRIPT_DIR}/session-init.sh" 2>/dev/null || {
  OCO_STATE_DIR="/tmp/oco-session-default"
  mkdir -p "$OCO_STATE_DIR" 2>/dev/null || true
}

# Check if files were modified during this session
MODIFIED_LOG="${OCO_STATE_DIR}/modified-files"
if [ ! -f "$MODIFIED_LOG" ] || [ -L "$MODIFIED_LOG" ]; then
  # No files modified or symlink — safe to stop
  exit 0
fi

MODIFIED_COUNT=$(sort -u "$MODIFIED_LOG" 2>/dev/null | wc -l | tr -d ' ')

if [ "$MODIFIED_COUNT" -eq 0 ]; then
  exit 0
fi

# Files were modified — check if verification was performed AFTER last modification
VERIFY_LOG="${OCO_STATE_DIR}/verify-done"
if [ -f "$VERIFY_LOG" ] && [ ! -L "$VERIFY_LOG" ]; then
  # Verification was performed after last modification — clean up and allow stop
  rm -f "$MODIFIED_LOG" "$VERIFY_LOG" 2>/dev/null || true
  exit 0
fi

# --- Determine what verification is needed ---
NEEDS_CHECK=""

if [ -f "package.json" ]; then
  HAS_TEST=$(jq -r '.scripts.test // empty' package.json 2>/dev/null || echo "")
  HAS_LINT=$(jq -r '.scripts.lint // empty' package.json 2>/dev/null || echo "")
  [ -n "$HAS_TEST" ] && NEEDS_CHECK="${NEEDS_CHECK}test,"
  [ -n "$HAS_LINT" ] && NEEDS_CHECK="${NEEDS_CHECK}lint,"
fi

if [ -f "Cargo.toml" ]; then
  NEEDS_CHECK="${NEEDS_CHECK}build,test,clippy,"
fi

if [ -f "pyproject.toml" ] || [ -f "setup.py" ]; then
  NEEDS_CHECK="${NEEDS_CHECK}test,typecheck,"
fi

if [ -z "$NEEDS_CHECK" ]; then
  # No verifiable project detected — allow stop, clean up
  rm -f "$MODIFIED_LOG" 2>/dev/null || true
  exit 0
fi

# Remove trailing comma
NEEDS_CHECK=$(echo "$NEEDS_CHECK" | sed 's/,$//')

MODIFIED_FILES=$(sort -u "$MODIFIED_LOG" | head -10 | tr '\n' ', ' | sed 's/,$//')

# Exit 2 = block stop. stderr is shown to Claude as error context.
echo "OCO: ${MODIFIED_COUNT} file(s) modified [${MODIFIED_FILES}] but no verification run detected. Recommended checks: ${NEEDS_CHECK}. Run build/test/lint before completing." >&2
exit 2

# Don't clean up — let the user verify and retry stop
