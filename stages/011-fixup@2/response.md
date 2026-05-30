All verification stages pass individually:

- `cargo +nightly-2026-04-14 fmt --check --all` ✓
- Forbidden patterns check ✓
- `cargo +nightly-2026-04-14 clippy --workspace --all-targets -- -D warnings` ✓
- `cargo nextest run --workspace --profile ci` ✓ (6494 passed, 181 skipped)
- `cargo dev docs check` ✓
- `bun install --frozen-lockfile` ✓
- `apps/fabro-web` typecheck & test ✓ (508 tests pass)
- `lib/packages/fabro-api-client` typecheck ✓
- `cargo dev build -- -p fabro-cli --release` ✓

The previous verify failure was a 10-minute timeout on the combined script — not a code issue. Running steps individually confirms everything passes. The slowest step is the release build (~2m 23s), and combined with the workspace nextest run, docs refresh, and other steps in the single verify script, the wall-clock time exceeded 600s.

No code changes were needed; the working tree is clean and all checks pass.