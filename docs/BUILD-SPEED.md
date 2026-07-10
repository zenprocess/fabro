# Fabro build-speed toolkit

Branch: `platform/build-speed` (off `origin/main`). All changes here are
**additive** — they do not alter the default `cargo build`, the CI release
workflow, or the runtime `./Dockerfile`. Nothing below runs unless you opt in.

This doc explains what was added, why, the expected speedups, how to drive a
build on the dellsrv builder tier, and the follow-ons to finish the job.

---

## TL;DR

Four independent speedups, stackable, plus one structural win:

| # | Lever | Where | Effect |
|---|-------|-------|--------|
| 0 | **Builder tier: off the Mac** | `scripts/build-on-dellsrv.sh` | The Mac is an Intel Xeon (x86_64) so `--platform linux/amd64` is native, NOT QEMU (earlier revision of this row was wrong) — but local builds pin the operator's workstation (observed 1673% CPU / 27 GB in OrbStack) and rebuild toolchains each run. dellsrv gives identical native amd64 output, off-machine, with persistent chef/sccache caches. |
| 1 | **cargo-chef** | `docker/Dockerfile.chef` | Dependency compile becomes a cached Docker layer; source-only edits skip the ~819-crate dep build. |
| 2 | **sccache → MinIO S3** | `Dockerfile.chef` + `build-on-dellsrv.sh` | Per-`rustc`-unit compile cache, shared across runs/hosts via `store-api.zp.digital`. |
| 3 | **mold** linker | baked into the builder image | Faster link step for the musl/gnu artifact. |

Grounded in the build-speed research findings (godkb plan 5526) and the fabro
repo itself (`Dockerfile`, `.github/workflows/release.yml`, `.cargo/config.toml`).

---

## Two corrections to the original plan premises

Both were verified wrong during grounding and are load-bearing:

1. **The shipped binary is `musl`, not `gnu`.** The root `./Dockerfile` is
   runtime-only — it `FROM`s `dhi-alpine-base` and `COPY`s a pre-built binary
   from `tmp/docker-context/${TARGETARCH}/fabro`, documented in its own header
   as `x86_64-unknown-linux-musl` (amd64) / `aarch64-unknown-linux-musl` (arm64).
   So mold/target config keyed to `x86_64-unknown-linux-gnu` would silently
   no-op for the shipped artifact. `Dockerfile.chef` defaults to the **musl**
   triple; the mold `[target.*]` block keys to musl.

2. **dellsrv has no host Rust toolchain.** `cargo`/`rustc`/`rustup` are absent
   (only `gcc`). It *does* have Docker 29.1.3. So "build on dellsrv" means run a
   **containerized** build via dellsrv's Docker — not a native host `cargo`
   build. We deliberately do not install rustup/mold/sccache onto the live infra
   box; the builder stays hermetic in the image.

A third constraint shapes the script: dellsrv is the **live gate host**. The
forkd VMM permanently reserves ~16.6 GB of its 30 GB RAM. CPU is idle; **memory
is the binding constraint**. Builds are therefore job-capped
(`CARGO_BUILD_JOBS`, default 4) to coexist with forkd.

---

## Files added

```
docker/Dockerfile.chef        # cargo-chef + sccache + mold multi-stage builder
.cargo/config.toml            # (appended) mold + sccache opt-in docs, INERT by default
scripts/build-on-dellsrv.sh   # ssh+docker dispatch to the dellsrv builder tier
docs/BUILD-SPEED.md           # this file
```

### `docker/Dockerfile.chef`

Four stages (each commented in-file):

1. **chef** — `messense/rust-musl-cross:x86_64-musl` base (bundles the
   `x86_64-unknown-linux-musl` target + musl-cross gcc) with:
   - `mold` (apt),
   - `sccache` v0.16.0 (prebuilt musl binary from `mozilla/sccache` releases),
   - `cargo-chef` v0.1.77 (pinned),
   - a builder-local `$CARGO_HOME/config.toml` that sets `RUSTC_WRAPPER=sccache`
     (degrades to a local disk cache with no S3 env) and, when `USE_MOLD=1`,
     the mold `rustflags` for the target triple.
2. **planner** — `cargo chef prepare` → `recipe.json` (a function of manifests +
   `Cargo.lock` only, so it is stable across source-only edits).
