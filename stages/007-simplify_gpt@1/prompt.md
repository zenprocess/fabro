Goal: # Ask Fabro Sidebar Wiring — Implementation Plan

> **For agentic workers:** Use superpowers:subagent-driven-development or superpowers:executing-plans. Steps use `- [ ]` checkboxes.

**Goal:** Ship the Ask Fabro sidebar on run pages — wired to real session APIs, with the agent able to inspect *and control* its owning run.

**Architecture:** Two phases. Phase 1 (Rust): give Ask Fabro agent sessions the full `fabro_run_interact` + `fabro_run_events` tools, scoped to the owning run, via the existing HTTP `FabroClient` backend. Phase 2 (web): replace the scripted sidebar adapter with real session calls; drop `?ask=1`, gate on `run.ask_fabro.available`.

**Tech Stack:** Rust (fabro-server, fabro-workflow, fabro-tool), React 19 + assistant-ui (fabro-web), generated API clients.

**Decisions locked:**
- Reuse `fabro-tool` — no new tool.
- Subset = `fabro_run_interact` + `fabro_run_events`, **full access** (incl. mutating actions: start/cancel/steer/archive/answer).
- Backend = existing HTTP `FabroClient` (already implements all reads + mutations).
- Scoped to owning run — enforced by a same-run worker token; the API 403s cross-run calls.
- File/shell tools stay read-only in Ask Fabro sessions (only the two run tools get full access).
- Gate the sidebar on `run.ask_fabro.available`; drop `?ask=1`.

---

## Background (current state)

- API endpoints exist (`296fbddec`): `SessionDetail`, `/sessions/{id}/events`, `/sessions/{id}/attach`, turn submission w/ `x-fabro-turn-id`, `Run.ask_fabro` readiness. Web helpers exist: `session-stream.ts`, `sessionsApi`.
- Sidebar (`ask-fabro-sidebar.tsx`) is a prototype: scripted adapter (`chats-runtime.ts`/`chats-script.ts`), no API calls, gated behind `?ask=1` (`run-detail.tsx:363`).
- Ask Fabro sessions built by `build_agent_session` (`fabro-server/.../handler/sessions.rs:633`): profile + run sandbox + `ReadOnly` gate (`build_ask_fabro_tool_approval`, `sessions.rs:867`). **No `fabro_run_*` tools.**
- `fabro-tool`: tools built on the `FabroToolBackend` trait. `fabro_client::FabroClient` is the HTTP impl — already implements every trait method (reads + mutations). `FabroRunToolServices`, `register_fabro_run_tools`, `execute_fabro_run_tool` live in `fabro-workflow` (`handler/llm/api.rs`, `services.rs`); `register_fabro_run_tools` registers all 5 tools.
- Worker tokens: `worker_token.rs` — `issue_worker_token(keys, &run_id)` mints a base same-run token; `AppState::worker_token_keys()` exposes the keys. `server.rs` already mints tokens this way for dispatched workers.
- `register_fabro_run_tools` is `pub(crate)`; `fabro-server` already depends on `fabro-workflow`.

---

## File structure

**Phase 1 — Rust**
- Modify: `lib/crates/fabro-workflow/src/handler/llm/api.rs` — make `register_fabro_run_tools` `pub`; add subset variant.
- Modify: `lib/crates/fabro-server/src/server/handler/sessions.rs` — `build_profile` returns `Box`; mint worker token, build `FabroClient`, register subset; allowlist the two tools in the gate.

**Phase 2 — Web**
- Create: `apps/fabro-web/app/lib/ask-fabro-runtime.ts` — real session adapter.
- Modify: `apps/fabro-web/app/components/chats/ask-fabro-sidebar.tsx` — use real adapter, take `runId`.
- Modify: `apps/fabro-web/app/routes/run-detail.tsx` — drop `?ask=1`, gate on `run.ask_fabro`.
- Delete (verify orphaned first): `apps/fabro-web/app/lib/chats-script.ts` + scripted paths in `chats-runtime.ts`.

---

## Phase 1: Run tools for Ask Fabro sessions

### Task 1 — Make run-tool registration callable from fabro-server

- [ ] In `fabro-workflow/src/handler/llm/api.rs`: change `register_fabro_run_tools` from `pub(crate)` to `pub`. Add a subset variant:
  ```rust
  pub fn register_fabro_run_tools_subset(
      registry: &mut ToolRegistry,
      services: &FabroRunToolServices,
      only: &[&str],
  ) {
      for definition in fabro_tool::tool_definitions() {
          if only.is_empty() || only.contains(&definition.name) {
              registry.register(fabro_run_tool(definition, services.clone()));
          }
      }
  }
  ```
  Refactor `register_fabro_run_tools` to call it with `&[]`. `fabro_run_tool` stays private.
