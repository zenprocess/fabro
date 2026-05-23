Goal: # Context Window Breakdown Endpoint Plan

Date: 2026-05-23

## Context

Agent stage pages already have enough projected data for todos, subagents,
skills, and MCP servers through `StageProjection` in
`lib/crates/fabro-types/src/run_projection.rs` and the reducer in
`lib/crates/fabro-store/src/run_state.rs`. Context-window usage is different:
Fabro emits context-window warnings and compaction events today, but it does not
store a category breakdown of the model-visible request context.

`fabro-llm` already exposes the right counting primitive:
`Client::count_input_tokens(request, InputTokenCountPreference::PreferProvider)`
in `lib/crates/fabro-llm/src/client.rs`. Provider adapters can call native count
endpoints for OpenAI, Anthropic, and Gemini, and the client already falls back
to local estimates for fallback-eligible failures.

## Goal

Add a best-effort context-window API for agent stages:

```text
GET /api/v1/runs/{id}/stages/{stageId}/context-window
```

The endpoint should return the best context-window usage Fabro can produce with
no caller-controlled count or accuracy parameters. It may call the configured
LLM provider by default. If provider counting is not possible, it should degrade
to a local estimate or the latest stored snapshot instead of making the sidebar
treat ordinary count gaps as hard errors.

## Scope

In scope:

- OpenAPI contract and generated Rust/TypeScript clients.
- Content-free context-window DTOs in the run projection.
- A typed agent event for latest context-window snapshots.
- Server endpoint that combines live provider counting, local estimates, and
  stored-snapshot fallback.
- Web query key, hook, and SSE invalidation support so the future sidebar can
  consume the endpoint.

Out of scope:

- Building the full new left sidebar UI.
- Persisting raw prompt, memory, tool arguments, or message contents for later
  token counting.
- Adding user-visible count-mode or accuracy knobs.
- Retrofitting exact historical context-window counts for older completed runs.

Before implementing, read:

- `docs/internal/events-strategy.md`
- `docs/internal/error-handling-strategy.md`
- `docs/internal/testing-strategy.md`

## API Contract

Add the route under the existing Run Internals tag in
`docs/public/api-reference/fabro-api.yaml`:

```text
GET /runs/{id}/stages/{stageId}/context-window
```

Proposed response shape:

```json
{
  "stage_id": "implement@1",
  "available": true,
  "unavailable_reason": null,
  "provider": "openai",
  "model": "gpt-5.4",
  "context_window_tokens": 400000,
  "input_tokens": 123456,
  "usage_percent": 30.86,
  "count_method": "provider_api_scaled_breakdown",
  "staleness": "live",
  "generated_at": "2026-05-23T12:34:56Z",
  "event_seq": 42,
  "breakdown": [
    {
      "category": "system_prompt",
      "label": "System prompt",
      "tokens": 30000,
      "usage_percent": 7.5,
      "source": "scaled_local_estimate"
    }
  ],
  "warnings": []
}
```

Required schemas:

- `StageContextWindow`
- `StageContextWindowBreakdownItem`
- `StageContextWindowCategory`
- `StageContextWindowCountMethod`
- `StageContextWindowStaleness`
- `StageContextWindowUnavailableReason`
- `StageContextWindowWarning`

Enums:

```text
StageContextWindowCategory:
  system_prompt
  tools
  mcp_tools
  skills
  memory
  conversation
  other

StageContextWindowCountMethod:
  provider_api_scaled_breakdown
  response_usage_scaled_breakdown
  local_estimate

StageContextWindowStaleness:
  live
  stored
  unavailable

StageContextWindowUnavailableReason:
  not_agent_stage
  not_observed
  provider_unconfigured
```

Use `available: false` for a real run/stage where Fabro has no context-window
data yet. Missing runs and missing stages should still return 404. For
`available: false`, return `breakdown: []`, `warnings` explaining the gap, and
nullable token fields.

Define context-window usage as model-visible input/context tokens only:

- Include prompt input, system/developer content, tool definitions, MCP tool
  definitions, skills, memory files, conversation history, tool results, and
  cache input tokens when response usage is the source.
- Exclude output tokens and reasoning tokens.
- Do not report cost/billing totals here. Existing billing APIs own billing.

## Data Model

