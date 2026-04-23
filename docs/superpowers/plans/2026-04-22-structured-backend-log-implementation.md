# Structured Backend Log Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace cumulative full-state command log retention with a bounded structured backend log model that remains stable under very large command output.

**Architecture:** Codex command normalization will emit bounded runtime command state plus final summaries instead of retaining cumulative full-output replace history. `MsgStore` and container log-cap behavior will be aligned so raw and normalized retention degrade consistently when compact mode is entered.

**Tech Stack:** Rust, Tokio, Axum WebSocket streams, JSON Patch, SQLx/SQLite, cargo test

---

## File Structure

- Modify: `crates/executors/src/executors/codex/normalize_logs.rs`
  Responsibility: convert Codex command output from cumulative full-state replacement into bounded runtime command state and final summaries.
- Modify: `crates/executors/src/logs/utils/patch.rs`
  Responsibility: define any additional patch helpers needed for command runtime updates and summary events.
- Modify: `crates/utils/src/msg_store.rs`
  Responsibility: bound runtime history retention and preserve only the latest meaningful command state for repeated updates.
- Modify: `crates/services/src/services/container.rs`
  Responsibility: make compact logging mode explicit and align raw history retention with normalized retention behavior.
- Modify: `crates/db/src/models/execution_process_logs.rs`
  Responsibility: keep append-only persistence compatible with structured compact log events if needed.
- Test in:
  - `crates/executors/src/executors/codex/normalize_logs.rs`
  - `crates/utils/src/msg_store.rs`
  - `crates/services/src/services/container.rs`

## Task 1: Lock Down Runtime Command State Boundaries

**Files:**
- Modify: `crates/executors/src/executors/codex/normalize_logs.rs`
- Test: `crates/executors/src/executors/codex/normalize_logs.rs`

- [ ] **Step 1: Write failing tests for bounded running command state**

Add tests that prove repeated `ExecCommandOutputDelta` handling never retains cumulative full command output beyond the configured command-state limits, and that final command summaries retain truncated-byte markers plus recent tail.

- [ ] **Step 2: Run tests to verify the new cases fail**

Run: `cargo test -p executors codex::normalize_logs -- --nocapture`
Expected: new tests fail because current normalization still treats running output as full mutable state or because missing helpers prevent representing the new bounded state.

- [ ] **Step 3: Implement bounded command runtime state**

Change `CommandState` in `crates/executors/src/executors/codex/normalize_logs.rs` so it stores bounded tails and explicit truncated-byte counts for running output. Ensure repeated output deltas update a bounded state object instead of growing an unbounded string.

- [ ] **Step 4: Emit final command summaries from bounded state**

Update command-finish handling so the final visible summary includes exit status plus retained tail and truncation markers, without requiring full historical output to still exist in memory.

- [ ] **Step 5: Run tests to verify the task passes**

Run: `cargo test -p executors codex::normalize_logs -- --nocapture`
Expected: all Codex normalization tests pass, including the new bounded-state cases.

## Task 2: Formalize Structured Command Log Events

**Files:**
- Modify: `crates/executors/src/logs/utils/patch.rs`
- Modify: `crates/executors/src/executors/codex/normalize_logs.rs`
- Test: `crates/executors/src/executors/codex/normalize_logs.rs`

- [ ] **Step 1: Write failing tests for command lifecycle event semantics**

Add tests showing that command start, running updates, and command finish are represented as bounded lifecycle-oriented updates rather than a long chain of cumulative full-state replacements.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p executors codex::normalize_logs -- --nocapture`
Expected: new lifecycle tests fail because patch helpers and event semantics still mirror the old replace-heavy model.

- [ ] **Step 3: Add patch helpers for structured command updates**

Extend `crates/executors/src/logs/utils/patch.rs` with explicit helpers for the command runtime update pattern used by Codex normalization. Keep compatibility with existing patch transport and existing patch extraction behavior where possible.

- [ ] **Step 4: Switch Codex command output handling to the new helpers**

Update `crates/executors/src/executors/codex/normalize_logs.rs` so command output deltas use the new structured update path. Preserve start and finish semantics and keep other tool event handling unchanged.

- [ ] **Step 5: Run tests to verify the task passes**

Run: `cargo test -p executors codex::normalize_logs -- --nocapture`
Expected: lifecycle tests pass and existing Codex log normalization tests remain green.

## Task 3: Make `MsgStore` Retention Match Structured Runtime State

**Files:**
- Modify: `crates/utils/src/msg_store.rs`
- Test: `crates/utils/src/msg_store.rs`

- [ ] **Step 1: Write failing tests for repeated command-state updates**

Add tests showing that repeated updates to the same logical command state do not accumulate historical copies in `MsgStore` once the system has already retained a newer equivalent state.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --workspace msg_store -- --nocapture`
Expected: new tests fail because repeated runtime state updates still consume too much history or because equivalence rules are incomplete.

- [ ] **Step 3: Implement retention rules for repeated structured command updates**

