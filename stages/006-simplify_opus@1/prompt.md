Goal: ---
title: "feat: Add run approval controls to MCP and CLI"
type: feat
status: active
date: 2026-05-25
---

# feat: Add run approval controls to MCP and CLI

## Summary

Fabro already has pre-execution run approval state and REST endpoints:

- `POST /api/v1/runs/{id}/approve`
- `POST /api/v1/runs/{id}/deny`

This plan exposes that capability through the existing human run-management surfaces:

- `fabro_run_interact` gains `approve` and `deny` actions.
- The CLI gains top-level batch commands:
  - `fabro approve <RUNS>...`
  - `fabro deny [--reason <REASON>] <RUNS>...`

Approval remains a human/user action. Workflow-agent `fabro_tools` must not be able to approve or deny child runs it created.

## Key Interface Changes

### MCP

Extend `fabro_run_interact`:

- Add actions: `approve`, `deny`.
- Add optional parameter: `reason: string | null`.
- `reason` is valid only for `deny`; trim whitespace and send `None` for absent or blank values.
- `approve` and `deny` return the updated run summary using the existing `result.summary` shape.
- Update the tool description to include approval actions.

Example:

```json
{
  "run_id": "nightly",
  "action": "approve"
}
```

```json
{
  "run_id": "nightly",
  "action": "deny",
  "reason": "Not approved for execution"
}
```

### CLI

Add flattened top-level commands, alongside `archive` and `unarchive`:

```bash
fabro approve <RUNS>...
fabro deny [--reason <REASON>] <RUNS>...
```

Batch behavior:

- Resolve each argument using the same selector path as archive/unarchive.
- Attempt every requested run even if earlier runs fail.
- Text mode prints each successful short run id to stderr.
- If any run fails, exit non-zero after processing all runs.

JSON output:

```json
{
  "approved": ["01K..."],
  "errors": []
}
```

```json
{
  "denied": ["01K..."],
  "errors": []
}
```

Errors use the existing batch shape:

```json
{
  "identifier": "nightly",
  "error": "Run is not pending approval."
}
```

`fabro deny --reason <REASON>` applies the same reason to every run in the batch.

## Implementation Plan

### 1. Add client and shared tool backend methods

Files:

- `lib/crates/fabro-client/src/client.rs`
- `lib/crates/fabro-tool/src/common.rs`
- `lib/crates/fabro-tool/src/fabro_client.rs`
- Mock `FabroToolBackend` impls in tests under `fabro-tool` and `fabro-workflow`

Changes:

- Add `Client::approve_run(&RunId) -> Result<Run>`.
- Add `Client::deny_run(&RunId, Option<String>) -> Result<Run>`.
- Implement them by calling the generated `approve_run` and `deny_run` OpenAPI client methods.
- Add matching methods to `FabroToolBackend`.
- Implement them in `ClientBackend`.
- For `deny_run`, build `types::DenyRunRequest { reason }` only when the generated client requires a body; preserve the API behavior that omitted, null, empty, or whitespace-only reasons are stored as absent.

No OpenAPI or generated-client regeneration is expected because the endpoints and DTO already exist.

### 2. Extend `fabro_run_interact`

Files:

- `lib/crates/fabro-tool/src/interact.rs`
- `lib/crates/fabro-tool/src/common.rs`
- `lib/crates/fabro-tool/src/lib.rs`
- `lib/crates/fabro-mcp-server/src/server.rs`

Changes:

- Add `RunInteractAction::Approve` and `RunInteractAction::Deny`.
- Add `reason: Option<String>` to `FabroRunInteractParams`.
- Add `ValidatedInteractAction::Approve` and `ValidatedInteractAction::Deny { reason: Option<String> }`.
- Validate `reason` by trimming whitespace and dropping blank values.
- Dispatch:
  - `Approve` -> `backend.approve_run(&run_id)`
  - `Deny` -> `backend.deny_run(&run_id, reason)`
- Return `json!({ "summary": common::run_summary_result(&summary) })`.
- Update `interact_run_text` output automatically through the action name; no special summary text is required.
- Update tool descriptions from “start, message, interrupt, cancel…” to include “approve, deny”.

### 3. Block workflow-agent self-approval

Files:

- `lib/crates/fabro-workflow/src/handler/llm/api.rs`
- Existing run-tool tests in the same file

Changes:

- In `execute_fabro_run_tool`, parse `FabroRunInteractParams` before dispatching.
- If the tool call is `fabro_run_interact` with action `approve` or `deny`, return a `ToolError` explaining that run approval must be performed by a user through the API, CLI, web UI, or human MCP server.
- Do not rely only on server auth for this guard; the tool error should be immediate and explicit for workflow agents.
- Keep server `approve_run` and `deny_run` handlers on `RequiredUser`.
- Keep the existing server negative test that run-tools workers cannot call user-only routes, and extend it to include `/runs/{id}/deny`.

### 4. Add CLI approval commands

Files:

- `lib/crates/fabro-cli/src/args.rs`
- `lib/crates/fabro-cli/src/commands/runs/mod.rs`
- Create `lib/crates/fabro-cli/src/commands/runs/approval.rs`

Changes:

