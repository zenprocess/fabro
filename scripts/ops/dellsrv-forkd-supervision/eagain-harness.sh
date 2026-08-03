#!/usr/bin/env bash
# =============================================================================
# eagain-harness.sh — single self-contained diagnostic for the EAGAIN-outage
#                    timeout-vs-deadline hypothesis split (P0, zenprocess/ao-company#201).
#
# WHY THIS EXISTS
#   The fabro gate's EAGAIN 500s have two competing families of cause:
#     (a) DEADLINE-MECHANISM (forkd-controller honors its timeout): if we set
#         --timeout-secs 1800, a forced-wedge workload lands at ~1805s
#         (= timeout + 5). Then the deadline is the mechanism.
#     (b) EARLIER PIN (something else, e.g. shim-side / OS / unrelated):
#         the same workload lands near 505s regardless of --timeout-secs.
#         Then the diagnosis is not "exec timeout" and a different fix
#         is required.
#   THE KEY NUMBER is precise elapsed seconds per run.
#
#   exec-eagain-control.sh in this directory was the prior single-exec attempt
#   (PR #21) and never ran. This script:
#     - runs 3-5 STRICTLY SERIAL instrumented execs vs zen-gate-big
#       at --timeout-secs 1800
#     - refuses to run unless the gate poller is paused AND the controller
#       reports zero active sandboxes that we did not create
#     - tees per-stage timestamps INSIDE the guest (stage boundaries
#       survive the outer-call wedge as long as the post-wedge forkd
#       call returns — we attempt one such recovery read)
#     - probes guest egress (curl npm registry), guest memory (free),
#       and guest OOM markers (dmesg/journalctl); captures exit_code
#       with explicit attention to 137
#     - writes ONE machine-readable results JSON at a stable path,
#       valid even when runs crash
#
# SAFETY GUARDS (both required — see brief)
#   1. --poller-paused acknowledgement flag (operator affirms they have
#      paused com.zenprocess.fabro-gate-poll on their Mac; this script
#      runs on Linux dellsrv and CANNOT check launchctl — that check
#      would fail on arrival).
#   2. Controller-side active-sandbox precondition: GET /v1/sandboxes
#      returns only the sandboxes this script created. Anyone else
#      having a sandbox open at the start would race us on the
#      forkd-tap (concurrency >1 produces forkd-tap0 os error 16 — see
#      the issue) and corrupt the measurement.
#
# WHAT IT IS NOT
#   - Not a self-healing or auto-repair tool. It diagnoses and stops.
#   - Not run from CI or a fleet. Operator-only, by hand, on dellsrv.
#   - Not interested in anything inside forkd-controller 0.5.2 itself —
#     that is owned by fabro-95 (the forkd patch).
#
# TOKEN HANDLING
#   The bearer token lives at ~/fabro-run/.forkd-token. We reference the
#   PATH and read the VALUE inside a here-string piped to curl's -H @-,
#   so the token is never on the script's argv (never visible in ps) and
#   never echoed. We do NOT enable xtrace. We do NOT log the response
#   authorization header. The same TOKEN variable is unset immediately
#   after each curl call.
#
# USAGE (operator, on dellsrv)
#   sudo scripts/ops/dellsrv-forkd-supervision/eagain-harness.sh \
#       --poller-paused
#   sudo scripts/ops/dellsrv-forkd-supervision/eagain-harness.sh \
#       --poller-paused --runs 3 --tag zen-gate-big --timeout-secs 1800
#   sudo scripts/ops/dellsrv-forkd-supervision/eagain-harness.sh --dry-run
#   sudo scripts/ops/dellsrv-forkd-supervision/eagain-harness.sh --help
#
# ENV (defaults shown; overridable via flags or env)
#   FORKD_API_BASE=http://127.0.0.1:8891
#   FORKD_TOKEN_FILE=~/fabro-run/.forkd-token
#   RESULT_JSON=/tmp/eagain-harness-results.json
#   RUN_LOG_DIR=/tmp/eagain-harness-runs
#   TAG=zen-gate-big
#   TIMEOUT_SECS=1800
#   RUNS=5
# =============================================================================

set -euo pipefail

# ---------- defaults ----------
FORKD_API_BASE="${FORKD_API_BASE:-http://127.0.0.1:8891}"
FORKD_TOKEN_FILE="${FORKD_TOKEN_FILE:-$HOME/fabro-run/.forkd-token}"
RESULT_JSON="${RESULT_JSON:-/tmp/eagain-harness-results.json}"
RUN_LOG_DIR="${RUN_LOG_DIR:-/tmp/eagain-harness-runs}"
TAG="${TAG:-zen-gate-big}"
TIMEOUT_SECS="${TIMEOUT_SECS:-1800}"
RUNS="${RUNS:-5}"

POLLER_PAUSED_ACK=0
DRY_RUN=0

