# forkd snapshot registry truthfulness (fabro-123)

**Status**: root-cause note, written BEFORE any fix is attempted.
**Author**: fabro-121 worker (reassigned to #123 after #121 proved the
status-posting pipeline is intact and the real blocker is upstream of it).
**Date**: 2026-08-01.

This document is the writing-FIRST deliverable required by the brief. It
records what the evidence shows, what is consistent, what is contradictory,
and what the operator must still verify live before any code change to the
controller on dellsrv is merged.

---

## 1. The failure as observed (fresh evidence from #121 work)

The descriptor-driven gate poller (PR #140, branch
`ao/fabro-121/gate-poll-descriptors`) ran a live cycle at
**2026-08-01T08:32:27Z** and successfully posted a `fabro/qa-pipeline` commit
status to two previously-silent repos:

| repo   | open PR / head                                | state posted | description |
|--------|-----------------------------------------------|--------------|-------------|
| trader | PR #354 / `3ed3b8a4da3cc4e7f41371f6ab0ace779e8a155c` | `error` | `infrastructure: controller POST /v1/sandboxes -> HTTP 500 {"error":"restore_many: firecracker API PUT /snapshot/load returned 400: {"fault_...` |
| foundry| PR #53 / `b919955d569f639fec627a2d09ae3f2909398876` | `error` | (same string) |

Source: `~/.ao/state/fabro-gate-poll.log` (poll end 2026-08-01T08:32:43Z)
cross-checked against `git -C ~/.ao/data/worktrees/ao-company-gate121
status --porcelain` (uncommitted: nothing — branch `258a5f0` is what
actually ran).

So the **status-posting pipeline is intact**. PR #140 ended the silent-merge
class of failure described in #121. What #121 surfaced instead is the
distinct failure described in #123: every `cmd_gate` call into the forkd
controller returns HTTP 500 on the `restore_many` step, which lives inside
the boot path before any in-VM command runs.

## 2. What #123 as filed said, and what my #121 work changed about that

The issue text (zenprocess/ao-company#123) is built on a probe dated
**2026-07-31** that found:

> the forkd controller successfully booted a sandbox from snapshot tag
> `zen-gate-base` and exec'd with exit 0 — yet `GET /v1/snapshots` returned
> `[]`. The listing endpoint lies about available snapshots while the boot
> path resolves tags fine.

The 07-31 probe name (`zen-gate-base`) and the failing gate's request name
(`zen-gate-big`) are different tags. The `restore_many` 400 in #121 is the
first hard evidence I have that the gap is not just a stale listing but a
**real boot-path failure for the `zen-gate-big` tag specifically**. Either:

- the registry lost `zen-gate-big` while still holding `zen-gate-base`; or
- the on-disk snapshot file for `zen-gate-big` is present but
  unbootable; or
- the registry has an entry for `zen-gate-big` but its file handle is stale
  (firecracker's `PUT /snapshot/load` 400 includes a `fault_` JSON whose
  truncated tail I cannot read from outside the controller).

The brief flags the same uncertainty: "*Establish which BEFORE fixing.*"

## 3. What I CAN state from the evidence in front of me

- `GET /v1/snapshots` is currently a **lie** for callers that trust it. The
  on-host audit probe (2026-07-31) got `[]` back while `zen-gate-base`
  booted fine, so the listing endpoint is decoupled from the boot path.
- The boot path **can fail with HTTP 500 on `restore_many` even when the
  requested snapshot tag is one we have observed boot previously** (because
  the 07-31 probe booted `zen-gate-base` successfully, and the failure
  shown in #121 is on `zen-gate-big`, but the failure mode is the same
  class — snapshot registry vs. snapshot file — so the same fix covers
  both).
- The forkd-shim and `fabro-sandbox/src/forkd/mod.rs` only consume three
  endpoints: `POST /v1/sandboxes`, `POST /v1/sandboxes/{id}/exec`,
  `DELETE /v1/sandboxes/{id}`. `GET /v1/snapshots` is not consumed by any
  client code in `lib/`; the on-host 07-31 probe was the only live caller
  on record.
- There is **no forkd CONTROLLER source tree in the fabro repo**. The repo
  contains the client (`lib/crates/fabro-sandbox/src/forkd/mod.rs`), the
  provider (`lib/crates/fabro-sandbox/src/provider/forkd.rs`), the e2e
  BDD spec (`specs/forkd-e2e/behavior.feature`), and the gate driver
  shim (`bin/forkd-shim.py` is staged in `ao-company/bin/`, not in
  `fabro/`). The controller is a separate service running on dellsrv; its
  source is not local to this checkout.

## 4. What I CANNOT state from the evidence in front of me

These are the things the brief is asking for, that I cannot determine
without reaching dellsrv:

- **Whether the snapshot REGISTRY is in-memory only, file-backed, or
  remote-DB-backed.** The hypothesis the issue proposes is "volatile across
  restart while files persist on disk", but the controller's actual storage
  choice is not visible from the client side or the spec.
- **Whether `zen-gate-big` is missing entirely from on-disk storage, or
  present-but-broken.** A `restore_many` 400 from firecracker is a
  firecracker-side complaint, which suggests the file is at least found —
  but a corrupt file (truncated, wrong version, mismatched uffd page) all
  present in the same way to the loader and the same 400 to the caller.
- **Whether `zen-gate-base` is still bootable today.** The 07-31 probe
  booted it once; a probe run after the controller's last restart is the
  only way to know. The 07-11 anchor (`.cal/skill-drafts/anchor-sandbox-
  network-boundary-.../ANCHOR.md`) shows `zen-gate-base` booted and exec'd
  on that earlier date, but the controller has been restarted since (per
  the issue text — the registry is hypothesized to lose state on restart).
- **What the `fault_` JSON in the 400 actually says.** The string is
  truncated in the gate's `--desc` field (description is capped at 140
  chars in `gh-status.sh`); the full payload is only on the controller.

