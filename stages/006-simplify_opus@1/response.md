All changes complete. Here's a summary of what was fixed.

## Summary

Three review agents identified findings across reuse, quality, and efficiency. I applied the highest-impact, clearly-correct fixes and skipped the ones that turned out to be wrong on closer inspection (e.g. cache-token math; the reviewer was mistaken — input_tokens excludes cache tokens for all three providers, so the original addition is correct).

### Fixes applied

1. **Web invalidation scope** (`apps/fabro-web/app/lib/run-events.ts` + tests): the context-window endpoint's projection only changes on `agent.context_window.snapshot` events, so `stageContextWindow` invalidation was removed from `STAGE_EVENTS`, `STEERING_EVENTS`, `STAGE_ACTIVITY_EVENTS`, and `TODO_EVENTS` branches. Saves an HTTP refetch on every agent activity event.

2. **Reuse `StageState::is_terminal()`** (`lib/crates/fabro-server/src/server/handler/runs.rs`): replaced the local `is_live_stage` helper with `stage.state.is_terminal()` and dropped the unused `StageState` import.

3. **Consolidate breakdown item type** (`lib/crates/fabro-types/src/run_projection.rs` + OpenAPI + generated TS): dropped the `StageContextWindowBreakdownItem` type alias by renaming `StageContextWindowBreakdownProjection` to `StageContextWindowBreakdownItem`. Per CLAUDE.md API DTO guidance.

4. **Dropped redundant `label` and `source` fields** from breakdown items (OpenAPI, Rust projection, generated TS, agent estimator, all test fixtures). `label` was UI text in the API; `source` duplicated the outer `count_method`.

5. **Simplified `scale_breakdown`** (`lib/crates/fabro-agent/src/context_window.rs`): replaced ~60-line largest-remainder algorithm with a simple proportional scale where the last bucket absorbs the rounding leftover. Acceptable for a best-effort estimate.

6. **Consolidated `ToolRegistry` policy filters** (`lib/crates/fabro-agent/src/tool_registry.rs`): `definitions_for_policy` now delegates to `definitions_with_source_for_policy`, removing the duplicated filter logic.

### Validation

- `cargo +nightly-2026-04-14 clippy --workspace --all-targets -- -D warnings`: clean
- `cargo +nightly-2026-04-14 fmt --check --all`: clean
- `cargo nextest run -p fabro-agent -p fabro-api -p fabro-store -p fabro-server -p fabro-types`: 1674 passed, 3 failed (all pre-existing graphviz subprocess failures unrelated to this change, confirmed by running the same tests on `origin/main`).
- `apps/fabro-web bun test app/lib/query-keys.test.ts app/lib/run-events.test.tsx`: 21/21 pass
- `apps/fabro-web bun run typecheck`: clean

### Findings deliberately skipped

- **Drop spawned provider-API count**: per Decision 2 of the original plan, provider counting at request assembly is explicitly required. Removing it would deviate from the spec.
- **Cache-token math (`input + cache_read + cache_write`)**: confirmed correct by reading each provider's adapter — OpenAI/Gemini explicitly `saturating_sub(cached_tokens)` from `input_tokens`, and Anthropic's `input_tokens` is documented as uncached. Sum is the right total context size.
- **Per-message token cache (O(N²) concern)**: real but the estimator already short-circuits zero-token additions and only runs once per agent turn; deferring this since it would add cache invalidation complexity to a best-effort feature.
- **Bigger DTO/projection unification (`StageContextWindow` flattens projection)**: legitimate but a larger refactor that risks the response shape; left in place since current shape is already covered by tests and the build.rs round-trip.
- **Several lower-severity findings** (e.g. `BuiltRequest` wrapper, `close_token` field, `request_fingerprint` smell) are noise compared to the size of the diff; not worth the churn in a greenfield-priority pass that didn't elect to drop the provider count path itself.