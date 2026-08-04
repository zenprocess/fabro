# forkd mode-B fix — local verification report (fabro-108, 2026-08-04)

**Issue:** ao-company #201 (mode B — mis-reported exec timeout)
**Branch:** `ao/fabro-108/forkd-mode-b`
**Upstream pin:** c6595399+
**Patch source:** `ops/fabro/forkd-patches/exec-deadline-honest-reporting/forkd-exec-deadline.patch` (3 commits, 520 lines)

## What ships in this PR

1. **The existing patchset, RECONCILED against pinned upstream `c659539+`.** All hunks apply cleanly via `git apply --check` and `git apply`. No rewrite from scratch per the brief.
2. **`ops/fabro/forkd-patches/exec-deadline-honest-reporting/test-forkd-agent.py`** — two commit-litmus unit tests for the Python half (rootfs-init/forkd-agent.py), offline:
   - **test_408_wire_contract_marker**: the reply on timeout MUST contain the literal `exec deadline exceeded: timeout_secs=N` in the `error` field. FAILS if the wire-contract marker is removed.
   - **test_process_group_kill_grandchild**: a grandchild of a timed-out exec MUST be killed (no zombie leak). FAILS if the patch is reverted to `subprocess.run(timeout=...)` without `start_new_session=True` + `os.killpg(SIGKILL)`.
3. **A reconciliation note** (`ops/fabro/forkd-patches/exec-deadline-honest-reporting/fabro-108-reconciles.md`) stating which hunks still apply and which needed rework (per the brief: "say clearly which hunks still apply and which needed rework").

## Local verification results (this session, 2026-08-04)

| box | command | result |
|---|---|---|
| patch applies | `git apply --check forkd-exec-deadline.patch` against c659539+ | clean (no errors) |
| patch applies | `git apply forkd-exec-deadline.patch` (with `--check` validated first) | clean (no errors) |
| python compiles | `python3 -m py_compile rootfs-init/forkd-agent.py` | OK |
| test 1: wire contract | `python3 test-forkd-agent.py test_408_wire_contract_marker` | PASS — `error='exec deadline exceeded: timeout_secs=1'` |
| test 2: process-group kill | `python3 test-forkd-agent.py test_process_group_kill_grandchild` | PASS — grandchild pid is dead after timeout |

## What is UNVERIFIED in this session (and why)

| item | reason | operator action required |
|---|---|---|
| Rust patch builds (`cargo check -p forkd-vmm -p forkd-controller`) | sandbox blocks writes to `~/.cargo/registry/cache/` | run `cargo check` and `cargo test` in a network-allowed environment |
| Patch applies to a newer upstream commit (beyond c659539) | not tested (brief said build from `c659539+`, not the latest main) | operator may evaluate the pin to main HEAD and report any further reconciliation |
| Live HTTP smoke test (POST /v1/sandboxes/:id/exec returns 408 with the marker) | no live forkd controller is reachable from this session | operator runs `exec-eagain-control.sh --snapshot zen-gate-big --timeout 1800 --runs 5 --instrumented` per #201 step 1 |
| Snapshot re-bake (guest half lives in the rootfs) | T3 (operator) | operator runs `scripts/build-rootfs.sh` and re-bakes `zen-gate-big` |
| The "EAGAIN 500" classifier in the deployed gate-health-probe | patch is wire-contract compatible but the classifier text still matches the old 500+EAGAIN signature | operator updates the classifier to also recognize the new 408+marker signature (per #201 rank 4 — error-text coupling) |

## Boundaries honoured

- No deploy performed. No `kubectl`, no `docker restart`, no `zenctl maint` calls.
- No re-bake performed. No `scripts/build-rootfs.sh` invocation.
- No upstream issue filed at `deeplethe/forkd` (the sanitized draft is staged at `ops/fabro/forkd-patches/exec-deadline-honest-reporting/forkd-upstream-draft.md`; filing is operator-gated per #201 T3 items).
- No reachability to the gate host was sought. The sandbox's gate-host block was respected.
- No use of the `zen-gates-*` broker for verification (failing 7/8 with `Session not found`, filed as #304). `gh` commit-status evidence is the documented evidence path going forward.
