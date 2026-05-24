Compiles clean.

## Summary

I ran three parallel review agents over the diff. Aggregated findings:

**Code Reuse (Agent 1):**
- MED: `pair_test_target()` duplicated an existing inline `PairTarget` literal at `tests.rs:2205` — **fixed**.
- LOW: `test_user_principal()` could live in `test_support.rs`, recurring `(Method, path)` rejection loop could share a helper, hard-coded pair-id literal — all pre-existing patterns. Skipped.

**Code Quality (Agent 2):**
- Verified clean: no stale references to old names (`RequiredRunToolActor`, `RequireRunScopedOrRunTools`, etc.), tidy imports in `pair.rs`, no leftover `Principal::User(auth.0)` constructions.
- MED: Double Path extraction in `pair.rs` handlers (`RequireRunManagementTarget` + `Path<(String, String)>`) — pre-existing convention also used by `submit_answer`. Refactoring requires a new extractor variant; not introduced by this diff. Skipped.
- LOW: `Required…` vs `Require…` naming asymmetry is intentional and pre-existing. Skipped.

**Efficiency (Agent 3):**
- LOW: Same double Path extraction in pair/answer handlers — one extra serde deserialization per request on pair endpoints, cost is small. Skipped.
- Everything else (auth check shape, LazyLock, trait defaults) is equivalent to or cleaner than before.

**Fix applied:** Deduplicated the `PairTarget` literal at `tests.rs:2205` to call the new `pair_test_target()` helper. `cargo check -p fabro-server --tests` passes.

The remaining flagged items are pre-existing patterns rather than regressions introduced by this change.