- [ ] Confirm `FabroRunToolServices` (`fabro-workflow/src/services.rs`) is `pub` — it is. No change.
- [ ] `cargo build --workspace`; `cargo nextest run -p fabro-workflow agent_run`.
- [ ] Commit: `refactor(fabro-workflow): expose run-tool registration with tool subset`.

### Task 2 — Wire FabroClient + worker token into `build_agent_session`

The session's run-control backend is the HTTP `FabroClient` pointed at the server's own API, authed with a same-run worker token. The token enforces run scoping (cross-run calls 403).

**Files:** `fabro-server/src/server/handler/sessions.rs`

- [ ] Change `build_profile` to return `Box<dyn AgentProfile>` (currently `Arc`); the caller registers tools on `&mut` then `Arc::from`s.
- [ ] In `build_agent_session`, after `build_profile`, before `Session::from_record`:
  ```rust
  let worker_token = issue_worker_token(state.worker_token_keys(), &run_id)
      .map_err(|err| AskFabroBuildError::Agent(anyhow::Error::new(err)))?;
  // fabro_client::Client = generated reqwest client from the `fabro-client` crate.
  let api_client = fabro_client::Client::new_with_client(
      &state.self_base_url(),                 // server's own loopback base URL
      reqwest_client_with_bearer(&worker_token),
  );
  let backend = fabro_tool::fabro_client::FabroClient::new(Arc::new(api_client));
  let services = FabroRunToolServices {
      backend:            Arc::new(backend),
      current_run_id:     run_id,
      base_cwd:           PathBuf::new(),     // unused by events/interact
      user_settings_path: PathBuf::new(),    // unused by events/interact
  };
  register_fabro_run_tools_subset(
      profile.tool_registry_mut(),
      &services,
      &[fabro_tool::FABRO_RUN_EVENTS_TOOL_NAME, fabro_tool::FABRO_RUN_INTERACT_TOOL_NAME],
  );
  ```
  Reference impls: the worker-token mint in `server.rs`; `FabroRunToolServices` construction in `fabro-cli/src/commands/run/runner.rs`.
- [ ] Resolve `self_base_url()` — the server's own loopback address. If `AppState` doesn't already expose it, add an accessor from the bound listen addr (`http://127.0.0.1:<port>`). Local-only call; never the public URL.
- [ ] Ensure the session prompt/context names the owning run id so the agent passes the correct `run_id` to the tools. (Backstop: wrong id → API 403, agent self-corrects.)
- [ ] `cargo build --workspace`.
- [ ] Commit: `feat(fabro-server): give Ask Fabro sessions run-control tools`.

### Task 3 — Allowlist the two run tools in the session gate

`build_ask_fabro_tool_approval` (`sessions.rs:867`) currently denies everything not `ReadOnly`-approved. The two run tools need full access; file/shell stay read-only.

- [ ] Update the closure:
  ```rust
  Arc::new(move |tool_name: &str, _args: &Value| {
      if matches!(tool_name, "fabro_run_interact" | "fabro_run_events") {
          return Ok(()); // run-control tools: full access, scoped by worker token
      }
      if is_tool_auto_approved(PermissionLevel::ReadOnly, tool_name) {
          Ok(())
      } else {
          Err(format!("{tool_name} tool denied by Ask Fabro tool policy"))
      }
  })
  ```
- [ ] Rename `build_ask_fabro_tool_approval` comment / any "read-only policy" wording — the session is no longer read-only (it can control its run via the API).
- [ ] Tests: `fabro_run_interact` and `fabro_run_events` approved; `write_file` and shell denied; `read_file` approved.
- [ ] `cargo +nightly-2026-04-14 fmt --all && cargo +nightly-2026-04-14 clippy --workspace --all-targets -- -D warnings`.
- [ ] `cargo nextest run -p fabro-server --features test-support api::sessions`.
- [ ] Commit: `feat(fabro-server): allow run tools through the Ask Fabro session gate`.

### Task 4 — E2E coverage