I am NOT going to invent answers to any of these. The brief is explicit:
*“An honest ‘could not verify live’ is required. Do NOT fabricate evidence
of a passing probe or a non-empty snapshot list.”*

## 5. Boundary and what I attempted

The brief permits one brokered attempt and then mandates stop. I checked:

- `which zen-gates` — not installed.
- `which godkb` — not installed.
- MCP servers currently exposed to this session — `codebase-memory-mcp`,
  `context7`, `computer-use`. The `zen` MCP server is registered in
  `~/.claude/settings.json` but is not currently connected
  (`ListMcpResourcesTool { server: "zen" }` returned
  `Server "zen" not found. Available servers: context7,
  codebase-memory-mcp, computer-use.`).
- The codebase-memory index — 429 indexed projects; grepping for
  `forkd`-named projects returns zero. The forkd controller is not in the
  index.

I made one attempt to discover a brokered path. It is not currently
available to me. I am not going to:
- try to dial `dellsrv:8891` directly (sandbox denies it; the
  `sandbox-evasion-guard` hook and the egress allowlist would block it
  anyway, but the boundary is a *policy* I follow, not just a *mechanism*
  I trip);
- resolve a `*.zp.digital` hostname and connect by IP literal;
- use DoH-then-connect, `nc`, `websocat`, `curl --resolve`, or any other
  tunneling;
- reconfigure the sandbox.

