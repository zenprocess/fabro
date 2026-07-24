#!/usr/bin/env bash
#
# scripts/backfill-referee.sh — retro-scoring backfill driver.
#
# Replays historical fabro fleet attempts through the hermetic
# referee scorer, emitting one RunRow per qualifying session tagged
# backfill:true. Diagnostic/triage use (re-scoring old diffs to
# populate the episode store) — NOT a live gate check.
#
# Source: ao.db (LIVE AO daemon database). Open READ-ONLY via sqlite
# URI mode=ro. Written only to the sink dir + scratch.
#
# Qualifying session = (project_id='fabro') AND (branch != '') AND
# (branch ends in -mm, -sn, or -qw). Other branches are skipped:
# root/orchestrator branches carry no tier-suffix and have no
# comparable sibling route.
#
# Per-session base_ref = `git merge-base main <branch>` (falls back
# to the PR row's base_sha/head_sha when present, but in practice
# the PR-joined sessions are root branches and skipped anyway).
#
# Acceptance = `true` so the diff-apply step is the meaningful
# backfill signal. The hermetic gate FAILs when the diff does not
# apply against the seeded valset_root@main, which is the correct
# verdict for "this old branch's diff is stale / no longer lands".
#
# See ITEM5-BRIEF.md for the operator scope; this script is the
# build-only deliverable for the brief.
#
# Usage:
#   scripts/backfill-referee.sh                    # full run
#   scripts/backfill-referee.sh --dry-run          # print plan, score nothing
#   scripts/backfill-referee.sh --limit 5          # score up to 5 sessions
#   scripts/backfill-referee.sh --sink-dir <path>  # override sink
#   FABRO_BACKFILL_DRY_RUN=1 scripts/backfill-referee.sh
#
# Env:
#   FABRO_AO_DB             path to ao.db (default ~/.ao/data/ao.db)
#   REPO_ROOT               path to the fabro worktree (default: autodetect)
#   FABRO_BACKFILL_SINK_DIR path to the sink dir (default ~/.ao/data/aofactory/referee/runs-backfill)
#   FABRO_BACKFILL_DRY_RUN  "1" to print the plan and skip scoring
#   FABRO_BACKFILL_BIN      path to the fabro-referee binary (default: cargo target/debug)

set -euo pipefail

# ---------- args ----------
DRY_RUN=0
LIMIT=0
while [ $# -gt 0 ]; do
    case "$1" in
        --dry-run) DRY_RUN=1; shift ;;
        --sink-dir) SINK_DIR_OVERRIDE="$2"; shift 2 ;;
        --limit) LIMIT="$2"; shift 2 ;;
        -h|--help)
            sed -n '2,40p' "$0"
            exit 0
            ;;
        *) echo "unknown arg: $1" >&2; exit 64 ;;
    esac
done

# ---------- paths ----------
DB="${FABRO_AO_DB:-$HOME/.ao/data/ao.db}"
REPO_ROOT="${REPO_ROOT:-$(git rev-parse --show-toplevel)}"
SINK_DIR="${SINK_DIR_OVERRIDE:-${FABRO_BACKFILL_SINK_DIR:-$REPO_ROOT/target/backfill-referee-runs}}"
WORK_DIR="${TMPDIR:-/tmp}/fabro-referee-backfill-$$-$(date +%s).$(od -An -N4 -tu4 /dev/urandom | tr -d ' ')"
mkdir -p "$WORK_DIR"
BIN="${FABRO_BACKFILL_BIN:-$REPO_ROOT/target/debug/fabro-referee}"

cleanup() { rm -rf "$WORK_DIR"; }
trap cleanup EXIT

mkdir -p "$SINK_DIR"

# ---------- counters ----------
TOTAL=0
SCORED=0
SKIP_NO_TIER=0
SKIP_NO_REF=0
SKIP_NO_DIFF=0
SKIP_GATE_ERROR=0

log() { printf '%s\n' "$*" >&2; }
log "[backfill] start: db=$DB repo=$REPO_ROOT sink=$SINK_DIR binary=$BIN dry=$DRY_RUN limit=$LIMIT"

# ---------- preflight ----------
if [ ! -f "$DB" ]; then
    echo "FATAL: ao.db not found at $DB" >&2; exit 1
fi
if [ ! -x "$BIN" ] && [ "$DRY_RUN" != "1" ]; then
    echo "FATAL: fabro-referee binary not found at $BIN; build first with: cargo build -p fabro-referee" >&2
    exit 1
fi

# ---------- pull sessions ----------
# Order by num so the run is deterministic.
SESSIONS=$(sqlite3 "file:$DB?mode=ro" "SELECT id, branch, num FROM sessions WHERE project_id='fabro' AND branch != '' ORDER BY num;")

