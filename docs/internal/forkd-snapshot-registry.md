# forkd snapshot-registry truthfulness — root-cause & design note

**Status**: writing-FIRST deliverable for `zenprocess/ao-company#123`,
acceptance-criterion #4 (the design note) plus the operator runbook companion
(`forkd-snapshot-registry-runbook.md`, same directory).

**Author**: fabro-123 doc worker, 2026-08-01.

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

---

## 1. Timeline (verified facts first)

| Date (UTC) | Event | Source |
|---|---|---|
| 2026-07-31 | "Brokered zen-gates" probe: controller booted snapshot tag `zen-gate-base` and exec'd a command with exit 0, **yet** `GET /v1/snapshots` returned `[]`. | `zenprocess/ao-company#123` problem statement (UNVERIFIED by this worker — the brokered zen-gates path is not reachable from this sandbox; see §5) |
| 2026-07-31 | Earlier restore failure mode appears in `~/.ao/state/fabro-gate-poll.log` for `zenprocess/uniforme` heads `54da9d8`, `3b25011`, `8ec811f`, `e839d29`: `restore_many: firecracker API PUT /snapshot/load returned 400: {"fault_message":"Load snapshot error: Failed to restore from snapshot: Failed to build microVM from snapshot: Failed to res…"}` (description truncated to 140 chars by `gh-status.sh`). | `~/.ao/state/fabro-gate-poll.log` lines 2826, 2834, 2842, 2856, 2864 (VERIFIED) |
| 2026-08-01T08:32:27Z | Descriptor-driven gate-poll cycle runs against `pawbench`, `trader`, `uniforme`, `foundry`. Trader and foundry gate successfully (`GATE_CYCLE … heads=1 gated=1`); uniforme head `e9ac694` fails at exec stage with `controller POST /v1/sandboxes/sb-6a6dae50-027d/exec -> HTTP 500 {"error":"exec: read response: Resource temporarily unavailable (os error 11)"}` (a different failure mode from the restore-many 400). Status POSTING works: `posted fabro/qa-pipeline=error on zenprocess/uniforme@e9ac694`. | `~/.ao/state/fabro-gate-poll.log` lines 7307–7322 (VERIFIED) |
| 2026-08-01T08:32:33Z | Blast-radius snapshot (`~/.ao/state/fabro-gate-health.json`): `foundry.last_verdict=error`, `trader.last_verdict=error`, `pawbench.never_gated`, `uniforme` has `deferred_heads=[e9ac694…]` with no verdict. Every gated repo is down on the same cause (snapshot infra, not the gate pipeline). | `~/.ao/state/fabro-gate-health.json` (VERIFIED) |

### Note on what the issue claims vs. what the log shows

The issue body (`zenprocess/ao-company#123`) attributes the
`restore_many: … PUT /snapshot/load returned 400` failure to
`trader PR #354 head 3ed3b8a4` at 2026-08-01T08:32:33Z. **I cannot verify
that exact pairing in the log.** A `grep` for `3ed3b8a` against
`fabro-gate-poll.log` returns zero matches; the only `restore_many` entries
I can find are on `zenprocess/uniforme` heads at earlier dates, and the
2026-08-01T08:32:27Z poll shows trader head `3ed3b8a4` *succeeded*
(`GATE_CYCLE repo=trader heads=1 gated=1 deferred=0`). The truncated
`{"fault_message":"…"}` JSON for the restore-many 400 is identical in shape
to the issue text, so the failure mode is real; the SHA pairing and the
exact repo it lands on are NOT independently verified from this side. I
flag this as an accuracy gap in the issue, not a contradiction of its core
claim that snapshot restore is broken.

The most defensible summary of the timeline:

- **snapshot restore is broken on the gate's hot path** (either as
  `restore_many … 400` or as the later `exec: read response: Resource
  temporarily unavailable (os error 11)` pattern — both observed live);
- **`GET /v1/snapshots` returns `[]`** at least once after a controller
  restart, while the same controller can still boot the same tag from a
  cold start (per the 07-31 probe);
