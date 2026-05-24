Goal: # Fold Context Window Into Agent Messages Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove the chatty `agent.context_window.snapshot` event and persist context-window data through existing `agent.message` events.

**Architecture:** Compute the context-window breakdown locally while the agent still has the exact request, attach the final content-free projection to the next `agent.message`, and let the run projection reducer store that latest projection for `GET /runs/{id}/stages/{stageId}/context-window`. Do not call provider token-count APIs during normal execution.

**Tech Stack:** Rust, Serde, Fabro agent/session events, Fabro workflow event conversion, Fabro store projections, OpenAPI/progenitor, generated TypeScript client, React/SWR.

---

## Scope And Decisions

- Remove `agent.context_window.snapshot` completely from new code. This is a greenfield/no-prod app, so do not preserve backward-compatible deserialization or frontend handling for old snapshot events.
- Keep `StageContextWindowProjection`, `StageContextWindow`, `StageProjection.context_window`, and the existing context-window GET endpoint.
- Add optional `context_window` data to `agent.message` properties.
- Normal execution uses only local estimates and token usage returned by normal LLM responses.
- Do not call `Client::count_input_tokens` from `fabro-agent::Session`.
- The GET endpoint remains projection-backed for this pass. It must not require an in-memory session and must not trigger provider token-count API calls.
- Failed-before-response turns do not persist context-window data.
- Live pre-response context-window updates are not required. The UI updates after `agent.message`.

## File Map

- Modify `lib/crates/fabro-types/src/run_event/agent.rs`
  - Add `context_window: Option<StageContextWindowProjection>` to `AgentMessageProps`.
  - Remove `AgentContextWindowSnapshotProps`.
- Modify `lib/crates/fabro-types/src/run_event/mod.rs`
  - Remove the `AgentContextWindowSnapshot` event variant, name mapping, known-event entry, and serde tests.
  - Update `agent.message` serde tests to cover optional context-window data.
- Modify `lib/crates/fabro-agent/src/types.rs`
  - Remove `AgentEvent::ContextWindowSnapshot`.
  - Add `context_window: Option<StageContextWindowProjection>` to `AgentEvent::AssistantMessage`.
- Modify `lib/crates/fabro-agent/src/session.rs`
  - Remove async provider-count task and response-usage snapshot event emission.
  - Keep local context-window snapshot construction at request-build time.
  - Attach the scaled or local projection to `AssistantMessage`.
- Keep `lib/crates/fabro-agent/src/context_window.rs`
  - Reuse `build_local_snapshot` and `scaled_snapshot`.
  - Remove only tests or helpers that exist solely for provider-count snapshot emission.
- Modify `lib/crates/fabro-workflow/src/event/convert.rs`
  - Remove conversion for `AgentEvent::ContextWindowSnapshot`.
  - Copy `context_window` from `AgentEvent::AssistantMessage` into `AgentMessageProps`.
- Modify `lib/crates/fabro-workflow/src/event/names.rs`
  - Remove the snapshot event name.
- Modify `lib/crates/fabro-store/src/run_state.rs`
  - Remove reducer support for `EventBody::AgentContextWindowSnapshot`.
  - When reducing `EventBody::AgentMessage`, copy `props.context_window` into `stage.context_window` when present and stamp `event_seq`.
- Modify `lib/crates/fabro-server/src/server/tests.rs`
  - Seed context-window endpoint tests with `agent.message` events that include context-window data.
- Modify `docs/public/api-reference/fabro-api.yaml`
  - Add optional `context_window` to `AgentMessageProps`.
  - Remove the snapshot event schema/variant.
- Regenerate/update `lib/packages/fabro-api-client/src`.
- Modify `apps/fabro-web/app/lib/run-events.ts` and tests
  - Remove special handling for `agent.context_window.snapshot`.
  - Rely on existing `agent.message` invalidation for stage events and context-window data.

## Implementation Steps

### Task 1: Move The Event Contract Onto `agent.message`

