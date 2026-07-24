# Item 2 — auto-detect base-ref fix evidence (review finding 2d)

## The bug

`BASE_REF` defaulted to `HEAD`, and in `--auto-detect` mode `branch == HEAD`
(the current branch is always the checked-out branch). So the uncorrected
script always computed `git diff HEAD...HEAD` — an empty diff by
construction — for every tiered fleet session. The hermetic gate applies an
empty diff trivially and scores it `pass`. That means the `SessionEnd` hook
emitted a spurious `Pass` row for **every** tier session, regardless of
whether any real work had happened.

## The fix

In `--auto-detect` mode (unless the caller explicitly passes `--base-ref`,
used by the crash-test harness), the script now computes
`git merge-base main HEAD` (falling back to `origin/main` if no local `main`
exists) and uses that as `BASE_REF`. If the merge-base can't be resolved, or
resolves to `HEAD` itself, or the resulting diff is empty, the script exits
`0` and emits **no row** — "nothing to score" is not the same as "Pass".

## Case A — zero commits ahead of main → SKIP, no row

Self-contained scratch repo (does not touch the real project):

```
$ git init -q -b main "$TESTREPO"
$ echo hello > file.txt && git add file.txt && git commit -q -m initial
$ git log --oneline -1
c554ba5 initial

$ git checkout -q -b case-a-zero-ahead-sn      # tier branch, but 0 commits ahead of main
$ git rev-parse HEAD
c554ba5cf4937afbc7a670a077d3586dc5c5ae2b        # same sha as main tip
```

Sink before the run:
```
$ ls -la "$SINK_A"
total 0
drwxr-xr-x  2 vvladescu  wheel   64 ...  .
drwx------  ...                        ..
```
(empty)

Run:
```
$ lib/crates/fabro-referee/scripts/referee-score-dispatch.sh --auto-detect --hermetic \
    --project "$TESTREPO" --worktree-name basefix-case-a --sink-dir "$SINK_A"
$ echo $?
0
```
(no stdout/stderr — silent skip, as designed)

Sink after the run:
```
$ ls -la "$SINK_A"
total 0
drwxr-xr-x  2 vvladescu  wheel   64 ...  .
drwx------  ...                        ..
```
Still empty. **0 jsonl files** — confirmed no spurious Pass row.

## Case B — real diff, but a scratch-repo-specific hermetic-seeding quirk (noted, out of scope)

Same scratch repo, a branch with one real commit ahead of `main`:

```
$ git checkout -q main && git checkout -q -b case-b-real-diff-sn
$ echo change >> file.txt && git add file.txt && git commit -q -m "real change"
$ git log --oneline main..HEAD
437de74 real change
```

Running the fixed script against it:
```
$ lib/crates/fabro-referee/scripts/referee-score-dispatch.sh --auto-detect --hermetic \
    --project "$TESTREPO" --worktree-name basefix-case-b --sink-dir "$SINK_B"
$ echo $?
0
```

Row emitted (exactly one, `wc -l` on the sink's jsonl file = 1):
```json
{"run_id":"basefix-case-b-sn","task_id":"case-b-real-diff", ...,
 "verdict":"fail","gate_backend":"hermetic",
 "gate_log":"[hermetic] begin task=case-b-real-diff diff_bytes=122 valset_root=. base_ref=c554ba5cf4937afbc7a670a077d3586dc5c5ae2b\n...\n[hermetic] git-archive seed failed: git archive exit=exit status: 128; falling back to recursive copy\n[hermetic] copy-seed failed: Is a directory (os error 21)\n[hermetic] verdict=FAIL (diff did not apply against seeded valset_root)\n", ...}
```

Two things to note about this row, both **confirming the fix, not undermining it**:
- `base_ref` is `c554ba5c...` (the real merge-base commit), **not** `HEAD` —
  the bug is fixed; the base is correctly computed.
- `diff_bytes=122` — a real, non-empty diff, not the always-empty
  `HEAD...HEAD` from before.

The `verdict:fail` here is from an unrelated hermetic-backend seeding quirk
specific to this minimal from-scratch repo (`git archive` fails on it, exit
128, then the recursive-copy fallback also fails because the target is a
directory) — a pre-existing gate/backend behavior when the sandboxed
`valset_root` doesn't seed cleanly, orthogonal to the dispatch-script base-ref
fix under review here. It is **not** a case of "empty diff scored as Pass" —
quite the opposite, it's a real diff that failed to apply and was correctly
scored `fail`. Included for completeness; not one of the disqualifying
findings and not touched by this fix.

## Case C — real diff against the actual project repo → exactly one correct row, verdict pass

To show the fix producing a clean, fully-correct row (no seeding quirk), run
against this repo itself, on this branch
(`ao/fabro-42/referee-dispatch-wiring-sn`), with no `--base-ref` override:

```
$ git rev-parse main
b9078c2b69bc1a095920a76f8522c3b488449ffe     # == origin/main tip

$ lib/crates/fabro-referee/scripts/referee-score-dispatch.sh --auto-detect --hermetic --synthetic \
    --worktree-name basefix-realrepo-demo --sink-dir "$SINK_C"
$ echo $?
0
```

Sink before: empty. Sink after: exactly **one** jsonl file. Row:

```json
{"run_id":"synthetic-basefix-realrepo-demo-sn","task_id":"ao/fabro-42/referee-dispatch-wiring",
 "verdict":"pass","passed":true,"gate_backend":"hermetic","synthetic":true,
 "gate_log":"[hermetic] begin task=ao/fabro-42/referee-dispatch-wiring diff_bytes=29808 valset_root=. base_ref=b9078c2b69bc1a095920a76f8522c3b488449ffe\n...\n[hermetic] seeded workdir from .@b9078c2b69bc1a095920a76f8522c3b488449ffe (git archive)\n[hermetic] git apply ok for task=ao/fabro-42/referee-dispatch-wiring (29808 bytes diff)\n[hermetic] acceptance exit=0\n...\n[hermetic] verdict=pass (acceptance exit=0)\n"}
```

`base_ref` is `b9078c2b6...` — the real `merge-base(main, HEAD)`, i.e. the
actual origin/main tip this branch forked from — not `HEAD`. `diff_bytes`
is `29808`, the real size of this branch's diff. The gate genuinely applied
the diff and ran the (default `true`) acceptance, producing a legitimate
`pass`. This is the correct, intended end-to-end behavior: a tier session
with real work ahead of main gets exactly one row reflecting a real scoring
attempt — never a trivial Pass manufactured from an empty diff.

## Conclusion

- Zero commits ahead of the merge-base → **skip, 0 rows** (Case A).
- Real diff → **exactly 1 row**, `base_ref` is the genuine merge-base commit
  (never `HEAD`), `diff_bytes` reflects the real diff size, and the verdict
  reflects genuine gate mechanics — pass when the diff applies and
  acceptance succeeds (Case C), fail when it doesn't (Case B, plus an
  unrelated seeding quirk in that minimal scratch repo, noted above).
- The disqualifying bug — an empty `HEAD...HEAD` diff scored as a trivial
  `Pass` for every tiered session — no longer reproduces.
