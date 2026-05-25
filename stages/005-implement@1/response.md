Implemented the provider-backed sandbox inventory API end-to-end.

Summary:
- Added OpenAPI `Sandboxes` endpoints:
  - `GET /api/v1/sandboxes`
  - `GET /api/v1/sandboxes/{id}`
- Added canonical Rust DTOs for sandbox inventory in `fabro-types`.
- Wired progenitor replacements and JSON parity tests in `fabro-api`.
- Added provider runtime layer:
  - `SandboxProvider` trait
  - `SandboxProviderRegistry`
  - list/get semantics with partial list errors and get `404`/`409`/`502` behavior
  - local no-inventory provider
- Added Docker and Daytona provider inventory implementations using managed label filtering/verification.
- Refactored shared sandbox detail normalization for run-scoped details and provider-backed inventory.
- Added server routes with run-management authorization and provider registry state.
- Regenerated the TypeScript Axios client for the new endpoints/models.
- Added/updated tests for DTO serde, API replacement parity, provider registry behavior, and server route behavior, including the missing `404` route assertion.
- Cleaned a test-only conditional import warning in `fabro-sandbox`.

Verification passed:
- `cargo test -p fabro-types sandbox_inventory`
- `cargo test -p fabro-api sandbox_inventory_round_trip`
- `cargo test -p fabro-sandbox provider`
- `cargo test -p fabro-server sandboxes`
- `cargo +nightly-2026-04-14 fmt --check --all`
- `cargo nextest run -p fabro-sandbox -p fabro-server -p fabro-api -p fabro-types`
- `cargo +nightly-2026-04-14 clippy --workspace --all-targets -- -D warnings`
- `cd lib/packages/fabro-api-client && bun run typecheck`

No git commit was created.