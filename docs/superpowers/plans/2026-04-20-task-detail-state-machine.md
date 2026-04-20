# Task Detail State Machine Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the task detail page treat closed tasks as read-only by default, and allow branch preparation only through an explicit, durable “resume task” flow.

**Architecture:** Replace scattered local booleans with a single task-detail state machine shared by the task page, follow-up area, and aux panels. Move “resume task” from an overloaded `branch-status?resume=true` convention to a dedicated backend endpoint, then gate every workspace-preparing route on the same closed-task rule so viewing history can never switch branches.

**Tech Stack:** React + TypeScript, TanStack Query, Rust + Axum, shared TS types via `ts-rs`

---

## File Map

**Create**
- `frontend/src/features/task-details/state-machine.ts` — central state derivation for closed/open task detail behavior.
- `frontend/src/features/task-details/state-machine.types.ts` — state and capability types used across page/panels.
- `frontend/src/hooks/useResumeTaskAttempt.ts` — dedicated resume mutation hook.

**Modify**
- `frontend/src/pages/ProjectTasks.tsx` — replace local unlock booleans with state machine + resume mutation.
- `frontend/src/components/panels/TaskAttemptPanel.tsx` — consume explicit capabilities instead of ad hoc booleans.
- `frontend/src/components/tasks/TaskFollowUpSection.tsx` — read branch-status policy from derived capabilities.
- `frontend/src/hooks/useBranchStatus.ts` — support a stricter access mode contract.
- `frontend/src/lib/api.ts` — add `resumeTaskAttempt` API; stop using `branch-status` as resume side effect.
- `frontend/src/components/panels/AttemptHeaderActions.tsx` — disable/normalize view toggles when task is locked.
- `crates/server/src/routes/task_attempts.rs` — add `POST /task-attempts/{id}/resume`; gate branch-status, diff WS, and open-editor with a shared helper.
- `shared/types.ts` — regenerated only.
- `crates/server/src/bin/generate_types.rs` — export new resume response type if needed.

**Validation**
- `frontend`: `pnpm run check`
- repo root: `cargo check -p server`
- repo root: `pnpm run generate-types`

---

### Task 1: Define the task-detail state machine

**Files:**
- Create: `frontend/src/features/task-details/state-machine.types.ts`
- Create: `frontend/src/features/task-details/state-machine.ts`
- Modify: `frontend/src/pages/ProjectTasks.tsx`

- [ ] **Step 1: Add explicit state and capability types**

Create `frontend/src/features/task-details/state-machine.types.ts` with the following core model:

```ts
import type { TaskStatus } from 'shared/types';
import type { LayoutMode } from '@/components/layout/TasksLayout';

export type TaskDetailStateId =
  | 'no_task'
  | 'open_task'
  | 'closed_locked'
  | 'closed_resuming'
  | 'closed_resumed';

export interface TaskDetailStateInput {
  taskStatus: TaskStatus | null;
  hasAttempt: boolean;
  requestedView: LayoutMode;
  resumePending: boolean;
  resumeSucceeded: boolean;
}

export interface TaskDetailCapabilities {
  state: TaskDetailStateId;
  isClosedTask: boolean;
  canShowFollowUp: boolean;
  canPollBranchStatus: boolean;
  canPrepareWorkspace: boolean;
  canShowPreview: boolean;
  canShowDiffs: boolean;
  effectiveView: LayoutMode;
  shouldShowResumeCard: boolean;
}
```

- [ ] **Step 2: Implement the pure derivation function**

Create `frontend/src/features/task-details/state-machine.ts`:

