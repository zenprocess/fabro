Goal: # Worker Control Bus Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the server-to-worker stdin JSONL control pipe with a backend-agnostic worker control bus, implemented now with a local in-memory bus and delivered to workers over a worker-initiated WebSocket.

**Architecture:** API handlers publish `WorkerControlEnvelope` messages to a `WorkerControlBus`; the worker WebSocket route subscribes to that bus and forwards ordered delivery frames to the worker. Workers track the last fully applied delivery id and reconnect with `?after=<id>` after any unexpected WebSocket close. The first backend is an in-process `LocalWorkerControlBus` for local and single-node deployments. A Redis Streams backend must fit behind the same trait later, but Redis is explicitly out of scope for this implementation plan.

**Tech Stack:** Rust, Axum WebSockets, tokio-tungstenite, UnixStream, async-trait or boxed async traits, tokio channels/Notify, worker JWT auth, existing `WorkerControlEnvelope`.

---

## Key Decisions

- WebSocket fully replaces stdin control. Do not keep stdin JSONL as a compatibility path.
- The worker protocol stays identical across local, single-node, ECS, and later SaaS deployments.
- The server-side delivery backend is the only thing that varies by deployment.
- This plan implements only `LocalWorkerControlBus`.
- This plan does not add Redis dependencies, Redis configuration, Redis tests, Redis health checks, or Redis runtime behavior.
- Redis Streams are covered only as a future backend contract so the local design does not paint us into a corner.
- There is no `Latest` cursor. The first worker connection starts at the beginning of the run's retained control stream; reconnects resume after the worker's last fully applied delivery id.
- Every WebSocket text frame is a delivery frame with an id and envelope. The worker advances `last_applied_id` only after applying the envelope.
- Workers reconnect forever while the local run is not terminal, using backoff from 100ms, doubled after each failure, capped at 5s.
- The worker must complete its first control-stream connection before starting or resuming workflow execution. Temporary first-connect failures wait and retry; they do not start a control-disconnected run.
- Invalid cursor means the bus can no longer prove replay correctness. The worker treats it as fatal control-channel loss and fails/aborts the run as infrastructure failure, not as user cancellation.
- WebSocket liveness is handled at the WebSocket layer with explicit ping/pong and timeout logic. The bus does not know about heartbeats.
- ECS task launch, ECS stop/reconciliation, Redis-backed multi-node delivery, and remote hard-kill behavior are follow-up work.

## Redis Fit Later: Out of Scope Now

Redis should later implement the same `WorkerControlBus` API introduced here.

- `publish(run_id, envelope)` maps to `XADD fabro:run:{run_id}:control ...`.
- First `subscribe(run_id, Start)` maps to `XREAD BLOCK ... STREAMS fabro:run:{run_id}:control 0-0`.
- Reconnect `subscribe(run_id, After(id))` maps to `XREAD BLOCK ... STREAMS fabro:run:{run_id}:control {id}`.
- Local message ids use an opaque string format such as `local:1`; Redis message ids can use Redis stream ids such as `1716810000000-0`.
- The WebSocket route should not care whether the subscription source is local memory or Redis.
- The worker should not care whether the frame came from a local bus or Redis.
- Redis trimming/retention, consumer groups, per-tenant key naming, TLS/auth, reconnect-after-redeploy semantics, and SaaS config validation are not part of this plan.

## Proposed File Structure

- Create `lib/crates/fabro-server/src/worker_control/mod.rs`
  - Owns the server-side control bus abstraction and re-exports the local backend.
- Create `lib/crates/fabro-server/src/worker_control/bus.rs`
  - Defines `WorkerControlBus`, `WorkerControlDelivery`, `WorkerControlMessageId`, `WorkerControlCursor`, and bus errors.
- Create `lib/crates/fabro-server/src/worker_control/local.rs`
  - Implements `LocalWorkerControlBus` using process memory.
- Create `lib/crates/fabro-server/src/server/handler/worker_control.rs`
  - Adds the worker-only WebSocket route.
- Modify `lib/crates/fabro-server/src/server.rs`
  - Adds the bus to `AppState`, replaces subprocess `RunAnswerTransport` sends with bus publishes, removes stdin pumping.
- Modify `lib/crates/fabro-server/src/server/handler/mod.rs`
  - Registers the worker control route.
