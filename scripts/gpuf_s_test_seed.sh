#!/bin/sh
set -eu

: "${POSTGRES_DB:=GPUFabric}"
: "${POSTGRES_USER:=postgres}"
: "${GPUF_TEST_GATEWAY_TOKEN:?GPUF_TEST_GATEWAY_TOKEN is required}"
: "${GPUF_TEST_CLIENT_ID_HEX:?GPUF_TEST_CLIENT_ID_HEX is required}"
: "${GPUF_TEST_USER_ID:=1}"

if [ "${#GPUF_TEST_GATEWAY_TOKEN}" -ne 48 ]; then
  echo "GPUF_TEST_GATEWAY_TOKEN must be exactly 48 characters for tokens.key" >&2
  exit 1
fi

case "$GPUF_TEST_CLIENT_ID_HEX" in
  *[!0123456789abcdefABCDEF]*|'')
    echo "GPUF_TEST_CLIENT_ID_HEX must be 32 hex characters" >&2
    exit 1
    ;;
esac

if [ "${#GPUF_TEST_CLIENT_ID_HEX}" -ne 32 ]; then
  echo "GPUF_TEST_CLIENT_ID_HEX must be 32 hex characters" >&2
  exit 1
fi

psql -v ON_ERROR_STOP=1 \
  --username "$POSTGRES_USER" \
  --dbname "$POSTGRES_DB" \
  --set test_token="$GPUF_TEST_GATEWAY_TOKEN" \
  --set test_client_hex="$GPUF_TEST_CLIENT_ID_HEX" \
  --set test_user_id="$GPUF_TEST_USER_ID" <<'EOSQL'
INSERT INTO tokens (user_id, key, status, expired_time, deleted_at, access_level)
VALUES (:'test_user_id'::bigint, :'test_token'::char(48), 1, -1, NULL, 1)
ON CONFLICT (key) DO UPDATE SET
    user_id = EXCLUDED.user_id,
    status = EXCLUDED.status,
    expired_time = EXCLUDED.expired_time,
    deleted_at = NULL,
    access_level = EXCLUDED.access_level;

INSERT INTO gpu_assets (
    user_id,
    client_id,
    client_name,
    client_status,
    valid_status,
    os_type,
    outo_set_model,
    model,
    updated_at
)
VALUES (
    :'test_user_id',
    decode(:'test_client_hex', 'hex'),
    'gpuf-s-test-worker',
    'offline',
    'valid',
    'linux',
    true,
    'qwen2.5-coder:3b',
    now()
)
ON CONFLICT (client_id) DO UPDATE SET
    user_id = EXCLUDED.user_id,
    client_name = EXCLUDED.client_name,
    valid_status = 'valid',
    os_type = EXCLUDED.os_type,
    model = EXCLUDED.model,
    updated_at = now();
EOSQL

echo "Seeded gpuf-s test token and client row."
