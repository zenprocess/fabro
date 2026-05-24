Goal: ---
title: feat: Batch run archive actions
type: feat
status: active
date: 2026-05-24
---

# feat: Batch Run Archive Actions

## Overview

Add API support for archiving and unarchiving multiple runs in one request, then update the web list and board multi-run actions to use the new batch endpoints. The existing single-run archive and unarchive endpoints remain unchanged for run-scoped callers and direct lifecycle actions.

## Problem Frame

The web UI currently performs multi-run archive/unarchive actions by issuing one lifecycle request per selected run. That works, but it puts batch orchestration in the browser, repeats request overhead, and leaves API/CLI/MCP consumers without a first-class batch contract. A bounded fail-soft batch endpoint gives the server ownership of the multi-run operation while preserving the independent event stream semantics of each run.

## Requirements Trace

- R1. Provide public API endpoints that archive and unarchive many runs in one request.
- R2. Preserve existing single-run archive/unarchive behavior, including eligibility, idempotency, and emitted events.
- R3. Return per-run results so mixed-success batches can be reported without rolling back successful items.
- R4. Update web bulk actions to make one request per batch action instead of one request per run.
- R5. Keep cache invalidation and toast behavior equivalent to the current UI.

## Scope Boundaries

- Do not make batch archive/unarchive transactional; each run remains an independent event stream.
- Do not add new workflow event variants; successful batch items emit the same run archive/unarchive events as single-run actions.
- Do not change the existing `POST /api/v1/runs/{id}/archive` or `POST /api/v1/runs/{id}/unarchive` contracts.
- Do not add CLI commands in this change; this plan is limited to HTTP API and web UI usage.

## Context & Research

### Relevant Code and Patterns

- OpenAPI is the source of truth for HTTP contracts in `docs/public/api-reference/fabro-api.yaml`.
- Server lifecycle routes live in `lib/crates/fabro-server/src/server/handler/lifecycle.rs`; single-run archive/unarchive already funnel through `operations::archive` and `operations::unarchive`.
- Batch routes that are not tied to one path run ID should use `RequiredUser`, not `RequireRunScopedOrRunTools`, because a run-scoped worker token cannot safely authorize mutation of arbitrary run IDs from a request body.
- Web lifecycle helpers live in `apps/fabro-web/app/lib/run-actions.ts`; list and board multi-run archive behavior lives in `apps/fabro-web/app/routes/runs.tsx`.
- Run-list cache invalidation already uses `mutateRunListCaches` in `apps/fabro-web/app/lib/board-cache.ts`.

### Strategy Docs

- `docs/internal/testing-strategy.md`: server tests should assert public HTTP contracts and prefer structured assertions.
- `docs/internal/error-handling-strategy.md`: preserve structured errors internally and return curated API messages at HTTP boundaries.
- `docs/internal/events-strategy.md`: no new events are needed because existing per-run lifecycle events already represent the durable state transition.

## Key Technical Decisions

- Add collection action endpoints `POST /api/v1/runs/archive` and `POST /api/v1/runs/unarchive`. These avoid changing single-run URLs and keep generated client methods clear (`batchArchiveRuns`, `batchUnarchiveRuns`).
- Use fail-soft HTTP `200` responses for valid batch requests, even when individual items fail. Per-item failures carry structured result entries; request-level validation failures still return normal `400` errors.
- Validate `run_ids` at the request boundary: non-empty, maximum 250 IDs, no duplicates, and every value parseable as a `RunId`. Invalid request bodies must not mutate any runs.
- Process eligible IDs sequentially in server code. The batch is bounded, lifecycle operations append events, and sequential processing avoids adding lock-order or concurrency behavior that the feature does not need.
- Treat idempotent single-run outcomes as successful batch outcomes: already archived counts as success for archive; terminal not-archived counts as success for unarchive.

## API Contract

Add these OpenAPI operations:

- `POST /api/v1/runs/archive`
  - operationId: `batchArchiveRuns`
  - request: `BatchRunLifecycleRequest`
  - response: `BatchRunLifecycleResponse`
- `POST /api/v1/runs/unarchive`
  - operationId: `batchUnarchiveRuns`
  - request: `BatchRunLifecycleRequest`
  - response: `BatchRunLifecycleResponse`

Add schemas:

- `BatchRunLifecycleRequest`
  - required `run_ids`
  - `run_ids`: array of strings, `minItems: 1`, `maxItems: 250`, `uniqueItems: true`
- `BatchRunLifecycleResponse`
  - required `results`, `summary`
  - `results`: array of `BatchRunLifecycleResult`, ordered exactly like the request `run_ids`
  - `summary`: `BatchRunLifecycleSummary`
- `BatchRunLifecycleResult`
  - required `run_id`, `ok`, `outcome`
  - `run_id`: string
  - `ok`: boolean
  - `outcome`: enum covering `archived`, `already_archived`, `unarchived`, `not_archived`, `not_found`, `conflict`, `error`
  - optional `run`: `Run`, present for successful items when a decorated summary can be loaded
  - optional `error`: `ErrorResponseEntry`, present for failed items
