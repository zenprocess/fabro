# forkd snapshot-registry — operator runbook (dellsrv)

**Companion to**: `docs/internal/forkd-snapshot-registry.md` (design note).
**Audience**: only the operator — controller restart is T3.

The runbook is the live-verification branch of `zenprocess/ao-company#123`.
The goal is to determine which of three states each golden tag is in
AFTER the controller's last restart:

| state | meaning | next step |
|---|---|---|
| **missing** | the snapshot registry lacks the tag (was lost on restart) and the on-disk file is also absent | re-commit the golden from the canonical rootfs (20GB golden per QA-infra facts) |
| **present-but-broken** | the registry has the tag, the on-disk file exists, but `restore_many` returns 400 | the file is corrupt or the cached metadata handle is stale; needs an operator decision (re-commit or restore from a prior good snapshot) |
| **healthy** | the registry has the tag, boot + exec `true` succeeds | nothing to do |

The runbook does NOT itself modify the controller; it records the data
points the controller-side fix needs.

**Placement** — this runbook lives next to the design note in the fabro
repo (`docs/internal/`). Alternative placement was
`docs/runbooks/forkd-snapshot-registry.md` in the `ao-company` repo
(alongside `DEPLOY-forkd-controller-gate.md`), but that directory does
not exist in the shared checkout and the operator-runbook content is
deeply coupled to the design note in §5 of the design file. Co-locating
them keeps the operator and design context in one place.

---

## 0. Preconditions (operator-only, T3)

- The forkd-controller is a LIVE service on dellsrv. Restarting it is
  **T3** — always inside a `zenctl maint on` window. The forkd-shim and
  the gate driver both go through the controller; every gate attempt
  during the outage will post a `snapshot-not-registered: unreachable <reason>`
  infra verdict (the preflight's transport-failure branch — see design
  note §5 layer 1).
- The controller listens on `http://127.0.0.1:8891`. The bearer token
  lives at `~/fabro-run/.forkd-token` on the controller host (`@dellsrv`).
  **The token value is NEVER to be printed, echoed, or inlined into a
  command.** All commands below reference the token by file path and
  read it via `$(cat ~/fabro-run/.forkd-token)` inside the `curl` header
  only; the file content stays in process memory between the `cat` and
  the `curl` and never lands on a log line because the bash subshell is
  the only place it is expanded.
- The maintenance window is announced BEFORE step 1. Sample wording:
  ```
  zenctl maint on 'forkd snapshot re-registration verification (fabro-123)'
  ```

---

## 1. Confirm or refute registry volatility

This is the live probe the brief's items 1–2 hinge on. The question: does
`GET /v1/snapshots` empty after a restart, even though `zen-gate-base` and
`zen-gate-big` boot fine afterwards?

```bash
# 1.1  Tail the controller journal BEFORE the restart so the post-boot
#      log lines are easy to correlate.
journalctl -u forkd-controller -f > /tmp/forkd-journal.log 2>&1 &
JOURNAL_PID=$!

# 1.2  Restart the controller. T3 — operator only.
sudo systemctl restart forkd-controller

# 1.3  Wait for the controller to come back. The /v1/snapshots endpoint
#      returns 200 on a populated registry and 200 [] on an empty one;
#      both are "reachable". Return anything else and the loop continues.
TOKEN_FILE=~/fabro-run/.forkd-token
until curl -sS -o /dev/null -w '%{http_code}\n' \
    -H "Authorization: Bearer $(cat "$TOKEN_FILE")" \
    http://127.0.0.1:8891/v1/snapshots 2>/dev/null \
    | grep -qE '^(200|404)$'; do
  sleep 1
done

# 1.4  Record the listing immediately after the boot finishes.
#      Boot-scan re-registration can take a minute; the operator should
#      RE-RUN this step at +60s and +5min and compare to detect whether
#      re-registration ran.
RECORD_DIR=/tmp/forkd-snapshot-verify-$(date -u +%Y%m%dT%H%M%SZ)
mkdir -p "$RECORD_DIR"
for delay in 0 60 300; do
  sleep "$delay"
  curl -sS \
    -H "Authorization: Bearer $(cat "$TOKEN_FILE")" \
    http://127.0.0.1:8891/v1/snapshots \
    | tee "$RECORD_DIR/snapshots-t+${delay}s.json" \
    | jq 'length, [.[].tag // .[].snapshot_tag]'
  echo "--- snapshot at t+${delay}s ---"
done

# 1.5  Stop the journal tail.
kill "$JOURNAL_PID" 2>/dev/null || true
```

