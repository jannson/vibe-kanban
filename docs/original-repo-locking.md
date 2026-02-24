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
