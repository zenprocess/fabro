Goal: ---
title: "feat: Wall and active time metrics"
type: feature
status: active
date: 2026-05-21
---

# feat: Wall and active time metrics

## Summary

Rename runtime duration concepts from ambiguous duration/runtime/elapsed fields
to explicit wall-time fields, then add first-class active timing.

Definitions:

- `wall_time_ms`: elapsed clock time from start to finish.
- `inference_time_ms`: Fabro-observed LLM request/stream elapsed time.
- `tool_time_ms`: tool or command execution elapsed time.
- `active_time_ms`: `inference_time_ms + tool_time_ms`.

This is greenfield API churn. Do not preserve old public run/stage timing
fields, aliases, or compatibility shims for `duration_ms`, `runtime_secs`, or
`elapsed_secs` on run/stage runtime surfaces.

Run-level active time is total work performed: sum active timing across stage
visits. Parallel work is summed, so run active time can exceed run wall time.

## Key Changes

- Add a shared timing value object in `fabro-types` for stage/run active timing:
  - `wall_time_ms`
  - `inference_time_ms`
  - `tool_time_ms`
  - derived or stored `active_time_ms`
- Replace run/stage public timing fields:
  - stage/run terminal event props use `wall_time_ms` plus the active timing
    breakdown.
  - `StageProjection` stores the timing breakdown instead of stage
    `duration_ms`.
  - `RunTimestamps` keeps timestamps only; move elapsed values into a separate
    run timing object.
  - `/runs/{id}/stages` and `/runs/{id}/billing` expose timing in milliseconds,
    not `runtime_secs`.
- Keep `duration_ms` only for unrelated subsystem-specific operational events
  where the name is still local and unambiguous, such as sandbox setup,
  metadata snapshot, devcontainer lifecycle, and hook execution. The cleanup
  target is public run/stage runtime semantics.
- Update OpenAPI and regenerate the Rust and TypeScript API clients after
  schema edits.

## Timing Behavior

- `prompt` nodes:
  - inference = elapsed time spent in the one-shot LLM backend call.
  - tool = 0.
- native `agent` nodes:
  - inference = sum of elapsed time spent opening/consuming LLM streams for new
    turns in the stage.
  - tool = sum of elapsed time spent executing agent tool calls.
  - retry backoff and waiting for steering are wall time, not active time.
- opaque external/ACP agent nodes:
  - inference = 0 for v1 because Fabro cannot reliably separate model time from
    process runtime.
  - tool = external agent process wall time.
- `command` nodes:
  - inference = 0.
  - tool = command wall time from the sandbox command result.
- `human`, `wait`, `conditional`, `fan-in`, `start`, and `exit`:
  - inference = 0.
  - tool = 0.
- `parallel` container nodes:
  - active = 0 on the container stage.
  - child/branch stages carry work timing so rollups do not double count.

## Implementation

- In `fabro-types`, introduce the timing structs and replace the relevant fields
  in `Outcome`, `NodeResult` consumers, `StageProjection`, `Conclusion`,
  `RunTimestamps`, `RunCompletedProps`, `RunFailedProps`,
  `StageCompletedProps`, `StageFailedProps`, `RunBillingStage`, and
  `RunBillingTotals`.
- In `fabro-workflow`, rename run/stage execution fields from `duration_ms` to
  `wall_time_ms` and thread timing through lifecycle events, terminal events,
  conclusion building, pull request summaries, timeline/billing rollups, and
  test support fixtures.
- In `fabro-agent`, add timing data to agent events or session results so
  `fabro-workflow` can aggregate:
  - LLM stream/request elapsed time per assistant response.
  - tool call elapsed time per tool completion.
  - preserve token billing behavior separately from timing.
- In `fabro-store`, update event projection to write stage `started_at`, timing
  breakdowns, and run summary timing from the new event props.
- In `fabro-server`, replace runtime billing aggregation with a timing rollup
  owned by workflow/projection code. Billing endpoints may include timing, but
  billing logic should not define timing semantics.
- In `apps/fabro-web`, update run list/detail/stages/billing views and tests to
  render wall time and active time from the new fields.
- Remove all run/stage public API references to old timing names from
  `docs/public/api-reference/fabro-api.yaml` and regenerated clients.

