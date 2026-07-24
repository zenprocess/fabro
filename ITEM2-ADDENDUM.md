# Item 2 — status check + what's left

I read your uncommitted diff. Good work so far:
- `TaskSpec.synthetic` + `RunRow.synthetic` fields, well-documented, tests pass.
- `referee-score-dispatch.sh` gained `--branch`/`--tier` single-route mode, `--auto-detect`,
  `--synthetic`, `--worktree-name` — solid design, matches the brief's option (a)/(b) tension well.

**First: commit this now.** `git add -A && git commit -m "wip: item 2 synthetic field + dispatch script auto-detect mode"`
on your branch. Uncommitted work in an idle session is at risk — commit before doing anything else,
even if incomplete. You can keep amending/adding commits after.

## What's still missing (the actual point of item 2)

1. **The hook call-site itself.** Nothing in `.claude/settings.json` (tracked, repo root) invokes
   `referee-score-dispatch.sh` yet — that's the literal bug this item exists to fix (0 call sites).
   Add the entry (confirm `SessionEnd` vs `Stop` by checking an existing worktree's
   `.claude/settings.local.json` for which event ao actually fires reliably at session completion —
   e.g. `cat /Users/vvladescu/.ao/data/worktrees/fabro/*/.claude/settings.local.json | grep -A3 SessionEnd`).
   The hook command should call `referee-score-dispatch.sh --auto-detect --hermetic` (your own
   `--auto-detect` mode already handles "not a tier branch → exit 0" gracefully per your usage text —
   confirm that's actually implemented, since it's what makes this hook safe to add repo-wide).

2. **3 synthetic end-to-end proof rows.** Actually run your new script 3 times with `--synthetic`
   (or equivalent), confirm 3 new JSONL rows land in the sink dir with `run_id` prefixed `synthetic-`,
   `synthetic:true`, `gate_backend:"hermetic"`. Paste the 3 lines in your report.

3. **Crash-mid-run adversarial test.** Kill a run mid-flight (e.g. `set -e` + `kill -9` the scorer
   subprocess, or interrupt before the gate call completes) and confirm: (a) no orphaned RunRow
   (the existing `trap cleanup EXIT` in the script should already cover this — verify it still holds
   with your changes), (b) the failure is observable (non-zero exit, a log line) not silently dropped.

4. **OCR review** via `cal-tmux-dispatch <slug> "Review this diff: $(git diff main)"` before you
   consider this done — report the tmux session name.

5. **Kiwi.** Check `KIWI_TCMS_URL` etc. If unset, touch a `kiwi-waived` file in your worktree root
   with reason `"KIWI_TCMS_URL not configured in this environment"`.

6. **Push + PR** against `zenprocess/fabro` main (current tip: `b9078c2b6` — you're currently
   based on the older `9e87d31c6`; rebase before pushing, main now also has `attempt_key`/`model`/
   `passed` fields on `RunRow` from a parallel item-4 PR — reconcile your `synthetic` field addition
   against that, it should be a clean additive merge). `gh` CLI may have broken TLS in this env —
   fall back to `git push` and note it if so.

Report back: PR URL or pushed branch, the hook diff, the 3 synthetic row samples, crash-test
evidence, OCR tmux session name, kiwi-waived status. If you get stuck or run low on context,
commit what you have and say so explicitly rather than going idle silently — the orchestrator
is actively watching for either a completion report or a stall.
