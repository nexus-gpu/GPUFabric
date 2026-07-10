#!/bin/bash
set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# init log file
LOG_FILE="/tmp/gpuf_c_llamacpp_install_$(date +%Y%m%d_%H%M%S).log"
echo "Installation started at $(date)" > "$LOG_FILE"

DEFAULT_BGE_M3_MODEL_URL="https://modelscope.cn/models/OllmOne/bge-m3-GGUF/resolve/master/bge-m3-q8_0.gguf"
DEFAULT_BGE_M3_MODEL_FILE="bge-m3-q8_0.gguf"
DEFAULT_ONLINE_SERVER_ADDR="agent.gpunexus.com"
DEFAULT_ONLINE_TLS_SERVER_NAME="agent.gpunexus.com"
DEFAULT_ONLINE_CLIENT_ID="00112233445566778899aabbccddeeff"
DEFAULT_CONTROL_PORT="17000"
DEFAULT_PROXY_PORT="17001"

# log function
log() {
    echo -e "$1"
    echo -e "[$(date +'%Y-%m-%d %H:%M:%S')] $1" >> "$LOG_FILE"
}

# check command exists
check_command() {
    if ! command -v "$1" &> /dev/null; then
        log "${RED}error: need $1 but not installed${NC}"
        exit 1
    fi
}

verify_macos_binary_format() {
    local file_path="$1"

    if [ "$OS" != "darwin" ]; then
        return 0
    fi

    if ! command -v file &> /dev/null; then
        log "${YELLOW}warning: 'file' command not found; skip macOS binary format check${NC}"
        return 0
    fi

    local out
    out=$(file "$file_path" 2>/dev/null || true)
    if [[ "$out" != *"Mach-O"* ]]; then
        log "${RED}invalid gpuf-c binary for macOS (expect Mach-O, got something else)${NC}"
        log "${YELLOW}$out${NC}"
        log "${YELLOW}hint: your download package may be wrong (e.g., Linux tarball uploaded to mac key)${NC}"
        return 1
    fi

    local mach
    mach=$(uname -m | tr '[:upper:]' '[:lower:]')
    case "$mach" in
        arm64)
            if [[ "$out" != *"arm64"* ]]; then
                log "${RED}invalid gpuf-c binary architecture for this Mac (need arm64)${NC}"
                log "${YELLOW}$out${NC}"
                return 1
            fi
            ;;
        x86_64)
            if [[ "$out" != *"x86_64"* && "$out" != *"x86-64"* ]]; then
                log "${RED}invalid gpuf-c binary architecture for this Mac (need x86_64)${NC}"
                log "${YELLOW}$out${NC}"
                return 1
            fi
            ;;
        *)
            # Unknown arch; allow Mach-O only
            ;;
    esac
}

# detect os and architecture
# NOTE: This installer is for the llama.cpp version of gpuf-c.
# It downloads a compressed release archive and extracts it.
detect_system() {
    echo "=== system detect ==="

    local u
    u="$(uname)"

    case "$u" in
        Darwin)
            OS="darwin"
            ARCH="$(uname -m)"
            echo "OS: macOS ($ARCH)"
            ;;
        Linux)
            OS="linux"
            ARCH="$(uname -m)"
            echo "OS: Linux ($ARCH)"
            ;;
        MINGW*|MSYS*|CYGWIN*)
            OS="windows"
            ARCH="x86_64"
            echo "OS: Windows (via $u)"
            ;;
        *)
            OS="linux"
            ARCH="$(uname -m)"
            echo "OS: $u ($ARCH)"
            ;;
    esac

    export OS
    export ARCH
}

# Installation directory
get_install_dir() {
    echo "/usr/local/bin"
}

get_share_dir() {
    echo "/usr/local/share/gpuf-c"
}

normalize_arch() {
    case "$ARCH" in
        x86_64|amd64)
            echo "x86_64"
            ;;
        aarch64|arm64)
            echo "arm64"
            ;;
        *)
            echo "$ARCH"
            ;;
    esac
}

