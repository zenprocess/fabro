Goal: # Issue #399: Add automation run endpoints

- URL: https://github.com/fabro-sh/fabro/issues/399
- State: OPEN
- Author: Bryan Helmkamp (@brynary)
- Created: 2026-05-25T15:06:27Z
- Updated: 2026-05-25T15:06:27Z
- Labels: None
- Assignees: None
- Milestone: None
- Comments: 0

---

## Goal

Expose API endpoints for listing runs associated with an automation and starting a run through an enabled API trigger.

## Scope

Implement these endpoints:

```http
GET  /automations/{id}/runs
POST /automations/{id}/runs
```

`GET /automations/{id}/runs` behavior:

- Require the automation definition to exist; return 404 when it does not.
- List cached runs from the existing run store.
- Filter by `run.automation.as_ref().is_some_and(|a| a.id == id)`.
- Sort newest first.
- Support `page[limit]` and `page[offset]` using existing pagination behavior.
- Return the existing paginated run list envelope:

```json
{
  "data": [],
  "meta": { "has_more": false, "total": 0 }
}
```

`POST /automations/{id}/runs` behavior:

- Use `RequiredRunToolActor`.
- Require the automation to exist and be enabled.
- Find an enabled trigger where `type = "api"`.
- Return 409 with API error code `automation_api_trigger_disabled` when the automation is disabled or no enabled API trigger is available.
- Materialize the run manifest using the configured `AutomationRunMaterializer`.
- Call the shared create-run helper with:

```rust
AutomationRef {
    id: automation.id.to_string(),
    name: Some(automation.name.clone()),
    trigger_id: Some(api_trigger.id.to_string()),
}
```

- Return 201 and the normal `Run` response shape with automation metadata populated.

Final integration expectations:

- Automation-created runs are visible through normal run APIs.
- Automation-created runs are visible through `GET /automations/{id}/runs`.
- Run history is derived from persisted/cached runs; no runtime automation state store is introduced.
- Schedule trigger expressions are stored and validated by earlier phases but are not scheduled by this endpoint work.

## Files

Modify:

- `lib/crates/fabro-server/src/server/handler/automations.rs`
- `lib/crates/fabro-server/src/server/handler/runs.rs`, only if additional helper exposure is needed from the previous phase
- `lib/crates/fabro-server/tests/it/api/automations.rs`
- `lib/crates/fabro-server/tests/it/api/mod.rs`

## Acceptance Criteria

- Disabled automations cannot start runs through the automation run endpoint.
- Automations without an enabled API trigger cannot start runs through the automation run endpoint.
- A successful API-triggered automation run returns a normal `Run` response with `automation.id`, `automation.name`, and `automation.trigger_id`.
- The automation run listing endpoint returns only runs linked to that automation.
- Automation run listings are newest-first and paginate correctly.
- No scheduler, web UI route/component, or CLI command is added.

## Verification

Add integration tests using the fake materializer for:

- Disabled automation returns 409.
- Disabled API trigger returns 409.
- Missing API trigger returns 409.
- Successful run creation returns 201.
- Created run persists `Run.automation`.
- Associated run listing includes the run.
- Run listing excludes runs from other automations.
- Run listing pagination and newest-first sorting.

Run:

```bash
cargo nextest run -p fabro-automation
cargo nextest run -p fabro-api
cargo nextest run -p fabro-server automations
cargo nextest run -p fabro-server openapi_conformance
cargo +nightly-2026-04-14 fmt --check --all
cargo +nightly-2026-04-14 clippy --workspace --all-targets -- -D warnings
git diff -- apps/fabro-web lib/crates/fabro-cli
```

Expected: focused tests and checks pass; web UI and CLI command modules remain unchanged.


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
  - Model: gpt-5.5, 1.6m tokens in / 24.4k out


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