Add projection-only, content-free context-window types to
`lib/crates/fabro-types/src/run_projection.rs`:

- `StageContextWindowProjection`
- `StageContextWindowBreakdownProjection`
- category/count/staleness/warning enums, shared with the API through
  `fabro-api` replacements if the serde shape matches.

Extend `StageProjection` with:

```rust
#[serde(default, skip_serializing_if = "Option::is_none")]
pub context_window: Option<StageContextWindowProjection>,
```

Do not store raw content. The stored snapshot may contain only:

- provider and model
- context window size
- input token total
- category token counts
- count method
- generated timestamp
- source event sequence
- warning codes and messages

## Events

Add a typed event in `lib/crates/fabro-types/src/run_event/agent.rs` and
`lib/crates/fabro-types/src/run_event/mod.rs`:

```text
agent.context_window.snapshot
```

Event payload should carry the same content-free counts as the projection plus
the stage id. The reducer in `lib/crates/fabro-store/src/run_state.rs` should
replace the selected stage's `context_window` with the latest snapshot.

This event is the durable fallback for inactive stages. It should be emitted
when the agent assembles or refreshes the LLM request context, before the
provider request is sent. If a provider response later supplies better input
usage for the same request, emit another snapshot using
`response_usage_scaled_breakdown`.

## Live Counting Design

The agent should produce snapshots using this decision order whenever it builds
an LLM request:

1. Build a content-free local category breakdown from the same inputs used to
   assemble the `fabro_llm::Request`.
2. Emit an immediate `agent.context_window.snapshot` with `local_estimate` so a
   sidebar has data even if provider counting is slow or unavailable.
3. Attempt provider counting with
   `Client::count_input_tokens(..., InputTokenCountPreference::PreferProvider)`
   using the exact in-memory request. This work must not persist or log the raw
   request.
4. If provider count succeeds, scale the local category estimates to the
   provider total and emit a replacement snapshot with
   `provider_api_scaled_breakdown`.
5. If provider count falls back or fails, keep the local snapshot and include a
   warning code on the next emitted snapshot. Count failures must not block the
   agent's normal LLM request.

The API endpoint should follow this decision order:

1. Validate the run and stage exist.
2. If the stage is not an agent stage, return `available: false` with
   `unavailable_reason: not_agent_stage`.
3. Return the latest projected `StageContextWindowProjection`.
4. If no snapshot has ever been observed, return `available: false` with
   `unavailable_reason: not_observed`.

Do not make the HTTP server own raw LLM requests. The existing server state
tracks live run control and durable projections, while the exact request exists
inside the active agent session. Provider counting should therefore happen in
the agent/worker process at request-assembly time, and the server endpoint
should expose the latest durable snapshot.

If a future implementation needs user-triggered refreshes, add a separate
worker request/response control path. Do not tunnel raw request content through
run events or store it in `ManagedRun`.

The live request snapshot needs to be short-lived and content-safe:

- Hold raw `Request` content only inside active agent sessions.
- Never write that raw request to run events, projection state, logs, or API
  responses.
- Cache provider-count results by run id, stage id, provider, model, and request
  fingerprint or source event seq so each LLM request is counted at most once.
- Clear the live request handle when the session/stage deactivates.

## Resolved Handoff Decisions

### Decision 1: Category-Aware Estimation Ownership

Options:

- Agent-only estimator: build all category counts in `fabro-agent`.
- LLM-only estimator: move the full breakdown model into `fabro-llm`.
- Hybrid estimator: keep category ownership in `fabro-agent`, but expose small
  reusable token-estimation helpers from `fabro-llm`.

Recommendation: use the hybrid estimator.

Justification:

- `fabro-agent` has the category knowledge. It sees memory documents, skills,
  MCP registration, tool registry policy, and the final session history before
  `Session::build_request` flattens everything into a generic LLM request.
- `fabro-llm` has the token math and provider-neutral request structures. It
  already owns local count behavior in `token_count.rs`, so duplicating that
  estimator in `fabro-agent` would drift.
- `fabro-llm` should not learn Fabro-specific categories like `skills` or
  `memory`; that would couple a provider abstraction crate to agent UI
  semantics.

Implementation guidance:

- Add a `fabro-agent/src/context_window.rs` builder that owns the category
  taxonomy and content-free snapshot assembly.
