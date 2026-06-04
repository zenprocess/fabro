Done.

Created the requested two commits in order:

1. `04d658727 Remove anonymous principal variant`
2. `e3bfa60ac Make run provenance and creators total`

Implemented commit 2 end-to-end:
- Made `Run.created_by`, `RunSpec.provenance`, `RunProvenance.subject`, and run-created provenance total/non-optional.
- Updated create/retry/server/demo/projection/event conversion flows.
- Removed projection fallback creator behavior.
- Updated OpenAPI and regenerated generated TS API models.
- Added gated `fabro_types::test_support`.
- Updated Rust/TS fixtures, snapshots, and tests.
- Confirmed no pending snapshot artifacts and working tree is clean.

Verification passed:
- `cargo +nightly-2026-04-14 fmt --check --all`
- `cargo +nightly-2026-04-14 clippy --workspace --all-targets -- -D warnings`
- `cargo build --workspace`
- `cargo nextest run --workspace --no-fail-fast`
- `cargo nextest run -p fabro-server`
- `cargo nextest run -p fabro-cli cmd::inspect::inspect_resolves_selector_via_server_endpoint --no-capture`
- `cd lib/packages/fabro-api-client && bun run typecheck`
- `cd apps/fabro-web && bun run typecheck && bun test && bun run build`
- `git diff --check`
- final invariant sweep for removed anonymous/nullable provenance patterns; remaining `subject` hits were unrelated todo/web-auth fields.

Manual browser/UI E2E was not run in this terminal session.