```ts
import type { TaskDetailCapabilities, TaskDetailStateInput } from './state-machine.types';

const CLOSED_STATUSES = new Set(['done', 'cancelled'] as const);

export function deriveTaskDetailCapabilities(
  input: TaskDetailStateInput
): TaskDetailCapabilities {
  const isClosedTask =
    input.taskStatus !== null && CLOSED_STATUSES.has(input.taskStatus as 'done' | 'cancelled');

  if (input.taskStatus === null || !input.hasAttempt) {
    return {
      state: 'no_task',
      isClosedTask,
      canShowFollowUp: false,
      canPollBranchStatus: false,
      canPrepareWorkspace: false,
      canShowPreview: false,
      canShowDiffs: false,
      effectiveView: null,
      shouldShowResumeCard: false,
    };
  }

  if (!isClosedTask) {
    return {
      state: 'open_task',
      isClosedTask: false,
      canShowFollowUp: true,
      canPollBranchStatus: true,
      canPrepareWorkspace: true,
      canShowPreview: true,
      canShowDiffs: true,
      effectiveView: input.requestedView,
      shouldShowResumeCard: false,
    };
  }

  if (input.resumePending) {
    return {
      state: 'closed_resuming',
      isClosedTask: true,
      canShowFollowUp: false,
      canPollBranchStatus: false,
      canPrepareWorkspace: false,
      canShowPreview: false,
      canShowDiffs: false,
      effectiveView: null,
      shouldShowResumeCard: true,
    };
  }

  if (input.resumeSucceeded) {
    return {
      state: 'closed_resumed',
      isClosedTask: true,
      canShowFollowUp: true,
      canPollBranchStatus: true,
      canPrepareWorkspace: true,
      canShowPreview: true,
      canShowDiffs: true,
      effectiveView: input.requestedView,
      shouldShowResumeCard: false,
    };
  }

  return {
    state: 'closed_locked',
    isClosedTask: true,
    canShowFollowUp: false,
    canPollBranchStatus: false,
    canPrepareWorkspace: false,
    canShowPreview: false,
    canShowDiffs: false,
    effectiveView: null,
    shouldShowResumeCard: true,
  };
}
```

- [ ] **Step 3: Replace ad hoc booleans in `ProjectTasks`**

In `frontend/src/pages/ProjectTasks.tsx`, replace:
- `requiresExplicitResume`
- `isResumeUnlocked`
- `isFollowUpEnabled`
- `shouldResumePreparedWorkspace`
- local `effectiveMode` hacks

with one derived object:

```ts
const capabilities = deriveTaskDetailCapabilities({
  taskStatus: selectedTask?.status ?? null,
  hasAttempt: !!attempt,
  requestedView: mode,
  resumePending: resumeMutation.isPending,
  resumeSucceeded: hasExplicitResume,
});
```

Use only `capabilities.*` values downstream.

- [ ] **Step 4: Run frontend typecheck**

Run: `cd frontend && pnpm run check`
Expected: `tsc --noEmit` exits successfully.

---

### Task 2: Replace implicit resume with a dedicated backend endpoint

**Files:**
- Modify: `crates/server/src/routes/task_attempts.rs`
- Modify: `frontend/src/lib/api.ts`
- Modify: `frontend/src/hooks/useResumeTaskAttempt.ts`
- Modify: `crates/server/src/bin/generate_types.rs`
- Regenerate: `shared/types.ts`

- [ ] **Step 1: Add explicit resume response type**

In `crates/server/src/routes/task_attempts.rs`, add:

```rust
#[derive(Debug, Serialize, TS)]
pub struct ResumeTaskAttemptResponse {
    pub workspace_id: Uuid,
    pub task_id: Uuid,
    pub task_status: TaskStatus,
    pub branch_status: Vec<RepoBranchStatus>,
}
```

- [ ] **Step 2: Add a shared closed-task guard helper**

In the same file, add a helper above the route handlers:

```rust
async fn load_parent_task_for_workspace(
    pool: &SqlitePool,
    workspace: &Workspace,
) -> Result<Task, ApiError> {
    workspace
        .parent_task(pool)
        .await?
        .ok_or(ApiError::Workspace(WorkspaceError::TaskNotFound))
}

fn is_closed_task_status(status: &TaskStatus) -> bool {
    matches!(status, TaskStatus::Done | TaskStatus::Cancelled)
}
```

- [ ] **Step 3: Add `POST /task-attempts/{id}/resume`**

Implement a new handler that:
- loads parent task
- verifies the task is `Done` or `Cancelled`
- runs `ensure_container_exists(&workspace)`
- computes branch status once
- returns `ResumeTaskAttemptResponse`

