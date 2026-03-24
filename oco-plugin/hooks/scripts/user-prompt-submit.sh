#!/usr/bin/env bash
# OCO Hook: UserPromptSubmit
# Lightweight triage on every user request.
# Classifies task, identifies verification needs, injects compact guidance.
# Must be fast (<200ms), degrade gracefully if OCO unavailable.

# No set -e: hooks must degrade gracefully, never crash
set -o pipefail

# Read hook input from stdin (Claude Code passes JSON)
INPUT=$(cat || true)

# Claude Code uses "prompt" field for UserPromptSubmit
USER_PROMPT=$(echo "$INPUT" | jq -r '.prompt // empty' 2>/dev/null || true)

if [ -z "$USER_PROMPT" ]; then
  exit 0
fi

# Try calling local OCO binary for classification
OCO_BIN="${OCO_BIN:-oco}"
# Use cwd from hook input if available, fallback to PWD
WORKSPACE=$(echo "$INPUT" | jq -r '.cwd // empty' 2>/dev/null || echo "")
[ -z "$WORKSPACE" ] && WORKSPACE="${PWD}"

# Check dependencies
if ! command -v jq &>/dev/null || ! command -v "$OCO_BIN" &>/dev/null; then
  exit 0
fi

# Call OCO classify (lightweight, no LLM, pure heuristics)
CLASSIFICATION=$("$OCO_BIN" classify "$USER_PROMPT" --workspace "$WORKSPACE" --format json 2>/dev/null || echo '{}')

COMPLEXITY=$(echo "$CLASSIFICATION" | jq -r '.complexity // "medium"' 2>/dev/null || echo "medium")
NEEDS_VERIFY=$(echo "$CLASSIFICATION" | jq -r '.needs_verification // false' 2>/dev/null || echo "false")
TASK_TYPE=$(echo "$CLASSIFICATION" | jq -r '.task_type // "unknown"' 2>/dev/null || echo "unknown")
PRIORITY_FILES=$(echo "$CLASSIFICATION" | jq -r '.priority_files // [] | join(", ")' 2>/dev/null || echo "")

# Only inject guidance for non-trivial tasks
if [ "$COMPLEXITY" = "trivial" ]; then
  exit 0
fi

# Build compact guidance block
GUIDANCE="[OCO] complexity=${COMPLEXITY} type=${TASK_TYPE} verify=${NEEDS_VERIFY}"

if [ -n "$PRIORITY_FILES" ]; then
  GUIDANCE="${GUIDANCE} files=[${PRIORITY_FILES}]"
fi

# Inject specific workflow hints based on complexity
case "$COMPLEXITY" in
  high|critical)
    GUIDANCE="${GUIDANCE} | Recommended: investigate before acting. Use oco-inspect-repo-area skill for context."
    ;;
  medium)
    if [ "$NEEDS_VERIFY" = "true" ]; then
      GUIDANCE="${GUIDANCE} | Verify after changes: use oco-verify-fix skill."
    fi
    ;;
esac

# Output as Claude Code hook response — additionalContext for Claude
jq -n --arg msg "$GUIDANCE" '{"hookSpecificOutput": {"additionalContext": $msg}}'
