# fabro-122: exec EAGAIN (os error 11) — root-cause HYPOTHESIS and client-side defense

> **MARKED AS HYPOTHESIS.** The forkd **controller** source is not local on
> this box (no `~/ao-projects/forkd`, no separate AO project for it; the fabro
> repo has only the client side under `lib/crates/fabro-sandbox/src/forkd/`).
> All claims below are derived from the OBSERVED controller error string in
> `~/.ao/state/fabro-gate-poll.log`, plus a reading of the client-side
> source. The operator runbook for the controller half lives at
> `docs/internal/forkd-snapshot-registry-runbook.md` (revision 2 covers the
> EAGAIN diagnostic in §2); the dellsrv controller source is what would
> prove or refute the hypothesis.

## 1. The observed signature (verbatim)

From `~/.ao/state/fabro-gate-poll.log` (2026-07-31 onward):

```
{"outcome": "infra", "reason": "controller POST /v1/sandboxes/sb-6a6cf9ce-0234/exec -> HTTP 500 {\"error\":\"exec: read response: Resource temporarily unavailable (os error 11)\"}", "stage": "exec"}
posted fabro/qa-pipeline=error on zenprocess/uniforme@8eadd14
```

Reproduction rates from the poll log:
- `os error 11` (this signature): **1092** occurrences
- `restore_many 400` (snapshot-restore, fabro-123): **12** occurrences
- Ratio: **~91:1** — exec-stage EAGAIN dominates by two orders of magnitude

The failure class is **intermittent**, not deterministic — the post-audit
brokered probe on the same controller returned `exec exit 0`. A deterministic
infra failure would never see a green; an intermittent one is consistent with
a transient race in a non-blocking read path, NOT with a dead host.

## 2. Leading HYPOTHESIS (controller-side)

