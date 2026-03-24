#!/usr/bin/env bash
# OCO Hook: PreToolUse
# Enforces tool policy gates before execution.
# Exit 0 = allow, Exit 2 = block (stderr shown to Claude as error).

set -o pipefail

INPUT=$(cat || true)

TOOL_NAME=$(echo "$INPUT" | jq -r '.tool_name // empty' 2>/dev/null || echo "")
TOOL_INPUT=$(echo "$INPUT" | jq -c '.tool_input // {}' 2>/dev/null || echo "{}")

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

# --- Policy Gate: Destructive command detection ---
if [ "$TOOL_NAME" = "Bash" ] || [ "$TOOL_NAME" = "bash" ]; then
  COMMAND=$(echo "$TOOL_INPUT" | jq -r '.command // empty' 2>/dev/null || echo "")

  if [ -n "$COMMAND" ]; then
    DESTRUCTIVE_PATTERNS=(
      "rm -rf" "rm -r " "rmdir"
      "git reset --hard" "git push --force" "git push -f "
      "git clean -fd" "git checkout -- ." "git restore ."
      "drop table" "drop database" "truncate table"
    )

    CMD_LOWER=$(echo "$COMMAND" | LC_ALL=C tr '[:upper:]' '[:lower:]')

    for pattern in "${DESTRUCTIVE_PATTERNS[@]}"; do
      if [[ "$CMD_LOWER" == *"$pattern"* ]]; then
        # Exit 2 = block tool execution. stderr is shown to Claude.
        echo "OCO policy: destructive command detected (${pattern}). Use a safer alternative or confirm explicitly." >&2
        exit 2
      fi
    done
  fi
fi

# --- Policy Gate: Write tool risk assessment ---
if [ "$TOOL_NAME" = "Edit" ] || [ "$TOOL_NAME" = "Write" ] || [ "$TOOL_NAME" = "MultiEdit" ]; then
  FILE_PATH=$(echo "$TOOL_INPUT" | jq -r '.file_path // .path // empty' 2>/dev/null || echo "")

  if [ -n "$FILE_PATH" ]; then
    SENSITIVE_PATTERNS=(".env" "credentials" "secrets" ".key" ".pem" "id_rsa")
    FILE_LOWER=$(echo "$FILE_PATH" | LC_ALL=C tr '[:upper:]' '[:lower:]')

    for pattern in "${SENSITIVE_PATTERNS[@]}"; do
      if [[ "$FILE_LOWER" == *"$pattern"* ]]; then
        echo "OCO policy: write to sensitive file (${pattern}) blocked. Review manually." >&2
        exit 2
      fi
    done
  fi
fi

# --- Loop detection: track tool call frequency ---
LOOP_FILE="${OCO_STATE_DIR}/loop-${TOOL_NAME}"
if [ -f "$LOOP_FILE" ] && [ ! -L "$LOOP_FILE" ]; then
  COUNT=$(cat "$LOOP_FILE" 2>/dev/null || echo "0")
  COUNT=$((COUNT + 1))
  echo "$COUNT" > "$LOOP_FILE"

  if [ "$COUNT" -ge 5 ]; then
    # Warn but don't block (exit 0 with context)
    jq -n --arg msg "OCO: tool '${TOOL_NAME}' called ${COUNT} times. Possible loop — consider a different approach." \
      '{"hookSpecificOutput": {"additionalContext": $msg}}'
    if [ "$COUNT" -ge 8 ]; then
      echo "0" > "$LOOP_FILE"
    fi
    exit 0
  fi
else
  echo "1" > "$LOOP_FILE"
fi

# --- If OCO binary available, run advanced gate check ---
if command -v "$OCO_BIN" &>/dev/null; then
  GATE_RESULT=$("$OCO_BIN" gate-check --tool "$TOOL_NAME" --input "$TOOL_INPUT" --format json 2>/dev/null || echo '{}')
  GATE_DECISION=$(echo "$GATE_RESULT" | jq -r '.decision // "allow"' 2>/dev/null || echo "allow")

  if [ "$GATE_DECISION" = "deny" ]; then
    GATE_REASON=$(echo "$GATE_RESULT" | jq -r '.reason // "denied by policy"' 2>/dev/null)
    echo "OCO policy: ${GATE_REASON}" >&2
    exit 2
  fi
fi

# Default: allow
exit 0
