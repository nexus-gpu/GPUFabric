#!/usr/bin/env bash
set -euo pipefail

CONFIG_FILE="${GPUF_C_CONFIG:-$HOME/.gpuf-c.env}"
LOG_FILE="${GPUF_C_LOG:-$HOME/gpuf-c.log}"
PID_FILE="${GPUF_C_PID:-$HOME/.gpuf-c.pid}"

DEFAULT_CLIENT_ID="00112233445566778899aabbccddeeff"
DEFAULT_SERVER_ADDR="agent.gpunexus.com"
DEFAULT_CONTROL_PORT="17000"
DEFAULT_PROXY_PORT="17001"
DEFAULT_TLS_SERVER_NAME="agent.gpunexus.com"
DEFAULT_MODEL_PATH="/usr/local/share/gpuf-c/models/bge-m3-q8_0.gguf"
DEFAULT_CERT_CHAIN_PATH="/usr/local/bin/ca-cert.pem"
DEFAULT_BIN="/usr/local/bin/gpuf-c"

ensure_config() {
  if [[ -f "$CONFIG_FILE" ]]; then
    return
  fi

  {
    printf 'CLIENT_ID=%s\n' "$DEFAULT_CLIENT_ID"
    printf 'SERVER_ADDR=%s\n' "$DEFAULT_SERVER_ADDR"
    printf 'CONTROL_PORT=%s\n' "$DEFAULT_CONTROL_PORT"
    printf 'PROXY_PORT=%s\n' "$DEFAULT_PROXY_PORT"
    printf 'TLS_SERVER_NAME=%s\n' "$DEFAULT_TLS_SERVER_NAME"
    printf 'MODEL_PATH=%s\n' "$DEFAULT_MODEL_PATH"
    printf 'CERT_CHAIN_PATH=%s\n' "$DEFAULT_CERT_CHAIN_PATH"
    printf 'GPUF_C_BIN=%s\n' "$DEFAULT_BIN"
    printf 'RUST_LOG=%s\n' "gpuf_c=debug,common=info"
    printf 'N_GPU_LAYERS=%s\n' "99"
    printf 'N_CTX=%s\n' "2048"
    printf 'N_BATCH=%s\n' "512"
  } > "$CONFIG_FILE"
  chmod 600 "$CONFIG_FILE"
  echo "Created config: $CONFIG_FILE"
}

load_config() {
  ensure_config
  # shellcheck disable=SC1090
  source "$CONFIG_FILE"

  CLIENT_ID="${CLIENT_ID:-$DEFAULT_CLIENT_ID}"
  SERVER_ADDR="${SERVER_ADDR:-$DEFAULT_SERVER_ADDR}"
  CONTROL_PORT="${CONTROL_PORT:-$DEFAULT_CONTROL_PORT}"
  PROXY_PORT="${PROXY_PORT:-$DEFAULT_PROXY_PORT}"
  TLS_SERVER_NAME="${TLS_SERVER_NAME:-$DEFAULT_TLS_SERVER_NAME}"
  MODEL_PATH="${MODEL_PATH:-$DEFAULT_MODEL_PATH}"
  CERT_CHAIN_PATH="${CERT_CHAIN_PATH:-$DEFAULT_CERT_CHAIN_PATH}"
  GPUF_C_BIN="${GPUF_C_BIN:-$DEFAULT_BIN}"
  RUST_LOG="${RUST_LOG:-gpuf_c=debug,common=info}"
  N_GPU_LAYERS="${N_GPU_LAYERS:-99}"
  N_CTX="${N_CTX:-2048}"
  N_BATCH="${N_BATCH:-512}"
}

is_running() {
  [[ -f "$PID_FILE" ]] || return 1
  local pid
  pid="$(cat "$PID_FILE")"
  [[ -n "$pid" ]] || return 1
  kill -0 "$pid" >/dev/null 2>&1
}

start() {
  load_config

  if is_running; then
    echo "gpuf-c is already running: pid=$(cat "$PID_FILE")"
    return
  fi

  touch "$LOG_FILE"

  sudo nohup env RUST_LOG="$RUST_LOG" "$GPUF_C_BIN" \
    --client-id "$CLIENT_ID" \
    --server-addr "$SERVER_ADDR" \
    --control-port "$CONTROL_PORT" \
    --proxy-port "$PROXY_PORT" \
    --engine-type llama \
    --llama-model-path "$MODEL_PATH" \
    --n-gpu-layers "$N_GPU_LAYERS" \
    --n-ctx "$N_CTX" \
    --n-batch "$N_BATCH" \
    --control-tls \
    --control-tls-server-name "$TLS_SERVER_NAME" \
    --cert-chain-path "$CERT_CHAIN_PATH" \
    > "$LOG_FILE" 2>&1 &

  echo $! > "$PID_FILE"
  echo "Started gpuf-c: pid=$(cat "$PID_FILE")"
  echo "Log: $LOG_FILE"
}

stop() {
  if is_running; then
    local pid
    pid="$(cat "$PID_FILE")"
    sudo kill "$pid"
    rm -f "$PID_FILE"
    echo "Stopped gpuf-c: pid=$pid"
    return
  fi

  echo "gpuf-c is not running by pid file"
}

status() {
  load_config
  if is_running; then
    echo "gpuf-c running"
    echo "pid: $(cat "$PID_FILE")"
  else
    echo "gpuf-c not running"
  fi
  echo "client_id: $CLIENT_ID"
  echo "server: $SERVER_ADDR:$CONTROL_PORT/$PROXY_PORT"
  echo "model: $MODEL_PATH"
  echo "log: $LOG_FILE"
}

logs() {
  touch "$LOG_FILE"
  tail -f "$LOG_FILE"
}

set_client_id() {
  local client_id="${1:-}"
  if [[ -z "$client_id" ]]; then
    echo "Usage: $0 client-id <client_id>" >&2
    exit 2
  fi

  ensure_config
  if grep -q '^CLIENT_ID=' "$CONFIG_FILE"; then
    sed -i.bak "s/^CLIENT_ID=.*/CLIENT_ID=$client_id/" "$CONFIG_FILE"
  else
    printf '\nCLIENT_ID=%s\n' "$client_id" >> "$CONFIG_FILE"
  fi
  echo "Updated CLIENT_ID in $CONFIG_FILE"
}

usage() {
  printf 'Usage: %s <command>\n\n' "$0"
  printf 'Commands:\n'
  printf '  start                 Start gpuf-c in background\n'
  printf '  stop                  Stop gpuf-c by pid file\n'
  printf '  restart               Stop then start gpuf-c\n'
  printf '  status                Show process and config status\n'
  printf '  logs                  Follow log file\n'
  printf '  client-id <client_id> Update CLIENT_ID in %s\n\n' "$CONFIG_FILE"
  printf 'Config:\n'
  printf '  %s\n\n' "$CONFIG_FILE"
  printf 'Examples:\n'
  printf '  %s start\n' "$0"
  printf '  %s logs\n' "$0"
  printf '  %s client-id 1dd716bde4434f969d4f0577a170b1b8\n' "$0"
  printf '  %s restart\n' "$0"
}

cmd="${1:-}"
case "$cmd" in
  start)
    start
    ;;
  stop)
    stop
    ;;
  restart)
    stop || true
    start
    ;;
  status)
    status
    ;;
  logs)
    logs
    ;;
  client-id)
    shift
    set_client_id "${1:-}"
    ;;
  *)
    usage
    exit 2
    ;;
esac
