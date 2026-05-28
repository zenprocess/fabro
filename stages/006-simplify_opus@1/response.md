Web UI and CLI remain untouched.

## Summary

Reviewed 3 changed files via parallel reuse/quality/efficiency agents and applied the following cleanups:

**Removed dead/redundant code:**
- Deleted `require_run_tool_actor` helper in `principal_middleware.rs` — was a trivial alias for `require_run_management_actor`; `RequiredRunToolActor::from_request_parts` now calls the real function directly.
- Removed stale `#[allow(dead_code, reason = "...")]` on `AppState::materialize_automation_run` in `server.rs` (now called from `create_automation_run`).

**Inlined single-use helpers in `automations.rs`:**
- `automation_api_trigger_disabled_error` + the matching const `AUTOMATION_API_TRIGGER_DISABLED_CODE` — only used by one `else` branch.
- `automation_materialization_temp_root` — one-line `Storage::new(...).scratch_dir().join("automations")`.
- `automation_materialize_error` — one-line wrapper around `ApiError::new(UNPROCESSABLE_ENTITY, …)`.

**Moved logic to its proper home:**
- Promoted `enabled_api_trigger` from a private function in the HTTP handler to `Automation::enabled_api_trigger()` in the `fabro-automation` crate, where future trigger consumers (scheduler) can reuse it.

**Quality tweaks:**
- `chrono::Utc::now()` → `Utc::now()` via `use chrono::Utc;` to match the idiom in `runs.rs`.
- Paginate before decorate in `list_automation_runs` so `decorate_run_summaries` only runs over the returned page instead of all filtered runs.

**Considered and intentionally skipped:**
- Adding `automation_id` to `fabro_store::ListRunsQuery` to push filtering into the store (would scale better but is explicitly out of scope — issue spec mandates in-memory filtering).
- Sharing a list-runs envelope/sort helper between `list_runs` and `list_automation_runs` (would require exposing `RunsSortKey`/`RunsSortDirection` and a generic helper; the 4-line inline sort is short enough).
- Collapsing `RequiredRunToolActor` into `RequiredRunManagementActor` (issue explicitly mandates the `RequiredRunToolActor` name).

**Verification:** `cargo check --workspace --all-targets`, `cargo nextest run -p fabro-automation -p fabro-api` (176/176 pass), `cargo nextest run -p fabro-server --test it --features test-support` (171/171 pass, including all 24 automation API tests), `cargo +nightly-2026-04-14 clippy --workspace --all-targets -- -D warnings` clean, `cargo +nightly-2026-04-14 fmt --check --all` clean. Net change: 37 inserts / 54 deletes across 4 files; `apps/fabro-web` and `lib/crates/fabro-cli` untouched.