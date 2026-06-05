# GPUFabric GUI and Frontend Integration

This directory currently contains the PyQt statistics dashboard (`stats_dashboard.py`). The dashboard reads PostgreSQL directly for local operations, while browser or service frontends should use the REST API documented in `../docs/api_server.md`.

## API Defaults

- Management API base URL for local development: `http://127.0.0.1:18081`
- Standalone `api_server` bind default: `127.0.0.1`
- Public bind: use `--bind-addr 0.0.0.0` only behind a reverse proxy, firewall, TLS, and deployment access control
- Compatible endpoints: existing `/api/user/*`, `/api/models/*`, `/api/apk/*`, and `/api/user/points` paths are unchanged

For web frontends, configure the base URL through deployment config, for example `GPUFABRIC_API_BASE_URL` or `VITE_GPUFABRIC_API_BASE_URL`. Do not hardcode public IPs, long-lived tokens, database passwords, TURN credentials, or release signing keys into frontend code.

## Run The Local Dashboard

```bash
cd gui
python3 -m venv .venv
source .venv/bin/activate
pip install -r requirements.txt
python3 stats_dashboard.py
deactivate
```

If `requirements.txt` is not present, install the current dashboard dependencies first:

```bash
pip install matplotlib pyqt5 pandas psycopg2-binary
pip freeze > requirements.txt
```

## API Server For Frontend Development

```bash
cargo run --release -p gpuf-s --bin api_server -- \
  --bind-addr 127.0.0.1 \
  --port 18081 \
  --database-url "$DATABASE_URL" \
  --redis-url "redis://127.0.0.1:6379"
```

Use `docs/api_server.md` as the source of truth for response envelopes and request fields. The 2026-06-04 security remediation is frontend-compatible: it does not remove existing REST paths or native SDK function signatures, but it does require safer deployment defaults such as loopback binding, explicit server addresses, and SHA256-verified artifacts.

Worker onboarding over non-loopback networks should use gpuf-s/gpuf-c control TLS (`--control-tls` on both sides plus CA/SNI configuration). Mobile native wrappers can use `startRemoteWorkerWithTls`; this is separate from browser/front-end REST integration and does not change the management API contract.
