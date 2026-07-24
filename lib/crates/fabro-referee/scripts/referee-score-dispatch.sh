#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat >&2 <<'USAGE'
Usage:
  referee-score-dispatch.sh --task-id ID --branch-mm B1 --branch-sn B2 --run-id ID [options]
  referee-score-dispatch.sh --task-id ID --branch B --tier T --run-id ID [options]
  referee-score-dispatch.sh --auto-detect [--hermetic] [--synthetic] [options]

Two-tier / single-route options:
  --task-id ID                Task id (e.g. T-foo).
  --base-ref REF              Base ref (default: HEAD).
  --project PATH              Project path (default: .).
  --branch-mm BRANCH          minimax route branch (two-tier mode).
  --branch-sn BRANCH          sonnet route branch (two-tier mode).
  --branch BRANCH             Single route branch (single-route mode).
  --tier TIER                 Tier for --branch: mm | sn | qw (single-route mode).
  --acceptance-cmd COMMAND    Closed-form acceptance (default: true).
  --run-id ID                 Run id (caller-supplied, stable).

Auto-detect options (hook-driven surface):
  --auto-detect               Derive branch + tier + task_id + run_id from
                              `git rev-parse --abbrev-ref HEAD` and
                              $AO_SESSION_ID. Exits 0 if not a tier branch.
                              Also computes the diff base as
                              `merge-base(main, HEAD)` (unless --base-ref is
                              passed explicitly) and exits 0 with no row if
                              that base equals HEAD or the resulting diff is
                              empty — an empty diff is "nothing to score",
                              never a Pass.
  --worktree-name NAME        Override AO_SESSION_ID (used in the run_id when
                              AO_SESSION_ID is unset; usually the worktree
                              dir basename).

Common:
  --synthetic                 Tag the source task spec / emitted row as
                              `synthetic: true` AND prefix run_id with
                              "synthetic-". Real fleet callers MUST NOT
                              pass this.
  --hermetic                  Use the no-network hermetic backend (default
                              for hook invocations; T3 forkd is not safe
                              from a session-end hook in this worktree).
  --forkd-endpoint URL        Forkd endpoint (default: http://dellsrv:8891).
  --sink-dir PATH             Sink directory (default: ~/.ao/data/aofactory/referee/runs).
USAGE
  exit 2
}

TASK_ID="${TASK_ID:-}"
BASE_REF="${BASE_REF:-HEAD}"
BASE_REF_EXPLICIT=0
PROJECT="${PROJECT:-.}"
BRANCH_MM="${BRANCH_MM:-}"
BRANCH_SN="${BRANCH_SN:-}"
BRANCH_SINGLE="${BRANCH_SINGLE:-}"
TIER_SINGLE="${TIER_SINGLE:-}"
ACCEPTANCE_CMD="${ACCEPTANCE_CMD:-true}"
RUN_ID="${RUN_ID:-${FABRO_REFEREE_RUN_ID:-}}"
FORKD_ENDPOINT="${FORKD_ENDPOINT:-http://dellsrv:8891}"
SINK_DIR="${SINK_DIR:-${HOME}/.ao/data/aofactory/referee/runs}"
HERMETIC=0
SYNTHETIC=0
AUTO_DETECT=0
WORKTREE_NAME="${WORKTREE_NAME:-${AO_SESSION_ID:-}}"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --task-id) TASK_ID="$2"; shift 2 ;;
    --base-ref) BASE_REF="$2"; BASE_REF_EXPLICIT=1; shift 2 ;;
    --project) PROJECT="$2"; shift 2 ;;
    --branch-mm) BRANCH_MM="$2"; shift 2 ;;
    --branch-sn) BRANCH_SN="$2"; shift 2 ;;
    --branch) BRANCH_SINGLE="$2"; shift 2 ;;
    --tier) TIER_SINGLE="$2"; shift 2 ;;
    --acceptance-cmd) ACCEPTANCE_CMD="$2"; shift 2 ;;
    --run-id) RUN_ID="$2"; shift 2 ;;
    --forkd-endpoint) FORKD_ENDPOINT="$2"; shift 2 ;;
    --sink-dir) SINK_DIR="$2"; shift 2 ;;
    --hermetic) HERMETIC=1; shift ;;
    --synthetic) SYNTHETIC=1; shift ;;
    --auto-detect) AUTO_DETECT=1; shift ;;
    --worktree-name) WORKTREE_NAME="$2"; shift 2 ;;
    -h|--help) usage ;;
    *) printf 'referee-score-dispatch.sh: unknown argument: %s\n' "$1" >&2; usage ;;
  esac
