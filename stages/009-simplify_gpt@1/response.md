Cleanup complete.

Fixed:
- `lib/crates/fabro-server/src/automation_materializer.rs`
  - Corrected the materializer workflow path test to use `flows/deps.fabro`.
  - The previous extensionless `flows/deps` was treated by `WorkflowLocation::resolve` as a workflow selector/literal and failed with `workflow not found`.

Validated:
- `cargo nextest run -p fabro-automation` ✅
- `cargo nextest run -p fabro-server 'automation_'` ✅
- `cargo nextest run -p fabro-api automation` ✅
- `cargo +nightly-2026-04-14 fmt --check --all` ✅
- `cargo +nightly-2026-04-14 clippy -q -p fabro-server --all-targets -- -D warnings` ✅

Working tree now has one cleanup change:
- `M lib/crates/fabro-server/src/automation_materializer.rs`