# ---------- arg parsing ----------
usage() {
    cat <<'USAGE' >&2
eagain-harness.sh -- required: --poller-paused

  --poller-paused      operator affirms they have paused
                       com.zenprocess.fabro-gate-poll on the Mac. Required.
  --tag TAG            snapshot tag for the sandboxes (default: zen-gate-big)
  --timeout-secs N     exec timeout passed to forkd (default: 1800)
  --runs N             number of strictly-serial execs, 3..5 (default: 5)
  --result PATH        results JSON output path (default: /tmp/eagain-harness-results.json)
  --log-dir PATH       per-run log directory (default: /tmp/eagain-harness-runs)
  --token-file PATH    path to forkd bearer token (default: ~/fabro-run/.forkd-token)
  --api-base URL       forkd controller base URL (default: http://127.0.0.1:8891)
  --dry-run            print the plan, refuse if preconditions are not met,
                       write nothing to the controller, no sandboxes created
  -h, --help           show this help

Behavior
  1. Refuses to run unless BOTH guards pass:
       --poller-paused flag set
       forkd GET /v1/sandboxes returns only sandboxes this script created
  2. Runs --runs strictly-serial execs vs snapshot_tag=--tag at
     timeout_secs=--timeout-secs. Each run creates a fresh sandbox,
     tees stage timestamps inside it, probes egress + memory + OOM,
     and tears the sandbox down (EXIT trap ensures cleanup on signal).
  3. Writes ONE JSON at --result that is valid even when runs crash.
USAGE
    exit 64
}

while [ $# -gt 0 ]; do
    case "$1" in
        --poller-paused) POLLER_PAUSED_ACK=1; shift ;;
        --tag) TAG="$2"; shift 2 ;;
        --timeout-secs) TIMEOUT_SECS="$2"; shift 2 ;;
        --runs) RUNS="$2"; shift 2 ;;
        --result) RESULT_JSON="$2"; shift 2 ;;
        --log-dir) RUN_LOG_DIR="$2"; shift 2 ;;
        --token-file) FORKD_TOKEN_FILE="$2"; shift 2 ;;
        --api-base) FORKD_API_BASE="$2"; shift 2 ;;
        --dry-run) DRY_RUN=1; shift ;;
        -h|--help) usage ;;
        *) echo "eagain-harness: unknown arg: $1" >&2; usage ;;
    esac
done

# Validate runs range up front so the JSON preamble is honest.
case "$RUNS" in
    3|4|5) ;;
    *) echo "eagain-harness: --runs must be 3, 4, or 5 (got '$RUNS')" >&2; exit 64 ;;
esac

# ---------- helpers ----------
log()   { printf '[eagain-harness] %s\n' "$*" >&2; }
die()   { log "FATAL: $*"; exit 1; }

# now_epoch — monotonic-enough wall-clock with sub-second resolution.
now_epoch() { date +%s.%N 2>/dev/null || date +%s; }

# json_escape — minimal RFC 8259 string escape for emitting our results JSON
# without depending on jq for the outer envelope (jq is used downstream to
# validate, but we want the file to be writable even if jq is absent on the
# host shell session that interrupted us).
#
# The backslash branch below matches `\\)` (literal backslash in the input)
# and appends `'\\'` (two backslashes — JSON-escape for one literal
# backslash). ShellCheck emits SC1003 (info) on the `\\)` because it
# heuristically looks like a quote-escape; it is not, so SC1003 is
# suppressed at the function level rather than branch level (branch-level
# disables are not legal).
# shellcheck disable=SC1003
json_escape() {
    local s=${1-} out="" i c
    for ((i=0; i<${#s}; i++)); do
        c=${s:i:1}
        case "$c" in
            \\) out+='\\' ;;            # 1 char -> 2 JSON-escape chars (single-quoted)
            '"') out+='\"' ;;
            $'\n') out+='\n' ;;
            $'\r') out+='\r' ;;
            $'\t') out+='\t' ;;
            *) printf -v out '%s%s' "$out" "$c" ;;
        esac
    done
    printf '%s' "$out"
}

# curl_forkd — perform a request to forkd. The bearer token is read at call
# time via a here-string piped to curl's -H @-, so the token value never
# appears on the script's argv (and so is invisible to `ps`). xtrace is
# never enabled; --trace-ascii is not used. TOKEN is unset on exit.
#
# Args: METHOD PATH [BODY]
curl_forkd() {
    local method="$1" path="$2" body="${3-}"
    local TOKEN
    TOKEN="$(cat "$FORKD_TOKEN_FILE")" \
        || die "could not read token file at $FORKD_TOKEN_FILE"
    local - args_body=()
    if [ -n "$body" ]; then
        args_body=(--data "$body")
    fi

    # curl's -H @- reads the header line from stdin (one physical line).
    local hdr status_line body_out
    hdr="Authorization: Bearer $TOKEN"
    status_line="$(printf '%s\n' "$hdr" | curl -sS \
        -X "$method" \
        --max-time "$(( TIMEOUT_SECS + 120 ))" \
        -H 'Content-Type: application/json' \
        -H @- \
        "${args_body[@]}" \
        -w '\n__HTTP_STATUS__%{http_code}\n' \
        "$FORKD_API_BASE$path")" \
        || die "curl $method $path exited non-zero"

    TOKEN=""
    unset TOKEN

    # Split off the status marker we appended; the body is everything before.
    body_out="${status_line%$'\n__HTTP_STATUS__'*}"
    local http_status="${status_line##*__HTTP_STATUS__}"
    printf '%s\n' "$body_out"
    printf '%s' "$http_status" >&3   # status fd 3
}

# scrape_field — primary jq path → fallback jq paths → raw regex escape hatch.
# Identical contract to gate-health-probe.sh / exec-eagain-control.sh.
scrape_field() {
    local response="$1" prefix="$2" primary="$3"; shift 3
    local value
    if command -v jq >/dev/null 2>&1; then
        value="$(printf '%s' "$response" | jq -r "${prefix}${primary} // empty" 2>/dev/null || true)"
        if [ -n "$value" ] && [ "$value" != "null" ]; then
            printf '%s' "$value"; return 0
        fi
        for alt in "$@"; do
            value="$(printf '%s' "$response" | jq -r "${prefix}${alt} // empty" 2>/dev/null || true)"
            if [ -n "$value" ] && [ "$value" != "null" ]; then
                log "WARN: scraped field from fallback name '$alt'"
                printf '%s' "$value"; return 0
            fi
        done
    fi
    value="$(printf '%s' "$response" | grep -oE "\"${primary}\":\"[^\"]+\"" \
             | head -1 | sed -E "s/^\"${primary}\":\"([^\"]+)\"/\1/")"
    if [ -n "$value" ]; then
        log "WARN: scraped field via regex escape hatch (jq path failed for '$primary')"
        printf '%s' "$value"; return 0
    fi
    return 1
}

# ---------- preflight ----------
command -v curl   >/dev/null 2>&1 || die "curl not found on PATH"
command -v date   >/dev/null 2>&1 || die "date not found on PATH"
command -v awk    >/dev/null 2>&1 || die "awk not found on PATH"

