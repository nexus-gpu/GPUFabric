#!/bin/bash

set -e

echo "🍎 Building llama.cpp static libraries for iOS (device + simulator)..."

if [ "$(uname)" != "Darwin" ]; then
    echo "❌ iOS build requires macOS (Xcode toolchain)."
    exit 1
fi

if ! command -v xcrun >/dev/null 2>&1; then
    echo "❌ xcrun not found. Install Xcode."
    exit 1
fi

if ! command -v cmake >/dev/null 2>&1; then
    echo "❌ cmake not found. Install cmake (e.g. brew install cmake)."
    exit 1
fi

if ! command -v ninja >/dev/null 2>&1; then
    echo "❌ ninja not found. Install ninja (e.g. brew install ninja)."
    exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$SCRIPT_DIR"
WORKSPACE_ROOT="$(cd "$PROJECT_ROOT/.." && pwd)"

LLAMA_CPP_ROOT="${LLAMA_CPP_ROOT:-$WORKSPACE_ROOT/llama.cpp}"

# Version selection:
# - Set LLAMA_CPP_REF to a tag/branch/commit (preferred)
# - Or set LLAMA_CPP_COMMIT (kept for compatibility)
LLAMA_CPP_REF="${LLAMA_CPP_REF:-${LLAMA_CPP_COMMIT:-16cc3c606efe1640a165f666df0e0dc7cc2ad869}}"

if [ ! -d "$LLAMA_CPP_ROOT" ]; then
    if ! command -v git >/dev/null 2>&1; then
        echo "❌ git not found. Please install git to fetch llama.cpp sources."
        exit 1
    fi
    echo "📥 llama.cpp not found, cloning into: $LLAMA_CPP_ROOT"
    echo "🔒 Using llama.cpp ref: $LLAMA_CPP_REF"
    git clone https://github.com/ggerganov/llama.cpp.git "$LLAMA_CPP_ROOT"
fi

cd "$LLAMA_CPP_ROOT"

if [ -d "$LLAMA_CPP_ROOT/.git" ]; then
    if ! command -v git >/dev/null 2>&1; then
        echo "❌ git not found. Please install git to fetch llama.cpp sources."
        exit 1
    fi

    echo "🔍 Ensuring llama.cpp ref is checked out: $LLAMA_CPP_REF"

    # Fetch to make sure tags/commits are available; best-effort to keep script robust.
    git fetch --all --tags >/dev/null 2>&1 || true

    if git rev-parse --verify "$LLAMA_CPP_REF" >/dev/null 2>&1; then
        git checkout "$LLAMA_CPP_REF" >/dev/null 2>&1 || git checkout -f "$LLAMA_CPP_REF"
    else
        # If it's a remote branch name, try origin/<ref>
        git checkout "$LLAMA_CPP_REF" >/dev/null 2>&1 || git checkout -f "origin/$LLAMA_CPP_REF"
    fi
else
    if [ ! -f "$LLAMA_CPP_ROOT/CMakeLists.txt" ]; then
        echo "❌ LLAMA_CPP_ROOT is not a llama.cpp source tree: $LLAMA_CPP_ROOT"
        exit 1
    fi
    echo "🔍 Using non-git llama.cpp source tree: $LLAMA_CPP_ROOT"
fi

FEATURES="${FEATURES:-metal}"
DEPLOYMENT_TARGET="${IPHONEOS_DEPLOYMENT_TARGET:-13.0}"

IOS_DEVICE_TRIPLE="aarch64-apple-ios"
IOS_SIM_ARM64_TRIPLE="aarch64-apple-ios-sim"

OUT_ROOT="$WORKSPACE_ROOT/target/llama-ios"
mkdir -p "$OUT_ROOT/$IOS_DEVICE_TRIPLE" "$OUT_ROOT/$IOS_SIM_ARM64_TRIPLE"

build_one() {
    local platform="$1"   # iphoneos / iphonesimulator
    local arch="$2"       # arm64
    local triple="$3"
    local build_dir="$4"

    local sysroot
    sysroot="$(xcrun --sdk "$platform" --show-sdk-path)"

    local metal_flag="OFF"
    if [ "$FEATURES" = "metal" ]; then
        metal_flag="ON"
    fi

    # llama.cpp enables CURL support by default in some versions.
    # When cross-compiling for iOS, curl headers/libs are typically unavailable.
    # Default to OFF for SDK builds; override with LLAMA_CURL=ON if needed.
    local llama_curl_flag="${LLAMA_CURL:-OFF}"

    echo "🔧 Configuring llama.cpp for $platform ($arch) ..."

    rm -rf "$build_dir"
    mkdir -p "$build_dir"

    cmake -S "$LLAMA_CPP_ROOT" -B "$build_dir" -G Ninja \
        -DCMAKE_BUILD_TYPE=Release \
        -DBUILD_SHARED_LIBS=OFF \
        -DLLAMA_BUILD_COMMON=OFF \
        -DLLAMA_BUILD_TESTS=OFF \
        -DLLAMA_BUILD_EXAMPLES=OFF \
        -DLLAMA_BUILD_TOOLS=OFF \
        -DGGML_BUILD_TESTS=OFF \
        -DGGML_BUILD_EXAMPLES=OFF \
        -DGGML_METAL="$metal_flag" \
        -DGGML_BLAS=OFF \
        -DLLAMA_CURL="$llama_curl_flag" \
        -DCMAKE_OSX_SYSROOT="$sysroot" \
        -DCMAKE_OSX_ARCHITECTURES="$arch" \
        -DCMAKE_OSX_DEPLOYMENT_TARGET="$DEPLOYMENT_TARGET"

    echo "🔨 Building llama.cpp for $platform ($arch) ..."
    cmake --build "$build_dir" --config Release --parallel "${CMAKE_BUILD_PARALLEL_LEVEL:-8}"

    local out_dir="$OUT_ROOT/$triple"

    echo "📦 Collecting libraries into: $out_dir"
    rm -f "$out_dir"/lib*.a

    for lib in libllama.a libggml.a libggml-base.a libggml-cpu.a; do
        local found
        found="$(find "$build_dir" -name "$lib" -type f | head -n 1)"
        if [ -z "$found" ]; then
            echo "❌ Missing $lib in build dir: $build_dir"
            exit 1
        fi
        cp "$found" "$out_dir/$lib"
    done

    # Optional ggml backend libraries (may or may not exist depending on llama.cpp version/options)
    for optlib in libggml-metal.a libggml-blas.a; do
        local opt_found
        opt_found="$(find "$build_dir" -name "$optlib" -type f | head -n 1)"
        if [ -n "$opt_found" ]; then
            cp "$opt_found" "$out_dir/$optlib"
        fi
    done

    local mtmd
    mtmd="$(find "$build_dir" -name "libmtmd.a" -type f | head -n 1)"
    if [ -n "$mtmd" ]; then
        cp "$mtmd" "$out_dir/libmtmd.a"
    fi
}

BUILD_DIR_ROOT="$PROJECT_ROOT/build_llama_ios"

build_one "iphoneos" "arm64" "$IOS_DEVICE_TRIPLE" "$BUILD_DIR_ROOT/iphoneos-arm64"
build_one "iphonesimulator" "arm64" "$IOS_SIM_ARM64_TRIPLE" "$BUILD_DIR_ROOT/iphonesimulator-arm64"

echo "✅ llama.cpp iOS static libraries built."
echo "📁 Device libs: $OUT_ROOT/$IOS_DEVICE_TRIPLE"
echo "📁 Simulator libs: $OUT_ROOT/$IOS_SIM_ARM64_TRIPLE"
