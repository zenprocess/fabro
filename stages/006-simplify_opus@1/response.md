## Summary

The previous `implement` stage failed with `budget_exhausted` and committed only the tests, leaving the actual reducer fix missing. I completed the implementation:

**Changes to `lib/crates/fabro-store/src/run_state.rs`:**
1. Added `TodoListKind` to the `fabro_types` imports.
2. Added the `should_project_stage_todo_event(stored: &RunEvent, list_kind: TodoListKind) -> bool` helper (returns `false` only for OpenAI plan events with a non-`None` `parent_session_id`).
3. Wired the helper as an early `return Ok(())` in the `EventBody::TodoCreated`, `TodoUpdated`, and `TodoDeleted` arms — before the existing `stage_at_stored_or_current_visit` lookup so the cheap filter gates the expensive one.

**Verification:**
- `cargo nextest run -p fabro-store todo_reducer` → 7 passed (incl. the 3 new regression tests).
- `cargo nextest run -p fabro-store` → 156 passed.
- `cargo +nightly-2026-04-14 fmt --check --all` → clean (re-formatted after edit).
- `cargo +nightly-2026-04-14 clippy -p fabro-store --all-targets -- -D warnings` → clean.

**Code review (Phase 2):**
- Reuse: No existing root-vs-child helper exists; the inline `parent_session_id.is_some()` check matches the convention used in `fabro-agent`.
- Quality: Three-arm early-return is intentional per spec (all three arms must be gated). One-liner helper centralizes the rule.
- Efficiency: Cheap-before-expensive ordering correct, `TodoListKind: Copy`, zero allocations, no new clones/lookups.

No follow-up fixes were required from the reviews.