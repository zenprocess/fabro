# dellsrv forkd supervision — operator runbook

This directory holds operator-executed deliverables for the **dellsrv**
gate host. They are designed for offline authoring review and require
no live credentials to validate as scripts (they are
shellcheck-clean and `--dry-run` is supported where it makes sense).

Neither script is run from the Mac. They are committed here so a
human operator can `git pull`, read the code, and execute on dellsrv
with eyes on the actual commands.

---

## Files

| Path | Role |
|---|---|
| `forkd-controller.service` | systemd unit. Supervises the boot **script**, NOT the raw binary. |
| `forkd-controller-supervisor.sh` | systemd `ExecStart` target. Runs the boot script and blocks on `/v1/health`. |
| `re-register-golden-tags.sh` | Re-registers `zen-gate-base` (1024 MiB) and `zen-gate-big` (4096 MiB) off the same existing 20GB rootfs. |
| `gate-health-probe.sh` | Exercises the real `per_child_netns=true` sandbox path end-to-end. Default: alert-only. `--heal` for auto-repair. |
| `gate-health-probe.service` | systemd one-shot service for the probe. Independent of `forkd-ec.service`. |
| `gate-health-probe.timer` | Periodic trigger (5 min, `Persistent=true`, `AccuracySec=60s`). |

---

## Execution order

### 1. Install the supervision unit

On dellsrv, as root:

```bash
sudo install -m 0755 scripts/ops/dellsrv-forkd-supervision/forkd-controller-supervisor.sh \
    /usr/local/sbin/forkd-controller-supervisor.sh
sudo install -m 0644 scripts/ops/dellsrv-forkd-supervision/forkd-controller.service \
    /etc/systemd/system/forkd-controller.service

# 0600 token file. Populate from your secret store; do NOT inline.
sudo install -m 0600 -o root -g root /dev/null /etc/forkd/forkd.env
sudo ${EDITOR:-vi} /etc/forkd/forkd.env   # add: FORKD_TOKEN_FILE=/etc/forkd-token

sudo systemctl daemon-reload
sudo systemctl enable --now forkd-controller.service
sudo systemctl status forkd-controller.service
```

The `enable --now` step does **not** start a second controller. The
unit owns `forkd-ec-boot-dellsrv.sh`, which is idempotent and only
brings up the `forkd` docker container if it is not already up.

### 2. Verify the gate is healthy

```bash
curl -fsS http://127.0.0.1:8891/v1/health
```

If this returns 200, the supervision unit is wired correctly. The
existing manual `docker exec` flow is unchanged from the container's
point of view.

### 3. Re-register the golden tags

Pick the existing 20 GB rootfs and kernel paths on dellsrv. They are
the same artifacts the previous (broken, 512 MiB) `zen-gate` tag was
built from — this script does **not** rebuild them.

```bash
# First, dry-run to print the plan:
sudo scripts/ops/dellsrv-forkd-supervision/re-register-golden-tags.sh \
    --rootfs /var/lib/forkd/golden/rootfs-20g.ext4 \
    --kernel /var/lib/forkd/golden/vmlinux \
    --tap zen0 \
    --boot-wait-secs 30 \
    --old-tag zen-gate \
    --dry-run

# Then for real:
sudo scripts/ops/dellsrv-forkd-supervision/re-register-golden-tags.sh \
    --rootfs /var/lib/forkd/golden/rootfs-20g.ext4 \
    --kernel /var/lib/forkd/golden/vmlinux \
    --tap zen0 \
    --boot-wait-secs 30 \
    --old-tag zen-gate
```

The script will refuse to exit 0 unless each registered tag's
`memory.bin` is **exactly** the requested number of bytes
(`requested_mib * 1048576`). If verification fails, the tag is left
in a known-bad state and the operator is told loudly.

The `--old-tag zen-gate` step deregisters the existing 512 MiB tag
after the new ones verify. Pass `--old-tag ""` (or omit) to skip.

---

## Rollback

### Roll back the supervision unit

Returning to "manual `docker exec`" is safe and does **not** drop the
gate:

```bash
sudo systemctl disable --now forkd-controller.service
sudo systemctl stop forkd-controller.service   # idempotent if already stopped
```

The `forkd` docker container is left running. The operator can now
manage it by hand with `forkd-ec-boot-dellsrv.sh`.

If you actually need to take the container down (e.g. to roll back a
bad image), stop the unit **first**, then:

```bash
docker rm -f forkd
```

Never `docker rm -f forkd` while the unit is active — systemd will
immediately try to re-create it, and you'll fight the supervisor.

### Roll back the re-register

If a new tag verifies wrong, the script exits non-zero **before**
deregistering the old tag, so `zen-gate` (or whatever you passed as
`--old-tag`) remains in place as the fallback. To forcibly revert to
the pre-script state:

```bash
# Re-register the broken-but-working tag by hand if you removed it:
sudo forkd snapshot --tag zen-gate --kernel <K> --rootfs <ext4> \
    --tap zen0 --boot-wait-secs 30 --mem-size-mib 512
```

---

## Why this matters

- **No second controller.** The unit owns the boot script, not the
  binary. The live controller stays inside the `forkd` container, on
  port 8891, exactly as today.
- **The 512 MiB bug cannot recur silently.** Every re-register
  verifies the persisted `memory.bin` against the requested MIB
  before exiting 0. If the CLI is bypassed (raw REST POST) the script
  fails loud.
- **Secrets stay out of the unit.** The token lives in
  `/etc/forkd/forkd.env` (mode 0600), referenced via
  `EnvironmentFile=`. Nothing is inlined.

---

# Gate health probe (per_child_netns=true canary)

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

1. `POST /v1/sandboxes` with `tag=zen-gate-base` and
   `per_child_netns=true`. **The `true` is load-bearing** — a probe
   using `false` would have passed throughout the entire outage and
   is worthless.
2. Inspect the response for the netns-failure signature
   (`Invalid argument` / `socket never appeared`). If matched,
   alert (or, with `--heal`, attempt the documented repair).
3. `POST /v1/sandboxes/{id}/exec` with body
   `{"args":["/bin/true"]}` and assert `exit_code == 0`.
4. `DELETE /v1/sandboxes/{id}` — **always**, even on prior failure.
5. Exit 0 on success, non-zero with a precise, greppable reason on
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

## Rollback

```bash
sudo systemctl disable --now gate-health-probe.timer
sudo systemctl stop gate-health-probe.service  # idempotent if idle
sudo rm /etc/systemd/system/gate-health-probe.{service,timer}
sudo systemctl daemon-reload
```

This stops the periodic probe ONLY. The forkd controller and
`forkd-ec.service` are unaffected.

