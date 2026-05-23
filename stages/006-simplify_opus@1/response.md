## Summary

Three reviewers ran in parallel; I applied three focused fixes from their findings and skipped lower-value or intentional-by-design suggestions.

### Fixes applied

1. **Dropped redundant `tool_count` field on `McpServerProjection`** (Q1 from quality review). It was cached derived state alongside `status: Ready { tools }`. Replaced with a `tool_count()` method that returns `tools.len()` for `Ready` and `0` for `Failed`. Updated:
   - `lib/crates/fabro-types/src/run_projection.rs`
   - `lib/crates/fabro-store/src/run_state.rs` (reducer + reducer test)
   - `docs/public/api-reference/fabro-api.yaml`
   - `lib/crates/fabro-api/tests/stage_projection_round_trip.rs`
   - Regenerated TS client (drops the field cleanly)

2. **Extracted `subagent_mut` helper** (A1 from reuse review, Q6 from quality review). Replaced triplicated `iter_mut().find(|s| s.agent_id == ...)` in the three `AgentSub{Completed,Failed,Closed}` reducer arms.

3. **Collapsed two-stage `should_clear` dance in `apply_todo_deleted`** (Q8 from quality review) into a straight-line early-return.

### Skipped findings (with rationale)

- **`SubAgentStatus::Completed { success: false }` overlaps `Failed`**: mirrors the source event-payload semantics (`AgentSubCompleted` vs `AgentSubFailed` are distinct events); not introduced here.
- **`AgentSubSpawned` push vs `AgentMcpReady` upsert inconsistency**: justified — subagent IDs are unique per spawn, MCP `ready` events can re-emit. Different semantics, both correct.
- **`SkillActivated` no-dedupe**: intentional per plan default (event-sourced replay append).
- **`SubAgentStatus::Failed { error: Value }` stringly-typed**: matches `AgentSubFailedProps::error: Value`; changing the event payload contract is out of scope.
- **Test builder for `apply_event(&test_stage_event(...))` boilerplate**: pre-existing pattern across the file; out of scope cleanup.
- **`assert_same_type` duplicated across `fabro-api/tests/*_round_trip.rs`**: pre-existing convention across ~40 files; this PR follows it.
- **`upsert_mcp_server` could `clone_from` the tools vec**: negligible — tool lists are tiny and updates rare.

### Verification

- `cargo build --workspace` — clean
- `cargo nextest run -p fabro-types -p fabro-store -p fabro-api` — 575 passed, 0 failed
- `bun run generate && bun run typecheck` — clean
- `cargo +nightly-2026-04-14 fmt --all` and `clippy --workspace --all-targets -- -D warnings` — clean