- Modify `lib/crates/fabro-server/src/server/handler/lifecycle.rs`
  - Sends pause/unpause/cancel controls through the transport/bus where appropriate.
- Modify `lib/crates/fabro-cli/src/commands/run/runner.rs`
  - Replaces stdin reading with worker WebSocket client handling.
- Modify `lib/crates/fabro-cli/Cargo.toml`
  - Adds `tokio-tungstenite` as a direct dependency if needed.
- Modify `lib/crates/fabro-interview/src/control_protocol.rs`
  - Adds pause/unpause control messages and a transport delivery frame type shared by server and worker.

## Task 1: Define the Control Bus Contract

**Files:**
- Create: `lib/crates/fabro-server/src/worker_control/mod.rs`
- Create: `lib/crates/fabro-server/src/worker_control/bus.rs`
- Modify: `lib/crates/fabro-server/src/lib.rs`

- [ ] Add a private `worker_control` module in `fabro-server`.
- [ ] Define `WorkerControlMessageId` as an opaque cloneable id rather than a numeric type.
- [ ] Define `WorkerControlCursor` with `Start` and `After(WorkerControlMessageId)` variants.
- [ ] Define `WorkerControlDelivery { id: WorkerControlMessageId, envelope: WorkerControlEnvelope }`.
- [ ] Define `WorkerControlBus` with async `publish(run_id, envelope)` and `subscribe(run_id, cursor)` methods.
- [ ] Make `subscribe` return a stream-like receiver owned by the caller, so the WebSocket handler can forward messages without knowing the backend.
- [ ] Define explicit bus errors for closed backend, unavailable backend, invalid cursor, and publish timeout.
- [ ] Document in code comments that `Start` maps to Redis stream id `0-0` and `After(id)` maps to Redis `XREAD` after that id, but do not add Redis code.
- [ ] Add unit tests for id equality/debug formatting and cursor parsing from the optional `after` query parameter.
- [ ] Test that absent `after` parses as `WorkerControlCursor::Start`.
- [ ] Test that present `after=local:42` parses as `WorkerControlCursor::After(...)`.
- [ ] Run `cargo nextest run -p fabro-server worker_control`.

## Task 2: Implement the Local In-Memory Bus

**Files:**
- Create: `lib/crates/fabro-server/src/worker_control/local.rs`
- Test: `lib/crates/fabro-server/src/worker_control/local.rs`

- [ ] Implement `LocalWorkerControlBus` as `Arc<Mutex<HashMap<RunId, LocalRunControlStream>>>`.
- [ ] Store messages per run in insertion order with a monotonic local sequence id.
- [ ] Wake active subscribers when `publish` appends a message.
- [ ] Support `subscribe(run_id, Start)` for first worker startup; it must replay retained messages from the beginning of the run control stream.
- [ ] Support `subscribe(run_id, After(id))` so reconnect uses the same API that later maps to Redis `XREAD`.
- [ ] Allow `publish` before the worker subscribes; retained messages must be visible to the first `Start` subscriber.
- [ ] Trim retained local messages to a bounded per-run size so a disconnected local worker cannot grow memory without bound. Use a named constant with initial value 1024 messages per run.
- [ ] Return a clear `invalid cursor` error when a subscriber asks for an id that has been trimmed or belongs to a different local stream.
- [ ] Add a cleanup method for terminal runs so completed/cancelled runs can release retained control messages.
- [ ] Test that messages publish in order.
- [ ] Test that an active subscriber receives a message published after subscription.
- [ ] Test that messages published before subscription are replayed to a `Start` subscriber.
- [ ] Test that `After(id)` receives only later messages.
- [ ] Test that trimming bounds retained messages and reports an invalid old cursor.
- [ ] Run `cargo nextest run -p fabro-server worker_control`.

## Task 3: Add Control Bus to Server State

**Files:**
- Modify: `lib/crates/fabro-server/src/server.rs`
- Test: `lib/crates/fabro-server/src/server/tests.rs`

- [ ] Add `worker_control_bus: Arc<dyn WorkerControlBus>` to `AppState`.
- [ ] Construct `LocalWorkerControlBus` in normal server state initialization.
- [ ] Add a test-only way to inject a fake or local bus without exposing test helpers to production builds.
- [ ] Keep demo/in-process execution behavior unchanged unless it currently depends on subprocess control.
- [ ] Add a state construction test proving the default bus is local and available.
- [ ] Run `cargo nextest run -p fabro-server worker_control`.