calc_sha256() {
    local file="$1"

    if command -v sha256sum &> /dev/null; then
        sha256sum "$file" | awk '{print $1}'
        return 0
    fi

    if command -v shasum &> /dev/null; then
        shasum -a 256 "$file" | awk '{print $1}'
        return 0
    fi

    if command -v openssl &> /dev/null; then
        openssl dgst -sha256 "$file" | awk '{print $NF}'
        return 0
    fi

    return 1
}

calc_md5() {
    local file="$1"

    if command -v md5sum &> /dev/null; then
        md5sum "$file" | awk '{print $1}'
        return 0
    fi

    if command -v md5 &> /dev/null; then
        md5 -q "$file"
        return 0
    fi

    if command -v openssl &> /dev/null; then
        openssl md5 "$file" | awk '{print $NF}'
        return 0
    fi

    return 1
}

read_sha256_file() {
    local sha_file="$1"
    local archive_name="$2"

    if [ ! -f "$sha_file" ]; then
        return 1
    fi

    local line
    line=$(tr -d '\r' < "$sha_file" | grep -E "(^|[[:space:]])[* ]?$archive_name$|^[0-9a-fA-F]{64}([[:space:]]|$)" | head -n 1)
    if [ -z "$line" ]; then
        return 1
    fi

    echo "$line" | awk '{print $1}' | tr '[:upper:]' '[:lower:]'
}

verify_sha256_required() {
    local file="$1"
    local expected="$2"

    if [ -z "$expected" ]; then
        log "${RED}sha256 check failed: expected hash missing${NC}"
        return 1
    fi

    if [[ ! "$expected" =~ ^[0-9a-f]{64}$ ]]; then
        log "${RED}sha256 check failed: invalid expected hash format${NC}"
        return 1
    fi

    if [ ! -f "$file" ]; then
        log "${RED}sha256 check failed: file not found: $file${NC}"
        return 1
    fi

    local actual
    if ! actual=$(calc_sha256 "$file"); then
        log "${RED}sha256 check failed: sha256 tool not available (need sha256sum/shasum/openssl)${NC}"
        return 1
    fi

    actual=$(echo "$actual" | tr '[:upper:]' '[:lower:]')
    if [ "$actual" != "$expected" ]; then
        log "${RED}sha256 mismatch for $file${NC}"
        log "${YELLOW}expected: $expected${NC}"
        log "${YELLOW}actual:   $actual${NC}"
        return 1
    fi

    log "${GREEN}sha256 match ok: $actual${NC}"
}

read_md5_prefix_from_filename() {
    local file_path="$1"
    local base
    base=$(basename "$file_path")

    if [[ "$base" =~ ^([0-9a-fA-F]{6})- ]]; then
        echo "${BASH_REMATCH[1]}" | tr '[:upper:]' '[:lower:]'
        return 0
    fi

    echo ""
}

verify_md5_prefix_from_filename_if_possible() {
    local file="$1"

    if [ ! -f "$file" ]; then
        log "${RED}md5 check failed: file not found: $file${NC}"
        return 1
    fi

    local prefix
    prefix=$(read_md5_prefix_from_filename "$file")
    if [ -z "$prefix" ]; then
        log "${YELLOW}warning: md5 prefix not found in filename (skip md5 prefix check): $(basename "$file")${NC}"
        return 0
    fi

    local md5
    if ! md5=$(calc_md5 "$file"); then
        log "${RED}md5 check failed: md5 tool not available (need md5sum/md5/openssl)${NC}"
        return 1
    fi

    md5=$(echo "$md5" | tr '[:upper:]' '[:lower:]')
    if [ "${md5:0:6}" != "$prefix" ]; then
        log "${RED}md5 prefix mismatch for $file${NC}"
        log "${YELLOW}expected prefix: $prefix${NC}"
        log "${YELLOW}actual md5:      $md5${NC}"
        return 1
    fi

    log "${GREEN}md5 prefix match ok: $md5${NC}"
}