**What to record**: the three listings (count + tag names) and the
journal slice from boot to +5min. Feed back to the design-note author:

- `GET /v1/snapshots` post-boot — was the listing empty, partial, or complete?
- Did the journal contain `snapshot re-registered tag=<tag> source=<path>` lines? (If yes, the controller fix is already deployed; if no, the fix is pending and the listings reflect the default registry state.)
- Did the listing GROW between t+0s and t+300s? (If yes, the boot-scan is async — record the time-to-populate for the design note.)

If the listing is empty at all three sample points AND boot succeeds for
`zen-gate-base` (step 2), the volatility hypothesis is confirmed. If the
listing is populated AND boot fails, the hypothesis is wrong and we need
to look at the on-disk artifact (present-but-broken state).

---

## 2. Per-tag bootability check (which is missing, which is broken)

For each golden tag, run a real restore-boot canary. The canary must be
a real boot + exec `true`; a files-exist check is exactly the wrong
validation (design note §5). Tags to verify against: `zen-gate-base`,
`zen-gate-big`, and any other golden listed in the controller's
configuration (`FORKD_GOLDEN_TAGS` or the equivalent — document what is
actually used on dellsrv before this runbook is run, since the source
is not local).

```bash
TOKEN_FILE=~/fabro-run/.forkd-token
RECORD_DIR=/tmp/forkd-snapshot-verify-$(date -u +%Y%m%dT%H%M%SZ)
mkdir -p "$RECORD_DIR"

canary() { # $1=tag
  local tag="$1"
  echo "=== canary tag=$tag ==="

  # 2.1  POST /v1/sandboxes with the tag. 201 with a sid is the happy path.
  local create_body
  create_body=$(curl -sS -X POST \
    -H "Authorization: Bearer $(cat "$TOKEN_FILE")" \
    -H "Content-Type: application/json" \
    -d "{\"snapshot_tag\":\"$tag\"}" \
    http://127.0.0.1:8891/v1/sandboxes)
  echo "create response: $create_body"
  echo "$create_body" | tee "$RECORD_DIR/create-$tag.json"

  # 2.2  Extract the sid. The controller's response shape is documented
  #      in `docs/DEPLOY-forkd-controller-gate.md` as confirmed-working
  #      across ~30 live runs; the multi-shape fallback is in
  #      forkd-shim.py:_extract_sid.
  local sid
  sid=$(printf '%s' "$create_body" \
    | python3 -c '
import json, sys
try:
    d = json.loads(sys.stdin.read())
except Exception as e:
    print(""); sys.exit(0)
if isinstance(d, dict):
    print(d.get("id") or d.get("sandbox_id") or d.get("sid") or "")
elif isinstance(d, str):
    print(d)
elif isinstance(d, list) and d:
    v = d[0]
    print(v if isinstance(v, str) else (v.get("id") or ""))
')
  if [ -z "$sid" ]; then
    echo "canary tag=$tag NO_SID (controller refused to boot)"
    return 0
  fi

  # 2.3  POST /v1/sandboxes/{sid}/exec with `sh -c true`. This is the
  #      REAL restore-boot canary — not a files-exist check.
  local exec_body
  exec_body=$(curl -sS -X POST \
    -H "Authorization: Bearer $(cat "$TOKEN_FILE")" \
    -H "Content-Type: application/json" \
    -d '{"args":["sh","-c","true"],"timeout_secs":30}' \
    "http://127.0.0.1:8891/v1/sandboxes/$sid/exec")
  echo "exec response: $exec_body"
  echo "$exec_body" | tee "$RECORD_DIR/exec-$tag.json"

  # 2.4  DELETE /v1/sandboxes/{sid} to clean up. The forkd-shim's
  #      teardown is confirmed live across the same ~30 runs.
  curl -sS -X DELETE \
    -H "Authorization: Bearer $(cat "$TOKEN_FILE")" \
    "http://127.0.0.1:8891/v1/sandboxes/$sid" -o /dev/null -w 'delete http=%{http_code}\n'
}

canary zen-gate-base
canary zen-gate-big
# add any other tag the controller's golden list declares
```

**What to record**: the create + exec response bodies for each tag.
Feed back to the design-note author:

- `zen-gate-base` create http status + sid (or empty if refused)
- `zen-gate-base` exec `exit_code` (0 = healthy, non-zero = present-but-broken)
- `zen-gate-big` create http status + sid
- `zen-gate-big` exec `exit_code`

State table the runbook produces:

