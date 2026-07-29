# dellsrv completion runbook — operator execution, 2026-07-28 morning

**Author:** fabro orchestrator (session fabro-71), overnight 2026-07-27→28.
**Audience:** Val, executing by hand on dellsrv.
**Status of everything below:** authored from the git/PR trail ONLY. No live
dellsrv access was available (see "Why nothing was verified live" at the end).

> ## Read this first
>
> **Every claim about live dellsrv state in this document is UNKNOWN until Part A
> is run.** Nothing here was observed. The campaign's own commit trail contains at
> least one unproven assertion (commit `6c3ed8bac` states "golden option-2 is live
> on dellsrv" with no evidence attached), so treat the trail as a *hypothesis
> generator*, not as ground truth.
>
> Part A is read-only and answers every open question. Part B is gated on Part A.
> **Do not run any Part B step whose Part A precondition passed** — you would be
> re-doing work that is already done, and in the golden-tag case that means
> churning a live gate dependency for nothing.
>
> **🔴 There is an ACTIVE OUTAGE as of 2026-07-27 night — start at PART 0, not
> Part A.** Part 0 is self-contained (it has its own read-only diagnosis step).
> Come back to Part A once production flow is restored.
>
> One correction to how the outage was escalated: it was filed under the
> controller-durability item, but `os error 11` (EAGAIN) is resource exhaustion,
> **not** the deleted-inode problem that step B1 fixes. Part 0 explains the
> distinction. Do not expect B1 to clear the outage.

---

# PART 00 — ⚡ THE EXECUTABLE LIST (operator hands, in run order)

_Added 2026-07-28 evening. Everything here needs a host no agent in this loop can
reach. Steps 1-3 are read-only and safe to run immediately, in order. Step 4 decides.
Steps 5+ mutate and are gated on what 1-4 return._

**Current signature** (as of the 17:09Z poll cycle, three consecutive cycles):

```
POST /v1/sandboxes -> HTTP 500
{"error":"restore_many: firecracker API PUT /snapshot/load returned 400:
 {\"fault_message\":\"Load snapshot error: Failed to restore from snapshot:
  Failed to build microVM from snapshot: Failed to res...   <-- TRUNCATED
```

**Established by forensics, do not re-derive:** sandbox IDs are monotonic across the
EAGAIN→restore_many transition (`003a` … `00a7, 00a8, 00a9, 00aa, 00ab, 00ac`) with
**no reset**. An in-process counter would have reset on restart. **The controller
process has been continuously up.** This kills "controller restarted and lost its
in-memory snapshot registry." Something changed the *snapshot's restorability* while
the controller stayed up.

---

### STEP 1 — Get the full fault message (read-only, ~10s)

The truncation at `Failed to res...` is hiding the one string that names the cause.
It usually identifies the missing/mismatched piece: memory file, vmstate, kernel or
rootfs drive path, or a CPU-feature/template mismatch.

```bash
# a) From the controller's own logs (not the poll log, which truncates):
docker logs --since 2h forkd 2>&1 | grep -iA5 'restore_many\|snapshot/load\|fault_message' | tail -40

# b) Or provoke one directly and read the untruncated body:
docker exec -i forkd sh <<'EOF'
TOKEN=$(cat /etc/forkd-token)
curl -sS -X POST http://127.0.0.1:8891/v1/sandboxes \
  -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" \
  --data '{"snapshot_tag":"zen-gate-big"}'
EOF
```

**This single string likely collapses steps 2-4.** If it names a specific file, go
straight to that file's forensics.

### STEP 2 — Golden / snapshot artifact forensics (read-only, ~30s)

Tests the three live candidates: (a) the backing golden was modified/replaced/
truncated under a live controller, (b) the memory/vmstate file was evicted or its fd
invalidated, (c) the VM store filled up.

```bash
# Golden artifacts + integrity state (host paths):
ls -la --time-style=full-iso /data/forkd-cow/golden/
cat /data/forkd-cow/golden/golden.sha 2>/dev/null
cat /data/forkd-cow/golden/.golden-integrity-state 2>/dev/null
lsattr /data/forkd-cow/golden/*.ext4 2>/dev/null      # is the +i immutable flag still set?

# Snapshot artifacts — per tag, with sizes and mtimes:
ls -la --time-style=full-iso /var/lib/forkd-dellsrv/forkd-snapshots/*/

# THE COMPARISON THAT MATTERS: does the golden's CURRENT sha match the recorded one,
# and is its mtime NEWER than the snapshot that was built from it?
sha256sum /data/forkd-cow/golden/*.ext4

# Disk pressure on both stores:
df -h /data/forkd-cow /var/lib/forkd-dellsrv

# Did the integrity service act recently?
systemctl status fabro-golden-integrity.service --no-pager | tail -20
journalctl -u fabro-golden-integrity.service --since -48h --no-pager | tail -40
```

**Decision:** golden mtime/sha newer than the snapshot, or `df` near full, or the
integrity service having fired → candidate (a)/(c) confirmed, and it explains *both*
the earlier EAGAIN (VM boots from a half-valid image, guest agent never answers) and
today's honest 400.

### STEP 3 — Per-tag control: which snapshot is broken? (read-only, ~1min)

Scopes the blast radius. Merged as **PR #21** on `zenprocess/fabro`, branch
`fabro-71/exec-eagain-control` — `scripts/ops/dellsrv-forkd-supervision/exec-eagain-control.sh`.

```bash
# Run BOTH. Do not pipe (piping reports tee's exit code, not the script's).
sudo scripts/ops/dellsrv-forkd-supervision/exec-eagain-control.sh --tag zen-gate-big --diagnose
echo "big exit: $?"
sudo scripts/ops/dellsrv-forkd-supervision/exec-eagain-control.sh --tag zen-gate-base --diagnose
echo "base exit: $?"
```

Read the `CONTROL-RESULT:` and `EXEC-ELAPSED:` lines.

| big | base | meaning |
|---|---|---|
| fails | **passes** | scoped to `zen-gate-big` → re-bake only that tag (step 5) |
| fails | fails | both snapshots non-restorable → shared cause (the golden itself, or the store) |
| passes | passes | the fault is intermittent or load-dependent — do NOT re-bake; re-run under real gate load |

### STEP 4 — Decide

Steps 1-3 determine which of these you are in. **Do not skip to step 5 on a hunch** —
re-baking a golden that was fine wastes an hour and destroys the evidence.

---

### STEP 5 — Re-bake / re-register (MUTATING, T3, maintenance window required)

> **Open the window first.** A forkd/golden operation is disruptive; standing policy
> requires it or you page people for your own planned action:
> ```bash
> zenctl maint on 60m "golden re-bake — non-restorable snapshot, fabro gate outage"
> ```
> Close with `zenctl maint off` when done.

**The non-negotiable rule from the recorded lesson — this is the whole point:**
**do NOT re-baseline a golden you have not restore-booted.** Disk-clean is not
bootable. Verify in this order:

```bash
# 1. Filesystem integrity (read-only check):
e2fsck -n /data/forkd-cow/golden/zen-gate-clean.ext4

# 2. REAL restore-boot canary — the step that has been missing.
#    Must prove real work inside the guest, not just that it booted.
#    Boot-only proof (node -v + exit 0) is EXACTLY what let a broken golden
#    promote looking healthy on 2026-07-22. Assert something real:
#      - node --version inside the VM
#      - a real `npm ci --no-audit --no-fund` in a scratch dir
#      - free disk > 2GiB inside the guest
#    Only if ALL pass, continue.

# 3. Only then re-baseline:
chattr -i /data/forkd-cow/golden/<file>.ext4
sha256sum /data/forkd-cow/golden/<file>.ext4 > /data/forkd-cow/golden/golden.sha
stat  /data/forkd-cow/golden/<file>.ext4 > /data/forkd-cow/golden/.golden-integrity-state
chattr +i /data/forkd-cow/golden/<file>.ext4
systemctl start fabro-golden-integrity.service   # expect ok=1

# 4. Re-register the tag(s) — a RE-REGISTER off the same rootfs, not a re-bake:
#    forkd snapshot --tag <T> --kernel <K> --rootfs <ext4> --tap <tap> \
#      --boot-wait-secs <N> --mem-size-mib <MIB>
#    zen-gate-base = 1024, zen-gate-big = 4096.
#    Source kernel/tap/boot-wait values from the EXISTING registration first —
#    record them before you deregister anything, so rollback is exact.
```

**Rollback:** deregister the new tag, restore the prior registration from the values
recorded above.

### STEP 6 — PROVE IT (nothing counts as fixed without this)

```bash
# a) Fast precheck — trivial exec must pass on both tags:
sudo scripts/ops/dellsrv-forkd-supervision/exec-eagain-control.sh --tag zen-gate-big
echo "exit: $?"

# b) The authoritative proof — a real gate run returning PASS or a genuine FAIL,
#    never another INFRA:
fabro-github-gate.sh reverdict --repo uniforme --sha <current-uniforme-head> \
  --test "npm ci --no-audit --no-fund && npm test" \
  --paths "src test features specs e2e public migrations scripts qa import .zp"
```

The `--paths` flag is load-bearing for npm repos: sparse clone materializes root only,
and without it vitest reports `No test files found` — a **false failure** that would
look like a real red verdict.

Verify the resulting status **on GitHub** for uniforme (it is not on Forgejo — see
Part A step A6):
```bash
gh api repos/zenprocess/uniforme/commits/<sha>/status \
  --jq '.state, (.statuses[] | {context, state, description})'
```

---

### DURABLE FIXES (do not skip once green — these prevent recurrence)

**D1. Add a real restore-boot canary to `golden-validate.sh` permanently.** This is
the recorded lesson's actual prescription and belongs in the fix, not as a follow-up.
It must run real work (npm ci + free-disk assertion), not `node -v`. **Verified
2026-07-28: the equivalent hardening authored on 2026-07-22 as a zeninfra PR NEVER
MERGED** — `GATE_CANARY_DEEP` returns 0 results in a repo code search and is absent
from the on-disk `gates-mcp/ensure-base.sh`. Assume it needs authoring from scratch
unless the old unmerged branch is located first.

**D2. forkd upstream #269 — the fail-fast capability.** #269 adds controller-side
bootability checking (`GET /v1/snapshots`) and returns **409 on `POST /v1/sandboxes`
for a non-restorable tag**, instead of attempting a doomed restore and emitting a
misleading error. The deployed gate pins **v0.5.3**, which predates #269; the
recovered binary is **v0.5.2**, older still. **Neither binary can fail fast** — both
will keep producing confusing errors for this failure class. Getting #269 into the
deployed version is the durable fix for the *diagnosability* of this whole incident.
(Upstream repo is external — operator-gated, T3.)

**D3. Exec-hop retry asymmetry.** `~/fabro-run/forkd-shim.py` gives `sandbox-create`
extra attempts on a known transient signature but every other request shape —
including `exec` — exactly one attempt. Only worth changing if the control experiment
shows a *transient* fault; a deterministic one just fails N times instead of once.

---

# PART 0 — 🔴 ACTIVE OUTAGE (escalated 2026-07-27 night) — DO THIS FIRST

**Symptom — CONFIRMED 2026-07-28 from GitHub commit statuses** (no longer just a
relayed report; this is the verbatim `description` field the gate itself posted):

```
context:     fabro/qa-pipeline
state:       error
target_url:  None
description: infrastructure: controller POST /v1/sandboxes/sb-6a686d2b-0097/exec
             -> HTTP 500 {"error":"exec: read response: Resource temporarily unavaila
             (truncated by GitHub at 140 chars)
```

Reproduce with:
`gh api repos/zenprocess/uniforme/commits/<PR-head-sha>/status`

**Systematic, not a one-off — same string, three distinct sandboxes, 8+ hours:**

| commit | time (UTC) | sandbox |
|---|---|---|
| `18257180` | 2026-07-28T00:48:57Z | `sb-6a67fa76-008a` |
| `bf8f62d` (PR #807) | 2026-07-28T08:58:17Z | `sb-6a686d2b-0097` |
| `3b72eeb` (PR #808) | 2026-07-28T09:11:56Z | `sb-6a68705c-0098` |

Note the gate **correctly labels this `infrastructure:`** in its own description — so
the classifier is identifying it right, yet it still posts a red `error` status. That
gap is step 0.55's subject.

**Reported blast radius:** every uniforme gate run dies, so `fabro/qa-pipeline`
never goes green, and `cfw-autodeploy` has skipped **~23 merges** on
`uniforme/preprod` over **4+ hours**.

## ⚠️ Read this before you restart anything

The escalation labels this "the broken forkd controller" and files it under the
controller-durability item. **Those are two different faults and the durability fix
will not resolve this one.**

- `os error 11` is **EAGAIN** — "resource temporarily unavailable". On an exec path
  this is a `fork()`/thread-spawn or VM-launch failure caused by **resource
  exhaustion**: PID/task limit, `RLIMIT_NPROC`, FD limit, or memory pressure.
- The durability item (step **B1**, canonicalize the recovered v0.5.2 binary) is
  about surviving a *restart* after the deleted-inode incident. It has nothing to do
  with EAGAIN. **Running B1 will not clear this outage.**

If you run B1 expecting the gate to come back and it doesn't, that is not a new
problem — it's this misattribution. Do Part 0, then B1 separately for its own reason.

## 🔬 REVISED AGAIN 2026-07-28 (3rd pass) — both prior analyses were reading a STALE checkout

Two rounds of "verified" facts below (mine and uniforme-781's) were drawn from a
uniforme working copy that was **345 commits behind `origin/main`** — it predates
commit `be30943` (2026-07-24, "wire the hermetic E2E lane into the fabro gate, #477")
entirely. Neither of us checked `git status` against the remote before asserting facts
from the file. Re-verified against `origin/main`; corrections below **supersede**
everything under the "PER-REPO" heading further down (kept intact underneath, struck
through in spirit not in text, because the differential reasoning in it is still
correct — only the specific command/config facts were wrong).

**What was actually wrong:**
- uniforme's `testCmd` is not `npm test` — it's `npm ci ... && npm run qa:gate`, where
  `qa:gate = npm test && npm run qa:e2e`, and `qa:e2e` boots a Cloudflare Worker
  (`wrangler dev`) and launches **real Playwright Chromium** inside the guest.
- uniforme **does** have a `.zp/qa-diamond.yaml` — I said it didn't; that file is what
  makes this diagnosable at all.

**What that file actually says, verbatim comment, and it is the sharpest evidence yet:**

```yaml
# Gate VM snapshot. The Lane-1 compiler passes this to the gate; without
# it the gate boots zen-gate-base at 1GB of RAM and the uniformetest
# suite OOMs (same class of failure that forced the serial-vitest
# workaround). zen-gate-big is the 4GB guest on which the full suite
# ran 6911/6911 green. Pin this here AND in test/qa-diamond.test.ts —
# a silent drop / typo falls back to the 1GB base with no error.
snapshot_tag: zen-gate-big
```

Three things this settles at once:

1. **`zen-gate-base` is already 1024 MiB.** Golden option-2's base tag is confirmed
   live by this comment (an independent source from the fabro-side commit trail) — so
   Part A step A3's 512 MB trap is very likely already cleared. Still worth running A3
   to confirm `zen-gate-big` (4096 MiB) specifically exists, since that's the tag this
   file actually depends on.
2. **The undersized-guest hypothesis and the EAGAIN evidence are both still right** —
   the file's own author independently arrived at "OOM-class failure without the 4 GB
   tag," which matches uniforme-781's `exec: Resource temporarily unavailable` finding
   exactly. Chromium + a Worker + a 6911-test suite in 1 GB is more than enough to
   explain EAGAIN at `fork()`/`mmap()`.
3. **The file's own author already named the exact failure mode we're chasing**:
   *"a silent drop / typo falls back to the 1GB base with no error."* If the Lane-1
   compiler either doesn't read `snapshot_tag` from this file, or reads it but the
   value doesn't reach the sandbox spec, uniforme silently runs on the 1 GB guest with
   **no error indicating that happened** — and then EAGAINs on Chromium, indistinguishable
   from the outside.

**I cannot verify whether the compiler honors this pin — and neither can uniforme.**
`FABRO-COMPLETION-RUNBOOK.md` (the original operator mandate) asserts *"platform side
is ready: qa-diamond-compile.py reads top-level `snapshot_tag:` and the gate passes it
to `--snapshot` (merged, cal-green-proven)"* — but **that compiler is not checked into
either the fabro repo or the uniforme repo.** I grepped both trees at `origin/main`;
it isn't there. Same gap as the forkd-controller source earlier in this document: the
code that would actually answer the question lives somewhere neither of us can read.
**This makes the following the single highest-priority live check, ahead of even
Part A step A3:**

```bash
# On dellsrv/wherever the Lane-1 compiler runs — find it first if its location
# isn't already known, then confirm it actually reads .zp/qa-diamond.yaml:
grep -rn "snapshot_tag" <compiler location> 2>&1

# Then confirm empirically, on a REAL uniforme gate invocation: which snapshot_tag
# did the actual `forkd snapshot create`/sandbox-create call receive? Compare
# against the .zp/qa-diamond.yaml pin (zen-gate-big). If the create call shows
# zen-gate-base, the pin is being silently dropped — exactly as warned.
docker exec -i forkd sh <<'EOF'
TOKEN=$(cat /etc/forkd-token)
curl -s -H "Authorization: Bearer $TOKEN" http://127.0.0.1:8891/v1/sandboxes
EOF
# Look at the snapshot_tag field on the most recent uniforme-related sandbox.
```

> ### ⏱️ The timeline eliminates the "pin wasn't there yet" explanation
>
> The pin landed in `origin/main` as **`bfcc588` "ci: pin snapshot_tag to zen-gate-big
> (#664)", 2026-07-25 19:11 UTC**. The **first EAGAIN was 2026-07-25 23:44 UTC** —
> **4.5 hours later**. Every one of the 23 `failed/error` rows since (through
> 2026-07-28 09:28 UTC) occurred with `snapshot_tag: zen-gate-big` already pinned in
> the repo.
>
> So the failures are not "the pin hadn't been added yet." The pin was present and the
> gate kept EAGAINing — which means either the compiler never reads it, the value is
> dropped before sandbox-create, or `zen-gate-big` exists but cannot boot. All three
> are answered by the single check below. This is the strongest evidence in the
> document that the problem is on the **fabro/platform side**, not in uniforme's config.

If this shows `zen-gate-base` where `zen-gate-big` was pinned: **that is the root
cause**, no rootfs work needed, no restart needed — the fix is in the compiler
(reading the field) or in whatever bridges it to the sandbox-create call, and it
directly explains why uniforme (the only repo with heavy per-VM requirements AND a
`qa-diamond.yaml` pin depending on it) is the only one failing.

**Candidate fixes** (from uniforme-781, in the order they'd try them — this is a
fabro/platform-side judgment call, not something the uniforme side should self-serve
by editing its own gate config to make its own PRs pass):

1. If the compiler-honors-the-pin check above comes back clean (pin IS reaching the
   sandbox correctly) and EAGAIN still happens on `zen-gate-big`, the `weight: light`
   label in uniforme's `.zp/project.yaml` may be under-sizing something else in the
   lane-allocation path (concurrency slot, not just guest memory) — worth checking
   what `weight` actually controls before assuming it's irrelevant.
2. Confirm `zen-gate-big` (4096 MiB) exists at all (Part A step A3) — if the tag is
   simply missing, the pin is correct but has nothing to resolve to.
3. If both the pin and the tag check out and it *still* EAGAINs, consider splitting
   the lane: keep `npm test` in the blocking gate and move `qa:e2e` (the Chromium
   step) to a separate non-blocking lane, so a resource-hungry browser run cannot
   dark the entire gate for a repo. This is a bigger change (affects the pass/fail
   semantics uniforme relies on) — operator call, not a default.

**In-repo doc worth reading directly**: `docs/qa/fabro-e2e-gate.md` in the uniforme
repo (added by the same `be30943` commit that wired in the E2E lane). It documents
the author's own expectations of what fabro should provide, including a "256 MB risk"
section recommending Chromium be baked into the base snapshot to avoid a per-VM
download, and an explicit fabro-owned follow-up table:

| Follow-up (from `docs/qa/fabro-e2e-gate.md`) | Owner |
|---|---|
| Auto-discover PR heads lacking a `fabro/qa-pipeline` status and gate them | fabro |
| Stop swallowing `gh-status.sh` POST failures (`\|\| true` → real exit code) | fabro |
| Verify the driver reads `qa.testCmd` from the descriptor (or document the sync contract) | fabro |
| Bake Chromium into the base snapshot to avoid the 256 MB/per-VM download | fabro |

That table is independent evidence (written 2026-07-24, before this outage) that the
uniforme side already flagged "does the driver actually read our descriptor?" as an
open question — which is exactly what this outage turned out to hinge on.

**uniforme-781 deliberately did not touch `.zp/project.yaml` or `qa-diamond.yaml`**
— correctly: changing gate config to make one's own PRs pass would be the wrong actor
making that call. The weight/tier decision is explicitly left to the operator (and, if
delegated, to fabro-side code work).

---

## 🔬 REVISED 2026-07-28 (superseded above by the 3rd-pass finding, kept for the
## differential reasoning, which is still correct) — it is PER-REPO, which refutes my
## first analysis

An earlier draft of this section proposed two **host-global** hypotheses (leaked
microVMs exhausting host PIDs; golden option-2's 4 GB tag exhausting host RAM).
**Evidence from the uniforme orchestrator refutes both**, and I have corrected them
rather than leaving them to misdirect you.

**The refuting data** (from `ao.db pr_checks`, reported by uniforme-781, all-time):

| project | same 3-day window | result |
|---|---|---|
| `zenprocess/cal` PR #72 | 4 × `passed/success` | ✅ green |
| `zenprocess/cal` PR #56 | 4 × `failed/failure`, 1 × `passed/success` | ✅ real verdicts, both polarities |
| `zenprocess/zetronom` PR #134 | 4 × `passed/success`, 3 × `failed/failure` | ✅ real verdicts |
| `zenprocess/uniforme` | **ZERO** `passed/success` since 2026-07-21T20:59:39Z | ❌ only `error` + stuck `pending` |

A host-global resource exhaustion would take down cal and zetronom too. It didn't.
**The gate driver can run, and can post both green and red — just not for uniforme.**
So the fault is per-repo, and any explanation that is host-wide is wrong.

Also decisive: every uniforme `failed/error` row has **empty `details` and empty
`url`**, and on GitHub shows `state=ERROR` with `target_url=null`. An `error` with no
output means the run produced nothing at all — **the gate died before or during
setup, not at the test command.** That is a different fault from a test regression,
and it rules out "uniforme's tests broke."

### ⛔ SUPERSEDED — corrected in the "3rd pass" section above

Everything below this line, through the end of this subsection, was written from a
**stale uniforme checkout** (345 commits behind `origin/main`) and is factually wrong
on the specifics. Left visible rather than deleted so the reasoning trail is honest,
but **do not act on any command/config claim in this block** — use the "REVISED AGAIN
(3rd pass)" section above instead. Specifically wrong: uniforme's testCmd is not
`npm test` (it's `npm run qa:gate`, which includes a Chromium E2E lane); uniforme
**does** have a `.zp/qa-diamond.yaml`, and it already pins `snapshot_tag: zen-gate-big`.
The differential logic (uniforme is the only heavy-workload repo among the three
compared) is still correct — only the mechanism was wrong (Chromium/E2E weight, not
bare `npm ci`).

**What the corrected picture means for the remediation ladder:**

- **Do not restart first**, still. A restart cannot fix a mis-read config pin, and if
  the compiler is honoring the pin correctly, there is nothing to restart for.
- **The likely fix is NOT Part B step B2** (registering new tags) — `zen-gate-big`
  already exists per the uniforme file's own comment ("ran 6911/6911 green" on it).
  The likely fix is in the **Lane-1 compiler** (find it, confirm it reads
  `snapshot_tag`, confirm the value reaches the sandbox-create call) — see the live
  check at the end of the 3rd-pass section above. That component is not in either
  repo I could search; locating it is itself part of the fix.
- Still run step 0.1's diagnosis and Part A step A3, since a leak or an actually-missing
  `zen-gate-big` tag could coexist with a compiler bug, and they're cheap to rule out.

> **Do not restart before running step 0.1, Part A step A3, and the compiler check
> above.** A restart destroys evidence and none of the candidate causes are fixed by
> one.

## Step 0.1 — Diagnose (read-only, ~60s)

```bash
# a) How many sandboxes does the controller think exist? (orphan count)
docker exec -i forkd sh <<'EOF'
TOKEN=$(cat /etc/forkd-token)
curl -s -H "Authorization: Bearer $TOKEN" http://127.0.0.1:8891/v1/sandboxes
EOF

# b) How many Firecracker processes are actually alive?
pgrep -c firecracker || echo 0
docker exec forkd sh -c 'pgrep -c firecracker || echo 0'

# c) PID/task exhaustion — the direct EAGAIN cause
cat /sys/fs/cgroup/system.slice/docker-*.scope/pids.current 2>/dev/null | tail -5
cat /sys/fs/cgroup/system.slice/docker-*.scope/pids.max     2>/dev/null | tail -5
docker exec forkd sh -c 'cat /sys/fs/cgroup/pids.current /sys/fs/cgroup/pids.max 2>/dev/null'
ps -eLf | wc -l          # total threads on host
sysctl kernel.pid_max kernel.threads-max

# d) Memory pressure / OOM history
free -g
docker stats --no-stream
dmesg -T 2>/dev/null | grep -iE 'oom|out of memory|cannot allocate' | tail -20

# e) FD limits
docker exec forkd sh -c 'ulimit -n; ls /proc/1/fd 2>/dev/null | wc -l'
```

**Reading the result:**

| Observation | Hypothesis | Go to |
|---|---|---|
| Many sandboxes in (a) and/or many `firecracker` procs in (b), memory OK | **1 — leak** | 0.2 |
| `pids.current` at/near `pids.max` | **1 — leak** (exhausted task limit) | 0.2 |
| `free -g` shows little available, or `dmesg` shows OOM kills | **2 — capacity** | 0.4 |
| Few sandboxes, plenty of RAM and PIDs | Neither — controller-internal | 0.3, then escalate |

## Step 0.2 — Reap orphans (least-disruptive remediation; try FIRST)

**Precondition:** step 0.1 showed orphaned sandboxes or PID exhaustion.

This is targeted and does not interrupt healthy work.

```bash
# List, review, THEN delete. Do not blind-delete — a running gate lane is a sandbox too.
docker exec -i forkd sh <<'EOF'
TOKEN=$(cat /etc/forkd-token)
curl -s -H "Authorization: Bearer $TOKEN" http://127.0.0.1:8891/v1/sandboxes
EOF

# For each id you have confirmed is an orphan (not an in-flight lane):
docker exec -i forkd sh <<'EOF'
TOKEN=$(cat /etc/forkd-token)
curl -s -X DELETE -H "Authorization: Bearer $TOKEN" \
  http://127.0.0.1:8891/v1/sandboxes/<SANDBOX_ID>
EOF
```

Re-run 0.1(a)+(c). If PIDs drop and exec starts working, **stop here** and go to
step 0.5 (verification). Restarting is then unnecessary.

## Step 0.3 — Restart (only if 0.2 was insufficient)

> ### 🔒 Open a maintenance window FIRST
>
> A forkd/docker restart is a disruptive infra op. Standing policy requires a
> `zenctl` maintenance window before one, or you will fire `health_alert` +
> route-drift ntfy and page people for your own planned action:
>
> ```bash
> zenctl maint on 30m "forkd controller EAGAIN outage remediation"
> ```
>
> Close it when done: `zenctl maint off`.

Escalating ladder — **stop at the first one that works**, re-running 0.5 after each:

```bash
# (i) Restart the forkd container. This also re-provisions the per-child netns,
#     which the boot script creates at container start.
docker restart forkd
sleep 15
docker logs --tail 50 forkd

# (ii) If the container will not come up healthy, restart its owning unit:
sudo systemctl restart forkd-ec.service
sudo systemctl status forkd-ec.service --no-pager | tail -20

# (iii) Confirm the controller answers at all:
docker exec -i forkd sh <<'EOF'
TOKEN=$(cat /etc/forkd-token)
curl -s -o /dev/null -w '%{http_code}\n' \
  -H "Authorization: Bearer $TOKEN" http://127.0.0.1:8891/v1/sandboxes
EOF
```

**After ANY restart, re-run Part A step A5** (per-child netns). The 2026-07-22
three-day outage was those namespaces dying, and they are provisioned only at
container start — a restart is exactly when they can come back wrong.

**Rollback:** there is nothing to roll back for a restart. If the container will not
start, capture `docker logs forkd` and `journalctl -u forkd-ec.service -n 100` before
trying anything further — that log is the artifact for the next person.

## Step 0.4 — If capacity (hypothesis 2), do NOT just restart

**Precondition:** 0.1 showed memory exhaustion or OOM kills.

A restart clears the symptom for minutes and it returns. Options, cheapest first:

1. **Reduce concurrency** on the gate lanes so `concurrent_lanes × mem-size-mib`
   fits host RAM with headroom.
2. **Point the failing lane at `zen-gate-base` (1024 MiB) instead of `zen-gate-big`
   (4096 MiB)** — a one-line `snapshot_tag:` change in the consuming repo's
   `.zp/qa-diamond.yaml`, reversible.
3. Only then consider host-level capacity changes.

Record which you chose — if `zen-gate-big` is implicated, that directly affects the
uniforme-on-`zen-gate-big` plan in the cross-project ask below, and it means golden
option-2 needs a concurrency budget attached before it is used in anger.

## Step 0.5 — VERIFY: a real gate run on uniforme HEAD must go green

Do not declare this resolved on a 200 from the controller. The reported failure is at
the *gate* level, so the verification has to be at the gate level too.

**Fast precheck (~10s)** — exercises the real `per_child_netns=true` path end to end:

```bash
sudo /usr/local/sbin/gate-health-probe.sh
echo "probe exit: $?"     # 0 = create + exec + delete all succeeded
```

Note the README's warning: **do not pipe this** if you care about the exit code —
`probe.sh | tee log` reports `tee`'s status, not the probe's.

**Authoritative check — a real gate run on uniforme HEAD:**

```bash
# 1. Get the SHA you are proving green:
git -C <FILL: uniforme checkout path> rev-parse HEAD

# 2. Trigger the Lane-1 QA path for that SHA (the normal gate entry point for
#    uniforme — same path cfw-autodeploy waits on).

# 3. Watch the exec that previously failed actually succeed:
docker logs -f forkd 2>&1 | grep -iE 'exec|error 11|Resource temporarily'
```

**Green means:** the run completes AND a `fabro/qa-pipeline` status for that SHA flips
to `success`. For **uniforme specifically, read that on GitHub** — it is not on Forgejo
(see the corrected table in Part A step A6):

```bash
gh api repos/zenprocess/uniforme/commits/<SHA>/status \
  --jq '.state, (.statuses[] | {context, state, description, target_url})'
```

You are looking for `state: success` on context `fabro/qa-pipeline`. The **exact
before/after** is unusually clean here, so use it: the same query today returns
`state: error` with `description: 'infrastructure: controller POST
/v1/sandboxes/.../exec -> HTTP 500 ... Resource temporarily unavaila'` (truncated by
GitHub at 140 chars). That string disappearing, replaced by `success`, is your proof.

Note the status only appears for **PR heads**, not plain commits — so verify against a
PR head SHA, not a `main` commit, or you will read "no status" as failure.

**Then confirm the actual business symptom cleared:** `cfw-autodeploy` picks up the
backlog and the ~23 skipped `uniforme/preprod` merges drain. A green gate that
doesn't unblock autodeploy means the blockage was only partly the controller.

## Step 0.55 — 🚨 The false-ERROR status is actively misrouting agents (fix independent of root cause)

Reported by uniforme-781, and this is the part that causes harm *right now*,
regardless of when the root cause is fixed.

**What happened:** a `failed/error` status with empty details causes AO to emit a
*"CI is failing on PR #N … Review the output below and push a fix"* nudge. That nudge
was delivered into the composer of **uniforme-782, a VERIFY-ONLY worker** dispatched
via `--claim-pr` to independently verify PR #807. **Had it complied, it would have
pushed commits to the very branch it was verifying** — destroying the independence of
the verification and corrupting the PR. uniforme-781 caught it and sent an override.

The nudge also says *"review the output below"* when there **is** no output, because
an errored run produces none.

This is our own core failure class with a new blast radius: **an infrastructure fault
rendered as a code failure, which then instructs an agent to "fix" code that was never
broken.** It can misroute agents in *any* project using `--claim-pr`, not just uniforme.

**Two cheap mitigations, both worth doing before root cause:**

1. **Suppress or clearly label the auto-fix nudge when `conclusion=error` AND
   `details`/`url` are empty.** An infra error is not a code failure and must not tell
   anyone to push a fix. *(Owner: agent-orchestrator — this is AO nudge behavior, not
   the fabro gate. Needs routing to that project.)*
2. **If a run cannot start for a repo, post NO status (stay dark) rather than ERROR.**
   Dark is honest and harmless; a false ERROR actively misroutes agents.
   *(Owner: fabro — this is our gate poster.)*

> **⚠️ Ask 2 contradicts documented gate behavior — resolve before implementing.**
> During the 2026-07-22 netns outage the gate **correctly suppressed** verdicts rather
> than posting wrong ones; the recorded failure mode was *"silence, not noise."* The
> gate therefore already has a dark path. Yet uniforme is now receiving posted ERRORs.
> So either a regression introduced ERROR-on-infra, or this failure takes a code path
> the suppression logic does not cover. **Find out which before changing the poster** —
> if suppression exists and regressed, the fix is restoring it, not adding a second
> mechanism.
>
> **RESOLVED 2026-07-28** — the Forgejo-vs-GitHub discrepancy is settled: uniforme's
> verdicts land on **GitHub**, it is not on Forgejo at all, and this document's earlier
> "GitHub statuses are always empty" claim was **wrong for uniforme**. Corrected in
> Part A step A6. It mattered: that claim would have told you to ignore the one surface
> carrying the actual diagnostic.
>
> ### 🐛 Separate bug, route to agent-orchestrator: `ao.db` drops the diagnostic
>
> `ao.db pr_checks.details` is **empty** while the GitHub status `description` carries
> the real error (`infrastructure: controller POST /v1/sandboxes/.../exec -> HTTP 500
> ... Resource temporarily unavaila…`). The description is being **dropped on
> ingestion**. This is real data loss, and it is not hypothetical: it caused uniforme-781
> to characterize these failures as "no output to inspect" when a precise diagnostic
> existed the whole time. Anyone treating `ao.db` as authoritative will make the same
> misdiagnosis. Route alongside the nudge-suppression ask.

**Blast-radius note:** uniforme-781 is currently treating `ao.db` as authoritative,
ignoring the fabro status for merge decisions, and gating every uniforme PR on a
dispatched verify worker running `npm test` on a fresh checkout. That workaround is
holding, so this is urgent-but-not-blocking for them. Currently affected open PRs:
**#807** (fabro ERROR on head `bf8f62d`) and **#808** (stuck `in_progress`).

The **23 stuck `in_progress`/`pending` rows** (latest 2026-07-28T09:04:08Z) are a
third symptom worth its own look: runs that never resolve at all. A gate that leaves
checks pending forever blocks merges just as effectively as a red one, and it suggests
a lane that dies without ever reporting a terminal state.

## Step 0.6 — Check whether this outage poisoned the referee labels

Worth 2 minutes once service is restored. An infra fault must never be recorded as a
code verdict — that is this campaign's core failure class, and 4+ hours of failures
is a lot of potential bad rows.

```bash
# Any run rows recorded during the outage window that claim a real verdict?
ls -la ~/.ao/data/aofactory/referee/runs/ | tail -20
grep -l 'outcome_kind' ~/.ao/data/aofactory/referee/runs/*.jsonl 2>/dev/null | tail
```

Rows from the outage window must be `infra` / `inconclusive`, **never** `fail`. Any
that say `fail` are contaminated labels: quarantine them before they reach GEPA or
the trainset. If the classifier recorded EAGAIN as a code failure, that is a
classifier gap worth a fix of its own — the same shape as the exit-137 gap.

---

## PART A — read-only verification checklist

Nothing in this part mutates anything. Run it top to bottom; record each verdict.
Total time ≈ 5 minutes.

### A1. forkd-controller binary — is the deleted-inode emergency actually closed?

Background: on 2026-07-25 the running controller was a **deleted inode** —
the on-disk binary was gone and only `/proc/<pid>/exe` held it. It was recovered
by copying that back to `/usr/local/bin/forkd-controller`. The durability question
is whether the *running* process now corresponds to a real on-disk file.

```bash
ls -l /usr/local/bin/forkd-controller
/usr/local/bin/forkd-controller --version
sha256sum /usr/local/bin/forkd-controller

# The load-bearing check — is the RUNNING controller still a deleted inode?
pgrep -af forkd-controller || true
for p in $(pgrep -x forkd-controller 2>/dev/null); do
  printf 'pid %s -> %s\n' "$p" "$(readlink -f /proc/$p/exe 2>/dev/null || echo UNREADABLE)"
  ls -l /proc/$p/exe 2>/dev/null
done
```

- **DONE** — binary exists, `--version` reports `v0.5.2`, and `readlink
  /proc/<pid>/exe` resolves to `/usr/local/bin/forkd-controller` with **no
  `(deleted)` suffix**.
- **NOT DONE** — `/proc/<pid>/exe` shows `(deleted)`, or resolves to a path other
  than `/usr/local/bin/forkd-controller`. The emergency is NOT closed; the running
  process still cannot be restarted from disk. → Part B, step B1.
- **UNKNOWN / needs care** — `pgrep` finds nothing on the host. This is *expected*
  if the controller runs inside the `forkd` container (see A2). Re-run the `pgrep`
  and `/proc` checks inside the container:
  `docker exec forkd sh -c 'pgrep -af forkd-controller; readlink -f /proc/$(pgrep -x forkd-controller | head -1)/exe'`

> **Why `--version` alone is not sufficient:** a correct `v0.5.2` on disk tells you
> nothing about what the *running* process is executing. The 2026-07-25 failure
> mode was precisely a healthy-looking service whose binary had vanished. The
> `/proc/<pid>/exe` check is the one that actually bites; do not skip it.

### A2. Which unit supervises forkd, and does it reference the canonical binary?

```bash
systemctl cat forkd-ec.service
systemctl is-enabled forkd-ec.service
systemctl is-active  forkd-ec.service

# What the unit actually launches — follow ExecStart to the boot script, then read it:
systemctl show -p ExecStart --value forkd-ec.service
# then, against whatever path that prints (commonly forkd-ec-boot-dellsrv.sh):
grep -n 'forkd-controller' <FILL: the ExecStart script path from the line above>
```

- **DONE** — `forkd-ec.service` is `enabled` + `active`, and the boot script it owns
  references `/usr/local/bin/forkd-controller` (an absolute, on-disk path).
- **NOT DONE** — the boot script references a path under `/tmp`, a build directory,
  a relative path, or a container-ephemeral location. → Part B, step B1.

> ### ⚠️ DO NOT add a `forkd-controller.service` unit
>
> **PR #16 ("ops(dellsrv): forkd supervision unit + golden tag re-register") was
> CLOSED, not merged**, precisely because a new supervision unit would **duplicate
> `forkd-ec.service`'s ownership of the boot script** — two units racing to own one
> process. The review called this "the exact hazard the earlier review flagged."
>
> The durability fix is **canonicalizing the recovered v0.5.2 binary that
> `forkd-ec.service` already launches** (step B1), NOT a competing unit. If you find
> yourself writing a new `.service` file for the controller, stop — you are
> re-creating a rejected design.

### A3. Golden option-2 — do BOTH tags exist at the right memory sizes?

The gate consumes the `zen-gate-base` snapshot tag. The original was baked at
**512 MB**, which silently OOM-killed real test suites. Option 2 re-registers two
tags off the *same* existing 20 GB golden rootfs at larger memory sizes.

```bash
# List registered snapshot tags (inside the forkd container):
docker exec forkd sh -c 'ls -la /root/.local/share/forkd/snapshots/'

# The load-bearing check — memory.bin size PER TAG:
docker exec forkd sh -c 'for t in /root/.local/share/forkd/snapshots/*/; do
  printf "%s  " "$t"; stat -c %s "$t/memory.bin" 2>/dev/null || echo "(no memory.bin)"; done'
```

Interpret the byte size exactly:

| memory.bin bytes | MiB | Meaning |
|---|---|---|
| **536870912** | 512 | ⛔ **The original undersized bake. Option 2 did NOT happen.** |
| 1073741824 | 1024 | ✅ `zen-gate-base` at option-2 size |
| 4294967296 | 4096 | ✅ `zen-gate-big` at option-2 size |

- **DONE** — BOTH `zen-gate-base` (1073741824) AND `zen-gate-big` (4294967296) exist.
- **NOT DONE** — either tag is missing, or any tag still reports **536870912**.
  → Part B, step B2.

> **This is the check most likely to contradict the commit trail.** Commit
> `6c3ed8bac` asserts option-2 is already live. If you see 536870912 here, that
> assertion was wrong and every downstream conclusion built on it (including
> "uniforme can run on zen-gate-big") is void.

### A4. gate-health-probe — is the continuous canary actually running?

PR #17 merged the probe and its commit message records a hand-verification on
2026-07-25. That proves the **script works**; it does not prove the **timer is
installed and firing**. Those are different facts.

```bash
systemctl is-enabled gate-health-probe.timer
systemctl list-timers gate-health-probe.timer --all
systemctl status gate-health-probe.service --no-pager | tail -20

# Has it actually produced output in the last day?
journalctl -t gate-health-probe --since -24h | tail -20
journalctl -t gate-health-probe --since -24h | grep -c FORKD-GATE-ALERT || true
journalctl -t gate-health-probe --since -24h | grep -c FORKD-GATE-HEAL  || true
```

- **DONE** — timer `enabled`, `list-timers` shows a concrete NEXT/LEFT time, and
  journal shows probe entries within the last 24h.
- **NOT DONE** — `is-enabled` says `disabled`/`not-found`, or `list-timers` is
  empty, or the journal has **zero** entries in 24h. → Part B, step B3.

> **A zero ALERT count is not evidence of health.** Zero alerts and zero probe runs
> look identical in a `grep -c`. Confirm the probe *ran* (journal has entries, timer
> shows a next fire) before reading an alert count as good news. This is the exact
> shape of the 2026-07-22 three-day silent outage: the failure mode was *silence*,
> and silence reads as healthy.

### A5. Per-child network namespaces — the 2026-07-22 outage surface

```bash
ls -la /var/run/netns/ 2>/dev/null | grep forkd-child || echo "NO forkd-child netns"
docker exec forkd sh -c 'ls -la /var/run/netns/ 2>/dev/null' || true
```

- **DONE** — `forkd-child-1`, `-2`, `-3` present.
- **NOT DONE** — missing. Every gate exec will fail at infra level while appearing
  silent. The netns are provisioned only by the boot script at container start →
  the repair is a controlled restart of the forkd container (see PR #17's README
  `--heal` path; do not improvise).

### A6. Posting / poller — ON or OFF?

> ### ⚠️ CORRECTED 2026-07-28 — which forge to check is PER-REPO
>
> An earlier draft of this step said "`gh api .../statuses` is always empty, never use
> it." **That is false for uniforme and it is the single most useful diagnostic we
> have.** Corrected rule:
>
> | repo | where gate verdicts land | how to read them |
> |---|---|---|
> | **uniforme** | **GitHub commit statuses** (it is **not on Forgejo at all** — the repo 404s there) | `gh api repos/zenprocess/uniforme/commits/<sha>/status` |
> | **fabro** | Forgejo statuses; GitHub CI here is **Actions**, which posts **check-runs, not statuses** | `gh api .../commits/<sha>/check-runs` (statuses genuinely *are* empty for fabro — a statuses-vs-check-runs artifact, not proof of darkness) |
>
> So an empty `commits/{sha}/status` means different things per repo. **Never infer
> gate health from a github.com status query alone, in either direction.** The
> reliable cross-repo surface is `ao.db pr_checks` joined to `pr`/`sessions` for
> `name='fabro/qa-pipeline'`, judged on **recency of terminal verdicts** — but see the
> `ao.db` data-loss caveat in step 0.55 before trusting its `details` column.
>
> Two more facts worth having:
> - **Statuses post only for PR heads.** Plain `main` commits and pushed branch heads
>   get no status at all (verified: `a3114e1`, `942adb6`). A bare commit with no status
>   is *expected*, not a dark gate.
> - **GitHub truncates the status `description` at 140 chars**, so the diagnostic it
>   carries is clipped mid-sentence. For the full error, go to the controller logs.

Check the Forgejo side and the poster process instead:

```bash
# Is the poster present / kill-switched? (on the Mac, ~/.ao-mac/)
ls -la ~/.ao-mac/gh-status.sh*
launchctl list 2>/dev/null | grep -iE 'gh-status|poll|fabro' || echo "no launchd job loaded"

# On dellsrv, whatever runs the poller:
systemctl list-units --all | grep -iE 'poll|gate|forkd' || true
```

- **CURRENT KNOWN STATE: UNKNOWN.** As of 2026-07-27 the live `~/.ao-mac/gh-status.sh`
  and its `.DISABLED-until-trustworthy.bak-20260725T195158Z` backup are **byte-identical**,
  which is consistent with *either* "was disabled and already restored" *or* "was
  never content-disabled at all." No launchd job for it is loaded on the Mac. This
  cannot be resolved from the artifacts — only by checking whether verdicts are
  actually landing on Forgejo.
- **Resolve it by**: pick a recent PR that went through the gate and look for a
  `fabro/qa-pipeline` status **on Forgejo**. Verdicts present and recent → posting is
  ON. Nothing since 2026-07-25 → OFF. → Part B, step B4 only if OFF *and* A3+A4 both
  passed.

---

## PART B — mutating steps

**Precondition discipline:** each step names the Part A check that must have
**FAILED** for the step to be needed. If that check passed, skip the step entirely.

Order matters: B1 → B2 → B3 → B4. Do not re-enable posting (B4) before the golden
tags (B2) and the canary (B3) are confirmed good — that is how a bad verdict gets
published at scale.

### B1. Canonicalize the recovered controller binary

**Precondition:** A1 or A2 failed.
**Not in scope:** creating any new systemd unit — see the PR #16 warning in A2.

The goal is that `forkd-ec.service`'s boot script launches an absolute, on-disk,
persistent binary, so a container/process restart is survivable.

```bash
# 1. Confirm what you have before changing anything:
sha256sum /usr/local/bin/forkd-controller
/usr/local/bin/forkd-controller --version    # expect v0.5.2

# 2. Back up before touching the boot script:
sudo cp -a <FILL: boot script path from A2> <FILL: same path>.bak-$(date +%Y%m%dT%H%M%SZ)

# 3. Edit the boot script so its controller invocation uses the absolute path
#    /usr/local/bin/forkd-controller. The live invocation shape recorded during the
#    2026-07-25 recovery was:
#      FORKD_TOKEN_FILE=/etc/forkd-token FORKD_BIND=0.0.0.0:8891 \
#        forkd-controller serve --bind 0.0.0.0:8891 \
#        --snapshot-root /root/.local/share/forkd/snapshots
#    Change ONLY the binary reference to the absolute path. Do not restructure
#    the unit or the script.

# 4. Verify by re-running A1 and A2. Do NOT restart anything yet — a restart is
#    only warranted if A1 showed a deleted inode, and it should be done in a
#    zenctl maintenance window.
```

**Rollback:** restore the `.bak-<timestamp>` boot script, `systemctl daemon-reload`
if a unit file changed.

**Note on "rebuild from source":** the original mandate asked for a source rebuild
of forkd-controller. **No forkd-controller source repo is referenced anywhere in the
fabro repo** — verified by grepping both `HEAD` and `origin/main` trees. Per your
2026-07-28 decision, the recovered v0.5.2 binary is the canonical artifact and this
step is the durability fix. If you know where that source actually lives, a real
rebuild supersedes this.

### B2. Register the golden option-2 tags

**Precondition:** A3 failed (a tag missing, or any tag at 536870912 bytes).

This is a **RE-REGISTER, not a re-bake**. Both tags point at the *same* existing
20 GB golden rootfs; only the memory size differs. No rootfs rebuild, minutes of
work, reversible.

```bash
# The registration form (run inside the forkd container; cf. cow-run.sh cmd_register):
forkd snapshot --tag <TAG> \
  --kernel          <FILL FROM A3: existing kernel path> \
  --rootfs          <FILL FROM A3: existing 20GB golden ext4 path> \
  --tap             <FILL FROM A3: tap device> \
  --boot-wait-secs  <FILL FROM A3: value used by the existing registration> \
  --mem-size-mib    <MIB>

# Option 2 = two tags off that same rootfs:
#   zen-gate-base  --mem-size-mib 1024
#   zen-gate-big   --mem-size-mib 4096
```

**Read the existing registration first** to source the kernel/rootfs/tap/boot-wait
values — do not guess them. They are recoverable from the current `zen-gate-base`
registration and from `cow-run.sh cmd_register` inside the forkd container.

**Verify:** re-run A3. Both tags present, sizes 1073741824 and 4294967296.

**Rollback:** deregister the new tag. The prior `zen-gate-base` registration should
be recorded (tag name + all flag values) *before* you re-register it, so it can be
restored exactly.

> Host RAM headroom is not a concern for this change: COW warm-lanes already default
> to `FORKD_COW_MEM_MIB=12288`. Only the gate's direct `zen-gate-base` tag was baked
> small.

### B3. Enable the gate-health canary timer

**Precondition:** A4 failed.

The deploy commands are already documented in
`scripts/ops/dellsrv-forkd-supervision/README.md` on `origin/main` (merged as
PR #17) — **use that file, do not improvise**. It covers install paths, the
`daemon-reload`, `enable --now`, the alert-vs-`--heal` decision, exit-code
semantics, and rollback.

Two things from that README worth restating because they are easy to get wrong:

- **Default is alert-only.** `--heal` is opt-in and should stay off until you have
  seen the probe behave. Auto-heal that masks a recurring fault re-creates the
  original blind spot.
- **Do not pipe the probe manually** if you care about its exit code — `probe.sh |
  tee log.txt` gives you `tee`'s status, not the probe's. Run it via systemd, or use
  `set -o pipefail`.

**Verify:** re-run A4 — timer enabled, `list-timers` shows a next fire, and after
~6 minutes the journal has a fresh entry.

**Rollback:** the README's rollback block (`disable --now`, remove unit files,
`daemon-reload`). It stops the probe only; forkd is unaffected.

### B4. Re-enable posting + poller

**Precondition:** A6 determined posting is OFF, **AND** A3 passed, **AND** A4 passed.

Do not do this first. The kill-switch exists because verdicts were untrustworthy;
the golden tags and the canary are what make them trustworthy again.

1. Restore `~/.ao-mac/gh-status.sh` to its intended enabled state (note: it is
   currently byte-identical to the `.DISABLED-...bak` copy, so the kill-switch may
   have been enforced by *not loading the poller* rather than by editing the script
   — check the launchd side, not just the file).
2. Load the poller launchd job.
3. **Verify on Forgejo**, not github.com (see A6). A verdict appearing under
   `fabro/qa-pipeline` on a real PR is the proof.

**Rollback:** unload the launchd job. That is the switch that matters; editing the
script is not required.

---

## Cross-project ask (not executable on dellsrv)

**uniforme QA path on `zen-gate-big`** — once A3 confirms `zen-gate-big` exists,
uniforme's `.zp/qa-diamond.yaml` needs one added line:

```yaml
snapshot_tag: zen-gate-big
```

The platform side is ready: `qa-diamond-compile.py` reads top-level `snapshot_tag:`
and the gate forwards it to `--snapshot`. This is a **uniforme repo** change, not a
fabro one, and it is strictly gated on A3 passing — pointing a QA run at a tag that
does not exist just moves the failure later.

---

## Why nothing here was verified live

The orchestrator session that wrote this document could not reach dellsrv:
`ssh dellsrv` fails DNS resolution from the sandbox (`dellsrv.zp.digital` does not
resolve). Per the standing sandbox-network-boundary rule, that denial is the egress
allowlist working as designed — one attempt was made, no workaround was attempted,
and no IP-literal or tunneling path was tried.

This is also consistent with the design of the existing probe runbook, which states
plainly: *"Nothing here is run from the Mac... a human operator can `git pull`, read
the code, and execute on dellsrv with eyes on the actual commands."* All of Part B is
T3 by the standing policy (live services, prod deploys) and is operator-executed by
rule, not by preference.

**A second constraint applied overnight:** every attempt to dispatch this work to an
AO worker failed. Workers run `claude-code` pinned to `MiniMax-M3`, which routes
through the ccmax → vip edge; four separate worker sessions each died repeatedly to
`API Error: Connection closed mid-response` with zero forward progress, and the one
alternative authorized agent (`vibe`) is broken in AO's launcher integration. That
infra blocker is written up in `STATE.md` and queued for the morning digest.