## Task 4: Extend the Control Protocol

**Files:**
- Modify: `lib/crates/fabro-interview/src/control_protocol.rs`
- Test: `lib/crates/fabro-interview/src/control_protocol.rs`

- [ ] Add `WorkerControlEnvelope::pause_run()` and `WorkerControlEnvelope::unpause_run()` constructors.
- [ ] Add `WorkerControlMessage::RunPause` serialized as `"run.pause"`.
- [ ] Add `WorkerControlMessage::RunUnpause` serialized as `"run.unpause"`.
- [ ] Add `WorkerControlDeliveryFrame { id: String, envelope: WorkerControlEnvelope }` as the WebSocket text-frame payload shared by server and worker.
- [ ] Add round-trip serde tests for both new messages.
- [ ] Add round-trip serde tests for `WorkerControlDeliveryFrame`.
- [ ] Run `cargo nextest run -p fabro-interview control_protocol`.

## Task 5: Share Worker Message Handling

**Files:**
- Modify: `lib/crates/fabro-cli/src/commands/run/runner.rs`
- Test: `lib/crates/fabro-cli/src/commands/run/runner.rs`

- [ ] Split `apply_worker_control_line(...)` into parsing and `apply_worker_control_message(...)`.
- [ ] Route WebSocket delivery frames through `apply_worker_control_message(...)`.
- [ ] Route `run.pause` to `RunControlState::request_pause()`.
- [ ] Route `run.unpause` to `RunControlState::request_unpause()`.
- [ ] Add a small in-memory applied-id dedupe set in the worker control task; ignore duplicate delivery ids before applying envelopes.
- [ ] Update `last_applied_id` only after `apply_worker_control_message(...)` returns.
- [ ] Treat all current control messages as idempotent under delivery-id dedupe. `run.steer` must not be applied twice for the same delivery id.
- [ ] Keep control stream close behavior explicit: an unexpected close triggers reconnect; a fatal invalid cursor interrupts pending interviews and fails/aborts the run as control-channel loss.
- [ ] Update existing stdin-era tests to exercise the shared message handler directly.
- [ ] Add tests for pause and unpause routing.
- [ ] Add a test proving duplicate delivery ids are not applied twice.
- [ ] Run `cargo nextest run -p fabro-cli runner`.

## Task 6: Add Worker WebSocket Client

**Files:**
- Modify: `lib/crates/fabro-cli/Cargo.toml`
- Modify: `lib/crates/fabro-cli/src/commands/run/runner.rs`
- Test: `lib/crates/fabro-cli/src/commands/run/runner.rs`

- [ ] Add `tokio-tungstenite.workspace = true` as a direct `fabro-cli` dependency if the crate does not already have it.
- [ ] Add a helper that builds the control-stream request for a `ServerTarget` and `RunId`.
- [ ] For HTTP URLs, convert `http` to `ws` and `https` to `wss`.
- [ ] For Unix socket paths, connect `tokio::net::UnixStream` and use `ws://fabro/api/v1/runs/{run_id}/worker/control-stream` for the handshake host/path.
- [ ] Add the worker bearer token as an `Authorization: Bearer ...` request header.
- [ ] On the first connection, omit the `after` query parameter so the server maps it to `WorkerControlCursor::Start`.
- [ ] On reconnect, include `?after=<last_applied_id>` when `last_applied_id` is set.
- [ ] Spawn a WebSocket control manager task in `execute(...)` after `ControlInterviewer`, `RunControlState`, `CancellationToken`, and `SteeringHub` are created, and before `operations::start` or `operations::resume`.
- [ ] Gate `operations::start` and `operations::resume` on the first successful control-stream connection.
- [ ] The control manager should keep reconnecting while the run is not locally terminal, with backoff starting at 100ms, doubling after each failure, and capped at 5s.
- [ ] Deserialize each text frame into `WorkerControlDeliveryFrame`.
- [ ] Apply each envelope through `apply_worker_control_message(...)`, then record the frame id as `last_applied_id`.
- [ ] Respond to received WebSocket ping frames with pong frames.
- [ ] Send worker-initiated ping frames every 15s.
- [ ] Track pongs for worker-initiated pings and close the WebSocket after 45s without a matching pong or other proof of connection liveness.
- [ ] Treat normal close/error as reconnectable while the run is not terminal.
- [ ] Treat HTTP 410 Gone or a WebSocket close reason of `invalid_cursor` as fatal control-channel loss.
- [ ] On fatal control-channel loss, interrupt pending interviews and fail/abort the run with an infrastructure/control-channel error, not a user cancellation.
- [ ] Wire fatal control-channel loss back into `execute(...)` so the worker returns an error instead of silently continuing workflow execution.
- [ ] Add tests for URL/request construction for `http`, `https`, and Unix socket targets.
- [ ] Add tests proving first connection has no `after` query and reconnect includes `after=<last_applied_id>`.
- [ ] Add tests for reconnect backoff bounds.
- [ ] Add tests for ping/pong timeout behavior using paused Tokio time.
- [ ] Add a local Unix-socket WebSocket test proving the client can complete a handshake against an Axum route.
- [ ] Run `cargo nextest run -p fabro-cli runner`.

