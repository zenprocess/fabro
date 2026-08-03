# forkd patch notes — honest exec-deadline reporting (internal)

> **Internal-only.** Status: P0 step 2 of the RCA in
> ao-company issue #201. Patch is on a local fork of
> `github.com/deeplethe/forkd` at `c659539` (pinned upstream `main`).
> See [forkd-upstream-draft.md](./forkd-upstream-draft.md) for the
> sanitized upstream contribution draft (NOT POSTED, filing is
> operator-gated).

## TL;DR

Two coordinated changes against forkd `c659539` (pinned upstream
`main`):

1. **Controller half** — `forkd_vmm::exec_at` now distinguishes a
   read-deadline timeout from other I/O errors and returns a typed
   `ExecDeadlineExceeded { elapsed, requested_secs }`. The HTTP
   layer surfaces this as **HTTP 408 Request Timeout** with a JSON
   body whose `error` field contains the literal marker
   `exec deadline exceeded` and the elapsed seconds. The previous
   behavior was a generic 500 with body
   `exec: read response: Resource temporarily unavailable`.
2. **Guest half** — `rootfs-init/forkd-agent.py` now spawns exec
   victims with `start_new_session=True` and uses `os.killpg(SIGKILL)`
   on timeout, breaking the "ProcessGroup-Hang" mechanism that
   pin's `communicate()` on grandchildren-inherited pipes. Also
   subtracts a 10s `GUEST_TIMEOUT_MARGIN_SECS` from the inner
   subprocess timeout so the guest's reply lands inside the
   controller's `timeout_secs + 5` read window.

**Critical deployment fact**: the guest half lives in the SNAPSHOT
(`rootfs-init/forkd-agent.py` is copied into the rootfs by
`scripts/build-rootfs.sh` and runs as PID 1 inside the microVM).
**A controller-only deploy silently does nothing for the guest half.**
The guest fix takes effect ONLY after a snapshot re-bake. For the
zen-gate-big chain, that re-bake is also the right moment to pick
up the v0.5.1 entropy-fixed kernel (separate change; not in scope
here).

## Where the patch lives

- Local clone: `~/ao-projects/fabro/.scratchpad/forkd-probe` (see
  workflow-local at `902404f7-eb4d-5d06-a080-8f92420a760e`).
- Branch: `fix/exec-deadline-honest-reporting` from `c659539`
  (pinned upstream `main`).
- Commits (top of branch):
  - `804ccee` — `docs(changelog): add guest-half entry for honest
    exec-deadline reporting`
  - `9b624fd` — `fix(guest-agent): process-group kill +
    guest-timeout-wins margin on exec`
  - `b842726` — `fix(controller): report exec deadline honestly as
    HTTP 408 with elapsed seconds`
- **The patch is not pushed to any remote.** The host's
  `GITHUB_TOKEN` is invalid in this session
  (`gh auth status` reports `Failed to log in to github.com using
  token (GITHUB_TOKEN)` and the `vvladescu-tb` fallback is also
  inactive). The operator must push from a network-allowed
  environment to a fork of `deeplethe/forkd` (the upstream
  filing itself is T3-gated; see checkout field notes below).

## Wire contract (the bit a parallel worker is matching on)

The agreed wire contract for the upstream classifier:

- `POST /v1/sandboxes/:id/exec` returns **HTTP 408 Request Timeout**
  when the controller's blocking read of the guest agent's reply
  hits its `timeout_secs + 5` deadline.
- The body is `application/json` with a single `error` string
  field. The string contains:
  - the literal substring `exec deadline exceeded`, AND
  - the elapsed seconds (wall-clock since the controller handed
    the request to the OS read).
- Example body:
  ```json
  { "error": "exec deadline exceeded after 505.3s (requested timeout_secs=500)" }
  ```
- Everything else on the route is unchanged: 200 success and 500
  for non-deadline errors (parse / connection refused / etc.) keep
  their existing shape.

## Re-bake requirement (the part a controller-only deploy misses)

The guest half is shipped inside the rootfs. Concretely:

- `rootfs-init/forkd-agent.py` is copied into the rootfs by
  `scripts/build-rootfs.sh` and launched as PID 1 by
  `rootfs-init/forkd-init.sh` after the kernel mounts
  `/proc`, `/sys`, `/dev`.
- Once a snapshot is baked from a given rootfs, that snapshot's
  `forkd-agent.py` is frozen for the lifetime of the snapshot —
  re-deploying the controller container does not change it.
- So the operator must run, AT MINIMUM:
  1. Build a new rootfs (the upstream `scripts/build-rootfs.sh`).
  2. Re-bake `zen-gate-big` (and any other snapshot consumers
     that want the fix) from the new rootfs.
  3. Optionally: also pick up the v0.5.1 entropy-fixed kernel
     (`scripts/install-guest-kernel.sh`) at the same time —
     that's a separate concern but the maintenance window is
     the same.
