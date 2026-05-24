Goal: ---
title: feat: StageProjection agent tools API
type: feat
status: active
date: 2026-05-24
---

# feat: StageProjection Agent Tools API

## Overview

Expose the complete effective tool list for each agent-backed stage through `StageProjection`, so UI and API consumers can show actual tools such as `apply_patch`, `grep`, `glob`, `read_file`, MCP tools, skill tools, and subagent tools without inferring them from `permission_level`.

The API should expose tool summaries with `name`, `description`, `source`, `category`, and `invoked`. It must not expose full JSON parameter schemas in the run projection.

## Problem Frame

The stage sidebar currently has permission metadata such as "Full access", but `permission_level` is only an access mode. It does not tell consumers which tools were actually exposed to the model after provider profile setup, optional tool registration, MCP integration, and tool access policy filtering.

The authoritative list already exists at request-build time in the agent session registry. The API needs to capture that effective list once per stage session and project it onto the stage.

## Requirements Trace

- R1. Add a StageProjection API field containing the complete effective tools for a stage session.
- R2. Include `name`, `description`, `source`, `category`, and `invoked` for each tool.
- R3. Do not infer tools from `permission_level` in backend or frontend code.
- R4. Do not expose full tool parameter schemas through StageProjection.
- R5. Preserve existing `permission_level` and `mcp_servers` fields for compatibility.
- R6. Mark individual tools as invoked when matching `agent.tool.started` events are projected.
- R7. Keep legacy runs backward compatible by defaulting missing tool lists to empty.

## Scope Boundaries

- Do not remove or rename `StageProjection.permission_level`.
- Do not remove the existing MCP server projection or `AgentMcpToolSummary`.
- Do not change completion API tool definitions.
- Do not add ACP-native tool discovery in this pass unless an ACP path already has an equivalent effective tool list available.
- Do not render parameter schemas in the web UI.

## Context & Research

### Relevant Code and Patterns

- OpenAPI is the source of truth for HTTP contracts in `docs/public/api-reference/fabro-api.yaml`.
- Shared API/projection DTOs should live in `fabro-types`, with `fabro-api/build.rs` replacements to avoid duplicate generated Rust types.
- `StageProjection` lives in `lib/crates/fabro-types/src/run_projection.rs`.
- Durable run event props live in `lib/crates/fabro-types/src/run_event/agent.rs` and `lib/crates/fabro-types/src/run_event/mod.rs`.
- Workflow event conversion and event names live in `lib/crates/fabro-workflow/src/event/convert.rs`, `events.rs`, and `names.rs`.
- Projection replay lives in `lib/crates/fabro-store/src/run_state.rs`.
- The effective request tool list is built in `lib/crates/fabro-agent/src/session.rs` from `ToolRegistry::definitions_with_source_for_policy`.
- Tool source metadata already exists as `ToolSource` and `ToolDefinitionWithSource` in `lib/crates/fabro-agent/src/tool_registry.rs`.
- Tool category mapping already exists in `lib/crates/fabro-agent/src/tool_permissions.rs`.
- The sidebar display lives in `apps/fabro-web/app/components/stage-insights-sidebar.tsx`.

### Strategy Docs

- Read `docs/internal/events-strategy.md` before adding the new durable event.
- Read `docs/internal/testing-strategy.md` before adding or reorganizing tests.
- Follow the OpenAPI type ownership guidance in `AGENTS.md`: reuse `fabro-types` through `fabro-api/build.rs` replacements when the API schema has the same product meaning and serde shape.

## Key Technical Decisions

