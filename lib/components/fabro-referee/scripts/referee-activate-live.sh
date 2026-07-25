#!/usr/bin/env bash
set -euo pipefail

# T3 boundary: this script scores/reads only; it never activates, reconfigures,
# restarts, or re-baselines the controller or golden rootfs. The operator
# activates the plane.

usage() {
  cat >&2 <<'USAGE'
Usage: referee-activate-live.sh --run-id ID [--live] [--dry-run] [options]
  --forkd-endpoint URL   Forkd endpoint (default: http://dellsrv:8891)
  --live                  Run the operator-confirmed live forkd canary
  --dry-run               Print all phases without running any command
USAGE
  exit 2
}

RUN_ID="${RUN_ID:-${FABRO_REFEREE_RUN_ID:-}}"
FORKD_ENDPOINT="${FORKD_ENDPOINT:-http://dellsrv:8891}"
LIVE=0
DRY_RUN=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --run-id) RUN_ID="$2"; shift 2 ;;
    --forkd-endpoint) FORKD_ENDPOINT="$2"; shift 2 ;;
    --live) LIVE=1; shift ;;
    --dry-run) DRY_RUN=1; shift ;;
    -h|--help) usage ;;
    *) printf 'referee-activate-live.sh: unknown argument: %s\n' "$1" >&2; usage ;;
  esac
done

[[ -n "$RUN_ID" ]] || { printf '%s\n' 'referee-activate-live.sh: --run-id is required (caller-supplied and stable)' >&2; exit 2; }

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "$SCRIPT_DIR/../../../.." && pwd)"
if [[ -n "${FABRO_REFEREE:-}" ]]; then
  REF="$FABRO_REFEREE"
elif [[ -x "$REPO_ROOT/target/release/fabro-referee" ]]; then
  REF="$REPO_ROOT/target/release/fabro-referee"
elif [[ -x "$REPO_ROOT/target/debug/fabro-referee" ]]; then
  REF="$REPO_ROOT/target/debug/fabro-referee"
elif command -v fabro-referee >/dev/null 2>&1; then
  REF="$(command -v fabro-referee)"
else
  printf '%s\n' 'referee-activate-live.sh: fabro-referee not found; set FABRO_REFEREE or build target/{release,debug}/fabro-referee' >&2
  exit 127
fi
[[ -x "$REF" ]] || { printf 'referee-activate-live.sh: referee binary is not executable: %s\n' "$REF" >&2; exit 127; }

PHASE1_CMD="\"$REF\" --backend forkd --forkd-endpoint \"$FORKD_ENDPOINT\" doctor"
PHASE2_CMD="\"$REF\" --backend hermetic canary --run-id \"$RUN_ID\" --green"
PHASE3_CMD="\"$REF\" --backend forkd --forkd-endpoint \"$FORKD_ENDPOINT\" canary --run-id \"$RUN_ID\""

if [[ "$DRY_RUN" -eq 1 ]]; then
  printf 'Phase 1: %s\n' "$PHASE1_CMD"
  printf 'Phase 2: %s\n' "$PHASE2_CMD"
  printf 'Phase 3: %s\n' "$PHASE3_CMD"
  exit 0
fi

printf 'Phase 1: doctor\n'
if ! PHASE1_OUTPUT="$("$REF" --backend forkd --forkd-endpoint "$FORKD_ENDPOINT" doctor 2>&1)"; then
  printf '%s\n' "$PHASE1_OUTPUT" >&2
  printf '%s\n' 'referee-activate-live.sh: doctor failed; stop at the egress boundary and hand back to the operator' >&2
  exit 1
fi
printf '%s\n' "$PHASE1_OUTPUT"
if [[ "$PHASE1_OUTPUT" == *'UNREACHABLE'* ]]; then
  printf '%s\n' 'referee-activate-live.sh: forkd endpoint is unreachable; stop at the egress boundary and run no workaround or retry' >&2
  exit 1
fi

printf 'Phase 2: hermetic dry-run\n'
"$REF" --backend hermetic canary --run-id "$RUN_ID" --green

printf 'Phase 3 command (operator-gated): %s\n' "$PHASE3_CMD"
if [[ "$LIVE" -ne 1 ]]; then
  printf '%s\n' 'Phase 3 not run: pass --live and confirm on an egress-allowlisted host.'
  exit 0
fi

if [[ "${FABRO_REFEREE_LIVE_CONFIRM:-}" != 'yes' ]]; then
  if [[ ! -t 0 ]]; then
    printf '%s\n' 'referee-activate-live.sh: --live requires FABRO_REFEREE_LIVE_CONFIRM=yes or an interactive y/N confirmation' >&2
    exit 2
  fi
  printf 'Run the live forkd canary now? [y/N] '
  read -r CONFIRM
  [[ "$CONFIRM" == 'y' || "$CONFIRM" == 'Y' ]] || { printf '%s\n' 'Phase 3 cancelled.'; exit 0; }
fi

printf 'Phase 3: live forkd canary\n'
"$REF" --backend forkd --forkd-endpoint "$FORKD_ENDPOINT" canary --run-id "$RUN_ID"
