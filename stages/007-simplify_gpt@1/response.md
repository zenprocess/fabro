Completed the simplify/code-review cleanup.

Changes made:
- `lib/crates/fabro-server/src/server/automation_scheduler.rs`
  - Added direct shutdown-token handling to the automation scheduler loop so it exits promptly on cancellation, not only via notify/shutdown flag.
  - Avoided repeated linear automation lookup for multiple due triggers by building an automation ID map once per due batch.

Validation passed:
- `cargo nextest run -p fabro-server automation_scheduler`
- `cargo nextest run -p fabro-automation`
- `cargo nextest run -p fabro-server automation_materializer`
- `cargo nextest run -p fabro-server automation_metadata`
- `cargo nextest run -p fabro-server --features test-support api::automations`
- `cargo +nightly-2026-04-14 fmt --check --all`
- `cargo +nightly-2026-04-14 clippy --workspace --all-targets -- -D warnings`

Working tree has one modified file:
- `lib/crates/fabro-server/src/server/automation_scheduler.rs`