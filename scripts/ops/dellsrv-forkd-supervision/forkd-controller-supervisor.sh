#!/usr/bin/env bash
# =============================================================================
# forkd-controller-supervisor.sh — systemd ExecStart target for the
#                                   forkd-controller.service unit on dellsrv.
#
# WHAT THIS DOES
#   Runs /usr/local/sbin/forkd-ec-boot-dellsrv.sh, then blocks until the
#   gate is reachable on http://127.0.0.1:8891/v1/health. Exits 0 once
#   the gate answers 200, or non-zero if it never comes up within the
#   timeout. systemd (Restart=on-failure) re-invokes this on non-zero
#   exit, so a transient docker hiccup will self-heal.
#
# WHY THIS LAYER EXISTS
#   The systemd unit must NOT ExecStart the raw forkd-controller binary:
#   the live controller runs inside the 'forkd' docker container, and
#   `forkd-ec-boot-dellsrv.sh` is the idempotent wrapper that brings
#   that container up via `docker exec`. systemd-supervising the boot
#   script is the only correct shape — see the OWNERSHIP NOTE in the
#   .service file.
#
# USAGE
#   Installed by the .service file's deploy block. Operator-invoked:
#       sudo /usr/local/sbin/forkd-controller-supervisor.sh
#   Dry-run prints what it would do and skips docker + health probe:
#       DRY_RUN=1 sudo /usr/local/sbin/forkd-controller-supervisor.sh
#
# ENV (with defaults)
#   FORKD_BOOT_SCRIPT=/usr/local/sbin/forkd-ec-boot-dellsrv.sh
#   FORKD_HEALTH_URL=http://127.0.0.1:8891/v1/health
#   FORKD_HEALTH_TIMEOUT_S=60     # total wall-clock budget
#   FORKD_HEALTH_INTERVAL_S=2     # probe period
#   DRY_RUN=0                     # 1 = print only
# =============================================================================

set -euo pipefail

FORKD_BOOT_SCRIPT="${FORKD_BOOT_SCRIPT:-/usr/local/sbin/forkd-ec-boot-dellsrv.sh}"
FORKD_HEALTH_URL="${FORKD_HEALTH_URL:-http://127.0.0.1:8891/v1/health}"
FORKD_HEALTH_TIMEOUT_S="${FORKD_HEALTH_TIMEOUT_S:-60}"
FORKD_HEALTH_INTERVAL_S="${FORKD_HEALTH_INTERVAL_S:-2}"
DRY_RUN="${DRY_RUN:-0}"

log() { printf '[forkd-supervisor] %s\n' "$*" >&2; }

# ---------- preflight: required tools ----------
for bin in docker curl; do
    if ! command -v "$bin" >/dev/null 2>&1; then
        log "FATAL: required binary '$bin' not found in PATH"
        exit 1
    fi
done

if [ ! -x "$FORKD_BOOT_SCRIPT" ]; then
    log "FATAL: boot script not found or not executable: $FORKD_BOOT_SCRIPT"
    log "       (see deploy block in forkd-controller.service)"
    exit 1
fi

# ---------- run the boot script ----------
log "running boot script: $FORKD_BOOT_SCRIPT"
if [ "$DRY_RUN" = "1" ]; then
    log "DRY-RUN: would exec $FORKD_BOOT_SCRIPT (script is idempotent; safe to re-run)"
else
    "$FORKD_BOOT_SCRIPT"
fi

# ---------- wait for the gate to answer health ----------
if [ "$DRY_RUN" = "1" ]; then
    log "DRY-RUN: would poll $FORKD_HEALTH_URL until 200 (timeout=${FORKD_HEALTH_TIMEOUT_S}s)"
    log "DRY-RUN: skipping health probe"
    exit 0
fi

deadline=$(( $(date +%s) + FORKD_HEALTH_TIMEOUT_S ))
attempt=0
while :; do
    attempt=$(( attempt + 1 ))
    if curl --silent --show-error --fail --max-time 2 \
            --output /dev/null "$FORKD_HEALTH_URL"; then
        log "gate healthy after ${attempt} probe(s): $FORKD_HEALTH_URL"
        exit 0
    fi
    if [ "$(date +%s)" -ge "$deadline" ]; then
        log "FATAL: gate did not answer $FORKD_HEALTH_URL within ${FORKD_HEALTH_TIMEOUT_S}s"
        log "       (the boot script exited 0 but the controller is not serving; check 'docker logs forkd')"
        exit 1
    fi
    sleep "$FORKD_HEALTH_INTERVAL_S"
done
