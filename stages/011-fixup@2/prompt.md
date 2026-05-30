Goal: # Plan: Make run actors and provenance total

## Context

This is a greenfield app. Backward compatibility with old serialized runs, old API clients, old generated models, and old tests is not a constraint. Prefer the clean invariant and remove all traces of the placeholder shape.

`Principal::Anonymous` currently represents "no authenticated actor on this request" inside auth middleware. That is auth state, not an actor. A `Principal` should only mean "who acted."

Likewise, a persisted run should always have a creator. `Run.created_by`, `RunSpec.provenance`, `RunProvenance.subject`, and `run.created` event provenance should all be total. No `Option<Principal>`, no nullable OpenAPI fields, no legacy deserialization defaults, and no fallback creator in projection code.

Two commits, in order.

---

## Commit 1 - Remove `Principal::Anonymous`

Breaking cleanup. `Principal` becomes actor-only. Missing/invalid auth is represented as absent request principal, not as an anonymous principal variant.

### Rust

`lib/crates/fabro-types/src/principal.rs`:
- Drop `Anonymous`.
- Drop `Anonymous` arms in `kind()` and `display()`.
- Delete anonymous serialization/round-trip test coverage.

`lib/crates/fabro-server/src/principal_middleware.rs`:
- `RequestAuthContext.principal: Principal` -> `Option<Principal>`.
- `RequestAuthLogContext.principal: Principal` -> `Option<Principal>`.
- `initial()` and `rejected()` set `principal: None`.
- `authenticated(...)`, `authenticated_worker(...)`, and `authenticated_user(...)` set `principal: Some(...)`.
- Update `principal_without_log_unused_fields` to preserve `None` and strip user avatar data only inside `Some(Principal::User(...))`.
- Update all gate helpers to match `Option<Principal>`:
  - `require_user`
  - `require_authenticated_user`
  - `require_run_management_actor`
  - `require_worker_or_user_for_run`
  - `require_run_management_target`
- `None` routes to the existing `auth_rejection(context.auth_status, context.auth_error_code)` behavior.
- `Some(Principal::Worker { .. })` keeps the current forbidden-vs-auth-rejection distinctions.
- Update tests that assert the initial/rejected principal to assert `None`.

`lib/crates/fabro-server/src/server.rs` HTTP logging:
- Keep the `principal_kind` field on every HTTP log line.
- Compute `principal_kind` as `auth_context.principal.as_ref().map(Principal::kind).unwrap_or("none")`.
- Match `auth_context.principal` as an `Option<Principal>`:
  - `Some(User(...))`, `Some(Worker { ... })`, `Some(Webhook { ... })`, `Some(Slack { ... })` keep their extra fields.
  - `None | Some(Agent { .. } | System { .. })` emits only the common HTTP fields.

`docs/internal/logging-strategy.md`:
- Replace the `anonymous` HTTP caller category guidance with `none` for requests that have no principal.
- Keep `auth_status` as the field that distinguishes missing, invalid, expired, and authenticated auth state.

### OpenAPI and generated clients

`docs/public/api-reference/fabro-api.yaml`:
- Remove `PrincipalAnonymous` from the `Principal` `oneOf`.
- Remove `anonymous` from the `Principal` discriminator mapping.
- Delete the `PrincipalAnonymous` schema.

Regenerate:
- `cargo build -p fabro-api`
- `cd lib/packages/fabro-api-client && bun run generate`

Expected generated cleanup:
- `lib/packages/fabro-api-client/src/models/principal-anonymous.ts` disappears.
- `Principal` union no longer includes `{ kind: "anonymous" }`.
- `lib/packages/fabro-api-client/src/models/index.ts` no longer exports `principal-anonymous`.

### Frontend

`apps/fabro-web/app/lib/principal-display.tsx`:
- Remove the `"anonymous"` switch case and unused icon import.