done

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "$SCRIPT_DIR/../../../.." && pwd)"

# Auto-detect mode: derive everything from the worker's git + AO state.
# Exits 0 silently if this is not a tier attempt (e.g. the operator is
# on main, the merge target, or any non-tier branch).
if [[ "$AUTO_DETECT" -eq 1 ]]; then
  if [[ ! -d "$PROJECT" ]]; then
    # Try $PWD as a fallback for hook invocations where the worktree
    # CWD is the project root. Hook commands inherit the worker's CWD.
    if [[ -n "${PWD:-}" && -d "$PWD" ]]; then
      PROJECT="$PWD"
    else
      printf 'referee-score-dispatch.sh: --project is not a directory and PWD is unset\n' >&2
      exit 2
    fi
  fi
  CUR_BRANCH="$(git -C "$PROJECT" rev-parse --abbrev-ref HEAD 2>/dev/null || true)"
  if [[ -z "$CUR_BRANCH" || "$CUR_BRANCH" == "HEAD" ]]; then
    # Detached HEAD or non-git project — nothing to score.
    exit 0
  fi
  # Detect tier suffix: exactly one of -mm, -sn, -qw, anchored at the end
  # of the ref so a coincidental substring (e.g. a date/token that happens
  # to end in the same two letters) can never masquerade as a tier branch.
  if [[ "$CUR_BRANCH" =~ -(mm|sn|qw)$ ]]; then
    TIER_SINGLE="${BASH_REMATCH[1]}"
  else
    # Not a tier branch — main session, merge target, etc.
    # Fail-open: this is the "sibling doesn't exist yet" case the
    # operator asked for. Silent exit 0 so the hook doesn't noise
    # on the orchestrator's own sessions.
    exit 0
  fi
  BRANCH_SINGLE="$CUR_BRANCH"
  TASK_ID="${CUR_BRANCH%-"$TIER_SINGLE"}"

  # BASE_REF defaults to HEAD, and CUR_BRANCH *is* HEAD in auto-detect mode,
  # so leaving it at the default turns `git diff BASE_REF...branch` into
  # `git diff HEAD...HEAD` — always empty, which the gate scores as a
  # trivial Pass. That's a false-positive Pass row for every tiered session,
  # not "nothing to score". Compute the real merge-base against main instead,
  # unless the caller explicitly passed --base-ref (used by the crash-test
  # harness to force a real diff on demand).
  if [[ "$BASE_REF_EXPLICIT" -ne 1 ]]; then
    MERGE_TARGET="main"
    if ! git -C "$PROJECT" rev-parse --verify --quiet "$MERGE_TARGET" >/dev/null 2>&1; then
      MERGE_TARGET="origin/main"
    fi
    AUTO_BASE="$(git -C "$PROJECT" merge-base "$MERGE_TARGET" HEAD 2>/dev/null || true)"
    if [[ -z "$AUTO_BASE" ]]; then
      # No common ancestor found (main/origin-main unresolvable or history
      # is disjoint) — nothing safe to score against. Skip, no row.
      exit 0
    fi
    BASE_REF="$AUTO_BASE"
  fi

  HEAD_SHA="$(git -C "$PROJECT" rev-parse HEAD 2>/dev/null || true)"
  if [[ -z "$HEAD_SHA" || "$BASE_REF" == "$HEAD_SHA" ]]; then
    # Base resolves to HEAD itself: nothing ahead of the merge-base yet.
    # An empty/absent diff is "nothing to score" — never emit a Pass row
    # for it.
    exit 0
  fi

  if ! AUTO_DIFF="$(git -C "$PROJECT" diff "$BASE_REF...$CUR_BRANCH" 2>&1)"; then
    printf 'referee-score-dispatch.sh: git diff %s...%s failed during auto-detect skip-check; skipping (no row emitted):\n%s\n' \
      "$BASE_REF" "$CUR_BRANCH" "$AUTO_DIFF" >&2
    exit 0
  fi
  if [[ -z "$AUTO_DIFF" ]]; then
    # Real merge-base, but nothing changed relative to it — still "nothing
    # to score", never a Pass.
    exit 0
  fi

  if [[ -z "$WORKTREE_NAME" ]]; then
    # Last-ditch: use the project dir basename (worktree name).
    WORKTREE_NAME="$(basename "$PROJECT")"
  fi
  [[ -n "$WORKTREE_NAME" ]] || {
    printf 'referee-score-dispatch.sh: auto-detect could not derive run_id (no AO_SESSION_ID, no worktree name)\n' >&2
    exit 2
  }
  RUN_ID="${WORKTREE_NAME}-${TIER_SINGLE}"
