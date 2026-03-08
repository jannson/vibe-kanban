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