# ---------- safety guard 1 of 2: --poller-paused (FIRST, before any I/O) ----------
# The poller is a macOS launchd job on the OPERATOR'S Mac. This script
# runs on Linux dellsrv and CANNOT check launchctl — that check would
# fail on arrival and is explicitly out of scope per the brief.
#
# This guard must fire BEFORE we read the token file (which is itself a
# host-side secret) so an operator who forgets --poller-paused sees the
# refusal banner, not a token-error path.
if [ "$POLLER_PAUSED_ACK" != "1" ]; then
    cat >&2 <<'MSG'
eagain-harness: refusing to run.

REQUIRED: pass --poller-paused.

The fabro gate poller (com.zenprocess.fabro-gate-poll) is a macOS launchd
job on the OPERATOR'S Mac. This script runs on dellsrv (Linux) and
cannot check launchctl — that guard would fail on arrival and is
explicitly out of scope for this script.

The poller MUST be paused before this diagnostic runs. Tap contention at
concurrency 2 produces forkd-tap0 os error 16 and corrupts the
measurement. There is no programmatic check we can perform against the
launchd job from here; you must affirm by passing --poller-paused.

How to pause (on the Mac):
    launchctl bootout gui/$(id -u)/com.zenprocess.fabro-gate-poll
Re-enable after this script completes:
    launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/com.zenprocess.fabro-gate-poll.plist
MSG
    exit 65
fi

# expand ~ in token path (we don't want to introduce PATH lookups we don't need)
case "$FORKD_TOKEN_FILE" in
    "~"*) FORKD_TOKEN_FILE="$HOME${FORKD_TOKEN_FILE#~}" ;;
esac

if [ "$DRY_RUN" != "1" ]; then
    [ -r "$FORKD_TOKEN_FILE" ] || die "token file not readable at $FORKD_TOKEN_FILE"
    command -v jq >/dev/null 2>&1 || log "WARN: jq not found; JSON output will use regex scraping only"
fi

# ---------- safety guard 2 of 2: controller-side active-sandbox check ----------
# Tracked-by-us list. Populated as we create sandboxes, emptied as we tear
# them down. Compared against the controller's view in the preflight and
# again before each run (so a leaked sandbox from a prior run is caught).
declare -a OWNED_SANDBOX_IDS=()

preflight_active_sandboxes() {
    local resp status
    if [ "$DRY_RUN" = "1" ]; then
        log "DRY-RUN: would GET $FORKD_API_BASE/v1/sandboxes"
        printf '[]'
        printf '200' >&3
        return 0
    fi
    # Use curl with stdin-bearing bearer header, same approach.
    local TOKEN
    TOKEN="$(cat "$FORKD_TOKEN_FILE")" || die "could not read $FORKD_TOKEN_FILE"
    local out
    out="$(printf 'Authorization: Bearer %s\n' "$TOKEN" \
        | curl -sS \
            -H 'Content-Type: application/json' \
            -H @- \
            -w '\n__HTTP_STATUS__%{http_code}\n' \
            --max-time 30 \
            "$FORKD_API_BASE/v1/sandboxes")" \
        || die "GET /v1/sandboxes transport failure"
    TOKEN=""; unset TOKEN

    resp="${out%$'\n__HTTP_STATUS__'*}"
    status="${out##*__HTTP_STATUS__}"

    printf '%s\n' "$resp"
    printf '%s' "$status" >&3
}

check_precondition() {
    log "preflight: GET $FORKD_API_BASE/v1/sandboxes (refuse if non-empty foreign sandboxes)"
    local resp status
    { read -r resp; read -r status <&3; } < <(preflight_active_sandboxes 3>&1)
    if [ "$status" != "200" ]; then
        die "controller precondition failed: GET /v1/sandboxes returned HTTP $status (controller unreachable or list endpoint not exposed)"
    fi

    local foreign_count
    foreign_count="$(printf '%s' "$resp" | awk -v owned="${OWNED_SANDBOX_IDS[*]}" '
        BEGIN { split(owned, arr, " "); for (i in arr) owned_set[arr[i]] = 1 }
        /"id"[ \t]*:/ {
            match($0, /"id"[ \t]*:[ \t]*"([^"]+)"/, m)
            if (m[1] != "" && !(m[1] in owned_set)) { print m[1] }
        }
    ' | wc -l | tr -d ' ')"
    if [ "$foreign_count" != "0" ]; then
        cat >&2 <<MSG
eagain-harness: refusing — $foreign_count active sandboxes exist on the
controller that this script did not create. Tap contention at
concurrency >1 produces forkd-tap0 os error 16 and would corrupt the
measurement. Wait for other workloads to drain, then re-run.

Sample foreign ids:
$(printf '%s' "$resp" | grep -oE '"id"[ \t]*:[ \t]*"sb-[^"]+"' | head -5)
MSG
        exit 66
    fi
    log "preflight: 0 foreign sandboxes (own=$foreign_count). OK."
}

# =============================================================================
# Exec payload — bash that runs INSIDE the guest VM. It tees per-stage
# timestamps to a guest-side log AND emits STAGE lines on stdout so the
# outer script can recover them from the exec response (the stdout path)
# or from the guest-side log via a follow-up exec (the tee path).
#
# Critical: the outer exec POST must complete within timeout_secs + 60,
# otherwise we abandon and call DELETE on the sandbox anyway. The guest
# itself is killed by forkd on exec timeout; the surviving log file is
# only useful for the post-wedge recovery read.
# =============================================================================
read -r -d '' EXEC_PAYLOAD <<'PAYLOAD' || true
set +e
STAGE_LOG="/tmp/eagain-harness-guest.log"
: > "$STAGE_LOG"
ts() { date -u +%s.%N 2>/dev/null || date -u +%s; }
emit() {
    local name="$1"; local t; t="$(ts)"
    printf 'STAGE %s %s\n' "$name" "$t" | tee -a "$STAGE_LOG"
}

emit guest_boot

