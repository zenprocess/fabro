Implemented the plan in two commits.

Commits:
- `26fdc23a9 refactor(auth): remove anonymous principal`
- `5fea2c5ad Make run provenance and creator total`

Summary:
- Removed anonymous principal representation from auth/API/frontend.
- Request auth now represents absent/invalid auth as `None`.
- HTTP logs always emit `principal_kind`, using `"none"` for absent principal.
- Made run creator/provenance total across Rust types, workflow events, store projections, server create/retry flows, OpenAPI, generated TS client, frontend data/UI, demo mode, and tests.
- Added gated `fabro-types::test_support` helpers.
- Regenerated Rust OpenAPI types and TypeScript API client.
- Working tree is clean.

Verification run:
- `cargo build -p fabro-api`
- `cd lib/packages/fabro-api-client && bun run generate`
- `cargo +nightly-2026-04-14 fmt --check --all`
- `cargo +nightly-2026-04-14 clippy --workspace --all-targets -- -D warnings`
- `cargo build --workspace`
- `cargo nextest run --workspace`
- `cargo nextest run -p fabro-server`
- `cd lib/packages/fabro-api-client && bun run typecheck`
- `cd apps/fabro-web && bun run typecheck && bun test && bun run build`
- Final sweeps for anonymous principal / nullable creator-provenance remnants; only unrelated matches remained (`crossorigin="anonymous"`, todo/auth subject fields).
- Manual unauthenticated protected endpoint check:
  - `/api/v1/runs` returned `401`
  - HTTP log included `principal_kind="none"` and `auth_status="missing"`
- Manual authenticated demo-mode API check:
  - `X-Fabro-Demo: 1` run list returned synthetic creator:
    - `login: "demo"`
    - `auth_method: "dev_token"`
    - `identity: { issuer: "fabro:demo", subject: "demo" }`

Not run:
- Full browser-driven UI create/retry manual flow; this terminal environment has no interactive browser/auth session.