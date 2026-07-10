#!/usr/bin/env bash
# =============================================================================
# build-on-dellsrv.sh — dispatch the accelerated Fabro build to the dellsrv
#                        Docker builder tier.
# =============================================================================
#
# WHY THIS EXISTS (grounded in the build-speed findings)
#   * dellsrv is NATIVE amd64 Linux. The Mac worker compiles under
#     `docker run --platform linux/amd64` on OrbStack = QEMU emulation, a large
#     hidden cost. Running the identical build natively on dellsrv is the single
#     biggest, zero-code win — this script drops the emulation.
#   * dellsrv has Docker 29.1.3 but NO host cargo/rustc. So we build inside a
#     container (docker/Dockerfile.chef), NOT with a native host toolchain. We
#     deliberately do not install rustup/mold/sccache onto the live infra box.
#   * dellsrv is the LIVE gate host: the forkd VMM permanently reserves ~16.6 GB
#     of its 30 GB RAM. CPU is idle; MEMORY is the binding constraint. So the
#     build is hard-capped (--memory, CARGO_BUILD_JOBS) to coexist with forkd.
#
# WHAT IT DOES
#   1. rsync the working tree to dellsrv (excludes target/ and .git).
#   2. Optionally fetch MinIO/sccache creds from Infisical and hand them to the
#      build as a BuildKit --secret (NEVER baked into the image or printed).
#   3. `docker build` docker/Dockerfile.chef on dellsrv, memory/job capped,
#      exporting the musl binary.
#   4. Copy the binary back to ./tmp/docker-context/<arch>/fabro (where the
#      runtime ./Dockerfile expects it).
#   5. Optionally build+push the runtime image to registry.zp.digital.
#   Prints wall-clock timing. Idempotent: reruns reuse the chef layer + sccache
#   + cargo-registry caches on the daemon.
#
# SECRETS
#   MinIO creds live in Infisical (project zen-infra, path /infrastructure/minio,
#   machine identity ao-trader-test@orb). They are fetched at RUN TIME into a
#   0600 temp dotenv, shipped to the builder as a BuildKit secret, and shredded.
#   Nothing secret is ever echoed, baked into a layer, or written to the image
#   history. S3 caching is OFF by default (SCCACHE_S3=0) so the script runs with
#   zero secrets, using a local sccache cache only.
#
# USAGE
#   scripts/build-on-dellsrv.sh                 # native build, local sccache, no push
#   SCCACHE_S3=1 scripts/build-on-dellsrv.sh    # + MinIO S3 compile cache
#   PUSH=1 IMAGE_TAG=dev scripts/build-on-dellsrv.sh   # + build & push runtime image
#
# OVERRIDABLE ENV (defaults shown)
#   HOST=dellsrv                    ssh alias (see ~/.ssh/config)
#   TARGET=x86_64-unknown-linux-musl   rust triple (musl = shipped artifact)
#   MEMORY=6g                       hard memory cap so we coexist with forkd
#   JOBS=4                          CARGO_BUILD_JOBS (memory-bound, not CPU)
#   SCCACHE_S3=0                    1 = wire MinIO S3 sccache backend
#   PUSH=0                          1 = build+push runtime image after compile
#   REGISTRY=registry.zp.digital/zenprocess/fabro
#   IMAGE_TAG=build-speed-<shortsha>
#   SCCACHE_ENV_FILE=<unset>        pre-made dotenv to use instead of Infisical
#   INFISICAL_PROJECT=zen-infra   INFISICAL_PATH=/infrastructure/minio
#   SCCACHE_BUCKET=fabro-sccache  SCCACHE_ENDPOINT=store-api.zp.digital
# =============================================================================
set -euo pipefail

# ------------------------------- configuration -------------------------------
HOST="${HOST:-dellsrv}"
TARGET="${TARGET:-x86_64-unknown-linux-musl}"
MEMORY="${MEMORY:-6g}"
JOBS="${JOBS:-4}"
SCCACHE_S3="${SCCACHE_S3:-0}"
PUSH="${PUSH:-0}"
REGISTRY="${REGISTRY:-registry.zp.digital/zenprocess/fabro}"

# sccache/MinIO S3 knobs (values non-secret; creds fetched separately).
SCCACHE_BUCKET="${SCCACHE_BUCKET:-fabro-sccache}"
SCCACHE_ENDPOINT="${SCCACHE_ENDPOINT:-store-api.zp.digital}"
SCCACHE_REGION="${SCCACHE_REGION:-auto}"
SCCACHE_S3_KEY_PREFIX="${SCCACHE_S3_KEY_PREFIX:-fabro}"
INFISICAL_PROJECT="${INFISICAL_PROJECT:-zen-infra}"
INFISICAL_PATH="${INFISICAL_PATH:-/infrastructure/minio}"