- Expose narrow helpers from `fabro-llm::token_count`, such as tool-definition
  and message/content-part estimators, instead of making the private estimator
  logic public wholesale.
- Keep the existing `Client::count_input_tokens` provider call as the
  authoritative total when available.

### Decision 2: Provider Count Location

Options:

- Server-side count: store or reconstruct the exact `fabro_llm::Request` in the
  HTTP server and call the provider from the endpoint.
- Synchronous worker query: add a request/response control channel so the HTTP
  server can ask the active worker for a fresh count on demand.
- Agent-side count on request assembly: the active session counts the exact
  request it already has and emits content-free snapshots; the endpoint returns
  the latest projection.

Recommendation: use agent-side count on request assembly for the first
implementation.

Justification:

- It satisfies the requirement that provider counting is attempted by default,
  because every active agent request can be counted once as it is assembled.
- It avoids moving raw prompts, memory, tool results, or message history into
  server-managed state.
- The existing subprocess control path is one-way JSONL for actions like steer,
  interrupt, and pair. Adding a synchronous query path just for this read model
  would be more complex than emitting the durable projection the UI already
  needs.
- It works for both subprocess and in-process runs because the agent session is
  the common place where the exact request exists.

Implementation guidance:

- Emit a local snapshot immediately, then emit a provider-scaled replacement if
  provider counting succeeds.
- Do not delay the LLM stream on provider counting unless implementation finds
  that provider count latency is consistently negligible. A spawned count task
  with the cloned request is acceptable as long as it is cancelled/ignored when
  the session closes.
- Treat provider count errors as snapshot warnings, not stage failures.

### Decision 3: Memory and Skills Attribution

Options:

- Keep the current flattened system prompt and count all prompt additions as
  `system_prompt`.
- Refactor prompt assembly to retain component boundaries, then estimate
  memory and skills before the final prompt string is concatenated.
- Add origin metadata to every history message and tool result so activated
  skill instructions can be attributed even after entering the conversation.

Recommendation: refactor prompt assembly for this first slice; defer full
history-origin metadata.

Justification:

- `assemble_system_prompt` already receives `memory` and `skills` separately,
  then concatenates them with the core prompt. Returning component metadata from
  that boundary is a small, local change.
- This gives useful and accurate first-slice attribution for loaded memory
  files and the available-skills prompt without changing the persisted
  conversation format.
- Activated skill instructions are harder: slash expansion becomes a user turn,
  and `use_skill` returns a tool result. The current `Message` enum does not
  preserve source metadata. Adding it is possible, but it is a broader history
  serialization migration and should not block the endpoint.

Implementation guidance:

- Count the base/core prompt as `system_prompt`.
- Count memory document text appended by prompt assembly as `memory`.
- Count the available-skills section and the `use_skill` tool definition as
  `skills`.
- Count slash-expanded skill templates and `use_skill` tool results as
  `conversation` in this first implementation, with a warning such as
  `activated_skill_context_counted_as_conversation` when such activations are
  present.

### Decision 4: Tool vs MCP Tool Attribution

Options:

- Split MCP tools by name prefix, such as `mcp__`.
- Add source metadata to `RegisteredTool` / `ToolRegistry`.
- Recompute MCP membership from `McpConnectionManager` at request time.

Recommendation: add source metadata to `RegisteredTool` / `ToolRegistry`.

Justification:

- The current registry stores only `ToolDefinition` plus executor, so origin is
  lost after registration.
- Prefix-based classification matches today's naming convention but is brittle
  and will misclassify any future native tool that shares the prefix or any MCP
  naming change.
- `McpConnectionManager` has source knowledge during registration, but the
  request builder only sees the final registry and policy-filtered tool
  definitions.

Implementation guidance:

- Add a small `ToolSource` enum, for example `Native`, `Mcp { server_name }`,
  and `Skill`.
- Set `ToolSource::Mcp` in `mcp_integration::make_mcp_tools`.
- Set `ToolSource::Skill` for `make_use_skill_tool`.
- Keep existing public `definitions()` behavior unchanged; add a parallel
  method that returns definitions with source metadata for context-window
  accounting.

### Decision 5: Unavailable and Error Semantics

Options:

- Return 404/409 for non-agent stages or stages with no snapshot.
- Return 200 with `available: false` for known stages where context-window data
  is not applicable or not observed.
