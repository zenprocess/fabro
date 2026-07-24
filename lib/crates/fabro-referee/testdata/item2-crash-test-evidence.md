# Item 2 — crash-mid-run adversarial test evidence

Reproduces: kill the `fabro-referee score` child process mid-flight while it is
genuinely inside the scoring path (not the near-instant `diff_bytes=0` fail
path — this run used `--base-ref origin/main` so the range diff against the
branch tip is real, forcing the gate to apply the diff and start the
`sleep 30` closed-form acceptance, giving a real multi-second execution
window to interrupt).

## Command

```
lib/crates/fabro-referee/scripts/referee-score-dispatch.sh --auto-detect --hermetic --synthetic \
  --base-ref origin/main \
  --worktree-name crash-victim-4 --acceptance-cmd 'sleep 30' --sink-dir "$SINK" &
SCRIPT_PID=$!
```

## Sink before the run

```
$ ls -la "$SINK"
total 0
drwxr-xr-x@ 2 vvladescu  wheel   64 Jul 24 20:05 .
drwx------@ 8 vvladescu  wheel  256 Jul 24 20:05 ..
```

(empty)

## Sink 2s into the run, before the kill

```
$ ls -la "$SINK"/*.jsonl
no matches found  # still running, no row yet
```

stderr at that point:

```
INFO scoring route task=ao/fabro-42/referee-dispatch-wiring branch=ao/fabro-42/referee-dispatch-wiring-sn tier=sonnet
```

This confirms the binary is inside the scoring call (past task/route JSON
assembly, into `Gate::score`), not merely at shell setup.

## The kill

The scoring **binary child** (not the wrapper script) was isolated via
`pgrep -f 'target/debug/fabro-referee' | grep -v "^$SCRIPT_PID$"` and killed:

```
$ kill -9 59207
killed BIN_PID=59207
$ wait $SCRIPT_PID
SCRIPT_EXIT=137        # 128+9 SIGKILL — non-zero, observable failure
```

Wrapper's own log line for the kill (bash job control, `set -e` propagates
the child's failure as the script's own exit):

```
lib/crates/fabro-referee/scripts/referee-score-dispatch.sh: line 250: 59207 Killed: 9  "$REF" "${BACKEND_ARGS[@]}" --sink-dir "$SINK_DIR" score --task "$TMP_DIR/task.json" --routes "$TMP_DIR/routes.json" --run-id "$RUN_ID"
```

## Sink immediately after the kill

```
$ ls -la "$SINK"/*.jsonl
no matches found  # zero rows — no orphaned/partial row
```

## Sink after an extended 32s wait (2s past the 30s acceptance window)

Rules out a delayed row from a reparented/orphaned grandchild finishing
asynchronously after the parent binary was killed:

```
$ ls -la "$SINK"/*.jsonl
no matches found  # still zero rows

$ grep -c 'run complete' "$SINK/stderr.log"
0                  # the run never logged a normal completion

$ find "${TMPDIR:-/tmp}" -maxdepth 1 -name 'fabro-referee-dispatch.*' -exec ls -ld {} \;
(no output — zero leftover dispatch tmpdirs)
```

## Conclusion

- Kill lands mid-scoring (confirmed by the `scoring route` log line, not a
  fast diff_bytes=0 short-circuit).
- Script exit code is **137** — the failure is observable via exit status,
  not silently dropped.
- **Zero** orphaned or partial `RunRow`s in the sink, immediately after the
  kill and after an extended 32s wait past the acceptance window.
- **Zero** leftover dispatch tmpdirs — the wrapper's own `trap cleanup EXIT`
  fires because the wrapper itself exits normally via `set -e` after its
  child dies (it is never SIGKILL'd itself).
