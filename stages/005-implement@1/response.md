Implemented Environment REST CRUD API end-to-end.

Summary:
- Added OpenAPI `Environments` tag, `/api/v1/environments` CRUD paths, `EnvironmentId` parameter, list envelope, and REST-safe inline-only Dockerfile schemas.
- Added server handler module: `lib/crates/fabro-server/src/server/handler/environments.rs`.
- Mounted real API routes only; no demo routes added.
- Added API DTO boundary that rejects REST Dockerfile `path` sources with `422` before calling `EnvironmentStore`.
- Added ETag / `If-Match` behavior matching Automations.
- Added `EnvironmentStoreError` → `ApiError` mapping with stale/duplicate/protected as `409`.
- Refreshes cached manifest run settings after create/replace/delete so `/system/info` reflects catalog mutations.
- Tightened `fabro-environment` create/replace behavior to validate dense settings before persistence.
- Regenerated Rust API types and TypeScript Axios client; TS now exposes `EnvironmentsApi`.
- Added server integration tests, OpenAPI/SDK conformance test, and Rust API JSON/type-boundary tests.

Validation run:
- Red TDD check: new environment API tests initially failed with `404` before routes existed.
- `cargo nextest run -p fabro-server --features test-support --test it api::environments`
- `cargo nextest run -p fabro-server --features test-support --test it openapi_conformance`
- `cargo test -p fabro-api --test environment_round_trip`
- `cargo nextest run -p fabro-environment`
- `cargo check -q --workspace`
- `cargo +nightly-2026-04-14 fmt --check --all`
- `cargo +nightly-2026-04-14 clippy -q --workspace --all-targets -- -D warnings`
- `cd lib/packages/fabro-api-client && bun run generate`
- `cd lib/packages/fabro-api-client && bun run typecheck`

Note: I installed `openjdk-21-jre-headless` in the container because `openapi-generator-cli` requires `java` for TypeScript client generation.