# TASK T2 — read-only dellsrv state-capture script (code-side only)

**NO live actions.** You are authoring a script that the operator will run on dellsrv
by hand. You must NOT ssh to dellsrv, run systemctl, or touch any live service. The
sandbox cannot reach `dellsrv.zp.digital` — DNS denial there is the egress boundary
working as designed. Do NOT attempt any workaround. Author + test locally only.

## Why this exists

We cannot see live dellsrv state. Four campaign items (forkd-controller durability,
golden option-2 tags, gate-health-probe deploy, posting/poller state) are each blocked
on ground truth we don't have. This script captures ALL of it in one paste-able run so
the operator spends 30 seconds, not 20 minutes, and so the output is machine-comparable
next time.

## Deliverable

`scripts/ops/dellsrv-forkd-supervision/dellsrv-state-capture.sh` plus a new section in
that directory's existing `README.md`. Branch off `origin/main`, commit, push, open a
**DRAFT** PR against `zenprocess/fabro`.

## Hard requirements

- **STRICTLY READ-ONLY.** No `systemctl start|stop|restart|enable|disable`, no
  `forkd snapshot` registration, no `docker restart`, no file writes outside a
  `mktemp` output file, no DELETE/POST to the controller. A reviewer must be able to
  confirm read-only-ness by reading the script once. Put a comment block at the top
  stating this contract, and make it true.
- **Never print secret VALUES.** The forkd token lives at `/etc/forkd-token` INSIDE the
  `forkd` docker container. If you need it for a GET, follow the existing pattern in
  `gate-health-probe.sh` (`docker exec -i forkd sh` with a heredoc whose first action is
  `TOKEN=$(cat /etc/forkd-token)`) so the value never crosses the host argv or the
  process table. Report only presence/absence, never content.
- **Every probe must be independently failable.** Wrap each check so one missing binary
  or one absent unit does not abort the rest — the operator needs the WHOLE picture from
  one run. Use `|| true` guards deliberately, but never in a way that turns a real
  failure into a silent pass; each check must print an explicit `OK` / `MISSING` /
  `UNKNOWN` verdict line.
- **Output must be diffable**: a stable, greppable line format, one fact per line, e.g.
  `CHECK <name> <OK|FAIL|UNKNOWN> <detail>`. Print a summary count at the end.

## Checks the script must capture

1. `forkd-controller` binary: path, `--version`, sha256, and whether the RUNNING
   controller's `/proc/<pid>/exe` is a deleted inode or resolves to the on-disk file.
   (The 2026-07-25 emergency was exactly a deleted-inode process; this check is the
   durability question.)
2. `forkd-ec.service`: `systemctl cat`, `is-enabled`, `is-active`, and whether the boot
   script it owns references `/usr/local/bin/forkd-controller` or an ephemeral path.
3. Golden snapshot tags inside the forkd container: which tags exist, and the byte size
   of each tag's `memory.bin`. Known baseline to flag loudly: **536870912** bytes
   (512 MB) means golden option-2 did NOT happen. Targets are 1073741824 (1024 MiB,
   `zen-gate-base`) and 4294967296 (4096 MiB, `zen-gate-big`).
4. `gate-health-probe`: installed? `is-enabled`? timer scheduled? count of
   `FORKD-GATE-ALERT` and `FORKD-GATE-HEAL` lines in the last 24h of
   `journalctl -t gate-health-probe`.
5. Per-child netns presence (`/var/run/netns/forkd-child-*`) — the 2026-07-22 three-day
   silent outage was these dying.
6. Posting/poller: whether the verdict poster is running, and where it posts.
   **CRITICAL FACT to encode in a comment**: the gate posts to the **Forgejo** forge
   (contexts `fabro/qa-pipeline`, `fabro/qwen-review`), NOT github.com. A `gh api
   .../statuses` check is always empty and is NOT valid evidence of gate health. Do not
   write a github.com-based check.

## Acceptance command (must pass; paste the real output in your report)

```
bash -n scripts/ops/dellsrv-forkd-supervision/dellsrv-state-capture.sh \
  && shellcheck -S warning scripts/ops/dellsrv-forkd-supervision/dellsrv-state-capture.sh \
  && ! grep -nE '(systemctl (start|stop|restart|enable|disable))|(docker (restart|rm|stop))|(forkd snapshot )|(-X (POST|DELETE|PUT))' scripts/ops/dellsrv-forkd-supervision/dellsrv-state-capture.sh \
  && grep -q 536870912 scripts/ops/dellsrv-forkd-supervision/dellsrv-state-capture.sh \
  && echo ACCEPT
```

(The negated grep is the read-only proof — it must find NOTHING. If shellcheck is not
installed, install it or state clearly that you could not run it; do not silently drop
it from the acceptance chain.)

## Adversarial check (required — do not skip)

The acceptance command above is a **negative control**: it passes when the script
contains no mutating verbs. Prove it actually bites. Temporarily insert a line like
`systemctl restart forkd-ec.service` into a **scratch copy** of the script, re-run the
acceptance chain, and confirm it FAILS. Then remove it and confirm it passes again.
Report both outputs. A guard you never saw fail is not a guard.

Also: pick the check you judge weakest (most likely to report OK while the underlying
thing is broken) and either strengthen it or state plainly why it cannot be strengthened
without live access.

## Report back

File path, draft PR number, real acceptance output, and the negative-control proof
(the FAIL output and the restored PASS output). Do NOT mark done without both pasted.