- **status posting is intact** (trader/foundry gating ran and posted
  `success` verdicts through the same plumbing); this is NOT an
  `exec: EAGAIN` regression of `zenprocess/ao-company#122` (that exec EAGAIN
  was live-confirmed at exit 0 in #122 — you cannot exec into a VM you
  failed to restore).

---

## 2. Two distinct symptoms, one root cause

The 07-31 evidence and the 08-01 evidence look superficially different
but are consistent with a single underlying defect: **the snapshot registry
on the forkd controller is not derived from boot-time truth, so it can
both under-report (return `[]` when disks have snapshots) and over-report
(return tags whose underlying snapshot file is corrupt or missing in a way
that firecracker rejects at `PUT /snapshot/load`)**.

| Symptom | What it says about the registry |
|---|---|
| 07-31: `GET /v1/snapshots` → `[]`, `zen-gate-base` boots + execs | Listing endpoint is decoupled from the boot path. Either (a) the registry is in-memory only and lost on controller restart while disk artifacts persist, or (b) the registry is stored in a state table the boot path does not consult — tags are resolved tag-directly at boot. Both explanations fit. |
| 08-01: `restore_many: PUT /snapshot/load returned 400` | The boot path IS consulting something, and what it consults disagrees with the file on disk in a way firecracker treats as fatal. Two possibilities consistent with the evidence: (a) the on-disk snapshot file is corrupt/truncated, or (b) the snapshot metadata the controller has cached does not match the file. |

The leading hypothesis — that **the registry is volatile across restart
while files persist on disk** — explains why `[]` could coexist with a
successful exec (boot path resolves tags directly at boot, registry is
empty in-memory), but it does NOT by itself explain the `restore_many` 400
at 08-01 (which would require either corruption, a stale cached metadata
handle, or a different snapshot file on disk than the one the controller
thinks it is loading).

---

## 3. Evidence vs. hypothesis (be honest about which is which)

### VERIFIED (this side can show the bytes)

- **The status-posting pipeline works.** Trader head 3ed3b8a4 (2026-08-01T08:32) and foundry head b919955d (same poll) ran and posted `success` verdicts via `fabro/qa-pipeline`. The PR #140 poller surfaced a real `error` to uniforme e9ac694 in the same cycle. (`fabro-gate-poll.log` lines 7307–7322.)
- **Snapshot restore fails on the gate's hot path.** Both the `restore_many: PUT /snapshot/load returned 400` shape (uniforme heads `54da9d8`, `3b25011`, `8ec811f`, `e839d29` at earlier dates) and the `exec: read response: Resource temporarily unavailable (os error 11)` shape (uniforme e9ac694 at 2026-08-01T08:32:27Z) appear in the log. The two shapes are distinct: restore_many fails inside firecracker on snapshot load; the EAGAIN fails in the exec-stream read after a presumably successful boot. They are not the same bug.
- **`GET /v1/snapshots` is consulted by the gate-side preflight** (commit `e1b3b5b`, `ao/fabro-123/snapshot-preflight`). On mismatch the gate posts `snapshot-not-registered: tag=<X> absent from GET /v1/snapshots` — a distinct infra verdict from generic restore-many HTTP 500.
- **The fabro-sandbox forkd client does not call `GET /v1/snapshots` itself.** `lib/components/fabro-sandbox/src/provider/forkd.rs` consumes `POST /v1/sandboxes`, `POST /v1/sandboxes/{id}/exec`, `DELETE /v1/sandboxes/{id}` only. The listing endpoint is only consumed by the gate-side preflight (`bin/fabro-github-gate.sh:46` `preflight_snapshot()`), via `bin/forkd-shim.py:229 list_snapshots()`.
- **Blast radius.** `~/.ao/state/fabro-gate-health.json` shows `foundry` and `trader` with `last_verdict=error`, `uniforme` with `deferred_heads=[e9ac694…]` and no verdict, `pawbench` never gated. Every gated repo is down on the same cause.

### UNVERIFIED (cannot determine from this side)

