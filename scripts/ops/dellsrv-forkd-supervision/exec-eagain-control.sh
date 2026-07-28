#!/usr/bin/env bash
# =============================================================================
# exec-eagain-control.sh — the ONE experiment that splits the EAGAIN outage's
# hypothesis space in half.
#
# WHY THIS EXISTS
#   Starting 2026-07-25T23:44Z the fabro gate on uniforme began dying at
#       POST /v1/sandboxes/<id>/exec -> HTTP 500
#       {"error":"exec: read response: Resource temporarily unavailable"}
#   (EAGAIN). 182 occurrences by 2026-07-28, 218 INFRA / 10 FAIL / ZERO PASS.
#
#   Two families of explanation were on the table and they demand OPPOSITE
#   fixes:
#     (a) DETERMINISTIC/GLOBAL — a bug that fires on every exec regardless
#         of what runs (e.g. the reverse-proxy hop at
#         ~/fabro-run/forkd-shim.py forwards every request-shape except
#         sandbox-create with exactly ONE attempt — see that file's
#         `max_attempts` logic). Fix: retry/backoff at the exec hop.
#     (b) WORKLOAD-DEPENDENT — undersized guest, wrong snapshot_tag reaching
#         the sandbox, or output-volume/duration exceeding some buffer.
#         Fix: sizing/config, not retries.
#
#   A trivial `echo hi` exec on the SAME snapshot the gate actually uses
#   is the one experiment that discriminates between them: if the trivial
#   command ALSO EAGAINs, (a) is confirmed and (b) is dead — retrying a
#   deterministic failure just fails N times instead of once. If it
#   PASSES, (b) is back in play with a real workload/duration angle, and
#   the SLOW vs LARGE-OUTPUT follow-up (Part 2 below) narrows it further.
#
#   This script also timestamps the failure. A sub-second EAGAIN implies a
#   non-blocking fd with no poll/retry loop (a code defect in the
#   controller or its proxy); an EAGAIN near a configured timeout implies
#   a timeout, not a genuine resource-exhaustion signal.
#
# WHAT IT DOES (Part 1 — the control, always runs)
#   1. POST /v1/sandboxes with the given --tag.
#   2. POST /v1/sandboxes/{id}/exec with args=["echo","hi"], timing the call.
#   3. DELETE /v1/sandboxes/{id} — ALWAYS, via the EXIT trap, even on
#      failure. A prior probe (gate-health-probe.sh's early design) leaked
#      a live microVM on every failed run because teardown ran only after
#      a successful parse; this script scrapes the id BEFORE any die(),
#      same discipline as that script's fix.
#   4. Prints an unmistakable, greppable verdict line:
#        CONTROL-RESULT: PASS         (exit_code 0, workload-dependent theory survives)
#        CONTROL-RESULT: EAGAIN       (same signature as the outage, global fault)
#        CONTROL-RESULT: OTHER <detail>  (anything else — do not force a bucket)
#      and:
#        EXEC-ELAPSED: <seconds>      (timing evidence, see above)
#
# WHAT IT DOES (Part 2 — read-only diagnostic capture, opt-in via --diagnose)
#   One-pass capture so there is no round-trip per command needed on the
#   host. Every check prints `CHECK <name> <OK|FAIL|UNKNOWN> <detail>` and
#   is independently failable — one missing binary must not abort the rest.
#   Covers: forkd-ec.service state + recent journal; controller restart
#   detection vs registered /v1/snapshots (in-memory snapshot state is lost
#   on restart); orphan sandbox count; fd/proc limits; dmesg OOM/firecracker
#   signatures; disk space on the VM/snapshot store; memory + per-container
#   stats; per-child netns presence (the 2026-07-22 outage surface); and
#   whether the ~/fabro-run/forkd-shim.py reverse proxy is running, on
#   which port, and its configured FORKD_SHIM_FORWARD_TIMEOUT_S.
#
#   STRICTLY READ-ONLY. No systemctl start/stop/restart/enable/disable, no
#   docker restart/rm/stop, no forkd snapshot registration, no state-
#   mutating verb of any kind. A reviewer must be able to confirm this by
#   reading the script once.
#
# TOKEN HANDLING
#   The token at /etc/forkd-token lives INSIDE the forkd docker container.
#   Every controller call is wrapped in `docker exec -i forkd sh` with a
#   heredoc whose first action is `TOKEN=$(cat /etc/forkd-token)` — the
#   value never crosses the host argv or process table. Same pattern as
#   gate-health-probe.sh in this directory.
#
# USAGE (operator, on dellsrv)
#   sudo scripts/ops/dellsrv-forkd-supervision/exec-eagain-control.sh
#   sudo scripts/ops/dellsrv-forkd-supervision/exec-eagain-control.sh --tag zen-gate-big
#   sudo scripts/ops/dellsrv-forkd-supervision/exec-eagain-control.sh --diagnose
#   sudo scripts/ops/dellsrv-forkd-supervision/exec-eagain-control.sh --dry-run
#
#   Do NOT pipe this script if you care about its exit code — `script.sh |
#   tee log` reports tee's status, not the control's. Same caveat as
#   gate-health-probe.sh's "Exit code semantics" section in this
#   directory's README.
# =============================================================================
set -euo pipefail

