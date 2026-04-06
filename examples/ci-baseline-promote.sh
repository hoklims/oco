#!/usr/bin/env bash
# Example: OCO baseline promotion in CI
#
# After a gate evaluation passes, promotes the candidate scorecard
# to become the new baseline. Leaves an audit trail entry in
# .oco/baseline-history.json.
#
# Usage:
#   ./ci-baseline-promote.sh [source] [name] [reason]
#
# Arguments:
#   source   Run ID or file path (default: "last")
#   name     Baseline name (default: auto-generated from date)
#   reason   Promotion reason (default: "CI gate passed")

set -uo pipefail

SOURCE="${1:-last}"
NAME="${2:-$(date +%Y%m%d-%H%M%S)}"
REASON="${3:-CI gate passed}"

echo "Promoting baseline..."
echo "  Source: $SOURCE"
echo "  Name:   $NAME"
echo "  Reason: $REASON"
echo ""

# Step 1: Run the gate check first — only promote if it passes.
EXIT_CODE=0
oco eval-gate || EXIT_CODE=$?

if [ "$EXIT_CODE" -eq 2 ]; then
    echo "FAIL: Gate failed — refusing to promote."
    exit 2
fi

if [ "$EXIT_CODE" -eq 1 ]; then
    echo "WARN: Gate produced warnings — promoting with review flag."
fi

# Step 2: Promote the candidate to baseline.
oco baseline-promote "$SOURCE" --name "$NAME" --reason "$REASON"

# Step 3: Show the audit trail.
echo ""
echo "Audit trail:"
oco baseline-history --limit 3

# Q11: The promote command:
# - Backs up the old baseline to .oco/baseline.json.bak
# - Saves the new baseline to the [gate].baseline_path location
# - Appends an entry to .oco/baseline-history.json
# - Shows a promotion recommendation (promote/review/reject)
# - Outputs a diff between old and new baseline

# Q11: Safety: if the gate fails, baseline-promote aborts with exit code 2.
#   Use --force to override: oco baseline-promote last --name v2 --force
#
# Q11: JSON output for CI integration:
#   oco baseline-promote last --name v2 --reason "release" --json
#   oco baseline-history --json

echo ""
echo "Promotion complete."
