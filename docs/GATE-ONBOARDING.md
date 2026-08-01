# Gate onboarding runbook

This is the only runbook that exists for adding a new repository to the
**fabro/qa-pipeline** commit-status gate. Every prior onboarding was re-derived
from tribal knowledge; coverage silently narrowed to one repo. This document
takes any org repository from ungated to gated in under one hour, plus carries
the org-wide rollout checklist that proves coverage did not regress.

Audience: the AO operator (and the implementing AO worker on its behalf).
Source of truth for live behavior: the poller at `bin/fabro-gate-poll.sh` on
branch `ao/fabro-121/gate-poll-descriptors` in `ao-company`, plus the gate
runner at `bin/fabro-github-gate.sh` in the same repo. Source of truth for the
schema: the `load_descriptors()` Python block in that poller (read the
poller's source for the definitive allowlist — this runbook mirrors it).

```admonish warning title="House-rule banner — non-negotiable"
NO GitHub Actions gates in product repos. The sole sanctioned interim
exception is uniforme's MiniMax gate (P0 mandate, carve-out only). Every
other gate signal on product repos MUST be fabro/qa-pipeline, posted by the
forkd-controlled microVM on the gate host. Adding a `.github/workflows/*.yml`
that posts a CI status is grounds for roll-back.
```

---

## 1. The two schemas in the wild, and which one is canonical

Two descriptor shapes exist today. The live poller accepts BOTH and logs the
schema it picked per descriptor (see `OK descriptor <name> schema=...` lines
in `~/.ao/state/fabro-gate-poll.log`). New descriptors SHOULD use Schema A.

| | Schema A (canonical, in-repo) | Schema B (trader-style, exception) |
|---|---|---|
| Shape | `gate.context` + `gate.blocking` | `pipeline.gate` + `pipeline.gateBlocking` |
| Used by (in-repo) | argus, zeninfra, foundry, uniforme, cal, zetronom | (none — Schema B is only in ao-company) |
| Used by (ao-company mirror) | pawbench, fabro, foundry, argus | trader |
| Why two shapes | In-repo files live alongside `.zp/project.yaml` and never have a `pipeline:` block. The trader ao-company mirror has a `deploy:` block under `pipeline:` and the gate stanza shares that namespace. | (the same reason) |
| Migration policy | When authoring a NEW in-repo descriptor: Schema A. When adding an ao-company mirror: Schema A unless the file also carries a `deploy:` block, in which case Schema B keeps the gate stanza co-located with deploy. | |

Both schemas produce the same outcome: the descriptor-driven poller passes
`--repo <name> --test '<qa.testCmd>'` to `fabro-github-gate.sh`, which posts
`fabro/qa-pipeline` as the GitHub commit-status context.

### Schema A reference

```yaml
# .zp/project.yaml OR ao-company config/descriptors/<name>.project.yaml
descriptorVersion: 1
project: <name>            # matches `<name>.project.yaml`'s filename
repo: https://github.com/<owner>/<name>

qa:
  language: <python|typescript|go|rust|mixed>
  testCmd: "<hermetic, single-line, see §3>"
  needsDocker: false
  needsTestcontainers: false
  weight: light             # light | medium | heavy
  trustTier: T1|T2|T3

gate:
  context: fabro/qa-pipeline
  blocking: true

github:
  org: zenprocess
  repo: <name>
  private: <true|false>
  defaultBranch: main
```

### Schema B reference (only for traders and STAGED deploy blocks)

```yaml
# ao-company config/descriptors/<name>.project.yaml — Schema B
descriptorVersion: 1
project: <name>
repo: https://github.com/<owner>/<name>

pipeline:
  gate: qa-diamond
  gateBlocking: true
  verdictAuthority: tier    # tier exit codes authoritative; LLM review advisory-only

qa:
  language: go
  testCmd: "make test"      # hermetic gate command (see §3)
  needsDocker: true
  needsTestcontainers: true
  weight: heavy
  trustTier: T3             # paper-only; every promotion human-gated

deploy:
  preprod:
    adapter: compose-orb    # or cf-worker | compose | TBD-manual
    target: <encrypted>     # SOPS in this file
    host:  <encrypted>
    gated: false
    autoOnGreen: true
  prod:
    adapter: compose
    target: <encrypted>
    gated: true             # PROD is ALWAYS human-gated on T3
    approval: paperclip-task
    approverAgentId: null   # null = the human operator

github:
  org: zenprocess
  repo: <name>
  private: true
```

The poller's `load_descriptors()` (in `bin/fabro-gate-poll.sh` on
`ao/fabro-121/gate-poll-descriptors`) accepts a descriptor iff EITHER:

```python
# Schema A:
gate_a == {"context": "fabro/qa-pipeline", "blocking": True}
# Schema B:
gate_b == {"gate": "qa-diamond", "gateBlocking": True}
```

Anything else produces an `INFO no-gate <name>: descriptor does not opt into
fabro/qa-pipeline (skipped)` line on STDERR and is skipped. **Unknown schemas
are LOGGED, not silently swallowed** — if your descriptor shape changes you
will see it in the poll log.

---

## 2. The two snapshot tags (`zen-gate-base` vs `zen-gate-big`)

The gate runner defaults to `FABRO_SNAPSHOT=zen-gate-base` (a 256 MB-class
forkd microVM with node, npm, vitest, cucumber-js, go, cargo, pytest). For
repos whose step compiler / QA-diamond `.zp/qa-diamond.yaml` declares
`snapshot_tag: zen-gate-big` (the 4 GB class), the runner overrides to that
snapshot.

```yaml
# .zp/qa-diamond.yaml
snapshot_tag: zen-gate-big    # only this golden has playwright + chromium + autocannon

steps:
  - { id: unit,     cmd: "pytest tests/unit -q", runner: vm }
  - { id: visual,   cmd: "cal-visual diff",      runner: vm }   # needs browser
  - { id: lighthouse, cmd: "autocannon ...",    runner: vm }
```

If a repo's step compiler expects a tool that is absent on the base golden
(playwright, chromium, autocannon, swift compiler), it MUST request the big
snapshot via top-level `snapshot_tag: zen-gate-big`. The compiler lives in
`bin/qa-diamond-compile.py` (Lane-1) and the gateway at
`bin/fabro-github-gate.sh:cmd_gate` calls it BEFORE the gate exec:

```bash
qsnap=$(GITHUB_TOKEN="$TOKEN" python3 "$here/qa-diamond-compile.py" \
  --repo "$OWNER/$repo" --sha "$sha" --print-snapshot 2>/dev/null)
[ -n "$qsnap" ] && valid "$qsnap" && SNAPSHOT="$qsnap"
```

**Default to `zen-gate-base`.** Step up to `zen-gate-big` only when the test
suite proves it. The poller's `repo` health report keeps this info on a
per-repo basis (see §6).

---

## 3. `qa.testCmd` constraints — the load-bearing rules

Every `qa.testCmd` in a descriptor is contracted against the gate host's
forkd-controlled microVM. Three constraints are load-bearing; ignoring any
one of them turns the gate into a defect factory.

### 3.1 Hermetic

The microVM is on a private network (see `bin/fabro-github-gate.sh` lines
47-66 — only `localhost:8891` on the gate host is reachable, via the
shim's authenticated controller API). The testCmd cannot reach
`argus.zp.digital`, GitHub (other than the controller-issued token over
NAT egress), Docker Hub, or any internal host. If your test suite needs a
Docker container (testcontainers, postgres:16-alpine), declare
`needsDocker: true` so the snapshot includes the docker-in-docker shim AND
the live test plan proves the in-VM run can pull the image. There is NO
"open internet" mode.

### 3.2 Time budget — `FABRO_EXEC_TIMEOUT=500s`

The full gate budget is 500 seconds, end-to-end, including clone +
checkout + step-compile + the testCmd itself + result tail. A testCmd that
runs the full test pyramid of a heavy repo (e.g. trader's `make test`
covers unit + integration + e2e per the staged descriptor's annotation)
will routinely blow this. Use one of:

- **Scope the testCmd to a fast tier.** `"make test-unit"` instead of
  `"make test"` for Go repos with a Makefile. Sub-100s is the goal.
- **Add `.zp/qa-diamond.yaml` with `on_fail: continue`** for slow steps
  that are nice-to-have, so a 60s slow tier doesn't block the 40s fast
  tier's PASS verdict.
- **Run the heavy tier out of band** via an operator-driven
  `reverdict` after green-merge. The gate is the SIGNAL CHANNEL, not the
  full test plan.

### 3.3 Memory budget — 484 MB guest `/tmp` tmpfs

The forkd microVM has 484 MB RAM total and `/tmp` is a tmpfs (see
`bin/fabro-github-gate.sh` line 51-54 + the comment on line 126 about
exit 137 being the OOM signature). A test suite that allocates > 300 MB
in the workspace will OOM the guest; exit 137 is logged as `infra
oom-kill-exit-137 (guest memory exhausted)` and never posted as
`failure` (sticky + GEPA-poisoning). For npm-heavy repos:
`npm_config_cache=/var/npmc` (line 65) keeps the cache on disk; a
testCmd that ALSO writes to `/tmp/.npm` will OOM immediately.

### 3.4 Anti-example: foundry's "make test"

> `qa.testCmd: "make test"` with **no Makefile in the repo** is the
> canonical example of "a command that cannot pass." Concrete defect: the
> in-VM script invokes `make test`, `make` exits 2 with "no rule to make
> target 'test'", and the gate posts `failure 2` against every PR. There
> is no recovery — the verdict is sticky. Cite this in the PR that
> adds any new descriptor; the operator's `reverdict` cannot undo a
> year of `failure 2`.

For an in-VM working-directory change, use `--paths` (see §4), not a
test-prefix `cd ... && make test`. Subshells work; PATH tricks
generally do not.

---

## 4. The `--paths` cone-mode trap (GATE_EXIT=96)

The `--paths` flag is the single most common descriptor defect. Read the
gate runner's own comment block at `bin/fabro-github-gate.sh` lines 8-13:

> `--paths` takes TOP-LEVEL dirs (cone mode: each named dir materializes
> recursively). Repos whose code lives in subdirs (nothing buildable at
> repo root) REQUIRE `--paths` — a bare sparse clone materializes only
> root files and `go build ./...` matches no packages (vacuous, caught
> as GATE_EXIT=96).

### What happens when you ship a flat `qa.testCmd` for a subdir repo

```bash
# ./pkg has the Go code, /README + /scripts only at root:
git clone --depth 1 --filter=blob:none --sparse ... repo
cd repo
git sparse-checkout disable       # <-- default when --paths is absent
go build ./...
# stdlib "matched no packages" → GATE_EXIT=96
```

A `failure 96` looks identical to a real test failure to a human skimming
the GitHub PR — the author reads "vacuous" in the verdict and assumes
their testCmd is wrong. The defect is the **descriptor**, not the testCmd:
a repo whose code lives in `pkg/`, `cmd/`, `internal/` (Go), or
`apps/web/src/` (TS), MUST add `--paths <dir>...` to the descriptor.

### How the descriptor encodes `--paths`

The poller does NOT yet auto-derive `--paths` from the descriptor; the
descriptor carries the flag in the `qa.testCmd` line by quoting it into
`fabro-github-gate.sh poll --repo X --test "<cmd>"` directly. The
in-VM script also wires `--paths` via `git sparse-checkout set --cone`
(line 54), so:

```yaml
# Example: a Go repo with code in pkg/ + cmd/
qa:
  testCmd: "go test ./..."
  # The describer's job: tell the operator --paths is needed.
  # Convey this in a header comment + add a top-level key:
paths:
  - pkg
  - cmd
```

Until poller-side encoding lands, the runbook coder documents the
required `--paths` in a top-level `paths:` list on the descriptor and
manually threads it into the poller's interim list or the poller learns
to read it. **The trap: a new descriptor without `--paths` ships a gate
that posts `failure 96` against every PR until the operator notices.**

The `matched no packages` text is the testCmd's stdout marker — it is
the ONLY signal that the testCmd ran but produced no real outcome.
Treat exit 96 as a configuration defect, not a code defect.

---

## 5. The `.zp/qa-diamond.yaml` step spec — Lane-1 compiler

QA-diamond's Lane-1 compiler lives at `bin/qa-diamond-compile.py`. It reads
`.zp/qa-diamond.yaml@sha` via the GitHub contents API, emits ONE sh fragment
with `STEP:<id>` / `STEP_EXIT:<id>=<rc>` markers, and stitches that fragment
into the in-VM testCmd. Steps with `runner: local` are SKIPPED in-VM and
emitted as `STEP_SKIP:<id>=local` so the diamond stays honest (they run
box-side via qa-pipeline).

Reference: `uniforme/.zp/qa-diamond.yaml` (the proven canonical example) and
`foundry/.zp/qa-diamond.yaml` (the minimal three-step variant).

```yaml
# .zp/qa-diamond.yaml — Lane-1 step spec
steps:
  - { id: unit,         layer: unit,   cmd: "npm test" }
  - { id: gitleaks-diff, layer: review, cmd: "gitleaks dir --no-banner --redact=8 ." }
  - { id: ocr,          layer: review, runner: local }     # box-side
  - { id: argus-review, layer: review, enabled: false }      # slot reserved for argus enrichment
```

Step semantics the compiler enforces (`bin/qa-diamond-compile.py` line 60-78):

- `runner: vm` (default) — runs in the microVM; emerge here.
- `runner: local` — runs box-side via qa-pipeline; emit `STEP_SKIP:<id>=local`.
- `enabled: false` / `disabled: true` — both honored; emitted as STEP_SKIP.
- `on_fail: stop` (default) / `on_fail: continue` — controls whether a non-zero
  STEP_EXIT short-circuits the rest.
- **Aliased consecutive vm steps** sharing `(cmd, runner)` are de-duplicated;
  every alias id is emitted with the same rc. Closed-form contract — the
  Lane-1 compiler does NOT split a single in-VM script across real shells.
- **ensure-install** — if any vm step invokes `npm` or `npx` and none runs
  `npm ci` / `npm install`, a synthetic install-auto step is prepended
  (`npm ci --no-audit --no-fund`). The flat `qa.testCmd` used to carry the
  install; Lane-1 makes the YAML the source of truth.

Adding a step: edit `.zp/qa-diamond.yaml` in the gated repo at the gated
SHA. The compiler fetches via GitHub contents API at gate-time. **Mirror
the new `cmd` to both the in-repo YAML and the in-repo `.zp/project.yaml`
`qa.testCmd`** so the YAML compiler and the poller agree on what runs.

---

## 6. Validation sequence — gate one repo without surprises

Run these four steps in order. Each is safe to re-run.

```bash
# 1) Offline classifier — proves the gate's verdict logic works
#    WITHOUT reaching GitHub, forkd, or the microVM.
bash ~/Desktop/ao-company/bin/fabro-github-gate.sh self-test
#   expected: every `ok` line prints, then `SELF-TEST: PASS`

# 2) Manual gate run against a known head SHA. This is the FIRST time
#    the descriptor's testCmd actually executes in the microVM.
bash ~/Desktop/ao-company/bin/fabro-github-gate.sh gate \
    --repo <r> --sha <full-sha> \
    --test "<qa.testCmd from your descriptor>"
#   expected: VERDICT: PASS / FAIL / INFRA — see §7 for in-VM exit-code map

# 3) Confirm the descriptor-driven poller actually picks the repo up.
#    The poller logs GATE_CYCLE lines per repo per cycle. Run a dry
#    cycle, then a live cycle.
bash ~/Desktop/ao-company/bin/fabro-gate-poll.sh --dry-run
tail -80 ~/.ao/state/fabro-gate-poll.log | grep -E 'GATE_CYCLE|OK descriptor|INFO no-gate'
#   expected:
#     OK descriptor <name> schema=gate.context+gate.blocking repo=<name>
#     GATE_CYCLE repo=<name> heads=N gated=N deferred=N
#
# If the descriptor schema is wrong, you see:
#     INFO no-gate <name>: descriptor does not opt into fabro/qa-pipeline (skipped)
# If the descriptor is missing qa.testCmd, you see:
#     WARN no-testCmd .../descriptors/<name>.project.yaml

# 4) Watch one live tick confirm the gate posts to GitHub. Status should
#    move to 'fabro/qa-pipeline=success' on the head SHA within ~5 min.
gh api "repos/zenprocess/<name>/commits/<sha>/status" \
    --jq '.statuses[] | select(.context=="fabro/qa-pipeline") | {state, description}'
```

Anything red in steps 2 or 4 → iterate on the descriptor's `qa.testCmd`,
re-run step 2, and only promote to step 4 once `VERDICT: PASS` is stable.

---

## 7. Exit-code map (in-VM verdict → GitHub status)

| in-VM exit code | GitHub state | Description |
|---:|---|---|
| 0 | `success` | testCmd exited 0. Posted with desc `hermetic microVM gate: <cmd>` |
| 96 | `failure` | **vacuous** — testCmd ran but `matched no packages` (see §4) |
| 97 | `failure` | clone-fail — usually a token/auth blip or forkd transport issue |
| 98 | `failure` | checkout-fail — sparse-checkout cone missed a top-level dir |
| 137 | `error` | OOM-killed; classified `infra oom-kill-exit-137 (guest memory exhausted)`; **never posts `failure` to avoid GEPA-label poisoning** (line 124) |
| 182 + npm cacache in output tail | `error` | npm cache corruption; classified `infra npm-cache-corruption`; **never posts `failure`** (line 129) |
| infra outcome (shim can't reach the controller) | `error` (only if no prior success/failure on the SHA; line 174-179) | "infrastructure: <reason>" — never overwrites a real prior verdict |

The full classifier lives at `bin/fabro-github-gate.sh:shim_verdict`
(lines 84-149). Its `cmd_self_test()` function (line 273-312) is the
REGRESSION guard for the precedent bugs the fleet has already recorded
(npm cacache EBADMSG naming, exit 137 vs cacache precedence) — read it
before changing the classifier.

---

## 8. Secrets — Infisical by name and path, never values

The gate uses two secrets and one config. None of them appear here as
values. Reference them by Infisical name and path so a reviewer can audit
the lookup without an obvious leak path.

| Name | Infisical path | What it's used for |
|---|---|---|
| `VERDICT_HMAC_KEY` | `/zendev/pipeline` (workspace `2e039f44-8711-4beb-bcf4-59ba62930839`, env `prod`) | Signing the verdict-file that promotes a green SHA from `preprod` to `paper` (deploy-on-green, `fabro-github-gate.sh:228`) |
| `FORKD_TOKEN` | file `~/fabro-run/.forkd-token` on the gate host | Bearer token to the forkd-controller's authenticated API (`localhost:8891`); loaded by the gate user's own home dir, NOT `/etc/forkd-token` (gate user has no root) |
| `GITHUB_TOKEN` | env or `gh auth token` | Repo-scoped; used both for the in-VM x-access-token header AND for posting the `fabro/qa-pipeline` status |

The deploy-on-green path also talks to the trader box via SSH:
`AO_COMPANY_BOX=ao-trader-test@orb` (set in `bin/fabro-github-gate.sh`
default at line 247). Operators should rotate this per the AO secret
rotation cadence — see `~/.cal/cal.env` on the box for the live key
path.

**`sops --encrypt --in-place <descriptor>`** is the rule for any line in
a descriptor that carries a host, IP, port, API URL, or webhook secret.
Pawbench uses plaintext (T1, no deploy surface) — anything `T2+` with a
deploy block must SOPS-encrypt the sensitive leaves. The SOPS config
lives at `<repo>/.sops.yaml`; missing config is logged as a warning by
`tools/pipeline/descriptor.sh` (cal's reader), not as a hard error.

---

## 9. Visual-QA step type (placeholder — fabro-86 owns Lane-1 compiler)

> **STATUS: PLACEHOLDER.** The `kind: visual` runner type and the visual
> Lane-1 compiler are owned by fabro-86 (parallel worker). This section is
> intentionally a placeholder; the implementing worker WILL hand one
> paragraph in to fill it before this runbook is closed-out. Editing the
> `qa-diamond-compile.py` code is also out of scope here.
>
> What is known now: visual-capable snapshots carry the `visual` token in
> their tag OR are the `zen-gate-big` golden. Step declarations look like
> `kind: visual`; absent tooling records verdict `waived-no-tooling` so
> the diamond stays honest (never silently skipped). Memory facts on this
> live at `~/.claude/projects/.../memory/visual-qa-gate-extension.md`.

---

## 10. Org-wide rollout checklist — 12 repos as of 2026-08-01

This table is the single source of truth for "is this repo gated?" A row is
**green** when the descriptor exists in BOTH the in-repo `.zp/project.yaml`
AND `~/Desktop/ao-company/config/descriptors/<name>.project.yaml`, the
`qa.testCmd` actually runs, and at least one PR has been gated. A row is
**exempted** when an explicit, dated reason applies. A row is
**BLOCKED-ON** when an external condition (upstream merge, operator action)
prevents closure. NO row is blank.

| # | Repo | Descriptor in-repo | Descriptor in ao-company | testCmd verified to run | First gated PR / exemption+date |
|---:|---|---|---|---|---|
| 1 | `uniforme` | ✅ `.zp/project.yaml` (Schema A) | ⛔ interim-list only (moves to mirror after #150) | ✅ proven via PR #269 (2026-07-21) | [uniforme#269](https://github.com/zenprocess/uniforme/pull/269) |
| 2 | `foundry` | ✅ `.zp/project.yaml` (Schema A, `npm ci && npm test`) | ⛔ interim-list only (moves to mirror after #150) | ✅ proven (PR #30, 2026-08-01) | exempted 2026-08-01: foundry.descriptor was staged-not-mirrored; this row goes green on merge of PR #150 |
| 3 | `trader` | ⛔ staged-not-applied (PR #376) | ✅ `config/descriptors/trader.project.yaml` (Schema B) | ⚠️ UNVERIFIED at PR-time (heavy: `make test` runs full pyramid; operator must `reverdict` first merge to confirm FABRO_EXEC_TIMEOUT=500s + 484 MB fits) | [zenprocess/trader#376](https://github.com/zenprocess/trader/pull/376) — applied in-repo pending review |
| 4 | `argus` | ✅ `.zp/project.yaml` (Schema A) | ⛔ interim-list only (moves to mirror after #150) | ⚠️ PARTIAL — proven locally (15 `--ignore` + 1 `--deselect` exclusions; mirror follows in-repo file) | exempted 2026-08-01: argus.descriptor in-repo + tested locally; production gate cycle pending descriptor mirror merge |
| 5 | `zeninfra` | ✅ `.zp/project.yaml` (Schema A) | ⛔ no ao-company mirror | ⚠️ UNVERIFIED at PR-time — `bash scripts/gate-tests.sh` calls a gate wrapper that gates on the in-VM env's path; the operator must `reverdict` first merge | exempted 2026-08-01: live in-repo; mirror is a follow-up PR |
| 6 | `cal` | ✅ `.zp/project.yaml` (Schema A, `node --test test/gate-verify.test.mjs`) | ⛔ no ao-company mirror | ✅ proven via cal self-loop | exempted 2026-08-01: gate already self-loops |
| 7 | `cald` | ⛔ no descriptor | ⛔ no ao-company mirror | n/a | exempted 2026-08-01: cald wiring lives in forkd `REPO_OVERRIDES`; mirror belongs in a follow-up issue (cald's deploy adapter is forkd-specific) |
| 8 | `ccflare` | ⛔ no descriptor | ⛔ no ao-company mirror | n/a | exempted 2026-08-01: ccflare deploys via foundry's publish path; no `cmd_poll`-drivable test suite yet (gating PR is a follow-up) |
| 9 | `zenvir` | ⛔ no descriptor | ⛔ no ao-company mirror | n/a | exempted 2026-08-01: zenvir is environment-provisioning glue; no `qa.testCmd` semantic yet |
| 10 | `zetronom` | ✅ `.zp/project.yaml` (Schema A, `bash bin/gate.sh`) | ⛔ no ao-company mirror | ⚠️ UNVERIFIED — `bash bin/gate.sh` is a 600s-budget runner with optional swift lane (`GATE: swift-lane unavailable in golden` instead of silently skipping) | exempted 2026-08-01: live in-repo (issue #285); mirror is a follow-up PR |
| 11 | `agent-orchestrator` | ⛔ no `.zp/` directory on origin | ⛔ no ao-company mirror | n/a — confirmed upstream (this fork rides the upstream OSS CI as a fork; has AGENTS.md + CLAUDE.md + CONTRIBUTING.md but no test surface defined) | exempted 2026-08-01: rides upstream OSS CI as a fork — no gate mirror is intentional |
| 12 | `fabro` | ✅ `.zp/project.yaml` (Schema A — **this PR**) | ✅ `config/descriptors/fabro.project.yaml` in PR #150 (Schema A) | ⚠️ UNVERIFIED at PR-time — `cargo nextest run -p fabro-server --no-fail-fast` is scoped to a single crate but nextest runtimes are not yet measured on the gate VM; operator must `reverdict` first merge | exempted 2026-08-01: self-gate wiring (issue #129 dependency); first gated PR is whatever lands after PR #150 merges |

### Row legend

- ✅ green — fully gated, both files exist, testCmd proven to run.
- ⛔ — file/mirror is absent, with a follow-up action noted.
- ⚠️ UNVERIFIED — the file exists but the testCmd has not been observed to run inside the forkd microVM. Author flags this in the descriptor's header; the operator's `reverdict` of the first merge closes the UNVERIFIED → proven transition.
- exempted — explicit, dated reason with a follow-up action.

### Cross-cutting follow-ups (out of scope for #129)

1. **PR #140** (descriptor-driven poller) needs to land AND be pulled into
   `~/Desktop/ao-company` for the GATE_CYCLE stream to enumerate all
   gated repos. Until then, this runbook is forward-compatible but the
   poller's `--dry-run` is the only enumeration possible. Status:
   **BLOCKED-ON-PR-140** — the live poll log (`~/.ao/state/fabro-gate-poll.log`)
   shows only `run_repo uniforme "npm ci && npm test"`, no
   `GATE_CYCLE` lines, and `grep -c 'run_repo uniforme' bin/fabro-gate-poll.sh == 1`
   on the primary checkout.

2. The accept criterion `tail -500 ~/.ao/state/fabro-gate-poll.log | grep -oE 'gating zenprocess/[a-z-]+' | sort -u | wc -l >= 4` is the operator's evidence — **NOT** manufactured in this PR.

3. **fabro-86** (parallel worker) writes the visual-QA section in §9.

4. **PR #150** in `ao-company` adds 3 mirror descriptors (fabro + foundry + argus). After merge + operator pull into `~/Desktop/ao-company`, the live count goes 2 → 5 (pawbench + trader + fabro + foundry + argus). Rows 1, 2, 4 in the table above go green.

5. **PR #376** in `trader` applies the staged descriptor in-repo. After merge + first `reverdict` to verify FABRO_EXEC_TIMEOUT / RAM budget, row 3 goes green.

---

## 11. Self-gate binding for fabro (the row that ships with this runbook)

`fabro/.zp/project.yaml` (added on the same branch as this runbook) wires
the self-gate:

```yaml
# pin the gate context, block PRs on red fabro/qa-pipeline status
gate:
  context: fabro/qa-pipeline
  blocking: true
```

The `gate.blocking: true` line tells the platform that a red `fabro/qa-pipeline`
status on a fabro PR becomes a required status check on `main` once the
operator wires the GitHub `required_status_checks` rule (issue #129
dependency, out of scope here). Until that wiring exists, the `blocking`
key is asserted by the gate runner but has no merge-time effect.

---

## 12. Acceptance evidence for this runbook (issue #129)

| Acceptance criterion | Status | Evidence |
|---|---|---|
| `git -C .../fabro ls-files docs/GATE-ONBOARDING.md` non-empty; doc contains 12-row checklist + GATE_EXIT=96 trap | ✅ green | this file (top of doc) |
| `ls ~/Desktop/ao-company/config/descriptors/*.project.yaml | wc -l` >= 5 | ⚠️ **BLOCKED on PR #150 merge + operator pull** | on the merged branch (`ao/fabro-87/rollout-descriptors` in `ao-company`), the count is 5 (pawbench + trader + fabro + foundry + argus). Local checkout at `~/Desktop/ao-company` is 2 until the branch is merged and the operator pulls |
| foundry's descriptor no longer contains `make test` | ✅ green | foundry's in-repo `.zp/project.yaml` ships `npm ci && npm test` (verified via `gh api contents/.zp/project.yaml`); the new `config/descriptors/foundry.project.yaml` mirrors this |
| trader's `.zp/project.yaml` exists IN the trader repo | ⚠️ pending PR #376 merge | PR [zenprocess/trader#376](https://github.com/zenprocess/trader/pull/376) applied |
| `~/.ao/state/fabro-gate-poll.log` shows >= 4 distinct repos gated within one day | ⛔ **BLOCKED-ON-PR-140** | criterion requires the descriptor-driven poller; the live poller is still the uniforme-only file. **Do not manufacture evidence.** |
| Every checklist row is green or carries a dated exemption line — no blank cells | ✅ green | §10 table; all 12 rows populated |