fi

# Synthetic runs get a "synthetic-" run_id prefix so their sink files and
# every downstream consumer can filter them out by id alone, not just by the
# row's `synthetic` field. Idempotent: never double-prefixes.
if [[ "$SYNTHETIC" -eq 1 && -n "$RUN_ID" && "$RUN_ID" != synthetic-* ]]; then
  RUN_ID="synthetic-${RUN_ID}"
fi

# --- Validation: fail-closed on malformed input ---

# Exactly one of (two-tier) or (single-route) must be configured.
TWO_TIER=0
SINGLE_ROUTE=0
if [[ -n "$BRANCH_MM" && -n "$BRANCH_SN" ]]; then
  TWO_TIER=1
elif [[ -n "$BRANCH_SINGLE" && -n "$TIER_SINGLE" ]]; then
  SINGLE_ROUTE=1
elif [[ -n "$BRANCH_MM" || -n "$BRANCH_SN" ]]; then
  printf 'referee-score-dispatch.sh: --branch-mm and --branch-sn must be set together (two-tier mode)\n' >&2
  usage
elif [[ -n "$BRANCH_SINGLE" || -n "$TIER_SINGLE" ]]; then
  printf 'referee-score-dispatch.sh: --branch and --tier must be set together (single-route mode)\n' >&2
  usage
else
  printf 'referee-score-dispatch.sh: must supply either (--branch-mm + --branch-sn) or (--branch + --tier), or use --auto-detect\n' >&2
  usage
fi

[[ -n "$TASK_ID" ]] || { printf '%s\n' 'referee-score-dispatch.sh: --task-id is required' >&2; exit 2; }
[[ -n "$RUN_ID" ]] || { printf '%s\n' 'referee-score-dispatch.sh: --run-id is required (caller-supplied and stable)' >&2; exit 2; }
[[ -d "$PROJECT" ]] || { printf 'referee-score-dispatch.sh: project is not a directory: %s\n' "$PROJECT" >&2; exit 2; }

# A synthetic row must be unambiguously hermetic-tagged. Refuse to emit a
# synthetic row under a live (forkd) backend under any circumstance.
if [[ "$SYNTHETIC" -eq 1 && "$HERMETIC" -ne 1 ]]; then
  printf 'referee-score-dispatch.sh: --synthetic requires --hermetic; refusing to emit a synthetic row under a non-hermetic backend\n' >&2
  exit 2
fi

# Invariant: synthetic:true must always carry a synthetic- run_id prefix.
# The prefixing above is idempotent and unconditional whenever SYNTHETIC=1,
# so this should never trip; it exists purely as a guard against a future
# code path adding a synthetic row without going through that prefix step.
if [[ "$SYNTHETIC" -eq 1 && "$RUN_ID" != synthetic-* ]]; then
  printf 'referee-score-dispatch.sh: internal invariant violated: synthetic run without synthetic- run_id prefix (run_id=%s)\n' "$RUN_ID" >&2
  exit 70
fi

# Tier validation for single-route mode.
if [[ "$SINGLE_ROUTE" -eq 1 ]]; then
  case "$TIER_SINGLE" in
    mm|sn|qw) ;;
    *) printf 'referee-score-dispatch.sh: --tier must be one of mm|sn|qw (got: %s)\n' "$TIER_SINGLE" >&2; exit 2 ;;
  esac
fi

if [[ -n "${FABRO_REFEREE:-}" ]]; then
  REF="$FABRO_REFEREE"
elif [[ -x "$REPO_ROOT/target/release/fabro-referee" ]]; then
  REF="$REPO_ROOT/target/release/fabro-referee"
elif [[ -x "$REPO_ROOT/target/debug/fabro-referee" ]]; then
  REF="$REPO_ROOT/target/debug/fabro-referee"
elif command -v fabro-referee >/dev/null 2>&1; then
  REF="$(command -v fabro-referee)"
else
  printf '%s\n' 'referee-score-dispatch.sh: fabro-referee not found; set FABRO_REFEREE or build target/{release,debug}/fabro-referee' >&2
  exit 127
fi
[[ -x "$REF" ]] || { printf 'referee-score-dispatch.sh: referee binary is not executable: %s\n' "$REF" >&2; exit 127; }

# --- Build the routes JSON (the only per-mode difference) ---

