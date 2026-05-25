Reviewed the branch changes vs. the merge-base and ran the requested parallel review-agent pass. The agent results weren’t exposed by the tool output, so I also did a direct cleanup review.

Fixed one small reuse/quality issue:

- `lib/crates/fabro-sandbox/src/managed_labels.rs`
  - Replaced the duplicated `"true"` literal with the existing `MANAGED_LABEL_VALUE` constant.
  - Added a test assertion that exercises `is_managed()`, which also avoids dead-code warnings in the no-provider-feature test build.

Validation passed:

```bash
cargo +nightly-2026-04-14 fmt --check --all
cargo test -p fabro-types sandbox_inventory
cargo test -p fabro-api sandbox_inventory_round_trip
cargo test -p fabro-sandbox provider::tests --lib
cargo test -p fabro-server sandboxes --lib
cargo +nightly-2026-04-14 clippy -q -p fabro-sandbox --all-targets -- -D warnings
```

Working tree has one intentional cleanup modification:

```text
M lib/crates/fabro-sandbox/src/managed_labels.rs
```