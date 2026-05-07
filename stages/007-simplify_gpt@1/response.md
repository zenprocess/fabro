Implemented cleanup fixes after reviewing the branch diff.

Notes:
- The subagent review tool failed repeatedly with a tool-side persistence error, so I completed the review manually.
- Fixed two regressions introduced by the prior simplification:
  - Restored the hand-rolled `Combine` impl for `RunIntegrationsGithubLayer` so `permissions = {}` remains an explicit clear sentinel.
  - Removed the broad `Option<HashMap<String, InterpString>>` `Combine` impl to avoid changing global merge semantics.
  - Removed unintended `fabro-api` `with_replacement` entries for run integrations, matching the requested OpenAPI-generated DTO design.
  - Restored `lib/crates/fabro-api/tests/run_integrations_round_trip.rs` JSON parity coverage.

Kept the valid reuse cleanup:
- `fabro-cli` / `fabro-server` now use `SandboxProvider::is_clone_based()` instead of duplicated string matching.

Validation run:
- `cargo test -p fabro-config run_integrations_github_permissions --lib`
- `cargo test -p fabro-api --test run_integrations_round_trip`
- `cargo check -p fabro-cli -p fabro-server`
- `cargo test -p fabro-cli --lib`
- `cargo test -p fabro-server run_integrations --lib`
- `cargo +nightly-2026-04-14 fmt --check --all`
- `cargo +nightly-2026-04-14 clippy -q -p fabro-config -p fabro-api -p fabro-cli -p fabro-server --all-targets -- -D warnings`

All passed.