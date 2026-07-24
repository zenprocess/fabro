#!/usr/bin/env python3
"""Join-proof: prove a RunRow JSONL line and a wrapper-decisions.jsonl
line share the same (session_id, branch) and that the joined record is
internally consistent.

This is the operator-acceptance bar from ITEM4-BRIEF.md: a synthetic
end-to-end demonstration that the join keys `session_id` + `branch`
(which fabro-referee already carries on `RunRow`, and which the
wrapper already writes to `wrapper-decisions.jsonl`) actually line up,
and that `RunRow.model` corresponds to `DecisionLogLine.final_model`.

Usage:
    python3 scripts/join_proof.py [--runrow <path>] [--decisions <path>]

Default fixtures live in tests/fixtures/ (see commit).
Exits non-zero if the join fails.
"""
from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any

REPO_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_RUNROW = REPO_ROOT / "tests" / "fixtures" / "runrow_v2_full.jsonl"
DEFAULT_DECISIONS = REPO_ROOT / "tests" / "fixtures" / "wrapper_decision_one_line.jsonl"


def _load_jsonl(path: Path) -> list[dict[str, Any]]:
    out = []
    for line in path.read_text().splitlines():
        line = line.strip()
        if not line:
            continue
        out.append(json.loads(line))
    return out


def join_one(runrow: dict[str, Any], decisions: list[dict[str, Any]]) -> dict[str, Any] | None:
    """Return the joined record, or None if no decision-line matches.

    Joins on (session_id, branch). The joined record flattens the
    treatment side (`tier_resolved`, `decision_basis`, `final_model`)
    onto the scoring side so a reviewer can read it in one go.
    """
    sid = runrow.get("session_id")
    branch = runrow.get("branch")
    if sid is None or branch is None:
        return None
    hit = next(
        (
            d for d in decisions
            if d.get("session_id") == sid and d.get("branch") == branch
        ),
        None,
    )
    if hit is None:
        return None
    return {
        # Scoring side (RunRow):
        "task_id":     runrow["task_id"],
        "attempt_key": runrow.get("attempt_key"),
        "run_id":      runrow["run_id"],
        "ts_scored":   runrow["ts"],
        "verdict":     runrow["verdict"],
        "passed":      runrow.get("passed"),
        "score":       runrow.get("score"),
        "valset_hash": runrow.get("valset_hash"),
        "harness":     runrow["harness"],
        # Treatment side (DecisionLogLine):
        "tier_resolved":       hit["tier_resolved"],
        "decision_basis":      hit["decision_basis"],
        "model":               hit["final_model"],
        "endpoint":            hit["final_endpoint"],
        "fallback_taken":      hit["fallback_taken"],
        "ts_dispatched":       hit["ts"],
        # Cross-plane consistency check:
        "model_match":         runrow.get("model") == hit["final_model"],
        "passed_match_verdict": (
            runrow.get("passed") is (runrow["verdict"] == "pass")
        ),
        "join_key_session_id": sid,
        "join_key_branch":     branch,
    }


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument(
        "--runrow",
        type=Path,
        default=DEFAULT_RUNROW,
        help=f"path to RunRow JSONL (default: {DEFAULT_RUNROW})",
    )
    p.add_argument(
        "--decisions",
        type=Path,
        default=DEFAULT_DECISIONS,
        help=f"path to wrapper-decisions JSONL (default: {DEFAULT_DECISIONS})",
    )
    args = p.parse_args()

    rows = _load_jsonl(args.runrow)
    decisions = _load_jsonl(args.decisions)
    if not rows:
        print(f"no rows in {args.runrow}", file=sys.stderr)
        return 2
    if not decisions:
        print(f"no decision-lines in {args.decisions}", file=sys.stderr)
        return 2

    joined = [j for j in (join_one(r, decisions) for r in rows) if j is not None]
    if not joined:
        print(
            "JOIN FAILED: no (session_id, branch) match between "
            f"{args.runrow} and {args.decisions}",
            file=sys.stderr,
        )
        return 1

    print(f"joined {len(joined)}/{len(rows)} runrow(s)")
    for j in joined:
        print(json.dumps(j, indent=2, sort_keys=True))

    # Sanity: every joined record must pass the cross-plane checks.
    for j in joined:
        assert j["model_match"], (
            f"model drift: RunRow.model={j.get('model')!r} vs "
            f"DecisionLogLine.final_model={j['model']!r}"
        )
        assert j["passed_match_verdict"], (
            f"passed/verdict drift: passed={j['passed']!r} verdict={j['verdict']!r}"
        )
    return 0


if __name__ == "__main__":
    sys.exit(main())