`apps/fabro-web/app/components/run-summary-panel.test.tsx` and API-client exhaustiveness tests:
- Remove anonymous principal cases.

### Documentation sweep

Remove anonymous-principal references from product/API docs and tests. Be careful not to touch unrelated uses of "anonymous" such as telemetry anonymous IDs or Git's `remote_anonymous` API.

Useful sweep:
- `rg -n "Principal::Anonymous|PrincipalAnonymous|kind: 'anonymous'|kind: \"anonymous\"|anonymous actor|anonymous subject|principal_kind.*anonymous|\"anonymous\"" lib/crates apps/fabro-web lib/packages/fabro-api-client docs/public docs/internal`

### Verification

- `cargo +nightly-2026-04-14 fmt --check --all`
- `cargo +nightly-2026-04-14 clippy --workspace --all-targets -- -D warnings`
- `cargo build --workspace`
- `cargo nextest run --workspace`
- `cd apps/fabro-web && bun run typecheck && bun test`
- Manual: start `fabro server start`, hit a protected endpoint without a token, confirm 401 and an HTTP log with `principal_kind="none"` and `auth_status="missing"`.

---

## Commit 2 - Make run provenance and creator non-optional

Full-chain invariant. Every persisted run has exactly one creator principal. No nullable schema fields, no legacy defaults, no projection fallbacks.

### Core type changes

`lib/crates/fabro-types/src/run_summary.rs`:
- `Run.created_by: Option<Principal>` -> `Principal`.
- Drop `#[serde(default)]`.

`lib/crates/fabro-types/src/run.rs`:
- `RunProvenance.subject: Option<Principal>` -> `Principal`.
- Drop `#[serde(default, skip_serializing_if = "Option::is_none")]`.
- Drop `Default` derive on `RunProvenance`.
- `RunSpec.provenance: Option<RunProvenance>` -> `RunProvenance`.
- Drop `#[serde(default, skip_serializing_if = "Option::is_none")]` on `RunSpec.provenance`.

`lib/crates/fabro-types/src/run_event/run.rs`:
- `RunCreatedProps.provenance: Option<RunProvenance>` -> `RunProvenance`.
- Drop default/skip serialization attributes for provenance.

`lib/crates/fabro-workflow/src/event/events.rs`:
- `Event::RunCreated.provenance: Option<RunProvenance>` -> `RunProvenance`.
- Drop default/skip serialization attributes for provenance.

### Creation and retry flow

`lib/crates/fabro-workflow/src/operations/create.rs`:
- `CreateRunInput.provenance: Option<RunProvenance>` -> `RunProvenance`.
- `PersistCreateOptions.provenance: Option<RunProvenance>` -> `RunProvenance`.
- `RunSpec { provenance }` stores the total provenance directly.
- `Event::RunCreated { provenance }` emits total provenance directly.

`lib/crates/fabro-server/src/server/handler/runs.rs`:
- `run_provenance(headers, subject)` returns `RunProvenance { subject: subject.clone(), ... }`.
- Build provenance before creating `CreateRunInput`.

`lib/crates/fabro-server/src/run_manifest.rs`:
- Change `create_run_input(...)` to accept `provenance: RunProvenance` and set it directly, or stop using the helper for the final `CreateRunInput` construction. Do not create a temporary input with missing provenance.

`lib/crates/fabro-workflow/src/operations/retry.rs`:
- `RetryRunInput.provenance: Option<RunProvenance>` -> `RunProvenance`.
- `retry_run(...)` writes the new run's `run.created` event with total provenance.

`lib/crates/fabro-server/src/server/handler/lifecycle.rs`:
- Pass `run_provenance(&headers, &actor)` directly into `RetryRunInput`.

### Event conversion and projections

`lib/crates/fabro-workflow/src/event/convert.rs`:
- Convert `Event::RunCreated.provenance` into `RunCreatedProps.provenance` directly.
- Remove `Some(...)` wrapping for run-created provenance.

