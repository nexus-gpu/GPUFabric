#!/usr/bin/env bash
set -euo pipefail

usage() {
    cat <<'EOF'
Usage: scripts/create_mobile_tls_cert.sh [options]

Create a local test CA and a gpuf-s server certificate with OpenSSL.

Generated files:
  ca-cert.pem       Trust bundle for Android/iOS SDK clients
  ca-key.pem        Local CA private key, keep private
  cert.pem          Server certificate for gpuf-s
  key.pem           Server private key for gpuf-s
  key-readable.pem  Copy of key.pem with container-friendly read permissions
  openssl.cnf       OpenSSL config used to create the SAN certificate

Options:
  --out-dir DIR           Output directory (default: ./certs)
  --server-name NAME      CN and mobile TLS server name/SNI (default: localhost)
  --dns NAME              Add DNS SAN, repeatable or comma-separated
  --ip ADDR               Add IP SAN, repeatable or comma-separated
  --days N                Validity days (default: 365)
  --ca-name NAME          CA common name (default: GPUFabricLocalCA)
  --force                 Overwrite existing generated files
  -h, --help              Show this help

Environment defaults:
  GPUF_CERT_OUT_DIR, GPUF_CERT_SERVER_NAME, GPUF_CERT_DNS, GPUF_CERT_IP,
  GPUF_CERT_DAYS, GPUF_CERT_CA_NAME

Examples:
  # Same style as the current test cert: connect by IP, validate as localhost.
  scripts/create_mobile_tls_cert.sh \
    --out-dir /tmp/gpuf-cert \
    --server-name localhost \
    --ip <test-server-ip> \
    --force

  # Preferred for a real test environment: validate by DNS name.
  scripts/create_mobile_tls_cert.sh \
    --out-dir /tmp/gpuf-cert \
    --server-name test-gpuf.example.com \
    --dns test-gpuf.example.com \
    --ip <test-server-ip> \
    --force
EOF
}

die() {
    printf 'error: %s\n' "$*" >&2
    exit 1
}

