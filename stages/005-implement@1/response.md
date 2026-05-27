Implemented #396.

Summary:
- Added the public `/api/v1/automations` OpenAPI contract:
  - CRUD automation endpoints
  - automation run list/create endpoints
  - `If-Match` parameters and `ETag` response headers
  - discriminator-based trigger schemas
  - requested response/status coverage
- Reused compatible `fabro-automation` Rust domain types in `fabro-api` via `with_replacement(...)`.
- Aligned automation domain serde wire shape with the public API (`target.ref`, schedule `expression`, required response fields).
- Added `lib/crates/fabro-api/tests/automation_round_trip.rs` with type identity and JSON round-trip/parity coverage.
- Regenerated the TypeScript Axios client, including `AutomationsApi` and automation models.
- No web UI or CLI consumer imports/changes were added.

Verification run:
- Red phase confirmed initially: new automation round-trip test failed before schema/replacement work.
- `cargo build -p fabro-api` ✅
- `cargo nextest run -p fabro-api` ✅
- `cargo nextest run -p fabro-api --test automation_round_trip` ✅
- `cargo nextest run -p fabro-automation` ✅
- `cd lib/packages/fabro-api-client && bun run generate` ✅
- `cd lib/packages/fabro-api-client && bun run typecheck` ✅
- `cargo +nightly-2026-04-14 clippy -p fabro-api --all-targets -- -D warnings` ✅
- `git diff -- apps/fabro-web lib/crates/fabro-cli` ✅ empty