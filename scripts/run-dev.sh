#!/usr/bin/env bash
set -euo pipefail

# Load Cargo and any shell setup (nvm, etc.) if available.
if [ -f "$HOME/.profile" ]; then
  # shellcheck disable=SC1090
  source "$HOME/.profile"
fi

# Bind backend to all interfaces for remote access.
export HOST="${HOST:-0.0.0.0}"

# Fix ports for remote access.
export FRONTEND_PORT="${FRONTEND_PORT:-3002}"
export BACKEND_PORT="${BACKEND_PORT:-3102}"

# Use original repositories instead of worktrees (see config.json workspace_dir).
export VIBE_KANBAN_USE_ORIGINAL_REPOS="${VIBE_KANBAN_USE_ORIGINAL_REPOS:-1}"

# Avoid global git URL rewrites (https -> ssh) when fetching dependencies.
export CARGO_NET_GIT_FETCH_WITH_CLI=true
export GIT_CONFIG_GLOBAL=/dev/null
export GIT_CONFIG_SYSTEM=/dev/null

PNPM_CMD=(pnpm)
if ! command -v pnpm >/dev/null 2>&1; then
  if command -v corepack >/dev/null 2>&1; then
    PNPM_CMD=(corepack pnpm)
  else
    echo "pnpm not found and corepack is unavailable." >&2
    exit 1
  fi
fi

"${PNPM_CMD[@]}" exec concurrently \
  "BACKEND_PORT=${BACKEND_PORT} DISABLE_WORKTREE_ORPHAN_CLEANUP=1 RUST_LOG=debug cargo watch -w crates -x 'run --bin server'" \
  "cd frontend && npm run dev -- --port ${FRONTEND_PORT} --host --strictPort"
