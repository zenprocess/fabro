All green. Here's a summary of the changes implemented per the plan:

## Summary

**Types & config (`fabro-types`, `fabro-config`)**
- `RunCheckpointSettings`: added `skip_git_hooks: bool` (defaults to `false`).
- `RunCheckpointLayer`: added `skip_git_hooks: Option<bool>` (sparse, distinguishes unset from explicit `false`).
- `Combine for RunCheckpointLayer`: `exclude_globs` keeps replace/inherit semantics; `skip_git_hooks` uses normal option-override semantics.
- `resolve_checkpoint`: missing config now resolves `skip_git_hooks = false`.

**Workflow engine (`fabro-workflow`)**
- `GitState.checkpoint_skip_hooks` field threaded through `pipeline/execute.rs` and `RunOptions::checkpoint_skip_git_hooks()`.
- `git_checkpoint` / `checked_git_checkpoint`: new `skip_git_hooks: bool` parameter that appends ` --no-verify` to the `git commit` command when `true`.
- `lifecycle/git.rs` lifecycle checkpoint path threads the resolved setting through.
- `handler/parallel.rs`: both the parallel-base checkpoint (via `checked_git_checkpoint`) and the per-branch worktree commit respect the setting; extracted `parallel_branch_commit_cmd` helper for testability.

**OpenAPI / clients**
- `docs/public/api-reference/fabro-api.yaml` `RunCheckpointSettings`: added `skip_git_hooks: boolean` (required).
- `lib/packages/fabro-api-client/src/models/run-checkpoint-settings.ts`: regenerated with the new field.

**Docs**
- `docs/public/execution/run-configuration.mdx`: documents `skip_git_hooks`, default `false`, that it doesn't disable `[[run.hooks]]`.
- `docs/public/administration/server-configuration.mdx`: same.

**Tests added (TDD: red → green for each)**
- `fabro-config` (`tests::resolve_run::run_checkpoint_skip_git_hooks::*`): 4 tests — `true` resolves, default `false`, higher-layer `false` overrides lower `true`, and `exclude_globs` replace behavior is preserved when only `skip_git_hooks` is set on a higher layer.
- `fabro-workflow` (`sandbox_git::tests::*`): 2 new tests — checkpoint commit includes `--no-verify` when enabled, omits when disabled.
- `fabro-workflow` (`handler::parallel::tests::*`): 2 new tests covering the parallel-branch commit command builder.
- `fabro-api` (`workflow_settings_round_trip.rs`): JSON round-trip includes `skip_git_hooks`, defaults to `false`.

**Snapshot updates**
- `lib/crates/fabro-cli/tests/it/cmd/attach.rs` and `inspect.rs` inline JSON updated to include `"skip_git_hooks": false`.

Final status: workspace `cargo nextest run` → 6067 passed, clippy clean, fmt clean.