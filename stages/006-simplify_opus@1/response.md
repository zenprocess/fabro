All targeted tests pass, fmt is clean, clippy is clean.

## Summary of Fixes Applied

Three reviewers ran in parallel. After aggregating findings, I applied these concrete fixes:

1. **Restored trailing newlines on 10 files** that lost them during the change (TypeScript routes, regenerated API client models, MDX docs, and `defaults.toml`):
   - `apps/fabro-web/app/routes/{run-detail.tsx, settings-resources.test.tsx, start.tsx}`
   - `docs/public/{administration/server-configuration.mdx, execution/run-configuration.mdx, reference/user-configuration.mdx}`
   - `lib/crates/fabro-config/src/defaults.toml`
   - `lib/packages/fabro-api-client/src/models/{ask-fabro.ts, server-settings.ts, system-info-response.ts}`

2. **Removed dead `_user_settings` binding** in `lib/crates/fabro-config/src/tests/resolve_server.rs::server_settings_from_layer_matches_namespace_resolvers` — its only consumer (`assert_eq!(context.features, user_settings.features)`) was deleted in the original change, so the discarded builder call was pure dead code.

3. **Dropped brittle `top_level.len() == 1` assertion** in `lib/crates/fabro-server/tests/it/api/settings.rs` — the explicit `body.get("features").is_none()`, `cli`, and `run` absence checks already express the intent and don't break when unrelated fields are added later.

4. Re-added trailing newlines on the two files above (`tests/resolve_server.rs`, `tests/it/api/settings.rs`) that `edit_file` stripped during step 2 and 3.

## Findings I Skipped (with reasons)

- **Rename `AskFabroReadiness` → `AskFabroDefaults`**: subjective naming nit; the struct still earns its keep by memoizing the default model across many run rows in `decorate_run_summaries`. Skipped.
- **Collapse `ALLOWED_TOP_LEVEL_KEYS` and `Display` impl**: pre-existing duplication, not introduced by this diff. Skipped per task scope.
- **Redundant `ml-auto` in `start.tsx`**: harmless; out of scope.
- **`ask_fabro_readiness` efficiency regression**: intentional behavior per the task spec ("behavior should be as if `session_sandboxes = true` was always set"). Any caching is a separate concern.
- **`features` absence assertion in `tests/it/api/system.rs`**: defensible regression guard against accidental reintroduction.

Verification: `cargo nextest run` passes the affected tests, `cargo +nightly-2026-04-14 fmt --check --all` is clean, `cargo +nightly-2026-04-14 clippy -p fabro-config -p fabro-server --features fabro-server/test-support --all-targets -- -D warnings` passes.