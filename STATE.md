# fabro orchestrator — STATE.md

_Last updated: 2026-07-28 (fresh recycle, session start)_

## Mandate (operator /goal 2026-07-25, via ~/.ao/data/aofactory/FABRO-COMPLETION-RUNBOOK.md)

1. SYNC 263-commit fabro+forkd sync to main
2. REBUILD fabro-cli + forkd-controller from source, redeploy
3. CONTROLLER DURABILITY — systemd unit, no more deleted-inode risk
4. GOLDEN OPTION 2 — zen-gate-base (1024 MiB) + zen-gate-big (4096 MiB) tags
5. GOLDEN-HEALTH CANARY before any posting
6. RE-ENABLE posting (gh-status.sh) + poller once canary green
7. FULL QA PATH — uniforme real suite green on zen-gate-big via Lane-1
8. FABRO RUNS UI — register gate/referee executions as fabro server runs
9. Dispatch discipline: ao spawn workers, review-gate, STATE.md, KIWI/feature

## Reconstructed status (this session — no STATE.md existed; predecessor context
came from HANDOFF-...-fabro-67.md, which turned out to be a DIFFERENT sub-campaign
— gate classifier + PR #20 adversarial testing, already closed out. Real progress
on the runbook items above was found via git/PR archaeology, not the handoff file.)

### Item 1 — SYNC: **DONE**
- `main` on origin (zenprocess/fabro) contains PR #15 "sync: merge 263 upstream
  commits from fabro-sh/fabro (incl. #603 path reorg)" — MERGED 2026-07-25, plus
  4 follow-up fixup commits (dropped forkd arms, REDACTION_MARKER dup, CI removal).
- Verified: `git merge-base --is-ancestor` chain confirms origin/main = tip
  `5f5c5d891` (PR #19 merge). Local `main` ref in this worktree is STALE (only
  points at `b9078c2b6`, 50 commits behind origin/main) — cosmetic, origin is
  the source of truth.

### Item 2 — REBUILD (fabro-cli + forkd-controller): **UNCLEAR / LIKELY NOT DONE**
- No forkd-controller source repo reference found anywhere in the fabro repo
  (grepped both HEAD and origin/main trees). Runbook's own fallback: "treat the
  recovered v0.5.2 binary as canonical" — this seems to be the path actually
  taken (see item 3).
- No evidence found of a fresh fabro-cli image redeploy to dellsrv since the
  sync landed. Needs live confirmation — T3/operator territory.

### Item 3 — CONTROLLER DURABILITY: **RESOLVED DIFFERENTLY THAN SPECIFIED — needs confirmation**
- PR #16 ("forkd supervision unit + golden tag re-register") was **CLOSED, not
  merged** (2026-07-25). Reviewer finding: a new `forkd-controller.service`
  would duplicate `forkd-ec.service`'s existing ownership of
  `forkd-ec-boot-dellsrv.sh` — "the exact hazard the earlier review flagged."
  The PR was pruned down to just the health-probe (which became PR #17).
- Conclusion in the commit trail: **no new systemd unit needed** because
  `forkd-ec.service` already supervises the boot script. This resolves the
  "no more deleted-inode process" concern IF `forkd-ec.service`/
  `forkd-ec-boot-dellsrv.sh` was updated to reference the recovered
  `/usr/local/bin/forkd-controller` (v0.5.2) canonically — **not independently
  confirmed this session**. Live check needed (operator/T3).

### Item 4 — GOLDEN OPTION 2 (zen-gate-base 1024 / zen-gate-big 4096): **DONE (per commit trail), live status not independently re-verified this session**
- Commit `6c3ed8bac` (2026-07-25, on the now-abandoned fabro-56 branch) states
  explicitly: "the re-register script is moot now that golden option-2 is live
  on dellsrv." Treat as done; a live spot-check (`forkd snapshot list` inside
  the forkd container) would confirm both tags exist with correct
  `--mem-size-mib`.

### Item 5 — GOLDEN-HEALTH CANARY: **CODE MERGED + LIVE-VERIFIED once; persistent deploy (timer enabled) status UNKNOWN**
- PR #17 merged 2026-07-25: `scripts/ops/dellsrv-forkd-supervision/{gate-health-probe.sh,.service,.timer,README.md}`.
  Commit message: "Live-verified on dellsrv 2026-07-25 by operator after three
  rounds of bug fixes (wrong JSON key, VM leak on parse failure, shape-mismatched
  parse)." — the SCRIPT works.
- BUT: README is explicit that nothing here runs from the Mac/agent — a human
  operator must `git pull` + `sudo systemctl enable --now gate-health-probe.timer`
  on dellsrv themselves. **Whether that deploy step (persistent 5-min timer) was
  actually done is unconfirmed** — only a one-shot manual verification is
  evidenced in the commit trail.

### Item 6 — RE-ENABLE posting + poller: **AMBIGUOUS, needs operator confirmation**
- `~/.ao-mac/gh-status.sh` (live) and `~/.ao-mac/gh-status.sh.DISABLED-until-trustworthy.bak-20260725T195158Z`
  (backup) are byte-identical — consistent with either "never actually disabled
  the content, only intended to" or "was disabled and already restored." No
  launchd agent for gh-status/poller found loaded on this Mac (`launchctl list`
  empty for gh/poll/fabro/zen) — the poller may live elsewhere (dellsrv?) or may
  not be currently running regardless of intent.
- Per memory `gate-observability-forge.md`: the REAL gate posts to **Forgejo**
  (fabro/qa-pipeline, fabro/qwen-review), not github.com — `gh api statuses` is
  known-empty and NOT a valid health check. Don't use it to verify.
- **Open question for operator**: is posting/poller currently on or off, and is
  item 5's canary considered green enough to flip it (if still off)?

### Item 7 — FULL QA PATH (uniforme real suite, zen-gate-big): **NOT STARTED this session**
- No PR/commit evidence found of a uniforme run through zen-gate-big yet.
  Blocked behind items 3-6 live confirmation (don't want a QA run against an
  unconfirmed golden/canary state).

### Item 8 — FABRO RUNS UI: **PARTIALLY DONE**
- PR #19 merged: `POST /api/v1/runs/registrations` (re-land of #18, closed for
  rework). Endpoint exists. **Not confirmed**: whether anything actually calls
  it yet to register real gate/referee executions, or whether
  fabro.zp.digital/runs shows anything post-merge.