> **HYPOTHESIS.** The forkd controller's exec-response read loop reads
> from a non-blocking fd (vsock or socket) without an EAGAIN-retry-with-
> deadline. When the fd is temporarily not-ready (a normal non-blocking
> condition), `read()` returns `EAGAIN` (Linux errno 11), and the
> controller's read path treats that as fatal — returning `HTTP 500
> {"error":"exec: read response: Resource temporarily unavailable
> (os error 11)"}` to the gate's `forkd-shim.py`. The fix is to retry
> the read on `EAGAIN` with a poll/select deadline bounded by the
> existing `FABRO_EXEC_TIMEOUT` (default 500 s). The fact that the
> error text reads like a verbatim Go `os.PathError` string (`os error
> 11`) is the strongest textual evidence — Go's `os` package returns
> exactly that wording for `Errno(11)`.

**Why this is HYPOTHESIS, not confirmed:**

1. The controller source is not in this repo. Any "fix" written here
   touches the wrong side — the controller is on `dellsrv` (behind the
   egress boundary from this sandbox) and is not the operator of any
   project under `~/ao-projects/`.
2. The error string is consistent with the hypothesis but does not
   exclude alternatives (an unhandled `io.EOF` that was misreported;
   a kernel-level vsock backpressure timeout; a transient `ENOBUFS`).
3. No live trace from the controller side was inspected; the only
   evidence is the gate-side error text and the client-side behavior.

**What would promote HYPOTHESIS → confirmed:**

- Operator runs `strace -f -e read,recvmsg -p $(pgrep -f forkd)` on
  the dellsrv controller during one EAGAIN-500 emission, and observes
  `EAGAIN (Resource temporarily unavailable)` on a `read()` of a
  vsock or unix socket with no retry loop.
- OR: operator adds a single retry-on-EAGAIN to the controller and
  the 1092-occurrence class drops to zero within a rolling-48h window.
  The runbook in §2 of `forkd-snapshot-registry-runbook.md` covers
  the operator-side controller patch shape (no actual patch here —
  the controller source must be edited on dellsrv, inside a
  `zenctl maint on <ttl> <reason>` window per `T3` discipline).

## 3. Client-side defense in depth (what this box CAN do)

### 3a. Rust client retry — already in place (verified by reading source)

In `lib/crates/fabro-sandbox/src/forkd/mod.rs::ForkdSandbox::exec_in_sandbox`
(lines 337-401 on fabro main at PR-review time):

```rust
const HTTP_RETRY_LIMIT: u32 = 3;
const HTTP_RETRY_INITIAL_BACKOFF: Duration = Duration::from_millis(250);

async fn exec_in_sandbox(&self, args: Vec<String>, timeout_secs: u64)
    -> crate::Result<ExecResponse>
{
    ...
    loop {
        let result = client.post(&url)...send().await;
        match result {
            Ok(resp) if resp.status().is_success() => { return Ok(...); }
            Ok(resp)
                if Self::is_retryable_status(resp.status())
                    && attempt < HTTP_RETRY_LIMIT => { ... retry ... }
            Ok(resp) => return Err(...),
            Err(e) if e.is_connect() && attempt < HTTP_RETRY_LIMIT => { ... }
            Err(e) => return Err(...),
        }
    }
}
```

`is_retryable_status` is `status.is_server_error()` — i.e., any 5xx (which
includes HTTP 500) is retried up to 3 times with 250 ms exponential backoff
(capped at 10 s). The observed HTTP 500-with-EAGAIN-body signature is
covered by this branch: a single transient EAGAIN that lands as a 500 gets
retried 3 times before being surfaced as fatal.

This means a transient EAGAIN lasting <~1.25 s (250 + 500 + 1000 ms backoff
between 3 attempts, all within `FABRO_EXEC_TIMEOUT=500` s) is already
absorbed client-side. The remaining observed rate of `os error 11` infra
verdicts is therefore the **tail** beyond the client's retry envelope
(either the EAGAIN persists longer than the backoff schedule, or many
EAGAINs interleave across polling cycles).

### 3b. Gate-side deferral — added by fabro-122

In `bin/fabro-github-gate.sh` (ao-company, branch `ao/fabro-123/snapshot-preflight`):

- A new helper `is_eagain_500_reason` matches the EAGAIN-500 signature
  in the shim's infra emission.
- A per-SHA cycle budget (`FABRO_EAGAIN_MAX_RETRIES`, default 3) lives at
  `~/.ao/state/fabro-gate-eagain.json` and bounds the deferral to ~15 min
  at the launchd 5-min cadence.
- Inside `cmd_gate`, an infra outcome that matches the EAGAIN-500 signature
  **and** has budget remaining is deferred (no GitHub `error` post, return
  code 4, log marker) so the next launchd poll cycle re-gates the SHA.
- When the budget is exhausted, the verdict falls through to the
  ordinary `post_infra` path — the three-outcome contract is preserved
  (`success | failure | infra`); the no-overwrite guard on prior
  success|failure still applies.
- `self-test` extended to cover the classifier + the bounded-defer
  contract (`bash bin/fabro-github-gate.sh self-test` PASS).

### 3c. Probe-side classification — extended by fabro-71

In `scripts/ops/dellsrv-forkd-supervision/gate-health-probe.sh`
(fabro-71, on this branch):

- The exec round-trip (already exercised by the probe since the 2026-07-22
  netns fix; see `fec175a65 ops(dellsrv): gate-health probe for per_child_netns=true path`)
  now ALSO inspects the raw exec response for the EAGAIN signature and
  emits a structured `FORKD-GATE-ALERT reason=exec_eagain_500` marker
  BEFORE the existing `exec_nonzero_exit` alert.
- Monitoring can split the failure class from generic exec failures
  via `journalctl -t gate-probe | grep exec_eagain_500`, satisfying
  the "caught by monitoring, not by users" requirement.
- The probe change is local + hermetic (no token or argv changes); it
  reuses the existing `in_container_curl`, `alert`, and `teardown_sandbox`
  infra.

## 4. What this PR does NOT do

- **No controller edits.** The controller source is not local. Any
  fix on this side would touch the wrong code (the gate and probe are
  already mitigating what they can; the controller is the upstream
  cause).
- **No live canary loop.** A 20-iteration create+exec+delete loop on
  dellsrv:8891 would prove or refute the hypothesis — but dellsrv
  is behind the egress boundary from this sandbox. The operator runs
  that, with the exact commands in
  `docs/internal/forkd-snapshot-registry-runbook.md` §2.
- **No new persistent state in production.** The
  `~/.ao/state/fabro-gate-eagain.json` counter is the only new file,
  written atomically (`mktemp + os.replace`) and bounded by the
  per-SHA budget.

## 5. Acceptance evidence (local-only)

- `bash bin/fabro-github-gate.sh self-test` → **PASS** (extended suite
  covers classifier + per-SHA budget deferral + counter persistence).
- `bash scripts/ops/dellsrv-forkd-supervision/gate-health-probe.sh --dry-run`
  → prints the (offline) plan without touching the controller.
- Live evidence (poll-log zero-rate, canary pass, controller `ps`)
  requires an operator on dellsrv and is explicitly handed off.

---

Co-Authored-By: Claude <noreply@anthropic.com>
