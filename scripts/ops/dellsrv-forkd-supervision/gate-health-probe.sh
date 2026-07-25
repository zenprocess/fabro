#!/usr/bin/env bash
# =============================================================================
# gate-health-probe.sh — exercises the real forkd sandbox path end-to-end.
#
# WHY THIS EXISTS
#   On 2026-07-22 the fabro gate went dark for three days because the
#   per-child network namespaces (`/var/run/netns/forkd-child-{1,2,3}`)
#   died WITHOUT the forkd container restarting. The Firecracker console
#   emitted:
#       setting the network namespace "forkd-child-1" failed: Invalid argument
#   and the sandbox-create path returned:
#       restore_many: socket /tmp/forkd-daemon-<tag>-o0/child-1.sock never appeared within 5s
#   forkd-shim.py injects per_child_netns=true on every sandbox-create, so
#   every gate exec failed at infra level. The gate correctly suppressed
#   the verdict rather than posting a wrong one — so the failure mode is
#   silence, not noise. That is the worst possible signal: it looks like
#   the fleet is healthy because nothing is wrong enough to be scored.
#
#   forkd-ec-boot.sh provisions the netns only at container start. Nothing
#   probes them. This script IS that probe.
#
# WHAT IT DOES
#   1. POST /v1/sandboxes (create) with tag=zen-gate-base and
#      per_child_netns=true. The `true` is load-bearing: a probe using
#      per_child_netns=false would have passed throughout the entire
#      outage and is worthless. We test the mode that actually breaks.
#   2. Inspect the response. If it carries the netns-failure signature
#      ('Invalid argument' / 'socket never appeared'), ALERT (or, with
#      --heal, attempt the documented teardown+setup repair).
#   3. POST /v1/sandboxes/{id}/exec with body {"args":["/bin/true"]} and
#      assert exit_code == 0.
#   4. DELETE /v1/sandboxes/{id} — ALWAYS, even on prior failure.
#   5. Exit 0 on full success, non-zero with a precise, greppable reason
#      on any failure.
#
# TOKEN HANDLING
#   The token at /etc/forkd-token lives INSIDE the forkd docker container.
#   This probe NEVER touches the host argv with a token value. Every API
#   call is wrapped in `docker exec -i forkd sh` with a heredoc whose
#   first action is `TOKEN=$(cat /etc/forkd-token)` — the token value is
#   only ever a variable inside the container's shell. The host process
#   table only ever sees `docker exec -i forkd sh` as the argv; the heredoc
#   body travels over the docker exec protocol's stdin pipe.
#
# USAGE (operator, on dellsrv)
#   sudo scripts/ops/dellsrv-forkd-supervision/gate-health-probe.sh
#   sudo scripts/ops/dellsrv-forkd-supervision/gate-health-probe.sh --heal
#   sudo scripts/ops/dellsrv-forkd-supervision/gate-health-probe.sh --dry-run
#
# ENV (with defaults)
#   PROBE_TAG=zen-gate-base            # snapshot tag to fork from
#   PROBE_CMD=/bin/true                # command exec'd inside the guest
#   FORKD_CONTAINER=forkd              # docker container name
#   FORKD_TOKEN_FILE=/etc/forkd-token  # token file, inside the container
#   FORKD_API_BASE=http://127.0.0.1:8891
#   DRY_RUN=0                          # 1 = print the plan, touch nothing
# =============================================================================

set -euo pipefail

# ---------- defaults ----------
PROBE_TAG="${PROBE_TAG:-zen-gate-base}"
PROBE_CMD="${PROBE_CMD:-/bin/true}"
FORKD_CONTAINER="${FORKD_CONTAINER:-forkd}"
FORKD_TOKEN_FILE="${FORKD_TOKEN_FILE:-/etc/forkd-token}"
FORKD_API_BASE="${FORKD_API_BASE:-http://127.0.0.1:8891}"
DRY_RUN="${DRY_RUN:-0}"
HEAL_ON_NETNS_FAILURE=0

# Response field names. REST APIs vary; these are the conventional shapes.
# If the controller uses different names (e.g. sandboxId, exitCode), the
# parse step will fail LOUD with the raw response — that is the intended
# signal: don't silently mis-detect.
SANDBOX_ID_FIELD=".id"
EXIT_CODE_FIELD=".exit_code"