- The controller-side change is independent and can be deployed
  first; it is silent-no-op against an old guest (the typed
  `ExecDeadlineExceeded` would simply never be raised because
  the old guest never sends a reply past the controller's
  deadline).

## Verification status — required reading

The patch was authored in a sandbox where cargo's registry cache
write is blocked (`Operation not permitted (os error 1)` on
`~/.cargo/registry/cache/index.crates.io-...`). Therefore:

- **Rust patch is UNVERIFIED at build time.** No `cargo check -p
  forkd-vmm -p forkd-controller` was run. The operator MUST run
  both before merging.
- **Python patch is verified at py_compile time** (`python3 -m
  py_compile rootfs-init/forkd-agent.py` → OK). It is UNVERIFIED
  at runtime — the ProcessGroup-Hang mechanism requires a real
  guest VM to demonstrate end-to-end.
- **HTTP smoke test is UNVERIFIED.** No live forkd controller is
  reachable from this session (`*.zp.digital` and the
  `10.0.201.0/24` block are behind the egress boundary by
  design). The operator MUST run a 3-5 exec probe against the
  staged forkd instance per ao-company issue #201's ranked plan
  item 1 (which already exists as `exec-eagain-control.sh` from
  PR #21).

Concrete commands the operator should run (UNVERIFIED here):

```bash
# In an environment with cargo + crates.io access:
git fetch https://github.com/deeplethe/forkd.git c659539
git checkout -b fix/exec-deadline-honest-reporting c659539
# (cherry-pick or apply the three commits above)
cargo check -p forkd-vmm -p forkd-controller
cargo test  -p forkd-vmm  -p forkd-controller

# In an environment with a live forkd controller + zen-gate-big:
bash exec-eagain-control.sh --snapshot zen-gate-big --timeout 1800 \
  --runs 5 --instrumented
# Expected after fix: timeouts (if any) come back as 408 with
# "exec deadline exceeded"; no 500 with EAGAIN.
```

## Boundaries honoured

- No deploy performed. No `kubectl`, no `docker restart`, no
  `zenctl maint` calls.
- No re-bake performed. No `scripts/build-rootfs.sh` invocation.
- No upstream issue filed. The sanitized draft is staged at
  `docs/internal/forkd-upstream-draft.md` and explicitly
  marked `NOT POSTED`.
- No live forkd controller probed. The `*.zp.digital` and
  `10.0.201.0/24` egress boundary is treated as a hard no-fly
  per the sandbox policy.
- No edits to `~/.ao-mac/fabro-github-gate.sh` or
  `~/Desktop/ao-company` (parallel worker owns the gate script).
- No repo-wide build. Only the two touched forkd crates would
  be in scope, and even those weren't built (registry-blocked).

## Files touched (this session)

| File | Change |
|---|---|
| `scratchpad/forkd-probe/crates/forkd-vmm/src/lib.rs` | New `ExecDeadlineExceeded` typed error; `exec_at` detects `WouldBlock` / `TimedOut` and returns it. |
| `scratchpad/forkd-probe/crates/forkd-controller/src/http.rs` | `exec_sandbox` `downcast_ref`s `ExecDeadlineExceeded` and emits 408 with the deadline body. |
| `scratchpad/forkd-probe/rootfs-init/forkd-agent.py` | New `_handle_exec` + `_reap` helpers; process-group kill on timeout; 10s `GUEST_TIMEOUT_MARGIN_SECS`. |
| `scratchpad/forkd-probe/CHANGELOG.md` | Two `Unreleased` entries: controller half and guest half. |
| `docs/internal/forkd-upstream-draft.md` | Sanitized upstream contribution draft, NOT POSTED. |
| `docs/internal/forkd-patch-notes.md` | This file. |

## Out-of-scope items (called out so the orchestrator sees them)

- **Poller reconcile** (ao-company issue #201 ranked plan item 4):
  the deployed `~/.ao-mac/fabro-github-gate.sh` contains a newer
  two-pass `cmd_poll` that exists nowhere on `origin/main`. This
  is owned by the parallel worker on the gate script; not touched
  here.
- **OOM / npm-cache findings** (issue #201 verifier correction
  #2): even completed runs show exit-1, and the suite is
  genuinely red in the gate env. That is a separate uniforme
  problem, handed to the uniforme orchestrator separately.
- **0.5.3 upgrade rejection** (issue #201 ranked plan item 5):
  the v0.5.3 tag touches nothing in the exec path. Build from
  pinned upstream `main` (picks up unreleased #269/#272) instead.
- **Snapshot re-bake**: pending T3 operator action inside a
  `zenctl` maintenance window.
- **Upstream filing**: pending T3 operator action.
- **Pushing the forkd branch**: pending network-allowed
  environment with a valid `GITHUB_TOKEN`.
