Review/cleanup complete. I found and fixed two issues:

- Added `skip_git_hooks: false` to the `WorkflowSettings` sample object in `apps/fabro-web/app/routes/automation-detail.tsx`, since the generated API client now requires the field.
- Added machine-readable `default: false` to OpenAPI `RunCheckpointSettings.skip_git_hooks` in `docs/public/api-reference/fabro-api.yaml`.

Validation run:

- `cargo test -p fabro-api --test workflow_settings_round_trip` ✅
- `cargo test -p fabro-config run_checkpoint_skip_git_hooks` ✅
- `cargo test -p fabro-workflow skip_hooks` ✅
- `git diff --check` ✅

Attempted but could not complete due local tool/dependency environment:

- `cd apps/fabro-web && bun run typecheck` failed because `tsc` is not installed/found.
- `cd lib/packages/fabro-api-client && bun run generate` failed during OpenAPI generator CLI startup; no generated files were changed.

Current uncommitted cleanup changes:

- `apps/fabro-web/app/routes/automation-detail.tsx`
- `docs/public/api-reference/fabro-api.yaml`