- `BatchRunLifecycleSummary`
  - required `requested`, `succeeded`, `failed`
  - all integer counts

## Implementation Units

- [ ] **Unit 1: OpenAPI batch lifecycle contract**

**Goal:** Add the public API contract and generated clients for batch archive/unarchive.

**Requirements:** R1, R3

**Dependencies:** None

**Files:**
- Modify: `docs/public/api-reference/fabro-api.yaml`
- Generated by build/codegen: `lib/crates/fabro-api/src/generated.rs`
- Generated by codegen: `lib/packages/fabro-api-client/src/api/runs-api.ts`
- Generated by codegen: `lib/packages/fabro-api-client/src/models/*`

**Approach:**
- Add the two collection action paths and the four batch lifecycle schemas described in the API Contract section.
- Keep response status simple: `200` for a valid processed batch; `400`, `401`, and `500` for request-level failures.
- Run Rust generation through `cargo build -p fabro-api`.
- Run TypeScript generation through `cd lib/packages/fabro-api-client && bun run generate`.

**Patterns to follow:**
- Existing single-run archive/unarchive path docs in the same OpenAPI file.
- Existing generated client workflow described in `AGENTS.md`.

**Test scenarios:**
- Happy path: generated Rust and TypeScript clients expose `batchArchiveRuns` and `batchUnarchiveRuns`.
- Contract: request schema enforces `run_ids` as the only required input.
- Contract: result entries can represent both a successful decorated `Run` and a failed `ErrorResponseEntry`.

**Verification:**
- API generation completes without hand-edited generated files.

- [ ] **Unit 2: Server batch lifecycle handlers**

**Goal:** Implement the two new endpoints in the server using existing archive/unarchive operations.

**Requirements:** R1, R2, R3

**Dependencies:** Unit 1

**Files:**
- Modify: `lib/crates/fabro-server/src/server/handler/lifecycle.rs`
- Test: `lib/crates/fabro-server/src/server/tests.rs`

**Approach:**
- Register `POST /runs/archive` and `POST /runs/unarchive` in the lifecycle route set.
- Use `RequiredUser` for both handlers and convert the user into `Principal::User` for per-run operation calls.
- Add a small shared batch helper that accepts the parsed request, the action, and the actor, then returns `BatchRunLifecycleResponse`.
- Before processing any item, validate the entire request for empty list, over-limit list, duplicate IDs, and invalid IDs. Return a normal `400` API error if validation fails.
- For each valid ID, call `operations::archive` or `operations::unarchive`; map success outcomes to item-level success results and map `RunNotFound`/`Precondition` to item-level `not_found`/`conflict` failures.
- For successful items, load and decorate the current run summary the same way single-run lifecycle responses do. If summary loading fails after the operation succeeds, record that item as `error` rather than hiding the failure.

**Patterns to follow:**
- `run_archive_action` for operation mapping and existing error semantics.
- `run_response` / `state.decorate_run_summary` for response shape.
- Existing server tests around `archive_and_unarchive_updates_listing_visibility`.

**Test scenarios:**
- Happy path: two terminal runs archived in one request return two successful result entries, summary `requested=2/succeeded=2/failed=0`, and both runs are hidden from default `GET /api/v1/runs`.
- Happy path: two archived runs unarchived in one request return successful entries and both runs reappear in default listing.
- Idempotency: archiving an already archived run returns `ok=true` with `already_archived`; unarchiving a terminal non-archived run returns `ok=true` with `not_archived`.
- Mixed result: a batch containing one terminal run, one running run, and one missing run returns ordered results with one success, one `conflict`, and one `not_found`; the terminal run is still archived.
- Error path: empty `run_ids`, duplicate IDs, invalid IDs, and more than 250 IDs return request-level `400` and mutate no runs.
- Auth path: unauthenticated requests are rejected; run-scoped worker authentication is not accepted for batch endpoints.

**Verification:**
- Existing single-run archive/unarchive tests pass unchanged.
- New endpoint tests prove both API contract and run-list visibility effects.

- [ ] **Unit 3: Frontend lifecycle helpers**

**Goal:** Add typed web helpers for batch archive/unarchive.

**Requirements:** R3, R4, R5

**Dependencies:** Unit 1

**Files:**
- Modify: `apps/fabro-web/app/lib/run-actions.ts`
- Test: `apps/fabro-web/app/lib/run-actions.test.ts`

**Approach:**
- Add `archiveRuns(runIds: string[])` and `unarchiveRuns(runIds: string[])` wrappers around the generated client methods.
- Keep existing `archiveRun` and `unarchiveRun` helpers unchanged.
- Preserve existing `LifecycleActionError` behavior for single-run actions. Batch helpers should return the generated batch response for valid mixed results and throw only for request-level API failures.