verify_binary_name_checksums() {
    local extracted_dir="$1"

    if [ "$OS" = "linux" ]; then
        local linux_cuda
        linux_cuda=$(find "$extracted_dir" -maxdepth 1 -type f -name "*-cuda-gpuf-c" | head -n 1)
        local linux_vulkan
        linux_vulkan=$(find "$extracted_dir" -maxdepth 1 -type f -name "*-vulkan-gpuf-c" | head -n 1)

        if [ -n "$linux_vulkan" ] && [ -f "$linux_vulkan" ]; then
            verify_md5_prefix_from_filename_if_possible "$linux_vulkan"
        fi

        if [ -n "$linux_cuda" ] && [ -f "$linux_cuda" ]; then
            verify_md5_prefix_from_filename_if_possible "$linux_cuda"
        fi
    else
        local mac_bin
        mac_bin=$(find "$extracted_dir" -maxdepth 1 -type f -name "*-metal-gpuf-c" | head -n 1)
        if [ -n "$mac_bin" ] && [ -f "$mac_bin" ]; then
            verify_md5_prefix_from_filename_if_possible "$mac_bin"
        fi
    fi
}

ensure_dir() {
    local dir="$1"
    if [ ! -d "$dir" ]; then
        mkdir -p "$dir"
    fi
}

# download helper (curl)
download_file() {
    local url="$1"
    local out="$2"

    log "${YELLOW}download: $url${NC}"
    # Use curl with progress bar (-#) instead of silent mode
    # -f: fail silently on HTTP errors
    # -L: follow redirects
    # -#: show progress bar
    # -o: output file
    if ! curl -fL# "$url" -o "$out" 2>&1 | tee -a "$LOG_FILE"; then
        log "${RED}download failed: $url${NC}"
        return 1
    fi
    echo "" # Add newline after progress bar
}

extract_archive() {
    local archive="$1"
    local dest_dir="$2"

    ensure_dir "$dest_dir"

    case "$archive" in
        *.tar.gz|*.tgz)
            check_command tar
            tar -xzf "$archive" -C "$dest_dir" >> "$LOG_FILE" 2>&1
            ;;
        *.zip)
            check_command unzip
            unzip -q "$archive" -d "$dest_dir" >> "$LOG_FILE" 2>&1
            ;;
        *)
            log "${RED}unsupported archive format: $archive${NC}"
            return 1
            ;;
    esac
}

