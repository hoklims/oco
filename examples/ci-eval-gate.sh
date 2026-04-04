#!/usr/bin/env bash
# Example: OCO evaluation gate for CI
#
# Runs an eval suite against a baseline scorecard and exits with a
# verdict code that CI can branch on:
#   0 = PASS  — all quality thresholds met
#   1 = WARN  — minor regressions detected, review recommended
#   2 = FAIL  — quality gate failed, merge should be blocked
#
# Usage:
#   ./ci-eval-gate.sh [baseline.json] [candidate-results.json] [policy]
#
# Arguments:
#   baseline.json          Path to saved EvalBaseline (default: .oco/baseline.json)
#   candidate-results.json Path to write candidate eval results (default: eval-results.json)
#   policy                 Gate policy: strict | balanced | lenient (default: balanced)

set -euo pipefail

BASELINE="${1:-.oco/baseline.json}"
CANDIDATE="${2:-eval-results.json}"
POLICY="${3:-balanced}"

echo "Running OCO eval gate..."
echo "  Baseline:  $BASELINE"
echo "  Candidate: $CANDIDATE"
echo "  Policy:    $POLICY"
echo ""

# Step 1: Run the evaluation suite and save results.
oco eval scenarios.jsonl --output "$CANDIDATE"

# Step 2: Run the gate check against the baseline.
# The exit code encodes the verdict (0=pass, 1=warn, 2=fail).
oco eval-gate "$BASELINE" "$CANDIDATE" --policy "$POLICY"
EXIT_CODE=$?

# Step 3: Interpret the result for CI logs.
echo ""
case $EXIT_CODE in
  0) echo "PASS: All quality thresholds met." ;;
  1) echo "WARN: Minor regressions detected. Review recommended." ;;
  2) echo "FAIL: Quality gate failed. Merge blocked." ;;
  *) echo "ERROR: Unexpected exit code $EXIT_CODE" ;;
esac

exit $EXIT_CODE
