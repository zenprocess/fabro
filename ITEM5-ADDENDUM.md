# Item 5 — status check + what's left

I read your uncommitted diff. Good foundational work:
- `RunRow.backfill` field, threaded cleanly through `runner::run()` / `make_row()` and the
  `Score` CLI subcommand (`--backfill` flag, `default_value_t = false`), with tests proving
  propagation on both Pass and Fail verdicts.

**First: commit this now.** `git add -A && git commit -m "wip: item 5 backfill flag plumbing"`
on your branch. Uncommitted work in an idle session is at risk — commit before anything else.

## What's still missing (the actual point of item 5)

You've built the plumbing the driver will call into, but **the batch driver itself doesn't exist
yet** — no new script/binary that actually queries `ao.db`, recovers diffs, and invokes
`fabro-referee score --backfill`. That's the real deliverable. Concretely, you still need:

1. A driver (bash script under `lib/crates/fabro-referee/scripts/`, e.g.
   `referee-backfill.sh`, is simplest — reuse the CLI's `score --backfill` you just built rather
   than reimplementing scorer logic in the driver) that:
   - Reads `sqlite3 "file:$HOME/.ao/data/ao.db?mode=ro"` for
     `SELECT * FROM sessions WHERE project_id='fabro' AND branch != ''` (join `pr` table where
     present for `base_sha`/`head_sha`).
   - Verify current row count yourself (brief noted ~40 as of investigation, may have grown) —
     report the actual number, don't assume 40.
   - For each session: recovers `route_diff` (via PR base/head shas, or `git merge-base main
     <branch>` against a still-resolvable ref), derives tier from branch suffix (`-mm`/`-sn`/`-qw`;
     skip untagged/root branches — no sibling to compare), skips (and counts/categorizes) any
     session whose branch no longer resolves.
   - Invokes `fabro-referee score --task ... --routes ... --run-id ... --backfill` per qualifying
     session (hermetic backend only — never forkd).
   - Tallies: scored count, and skip reasons (no-tier-suffix, branch-unresolvable, no-diff,
     gate-error).

2. Actually run it and get as close to **30 scored attempts** as real data allows. If short,
   report the real number + the skip breakdown honestly rather than padding.

3. Reconcile the 906-vs-actual-session-count discrepancy in your report (investigate whether
   it's grown past 40, but don't force-fit to 906 — flag if the operator's figure doesn't match
   reality).

4. Paste 2-3 sample backfilled RunRow JSON lines (with `"backfill":true`) in your report.

5. **Push + PR** against `zenprocess/fabro` main (current tip `b9078c2b6` — you're on the older
   `9e87d31c6`; rebase before pushing. Main now also has `attempt_key`/`model`/`passed` fields
   from item 4, and a `synthetic` field may be landing from a parallel item-2 PR — your `backfill`
   field is additive and should merge cleanly against both, but check). `gh` CLI may have broken
   TLS in this env — fall back to `git push` and note it if so.

## Constraints (repeat from original brief, still binding)

Hermetic backend only. Read-only against `ao.db` (`mode=ro`). Never touch zeninfra's repo files.

Report back: PR URL or pushed branch, final scored count + skip breakdown, 2-3 sample RunRow
lines. If you get stuck or run low on context, commit what you have and say so explicitly rather
than going idle silently — the orchestrator is actively watching for either a completion report
or a stall.