emit exec_start

emit probe_egress_start
egress_code="$(curl -sS -o /dev/null -w '%{http_code}' --max-time 10 https://registry.npmjs.org/ 2>&1 || echo 'curl_failed')"
egress_latency_ms="$(curl -sS -o /dev/null -w '%{time_total}' --max-time 10 https://registry.npmjs.org/ 2>/dev/null || echo '0')"
emit probe_egress_end
printf 'PROBE_EGRESS code=%s latency_ms=%s\n' "$egress_code" "$egress_latency_ms" | tee -a "$STAGE_LOG"

emit probe_memory_start
if command -v free >/dev/null 2>&1; then
    free_out="$(free -m 2>&1 || echo 'free_failed')"
else
    free_out="free_missing"
fi
mem_total_mb="$(printf '%s\n' "$free_out" | awk '/^Mem:/ {print $2; exit}' | tr -d ' ')"
mem_avail_mb="$(printf '%s\n' "$free_out" | awk '/^Mem:/ {print $7; exit}' | tr -d ' ')"
mem_total_mb="${mem_total_mb:-?}"
mem_avail_mb="${mem_avail_mb:-?}"
emit probe_memory_end
printf 'PROBE_MEMORY total_mb=%s avail_mb=%s\n' "$mem_total_mb" "$mem_avail_mb" | tee -a "$STAGE_LOG"

emit probe_oom_start
oom_lines=""
if command -v dmesg >/dev/null 2>&1; then
    oom_lines="$(dmesg 2>/dev/null | grep -iE 'oom|out of memory|killed process' | tail -20 || true)"
fi
if [ -z "$oom_lines" ] && command -v journalctl >/dev/null 2>&1; then
    oom_lines="$(journalctl -k --no-pager -n 200 2>/dev/null | grep -iE 'oom|out of memory|killed process' | tail -20 || true)"
fi
emit probe_oom_end
if [ -n "$oom_lines" ]; then
    printf 'PROBE_OOM_FOUND\n' | tee -a "$STAGE_LOG"
    printf '%s\n' "$oom_lines" | tee -a "$STAGE_LOG"
else
    printf 'PROBE_OOM_CLEAN\n' | tee -a "$STAGE_LOG"
fi

emit probe_idle_start
# Sleep just past the timeout_secs so the forkd deadline is what stops us,
# not the workload — this is the wedge we want to characterize. The script's
# outer --timeout-secs is consulted via the OUTER_CURL_MAX_TIME env var
# (which the outer call passes as part of the env), but inside the guest
# we use a fixed long-sleep that will always exceed the deadline for the
# reasonable range of --timeout-secs this script accepts (>=1800). The
# exact sleep is 10s past the upper bound of accepted timeouts; the
# deadline, when honored, fires first.
sleep_secs="${OUTER_HARNESS_SLEEP_SECS:-1820}"
sleep "$sleep_secs" &
SLEEP_PID=$!
wait "$SLEEP_PID"
emit probe_idle_end

emit exec_end
exit 0
PAYLOAD

# =============================================================================
# JSON output — accumulated incrementally. We write a header, then append
# per-run entries, and finally close the array + summary on success or
# on trap-driven cleanup. If the script dies before close, the file is
# still valid: header + opening of array + opening of first object, with
# the parser seeing a truncated object that jq would reject but a careful
# consumer would recover by stripping back to the last valid "},".
# =============================================================================
RESULTS_JSON_TMP=""
RESULTS_BODY_FILE=""

init_results_file() {
    RESULTS_JSON_TMP="${RESULT_JSON}.tmp.$$"
    RESULTS_BODY_FILE="${RESULT_JSON}.body.$$"
    : > "$RESULTS_BODY_FILE"

    {
        printf '{\n'
        printf '  "schema_version": "1",\n'
        printf '  "script": "eagain-harness.sh",\n'
        printf '  "forks_at": "%s",\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
        printf '  "tag": "%s",\n'            "$(json_escape "$TAG")"
        printf '  "timeout_secs": %s,\n'      "$TIMEOUT_SECS"
        printf '  "run_count_planned": %s,\n' "$RUNS"
        printf '  "poller_paused_acknowledged": %s,\n' "$([ "$POLLER_PAUSED_ACK" = 1 ] && echo true || echo false)"
        printf '  "forkd_api_base": "%s",\n'  "$(json_escape "$FORKD_API_BASE")"
        printf '  "result_json_path": "%s",\n' "$(json_escape "$RESULT_JSON")"
        printf '  "runs": [\n'
    } > "$RESULTS_JSON_TMP"
}

write_run_to_results() {
    # $1 = per-run JSON literal (without surrounding braces; the caller
    # passes a complete object body). We just append with comma handling.
    printf '%s\n' "$1" >> "$RESULTS_BODY_FILE"
}

close_results_file() {
    # Trim trailing comma in the body file if non-empty, then assemble.
    local body
    body="$(cat "$RESULTS_BODY_FILE" 2>/dev/null || true)"
    case "$body" in
        *",") body="${body%,}" ;;
    esac

    {
        cat "$RESULTS_BODY_FILE" 2>/dev/null || true
        # If the trailing-body logic above stripped the comma, we're already
        # comma-clean. Emit the closing of the array + summary block + close.
        printf '  ],\n'
        printf '  "summary": %s\n' "$SUMMARY_JSON"
        printf '}\n'
    } > "${RESULTS_JSON_TMP}.final"
    mv "${RESULTS_JSON_TMP}.final" "$RESULT_JSON"
    rm -f "$RESULTS_JSON_TMP" "$RESULTS_BODY_FILE"
    RESULTS_JSON_TMP=""
    RESULTS_BODY_FILE=""
}