- Return partial data with warnings for provider-count failures.

Recommendation: return 200 with `available: false` for known-but-unavailable
data, and reserve HTTP errors for missing run/stage or malformed requests.

Justification:

- The sidebar needs to render stable empty states without treating normal
  projection gaps as transport errors.
- Provider-count support varies by provider and credentials; those are data
  quality issues, not endpoint availability issues.
- This matches the broader best-effort contract and avoids UI retry loops when
  a completed older run simply has no context-window snapshot.

Implementation guidance:

- Use 404 only for missing run or missing stage.
- Use `available: false` + `unavailable_reason` for `not_agent_stage`,
  `not_observed`, or `provider_unconfigured`.
- Use `warnings` for local estimate, provider fallback, ambiguous categories,
  and activated skill content counted as conversation.

The previous open questions are resolved by these decisions.

## Implementation Units

### Unit 1: OpenAPI and Generated Clients

Files:

- `docs/public/api-reference/fabro-api.yaml`
- `lib/crates/fabro-api/build.rs`
- `lib/crates/fabro-api/tests/`
- `lib/packages/fabro-api-client/`

Tasks:

- Add the path and schemas under Run Internals.
- Prefer reusing hand-written Rust projection types through
  `with_replacement(...)` when serde shape and semantics are identical.
- Add a `fabro-api` test proving type identity and JSON parity for any new
  replacement.
- Regenerate Rust and TypeScript clients.

Tests:

- `cargo build -p fabro-api`
- `cargo nextest run -p fabro-api`
- `cd lib/packages/fabro-api-client && bun run generate`

### Unit 2: Content-Free Snapshot Builder

Files:

- `lib/crates/fabro-llm/src/token_count.rs`
- `lib/crates/fabro-agent/src/context_window.rs` (new)
- `lib/crates/fabro-agent/src/session.rs`
- `lib/crates/fabro-agent/src/profiles/mod.rs`
- `lib/crates/fabro-agent/src/tool_registry.rs`
- `lib/crates/fabro-agent/src/mcp_integration.rs`
- `lib/crates/fabro-agent/src/skills.rs`
- `lib/crates/fabro-agent/src/compaction.rs`
- `lib/crates/fabro-agent/src/lib.rs`

Tasks:

- Build category estimates at the same boundary where `Session::build_request`
  assembles the `fabro_llm::Request`.
- Expose narrow reusable token-estimation helpers from `fabro-llm` rather than
  duplicating the estimator in `fabro-agent`.
- Refactor prompt assembly enough to retain component boundaries for core
  system prompt, memory, available skills, and user instructions before the
  final prompt string is concatenated.
- Add source metadata to registered tools so the builder can split native tools,
  MCP tools, and skill-related tools after policy filtering.
- Classify the system prompt separately from conversation history.
- Count slash-expanded skill templates and `use_skill` tool results as
  `conversation` for the first implementation, with a warning when activated
  skill context is present.
- Emit a local snapshot immediately and a provider-scaled replacement snapshot
  when provider counting succeeds.
- Keep local estimates deterministic and content-free.

Tests:

- system prompt, tools, MCP tools, skills, memory, conversation, and other
  categories are counted into the expected buckets.
- category totals equal the local total before provider scaling.
- provider-scaled totals add up to the provider total.
- snapshots do not serialize prompt text, memory contents, tool arguments, or
  message text.
- opaque/ambiguous inputs produce warnings rather than silent misclassification.
- provider count failure does not fail the agent turn.
- each request fingerprint is counted by the provider at most once.

### Unit 3: Event and Projection

Files:

- `lib/crates/fabro-types/src/run_event/agent.rs`
- `lib/crates/fabro-types/src/run_event/mod.rs`
- `lib/crates/fabro-types/src/run_projection.rs`
- `lib/crates/fabro-store/src/run_state.rs`

Tasks:

- Add `agent.context_window.snapshot`.
- Emit snapshots from the agent session path when requests are assembled and
  when later response usage improves the count.
- Project the latest snapshot onto `StageProjection.context_window`.
- Preserve backwards-compatible deserialization for run projections that do not
  have the new field.

Tests:

- event `type_name()` returns `agent.context_window.snapshot`.
- reducer updates only the matching stage.
- later snapshots replace earlier snapshots for the same stage.
- old projection JSON without `context_window` still deserializes.