Reporting this as the boundary requires and stopping the live-verification
branch of work. I am proceeding with what I CAN do from this side: the
root-cause note (this document), the gate-side preflight in
`fabro-github-gate.sh` (#123 scope item 4, explicitly mine), the operator
runbook for the controller-side fix (#123 scope item 2, mine to write,
not to execute), and the preflight self-test extension.

## 6. Design choice for the controller fix (mine to write, not to merge)

The fix proposed in #123 has two parts. This section records the design so
the operator-runbook in §7 has a coherent target, and so a future review
can compare the design to whatever the operator eventually implements.

### 6.1 Boot-time re-registration

On controller boot, walk the snapshot storage directory. For each entry
that has a complete file (metadata + payload, sizes match what
`restore_many` needs), run a real **restore-boot canary**:

1. `POST /v1/sandboxes` with that tag (this is what `forkd-shim.py` calls
   `EP_CREATE`).
2. If the POST returns 201 with a sid, `POST /v1/sandboxes/{sid}/exec`
   with `["sh", "-c", "true"]` and a small timeout.
3. If the exec returns 200 with `exit_code=0`, mark the snapshot
   registered and emit one log line per snapshot:
   `snapshot re-registered tag=<tag> source=<storage_path>`.
4. If the POST or exec fails for any reason, mark it NOT-registered and
   emit a `snapshot re-registration FAILED tag=<tag> reason=<...>` line.

After the walk, `GET /v1/snapshots` must return the registered set
**and only the registered set**. A snapshot is in the listing IFF it
just passed the canary. This is the standing anchor called out in the
brief: a files-exist check is exactly how an unshippable golden passes
the validation, so the canary IS the validation.

### 6.2 Golden re-commit on missing

For each golden tag in a configured list (`zen-gate-base`, `zen-gate-big`,
anything else declared golden), if the walk does not find a passing
entry:

1. Re-commit the golden from the canonical rootfs (the 20GB golden per
   the operator's QA-infra facts).
2. Run the restore-boot canary on the freshly-committed snapshot.
3. If the canary passes, register it and emit
   `snapshot golden-recommitted tag=<tag>`.
4. If the canary still fails, emit a `snapshot golden-recommit FAILED
   tag=<tag>` line AND push a `ntfy` alert (per the brief). The
   preflight on the gate side will then catch every gate attempt with
   `snapshot-not-registered`, which the monitor can route as its own
   alert class.

### 6.3 Why the canary cannot be skipped

The 2026-07-11 anchor (`.cal/skill-drafts/anchor-sandbox-network-
boundary-.../ANCHOR.md`) records the same kind of evidence in the
opposite direction: the snapshot list returned `[]` while `zen-gate-base`
exec worked, which was used at the time to argue that the list endpoint
"does not prove snapshot absence". That anchor is correct as a
**diagnostic** — do not trust the list alone to decide whether a snapshot
exists. The proposed fix is also correct in the same direction: the list
should be derived from "what boots", not "what's on disk". Files on disk
without a canary-pass is the path that ships an unshippable golden.

## 7. Operator runbook (this is mine to write; the operator runs it)

The exact commands the operator must run inside a `zenctl maint on`
window on dellsrv. The goal: bring the controller up, run the
re-registration walk (if the fix is already deployed), observe the
listing, then exercise a single boot+exec on `zen-gate-base` AND
`zen-gate-big` to determine which is missing, which is present-but-
broken, and which is healthy. Hand the output back so the design in §6
can be tuned.

```bash
# 0. Announce the maintenance window (zenctl is the operator's tool).
zenctl maint on 'forkd snapshot re-registration verification (fabro-123)'

# 1. Tail the controller journal BEFORE touching it.
journalctl -u forkd-controller -f &
JOURNAL_PID=$!

# 2. Restart the controller. (T3 — operator only.)
sudo systemctl restart forkd-controller

# 3. Wait for the controller to be reachable. The token stays at the
#    same path; the brief is explicit that it is NEVER to be printed.
until curl -sS -o /dev/null -w '%{http_code}\n' \
    -H "Authorization: Bearer $(cat ~/fabro-run/.forkd-token)" \
    http://127.0.0.1:8891/v1/snapshots | grep -qE '^(200|404)$'; do
  sleep 1
done

# 4. Record what the listing returns now (boot scan may take a minute).
curl -sS -H "Authorization: Bearer $(cat ~/fabro-run/.forkd-token)" \
    http://127.0.0.1:8891/v1/snapshots \
  | tee /tmp/forkd-snapshots-post-boot.json | jq 'length, [.[].tag // .[].snapshot_tag]'

# 5. Re-create a sandbox from zen-gate-base (the one the 07-31 probe
#    proved bootable). Capture the sid.
BASE_BODY=$(curl -sS -X POST -H "Authorization: Bearer $(cat ~/fabro-run/.forkd-token)" \
    -H "Content-Type: application/json" \
    -d '{"snapshot_tag":"zen-gate-base"}' \
    http://127.0.0.1:8891/v1/sandboxes)
echo "zen-gate-base create: $BASE_BODY"
BASE_SID=$(echo "$BASE_BODY" | python3 -c 'import json,sys; print(json.load(sys.stdin).get("id") or "")')
[ -n "$BASE_SID" ] && curl -sS -X POST -H "Authorization: Bearer $(cat ~/fabro-run/.forkd-token)" \
    -H "Content-Type: application/json" \
    -d '{"args":["sh","-c","true"],"timeout_secs":30}' \
    "http://127.0.0.1:8891/v1/sandboxes/$BASE_SID/exec" | jq '{exit_code, completed}'

# 6. Repeat for zen-gate-big. This is the tag the gate is currently
#    failing on, so its result is the one that drives the next
#    re-commit decision.
BIG_BODY=$(curl -sS -X POST -H "Authorization: Bearer $(cat ~/fabro-run/.forkd-token)" \
    -H "Content-Type: application/json" \
    -d '{"snapshot_tag":"zen-gate-big"}' \
    http://127.0.0.1:8891/v1/sandboxes)
echo "zen-gate-big create: $BIG_BODY"
BIG_SID=$(echo "$BIG_BODY" | python3 -c 'import json,sys; print(json.load(sys.stdin).get("id") or "")')
[ -n "$BIG_SID" ] && curl -sS -X POST -H "Authorization: Bearer $(cat ~/fabro-run/.forkd-token)" \
    -H "Content-Type: application/json" \
    -d '{"args":["sh","-c","true"],"timeout_secs":30}' \
    "http://127.0.0.1:8891/v1/sandboxes/$BIG_SID/exec" | jq '{exit_code, completed}'

# 7. After the walk, GET /v1/snapshots should agree with the union of
#    the tags that just booted. If it does NOT, the boot-scan is not
#    yet deployed; the operator should record the result and feed it
#    back before any code change.
curl -sS -H "Authorization: Bearer $(cat ~/fabro-run/.forkd-token)" \
    http://127.0.0.1:8891/v1/snapshots | jq 'length, [.[].tag // .[].snapshot_tag]'

# 8. Once the gate-side preflight (this PR) is deployed, re-trigger
#    one of the deferred/failing heads to confirm the preflight is
#    seeing a non-empty listing. The poller lives at
#    ~/.ao-mac/fabro-gate-poll.sh (or the staged
#    ~/Desktop/ao-company/bin/fabro-gate-poll.sh on the operator's
#    checkout), and the easiest one-shot is:
#      bash ~/Desktop/ao-company/bin/fabro-gate-poll.sh
#    and then:
#      tail -F ~/.ao/state/fabro-gate-poll.log | grep GATE_CYCLE
#    Look for non-zero `gated=N` lines for trader / foundry. If a
#    `snapshot-not-registered` reason shows up, the preflight is
#    working AND the controller is missing a tag — the operator should
#    re-commit the missing golden and re-run.

# 9. Close the maintenance window.
kill "$JOURNAL_PID" 2>/dev/null || true
zenctl maint off
```

What to feed back: the post-boot listing, the two `exit_code` values from
the canary execs, the journal slice covering the controller boot, and
the latest `fabro-gate-poll.log` GATE_CYCLE block. The data points the
fix needs:

- `GET /v1/snapshots` post-boot — was the listing empty, partial, or
  complete?
- `zen-gate-base` exec — does the canary tag still boot?
- `zen-gate-big` exec — does the canary tag boot? (If yes, the failure
  is something else; if no, the gate-side preflight is the right
  diagnostic, not a re-commit.)
- controller journal — does it show any "snapshot re-registered"
  log lines (those would come from the fix being deployed)?

## 8. What this PR contains vs. what the operator owns

This PR contains (in the fabro worktree at
`~/.ao/data/worktrees/fabro/fabro-123`):

- this document (`docs/internal/forkd-snapshot-truthfulness.md`).
- a gate-side preflight in `bin/fabro-github-gate.sh` (staged via the
  existing `ao-company` worktree `~/.ao/data/worktrees/ao-company-gate121`,
  on the new branch `ao/fabro-123/snapshot-preflight`) that asserts the
  requested `snapshot_tag` appears in `GET /v1/snapshots` before
  `cmd_gate` runs and emits an infra verdict with reason
  `snapshot-not-registered` on mismatch. The preflight's self-test is
  extended to cover the new verdict class.

This PR does NOT contain:

- the controller-side boot-scan re-registration (no controller source is
  local; the design is in §6 for the operator to implement against the
  real controller source).
- a restart of the controller (T3, operator only).
- a fix to the on-disk state of `zen-gate-big` (depends on the operator
  runbook's findings).

## 9. Links and references

- zenprocess/ao-company#123 — issue as filed (and as
  `gh issue view 123 --repo zenprocess/ao-company`).
- zenprocess/ao-company#121 — the org-wide gate silence; PR #140
  restored the status-posting pipeline and surfaced the snapshot
  failure that #123 is now the home for.
- `.cal/skill-drafts/anchor-sandbox-network-boundary-.../ANCHOR.md` —
  2026-07-11 anchor recording that `forkd snapshot list returned []`
  while `zen-gate-base` exec worked, which is the same class of
  evidence the 07-31 probe produced.
- `specs/forkd-e2e/behavior.feature` — the e2e BDD spec for the
  client. It names the snapshot registry as a precondition but does
  not currently assert anything about its content.
- `lib/crates/fabro-sandbox/src/forkd/mod.rs` — the client; consumes
  `POST /v1/sandboxes`, `POST /v1/sandboxes/{id}/exec`,
  `DELETE /v1/sandboxes/{id}`. Does not call `GET /v1/snapshots`.
- `bin/forkd-shim.py` (in `ao-company/bin/`) — the gate-side shim that
  brokers between `fabro-github-gate.sh` and the controller.
