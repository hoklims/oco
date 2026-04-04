#!/usr/bin/env bash
# Example: OCO review packet generation for CI
#
# Generates a unified review packet after an OCO run, producing
# merge-readiness artifacts for PR review and CI archival.
#
# Usage:
#   ./ci-review-pack.sh [run-id] [output-dir]
#
# Arguments:
#   run-id      Run identifier (default: "last" for most recent)
#   output-dir  Directory to save artifacts (default: uses oco.toml [review] config)
#
# With [review] config in oco.toml:
#   [review]
#   auto_save = true
#   default_format = "markdown"
#   output_dir = ".oco/reviews"
#
# Then simply: ./ci-review-pack.sh
# Or without config: ./ci-review-pack.sh last ./review-artifacts

set -euo pipefail

RUN_ID="${1:-last}"
OUTPUT_DIR="${2:-}"

echo "Generating OCO review packet..."
echo "  Run: $RUN_ID"

if [ -n "$OUTPUT_DIR" ]; then
    echo "  Output: $OUTPUT_DIR"
    oco runs review-pack "$RUN_ID" --save "$OUTPUT_DIR"
else
    # Relies on [review] config in oco.toml or saves to run directory
    oco runs review-pack "$RUN_ID" --save
fi

echo ""
echo "Review packet generated."

# If output dir was specified, list the artifacts
if [ -n "$OUTPUT_DIR" ] && [ -d "$OUTPUT_DIR" ]; then
    echo "Artifacts:"
    ls -la "$OUTPUT_DIR"/review-packet.* 2>/dev/null || true
fi

# Optional: output the JSON packet for CI consumption
# oco runs review-pack "$RUN_ID" --json
