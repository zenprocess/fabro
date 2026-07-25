# Referee P0 live-wiring handoff

This handoff is for the main, non-sandboxed session. The artifacts in this
folder are build-only wiring; they do not activate the live plane by
themselves.

## Bridge conditions for live per-tier scores

Both conditions gate enabling per-tier scores:

1. **Independent adversarial re-review: MET.** Worker `fabro-34`
   re-reviewed fix commit `3c46844a8` against all six original REJECT
   findings and returned **ACCEPT**. The review included 15/15 tests,
   including the new modify-existing-file backend-correctness regression.
2. **Real live-forkd canary: OWED.** It has never been run from an
   egress-allowlisted host; only `HermeticLocal` has been exercised.

Until condition 2 is met, do not treat hermetic output as live-controller
proof.

## Required dellsrv-side bridge, in order

The main session/operator must bridge these steps on an egress-allowlisted
host:

1. Confirm the forkd endpoint path (`/gate-run` versus `/forkd-exec`) and the
   request-body schema against the live controller. `CONTRACTS.md` §1 records
   both as **not yet observed live**.
2. Confirm the episode-store sink path
   (`~/.ao/data/aofactory/referee/runs`) and the `gate_backend` field with
   zeninfra before the harvest ETL lands (`CONTRACTS.md` §3 item 3).
3. Run `referee-activate-live.sh --live` from that egress-allowlisted host,
   with the explicit operator confirmation required by the script.
4. Once the live canary is green, enable `referee-score-dispatch.sh` in the
   real multi-tier dispatch lifecycle so per-tier gate logs fire for every
   attempt.

The dispatch driver requires a caller-supplied stable `--run-id`; it captures
both `-mm` and `-sn` branch diffs and invokes the scorer without fabricating a
verdict. Use `--hermetic` for a no-network driver dry run.

## T3 boundary

**Build + canary only from the fleet; live activation is operator-driven.**
The scorer and these scripts only score/read the controller path. They never
activate, reconfigure, restart, or re-baseline the forkd controller or golden
rootfs. Do not run live activation from this sandboxed worktree.

## Operator command

The exact phase-3 command printed by the activation script is:

```sh
"$REF" --backend forkd --forkd-endpoint "$FORKD_ENDPOINT" canary --run-id "$RUN_ID"
```

The script expands those variables when it prints the command. It will not
execute phase 3 unless `--live` is supplied and the operator confirms with
`FABRO_REFEREE_LIVE_CONFIRM=yes` or an interactive `y` response.