| tag | create | exec exit_code | state |
|---|---|---|---|
| `zen-gate-base` | 201 + sid | 0 | healthy |
| `zen-gate-base` | 201 + sid | non-zero | present-but-broken — restore file is corrupt, must re-commit |
| `zen-gate-base` | 4xx/5xx | n/a | missing — registry refused to boot; re-commit from canonical rootfs |
| `zen-gate-big` | (same) | (same) | (same) |

---

## 3. Token path reference (NEVER print the value)

The token is referenced at `~/fabro-run/.forkd-token` on the controller
host (dellsrv). The path is set in `bin/forkd-shim.py` and overrides the
shim's built-in default (`/etc/forkd-token`, which is root-only on this
host and is NOT what the gate uses). The driver-side override is
`FABRO_TOKEN_FILE_REMOTE` in `bin/fabro-github-gate.sh`. If the token
ever moves, the runbook still works as long as that env var is updated.

The token VALUE is never in this runbook. The `$(cat ~/fabro-run/.forkd-token)`
substitution lives inside the bash subshell that builds the curl header
argument; it never reaches a log line because the curl `-H` value is
processed in-process. If a runbook command needs to be pasted into a
shell history, the operator should configure `HISTCONTROL=ignorespace`
and prefix with a space, or use a one-shot alias that does not record.

---

## 4. Expected success log line (after the controller fix is deployed)

The controller MUST emit exactly one log line per re-registered snapshot,
in the format:

```
snapshot re-registered tag=<tag> source=<storage_path>
```

Search for it in the journal slice from step 1:

```bash
grep -E 'snapshot re-registered tag=' /tmp/forkd-journal.log
```

Each line corresponds to one tag that passed the canary. A tag that
FAILED the canary should emit:

```
snapshot re-registration FAILED tag=<tag> reason=<reason>
```

And a golden that was re-committed from the canonical rootfs should emit:

```
snapshot golden-recommitted tag=<tag>
```

If `zen-gate-big` is healthy, the journal should show one line matching
`snapshot re-registered tag=zen-gate-big`. If `zen-gate-big` is
present-but-broken, the journal should show one matching
`snapshot re-registration FAILED tag=zen-gate-big` (the failure to
re-register IS the evidence the file is unbootable). If the journal
shows NEITHER for `zen-gate-big`, the controller fix is not deployed.

---

## 5. Hand-off back to the design-note author

After the runbook completes, the operator should send back:

1. The three `GET /v1/snapshots` listings from step 1 (`$RECORD_DIR/snapshots-t+0s.json`, `…t+60s.json`, `…t+300s.json`).
2. The journal slice (`/tmp/forkd-journal.log`).
3. The four create+exec response bodies from step 2 (`$RECORD_DIR/create-<tag>.json`, `$RECORD_DIR/exec-<tag>.json`).
4. The state table from step 2 (which golden is missing, which is present-but-broken, which is healthy).

The design note's §5 fix direction depends on this signal:

- If `zen-gate-big` is **missing** on registry AND on disk → the fix is a re-commit from the canonical rootfs; the controller-side re-registration will pick it up.
- If `zen-gate-big` is **present-but-broken** → the fix is to replace the on-disk file (restore from a prior good snapshot, or re-commit from canonical rootfs); the cached metadata handles are stale.
- If `zen-gate-big` is **healthy by registry but restore_many still 400s at the gate** → the registry's success criterion is not a real canary; the controller-side fix is wrong and needs to be reworked to require a real restore-boot (not a files-exist check).

Once the operator's live signal is in, the design note's §5 layer 2
implementation can be reviewed against the actual controller source on
dellsrv.

---

## 6. Closing the maintenance window

```bash
# 6.1  Confirm the registry is healthy.
curl -sS -H "Authorization: Bearer $(cat ~/fabro-run/.forkd-token)" \
  http://127.0.0.1:8891/v1/snapshots \
  | jq 'length, [.[].tag // .[].snapshot_tag]'

# 6.2  Optionally fire one gate-poll cycle to verify the preflight's
#      `ok <tag>` path lands cleanly.
bash ~/Desktop/ao-company/bin/fabro-gate-poll.sh

# 6.3  Close the maintenance window.
zenctl maint off
```

If the post-cycle snapshot listing is non-empty and contains both golden
tags, the gate's preflight will start returning `ok <tag>` for them
immediately; the descriptions on previously-failing SHAs will not change
(the existing post_infra no-overwrite guard is preserved on purpose), so
the next NEW head will be the first to land with the preflight's clean
verdict.
