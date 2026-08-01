# forkd snapshot-registry — operator runbook (dellsrv)

**Companion to**: `docs/internal/forkd-snapshot-registry.md` (design note).
**Audience**: only the operator — controller restart is T3.
**Revision**: 2 (2026-08-01). Adds §2 for the exec-stage EAGAIN diagnostic
(see §1 of the design note for why both failure modes must be covered), and
softens §3 on token handling (this worker previously asserted the token
never lands on a log line because of bash subshell expansion; that was
overconfident — shell tracing, error paths, and command capture can expose
it).

The runbook is the live-verification branch of `zenprocess/ao-company#123`
(restore-stage 400) AND the live-diagnostic branch of
`zenprocess/ao-company#122` (exec-stage EAGAIN). The two issues share a
controller but have different root causes and different fixes; the
runbook must characterize both before either fix can land.

For the restore-stage (#123), the goal is to determine which of three
states each golden tag is in AFTER the controller's last restart:

| state | meaning | next step |
|---|---|---|
| **missing** | the snapshot registry lacks the tag (was lost on restart) and the on-disk file is also absent | re-commit the golden from the canonical rootfs (20GB golden per QA-infra facts) |
| **present-but-broken** | the registry has the tag, the on-disk file exists, but `restore_many` returns 400 | the file is corrupt or the cached metadata handle is stale; needs an operator decision (re-commit or restore from a prior good snapshot) |
| **healthy** | the registry has the tag, boot + exec `true` succeeds | nothing to do |

For the exec-stage (#122), the runbook asks: does a restored VM accept
exec, and what controller-side resource is exhausted (fd / pid / memory)
when the exec-stream read returns EAGAIN? See §2 below.

The runbook does NOT itself modify the controller; it records the data
points the controller-side fix needs.

**Placement** — this runbook lives next to the design note in the fabro
repo (`docs/internal/`). Alternative placement was
`docs/runbooks/forkd-snapshot-registry.md` in the `ao-company` repo
(alongside `DEPLOY-forkd-controller-gate.md`), but that directory does
not exist in the shared checkout and the operator-runbook content is
deeply coupled to the design note in §7 of the design file. Co-locating
them keeps the operator and design context in one place.

---

## 0. Preconditions (operator-only, T3)

- The forkd-controller is a LIVE service on dellsrv. Restarting it is
  **T3** — always inside a `zenctl maint on` window. The forkd-shim and
  the gate driver both go through the controller; every gate attempt
  during the outage will post a `snapshot-not-registered: unreachable <reason>`
  infra verdict (the preflight's transport-failure branch — see design
  note §7 layer 1).
- The controller listens on `http://127.0.0.1:8891`. The bearer token
  lives at `~/fabro-run/.forkd-token` on the controller host (`@dellsrv`).
  **Reference the token by its file path only.** Never `echo`, `cat`,
  `print`, or otherwise surface the token VALUE on stdout, stderr, or in
  any file that might be journaled. See §3 for the strict handling rules.
- The maintenance window is announced BEFORE step 1. Sample wording:
  ```
  zenctl maint on 'forkd snapshot + exec diagnostics (fabro-123 + fabro-122)'
  ```

---

## 1. Confirm or refute registry volatility (the snapshot side)

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
`zen-gate-base` (step 3 below), the volatility hypothesis is confirmed.
If the listing is populated AND boot fails, the hypothesis is wrong and
we need to look at the on-disk artifact (present-but-broken state).

---

## 2. Diagnose exec-stage EAGAIN (os error 11) — the dominant failure

The git evidence shows exec-stage EAGAIN dominates restore-stage 400 by
~91:1 in the live log (1092 vs 12 occurrences). It is a different bug
with a different fix path. This section asks the operator to characterize
it well enough that the controller-side fix for `zenprocess/ao-company#122`
can be sized correctly.

The question is: when the in-VM exec stream returns EAGAIN (`os error 11`,
`Resource temporarily unavailable`), which resource is exhausted on the
controller host or in the guest? The three leading candidates are file
descriptors, process IDs, and (resident) memory.

```bash
# 2.1  Capture the controller process baseline BEFORE any gate load.
#      Run this ON dellsrv as the controller's runtime user; if the
#      controller is in its own cgroup or systemd unit, prefer the
#      cgroup path (replace $UNIT with the actual unit name).
UNIT=forkd-controller.service
echo "=== controller baseline ==="
ps -o pid,ppid,user,rss,vsz,cmd -C forkd-controller 2>/dev/null \
  | tee "$RECORD_DIR/baseline-ps.txt"
cat /proc/$(pgrep -f forkd-controller | head -1)/limits 2>/dev/null \
  | tee "$RECORD_DIR/baseline-limits.txt"
systemctl show "$UNIT" \
  --property=LimitNOFILE,LimitNPROC,MemoryMax,MemoryHigh,TasksMax \
  | tee "$RECORD_DIR/baseline-cgroup.txt"

# 2.2  For each fd/pid/memory hypothesis, capture the live state at the
#      moment an EAGAIN 500 fires. The trigger is the poll log line
#      "exec: read response: Resource temporarily unavailable (os error 11)".
#      The cleanest way is to watch the journal and snapshot the relevant
#      counters on each occurrence.
journalctl -u forkd-controller -f -n 0 \
  | awk '/exec: read response: Resource temporarily unavailable/ {
      system("date -u +%Y-%m-%dT%H:%M:%SZ > /tmp/eagain-trigger");
      system("ls /proc/$(pgrep -f forkd-controller | head -1)/fd 2>/dev/null | wc -l > /tmp/eagain-fds");
      system("ps -o pid,nlwp --no-headers -C forkd-controller 2>/dev/null > /tmp/eagain-threads");
      system("ps -o pid,rss,vsz --no-headers -C forkd-controller 2>/dev/null > /tmp/eagain-mem");
    }' &
EAGAIN_TAP_PID=$!

# 2.3  Generate a small amount of gate load so EAGAIN fires (run only
#      if the live queue has no pending SHAs). The exact trigger is
#      environment-specific; the operator may instead drive load by
#      running fabro-gate-poll.sh a few times back-to-back. The goal is
#      to capture ≥3 EAGAIN-triggered samples, then STOP and look at the
#      samples. Do NOT drive load longer than needed.
bash ~/Desktop/ao-company/bin/fabro-gate-poll.sh

# 2.4  Stop the journal tap and dump the captured samples.
kill "$EAGAIN_TAP_PID" 2>/dev/null || true
echo "=== EAGAIN-triggered snapshots ==="
for f in /tmp/eagain-fds /tmp/eagain-threads /tmp/eagain-mem /tmp/eagain-trigger; do
  [ -f "$f" ] && echo "$f:" && cat "$f" && echo
done | tee "$RECORD_DIR/eagain-samples.txt"
```

**What to record**: the controller baseline (process info, rlimit/cgroup
limits) and the EAGAIN-triggered snapshots of fd count, thread count, RSS.
Feed back to the design-note author:

- **File descriptors**: open-fd count at the moment of EAGAIN vs the
  controller's `LimitNOFILE`. If `open_fds / LimitNOFILE > 0.8`, fd
  exhaustion is the dominant cause.
- **Process / thread count**: `nlwp` (number of light-weight processes) at
  the moment of EAGAIN vs `LimitNPROC` / `TasksMax`. If `nlwp` is at or
  near the limit, PID exhaustion is the dominant cause.
- **Memory**: `rss` / `vsz` at the moment of EAGAIN vs `MemoryMax` /
  `MemoryHigh`. If rss is at or near the limit, memory exhaustion is
  the dominant cause.
- **Pattern across EAGAIN samples**: is the resource monotonically
  growing across samples (cumulative leak) or stable (just hitting a
  fixed ceiling)? The fix shape depends on this — a leak needs
  identification + close; a fixed ceiling needs raising or pool sizing.

The design note's §5 (UNVERIFIED items) calls out this exact question;
the live answer closes that gap and lets the operator file a sized fix
against `zenprocess/ao-company#122`.

---

## 3. Per-tag bootability check (which golden is missing, which is broken)

For each golden tag, run a real restore-boot canary. The canary must be
a real boot + exec `true`; a files-exist check is exactly the wrong
validation (design note §7 layer 2). Tags to verify against:
`zen-gate-base`, `zen-gate-big`, and any other golden listed in the
controller's configuration (`FORKD_GOLDEN_TAGS` or the equivalent —
document what is actually used on dellsrv before this runbook is run,
since the source is not local).

```bash
TOKEN_FILE=~/fabro-run/.forkd-token
RECORD_DIR=/tmp/forkd-snapshot-verify-$(date -u +%Y%m%dT%H%M%SZ)
mkdir -p "$RECORD_DIR"

canary() { # $1=tag
  local tag="$1"
  echo "=== canary tag=$tag ==="

  # 3.1  POST /v1/sandboxes with the tag. 201 with a sid is the happy path.
  local create_body
  create_body=$(curl -sS -X POST \
    -H "Authorization: Bearer $(cat "$TOKEN_FILE")" \
    -H "Content-Type: application/json" \
    -d "{\"snapshot_tag\":\"$tag\"}" \
    http://127.0.0.1:8891/v1/sandboxes)
  echo "create response: $create_body"
  echo "$create_body" | tee "$RECORD_DIR/create-$tag.json"

  # 3.2  Extract the sid. The controller's response shape is documented
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

  # 3.3  POST /v1/sandboxes/{sid}/exec with `sh -c true`. This is the
  #      REAL restore-boot canary — not a files-exist check.
  local exec_body
  exec_body=$(curl -sS -X POST \
    -H "Authorization: Bearer $(cat "$TOKEN_FILE")" \
    -H "Content-Type: application/json" \
    -d '{"args":["sh","-c","true"],"timeout_secs":30}' \
    "http://127.0.0.1:8891/v1/sandboxes/$sid/exec")
  echo "exec response: $exec_body"
  echo "$exec_body" | tee "$RECORD_DIR/exec-$tag.json"

  # 3.4  DELETE /v1/sandboxes/{sid} to clean up. The forkd-shim's
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

## 4. Token path reference (handling rules — softened from revision 1)

The token is referenced at `~/fabro-run/.forkd-token` on the controller
host (dellsrv). The path is set in `bin/forkd-shim.py` and overrides the
shim's built-in default (`/etc/forkd-token`, which is root-only on this
host and is NOT what the gate uses). The driver-side override is
`FABRO_TOKEN_FILE_REMOTE` in `bin/fabro-github-gate.sh`. If the token
ever moves, the runbook still works as long as that env var is updated.

### Handling rules

- **Reference the token by file path only.** Every command in this
  runbook uses `$(cat "$TOKEN_FILE")` inside the curl `-H` argument
  and nowhere else. The token VALUE is never typed, echoed, or inlined
  into a shell snippet, a script, a config, or a log line.
- **Before starting the maintenance window, confirm shell tracing is
  OFF.** Run `set +x` and check `$SHELLOPTS` does not contain `xtrace`.
  The previous revision of this runbook asserted that "the bash
  subshell is the only place it is expanded" — that was overconfident.
  A shell with `set -x`, a `PS4` that prints expanded commands, a
  process tracer (`strace`, `dtrace`), or a wrapping utility that
  captures argv can expose the expanded token. The operator must
  verify none of these are in play before the runbook commands run.
- **Disable shell history capture for the maintenance window.**
  `HISTCONTROL=ignorespace` plus a leading space on each command, or
  a one-shot `env HISTFILE=/dev/null bash --noprofile --norc` for the
  whole runbook session.
- **Do not paste runbook commands into a chat window, terminal
  capture, or pastebin.** The expanded `$(cat ...)` shows up in the
  rendered command. Operators running this from a tmux session with
  `capture-pane` or from a screen recording should disable capture
  for the duration.
- **Do not redirect curl output containing the token** (e.g. via `-v`,
  `-w`, or `--trace`). The default `curl -sS` is fine because the
  `-H` value is processed in-process and never reappears in the
  output body; `-v` and `--trace` violate this.

The token VALUE is never in this runbook. Following the rules above is
what keeps it out of any artifact the operator might keep.

---

## 5. Expected success log line (after the controller fix is deployed)

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

## 6. Hand-off back to the design-note author

After the runbook completes, the operator should send back:

1. The three `GET /v1/snapshots` listings from step 1 (`$RECORD_DIR/snapshots-t+0s.json`, `…t+60s.json`, `…t+300s.json`).
2. The journal slice (`/tmp/forkd-journal.log`).
3. The four create+exec response bodies from step 3 (`$RECORD_DIR/create-<tag>.json`, `$RECORD_DIR/exec-<tag>.json`).
4. The state table from step 3 (which golden is missing, which is present-but-broken, which is healthy).
5. The controller baseline + EAGAIN-triggered samples from step 2 (`$RECORD_DIR/baseline-*.txt`, `$RECORD_DIR/eagain-samples.txt`) and a one-line summary of which resource (fd / pid / memory) is exhausted when EAGAIN fires.

The design note's §7 fix direction depends on signals 1–4:

- If `zen-gate-big` is **missing** on registry AND on disk → the fix is a re-commit from the canonical rootfs; the controller-side re-registration will pick it up.
- If `zen-gate-big` is **present-but-broken** → the fix is to replace the on-disk file (restore from a prior good snapshot, or re-commit from canonical rootfs); the cached metadata handles are stale.
- If `zen-gate-big` is **healthy by registry but restore_many still 400s at the gate** → the registry's success criterion is not a real canary; the controller-side fix is wrong and needs to be reworked to require a real restore-boot (not a files-exist check).

Signal 5 closes `zenprocess/ao-company#122`'s UNVERIFIED item (which
resource is exhausted). Once the operator's live signals are in, the
controller-side fixes for both issues can be reviewed against the actual
controller source on dellsrv.

---

## 7. Closing the maintenance window

```bash
# 7.1  Confirm the registry is healthy.
curl -sS -H "Authorization: Bearer $(cat ~/fabro-run/.forkd-token)" \
  http://127.0.0.1:8891/v1/snapshots \
  | jq 'length, [.[].tag // .[].snapshot_tag]'

# 7.2  Optionally fire one gate-poll cycle to verify the preflight's
#      `ok <tag>` path lands cleanly.
bash ~/Desktop/ao-company/bin/fabro-gate-poll.sh

# 7.3  Close the maintenance window.
zenctl maint off
```

If the post-cycle snapshot listing is non-empty and contains both golden
tags, the gate's preflight will start returning `ok <tag>` for them
immediately; the descriptions on previously-failing SHAs will not change
(the existing post_infra no-overwrite guard is preserved on purpose), so
the next NEW head will be the first to land with the preflight's clean
verdict.