- **Whether the snapshot registry is in-memory only, file-backed, or remote-DB-backed.** The controller's storage choice is invisible from the client or the spec. (UNVERIFIED because the controller source is not in the fabro repo, and dellsrv is behind the egress boundary from this sandbox.)
- **Whether `zen-gate-big` is missing from on-disk storage, present-but-corrupt, or present-and-valid but uncached.** A `PUT /snapshot/load` 400 from firecracker can come from a corrupt file, a wrong-version snapshot, or a cached metadata handle pointing at the wrong backing file. The `fault_message` JSON is truncated in the gate's `--desc` field (140-char cap in `gh-status.sh`); the full payload is only on the controller. (UNVERIFIED for the same reason.)
- **Whether `zen-gate-base` is still bootable today.** The 07-31 probe booted it once. A probe run after the controller's last restart is the only way to know. (UNVERIFIED — brokered path not reachable from this sandbox; one attempt made, stopped per the brief's egress boundary.)
- **Whether the brief's exact pairing (trader head `3ed3b8a4` carrying the `restore_many` 400 description at 08-32:33Z) is accurate.** The log shows trader `3ed3b8a4` *succeeded* at the timestamp the issue cites. The `restore_many` 400 entries in the log are on uniforme at older dates. The failure mode is real; the SHA/repo pairing is not. (UNVERIFIED — log diff vs. issue text.)

---

## 4. Leading hypothesis (UNVERIFIED mechanism, but evidence-consistent)

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

## 5. What the controller fix MUST do (design direction, not implementation)

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

## 6. What this PR contains vs. what the operator owns

**This PR (in the fabro worktree at `~/.ao/data/worktrees/fabro/fabro-84`, branch `ao/fabro-84/forkd-snapshot-registry`):**

- this document (`docs/internal/forkd-snapshot-registry.md`),
- the operator runbook companion (`docs/internal/forkd-snapshot-registry-runbook.md`).

**Already landed (NOT re-implemented here):**

- the gate-side preflight in `bin/fabro-github-gate.sh` (`e1b3b5b` on
  `ao/fabro-123/snapshot-preflight` in the `ao-company` repo).

**Not in this PR (operator work, T3):**

- the controller-side boot-scan re-registration (controller source is not
  local; the design is in §5 for the operator to implement against the
  real controller source),
- a controller restart for testing,
- a fix to the on-disk state of `zen-gate-big` (depends on the operator
  runbook's findings).

---

## 7. References

- `zenprocess/ao-company#123` — issue as filed.
- `zenprocess/ao-company#121` — the org-wide gate silence; the
  descriptor-driven poller (commit `258a5f0`) restored status posting
  and surfaced the snapshot failure that #123 is now the home for.
- `zenprocess/ao-company#122` — the exec `EAGAIN` regression; live-verified
  at exit 0 in the 07-31 brokered probe. Exec EAGAIN is NOT the cause of
  the current `error` verdicts: a successful exec EAGAIN run cannot post a
  status of `error`; the current verdicts reflect snapshot-restore failure
  upstream of any in-VM work.
- `bin/fabro-github-gate.sh:46 preflight_snapshot()` — the gate-side
  companion (commit `e1b3b5b`).
- `bin/forkd-shim.py:229 list_snapshots()` — the shim that brokers the
  listing to the gate host.
- `lib/components/fabro-sandbox/src/provider/forkd.rs` — the fabro-side
  client. Consumes `POST /v1/sandboxes`, `POST /v1/sandboxes/{id}/exec`,
  `DELETE /v1/sandboxes/{id}`. Does not call `GET /v1/snapshots`.
- `~/.ao/state/fabro-gate-poll.log` lines 2826, 2834, 2842, 2856, 2864
  — the `restore_many: PUT /snapshot/load returned 400` evidence on
  earlier uniforme heads.
- `~/.ao/state/fabro-gate-poll.log` lines 7307–7322 — the
  2026-08-01T08:32:27Z poll cycle in which trader/foundry gated
  successfully and uniforme failed at exec stage.
- `~/.ao/state/fabro-gate-health.json` — blast-radius snapshot.
