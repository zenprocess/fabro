Reviewed the branch diff and made two cleanup fixes:

- Prevented late async provider token-count results from overwriting a better response-usage context-window snapshot for the same request.
  - Added per-request fingerprint tracking for response-usage snapshots in `lib/crates/fabro-agent/src/session.rs`.
  - Provider count tasks now skip emitting if response usage has already produced the newer/better snapshot.

- Completed web SSE invalidation coverage for the new context-window query.
  - Stage lifecycle, steering/session, and stage activity events now invalidate `queryKeys.runs.stageContextWindow(runId, stageId)` in addition to stage events.
  - Updated tests in:
    - `apps/fabro-web/app/lib/query-keys.test.ts`
    - `apps/fabro-web/app/lib/run-events.test.tsx`

Validation run:

- `cargo check -q -p fabro-agent` ✅
- `cargo nextest run -p fabro-agent context_window` ✅
- `cargo check -q -p fabro-server` ✅
- `cd apps/fabro-web && bun test app/lib/run-events.test.tsx app/lib/query-keys.test.ts` ✅
- `cd apps/fabro-web && bun run typecheck` ✅
- `cargo +nightly-2026-04-14 fmt --check --all` ✅
- `git diff --check` ✅

One attempted focused clippy command failed on an existing unrelated `fabro-sandbox` `large_enum_variant` warning, not caused by these cleanup changes.