### Unit 4: Server Endpoint

Files:

- `lib/crates/fabro-server/src/server/handler/mod.rs`
- `lib/crates/fabro-server/src/server/handler/runs.rs` or a new
  `context_window.rs` handler module
- `lib/crates/fabro-server/src/server/tests.rs`

Tasks:

- Add `GET /runs/{id}/stages/{stageId}/context-window`.
- Use the same run-scoped authorization pattern as adjacent run internals.
- Resolve run/stage from the cached projection first for fast 404s and stored
  fallback.
- Return the latest projected context-window snapshot for the stage.
- Return `available: false` for known stages where context-window data is not
  applicable or has not been observed.
- Return 404 only for missing run/stage. Provider count failures are represented
  as snapshot warnings because provider counting happens in the agent.

Tests:

- missing run returns 404.
- missing stage returns 404.
- non-agent stage returns 200 with `available: false`.
- projected provider-count success returns `staleness: live` and
  `count_method: provider_api_scaled_breakdown`.
- inactive stage returns the latest stored projection snapshot.
- no observed snapshot returns `available: false` with `not_observed`.
- projected warning payloads are returned without changing HTTP status.

### Unit 5: Web Query Support

Files:

- `apps/fabro-web/app/lib/query-keys.ts`
- `apps/fabro-web/app/lib/queries.ts`
- `apps/fabro-web/app/lib/run-events.ts`
- `apps/fabro-web/app/lib/query-keys.test.ts`
- `apps/fabro-web/app/lib/run-events.test.tsx`

Tasks:

- Add `queryKeys.runs.stageContextWindow(id, stageId)`.
- Add `useRunStageContextWindow(runId, stageId)` using the generated
  TypeScript client.
- Invalidate the context-window key for:
  - `agent.context_window.snapshot`
  - stage lifecycle events for the same stage
  - agent activity events that can change the request context
- Keep the full sidebar UI as follow-up work, but make the hook ready for the
  agent-node page.

Tests:

- query key encodes run id and stage id stably.
- snapshot event invalidates the context-window key, run events, and stage
  events.
- stage lifecycle events invalidate the context-window key for the selected
  stage.
- activity events without a stage id do not invalidate unrelated stage context
  windows.

## Security and Privacy

- Do not persist raw request content to make inactive-stage provider counting
  possible.
- Do not log prompt, memory, tool args, or message contents while computing
  counts.
- Warning messages should identify count quality, not repeat provider error
  bodies if those bodies may contain request excerpts.
- The endpoint should expose counts and category labels only.

## Validation

Expected validation after implementation:

```bash
cargo build -p fabro-api
cargo nextest run -p fabro-api -p fabro-agent -p fabro-store -p fabro-server
cd apps/fabro-web && bun test
cd apps/fabro-web && bun run typecheck
cargo +nightly-2026-04-14 fmt --check --all
git diff --check
```

## Remaining Follow-Ups

- Full sidebar visualization.
- Optional history-origin metadata if we later want activated skill templates
  to move from `conversation` into `skills`.
- Optional user-triggered live refresh path if future product needs require
  provider counting on demand rather than at request assembly time.


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
  - Model: gpt-5.5, 919.5k tokens in / 65.6k out
  - Files: /home/daytona/workspace/fabro/lib/crates/fabro-agent/src/context_window.rs, /home/daytona/workspace/fabro/lib/packages/fabro-api-client/src/models/stage-context-window-breakdown-item.ts, /home/daytona/workspace/fabro/lib/packages/fabro-api-client/src/models/stage-context-window-category.ts, /home/daytona/workspace/fabro/lib/packages/fabro-api-client/src/models/stage-context-window-count-method.ts, /home/daytona/workspace/fabro/lib/packages/fabro-api-client/src/models/stage-context-window-projection.ts, /home/daytona/workspace/fabro/lib/packages/fabro-api-client/src/models/stage-context-window-staleness.ts, /home/daytona/workspace/fabro/lib/packages/fabro-api-client/src/models/stage-context-window-unavailable-reason.ts, /home/daytona/workspace/fabro/lib/packages/fabro-api-client/src/models/stage-context-window-warning.ts, /home/daytona/workspace/fabro/lib/packages/fabro-api-client/src/models/stage-context-window.ts


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