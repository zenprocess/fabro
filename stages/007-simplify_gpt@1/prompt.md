Goal: ---
title: Add run.checkpoint.skip_git_hooks
type: feat
status: active
date: 2026-05-22
---

# Add `run.checkpoint.skip_git_hooks`

## Summary

Add an opt-in setting:

```toml
[run.checkpoint]
skip_git_hooks = true
```

When enabled, Fabro-created run-branch checkpoint commits bypass Git commit hooks. Default remains `false`, preserving current behavior. This setting does not affect Fabro workflow hooks or metadata-branch snapshots.

## Key Changes

- Add `skip_git_hooks: bool` to dense checkpoint settings in `fabro-types`, defaulting to `false`.
- Add `skip_git_hooks: Option<bool>` to sparse `fabro-config::RunCheckpointLayer` so layered config can distinguish unset from explicit `false`.
- Update checkpoint layer merging so `exclude_globs` keeps its existing replace/inherit behavior and `skip_git_hooks` uses normal override semantics.
- Update checkpoint resolution so missing config resolves to `skip_git_hooks = false`.
- Thread the resolved setting into run-branch checkpoint commit creation.
- Append the hook-skipping commit option only for Fabro-managed run-branch checkpoint commits, including:
  - normal lifecycle checkpoint commits in `sandbox_git.rs`
  - parallel base checkpoint commits through the same helper
  - parallel branch worktree commits in `handler/parallel.rs`
- Update OpenAPI `RunCheckpointSettings` and regenerate the TypeScript API client so persisted run settings expose the new field.
- Update user docs/options reference to document `skip_git_hooks`, its default, and that it does not disable Fabro `[[run.hooks]]`.

## Test Plan

- `fabro-config` tests:
  - `[run.checkpoint] skip_git_hooks = true` resolves to `true`.
  - omitted `skip_git_hooks` resolves to `false`.
  - higher-layer `skip_git_hooks = false` overrides lower-layer `true`.
  - `exclude_globs` merging behavior remains unchanged.
- `fabro-workflow` tests:
  - checkpoint commit command includes the hook-skip option when enabled.
  - checkpoint commit command omits it when disabled.
  - parallel branch commit command respects the setting.
- API/client tests:
  - `WorkflowSettings` / `RunCheckpointSettings` round-trip includes `skip_git_hooks`.
  - OpenAPI-generated Rust and TypeScript clients reflect the new boolean field.
- Regression:
  - existing git checkpoint tests still pass, including metadata branch tests.

## Assumptions

- The setting applies to Git commit hooks for run-branch checkpoint commits; it does not change metadata-branch behavior because metadata snapshots already use `git2` and do not fire local hooks.
- The setting does not disable Fabro lifecycle hooks under `[[run.hooks]]`.
- Default `false` is required for backward compatibility.


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
  - Model: claude-opus-4-7, 150.3k tokens in / 39.4k out
  - Files: /home/daytona/workspace/fabro/docs/public/administration/server-configuration.mdx, /home/daytona/workspace/fabro/docs/public/api-reference/fabro-api.yaml, /home/daytona/workspace/fabro/docs/public/execution/run-configuration.mdx, /home/daytona/workspace/fabro/lib/crates/fabro-api/tests/workflow_settings_round_trip.rs, /home/daytona/workspace/fabro/lib/crates/fabro-cli/tests/it/cmd/attach.rs, /home/daytona/workspace/fabro/lib/crates/fabro-cli/tests/it/cmd/inspect.rs, /home/daytona/workspace/fabro/lib/crates/fabro-config/src/layers/combine.rs, /home/daytona/workspace/fabro/lib/crates/fabro-config/src/layers/run.rs, /home/daytona/workspace/fabro/lib/crates/fabro-config/src/resolve/run.rs, /home/daytona/workspace/fabro/lib/crates/fabro-config/src/tests/resolve_run.rs, /home/daytona/workspace/fabro/lib/crates/fabro-types/src/settings/run.rs, /home/daytona/workspace/fabro/lib/crates/fabro-workflow/src/handler/parallel.rs, /home/daytona/workspace/fabro/lib/crates/fabro-workflow/src/lifecycle/git.rs, /home/daytona/workspace/fabro/lib/crates/fabro-workflow/src/pipeline/execute.rs, /home/daytona/workspace/fabro/lib/crates/fabro-workflow/src/run_options.rs, /home/daytona/workspace/fabro/lib/crates/fabro-workflow/src/sandbox_git.rs, /home/daytona/workspace/fabro/lib/packages/fabro-api-client/src/models/run-checkpoint-settings.ts
- **simplify_opus**: succeeded
  - Model: claude-opus-4-7, 46.9k tokens in / 13.0k out
  - Files: /home/daytona/workspace/fabro/lib/crates/fabro-workflow/src/handler/parallel.rs, /home/daytona/workspace/fabro/lib/crates/fabro-workflow/src/pipeline/execute.rs, /home/daytona/workspace/fabro/lib/crates/fabro-workflow/src/sandbox_git.rs


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