## Test Plan

- `fabro-types`:
  - run and stage event round trips serialize the new timing payloads.
  - old public run/stage timing properties are absent from serialized fixtures.
  - API-facing timing structs round trip through generated schemas.
- `fabro-store`:
  - `stage.started` records `started_at`.
  - stage terminal events store `wall_time_ms` and active breakdowns.
  - run summaries expose timestamp fields and run timing without
    `elapsed_secs`.
  - retried stages reset per-attempt live wall-time state correctly.
- `fabro-workflow`:
  - prompt stages report inference-only active timing.
  - command stages report tool-only active timing.
  - native agent stages sum LLM turn timing and tool timing.
  - human/wait/conditional/fan-in/start/exit stages report zero active timing.
  - parallel stage rollups sum child active work and avoid container double
    counting.
  - repeated node visits sum timing by node in rollups.
- `fabro-server`:
  - `/runs/{id}/stages`, `/runs/{id}/billing`, run detail, and run list return
    new timing fields only.
  - aggregate billing/timing totals sum active work across completed runs.
  - OpenAPI conformance passes after regeneration.
- `apps/fabro-web`:
  - run list/detail/billing/stages render wall time and active time.
  - in-flight wall-time ticking still uses `started_at`.
  - no UI code reads `runtime_secs`, `elapsed_secs`, or run/stage
    `duration_ms`.

## Validation

Run focused checks first:

```bash
cargo nextest run -p fabro-types -p fabro-store -p fabro-workflow -p fabro-server
cd apps/fabro-web && bun test && bun run typecheck
```

Then run full workspace checks before merging:

```bash
cargo build --workspace
cargo nextest run --workspace
cargo +nightly-2026-04-14 fmt --check --all
cargo +nightly-2026-04-14 clippy --workspace --all-targets -- -D warnings
git diff --check
```

## Assumptions

- Inference time is Fabro-observed LLM request/stream elapsed time, not
  provider-reported model-only compute time.
- LLM retry backoff, queueing outside a request/stream, human waits, steering
  waits, and scheduler gaps are wall time but not active time.
- Active timing is finalized-event based in v1; live active-time ticking can be
  added later if it becomes necessary.
