# Task Detail Resume Key Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Keep `Done/Cancelled` task status unchanged while allowing an explicit, current-page-only resume flow controlled by a short-lived `resume_key`.

**Architecture:** Replace the current implicit/unlocked resume logic with a dedicated resume endpoint that returns a transient `resume_key`. Frontend stores that key in page memory only; backend requires it on all workspace-preparing routes for closed tasks. Remove the incorrect `Done -> InProgress` mutation and keep task lifecycle semantics pure.

**Tech Stack:** React + TypeScript, TanStack Query, Rust + Axum, SQLx, `ts-rs`

---

## File Map

**Create**
- `frontend/src/hooks/useResumeTaskAttempt.ts` — resume mutation hook that requests a `resume_key`.
- `frontend/src/features/task-details/state-machine.ts` — pure state derivation for open/closed detail states.
- `frontend/src/features/task-details/state-machine.types.ts` — task detail capability types.

**Modify**
- `frontend/src/pages/ProjectTasks.tsx` — use in-memory `resume_key`, locked/resumed state machine, and no task status mutation assumptions.
- `frontend/src/components/panels/TaskAttemptPanel.tsx` — mount follow-up only when capabilities permit.
- `frontend/src/components/tasks/TaskFollowUpSection.tsx` — pass `resume_key` to follow-up and branch-status consumers when needed.
- `frontend/src/hooks/useBranchStatus.ts` — support optional `resume_key` header/query plumbing.
- `frontend/src/lib/api.ts` — add `resumeTaskAttempt`, add `resume_key` support to closed-task-sensitive APIs.
- `crates/server/src/routes/task_attempts.rs` — replace current resume implementation, add `resume_key` issuance and validation, remove `Done -> InProgress` mutation, guard closed-task prepare routes.
- `crates/server/src/routes/sessions/mod.rs` — require `resume_key` for follow-up on closed tasks.
- `crates/server/src/bin/generate_types.rs` — export new resume response type.
- `shared/types.ts` — regenerated only.

**Validation**
- `pnpm run generate-types`
- `cd frontend && pnpm run check`
- `cargo check -p server`

---

### Task 1: Correct the backend resume model

**Files:**
- Modify: `crates/server/src/routes/task_attempts.rs`
- Modify: `crates/server/src/bin/generate_types.rs`
- Regenerate: `shared/types.ts`

- [ ] **Step 1: Replace the current `ResumeTaskAttemptResponse` shape**

Update `crates/server/src/routes/task_attempts.rs` so the response includes a transient key instead of implying status transition:

```rust
#[derive(Debug, Serialize, TS)]
pub struct ResumeTaskAttemptResponse {
    pub workspace_id: Uuid,
    pub task_id: Uuid,
    pub task_status: TaskStatus,
    pub resume_key: String,
}
```

- [ ] **Step 2: Remove the incorrect `Done -> InProgress` mutation**

In `resume_task_attempt`, delete this line entirely:

```rust
Task::update_status(pool, task.id, TaskStatus::InProgress).await?;
```

And return the existing task status unchanged:

```rust
task_status: task.status,
```

- [ ] **Step 3: Add transient `resume_key` issuance**

Implement resume key generation in `resume_task_attempt` using a short-lived signed string or opaque UUID stored in memory. The minimal acceptable first version is an in-memory map keyed by random UUID with expiration metadata.

The handler should:
1. verify task is `Done/Cancelled`
2. create `resume_key`
3. store `(workspace_id, task_id, expires_at)` in server memory
4. return `resume_key`

- [ ] **Step 4: Export the new type and regenerate shared types**

Run: `pnpm run generate-types`
Expected: `shared/types.ts` contains `ResumeTaskAttemptResponse` with `resume_key`.

- [ ] **Step 5: Run backend compile check**

Run: `cargo check -p server`
Expected: compile succeeds after removing the task-status mutation path.

---

### Task 2: Add backend validation for closed-task dangerous routes

**Files:**
- Modify: `crates/server/src/routes/task_attempts.rs`
- Modify: `crates/server/src/routes/sessions/mod.rs`

- [ ] **Step 1: Add a reusable closed-task resume validator**

In `crates/server/src/routes/task_attempts.rs`, add a helper with this shape:

```rust
async fn validate_closed_task_resume_key(
    deployment: &DeploymentImpl,
    workspace: &Workspace,
    task: &Task,
    resume_key: Option<&str>,
    action: &str,
) -> Result<(), ApiError>
```

Behavior:
- if task is not `Done/Cancelled`, allow immediately
- if task is closed and `resume_key` missing/invalid/expired, return `ApiError::Conflict`
- otherwise allow

- [ ] **Step 2: Add `resume_key` query/body plumbing to dangerous routes**

Update these routes to accept `resume_key` and validate it before `ensure_container_exists(...)`:
- `GET /task-attempts/:id/branch-status`
- `GET /task-attempts/:id/diff/ws`
- `POST /task-attempts/:id/open-editor`
- `POST /sessions/:id/follow-up`

Use the same business error message everywhere:

```rust
"This completed task must be explicitly resumed before performing this action."
```

- [ ] **Step 3: Keep logs but distinguish resume-key authorization**

Keep current route-level tracing, but add whether validation used a `resume_key`:

