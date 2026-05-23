Implemented the mid-stage agent interview tools and metadata plumbing.

Summary of changes:
- Extended canonical `InterviewOption` with optional `description` and `preview`.
- Updated OpenAPI and regenerated the TypeScript API client:
  - `ApiQuestion.options` now uses `InterviewOption`.
  - Removed stale `ApiQuestionOption` export/file.
- Added model-native agent question tools:
  - OpenAI: `request_user_input`
  - Anthropic: `AskUserQuestion`
  - Gemini: no question tool
- Added per-turn `AgentToolRuntime` so cached sessions receive current workflow/stage interview context dynamically.
- Enforced question-tool round rules:
  - question tool runs alone
  - mixed tool peers get model-visible errors
  - multiple separate question-tool calls execute only the first and tell the model to batch
- Added shared run-level interview blocker/guard used by both human gates and agent questions.
- Added workflow runtime for agent-originated batched questions:
  - emits/registers all questions first
  - blocks once per batch
  - waits concurrently
  - emits completion/interruption/timeout events per question
  - cleans up pending questions on cancellation/drop
- Preserved answer endpoints/paths through existing pending interview projection and submission flow.
- Updated web/Slack display:
  - web shows option descriptions
  - Slack renders option descriptions where practical
  - preview is captured/exposed but not specially rendered
- Added/updated tests across agent, workflow, API, store, Slack, and web.

Validation run:
- `cargo check -q -p fabro-agent -p fabro-workflow -p fabro-api -p fabro-server -p fabro-slack -p fabro-store -p fabro-cli --tests` ✅
- `cargo +nightly-2026-04-14 fmt --check --all` ✅
- `cargo +nightly-2026-04-14 clippy -q -p fabro-agent -p fabro-workflow -p fabro-api -p fabro-server -p fabro-slack -p fabro-store -p fabro-cli --all-targets -- -D warnings` ✅
- `cargo nextest run -p fabro-agent -p fabro-workflow -p fabro-api ...focused question/API tests...` ✅
- `cargo nextest run -p fabro-interview` ✅
- `cargo nextest run -p fabro-slack -p fabro-store --lib --tests` ✅
- `cd apps/fabro-web && bun test ./app/components/interview-dock.test.tsx ./app/components/stage-renderers/helpers.test.ts` ✅
- `cd apps/fabro-web && bun run typecheck` ✅
- `cd lib/packages/fabro-api-client && bun run typecheck` ✅

Noted failures:
- `cargo nextest run -p fabro-server ...` fails on `server::tests::get_graph_returns_svg`; rerun of the single test also fails. The error is the graph renderer subprocess returning nextest output (`running 0 tests`) instead of SVG, unrelated to the interview changes.
- Full `cd apps/fabro-web && bun test` still has existing unrelated failures in run-detail/lifecycle tests; focused changed web tests pass and typecheck passes.