- [ ] E2E test (twin), mirroring `tests/it/api/sessions.rs`: create a run with a sandbox, open an Ask Fabro session, submit a turn asking about run stages — assert the agent calls a run tool and the turn completes. Add a second case: a turn that triggers a mutating `fabro_run_interact` action (e.g. `questions`/`answer` against a run with a pending question) succeeds.
- [ ] Cross-run guard test: a tool call with a different `run_id` is rejected (worker-token scope).
- [ ] `cargo nextest run -p fabro-server --features test-support --test it api::sessions`.
- [ ] Commit: `test(fabro-server): Ask Fabro run-tool E2E coverage`.

---

## Phase 2: Wire the sidebar

### Task 5 — Real session adapter

**Files:** Create `apps/fabro-web/app/lib/ask-fabro-runtime.ts`

- [ ] assistant-ui adapter parameterized by `runId`:
  - First turn: `sessionsApi.createRunSession(runId, { model })` (model from `run.ask_fabro.default_model`); persist session id in `sessionStorage` keyed by `runId` so reopen resumes.
  - Open with existing session id: `sessionsApi.getSession(id)` → render `SessionDetail.messages`, then `attachSessionEvents(id, { sinceSeq: last_seq })`.
  - Send: `streamSessionTurn(id, { input })`; map streamed `EventEnvelope`s (incl. `run.session.*` tool-call events) to assistant-ui messages.
- [ ] Route tool-call events through the existing `tool-fallback.tsx` renderer.
- [ ] `bun test app/lib/ask-fabro-runtime.test.ts` (mock SSE as `session-stream.test.ts` does).
- [ ] Commit: `feat(web): real session adapter for Ask Fabro sidebar`.

### Task 6 — Sidebar uses the adapter

- [ ] `ask-fabro-sidebar.tsx`: accept a `runId` prop; replace `createScriptedAdapter` with `ask-fabro-runtime`; remove `EMPTY_CHAT`/`scriptIndexRef`.
- [ ] `rg createScriptedAdapter` — if `chats-script.ts`/scripted paths are orphaned, delete them.
- [ ] `bun run typecheck`.
- [ ] Commit: `feat(web): drive Ask Fabro sidebar from session API`.

### Task 7 — Drop `?ask=1`, gate on readiness

- [ ] `run-detail.tsx`: remove `askEnabled`/`searchParams.get("ask")` (lines ~363-368, 635-648, 724-730).
- [ ] Render the Ask Fabro button always; `disabled={!run.ask_fabro.available}`. Disabled tooltip from `unavailable_reason`: `feature_disabled` → "Ask Fabro is disabled"; `no_sandbox`/`sandbox_not_ready` → "Run sandbox isn't ready"; `llm_unconfigured` → "No LLM configured".
- [ ] Pass `runId={params.id}` to `<AskFabroSidebar>`.
- [ ] `bun run typecheck && bun test`.
- [ ] Commit: `feat(web): enable Ask Fabro sidebar on run pages`.

---

## Tests to run before each PR

- Rust: `cargo +nightly-2026-04-14 fmt --check --all` · `cargo +nightly-2026-04-14 clippy --workspace --all-targets -- -D warnings` · `cargo build --workspace` · `cargo nextest run -p fabro-server -p fabro-workflow`
- Web: `cd apps/fabro-web && bun run typecheck && bun test`

## Unresolved questions

1. **`interact:get` payload size** — `get` may return a large `RunProjection` (all stage data). If it blows agent context, consider a trimmed projection. Verify before Task 4.
2. **Server self-base-URL** — does `AppState` already expose its bound loopback address? If not, Task 2 must add an accessor. Confirm the server always binds a loopback-reachable addr (vs. a unix socket only — see `server.listen`).
3. **Session reuse** — one Ask Fabro session per run reused across sidebar opens (plan assumes this via `sessionStorage`), or fresh each open?
4. **Capability scope** — Ask Fabro can now cancel/archive/steer/answer its run. Confirm that's the intended product surface; consider whether `archive`/`unarchive` should be excluded even though `interact` is otherwise full-access.
5. **Phase split** — Phase 1 + 2 as two PRs (Phase 2 works without 1; agent just lacks run tools), or ship together so the feature only appears once useful?


## Completed stages
- **toolchain**: succeeded
  - Script: `command -v cargo >/dev/null || { curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y && sudo ln -sf $HOME/.cargo/bin/* /usr/local/bin/; }; cargo --version 2>&1`
  - Output:
    ```
    cargo 1.95.0 (f2d3ce0bd 2026-03-21)
    ```
- **preflight_compile**: succeeded
  - Script: `cargo check -q --workspace 2>&1`
  - Output: (empty)
