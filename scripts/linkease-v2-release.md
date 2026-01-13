# Linkease v2 release mode notes

Keep the release setup in "release mode" only. Do NOT start the Vite dev server.

Checklist for release-mode publish:
- Build frontend once: `corepack pnpm --dir frontend build` (outputs `frontend/dist`).
- Rebuild server: `cargo build --release --bin server`.
- Copy binary to `/config/vibe-kanban/linkease2/vibe-kanban`.
- `/config/vibe-kanban/linkease-v2.sh` should NOT run `pnpm run dev`.
- `FRONTEND_UPSTREAM` should point to the backend host/port (server serves embedded `frontend/dist`).

If "Development mode" appears in the UI, it means the frontend dev server is running.
