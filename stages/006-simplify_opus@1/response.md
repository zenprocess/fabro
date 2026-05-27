Net 47 lines deleted. Here's a summary of the cleanup applied based on the three parallel reviews:

## Cleanup Summary

**Correctness fix:**
- **Lost-wakeup race in `LocalWorkerControlBus`**: `local_subscription_task` could miss a publish that landed between message-read and `notify.notified()` registration. Now registers `notified()` before inspecting the stream and uses the proper enable-before-check pattern.

**Memory / efficiency:**
- **Removed unbounded `applied_ids: HashSet<String>`** in the CLI worker control task. The server cursor + monotonic IDs already guarantee no replay; kept a simple `last_applied_id == frame.id` check as defense-in-depth.
- **Dropped `Arc<Mutex<Option<String>>>` for `last_applied_id`**: it's owned by a single task. Now a plain `&mut Option<String>`, removing two lock-held-across-await sites per loop iteration.
- **Pinned the `Sleep` once outside the WS loop** in both server and worker handlers, calling `Pin::reset` instead of allocating a new `Sleep` per iteration.
- **Replaced O(N) `iter().filter().cloned()` over the retained deque** with `partition_point` + tail iteration in `collect_messages_from`.

**Reuse / consolidation:**
- **Moved `WORKER_CONTROL_WS_PING_INTERVAL`, `WORKER_CONTROL_WS_LIVENESS_TIMEOUT`, `WORKER_CONTROL_INVALID_CURSOR_REASON`, `WORKER_CONTROL_PONG_TIMEOUT_REASON`** into `fabro-interview::control_protocol` so server and worker share one source of truth.
- **Used `axum::extract::ws::close_code::AWAY` / `POLICY` constants** instead of raw `1001` / `1008` literals on the server.

**Dead code removal:**
- **Removed `WorkerControlBus::backend_name`** trait method (only used by a tautological test) and the associated `#[allow(dead_code)]` and `Hash` derive on `WorkerControlMessageId`.
- **Removed the single-variant `WorkerCommandStdin` enum** + constant + tautological test; inlined `Stdio::null()`.
- **Deleted `#[cfg(test)] parse_worker_control_line` and `apply_worker_control_line`** wrappers; rewrote the four tests to call `apply_worker_control_message` directly with typed envelopes.

**Verification:**
- `cargo nextest run -p fabro-server worker_control` — 15/15 pass
- `cargo nextest run -p fabro-interview` — 62/62 pass
- `cargo nextest run -p fabro-cli runner` — 33/33 pass
- `cargo nextest run -p fabro-server --test it --features test-support lifecycle` — 3/3 pass
- `cargo +nightly-2026-04-14 clippy --workspace --all-targets -- -D warnings` — clean
- Two pre-existing `pause_run_*` / `unpause_run_sets_pending_control` failures verified to exist before this cleanup (confirmed via `git stash`).

**Findings I deliberately skipped** (not high-impact enough or would require API surface changes outside this cleanup):
- Splitting `WorkerControlBusError` into publish/subscribe error types
- Replacing `BoxFuture` with `async_trait` (style consistency)
- Moving WS-URL construction onto `ServerTarget` (touches `fabro-client`, `fabro-sandbox`)
- Adopting `fabro_util::backoff::BackoffPolicy` (small benefit; existing hand-rolled is correct)
- Collapsing `WorkerControlSocket` enum into `Box<dyn AsyncRead + AsyncWrite>` (works but tungstenite's `from_raw_socket` generic constraints make this fiddly)
- Per-run inner mutex sharding in the bus (premature for local single-node)
- `cleanup_worker_control_bus_for_run` spawn-per-call (sync-fast-path optimization; correctness fine)