### Side deliverable (from prior fabro-67 sub-campaign, unrelated to the runbook
numbered items but still open): **PR #20** "reference: fabro-sandbox-forkd
JSON-RPC plugin" — OPEN, DRAFT, MERGEABLE, adversarially tested (7/7, 5
mutations all caught), 10 files incl. Cargo.lock fix. Feedback-style PR for
upstream fabro-sh/fabro #567/#583; not gating anything. Awaiting operator
review/merge decision — not urgent.

## Sandbox constraint discovered this session

`ssh dellsrv` fails DNS resolution from this sandboxed orchestrator shell
(`Could not resolve hostname dellsrv.zp.digital`) even though `*.zp.digital` is
nominally allowlisted — this matches the documented sandbox-network-boundary
pattern (internal-only DNS, not reachable from the sandbox netns). Per SAFETY
rule (rule-sandbox-network-boundary, always in force): did NOT attempt a
workaround (no dangerouslyDisableSandbox for this host, no IP-literal connect).
Also independently corroborated by the gate-health-probe README itself:
"Nothing here is run from the Mac... a human operator can git pull and execute
on dellsrv with eyes on the actual commands" — T3 dellsrv execution is an
operator-hands action, not an agent action (mine or a worker's), consistent
with the global CLAUDE.md T3 rule (always operator, not waivable by fleet
judgment) taking precedence over the project charter's looser "you execute
T3 yourself" framing.

**Implication**: my role for items 3/4/5/6/7 (all T3, all live-dellsrv) is to
get everything code-ready and produce a precise consolidated execution runbook
for the operator, not to execute them myself or dispatch a worker to attempt
dellsrv SSH/systemctl (that would just fail the same sandbox boundary, if
workers share the same network policy — unconfirmed either way, so don't
assume a worker can jump the boundary I can't).

## OPERATOR DIRECTIVE (2026-07-28 overnight, via platform session)

Answers to the three questions I raised:
1. **Live dellsrv state = treat as partially-done/unverifiable tonight.** Do NOT
   run any live systemctl/forkd steps overnight — they are T3 and queued for
   Val's morning digest. Instead: produce the consolidated runbook as a file in
   the worktree, plus a read-only verification checklist Val runs in the morning.
2. **gh-status posting**: do not touch live switches tonight. Note as UNKNOWN in
   the runbook.
3. **forkd-controller source**: go with my recommended fallback — canonicalize
   the recovered v0.5.2 binary — but **code-side prep only, no live deploy**.

General: **dispatch-only**, use AO workers for any code work, queue all T3 asks
for the morning digest, idle when code-side work is exhausted.

## Dispatched this turn (wave 1 — cal-route: one wave, all parallel, sonnet-tier)

`cal-route` returned `waves: [[T1,T2,T3]]`, tier sonnet, arm cal-sidecar. Its
`transport: inline-parallel` was **overridden to `ao spawn`** per the AO charter
(inline Agent subagents are hook-blocked here) and the operator's dispatch-only
directive. Briefs live in `.briefs/` (worktree-local; `~/.ao/data/aofactory/` is
not writable from this sandbox).

| id | session | name | deliverable | acceptance |
|---|---|---|---|---|
| T1 | fabro-72 | dellsrv-runbook | `docs/internal/dellsrv-completion-runbook.md` — Part A read-only verification checklist + Part B mutating steps, gated on Part A. Draft PR. | grep-chain asserting forgejo fact, UNKNOWN honesty markers, the 536870912 baseline trap, and the PR #16 warning |
| T2 | fabro-73 | state-capture | `scripts/ops/dellsrv-forkd-supervision/dellsrv-state-capture.sh` — strictly read-only one-shot dellsrv diagnostic. Draft PR. | `bash -n` + shellcheck + **negated** grep proving zero mutating verbs, with a required negative-control proof that the guard actually fails when a mutating line is injected |
| T3 | fabro-74 | referee-runs-reg | `lib/components/fabro-referee` → `POST /api/v1/runs/registrations` (item 8's missing caller; PR #19 landed the endpoint). Opt-in, OFF by default, httpmock-only tests. Draft PR. | `cargo nextest run -p fabro-referee` + fmt + clippy `-D warnings`, plus a 4-mutation adversarial pass with collapse check |

All three briefs carry the sandbox-boundary instruction explicitly (dellsrv DNS
denial is a boundary, not a puzzle — no workaround attempts) and the false-green
lessons from the predecessor session (verify remote tip not echoed "pushed";
diff the mutated file before trusting a "not caught" result).

**Review gate**: 3 implementation dispatches ⇒ at least 1 review dispatch is
required before I close any of them. Queued as wave 2, blocked on wave 1.

## 🔬 CROSS-PROJECT REPORT (uniforme-781, 2026-07-28 09:xx) — REFUTED my PART 0 hypotheses

uniforme's orchestrator reported real `ao.db pr_checks` data showing the fabro gate
gives **zero green** to uniforme since 2026-07-21T20:59:39Z, while `cal` and
`zetronom` got real verdicts (both polarities) in the same 3-day window. That is
decisive: a host-global cause (leaked VMs exhausting host PIDs, golden option-2
exhausting host RAM — my two Part-0 hypotheses) would hit all three repos. It didn't.
**The fault is per-repo.** Corrected PART 0 in the runbook rather than leaving a wrong
analysis in place — flagged prominently as "REVISED, refutes my first analysis" so
nobody acts on the superseded version.

**Verified locally (no dellsrv needed):** `.zp/project.yaml` differential is exact —
uniforme's `qa.testCmd` is `npm ci --no-audit --no-fund && npm test` (full install);
cal's is `node --test test/gate-verify.test.mjs` (near-zero cost); zetronom has no
`.zp/project.yaml` gate config at all. Also: **uniforme has no `.zp/qa-diamond.yaml`**,
so it runs on the DEFAULT snapshot tag, not an explicit one.

**New leading hypothesis**: if the default tag (`zen-gate-base`) is still baked at
512 MB, uniforme is plausibly the only repo among the three whose setup step doesn't
fit the guest. This makes **Part A step A3 (the memory.bin byte check) the
single highest-priority thing to run in the whole runbook** — it would explain the
outage, the "corrupt golden" hunch, and why only the heaviest repo is affected, all at
once. Written up in the runbook with a ⭐ callout ahead of the remediation ladder.

Also relayed (step 0.55, new): uniforme-781's report that the false `error` status
triggers an AO auto-fix nudge that was delivered into a **verify-only** worker's
composer — had it complied, it would have pushed to the branch it was independently
verifying, corrupting the PR. Caught by their own orchestrator, not by fabro tooling.
Two asks queued: suppress the nudge on error+empty-output (routed to agent-orchestrator,
not us), and post no status rather than a false ERROR (routed to us, but I flagged that
the gate's own 2026-07-22 incident write-up already claims a dark-on-failure path
exists — so this needs "did it regress" answered before anyone bolts on a second
mechanism, not a blind implementation).

**RESOLVED 2026-07-28 — and it exposed an error of mine.** uniforme-781 sent the
verbatim `gh api repos/zenprocess/uniforme/commits/bf8f62d/status` response: a
populated `fabro/qa-pipeline` status, `state: error`, with a real `description`
carrying the diagnostic. **My runbook's "GitHub statuses are always empty, not a valid
check" was wrong for uniforme** — and it would have told Val to ignore the one surface
carrying the actual error.

Root cause of my error is worth recording: **the correct rule was already in my own
project memory** (`gate-observability-forge`, corrected 2026-07-25, under a heading
reading "READ FIRST, the rule below is OVER-GENERALIZED") — and I wrote the runbook
from the superseded section *below* that correction. Not stale data; I mis-read my own
notes. Fixed in three runbook locations and the memory file re-confirmed with the
verbatim response.

Corrected rule is **per-repo**: uniforme → GitHub statuses (not on Forgejo at all, 404s
there); fabro → Forgejo, and its GitHub CI is *Actions* which posts **check-runs, not
statuses**, so `commits/{sha}/status` genuinely is empty for fabro. That artifact is
what got over-generalized into a false fleet-wide rule.

**Hypothesis now CONFIRMED with a real error string** (2026-07-28, uniforme-781's
second message): the gate's own posted description is
`infrastructure: controller POST /v1/sandboxes/sb-6a686d2b-0097/exec -> HTTP 500
{"error":"exec: read response: Resource temporarily unavaila` — EAGAIN **at exec**,
i.e. resource exhaustion at the moment the command runs in the guest. Systematic, not
a one-off: same string across three distinct sandboxes spanning 8+ hours
(`18257180` 00:48Z, `bf8f62d` 08:58Z, `3b72eeb` 09:11Z). Part 0 now carries the
verbatim evidence table instead of my inference.

**Key new finding neither of us initially flagged**: the gate's description *begins*
with `infrastructure:` — it **classifies the fault correctly** and posts a red `error`
status anyway. So this is NOT a classifier gap. It sharpens the "go dark" ask into a
precise question: suppression exists and something *downstream of classification*
still posts. That is a far better bug report than "add a dark path."

**New bug for agent-orchestrator**: `ao.db pr_checks.details` drops the status
`description` on ingestion (empty in ao.db, populated on GitHub). Real data loss, with
a concrete cost already realized — it is why uniforme-781 initially characterized these
as "no output to inspect." Queued alongside the nudge-suppression ask.

## 2026-07-29 — GATE CONFIRMED WORKING. My "chromium removal" explanation FALSIFIED.

**Capture question RESOLVED (operator, from the local script — no host access needed):**
`fabro-github-gate.sh:65` runs `{ ${tcmd} ; } > /var/g.out 2>&1; rc=$?; ... tail -8
/var/g.out; echo GATE_EXIT=$rc`. **Only the last 8 lines are echoed back.** The
STEP:/STEP_EXIT: markers were written to `g.out` and scrolled past the tail window.
All three runs byte-identical at 208 chars / 9 lines = the signature of a fixed-size
tail, not skipped work. **Not a decorative gate.** The steps ran; exit 1 is the real
aggregate. My "volume-dependent truncation" hypothesis was the right family but the
wrong mechanism, and the answer was knowable from the local script all along.

This also corroborates my local reproduction: both graded SHAs genuinely fail at the
first link, both exit 1, matching both verdicts.

**⛔ MY ITEM-3 ANSWER WAS WRONG — retracted.** I claimed the recovery was explained by
PR #868 removing chromium from `qa:gate`. **Falsified by `ao.db`:** SHA `33439b28` was
gated at **00:01:59Z — inside the EAGAIN window — and it already contains #868**
(`qa:gate` = `npm test`, chromium-free). It never produced a verdict (still
`in_progress/pending`). If chromium removal were sufficient, that run should have
succeeded. **So what changed between 00:44 and 00:57 is genuinely still UNNAMED.**
Needs host logs (journalctl/controller) I cannot reach. Flagged rather than defended —
an unexplained recovery is an unrepeatable one, and I was about to let a wrong cause
stand.

**Item 1 DONE — defect routed to uniforme** (uniforme-781 was terminated/recycled;
found the live orchestrator `uniforme-827` via `ao orchestrator ls`, since `ao session
ls` hides orchestrators). Sent the exact reproduction: `4a611ec` fails
`check-test-ro-literals` on 1 new literal (`test/admin-footer.test.tsx:59`),
`efc8bca` on 2 (adds `test/rapoarte-routes.test.ts:130`); chain dies at link 1 so
**vitest never runs**; literal attributed to PR #875 (`1f0c749`), not either graded
commit. Made explicit to them that the red does NOT mean "tests failing" — the tests
have not executed yet, and fixing the lint is what unblocks the first real test-layer
signal in ~7 days.

**Items 2/4 remain host-gated**: reverdict of `c1b18a4`/`3b25011`/`8ec811f` needs gate-
host access. Standing caveat: all three predate #868, so their `qa:gate` still chains
`npm run qa:e2e` → forks chromium in the guest → per uniforme's own #864 analysis that
is the original EAGAIN trigger. Expect them to fail differently from the three recent
green-path SHAs; they are not a clean stability test.

**forkd #269 (D2) unchanged** and still the durable fix — v0.5.3 (deployed) and v0.5.2
(recovered binary) both predate it, so neither can fail fast on a non-restorable
snapshot.

## ✅ 2026-07-29T00:57Z — (superseded above) first non-INFRA verdict; cause since retracted

Gate posted `fabro/qa-pipeline=FAILURE` (not error) on uniforme@`4a611ec`,
`{"outcome":"ran","exit_code":1}`. Prior cycle 00:44:17Z still EAGAIN'd.

**Q1 — what changed? NOT me, NOT the infra, NOT a restart.** Verified from uniforme's
own git history:

| | |
|---|---|
| `qa:gate` **before** PR #868 | `npm test && npm run qa:e2e` ← forks chromium |
| `qa:gate` **at 4a611ec** | `npm test` ← chromium removed |
| `4a611ec` committed | 2026-07-29 **00:52:30 UTC** |
| gate ran on it | **00:57:56 UTC** (5.5 min later) |

uniforme's PR #868 comment states the cause outright: *"the chromium-EAGAIN lane
(#817 — 'Resource temporarily unavailable', chromium cannot fork in the 256 MB
zen-gate-base guest) has been silently skipped for ~7 days, while `qa:gate` chained
`&& npm run qa:e2e` so a green `qa:gate` falsely implied an e2e pass."*

**The gate started working the moment it gated a SHA whose in-VM command no longer
forks chromium.** The EAGAIN root cause is NOT fixed — it is *avoided*. The e2e lane
is now DISABLED (`disabled: true`) pending "fabro raises the guest weight tier above
'light'". **That is a fabro-side ask now sitting in uniforme's config as a blocker.**

**Q3 — did the test steps run? NO. But the verdict is GENUINE, not decorative.**
Reproduced locally at `4a611ec` (full `git archive` extract, exit captured before any
pipe):
```
check-test-ro-literals EXIT: 1
FAIL — 1 NEW bare Romanian literal(s) in test assertions:
  test/admin-footer.test.tsx:59:36  școlar');
```
`npm test` = `check-test-ro-literals && preflight-prod && vitest run`. It **fails at
the first link**, so `vitest` never runs — which exactly explains the observed
`stdout_tail` (no `STEP:unit`, no `STEP_EXIT:contract`, no vitest output, `GATE_EXIT=1`).
`test:serial` starts with the same check, so `unit` fails identically.

Attribution: the literal came from PR #875 (`1f0c749`, footer work), **not** from
4a611ec — it is a genuine pre-existing defect that the gate correctly caught the
moment it could run at all.

**So: NOT a second decorative-gate incident.** The gate produced a true positive on a
real repo defect. **But this run proves nothing about the test layers** — vitest/BDD
never executed, so there is still zero evidence that the unit/contract/bdd steps work
in the VM. That requires a SHA which passes the lint gate. The one-line fix in
uniforme (import the literal from `src/strings`, or add `// ro-literal-allow:`) would
produce that SHA.

**Q2/Q4 — cannot run.** Both need gate-host access I have never had this session.

## ⛔ 2026-07-28 (later) — signature moved to CREATE (snapshot restore failure).
## Correcting a wrong causal attribution before it propagates.

Operator reported the failure moved earlier in the chain: `POST /v1/sandboxes` now
fails with `restore_many: firecracker API PUT /snapshot/load returned 400 ...
"Load snapshot error: Failed to restore from snapshot..."` (truncated), three
consecutive cycles. Framed as *"whatever you just did (controller restart / snapshot
re-register?) has replaced a silent hang with an honest error."*

**Declined that attribution.** I have taken zero live dellsrv actions all session —
confirmed repeatedly, and the only thing I touched this cycle was authoring PR #21
(create+exec+delete only, and unconfirmed whether anyone had even run it yet). If the
signature changed, it was not me. Not accepting credit/blame for something I didn't
do — building the next diagnosis on a wrong cause is its own failure mode.

**This does match a real prior incident, verified before repeating it as fact**:
`gate-promotions.md` (my own memory, 6 days old) records a near-identical
2026-07-22 case on this same host — `zen-gate-base` restore-boot succeeded but the
golden's npm-cache layer was corrupt, and the existing restore-boot canary (checks
only `node -v`/`npm -v` + exit 0) passed it anyway ("self-attestation gap"). The
prescribed fix: a deep canary running real `npm ci` + a free-disk assertion before
promotion (`GATE_CANARY_DEEP`, `GATE_CANARY_NPM` in `ensure-base.sh`), authored as
zeninfra PR #5 branch `gate/incident-runbook`, noted as "not self-merged — awaiting
operator review/merge + dellsrv deploy."

**Checked whether that fix already exists before assuming it does — it does not.**
`zenprocess/zeninfra`'s current PR history (checked via `gh pr list`) has no PRs #1/
#2/#5 matching that description; its PR numbering is now in the 180s-210s range, all
unrelated. Code search for `GATE_CANARY_DEEP` in the repo: **0 results**. The local
disk checkout of `gates-mcp/ensure-base.sh` (`ZenInfra/`) has **zero** matches for
`GATE_CANARY_DEEP`, `GATE_CANARY_NPM`, or `restore-boot` — the file simply doesn't
have this logic. **The 6-day-old prescribed fix was never merged.** This is not
"redundant with existing work" — the operator's step 4 (add a restore-boot canary
that runs real work, not just `node -v`) is still fully needed, and whoever does it
should not assume there's unmerged prior art to pull from; it would need to be
authored again, or the old zeninfra branch (if it still exists unmerged somewhere)
would need to be located first.