`lib/crates/fabro-workflow/src/event/stored_fields.rs`:
- `Event::RunCreated { provenance, .. }` sets `actor: Some(provenance.subject.clone())`.

`lib/crates/fabro-store/src/run_state.rs`:
- `projection_from_created(...)` builds `RunSpec { provenance: props.provenance.clone(), ... }`.
- `build_summary(...)` sets `created_by: state.spec.provenance.subject.clone()`.
- Delete or rewrite tests that deserialize projections with `"provenance": null`.

`lib/crates/fabro-types/src/run_projection.rs` and projection tests:
- Replace all test `RunSpec` literals with total provenance.
- Remove tests whose only purpose is legacy/null provenance tolerance.

### OpenAPI

`docs/public/api-reference/fabro-api.yaml`:
- `Run.created_by` references `Principal` directly. Remove `oneOf [..., null]`.
- `RunProvenance.required` includes `subject`.
- `RunProvenance.subject` references `Principal` directly. Remove `oneOf [..., null]`.
- `RunSpec.required` includes `provenance`.
- `RunSpec.provenance` references `RunProvenance` directly. Remove `oneOf [..., null]`.
- If `run.created` event properties are represented separately in the spec, make that event provenance required and non-nullable too.

Regenerate:
- `cargo build -p fabro-api`
- `cd lib/packages/fabro-api-client && bun run generate`

Do not hand-edit generated client files.

### Demo mode

`lib/crates/fabro-server/src/demo/mod.rs`:
- Add a clearly synthetic demo principal using `AuthMethod::DevToken`, not GitHub:
  ```rust
  static DEMO_PRINCIPAL: LazyLock<Principal> = LazyLock::new(|| {
      Principal::user(
          IdpIdentity::new("fabro:demo", "demo").unwrap(),
          "demo".to_string(),
          AuthMethod::DevToken,
      )
  });
  ```
- Replace `created_by: None` with `created_by: DEMO_PRINCIPAL.clone()`.
- If demo creates any full `RunSpec` or `run.created` event data, give it `RunProvenance { subject: DEMO_PRINCIPAL.clone(), ... }`.

### Test support

Do not add fake auth helpers to `fabro_types::fixtures`; that module is run-id constants.

Use the existing `fabro-types` `test-support` feature:
- Add `#[cfg(any(test, feature = "test-support"))] pub mod test_support;` in `lib/crates/fabro-types/src/lib.rs` if it does not already exist.
- Add `lib/crates/fabro-types/src/test_support.rs` with:
  - `test_principal() -> Principal`
  - `test_run_provenance() -> RunProvenance`
- Use an obviously fake dev-token identity, e.g. issuer `fabro:test`, subject `test-user`, login `test`.
- In crates that need the helper from integration tests or cross-crate tests, dual-list `fabro-types` in `dev-dependencies` with `features = ["test-support"]`, following existing repo patterns.

Update all constructors:
- Replace `provenance: None` in `RunSpec`, `CreateRunInput`, `RetryRunInput`, `Event::RunCreated`, and `RunCreatedProps` literals with `test_run_provenance()` or a locally meaningful provenance.
- Replace `subject: Some(...)` with `subject: ...`.
- Replace `subject: None` only when it is actually `RunProvenance.subject`; leave unrelated todo/commit/message `subject` fields alone.
- Replace `created_by: None` / `created_by: null` with `test_principal()` or a frontend TS principal fixture.
- Delete tests that assert nullable or omitted creator/provenance behavior.