Adjust `crates/utils/src/msg_store.rs` so `MsgStore` keeps the latest meaningful structured command state while dropping obsolete intermediate state for the same logical entry. Preserve correctness for append-only events that are not safe to compact.

- [ ] **Step 4: Re-check raw/stdout retention interaction**

Ensure `disable_raw_history_retention()` still drops future raw history, but no longer creates a situation where large structured runtime updates become the dominant history consumer.

- [ ] **Step 5: Run tests to verify the task passes**

Run: `cargo test --workspace msg_store -- --nocapture`
Expected: all `msg_store` tests pass, including the new repeated-command-state retention cases.

## Task 4: Promote Compact Logging Mode to an Explicit Backend Policy

**Files:**
- Modify: `crates/services/src/services/container.rs`
- Test: `crates/services/src/services/container.rs`

- [ ] **Step 1: Write failing tests for compact-mode transitions**

Add tests that prove once raw persistence reaches the configured DB cap, the backend enters a consistent compact mode rather than only disabling raw history retention.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p services container -- --nocapture`
Expected: new compact-mode tests fail because current behavior only toggles raw history retention and does not explicitly align normalized retention or runtime mode semantics.

- [ ] **Step 3: Implement explicit compact logging mode in container log persistence**

Update `crates/services/src/services/container.rs` so reaching the persistence cap becomes a first-class state transition. Use that transition to coordinate raw history retention, normalized retention behavior, and any explicit system messages needed for downstream replay.

- [ ] **Step 4: Preserve append-only persistence semantics**

Keep DB writes append-only. If additional system markers are needed, write them as append-only log records instead of mutating previous data.

- [ ] **Step 5: Run tests to verify the task passes**

Run: `cargo test -p services container -- --nocapture`
Expected: compact-mode tests pass and no existing container log tests regress.

## Task 5: Keep Historical Replay Compatible With Old and New Log Shapes

**Files:**
- Modify: `crates/services/src/services/container.rs`
- Modify: `crates/db/src/models/execution_process_logs.rs`
- Test: `crates/services/src/services/container.rs`

- [ ] **Step 1: Write failing tests for dual-format replay**

Add replay tests covering:
- old raw/patch history
- new structured compact history
- command-final-summary replay after compact mode

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p services container -- --nocapture`
Expected: dual-format replay tests fail because replay logic only understands the old shape or incorrectly reconstructs new compact events.

- [ ] **Step 3: Implement dual-format replay support**

Update replay logic in `crates/services/src/services/container.rs` and any parsing helpers in `crates/db/src/models/execution_process_logs.rs` so old logs continue to replay, while new compact structured logs replay to bounded useful history.

- [ ] **Step 4: Verify degraded replay semantics are explicit**

Ensure replay for compact-mode executions clearly preserves:
- command start
- recent tail
- truncation markers
- exit status
- final summary

- [ ] **Step 5: Run tests to verify the task passes**

Run: `cargo test -p services container -- --nocapture`
Expected: replay tests pass for both old and new log formats.

## Task 6: End-to-End Regression Protection for High-Output Commands

**Files:**
- Modify: `crates/services/src/services/container.rs`
- Modify: `crates/utils/src/msg_store.rs`
- Test: `crates/services/src/services/container.rs`
- Test: `crates/utils/src/msg_store.rs`
- Test: `crates/executors/src/executors/codex/normalize_logs.rs`

- [ ] **Step 1: Write failing regression tests for high-output command pressure**

Add an end-to-end-style regression test or tightly-scoped simulation proving that many command output deltas do not push `MsgStore` history usage into the old amplification pattern after compact mode engages.

- [ ] **Step 2: Run tests to verify they fail**

Run:
- `cargo test --workspace msg_store -- --nocapture`
- `cargo test -p executors codex::normalize_logs -- --nocapture`
- `cargo test -p services container -- --nocapture`

Expected: at least one regression test fails before the final wiring is complete.

- [ ] **Step 3: Wire the final cross-layer behavior**

Connect the final normalization, history retention, and compact-mode policy so the simulated high-output command exercises the new bounded path end-to-end.

- [ ] **Step 4: Run targeted verification**

Run:
- `cargo test --workspace msg_store -- --nocapture`
- `cargo test -p executors codex::normalize_logs -- --nocapture`
- `cargo test -p services container -- --nocapture`

Expected: all targeted tests pass.

- [ ] **Step 5: Run a final broader verification pass**

Run: `cargo test --workspace -- --nocapture`
Expected: workspace tests pass or, if there are unrelated pre-existing failures, they are identified explicitly with no failures attributable to this work.

## Self-Review

Spec coverage:
- runtime memory amplification: covered in Tasks 1, 3, and 6
- compact logging mode: covered in Task 4
- dual-format compatibility: covered in Task 5
- append-only persistence: covered in Tasks 4 and 5

Placeholder scan:
- No `TODO`, `TBD`, or implicit "handle appropriately" steps remain.

Type consistency:
- Plan consistently refers to bounded command runtime state, structured command updates, compact mode, and dual-format replay.
