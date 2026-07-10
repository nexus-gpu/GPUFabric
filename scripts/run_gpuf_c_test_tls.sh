#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# Test gpuf-s defaults.
SERVER_ADDR="${SERVER_ADDR:-8.140.251.142}"
CONTROL_PORT="${CONTROL_PORT:-17000}"
PROXY_PORT="${PROXY_PORT:-17001}"
TLS_SERVER_NAME="${TLS_SERVER_NAME:-gpuf-test.local}"
CLIENT_ID="${CLIENT_ID:-00112233445566778899aabbccddeeff}"
TEST_SSH_HOST="${TEST_SSH_HOST:-test}"
REMOTE_CA_PATH="${REMOTE_CA_PATH:-/home/gpuf-s/ca-cert.pem}"
CA_PATH="${CA_PATH:-/private/tmp/gpuf-test-tls-ca.pem}"
CA_URL="${CA_URL:-https://bucket-gpunexus-com.oss-cn-beijing.aliyuncs.com/client/test-ca-cert.pem}"

# Default model is the embedding model used to validate /v1/embeddings.
# Override MODEL_URL/MODEL_PATH to run another GGUF.
MODEL_DIR="${MODEL_DIR:-$ROOT_DIR/models}"
MODEL_NAME="${MODEL_NAME:-bge-m3-q8_0.gguf}"
MODEL_PATH="${MODEL_PATH:-$MODEL_DIR/$MODEL_NAME}"
MODEL_URL="${MODEL_URL:-https://modelscope.cn/models/OllmOne/bge-m3-GGUF/resolve/master/bge-m3-q8_0.gguf}"

BINARY="${BINARY:-$ROOT_DIR/target/release/gpuf-c}"

mkdir -p "$MODEL_DIR"

if [ ! -x "$BINARY" ]; then
  echo "gpuf-c binary not found: $BINARY"
  echo "Build it first:"
  echo "  cargo build -p gpuf-c --release --no-default-features --features metal"
  exit 1
fi

if [ ! -f "$MODEL_PATH" ]; then
  echo "Downloading model:"
  echo "  $MODEL_URL"
  echo "-> $MODEL_PATH"
  curl -L --fail --progress-bar -o "$MODEL_PATH" "$MODEL_URL"
else
  echo "Model exists: $MODEL_PATH"
fi

if [ -n "$CA_URL" ]; then
  echo "Downloading test CA:"
  echo "  $CA_URL"
  echo "-> $CA_PATH"
  curl -L --fail --progress-bar -o "$CA_PATH" "$CA_URL"
else
  echo "Fetching test CA:"
  echo "  $TEST_SSH_HOST:$REMOTE_CA_PATH"
  echo "-> $CA_PATH"
  scp "$TEST_SSH_HOST:$REMOTE_CA_PATH" "$CA_PATH"
fi

echo
echo "Starting gpuf-c against test TLS gpuf-s..."
echo "Server: $SERVER_ADDR:$CONTROL_PORT/$PROXY_PORT"
echo "TLS server name: $TLS_SERVER_NAME"
echo "CA: $CA_PATH"
echo "Model: $MODEL_PATH"
echo

exec env RUST_LOG="${RUST_LOG:-gpuf_c=debug,common=info}" "$BINARY" \
  --client-id "$CLIENT_ID" \
  --server-addr "$SERVER_ADDR" \
  --control-port "$CONTROL_PORT" \
  --proxy-port "$PROXY_PORT" \
  --engine-type llama \
  --llama-model-path "$MODEL_PATH" \
  --n-gpu-layers "${N_GPU_LAYERS:-99}" \
  --n-ctx "${N_CTX:-2048}" \
  --n-batch "${N_BATCH:-512}" \
  --control-tls \
  --control-tls-server-name "$TLS_SERVER_NAME" \
  --cert-chain-path "$CA_PATH"
