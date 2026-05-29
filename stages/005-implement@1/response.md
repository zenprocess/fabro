Implemented the automation master `enabled` gate removal end-to-end.

Summary:
- Removed top-level `enabled` from Rust automation domain/persistence/request types.
- Kept trigger-level `enabled` as the only activation control.
- Updated `enabled_api_trigger()` to only check for an enabled API trigger.
- Added the no-compatibility TOML test rejecting legacy top-level `enabled`.
- Updated server conflict detail to `automation has no enabled API trigger`.
- Removed top-level `enabled` from server fixtures/tests and deleted the disabled-automation master-gate test.
- Updated OpenAPI schemas and regenerated the TypeScript Axios client.
- Removed the web UI master toggle and all create/replace payload usage.
- Updated web run availability to derive from the API trigger only.
- Used a subagent audit and a red/green cycle: the new legacy-TOML rejection test failed before implementation and passed after the model change.

Verification run:
- `cargo nextest run -p fabro-automation` ✅
- `cargo build -p fabro-api` ✅
- `cargo nextest run -p fabro-api --test automation_round_trip` ✅
- `cargo nextest run -p fabro-server --features test-support 'api::automations'` ✅
- `cd lib/packages/fabro-api-client && bun run generate` ✅
- `cd lib/packages/fabro-api-client && bun run typecheck` ✅
- `cd apps/fabro-web && bun run typecheck` ✅
- `cargo +nightly-2026-04-14 fmt --check --all` ✅
- `git diff --check` ✅

Reference scan:
- No removed top-level `automation.enabled` / master-gate references remain.
- Remaining `enabled` hits are trigger-level fields/usages or the intended `enabled_api_trigger()` call.

Environment note:
- Installed `default-jre-headless` in the container because OpenAPI Generator required `java` to regenerate the TypeScript client.