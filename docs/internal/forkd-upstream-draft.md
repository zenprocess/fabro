# Upstream bug-report draft — forkd exec timeout mis-reported as EAGAIN 500

> **DRAFT. NOT POSTED.** This file is a sanitized upstream contribution
> draft prepared for `github.com/deeplethe/forkd`. **Filing is
> operator-gated** per `docs/internal/` policy and the
> T3 boundary on publishing outside private repos. The original
> authoring trail is internal and must not be re-pasted into the
> upstream issue tracker; this draft is the only artifact intended to
> cross the boundary.
>
> Sanitization: stripped of all internal hostnames, internal subnets,
> internal paths, tokens, and repo-internal issue numbers. Generic
> upstream framing only.

---

## Title (proposed)

`exec_at` blocking read + `set_read_timeout(timeout_secs + 5)` reports
all deadline failures as a generic 500 with
`"Resource temporarily unavailable (os error 11)"`; the timeout is
never reported as a timeout

---

## Body (proposed)

### Environment

- forkd-controller: 0.5.2 (and confirmed unchanged on `main` at the
  time of writing — see also `v0.5.3` changelog which does not touch
  this path).
- forkd-vmm `exec_at()`: `crates/forkd-vmm/src/lib.rs` (v0.5.2 lines
  576-601; structurally unchanged on `main`).
- Guest agent: `rootfs-init/forkd-agent.py` (runs as PID 1 inside the
  microVM, served on port 8888 of the guest TAP).
- Workload: a multi-stage CI/test command invoked via
  `POST /v1/sandboxes/:id/exec` with `timeout_secs = 500`. Realistic
  stage length varies from <1s to several minutes.

### Summary

`exec_at()` blocks on a single TCP read of the guest agent's one-shot
JSON reply, with a `set_read_timeout(timeout_secs + 5)` deadline. The
guest agent only sends its reply after its own internal
`subprocess.run(...)` returns. When the inner subprocess overruns the
guest-side timeout, the situation is:

1. The agent-side `subprocess.run(..., timeout=N)` raises
   `TimeoutExpired` and CPython kills only the direct child.
2. The grandchild process(es) still hold the stdout/stderr pipes
   inherited from the original child, so the kernel keeps the read
   end open.
3. CPython's `communicate()` therefore blocks indefinitely on the
   read side — the agent's own timeout reply never gets serialized
   and sent.
4. Concurrently, the controller's `read_to_string` hits its
   `timeout_secs + 5` `SO_RCVTIMEO` deadline and returns
   `ErrorKind::WouldBlock` ("Resource temporarily unavailable",
   `os error 11`).
5. The controller wraps the error in `anyhow::Error` with the context
   `"read response"` and the HTTP layer maps it to a generic 500.

The caller therefore cannot tell a real exec-timeout apart from a
genuine `EAGAIN` resource problem; both look like the same 500 body
`exec: read response: Resource temporarily unavailable (os error 11)`.

### Fingerprint

A large controlled sample of `POST /v1/sandboxes/:id/exec` against
the deployed controller shows:

- Every `exec`-timeout failure fires at exactly
  `requested_timeout + 5s` wall-clock (505.3s for `timeout_secs=500`).
- Every successful run completes in well under the deadline (the
  observed band tops out around 180s in the workload that triggers
  this).
- The band between `~180s` and `~505s` is empty: there are no
  intermediate "slow but successful" runs, only two disjoint
  populations. This is incompatible with a stochastic `EAGAIN`
  resource-exhaustion interpretation and is consistent with the
  deadline hypothesis.

### Suggested fix (proposed alongside the report)

Two coordinated changes, one in the controller, one in the guest
agent.

**Controller (`crates/forkd-vmm/src/lib.rs` `exec_at`):**

Distinguish the `WouldBlock` / `TimedOut` read outcome from other I/O
errors. When the read deadline fires, return a typed
`ExecDeadlineExceeded { elapsed: Duration, requested_secs: u64 }`
error. The HTTP layer (`POST /v1/sandboxes/:id/exec`) `downcast_ref`s
the error and returns:

- Status: `408 Request Timeout`
- Body: `application/json` with a single `error` string field whose
  value contains the literal substring `exec deadline exceeded` and
  the elapsed seconds, e.g.
  ```json
  { "error": "exec deadline exceeded after 505.3s (requested timeout_secs=500)" }
  ```

The literal substring is a public contract for upstream classifiers;
any consumer matching on it should grep for that token.

**Guest agent (`rootfs-init/forkd-agent.py`):**

Two mechanisms compound in the failure path:

1. `subprocess.run(capture_output=True, timeout=...)` only kills the
   direct child on `TimeoutExpired`. The child should be spawned with
   `start_new_session=True` (or `preexec_fn=os.setsid`), making it
   its own process-group leader. On timeout, call
   `os.killpg(proc.pid, signal.SIGKILL)` to take the whole group
   down — that releases the stdout/stderr pipes that the grandchildren
   inherited. A `proc.kill()` fallback for `PermissionError` is
   reasonable.