Use this shape:

```rust
pub async fn resume_task_attempt(
    Extension(workspace): Extension<Workspace>,
    State(deployment): State<DeploymentImpl>,
) -> Result<ResponseJson<ApiResponse<ResumeTaskAttemptResponse>>, ApiError> {
    let pool = &deployment.db().pool;
    let task = load_parent_task_for_workspace(pool, &workspace).await?;

    if !is_closed_task_status(&task.status) {
        return Err(ApiError::BadRequest(
            "Only completed or cancelled tasks can be resumed".to_string(),
        ));
    }

    let _container_ref = deployment.container().ensure_container_exists(&workspace).await?;
    let branch_status = build_repo_branch_status(&deployment, &workspace).await?;

    Ok(ResponseJson(ApiResponse::success(ResumeTaskAttemptResponse {
        workspace_id: workspace.id,
        task_id: task.id,
        task_status: task.status,
        branch_status,
    })))
}
```

Hook it into the router as:

```rust
.route("/resume", post(resume_task_attempt))
```

- [ ] **Step 4: Expose the new frontend API and hook**

In `frontend/src/lib/api.ts` add:

```ts
resumeTaskAttempt: async (
  attemptId: string
): Promise<ResumeTaskAttemptResponse> => {
  const response = await makeRequest(`/api/task-attempts/${attemptId}/resume`, {
    method: 'POST',
  });
  return handleApiResponse<ResumeTaskAttemptResponse>(response);
},
```

Create `frontend/src/hooks/useResumeTaskAttempt.ts`:

```ts
import { useMutation } from '@tanstack/react-query';
import { attemptsApi } from '@/lib/api';

export function useResumeTaskAttempt() {
  return useMutation({
    mutationFn: (attemptId: string) => attemptsApi.resumeTaskAttempt(attemptId),
  });
}
```

- [ ] **Step 5: Generate shared types and compile backend**

Run:
- `pnpm run generate-types`
- `cargo check -p server`

Expected: types regenerate cleanly and the server compiles.

---

### Task 3: Move the page to the explicit resume workflow

**Files:**
- Modify: `frontend/src/pages/ProjectTasks.tsx`
- Modify: `frontend/src/components/panels/TaskAttemptPanel.tsx`
- Modify: `frontend/src/components/tasks/TaskFollowUpSection.tsx`
- Modify: `frontend/src/hooks/useBranchStatus.ts`

- [ ] **Step 1: Remove resume side effects from `useBranchStatus` usage**

Keep `useBranchStatus` as a pure status hook. Its only API should be:

```ts
export function useBranchStatus(
  attemptId?: string,
  options?: { enabled?: boolean }
)
```

Its implementation should go back to:

```ts
queryKey: ['branchStatus', attemptId],
queryFn: () => attemptsApi.getBranchStatus(attemptId!),
```

The hook must not carry resume state anymore.

- [ ] **Step 2: Replace `isResumeUnlocked` with mutation result state**

In `ProjectTasks.tsx` add:

```ts
const resumeMutation = useResumeTaskAttempt();
const [resumedAttemptId, setResumedAttemptId] = useState<string | null>(null);

useEffect(() => {
  setResumedAttemptId(null);
  resumeMutation.reset();
  setResumeError(null);
}, [selectedTask?.id, selectedTask?.status, attempt?.id]);
```

Resume success should do only this:

```ts
await resumeMutation.mutateAsync(attempt.id);
setResumedAttemptId(attempt.id);
```

Then derive:

```ts
const hasExplicitResume = resumedAttemptId === attempt?.id;
```

- [ ] **Step 3: Gate all view mounts from the state machine**

Use `capabilities` to enforce these rules:
- `TaskAttemptPanel.showFollowUp = capabilities.canShowFollowUp`
- `gitEnabled = capabilities.canPrepareWorkspace`
- `AttemptHeaderActions.mode = capabilities.effectiveView`
- `auxContent` only mounts `PreviewPanel` or `DiffsPanelContainer` when allowed
- resume guard card shows only when `capabilities.shouldShowResumeCard`

Apply this exact shape:

