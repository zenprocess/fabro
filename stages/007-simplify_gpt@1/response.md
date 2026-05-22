Review/cleanup complete.

Fixed:
- Removed obsolete public changelog guidance that still told users to configure `[features]`.
- Adjusted the config parser regression test so it still verifies the legacy `features` namespace is rejected, without reintroducing retired `session_sandboxes` / `[features]` grep matches.

Validation run:
- `cargo nextest run -p fabro-config` ✅
- `cargo build -p fabro-api` ✅
- `cargo +nightly-2026-04-14 fmt --check --all` ✅
- `cargo +nightly-2026-04-14 clippy -p fabro-config --all-targets -- -D warnings` ✅
- Acceptance greps for removed feature/API strings and `[features]` settings namespace ✅

Current working tree only has the two cleanup edits:
- `docs/public/changelog/2026-03-15.mdx`
- `lib/crates/fabro-config/src/parse.rs`