# TASK T3 — register referee scores as fabro server runs (code-side only, no live calls)

Rust implementation task in `lib/components/fabro-referee`. Branch off `origin/main`,
commit, push, open a **DRAFT** PR against `zenprocess/fabro`.

## Why

Campaign item 8: `fabro.zp.digital/runs` shows nothing since Jul 10 because gate execs go
through forkd and referee runs write only the Mac JSONL sink — neither becomes a fabro
server run. PR #19 already merged the receiving endpoint
(`POST /api/v1/runs/registrations`, operationId `registerExternalRun`). The missing half
is the CALLER.

## Read first

- `docs/public/api-reference/fabro-api.yaml` — the `/api/v1/runs/registrations` path
  (around line 1316) and the `RunRegistrationRequest` schema (around line 11757). **This
  spec is the source of truth for the wire contract.** Do not invent field names.
- `lib/components/fabro-referee/src/emit.rs` — the existing sink emitter. Note its
  idempotency design (re-running the same `(run_id, route)` overwrites rather than
  double-appends). Your registration must respect the same replay semantics: the endpoint
  returns 201 on create and **200 on idempotent re-registration** — treat BOTH as success.
- `lib/components/fabro-referee/src/types.rs` — `RunRow`, `Verdict`.
- Repo conventions in `AGENTS.md`/`CLAUDE.md`: Rust import style (types by name, functions
  via parent module, no glob imports), the API type-ownership rules, and the test-support
  boundary rules (`#[cfg(any(test, feature = "test-support"))]`, never in default features).

## Scope — what to build

Emit the registration alongside the existing sink write, **opt-in and off by default**:

- Gated behind an explicit env var (e.g. `FABRO_REFEREE_REGISTER_RUNS=1` plus a base-URL
  var). **Default OFF** — a fresh checkout must not attempt any network call.
- Map the verdict exactly as the spec documents: `pass` → Succeeded/Completed,
  `fail` → Failed/WorkflowError, `inconclusive` → Failed/TransientInfra.
- Populate `origin.details` with real provenance (gate backend, route/tier, commit SHA).
- **Truthfulness rule from the spec, enforce it**: there is no "register a placeholder"
  path. If the source cannot produce a real verdict it MUST send `inconclusive` — never
  fabricate `pass`. Encode this so a future refactor cannot quietly default to `pass`.
- **Registration failure must never corrupt or block the sink write.** The JSONL sink is
  the authoritative artifact; a server that is down must degrade to a logged warning, not
  a lost score and not a crash.
- The spec rejects a ref NAME where a SHA is required — validate before sending and fail
  loudly with a clear message rather than sending garbage.

Keep the diff tight: roughly a new `register.rs`, its wiring in `lib.rs`/the emit path,
and a test file. Do not refactor unrelated referee code.

## Testing — mandatory, and no live network

Use `httpmock` (already a dev-dependency in this workspace — see PR #20's usage) to assert
against a mock HTTP responder. **No test may contact a real fabro server.** Cover at
minimum:

1. Request body matches the OpenAPI `RunRegistrationRequest` shape (field names verified
   against the YAML, not from memory).
2. All three verdict mappings, including `inconclusive` → TransientInfra.
3. **200 idempotent re-registration is treated as success**, not as an error.
4. Server 5xx / connection failure → sink write still succeeds, warning logged, no panic.
5. Feature is OFF by default: with the env var unset, no HTTP request is made at all
   (assert the mock received zero hits).

If you add shared test helpers, they go behind the `test-support` feature per the repo's
test-support boundary rules — never exported from a production module, never in default
features.

## Acceptance command (must pass; paste the real output in your report)

```
cargo nextest run -p fabro-referee \
  && cargo +nightly-2026-04-14 fmt --check -p fabro-referee \
  && cargo +nightly-2026-04-14 clippy -p fabro-referee --no-deps --tests -- -D warnings \
  && echo ACCEPT
```

Note: a full-workspace `cargo build` is hook-blocked on this Mac; crate-scoped commands
above are the authoritative local gate. Also confirm the root `Cargo.lock` is committed if
your change adds or changes any dependency — a prior PR in this repo (#20) shipped without
it and every contributor regenerated it as uncommitted drift. Run `git status` after
testing and check.

## Adversarial check (required — do not skip)

Run at least **4 mutations**, one at a time, each on a scratch copy, each reverted after,
and report a table of `mutation → failing test → tests executed`:

1. Change the `inconclusive` mapping to `pass` (this is the campaign's core failure class:
   misreporting infra as a real verdict — it MUST be caught).
2. Treat the 200 idempotent response as an error.
3. Make a registration failure propagate and abort the sink write.
4. Your own choice — target the test you judge WEAKEST, not an easy one.

**Collapse check**: the executed test count must stay constant across every mutation. A
mutation that yields `0 passed; 0 failed`, a reduced count, or a compile error only is
INCONCLUSIVE, not a pass — redo it as a clean edit. Before trusting any "not caught"
result, `diff` the mutated file to confirm the mutation actually applied (a silently
non-applied mutation looks exactly like a vacuous test).

If a mutation is NOT caught, that is the finding — add the missing test, re-run the
identical mutation, and prove it now fails.

## Report back

Draft PR number, real acceptance output, the mutation table with executed counts, and
confirmation that `git diff` vs origin is clean of mutations afterward. Verify your push
by checking the REMOTE tip SHA, not by trusting an echoed "pushed" — a `git push origin
<branch>` can succeed as a no-op while your commit sits on a detached HEAD. Do NOT mark
done without pasted acceptance output.
