#!/usr/bin/env bash
# =============================================================================
# re-register-golden-tags.sh — re-register TWO snapshot tags off the SAME
#                              existing 20GB golden rootfs.
#
# WHAT THIS DOES
#   Registers BOTH tags off the same ext4 rootfs. This is a RE-REGISTER,
#   not a re-bake — the rootfs is unchanged on disk. We only differ in
#   the --mem-size-mib knob the controller persists into the tag.
#
# TAGS
#   zen-gate-base   at --mem-size-mib 1024    (1 GiB guest)
#   zen-gate-big    at --mem-size-mib 4096    (4 GiB guest)
#
# WHY THE CLI IS LOAD-BEARING (read carefully)
#   The --mem-size-mib flag is ONLY honored by the `forkd snapshot`
#   CLI. A raw POST /v1/snapshots SILENTLY IGNORES the memory field
#   and persists a 512 MiB guest. The current broken golden is
#   exactly this: its memory.bin is 536870912 bytes = 512 MiB, giving
#   a ~484 MiB usable guest that OOM-kills `npm ci`.
#
#   We therefore go through the CLI, and we VERIFY the persisted
#   memory.bin against the requested MIB before declaring success. A
#   successful HTTP 200 is NOT sufficient — the bug is silent.
#
# VERIFICATION
#   For each tag, we stat the controller's memory.bin on disk and
#   assert it equals exactly requested_mib * 1048576 bytes. Mismatch
#   exits non-zero. We do NOT trust the tag name; we trust the bytes.
#
# USAGE (operator, on dellsrv)
#   scripts/ops/dellsrv-forkd-supervision/re-register-golden-tags.sh
#       --rootfs /var/lib/forkd/golden/rootfs-20g.ext4
#       --kernel /var/lib/forkd/golden/vmlinux
#       --tap    zen0
#       --boot-wait-secs 30
#       --old-tag zen-gate     # tag to deregister after success
#
#   --dry-run prints every command and exits 0 before touching anything.
#
# ENV (with defaults; all overridable on the CLI)
#   FORKD_BIN=forkd                # CLI binary on the host
#   FORKD_CURL=http://127.0.0.1:8891   # health endpoint
# =============================================================================

set -euo pipefail

# ---------- defaults ----------
FORKD_BIN="${FORKD_BIN:-forkd}"
FORKD_HEALTH_URL="${FORKD_HEALTH_URL:-http://127.0.0.1:8891/v1/health}"
DRY_RUN=0

ROOTFS=""
KERNEL=""
TAP=""
BOOT_WAIT_SECS=""
OLD_TAG=""
SNAPSHOT_ROOT="/root/.local/share/forkd/snapshots"  # matches live invocation

# ---------- tag specs ----------
# (tag, mib)
TAG_SPECS=(
    "zen-gate-base:1024"
    "zen-gate-big:4096"
)

# ---------- args ----------
usage() {
    sed -n '2,52p' "$0"
    exit 64
}

while [ $# -gt 0 ]; do
    case "$1" in
        --rootfs)         ROOTFS="$2"; shift 2 ;;
        --kernel)         KERNEL="$2"; shift 2 ;;
        --tap)            TAP="$2"; shift 2 ;;
        --boot-wait-secs) BOOT_WAIT_SECS="$2"; shift 2 ;;
        --old-tag)        OLD_TAG="$2"; shift 2 ;;
        --snapshot-root)  SNAPSHOT_ROOT="$2"; shift 2 ;;
        --dry-run)        DRY_RUN=1; shift ;;
        -h|--help)        usage ;;
        *) echo "unknown arg: $1" >&2; usage ;;
    esac
done

# ---------- helpers ----------
log() { printf '[re-register] %s\n' "$*" >&2; }
die() { log "FATAL: $*"; exit 1; }

# Run a command, or print it under --dry-run. Always shell-quoted.
run() {
    if [ "$DRY_RUN" = "1" ]; then
        # %q quotes for shell re-execution; we just print for the operator.
        printf 'DRY-RUN:'
        printf ' %q' "$@"
        printf '\n'
    else
        "$@"
    fi
}

