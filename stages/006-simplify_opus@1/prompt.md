Goal: ---
date: 2026-05-25
status: active
---

# Add Provider Probe Endpoint

## Summary

Add `POST /api/v1/providers/test` as the focused LLM-provider subset of server diagnostics. V1 has no request body: it tests every configured LLM provider once, using the catalog probe model, and returns typed results suitable for API, CLI, or UI callers without parsing `/health/diagnostics`.

## Key Changes

- Add OpenAPI operation `testProviders` under the Models tag:
  - `POST /api/v1/providers/test`
  - Response schema `ProviderTestList`
  - `data[]`: `{ provider, model_id, status, error_message }`
  - `summary`: `{ status, total, passed, failed }`
- Use provider status enum `ok | error`; do not include `skip` because v1 only tests configured providers.
- Make `model_id` nullable for configuration, auth, or registration failures where no probe was sent.
- Return HTTP `200` for provider-level failures; use `summary.status = "error"` when any provider fails or when no providers are configured.
- Keep auth required via the same `RequiredUser` path as `/models` and `/providers`.

## Implementation Changes

- Add `.route("/providers/test", post(test_providers))` alongside existing model routes in the models handler.
- Extract the LLM provider probing portion of diagnostics into shared logic used by both `/providers/test` and `/health/diagnostics`.
- Probe flow:
  - Determine configured providers from server credentials/config in catalog order.
  - For each configured provider, report auth or registration issues as `error` without making a network call.
  - Otherwise choose `Catalog::probe_for_provider(provider)` and send the existing cheap basic probe (`Say OK`, `max_tokens=16`, 30s timeout).
  - Preserve diagnostics output by mapping the shared structured results back into the existing `LLM Providers` check/details format.
- Regenerate API clients after editing `docs/public/api-reference/fabro-api.yaml`:
  - Rust: `cargo build -p fabro-api`
  - TypeScript: `cd lib/packages/fabro-api-client && bun run generate`

## Test Plan

- Add server handler tests for:
  - No configured providers returns `200`, empty `data`, and `summary.status = "error"`.
  - One configured provider with a mocked successful probe returns one `ok` row with the selected probe model.
  - Provider auth or registration issue returns one `error` row and does not call the upstream provider.
  - Mixed providers preserve catalog order and summary counts.
  - Response body does not leak API keys or internal credential material.
- Add or adjust diagnostics test coverage to confirm `/health/diagnostics` still reports the same `LLM Providers` pass/error summaries after sharing the probe logic.
- Run focused verification:
  - `cargo nextest run -p fabro-server test_providers`
  - `cargo nextest run -p fabro-server diagnostics`
  - `cargo build -p fabro-api`
  - `cd apps/fabro-web && bun run typecheck` after TypeScript client regeneration if web imports are affected.

## Assumptions

- V1 intentionally has no request body or provider filter.
- This endpoint is API-only; no CLI command or web UI flow changes are included.
- Provider probe failures are data results, not HTTP failures, matching `POST /api/v1/models/{id}/test` behavior.


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
  - Model: gpt-5.5, 4.6m tokens in / 33.5k out


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