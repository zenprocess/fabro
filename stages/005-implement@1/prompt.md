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


Read the plan file referenced in the goal and implement every step. Make all the code changes described in the plan. Use red/green TDD.