Representative Rust areas:
- `lib/crates/fabro-store/src/run_state.rs`
- `lib/crates/fabro-store/tests/serializable_projection.rs`
- `lib/crates/fabro-workflow/src/operations/{create,retry,start}.rs`
- `lib/crates/fabro-workflow/src/event/{convert,sink,stored_fields}.rs`
- `lib/crates/fabro-workflow/src/handler/**`
- `lib/crates/fabro-workflow/src/pipeline/**`
- `lib/crates/fabro-workflow/src/run_{lookup,metadata}.rs`
- `lib/crates/fabro-server/src/server/tests.rs`
- `lib/crates/fabro-server/src/server/handler/**`
- `lib/crates/fabro-server/tests/it/**`
- `lib/crates/fabro-cli/tests/it/support/mod.rs`
- `lib/crates/fabro-dump/src/lib.rs`
- `lib/crates/fabro-tool/src/{common,create,interact,search}.rs`
- `lib/crates/fabro-api/tests/{principal_round_trip,run_summary_round_trip,run_projection_round_trip,run_event_round_trip}.rs`
- `lib/crates/fabro-types/tests/{run_spec_serde,run_spec_methods,run_event_serde}.rs`

Representative TypeScript areas:
- `apps/fabro-web/app/**` tests with `created_by: null`
- `apps/fabro-web/app/data/runs.ts`
- `apps/fabro-web/app/components/run-summary-panel.tsx`
- `apps/fabro-web/app/components/runs-list/**`
- `lib/packages/fabro-api-client/tests/principal-exhaustive.ts`

Useful sweep after edits:
- `rg -n "Principal::Anonymous|PrincipalAnonymous|principal-anonymous|kind: ['\"]anonymous|created_by:\\s*(None|null)|provenance:\\s*None|subject:\\s*Some\\(|subject:\\s*None" lib/crates apps/fabro-web lib/packages/fabro-api-client docs/public docs/internal`

Review each hit. The only acceptable remaining matches should be unrelated uses of "anonymous" and unrelated non-principal `subject` fields.

### Frontend

`apps/fabro-web/app/components/run-summary-panel.tsx`:
- `run?.created_by` may still be guarded by `run` loading state, but `created_by` itself is non-null once `run` exists.
- Pass `run.created_by` directly to `principalDisplay(...)` inside loaded-run branches.

`apps/fabro-web/app/data/runs.ts` and run-list components:
- Treat `createdBy` as a total principal in UI data derived from a loaded API run.
- Remove empty/fallback rendering that only existed for missing creator data.

### Verification

- `cargo +nightly-2026-04-14 fmt --check --all`
- `cargo +nightly-2026-04-14 clippy --workspace --all-targets -- -D warnings`
- `cargo build --workspace`
- `cargo nextest run --workspace`
- `cargo nextest run -p fabro-server`
- `cd apps/fabro-web && bun run typecheck && bun test && bun run build`
- Manual end-to-end:
  - `fabro server start`
  - `cd apps/fabro-web && bun run dev`
  - Authenticate and create a run through the UI.
  - Confirm `/api/v1/runs/:id` has non-null `created_by`.
  - Confirm `/api/v1/runs/:id/state` has non-null `spec.provenance.subject`.
  - Retry a failed run and confirm the retried run has the retrying user as creator.
  - Hit demo mode with `X-Fabro-Demo: 1` and confirm the run summary renders the synthetic `demo` dev-token user.


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
  - Model: gpt-5.5, 4.5m tokens in / 66.1k out
  - Files: /home/daytona/workspace/fabro/lib/crates/fabro-types/src/test_support.rs
- **simplify_opus**: succeeded
  - Model: claude-opus-4-7, 75.5k tokens in / 19.3k out
  - Files: /home/daytona/workspace/fabro/lib/crates/fabro-cli/tests/it/cmd/support.rs, /home/daytona/workspace/fabro/lib/crates/fabro-server/src/principal_middleware.rs, /home/daytona/workspace/fabro/lib/crates/fabro-types/Cargo.toml, /home/daytona/workspace/fabro/lib/crates/fabro-types/src/principal.rs, /home/daytona/workspace/fabro/lib/crates/fabro-types/tests/run_event_serde.rs, /home/daytona/workspace/fabro/lib/crates/fabro-types/tests/run_spec_methods.rs, /home/daytona/workspace/fabro/lib/crates/fabro-types/tests/run_spec_serde.rs, /home/daytona/workspace/fabro/lib/crates/fabro-workflow/src/operations/retry.rs
