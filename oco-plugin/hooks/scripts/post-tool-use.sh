#!/usr/bin/env bash
# OCO Hook: PostToolUse
# Normalizes tool results into OCO observations.
# Captures telemetry. Keeps summaries compact.
# Never floods Claude with raw output.

set -o pipefail

INPUT=$(cat || true)

TOOL_NAME=$(echo "$INPUT" | jq -r '.tool_name // empty' 2>/dev/null || echo "")
TOOL_ERROR=$(echo "$INPUT" | jq -r '.error // empty' 2>/dev/null || echo "")

if [ -z "$TOOL_NAME" ]; then
  exit 0
fi

OCO_BIN="${OCO_BIN:-oco}"

# Source shared session init (secure state dir, canonical workspace)
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=session-init.sh
source "${SCRIPT_DIR}/session-init.sh" 2>/dev/null || {
  OCO_STATE_DIR="/tmp/oco-session-default"
  mkdir -p "$OCO_STATE_DIR" 2>/dev/null || true
}

# --- Telemetry: record observation locally ---
if command -v "$OCO_BIN" &>/dev/null; then
  "$OCO_BIN" observe \
    --tool "$TOOL_NAME" \
    --status "$([ -n "$TOOL_ERROR" ] && echo 'error' || echo 'ok')" \
    --format json \
    >> "${OCO_STATE_DIR}/observe.log" 2>&1 &
fi

# --- Track which files were modified (for Stop hook verification check) ---
if [ "$TOOL_NAME" = "Edit" ] || [ "$TOOL_NAME" = "Write" ] || [ "$TOOL_NAME" = "MultiEdit" ]; then
  FILE_PATH=$(echo "$INPUT" | jq -r '.tool_input.file_path // .tool_input.path // .tool_input.destination // empty' 2>/dev/null || echo "")
  if [ -n "$FILE_PATH" ]; then
    echo "$FILE_PATH" >> "${OCO_STATE_DIR}/modified-files" 2>/dev/null || true
    # Invalidate previous verification — new changes require re-verification
    rm -f "${OCO_STATE_DIR}/verify-done" 2>/dev/null || true
  fi
fi

# --- Detect verification tool runs and mark them ---
# Only mark verified if no error AND the command is a recognized verification command
if [ -z "$TOOL_ERROR" ]; then
  case "$TOOL_NAME" in
    Bash|bash)
      COMMAND=$(echo "$INPUT" | jq -r '.tool_input.command // empty' 2>/dev/null || echo "")
      CMD_LOWER=$(echo "$COMMAND" | LC_ALL=C tr '[:upper:]' '[:lower:]')
      # Match only when the verification command is the primary command (starts the line or follows && / ;)
      # Use word-boundary-aware patterns to reduce false positives
      VERIFIED=false
      for verify_cmd in "cargo test" "cargo build" "cargo check" "cargo clippy" \
                        "npm test" "npm run build" "npm run lint" \
                        "pytest" "python -m pytest" "go test" "go build" \
                        "tsc --noemit" "npx tsc" "mypy" "ruff check"; do
        if [[ "$CMD_LOWER" == "$verify_cmd"* ]] || \
           [[ "$CMD_LOWER" == *" && $verify_cmd"* ]] || \
           [[ "$CMD_LOWER" == *"; $verify_cmd"* ]]; then
          VERIFIED=true
          break
        fi
      done
      if [ "$VERIFIED" = "true" ]; then
        echo "verified" > "${OCO_STATE_DIR}/verify-done" 2>/dev/null || true
      fi
      ;;
  esac
fi

# --- Reset loop counter for this tool on success ---
if [ -z "$TOOL_ERROR" ]; then
  LOOP_FILE="${OCO_STATE_DIR}/loop-${TOOL_NAME}"
  echo "0" > "$LOOP_FILE" 2>/dev/null || true
fi

# Default: no message injection (keep Claude's context clean)
exit 0