# derive arch (amd64/arm64) from the rust triple for the docker-context path.
case "$TARGET" in
  x86_64-*)  ARCH="amd64" ;;
  aarch64-*) ARCH="arm64" ;;
  *) echo "ERROR: unsupported TARGET '$TARGET'" >&2; exit 2 ;;
esac

# repo root = parent of this script's dir (works from any CWD).
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
SHORT_SHA="$(git -C "$REPO_ROOT" rev-parse --short HEAD 2>/dev/null || echo nogit)"
IMAGE_TAG="${IMAGE_TAG:-build-speed-$SHORT_SHA}"

REMOTE_DIR="/data/fabro-build/${TARGET}"   # /data has 1.4T free on dellsrv
REMOTE_SECRET=""                            # set later if S3 enabled

log()  { printf '\033[1;34m[build-on-dellsrv]\033[0m %s\n' "$*"; }
die()  { printf '\033[1;31m[build-on-dellsrv] ERROR:\033[0m %s\n' "$*" >&2; exit 1; }

# ------------------------------ preflight guards -----------------------------
[ -f "$REPO_ROOT/docker/Dockerfile.chef" ] || die "docker/Dockerfile.chef not found (run from the platform/build-speed tree)"
command -v ssh   >/dev/null || die "ssh not on PATH"
command -v rsync >/dev/null || die "rsync not on PATH"

log "checking ssh + docker on '$HOST' ..."
ssh -o BatchMode=yes -o ConnectTimeout=10 "$HOST" 'docker version --format "{{.Server.Version}}"' \
  >/dev/null 2>&1 || die "cannot reach docker on '$HOST' (is the ssh sandbox disabled? is dellsrv on the LAN?)"

# ----------------------- clean up remote artifacts on exit -------------------
cleanup() {
  # shred any secret file we shipped; never leave creds on the box.
  if [ -n "$REMOTE_SECRET" ]; then
    ssh "$HOST" "shred -u '$REMOTE_SECRET' 2>/dev/null || rm -f '$REMOTE_SECRET'" || true
  fi
}
trap cleanup EXIT

# ------------------------------ sync the source ------------------------------
log "syncing working tree -> $HOST:$REMOTE_DIR (excluding target/, .git) ..."
ssh "$HOST" "mkdir -p '$REMOTE_DIR'"
rsync -a --delete \
  --exclude '.git/' --exclude 'target/' --exclude 'tmp/docker-context/' \
  --exclude 'node_modules/' \
  "$REPO_ROOT"/ "$HOST:$REMOTE_DIR/"

# --------------------- optional: wire MinIO sccache secret -------------------
if [ "$SCCACHE_S3" = "1" ]; then
  log "SCCACHE_S3=1 -> assembling MinIO sccache secret (values never printed) ..."
  LOCAL_ENV="$(mktemp -t sccache.env.XXXXXX)"
  chmod 600 "$LOCAL_ENV"
  # shellcheck disable=SC2064
  trap "rm -f '$LOCAL_ENV'; cleanup" EXIT

  # Non-secret sccache config lines.
  {
    echo "RUSTC_WRAPPER=sccache"
    echo "SCCACHE_BUCKET=$SCCACHE_BUCKET"
    echo "SCCACHE_ENDPOINT=$SCCACHE_ENDPOINT"
    echo "SCCACHE_REGION=$SCCACHE_REGION"
    echo "SCCACHE_S3_USE_SSL=true"
    echo "SCCACHE_S3_KEY_PREFIX=$SCCACHE_S3_KEY_PREFIX"
  } >> "$LOCAL_ENV"

  # Credentials: either a pre-made dotenv (SCCACHE_ENV_FILE) or Infisical.
  if [ -n "${SCCACHE_ENV_FILE:-}" ]; then
    [ -f "$SCCACHE_ENV_FILE" ] || die "SCCACHE_ENV_FILE '$SCCACHE_ENV_FILE' not found"
    cat "$SCCACHE_ENV_FILE" >> "$LOCAL_ENV"
  elif command -v infisical >/dev/null 2>&1; then
    # Fetch AWS_ACCESS_KEY_ID / AWS_SECRET_ACCESS_KEY as dotenv. Adjust the
    # export flags to your Infisical CLI/auth setup; creds are appended without
    # ever being echoed to the terminal.
    infisical export --format=dotenv \
      --projectId "$INFISICAL_PROJECT" --env prod --path "$INFISICAL_PATH" \
      2>/dev/null | grep -E '^AWS_(ACCESS_KEY_ID|SECRET_ACCESS_KEY)=' >> "$LOCAL_ENV" \
      || die "infisical export failed — supply creds via SCCACHE_ENV_FILE instead"
  else
    die "S3 cache requested but no infisical CLI and no SCCACHE_ENV_FILE — refusing to build without creds"
  fi

  grep -q '^AWS_ACCESS_KEY_ID=' "$LOCAL_ENV" || die "no AWS_ACCESS_KEY_ID resolved for sccache S3"

  REMOTE_SECRET="$REMOTE_DIR/.sccache.env"
  scp -q "$LOCAL_ENV" "$HOST:$REMOTE_SECRET"
  ssh "$HOST" "chmod 600 '$REMOTE_SECRET'"
  rm -f "$LOCAL_ENV"
  log "MinIO sccache backend wired via BuildKit secret."
