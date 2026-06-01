All verification steps pass. Summary:

**Root cause**: The verify script failed at `git merge origin/main` because the repository's git config only fetches the feature branch (`feat/settings-environments-crud`), not `main`. The plain `git fetch origin main` only updates `FETCH_HEAD`, leaving `origin/main` undefined.

**Fixes applied**:
1. Fetched main explicitly with `git fetch origin main:refs/remotes/origin/main` and merged it cleanly.
2. The merge revealed a compile failure in `lib/crates/fabro-workflow/src/test_support.rs`: the local `simplify_opus` version imported `fabro_types::test_support::test_run_provenance_with`, but that module is feature-gated and `fabro-workflow`'s `src/test_support.rs` is compiled unconditionally during library builds. Replaced the helper call with the inline `RunProvenance { server: None, client: None, subject: ... }` literal (matching the version on `origin/main`), removing the offending cross-crate `test_support` import.

**Verification results** (all green):
- `cargo +nightly-2026-04-14 fmt --check --all` ✓
- forbidden-symbol sweep ✓
- `cargo +nightly-2026-04-14 clippy --workspace --all-targets -- -D warnings` ✓
- `cargo nextest run --workspace --profile ci`: 6494 passed, 181 skipped ✓
- `cargo dev docs check` ✓
- `bun install --frozen-lockfile` ✓
- `apps/fabro-web` typecheck ✓ and 508 tests passed ✓
- `lib/packages/fabro-api-client` typecheck ✓
- `cargo dev build -- -p fabro-cli --release` ✓