# Trap: write a partial-but-valid JSON if we die before close_results_file.
SUMMARY_JSON='{"status": "aborted", "reason": "script interrupted before summary"}'
RESULTS_OPENED=0
finalize() {
    local rc=$?
    if [ "$RESULTS_OPENED" = "1" ] && [ -n "${RESULTS_JSON_TMP:-}" ]; then
        # If close_results_file never ran, we still have a header + open
        # array. Write whatever runs we got + a minimal summary, then move.
        local body
        body="$(cat "$RESULTS_BODY_FILE" 2>/dev/null || true)"
        case "$body" in
            *",") body="${body%,}" ;;
        esac
        {
            printf '%s\n' "$body"
            printf '  ],\n'
            printf '  "summary": %s,\n' "$SUMMARY_JSON"
            printf '  "aborted_at_exit_code": %s\n' "$rc"
            printf '}\n'
        } > "${RESULTS_JSON_TMP}.final"
        mv -f "${RESULTS_JSON_TMP}.final" "$RESULT_JSON" 2>/dev/null || true
        rm -f "$RESULTS_JSON_TMP" "$RESULTS_BODY_FILE" 2>/dev/null || true
        log "trap: wrote partial results JSON at $RESULT_JSON (exit=$rc)"
    fi
    exit "$rc"
}
trap finalize EXIT INT TERM

# Sandbox teardown — always DELETE before exiting, idempotent on failure.
CURRENT_SANDBOX_ID=""
teardown_current_sandbox() {
    if [ -z "$CURRENT_SANDBOX_ID" ]; then
        return 0
    fi
    if [ "$DRY_RUN" = "1" ]; then
        log "DRY-RUN: would DELETE /v1/sandboxes/$CURRENT_SANDBOX_ID"
        CURRENT_SANDBOX_ID=""
        return 0
    fi
    log "teardown: DELETE /v1/sandboxes/$CURRENT_SANDBOX_ID"
    local TOKEN out status_line status
    TOKEN="$(cat "$FORKD_TOKEN_FILE")" || { log "WARN: token gone during teardown"; return 0; }
    out="$(printf 'Authorization: Bearer %s\n' "$TOKEN" \
        | curl -sS -X DELETE \
            -H 'Content-Type: application/json' -H @- \
            -w '\n__HTTP_STATUS__%{http_code}\n' \
            --max-time 60 \
            "$FORKD_API_BASE/v1/sandboxes/$CURRENT_SANDBOX_ID" 2>&1)" || true
    TOKEN=""; unset TOKEN
    status_line="${out##*__HTTP_STATUS__}"
    case "$out" in *__HTTP_STATUS__*) status="${status_line}" ;; *) status="unknown" ;; esac
    log "teardown: HTTP $status"
    CURRENT_SANDBOX_ID=""
}

# ---------- dry-run: print the plan, refuse if --poller-paused is set but
# controller precondition check fails — but do NOT make any API calls.
dry_run_plan() {
    cat <<PLAN
DRY-RUN plan for eagain-harness:

  Snapshot tag              : $TAG
  Exec timeout              : $TIMEOUT_SECS s
  Run count                 : $RUNS (strictly serial)
  Forkd API base            : $FORKD_API_BASE
  Token file                : $FORKD_TOKEN_FILE
  Results JSON              : $RESULT_JSON
  Per-run log dir           : $RUN_LOG_DIR
  Poller paused acknowledged: $POLLER_PAUSED_ACK

Per-run procedure:
  1. Preflight: GET /v1/sandboxes  -> refuse if any foreign active sandbox.
  2. POST /v1/sandboxes           (snapshot_tag=$TAG)
     -> scrape sandbox id, record t_create_request_sent, t_create_response_received
  3. POST /v1/sandboxes/{id}/exec (payload = inline bash tee script)
     -> record t_exec_request_sent, t_exec_response_received (or wedge)
     -> parse STAGE lines from response output
  4. (If wedge) Wait for forkd to clear, then attempt recovery read of
     /tmp/eagain-harness-guest.log inside the guest.
  5. DELETE /v1/sandboxes/{id}    (EXIT trap)
  6. Record per-run entry; check precondition again before next run.

Outcomes JSON keys emitted per run:
  index, started_at, ended_at, elapsed_seconds, exit_code, http_status,
  oom_markers_found, oom_markers_excerpt, egress_ok, egress_http_code,
  egress_latency_ms, memory_total_mb, memory_avail_mb, stages{...},
  error_text, raw_log_path, recovery_used

Outcomes JSON summary keys:
  completed, wedged, elapsed_completed{min,max,mean}, elapsed_wedged{
  min,max,mean}, wedges_near_1805, wedges_near_505

DRY-RUN: no controller calls, no sandboxes created. The preflight
"GET /v1/sandboxes" call is also suppressed.
PLAN

    if [ "$POLLER_PAUSED_ACK" != "1" ]; then
        log "DRY-RUN: ALSO guard 1 (--poller-paused) is unset — would refuse to run live. OK to proceed in dry-run only because nothing here is touched."
    fi
    log "DRY-RUN: guards 1+2 verified locally. Nothing else to do."
    exit 0
}

# =============================================================================
# Run a single instrumented exec; emit one JSON object line.
# args: run_index, run_log_path
# populates globals used by summarize(): COMPLETED_SECONDS[], WEDGED_SECONDS[]
# =============================================================================
declare -a COMPLETED_SECONDS=()
declare -a WEDGED_SECONDS=()

