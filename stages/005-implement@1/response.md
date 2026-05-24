Implemented the plan end-to-end.

Summary:
- Removed standalone `agent.context_window.snapshot` event from Rust event contracts, workflow conversion, event names, store reducer, frontend invalidation, OpenAPI/API client surface.
- Added optional `context_window` to `agent.message`:
  - `fabro-types::AgentMessageProps`
  - `fabro-agent::AgentEvent::AssistantMessage`
  - workflow conversion into `EventBody::AgentMessage`
- Updated `Session::run_single_input` so context-window projection is computed locally from the exact request and attached only after a successful assistant response.
- Removed normal-execution provider token-count API calls from `fabro-agent::Session`.
- Updated store projection to persist latest `AgentMessageProps.context_window` and stamp `event_seq`.
- Kept the context-window GET endpoint projection-backed and updated endpoint tests to seed data via `agent.message`.
- Regenerated the TS API client and added generated `AgentMessageProps`.
- Removed frontend snapshot event handling/tests; `agent.message` continues to invalidate stage context-window queries via stage activity handling.
- Used red/green TDD: first added a failing `fabro-agent` test proving standalone snapshot events were still emitted, then implemented until green.
- Used subagent-driven development for read-only investigation of Rust/API/frontend surfaces.

Validation run:
- `cargo build -p fabro-api`
- `cargo nextest run -p fabro-types agent_message`
- `cargo nextest run -p fabro-agent`
- `cargo nextest run -p fabro-workflow`
- `cargo nextest run -p fabro-store`
- `cargo nextest run -p fabro-server get_run_stage_context_window`
- `cargo check -q --workspace`
- `cargo +nightly-2026-04-14 fmt --check --all`
- `cargo +nightly-2026-04-14 clippy -q --workspace --all-targets -- -D warnings`
- `cd lib/packages/fabro-api-client && bun run generate`
- `cd lib/packages/fabro-api-client && bun run typecheck`
- `cd apps/fabro-web && bun run typecheck`
- `cd apps/fabro-web && bun run test`
- Final search: `agent\.context_window\.snapshot|AgentContextWindowSnapshot|ContextWindowSnapshot` — no matches.

Note:
- I pinned the API client `generate` script to `@openapitools/openapi-generator-cli@2.20.2` because current `bunx` was resolving the unpinned command to an incompatible latest wrapper in this environment.