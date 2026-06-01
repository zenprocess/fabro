## Summary

Reviewed the PR diff (vs origin/main) with three parallel agents covering reuse, quality, and efficiency. The PR is largely clean. Fixed the genuine issues:

**High-priority regressions (Agent 3):**
1. **`principal_middleware.rs::log_snapshot`** — re-introduced an avatar-stripping helper so HTTP middleware doesn't allocate a `String` for `avatar_url` on every authenticated request. The previous code (deleted in this PR) had this optimization; it's now restored via a small `principal_for_log` helper.
2. **`run-table-row.tsx`** — gated `principalDisplay(run.createdBy)` behind `show("created_by")`. Previously the call ran on every row regardless of column visibility, causing wasted React element construction.

**Quality cleanup (Agent 2 #3/#4, Agent 1 #1):**
3. Added `test_run_provenance_with(subject)` helper in `fabro-types::test_support` so callers don't have to hand-build the full `RunProvenance` struct just to override the subject.
4. Replaced the inline `RunProvenance { server: None, client: None, subject: Principal::System {…} }` literal in `fabro-workflow::test_support` with `test_run_provenance_with(Principal::System {…})`, eliminating the silent drift risk.

**Findings reviewed and skipped (false positives or out of scope):**
- Cargo version downgrade & `environment-form.tsx` (Agent 2 #12/#13): rebase artifacts — the branch base is older than current `origin/main`, both will resolve at merge.
- Hand-written JSON in `inspect.rs` (Agent 1 #5): false positive — those are insta inline snapshot string literals (expected stdout), not constructed `json!()` values.
- Local `test_user_principal()` in `principal_middleware.rs` (Agent 1 #3): intentionally GitHub/octocat-shaped to match JWT fixtures elsewhere in the same file.
- Infallible `IdpIdentity` constructor, `Principal::system/worker/engine` constructors, `RunSpec::test_default` builder (Agent 2 #1/#2/#9): out of scope — broader API/refactor work.

**Verification:**
- `cargo +nightly-2026-04-14 fmt --check --all` ✓
- `cargo +nightly-2026-04-14 clippy --workspace --all-targets -- -D warnings` ✓
- `cargo nextest run -p fabro-server -p fabro-types -p fabro-workflow` ✓ (3 pre-existing SVG-render env failures unrelated to this PR)
- `bun run typecheck && bun test` ✓ (508/508)