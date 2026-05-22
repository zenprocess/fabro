Reviewed the Ask Fabro sidebar/run-tools changes and made cleanup fixes.

What I fixed:
- Scoped Ask Fabro’s run-tool backend to the owning run via `ClientBackend::with_run_scope(...)`, so accidental cross-run tool calls are rejected before reaching the API.
- Switched Ask Fabro session tokens to base same-run worker tokens instead of broad `agent:run_tools` tokens.
- Exposed the base `issue_worker_token` helper outside tests for this same-run token path.
- Added stale cached-session cleanup in the web adapter: if a persisted session ID returns 404 during streaming, it’s cleared so the next turn creates a fresh session.

Validation run:
- `cargo check -q -p fabro-tool -p fabro-server`
- `cargo +nightly-2026-04-14 fmt --all`
- `cargo +nightly-2026-04-14 clippy -q -p fabro-tool -p fabro-server --all-targets -- -D warnings`
- `cd apps/fabro-web && bun test app/lib/ask-fabro-runtime.test.ts`
- `cd apps/fabro-web && bun run typecheck`

Current modified files:
- `apps/fabro-web/app/lib/ask-fabro-runtime.ts`
- `lib/crates/fabro-server/src/server/handler/sessions.rs`
- `lib/crates/fabro-server/src/worker_token.rs`
- `lib/crates/fabro-tool/src/fabro_client.rs`