# gpuf-s Test Environment Runbook

This runbook deploys an isolated local test stack for `gpuf-s`, PostgreSQL,
Redis, Kafka/ZooKeeper, the management API, and `heartbeat_consumer`.

It does not reuse the default `GPUFabric` compose project, container names,
network, ports, or volumes. This keeps rollback simple and avoids touching an
already-running stack.

## Layout

- Compose file: `docker/gpuf_s_test_compose.yaml`
- Local env file: `docker/.env.gpuf-s-test`
- Env template: `docker/gpuf_s_test.env.example`
- DB schema: `scripts/db.sql`
- DB points migration: `scripts/device_points_daily_incremental.sql`
- Test seed: `scripts/gpuf_s_test_seed.sh`
- TLS files: `docker/cert.pem`, `docker/key.pem`, `docker/ca-cert.pem`
- Backup directory: `backups/gpuf-s-test/<timestamp>/`

Default local ports:

| Service | Host port |
|---|---:|
| gpuf-s control TLS | 17100 |
| gpuf-s proxy | 17101 |
| gpuf-s public | 18180 |
| api-server | 18181 |
| inference gateway | 18182 |
| PostgreSQL | 15432 |
| Redis | 16379 |
| ZooKeeper | 12181 |
| Kafka host listener | 39092 |

## Preflight

```bash
git status --short
docker compose version
docker ps --format 'table {{.Names}}\t{{.Status}}\t{{.Ports}}'
ss -ltnp
```

Confirm the default test ports above are free. If any port is occupied, edit the
matching variable in `docker/.env.gpuf-s-test`.

## Configure Secrets

```bash
cp docker/gpuf_s_test.env.example docker/.env.gpuf-s-test
```

Edit `docker/.env.gpuf-s-test` before shared use:

- `GPUF_TEST_POSTGRES_PASSWORD`: unique test DB password.
- `GPUF_TEST_GATEWAY_TOKEN`: exactly 48 characters, used as Bearer token for
  the inference gateway seed row.
- `GPUF_TEST_CLIENT_ID_HEX`: 32 hex characters, used by `gpuf-c --client-id`.

Do not commit `docker/.env.gpuf-s-test`. The committed compose uses a dummy
`--api-key` value for the legacy gpuf-s public listener; the gateway bearer token
is seeded into PostgreSQL instead of being placed in `gpuf-s` argv.

## TLS Certificates

For a local-only test stack, generate disposable certs:

```bash
cd docker
bash ../scripts/create_cert.sh
cd ..
```

Production or shared test hosts must replace these files with environment
specific certificates. Do not commit PEM files.

## Backup Before Deploy

Use a timestamped directory. This captures the current git revision, compose
configuration, env template, existing default DB if present, and existing test DB
if present.

```bash
TS="$(date +%Y%m%d-%H%M%S)"
BK="backups/gpuf-s-test/$TS"
mkdir -p "$BK"

git rev-parse HEAD > "$BK/git-revision.txt"
cp docker/gpuf_s_test_compose.yaml "$BK/"
cp docker/gpuf_s_test.env.example "$BK/"

docker exec gpuf-postgres pg_dump -U postgres -d GPUFabric \
  > "$BK/default-gpuf-postgres-before.sql" || true

docker exec gpuf-s-test-postgres pg_dump -U postgres -d GPUFabric \
  > "$BK/test-postgres-before.sql" || true
```

## Build Images

If local network proxy is needed during Docker build, use the host-reachable
proxy address rather than `127.0.0.1` inside the build container:

```bash
env -u http_proxy -u https_proxy -u all_proxy -u HTTP_PROXY -u HTTPS_PROXY -u ALL_PROXY \
  docker compose --env-file docker/.env.gpuf-s-test \
  -f docker/gpuf_s_test_compose.yaml build \
  --build-arg HTTP_PROXY=http://172.17.0.1:7897 \
  --build-arg HTTPS_PROXY=http://172.17.0.1:7897 \
  --build-arg http_proxy=http://172.17.0.1:7897 \
  --build-arg https_proxy=http://172.17.0.1:7897 \
  gpuf-s api-server heartbeat-consumer
```

If no proxy is needed:

```bash
docker compose --env-file docker/.env.gpuf-s-test \
  -f docker/gpuf_s_test_compose.yaml build gpuf-s api-server heartbeat-consumer
```

