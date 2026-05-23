Goal: # Per-stage agent state in `StageProjection`

## Context

The agent-node stage pages (`/runs/<id>/stages/<sid>`) will get a new left sidebar showing: todo list, subagents, skills (available/activated), MCP servers, permissions, and (later) context-window breakdown.

Investigation showed:
- **Permissions** (`AgentPermissions`) are already on `RunAgentSettings` and returned by the run-settings endpoint — no work needed.
- **Todos, subagents, skills, MCP servers** all emit events today (`Event::Agent { stage, visit, session_id, event: AgentEvent::... }`) and the event envelope already carries `stage_id` (`lib/crates/fabro-workflow/src/event/stored_fields.rs:226`). The reducer just ignores most of them.
- **Todos** are partially projected — into a *run-level* `RunProjection.todos_by_list`. There's no cross-stage use case for that placement, and the existing keying (`anthropic_tasks:<root_session>`, `openai_plan:<session>`) is already 1:1 with stages.

This plan moves todos onto `StageProjection` and folds the other three event families into the same per-stage shape, all surfaced through the existing `GET /runs/{id}` endpoint. **No new endpoints, no parallel types.** The token-breakdown sidebar item is out of scope.

## Approach

### 1. Extend `StageProjection` (`lib/crates/fabro-types/src/run_projection.rs`)

Add four fields, reusing existing event payload types wherever possible:

```rust
pub struct StageProjection {
    // ...existing fields...
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub todos:       Option<TodoListProjection>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subagents:   Vec<SubAgentProjection>,
    #[serde(default, skip_serializing_if = "SkillsProjection::is_empty")]
    pub skills:      SkillsProjection,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mcp_servers: Vec<McpServerProjection>,
}
```

Three small new projection structs in the same file (avoid sprawl by colocating, not creating a new module):

```rust
pub struct SubAgentProjection {
    pub agent_id: String,
    pub depth:    usize,
    pub task:     String,
    pub status:   SubAgentStatus,  // Running | Completed{success, turns_used} | Failed{error} | Closed
}

pub struct SkillsProjection {
    pub available: Vec<AgentSkillSummary>,                // reused from run_event::agent
    pub activated: Vec<ActivatedSkill>,                   // { name: String, source: AgentSkillActivationSource }
}

pub struct McpServerProjection {
    pub server_name: String,
    pub status:      McpServerStatus,                     // Ready{tools: Vec<AgentMcpToolSummary>} | Failed{error}
    pub tool_count:  usize,
}
```

Reused as-is (no new types): `TodoListProjection` (`lib/crates/fabro-types/src/todo.rs:230`), `AgentSkillSummary`, `AgentSkillActivationSource`, `AgentMcpToolSummary` (`lib/crates/fabro-types/src/run_event/agent.rs:244,283,296`).

`SubAgentStatus` and `McpServerStatus` are projection-side enums — distinct from the runtime `fabro-agent::subagent::Status` (which is per-process and not in `fabro-types`). Don't pull that across the crate boundary.

### 2. Remove `RunProjection.todos_by_list`

Drop the field entirely (`run_projection.rs:40`) and its initializer (`:188`). No cross-stage aggregation use case exists. The only TS reference is a stale comment in `apps/fabro-web/app/lib/run-events.ts:100` — update or delete.

### 3. Teach the reducer (`lib/crates/fabro-store/src/run_state.rs`)

Existing pattern: agent events route to a stage via `stage_at_stored_or_visit` / `stage_at_stored_or_current_visit` helpers (lines 594–614). Apply the same pattern for the new variants:

- `TodoCreated/Updated/Deleted` — currently calls `apply_todo_created/updated/deleted` against `state.todos_by_list` (lines 456–504). Rewrite each to resolve the stage from `event.stage_id` and mutate `stage.todos: Option<TodoListProjection>`. Reuse the existing per-list upsert/patch/delete logic against a single `TodoListProjection` instead of a map.
- `SubAgentSpawned` → push a new `SubAgentProjection { status: Running }`
- `SubAgentCompleted` → find by `agent_id`, set `status: Completed{success, turns_used}`
- `SubAgentFailed` → find by `agent_id`, set `status: Failed{error}`
- `SubAgentClosed` → find by `agent_id`, set `status: Closed`
- `SkillsDiscovered` → set `stage.skills.available = props.skills`
- `SkillActivated` → push to `stage.skills.activated` (dedupe-by-name policy: keep the latest source, or append every activation? **default: append every activation** — matches event-sourced replay; UI can collapse if it wants)
- `McpServerReady` → upsert by `server_name`, status `Ready{tools}`, `tool_count = props.tool_count`
- `McpServerFailed` → upsert by `server_name`, status `Failed{error}`

