Goal: # Remove `features.session_sandboxes`

## Summary

Remove the `session_sandboxes` feature flag and the now-empty `[features]` settings namespace entirely. Behavior should be as if `session_sandboxes = true` was always set: Ask Fabro is never disabled by a feature flag, and UI controls previously hidden behind the flag are always shown.

## Key Changes

- Remove the settings namespace from config:
  - Delete `FeaturesNamespace`, `FeaturesLayer`, `resolve_features`, and `[features]` defaults.
  - Remove `features` from resolved `ServerSettings` and `UserSettings`.
  - Remove `features` from the top-level settings parser allow-list, so old `[features]` config is rejected as unknown.
- Remove the runtime gate:
  - Simplify Ask Fabro readiness to check only sandbox presence/runtime and LLM configuration.
  - Remove `AskFabroUnavailableReason::FeatureDisabled` and the "Ask Fabro is disabled" tooltip.
- Update frontend behavior:
  - Run detail page no longer handles `FEATURE_DISABLED`.
  - Start page always renders the project/branch controls and no longer fetches system info just for this flag.
- Remove public API surfaces:
  - `/api/v1/settings` `ServerSettings` no longer includes `features`.
  - `/api/v1/system/info` no longer includes `features`.
  - OpenAPI removes `FeaturesNamespace`, `SystemFeatures`, `ServerSettings.features`, `SystemInfoResponse.features`, and `feature_disabled`.
  - Regenerate Rust API types and TypeScript Axios client.
- Update current docs:
  - Remove `[features]` from active configuration docs, generated options docs, API docs, and unknown-key guidance.
  - Do not touch unrelated meanings of "features" such as Cargo features, LLM model features, or devcontainer features.

## Test Plan

- Update or remove tests that assert `features.session_sandboxes` in config, settings, system info, and Ask Fabro readiness.
- Add or adjust coverage for:
  - Ask Fabro unavailable reasons are only `no_sandbox`, `sandbox_not_ready`, or `llm_unconfigured`.
  - Settings parsing rejects top-level `[features]`.
  - `/api/v1/settings` response contains only `server` at the top level.
  - `/api/v1/system/info` has no `features` field.
  - Start page renders project/branch controls without consulting `SystemInfo.features`.
- Run:
  - `cargo build -p fabro-api`
  - `cd lib/packages/fabro-api-client && bun run generate`
  - `cargo dev docs refresh && cargo dev docs check`
  - `cargo nextest run -p fabro-config -p fabro-api -p fabro-server -p fabro-cli`
  - `cd apps/fabro-web && bun test && bun run typecheck`
  - `cargo +nightly-2026-04-14 fmt --check --all`
  - `cargo +nightly-2026-04-14 clippy --workspace --all-targets -- -D warnings`

## Acceptance Checks

- `rg -n "session_sandboxes|FeaturesNamespace|SystemFeatures|feature_disabled|Ask Fabro is disabled" lib apps docs/public` returns no relevant matches.
- `rg -n "\\[features\\]" docs/public lib/crates/fabro-config/src lib/crates/fabro-types/src lib/crates/fabro-server/src apps/fabro-web/app` returns no settings-namespace matches.
- Existing sandbox runtime behavior remains unchanged; only the feature flag and schema surface are removed.

## Assumptions

- This is intentionally a breaking config/API cleanup: existing user config containing `[features]` should fail validation until removed.
- Historical internal plans may still contain old text unless they are part of active public docs; implementation should prioritize product code, generated clients, and current docs.


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
  - Model: claude-opus-4-7, 226.6k tokens in / 53.2k out
  - Files: /home/daytona/workspace/fabro/apps/fabro-web/app/routes/run-detail.tsx, /home/daytona/workspace/fabro/apps/fabro-web/app/routes/settings-resources.test.tsx, /home/daytona/workspace/fabro/apps/fabro-web/app/routes/start.tsx, /home/daytona/workspace/fabro/docs/public/administration/server-configuration.mdx, /home/daytona/workspace/fabro/docs/public/api-reference/fabro-api.yaml, /home/daytona/workspace/fabro/docs/public/execution/run-configuration.mdx, /home/daytona/workspace/fabro/docs/public/reference/user-configuration.mdx, /home/daytona/workspace/fabro/lib/crates/fabro-api/build.rs, /home/daytona/workspace/fabro/lib/crates/fabro-api/src/lib.rs, /home/daytona/workspace/fabro/lib/crates/fabro-api/tests/server_settings_round_trip.rs, /home/daytona/workspace/fabro/lib/crates/fabro-cli/tests/it/cmd/config.rs, /home/daytona/workspace/fabro/lib/crates/fabro-config/src/builders.rs, /home/daytona/workspace/fabro/lib/crates/fabro-config/src/defaults.toml, /home/daytona/workspace/fabro/lib/crates/fabro-config/src/layers/combine.rs, /home/daytona/workspace/fabro/lib/crates/fabro-config/src/layers/mod.rs, /home/daytona/workspace/fabro/lib/crates/fabro-config/src/layers/settings.rs, /home/daytona/workspace/fabro/lib/crates/fabro-config/src/lib.rs, /home/daytona/workspace/fabro/lib/crates/fabro-config/src/parse.rs, /home/daytona/workspace/fabro/lib/crates/fabro-config/src/resolve/mod.rs, /home/daytona/workspace/fabro/lib/crates/fabro-config/src/tests/mod.rs, /home/daytona/workspace/fabro/lib/crates/fabro-config/src/tests/resolve_cli.rs, /home/daytona/workspace/fabro/lib/crates/fabro-config/src/tests/resolve_server.rs, /home/daytona/workspace/fabro/lib/crates/fabro-server/src/demo/mod.rs, /home/daytona/workspace/fabro/lib/crates/fabro-server/src/server.rs, /home/daytona/workspace/fabro/lib/crates/fabro-server/src/server/handler/system.rs, /home/daytona/workspace/fabro/lib/crates/fabro-server/src/server/tests.rs, /home/daytona/workspace/fabro/lib/crates/fabro-server/tests/it/api/runs.rs, /home/daytona/workspace/fabro/lib/crates/fabro-server/tests/it/api/settings.rs, /home/daytona/workspace/fabro/lib/crates/fabro-server/tests/it/api/system.rs, /home/daytona/workspace/fabro/lib/crates/fabro-types/src/dense.rs, /home/daytona/workspace/fabro/lib/crates/fabro-types/src/run_summary.rs, /home/daytona/workspace/fabro/lib/crates/fabro-types/src/settings/mod.rs, /home/daytona/workspace/fabro/lib/packages/fabro-api-client/src/.openapi-generator/FILES, /home/daytona/workspace/fabro/lib/packages/fabro-api-client/src/models/ask-fabro.ts, /home/daytona/workspace/fabro/lib/packages/fabro-api-client/src/models/index.ts, /home/daytona/workspace/fabro/lib/packages/fabro-api-client/src/models/server-settings.ts, /home/daytona/workspace/fabro/lib/packages/fabro-api-client/src/models/system-info-response.ts
- **simplify_opus**: succeeded
  - Model: claude-opus-4-7, 62.9k tokens in / 13.3k out
  - Files: /home/daytona/workspace/fabro/lib/crates/fabro-config/src/tests/resolve_server.rs, /home/daytona/workspace/fabro/lib/crates/fabro-server/tests/it/api/settings.rs


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