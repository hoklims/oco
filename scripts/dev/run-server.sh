#!/usr/bin/env bash
set -euo pipefail

# Start the OCO development server with verbose logging
RUST_LOG=${RUST_LOG:-"oco=debug,tower_http=debug"} \
cargo run -p oco-dev-cli -- serve \
    --host "${OCO_HOST:-127.0.0.1}" \
    --port "${OCO_PORT:-3000}" \
    --log-level "${OCO_LOG_LEVEL:-debug}"
