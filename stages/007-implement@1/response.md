Implemented the root-stage TODO projection fix in `lib/crates/fabro-store/src/run_state.rs`.

What changed:
- Added `should_project_stage_todo_event(...)`.
- The reducer now ignores `TodoCreated`, `TodoUpdated`, and `TodoDeleted` events when:
  - `list_kind == TodoListKind::OpenAiPlan`
  - and `parent_session_id.is_some()`
- Anthropic task events remain projectable even from child-session envelopes.
- No API, schema, event serialization, or frontend changes.

Added focused regression coverage:
- Root OpenAI plan remains projected after child OpenAI plan events.
- Child OpenAI plan events alone do not create `stage.todos`.
- Anthropic child-session task events still project and update.

TDD verification:
- Red: `cargo nextest run -p fabro-store todo_reducer` failed as expected before the fix.
- Green:
  - `cargo nextest run -p fabro-store todo_reducer` ✅
  - `cargo +nightly-2026-04-14 fmt --check --all` ✅

Only changed file:
- `lib/crates/fabro-store/src/run_state.rs`