FORKD_CONTAINER="${FORKD_CONTAINER:-forkd}"
FORKD_TOKEN_FILE="${FORKD_TOKEN_FILE:-/etc/forkd-token}"
FORKD_API_BASE="${FORKD_API_BASE:-http://127.0.0.1:8891}"
CONTROL_TAG="${FORKD_SNAPSHOT_TAG:-zen-gate-base}"
CONTROL_TIMEOUT_SECS="${CONTROL_TIMEOUT_SECS:-10}"
DRY_RUN=0
DIAGNOSE=0

SANDBOX_ID_FIELD="id"
SANDBOX_ID_FALLBACKS=("sandbox_id" "sid")
EXIT_CODE_FIELD="exit_code"
EXIT_CODE_FALLBACKS=("exitcode" "code")

usage() {
    cat <<'USAGE' >&2
exec-eagain-control.sh [--tag TAG] [--diagnose] [--dry-run]

  --tag TAG     snapshot_tag to test against (default: zen-gate-base;
                pass zen-gate-big to test the tag uniforme actually pins)
  --diagnose    also run the read-only diagnostic capture (Part 2)
  --dry-run     print what would run without making any API/host calls
USAGE
    exit 1
}

while [ $# -gt 0 ]; do
    case "$1" in
        --tag) CONTROL_TAG="$2"; shift 2 ;;
        --diagnose) DIAGNOSE=1; shift ;;
        --dry-run) DRY_RUN=1; shift ;;
        -h|--help) usage ;;
        *) echo "unknown arg: $1" >&2; usage ;;
    esac
done

# ---------- helpers (same conventions as gate-health-probe.sh) ----------
log() { printf '[exec-control] %s\n' "$*" >&2; }
die() { log "FATAL: $*"; exit 1; }

check() {
    # check <name> <OK|FAIL|UNKNOWN> <detail...>
    printf 'CHECK %s %s %s\n' "$1" "$2" "${*:3}"
}

# now_epoch prints a monotonic-enough wall-clock timestamp in seconds with
# millisecond resolution where available. Used only for elapsed-time
# evidence, never for correctness-critical logic.
now_epoch() {
    date +%s.%N 2>/dev/null || date +%s
}

in_container_curl() {
    local method="$1" path="$2" body="$3"
    local body_arg=""
    if [ -n "$body" ]; then
        body_arg="--data ${body@Q}"
    fi
    local path_q
    path_q="$(printf '%q' "$path")"

    docker exec -i "$FORKD_CONTAINER" sh <<EOF
TOKEN=\$(cat ${FORKD_TOKEN_FILE@Q})
curl -sS -X ${method@Q} ${FORKD_API_BASE@Q}${path_q} \\
  -H "Authorization: Bearer \$TOKEN" \\
  -H "Content-Type: application/json" \\
  ${body_arg:+$body_arg}
EOF
}

# scrape_field — identical contract to gate-health-probe.sh's helper of the
# same name (jq primary -> jq fallbacks -> regex escape hatch). Duplicated
# rather than sourced so this script has no load-bearing dependency on the
# other file's internals changing.
scrape_field() {
    local response="$1" prefix="$2" primary="$3"; shift 3
    local value

    value="$(printf '%s' "$response" | jq -r "${prefix}${primary} // empty" 2>/dev/null || true)"
    if [ -n "$value" ] && [ "$value" != "null" ]; then
        printf '%s' "$value"
        return 0
    fi

    for alt in "$@"; do
        value="$(printf '%s' "$response" | jq -r "${prefix}${alt} // empty" 2>/dev/null || true)"
        if [ -n "$value" ] && [ "$value" != "null" ]; then
            log "WARN: scraped field from fallback name '$alt'"
            printf '%s' "$value"
            return 0
        fi
    done

    value="$(printf '%s' "$response" | grep -oE "\"${primary}\":\"[^\"]+\"" \
             | head -1 | sed -E "s/^\"${primary}\":\"([^\"]+)\"/\1/")"
    if [ -n "$value" ]; then
        log "WARN: scraped field via regex escape hatch (jq path failed for '$primary')"
        printf '%s' "$value"
        return 0
    fi

    return 1
}

