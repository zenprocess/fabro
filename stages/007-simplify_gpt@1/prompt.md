Goal: # Output Schema Validation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `output_schema` validation for agent and prompt nodes, with context-preserving repair turns when structured output does not validate.

**Architecture:** Introduce a small structured-output layer in `fabro-workflow` that resolves node-level schema declarations, extracts JSON output, validates it, and produces either routing side effects or a parsed custom output context update. Agent and prompt execution must perform schema repair inside the active LLM conversation instead of using the workflow executor retry path.

**Tech Stack:** Rust, Graphviz workflow attrs, `serde_json`, workspace `jsonschema`, existing `fabro-llm::ResponseFormat`, Fabro agent sessions, `cargo nextest`.

---

## Public Interface

Workflow authors can opt in on agent and prompt nodes:

```dot
review [
  shape=tab,
  output_schema="routing",
  output_retries=2
]

audit [
  shape=tab,
  output_schema="@schemas/audit-result.schema.json",
  output_retries=2
]
```

- `output_schema="routing"` uses Fabro's built-in routing directive schema.
- `output_schema="@path/to/schema.json"` loads a JSON Schema file through existing workflow file-reference rules.
- `output_retries` controls corrective turns inside the same node execution. Default: `2`. `0` means validate once and fail without a repair turn.
- Schema failures are terminal node failures after `output_retries` is exhausted. They are not `retry_requested` outcomes and do not consume `max_retries`.
- `backend="acp"` with `output_schema` is unsupported in v1 and returns a clear validation error.

## Implementation Tasks

### Task 1: Node Attributes And File Reference Resolution

**Files:**
- Modify: `lib/crates/fabro-types/src/graph.rs`
- Modify: `lib/crates/fabro-workflow/src/static_reference.rs`
- Modify: `lib/crates/fabro-workflow/src/transforms/file_inlining.rs`
- Test: existing unit tests in those files

- [ ] Add `Node::output_schema(&self) -> Option<&str>` next to other agent/prompt attrs.
- [ ] Add `Node::output_retries(&self) -> i64` returning `self.int_attr("output_retries").unwrap_or(2).max(0)`.
- [ ] Teach static reference validation that node attr `output_schema` values starting with `@` are file inline references.
- [ ] Extend file inlining so `output_schema="@schemas/foo.json"` is replaced with the schema file contents before execution, while `output_schema="routing"` stays unchanged.
- [ ] Add tests for absent attrs, default retries, zero retries, file inlining, and unresolved schema reference diagnostics.

Run:

```bash
cargo nextest run -p fabro-types -p fabro-workflow graph:: file_inlining static_reference
```

Expected: targeted tests pass.

### Task 2: Structured Output Module

**Files:**
- Create: `lib/crates/fabro-workflow/src/handler/structured_output.rs`
- Modify: `lib/crates/fabro-workflow/src/handler/mod.rs`
- Modify: `lib/crates/fabro-workflow/Cargo.toml`
- Test: unit tests in `structured_output.rs`

- [ ] Add `jsonschema.workspace = true` to `fabro-workflow` dependencies.
- [ ] Define `OutputSchemaKind` with `Routing` and `JsonSchema { schema: serde_json::Value }`.
- [ ] Parse `node.output_schema()` into `None`, `Routing`, or custom JSON Schema. Treat literal `routing` as the only built-in keyword.
- [ ] Add a built-in routing schema requiring an object with at least one recognized field: `preferred_next_label`, `outcome`, `failure_reason`, `suggested_next_ids`, or `context_updates`.
- [ ] Reuse balanced-object scanning semantics for response text: validate the last JSON object that is relevant to the selected schema.
- [ ] Return a structured validation result containing the parsed JSON object, concise error messages, and enough information to build a repair prompt.
- [ ] Add tests for valid routing JSON, missing routing fields, wrong routing field types, valid custom schema, invalid custom schema, invalid JSON, and no JSON object.

Run:

```bash
cargo nextest run -p fabro-workflow structured_output
```

Expected: structured-output unit tests pass.

### Task 3: Routing Extraction Compatibility

**Files:**
- Modify: `lib/crates/fabro-workflow/src/handler/agent.rs`
- Test: existing agent handler unit tests

- [ ] Keep the loose default unchanged when `output_schema` is absent.
- [ ] Move current `STATUS_FIELDS`, balanced JSON scanning, and routing-field application behind reusable functions in `structured_output.rs` or call the new module from `agent.rs`.
- [ ] For `output_schema="routing"`, require schema-valid routing JSON and surface validation failures for repair instead of silently ignoring bad candidates.
- [ ] Preserve existing routing fallback priority for agent nodes: response text first, then `status.json`, then last file touched.
- [ ] Keep prompt-node routing behavior response-only unless later tasks explicitly add prompt `status.json` support.

Run:

```bash
cargo nextest run -p fabro-workflow handler::agent
```

Expected: existing loose routing tests still pass, plus new strict routing tests pass.

### Task 4: Prompt Node Same-Context Repair

**Files:**
- Modify: `lib/crates/fabro-workflow/src/handler/llm/api.rs`
- Modify: `lib/crates/fabro-workflow/src/handler/prompt.rs`
- Test: prompt/API backend tests in those files