```tsx
<TaskAttemptPanel
  attempt={attempt}
  task={selectedTask}
  gitEnabled={!isSubtaskOriginalNoGit && capabilities.canPrepareWorkspace}
  showFollowUp={capabilities.canShowFollowUp}
>
```

and:

```tsx
{capabilities.effectiveView === 'preview' && <PreviewPanel />}
{capabilities.effectiveView === 'diffs' && (
  <DiffsPanelContainer
    attempt={attempt}
    selectedTask={selectedTask}
    branchStatus={branchStatus ?? null}
  />
)}
```

- [ ] **Step 4: Keep resume action dedicated**

In `handleContinueCompletedTask`, stop calling `getBranchStatus(..., { resume: true })` and switch to:

```ts
await resumeMutation.mutateAsync(attempt.id);
setResumedAttemptId(attempt.id);
```

- [ ] **Step 5: Run frontend typecheck**

Run: `cd frontend && pnpm run check`
Expected: passes with no TS errors.

---

### Task 4: Add server-side business guards for all read-only routes

**Files:**
- Modify: `crates/server/src/routes/task_attempts.rs`
- Modify: `crates/server/src/routes/sessions/mod.rs`

- [ ] **Step 1: Centralize “closed task cannot prepare workspace” logic**

Add one helper in `crates/server/src/routes/task_attempts.rs`:

```rust
fn reject_closed_task_workspace_prepare(task: &Task, action: &str) -> Result<(), ApiError> {
    if is_closed_task_status(&task.status) {
        return Err(ApiError::Conflict(format!(
            "This completed task must be explicitly resumed before {}.",
            action
        )));
    }
    Ok(())
}
```

- [ ] **Step 2: Use that helper for read-only-but-dangerous routes**

Guard these handlers before `ensure_container_exists`:
- `get_task_attempt_branch_status`
- `stream_task_attempt_diff_ws`
- `open_task_attempt_in_editor`

Use exact intent labels:

```rust
reject_closed_task_workspace_prepare(&task, "checking branch status")?;
reject_closed_task_workspace_prepare(&task, "streaming diffs")?;
reject_closed_task_workspace_prepare(&task, "opening the editor")?;
```

- [ ] **Step 3: Keep follow-up as the only action route**

Do **not** block `sessions::follow_up`; sending a follow-up after explicit resume is a real action.
Keep the route logs, but remove the temporary `tracing::info!("{:?}", workspace);` dump if it is no longer needed.

- [ ] **Step 4: Compile backend**

Run: `cargo check -p server`
Expected: passes.

---

### Task 5: Smoke-test the business flow end-to-end

**Files:**
- Modify: `docs/superpowers/plans/2026-04-20-task-detail-state-machine.md` (mark completion during execution only)

- [ ] **Step 1: Validate the locked closed-task flow manually**

Manual smoke test:
1. Open a `done` task detail page.
2. Ensure the URL may still contain `?view=diffs`.
3. Confirm the page stays read-only.
4. Confirm follow-up input is hidden behind the resume guard.
5. Confirm preview/diff side panels do not mount.

Expected: no branch switch, no diff stream, no editor open.

- [ ] **Step 2: Validate the explicit resume flow manually**

Manual smoke test:
1. Click `继续任务`.
2. Confirm the server prepares the workspace once.
3. Confirm follow-up UI becomes available.
4. Confirm diffs/preview become available.
5. Send a follow-up.

Expected: branch prepare happens only after explicit resume.

- [ ] **Step 3: Validate with release logs**

Check the production-style logs for a closed task before and after resume.

Before clicking resume, these must **not** appear for a closed task:
- `preparing workspace for branch status request`
- `diff stream requested`
- `open editor requested`

After clicking resume, a single explicit resume path is expected.

Suggested command:

```bash
grep -E "branch status requested|preparing workspace|diff stream requested|open editor requested|follow-up requested" /tmp/linkease-v2.log | tail -n 80
```

- [ ] **Step 4: Final validation commands**

Run:
- `cd frontend && pnpm run check`
- `cargo check -p server`
- `pnpm run generate-types`

Expected: all commands succeed.
