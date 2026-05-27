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
- **simplify_opus**: succeeded
  - Model: claude-opus-4-7, 59.4k tokens in / 16.0k out
  - Files: /home/daytona/workspace/fabro/docs/public/api-reference/fabro-api.yaml, /home/daytona/workspace/fabro/lib/crates/fabro-api/build.rs, /home/daytona/workspace/fabro/lib/crates/fabro-api/tests/automation_round_trip.rs, /home/daytona/workspace/fabro/lib/crates/fabro-automation/src/model.rs
- **simplify_gpt**: succeeded
  - Model: gpt-5.5, 702.4k tokens in / 6.3k out
- **verify**: failed
  - Script: `git fetch origin main 2>&1 && git merge --no-edit --no-stat origin/main 2>&1 && cargo +nightly-2026-04-14 fmt --all 2>&1 && cargo dev docs refresh 2>&1 && cargo +nightly-2026-04-14 fmt --check --all 2>&1 && { command -v rg >/dev/null 2>&1 || { echo 'rg is required for verify'; exit 127; }; } && ! rg -n 'AuthMode::Disabled|RunAuthMethod|RunSubjectProvenance|\bActorRef\b|\bActorKind\b|AuthenticatedSubject|AuthenticatedService|AuthorizeRunScoped|AuthorizeRunBlob|AuthorizeStageArtifact|AuthorizeCommandLog|auth_method\s*==\s*"disabled"' lib/crates apps lib/packages docs/public/api-reference/fabro-api.yaml 2>&1 && cargo +nightly-2026-04-14 clippy --workspace --all-targets -- -D warnings 2>&1 && cargo nextest run --workspace --status-level slow --profile ci 2>&1 && cargo dev docs check 2>&1 && bun install --frozen-lockfile 2>&1 && (cd apps/fabro-web && bun run typecheck) 2>&1 && (cd apps/fabro-web && bun run test) 2>&1 && (cd lib/packages/fabro-api-client && bun run typecheck) 2>&1 && cargo dev build -- -p fabro-cli --release 2>&1`

## Context
- failure_class: transient_infra
- failure_signature: verify|transient_infra|failed to spawn script


The verify step failed. Read the build output from context and fix all format, clippy, Rust test, docs, TypeScript typecheck/test, and build failures.