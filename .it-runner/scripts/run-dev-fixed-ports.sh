#!/usr/bin/env bash
set -euo pipefail

# Force stable legacy dev ports for it-runner task execution.
export FRONTEND_PORT=3032
export BACKEND_PORT=3033
export PROXY_LISTEN_ADDR=:3002

exec ./scripts/run-dev.sh
