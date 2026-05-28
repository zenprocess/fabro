Implemented Issue #399 automation run endpoints.

Summary:
- Added `GET /automations/{id}/runs`
  - Requires automation existence.
  - Lists cached persisted runs only.
  - Filters by `Run.automation.id`.
  - Sorts newest-first.
  - Uses existing `page[limit]` / `page[offset]` pagination envelope.

- Added `POST /automations/{id}/runs`
  - Uses new `RequiredRunToolActor`.
  - Requires automation to exist, be enabled, and have an enabled API trigger.
  - Returns `409` with code `automation_api_trigger_disabled` for disabled automation/API trigger or missing API trigger.
  - Uses configured `AutomationRunMaterializer`.
  - Calls the shared run creation helper with populated `AutomationRef`.

- Added integration coverage using the fake materializer for:
  - Disabled automation.
  - Disabled API trigger.
  - Missing API trigger.
  - Missing automation 404s.
  - Successful API-triggered run creation.
  - Persisted/retrievable `Run.automation`.
  - Automation run listing filtering.
  - Newest-first sorting and pagination.

Changed files:
- `lib/crates/fabro-server/src/server/handler/automations.rs`
- `lib/crates/fabro-server/src/principal_middleware.rs`
- `lib/crates/fabro-server/tests/it/api/automations.rs`

Verification run:
- `cargo nextest run -p fabro-automation` ✅
- `cargo nextest run -p fabro-api` ✅
- `cargo nextest run -p fabro-server --features test-support automations` ✅
- `cargo nextest run -p fabro-server --features test-support openapi_conformance` ✅
- `cargo check -p fabro-server` ✅
- `cargo +nightly-2026-04-14 fmt --check --all` ✅
- `cargo +nightly-2026-04-14 clippy --workspace --all-targets -- -D warnings` ✅
- `git diff -- apps/fabro-web lib/crates/fabro-cli` ✅ empty

Note: the `fabro-server` integration test binary is gated behind the crate’s `test-support` feature, so I used `--features test-support` for the focused `automations` and `openapi_conformance` integration runs.