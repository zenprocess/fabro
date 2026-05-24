Goal: ---
title: fix: Prefer root stage TODO projection
type: fix
status: active
date: 2026-05-24
---

# fix: Prefer Root Stage TODO Projection

## Overview

Fix stage TODO projection so `StageProjection.todos` represents the selected
stage agent's root session plan. Today a child OpenAI session can emit its own
`todo.created` events on the same `stage_id`, replacing the root list and
causing later root `todo.updated` completions to be ignored. The visible
symptom is an agent sidebar showing a stale child TODO list such as `0/3`
completed even though the root stage plan completed.

## Problem Frame

OpenAI `update_plan` lists are scoped per agent session as
`openai_plan:<session_id>`. A stage may contain both the root agent session and
child/subagent sessions. `StageProjection` currently has only one
`todos: Option<TodoListProjection>`, so the reducer must choose which list is
the stage-level list. The stage sidebar is a stage-agent summary, so it should
show the root stage session's list rather than whichever session most recently
created todos.

## Requirements Trace

- R1. Root OpenAI plan TODOs must remain the projected `stage.todos` list even
  when child OpenAI sessions emit TODO events on the same stage.
- R2. Later root OpenAI `todo.updated` and `todo.deleted` events must continue
  to apply after child OpenAI TODO events are observed.
- R3. Child OpenAI TODO lists must not create, replace, or mutate
  `StageProjection.todos`.
- R4. Anthropic task projection must remain unchanged because Anthropic task
  lists are intentionally scoped to the root session and shared across
  subagents.
- R5. Do not change public API shapes, generated API types, or frontend
  rendering code for this fix.

## Scope Boundaries

- Do not add a multi-list TODO projection in this change.
- Do not expose child/subagent TODO lists in the sidebar in this change.
- Do not change `TodoListProjection`, `StageProjection`, or OpenAPI schemas.
- Do not change event serialization, event names, or the agent `update_plan`
  tool behavior.

## Context & Research

### Relevant Code and Patterns

- `lib/crates/fabro-agent/src/todo_tools.rs` scopes OpenAI plans by
  `session_id`, producing `openai_plan:<session_id>`.
- `lib/crates/fabro-types/src/run_event/mod.rs` already carries
  `session_id` and `parent_session_id` on event envelopes.
- `lib/crates/fabro-store/src/run_state.rs` owns the persisted-event reducer
  that updates `StageProjection.todos` from `todo.created`, `todo.updated`,
  and `todo.deleted`.
- The existing `todo_reducer` test module in `run_state.rs` is the right place
  for focused regression coverage.
- `apps/fabro-web/app/routes/run-stages.tsx` and
  `apps/fabro-web/app/components/stage-insights-sidebar.tsx` already render
  `stage.todos`; no UI change is needed if the projection is corrected.

### Observed Failing Case

For run `01KSBT48J14ZMK9HQN48SVMG3T`, stage `simplify_gpt@1` had:

- Root list `openai_plan:2f4458b9-1128-4a96-8dfa-bff4b73b9c33`: five root
  todos, all completed by later `todo.updated` events.
- Child list `openai_plan:a09b9432-823d-4068-91fa-5c6185578e8e`: three child
  todos, created with `parent_session_id` set and never updated.

The child `todo.created` events replaced `stage.todos`, so the sidebar showed
the child list as `0/3` even after the root list completed.

## Key Technical Decisions

- Use `parent_session_id` as the root-vs-child signal for OpenAI plan
  projection. A root stage session has `parent_session_id == None`; child
  sessions have `parent_session_id != None`.
- Ignore child OpenAI plan events for `StageProjection.todos`. This preserves
  the current single-list schema while making the selected list match the
  stage sidebar's meaning.
- Keep Anthropic task projection unchanged. Anthropic tasks use
  `anthropic_tasks:<root_session_id>`, so child-session envelopes should still
  be allowed to update the shared root task list.
- Treat legacy OpenAI events without `parent_session_id` as root-compatible for
  backwards compatibility.

