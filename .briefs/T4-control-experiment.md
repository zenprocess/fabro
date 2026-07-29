# TASK T4 — author the CONTROL EXPERIMENT + one-pass diagnostic script

**Authoring only. NO live execution.** You cannot reach `dellsrv.zp.digital` (DNS
denial = egress boundary working as designed). Do NOT attempt any workaround, do not
disable the sandbox. Someone with host access runs what you write.

## Why this is the critical path

An active production outage: every fabro gate run dies at
`POST /v1/sandboxes/<id>/exec -> HTTP 500 "exec: read response: Resource temporarily
unavailable (os error 11)"` (EAGAIN). 182 occurrences, 218 INFRA / 10 FAIL / **ZERO
PASS**.

The hypothesis space splits cleanly in half and **one experiment resolves it**:

- If a **trivial** exec (`/bin/true`) on the **same snapshot** ALSO EAGAINs →
  the fault is **deterministic/global**. Workload, memory size, Chromium, and npm are
  all irrelevant, and retry/backoff would be useless (it would just fail N times).
- If the trivial exec **passes** → the fault is **workload/output-size/duration
  dependent**, and retry+backoff at the exec hop becomes the leading fix.

Everything downstream depends on this single result. Your script must produce it
unambiguously.

## Deliverable

`scripts/ops/dellsrv-forkd-supervision/exec-eagain-control.sh` in the fabro repo,
plus a section in that directory's existing `README.md`. Branch off `origin/main`,
commit, push, open a **DRAFT** PR against `zenprocess/fabro`.

## Part 1 — the control experiment (the point of the script)

Run the SAME create → exec → delete cycle the gate uses, but with a trivial command:

1. `POST /v1/sandboxes` with `{"snapshot_tag": "<TAG>"}` — take the tag as `--tag`,
   defaulting to `zen-gate-base`. Also support `--tag zen-gate-big` so both can be
   tested (uniforme pins `zen-gate-big`).
2. `POST /v1/sandboxes/{id}/exec` with `{"args":["/bin/true"]}` and a short
   `timeout_secs`.
3. `DELETE /v1/sandboxes/{id}` — **always**, via an EXIT trap, even on failure.
   A prior probe leaked a live microVM on every failed run; do not repeat that.
4. Print an unmistakable verdict line:
   - `CONTROL-RESULT: PASS` — trivial exec succeeded → fault is workload-dependent
   - `CONTROL-RESULT: EAGAIN` — trivial exec hit the same error → fault is global
   - `CONTROL-RESULT: OTHER <detail>` — anything else; do not force it into a bucket

**Capture the exit code correctly.** Do NOT pipe the script's own invocation —
`script.sh | tee log` reports `tee`'s status. Note this in the README.

**Timing matters and is evidence**: record and print how long the exec call took
before failing. An EAGAIN returned in <1s implies a non-blocking fd with no poll/retry
loop (a code defect); one returned after ~120s implies a timeout. Print
`EXEC-ELAPSED: <seconds>`.

## Part 2 — one-pass diagnostic capture (so there is no round-trip per command)

Strictly read-only. Each check prints `CHECK <name> <OK|FAIL|UNKNOWN> <detail>`, each
independently failable so one missing binary does not abort the rest:

- `systemctl is-active/is-enabled forkd-ec.service`; `journalctl -u forkd-ec.service -n 100`
- **Was the controller restarted?** Process start time vs. snapshot registry state.
  In-memory snapshots are lost on restart and must be rebuilt — check
  `GET /v1/snapshots` and list what is actually registered.
- `GET /v1/sandboxes` — orphan/leak count.
- fd and process limits: `ulimit -n`, `pids.current` vs `pids.max` (cgroup), `ps -eLf | wc -l`
- `dmesg -T | grep -iE 'oom|firecracker|cannot allocate'`
- `df -h` on the VM store / snapshot root — **disk exhaustion also surfaces as EAGAIN-adjacent failures**
- `free -g`, `docker stats --no-stream`
- per-child netns presence: `/var/run/netns/forkd-child-*`
- **The proxy hop**: is `~/fabro-run/forkd-shim.py` running, on which port, and what is
  `FORKD_SHIM_FORWARD_TIMEOUT_S` set to in its environment?

## Secrets discipline

The controller token lives at `/etc/forkd-token` **inside the `forkd` container**.
Follow the existing pattern in `gate-health-probe.sh`: wrap calls in
`docker exec -i forkd sh` with a heredoc whose first action is
`TOKEN=$(cat /etc/forkd-token)`, so the value never crosses the host argv or process
table. Report presence/absence only — **never print a token value**.

## Acceptance command (must pass; paste real output)

```
bash -n scripts/ops/dellsrv-forkd-supervision/exec-eagain-control.sh \
  && shellcheck -S warning scripts/ops/dellsrv-forkd-supervision/exec-eagain-control.sh \
  && grep -q 'CONTROL-RESULT' scripts/ops/dellsrv-forkd-supervision/exec-eagain-control.sh \
  && grep -q 'EXEC-ELAPSED' scripts/ops/dellsrv-forkd-supervision/exec-eagain-control.sh \
  && ! grep -nE '(systemctl (start|stop|restart|enable|disable))|(docker (restart|rm|stop))' scripts/ops/dellsrv-forkd-supervision/exec-eagain-control.sh \
  && echo ACCEPT
```

The negated grep is the read-only proof for Part 2 — it must find NOTHING. (The
control experiment's own create/exec/DELETE calls are intentional and are HTTP, not
those verbs.)

## Adversarial check (required)

Prove the acceptance chain actually bites: on a **scratch copy**, inject
`systemctl restart forkd-ec.service`, re-run the chain, confirm it FAILS; remove it,
confirm PASS. Paste both outputs. A guard you never saw fail is not a guard.

Then: verify your EXIT trap actually fires on the failure path — simulate an exec
failure and confirm the DELETE still runs. A cleanup path that only works on success
is the leak bug all over again.

## Report back

File path, draft PR number, real acceptance output, and the two adversarial proofs.
Verify your push by checking the **remote tip SHA**, not an echoed "pushed" — a
`git push origin <branch>` can succeed as a no-op while your commit sits on a detached
HEAD. Do NOT report done without pasted output.
