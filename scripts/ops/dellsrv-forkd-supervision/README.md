# dellsrv forkd supervision — operator runbook

This directory holds two deliverables that an operator executes on the
**dellsrv** gate host. Both are designed for offline authoring review
and require no live credentials to validate as scripts (they are
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