## Implementation Units

- [ ] **Unit 1: Add reducer policy for projectable stage TODO events**

**Goal:** Make the reducer distinguish root OpenAI plan events from child
OpenAI plan events before mutating `stage.todos`.

**Files:**
- Modify: `lib/crates/fabro-store/src/run_state.rs`

**Approach:**
- Add a small helper near the TODO reducer functions, for example
  `should_project_stage_todo_event(stored: &RunEvent, list_kind:
  TodoListKind) -> bool`.
- Return `false` only when `list_kind == TodoListKind::OpenAiPlan` and
  `stored.parent_session_id.is_some()`.
- Return `true` for root OpenAI events and all Anthropic task events.
- Call this helper in the `EventBody::TodoCreated`,
  `EventBody::TodoUpdated`, and `EventBody::TodoDeleted` match arms before
  resolving or mutating the stage projection.
- Leave `apply_todo_created`, `apply_todo_updated`, and
  `apply_todo_deleted` focused on list mutation once the caller has decided
  the event is projectable.

**Test scenarios:**
- Root OpenAI events with no `parent_session_id` still create and update
  `stage.todos`.
- Child OpenAI events with `parent_session_id` do not create `stage.todos`
  when no root list exists.
- Child OpenAI events do not replace an existing root OpenAI list.
- Root OpenAI updates still apply after ignored child OpenAI events.

- [ ] **Unit 2: Add focused reducer regression tests**

**Goal:** Lock the intended root-list behavior so future TODO projection work
does not regress the sidebar.

**Files:**
- Modify: `lib/crates/fabro-store/src/run_state.rs`

**Approach:**
- Extend the existing `todo_reducer` module rather than creating a new test
  file.
- Add a test helper or local event setup that sets
  `event.event.parent_session_id = Some(parent_session_id.to_string())` for
  child-session events.
- Add one regression test that reproduces the failing sequence:
  root OpenAI creates list, child OpenAI creates a different list on the same
  stage, root OpenAI completes its items. Assert the final projection is the
  root list and all root statuses are completed.
- Add one test proving child OpenAI events alone do not create a stage TODO
  projection.
- Add one test proving Anthropic child-session task events still project.

**Verification:**
- `cargo nextest run -p fabro-store todo_reducer`
- `cargo +nightly-2026-04-14 fmt --check --all`

## System-Wide Impact

- **API compatibility:** No response schema changes. Existing consumers of
  `StageProjection.todos` continue to receive a single list.
- **UI behavior:** The sidebar should show the root agent's TODO progress for
  the selected stage. Child OpenAI session plans remain available only in the
  raw event stream for now.
- **Historical runs:** Replaying existing event logs should produce corrected
  projections because the decision uses envelope fields already persisted on
  child events.
- **Future extensibility:** If child/subagent TODO display is needed later,
  add a multi-list projection separately rather than overloading
  `stage.todos`.

## Risks & Mitigations

| Risk | Mitigation |
|------|------------|
| Some legacy child OpenAI events lack `parent_session_id` and still project as root | Accept this for backwards compatibility; only events with explicit child-session evidence are filtered. |
| Anthropic child task updates could be accidentally filtered | Gate only `TodoListKind::OpenAiPlan`; add a regression test for `TodoListKind::AnthropicTasks`. |
| Root list replacement semantics become ambiguous if a root stage emits multiple OpenAI list IDs | Preserve current root replacement behavior; the fix only prevents child lists from replacing root lists. |

## Assumptions

- `parent_session_id == None` is the canonical signal for the root stage agent
  session in stored event envelopes.
- Child OpenAI TODO lists are not part of the current stage sidebar contract.
- The correct near-term fix is projection selection, not a frontend workaround
  or a schema expansion.


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
- **implement**: failed
- **simplify_opus**: succeeded
  - Model: claude-opus-4-7, 35.9k tokens in / 13.9k out
  - Files: /home/daytona/workspace/fabro/lib/crates/fabro-store/src/run_state.rs


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