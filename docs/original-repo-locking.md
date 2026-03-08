# Original Repo Mutual Exclusion

This document defines how we detect and prevent conflicts when multiple tasks try to use the same repository in `use_original_repos` mode.

**Scope**
These rules apply only to `use_original_repos = true` workspaces. Worktree mode is the default and safe fallback.

**Definitions**
- **Repository**: A repo associated to a workspace through `workspace_repos` and identified by `repo_id`.
- **Active Task**: A task attempt whose task status is `In Progress` or `In Review`.
- **Original Repo Workspace**: A workspace with `use_original_repos = true`.
- **Repo Occupied**: A repository that is already being used by an Active Task in Original Repo mode.

**DB-Based Lock Rule**
The original repo is considered occupied for a given `repo_id` if there exists any workspace that matches:
- `use_original_repos = true`
- The workspace is linked to the same `repo_id` via `workspace_repos`
- The workspace's task status is `In Progress` or `In Review`

If the repo is occupied, creating another `use_original_repos = true` workspace for the same `repo_id` must be blocked.

**Default Mode**
The default mode is controlled by config:
- `default_use_original_repos = false` means default to worktree.
- `default_use_original_repos = true` means default to original repo.
Environment variable `VIBE_KANBAN_USE_ORIGINAL_REPOS` is not used for defaults.

**Branch Prefix Check (Block Only When Dirty)**
If DB detection does not show occupancy and the current HEAD branch of the original repo matches the configured `git_branch_prefix` (for example `vk/*`), allow creation unless the repo is dirty. If there are uncommitted or untracked changes, block creation with an error that indicates the branch matches the reserved prefix and has local changes.

**Behavior Summary**
1. Default is worktree unless `default_use_original_repos = true`.
2. DB rule is the hard gate for `use_original_repos` per repo.
3. Branch prefix match only blocks when the repo is dirty (uncommitted or untracked changes).
4. Worktree mode is the recommended fallback when a repo is occupied or blocked by local changes.

## Subtask Original-Repo No-Git Mode (Planned)

**Requirement**
- Subtasks should be allowed to run without worktree (`use_original_repos = true`) in the current main project directory.
- In this mode, the subtask must not perform git operations.
- The UI must not show git operation/status prompts for that subtask attempt.

**Design Decision**
- Keep database schema unchanged.
- Detect the mode from existing fields:
  - `workspace.use_original_repos = true`
  - parent task exists and `task.parent_workspace_id IS NOT NULL` (subtask)
- Treat this combination as a special runtime mode: **subtask original-repo no-git**.

**Backend Behavior**
- Allow creation of subtask attempts in original repo mode even if the repo is already occupied by another original-repo attempt.
- For subtask original-repo no-git mode:
  - skip branch checkout/creation in original repo path setup
  - skip auto-commit
  - block git endpoints (commit/merge/push/rebase/rename branch/change target branch/abort conflicts/pr creation+attachment+comments)
  - return empty branch status

**Frontend Behavior**
- Remove the forced worktree restriction for subtasks.
- For subtask original-repo no-git mode:
  - hide git actions/toolbar entry points
  - disable branch-status polling and conflict UI that depends on it
  - do not show git-related prompts in the subtask flow

**Risk Assessment**
- Main risk remains concurrent edits in the same working directory; this mode intentionally delegates conflict avoidance to users.
- Change scope is moderate (frontend + backend route guards + container behavior), with low migration risk due to no DB changes.

## Loading History Slow: Root Cause and Impact Analysis

**Observed Symptom**
- Opening task attempt detail shows `Loading History` for a long time.
- User perception is "Git is stuck" while viewing history.

**Confirmed Call Chain**
1. Frontend `VirtualizedList` enters loading state and waits for history stream completion.
2. `useConversationHistory` sequentially loads historic execution processes.
3. Backend `stream_normalized_logs` uses DB fallback for processes not in memory.
4. DB fallback calls `ensure_container_exists(workspace)` before normalization.
5. In original-repo/worktree paths, `ensure_container_exists` may run branch checkout/create (`git checkout_branch_or_create`) and workspace restoration.

This means "view history" can trigger workspace/Git operations indirectly.

**Why it appears only on some projects/tasks**
- Only specific execution processes hit normalized-log DB fallback.
- If workspace/container is cold or missing, `ensure_container_exists` does real recovery work.
- If repo state is large/slow/locked, checkout/restore latency is visible as prolonged `Loading History`.

## If We Remove `ensure_container_exists` From History Path

### Direct Benefits
- History loading no longer blocks on workspace restore or Git checkout.
- Significantly lower latency variance for task detail page.
- Better isolation: "read history" remains read-only behavior.

### Negative Consequences
1. **Normalization may use invalid working directory**
- Current code computes effective dir from workspace path after `ensure_container_exists`.
- Without restoration, that path can be empty/nonexistent/stale.
- Some executor normalizers rely on repo-relative context and may produce incomplete or degraded normalized entries.

2. **Historic replay can fail for old attempts with missing workspace refs**
- For attempts whose `container_ref` is gone, normalization may fail early.
- Result: empty history or partial history for those attempts.

3. **Behavior divergence between in-memory and DB fallback**
- In-memory path may still work while fallback path degrades, causing inconsistent UX across attempts.

4. **Potential loss of patch fidelity**
- Features dependent on normalization metadata (tool blocks, structured entries, file-related patches) can degrade when effective dir context is unavailable.

### Recommendation
- Do **not** blindly remove `ensure_container_exists`.
- Prefer introducing a **history-safe mode**:
  - default: skip workspace/Git restore on history read
  - attempt normalization with best-effort current dir
  - if normalization requires workspace context and fails, return a degraded but fast stream (raw/stdout-stderr-based entries) instead of blocking
  - add trace logs for fallback cause and timing

This keeps history page responsive while preserving compatibility for entries that truly need workspace context.

## Implemented Minimal Guard (Current)

- `stream_normalized_logs` DB fallback now has a fast-path behavior:
  - default skips `ensure_container_exists` during history read
  - if workspace dir or executor action context is unavailable, fallback to raw->JsonPatch replay instead of returning empty/slow-failing
- Environment flag:
  - `VIBE_KANBAN_HISTORY_SKIP_ENSURE_CONTAINER=0` can restore old behavior (call `ensure_container_exists` in history fallback path)
  - unset (default) uses fast-path (skip ensure)
