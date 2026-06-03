Both commits are done. Summary:

**Commit 1** (`refactor: remove Principal::Anonymous, use Option<Principal> for absent auth`): Dropped `Principal::Anonymous` variant. Auth middleware now uses `Option<Principal>` for request context. HTTP logs emit `principal_kind="none"` for unauthenticated requests. Updated OpenAPI spec, regenerated TS client, removed frontend anonymous icon/case.

**Commit 2** (`refactor: make Run.created_by, RunSpec.provenance, and RunProvenance.subject total`): Made `Run.created_by`, `RunSpec.provenance`, `RunProvenance.subject`, `RunCreatedProps.provenance`, and `Event::RunCreated.provenance` non-optional. Threaded actual `RunProvenance` through `CreateRunInput`, `RetryRunInput`, and `PersistCreateOptions`. Added `fabro_types::test_support::{test_principal, test_run_provenance}` behind the existing `test-support` feature for tests. Added a clearly synthetic `DEMO_PRINCIPAL` (issuer `fabro:demo`, dev-token) in demo mode. Updated OpenAPI spec to require `subject`/`provenance`/`created_by`. Regenerated the TS client and updated the frontend to treat `createdBy` as total inside loaded-run branches. Backward-compatibility tests for null provenance/creator were either deleted or updated to include the total provenance.

Verification:
- `cargo +nightly-2026-04-14 fmt --check --all` ✓
- `cargo +nightly-2026-04-14 clippy --workspace --all-targets -- -D warnings` ✓
- `cargo build --workspace` ✓
- `cargo nextest run --workspace` — 6499 passed, 0 failed, 181 skipped ✓
- `cd apps/fabro-web && bun run typecheck` ✓
- `cd apps/fabro-web && bun test` — 508 passed, 0 failed ✓
- `cd apps/fabro-web && bun run build` ✓
- `cd lib/packages/fabro-api-client && bun run typecheck` ✓

Manual end-to-end verification (server start, UI run creation, demo header) was not exercised in this run.