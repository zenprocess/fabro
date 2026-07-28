# dellsrv forkd gate-health probe — operator runbook

This directory holds the operator-executed **gate-health probe** for
the **dellsrv** forkd VMM gate. It is designed for offline authoring
review and requires no live credentials to validate as a script
(shellcheck-clean, `--dry-run` supported).

Nothing here is run from the Mac. The probe is committed here so a
human operator can `git pull`, read the code, and execute on dellsrv
with eyes on the actual commands.

## Scope

This runbook covers **only** the gate-health probe. The previous
supervision unit (`forkd-controller.service`) and golden-tag
re-register script were authored in an earlier, closed PR
(`#16`) and are not part of this PR's scope — `forkd-ec.service`
already supervises the boot script, and the golden option-2
re-base is already live on dellsrv.

## Files

| Path | Role |
|---|---|
| `gate-health-probe.sh` | Exercises the real `per_child_netns=true` sandbox path end-to-end. Default: alert-only. `--heal` for auto-repair. |
| `exec-eagain-control.sh` | Added 2026-07-28 for the active EAGAIN-at-exec outage (`POST /v1/sandboxes/{id}/exec -> HTTP 500 "Resource temporarily unavailable"`). Runs one trivial `echo hi` exec on a chosen `--tag` to split the failure into deterministic/global vs. workload-dependent. `--diagnose` adds a strictly read-only one-pass diagnostic capture (controller/proxy state, orphans, limits, netns, disk/memory). See its header comment for the full decision tree. |
| `gate-health-probe.service` | systemd one-shot service for the probe. Independent of `forkd-ec.service`. |
| `gate-health-probe.timer` | Periodic trigger (5 min, `Persistent=true`, `AccuracySec=60s`). |

---

## Why this exists

On **2026-07-22** the fabro gate went silent for three days. The
per-child network namespaces
(`/var/run/netns/forkd-child-{1,2,3}`) died without the forkd
container restarting. Firecracker's console emitted:

```
setting the network namespace "forkd-child-1" failed: Invalid argument
```

and the sandbox-create path returned:

```
restore_many: socket /tmp/forkd-daemon-<tag>-o0/child-1.sock never appeared within 5s
```

`forkd-shim.py` injects `per_child_netns=true` on every
sandbox-create, so **every** gate exec failed at infra level. The
gate correctly suppressed the verdict rather than posting a wrong
one — so the failure mode is silence, not noise. That is the worst
possible signal: it looks healthy because nothing is wrong enough to
be scored.

The netns is provisioned only by `forkd-ec-boot.sh` at container
start. Nothing probes them. This probe is that probe.

## What it does

1. `POST /v1/sandboxes` with **`snapshot_tag=zen-gate-base`** and
   `per_child_netns=true`. **The `true` is load-bearing** — a probe
   using `false` would have passed throughout the entire outage and
   is worthless. The JSON key is `snapshot_tag` (NOT `tag`) —
   verified live on dellsrv 2026-07-25: sending `{"tag":...}` returns
   `missing field snapshot_tag`, which does NOT match the netns
   failure regex and would silently kill the probe with a wrong
   reason on every run.
2. **Defensively scrape the sandbox id immediately** from the
   create response. Verified live 2026-07-25: the response is a
   JSON ARRAY of one object, e.g.
   `[{"id":"sb-...","snapshot_tag":"...","netns":"forkd-child-1",...}]`,
   so the probe queries `.[0].id` (not `.id`). The `scrape_field`
   helper falls back to alternative field names and a regex escape
   hatch so the EXIT trap can always clean up if the controller
   actually created a sandbox. **This ordering is load-bearing** —
   a probe that parsed the id AFTER the netns check leaked a live
   microVM (sb-6a64b43f-0031) on every failed run.
3. Inspect the response for the netns-failure signature
   (`Invalid argument` / `socket never appeared`). If matched,
   alert (or, with `--heal`, attempt the documented repair).
4. `POST /v1/sandboxes/{id}/exec` with body
   `{"args":["/bin/true"]}` and assert `exit_code == 0`. Same
   array-aware parse as create.
5. `DELETE /v1/sandboxes/{id}` — **always**, even on prior failure.
6. Exit 0 on success, non-zero with a precise, greppable reason on
   any failure.

## Heal vs alert — design decision

**Default: alert-only. Opt-in: `--heal` for auto-repair.**

I chose this because auto-heal that masks a recurring fault is its
own hazard. The original outage lasted three days because nothing
*told anyone* — a probe that quietly fixes the problem and continues
on its way leaves the operator with the same blind spot, just with
the additional confusion of "but the probe says everything is fine."
We need the signal.

In `--heal` mode the probe still records its actions
(`FORKD-GATE-HEAL reason=netns_failure_signature_detected` is
emitted to stderr **before** the repair runs, and a second
`FORKD-GATE-HEAL completed teardown_and_setup` line is emitted
after). Even auto-heal cannot be silent. Operators can count heals
in `journalctl -t gate-health-probe` and decide whether the
underlying issue is worth fixing.