# tmpdir is trap-cleaned on EXIT, so a crash mid-run never leaves a
# half-built task.json that the binary could mistake for a real one.
TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/fabro-referee-dispatch.XXXXXX")"
cleanup() { rm -rf -- "$TMP_DIR"; }
trap cleanup EXIT

capture_route() {
  local tier="$1"
  local branch="$2"
  # The Route struct's `tier` is the full enum name (minimax/sonnet/qwen),
  # not the short code (mm/sn/qw) the script's auto-detect uses. Map the
  # short code to the full name so the binary can deserialize the JSON.
  local tier_full
  case "$tier" in
    mm)   tier_full="minimax" ;;
    sn)   tier_full="sonnet" ;;
    qw)   tier_full="qwen" ;;
    *)    tier_full="$tier" ;;
  esac
  local diff_file="$TMP_DIR/${tier}.diff"
  local stat_file="$TMP_DIR/${tier}.stat"
  # Fail closed: a failed `git diff` (bad ref, branch not found) must never
  # be silently swallowed into an empty diff that then gets scored as a
  # trivial Pass. Log and skip the whole run rather than emit a row derived
  # from a failed diff.
  if ! git -C "$PROJECT" diff "$BASE_REF...$branch" > "$diff_file" 2>"$TMP_DIR/${tier}.diff.err"; then
    printf 'referee-score-dispatch.sh: git diff %s...%s failed; skipping (no row emitted):\n' "$BASE_REF" "$branch" >&2
    cat "$TMP_DIR/${tier}.diff.err" >&2
    exit 1
  fi
  if ! git -C "$PROJECT" diff --stat "$BASE_REF...$branch" > "$stat_file" 2>"$TMP_DIR/${tier}.stat.err"; then
    printf 'referee-score-dispatch.sh: git diff --stat %s...%s failed; skipping (no row emitted):\n' "$BASE_REF" "$branch" >&2
    cat "$TMP_DIR/${tier}.stat.err" >&2
    exit 1
  fi
  jq -n \
    --arg tier "$tier_full" \
    --arg branch "$branch" \
    --rawfile diff "$diff_file" \
    --rawfile diff_stat "$stat_file" \
    '{tier: $tier, branch: $branch, tier_resolved: null, decision_basis: null, session_id: null, diff: $diff, diff_stat: $diff_stat}'
}

if [[ "$TWO_TIER" -eq 1 ]]; then
  capture_route minimax "$BRANCH_MM" > "$TMP_DIR/mm-route.json"
  capture_route sonnet  "$BRANCH_SN" > "$TMP_DIR/sn-route.json"
  jq -s '.' "$TMP_DIR/mm-route.json" "$TMP_DIR/sn-route.json" > "$TMP_DIR/routes.json"
else
  capture_route "$TIER_SINGLE" "$BRANCH_SINGLE" > "$TMP_DIR/route.json"
  jq -s '.' "$TMP_DIR/route.json" > "$TMP_DIR/routes.json"
fi

# --- Build the task JSON (synthetic flows through here) ---

SYNTHETIC_JSON="false"
if [[ "$SYNTHETIC" -eq 1 ]]; then
  SYNTHETIC_JSON="true"
fi

jq -n \
  --arg task_id "$TASK_ID" \
  --arg project_path "$PROJECT" \
  --arg base_ref "$BASE_REF" \
  --arg acceptance "$ACCEPTANCE_CMD" \
  --argjson synthetic "$SYNTHETIC_JSON" \
  '{task_id: $task_id, spec_ref: "fabro-referee/live-wiring", difficulty_bucket: "unknown", prompt_path: $project_path, project_path: $project_path, base_ref: $base_ref, acceptance: {kind: "shell_command", command: $acceptance}, synthetic: $synthetic}' \
  > "$TMP_DIR/task.json"

# --- Invoke the binary ---

BACKEND_ARGS=(--backend forkd --forkd-endpoint "$FORKD_ENDPOINT")
if [[ "$HERMETIC" -eq 1 ]]; then
  BACKEND_ARGS=(--backend hermetic)
fi

"$REF" "${BACKEND_ARGS[@]}" --sink-dir "$SINK_DIR" score \
  --task "$TMP_DIR/task.json" --routes "$TMP_DIR/routes.json" --run-id "$RUN_ID"

printf 'referee sinks:\n%s/%s.jsonl\n%s/%s.md\n' "$SINK_DIR" "$RUN_ID" "$SINK_DIR" "$RUN_ID"