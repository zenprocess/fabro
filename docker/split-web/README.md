# Split Web Compose PoC

This Compose stack proves that Fabro can serve the React SPA from a separate
static process while the Rust server remains the API and browser-auth origin.

Request ownership:

- `/api/*` -> `fabro-api:32276`
- `/auth/*` -> `fabro-api:32276`
- `/health` -> `fabro-api:32276`
- everything else -> `fabro-web:80`

The Rust server still contains bundled SPA assets. In this PoC they are simply
not reachable through the `edge` service for normal web paths.

The edge proxy adds `X-Fabro-PoC-Upstream` to responses so manual checks can
confirm which service handled a request.

## Run

Build the local Fabro image from the current tree:

```sh
cargo dev docker-build --tag fabro-sh/fabro:split-web-poc
```

Set local auth secrets:

```sh
export SESSION_SECRET="$(openssl rand -hex 32)"
export FABRO_DEV_TOKEN="fabro_dev_$(openssl rand -hex 32)"
```

Start the split stack:

```sh
docker compose -f docker-compose.split-web.yaml up
```

Open http://localhost:8080.

Use `SPLIT_WEB_PORT` to expose a different local port, or `FABRO_IMAGE` to use
a different API image.

## Docker Workers

The API service mounts `/var/run/docker.sock` so it can create sibling worker
containers on the host Docker daemon. Those workers join the stable
`fabro-split-web` network and call the API at `http://fabro-api:32276`.

Worker containers receive their config and scoped secrets from the API bootstrap
endpoint. They do not mount the server storage volume or `/config/settings.toml`.

This PoC sets `remove_on_exit = false` so QA can inspect a stopped worker
container after a run. Production examples should omit that setting and keep the
default `true`, because retained worker files include the worker-local bootstrap
config and vault.

## Validate

```sh
curl -i http://localhost:8080/health
curl -i http://localhost:8080/api/v1/health
curl -I http://localhost:8080/runs
curl -I http://localhost:8080/assets/app.css

curl -c /tmp/fabro.cookies \
  -H "content-type: application/json" \
  -d "{\"token\":\"$FABRO_DEV_TOKEN\"}" \
  http://localhost:8080/auth/login/dev-token

curl -b /tmp/fabro.cookies http://localhost:8080/api/v1/auth/me
```

Expected results:

- `/runs` and `/assets/*` are served by the static `fabro-web` container.
- `/api/*`, `/auth/*`, and `/health` are served by the Rust `fabro-api`
  container through the same browser origin.
- Dev-token login sets a same-origin session cookie, and
  `/api/v1/auth/me` accepts it.

After starting a workflow run through the UI or CLI, verify that the API created
a sibling worker container:

```sh
docker ps -a \
  --filter label=sh.fabro.managed=true \
  --filter label=sh.fabro.role=worker \
  --format 'table {{.Names}}\t{{.Status}}\t{{.Labels}}'
```

Expected evidence:

- A `fabro-worker-<run-id>-<suffix>` container exists.
- The container has `sh.fabro.role=worker` and `sh.fabro.run_id=<run-id>`
  labels.
- With `remove_on_exit = false`, exited workers remain visible for inspection.
