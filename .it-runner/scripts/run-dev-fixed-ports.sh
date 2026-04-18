#!/usr/bin/env bash
set -euo pipefail

# Keep stable legacy dev ports by default, but let task env override them.
export FRONTEND_PORT="${FRONTEND_PORT:-3032}"
export BACKEND_PORT="${BACKEND_PORT:-3033}"
export PROXY_LISTEN_ADDR="${PROXY_LISTEN_ADDR:-:3002}"

exec ./scripts/run-dev.sh