# ---------- args ----------
usage() {
    sed -n '2,72p' "$0"
    exit 64
}

while [ $# -gt 0 ]; do
    case "$1" in
        --heal)   HEAL_ON_NETNS_FAILURE=1; shift ;;
        --dry-run) DRY_RUN=1; shift ;;
        -h|--help) usage ;;
        *) echo "unknown arg: $1" >&2; usage ;;
    esac
done

# ---------- helpers ----------
log() { printf '[gate-probe] %s\n' "$*" >&2; }
die() { log "FATAL: $*"; exit 1; }

# alert emits a structured, greppable journal line. The journalctl
# identifier `gate-probe` is used so operators can grep with:
#   journalctl -t gate-probe
# OR the FORKD-GATE-ALERT / FORKD-GATE-HEAL prefixes for the failure
# path specifically.
alert() {
    # Single-line key=value blob; greppable in journald.
    local reason="$1" excerpt="$2" action="$3"
    printf 'FORKD-GATE-ALERT reason=%q excerpt=%q action=%q timestamp=%s\n' \
        "$reason" "$excerpt" "$action" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" >&2
}

# heal_record is logged BEFORE the heal runs so even a crash mid-heal
# leaves a forensic trail. The operator (and the post-mortem) can grep
# `FORKD-GATE-HEAL` and see that the probe attempted repair.
heal_record() {
    local reason="$1"
    printf 'FORKD-GATE-HEAL reason=%q timestamp=%s\n' \
        "$reason" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" >&2
}