**Operationally:** start with the default (alert only). After you
trust the netns path — and after one or two confirmed heals — you
may opt in by editing
`/etc/systemd/system/gate-health-probe.service` to pass `--heal`.
The change is one line and reversible.

## Token handling

The token at `/etc/forkd-token` lives INSIDE the `forkd` docker
container. The probe wraps every API call in
`docker exec -i forkd sh` with a heredoc body whose first action is
`TOKEN=$(cat /etc/forkd-token)` — the token value is only ever a
shell variable inside the container's `sh`. The host process table
never sees the token. The script's `in_container_curl` helper is
the only path that touches the controller.

## Deploy (on dellsrv, in order)

```bash
sudo install -m 0755 scripts/ops/dellsrv-forkd-supervision/gate-health-probe.sh \
    /usr/local/sbin/gate-health-probe.sh
sudo install -m 0644 scripts/ops/dellsrv-forkd-supervision/gate-health-probe.service \
    /etc/systemd/system/gate-health-probe.service
sudo install -m 0644 scripts/ops/dellsrv-forkd-supervision/gate-health-probe.timer \
    /etc/systemd/system/gate-health-probe.timer
sudo systemctl daemon-reload
sudo systemctl enable --now gate-health-probe.timer
sudo systemctl list-timers gate-health-probe.timer
```

The probe will run 1 minute after boot, then every 5 minutes.

## One-shot manual run

```bash
sudo /usr/local/sbin/gate-health-probe.sh
# or, with auto-heal on netns failure:
sudo /usr/local/sbin/gate-health-probe.sh --heal
# or, dry-run:
sudo /usr/local/sbin/gate-health-probe.sh --dry-run
```

## exec-eagain-control.sh — the EAGAIN-at-exec outage control experiment

Added 2026-07-28 for an active outage distinct from the netns failure above:
`POST /v1/sandboxes/{id}/exec` returning `HTTP 500 "exec: read response: Resource
temporarily unavailable"` (EAGAIN), 182 occurrences, zero successful gate runs on
the affected repo. Unlike the netns failure, sandbox **create** succeeds — only the
exec response read fails.

This script is not a continuous probe (no `.timer` — it is a diagnostic, run once
per investigation). It exists to answer one question before anyone reaches for a
fix: **is this deterministic (fires on any command) or workload-dependent (fires
only on heavier/longer/larger-output commands)?** Those two answers point at
opposite remediations — see the script's header comment for the full reasoning,
including the specific code path this investigation flagged in
`~/fabro-run/forkd-shim.py`'s reverse-proxy retry logic (sandbox-create gets
retries on a known transient signature; every other request shape, including
exec, gets exactly one attempt).

```bash
# Run against the tag the gate actually uses for the affected repo:
sudo scripts/ops/dellsrv-forkd-supervision/exec-eagain-control.sh --tag zen-gate-big

# Also capture read-only diagnostics in the same pass:
sudo scripts/ops/dellsrv-forkd-supervision/exec-eagain-control.sh --tag zen-gate-big --diagnose

# dry-run:
sudo scripts/ops/dellsrv-forkd-supervision/exec-eagain-control.sh --dry-run
```

Read the `CONTROL-RESULT:` and `EXEC-ELAPSED:` lines on stdout — everything else is
diagnostic logging on stderr. As with `gate-health-probe.sh` below, **do not pipe
this script** if you care about its exit code.

## Independence from forkd-ec.service

The probe's service unit has a **soft** `After=forkd-ec.service`
ordering for boot-time stability but **no `Requires=`** on it. The
probe must keep running when the thing it watches is broken —
that's how we get an alert. Coupling the canary to the supervised
process would defeat the purpose.

## Reading the alert

```bash
# All probe output, including successes:
journalctl -t gate-health-probe -f

# Just the failure-path lines:
journalctl -t gate-health-probe | grep FORKD-GATE-ALERT

# Just the heal events (only emitted with --heal):
journalctl -t gate-health-probe | grep FORKD-GATE-HEAL
```

## Exit code semantics

The systemd service invokes the probe **directly** — `ExecStart=
/usr/local/sbin/gate-health-probe.sh`, no shell, no pipe, no
`tee`. So a probe failure (`die`) propagates as a non-zero unit
exit, which systemd records as a failed `gate-health-probe.service`
activation. The timer fires the unit again on its next scheduled
interval; the unit is `Type=oneshot` so reactivation is cheap and
correct.

**Do not pipe the probe manually if you care about its exit code.**
If you invoke the script as `probe.sh | tee log.txt` or
`probe.sh | grep something`, `$?` reflects the last command in the
pipeline (typically `tee` or `grep`), not the probe. To preserve
the exit code while still logging output, use `set -o pipefail` in
a wrapper, or invoke via systemd where the direct exec path is
guaranteed. The timer + service pair is the supported way to run
this; ad-hoc shell pipelines are not.

## Rollback

```bash
sudo systemctl disable --now gate-health-probe.timer
sudo systemctl stop gate-health-probe.service  # idempotent if idle
sudo rm /etc/systemd/system/gate-health-probe.{service,timer}
sudo systemctl daemon-reload
```

This stops the periodic probe ONLY. The forkd controller and
`forkd-ec.service` are unaffected.
