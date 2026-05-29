Goal: # Scheduled Automation Executor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a backend scheduled executor that creates and starts normal Fabro runs from enabled automation schedule triggers.

**Architecture:** Add a deterministic scheduling planner plus a long-lived server task. The planner keeps only in-memory cron cursors, and the executor reuses the existing automation materializer and run creation/start path. This intentionally does not add persistent automation runtime state, public status fields, one-time schedules, event triggers, CLI commands, or API contract changes.

**Tech Stack:** Rust, Tokio, chrono UTC, croner, Axum server internals, existing `fabro-automation`, `fabro-server`, `fabro-workflow`, and SlateDB run store.

---

## Locked Decisions

- Schedule triggers use existing five-field UTC cron expressions only.
- Startup does not backfill missed occurrences from server downtime.
- Each scheduler wake fires each due trigger at most once, then advances that trigger to the next occurrence after the wake time.
- If a previous run from the same automation trigger is still queued or active, the executor still creates the newly due run.
- If materialization, run creation, or run start queuing fails, log a warning and wait until the next cron occurrence.
- Scheduled runs use `AutomationRef.trigger_id` set to the schedule trigger ID.
- Scheduled runs use `Principal::System { system_kind: SystemActorKind::Engine }` for provenance to avoid public enum/API churn.
- Do not add an automation state/status store, next-run status, last-run status, one-time schedule support, event triggers, CLI commands, or OpenAPI changes.

## File Structure

- Create `lib/crates/fabro-server/src/server/automation_scheduler.rs`
  - Owns the in-memory schedule planner, scheduler loop, due trigger firing, and tests.
- Modify `lib/crates/fabro-server/src/server.rs`
  - Register the new module, add an automation scheduler `Notify`, expose a notifier method, and initialize state.
- Modify `lib/crates/fabro-server/src/serve.rs`
  - Spawn the automation scheduler alongside the existing run scheduler.
- Modify `lib/crates/fabro-server/src/server/handler/automations.rs`
  - Notify the scheduler after successful create, replace, and delete.
- Modify `lib/crates/fabro-server/src/server/handler/mod.rs` and `lifecycle.rs`
  - Make `lifecycle` and `queue_run_start` visible inside `crate::server` so the scheduler can queue created runs through the same path as the API endpoint.
- Modify `lib/crates/fabro-automation/src/model.rs` only if a small method on `AutomationTrigger` or `ScheduleTrigger` makes scheduler code clearer.

## Implementation Tasks

### Task 1: Add Deterministic Schedule Planner

**Files:**
- Create: `lib/crates/fabro-server/src/server/automation_scheduler.rs`
- Modify: `lib/crates/fabro-server/src/server.rs`
- Test: `lib/crates/fabro-server/src/server/automation_scheduler.rs`

- [ ] Add `mod automation_scheduler;` to `server.rs` and `pub(crate) use automation_scheduler::spawn_automation_scheduler;` if `serve.rs` imports it through the server module.
- [ ] Define planner-only types in `automation_scheduler.rs`:
  - `ScheduleTriggerKey { automation_id: AutomationId, trigger_id: AutomationTriggerId }`
  - `ScheduleCursor { automation_revision: AutomationRevision, expression: String, next_due_at: DateTime<Utc> }`
  - `DueScheduleTrigger { automation: Automation, trigger_id: AutomationTriggerId, due_at: DateTime<Utc> }`
  - `AutomationSchedulePlanner { cursors: HashMap<ScheduleTriggerKey, ScheduleCursor> }`
- [ ] Add a private `next_occurrence(expression: &str, after: DateTime<Utc>) -> Result<DateTime<Utc>, croner::errors::CronError>` helper using the same croner options as automation validation: seconds disallowed, year disallowed, exclusive of `after`.
- [ ] Add `AutomationSchedulePlanner::reconcile(&mut self, automations: &[Automation], now: DateTime<Utc>)`.
  - Include only enabled automations and enabled `AutomationTrigger::Schedule` triggers.
  - Insert new cursors with `next_due_at = next_occurrence(expression, now)`.
  - Keep existing cursors when automation revision and expression are unchanged.
  - Reset existing cursors when revision or expression changes.
  - Remove cursors for deleted, disabled, or no-longer-scheduled triggers.
- [ ] Add `AutomationSchedulePlanner::take_due(&mut self, automations: &[Automation], now: DateTime<Utc>) -> Vec<DueScheduleTrigger>`.
  - Return cursors whose `next_due_at <= now`.
  - Advance each due cursor to `next_occurrence(expression, now)` before returning it.
  - Return at most one due trigger per cursor per call.
- [ ] Add unit tests with fixed `DateTime<Utc>` values:
  - New cursor starts at the next future occurrence and does not backfill an older occurrence.
  - Due cursor is returned once and advanced beyond `now`.
  - Disabled automation and disabled schedule trigger remove cursors.
  - Replacing an automation revision or expression resets the cursor.
  - Multiple schedule triggers on one automation produce independent cursors.

### Task 2: Add Scheduler Loop And Server Wiring

