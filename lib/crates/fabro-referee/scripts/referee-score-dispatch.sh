#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat >&2 <<'USAGE'
Usage: referee-score-dispatch.sh --task-id ID --branch-mm BRANCH --branch-sn BRANCH --run-id ID [options]
  --base-ref REF              Base ref (default: HEAD)
  --project PATH               Project path (default: .)
  --acceptance-cmd COMMAND     Closed-form acceptance command (default: true)
  --forkd-endpoint URL         Forkd endpoint (default: http://dellsrv:8891)
  --sink-dir PATH               Sink directory (default: ~/.ao/data/aofactory/referee/runs)
  --hermetic                    Use the no-network hermetic backend
USAGE
  exit 2
}

TASK_ID="${TASK_ID:-}"
BASE_REF="${BASE_REF:-HEAD}"
PROJECT="${PROJECT:-.}"
BRANCH_MM="${BRANCH_MM:-}"
BRANCH_SN="${BRANCH_SN:-}"
ACCEPTANCE_CMD="${ACCEPTANCE_CMD:-true}"
RUN_ID="${RUN_ID:-${FABRO_REFEREE_RUN_ID:-}}"
FORKD_ENDPOINT="${FORKD_ENDPOINT:-http://dellsrv:8891}"
SINK_DIR="${SINK_DIR:-${HOME}/.ao/data/aofactory/referee/runs}"
HERMETIC=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --task-id) TASK_ID="$2"; shift 2 ;;
    --base-ref) BASE_REF="$2"; shift 2 ;;
    --project) PROJECT="$2"; shift 2 ;;
    --branch-mm) BRANCH_MM="$2"; shift 2 ;;
    --branch-sn) BRANCH_SN="$2"; shift 2 ;;
    --acceptance-cmd) ACCEPTANCE_CMD="$2"; shift 2 ;;
    --run-id) RUN_ID="$2"; shift 2 ;;
    --forkd-endpoint) FORKD_ENDPOINT="$2"; shift 2 ;;
    --sink-dir) SINK_DIR="$2"; shift 2 ;;
    --hermetic) HERMETIC=1; shift ;;
    -h|--help) usage ;;
    *) printf 'referee-score-dispatch.sh: unknown argument: %s\n' "$1" >&2; usage ;;
  esac
done

[[ -n "$TASK_ID" ]] || { printf '%s\n' 'referee-score-dispatch.sh: --task-id is required' >&2; exit 2; }
[[ -n "$BRANCH_MM" ]] || { printf '%s\n' 'referee-score-dispatch.sh: --branch-mm is required' >&2; exit 2; }
[[ -n "$BRANCH_SN" ]] || { printf '%s\n' 'referee-score-dispatch.sh: --branch-sn is required' >&2; exit 2; }
[[ -n "$RUN_ID" ]] || { printf '%s\n' 'referee-score-dispatch.sh: --run-id is required (caller-supplied and stable)' >&2; exit 2; }
[[ -d "$PROJECT" ]] || { printf 'referee-score-dispatch.sh: project is not a directory: %s\n' "$PROJECT" >&2; exit 2; }

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
  printf '%s\n' 'referee-score-dispatch.sh: fabro-referee not found; set FABRO_REFEREE or build target/{release,debug}/fabro-referee' >&2
  exit 127
fi
[[ -x "$REF" ]] || { printf 'referee-score-dispatch.sh: referee binary is not executable: %s\n' "$REF" >&2; exit 127; }

TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/fabro-referee-dispatch.XXXXXX")"
cleanup() { rm -rf -- "$TMP_DIR"; }
trap cleanup EXIT

capture_route() {
  local tier="$1"
  local branch="$2"
  local diff_file="$TMP_DIR/${tier}.diff"
  local stat_file="$TMP_DIR/${tier}.stat"
  git -C "$PROJECT" diff "$BASE_REF...$branch" > "$diff_file"
  git -C "$PROJECT" diff --stat "$BASE_REF...$branch" > "$stat_file"
  jq -n \
    --arg tier "$tier" \
    --arg branch "$branch" \
    --rawfile diff "$diff_file" \
    --rawfile diff_stat "$stat_file" \
    '{tier: $tier, branch: $branch, tier_resolved: null, decision_basis: null, session_id: null, diff: $diff, diff_stat: $diff_stat}'
}

capture_route minimax "$BRANCH_MM" > "$TMP_DIR/mm-route.json"
capture_route sonnet "$BRANCH_SN" > "$TMP_DIR/sn-route.json"
jq -s '.' "$TMP_DIR/mm-route.json" "$TMP_DIR/sn-route.json" > "$TMP_DIR/routes.json"

jq -n \
  --arg task_id "$TASK_ID" \
  --arg project_path "$PROJECT" \
  --arg base_ref "$BASE_REF" \
  --arg acceptance "$ACCEPTANCE_CMD" \
  '{task_id: $task_id, spec_ref: "fabro-referee/live-wiring", difficulty_bucket: "unknown", prompt_path: $project_path, project_path: $project_path, base_ref: $base_ref, acceptance: {kind: "shell_command", command: $acceptance}}' \
  > "$TMP_DIR/task.json"

BACKEND_ARGS=(--backend forkd --forkd-endpoint "$FORKD_ENDPOINT")
if [[ "$HERMETIC" -eq 1 ]]; then
  BACKEND_ARGS=(--backend hermetic)
fi

"$REF" "${BACKEND_ARGS[@]}" --sink-dir "$SINK_DIR" score \
  --task "$TMP_DIR/task.json" --routes "$TMP_DIR/routes.json" --run-id "$RUN_ID"

printf 'referee sinks:\n%s/%s.jsonl\n%s/%s.md\n' "$SINK_DIR" "$RUN_ID" "$SINK_DIR" "$RUN_ID"
