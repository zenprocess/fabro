Goal: ---
title: Add Manual Run Retry
type: feat
status: active
date: 2026-05-23
---

# Add Manual Run Retry

## Summary

Add a **Retry** action for failed Fabro runs that creates and immediately starts a new run from the failed run's captured run definition. The new run is independent runtime state, records `retried_from: <source_run_id>`, and leaves the source run unchanged.

This is a fresh run, not resume/fork/rewind. It should copy the source run's durable definition and settings, but not checkpoints, stage state, sandbox state, PR links, billing, questions, conclusions, or pending controls.

## Key Changes

- Add `retried_from` as a nullable public field on `Run`.
  - Store it on the new run only.
  - Do not add a reverse `retried_by` field in v1.
  - Preserve backward compatibility with old events by defaulting to `null`.

- Add `POST /api/v1/runs/{id}/retry`.
  - Response: `201` with the newly created/queued `Run`.
  - Eligible source states: `failed` except `reason=cancelled`, and `dead`.
  - Reject active, succeeded, cancelled, archived, and missing runs with existing API error patterns.
  - The new run should use the current authenticated actor as `created_by`.
  - The new run should preserve the source run's current `parent_id`, title, labels, workflow graph/source, resolved settings, git context, manifest/definition blob refs, and `fork_source_ref` if present.

- Implement retry using a workflow operation similar in shape to `fork`, but without replaying checkpoint/runtime events.
  - Create a new run store.
  - Append `run.created` with `retried_from`.
  - Append `run.submitted`.
  - Queue/start it through the same internal start path used by `POST /runs/{id}/start`.

- Update OpenAPI and generated clients.
  - Edit `docs/public/api-reference/fabro-api.yaml`.
  - Regenerate Rust API types through `cargo build -p fabro-api`.
  - Regenerate TypeScript client in `lib/packages/fabro-api-client`.

- Update the web UI.
  - Add `Retry` to the run action menu for eligible failed/dead runs.
  - Disable the action while pending.
  - On success, navigate to the new run page and refresh run/list caches.
  - Add a compact "Retried from" link in the run summary panel when `retried_from` is present.
  - Add demo-mode support or hide the action in demo mode so the button never navigates to a missing demo run.

## Test Plan

- Rust workflow/store tests:
  - `run.created` serializes/deserializes `retried_from`.
  - Old `run.created` events project with `retried_from = None`.
  - Retry creates a new run with a different ID, copied durable definition, no runtime state, and `retried_from` set.
  - Retry preserves current `parent_id`, title, labels, git context, settings, and `fork_source_ref`.
  - Retry rejects succeeded, active, cancelled, and archived source runs.

- Rust server/API tests:
  - `POST /runs/{id}/retry` on a failed run returns `201`, a new run ID, `retried_from`, and queued/started lifecycle state.
  - Source run remains unchanged.
  - `404` for unknown run.
  - `409` for non-retryable status.
  - Generated Rust API compiles against the updated OpenAPI contract.

- Web tests:
  - `canRetry` returns true for failed/dead, false for cancelled/succeeded/active/archived.
  - Action menu renders `Retry` only when eligible.
  - Successful retry calls the generated client and navigates to `/runs/:newId`.
  - Retry errors show a useful toast/message.
  - Run summary panel renders the `Retried from` link when present.
  - Typecheck passes with regenerated client types.

## Assumptions

- V1 does not add a CLI `fabro retry` command.
- V1 does not add automatic retry attempts, retry counts, or idempotency keys.
- Multiple manual clicks after the first request completes may create multiple retry runs.
- "Same settings" means the source run's captured durable definition/settings, not latest local files from the user's machine.
- Cancelled runs are excluded because cancellation is user intent, not execution failure.


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


Read the plan file referenced in the goal and implement every step. Make all the code changes described in the plan. Use red/green TDD.