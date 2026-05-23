Implemented manual run retry end-to-end.

Summary:
- Added `retried_from` to:
  - `Run`
  - `RunProjection`
  - `run.created` event props
  - OpenAPI `Run` / `RunProjection`
  - generated TS API client models
- Added workflow retry operation:
  - Creates a fresh run store
  - Copies durable definition/settings/source/git/labels/title/current parent/fork source/blob refs
  - Sets new `created_by` via provenance
  - Sets `retried_from`
  - Does not copy checkpoints/runtime/sandbox runtime/PR/conclusion/questions/pending controls
  - Rejects succeeded, active, cancelled, archived, and missing runs
- Added `POST /api/v1/runs/{id}/retry`:
  - Returns `201` with the new queued `Run`
  - Queues via the same internal start path used by `POST /runs/{id}/start`
  - Leaves source run unchanged
- Updated web UI:
  - Added `Retry` action for eligible failed/dead runs, hidden in demo mode
  - Disables while pending
  - Calls generated client, refreshes caches, and navigates to the new run on success
  - Shows retry errors via toast
  - Adds compact `Retried from` link in run summary panel
- Regenerated Rust API and TypeScript Axios client.

Validation run:
- `cargo build -q -p fabro-api`
- `cargo check -q --workspace`
- `cargo +nightly-2026-04-14 fmt --check --all`
- `cargo +nightly-2026-04-14 clippy -q --workspace --all-targets -- -D warnings`
- `cargo test -q -p fabro-api`
- `cargo test -q -p fabro-store legacy_run_created_projects_retried_from_none --lib`
- `cargo test -q -p fabro-workflow retry_ --lib`
- `cargo test -q -p fabro-server retry_ --lib`
- `cd apps/fabro-web && bun run typecheck`
- `cd apps/fabro-web && bun test app/lib/run-actions.test.ts app/components/run-summary-panel.test.tsx app/routes/run-detail.test.ts`

Note: I also attempted the full `cd apps/fabro-web && bun test`; it still reports unrelated test-isolation failures around shared `generatedAxios.defaults.adapter` state plus an existing run-detail dock assertion, while the focused retry/UI tests above pass.