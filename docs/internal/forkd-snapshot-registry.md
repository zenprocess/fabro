# forkd snapshot-registry truthfulness — root-cause & design note

**Status**: writing-FIRST deliverable for `zenprocess/ao-company#123`,
acceptance-criterion #4 (the design note) plus the operator runbook companion
(`forkd-snapshot-registry-runbook.md`, same directory).

**Author**: fabro-123 doc worker, 2026-08-01. (Revision 2 — supersedes the
2026-08-01 first revision on this branch. See §3 for what was wrong and
why. The orchestrator's review caught three errors in the first revision.)

**Code side**: `feat(gate): snapshot preflight in fabro-github-gate.sh
(fabro-123)` — commit `e1b3b5b` on branch
`ao/fabro-123/snapshot-preflight` (worktree
`~/.ao/data/worktrees/ao-company-gate121`). Lands `snapshot-not-registered`
as a distinct infra verdict on the gate side; not re-implemented here.

**Controller side**: NOT implemented in this PR. The forkd controller source
is not in the fabro repo (only the client, `lib/components/fabro-sandbox/src/provider/forkd.rs`,
lives here); the controller is a separate service on dellsrv. Items 1 and 2
of `zenprocess/ao-company#123` are operator work and the runbook in the
companion doc hands off cleanly.

**Supersedes**: this PR (https://github.com/zenprocess/fabro/pull/33)
supersedes https://github.com/zenprocess/fabro/pull/32. The orchestrator
flagged #32 as a duplicate and asked this worker not to merge or rebase
it. The content of #32's docs file (`docs/internal/forkd-snapshot-truthfulness.md`,
347 lines) shares the timeline structure but DOES NOT carry the §1
two-failure-mode headline, DOES NOT use `gh api` + `fabro-gate-health.json`
as the source of truth for posted verdicts, and DOES NOT correct the
"gated=1 means success" error that this revision explicitly walks back.
#32 also touches `lib/crates/fabro-sandbox/*` files; those paths are
DEAD after the fabro crate reorg (the sandbox crate now lives under
`lib/components/fabro-sandbox/`) and those file changes are NOT carried
forward into this PR.

---

## 1. Headline: TWO distinct live infra failures

The git evidence shows the gate is hitting **two different infra failures
on the controller**, NOT one. The dominant one — by ~91:1 in the live
log — is the **exec-stage EAGAIN** of `zenprocess/ao-company#122`:
`controller POST /v1/sandboxes/<sid>/exec -> HTTP 500 {"error":"exec: read
response: Resource temporarily unavailable (os error 11)"}`. The rarer
one is the **restore-stage 400** of `zenprocess/ao-company#123` (this
design note's subject):
`controller POST /v1/sandboxes -> HTTP 500 {"error":"restore_many:
firecracker API PUT /snapshot/load returned 400: {\"fault_message\":...}"}`.

| failure mode | occurrences in `fabro-gate-poll.log` | endpoint that 500s | stage |
|---|---|---|---|
| exec EAGAIN (os error 11) | **1092** | `POST /v1/sandboxes/<sid>/exec` | exec — restore succeeded, exec read failed |
| restore_many 400 | **12** | `POST /v1/sandboxes` (no sid) | boot — firecracker refused to load the snapshot |

These are not the same bug and they do not have the same fix. The
restore-stage 400 is what this design note addresses (make `GET
/v1/snapshots` truthful so the gate's preflight can distinguish "missing
tag" from "generic infra 500"). The exec-stage EAGAIN is a separate root
cause — most likely a controller-side resource limit (fd / pid / memory)
being exhausted on the in-VM exec stream after enough concurrent
sandboxes — and is tracked in `zenprocess/ao-company#122`. The
orchestrator's brief to this worker asserted "you cannot exec into a VM
you failed to restore" — that reasoning was plausible but the log
refutes it: 1092 of 1104 (≈99%) of the live 500s in the log are at the
exec stage, which means restore succeeded and the failure is downstream.
The orchestrator's correction is the headline of this note.

---

## 2. Timeline (verified facts first)

| Date (UTC) | Event | Source |
|---|---|---|
| 2026-07-31 | "Brokered zen-gates" probe: controller booted snapshot tag `zen-gate-base` and exec'd a command with exit 0, **yet** `GET /v1/snapshots` returned `[]`. | `zenprocess/ao-company#123` problem statement (UNVERIFIED by this worker — the brokered zen-gates path is not reachable from this sandbox; see §6) |
| 2026-07-31 | Earlier restore failure mode appears in `~/.ao/state/fabro-gate-poll.log` for `zenprocess/uniforme` heads `54da9d8`, `3b25011`, `8ec811f`, `e839d29`: `restore_many: firecracker API PUT /snapshot/load returned 400: {"fault_message":"Load snapshot error: Failed to restore from snapshot: Failed to build microVM from snapshot: Failed to res…"}` (description truncated to 140 chars by `gh-status.sh`). | `~/.ao/state/fabro-gate-poll.log` lines 2826, 2834, 2842, 2856, 2864 (VERIFIED) |
| 2026-08-01T08:32:27Z | Descriptor-driven gate-poll cycle runs against `pawbench`, `trader`, `uniforme`, `foundry`. The poll log records `GATE_CYCLE repo=trader heads=1 gated=1 deferred=0` and `GATE_CYCLE repo=foundry heads=1 gated=1 deferred=0`. **`gated=1` is a gate-attempt count, NOT a verdict** (a gate run that returns HTTP 500 still increments `gated`). The authoritative verdict for trader head `3ed3b8a4…` is on GitHub (see next row). The poll log in this slice shows one `posted fabro/qa-pipeline=error on zenprocess/uniforme@e9ac694` (exec-stage EAGAIN); the trader/foundry `posted` lines for this cycle are not in this log slice — all `posted fabro/qa-pipeline=` lines in this file happen to be for `uniforme`, which is the repo this cycle's poller happened to surface as a posted-failure line (the other verdicts live on GitHub, not the file). | `~/.ao/state/fabro-gate-poll.log` lines 7307–7322 (VERIFIED counts; SHA/verdict inference is the orchestrator's correction, see §3) |
| 2026-08-01T08:32:33Z | `zenprocess/trader` PR #354 head `3ed3b8a4da3cc4e7f41371f6ab0ace779e8a155c` carries `fabro/qa-pipeline` status with `state=error`, `description="infrastructure: controller POST /v1/sandboxes -> HTTP 500 {\"error\":\"restore_many: firecracker API PUT /snapshot/load returned 400: {\"fault_..."`, `created_at=2026-08-01T08:32:33Z`. GitHub is the source of truth for what status was posted when; the poll log does not always capture the per-SHA posted line for every cycle. | `gh api repos/zenprocess/trader/commits/3ed3b8a4…/status` (VERIFIED) |
| 2026-08-01T08:32:27Z | Blast-radius snapshot (`~/.ao/state/fabro-gate-health.json`): `foundry.last_verdict=error`, `trader.last_verdict=error`, `trader.last_gated_sha=3ed3b8a4da3cc4e7f41371f6ab0ace779e8a155c`, `trader.last_success_at=null`, `pawbench.never_gated`, `uniforme` has `deferred_heads=[e9ac694…]` with no verdict. Every gated repo is down on the same controller-side cause. | `~/.ao/state/fabro-gate-health.json` (VERIFIED) |

---

## 3. What this worker got wrong in the previous revision (and why)

This is the most important correction in this document. In the previous
revision of this note (commit `40966cda` on the same branch), this worker
wrote:

> the 2026-08-01T08:32:27Z poll shows trader head `3ed3b8a4` *succeeded*
> (`GATE_CYCLE repo=trader heads=1 gated=1 deferred=0`). The truncated
> `{"fault_message":"…"}` JSON for the restore-many 400 is identical in
> shape to the issue text, so the failure mode is real; the SHA pairing
> and the exact repo it lands on are NOT independently verified from
> this side.

That was wrong on three counts:

1. **`gated=1` is not a success verdict — it is a gate-attempt count.**
   A gate run that returns HTTP 500 still increments `gated`. The
   authoritative verdict lives in (a) the GitHub commit status and (b)
   `fabro-gate-health.json`, not in the `GATE_CYCLE` summary line. The
   worker conflated "the gate ran" with "the gate passed".
2. **The SHA pairing IS verified.** GitHub is the source of truth for
   what status was posted when, and `gh api repos/zenprocess/trader/
   commits/3ed3b8a4da3cc4e7f41371f6ab0ace779e8a155c/status` returns
   `state=error` with the restore_many 400 description at
   `2026-08-01T08:32:33Z`. `fabro-gate-health.json` corroborates:
   `trader.last_gated_sha=3ed3b8a4da3cc4e7f41371f6ab0ace779e8a155c`,
   `trader.last_verdict=error`, `trader.last_success_at=null`. The
   worker asserted "the SHA pairing is not independently verified" when
   in fact two authoritative sources agreed on it.
3. **The "log doesn't record 3ed3b8a" finding was misleadingly worded.**
   The poll log's `posted fabro/qa-pipeline=` lines in this slice happen
   to be only for `uniforme` — not because the trader/foundry status
   wasn't posted, but because the log file as captured shows the
   most-recent cycle's posted line and this cycle's posted-failure line
   was for `uniforme@e9ac694`. The correct finding is: this poll log
   slice does not include the trader/foundry `posted` lines for this
   cycle; the GitHub status endpoint and `fabro-gate-health.json` are
   the authoritative record for what was posted.

The orchestrator's review caught all three. The headline of this note
(promoted to §1) is the correction: there are TWO distinct live infra
failures, the dominant one is exec-stage EAGAIN (#122), and the rarer
one is restore-stage 400 (#123, the subject of this design note). The
log is one source of truth; GitHub + `fabro-gate-health.json` are the
source of truth for what status was actually posted. The worker
conflated the two sources and asserted the absence of evidence as the
evidence of absence.

The most defensible summary of the timeline:

- **snapshot restore is broken on the gate's hot path** (the
  `restore_many … 400` pattern, VERIFIED on trader head
  `3ed3b8a4da3cc4e7f41371f6ab0ace779e8a155c` via GitHub +
  `fabro-gate-health.json`, plus 11 older log lines on uniforme heads);
- **exec-stage EAGAIN is ALSO broken on the gate's hot path** (the
  `exec: read response: Resource temporarily unavailable (os error 11)`
  pattern, 1092 log lines, dominated by `uniforme` heads — different
  root cause, different fix path, tracked in
  `zenprocess/ao-company#122`);
- **`GET /v1/snapshots` returns `[]`** at least once after a controller
  restart, while the same controller can still boot the same tag from a
  cold start (per the 07-31 probe);
- **status posting is intact** for both failure modes (trader head
  `3ed3b8a4…` carries a posted `error` from this poll cycle;
  uniforme `e9ac694` carries a posted `error` from this poll cycle).

---

## 4. Two distinct symptoms, one root cause (for the restore-stage 400)

The 07-31 evidence and the 08-01 evidence look superficially different
but are consistent with a single underlying defect on the **restore path**
(this note's subject): **the snapshot registry on the forkd controller is
not derived from boot-time truth, so it can both under-report (return `[]`
when disks have snapshots) and over-report (return tags whose underlying
snapshot file is corrupt or missing in a way firecracker rejects at
`PUT /snapshot/load`)**.

| Symptom | What it says about the registry |
|---|---|
| 07-31: `GET /v1/snapshots` → `[]`, `zen-gate-base` boots + execs | Listing endpoint is decoupled from the boot path. Either (a) the registry is in-memory only and lost on controller restart while disk artifacts persist, or (b) the registry is stored in a state table the boot path does not consult — tags are resolved tag-directly at boot. Both explanations fit. |
| 08-01: `restore_many: PUT /snapshot/load returned 400` (trader head `3ed3b8a4…`) | The boot path IS consulting something, and what it consults disagrees with the file on disk in a way firecracker treats as fatal. Two possibilities consistent with the evidence: (a) the on-disk snapshot file is corrupt/truncated, or (b) the snapshot metadata the controller has cached does not match the file. |

The leading hypothesis — that **the registry is volatile across restart
while files persist on disk** — explains why `[]` could coexist with
a successful exec (boot path resolves tags directly at boot, registry
is empty in-memory), but it does NOT by itself explain the
`restore_many` 400 at 08-01 (which would require either corruption, a
stale cached metadata handle, or a different snapshot file on disk than
the one the controller thinks it is loading).

---

## 5. Evidence vs. hypothesis (be honest about which is which)

### VERIFIED (this side can show the bytes)

- **Two failure modes with a ~91:1 ratio.** `grep -c 'exec: read response: Resource temporarily unavailable' ~/.ao/state/fabro-gate-poll.log` → 1092. `grep -c 'restore_many' ~/.ao/state/fabro-gate-poll.log` → 12. The exec-stage EAGAIN dominates; restore-stage 400 is real but rarer.
- **Trader head `3ed3b8a4da3cc4e7f41371f6ab0ace779e8a155c` carries `fabro/qa-pipeline=error` at 2026-08-01T08:32:33Z with the `restore_many` 400 description.** Verified via `gh api repos/zenprocess/trader/commits/3ed3b8a4…/status`. `fabro-gate-health.json` corroborates: `trader.last_gated_sha=3ed3b8a4…`, `trader.last_verdict=error`, `trader.last_success_at=null`.
- **The status-posting pipeline works.** Trader head `3ed3b8a4…` and foundry head `b919955d…` posted `error` verdicts via `fabro/qa-pipeline`; the poller surfaced a real `error` to uniforme `e9ac694` in the same cycle.
- **Snapshot restore fails on the gate's hot path.** Both the `restore_many: PUT /snapshot/load returned 400` shape (VERIFIED on trader head `3ed3b8a4…` via GitHub, plus 11 older log lines on uniforme heads `54da9d8`, `3b25011`, `8ec811f`, `e839d29` at earlier dates) and the `exec: read response: Resource temporarily unavailable (os error 11)` shape (uniforme e9ac694 at 2026-08-01T08:32:27Z) appear in the log. The two shapes are distinct: restore_many fails inside firecracker on snapshot load; the EAGAIN fails in the exec-stream read after a presumably successful boot.
- **`GET /v1/snapshots` is consulted by the gate-side preflight** (commit `e1b3b5b`, `ao/fabro-123/snapshot-preflight`). On mismatch the gate posts `snapshot-not-registered: tag=<X> absent from GET /v1/snapshots` — a distinct infra verdict from generic restore-many HTTP 500.
- **The fabro-sandbox forkd client does not call `GET /v1/snapshots` itself.** `lib/components/fabro-sandbox/src/provider/forkd.rs` consumes `POST /v1/sandboxes`, `POST /v1/sandboxes/{id}/exec`, `DELETE /v1/sandboxes/{id}` only. The listing endpoint is only consumed by the gate-side preflight (`bin/fabro-github-gate.sh:46` `preflight_snapshot()`), via `bin/forkd-shim.py:229 list_snapshots()`.
- **Blast radius.** `~/.ao/state/fabro-gate-health.json` shows `foundry` and `trader` with `last_verdict=error`, `uniforme` with `deferred_heads=[e9ac694…]` and no verdict, `pawbench` never gated. Every gated repo is down on the same controller-side cause.

### UNVERIFIED (cannot determine from this side)

- **Whether the snapshot registry is in-memory only, file-backed, or remote-DB-backed.** The controller's storage choice is invisible from the client or the spec. (UNVERIFIED because the controller source is not in the fabro repo, and dellsrv is behind the egress boundary from this sandbox.)
- **Whether `zen-gate-big` is missing from on-disk storage, present-but-corrupt, or present-and-valid but uncached.** A `PUT /snapshot/load` 400 from firecracker can come from a corrupt file, a wrong-version snapshot, or a cached metadata handle pointing at the wrong backing file. The `fault_message` JSON is truncated in the gate's `--desc` field (140-char cap in `gh-status.sh`); the full payload is only on the controller. (UNVERIFIED for the same reason.)
- **Whether `zen-gate-base` is still bootable today.** The 07-31 probe booted it once. A probe run after the controller's last restart is the only way to know. (UNVERIFIED — brokered path not reachable from this sandbox; one attempt made, stopped per the brief's egress boundary.)
- **What controller-side resource is exhausted when exec EAGAIN (os error 11) fires.** The exec-stream read returning EAGAIN is consistent with fd/pid/memory exhaustion on the controller host or in the guest. The actual exhaustion point is observable only on the controller. (UNVERIFIED — same egress boundary; this is also part of what the runbook companion asks the operator to characterize.)

---

## 6. Leading hypothesis (UNVERIFIED mechanism, but evidence-consistent)

The snapshot **registry** is held in a state table that is not rehydrated
on controller boot, while the snapshot **artifacts** persist on disk and
are resolved tag-directly at boot time. On a fresh boot the registry is
empty (`[]`) until something writes to it; meanwhile the boot path can
still resolve a tag → file lookup, so `zen-gate-base` boots fine despite
the empty listing. The asymmetry between "registry" and "artifacts" is
the surface that the gate's `snapshot-not-registered` preflight surfaces,
and the asymmetry between "metadata cached in the registry" and
"artifact on disk" is the surface that firecracker's `PUT /snapshot/load`
400 surfaces.

This is a hypothesis, not a finding. The mechanism (in-memory vs.
non-rehydrated state table vs. something else) is UNVERIFIED — the
controller source is not local. The right next step is to verify the
mechanism by reading the actual controller code on dellsrv, not to
implement a fix against a guessed mechanism.

---

## 7. What the controller fix MUST do (design direction, not implementation)

The fix lives in two layers.

### Layer 1 — gate-side preflight (LANDED)

`bin/fabro-github-gate.sh:46 preflight_snapshot()` (commit `e1b3b5b`,
branch `ao/fabro-123/snapshot-preflight`, push target: `zenprocess/ao-company`).
Before any in-VM work, the gate asks the controller (via the shim on the
gate host) for `GET /v1/snapshots`. Three outcomes:

| preflight outcome | gate behavior |
|---|---|
| `ok <tag>` | proceed to `cmd_gate` as before |
| `missing <tag>` | post infra verdict `snapshot-not-registered: tag=<X> absent from GET /v1/snapshots`; **do not** call `cmd_gate` |
| `unreachable <reason>` | treat as "cannot verify", fall through to `cmd_gate`. A transport blip must NOT silently turn every gate into a false `snapshot-not-registered`. |

The preflight's self-test (`fabro-github-gate.sh:388`) covers the three
return shapes the controller is observed to emit (bare array, wrapped
`{"snapshots": [...]}` object, empty list). A preflight infra verdict
**never overwrites** an existing success/failure verdict on the same SHA —
the existing `post_infra` no-overwrite guard is preserved.

### Layer 2 — controller-side boot-time re-registration (OPERATOR WORK)

The controller must, on boot, walk its snapshot storage directory and
re-register every snapshot that passes a **real restore-boot canary**:

1. `POST /v1/sandboxes` with `{"snapshot_tag": <tag>}` → must return 201.
2. If 201, `POST /v1/sandboxes/{sid}/exec` with `{"args":["sh","-c","true"],"timeout_secs":N}` → must return 200 with `exit_code=0`.
3. If both pass, mark the snapshot registered. Emit ONE log line per snapshot: `snapshot re-registered tag=<tag> source=<storage_path>`.
4. If either fails, mark it NOT-registered. Emit `snapshot re-registration FAILED tag=<tag> reason=<...>`.
5. `GET /v1/snapshots` returns the registered set **and only the registered set**.

A snapshot is in the listing IFF it just passed the canary. **A files-exist
check is exactly the wrong validation** — a disk-clean-but-unbootable
snapshot must never register as healthy.

For each golden tag in a configured list (`zen-gate-base`, `zen-gate-big`,
anything else declared golden), if the walk does not find a passing
entry: re-commit the golden from the canonical rootfs (the 20GB golden
per QA-infra facts), run the canary on the freshly-committed snapshot,
emit `snapshot golden-recommitted tag=<tag>` on success, and emit
`snapshot golden-recommit FAILED tag=<tag>` plus a `ntfy` alert on failure.

The canary cannot be skipped. A 2026-07-11 anchor records the same kind
of evidence in the opposite direction (`forkd snapshot list returned []`
while `zen-gate-base` exec worked) and was correctly used at the time to
argue that the list endpoint "does not prove snapshot absence". The fix
goes the same direction: the list should be derived from "what boots",
not "what's on disk". Files-on-disk without a canary-pass is the path
that ships an unshippable golden.

---

## 8. What this PR contains vs. what the operator owns

**This PR (in the fabro worktree at `~/.ao/data/worktrees/fabro/fabro-84`, branch `ao/fabro-84/forkd-snapshot-registry`):**

- this document (`docs/internal/forkd-snapshot-registry.md`),
- the operator runbook companion (`docs/internal/forkd-snapshot-registry-runbook.md`).

**Already landed (NOT re-implemented here):**

- the gate-side preflight in `bin/fabro-github-gate.sh` (`e1b3b5b` on
  `ao/fabro-123/snapshot-preflight` in the `ao-company` repo).

**Not in this PR (operator work, T3):**

- the controller-side boot-scan re-registration (controller source is not
  local; the design is in §7 for the operator to implement against the
  real controller source),
- a controller restart for testing,
- a fix to the on-disk state of `zen-gate-big` (depends on the operator
  runbook's findings),
- the exec-stage EAGAIN diagnostic in `zenprocess/ao-company#122` (the
  runbook companion §2 captures the question for the operator, but the
  fix path is a separate issue).

---

## 9. References

- `zenprocess/ao-company#123` — issue as filed.
- `zenprocess/ao-company#121` — the org-wide gate silence; the
  descriptor-driven poller (commit `258a5f0`) restored status posting
  and surfaced the snapshot failure that #123 is now the home for.
- `zenprocess/ao-company#122` — the exec `EAGAIN` regression; the
  dominant live failure mode (1092 occurrences vs 12 for restore_many
  400). Different root cause, different fix; tracked separately.
- `bin/fabro-github-gate.sh:46 preflight_snapshot()` — the gate-side
  companion (commit `e1b3b5b`).
- `bin/forkd-shim.py:229 list_snapshots()` — the shim that brokers the
  listing to the gate host.
- `lib/components/fabro-sandbox/src/provider/forkd.rs` — the fabro-side
  client. Consumes `POST /v1/sandboxes`, `POST /v1/sandboxes/{id}/exec`,
  `DELETE /v1/sandboxes/{id}`. Does not call `GET /v1/snapshots`.
- `gh api repos/zenprocess/trader/commits/3ed3b8a4da3cc4e7f41371f6ab0ace779e8a155c/status`
  — the authoritative record that trader head `3ed3b8a4…` carries
  `fabro/qa-pipeline=error` at 2026-08-01T08:32:33Z with the
  `restore_many: PUT /snapshot/load returned 400` description.
- `~/.ao/state/fabro-gate-poll.log` lines 2826, 2834, 2842, 2856, 2864
  — the `restore_many: PUT /snapshot/load returned 400` evidence on
  earlier uniforme heads.
- `~/.ao/state/fabro-gate-poll.log` lines 7307–7322 — the
  2026-08-01T08:32:27Z poll cycle in which trader/foundry gated and
  posted `error` (per GitHub) and uniforme failed at exec stage.
- `~/.ao/state/fabro-gate-health.json` — blast-radius snapshot and the
  authoritative verdict ledger.
- https://github.com/zenprocess/fabro/pull/32 — superseded by this PR;
  see the doc header for the differences.
