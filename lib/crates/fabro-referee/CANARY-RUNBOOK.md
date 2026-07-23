# fabro-referee — Canary Runbook (P0)

Operator-facing procedure for the **first live 2-tier canary** of the Referee
scorer against the real forkd controller. Read `CONTRACTS.md`
(`~/.ao/data/aofactory/referee/CONTRACTS.md`) alongside this — it holds the
frozen wire shapes this runbook drives.

The binary is `fabro-referee`. Build it once, then run the three phases below in
order: **doctor → hermetic dry-run → live forkd canary**. Do not skip the
hermetic dry-run; it proves the scorer end-to-end with zero network so a
failure in the live phase localizes to the controller, not the scorer.

---

## T3 boundary — read before running

The scorer **only calls the controller's score/read path** (`POST /gate-run`,
`GET /health`). It MUST NOT — and does not — activate, reconfigure, re-baseline,
restart, or otherwise mutate the forkd controller, the golden rootfs, or any
dellsrv service. Running this runbook scores diffs; **the operator activates the
plane**. If a phase needs the controller to be in a state it is not in, STOP and
hand back to the operator — do not mutate dellsrv to make a phase pass.

## Egress boundary — where to run this

`--backend forkd` needs `dellsrv:8891` in the egress allowlist. It is **not**
reachable from the fabro build worktree (one documented probe timed out; see
`CONTRACTS.md` §1). Run the **live** phase from a host that is on the allowlist.
A DNS/TCP denial from the controller is the boundary working as designed — do
**one** `doctor` probe, then stop and report; never work around it (no IP
literal, no `--resolve`, no tunnel, no DoH).

---

## Build

```sh
cd <fabro-worktree>
cargo build -p fabro-referee            # debug → target/debug/fabro-referee
# or, for the canary host: cargo build -p fabro-referee --release
REF=target/debug/fabro-referee          # adjust for release
```

The crate has no `test-support` in its normal build; the binary is a
self-contained synchronous CLI.

---

## Phase 1 — `doctor` (triage, no scoring)

Prints the resolved contracts and does one cheap `GET {endpoint}/health`. Use it
to confirm reachability **before** poking the orchestrator over a missing sink
row.

```sh
$REF --backend forkd --forkd-endpoint http://dellsrv:8891 doctor
```

Expected on an allowlisted host:

```
fabro-referee doctor
  backend       : Forkd (endpoint=http://dellsrv:8891)
  sink_dir      : ~/.ao/data/aofactory/referee/runs
  valset_root   : .
  decision_log  : <path or <absent>>
  task_id default: T-canary-p0-stamp
  canary file  : CANARY.md (operator-overridable via --file)
  forkd endpoint: reachable (HTTP 2xx)
```

`forkd endpoint: UNREACHABLE — …` from the build worktree is expected and is the
egress boundary, not a bug. Move to an allowlisted host. `doctor` never fails on
an unreachable endpoint — it only reports.

---

## Phase 2 — hermetic dry-run (proves the scorer, no network)

Runs the canary task through the local hermetic gate. This is the same code path
the live phase uses for stages 1 (routes) and 3 (emit); only the stage-2 backend
differs. Prove **both** oracle directions:

```sh
# GREEN: both routes add the marker → both must PASS
$REF --backend hermetic --valset-root <fabro-worktree> \
  canary --run-id smoke-green-1 --file CANARY.md --green

# RED: neither route adds the marker → both must FAIL
$REF --backend hermetic --valset-root <fabro-worktree> \
  canary --run-id smoke-red-1 --file CANARY.md
```

After each run, inspect the sink:

```sh
cat ~/.ao/data/aofactory/referee/runs/smoke-green-1.jsonl   # 2 rows, verdict "pass"
cat ~/.ao/data/aofactory/referee/runs/smoke-red-1.jsonl     # 2 rows, verdict "fail"
```

If GREEN is not pass/pass and RED is not fail/fail, the scorer or the acceptance
oracle is wrong — fix that before touching the live controller.

---

## Phase 3 — live forkd canary

Two shapes. Run **3a** first (cheapest reachability + scoring proof), then **3b**
for the real cross-tier signal.

### 3a — canned-diff canary against forkd

Exercises the full chain (task → 2 routes → `POST /gate-run` per route → sink)
with a trivial canned diff. Proves the controller answers the contract and the
scorer parses it.

```sh
$REF --backend forkd --forkd-endpoint http://dellsrv:8891 \
     --valset-root <fabro-worktree> \
  canary --run-id p0-live-1 --branch-base p0-canary --file CANARY.md --green
```

The runner appends `-mm` and `-sn` to `--branch-base`, so the two scored routes
are `p0-canary-mm` (minimax) and `p0-canary-sn` (sonnet). The controller POST
body per route is:

```json
{ "task_id": "T-canary-p0-stamp", "base_ref": "HEAD", "diff": "<route diff>", "ts": "<rfc3339>" }
```

and the controller must answer (see `CONTRACTS.md` §1):