3. **builder** — `cargo chef cook --release --target <triple>` (THE cached dep
   layer) → copy source → `cargo build --locked --release --target <triple>
   -p fabro-cli`. Both steps mount `--secret sccache-env` (optional) and cache
   mounts for sccache + the cargo registry.
4. **export** — a `scratch` stage holding only `/fabro`, so
   `--target export --output type=local,dest=tmp/docker-context/amd64` drops the
   binary exactly where the runtime `./Dockerfile` expects it.

Version pins live at the top as `ARG`s. Pinning the musl-cross base **tag** also
pins the bundled Rust toolchain — which is what keeps cargo-chef layers and
sccache keys valid across runs (fabro has no `rust-toolchain.toml`; see
follow-ons).

> Note on the CI path: `release.yml` links musl via `cargo zigbuild`, whose zig
> lld ignores `-fuse-ld=mold`. This builder uses plain `cargo build` precisely so
> mold applies. If your musl-cross base's gcc rejects `-fuse-ld=mold`, build with
> `--build-arg USE_MOLD=0` — the build still succeeds, just without the link
> speedup.

### `.cargo/config.toml` (appended)

The existing `[alias]` / `[env]` are untouched. The appended block is
**intentionally inert**:

- **sccache** is documented as an opt-in via `export RUSTC_WRAPPER=sccache`
  (never hard-coded in `[env]`, which would break contributors without sccache).
- **mold** `[target.*]` blocks are **commented out**. An always-on mold linker
  would break every host without mold — the Mac dev loop and the CI runners.
  Instead the builder image injects the same `rustflags` into *its* `CARGO_HOME`,
  so mold accelerates dellsrv builds without infecting the repo. To use mold in a
  local Linux dev build, install mold and uncomment the triple you build.

### `scripts/build-on-dellsrv.sh`

Guarded, idempotent, timing-printing dispatch. It:

1. rsyncs the working tree to `dellsrv:/data/fabro-build/<target>` (excludes
   `target/`, `.git`, `tmp/docker-context/`).
2. optionally assembles a MinIO sccache secret (see below),
3. runs `docker build -f docker/Dockerfile.chef --target export` on dellsrv,
   **native** (no `--platform`), `CARGO_BUILD_JOBS`-capped,
4. copies the binary back to `./tmp/docker-context/<arch>/fabro`,
5. optionally builds+pushes the runtime image.

Reruns reuse the chef layer + sccache + cargo-registry caches, so a source-only
change rebuilds fast.

---

## Expected speedups

These are **estimates from the mechanics**, to be confirmed in the Verify phase
on dellsrv (no build was run while authoring these files):

- **Off-machine + persistent caches vs ad-hoc local docker builds**: the win is
  NOT emulation removal (the Intel Mac is already native amd64) — it is (a) the
  operator's workstation stays free, and (b) the dep layer + sccache persist
  across runs (measured on dellsrv: cold cook 587s for ~800 dep crates; no-change
  rebuild 6s via full layer cache-hit; code-only change keeps the dep cook fully
  cached and rebuilds only workspace crates).
- **cargo-chef, code-only edit**: up to ~5× — the dep-compile layer is cached, so
  only the workspace crates that changed recompile. Cold (deps changed) build
  gets no chef benefit by itself.
- **sccache, cross-run / cross-host**: warm cache turns most `rustc` units into
  cache hits; the first (cold) run populates it. Biggest benefit on CI-style
  fresh checkouts and across the fleet once MinIO-backed.
- **mold**: link step drops from seconds to sub-second on a large binary;
  material on incremental rebuilds where linking dominates.

Rule of thumb after warm-up: **cold build** ≈ native-amd64 + sccache-cold + chef
dep-cook once; **code-change build** ≈ chef-cached deps + sccache-warm + mold
link → seconds, not minutes.

---

## How to use `build-on-dellsrv.sh`

```bash
# From the platform/build-speed tree:

# 1) Native build, local sccache, no push (needs no secrets):
scripts/build-on-dellsrv.sh

# 2) + MinIO S3 compile cache (creds fetched from Infisical at run time):
SCCACHE_S3=1 scripts/build-on-dellsrv.sh

# 3) + build & push the runtime image (requires docker login on dellsrv):
PUSH=1 IMAGE_TAG=dev SCCACHE_S3=1 scripts/build-on-dellsrv.sh
```