need_value() {
    [[ $# -ge 2 ]] || die "$1 requires a value"
}

trim() {
    local value="$1"
    value="${value#"${value%%[![:space:]]*}"}"
    value="${value%"${value##*[![:space:]]}"}"
    printf '%s' "$value"
}

add_dns_csv() {
    local value="$1"
    local item
    local parts=()
    IFS=',' read -r -a parts <<<"$value"
    for item in "${parts[@]}"; do
        item="$(trim "$item")"
        [[ -n "$item" ]] && dns_names+=("$item")
    done
}

add_ip_csv() {
    local value="$1"
    local item
    local parts=()
    IFS=',' read -r -a parts <<<"$value"
    for item in "${parts[@]}"; do
        item="$(trim "$item")"
        [[ -n "$item" ]] && ip_addrs+=("$item")
    done
}

is_ip_address() {
    [[ "$1" == *:* ]] || [[ "$1" =~ ^[0-9]+(\.[0-9]+){3}$ ]]
}

add_unique_dns() {
    local value="$1"
    local existing
    [[ -n "$value" ]] || return 0
    for existing in "${dns_names[@]}"; do
        [[ "$existing" == "$value" ]] && return 0
    done
    dns_names+=("$value")
}

add_unique_ip() {
    local value="$1"
    local existing
    [[ -n "$value" ]] || return 0
    for existing in "${ip_addrs[@]}"; do
        [[ "$existing" == "$value" ]] && return 0
    done
    ip_addrs+=("$value")
}

validate_subject_value() {
    local label="$1"
    local value="$2"
    [[ -n "$value" ]] || die "$label cannot be empty"
    [[ "$value" != *$'\n'* ]] || die "$label cannot contain newlines"
    [[ "$value" != */* ]] || die "$label cannot contain '/'"
}

command -v openssl >/dev/null 2>&1 || die "openssl is required"

out_dir="${GPUF_CERT_OUT_DIR:-./certs}"
server_name="${GPUF_CERT_SERVER_NAME:-localhost}"
ca_name="${GPUF_CERT_CA_NAME:-GPUFabricLocalCA}"
days="${GPUF_CERT_DAYS:-365}"
force=0
dns_names=()
ip_addrs=()

[[ -n "${GPUF_CERT_DNS:-}" ]] && add_dns_csv "$GPUF_CERT_DNS"
[[ -n "${GPUF_CERT_IP:-}" ]] && add_ip_csv "$GPUF_CERT_IP"

while [[ $# -gt 0 ]]; do
    case "$1" in
        --out-dir)
            need_value "$@"
            out_dir="$2"
            shift 2
            ;;
        --server-name)
            need_value "$@"
            server_name="$2"
            shift 2
            ;;
        --dns)
            need_value "$@"
            add_dns_csv "$2"
            shift 2
            ;;
        --ip)
            need_value "$@"
            add_ip_csv "$2"
            shift 2
            ;;
        --days)
            need_value "$@"
            days="$2"
            shift 2
            ;;
        --ca-name)
            need_value "$@"
            ca_name="$2"
            shift 2
            ;;
        --force)
            force=1
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            die "unknown option: $1"
            ;;
    esac
done

validate_subject_value "server name" "$server_name"
validate_subject_value "CA name" "$ca_name"
[[ "$days" =~ ^[0-9]+$ ]] && [[ "$days" -gt 0 ]] || die "--days must be a positive integer"

if is_ip_address "$server_name"; then
    add_unique_ip "$server_name"
else
    add_unique_dns "$server_name"
fi
add_unique_dns localhost
add_unique_ip 127.0.0.1

mkdir -p "$out_dir"
out_dir="$(cd "$out_dir" && pwd)"

ca_key="$out_dir/ca-key.pem"
ca_cert="$out_dir/ca-cert.pem"
ca_serial="$out_dir/ca-cert.srl"
server_key="$out_dir/key.pem"
server_cert="$out_dir/cert.pem"
readable_key="$out_dir/key-readable.pem"
server_csr="$out_dir/server.csr"
openssl_cnf="$out_dir/openssl.cnf"

generated=("$ca_key" "$ca_cert" "$ca_serial" "$server_key" "$server_cert" "$readable_key" "$server_csr" "$openssl_cnf")
if [[ "$force" -eq 1 ]]; then
    rm -f "${generated[@]}"
else
    for path in "${generated[@]}"; do
        [[ ! -e "$path" ]] || die "$path exists; use --force to overwrite"
    done
fi

{
    printf '[req]\n'
    printf 'prompt = no\n'
    printf 'distinguished_name = dn\n'
    printf 'req_extensions = v3_req\n'
    printf '\n[dn]\n'
    printf 'CN = %s\n' "$server_name"
    printf '\n[v3_req]\n'
    printf 'basicConstraints = CA:FALSE\n'
    printf 'keyUsage = digitalSignature, keyEncipherment\n'
    printf 'extendedKeyUsage = serverAuth\n'
    printf 'subjectAltName = @alt_names\n'
    printf '\n[alt_names]\n'
    dns_idx=1
    for name in "${dns_names[@]}"; do
        printf 'DNS.%d = %s\n' "$dns_idx" "$name"
        dns_idx=$((dns_idx + 1))
    done
    ip_idx=1
    for ip in "${ip_addrs[@]}"; do
        printf 'IP.%d = %s\n' "$ip_idx" "$ip"
        ip_idx=$((ip_idx + 1))
    done
} >"$openssl_cnf"

umask 077
openssl req -x509 -newkey rsa:4096 -sha256 -nodes \
    -keyout "$ca_key" \
    -out "$ca_cert" \
    -days "$days" \
    -subj "/CN=$ca_name"

openssl req -newkey rsa:4096 -sha256 -nodes \
    -keyout "$server_key" \
    -out "$server_csr" \
    -subj "/CN=$server_name" \
    -config "$openssl_cnf"

openssl x509 -req -sha256 \
    -in "$server_csr" \
    -CA "$ca_cert" \
    -CAkey "$ca_key" \
    -CAcreateserial \
    -out "$server_cert" \
    -days "$days" \
    -extfile "$openssl_cnf" \
    -extensions v3_req

cp "$server_key" "$readable_key"
chmod 0600 "$ca_key" "$server_key"
chmod 0644 "$ca_cert" "$server_cert" "$readable_key" "$openssl_cnf"
rm -f "$server_csr"

printf '\nGenerated GPUFabric TLS certificates in %s\n' "$out_dir"
printf '  CA cert for Android/iOS: %s\n' "$ca_cert"
printf '  Server cert for gpuf-s:  %s\n' "$server_cert"
printf '  Server key for gpuf-s:   %s\n' "$server_key"
printf '  Readable server key:     %s\n' "$readable_key"
printf '\nUse this mobile TLS server name/SNI: %s\n\n' "$server_name"
openssl x509 -in "$server_cert" -noout -subject -issuer -dates -ext subjectAltName
