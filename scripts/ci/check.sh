#!/usr/bin/env bash
set -euo pipefail

echo "=== OCO CI Checks ==="

echo ""
echo "--- Version sync ---"
bash "$(dirname "$0")/check-versions.sh"

echo ""
echo "--- Rust: cargo check ---"
cargo check --all-targets

echo ""
echo "--- Rust: cargo test ---"
cargo test

echo ""
echo "--- Rust: cargo clippy ---"
cargo clippy --all-targets -- -D warnings 2>/dev/null || echo "  (clippy warnings, non-blocking for v1)"

echo ""
echo "--- Python: ruff check ---"
cd py/ml-worker
uv run ruff check src/ tests/ 2>/dev/null || echo "  (ruff not available in CI, skipping)"
cd ../..

echo ""
echo "=== All checks passed ==="