- [ ] Add optional `context_window` to Rust `AgentMessageProps`.
- [ ] Remove `AgentContextWindowSnapshotProps` and the `agent.context_window.snapshot` `EventBody` variant.
- [ ] Update run-event serde tests so `agent.message` round-trips with and without `context_window`.
- [ ] Remove tests whose only assertion is that `agent.context_window.snapshot` is known or serializes.

### Task 2: Stop Emitting Snapshot Events

- [ ] Remove `AgentEvent::ContextWindowSnapshot` and its trace/debug handling.
- [ ] Change `AgentEvent::AssistantMessage` to carry `context_window: Option<StageContextWindowProjection>`.
- [ ] In `Session::run_single_input`, keep the local context-window snapshot returned from request construction.
- [ ] Delete the spawned `count_input_tokens(... PreferProvider)` task and its fingerprint suppression state.
- [ ] After the normal LLM response arrives, compute:
  - `ResponseUsageScaledBreakdown` when response input/cache usage is positive.
  - `LocalEstimate` when response usage has no usable input tokens.
- [ ] Attach that projection to the emitted `AssistantMessage`.

### Task 3: Update Workflow Conversion And Store Projection

- [ ] Remove snapshot event name/conversion branches.
- [ ] Include `context_window` when converting `AgentEvent::AssistantMessage` to `EventBody::AgentMessage`.
- [ ] In the store reducer, update `stage.context_window` from `AgentMessageProps.context_window`.
- [ ] Stamp the copied projection with the `agent.message` event sequence.
- [ ] Replace store tests for snapshot replacement with message-carried context-window tests.

### Task 4: Keep The GET Endpoint Projection-Backed

- [ ] Keep the endpoint route and response type unchanged.
- [ ] Keep existing `not_agent_stage`, `not_observed`, and terminal `stored` behavior.
- [ ] Update endpoint tests to seed context-window data via `agent.message`.
- [ ] Do not add endpoint-time provider token-count calls.

### Task 5: Remove Frontend And API Trace

- [ ] Remove frontend constants/tests for `agent.context_window.snapshot`.
- [ ] Ensure `agent.message` still invalidates `stageContextWindow` through existing stage activity handling.
- [ ] Update OpenAPI and regenerated TypeScript client so no snapshot event model remains.
- [ ] Run a final search for `agent.context_window.snapshot`, `AgentContextWindowSnapshot`, and `ContextWindowSnapshot`; only historical docs or this plan may remain.

## Test Plan

- Run `cargo build -p fabro-api` after OpenAPI/Rust type changes.
- Run targeted Rust tests:
  - `cargo nextest run -p fabro-agent`
  - `cargo nextest run -p fabro-workflow`
  - `cargo nextest run -p fabro-store`
  - `cargo nextest run -p fabro-server get_run_stage_context_window`
  - `cargo nextest run -p fabro-types agent_message`
- Regenerate TypeScript client with `cd lib/packages/fabro-api-client && bun run generate`.
- Run web checks:
  - `cd apps/fabro-web && bun test`
  - `cd apps/fabro-web && bun run typecheck`
  - `cd lib/packages/fabro-api-client && bun run typecheck`
- Run final targeted searches:
  - `rg -n "agent\\.context_window\\.snapshot|AgentContextWindowSnapshot|ContextWindowSnapshot" lib/crates apps/fabro-web lib/packages docs/public/api-reference/fabro-api.yaml`
  - Expected: no implementation/API/frontend matches.

## Acceptance Criteria

- Normal agent runs do not emit `agent.context_window.snapshot`.
- Normal agent runs do not call provider token-count endpoints for context-window reporting.
- `agent.message` includes context-window data when the agent produced a response.
- `GET /runs/{id}/stages/{stageId}/context-window` still returns the latest context-window projection.
- The event log contains no standalone context-window snapshot events.
- Public API/client/types no longer expose `agent.context_window.snapshot`.


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


Read the plan file referenced in the goal and implement every step. Make all the code changes described in the plan. Use red/green TDD.