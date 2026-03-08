# it-runner for vibe-kanban

This repository contains a project-local `.it-runner/` workspace for running common tasks in it-runner Web UI.

## Project entry

Entry file: `.it-runner/project.yaml`

- `name`: stable project key (`vibe-kanban`)
- `tasksDir`: task definitions root (`.it-runner/tasks`)
- `logsDir`: task logs directory (`${DATA_ROOT}/logs`)
- `cacheDir`: task cache directory (`${DATA_ROOT}/cache`)
- `envFiles`: dotenv files loaded by it-runner (missing files are ignored)
  - `.it-runner/envs/shared.env`
  - `.it-runner/envs/secrets.env` (optional local file)
  - `.it-runner/.env.local` (optional local override, highest priority)

## Path variables

- `PROJECT_ROOT`: repository root path injected by it-runner at runtime.
- `DATA_ROOT`: external data root path (set in `.it-runner/envs/shared.env`, can be locally overridden).
  - Current default is `/tmp/vibe-kanban-data` (recommended to change to a persistent local path).

## Task layouts

it-runner supports both layouts:

1. Directory style (recommended): `tasks/<task>/task.yaml`
2. Flat style (compatible): `tasks/<task>.yaml`

This project uses directory style.

## Dev task behavior

`dev` runs `./scripts/run-dev.sh` directly.

- `scripts/run-dev.sh` uses a fixed default `XDG_DATA_HOME` (`/config/vibe-kanban/repo-dev`) unless you explicitly set `XDG_DATA_HOME`.
- Outside it-runner, `scripts/run-dev.sh` keeps the same default path behavior unless you explicitly set `XDG_DATA_HOME`.
- `run-dev.sh` runs `check` before startup by default; set `SKIP_CHECKS=1` in `.it-runner/.env.local` if you want faster startup.
- Default ports stay fixed (`FRONTEND_PORT=3032`, `BACKEND_PORT=3033`, `PROXY_LISTEN_ADDR=:3002`); if occupied, the script now exits early with an explicit conflict message.

## Release task

- `linkease-v2-release`: release-mode publish from `scripts/linkease-v2-release.md`
  - Build frontend once (`corepack pnpm --dir frontend build`)
  - Build backend release binary (`cargo build --release --bin server`)
  - Install binary to `/config/vibe-kanban/linkease2/vibe-kanban`
  - Refuse to run when a Vite dev server is detected

## Minimal task fields

Every task should include at least:

- `name`
- `version` (`"1"`)
- execution block (`run.cmds` for short tasks, or `processes` for long-running tasks)

Shared defaults are in `.it-runner/tasks/_includes/base.yaml` and are merged with `include:`.

## Git ignore and local-only files

Do not commit local secrets or local overrides:

- `.it-runner/envs/secrets.env`
- `.it-runner/tasks/**/task.local.yaml`
- `.it-runner/logs/` and `.it-runner/cache/` when using repo-local storage

This repo keeps these patterns in `.it-runner/.gitignore`.