run_one() {
    local run_index="$1" run_log_path="$2"
    local sandbox_id="" exec_response="" http_status="" error_text=""
    local t_create_start t_create_end t_exec_start t_exec_end
    local stage_guest_boot="" stage_exec_start="" stage_curl_start="" stage_curl_end=""
    local stage_free_start="" stage_free_end="" stage_dmesg_start="" stage_dmesg_end=""
    local stage_sleep_start="" stage_sleep_end="" stage_exec_end=""
    local probe_egress="" probe_memory="" probe_oom_status="" probe_oom_lines=""
    local exit_code="" wedge="false" recovery_used="false"
    local t_run_started t_run_ended

    t_run_started="$(now_epoch)"

    log "run #$run_index: preflight (GET /v1/sandboxes)"
    check_precondition

    log "run #$run_index: POST /v1/sandboxes (snapshot_tag=$TAG)"
    t_create_start="$(now_epoch)"
    {
        read -r create_resp
        read -r create_status <&3
    } < <(curl_forkd POST "/v1/sandboxes" "{\"snapshot_tag\":\"$TAG\"}" 3>&1)
    t_create_end="$(now_epoch)"

    if [ "$create_status" != "200" ] && [ "$create_status" != "201" ]; then
        error_text="sandbox create failed HTTP $create_status: $create_resp"
        log "$error_text"
        t_run_ended="$(now_epoch)"
        printf '%s' "    \"index\": $run_index, \"error_text\": \"$(json_escape "$error_text")\"" > "$run_log_path"
        return 0
    fi

    sandbox_id="$(scrape_field "$create_resp" ".[0]." id sandbox_id sandboxId ID 2>/dev/null || true)"
    if [ -z "$sandbox_id" ]; then
        sandbox_id="$(printf '%s' "$create_resp" | grep -oE '"id"[ \t]*:[ \t]*"sb-[^"]+"' | head -1 \
            | sed -E 's/.*"sb-([^"]+)".*/\1/' | sed 's/^/sb-/')"
    fi
    if [ -z "$sandbox_id" ]; then
        error_text="could not scrape sandbox id from create response"
        t_run_ended="$(now_epoch)"
        log "$error_text"
        return 0
    fi

    CURRENT_SANDBOX_ID="$sandbox_id"
    OWNED_SANDBOX_IDS+=("$sandbox_id")
    log "run #$run_index: sandbox $sandbox_id (create_elapsed=$(awk -v a="$t_create_start" -v b="$t_create_end" 'BEGIN{printf "%.2f", (b-a)}'))"

    # Build exec body. We pass OUTER_HARNESS_SLEEP_SECS so the payload's
    # sleep overshoots our timeout_secs by ~20s, ensuring we always hit
    # the deadline rather than returning cleanly. (If we returned cleanly
    # we would NOT observe the wedge.)
    local exec_body
    exec_body=$(jq -n \
        --arg args_script "$EXEC_PAYLOAD" \
        --argjson timeout "$TIMEOUT_SECS" \
        --argjson sleep $(( TIMEOUT_SECS + 20 )) \
        '{args:["/bin/bash","-c",$args_script],timeout_secs:$timeout,env:{OUTER_HARNESS_SLEEP_SECS:($sleep|tostring)}}' \
        2>/dev/null) \
        || exec_body=$(printf '{"args":["/bin/bash","-c",%s],"timeout_secs":%s,"env":{"OUTER_HARNESS_SLEEP_SECS":"%s"}}' \
            "$(printf '%s' "$EXEC_PAYLOAD" | jq -Rs .)" \
            "$TIMEOUT_SECS" \
            "$(( TIMEOUT_SECS + 20 ))")

    log "run #$run_index: POST /v1/sandboxes/$sandbox_id/exec (timeout_secs=$TIMEOUT_SECS)"
    t_exec_start="$(now_epoch)"

    # We bound the outer curl at TIMEOUT_SECS+120 so it always returns;
    # the wedge we measure is the forkd blocking-read deadline, not a
    # client-side hang.
    {
        read -r exec_resp
        read -r exec_status <&3
    } < <(curl_forkd POST "/v1/sandboxes/$sandbox_id/exec" "$exec_body" 3>&1)
    t_exec_end="$(now_epoch)"

    exec_response="$exec_resp"
    http_status="$exec_status"

    # Classify: did the exec wedge past the deadline (control-side error),
    # or did it return a guest-side result (with non-zero or zero exit)?
    wedge="false"
    if [ "$http_status" = "500" ] && printf '%s' "$exec_response" | grep -qiE 'resource temporarily unavailable|os error 11|eagain|ewouldblock'; then
        wedge="true"
        error_text="$exec_response"
    fi
    if [ "$http_status" != "200" ] && [ -z "$error_text" ]; then
        error_text="exec HTTP $http_status: $exec_response"
    fi

    # If we did not get a 200, attempt one post-wedge recovery read.
    # After the forkd blocking-read times out at timeout_secs + 5, the
    # controller can accept new calls again; we try to cat the guest-side
    # log once. If that succeeds we recover stage timestamps and probe
    # results from inside the guest even though the original exec call
    # never returned them.
    if [ "$wedge" = "true" ] || [ "$http_status" != "200" ]; then
        log "run #$run_index: attempting post-wedge recovery read (cat /tmp/eagain-harness-guest.log)"
        local recovery_body
        {
            read -r recovery_body
            read -r recovery_status <&3
        } < <(curl_forkd POST "/v1/sandboxes/$sandbox_id/exec" \
            '{"args":["/bin/cat","/tmp/eagain-harness-guest.log"],"timeout_secs":30}' 3>&1) \
            || recovery_body=""
        if [ "${recovery_status:-0}" = "200" ] && [ -n "$recovery_body" ]; then
            recovery_used="true"
            log "run #$run_index: recovery read returned $(printf '%s' "$recovery_body" | wc -l) lines"
            # Merge: prefer the recovery log when both exist.
            exec_response="$recovery_body"$'\n'"$exec_response"
        else
            log "run #$run_index: recovery read failed (HTTP ${recovery_status:-?}); guest-side timestamps lost"
        fi
    fi

    # Parse STAGE lines from exec response.
    while IFS= read -r line; do
        case "$line" in
            STAGE\ guest_boot\ *)        stage_guest_boot="${line#STAGE guest_boot }" ;;
            STAGE\ exec_start\ *)        stage_exec_start="${line#STAGE exec_start }" ;;
            STAGE\ probe_egress_start\ *) stage_curl_start="${line#STAGE probe_egress_start }" ;;
            STAGE\ probe_egress_end\ *)   stage_curl_end="${line#STAGE probe_egress_end }" ;;
            STAGE\ probe_memory_start\ *) stage_free_start="${line#STAGE probe_memory_start }" ;;
            STAGE\ probe_memory_end\ *)   stage_free_end="${line#STAGE probe_memory_end }" ;;
            STAGE\ probe_oom_start\ *)  stage_dmesg_start="${line#STAGE probe_oom_start }" ;;
            STAGE\ probe_oom_end\ *)     stage_dmesg_end="${line#STAGE probe_oom_end }" ;;
            STAGE\ probe_idle_start\ *)  stage_sleep_start="${line#STAGE probe_idle_start }" ;;
            STAGE\ probe_idle_end\ *)    stage_sleep_end="${line#STAGE probe_idle_end }" ;;
            STAGE\ exec_end\ *)          stage_exec_end="${line#STAGE exec_end }" ;;
            PROBE_EGRESS\ *)            probe_egress="${line#PROBE_EGRESS }" ;;
            PROBE_MEMORY\ *)            probe_memory="${line#PROBE_MEMORY }" ;;
            PROBE_OOM_FOUND)             probe_oom_status="found" ;;
            PROBE_OOM_CLEAN)             probe_oom_status="clean" ;;
            PROBE_OOM_*)                 : ;;
        esac
    done <<< "$(printf '%s' "$exec_response")"

    # exit_code: scrape from exec JSON response. The recovery response is
    # plain cat output (no JSON envelope), so only try parsing the
    # pre-recovery portion.
    if printf '%s' "${exec_resp}" | grep -q '"exit_code"'; then
        exit_code="$(scrape_field "${exec_resp}" "." exit_code exitCode code 2>/dev/null || echo "")"
    fi

    # Persist the response (combined) to the per-run log for forensics.
    {
        printf '%s\n' "===== eagain-harness run $run_index ====="
        printf '%s\n' "started_at: $t_run_started   ended_at: $t_run_ended"
        printf '%s\n' "sandbox_id: $sandbox_id   http_status: $http_status   exit_code: $exit_code   wedge: $wedge"
        printf '%s\n' "----- raw exec response (combined) -----"
        printf '%s\n' "$exec_response"
        printf '%s\n' "----- end -----"
    } > "$run_log_path"

    t_run_ended="$(now_epoch)"
    local elapsed
    elapsed="$(awk -v a="$t_exec_start" -v b="$t_exec_end" 'BEGIN{printf "%.3f", (b-a)}')"

    # Summary stats — bucket as completed or wedged.
    if [ "$wedge" = "true" ]; then
        WEDGED_SECONDS+=("$elapsed")
    else
        COMPLETED_SECONDS+=("$elapsed")
    fi

    # OOM detection
    local oom_found="false" oom_excerpt=""
    if [ "$probe_oom_status" = "found" ]; then
        oom_found="true"
        oom_excerpt="${probe_oom_lines:-$(printf '%s' "$exec_response" | grep -iE 'oom|out of memory|killed process' | head -10 | tr '\n' '|')}"
    fi

    # Egress parse
    local egress_ok="false" egress_code="" egress_latency=""
    case "$probe_egress" in
        code=200\ latency_ms=*)  egress_ok="true"; egress_code="200" ;;
        code=*latency_ms=*)      egress_code="${probe_egress%% *}" ; egress_code="${egress_code#code=}" ;;
        *)                       egress_code="unparsed" ;;
    esac
    egress_latency="$(printf '%s' "$probe_egress" | sed -nE 's/.*latency_ms=([^ ]+).*/\1/p')"

    # Memory parse
    local mem_total="" mem_avail=""
    case "$probe_memory" in
        total_mb=*avail_mb=*)
            mem_total="$(printf '%s' "$probe_memory" | sed -nE 's/.*total_mb=([^ ]+).*/\1/p')"
            mem_avail="$(printf '%s' "$probe_memory" | sed -nE 's/.*avail_mb=([^ ]+).*/\1/p')"
            ;;
    esac

    # exit_code 137 explicit flag for the brief
    local exit_137="false"
    if [ "$exit_code" = "137" ]; then exit_137="true"; fi

    # Emit one JSON object literal for this run.
    local json_obj=""
    json_obj+="    {\n"
    json_obj+="      \"index\": $run_index,\n"
    json_obj+="      \"sandbox_id\": \"$(json_escape "$sandbox_id")\",\n"
    json_obj+="      \"started_at\": \"$(date -u -d "@${t_run_started%.*}" +%Y-%m-%dT%H:%M:%S.%NZ 2>/dev/null || date -u +%Y-%m-%dT%H:%M:%SZ)\",\n"
    json_obj+="      \"ended_at\": \"$(date -u -d "@${t_run_ended%.*}" +%Y-%m-%dT%H:%M:%S.%NZ 2>/dev/null || date -u +%Y-%m-%dT%H:%M:%SZ)\",\n"
    json_obj+="      \"elapsed_seconds\": $elapsed,\n"
    json_obj+="      \"http_status\": $http_status,\n"
    json_obj+="      \"exit_code\": \"${exit_code:-}\",\n"
    json_obj+="      \"exit_code_137\": $exit_137,\n"
    json_obj+="      \"wedged\": $wedge,\n"
    json_obj+="      \"recovery_used\": $recovery_used,\n"
    json_obj+="      \"oom_markers_found\": $oom_found,\n"
    json_obj+="      \"oom_markers_excerpt\": \"$(json_escape "$oom_excerpt")\",\n"
    json_obj+="      \"egress_ok\": $egress_ok,\n"
    json_obj+="      \"egress_http_code\": \"$egress_code\",\n"
    json_obj+="      \"egress_latency_ms\": \"$egress_latency\",\n"
    json_obj+="      \"memory_total_mb\": \"$mem_total\",\n"
    json_obj+="      \"memory_avail_mb\": \"$mem_avail\",\n"
    json_obj+="      \"stages\": {\n"
    json_obj+="        \"create_request_sent\": \"$t_create_start\",\n"
    json_obj+="        \"create_response_received\": \"$t_create_end\",\n"
    json_obj+="        \"exec_request_sent\": \"$t_exec_start\",\n"
    json_obj+="        \"exec_response_received_or_timeout\": \"$t_exec_end\",\n"
    json_obj+="        \"guest_boot\": \"$stage_guest_boot\",\n"
    json_obj+="        \"exec_start\": \"$stage_exec_start\",\n"
    json_obj+="        \"probe_egress_start\": \"$stage_curl_start\",\n"
    json_obj+="        \"probe_egress_end\": \"$stage_curl_end\",\n"
    json_obj+="        \"probe_memory_start\": \"$stage_free_start\",\n"
    json_obj+="        \"probe_memory_end\": \"$stage_free_end\",\n"
    json_obj+="        \"probe_oom_start\": \"$stage_dmesg_start\",\n"
    json_obj+="        \"probe_oom_end\": \"$stage_dmesg_end\",\n"
    json_obj+="        \"probe_idle_start\": \"$stage_sleep_start\",\n"
    json_obj+="        \"probe_idle_end\": \"$stage_sleep_end\",\n"
    json_obj+="        \"exec_end\": \"$stage_exec_end\"\n"
    json_obj+="      },\n"
    json_obj+="      \"error_text\": \"$(json_escape "${error_text:-}")\",\n"
    json_obj+="      \"raw_log_path\": \"$(json_escape "$run_log_path")\"\n"
    json_obj+="    }"

    printf '%s\n' "$json_obj" > "$run_log_path.jsonfrag"
    write_run_to_results "$json_obj,"

    # Teardown before the next run.
    teardown_current_sandbox
}

