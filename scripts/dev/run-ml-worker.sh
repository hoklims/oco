#!/usr/bin/env bash
set -euo pipefail

cd py/ml-worker
uv run python -m ml_worker.server
