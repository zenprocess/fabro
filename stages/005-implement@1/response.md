Implemented the scheduled automation executor.

Summary:
- Added `server/automation_scheduler.rs` with:
  - deterministic in-memory cron planner/cursors
  - scheduler loop with shutdown + notification wakeups
  - scheduled run firing through existing automation materializer + run creation + start queue path
  - unit/executor tests for planner behavior, schedule-only automations, disabled states, multiple triggers, queued prior runs, and failed materialization retry timing
- Wired scheduler into server startup next to the existing run scheduler.
- Added `AppState` automation scheduler `Notify` and notify calls after automation create/replace/delete.
- Exposed `handler::lifecycle::queue_run_start` within `crate::server`.
- Added direct `croner` dependency to `fabro-server`.

Verification passed:
- `cargo nextest run -p fabro-automation`
- `cargo nextest run -p fabro-server automation_scheduler`
- `cargo nextest run -p fabro-server automation_materializer`
- `cargo nextest run -p fabro-server automation_metadata`
- `cargo nextest run -p fabro-server --features test-support api::automations`
- `cargo +nightly-2026-04-14 fmt --check --all`
- `cargo +nightly-2026-04-14 clippy --workspace --all-targets -- -D warnings`