- **simplify_gpt**: succeeded
  - Model: gpt-5.5, 175.5k tokens in / 2.9k out
- **verify**: failed
  - Script: `git fetch origin main 2>&1 && git merge --no-edit --no-stat origin/main 2>&1 && cargo +nightly-2026-04-14 fmt --all 2>&1 && cargo dev docs refresh 2>&1 && cargo +nightly-2026-04-14 fmt --check --all 2>&1 && { command -v rg >/dev/null 2>&1 || { echo 'rg is required for verify'; exit 127; }; } && ! rg -n 'AuthMode::Disabled|RunAuthMethod|RunSubjectProvenance|\bActorRef\b|\bActorKind\b|AuthenticatedSubject|AuthenticatedService|AuthorizeRunScoped|AuthorizeRunBlob|AuthorizeStageArtifact|AuthorizeCommandLog|auth_method\s*==\s*"disabled"' lib/crates apps lib/packages docs/public/api-reference/fabro-api.yaml 2>&1 && cargo +nightly-2026-04-14 clippy --workspace --all-targets -- -D warnings 2>&1 && cargo nextest run --workspace --status-level slow --profile ci 2>&1 && cargo dev docs check 2>&1 && bun install --frozen-lockfile 2>&1 && (cd apps/fabro-web && bun run typecheck) 2>&1 && (cd apps/fabro-web && bun run test) 2>&1 && (cd lib/packages/fabro-api-client && bun run typecheck) 2>&1 && cargo dev build -- -p fabro-cli --release 2>&1`
- **fixup**: succeeded
  - Model: claude-opus-4-7, 24.8k tokens in / 4.7k out
- **verify**: failed
  - Script: `git fetch origin main 2>&1 && git merge --no-edit --no-stat origin/main 2>&1 && cargo +nightly-2026-04-14 fmt --all 2>&1 && cargo dev docs refresh 2>&1 && cargo +nightly-2026-04-14 fmt --check --all 2>&1 && { command -v rg >/dev/null 2>&1 || { echo 'rg is required for verify'; exit 127; }; } && ! rg -n 'AuthMode::Disabled|RunAuthMethod|RunSubjectProvenance|\bActorRef\b|\bActorKind\b|AuthenticatedSubject|AuthenticatedService|AuthorizeRunScoped|AuthorizeRunBlob|AuthorizeStageArtifact|AuthorizeCommandLog|auth_method\s*==\s*"disabled"' lib/crates apps lib/packages docs/public/api-reference/fabro-api.yaml 2>&1 && cargo +nightly-2026-04-14 clippy --workspace --all-targets -- -D warnings 2>&1 && cargo nextest run --workspace --status-level slow --profile ci 2>&1 && cargo dev docs check 2>&1 && bun install --frozen-lockfile 2>&1 && (cd apps/fabro-web && bun run typecheck) 2>&1 && (cd apps/fabro-web && bun run test) 2>&1 && (cd lib/packages/fabro-api-client && bun run typecheck) 2>&1 && cargo dev build -- -p fabro-cli --release 2>&1`

## Context
- failure_class: transient_infra
- failure_signature: verify|transient_infra|script timed out after 600000ms: git fetch origin main <n>>&<n> && git merge --no-edit --no-stat origin/main <n>>&<n> && cargo +nightly-<n>-<n>-<n> fmt --all <n>>&<n> && cargo dev docs refresh <n>>&<n> && cargo +nightly-<n>-<n>-<n> fmt --ch


The verify step failed. Read the build output from context and fix all format, clippy, Rust test, docs, TypeScript typecheck/test, and build failures.