install_from_extracted_dir() {
    local extracted_dir="$1"

    local linux_cuda
    linux_cuda=$(find "$extracted_dir" -maxdepth 1 -type f -name "*-cuda-gpuf-c" | head -n 1)
    local linux_vulkan
    linux_vulkan=$(find "$extracted_dir" -maxdepth 1 -type f -name "*-vulkan-gpuf-c" | head -n 1)
    local mac_bin
    mac_bin=$(find "$extracted_dir" -maxdepth 1 -type f -name "*-metal-gpuf-c" | head -n 1)

    if [ "$OS" = "linux" ]; then
        if [ -z "$linux_cuda" ] && [ -z "$linux_vulkan" ]; then
            log "${RED}not found linux binaries in extracted directory: $extracted_dir${NC}"
            return 1
        fi

        if [ -n "$linux_vulkan" ] && [ -f "$linux_vulkan" ]; then
            sudo install -m 0755 "$linux_vulkan" "$INSTALL_DIR/gpuf-c-vulkan" >> "$LOG_FILE" 2>&1
            log "${GREEN}installed: $INSTALL_DIR/gpuf-c-vulkan${NC}"
        fi

        if [ -n "$linux_cuda" ] && [ -f "$linux_cuda" ]; then
            sudo install -m 0755 "$linux_cuda" "$INSTALL_DIR/gpuf-c-cuda" >> "$LOG_FILE" 2>&1
            log "${GREEN}installed: $INSTALL_DIR/gpuf-c-cuda${NC}"
        fi

        if command -v nvidia-smi &> /dev/null && [ -f "$INSTALL_DIR/gpuf-c-cuda" ]; then
            sudo ln -sf "$INSTALL_DIR/gpuf-c-cuda" "$INSTALL_DIR/gpuf-c" >> "$LOG_FILE" 2>&1
            log "${GREEN}installed: $INSTALL_DIR/gpuf-c (CUDA)${NC}"
        elif command -v vulkaninfo &> /dev/null && [ -f "$INSTALL_DIR/gpuf-c-vulkan" ]; then
            sudo ln -sf "$INSTALL_DIR/gpuf-c-vulkan" "$INSTALL_DIR/gpuf-c" >> "$LOG_FILE" 2>&1
            log "${GREEN}installed: $INSTALL_DIR/gpuf-c (Vulkan)${NC}"
        elif [ -f "$INSTALL_DIR/gpuf-c-cuda" ]; then
            sudo ln -sf "$INSTALL_DIR/gpuf-c-cuda" "$INSTALL_DIR/gpuf-c" >> "$LOG_FILE" 2>&1
            log "${GREEN}installed: $INSTALL_DIR/gpuf-c (CUDA)${NC}"
        elif [ -f "$INSTALL_DIR/gpuf-c-vulkan" ]; then
            sudo ln -sf "$INSTALL_DIR/gpuf-c-vulkan" "$INSTALL_DIR/gpuf-c" >> "$LOG_FILE" 2>&1
            log "${GREEN}installed: $INSTALL_DIR/gpuf-c (Vulkan)${NC}"
        else
            log "${RED}failed to select default gpuf-c binary${NC}"
            return 1
        fi
    else
        if [ -z "$mac_bin" ]; then
            log "${RED}not found mac binary in extracted directory: $extracted_dir${NC}"
            return 1
        fi

        verify_macos_binary_format "$mac_bin"

        sudo install -m 0755 "$mac_bin" "$INSTALL_DIR/gpuf-c" >> "$LOG_FILE" 2>&1
        log "${GREEN}installed: $INSTALL_DIR/gpuf-c${NC}"
    fi

    if [ -f "$extracted_dir/read.txt" ]; then
        local share_dir
        share_dir=$(get_share_dir)
        sudo mkdir -p "$share_dir" >> "$LOG_FILE" 2>&1
        sudo install -m 0644 "$extracted_dir/read.txt" "$share_dir/read.txt" >> "$LOG_FILE" 2>&1
        log "${GREEN}installed: $share_dir/read.txt${NC}"
    fi

    if [ -f "$extracted_dir/ca-cert.pem" ]; then
        sudo install -m 0644 "$extracted_dir/ca-cert.pem" "$INSTALL_DIR/ca-cert.pem" >> "$LOG_FILE" 2>&1
        log "${GREEN}installed: $INSTALL_DIR/ca-cert.pem${NC}"
    fi
}

install_bge_m3_model() {
    if [ "${GPUF_C_SKIP_BGE_M3_DOWNLOAD:-false}" = "true" ]; then
        log "${YELLOW}skip bge-m3 model download: GPUF_C_SKIP_BGE_M3_DOWNLOAD=true${NC}"
        return 0
    fi

    local share_dir
    share_dir=$(get_share_dir)

    local model_dir="${GPUF_C_MODEL_DIR:-$share_dir/models}"
    local model_file="${GPUF_C_MODEL_FILE:-$DEFAULT_BGE_M3_MODEL_FILE}"
    local model_url="${GPUF_C_MODEL_URL:-$DEFAULT_BGE_M3_MODEL_URL}"
    local model_path="$model_dir/$model_file"

    sudo mkdir -p "$model_dir" >> "$LOG_FILE" 2>&1

    if [ -s "$model_path" ]; then
        log "${GREEN}model already exists: $model_path${NC}"
        return 0
    fi

    local tmp_dir
    tmp_dir=$(mktemp -d)
    local tmp_model="$tmp_dir/$model_file"

    if ! download_file "$model_url" "$tmp_model"; then
        rm -rf "$tmp_dir"
        return 1
    fi

    sudo install -m 0644 "$tmp_model" "$model_path" >> "$LOG_FILE" 2>&1
    rm -rf "$tmp_dir"
    log "${GREEN}installed bge-m3 model: $model_path${NC}"
}

