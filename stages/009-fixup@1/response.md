No matches for forbidden patterns. All verify steps now succeed:

- `cargo +nightly-2026-04-14 fmt --check --all` ✅
- `cargo dev docs refresh` ✅ (no changes)
- Forbidden pattern check ✅
- `cargo clippy --workspace --all-targets -- -D warnings` ✅
- `cargo nextest run --workspace --profile ci` ✅ — 6372 passed, 179 skipped
- `cargo dev docs check` ✅
- `bun install --frozen-lockfile` ✅
- `apps/fabro-web` typecheck + test ✅ (505 pass)
- `lib/packages/fabro-api-client` typecheck ✅
- `cargo dev build -- -p fabro-cli --release` ✅

The previous failure was a transient infrastructure issue ("failed to spawn script") rather than an actual code problem. No code changes were required.