- Add:
  - `RunsApproveArgs { server: ServerTargetArgs, runs: Vec<String> }`
  - `RunsDenyArgs { server: ServerTargetArgs, reason: Option<String>, runs: Vec<String> }`
- Add `RunsCommands::Approve(RunsApproveArgs)` and `RunsCommands::Deny(RunsDenyArgs)`.
- Command help:
  - approve: “Approve pending workflow runs.”
  - deny: “Deny pending workflow runs.”
  - run args: “Run IDs or workflow names to approve/deny.”
  - reason flag: “Reason for denying execution.”
- Implement shared batch logic in `commands/runs/approval.rs`, patterned after `archive.rs`:
  - Resolve each identifier with `client.resolve_run`.
  - Call `client.approve_run` or `client.deny_run`.
  - Collect successes and per-identifier errors.
  - Print short run ids to stderr in text mode.
  - Print JSON only in JSON mode.
  - Fail at the end with `some runs could not be approved` or `some runs could not be denied`.
- Register the new dispatch arms in `commands/runs/mod.rs`.
- Update command-name reporting in `RunsCommands::name()`.

### 5. Update docs

Files:

- `docs/public/agents/mcp.mdx`
- `docs/public/reference/cli.mdx`

Changes:

- Update the `fabro_run_interact` row to include approve/deny.
- Add a short MCP example for approving or denying a pending run.
- Add generated or manually updated CLI reference sections for:
  - `fabro approve`
  - `fabro deny`
- If `docs/public/reference/cli.mdx` is generated from `fabro __cli-reference`, regenerate it after the CLI args are implemented.

## Test Plan

### `fabro-tool`

- Add unit tests for action validation:
  - `approve` validates with only `run_id` and action.
  - `deny` validates with absent reason as `None`.
  - `deny` trims a nonblank reason.
  - `deny` converts blank reason to `None`.
- Add dispatch tests with a mock backend:
  - `approve` calls `approve_run` and returns `result.summary`.
  - `deny` calls `deny_run` with the expected reason and returns `result.summary`.
- Update schema/tool-definition assertions to prove `approve`, `deny`, and `reason` appear in the `fabro_run_interact` schema.

Run:

```bash
cargo nextest run -p fabro-tool interact
```

### MCP integration

File:

- `lib/crates/fabro-cli/tests/it/cmd/mcp.rs`

Scenarios:

- `fabro_run_interact` action `approve`:
  - resolves selector through `/api/v1/runs/resolve`
  - posts to `/api/v1/runs/{id}/approve`
  - returns updated summary
- `fabro_run_interact` action `deny`:
  - resolves selector
  - posts to `/api/v1/runs/{id}/deny`
  - sends `{ "reason": "Not approved for execution" }` when reason is supplied
  - returns updated summary
- Existing tool-list/schema test checks the new actions and parameter.

Run:

```bash
cargo nextest run -p fabro-cli mcp_interact
```

### CLI command tests

Files:

- Create `lib/crates/fabro-cli/tests/it/cmd/approve.rs`
- Create `lib/crates/fabro-cli/tests/it/cmd/deny.rs`
- Modify `lib/crates/fabro-cli/tests/it/cmd/mod.rs`
- Modify `lib/crates/fabro-cli/tests/it/cmd/fabro.rs`

Scenarios:

- Help snapshots for `fabro approve --help` and `fabro deny --help`.
- Required-argument snapshots for missing `<RUNS>...`.
- Mock-server success path:
  - resolve `nightly-build`
  - call approve/deny endpoint
  - print short run id in text mode
- JSON success path:
  - approve returns `approved`
  - deny returns `denied`
  - both include `errors: []`
- Partial-error path:
  - one selector or endpoint fails
  - remaining runs are still attempted
  - JSON includes both successes and errors
  - process exits non-zero
- `fabro deny --reason "Needs review"` sends that reason in the request body.
- Top-level help snapshot includes `approve` and `deny`.

Run:

```bash
cargo nextest run -p fabro-cli approve deny fabro::help
```

### Server auth regression

File:

- `lib/crates/fabro-server/src/server/tests.rs`

Scenario:

- Extend `run_tools_worker_cannot_call_user_only_non_mcp_routes` so a run-tools worker remains rejected for:
  - `POST /runs/{target}/approve`
  - `POST /runs/{target}/deny`
  - an existing user-only route such as timeline

Run:

```bash
cargo nextest run -p fabro-server run_tools_worker_cannot_call_user_only_non_mcp_routes
```

### Final verification

Run targeted checks first:

```bash
cargo nextest run -p fabro-tool -p fabro-cli -p fabro-server
```

Before merge, run:

```bash
cargo +nightly-2026-04-14 fmt --check --all
cargo +nightly-2026-04-14 clippy --workspace --all-targets -- -D warnings
```

Run full workspace tests if the client/backend trait changes produce broad compile churn:

```bash
cargo nextest run --workspace
```

## Assumptions

- Approval and denial remain user-authorized operations.
- Human MCP clients use normal CLI/server user auth and may approve or deny.
- Workflow-agent `fabro_tools` may inspect pending runs but must not approve or deny them.
- `fabro deny --reason <REASON>` applies one reason to every run in the batch.
- No API contract change is required because the REST approval endpoints already exist.


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
  - Model: gpt-5.5, 5.1m tokens in / 34.7k out


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