## Task 7: Add Worker-Only Control Stream Route

**Files:**
- Create: `lib/crates/fabro-server/src/server/handler/worker_control.rs`
- Modify: `lib/crates/fabro-server/src/server/handler/mod.rs`
- Modify: `lib/crates/fabro-server/src/principal_middleware.rs`
- Test: `lib/crates/fabro-server/src/server/tests.rs`

- [ ] Add a narrow helper or extractor that accepts only authenticated worker principals whose token run id matches the route run id.
- [ ] Add `GET /runs/{id}/worker/control-stream` to real API routes only.
- [ ] Reject missing runs, terminal runs, and archived runs before upgrading.
- [ ] Reject user JWTs and cross-run worker JWTs.
- [ ] Parse absent `after` into `WorkerControlCursor::Start`.
- [ ] Parse present `after` into `WorkerControlCursor::After(id)`.
- [ ] On upgrade, call `worker_control_bus.subscribe(run_id, cursor)`.
- [ ] If `subscribe` returns invalid cursor before upgrade, reject with HTTP 410 Gone.
- [ ] Serialize each `WorkerControlDelivery` to `WorkerControlDeliveryFrame` and send it as a WebSocket text frame.
- [ ] Send server-initiated ping frames every 15s.
- [ ] Respond to received WebSocket ping frames with pong frames.
- [ ] Track pongs for server-initiated pings and close the WebSocket after 45s without a matching pong or other proof of connection liveness.
- [ ] On timeout or disconnect, drop the bus subscription so local resources are released.
- [ ] Do not store the live WebSocket sender in `ManagedRun`; the bus is now the delivery boundary.
- [ ] Add tests for auth rejection, successful `Start` subscription, successful `After(id)` subscription, frame delivery, invalid cursor rejection as 410 Gone, ping/pong timeout cleanup, and cross-run worker rejection.
- [ ] Run `cargo nextest run -p fabro-server worker_control`.

## Task 8: Replace Server-Side Stdin Transport with Bus Publishing

**Files:**
- Modify: `lib/crates/fabro-server/src/server.rs`
- Modify: `lib/crates/fabro-server/src/server/handler/lifecycle.rs`
- Modify: `lib/crates/fabro-server/src/server/handler/pair.rs`
- Test: `lib/crates/fabro-server/src/server/tests.rs`

- [ ] Replace `RunAnswerTransport::Subprocess { control_tx }` with a bus-backed subprocess/worker variant.
- [ ] Ensure the bus-backed variant has enough context to publish messages for the correct `RunId`.
- [ ] Delete `pump_worker_control_jsonl(...)`.
- [ ] Change `worker_command(...)` so `__run-worker` uses `stdin(Stdio::null())` instead of `stdin(Stdio::piped())`.
- [ ] Remove child-stdin extraction and the control pump task from `execute_run_subprocess(...)`.
- [ ] Keep stderr capture and worker exit handling unchanged.
- [ ] Update `RunAnswerTransport` methods so answer, cancel, steer, interrupt, pair start/message/end all publish the existing envelope to `WorkerControlBus`.
- [ ] Add `pause_run()` and `unpause_run()` methods on `RunAnswerTransport`.
- [ ] Update pause/unpause lifecycle handlers to send `run.pause` and `run.unpause` over the bus for running workers.
- [ ] Keep process signals only for hard cleanup paths such as cancel fallback, shutdown, terminal delete, and force removal.
- [ ] Update existing tests that assert subprocess transport enqueue behavior to assert bus publish behavior instead.
- [ ] Add a `worker_command` test proving stdin is null/not piped and `FABRO_WORKER_TOKEN` still travels only through env.
- [ ] Run `cargo nextest run -p fabro-server worker_command`.

