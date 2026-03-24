#!/usr/bin/env bash
set -euo pipefail

echo "=== OCO Development Setup ==="
echo ""

# Check prerequisites
check_cmd() {
    if ! command -v "$1" &>/dev/null; then
        echo "ERROR: $1 is not installed. $2"
        exit 1
    fi
    echo "  ✓ $1 found: $($1 --version 2>&1 | head -1)"
}

echo "Checking prerequisites..."
check_cmd cargo "Install Rust: https://rustup.rs"
check_cmd node "Install Node.js 20+: https://nodejs.org"
check_cmd pnpm "Install pnpm: npm install -g pnpm"
check_cmd python "Install Python 3.11+: https://python.org"
check_cmd uv "Install uv: pip install uv"
echo ""

# Build Rust workspace
echo "Building Rust workspace..."
cargo build
echo ""

# Setup Python ML worker
echo "Setting up Python ML worker..."
cd py/ml-worker
uv venv
uv pip install -e ".[dev]" 2>/dev/null || echo "  (Python deps will be installed on first use)"
cd ../..
echo ""

# Setup VS Code extension
echo "Setting up VS Code extension..."
cd apps/vscode-extension
pnpm install
pnpm build 2>/dev/null || echo "  (Extension build will work after first pnpm install)"
cd ../..
echo ""

echo "=== Setup Complete ==="
echo ""
echo "Quick start:"
echo "  cargo run -p oco-dev-cli -- serve          # Start the server"
echo "  cargo run -p oco-dev-cli -- run 'request'   # One-shot orchestration"
echo "  cargo test                                   # Run all tests"