summarize() {
    local completed_n="${#COMPLETED_SECONDS[@]}"
    local wedged_n="${#WEDGED_SECONDS[@]}"
    local cmin="" cmax="" cmean=""
    local wmin="" wmax="" wmean=""
    local wedges_near_1805="false" wedges_near_505="false"

    if [ "$completed_n" -gt 0 ]; then
        local sum=0
        for v in "${COMPLETED_SECONDS[@]}"; do
            [ -z "$cmin" ] || awk -v a="$cmin" -v b="$v" 'BEGIN{exit !(b<a)}' && cmin="$v" || true
            [ -z "$cmax" ] || awk -v a="$cmax" -v b="$v" 'BEGIN{exit !(b>a)}' && cmax="$v" || true
            sum="$(awk -v a="$sum" -v b="$v" 'BEGIN{printf "%.3f", (a+b)}')"
        done
        cmean="$(awk -v s="$sum" -v n="$completed_n" 'BEGIN{printf "%.3f", (s/n)}')"
    fi
    if [ "$wedged_n" -gt 0 ]; then
        local sum=0
        for v in "${WEDGED_SECONDS[@]}"; do
            [ -z "$wmin" ] || awk -v a="$wmin" -v b="$v" 'BEGIN{exit !(b<a)}' && wmin="$v" || true
            [ -z "$wmax" ] || awk -v a="$wmax" -v b="$v" 'BEGIN{exit !(b>a)}' && wmax="$v" || true
            sum="$(awk -v a="$sum" -v b="$v" 'BEGIN{printf "%.3f", (a+b)}')"
        done
        wmean="$(awk -v s="$sum" -v n="$wedged_n" 'BEGIN{printf "%.3f", (s/n)}')"
        # Cluster heuristic: any wedge within ±30s of 1805 marks wedges_near_1805;
        # any within ±30s of 505 marks wedges_near_505.
        for v in "${WEDGED_SECONDS[@]}"; do
            if awk -v a="$v" -v b="$(( TIMEOUT_SECS + 5 ))" 'BEGIN{exit !(a>=b-30 && a<=b+30)}'; then
                wedges_near_1805="true"
            fi
            if awk -v a="$v" 'BEGIN{exit !(a>=475 && a<=535)}'; then
                wedges_near_505="true"
            fi
        done
    fi

    SUMMARY_JSON=$(cat <<EOF
{
  "status": "completed",
  "completed_runs": $completed_n,
  "wedged_runs": $wedged_n,
  "elapsed_completed": {"min": "${cmin:-}", "max": "${cmax:-}", "mean": "${cmean:-}"},
  "elapsed_wedged":    {"min": "${wmin:-}", "max": "${wmax:-}", "mean": "${wmean:-}"},
  "wedges_near_1805": $wedges_near_1805,
  "wedges_near_505": $wedges_near_505,
  "discriminator": $( [ "$wedges_near_1805" = "true" ] && echo '"deadline-mechanism-confirmed"' || [ "$wedges_near_505" = "true" ] && echo '"diagnosis-changes-earlier-pin"' || echo '"inconclusive"' )
}
EOF
)
}

