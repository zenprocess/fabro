Implemented a small cleanup from the simplify review.

Changed:
- `docs/public/api-reference/fabro-api.yaml`
  - Adjusted `AgentMessageProps.message` to use `oneOf: object | null` instead of `type: ["object", "null"]`, matching the style that generates correct nullable TypeScript.
- `lib/packages/fabro-api-client/src/models/agent-message-props.ts`
  - Regenerated/updated `message?: { ... } | null`.

Validation run:
- `cargo build -p fabro-api`
- `cargo nextest run -p fabro-types agent_message`
- `cargo nextest run -p fabro-server get_run_stage_context_window`
- `cd lib/packages/fabro-api-client && bun run generate`
- `cd lib/packages/fabro-api-client && bun run typecheck`
- `cd apps/fabro-web && bun test app/lib/run-events.test.tsx app/lib/query-keys.test.ts`
- Final search:
  - `agent.context_window.snapshot`
  - `AgentContextWindowSnapshot`
  - `ContextWindowSnapshot`
  - No matches in implementation/API/frontend paths.

No additional code reuse/quality/efficiency issues were found worth changing.