**Also relevant, from the same memory**: the deployed gate pins **forkd v0.5.3**,
which *already* lacked forkd upstream issue #269 (controller-side bootability check
returning 409 for non-restorable tags — i.e. detecting this exact failure class
before attempting a doomed restore). Tonight's recovered controller binary is
**v0.5.2** — older still. So the controller has never had the upstream capability to
pre-detect a non-restorable snapshot; today's create-stage 400 is the hypervisor
finding out the hard way, consistent with the golden simply not being restorable in
its current state, exactly as the operator's anchor predicts.

**What I still cannot do**: the operator's steps 1-3 (full fault message, per-tag
control run, re-bake/re-register) are all host-only, same boundary as all night.
Step 4 (harden the canary permanently) is authorable code — but in **zeninfra**, a
different project outside this orchestrator's scope tonight, and I'm not spinning up
cross-project implementation work unilaterally at 1am on someone else's repo. Flagged
for the digest with the exact gap (file, missing flags, prior PR that never landed)
so whoever picks it up doesn't have to re-derive any of this.

## ⛔⛔ 2026-07-28 — THE PER-REPO DIFFERENTIAL WAS FALSE. No control group exists.

**This invalidates the shared premise of every analysis below, mine and uniforme-781's.**

The whole diagnosis rested on "cal and zetronom get real `fabro/qa-pipeline` verdicts
in the same window while uniforme gets none — therefore the fault is uniforme-specific."
Checked directly against `ao.db`:

| repo | last `fabro/qa-pipeline` row | total rows |
|---|---|---|
| `zenprocess/uniforme` | 2026-07-28 16:13 | 120 |
| `zenprocess/foundry` | **2026-07-11** 23:41 | 20 |
| `zenprocess/cal` | **2026-07-11** 08:09 | 5 |

cal's fabro gate has not run in **17 days**. The "cal is green right now" rows
uniforme-781 cited are checks named **`diode-guard`** and **`qa`** — cal's own GitHub
Actions CI, an entirely different system from the forkd gate. Verified per-PR:
cal #72 and #56 have zero `fabro/qa-pipeline` rows in that window.

**Consequences:**
1. **"The driver can post green and red, just not for uniforme" is false.** The driver
   has not run for any other repo in 17 days. There is no evidence it can currently
   produce a PASS for *anything*.
2. **"Only uniforme fails" is trivially true** — uniforme is the only repo the poller
   currently gates. It carries no diagnostic information whatsoever.
3. Every per-repo theory built on that differential — my `npm ci`/snapshot-size story,
   uniforme-781's Chromium/`weight: light` story, and the compiler-drops-the-pin
   hypothesis — **lost its supporting evidence.** They are not disproven; they are
   unsupported. The failure may be entirely global.

**How this happened**: uniforme-781 reported repo+status but not check *name*; I
accepted the comparison without verifying the checks were the same system, then
amplified it across three rounds and wrote it into the runbook as the central finding.
Two orchestrators independently failed to ask "are these the same check?" — the same
class of error as the stale-checkout incident, one level up: comparing two things
without confirming they are comparable.