else
  log "SCCACHE_S3=0 -> using sccache LOCAL disk cache only (no secrets needed)."
fi

# --------------------------------- build -------------------------------------
# Native amd64 (NO --platform, so no OrbStack/QEMU emulation). Peak memory is
# bounded by CARGO_BUILD_JOBS (the real lever on this RAM-bound box) rather than
# `docker build --memory`, which BuildKit does not enforce per RUN step. The
# MEMORY var is advisory; operators needing a hard cgroup cap should run
# buildkitd with a memory limit or use the legacy builder (loses cache mounts).
log "building on $HOST: target=$TARGET arch=$ARCH jobs=$JOBS (advisory mem cap $MEMORY) ..."
BUILD_START=$(date +%s)

# REMOTE_SECRET is "" when SCCACHE_S3=0; the remote script adds --secret only if set.
ssh "$HOST" bash -s -- \
  "$REMOTE_DIR" "$TARGET" "$ARCH" "$JOBS" "${REMOTE_SECRET:-}" <<'REMOTE'
set -euo pipefail
REMOTE_DIR="$1"; TARGET="$2"; ARCH="$3"; JOBS="$4"; REMOTE_SECRET="${5:-}"
cd "$REMOTE_DIR"
export DOCKER_BUILDKIT=1
SEC=()
[ -n "$REMOTE_SECRET" ] && SEC=(--secret "id=sccache-env,src=$REMOTE_SECRET")
docker build \
  -f docker/Dockerfile.chef \
  --target export \
  --build-arg TARGET="$TARGET" \
  --build-arg CARGO_BUILD_JOBS="$JOBS" \
  "${SEC[@]}" \
  --output "type=local,dest=tmp/docker-context/$ARCH" \
  .
test -f "tmp/docker-context/$ARCH/fabro" || { echo "ERROR: binary not produced" >&2; exit 1; }
ls -la "tmp/docker-context/$ARCH/fabro"
REMOTE

BUILD_END=$(date +%s)
log "compile finished in $((BUILD_END - BUILD_START))s."

# --------------------------- pull the binary back ----------------------------
log "fetching binary -> $REPO_ROOT/tmp/docker-context/$ARCH/fabro ..."
mkdir -p "$REPO_ROOT/tmp/docker-context/$ARCH"
scp -q "$HOST:$REMOTE_DIR/tmp/docker-context/$ARCH/fabro" \
       "$REPO_ROOT/tmp/docker-context/$ARCH/fabro"
chmod +x "$REPO_ROOT/tmp/docker-context/$ARCH/fabro"

# --------------------- optional: build + push runtime image ------------------
if [ "$PUSH" = "1" ]; then
  log "building runtime image $REGISTRY:$IMAGE_TAG on $HOST and pushing ..."
  # NOTE: requires `docker login registry.zp.digital` on $HOST first (creds via
  # Infisical). Push auth was UNVERIFIED in findings — this will fail loudly if
  # the box is not logged in.
  ssh "$HOST" bash -s -- "$REMOTE_DIR" "$ARCH" "$REGISTRY:$IMAGE_TAG" <<'REMOTE'
set -euo pipefail
REMOTE_DIR="$1"; ARCH="$2"; IMAGE="$3"
cd "$REMOTE_DIR"
export DOCKER_BUILDKIT=1
# Runtime ./Dockerfile COPYs tmp/docker-context/${TARGETARCH}/fabro.
docker build --platform "linux/$ARCH" --build-arg TARGETARCH="$ARCH" -t "$IMAGE" .
docker push "$IMAGE"
REMOTE
  log "pushed $REGISTRY:$IMAGE_TAG"
else
  log "PUSH=0 -> skipping runtime image build/push."
fi

TOTAL_END=$(date +%s)
log "DONE. total wall-clock: $((TOTAL_END - BUILD_START))s. binary: tmp/docker-context/$ARCH/fabro"
