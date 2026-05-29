Goal: ---
title: "feat: Add Environment REST CRUD API"
type: feat
status: active
date: 2026-05-28
---

# feat: Add Environment REST CRUD API

## Summary

Add server-owned Environment CRUD under `/api/v1/environments`, modeled after
Automations and backed by the existing `EnvironmentStore`. The API manages only
the server-side environment catalog in `environments/*.toml`; client-side
environment definitions in `workflow.toml`, `.fabro/project.toml`, or run inputs
continue to work and are not managed by this API.

## API Contract

- Add OpenAPI paths:
  - `GET /api/v1/environments`
  - `POST /api/v1/environments`
  - `GET /api/v1/environments/{id}`
  - `PUT /api/v1/environments/{id}`
  - `DELETE /api/v1/environments/{id}`
- Mirror Automations semantics:
  - List returns `{ data: Environment[], meta: { total } }`, sorted by id.
  - Create body includes `id`; replace body omits `id`; path id is authoritative.
  - `GET` and `PUT` return `ETag: "<revision>"`.
  - `PUT` and `DELETE` require `If-Match`.
  - Use existing Automation-style statuses: `400`, `404`, `409`, `422`, `428`, `500`.
  - Stale revisions return `409` to match Automations.
- Add API-specific Environment request/response schemas so REST `image.dockerfile`
  accepts only inline content or `null`.
  - Existing workflow/settings schemas keep supporting Dockerfile `path`.
  - REST requests with Dockerfile `path` return `422` and must not read
    server-local files.
- Do not add `PATCH` in v1.

## Implementation Changes

- OpenAPI and generated clients:
  - Update `docs/public/api-reference/fabro-api.yaml` with an `Environments` tag,
    an `EnvironmentId` parameter, CRUD paths, list envelope, and inline-only API
    image schema.
  - Regenerate Rust API types and the TypeScript Axios client.
  - Keep the existing `EnvironmentSettings` schema intact for workflow settings.
- Server:
  - Add `lib/crates/fabro-server/src/server/handler/environments.rs`, following
    `automations.rs` for routes, auth, ETag parsing, and error mapping.
  - Merge the routes into real API routes; do not add demo routes unless an
    existing convention requires it.
  - Convert API request DTOs into `EnvironmentDraft` / `EnvironmentSettings` only
    after rejecting Dockerfile path sources.
  - Map `EnvironmentStoreError` similarly to Automations: duplicate, protected,
    and stale as `409`; missing as `404`; validation as `422`; internal
    storage/parse/io as curated `500`.
  - After successful create, replace, or delete, refresh cached manifest run
    settings from the current `EnvironmentStore` catalog so `/system/info` and
    default run settings reflect the updated catalog.
- Domain and API types:
  - Use a meaningful API DTO boundary rather than treating REST and TOML as
    identical Dockerfile-source surfaces.
  - Reuse `fabro-environment::Environment` for persisted domain behavior where
    the wire shape matches; keep API-only request schemas distinct where
    inline-only Dockerfile behavior differs.

## Implementation Units

- [ ] **Unit 1: Define the OpenAPI contract**
  - Add the environment CRUD paths, schemas, and path parameter.
  - Ensure the spec distinguishes REST-safe inline Dockerfile sources from the
    existing workflow/settings Dockerfile source schema.
  - Verification: OpenAPI route conformance can see the new paths and generated
    clients expose an `EnvironmentsApi`.

- [ ] **Unit 2: Add server environment handlers**
  - Implement a new handler module mirroring the Automation CRUD handler shape.
  - Enforce authentication, id parsing, ETag/If-Match behavior, and error mapping.
  - Reject REST Dockerfile path sources before calling `EnvironmentStore`.
  - Verification: server API tests prove CRUD behavior and failure responses.

- [ ] **Unit 3: Refresh derived server state after mutations**
  - Ensure successful environment create, replace, and delete refresh any cached
    manifest run settings derived from `EnvironmentStore::catalog_layer()`.
  - Preserve existing client-side environment precedence and behavior.
  - Verification: a test proves newly created server environments affect the
    resolved server default run environment where applicable.

- [ ] **Unit 4: Regenerate clients and add contract tests**
  - Regenerate `fabro-api` and `lib/packages/fabro-api-client`.
  - Add Rust server integration tests and keep OpenAPI conformance passing.
  - Verification: generated Rust and TypeScript surfaces compile and expose the
    new environment operations.

## Test Plan

- Add server API tests in
  `lib/crates/fabro-server/tests/it/api/environments.rs` and register the module.
- Cover:
  - List returns seeded environments and correct total.
  - Create persists `environments/{id}.toml`, returns `201`, and is visible via
    list/get.
  - Get returns current `ETag` matching `revision`.
  - Replace with valid `If-Match` updates the file, returns a new revision, and
    updates the `ETag`.
  - Replace/delete without `If-Match` return `428`.
  - Stale replace/delete return `409`.
  - Duplicate create returns `409`.
  - Invalid id/header returns `400`.
  - Domain validation failures return `422`.
  - Dockerfile `path` over REST returns `422` and does not persist or expose file
    contents.
  - Delete removes a non-default environment; deleting `default` returns a
    protected conflict.
  - Unauthenticated environment routes return `401`.
  - Creating an environment referenced by server default run settings refreshes
    cached manifest run settings.

## Assumptions

- This API manages server-owned environments only; client-defined catalogs remain
  file/request scoped.
- Built-in seed behavior follows the current store: seeded environments are
  listed, create conflicts with existing ids, and `default` is protected from
  delete.
- Create responses match Automations and do not need an `ETag`; clients can use
  the returned `revision` or call `GET`.
- Inline-only Dockerfile policy applies only to REST CRUD, not local TOML
  configuration.

## Sources

- `docs/public/api-reference/fabro-api.yaml`
- `lib/crates/fabro-server/src/server/handler/automations.rs`
- `lib/crates/fabro-environment/src/store.rs`
- `lib/crates/fabro-environment/src/model.rs`
- `docs/public/execution/environments.mdx`


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