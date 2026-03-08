#!/usr/bin/env bash
set -euo pipefail

SCRIPT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEV_ASSETS_DIR="${SCRIPT_ROOT}/dev_assets"
DEV_DB_PATH="${DEV_ASSETS_DIR}/db.sqlite"
echo "Dev DB path (debug builds): ${DEV_DB_PATH}"
HOME_DIR="${HOME:-}"

# Load Cargo and any shell setup (nvm, etc.) if available.
if [ -n "${HOME_DIR}" ] && [ -f "${HOME_DIR}/.profile" ]; then
  # shellcheck disable=SC1090
  source "${HOME_DIR}/.profile"
fi

# Bind backend to all interfaces for remote access.
export HOST="${HOST:-0.0.0.0}"

# Port helpers.
is_port_in_use() {
  local port="$1"
  ss -ltnH "sport = :${port}" 2>/dev/null | grep -q .
}

print_port_conflict() {
  local name="$1"
  local port="$2"
  echo "Port conflict: ${name} requires :${port}, but it is already in use." >&2
  echo "Current listeners on :${port}:" >&2
  ss -ltnp "sport = :${port}" >&2 || true
}

# Ensure dev config lives in a consistent location.
ORIGINAL_XDG_DATA_HOME="${XDG_DATA_HOME:-}"
DEFAULT_XDG_DATA_HOME="/config/vibe-kanban/repo-dev"
export XDG_DATA_HOME="${XDG_DATA_HOME:-${DEFAULT_XDG_DATA_HOME}}"
if [ -z "${ORIGINAL_XDG_DATA_HOME}" ]; then
  if [ -n "${HOME_DIR}" ]; then
    ORIGINAL_XDG_DATA_HOME="${HOME_DIR}/.local/share"
  else
    ORIGINAL_XDG_DATA_HOME="/tmp"
  fi
fi

OLD_CONFIG_DIR="${ORIGINAL_XDG_DATA_HOME}/vibe-kanban"
NEW_CONFIG_DIR="${XDG_DATA_HOME}/vibe-kanban"

if [ "${OLD_CONFIG_DIR}" != "${NEW_CONFIG_DIR}" ] && [ -d "${OLD_CONFIG_DIR}" ]; then
  mkdir -p "${NEW_CONFIG_DIR}"
  for file in config.json profiles.json credentials.json; do
    if [ -f "${OLD_CONFIG_DIR}/${file}" ] && [ ! -f "${NEW_CONFIG_DIR}/${file}" ]; then
      cp "${OLD_CONFIG_DIR}/${file}" "${NEW_CONFIG_DIR}/${file}"
      echo "Migrated ${file} -> ${NEW_CONFIG_DIR}/${file}"
    fi
  done
fi

# Fix ports for remote access.
# Frontend/backend run behind a local basic-auth proxy by default.
export FRONTEND_PORT="${FRONTEND_PORT:-3032}"
export BACKEND_PORT="${BACKEND_PORT:-3033}"
BACKEND_HOST="${BACKEND_HOST:-${HOST}}"
if [ "${BACKEND_HOST}" = "0.0.0.0" ]; then
  BACKEND_HOST="127.0.0.1"
fi
export BACKEND_HOST

# Use original repositories instead of worktrees (see config.json workspace roots).
#export VIBE_KANBAN_USE_ORIGINAL_REPOS="${VIBE_KANBAN_USE_ORIGINAL_REPOS:-1}"
# Enable auto-commit for dev runs unless explicitly disabled.
# Auto-commit is now configured via system settings.
# Default worktree base path (used when original repos mode is disabled).
export VIBE_KANBAN_WORKTREE_PATH="${VIBE_KANBAN_WORKTREE_PATH:-/projects/workspace-linkease-ubuntu/worktree-repos}"