- Add a new StageProjection field named `agent_tools`, not `tools`, to avoid ambiguity with MCP nested tools and completion tool definitions.
- Add a dedicated durable event named `agent.tools.available` instead of overloading `agent.session.activated`.
- Capture the effective tool list after session setup and filtering, using the same path as model request construction.
- Store descriptions in the summary because they are useful API/UI metadata; omit parameter schemas to keep projection payloads small and avoid leaking full implementation detail.
- Keep `AgentMcpToolSummary` MCP-only. Add a new general-purpose `AgentToolSummary` instead of stretching the MCP type beyond its meaning.
- Treat `invoked` as projected state. The availability event should emit tools with `invoked: false`; replay of `agent.tool.started` flips matching tools to true.

## API Contract

Add these schemas to OpenAPI and map them to `fabro_types` replacements:

- `AgentToolSummary`
  - required: `name`, `description`, `source`, `category`, `invoked`
  - `name`: exposed tool name, e.g. `apply_patch` or `mcp__filesystem__read_file`
  - `description`: model-facing tool description
  - `source`: `AgentToolSource`
  - `category`: `AgentToolCategory`
  - `invoked`: boolean
- `AgentToolSource`
  - tagged by `kind`
  - `native`
  - `mcp` with `server_name` and `original_name`
  - `skill`
- `AgentToolCategory`
  - enum: `read`, `write`, `shell`, `subagent`, `other`
- `AgentToolsAvailableProps`
  - required: `tools`, `visit`
  - `tools`: array of `AgentToolSummary`
  - `visit`: stage visit number

Add to `StageProjection`:

- `agent_tools`: array of `AgentToolSummary`
- Default to an empty array when omitted.
- Skip serializing when empty, matching existing projection optional-list style.

Add event body:

- Serialized event name: `agent.tools.available`
- Event body type: `AgentToolsAvailableProps`

## Implementation Units

- [ ] **Unit 1: Add shared tool summary types**

**Goal:** Define the canonical API/projection DTOs in `fabro-types`.

**Files:**
- Modify: `lib/crates/fabro-types/src/run_event/agent.rs`
- Modify: `lib/crates/fabro-types/src/run_event/mod.rs`
- Modify: `lib/crates/fabro-types/src/run_projection.rs`
- Modify: `lib/crates/fabro-types/src/lib.rs`

**Work:**
- Add `AgentToolSummary`, `AgentToolSource`, `AgentToolCategory`, and `AgentToolsAvailableProps`.
- Add `EventBody::AgentToolsAvailable` serialized as `agent.tools.available`.
- Add `agent_tools: Vec<AgentToolSummary>` to `StageProjection`.
- Ensure all new fields default cleanly for older persisted events/projections.
- Export the new public types from `fabro-types`.

- [ ] **Unit 2: Capture effective session tools**

**Goal:** Provide an authoritative one-time source for the list that the model can actually call.

**Files:**
- Modify: `lib/crates/fabro-agent/src/session.rs`
- Modify: `lib/crates/fabro-workflow/src/handler/llm/api.rs`
- Modify: `lib/crates/fabro-workflow/src/event/events.rs`
- Modify: `lib/crates/fabro-workflow/src/event/names.rs`
- Modify: `lib/crates/fabro-workflow/src/event/convert.rs`
- Modify: `lib/crates/fabro-workflow/src/event/stored_fields.rs` only if the new event needs non-standard stored fields.

**Work:**
- Add `Session::available_tools()` that returns the same effective `ToolDefinitionWithSource` list as `build_request()`.
- Map `ToolDefinitionWithSource` to `AgentToolSummary` at the workflow boundary.
- Populate `description` from `ToolDefinition.description`.
- Populate `source` from `ToolSource`.
- For MCP tools, include the server name from `ToolSource::Mcp` and derive `original_name` from the qualified exposed name using the existing MCP naming convention.
- Populate `category` from the existing tool category mapping for known exposed names; use `other` when no category mapping applies.
- Emit `agent.tools.available` once for the stage session after session setup and filtering are complete.

- [ ] **Unit 3: Project available and invoked tools**

**Goal:** Make `StageProjection.agent_tools` replay-authoritative.

**Files:**
- Modify: `lib/crates/fabro-store/src/run_state.rs`

