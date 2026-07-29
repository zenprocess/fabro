# TASK T1 — consolidated dellsrv T3 runbook + read-only verification checklist

**Docs deliverable. NO live actions.** You are writing the single consolidated T3
execution runbook for the fabro/dellsrv completion campaign, for operator Val to run
BY HAND in the morning.

You MUST NOT ssh to dellsrv, run systemctl, run forkd, or touch any live service. The
sandbox cannot reach `dellsrv.zp.digital` anyway — DNS denial there is the egress
boundary working as designed. Do NOT attempt any workaround (no IP literals, no
tunneling, no alternate hostnames). This is a pure authoring task.

## Deliverable

New file `docs/internal/dellsrv-completion-runbook.md` in the fabro repo. Branch off
`origin/main`, commit, push, open a **DRAFT** PR against `zenprocess/fabro`.

## Sources — read these first

1. `~/.ao/data/aofactory/FABRO-COMPLETION-RUNBOOK.md` — the operator mandate (9 numbered
   items). Authority on WHAT must happen.
2. `scripts/ops/dellsrv-forkd-supervision/README.md` on `origin/main` — the already-merged
   gate-health-probe runbook (PR #17). Its deploy section is the model for tone and
   precision. Do NOT duplicate its content; REFERENCE it.
3. `git log origin/main` plus `gh pr view 15`, `16`, `17`, `19` `--repo zenprocess/fabro`
   — the commit/PR trail showing what already landed.

## Content — exactly two parts

### PART A — READ-ONLY VERIFICATION CHECKLIST (put this FIRST; it is the more important half)

Numbered, copy-pasteable, strictly NON-MUTATING commands Val runs on dellsrv to establish
ground truth. Each entry needs: (a) the exact command, (b) what output means DONE, (c) what
output means NOT DONE. Cover at minimum:

- **forkd-controller binary**: exists at `/usr/local/bin/forkd-controller`, `--version`
  reports v0.5.2, and is NOT a deleted inode (compare against `/proc/<pid>/exe` of the
  running controller).
- **Which unit supervises forkd**: `systemctl cat forkd-ec.service`, `is-enabled`,
  `is-active`, and whether `forkd-ec-boot-dellsrv.sh` references the `/usr/local/bin`
  binary or something ephemeral.
- **Golden option-2 tags**: list snapshots INSIDE the forkd container; confirm BOTH
  `zen-gate-base` AND `zen-gate-big` exist; confirm each `memory.bin` size
  (`zen-gate-base` 1024 MiB = 1073741824 bytes, `zen-gate-big` 4096 MiB = 4294967296
  bytes). The pre-existing baseline was **536870912** bytes (512 MB) — if you see that,
  option-2 did NOT happen.
- **gate-health-probe**: `systemctl list-timers gate-health-probe.timer`, `is-enabled`,
  and `journalctl -t gate-health-probe --since -24h | grep -c FORKD-GATE-ALERT`.
- **Posting/poller state**: how to determine whether gate verdict posting is ON or OFF.
  **CRITICAL FACT you must encode**: the fabro gate posts to the **Forgejo** forge
  (contexts `fabro/qa-pipeline`, `fabro/qwen-review`), NOT github.com. `gh api .../statuses`
  is ALWAYS empty and is NOT a valid health check. Say so explicitly so nobody re-derives
  that wrong conclusion.

### PART B — MUTATING STEPS, gated on Part A results

Each remaining item in dependency order, each with a stated PRECONDITION (which Part A
check must have FAILED for this step to be needed) and a ROLLBACK. Cover:

- **Golden tag registration** via
  `forkd snapshot --tag <T> --kernel <K> --rootfs <ext4> --tap <tap> --boot-wait-secs <N> --mem-size-mib <MIB>`.
  Option 2 = `zen-gate-base` at 1024 and `zen-gate-big` at 4096, off the SAME existing
  20 GB golden rootfs — this is a **RE-REGISTER, not a re-bake**. Rollback = deregister
  the new tag.
- **gate-health-probe timer enable** — commands already exist in the PR #17 README;
  reference them, do not copy.
- **Re-enabling posting + poller.**

## Honesty requirements (non-negotiable — the whole point of the document)

- Anywhere live state is UNKNOWN to us, write it as **UNKNOWN** with the Part A check that
  resolves it. Do NOT assert live dellsrv state as fact. Everything we have is inferred
  from commit messages, and one commit (`6c3ed8bac`) merely *claims* "golden option-2 is
  live on dellsrv" without proof.
- Explicitly record that **PR #16** (a `forkd-controller.service` supervision unit) was
  CLOSED-not-merged because a new unit would DUPLICATE `forkd-ec.service`'s ownership of
  the boot script. Any reader tempted to re-add such a unit must hit that warning. The
  durability fix is canonicalizing the recovered v0.5.2 binary that forkd-ec already
  launches, NOT a competing unit.
- No invented paths or flag names. If you do not know a value (kernel path, tap name,
  boot-wait-secs), write `<FILL FROM PART A step N>` rather than guessing.

## Acceptance command (must pass; paste the real output in your report)

```
test -f docs/internal/dellsrv-completion-runbook.md \
  && grep -qi "forgejo" docs/internal/dellsrv-completion-runbook.md \
  && grep -q "UNKNOWN" docs/internal/dellsrv-completion-runbook.md \
  && grep -q "536870912" docs/internal/dellsrv-completion-runbook.md \
  && grep -qi "PR #16" docs/internal/dellsrv-completion-runbook.md \
  && echo ACCEPT
```

## Adversarial check (also required)

Pick the two most load-bearing claims in your Part A checklist and try to prove them
WRONG from the repo/PR trail. If a check would pass even when the thing it checks is
broken, it is a worthless check — fix it and say so. Report which checks you strengthened.

## Report back

File path, draft PR number, real acceptance output, adversarial findings. Do NOT mark
done without the pasted acceptance output.
