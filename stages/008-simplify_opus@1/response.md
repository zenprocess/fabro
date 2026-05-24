All tests pass, fmt and clippy clean.

## Summary

The diff was already small and well-scoped. Three review agents found no must-fix issues; the code matched surrounding patterns in the file.

I made two focused quality improvements:

1. **`should_project_stage_todo_event` clarity** (`run_state.rs`): Rewrote the predicate in the positive form (`is_child_openai_plan_event`) and added a doc comment explaining why OpenAI plan events from child sessions are filtered while Anthropic task events always project. The De Morgan inversion (`!matches!(...) || ...`) was hard to read without context.

2. **Tightened test assertion**: Replaced `state.stage(&stage_id).is_none_or(|stage| stage.todos.is_none())` with an explicit `state.stage(&stage_id).expect("stage present")` followed by `assert!(stage.todos.is_none())`, so a future regression that drops the stage entirely can't silently pass this test.

Findings I intentionally skipped:
- **Consolidating the three near-identical `TodoCreated/Updated/Deleted` arms**: every other reducer arm in this file uses the same inline `let Some(stage) = ... else { return Ok(()); };` pattern, so introducing a helper just for these three would diverge from the file's style.
- **Promoting `is_root_session` to `RunEvent` or `is_session_scoped` to `TodoListKind`**: one call site each — premature.
- **`child_stage_event` session_id stability and persistence-time filtering**: orthogonal to this fix's scope.

Verified with `cargo nextest run -p fabro-store todo_reducer` (7 passed), `cargo +nightly-2026-04-14 fmt --check --all`, and `clippy -p fabro-store --all-targets -D warnings`.