2. Even with the group-kill fix, the guest's own timeout reply is in
   a race with the controller's read deadline. Subtract a margin
   (e.g. 10s) from the inner subprocess timeout so the guest's
   self-deadline fires inside the controller's `+5s` read window.

Both fixes are needed: the process-group kill alone does not
eliminate the race, and the margin alone does not eliminate the
grandchild-pipe hang.

Always send a reply on the timeout path too — never let the
controller's read deadline be the only timeout surface. The reply
should include a recognizable `error` string (e.g.
`exec deadline exceeded: timeout_secs=N`) and a marker the
controller can ignore (e.g. `timed_out: true`). The existing
`ExecResponse` struct uses `#[serde(default)]` so additional fields
are forward-compatible.

### Acceptance

- A workload that overruns its timeout returns HTTP 408 with a JSON
  body whose `error` string contains `exec deadline exceeded` and
  the elapsed seconds.
- The same workload no longer returns HTTP 500 with body
  `exec: read response: Resource temporarily unavailable (os error 11)`.
- The 505s wall-clock fingerprint moves to approximately
  `timeout_secs + 5 - GUEST_TIMEOUT_MARGIN_SECS` (i.e. the agent
  times out and replies first; the controller's deadline never
  fires).
- The 180s-band / 505s-band bimodality collapses to a single band:
  either the run completes inside the budget, or it times out
  cleanly at the new wall-clock.
- The change is forward-compatible: the existing `ExecResponse` is
  unchanged on the wire, only its containing 500 error path is
  replaced with a 408.

### Why not a 5xx retry on the caller side?

Retrying on 5xx in the caller would not help: a real exec-timeout
will deterministically re-fire on retry with the same wall-clock
fingerprint. Honest 408 reporting lets the caller decide whether to
retry, fail, or report — instead of being forced to treat a
deterministic deadline as a transient resource blip.

### Why not just bump the controller's read timeout further?

`timeout_secs + 5` is already generous over the guest-side timeout
(`timeout_secs`). The problem is not that the headroom is too small;
the problem is that the headroom is consumed by a different failure
mode (the grandchild-pipe hang) which has no fixed duration. The
right fix is to make the guest return a real timeout response, not
to widen the controller's wait.

### Reproduction (sanitized)

```bash
# 1. Boot forkd-controller locally; create a snapshot; fork one child.
# 2. POST /v1/sandboxes/:id/exec with timeout_secs=500 and an
#    argv that is guaranteed to overrun:
#      ["sh", "-c", "sleep 600"]
# 3. Observe: HTTP 500, body
#      {"error": "exec: read response: Resource temporarily unavailable (os error 11)"}
#    and a wall-clock of ~505s.
# 4. Expected after fix: HTTP 408, body
#      {"error": "exec deadline exceeded after 4XX.Xs (requested timeout_secs=500)"}
#    and a wall-clock slightly under 500s.
```

### Notes on the agent's environment

The guest agent runs as PID 1 inside a microVM. It is the only
process capable of `killpg` against its own children (PID 1 retains
`CAP_KILL` across forks). No additional privileges are required
for the fix. The `signal` module is already part of the Python
standard library; no new dependencies are introduced.

### Notes for maintainers

- This is a behavior change on the public HTTP surface (`/exec`
  error status and body). The 408 is a new status for that route;
  existing 200/200-with-`error` paths are unchanged.
- The wire-contract marker is the `exec deadline exceeded`
  substring in the body. If you would prefer a different literal,
  please flag in the issue thread before merge so downstream
  classifiers can be updated in lockstep.
- The guest agent's reply gains a new `timed_out` boolean field.
  `ExecResponse` already uses `#[serde(default)]` for the four
  existing fields, so unknown fields are ignored on deserialization;
  older controllers see the same struct.

---

## Local-side checklist (NOT part of the upstream post)

When this draft is eventually operator-gated to be filed at
`deeplethe/forkd`, the local side should also do:

- [ ] Re-read the draft end-to-end and verify nothing in
      `/Users/vvladescu/...` hostnames, `10.0.201.0/24` subnets,
      `*.zp.digital` hostnames, or repo-internal issue numbers
      (e.g. `#201`, `#269`, `#272`) has slipped into the body.
- [ ] Strip the entire "Local-side checklist" section before
      copying the file into the upstream issue tracker.
- [ ] Confirm the maintainer's preferred issue template; this
      draft assumes GitHub's "Bug report" form.
- [ ] File from a non-corporate account if the project accepts
      community contributions that way; the maintainer has been
      responsive on prior reports.

The re-bake requirement on the consumer side is described in
`docs/internal/forkd-patch-notes.md` (this is internal-only and
should not be referenced in the upstream post).