# in_container_curl runs a curl request via `docker exec -i forkd sh`
# with a heredoc body. The heredoc is the only mechanism that lets us
# keep the token in an in-container variable without it appearing in
# the host's argv.
#
# Args:
#   $1 = HTTP method
#   $2 = path (e.g. /v1/sandboxes)
#   $3 = JSON body (or "" for GET/DELETE)
# Stdout: response body
in_container_curl() {
    local method="$1" path="$2" body="$3"
    local body_arg=""
    if [ -n "$body" ]; then
        body_arg="--data ${body@Q}"
    fi

    # Quote path with shell-escape so it survives the heredoc literal.
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

# ---------- preflight ----------
command -v docker >/dev/null 2>&1 || die "docker binary not found on host PATH"
command -v jq     >/dev/null 2>&1 || die "jq is required to parse API responses"

if [ "$DRY_RUN" != "1" ]; then
    if ! docker inspect "$FORKD_CONTAINER" >/dev/null 2>&1; then
        die "docker container '$FORKD_CONTAINER' is not present (probe cannot run)"
    fi
fi

# ---------- teardown (always) ----------
SANDBOX_ID=""
# Invoked indirectly via the EXIT trap below; SC2329 is a false positive here.
# shellcheck disable=SC2329
teardown_sandbox() {
    if [ -z "$SANDBOX_ID" ]; then
        return 0
    fi
    log "teardown: DELETE /v1/sandboxes/$SANDBOX_ID"
    if [ "$DRY_RUN" = "1" ]; then
        log "DRY-RUN: would DELETE $FORKD_API_BASE/v1/sandboxes/$SANDBOX_ID"
        return 0
    fi
    # Best-effort: don't fail the probe if teardown errors out — the
    # gate has its own reaping. But DO log the failure.
    if ! in_container_curl DELETE "/v1/sandboxes/$SANDBOX_ID" ""; then
        log "WARN: teardown DELETE failed for $SANDBOX_ID (gate will reap)"
    fi
}
trap 'teardown_sandbox' EXIT

# ---------- step 1: create sandbox with per_child_netns=true ----------
log "create: POST /v1/sandboxes tag=$PROBE_TAG per_child_netns=true"
create_body="$(printf '{"tag":"%s","per_child_netns":true}' "$PROBE_TAG")"

if [ "$DRY_RUN" = "1" ]; then
    log "DRY-RUN: would POST $FORKD_API_BASE/v1/sandboxes body=$create_body"
    log "DRY-RUN: would parse sandbox id from ${SANDBOX_ID_FIELD}, exec /bin/true, then DELETE"
    log "DRY-RUN: skipping all API calls"
    exit 0
fi

create_response="$(in_container_curl POST "/v1/sandboxes" "$create_body")"
log "create response (raw): $create_response"

# ---------- step 1a: netns-failure signature detection ----------
# This is the load-bearing check. The probe MUST fail loud on these
# strings even if HTTP returned 200 — that is exactly the bug-of-record:
# the API can return success while the sandbox never actually came up.
if printf '%s' "$create_response" | grep -qE 'Invalid argument|socket never appeared'; then
    excerpt="$create_response"
    # Trim to first 200 chars so the journal line stays readable.
    excerpt="${excerpt:0:200}"

    if [ "$HEAL_ON_NETNS_FAILURE" = "1" ]; then
        heal_record "netns_failure_signature_detected"
        log "heal: running netns-teardown + netns-setup"
        # Exact repair commands verified on dellsrv 2026-07-22.
        docker exec "$FORKD_CONTAINER" sh <<'INNER_EOF'
USER=root bash /opt/forkd-scripts/netns-teardown.sh --yes
USER=root bash /opt/forkd-scripts/netns-setup.sh 3
INNER_EOF
        # Surface a final HEAL log line so the operator can count heals.
        printf 'FORKD-GATE-HEAL completed teardown_and_setup timestamp=%s\n' \
            "$(date -u +%Y-%m-%dT%H:%M:%SZ)" >&2
        alert "netns_failure_signature_detected" "$excerpt" "heal_attempted_then_exit"
    else
        alert "netns_failure_signature_detected" "$excerpt" "alert_only_no_heal"
    fi
    die "netns failure signature in create response: $excerpt"
fi

# ---------- step 1b: parse sandbox id ----------
SANDBOX_ID="$(printf '%s' "$create_response" | jq -r "${SANDBOX_ID_FIELD} // empty" 2>/dev/null || true)"
if [ -z "$SANDBOX_ID" ] || [ "$SANDBOX_ID" = "null" ]; then
    # Fallback: try a few common alternative field names.
    for alt in .sandbox_id .sandboxId .ID; do
        if [ "$alt" != "$SANDBOX_ID_FIELD" ]; then
            candidate="$(printf '%s' "$create_response" | jq -r "${alt} // empty" 2>/dev/null || true)"
            if [ -n "$candidate" ] && [ "$candidate" != "null" ]; then
                SANDBOX_ID="$candidate"
                log "WARN: parsed sandbox id from fallback field $alt (update SANDBOX_ID_FIELD if this is the canonical name)"
                break
            fi
        fi
    done
fi
[ -n "$SANDBOX_ID" ] || die "could not parse sandbox id from create response: $create_response"
log "sandbox id: $SANDBOX_ID"

# ---------- step 2: exec /bin/true ----------
log "exec: POST /v1/sandboxes/$SANDBOX_ID/exec args=[$PROBE_CMD]"
exec_body="$(printf '{"args":["%s"]}' "$PROBE_CMD")"

exec_response="$(in_container_curl POST "/v1/sandboxes/$SANDBOX_ID/exec" "$exec_body")"
log "exec response (raw): $exec_response"

exit_code="$(printf '%s' "$exec_response" | jq -r "${EXIT_CODE_FIELD} // empty" 2>/dev/null || true)"
if [ -z "$exit_code" ] || [ "$exit_code" = "null" ]; then
    for alt in .exitCode .code .status; do
        if [ "$alt" != "$EXIT_CODE_FIELD" ]; then
            candidate="$(printf '%s' "$exec_response" | jq -r "${alt} // empty" 2>/dev/null || true)"
            if [ -n "$candidate" ] && [ "$candidate" != "null" ]; then
                exit_code="$candidate"
                log "WARN: parsed exit_code from fallback field $alt (update EXIT_CODE_FIELD if this is the canonical name)"
                break
            fi
        fi
    done
fi

if [ "$exit_code" != "0" ]; then
    alert "exec_nonzero_exit" "$exec_response" "alert_only_no_heal"
    die "exec returned exit_code=$exit_code (expected 0) for command=$PROBE_CMD"
fi

log "exec OK: exit_code=0"
log "probe SUCCESS"
exit 0