```json
{ "verdict": "pass" | "fail", "gate_log": "<text>", "score": <f64?>, "valset_hash": "<str?>" }
```

### 3b — real 2-tier scoring (the actual canary)

This is the binding P0 acceptance: **one task → two tiers → two scored rows**,
using the *real* diffs the two tier attempts produced.

1. **Spawn both tier attempts** (orchestrator-owned) on branches suffixed
   `-mm` and `-sn` of one base, e.g. `ao spawn … --branch p0-canary-mm` and
   `… --branch p0-canary-sn`. The `-mm`/`-sn` suffix is what the wrapper routes
   on (`decision_basis: branch-suffix`; see `CONTRACTS.md` §2).

2. **Capture each route's diff** against the shared base:

   ```sh
   git -C <project> diff <base_ref>...p0-canary-mm > /tmp/mm.diff
   git -C <project> diff <base_ref>...p0-canary-sn > /tmp/sn.diff
   ```

3. **Write the task spec** `task.json` (`kind: shell_command` acceptance; this is
   the canary shape — swap `command` for the real task's closed-form check):

   ```json
   {
     "task_id": "T-canary-p0-stamp",
     "spec_ref": "fabro-referee/CANARY-RUNBOOK.md",
     "difficulty_bucket": "easy",
     "prompt_path": "<project>/CANARY.md",
     "project_path": "<project>",
     "base_ref": "<base_ref>",
     "acceptance": { "kind": "shell_command", "command": "grep -F 'P0-CANARY:' CANARY.md" }
   }
   ```

4. **Write the routes** `routes.json` (`Vec<Route>`; `tier` is lowercase
   `minimax|sonnet|qwen|cloud`; set `session_id` to the wrapper's session id so
   the decision-log join can recover `tier_resolved`/`decision_basis`):

   ```json
   [
     { "tier": "minimax", "branch": "p0-canary-mm", "session_id": "<sid-mm>", "diff": "<contents of /tmp/mm.diff>" },
     { "tier": "sonnet",  "branch": "p0-canary-sn", "session_id": "<sid-sn>", "diff": "<contents of /tmp/sn.diff>" }
   ]
   ```

5. **Score**:

   ```sh
   $REF --backend forkd --forkd-endpoint http://dellsrv:8891 \
        --decision-log ~/.ao/data/wrapper-decisions.jsonl \
     score --task task.json --routes routes.json --run-id p0-live-2
   ```

`--decision-log` is optional but recommended: it joins each route on
`(session_id, branch)` to recover the wrapper's **actual** `tier_resolved` and
`decision_basis` (the goal tier and the tier that ran can diverge, e.g. minimax
quota → forced sonnet). Absent the log, rows carry the goal tier only and
`tier_resolved`/`decision_basis` are null — still a valid row.

---

## Success criteria

The canary passes when the sink holds **two rows for the run**, each carrying the
required fields:

```sh
RUN=p0-live-2
jq -c '{task_id, tier, tier_resolved, decision_basis, verdict, gate_backend, gate_log: (.gate_log|length)}' \
  ~/.ao/data/aofactory/referee/runs/$RUN.jsonl
# → two lines, one tier=minimax and one tier=sonnet, each with verdict ∈ {pass,fail},
#   gate_backend="forkd", and a non-empty gate_log.
```

The human-readable summary is `~/.ao/data/aofactory/referee/runs/$RUN.md`
(per-route verdict table + collapsible gate logs). The verdicts themselves are
**not** the pass condition — divergent tier verdicts are exactly the signal the
plane exists to capture. The pass condition is: **two tiers, two scored rows,
each with `{task_id, tier, verdict, gate_log, decision_basis}` landed in the
sink.**

## Triage

| Symptom | Likely cause | Action |
|---|---|---|
| `doctor` → UNREACHABLE from build worktree | egress boundary | Run from an allowlisted host; do not work around |
| `doctor` → UNREACHABLE from allowlisted host | controller down / wrong endpoint | Hand to operator (T3 — do not restart dellsrv) |
| `forkd returned non-2xx` | controller rejected the body / path drift | Check `CONTRACTS.md` §1 shape; hand path drift to operator |
| `forkd response missing verdict` / invalid JSON | response-shape drift | Reconcile with `CONTRACTS.md` §1; do not guess |
| Only one row in sink | one route errored mid-run | Read the `.md` summary + stderr; re-run that route |
| `tier_resolved` null with `--decision-log` set | no `(session_id, branch)` match | Confirm `session_id` in `routes.json` matches the wrapper's log line |

## Sink & ids reference

- **Sink dir:** `~/.ao/data/aofactory/referee/runs/` (override `--sink-dir`)
- **Per run:** `<run-id>.jsonl` (one row per route) + `<run-id>.md` (summary)
- **Canary task id:** `T-canary-p0-stamp`
- **Marker line:** `P0-CANARY: <run_id>`
- **Decision log:** `~/.ao/data/wrapper-decisions.jsonl` (join key: `session_id` + `branch`)
- **Controller:** `POST http://dellsrv:8891/gate-run`, health `GET /health`