mib_to_bytes() {
    # $1 = MIB (int). Echo exact byte count.
    printf '%d' "$(( $1 * 1048576 ))"
}

# ---------- preflight ----------
[ -n "$ROOTFS" ]         || die "--rootfs is required"
[ -n "$KERNEL" ]         || die "--kernel is required"
[ -n "$TAP" ]            || die "--tap is required"
[ -n "$BOOT_WAIT_SECS" ] || die "--boot-wait-secs is required"

[ -r "$ROOTFS" ] || die "rootfs not readable: $ROOTFS"
[ -r "$KERNEL" ] || die "kernel not readable: $KERNEL"
command -v "$FORKD_BIN" >/dev/null 2>&1 || die "forkd CLI not found: $FORKD_BIN"

if [ "$DRY_RUN" != "1" ]; then
    log "verifying gate is up at $FORKD_HEALTH_URL"
    curl --silent --show-error --fail --max-time 5 \
        --output /dev/null "$FORKD_HEALTH_URL" \
        || die "gate is not answering $FORKD_HEALTH_URL (start forkd first)"
fi

# ---------- register each tag ----------
for spec in "${TAG_SPECS[@]}"; do
    tag="${spec%%:*}"
    mib="${spec##*:}"
    expected_bytes="$(mib_to_bytes "$mib")"
    mem_bin="${SNAPSHOT_ROOT}/${tag}/memory.bin"

    log "registering tag=$tag mem=${mib}MiB (expected ${expected_bytes} bytes)"
    run "$FORKD_BIN" snapshot \
        --tag "$tag" \
        --kernel "$KERNEL" \
        --rootfs "$ROOTFS" \
        --tap "$TAP" \
        --boot-wait-secs "$BOOT_WAIT_SECS" \
        --mem-size-mib "$mib"

    if [ "$DRY_RUN" = "1" ]; then
        log "DRY-RUN: would stat $mem_bin and assert size == ${expected_bytes}"
        continue
    fi

    # ---------- POST-REGISTRATION VERIFICATION ----------
    # The CLI bug-of-record is a 512 MiB silent default. We don't trust
    # the tag name; we read the bytes. A wrong-sized memory.bin means
    # the CLI was bypassed (raw REST POST) or the controller is broken;
    # either way, FAIL LOUD so the operator notices before consumers
    # start OOM'ing.
    if [ ! -f "$mem_bin" ]; then
        die "tag $tag: expected $mem_bin to exist after register, but it does not"
    fi
    actual_bytes="$(stat -c '%s' "$mem_bin" 2>/dev/null || stat -f '%z' "$mem_bin")"
    if [ "$actual_bytes" != "$expected_bytes" ]; then
        die "tag $tag: memory.bin is ${actual_bytes} bytes, expected ${expected_bytes} bytes " \
            "(REQUESTED ${mib} MiB). The CLI flag was likely bypassed or the controller " \
            "silently defaulted to 512 MiB. Refusing to leave a broken tag in place."
    fi
    log "  ok: $mem_bin is ${actual_bytes} bytes (== ${mib} MiB)"
done

# ---------- deregister old tag (rollback) ----------
if [ -n "$OLD_TAG" ]; then
    log "deregistering old tag: $OLD_TAG"
    # Only proceed if every new tag verified. By this point we're past
    # the verification block; if we got here, the new tags are sound.
    run "$FORKD_BIN" snapshot --deregister --tag "$OLD_TAG"
    if [ "$DRY_RUN" != "1" ]; then
        # Verify it's actually gone.
        if [ -d "${SNAPSHOT_ROOT}/${OLD_TAG}" ]; then
            die "old tag $OLD_TAG still present at ${SNAPSHOT_ROOT}/${OLD_TAG} " \
                "after deregister (controller returned success but data lingers)"
        fi
        log "  ok: $OLD_TAG removed"
    fi
fi

log "done"