## Task 9: End-to-End Local Control Flow Regression

**Files:**
- Test: `lib/crates/fabro-cli/tests/it/cmd/runner.rs`
- Test: `lib/crates/fabro-server/tests/it/scenario/lifecycle.rs`

- [ ] Add a test run where the worker connects to the control WebSocket and receives a cancel request through `LocalWorkerControlBus`.
- [ ] Add a test where the server publishes a control message before the worker connects and the worker receives it on first connection.
- [ ] Add a reconnect test where the worker applies message A, reconnects with `after=<A>`, and then receives only message B.
- [ ] Add an invalid-cursor test proving the worker reports control-channel loss as infrastructure failure rather than user cancellation.
- [ ] Add a human-interview test proving submitted answers reach the worker through the bus and WebSocket.
- [ ] Add a steer or interrupt test proving live agent controls still reach the worker transport.
- [ ] Add a local Unix-socket server test proving the default local server target works without stdin.
- [ ] Run `cargo nextest run -p fabro-cli --test it runner`.
- [ ] Run `cargo nextest run -p fabro-server --test it lifecycle`.

## Task 10: Final Verification

**Files:**
- Modify only if failures expose necessary fixes.

- [ ] Run `cargo nextest run -p fabro-interview control_protocol`.
- [ ] Run `cargo nextest run -p fabro-server worker_control`.
- [ ] Run `cargo nextest run -p fabro-cli runner`.
- [ ] Run `cargo nextest run -p fabro-server worker_command`.
- [ ] Run `cargo nextest run -p fabro-server --test it lifecycle`.
- [ ] Run `cargo +nightly-2026-04-14 clippy --workspace --all-targets -- -D warnings`.
- [ ] Confirm no code path still writes `WorkerControlEnvelope` to child stdin.
- [ ] Confirm no Redis dependency, Redis config key, or Redis runtime path was added.
- [ ] Confirm there is no `Latest` cursor or wait-for-subscriber behavior in the control bus.
- [ ] Confirm WebSocket ping/pong handling is explicit on both worker and server.
- [ ] Confirm `run.steer` and other controls are protected from duplicate delivery-id application.
- [ ] Confirm `__run-worker` still scrubs `FABRO_WORKER_TOKEN` from process env before launching descendants.

## Acceptance Criteria

- All worker control traffic uses the worker control bus plus WebSocket last-mile transport.
- Local Unix-socket server targets and remote HTTP(S) server targets both support worker control without Redis.
- Local and single-node deployments require no external control-channel service.
- The bus API can later be implemented by Redis Streams without changing API handlers or worker message handling.
- First worker connection replays retained messages from the beginning of the run control stream; reconnect resumes after the last fully applied id.
- Invalid cursor is the only fatal control-stream replay failure and is surfaced as infrastructure/control-channel failure, not user cancellation.
- WebSocket liveness is explicit and backend-agnostic.
- Existing run event/blob/artifact HTTP paths are unchanged.
- Existing worker JWT scope rules remain authoritative.
- Temporary WebSocket disconnects reconnect and replay through the bus; only unrecoverable replay loss reports worker-control-unavailable behavior.
- Worker stdout/stderr behavior remains unchanged except that stdin is no longer a control channel.
- Redis is clearly documented as future work and is not required by this plan.


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
  - Model: gpt-5.5, 3.4m tokens in / 94.1k out
  - Files: /home/daytona/workspace/fabro/lib/crates/fabro-server/src/server/handler/worker_control.rs, /home/daytona/workspace/fabro/lib/crates/fabro-server/src/worker_control/bus.rs, /home/daytona/workspace/fabro/lib/crates/fabro-server/src/worker_control/local.rs, /home/daytona/workspace/fabro/lib/crates/fabro-server/src/worker_control/mod.rs


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