**Work:**
- On `EventBody::AgentToolsAvailable`, replace the current stage visit's `agent_tools` with the event tools.
- On `EventBody::AgentToolStarted`, find a matching `agent_tools` entry by exposed `tool_name` and set `invoked = true`.
- Keep the existing MCP server `invoked` update unchanged.
- If a legacy run has no availability event, do not synthesize a full list from permissions.

- [ ] **Unit 4: Update OpenAPI and generated clients**

**Goal:** Expose the new projection field and event contract through public API clients without duplicate Rust API types.

**Files:**
- Modify: `docs/public/api-reference/fabro-api.yaml`
- Modify: `lib/crates/fabro-api/build.rs`
- Modify: `lib/crates/fabro-api/src/lib.rs`
- Modify generated files under `lib/crates/fabro-api/src/generated.rs` via `cargo build -p fabro-api`.
- Modify generated files under `lib/packages/fabro-api-client/src` via TypeScript client generation.

**Work:**
- Add the OpenAPI schemas listed in the API Contract section.
- Add `StageProjection.agent_tools`.
- Add `fabro-api/build.rs` replacements for the new `fabro-types` types.
- Regenerate Rust API code.
- Regenerate TypeScript API client code.

- [ ] **Unit 5: Render tools in the web sidebar**

**Goal:** Replace the permission-derived sidebar display with the actual projected tool list.

**Files:**
- Modify: `apps/fabro-web/app/components/stage-insights-sidebar.tsx`
- Modify: `apps/fabro-web/app/components/stage-insights-sidebar.test.tsx`

**Work:**
- Render `stage.agent_tools` when present.
- Show each tool's name, description, source/category, and invoked state.
- Keep permission level as secondary metadata or fallback for legacy stages with no `agent_tools`.
- Do not infer tool availability from permission level.

## Test Plan

- `fabro-types`
  - JSON round-trip for `agent.tools.available`.
  - Serialization checks for `AgentToolSource` and `AgentToolCategory`.
  - Backward compatibility check that missing `agent_tools` deserializes to an empty list.

- `fabro-workflow`
  - Event name and conversion tests for `agent.tools.available`.
  - Capture test proving native tools such as `apply_patch`, `grep`, and `glob` are emitted from the effective registry path.
  - MCP source mapping test for a qualified MCP tool name.

- `fabro-store`
  - Projection test that `agent.tools.available` populates `StageProjection.agent_tools`.
  - Projection test that `agent.tool.started` marks only the matching tool as invoked.
  - Regression test that MCP server `invoked` status still updates as before.

- `fabro-api`
  - Type identity/parity tests confirming API types reuse `fabro_types::AgentToolSummary`, `AgentToolSource`, `AgentToolCategory`, and `AgentToolsAvailableProps`.
  - StageProjection round-trip test including `agent_tools`.

- `fabro-web`
  - Sidebar test rendering tool names and descriptions from `stage.agent_tools`.
  - Sidebar test showing invoked state.
  - Legacy fallback test for stages without `agent_tools`.

## Run Checks

- `cargo +nightly-2026-04-14 fmt --check --all`
- `cargo nextest run -p fabro-types -p fabro-workflow -p fabro-store -p fabro-api`
- `cargo build -p fabro-api`
- `cd lib/packages/fabro-api-client && bun run generate`
- `cd apps/fabro-web && bun test && bun run typecheck`

## Assumptions

- The first implementation targets normal API-backed agent sessions, not ACP-native sessions.
- `description` is safe to expose because it is already model-facing tool metadata, but parameter schemas remain out of scope for StageProjection.
- `agent_tools` is a complete list only for runs that emit `agent.tools.available`; legacy runs return an empty list and may still show existing permission/MCP metadata.
- If tool registration becomes mutable later, the event contract can be re-emitted and projection replacement semantics will still work.


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
  - Model: gpt-5.5, 6.7m tokens in / 34.2k out


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