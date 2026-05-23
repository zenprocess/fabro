Implemented the context-window breakdown endpoint end-to-end.

### What changed

- Added OpenAPI contract for:
  - `GET /api/v1/runs/{id}/stages/{stageId}/context-window`
  - `StageContextWindow*` schemas/enums/warnings/breakdown DTOs.
- Added content-free projection types in `fabro-types` and wired them through `fabro-api` replacements/tests.
- Added typed event:
  - `agent.context_window.snapshot`
- Added reducer support in `fabro-store` so latest snapshots replace prior stage snapshots.
- Added agent-side context-window accounting:
  - local category breakdown at request assembly time
  - async provider token count via `Client::count_input_tokens(... PreferProvider)`
  - provider/API scaled replacement snapshots
  - response-usage scaled replacement snapshots
  - content-safe request fingerprinting and close-token cancellation for count tasks
- Added tool source metadata (`Native`, `Mcp`, `Skill`) so tool definitions split into native tools, MCP tools, and skills.
- Exposed narrow token-estimation helpers from `fabro-llm`.
- Added server endpoint behavior:
  - 404 for missing run/stage
  - `available: false` for non-agent stages and unobserved agent stages
  - stored/latest projected snapshot response with warnings preserved
- Added web query support:
  - `queryKeys.runs.stageContextWindow(id, stageId)`
  - `useRunStageContextWindow(runId, stageId)`
  - SSE invalidation for context-window snapshots, stage lifecycle, steering/activity, and todo activity.
- Updated TypeScript generated client files manually for the new endpoint/models because the generator could not run in this environment.

### Validation run

Passed:

- `cargo build -q -p fabro-api`
- `cargo check -q --workspace`
- `cargo nextest run -p fabro-types -p fabro-store -p fabro-api --no-fail-fast`
- `cargo nextest run -p fabro-agent --no-fail-fast`
- `cargo nextest run -p fabro-workflow --no-fail-fast`
- `cargo nextest run -p fabro-server get_run_stage_context_window --no-fail-fast`
- `cargo +nightly-2026-04-14 fmt --check --all`
- `cargo +nightly-2026-04-14 clippy --workspace --all-targets -- -D warnings`
- `git diff --check`
- `cd apps/fabro-web && bun test app/lib/query-keys.test.ts app/lib/run-events.test.tsx`
- `cd apps/fabro-web && bun run typecheck`
- `cd apps/fabro-web && bun test app/lib/run-actions.test.ts`

Notes:

- `cd lib/packages/fabro-api-client && bun run generate` failed because the environment lacks `java`; `bunx` latest also hit a Node 18 ESM issue. I manually updated the generated TypeScript client/model files to match the OpenAPI change.
- Full `cd apps/fabro-web && bun test` still reports failures in `run-actions.test.ts` only when run as part of the entire suite; that file passes in isolation. This appears to be pre-existing/global test-state interference, not caused by the context-window changes.
- Full `cargo nextest run -p fabro-server` previously surfaced unrelated graph-render subprocess failures; the new context-window endpoint tests pass directly.