# Banking Admin API Deploy And Rollback Plan - 2026-07-05

## Scope

Deploy the `gpuf-s` standalone `api_server` binary for banking admin dashboard
changes:

- `/api/banking/admin/overview`
  - adds `offlineNodes`, `allNodes`, `onlineCompute`, `offlineCompute`,
    `allCompute`
  - adds `statusBreakdown.all`, `statusBreakdown.online`,
    `statusBreakdown.offline`
- `/api/banking/admin/network-map`
  - supports `nodeStatus=all|online|offline`
  - defaults to `all` when `nodeStatus` is absent
  - recalculates `cities`, `regions`, `highlightProvinces`, and `topCities`
    after filtering

The deployment target is `ssh pro`. On this host, `api_server` runs as a host
binary from `/home/ubuntu/v1.0.3/api_server`. This plan does not modify the
host `gpuf-s` process, PostgreSQL schema, Redis, Kafka, or frontend containers.

## Pre-Deploy Validation

Run locally:

```bash
cargo fmt --all --check
cargo test -p gpuf-s banking_admin
cargo check -p gpuf-s --bin api_server
cargo build -p gpuf-s --bin api_server --release --target x86_64-unknown-linux-musl
```

The musl build is required because `ssh pro` uses glibc 2.32 and binaries built
against newer host glibc may fail to start.

## Backup Plan

Create a timestamped release directory on `ssh pro`:

```text
/home/ubuntu/v1.0.3/api_server/releases/banking-admin-status-<timestamp>/
```

Save:

- current `/home/ubuntu/v1.0.3/api_server/api_server`
- current `start_api_server.sh`
- current process command line and working directory
- current binary sha256
- current `/api/banking/admin/overview` response
- current `/api/banking/admin/network-map` response for all/online/offline
- restart and rollback scripts

## Deploy Plan

1. Upload the new static `api_server` binary to the release directory.
2. Verify sha256 and `--help` on `ssh pro`.
3. Install it atomically via `install -m 0755`.
4. Stop the old `api_server` PID only.
5. Restart with the previous process argv from the previous working directory.
6. Validate:
   - process is alive
   - port `18081` is listening
   - `/overview` contains `statusBreakdown`, `offlineNodes`, `allNodes`,
     `onlineCompute`, `offlineCompute`, `allCompute`
   - `/network-map`, `/network-map?nodeStatus=online`, and
     `/network-map?nodeStatus=offline` return `code=0`

## Rollback Plan

Run the generated rollback script in the release directory:

```bash
ssh pro "/home/ubuntu/v1.0.3/api_server/releases/banking-admin-status-<timestamp>/rollback_api_server.sh"
```

Rollback behavior:

1. Reinstall the backed-up `api_server.before` binary.
2. Stop the current `api_server` PID only.
3. Restart with the saved original argv.
4. Validate `/api/banking/admin/overview`.

No database rollback is required because this change does not alter schema or
data.