```rust
resume_key_present = resume_key.is_some()
```

Do not log the raw key value.

- [ ] **Step 4: Run backend compile check**

Run: `cargo check -p server`
Expected: compile succeeds with new query/body parameters.

---

### Task 3: Move frontend to page-memory-only resume state

**Files:**
- Modify: `frontend/src/pages/ProjectTasks.tsx`
- Modify: `frontend/src/features/task-details/state-machine.ts`
- Modify: `frontend/src/features/task-details/state-machine.types.ts`
- Modify: `frontend/src/hooks/useResumeTaskAttempt.ts`
- Modify: `frontend/src/lib/api.ts`

- [ ] **Step 1: Replace boolean unlocked state with `resumeKey` memory state**

In `frontend/src/pages/ProjectTasks.tsx`, replace the current `resumedAttemptId` notion with:

```ts
const [resumeKey, setResumeKey] = useState<string | null>(null);
```

Reset it on:
- task id change
- task status change
- attempt id change
- page refresh naturally clears it because it is in-memory only

- [ ] **Step 2: Make the state machine depend on `resumeKey` presence**

Change derivation input to:

```ts
resumePending: resumeMutation.isPending,
resumeSucceeded: !!resumeKey,
```

The meaning of `closed_resumed` becomes: this page currently has a valid `resume_key`.

- [ ] **Step 3: On resume, store only the returned key**

In `handleContinueCompletedTask`, change success handling to:

```ts
const result = await resumeMutation.mutateAsync(attempt.id);
setResumeKey(result.resume_key);
```

Do not set any task status assumptions locally.

- [ ] **Step 4: Keep effective view locked until key exists**

The page must continue to derive:
- locked closed tasks → `effectiveView = null`
- resumed closed tasks → requested `view` allowed

This preserves the current fix for `?view=diffs` auto-mounting.

- [ ] **Step 5: Run frontend typecheck**

Run: `cd frontend && pnpm run check`
Expected: typecheck passes.

---

### Task 4: Thread `resume_key` through all closed-task-sensitive frontend APIs

**Files:**
- Modify: `frontend/src/lib/api.ts`
- Modify: `frontend/src/hooks/useBranchStatus.ts`
- Modify: `frontend/src/components/tasks/TaskFollowUpSection.tsx`
- Modify: `frontend/src/pages/ProjectTasks.tsx`

- [ ] **Step 1: Add `resumeKey` parameters to frontend API helpers**

Update API signatures to optionally accept `resumeKey`:

```ts
getBranchStatus(attemptId: string, options?: { resumeKey?: string })
openEditor(attemptId: string, data: OpenEditorRequest & { resume_key?: string })
```

And for follow-up, extend the request body with:

```ts
resume_key?: string
```

- [ ] **Step 2: Update `useBranchStatus` to pass `resumeKey`**

Change the hook to:

```ts
export function useBranchStatus(
  attemptId?: string,
  options?: { enabled?: boolean; resumeKey?: string | null }
)
```

Use query key:

```ts
['branchStatus', attemptId, options?.resumeKey ?? null]
```

and query fn:

```ts
attemptsApi.getBranchStatus(attemptId!, { resumeKey: options?.resumeKey ?? undefined })
```

- [ ] **Step 3: Pass `resumeKey` from `ProjectTasks` and `TaskFollowUpSection`**

`ProjectTasks` should call:

```ts
useBranchStatus(attempt?.id, {
  enabled: canLoadBranchStatus,
  resumeKey,
});
```

`TaskFollowUpSection` should accept a `resumeKey?: string | null` prop and use it in its own branch-status and follow-up-send paths.

- [ ] **Step 4: Ensure refresh re-locks closed tasks**

Confirm there is no `localStorage`, scratch, query cache hydration, or URL field preserving `resumeKey`.

- [ ] **Step 5: Run frontend typecheck**

Run: `cd frontend && pnpm run check`
Expected: passes.

---

### Task 5: Remove temporary/incorrect resume semantics and verify behavior

**Files:**
- Modify: `frontend/src/pages/ProjectTasks.tsx`
- Modify: `crates/server/src/routes/task_attempts.rs`
- Modify: `crates/server/src/routes/sessions/mod.rs`

- [ ] **Step 1: Remove any remaining `resume=true` branch-status flow**

Delete the temporary coupling where “continue task” is implemented by calling `branch-status?resume=true`.
All resume behavior must go through `POST /task-attempts/:id/resume`.

- [ ] **Step 2: Remove any logic that treats resumed closed tasks as reopened tasks**

Specifically verify there is no remaining code that:
- writes `TaskStatus::InProgress` during resume
- derives UI state from “resumed means reopened task”
- changes task badges/labels away from `Done/Cancelled`

- [ ] **Step 3: Validate production behavior mentally against the spec**

Closed task expected behavior:
1. Open details → locked
2. No follow-up, no diff mount, no editor open, no branch prepare
3. Click `继续任务` → confirmation → `resume_key` issued
4. Same page enters resumed state
5. Now branch/diff/editor/follow-up work
6. Refresh page → locked again

- [ ] **Step 4: Final validation commands**

Run all of:
- `pnpm run generate-types`
- `cd frontend && pnpm run check`
- `cargo check -p server`

Expected: all commands succeed.