install_online_bge_m3_start_script() {
    local share_dir
    share_dir=$(get_share_dir)
    sudo mkdir -p "$share_dir" >> "$LOG_FILE" 2>&1

    local model_dir="${GPUF_C_MODEL_DIR:-$share_dir/models}"
    local model_file="${GPUF_C_MODEL_FILE:-$DEFAULT_BGE_M3_MODEL_FILE}"
    local model_path="$model_dir/$model_file"

    local script_path="$share_dir/start-gpuf-c-bge-m3-online.sh"
    local tmp_script
    tmp_script=$(mktemp)

    cat > "$tmp_script" <<EOF
#!/usr/bin/env bash
set -euo pipefail

GPUF_C_BIN="\${GPUF_C_BIN:-$INSTALL_DIR/gpuf-c}"
GPUF_C_CLIENT_ID="\${GPUF_C_CLIENT_ID:-$DEFAULT_ONLINE_CLIENT_ID}"
GPUF_C_SERVER_ADDR="\${GPUF_C_SERVER_ADDR:-$DEFAULT_ONLINE_SERVER_ADDR}"
GPUF_C_CONTROL_PORT="\${GPUF_C_CONTROL_PORT:-$DEFAULT_CONTROL_PORT}"
GPUF_C_PROXY_PORT="\${GPUF_C_PROXY_PORT:-$DEFAULT_PROXY_PORT}"
GPUF_C_CONTROL_TLS_SERVER_NAME="\${GPUF_C_CONTROL_TLS_SERVER_NAME:-$DEFAULT_ONLINE_TLS_SERVER_NAME}"
GPUF_C_CERT_CHAIN_PATH="\${GPUF_C_CERT_CHAIN_PATH:-$INSTALL_DIR/ca-cert.pem}"
GPUF_C_MODEL_PATH="\${GPUF_C_MODEL_PATH:-$model_path}"
GPUF_C_N_GPU_LAYERS="\${GPUF_C_N_GPU_LAYERS:-99}"
GPUF_C_N_CTX="\${GPUF_C_N_CTX:-2048}"
GPUF_C_N_BATCH="\${GPUF_C_N_BATCH:-512}"

exec env RUST_LOG="\${RUST_LOG:-gpuf_c=debug,common=info}" "\$GPUF_C_BIN" \\
  --client-id "\$GPUF_C_CLIENT_ID" \\
  --server-addr "\$GPUF_C_SERVER_ADDR" \\
  --control-port "\$GPUF_C_CONTROL_PORT" \\
  --proxy-port "\$GPUF_C_PROXY_PORT" \\
  --engine-type llama \\
  --llama-model-path "\$GPUF_C_MODEL_PATH" \\
  --n-gpu-layers "\$GPUF_C_N_GPU_LAYERS" \\
  --n-ctx "\$GPUF_C_N_CTX" \\
  --n-batch "\$GPUF_C_N_BATCH" \\
  --control-tls \\
  --control-tls-server-name "\$GPUF_C_CONTROL_TLS_SERVER_NAME" \\
  --cert-chain-path "\$GPUF_C_CERT_CHAIN_PATH"
EOF

    sudo install -m 0755 "$tmp_script" "$script_path" >> "$LOG_FILE" 2>&1
    rm -f "$tmp_script"
    log "${GREEN}installed online bge-m3 startup script: $script_path${NC}"
}

