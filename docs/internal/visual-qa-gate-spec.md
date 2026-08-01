# Visual QA as a first-class gate job type

**Status:** Draft (2026-08-01)
**Owners:** fabro gate team
**Related:** issue #129 (GATE-ONBOARDING runbook), uniforme `.zp/qa-diamond.yaml`

## Problem

The `visual` step in the QA-diamond descriptor exists today and is compiled
into the in-VM gate block by `qa-diamond-compile.py`. It is hard-wired to
`runner: local` because the hermetic gate VM (`zen-gate-base`, 484 MB RAM
tmpfs) ships no Playwright, no browsers, and no Xvfb. The qa-pipeline that
runs the local steps honours the absent-tooling contract — it records
`waived-no-tooling` instead of pretending the step passed — but that means
no project can ever get a real visual diff from the gate today.

This spec promotes `visual` to a real `runner: vm` job type so the gate can
shoot and diff screenshots on every PR, while preserving the honesty
property: **a missing/invalid visual snapshot MUST record
`waived-no-tooling`, never a silent skip and never a false pass.**

## Goals

1. A web project turns on visual QA by declaring one `id: visual` step
   in `.zp/qa-diamond.yaml` with `runner: vm` and `kind: visual`.
2. The step runs entirely inside the gate microVM when the VM snapshot
   has Playwright + a headless browser baked in.
3. When the snapshot does NOT have the tooling, the step emits
   `STEP_SKIP:<id>=waived-no-tooling` and the overall gate verdict is
   unaffected. A project that has not opted into a visual snapshot
   stays exactly as honest as it was before.
4. The forkd controller boots a `visual`-capable snapshot (e.g.
   `zen-gate-visual`) on demand, hands the in-VM script the project
   config, and returns a three-outcome verdict (success / failure /
   infra) — identical contract to every other VM step.
5. The existing `bash bin/fabro-github-gate.sh self-test` continues to
   exit 0 — the visual-QA extension is additive.

## Non-goals