**Patterns to follow:**
- Existing lifecycle action helpers in `run-actions.ts`.
- Axios adapter tests in `run-actions.test.ts`.

**Test scenarios:**
- Happy path: `archiveRuns(["run-1", "run-2"])` sends one generated-client request and returns parsed batch results.
- Mixed result: helper resolves a response containing one success and one per-item failure without throwing.
- Request error: helper throws parsed `ApiError`/lifecycle-style data when the server returns a request-level `400`.
- Regression: existing `archiveRun`, `unarchiveRun`, `canArchive`, and `canUnarchive` tests continue to pass.

**Verification:**
- Web unit tests cover the new helper contract without changing single-run behavior.

- [ ] **Unit 4: Web bulk-action integration**

**Goal:** Replace multi-request UI orchestration with one batch request per bulk action.

**Requirements:** R4, R5

**Dependencies:** Units 1 and 3

**Files:**
- Modify: `apps/fabro-web/app/routes/runs.tsx`
- Test: `apps/fabro-web/app/routes/runs.test.tsx`

**Approach:**
- Update `BulkActionToolbar` to call `archiveRuns` or `unarchiveRuns` once with eligible selected IDs.
- Add a small pure batch-summary helper in `runs.tsx`, export it for route tests, and use it to compute toast messages from the batch response summary rather than `Promise.allSettled`.
- Keep current eligibility filtering: archive only selected runs whose lifecycle status is archivable, unarchive only selected archived runs.
- Clear selection only when all eligible items succeed, matching the current all-success behavior.
- Call `mutateRunListCaches` once after the batch settles.
- Update the kanban column archive-all action to call `archiveRuns` once with the column’s eligible IDs and reuse the same success/partial/failure toast behavior where practical.

**Patterns to follow:**
- Current `BulkActionToolbar` pending state, clear-selection behavior, and toast wording.
- Current `ColumnActionsMenu` archive-all action and cache invalidation.

**Test scenarios:**
- Happy path: selecting multiple archived rows and clicking Unarchive calls the batch helper once and clears selection after all items succeed.
- Happy path: selecting multiple terminal rows and clicking Archive calls the batch helper once and invalidates run-list caches once.
- Mixed result: partial failure keeps selection and shows an error-tone partial-success toast with succeeded/failed counts.
- Ineligible selection: selecting only non-archivable runs still shows the existing “No selected runs can be archived” style error without making an API request.
- Board action: archive-all for a column calls the batch helper once with all eligible IDs.

**Verification:**
- UI behavior remains equivalent from the user’s perspective, but network behavior becomes one request per bulk action.

## System-Wide Impact

- **Auth:** Batch endpoints are user-only. This intentionally avoids giving a worker token with one run scope the ability to mutate arbitrary run IDs from a request body.
- **Events:** No new event names or payloads. Each successful item appends the existing per-run archive/unarchive event.
- **Caching:** Frontend run-list caches are still invalidated after lifecycle changes. The batch path should reduce invalidation churn from once per selected run to once per user action.
- **Generated clients:** Both Rust and TypeScript generated clients change because OpenAPI is the source of truth.
- **Compatibility:** Single-run endpoints and existing generated methods remain available and unchanged.

## Risks & Mitigations

| Risk | Mitigation |
|------|------------|
| Route ambiguity with `/runs/{id}` paths | Use static collection action paths and add server tests that call the exact batch routes. |
| Partial mutation surprises | Use explicit fail-soft result entries and summarize succeeded/failed counts. |
| Worker token privilege expansion | Require `RequiredUser` for batch endpoints. |
| Duplicate IDs producing confusing results | Reject duplicates at request validation before any mutation. |
| UI toast regressions | Cover all-success, all-failure, partial-failure, and ineligible-selection behavior in frontend tests. |

## Documentation / Operational Notes

- The OpenAPI descriptions should explicitly state that batch actions are fail-soft and not transactional.
- No migration, feature flag, or rollout sequencing is required.
- No new public docs are required unless API reference publishing is part of the release process.

## Sources & References

- OpenAPI source: `docs/public/api-reference/fabro-api.yaml`
- Server lifecycle handlers: `lib/crates/fabro-server/src/server/handler/lifecycle.rs`
- Workflow archive operations: `lib/crates/fabro-workflow/src/operations/archive.rs`
- Web lifecycle helpers: `apps/fabro-web/app/lib/run-actions.ts`
- Web runs route: `apps/fabro-web/app/routes/runs.tsx`
- Testing guidance: `docs/internal/testing-strategy.md`
- Error handling guidance: `docs/internal/error-handling-strategy.md`
- Event guidance: `docs/internal/events-strategy.md`


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
  - Model: gpt-5.5, 97.5k tokens in / 7.1k out
- **simplify_opus**: succeeded
  - Model: claude-opus-4-7, 15.6k tokens in / 2.6k out


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