verify_installation() {
    log "${GREEN}=== installation completed ===${NC}"
    log "${YELLOW}verify installation:${NC}"

    local share_dir
    share_dir=$(get_share_dir)
    if [ -f "$share_dir/read.txt" ]; then
        log "${YELLOW}usage guide:${NC}"
        log "  ${GREEN}$share_dir/read.txt${NC}"
    fi

    if command -v gpuf-c &> /dev/null; then
        log "${GREEN}✓ gpuf-c installed successfully${NC}"
        gpuf-c --version 2>/dev/null || true
    else
        log "${RED}✗ gpuf-c installation failed${NC}"
    fi

    if [ -x "$share_dir/start-gpuf-c-bge-m3-online.sh" ]; then
        log "${YELLOW}online bge-m3 startup:${NC}"
        log "  ${GREEN}$share_dir/start-gpuf-c-bge-m3-online.sh${NC}"
    fi
}

# main install function
main() {
    log "${YELLOW}=== gpuf-c (llama.cpp) Install process ===${NC}"

    detect_system

    INSTALL_DIR=$(get_install_dir)

    check_command "curl"

    case "$OS" in
        linux|darwin)
            check_command sudo

            local arch_norm
            arch_norm=$(normalize_arch)

            BASE_URL="${GPUF_C_CLIENT_BASE_URL:-https://oss.gpunexus.com/client}"

            local pkg_os
            if [ "$OS" = "darwin" ]; then
                pkg_os="mac"
            else
                pkg_os="$OS"
            fi

            local archive_name
            archive_name="v1.0.4-${pkg_os}-gpuf-c.tar.gz"

            ARCHIVE_NAME="${GPUF_C_CLIENT_ARCHIVE_NAME:-$archive_name}"

            local tmp_dir
            tmp_dir=$(mktemp -d)
            local archive_path="$tmp_dir/$ARCHIVE_NAME"
            local extract_dir="$tmp_dir/extract"

            if ! download_file "$BASE_URL/$ARCHIVE_NAME" "$archive_path"; then
                exit 1
            fi

            local sha_path="$tmp_dir/$ARCHIVE_NAME.sha256"
            local expected_sha="${GPUF_C_CLIENT_SHA256:-}"
            if [ -z "$expected_sha" ]; then
                if download_file "$BASE_URL/$ARCHIVE_NAME.sha256" "$sha_path"; then
                    expected_sha=$(read_sha256_file "$sha_path" "$ARCHIVE_NAME" || true)
                else
                    local sums_path="$tmp_dir/SHA256SUMS"
                    if download_file "$BASE_URL/SHA256SUMS" "$sums_path"; then
                        expected_sha=$(read_sha256_file "$sums_path" "$ARCHIVE_NAME" || true)
                    fi
                fi
            fi
            verify_sha256_required "$archive_path" "$expected_sha"

            extract_archive "$archive_path" "$extract_dir"

            local payload
            payload="$extract_dir"
            if [ ! -d "$payload" ]; then
                log "${RED}failed to locate extracted payload${NC}"
                exit 1
            fi

            local top
            top=$(find "$payload" -maxdepth 1 -type d ! -path "$payload" | head -n 1)
            if [ -n "$top" ] && [ -f "$top/read.txt" ]; then
                payload="$top"
            fi

            verify_binary_name_checksums "$payload"

            if [ "$OS" = "linux" ]; then
                if command -v nvidia-smi &> /dev/null; then
                    log "${GREEN}detected: NVIDIA (CUDA)${NC}"
                elif command -v vulkaninfo &> /dev/null; then
                    log "${GREEN}detected: Vulkan runtime${NC}"
                else
                    log "${RED}error: Linux requires nvidia-smi (CUDA) OR vulkaninfo (Vulkan runtime)${NC}"
                    exit 1
                fi
            fi

            install_from_extracted_dir "$payload"
            install_bge_m3_model
            install_online_bge_m3_start_script

            rm -rf "$tmp_dir"
            ;;
        *)
            log "${RED}not support os: $OS${NC}"
            exit 1
            ;;
    esac

    verify_installation
}

main "$@"