- [ ] In `AgentApiBackend::one_shot`, keep `messages` mutable across attempts.
- [ ] When a prompt node has a custom JSON Schema, set `response_format=JsonSchema` on the initial and repair LLM requests. For `routing`, use `JsonObject` or no provider-native schema if provider behavior would conflict with Fabro's routing extraction.
- [ ] After each LLM response, validate according to `output_schema`.
- [ ] On validation failure with repair attempts remaining, append `Message::assistant(response.text())`, then append a corrective `Message::user(repair_message)`, and call `client.complete` again with the same messages.
- [ ] On success, return the validated response text and aggregate usage across all attempts.
- [ ] On exhaustion, return a terminal failed outcome with failure reason `output schema validation failed after N repair attempt(s)`.
- [ ] Update `PromptHandler` so validated custom output is added to `context_updates["output.{node_id}"]`; routing output still updates outcome routing fields.

Run:

```bash
cargo nextest run -p fabro-workflow handler::prompt handler::llm::api
```

Expected: prompt repair keeps previous assistant output in the message list and succeeds after a corrective response.

### Task 5: Agent Node Same-Session Repair

**Files:**
- Modify: `lib/crates/fabro-workflow/src/handler/llm/api.rs`
- Modify: `lib/crates/fabro-workflow/src/handler/agent.rs`
- Test: agent/API backend tests in those files

- [ ] In `AgentApiBackend::run`, validate the final assistant response before releasing, closing, or caching the session.
- [ ] On validation failure with repair attempts remaining, call `session.process_input(repair_message)` on the same `Session`.
- [ ] Recompute the final assistant response after each repair turn from `session.history()`.
- [ ] Aggregate usage across all new assistant turns, including repair turns, without double-counting reused session history.
- [ ] Do not set provider-native `response_format` for agent sessions in v1, because agent sessions may need normal tool-use messages before final output.
- [ ] Return terminal failure after exhaustion; do not return a retryable backend error and do not request workflow node retry.
- [ ] Update `AgentHandler` to apply validated routing/custom output to the final `Outcome`.

Run:

```bash
cargo nextest run -p fabro-workflow handler::agent handler::llm::api
```

Expected: agent repair sends a second `process_input` to the same session and final validated output drives outcome/context updates.

### Task 6: ACP Guardrail

**Files:**
- Modify: `lib/crates/fabro-workflow/src/handler/llm/acp.rs`
- Test: ACP backend tests in that file

- [ ] At the start of `AgentAcpBackend::run`, reject nodes where `node.output_schema().is_some()`.
- [ ] Use a clear error message: `output_schema is not supported with backend="acp" in this release`.
- [ ] Add a test proving the ACP backend does not launch a process when `output_schema` is present.

Run:

```bash
cargo nextest run -p fabro-workflow handler::llm::acp
```

Expected: ACP guardrail test passes.

### Task 7: Docs

**Files:**
- Modify: `docs/public/agents/outputs.mdx`
- Modify: `docs/public/reference/dot-language.mdx`

- [ ] Document `output_schema="routing"` and `output_schema="@schema.json"` under routing/structured outputs.
- [ ] Document same-context repair behavior explicitly: Fabro sends validation feedback to the same agent/prompt context before failing.
- [ ] Document `output_retries`, default `2`, and distinction from `max_retries`.
- [ ] Document v1 scope: agent/prompt nodes only; ACP unsupported; custom schema output stored at `output.{node_id}`.

Run:

```bash
rg -n "output_schema|output_retries|output\\." docs/public/agents/outputs.mdx docs/public/reference/dot-language.mdx
```

Expected: docs mention the new attrs and storage behavior.

### Task 8: Full Verification

**Files:**
- No new files beyond prior tasks

- [ ] Run focused workflow tests:

```bash
cargo nextest run -p fabro-workflow
```

- [ ] Run formatting check:

```bash
cargo +nightly-2026-04-14 fmt --check --all
```

- [ ] Run clippy:

```bash
cargo +nightly-2026-04-14 clippy --workspace --all-targets -- -D warnings
```

- [ ] If snapshots change, inspect before accepting:

```bash
cargo insta pending-snapshots
```

Only run `cargo insta accept` after verifying every pending snapshot is expected.

## Acceptance Criteria

- Existing workflows without `output_schema` behave exactly as before.
- `output_schema="routing"` prevents malformed/missing routing JSON from silently falling through to normal edge selection.
- Invalid structured output results in a corrective LLM turn in the same context window.
- Prompt repair preserves previous assistant output in the message list.
- Agent repair preserves the same live session and does not re-run the node from scratch.
- Exhausted output repair attempts produce a clear terminal failure.
- Custom schema output is available to downstream nodes at `output.{node_id}`.
- Docs clearly distinguish `output_retries` from `max_retries`.

## Assumptions

- `output_retries=2` is the default.
- Custom schema validation targets the final JSON object in the response text.
- `status.json` fallback remains routing-specific.
- Provider-native response schema is used for prompt nodes only where it is safe.
- ACP support can be added later after there is a guaranteed context-preserving repair mechanism.


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
  - Model: gpt-5.5, 329.8k tokens in / 54.8k out
  - Files: /home/daytona/workspace/fabro/lib/crates/fabro-workflow/src/handler/structured_output.rs
- **simplify_opus**: succeeded
  - Model: claude-opus-4-7, 141.5k tokens in / 35.9k out
  - Files: /home/daytona/workspace/fabro/lib/crates/fabro-workflow/src/handler/agent.rs, /home/daytona/workspace/fabro/lib/crates/fabro-workflow/src/handler/llm/api.rs, /home/daytona/workspace/fabro/lib/crates/fabro-workflow/src/handler/structured_output.rs


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