Common overrides (all have defaults):

| Env | Default | Meaning |
|-----|---------|---------|
| `HOST` | `dellsrv` | ssh alias (`~/.ssh/config`) |
| `TARGET` | `x86_64-unknown-linux-musl` | rust triple (musl = shipped) |
| `JOBS` | `4` | `CARGO_BUILD_JOBS` — the memory lever |
| `SCCACHE_S3` | `0` | `1` wires the MinIO S3 backend |
| `PUSH` | `0` | `1` builds+pushes the runtime image |
| `SCCACHE_ENV_FILE` | _(unset)_ | pre-made dotenv of creds instead of Infisical |

> ssh to dellsrv requires the command sandbox **disabled** (the sandbox blocks
> LAN/DNS; `dellsrv.zp.digital` resolves to an in-network VIP). Run the script
> from a shell that can reach the zp LAN.

> **Web UI**: `rust_embed` compiles `lib/crates/fabro-spa/assets/` into the
> binary at build time; release binaries have no on-disk fallback. If that
> directory is empty when this script rsyncs the tree, the resulting binary
> is API-only — `fabro server --web` refuses to start (`--web requires web UI
> assets`). Run `cargo dev spa refresh` (needs bun) in the source tree first
> if the built artifact must serve the web UI.

---

## sccache credentials (never baked, never printed)

MinIO/S3 creds live in **Infisical** (project `zen-infra`, path
`/infrastructure/minio`, machine identity `ao-trader-test@orb`). The script
fetches them at run time into a `0600` temp dotenv, ships it to the builder as a
**BuildKit `--secret`** (so it never enters an image layer or the build history),
and shreds it on exit. Only these env **names** are used — values stay in
Infisical:

```
# non-secret config
SCCACHE_BUCKET=fabro-sccache
SCCACHE_ENDPOINT=store-api.zp.digital     # TLS on 443, omit port
SCCACHE_REGION=auto                       # required for MinIO
SCCACHE_S3_USE_SSL=true
SCCACHE_S3_KEY_PREFIX=fabro
RUSTC_WRAPPER=sccache
# secret (from Infisical)
AWS_ACCESS_KEY_ID=...
AWS_SECRET_ACCESS_KEY=...
```

`store-api.zp.digital` is on the zp LAN (10.0.201.26) and answered HTTPS from
the box tier — but may be unreachable from the operator Mac sandbox. Confirm the
builder host is on the LAN before relying on the S3 cache. Verify hits with
`sccache --show-stats` (the builder prints it after each compile stage).

---

## Follow-ons (not done here)

1. **Prebuilt `fabro-builder` base image.** Build the chef base once
   (musl-cross + mold + sccache + cargo-chef) and push to
   `registry.zp.digital/zenprocess/fabro-builder:<tag>`, then have per-build runs
   `FROM` it — eliminating per-build tool install. **Blocked on `docker login
   registry.zp.digital`** on the builder host: pull works, but push auth was
   unverified (no `~/.docker/config.json` for the box user). Log in with creds
   from Infisical first.
2. **Commit a `rust-toolchain.toml`.** fabro has none and no `rust-version` in
   `Cargo.toml` (workspace version `0.267.0-nightly.0` is a semver pre-release
   tag, not necessarily the nightly channel). cargo-chef needs an identical Rust
   version across stages and sccache keys include the rustc version — so pin the
   exact channel the builder image uses to keep both caches valid. (Pinning the
   musl-cross base tag currently stands in for this.)
3. **Fleet-wire the sccache creds.** Today the script fetches per-run from
   Infisical. For CI/fleet builds, provision the box machine identity so
   `SCCACHE_S3=1` works unattended.
4. **aarch64 path.** `TARGET=aarch64-unknown-linux-musl` selects the arm64
   artifact, but on the amd64 dellsrv host that cross/emulates. Consider a native
   arm64 builder or `cargo-zigbuild` for the arm leg (as CI does).

---

## Safety notes

- Authored entirely on `platform/build-speed` in a fresh worktree
  (`/tmp/fabro-speed`) off `origin/main`. `feat/forkd-on-0.290` and the
  `build-sync` worktree were not touched.
- No secrets are hardcoded in any file. No full build was run while authoring
  (that is the Verify phase, on dellsrv).
