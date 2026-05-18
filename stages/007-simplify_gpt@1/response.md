Completed the cleanup review and fixed a board-contract mismatch.

What I fixed:
- Removed stale `queued` and `archived` board columns from:
  - OpenAPI `BoardColumn`
  - Rust server board projection/demo responses
  - TypeScript generated `BoardColumn` model
  - Web board column/status mappings
- Updated board mapping so:
  - `submitted | queued | starting` → `initializing`
  - archived/removing runs are off-board
- Removed the board “Show archived” query/state path and duplicate SWR invalidations for archived board keys.
- Updated generated TS `listBoardRuns` client signature to no longer accept `includeArchived`.
- Updated server/web tests for the five-column board contract.

Validation run:
- `cargo check -q -p fabro-server`
- `cargo nextest run -p fabro-server boards_runs --no-fail-fast`
- `cd apps/fabro-web && bun run typecheck`
- `cd apps/fabro-web && bun test app/data/runs.test.ts app/routes/runs.test.tsx app/lib/board-events.test.tsx`

All passed.