# ---------- preflight ----------
command -v docker >/dev/null 2>&1 || die "docker binary not found on host PATH"
command -v jq     >/dev/null 2>&1 || die "jq is required to parse API responses"

if [ "$DRY_RUN" != "1" ]; then
    docker inspect "$FORKD_CONTAINER" >/dev/null 2>&1 \
        || die "docker container '$FORKD_CONTAINER' is not present"
fi

# =============================================================================
# PART 1 — the control experiment
# =============================================================================
SANDBOX_ID=""
# Invoked via the EXIT trap; SC2329 is a false positive here.
# shellcheck disable=SC2329
teardown_sandbox() {
    if [ -z "$SANDBOX_ID" ]; then
        log "WARN: EXIT trap teardown: SANDBOX_ID is empty — VM may have leaked; check forkd sandboxes manually and delete orphans"
        return 0
    fi
    log "teardown: DELETE /v1/sandboxes/$SANDBOX_ID"
    if [ "$DRY_RUN" = "1" ]; then
        log "DRY-RUN: would DELETE $FORKD_API_BASE/v1/sandboxes/$SANDBOX_ID"
        return 0
    fi
    if ! in_container_curl DELETE "/v1/sandboxes/$SANDBOX_ID" ""; then
        log "WARN: teardown DELETE failed for $SANDBOX_ID (gate will reap)"
    fi
}
trap 'teardown_sandbox' EXIT

log "control: tag=$CONTROL_TAG cmd=[echo,hi] timeout_secs=$CONTROL_TIMEOUT_SECS"
create_body="$(printf '{"snapshot_tag":"%s"}' "$CONTROL_TAG")"

if [ "$DRY_RUN" = "1" ]; then
    log "DRY-RUN: would POST $FORKD_API_BASE/v1/sandboxes body=$create_body"
    log "DRY-RUN: would exec echo/hi, time it, then DELETE"
    [ "$DIAGNOSE" = "1" ] && log "DRY-RUN: would also run Part 2 diagnostics"
    echo "CONTROL-RESULT: DRY-RUN"
    exit 0
fi

create_response="$(in_container_curl POST "/v1/sandboxes" "$create_body")"
log "create response (raw): $create_response"

# Scrape id BEFORE any die() — same load-bearing ordering as
# gate-health-probe.sh's fix for the 2026-07-25 leak (sb-6a64b43f-0031
# stayed alive when the old code parsed after a check that could die()
# first). The EXIT trap must always have something to clean up.
SANDBOX_ID="$(scrape_field "$create_response" ".[0]." "$SANDBOX_ID_FIELD" "${SANDBOX_ID_FALLBACKS[@]}")" || SANDBOX_ID=""
if [ -n "$SANDBOX_ID" ]; then
    log "sandbox id: $SANDBOX_ID"
else
    log "WARN: could not extract sandbox id from create response — possible VM leak"
    echo "CONTROL-RESULT: OTHER create_response_unparseable"
    die "create response did not yield a sandbox id: $create_response"
fi

exec_body='{"args":["echo","hi"],"timeout_secs":'"$CONTROL_TIMEOUT_SECS"'}'
log "exec: POST /v1/sandboxes/$SANDBOX_ID/exec body=$exec_body"

start_ts="$(now_epoch)"
set +e
exec_response="$(in_container_curl POST "/v1/sandboxes/$SANDBOX_ID/exec" "$exec_body")"
exec_status=$?
set -e
end_ts="$(now_epoch)"
elapsed="$(awk -v a="$start_ts" -v b="$end_ts" 'BEGIN{printf "%.3f", (b-a)}' 2>/dev/null || echo "unknown")"

log "exec response (raw): $exec_response"
echo "EXEC-ELAPSED: ${elapsed}"

if [ $exec_status -ne 0 ]; then
    echo "CONTROL-RESULT: OTHER in_container_curl_failed_status=$exec_status"
    die "docker exec / curl transport itself failed (status=$exec_status), not a controller response"
fi

if printf '%s' "$exec_response" | grep -qiE 'resource temporarily unavailable|os error 11|eagain|ewouldblock'; then
    echo "CONTROL-RESULT: EAGAIN"
    log "control EAGAIN'd on a trivial echo — fault is deterministic/global, not workload-dependent"
    log "elapsed=${elapsed}s — sub-second implies a non-blocking fd with no retry loop; ~120s implies a proxy timeout (see FORKD_SHIM_FORWARD_TIMEOUT_S check under --diagnose)"
    exit_after_control=1
