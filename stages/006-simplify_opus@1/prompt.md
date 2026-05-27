Goal: # Define automation API contract and clients

- Number: #396
- State: OPEN
- Author: brynary (Bryan Helmkamp)
- Created: 2026-05-25T15:06:24Z
- Updated: 2026-05-25T15:06:24Z
- URL: https://github.com/fabro-sh/fabro/issues/396

## Body

## Goal

Define the public Automations API contract, reuse compatible Rust domain types, and regenerate generated API clients.

## Scope

Update the OpenAPI spec under `/api/v1` with these paths:

```http
GET    /automations
POST   /automations
GET    /automations/{id}
PUT    /automations/{id}
DELETE /automations/{id}
GET    /automations/{id}/runs
POST   /automations/{id}/runs
```

Add schemas for:

- `Automation`
- `AutomationTarget`
- `AutomationTrigger`
- `AutomationApiTrigger`
- `AutomationScheduleTrigger`
- `CreateAutomationRequest`
- `ReplaceAutomationRequest`
- `AutomationListResponse`

Use this response shape for automations:

```ts
type Automation = {
  id: string;
  revision: string;
  name: string;
  description: string | null;
  enabled: boolean;
  target: AutomationTarget;
  triggers: AutomationTrigger[];
};

type AutomationTarget = {
  repository: string;
  ref: string;
  workflow: string;
};

type AutomationTrigger =
  | { id: string; type: "api"; enabled: boolean }
  | { id: string; type: "schedule"; enabled: boolean; expression: string };
```

Request shapes:

```ts
type CreateAutomationRequest = {
  id: string;
  name: string;
  description?: string | null;
  enabled?: boolean;
  target: AutomationTarget;
  triggers: AutomationTrigger[];
};

type ReplaceAutomationRequest = {
  name: string;
  description?: string | null;
  enabled: boolean;
  target: AutomationTarget;
  triggers: AutomationTrigger[];
};
```

Contract details:

- Use an OpenAPI discriminator with `propertyName: type` for trigger variants.
- Unknown trigger discriminator values should be reported by handlers as domain validation errors with HTTP 422, not malformed JSON errors with HTTP 400.
- Reuse existing `Run` and paginated run-list envelope schemas for automation run endpoints.
- Add `If-Match` header parameters for replace and delete operations.
- Add `ETag` response headers on automation read and replace responses.
- Response codes should cover 200, 201, 204, 400, 404, 409, 422, and 428 where applicable.
- Before generating duplicate Rust types, search for matching `fabro-automation` domain types and add `with_replacement(...)` mappings where the serde wire shape is identical.
- Add JSON parity tests for every reused automation type.

## Files

Modify:

- `docs/public/api-reference/fabro-api.yaml`
- `lib/crates/fabro-api/Cargo.toml`
- `lib/crates/fabro-api/build.rs`
- Generated Rust API files under `lib/crates/fabro-api/src`, as produced by build/codegen
- Generated TypeScript client files under `lib/packages/fabro-api-client`

Create:

- `lib/crates/fabro-api/tests/automation_round_trip.rs`

## Acceptance Criteria

- The OpenAPI document exposes the full backend Automations API contract.
- Rust generated API types compile.
- Compatible automation domain types are reused instead of duplicated.
- Reused type JSON parity is covered by tests.
- The TypeScript API client contains generated automation operations and types.
- No web UI consumer imports are added.

## Verification

Run:

```bash
cargo build -p fabro-api
cargo nextest run -p fabro-api
cd lib/packages/fabro-api-client && bun run generate
git diff -- apps/fabro-web lib/crates/fabro-cli
```

Expected: generated API files may change; application UI and CLI command modules should not change.


## Comments

No comments.


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
  - Model: gpt-5.5, 4.5m tokens in / 34.5k out


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