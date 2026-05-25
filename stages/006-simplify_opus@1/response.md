All clean. Summary of fixes applied:

**Fixed (HIGH/MEDIUM priority issues from review):**

1. **Removed duplicate `normalize_optional_text`** — Deleted the unused helper in `lib/crates/fabro-client/src/client.rs` and simplified `deny_run` to pass `reason` directly. The validator in `interact.rs` already normalizes for the MCP path; the server handles blank values for the CLI path.

2. **Encapsulated user-only policy on the enum** — Added `ValidatedInteractAction::requires_user()` in `lib/crates/fabro-tool/src/interact.rs` and replaced the leaky `matches!` peek in `lib/crates/fabro-workflow/src/handler/llm/api.rs`.

3. **Aligned `approval.rs` style with `archive.rs`** — Dropped the redundant `json_key()` and `failure_message()` wrappers; renamed `verb()` → `past()`; switched `bail!(action.failure_message())` to `bail!("some runs could not be {}", action.past())` matching archive.rs.

4. **Deduplicated test helpers** — Moved `ulid_filter()` and `conflict_error_body()` into `lib/crates/fabro-cli/tests/it/cmd/support.rs`. Updated `approve.rs`, `deny.rs`, `archive.rs`, and `unarchive.rs` to import them, eliminating 4 duplicate copies.

**Skipped (out of scope or low-value):**

- Extracting a generic `run_bulk` shared between `archive.rs` and `approval.rs` — would require refactoring existing archive code; the two files now follow the same idiomatic pattern, which is the simpler win.
- Centralizing the 3 mock `FabroToolBackend` impls behind a `test_support::PanicBackend` — broader cross-crate refactor flagged by the reuse agent, not scoped to this change.
- Parallelizing the batch CLI loop — flagged LOW only because it would diverge from `archive.rs`.

**Verification:** `cargo check --workspace`, `cargo +nightly-2026-04-14 clippy --workspace --all-targets -- -D warnings`, `cargo +nightly-2026-04-14 fmt --check --all`, and `cargo nextest run -p fabro-tool -p fabro-client -p fabro-server -p fabro-workflow` (1893 tests) all pass. The new `approve`/`deny` CLI tests and existing `archive`/`unarchive` tests (36 tests) all pass.