- Not touching `docs/GATE-ONBOARDING.md` (parallel worker, issue #129).
- Not touching `config/descriptors/*.project.yaml`.
- Not baking the new VM snapshot. That is T3 operator work; the spec
  hands the operator exact commands to run on dellsrv.
- Not inventing an "extraction" narrative from uniforme — there is no
  visual artifact to extract yet. The schema is designed so uniforme's
  in-flight instance can slot in without a rewrite.

## Descriptor extension

The new shape is additive: every existing field keeps its meaning, and
a step opts into the visual job type by setting `kind: visual`. The
schema is still `descriptorVersion: 1`.

```yaml
descriptorVersion: 1
project: my-web-app

# The VM snapshot MUST be a visual-capable one (e.g. zen-gate-visual).
# The Lane-1 compiler passes snapshot_tag to the forkd controller; a
# snapshot that does not bundle playwright + chromium is REJECTED by the
# visual step (it records waived-no-tooling) but is still valid for the
# non-visual steps.
snapshot_tag: zen-gate-visual

steps:
  - id: unit
    cmd: npm run test:unit
    timeout_secs: 600
    layer: unit
    on_fail: stop
    runner: vm

  - id: visual
    runner: vm
    kind: visual
    timeout_secs: 500          # FABRO_EXEC_TIMEOUT ceiling
    layer: visual
    on_fail: continue          # default: never blocks the gate
    disabled: false
    config:
      server:
        cmd: npm run dev
        port: 3000
        ready_path: /          # HTTP probe path; 200 within timeout = ready
        ready_timeout_secs: 90
        env: {}                # extra env vars for the server, optional
      viewports:
        - { name: phone,  width: 375,  height: 812 }
        - { name: tablet,  width: 768,  height: 1024 }
        - { name: wide,    width: 1440, height: 900 }
      routes:
        - /
        - /admin
        - /account
      upload:
        provider: s3
        bucket: fabro-visual-qa
        path_prefix: "{repo}/{sha}/"
        public_base_url: https://visual-qa.fabro.sh
      diff:
        threshold_pct: 0.1      # pixel-diff threshold; 0.1% = 0.001
        baseline:               # baseline is OPTIONAL — without one the
          provider: s3          # step always records success and stores
          bucket: fabro-visual-qa-baselines  # the screenshots for the
          path_prefix: "{repo}/"             # next run to diff against
```

### Required vs optional fields

| Field | Required | Notes |
|-------|----------|-------|
| `kind` | yes | Must be `visual` to opt into this job type. |
| `runner` | yes | Must be `vm`. The compiler rejects `runner: local` for a `kind: visual` step. |
| `config.server.cmd` | yes | The dev server boot command (e.g. `npm run dev`, `wrangler dev`, `vite`). |
| `config.server.port` | yes | The TCP port the server listens on. |
| `config.server.ready_path` | yes | HTTP path the probe polls for a 2xx response. |
| `config.server.ready_timeout_secs` | no | Default 60s. Max 180s (gate VM `FABRO_EXEC_TIMEOUT` is 500s). |
| `config.viewports` | yes | At least one. Each must have `name`, `width`, `height`. |
| `config.routes` | yes | At least one route (absolute path, origin is the local server). |
| `config.upload.provider` | no | Default `s3`. |
| `config.upload.bucket` | no | Default `fabro-visual-qa`. |
| `config.upload.path_prefix` | no | Default `{repo}/{sha}/`. |
| `config.diff.threshold_pct` | no | Default `0.1` (0.1%). |
| `config.diff.baseline` | no | If absent, the step always passes and stores screenshots as new baselines. |

## Runner contract

The `visual` step is `runner: vm`, so it runs inside the forkd microVM.
The compiler (`qa-diamond-compile.py`) emits an in-VM shell fragment
that:

1. Asserts the snapshot has `playwright` and a chromium binary
   (`test -x /usr/local/share/playwright/.local-browsers/chromium-*/chrome-linux/chrome`).
   If not, prints `STEP_SKIP:<id>=waived-no-tooling` and exits 0.
2. Boots the dev server (`config.server.cmd`) in the background, exports
   `config.server.env` into its environment, and writes the server PID.
3. Probes `http://localhost:<port><ready_path>` every second up to
   `ready_timeout_secs`. Returns `STEP_EXIT:<id>=1` on timeout with the
   last probe response in the output.
4. Writes a generated Playwright spec to `/tmp/visual-qa/spec.mjs` that
   captures each route at each viewport and saves PNGs under
   `/tmp/visual-qa/shots/<viewport>/<route>.png`. Routes are slugified
   (`/` → `_root`, `/admin` → `_admin`).
5. Runs `node /tmp/visual-qa/spec.mjs`; non-zero exit = failure with the
   playwright log tail in the output.
6. Optionally fetches the baseline screenshots from
   `config.diff.baseline` (S3 GET), diffs with
   `pixelmatch`/`playwright`'s `toHaveScreenshot` API at
   `config.diff.threshold_pct`, and writes a diff PNG per shot to
   `/tmp/visual-qa/diffs/`. Any over-threshold diff = failure with the
   diff list in the output.
7. Uploads all PNGs to `config.upload.bucket` at
   `<path_prefix><viewport>/<route>.png`. Path template substitution
   uses `{repo}` and `{sha}` (the gated SHA, exported by the in-VM
   script).
8. Kills the dev server (`kill $SERVER_PID`), cleans `/tmp/visual-qa`,
   and exits 0 with a `STEP_EXIT:<id>=0` marker.

The `on_fail` field is honoured: `continue` means a failure does not
propagate to `overall`; `stop` means the first failure sets `overall`
and subsequent steps get `STEP_SKIP:<id>=after-stop-failure`.

## Snapshot requirement (T3 operator work)

The visual step can only run on a snapshot that has the tooling
provisioned. The recommended approach is a NEW snapshot tag
`zen-gate-visual` (bake on dellsrv), with the provisioner steps
documented for the operator.

**Tradeoff (explicit):** baking Chromium into a new snapshot costs
~280 MB of disk and ~120 MB of resident memory on top of `zen-gate-base`.
The 484 MB gate VM tmpfs cannot fit Playwright + Chromium + screenshots
for 3 viewports at 1440×900. `zen-gate-visual` MUST be a `zen-gate-big`
(4 GB) class snapshot, OR a new `*-visual` snapshot with the 484 MB
tmpfs raised. Recommended: build on top of `zen-gate-big` (4 GB
guest, 6911/6911 uniforme suite green) — the existing
`zeninfra/zengate` provisioner supports this via tag pinning.

The operator bakes it on dellsrv (out of scope for this session;
**dellsrv is behind the egress boundary — do not attempt to reach it
from this VM**):

```bash
# Run on dellsrv (the gate host). T3 — operator only.
sudo zengate bake visual \
  --base zen-gate-big \
  --tag zen-gate-visual \
  --provision 'apt-get install -y libnss3 libatk1.0-0 libatk-bridge2.0-0 \
    libcups2 libdrm2 libxkbcommon0 libxcomposite1 libxdamage1 \
    libxfixes3 libxrandr2 libgbm1 libpango-1.0-0 libcairo2 libasound2t64' \
  --npm 'playwright@1.49.0 --with-deps chromium' \
  --verify 'node -e "require.resolve(\"playwright\")"'
```

If the operator is unavailable, projects pin `snapshot_tag: zen-gate-big`
and the visual step records `STEP_SKIP:<id>=waived-no-tooling` until
the snapshot is baked. The honesty property is preserved.

## Why this is a `runner: vm` step and not a new runtime

The forkd controller's exec timeout is `FABRO_EXEC_TIMEOUT=500s`. A
visual step that boots a dev server + runs Playwright + screenshots 3
viewports + uploads typically fits in 180–300s, leaving headroom under
500s. A separate `runner: visual` would have meant a new code path in
the controller, a new snapshot tag class, and a new verdict contract —
none of which buy anything the existing `runner: vm` doesn't already
provide. The `kind: visual` marker is enough for the compiler to
generate the right in-VM script and for the snapshot preflight to
gate it.

## Compiler contract (additive, backwards compatible)

`qa-diamond-compile.py` is extended, not rewritten. New behaviour:

- If a step has `kind: visual`:
  - `runner` must be `vm`; any other value is a compile error (exit 3)
    with a clear stderr message.
  - The compiler checks the document's `snapshot_tag` against an
    internal allowlist of visual-capable tags (default: any tag whose
    name contains `visual` or equals `zen-gate-big`). If the snapshot
    is not visual-capable, the compiled fragment for this step is
    `echo STEP_SKIP:<id>=waived-no-tooling`.
  - Otherwise, the compiler generates the in-VM shell fragment from
    the step's `config` block (see "Runner contract" above) and emits
    the standard `STEP:<id>` / `STEP_EXIT:<id>=<rc>` pair.
  - The `cmd` field is IGNORED for `kind: visual` steps — the
    compiler-generated fragment is authoritative. A `cmd` field is
    allowed for documentation but never executed.
- All existing behaviour (install-auto prepend, dedupe, `disabled`,
  `on_fail`, `runner: local` skip) is unchanged.

`fabro-github-gate.sh` Lane-1 compiler is extended to:

- Recognize `kind: visual` in the compiled output and, when the
  snapshot is NOT visual-capable, log `== qa-diamond: visual step
  <id> skipped: snapshot <tag> is not visual-capable` BEFORE handing
  the script to the controller. This is informational only — the
  compiled fragment already emits the skip marker.

## forkd job template (new file in ao-company)

`bin/fabro-visual-qa-template.sh` is the on-disk template the compiler
substitutes into. It accepts environment variables exported by the
compiler and the in-VM script:

| Env var | Source | Notes |
|---------|--------|-------|
| `VISUAL_SERVER_CMD` | `config.server.cmd` | shell-quoted |
| `VISUAL_SERVER_PORT` | `config.server.port` | integer |
| `VISUAL_READY_PATH` | `config.server.ready_path` | path-only |
| `VISUAL_READY_TIMEOUT` | `config.server.ready_timeout_secs` | integer |
| `VISUAL_VIEWPORTS` | `config.viewports` | JSON array |
| `VISUAL_ROUTES` | `config.routes` | JSON array |
| `VISUAL_UPLOAD_BUCKET` | `config.upload.bucket` | string |
| `VISUAL_UPLOAD_PREFIX` | `config.upload.path_prefix` | string |
| `VISUAL_DIFF_THRESHOLD_PCT` | `config.diff.threshold_pct` | float |
| `VISUAL_BASELINE_BUCKET` | `config.diff.baseline.bucket` | string, optional |
| `VISUAL_REPO` | gate context | `owner/name` |
| `VISUAL_SHA` | gate context | full SHA |

The template is the only place the runner-side implementation lives;
the compiler just exports the env vars and the template handles boot,
probe, shoot, diff, upload, cleanup. This keeps the in-VM work in one
reviewable file.

## Three-outcome verdict contract (unchanged)

- **success** — visual step exited 0, screenshots uploaded, no over-threshold diffs.
- **failure** — visual step exited non-zero (server boot, readiness
  probe, Playwright crash, or diff over threshold). Exit code is
  carried. If a prior real verdict exists for the SHA, it is preserved
  (no overwrite).
- **infra** — VM could not boot, snapshot preflight failed, or the
  shim transport itself failed. `waived-no-tooling` is NOT an infra
  outcome — it is a per-step skip marker. The overall gate verdict
  reflects the OTHER steps; the visual step neither passes nor fails.

## Migration plan

1. This spec lands (fabro `docs/internal/visual-qa-gate-spec.md`).
2. Compiler + template land in `ao-company` on branch
   `fabro-86-visual-qa` (stacked on `fabro-122-eagain-retryable-classification`).
3. `bash bin/fabro-github-gate.sh self-test` stays green.
4. Operator bakes `zen-gate-visual` on dellsrv (T3, out of scope here).
5. First project to opt in: a web project with a `runner: vm` lane
   that has never had visual coverage. The first run records
   `STEP_SKIP:<id>=waived-no-tooling` until the snapshot exists —
   exactly the existing honesty property.
6. After the snapshot is baked, the same descriptor produces real
   screenshots and diffs on every PR.

## Paragraph for `docs/GATE-ONBOARDING.md` (issue #129)

> **Visual QA is a first-class gate job type.** Add a `kind: visual`
> step to `.zp/qa-diamond.yaml` with `runner: vm` and a `config`
> block describing the dev server, the viewports, the routes, and the
> diff threshold. The gate compiles the step into a single in-VM
> shell fragment that boots the server, polls an HTTP readiness probe
> (no `sleep`), captures each route at each viewport with Playwright,
> optionally diffs against an S3 baseline, and uploads screenshots.
> The step needs a `zen-gate-visual` class snapshot (Chromium +
> Playwright provisioned on top of `zen-gate-big`); on any other
> snapshot the step records `STEP_SKIP:<id>=waived-no-tooling` and
> does not affect the gate verdict — the diamond stays honest. See
> `docs/internal/visual-qa-gate-spec.md` for the full schema, the
> env-var contract, the operator bake commands, and the migration plan.