## Deploy

```bash
docker compose --env-file docker/.env.gpuf-s-test \
  -f docker/gpuf_s_test_compose.yaml up -d
```

Check status:

```bash
docker compose --env-file docker/.env.gpuf-s-test \
  -f docker/gpuf_s_test_compose.yaml ps

docker logs --tail 100 gpuf-s-test
docker logs --tail 100 gpuf-s-test-api-server
docker logs --tail 100 gpuf-s-test-heartbeat-consumer
```

Expected `gpuf-s-test` log:

```text
gpuf-server listening on ports: Control=17000 (tls=true), Proxy=17001, Public=18080, API=18081, InferenceGateway=8081
Connected to database successfully
Connected to Redis successfully
Inference Gateway listening on port 8081
```

## Smoke Tests

API health:

```bash
curl -fsS http://127.0.0.1:18181/api/models/get
```

Kafka topics:

```bash
docker exec gpuf-s-test-kafka kafka-topics \
  --bootstrap-server kafka:29092 \
  --list
```

Expected topics include `client-heartbeats` and `request-message`.

Control TLS with local CA:

```bash
openssl s_client -connect 127.0.0.1:17100 \
  -servername localhost \
  -CAfile docker/ca-cert.pem \
  -verify_return_error \
  -verify_hostname localhost </dev/null
```

Run `gpuf-c` against the test stack:

```bash
target/debug/gpuf-c \
  --client-id "$GPUF_TEST_CLIENT_ID_HEX" \
  --server-addr 127.0.0.1 \
  --control-port 17100 \
  --proxy-port 17101 \
  --local-addr 127.0.0.1 \
  --local-port 11434 \
  --worker-type tcp \
  --engine-type ollama \
  --cert-chain-path docker/ca-cert.pem \
  --control-tls \
  --control-tls-server-name localhost
```

Inference gateway request:

```bash
curl -sS -i http://127.0.0.1:18182/v1/completions \
  -H "Authorization: Bearer $GPUF_TEST_GATEWAY_TOKEN" \
  -H "Content-Type: application/json" \
  -H "x-target-client-id: $GPUF_TEST_CLIENT_ID_HEX" \
  -d '{"model":"qwen2.5-coder:3b","prompt":"Reply only: OK","max_tokens":8,"temperature":0.1,"stream":false}'
```

## Backup After Deploy

```bash
TS="$(date +%Y%m%d-%H%M%S)"
BK="backups/gpuf-s-test/$TS"
mkdir -p "$BK"

git rev-parse HEAD > "$BK/git-revision.txt"
cp docker/gpuf_s_test_compose.yaml "$BK/"
cp docker/gpuf_s_test.env.example "$BK/"

docker exec gpuf-s-test-postgres pg_dump -U postgres -d GPUFabric \
  > "$BK/test-postgres-after.sql"

docker compose --env-file docker/.env.gpuf-s-test \
  -f docker/gpuf_s_test_compose.yaml config > "$BK/compose-rendered.yaml"

docker ps --filter 'name=gpuf-s-test' \
  --format 'table {{.Names}}\t{{.Status}}\t{{.Ports}}' > "$BK/container-status.txt"

docker exec gpuf-s-test-kafka kafka-topics \
  --bootstrap-server kafka:29092 \
  --list > "$BK/kafka-topics.txt"

chmod -R go-rwx "$BK"
```

## Rollback

For a failed test deployment, stop and remove only the test stack:

```bash
docker compose --env-file docker/.env.gpuf-s-test \
  -f docker/gpuf_s_test_compose.yaml down
```

To discard test data and start from a clean DB:

```bash
docker compose --env-file docker/.env.gpuf-s-test \
  -f docker/gpuf_s_test_compose.yaml down -v
```

To restore a previous test DB dump:

```bash
docker compose --env-file docker/.env.gpuf-s-test \
  -f docker/gpuf_s_test_compose.yaml up -d postgres

cat backups/gpuf-s-test/<timestamp>/<dump>.sql | \
  docker exec -i gpuf-s-test-postgres psql -U postgres -d GPUFabric
```

The default running stack uses separate names such as `gpuf-postgres`,
`gpuf-redis`, `gpuf-kafka`, and `frpx-network`. Do not run `down -v` against
`docker/gpuf_s_compose.yaml` during test rollback unless the explicit goal is to
destroy the default stack data.