**Files:**
- Modify: `lib/crates/fabro-server/src/server.rs`
- Modify: `lib/crates/fabro-server/src/serve.rs`
- Modify: `lib/crates/fabro-server/src/server/handler/automations.rs`
- Create/modify tests in: `lib/crates/fabro-server/src/server/automation_scheduler.rs`

- [ ] Add `automation_scheduler_notify: Notify` to `AppState`.
- [ ] Initialize it in `build_app_state`.
- [ ] Add `pub(crate) fn notify_automation_scheduler(&self)` that calls `notify_one()`.
- [ ] Add `pub(crate) fn automation_scheduler_notified(&self) -> impl Future<Output = ()>` only if needed to avoid exposing the `Notify` field.
- [ ] In `create_automation`, notify after the store create succeeds.
- [ ] In `replace_automation`, notify after the store replace succeeds.
- [ ] In `delete_automation`, notify after the store delete succeeds.
- [ ] Implement `spawn_automation_scheduler(state: Arc<AppState>)`:
  - Own an `AutomationSchedulePlanner`.
  - Reconcile on every loop before computing due triggers.
  - Use `state.automation_store().list().await` as the source of truth.
  - Sleep until the nearest cursor due time, capped at 30 seconds, and also wake on `automation_scheduler_notify`.
  - Exit when `state.shutdown_token().cancelled()` resolves or `state.is_shutting_down()` is true.
- [ ] Call `spawn_automation_scheduler(Arc::clone(&state))` in `serve.rs` immediately after `spawn_scheduler(Arc::clone(&state))`.
- [ ] Add tests for notification-sensitive behavior by calling the planner directly, not by relying on wall-clock sleeps.

### Task 3: Fire Scheduled Automation Runs

**Files:**
- Modify: `lib/crates/fabro-server/src/server/automation_scheduler.rs`
- Modify: `lib/crates/fabro-server/src/server/handler/mod.rs`
- Modify: `lib/crates/fabro-server/src/server/handler/lifecycle.rs`
- Test: `lib/crates/fabro-server/src/server/automation_scheduler.rs`

- [ ] Change `handler/mod.rs` from `mod lifecycle;` to `pub(in crate::server) mod lifecycle;`.
- [ ] Change `queue_run_start` in `lifecycle.rs` from `pub(super)` to `pub(in crate::server)`.
- [ ] Add `fire_scheduled_automation_run(state: Arc<AppState>, automation: Automation, trigger_id: AutomationTriggerId, due_at: DateTime<Utc>)`.
  - Create a fresh `RunId`.
  - Materialize with `state.materialize_automation_run(AutomationRunMaterializeInput { automation_id, target, run_id, user_settings_path, temp_root })`.
  - Build `AutomationRef { id, name: Some(automation.name.clone()), trigger_id: Some(trigger_id.to_string()) }`.
  - Call `handler::runs::create_run_from_manifest` with:
    - `explicit_run_id: Some(run_id)`
    - `actor: Principal::System { system_kind: SystemActorKind::Engine }`
    - empty `HeaderMap`
    - `automation: Some(automation_ref)`
  - If the response status is success, call `handler::lifecycle::queue_run_start(state.as_ref(), run_id, false, actor).await`.
  - Log success at `info!` with `automation_id`, `trigger_id`, `run_id`, and `due_at`.
  - Log failures at `warn!` with `automation_id`, `trigger_id`, `due_at`, and safe error/status fields.
- [ ] Have the scheduler spawn one Tokio task per due trigger so slow materialization does not block cursor reconciliation for other triggers.
- [ ] Ensure the cursor advances before spawning the fire task so failed fires do not hot-loop.

### Task 4: Scheduled Executor Tests

**Files:**
- Modify: `lib/crates/fabro-server/src/server/automation_scheduler.rs`
- Modify only if needed: `lib/crates/fabro-server/src/test_support.rs`

- [ ] Add test helpers in `automation_scheduler.rs` tests:
  - Build a temp `AppState` with `TestAppStateBuilder`.
  - Use `TestAutomationRunMaterializer::succeed` with a minimal manifest.
  - Create automation definitions through `state.automation_store().create(...)` rather than hand-authoring run internals.
- [ ] Add an async test that calls a test-only `run_due_schedules_once(state, planner, now)` helper:
  - A schedule-only automation fires when due.
  - The created run has `automation.id`, `automation.name`, and `automation.trigger_id` set to the schedule trigger ID.
  - The run appears in cached run summaries after creation.
- [ ] Add an async test proving schedule-only automations do not require an API trigger.
- [ ] Add an async test proving a disabled automation and disabled schedule trigger do not create runs.
- [ ] Add an async test proving multiple due triggers create multiple runs.
- [ ] Add an async test proving active or queued prior runs do not suppress a newly due run.
- [ ] Add an async test with a failing fake materializer:
  - No run is created for the failed due occurrence.
  - A second call before the next cron occurrence does not retry.
  - A call at the next cron occurrence attempts again.
- [ ] Keep tests at server/unit level for scheduler internals and existing `api::automations` tests for API behavior.

### Task 5: Verification

**Files:**
- No source changes beyond previous tasks.

- [ ] Run:

```bash
cargo nextest run -p fabro-automation
```

- [ ] Run:

```bash
cargo nextest run -p fabro-server automation_scheduler
```

- [ ] Run:

```bash
cargo nextest run -p fabro-server automation_materializer
```

- [ ] Run:

```bash
cargo nextest run -p fabro-server automation_metadata
```

- [ ] Run:

```bash
cargo nextest run -p fabro-server --features test-support api::automations
```

- [ ] Run formatting and lint checks:

```bash
cargo +nightly-2026-04-14 fmt --check --all
cargo +nightly-2026-04-14 clippy --workspace --all-targets -- -D warnings
```

## Acceptance Criteria

- Enabled schedule triggers create and start normal Fabro runs at their UTC cron times.
- The executor does not create backfilled runs on startup.
- The executor creates every observed due run even when prior runs from the same trigger are still active.
- Failed materialization or creation waits until the next cron occurrence.
- Scheduled runs are visible through normal run APIs and `GET /automations/{id}/runs`.
- Manual/API-triggered automation behavior is unchanged.
- No public API schemas, generated clients, CLI commands, runtime status store, one-time schedule support, or event triggers are added.


## Completed stages
- **toolchain**: succeeded
  - Script: `command -v cargo >/dev/null || { curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y && sudo ln -sf $HOME/.cargo/bin/* /usr/local/bin/; }; cargo --version 2>&1`
  - Output:
    ```
    cargo 1.95.0 (f2d3ce0bd 2026-03-21)
    ```
- **preflight_compile**: succeeded
  - Script: `cargo check -q --workspace 2>&1`
  - Output: (empty)
- **preflight_lint**: succeeded
  - Script: `cargo +nightly-2026-04-14 clippy -q --workspace --all-targets -- -D warnings 2>&1`
  - Output: (empty)
- **implement**: succeeded
  - Model: gpt-5.5, 1.5m tokens in / 31.9k out
- **simplify_opus**: succeeded
  - Model: claude-opus-4-7, 91.4k tokens in / 32.4k out
  - Files: /home/daytona/workspace/fabro/lib/crates/fabro-automation/src/lib.rs, /home/daytona/workspace/fabro/lib/crates/fabro-automation/src/model.rs, /home/daytona/workspace/fabro/lib/crates/fabro-server/src/server.rs, /home/daytona/workspace/fabro/lib/crates/fabro-server/src/server/automation_scheduler.rs, /home/daytona/workspace/fabro/lib/crates/fabro-server/src/server/handler/automations.rs


# Simplify: Code Review and Cleanup

Review changes vs. origin for reuse, quality, and efficiency. Fix any issues found.

## Phase 1: Identify Changes

Run git diff (or git diff HEAD if there are staged changes) to see what changed. If there are no git changes, review the most recently modified files that the user mentioned or that you edited earlier in this conversation.

## Phase 2: Launch Three Review Agents in Parallel

Use the Agent tool to launch all three agents concurrently in a single message. Pass each agent the full diff so it has the complete context.

### Agent 1: Code Reuse Review

For each change:

1. Search for existing utilities and helpers that could replace newly written code. Use Grep to find similar patterns elsewhere in the codebase — common locations are utility directories, shared modules, and files adjacent to the changed ones.
2. Flag any new function that duplicates existing functionality. Suggest the existing function to use instead.
3. Flag any inline logic that could use an existing utility — hand-rolled string manipulation, manual path handling, custom environment checks, ad-hoc type guards, and similar patterns are common candidates.

Note: This is a greenfield app, so focus on maximizing simplicity and don't worry about changing things to achieve it.

### Agent 2: Code Quality Review

Review the same changes for hacky patterns:

1. Redundant state: state that duplicates existing state, cached values that could be derived, observers/effects that could be direct calls
2. Parameter sprawl: adding new parameters to a function instead of generalizing or restructuring existing ones
3. Copy-paste with slight variation: near-duplicate code blocks that should be unified with a shared abstraction
4. Leaky abstractions: exposing internal details that should be encapsulated, or breaking existing abstraction boundaries
5. Stringly-typed code: using raw strings where constants, enums (string unions), or branded types already exist in the codebase

Note: This is a greenfield app, so be aggressive in optimizing quality.

### Agent 3: Efficiency Review

Review the same changes for efficiency:

1. Unnecessary work: redundant computations, repeated file reads, duplicate network/API calls, N+1 patterns
2. Missed concurrency: independent operations run sequentially when they could run in parallel
3. Hot-path bloat: new blocking work added to startup or per-request/per-render hot paths
4. Unnecessary existence checks: pre-checking file/resource existence before operating (TOCTOU anti-pattern) — operate directly and handle the error
5. Memory: unbounded data structures, missing cleanup, event listener leaks
6. Overly broad operations: reading entire files when only a portion is needed, loading all items when filtering for one

## Phase 3: Fix Issues

Wait for all three agents to complete. Aggregate their findings and fix each issue directly. If a finding is a false positive or not worth addressing, note it and move on — do not argue with the finding, just skip it.

When done, briefly summarize what was fixed (or confirm the code was already clean).