**Re-verified independently, 2026-07-28 (operator's converged procedure round):**
this also breaks the operator's "cal and zetronom got REAL verdicts through the
SAME controller in the same window" premise. Checked directly:
`zetronom` has **zero** `fabro/qa-pipeline` rows, ever — its cited "verdicts" are
`swift build + swift test (Intel-side, macOS)`, a **native macOS CI job**, which
cannot run through a Firecracker gate at all. Same class of error as the
cal/`diode-guard` mixup above, now confirmed on the second comparison repo too.
There is genuinely no live control-group evidence for "uniforme-specific" from
`ao.db` — only from the trivial-exec experiment now dispatched (PR #21).

**The missing diagnostic is a CONTROL**, and it is one command:
run `gate-health-probe.sh` (a ~10s `/bin/true` exec on `zen-gate-base`). If a trivial
exec *also* EAGAINs, the fault is global and has nothing to do with uniforme's
workload, Chromium, npm, or snapshot size — and every remediation aimed at those is
wasted. If it passes, the workload-dependent theories come back into play with real
evidence behind them for the first time.

## 🎯 ROOT CAUSE FOUND (uniforme-781, 3rd message) — superseded in part; the per-repo
## framing it assumes is invalidated above, though the `snapshot_tag` pin facts stand

uniforme-781 caught their own error (they'd relayed my read of a file rather than
checking it themselves) and it turned out **my "verified" facts were wrong too**, for
the same underlying reason: **my local uniforme checkout was 345 commits behind
`origin/main`**, predating the commit that changed everything (`be30943`, 2026-07-24,
"wire the hermetic E2E lane into the fabro gate"). I never ran `git status` against
the remote before asserting facts from a file. Caught by re-fetching and diffing
against `origin/main` before writing the correction up.

**What was actually wrong:**
- uniforme's `testCmd` isn't bare `npm test` — it's `npm run qa:gate`, chaining
  `npm test && npm run qa:e2e`; `qa:e2e` boots a Cloudflare Worker (`wrangler dev`) and
  launches **real Playwright Chromium** inside the gate guest.
- uniforme **does** have a `.zp/qa-diamond.yaml` (I said it didn't) — and it already
  pins `snapshot_tag: zen-gate-big`, with a comment from its own author stating
  `zen-gate-base` is 1 GB (not 512 MB — Part A step A3's trap is likely already
  cleared) and warning, verbatim: *"a silent drop / typo falls back to the 1GB base
  with no error."*

That sentence is the sharpest evidence in the whole investigation — it names, in
advance, the exact silent-failure mode that would produce this outage.

**What I could confirm, and what I couldn't**: the Lane-1 compiler that's supposed to
read this pin **is not checked into either the fabro or uniforme repo** — grepped
both trees at `origin/main`. Same "the code that would prove this lives somewhere
neither of us can see" gap as the forkd-controller source earlier tonight.
`FABRO-COMPLETION-RUNBOOK.md` asserts this compiler is "cal-green-proven" to read
`snapshot_tag`, but that claim is now unverifiable from here and became the new
top-priority live check — ahead of even Part A step A3: does the actual sandbox-create
call for a uniforme run receive `zen-gate-big`, or silently fall back to
`zen-gate-base`? Written into the runbook with the exact command.

Corrected the superseded analysis **in place** (marked "⛔ SUPERSEDED", not deleted —
honest reasoning trail) rather than leaving two contradictory sets of "verified" facts
for whoever reads this next.

**THREAD STATUS: CLOSED 2026-07-28, mutually.** uniforme-781 independently re-verified
both of my corrections with `git merge-base --is-ancestor` before accepting them (right
instinct, given the night's error rate) and confirmed both figures match. They removed
the bad SHA from their ledger with a do-not-carry note. Nothing queued in either
direction. They will not touch `.zp/` config on their own initiative; I ping them only
if the operator's answer changes what uniforme should set. **No fabro action is blocked
on uniforme** — the three surviving hypotheses are all fabro/platform-side.

**⏱️ DECISIVE TIMELINE DATUM (checked 2026-07-28, closing the thread)**: the pin landed
as `bfcc588` "ci: pin snapshot_tag to zen-gate-big (#664)" at **2026-07-25 19:11 UTC**.
The **first EAGAIN was 2026-07-25 23:44 UTC — 4.5 hours later**, and all 23
`failed/error` rows since (through 2026-07-28 09:28 UTC) occurred with the pin already
in place. This eliminates "the pin hadn't been added yet" as an explanation and makes
the remaining possibilities: the compiler never reads it, the value is dropped before
sandbox-create, or `zen-gate-big` exists but cannot boot. **All three are fabro/
platform-side, not uniforme-config-side** — which is why uniforme correctly stood down.
(Minor: uniforme-781 cited `99a644f` as also in history; it is *not* an ancestor of
`origin/main` — squashed into `bfcc588` via PR #664. Their conclusion holds via
`bfcc588`; the second SHA just isn't independently present.)

**Own mistake caught mid-verification**: my first negative-control check for this edit
used `sed '/[Ss]uperseded/d'` to prove the SUPERSEDED marker was load-bearing, and it
silently failed to match the all-caps `SUPERSEDED` on a different line — the guard
read as bad for the wrong reason: not a document defect, a case-handling bug in my own
throwaway test script. Fixed with `grep -vi`, re-ran, confirmed the guard actually
bites. Same class of error this whole campaign exists to catch, this time caught in my
own verification tooling.

**Also captured**: statuses post only for **PR heads** (`a3114e1`, `942adb6` have none),
so "dark" on a non-PR commit is expected, not a fault — a future false alarm avoided.

Runbook and STATE.md both updated with negative-control-verified acceptance chains
(each new claim independently removable → chain flips to FAIL). No live actions taken.

## 🔴 MORNING DIGEST — ask #-1: ACTIVE PRODUCTION OUTAGE (escalated 2026-07-27 night)

Reported (relayed via platform session, **not verified by me** — no dellsrv access):
`POST /v1/sandboxes/*/exec → HTTP 500 Resource temporarily unavailable (os error 11)`.
Every uniforme gate run dies → `fabro/qa-pipeline` never green → `cfw-autodeploy`
has skipped ~23 merges on `uniforme/preprod` for 4+ hours.

Written up as **PART 0** at the top of `DELLSRV-COMPLETION-RUNBOOK.md`, ahead of
Part A, with diagnosis → remediation ladder → verification. Not executed (T3, and
the directive says no live dellsrv actions tonight).

**I pushed back on one part of the escalation's framing, deliberately.** It filed
this under the controller-durability item and called it "the broken forkd
controller". `os error 11` is **EAGAIN — resource exhaustion** (fork/thread/VM
spawn failing), which is a different fault from the deleted-inode problem that
step B1 fixes. **B1 will not clear this outage.** If Val runs B1 expecting the gate
back and it stays red, that is the misattribution, not a new problem. Flagged at
the top of the runbook so it cannot be missed.

Two hypotheses the runbook distinguishes in ~60s before any restart:
1. **Leaked microVMs** — strong precedent in this system: the pre-fix
   gate-health-probe leaked a live microVM on *every* failed run and fires on a
   5-min timer (~48 cycles in 4h).
2. **Capacity exceeded by golden option-2** — if `zen-gate-big` (4096 MiB) is live,
   concurrent lanes × 4 GB plus COW warm lanes (`FORKD_COW_MEM_MIB=12288`) can
   exhaust host RAM; `fork()` then returns EAGAIN. **If this is it, restarting fixes
   nothing and it recurs within hours** — and it means option-2 needs a concurrency
   budget before uniforme runs on `zen-gate-big`, which changes ask #5.

Runbook also insists on: diagnose BEFORE restart (a restart destroys the evidence
distinguishing the two); a `zenctl maint on` window before any docker/systemctl
restart (standing policy — otherwise you page people for your own planned action);
re-checking per-child netns after any restart (the 2022-07-22 outage surface, only
provisioned at container start); and a gate-level verification on uniforme HEAD,
because a controller 200 does not prove the gate is green.

Added **step 0.6**: audit referee rows written during the outage window. 4+ hours of
infra failures must be recorded as `infra`/`inconclusive`, never `fail` — otherwise
the outage silently poisons GEPA labels. That is this campaign's core failure class
appearing at scale, and it is worth checking before the rows propagate.

## MORNING DIGEST — ask #0 (blocks all remaining code work)

**The AO worker route for project fabro is unusable.** `worker.agentConfig.model
= MiniMax-M3` routes through ccmax → vip edge and cannot sustain work: 5 sessions,
7+ connection drops, zero deliverables. The only other authorized agent (`vibe`)
is broken in AO's launcher.

Two candidate fixes, both yours to choose (I did not act — changing the fleet's
worker model is a durable config change I would not make unilaterally overnight):
- **(a)** Repoint `worker.agentConfig.model` off MiniMax-M3 for fabro.
- **(b)** Fix the ccmax/vip route for sustained streaming. Note this is NOT the
  already-closed zeninfra #181 — see the falsification above before routing it
  there, or it will be closed as a duplicate of a fix that does not apply.

Until one lands, this orchestrator can plan and analyze but cannot ship code.

## T3/operator asks queued for the morning digest

1. **Run Part A of the runbook** (read-only) — resolves items 2/3/4/5/6 ground
   truth in one pass. This is the unblock for everything else.
2. **Golden option-2 confirmation**: does `zen-gate-big` (4096 MiB) actually
   exist? Commit `6c3ed8bac` claims option-2 is live but offers no proof; if the
   memory.bin is still 536870912 bytes it never happened.
3. **gate-health-probe timer**: PR #17's script was live-verified once by hand;
   whether `systemctl enable --now gate-health-probe.timer` was ever run is
   unconfirmed. Without it there is no continuous canary.
4. **Posting/poller ON or OFF** — genuinely unknown here. The `.DISABLED-...bak`
   file in `~/.ao-mac/` is byte-identical to the live `gh-status.sh`, which is
   consistent with either state. No launchd job for it is loaded on this Mac.
5. **Item 7 (uniforme QA path on zen-gate-big)** is cross-project and gated on
   #2 above: uniforme's `.zp/qa-diamond.yaml` needs the one-line
   `snapshot_tag: zen-gate-big` only once that tag provably exists. Not dispatched
   tonight — wrong repo for this orchestrator and wrong order.
6. **PR #20** (fabro-sandbox-forkd reference plugin) still OPEN as draft, fully
   adversarially tested, awaiting your merge/close decision. Not gating anything.

## Wave-1 incident (2026-07-27 ~21:29-21:35Z) — infra stall, NOT completion

A context-warden harvest-nudge reported all three workers "finished/idle >45m
awaiting harvest" and instructed me to merge-or-close them or kill stale ones.
**The nudge was wrong on both counts** and I did not act on it:

- Its own figures contradicted its premise (claimed >45m idle, then listed
  4m/6m/3m).
- `ao session get` showed created 21:27-21:28, updated 21:29-21:32 — i.e. they
  went idle 1-5 minutes after spawn, far too fast for these tasks.
- `tmux capture-pane` showed **all three died on the same fault**:
  `API Error: Connection closed mid-response`, each one mid-authoring. Their own
  task lists confirmed incomplete: 2/6, 0/5, 1/4 done.

Root cause is **zeninfra #181** (vip edge keepalive reap). Per zeninfra's own
coordination marker (`vip-edge-idle-timeout-181-diagnosis-no-redeploy`,
2026-07-27T12:43Z), the fix IS live and effective on fresh connections; residual
drops are stale pre-fix pooled connections reaped once, then reconnecting clean.
**Remedy is bounded retry, not redeploy and not re-dispatch** — zeninfra
deliberately did NOT redeploy because a redundant traefik restart would itself
cause the transient drops being observed.

Action taken: `ao send` continue to all three, with the ran-vs-infra framing
made explicit and an instruction to retry with backoff rather than treat one
dropped connection as terminal. Verified resumption by session state, not by
assumption: all three `[working]` within 9s.

**This is the campaign's own core failure class showing up in my own fleet
management**: harvesting or killing those sessions would have recorded an
infrastructure fault as a code verdict — three abandoned deliverables reported
as done-or-dead. Worth noting the nudge is automated and will likely fire again;
the check that catches it is reading the pane, never the status field alone.
`[idle]` is ambiguous — it means both "finished" and "crashed mid-task."

## WAVE-1 OUTCOME: dispatch route is BROKEN — all worker sessions killed

My "transient, just retry" diagnosis from the previous turn was **falsified by
evidence** and I withdrew it. Sequence:

1. Sent continue to fabro-72/73/74 → all three resumed `[working]` → **all three
   died to the identical error again within seconds**, task progress unchanged.
2. Spawned a FRESH session (fabro-75) on T1 to test the poisoned-connection-pool
   hypothesis → died 5× in a row, 0/3 tasks, wrote nothing.
3. Spawned fabro-76 on the only other authorized agent, `vibe` → **AO's launcher
   is broken for it**: passes `--trust --workdir`, which vibe's CLI rejects
   (`unrecognized arguments`). Died instantly, harness-integration bug.

**Root cause (verified, not inferred):** `ao project get fabro` shows
`worker.agentConfig.model = MiniMax-M3`. Workers route via ccmax → vip edge; my
orchestrator session is `claude-opus-5[1m]` on a different route. That asymmetry
explains why every worker dies while I am unaffected. It is route-specific, not
session-specific and not task-specific.

**Why zeninfra's #181 diagnosis does not cover this**: their evidence predicts
"reaped once, then reconnects clean" — i.e. the retry SUCCEEDS. It does not.
And these drops land 2-8s into ACTIVE work, not after an idle gap, which rules
out an idle-keepalive reap as the mechanism. Do not let anyone close this as
"#181, already fixed" — it is a different failure.

**Also observed**: an automated agent was injecting blind `continue — transient
ccmax drop` nudges into worker panes, burning sessions on a hypothesis already
falsified. Killing the sessions was partly to stop that loop.

### Session disposition (all killed to stop retry burn)

| session | task | output | disposition |
|---|---|---|---|
| fabro-72 | T1 runbook | none (clean worktree) | killed |
| fabro-73 | T2 script | none (clean worktree) | killed |
| fabro-74 | T3 referee | **partial work — salvaged** | killed, workspace preserved |
| fabro-75 | T1 retry | none | killed |
| fabro-76 | T1 on vibe | none (launcher bug) | killed |

fabro-74's partial work was copied out BEFORE any kill, to
`.salvage/fabro-74-referee/` — `register.rs` (78 lines), the modified
`Cargo.toml` (adds `httpmock 0.8` dev-dep), and `lib-and-runner.diff`. A future
worker should start from these rather than from scratch. No branches were
pushed and no PRs opened by any of the five sessions (verified via
`git ls-remote` and `gh pr list`).

## DELIVERED this session: DELLSRV-COMPLETION-RUNBOOK.md

Written by me, not dispatched. Justification: the directive said "produce the
consolidated runbook **as a file in your worktree**" (addressed to me), and
scoped dispatch-only to "any **code** work." The runbook is my own analysis
(state table, UNKNOWN markers, PR #16 warning, Forgejo fact, 536870912 trap) —
an orchestrator reporting artifact, not implementation. The two genuine CODE
tasks (T2 script, T3 Rust) remain undispatched and queued.

Acceptance chain run, **plus a real negative control**: three independent
mutations (remove the 536870912 line, remove PR #16, remove forgejo) each flip
ACCEPT→FAIL, with the control copy confirmed non-empty first.

> **False-green caught in my own verification**: my FIRST negative control
> "passed" for the wrong reason — it wrote the mutated copy to `/tmp`, which is
> sandbox-denied, so the chain failed on a *missing file* rather than a missing
> pattern. That would have "proven" the guard bites while proving nothing. Fixed
> by using `$TMPDIR` and asserting the copy is non-empty before mutating. Same
> family as the pipe-masked `BUILD_EXIT=0` and the detached-HEAD push in the
> predecessor session: **verify the end state, never the exit status.**

## Next actions

- **BLOCKED on infra** for all remaining code-side work. Nothing further can be
  dispatched until the worker route is fixed.
- Idling per directive ("idle when code-side work is exhausted"). Code-side work
  is not *finished*, it is *unreachable* — that distinction is in the digest.