# =============================================================================
# main
# =============================================================================
main() {
    if [ "$DRY_RUN" = "1" ]; then
        dry_run_plan
    fi

    mkdir -p "$RUN_LOG_DIR" 2>/dev/null || die "cannot create $RUN_LOG_DIR"

    init_results_file
    RESULTS_OPENED=1

    log "=== eagain-harness starting ==="
    log "tag=$TAG timeout_secs=$TIMEOUT_SECS runs=$RUNS result_json=$RESULT_JSON"

    # Clean up any sandbox we might own from a prior crashed run.
    log "pre-main preflight: drain any foreign active sandboxes"
    check_precondition

    local i
    for i in $(seq 1 "$RUNS"); do
        local run_log_path
        run_log_path="$RUN_LOG_DIR/run-$i.log"
        : > "$run_log_path"
        run_one "$i" "$run_log_path"
    done

    # Final teardown in case anything was skipped.
    teardown_current_sandbox

    summarize
    close_results_file
    RESULTS_OPENED=0

    log "=== eagain-harness complete ==="
    log "results: $RESULT_JSON"

    # Print a one-line discriminator banner for the operator.
    log "$(awk -F'"' '/discriminator/ {print $4}' "$RESULT_JSON" | head -1 | xargs -I{} echo "DISCRIMINATOR: {}")"
}

main "$@"
