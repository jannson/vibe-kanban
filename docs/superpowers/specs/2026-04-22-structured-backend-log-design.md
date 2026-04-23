# Structured Backend Log Design

## Goal

Rework the backend log pipeline so high-output tasks cannot exhaust server memory when SQLite log limits are reached. The system should preserve final outcomes and recent context without storing unbounded command output as runtime state.

## Problem Summary

The current backend log model mixes three concerns in one pipeline:

1. Raw `stdout/stderr` persistence into SQLite
2. Runtime in-memory history in `MsgStore`
3. Normalized log patches for the task UI

The main failure mode is in Codex command normalization. `ExecCommandOutputDelta` appends output to an in-memory command state, then emits a full `replace` patch for the same entry. That creates a sequence of progressively larger patches for the same command.

This problem existed before recent log-cap work, but `d132b7cc` made it much easier to trigger in production:

- SQLite persistence now stops after `execution_log_max_mb`
- Raw history retention in `MsgStore` is disabled once the cap is reached
- Large normalized `replace` patches then dominate the in-memory history budget

The result is that a run with less than 1MB of real command output can consume a large fraction of the 100MB `MsgStore` history budget because the history retains many increasingly large full-state patches.

## Constraints

- Do not depend on frontend-only fixes for stability
- Preserve the ability to stream logs live during execution
- Preserve final conclusions, exit status, and recent relevant output
- Keep DB writes append-only
- Support reading old execution logs without migration

## Requirements

### Functional

1. Running commands must stream meaningful progress to the UI.
2. Final command output must still be visible after completion.
3. High-output commands must not cause unbounded memory growth in backend runtime state.
4. Hitting SQLite log caps must not silently move the problem into `MsgStore`.
5. Historical log replay must remain available for existing tasks.

### Non-Functional

1. Memory usage must scale roughly linearly with retained tail and summary size, not with the number of cumulative `replace` events.
2. Runtime history retention must have explicit per-command and per-execution bounds.
3. The new design must degrade predictably under high output.

## Approaches Considered

### Approach A: Keep current patch model, add tighter caps

Keep `replace` patches but clamp command state more aggressively and reduce history retention.

Pros:
- Smallest code change
- Low migration risk

Cons:
- The protocol still models output as mutable full state
- Future regressions remain likely
- Runtime semantics stay inconsistent across raw and normalized layers

### Approach B: Split command lifecycle state from command output events

Model command execution as small lifecycle entries plus separate output delta events and final summaries.

Pros:
- Eliminates the full-state `replace` amplification pattern
- Makes memory behavior predictable
- Clean separation between live output and final command state

Cons:
- Requires moderate refactoring in backend normalization and replay
- Needs dual-format compatibility while old logs remain

### Approach C: Move raw logs to file-based spool storage

Use disk files for large output, keep only indices and summaries in memory and DB.

Pros:
- Strongest protection against large outputs
- Best long-term for extremely noisy tasks

Cons:
- Highest operational complexity
- Requires file lifecycle, cleanup, and path/security handling
- Larger scope than needed for the current incident

## Recommendation

Use Approach B.

It directly addresses the backend memory failure without introducing new storage infrastructure. It also creates a clearer contract for what the system guarantees:

- live progress
- bounded recent output
- durable final result

instead of trying to preserve an ever-growing mutable command entry.

## Proposed Architecture

### 1. Raw Layer

Raw `Stdout/Stderr` messages remain append-only inputs from executors. They are no longer treated as long-lived runtime history after caps are reached. Instead, they feed bounded tail buffers and summary generation.

### 2. Runtime Normalized Layer

Replace the current "single command entry repeatedly replaced with accumulated output" model with a command event model:

- `command_started`
- `command_output_delta`
- `command_finished`

Runtime state for a command should store only:

- command text
- current status
- exit code if available
- bounded stdout tail
- bounded stderr tail
- truncated byte counts

`command_output_delta` events are broadcast live but are not retained as unbounded history.

### 3. Historical Replay Layer

Persist enough structured events to reconstruct a compact but useful history:

- command start
- periodic compact output chunks or bounded tail updates
- command finished summary

Historical replay should prefer final summary plus retained tail, not every intermediate full-state mutation.

## Compact Logging Mode

The current "DB cap reached" behavior should become a first-class execution mode.

When raw persistence reaches `execution_log_max_mb`, the execution enters compact logging mode:

- raw DB persistence switches to truncation marker + bounded tail behavior
- runtime raw history retention remains disabled
- normalized runtime history switches to summary-first retention
- live clients continue receiving output deltas, but only bounded tail and lifecycle state are kept in history

This makes the system behavior explicit and consistent instead of relying on side effects.

## Data Model Changes

### Command Runtime State

Codex normalization should keep a bounded command state object in memory, not an ever-growing full output string.

Recommended fields:

- `command`
- `status`
- `exit_code`
- `stdout_tail`
- `stderr_tail`
- `stdout_truncated_bytes`
- `stderr_truncated_bytes`
- `final_formatted_output_tail`

### Normalized Patch Types

Extend normalized patch handling to support dedicated command events rather than encoding command progress exclusively through repeated `replace` of a `ToolUse` entry.

The exact wire shape can remain within the existing patch transport, but backend semantics should distinguish:

- state entry creation/update
- output delta event
- final summary event

## Compatibility Plan

### Read Path

- Old execution logs continue to replay through existing parsing logic.
- New execution logs use the structured event model.
- Replay code must support both formats concurrently.

### Write Path

- New executions write the new compact lifecycle-oriented format.
- No migration of old DB rows is required.

## Error Handling

- If normalization fails, fall back to bounded raw->patch replay instead of full-state replay.
- If compact mode is entered, emit an explicit system event so the user understands that middle output was compressed.
- If tail buffers overflow, preserve the newest output and record omitted byte counts.

## Testing Strategy

### Unit Tests

- command output deltas do not grow retained runtime state beyond configured bounds
- compact mode disables raw history retention and switches normalized retention policy
- final summaries preserve exit status and retained tail
- old-format replay still works

### Integration Tests

- simulate long-running Codex command output with many deltas
- confirm `MsgStore` total bytes remain bounded after DB cap is hit
- confirm replay returns final summary and tail for completed execution

## Rollout Strategy

Phase 1:
- Introduce bounded command runtime state
- Stop retaining cumulative replace history for command output

Phase 2:
- Introduce explicit compact logging mode
- Persist compact lifecycle events for new executions

Phase 3:
- Clean up old fallback paths once dual-format replay is proven stable

## Open Decisions Resolved

- Running commands may no longer show full accumulated output at every instant. This is intentional.
- The system guarantee is now "live progress plus bounded recent context plus final result", not "full live transcript in memory".
- Frontend changes are optional follow-up work, not required for backend stability.