else
    exit_code="$(scrape_field "$exec_response" "." "$EXIT_CODE_FIELD" "${EXIT_CODE_FALLBACKS[@]}")" || exit_code=""
    if [ "$exit_code" = "0" ]; then
        echo "CONTROL-RESULT: PASS"
        log "control PASSED — the workload-dependent hypothesis family survives; try the slow/large-output follow-up next"
        exit_after_control=0
    else
        echo "CONTROL-RESULT: OTHER unexpected_exit_code=${exit_code:-<unparsed>} response=$exec_response"
        exit_after_control=1
    fi
fi

# =============================================================================
# PART 2 — read-only diagnostic capture (opt-in: --diagnose)
# =============================================================================
if [ "$DIAGNOSE" = "1" ]; then
    log "---- Part 2: read-only diagnostics ----"

    if systemctl is-active forkd-ec.service >/dev/null 2>&1; then
        check forkd-ec.service OK "$(systemctl is-active forkd-ec.service 2>&1) / $(systemctl is-enabled forkd-ec.service 2>&1)"
    else
        check forkd-ec.service FAIL "not active: $(systemctl is-active forkd-ec.service 2>&1 || true)"
    fi
    journal_tail="$(journalctl -u forkd-ec.service -n 20 --no-pager 2>&1 | tail -20 | tr '\n' '|' || echo unavailable)"
    check forkd-ec.service-journal INFO "$journal_tail"

    snapshots_response="$(in_container_curl GET "/v1/snapshots" "" 2>/dev/null || echo "")"
    if [ -n "$snapshots_response" ]; then
        check registered-snapshots OK "$snapshots_response"
    else
        check registered-snapshots UNKNOWN "GET /v1/snapshots returned nothing — controller may not expose this route, or a restart lost in-memory state"
    fi

    sandboxes_response="$(in_container_curl GET "/v1/sandboxes" "" 2>/dev/null || echo "")"
    orphan_count="$(printf '%s' "$sandboxes_response" | jq 'length' 2>/dev/null || echo unknown)"
    check orphan-sandboxes "$([ "$orphan_count" = "0" ] && echo OK || echo FAIL)" "count=$orphan_count response=$sandboxes_response"

    fd_limit="$(docker exec "$FORKD_CONTAINER" sh -c 'ulimit -n' 2>/dev/null || echo unknown)"
    check fd-limit INFO "ulimit -n (in container) = $fd_limit"

    pids_current="$(docker exec "$FORKD_CONTAINER" sh -c 'cat /sys/fs/cgroup/pids.current 2>/dev/null' 2>/dev/null || echo unknown)"
    pids_max="$(docker exec "$FORKD_CONTAINER" sh -c 'cat /sys/fs/cgroup/pids.max 2>/dev/null' 2>/dev/null || echo unknown)"
    check pids-cgroup INFO "current=$pids_current max=$pids_max"

    ps_count="$(ps -eLf 2>/dev/null | wc -l | tr -d ' ' || echo unknown)"
    check host-thread-count INFO "$ps_count"

    oom_lines="$(dmesg -T 2>/dev/null | grep -iE 'oom|out of memory|firecracker|cannot allocate' | tail -10 || true)"
    if [ -n "$oom_lines" ]; then
        check dmesg-oom FAIL "$(printf '%s' "$oom_lines" | tr '\n' '|')"
    else
        check dmesg-oom OK "no OOM/firecracker-alloc signatures in recent dmesg"
    fi

    disk_line="$(df -h / 2>/dev/null | tail -1 || echo unknown)"
    check disk-space INFO "$disk_line"

    mem_line="$(free -g 2>/dev/null | tail -2 | tr '\n' '|' || echo unknown)"
    check memory INFO "$mem_line"

    netns_count=0
    for _ns in /var/run/netns/forkd-child-*; do
        [ -e "$_ns" ] && netns_count=$((netns_count + 1))
    done
    check per-child-netns "$([ "$netns_count" -ge 1 ] && echo OK || echo FAIL)" "forkd-child netns count=$netns_count"

    if pgrep -f "fabro-run/forkd-shim.py" >/dev/null 2>&1; then
        shim_pid="$(pgrep -f "fabro-run/forkd-shim.py" | head -1)"
        shim_env="$(tr '\0' ' ' < "/proc/$shim_pid/environ" 2>/dev/null | grep -o 'FORKD_SHIM_FORWARD_TIMEOUT_S=[0-9]*' || echo "unset (defaults to 120)")"
        check forkd-shim-proxy OK "pid=$shim_pid $shim_env"
    else
        check forkd-shim-proxy UNKNOWN "no fabro-run/forkd-shim.py process found on host PATH via pgrep — may run under a different invocation or on a different host"
    fi

    log "---- end diagnostics ----"
fi

exit "$exit_after_control"