### 4. Update OpenAPI + regenerated client (`docs/public/api-reference/fabro-api.yaml`)

Mirror the four fields onto the `StageProjection` schema. For the reused types (`TodoListProjection`, `AgentSkillSummary`, `AgentSkillActivationSource`, `AgentMcpToolSummary`), wire them through `lib/crates/fabro-api/build.rs` `with_replacement(...)` per CLAUDE.md "API type ownership" guidance so progenitor doesn't generate parallel `ApiFoo` types. Add `fabro-api` parity test entries.

Remove the `todos_by_list` field from the `RunProjection` schema in the same change.

The TS Axios client (`lib/packages/fabro-api-client`) regenerates via `bun run generate`.

### 5. Tests

- `lib/crates/fabro-store/src/run_state.rs` already has todo reducer tests at lines 3194–3383. Rewrite assertions to read from `state.stage(&stage_id).unwrap().todos` instead of `state.todos_by_list`.
- Add reducer tests for each new event family: emit the sequence and assert the projected state on `StageProjection`.
- Add a `fabro-api` test proving JSON parity for the new `StageProjection` schema (same pattern as existing `with_replacement` tests).
- No insta snapshots affected — confirmed none exist for these types.

### 6. Web UI consumption (out of scope for this plan, but unblocks)

Once the projection ships, the sidebar reads everything from the existing `GET /runs/{id}` response. The route already loads the projection. Sidebar component work is a follow-up.

## Files to modify

| File | Change |
|---|---|
| `lib/crates/fabro-types/src/run_projection.rs` | Add 4 fields + 3 new projection structs to `StageProjection`; remove `todos_by_list` from `RunProjection` |
| `lib/crates/fabro-store/src/run_state.rs` | Reroute todo handlers; add handlers for subagent/skill/MCP events; update tests |
| `docs/public/api-reference/fabro-api.yaml` | Mirror new fields on `StageProjection`; remove `todos_by_list` from `RunProjection` |
| `lib/crates/fabro-api/build.rs` | Add `with_replacement(...)` entries for reused types so progenitor reuses them |
| `apps/fabro-web/app/lib/run-events.ts` (comment only) | Delete the stale `todos_by_list` comment at line 100 |

## Verification

1. `cargo build --workspace` — compiles; progenitor regenerates without parallel API types.
2. `cargo nextest run -p fabro-types -p fabro-store -p fabro-api -p fabro-server` — reducer tests, projection parity, conformance tests pass.
3. `cd lib/packages/fabro-api-client && bun run generate && bun run typecheck` — TS client regenerates cleanly.
4. End-to-end: start `fabro server start`, run a workflow with an agent stage that creates todos, spawns a subagent, and activates a skill (use the `repl` workflow or similar). `curl /api/v1/runs/<id>` and confirm the new fields appear under `stages.<stage_id>` and that `todos_by_list` is gone from the top level.
5. `cargo +nightly-2026-04-14 fmt --check --all && cargo +nightly-2026-04-14 clippy --workspace --all-targets -- -D warnings` — formatting + lint clean.

## Anti-sprawl checks

- No new `ApiFoo` aliases or `foo_to_api`/`foo_from_api` adapters introduced.
- Reused: `TodoListProjection`, `AgentSkillSummary`, `AgentSkillActivationSource`, `AgentMcpToolSummary` (4 existing types).
- Added: `SubAgentProjection`, `SubAgentStatus`, `SkillsProjection`, `ActivatedSkill`, `McpServerProjection`, `McpServerStatus` — all colocated in `run_projection.rs`, none duplicate existing semantics. Projection-side status enums are intentionally distinct from runtime per-process status types in `fabro-agent`.
- One field removed (`RunProjection.todos_by_list`) — net structural simplification at the run level.

## Unresolved questions

- **Activation dedupe policy**: append every `SkillActivated` event vs. dedupe-by-name (keep latest source)? Plan defaults to append; flag if you want set semantics.
- **Subagent ordering**: insertion order vs. depth-then-spawn-order? Plan defaults to insertion order (matches event replay).
- **`AgentPermissions` ↔ `PermissionLevel` type duplication**: pre-existing, not introduced here. Worth a separate cleanup pass per CLAUDE.md "API type ownership" — out of scope for this plan.


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
  - Model: gpt-5.5, 223.5k tokens in / 34.3k out


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