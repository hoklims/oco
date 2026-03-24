#!/usr/bin/env bash
# OCO Hook: Shared session initialization
# Sourced by all hooks to derive a stable, secure session state directory.
# Never executed directly.

# Derive canonical workspace root (git root or realpath of PWD)
WORKSPACE_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd -P)"

# Stable session ID from canonical workspace path
SESSION_ID=$(printf '%s' "$WORKSPACE_ROOT" | md5sum 2>/dev/null | cut -c1-12 || echo "default")

# Secure state directory: prefer XDG_RUNTIME_DIR, fallback to ~/.cache/oco
_STATE_ROOT="${XDG_RUNTIME_DIR:-${HOME}/.cache/oco}"
mkdir -p "$_STATE_ROOT" 2>/dev/null && chmod 700 "$_STATE_ROOT" 2>/dev/null || true

OCO_STATE_DIR="${_STATE_ROOT}/session-${SESSION_ID}"
mkdir -p "$OCO_STATE_DIR" 2>/dev/null && chmod 700 "$OCO_STATE_DIR" 2>/dev/null || true

# Refuse to use if symlinked (basic symlink attack guard)
if [ -L "$OCO_STATE_DIR" ]; then
  OCO_STATE_DIR="/dev/null"
fi