# Basic auth proxy defaults for dev.
export BASIC_AUTH_USER="${BASIC_AUTH_USER:-admin}"
export BASIC_AUTH_PASS="${BASIC_AUTH_PASS:-admin}"
export PROXY_LISTEN_ADDR="${PROXY_LISTEN_ADDR:-:3002}"
PROXY_PORT="${PROXY_LISTEN_ADDR##*:}"
export FRONTEND_UPSTREAM="${FRONTEND_UPSTREAM:-http://127.0.0.1:${FRONTEND_PORT}}"
export BACKEND_UPSTREAM="${BACKEND_UPSTREAM:-http://127.0.0.1:${BACKEND_PORT}}"

if [ "${FRONTEND_PORT}" = "${BACKEND_PORT}" ] ||
  [ "${FRONTEND_PORT}" = "${PROXY_PORT}" ] ||
  [ "${BACKEND_PORT}" = "${PROXY_PORT}" ]; then
  echo "Port conflict: FRONTEND_PORT (${FRONTEND_PORT}), BACKEND_PORT (${BACKEND_PORT}), and PROXY_LISTEN_ADDR (${PROXY_LISTEN_ADDR}) must be distinct." >&2
  echo "Set FRONTEND_PORT/BACKEND_PORT/PROXY_LISTEN_ADDR to different values and retry." >&2
  exit 1
fi

if is_port_in_use "${FRONTEND_PORT}"; then
  print_port_conflict "frontend" "${FRONTEND_PORT}"
  exit 1
fi
if is_port_in_use "${BACKEND_PORT}"; then
  print_port_conflict "backend" "${BACKEND_PORT}"
  exit 1
fi
if is_port_in_use "${PROXY_PORT}"; then
  print_port_conflict "proxy" "${PROXY_PORT}"
  exit 1
fi

# Avoid global git URL rewrites (https -> ssh) when fetching dependencies.
export CARGO_NET_GIT_FETCH_WITH_CLI=true
export GIT_CONFIG_GLOBAL=/dev/null
export GIT_CONFIG_SYSTEM=/dev/null
export GIT_AUTHOR_NAME="${GIT_AUTHOR_NAME:-Vibe Kanban}"
export GIT_AUTHOR_EMAIL="${GIT_AUTHOR_EMAIL:-noreply@vibekanban.com}"
export GIT_COMMITTER_NAME="${GIT_COMMITTER_NAME:-Vibe Kanban}"
export GIT_COMMITTER_EMAIL="${GIT_COMMITTER_EMAIL:-noreply@vibekanban.com}"

PNPM_CMD=(pnpm)
if ! command -v pnpm >/dev/null 2>&1; then
  if command -v corepack >/dev/null 2>&1; then
    PNPM_CMD=(corepack pnpm)
  else
    echo "pnpm not found and corepack is unavailable." >&2
    exit 1
  fi
fi

if ! command -v go >/dev/null 2>&1; then
  echo "go not found; required to run the basic auth proxy." >&2
  exit 1
fi

# Optional: run checks before starting dev.
# Defaults to ON; set `SKIP_CHECKS=1` to skip.
if [ "${SKIP_CHECKS:-0}" != "1" ]; then
  echo "Running preflight checks (set SKIP_CHECKS=1 to skip)..."
  "${PNPM_CMD[@]}" run -s check
fi

# Optional: run lint before starting dev.
if [ "${RUN_LINT:-0}" = "1" ]; then
  echo "Running lint (set RUN_LINT=0 to skip)..."
  "${PNPM_CMD[@]}" run -s lint
fi

# Optional: run Rust tests before starting dev.
if [ "${RUN_TESTS:-0}" = "1" ]; then
  echo "Running Rust tests (set RUN_TESTS=0 to skip)..."
  cargo test --workspace
fi

echo "Starting dev servers..."

"${PNPM_CMD[@]}" exec concurrently \
  "BACKEND_PORT=${BACKEND_PORT} DISABLE_WORKTREE_ORPHAN_CLEANUP=1 RUST_LOG=debug cargo watch -w crates -x 'run --bin server'" \
  "cd frontend && BACKEND_PORT=${BACKEND_PORT} npm run dev -- --port ${FRONTEND_PORT} --host --strictPort" \
  "cd tools/basic-auth-proxy && go run ."
