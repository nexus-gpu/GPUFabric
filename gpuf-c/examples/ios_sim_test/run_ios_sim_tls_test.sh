#!/bin/bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CERT_DIR="${GPUF_IOS_TEST_TLS_CERT_DIR:-$SCRIPT_DIR/tls_test_certs}"
SERVER_NAME="${GPUF_IOS_TEST_TLS_SERVER_NAME:-localhost}"

mkdir -p "$CERT_DIR"

CA_KEY="$CERT_DIR/ca-key.pem"
CA_CERT="$CERT_DIR/ca-cert.pem"
SERVER_KEY="$CERT_DIR/server-key.pem"
SERVER_CSR="$CERT_DIR/server.csr"
SERVER_CERT="$CERT_DIR/server-cert.pem"
SERVER_EXT="$CERT_DIR/server.ext"

if ! command -v openssl >/dev/null 2>&1; then
  echo "❌ openssl not found"
  exit 1
fi

if [ ! -f "$CA_CERT" ] || [ ! -f "$CA_KEY" ]; then
  openssl req \
    -x509 \
    -newkey rsa:2048 \
    -nodes \
    -sha256 \
    -days 30 \
    -subj "/CN=GPUFabric iOS TLS Test CA" \
    -keyout "$CA_KEY" \
    -out "$CA_CERT" >/dev/null 2>&1
fi

cat >"$SERVER_EXT" <<EOF
subjectAltName=DNS:$SERVER_NAME,IP:127.0.0.1
extendedKeyUsage=serverAuth
EOF

openssl req \
  -newkey rsa:2048 \
  -nodes \
  -sha256 \
  -subj "/CN=$SERVER_NAME" \
  -keyout "$SERVER_KEY" \
  -out "$SERVER_CSR" >/dev/null 2>&1

openssl x509 \
  -req \
  -in "$SERVER_CSR" \
  -CA "$CA_CERT" \
  -CAkey "$CA_KEY" \
  -CAcreateserial \
  -out "$SERVER_CERT" \
  -days 30 \
  -sha256 \
  -extfile "$SERVER_EXT" >/dev/null 2>&1

echo "🔐 TLS test certs ready:"
echo "   CA:     $CA_CERT"
echo "   Server: $SERVER_CERT"

export GPUF_IOS_TEST_TLS=1
export GPUF_IOS_TEST_SERVER_ADDR="${GPUF_IOS_TEST_SERVER_ADDR:-127.0.0.1}"
export GPUF_IOS_TEST_TLS_SERVER_NAME="$SERVER_NAME"
export GPUF_IOS_TEST_CA_CERT_SOURCE_PATH="$CA_CERT"

exec "$SCRIPT_DIR/run_ios_sim_test.sh"
