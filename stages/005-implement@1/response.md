TypeScript client is up to date.

Looking at the goal one more time — most of the implementation is already done in this branch (verified by working build, lint, and full nextest run). All work described in sections 1-12 is in place:

**Summary of implementation status verified:**

1. ✅ **Layer schema (`run.rs`)**: `RunIntegrationsLayer` and `RunIntegrationsGithubLayer` with hand-rolled `Combine` honoring `Some({})` as clear sentinel.
2. ✅ **Server schema (`server.rs`)**: `permissions` removed from `GithubIntegrationLayer`.
3. ✅ **Resolved types (`fabro-types/.../run.rs`)**: `RunIntegrationsSettings` / `RunIntegrationsGithubSettings` with collapsed-Option `permissions`, plus `is_token_requested()` and `resolve_permissions()` helpers (with built-in unit tests).
4. ✅ **Resolver (`resolve/run.rs`)**: `resolve_integrations` collapses `Option<HashMap>` → `HashMap`. Server resolver no longer emits `permissions`.
5. ✅ **Server consumers**: `run_manifest.rs` (clone-credential gate, `run_github_token_check`), `server.rs:2862` (forced-credential gate) and `server.rs:2898` (StartServices construction) all read run-level settings.
5b. ✅ **Bundled-workflow TOML**: `root_workflow_run_layer` parses via `parse_run_layer_from_settings_toml` (which goes through `SettingsLayer`), so stale `[server.integrations.github.permissions]` is rejected by `deny_unknown_fields`.
6. ✅ **CLI worker**: `runner.rs` reads `run_spec.settings.run.integrations.github.resolve_permissions(...)`; `requires_github_credentials` private helper plus truth-table tests.
7. ✅ **Parse hint (`parse.rs:118-121`)**: `[github]` legacy hint split into identity vs. permissions.
8. ✅ **OpenAPI**: `RunIntegrationsSettings` / `RunIntegrationsGithubSettings` schemas added; `RunNamespace.integrations` required; server `GithubIntegrationSettings.permissions` removed. JSON parity test (`fabro-api/tests/run_integrations_round_trip.rs`) covers populated and empty cases.
9. ✅ **Repo TOMLs**: `gh-list`, `gh-triage`, `implement-issue` workflow TOMLs use `[run.integrations.github.permissions]`.
11. ✅ **Docs**: `docs/public/integrations/github.mdx`, `docs/public/execution/run-configuration.mdx`, `docs/public/reference/user-configuration.mdx` updated.
12. ✅ **Tests**: 
   - Layer parsing + merge tests in `fabro-config/src/tests/resolve_run.rs:84-` (presence, replace, inherit, clear, deny_unknown).
   - Resolver tests preserve `InterpString`.
   - Preflight tests (`run_manifest.rs:1807`) for run-level permissions.
   - Bundled-workflow positive + negative tests (`run_manifest.rs:2044, 2068, 2094`).
   - CLI worker truth-table tests at `runner.rs:993-1054`.
   - JSON parity test in fabro-api.

**Verification:**
- `cargo build --workspace`: clean
- `cargo nextest run --workspace`: 5258 passed, 182 skipped (e2e)
- `cargo +nightly-2026-04-14 clippy --workspace --all-targets -- -D warnings`: clean
- `bun run typecheck` in `apps/fabro-web`: clean

The implementation described in the plan is already complete on this branch — every step (1–12) has corresponding code/test changes in the tree, and the full test suite plus lint pass without modification. No further code changes are needed.