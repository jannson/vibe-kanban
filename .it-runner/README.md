# it-runner for vibe-kanban

This repository contains a project-local `.it-runner/` workspace for running common tasks in it-runner Web UI.

## Project entry

Entry file: `.it-runner/project.yaml`

- `name`: stable project key (`vibe-kanban`)
- `tasksDir`: task definitions root (`.it-runner/tasks`)
- `logsDir`: task logs directory (`${DATA_ROOT}/logs`)
- `cacheDir`: task cache directory (`${DATA_ROOT}/cache`)
- `envFiles`: dotenv files loaded by it-runner (missing files are ignored)
  - `.it-runner/envs/000-defaults.env`
  - `.it-runner/envs/080-secret-local.env` (optional local file)
  - `.it-runner/envs/010-local.env` (optional local override, highest priority)

## Path variables

- `PROJECT_ROOT`: repository root path injected by it-runner at runtime.
- `DATA_ROOT`: external data root path (set in `.it-runner/envs/000-defaults.env`, can be locally overridden).
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
- `run-dev.sh` runs `check` before startup by default; set `SKIP_CHECKS=1` in `.it-runner/envs/010-local.env` if you want faster startup.
- Default ports stay fixed (`FRONTEND_PORT=3032`, `BACKEND_PORT=3033`, `PROXY_LISTEN_ADDR=:3002`); if occupied, the script now exits early with an explicit conflict message.
- These defaults now come from `.it-runner/envsets/task-runtimes/dev/<profile>/`, and `dev` selects its profile via `.it-runner/tasks/dev/envs/000-defaults.env`.

## Release task

- `linkease-v2-release`: release-mode publish from `scripts/linkease-v2-release.md`
  - Build frontend once (`corepack pnpm --dir frontend build`)
  - Build backend release binary (`cargo build --release --bin server`)
  - Install binary to `/config/vibe-kanban/linkease2/vibe-kanban`
  - Refuse to run when a Vite dev server is detected

## Task-centric patterns in this repo

- `dev`
  - default selector: `.it-runner/tasks/dev/envs/000-defaults.env`
  - runtime profile: `.it-runner/envsets/task-runtimes/dev/<profile>/`
- `remote-dev`
  - default selector: `.it-runner/tasks/remote-dev/envs/000-defaults.env`
  - runtime profile: `.it-runner/envsets/task-runtimes/remote-dev/<profile>/`
- `linkease-v2-release`
  - default selector: `.it-runner/tasks/linkease-v2-release/envs/000-defaults.env`
  - runtime profile: `.it-runner/envsets/task-runtimes/linkease-v2-release/<profile>/`

## Minimal task fields

Every task should include at least:

- `name`
- `version` (`"1"`)
- execution block (`run.cmds` for short tasks, or `processes` for long-running tasks)

Shared defaults are in `.it-runner/tasks/_includes/base.yaml` and are merged with `include:`.

## Git ignore and local-only files

Do not commit local secrets or local overrides:

- `.it-runner/envs/080-secret-local.env`
- `.it-runner/tasks/**/task.local.yaml`
- `.it-runner/logs/` and `.it-runner/cache/` when using repo-local storage

This repo keeps these patterns in `.it-runner/.gitignore`.