- No compatibility layer is required for existing API clients or stored run
  event data.


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
  - Model: claude-opus-4-7, 529.6k tokens in / 176.3k out
  - Files: /home/daytona/workspace/fabro/apps/fabro-web/app/data/runs.test.ts, /home/daytona/workspace/fabro/apps/fabro-web/app/data/runs.ts, /home/daytona/workspace/fabro/apps/fabro-web/app/lib/stage-sidebar.test.ts, /home/daytona/workspace/fabro/apps/fabro-web/app/lib/stage-sidebar.ts, /home/daytona/workspace/fabro/apps/fabro-web/app/routes/run-detail.test.ts, /home/daytona/workspace/fabro/docs/public/api-reference/fabro-api.yaml, /home/daytona/workspace/fabro/lib/crates/fabro-api/build.rs, /home/daytona/workspace/fabro/lib/crates/fabro-api/tests/run_failure_round_trip.rs, /home/daytona/workspace/fabro/lib/crates/fabro-api/tests/run_summary_round_trip.rs, /home/daytona/workspace/fabro/lib/crates/fabro-api/tests/stage_projection_round_trip.rs, /home/daytona/workspace/fabro/lib/crates/fabro-cli/src/commands/run/events.rs, /home/daytona/workspace/fabro/lib/crates/fabro-cli/src/commands/run/output.rs, /home/daytona/workspace/fabro/lib/crates/fabro-cli/src/commands/run/run_progress/event.rs, /home/daytona/workspace/fabro/lib/crates/fabro-cli/src/commands/run/run_progress/mod.rs, /home/daytona/workspace/fabro/lib/crates/fabro-cli/src/commands/run/wait.rs, /home/daytona/workspace/fabro/lib/crates/fabro-cli/src/commands/runs/list.rs, /home/daytona/workspace/fabro/lib/crates/fabro-cli/src/server_runs.rs, /home/daytona/workspace/fabro/lib/crates/fabro-cli/tests/it/cmd/attach.rs, /home/daytona/workspace/fabro/lib/crates/fabro-cli/tests/it/cmd/run.rs, /home/daytona/workspace/fabro/lib/crates/fabro-cli/tests/it/cmd/support.rs, /home/daytona/workspace/fabro/lib/crates/fabro-cli/tests/it/cmd/wait.rs, /home/daytona/workspace/fabro/lib/crates/fabro-cli/tests/it/scenario/smoke.rs, /home/daytona/workspace/fabro/lib/crates/fabro-cli/tests/it/workflow/full_stack.rs, /home/daytona/workspace/fabro/lib/crates/fabro-core/src/executor.rs, /home/daytona/workspace/fabro/lib/crates/fabro-core/src/outcome.rs, /home/daytona/workspace/fabro/lib/crates/fabro-core/src/state.rs, /home/daytona/workspace/fabro/lib/crates/fabro-mcp-server/src/run_tools/common.rs, /home/daytona/workspace/fabro/lib/crates/fabro-mcp-server/src/run_tools/create.rs, /home/daytona/workspace/fabro/lib/crates/fabro-mcp-server/src/run_tools/search.rs, /home/daytona/workspace/fabro/lib/crates/fabro-server/src/demo/mod.rs, /home/daytona/workspace/fabro/lib/crates/fabro-server/src/server.rs, /home/daytona/workspace/fabro/lib/crates/fabro-server/src/server/handler/billing.rs, /home/daytona/workspace/fabro/lib/crates/fabro-server/src/server/handler/system.rs, /home/daytona/workspace/fabro/lib/crates/fabro-server/src/server/tests.rs, /home/daytona/workspace/fabro/lib/crates/fabro-server/tests/it/scenario/usage.rs, /home/daytona/workspace/fabro/lib/crates/fabro-store/src/run_state.rs, /home/daytona/workspace/fabro/lib/crates/fabro-types/src/conclusion.rs, /home/daytona/workspace/fabro/lib/crates/fabro-types/src/event_envelope.rs, /home/daytona/workspace/fabro/lib/crates/fabro-types/src/lib.rs, /home/daytona/workspace/fabro/lib/crates/fabro-types/src/outcome.rs, /home/daytona/workspace/fabro/lib/crates/fabro-types/src/run_event/mod.rs, /home/daytona/workspace/fabro/lib/crates/fabro-types/src/run_event/run.rs, /home/daytona/workspace/fabro/lib/crates/fabro-types/src/run_event/stage.rs, /home/daytona/workspace/fabro/lib/crates/fabro-types/src/run_projection.rs, /home/daytona/workspace/fabro/lib/crates/fabro-types/src/run_summary.rs, /home/daytona/workspace/fabro/lib/crates/fabro-types/src/timing.rs, /home/daytona/workspace/fabro/lib/crates/fabro-types/tests/run_failure_serde.rs, /home/daytona/workspace/fabro/lib/crates/fabro-workflow/src/billing_rollup.rs, /home/daytona/workspace/fabro/lib/crates/fabro-workflow/src/event/convert.rs, /home/daytona/workspace/fabro/lib/crates/fabro-workflow/src/event/events.rs, /home/daytona/workspace/fabro/lib/crates/fabro-workflow/src/lib.rs, /home/daytona/workspace/fabro/lib/crates/fabro-workflow/src/lifecycle/event.rs, /home/daytona/workspace/fabro/lib/crates/fabro-workflow/src/operations/archive.rs, /home/daytona/workspace/fabro/lib/crates/fabro-workflow/src/operations/start.rs, /home/daytona/workspace/fabro/lib/crates/fabro-workflow/src/pipeline/execute.rs, /home/daytona/workspace/fabro/lib/crates/fabro-workflow/src/pipeline/finalize.rs, /home/daytona/workspace/fabro/lib/crates/fabro-workflow/src/pipeline/pull_request.rs, /home/daytona/workspace/fabro/lib/crates/fabro-workflow/src/pipeline/types.rs, /home/daytona/workspace/fabro/lib/crates/fabro-workflow/src/run_lookup.rs, /home/daytona/workspace/fabro/lib/crates/fabro-workflow/src/test_support.rs


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