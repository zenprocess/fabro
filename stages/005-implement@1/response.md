Implemented MCP tool parity for `[run.agent] fabro_tools = true`.

Summary:
- Added `fabro_run_pair` to the shared `fabro-tool` catalog with schema/test coverage.
- Added workflow-agent dispatcher support for `fabro_run_pair`.
- Kept workflow-agent `fabro_run_create` forced-child behavior and conflict rejection.
- Replaced old run-tools extractor names with:
  - `RequiredRunManagementActor`
  - `RequireRunManagementTarget`
- Migrated MCP-backed run-management routes to the new extractors.
- Migrated pair routes so run-tools workers can call status/start/message/end/transcript while preserving worker provenance.
- Kept user-only APIs user-only, including negative coverage for run-tools workers.
- Confirmed Ask Fabro remains read-only with only `fabro_run_get` and `fabro_run_events`.
- Updated public docs for `fabro_tools` parity and the create-parent exception.

TDD/verification:
- Confirmed red first for new catalog/auth coverage.
- `cargo nextest run -p fabro-tool -p fabro-workflow -p fabro-server -p fabro-cli` ✅
- `cargo +nightly-2026-04-14 fmt --check --all` ✅
- `cargo +nightly-2026-04-14 clippy --workspace --all-targets -- -D warnings` ✅
- `cargo nextest run --workspace` ✅ (6343 passed, 181 skipped)