# ---------- per-session loop ----------
while IFS='|' read -r sid branch num; do
    [ -z "$sid" ] && continue
    TOTAL=$((TOTAL+1))

    if [ "$LIMIT" -gt 0 ] && [ "$SCORED" -ge "$LIMIT" ]; then
        log "[backfill] limit reached ($LIMIT); stopping"
        break
    fi

    # tier suffix. Order matters: longer (-sn-escalated) before
    # shorter (-sn) so the more-specific pattern wins.
    case "$branch" in
        *-sn-*) tier_name="sonnet";  route="sn" ;;
        *-sn)   tier_name="sonnet";  route="sn" ;;
        *-mm-*) tier_name="minimax"; route="mm" ;;
        *-mm)   tier_name="minimax"; route="mm" ;;
        *-qw-*) tier_name="qwen";    route="qw" ;;
        *-qw)   tier_name="qwen";    route="qw" ;;
        *)      SKIP_NO_TIER=$((SKIP_NO_TIER+1)); log "[skip] no-tier-suffix: $branch (num=$num)"; continue ;;
    esac

    # branch must resolve
    if ! git -C "$REPO_ROOT" rev-parse --verify "$branch" >/dev/null 2>&1; then
        SKIP_NO_REF=$((SKIP_NO_REF+1)); log "[skip] no-ref: $branch (num=$num)"; continue
    fi

    # base_ref = merge-base main <branch>; fall back to PR row's
    # base_sha when no merge-base exists (rare: a branch whose
    # main-line ancestor was rewound out from under it).
    base_ref=$(git -C "$REPO_ROOT" merge-base main "$branch" 2>/dev/null || echo "")
    if [ -z "$base_ref" ]; then
        base_ref=$(sqlite3 "file:$DB?mode=ro" "SELECT base_sha FROM pr WHERE session_id='$sid' AND base_sha != '' LIMIT 1;" 2>/dev/null || echo "")
    fi
    if [ -z "$base_ref" ]; then
        SKIP_NO_REF=$((SKIP_NO_REF+1)); log "[skip] no-base-ref: $branch (num=$num)"; continue
    fi

    # diff = branch's divergence from base_ref
    diff=$(git -C "$REPO_ROOT" diff "$base_ref"..."$branch" 2>/dev/null || echo "")
    if [ -z "$diff" ]; then
        SKIP_NO_DIFF=$((SKIP_NO_DIFF+1)); log "[skip] no-diff: $branch (num=$num base=$base_ref)"; continue
    fi

    # build task + routes JSON files
    run_id="backfill-${num}-${route}"
    task_path="$WORK_DIR/task-${run_id}.json"
    routes_path="$WORK_DIR/routes-${run_id}.json"

    # task spec: prompt_path points at a file the hermetic backend
    # will try to seed (it tolerates an absent file, but the path
    # must be inside the repo so workdir mkdir -p succeeds).
    cat > "$task_path" <<EOF
{"task_id":"T-backfill-${num}","spec_ref":"ITEM5-BRIEF.md","difficulty_bucket":"backfill","prompt_path":"$REPO_ROOT/.gitkeep","project_path":"$REPO_ROOT","base_ref":"$base_ref","acceptance":{"kind":"shell_command","command":"true"}}
EOF

    # routes: encode diff via python to avoid bash escaping hell.
    python3 - "$branch" "$route" "$tier_name" "$sid" "$diff" > "$routes_path" <<'PYEOF'
import json, sys
branch, route, tier_name, sid, diff = sys.argv[1:]
print(json.dumps([{
    "tier": tier_name,
    "branch": branch,
    "tier_resolved": None,
    "decision_basis": None,
    "session_id": sid,
    "diff": diff,
    "diff_stat": None,
}]))
PYEOF

    log "[score] $run_id: branch=$branch base=$base_ref diff_bytes=${#diff}"

    if [ "$DRY_RUN" = "1" ]; then
        log "[dry-run] would invoke: $BIN score --backend hermetic --backfill --run-id $run_id --task $task_path --routes $routes_path --sink-dir $SINK_DIR --valset-root $REPO_ROOT"
        continue
    fi

    # invoke the just-built binary
    if "$BIN" score \
        --backend hermetic \
        --backfill \
        --run-id "$run_id" \
        --task "$task_path" \
        --routes "$routes_path" \
        --sink-dir "$SINK_DIR" \
        --valset-root "$REPO_ROOT" 2>&1 | sed 's/^/  /' >&2; then
        SCORED=$((SCORED+1))
    else
        SKIP_GATE_ERROR=$((SKIP_GATE_ERROR+1)); log "[skip] gate-error: $branch (num=$num)"
    fi
done <<< "$SESSIONS"

# ---------- summary ----------
cat <<EOF
[backfill] done
  total:        $TOTAL
  scored:       $SCORED
  no-tier-suffix: $SKIP_NO_TIER
  branch-unresolvable / no-base-ref: $SKIP_NO_REF
  no-diff:       $SKIP_NO_DIFF
  gate-error:    $SKIP_GATE_ERROR
  sink_dir:      $SINK_DIR
EOF