- **preflight_lint**: succeeded
  - Script: `cargo +nightly-2026-04-14 clippy -q --workspace --all-targets -- -D warnings 2>&1`
  - Output: (empty)
- **implement**: succeeded
  - Model: claude-opus-4-7, 271.6k tokens in / 71.0k out
  - Files: /home/daytona/workspace/fabro/apps/fabro-web/app/components/chats/ask-fabro-sidebar.tsx, /home/daytona/workspace/fabro/apps/fabro-web/app/lib/ask-fabro-runtime.test.ts, /home/daytona/workspace/fabro/apps/fabro-web/app/lib/ask-fabro-runtime.ts, /home/daytona/workspace/fabro/apps/fabro-web/app/routes/ask-fabro.tsx, /home/daytona/workspace/fabro/apps/fabro-web/app/routes/run-detail.tsx, /home/daytona/workspace/fabro/lib/crates/fabro-server/Cargo.toml, /home/daytona/workspace/fabro/lib/crates/fabro-server/src/server.rs, /home/daytona/workspace/fabro/lib/crates/fabro-server/src/server/handler/sessions.rs, /home/daytona/workspace/fabro/lib/crates/fabro-workflow/src/handler/llm/api.rs
- **simplify_opus**: succeeded
  - Model: claude-opus-4-7, 108.3k tokens in / 35.8k out
  - Files: /home/daytona/workspace/fabro/apps/fabro-web/app/lib/ask-fabro-runtime.ts, /home/daytona/workspace/fabro/apps/fabro-web/app/routes/run-detail.tsx, /home/daytona/workspace/fabro/lib/crates/fabro-server/src/server.rs, /home/daytona/workspace/fabro/lib/crates/fabro-server/src/server/handler/sessions.rs, /home/daytona/workspace/fabro/lib/crates/fabro-workflow/src/handler/llm/api.rs


# Simplify: Code Review and Cleanup

Review changes vs. origin for reuse, quality, and efficiency. Fix any issues found.

## Phase 1: Identify Changes

Run git diff (or git diff HEAD if there are staged changes) to see what changed. If there are no git changes, review the most recently modified files that the user mentioned or that you edited earlier in this conversation.

## Phase 2: Launch Three Review Agents in Parallel

Use the Agent tool to launch all three agents concurrently in a single message. Pass each agent the full diff so it has the complete context.

### Agent 1: Code Reuse Review

For each change:

1. Search for existing utilities and helpers that could replace newly written code. Use Grep to find similar patterns elsewhere in the codebase — common locations are utility directories, shared modules, and files adjacent to the changed ones.
2. Flag any new function that duplicates existing functionality. Suggest the existing function to use instead.
3. Flag any inline logic that could use an existing utility — hand-rolled string manipulation, manual path handling, custom environment checks, ad-hoc type guards, and similar patterns are common candidates.

Note: This is a greenfield app, so focus on maximizing simplicity and don't worry about changing things to achieve it.

### Agent 2: Code Quality Review

Review the same changes for hacky patterns:

1. Redundant state: state that duplicates existing state, cached values that could be derived, observers/effects that could be direct calls
2. Parameter sprawl: adding new parameters to a function instead of generalizing or restructuring existing ones
3. Copy-paste with slight variation: near-duplicate code blocks that should be unified with a shared abstraction
4. Leaky abstractions: exposing internal details that should be encapsulated, or breaking existing abstraction boundaries
5. Stringly-typed code: using raw strings where constants, enums (string unions), or branded types already exist in the codebase

Note: This is a greenfield app, so be aggressive in optimizing quality.

### Agent 3: Efficiency Review

Review the same changes for efficiency:

1. Unnecessary work: redundant computations, repeated file reads, duplicate network/API calls, N+1 patterns
2. Missed concurrency: independent operations run sequentially when they could run in parallel
3. Hot-path bloat: new blocking work added to startup or per-request/per-render hot paths
4. Unnecessary existence checks: pre-checking file/resource existence before operating (TOCTOU anti-pattern) — operate directly and handle the error
5. Memory: unbounded data structures, missing cleanup, event listener leaks
6. Overly broad operations: reading entire files when only a portion is needed, loading all items when filtering for one

## Phase 3: Fix Issues

Wait for all three agents to complete. Aggregate their findings and fix each issue directly. If a finding is a false positive or not worth addressing, note it and move on — do not argue with the finding, just skip it.

When done, briefly summarize what was fixed (or confirm the code was already clean).