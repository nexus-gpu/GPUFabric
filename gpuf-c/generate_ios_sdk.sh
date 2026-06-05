#!/bin/bash

set -e

echo "🍎 Compiling gpuf-c for iOS (staticlib + XCFramework)..."

if [ -d "/opt/homebrew/bin" ]; then
    export PATH="/opt/homebrew/bin:$PATH"
fi
if [ -d "/usr/local/bin" ]; then
    export PATH="/usr/local/bin:$PATH"
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$SCRIPT_DIR"
WORKSPACE_ROOT="$(cd "$PROJECT_ROOT/.." && pwd)"

if [ "$(uname)" != "Darwin" ]; then
    echo "❌ iOS build requires macOS (Xcode toolchain)."
    echo "   Please run this script on a Mac with Xcode installed."
    exit 1
fi

if ! command -v rustup >/dev/null 2>&1; then
    echo "❌ rustup not found. Install Rust toolchain first."
    exit 1
fi

if ! command -v xcodebuild >/dev/null 2>&1; then
    echo "❌ xcodebuild not found. Install Xcode and run: xcode-select --install"
    exit 1
fi

echo "🛠️  Detecting Xcode toolchain and iOS SDKs..."

XCODE_DEVELOPER_DIR="${DEVELOPER_DIR:-}"
if [ -z "$XCODE_DEVELOPER_DIR" ]; then
    XCODE_DEVELOPER_DIR="$(xcode-select -p 2>/dev/null || true)"
fi

if [ -z "$XCODE_DEVELOPER_DIR" ] || [ ! -d "$XCODE_DEVELOPER_DIR" ]; then
    echo "❌ Unable to determine Xcode Developer directory."
    echo "   Try: sudo xcode-select -s /Applications/Xcode.app/Contents/Developer"
    exit 1
fi

if ! xcrun --sdk iphoneos --show-sdk-path >/dev/null 2>&1; then
    echo "❌ iPhoneOS SDK not found by xcrun. Current DEVELOPER_DIR: $XCODE_DEVELOPER_DIR"
    echo "   Fix suggestions:"
    echo "   1) Install full Xcode from App Store"
    echo "   2) sudo xcode-select -s /Applications/Xcode.app/Contents/Developer"
    echo "   3) sudo xcodebuild -license accept"
    exit 1
fi

if ! xcrun --sdk iphonesimulator --show-sdk-path >/dev/null 2>&1; then
    echo "❌ iPhoneSimulator SDK not found by xcrun."
    echo "   Ensure Xcode is fully installed."
    exit 1
fi

export DEVELOPER_DIR="$XCODE_DEVELOPER_DIR"
export SDKROOT="$(xcrun --sdk iphoneos --show-sdk-path)"

CLANG_IOS="$(xcrun --sdk iphoneos --find clang)"
CLANG_SIM="$(xcrun --sdk iphonesimulator --find clang)"

export CC_aarch64_apple_ios="$CLANG_IOS"
export CC_aarch64_apple_ios_sim="$CLANG_SIM"
export CC_x86_64_apple_ios="$CLANG_SIM"

echo "🧰 Checking CMake build tool..."
if command -v ninja >/dev/null 2>&1; then
    echo "   ✅ ninja found: $(command -v ninja)"
    export CMAKE_GENERATOR="Ninja"
    export CMAKE_MAKE_PROGRAM="$(command -v ninja)"
else
    echo "   ⚠️  ninja not found; falling back to Unix Makefiles (make)"
    export CMAKE_GENERATOR="Unix Makefiles"
    if command -v make >/dev/null 2>&1; then
        export CMAKE_MAKE_PROGRAM="$(command -v make)"
    fi
fi

BUILD_MODE="${BUILD_MODE:-release}"
FEATURES="${FEATURES:-metal}"

IOS_DEVICE_TARGET="aarch64-apple-ios"
IOS_SIM_ARM64_TARGET="aarch64-apple-ios-sim"
IOS_SIM_X64_TARGET="x86_64-apple-ios"

BUILD_DIR="$PROJECT_ROOT/build_ios"
DIST_DIR="$BUILD_DIR/dist"
INCLUDE_DIR="$DIST_DIR/include"

mkdir -p "$BUILD_DIR" "$DIST_DIR" "$INCLUDE_DIR"

sha256_manifest_line() {
    local base_dir="$1"
    local rel_path="$2"

    if command -v sha256sum >/dev/null 2>&1; then
        (cd "$base_dir" && sha256sum "$rel_path")
        return 0
    fi

    if command -v shasum >/dev/null 2>&1; then
        (cd "$base_dir" && shasum -a 256 "$rel_path")
        return 0
    fi

    if command -v openssl >/dev/null 2>&1; then
        local hash
        hash=$(cd "$base_dir" && openssl dgst -sha256 "$rel_path" | awk '{print $NF}')
        printf '%s  %s\n' "$hash" "$rel_path"
        return 0
    fi

    echo "❌ No SHA256 tool available (need sha256sum, shasum, or openssl)"
    exit 1
}

write_sha256_manifest() {
    local manifest="$1"
    shift

    : > "$manifest"
    for item in "$@"; do
        if [ -f "$item" ]; then
            sha256_manifest_line "$(dirname "$item")" "$(basename "$item")" >> "$manifest"
        elif [ -d "$item" ]; then
            local parent
            local dir_name
            parent="$(dirname "$item")"
            dir_name="$(basename "$item")"
            while IFS= read -r -d '' rel_path; do
                sha256_manifest_line "$parent" "$rel_path" >> "$manifest"
            done < <(cd "$parent" && find "$dir_name" -type f -print0 | sort -z)
        else
            echo "❌ Cannot hash missing release artifact: $item"
            exit 1
        fi
    done
}

if [ -f "$PROJECT_ROOT/gpuf_c_minimal.h" ]; then
    cp "$PROJECT_ROOT/gpuf_c_minimal.h" "$INCLUDE_DIR/"
fi
if [ -f "$PROJECT_ROOT/gpuf_c.h" ]; then
    cp "$PROJECT_ROOT/gpuf_c.h" "$INCLUDE_DIR/"
fi

echo "🦀 Ensuring Rust targets are installed..."
rustup target add "$IOS_DEVICE_TARGET" >/dev/null 2>&1 || true
rustup target add "$IOS_SIM_ARM64_TARGET" >/dev/null 2>&1 || true
rustup target add "$IOS_SIM_X64_TARGET" >/dev/null 2>&1 || true

echo "🔧 Building iOS device static library ($IOS_DEVICE_TARGET)..."
cd "$PROJECT_ROOT"
if [ "$BUILD_MODE" = "release" ]; then
    cargo rustc --target "$IOS_DEVICE_TARGET" --release --lib --crate-type=staticlib --features "$FEATURES"
else
    cargo rustc --target "$IOS_DEVICE_TARGET" --lib --crate-type=staticlib --features "$FEATURES"
fi

DEVICE_LIB="$WORKSPACE_ROOT/target/$IOS_DEVICE_TARGET/$BUILD_MODE/libgpuf_c.a"
if [ ! -f "$DEVICE_LIB" ]; then
    echo "❌ Device library not found: $DEVICE_LIB"
    exit 1
fi

echo "🔧 Building iOS simulator static library ($IOS_SIM_ARM64_TARGET)..."
if [ "$BUILD_MODE" = "release" ]; then
    cargo rustc --target "$IOS_SIM_ARM64_TARGET" --release --lib --crate-type=staticlib --features "$FEATURES"
else
    cargo rustc --target "$IOS_SIM_ARM64_TARGET" --lib --crate-type=staticlib --features "$FEATURES"
fi

SIM_ARM64_LIB="$WORKSPACE_ROOT/target/$IOS_SIM_ARM64_TARGET/$BUILD_MODE/libgpuf_c.a"

SIM_X64_LIB=""
LLAMA_IOS_SIM_X64_DIR="$WORKSPACE_ROOT/target/llama-ios/$IOS_SIM_X64_TARGET"
if rustup target list --installed | grep -q "^$IOS_SIM_X64_TARGET$"; then
    if [ -d "$LLAMA_IOS_SIM_X64_DIR" ]; then
        echo "🔧 Building iOS simulator static library ($IOS_SIM_X64_TARGET)..."
        if [ "$BUILD_MODE" = "release" ]; then
            cargo rustc --target "$IOS_SIM_X64_TARGET" --release --lib --crate-type=staticlib --features "$FEATURES"
        else
            cargo rustc --target "$IOS_SIM_X64_TARGET" --lib --crate-type=staticlib --features "$FEATURES"
        fi
        CANDIDATE="$WORKSPACE_ROOT/target/$IOS_SIM_X64_TARGET/$BUILD_MODE/libgpuf_c.a"
        if [ -f "$CANDIDATE" ]; then
            SIM_X64_LIB="$CANDIDATE"
        fi
    else
        echo "ℹ️  Skipping $IOS_SIM_X64_TARGET build: missing llama.cpp libs at $LLAMA_IOS_SIM_X64_DIR"
    fi
fi

if [ ! -f "$SIM_ARM64_LIB" ]; then
    echo "❌ Simulator (arm64) library not found: $SIM_ARM64_LIB"
    exit 1
fi

SIM_UNIVERSAL_LIB="$DIST_DIR/libgpuf_c_simulator.a"
if [ -n "$SIM_X64_LIB" ] && command -v lipo >/dev/null 2>&1; then
    echo "🔗 Creating universal simulator library (arm64 + x86_64)..."
    lipo -create "$SIM_ARM64_LIB" "$SIM_X64_LIB" -output "$SIM_UNIVERSAL_LIB"
else
    cp "$SIM_ARM64_LIB" "$SIM_UNIVERSAL_LIB"
fi

# Merge gpuf-c + prebuilt llama.cpp libs into a single archive per platform.
# This avoids requiring consumers to also link llama/ggml libs manually.
LIBTOOL_BIN="$(xcrun -f libtool)"
if [ -z "$LIBTOOL_BIN" ]; then
    echo "❌ libtool not found via xcrun"
    exit 1
fi

merge_one() {
    local in_gpuf_a="$1"
    local in_llama_dir="$2"
    local out_a="$3"

    if [ ! -f "$in_gpuf_a" ]; then
        echo "❌ Missing gpuf-c archive: $in_gpuf_a"
        exit 1
    fi
    if [ ! -d "$in_llama_dir" ]; then
        echo "❌ Missing llama-ios lib dir: $in_llama_dir"
        exit 1
    fi

    local libs=(
        "$in_gpuf_a"
        "$in_llama_dir/libllama.a"
        "$in_llama_dir/libggml.a"
        "$in_llama_dir/libggml-base.a"
        "$in_llama_dir/libggml-cpu.a"
    )

    if [ -f "$in_llama_dir/libggml-metal.a" ]; then
        libs+=("$in_llama_dir/libggml-metal.a")
    fi
    if [ -f "$in_llama_dir/libggml-blas.a" ]; then
        libs+=("$in_llama_dir/libggml-blas.a")
    fi
    if [ -f "$in_llama_dir/libmtmd.a" ]; then
        libs+=("$in_llama_dir/libmtmd.a")
    fi

    echo "🔗 Merging static libs into: $out_a"
    rm -f "$out_a"
    "$LIBTOOL_BIN" -static -o "$out_a" "${libs[@]}"
}

LLAMA_DEVICE_DIR="$WORKSPACE_ROOT/target/llama-ios/$IOS_DEVICE_TARGET"
LLAMA_SIM_ARM64_DIR="$WORKSPACE_ROOT/target/llama-ios/$IOS_SIM_ARM64_TARGET"

MERGED_DEVICE_LIB="$DIST_DIR/libgpuf_c_device.a"
MERGED_SIM_LIB="$DIST_DIR/libgpuf_c_simulator_merged.a"

merge_one "$DEVICE_LIB" "$LLAMA_DEVICE_DIR" "$MERGED_DEVICE_LIB"
merge_one "$SIM_UNIVERSAL_LIB" "$LLAMA_SIM_ARM64_DIR" "$MERGED_SIM_LIB"

XCFRAMEWORK_OUT="$DIST_DIR/gpuf_c_sdk.xcframework"
rm -rf "$XCFRAMEWORK_OUT"

echo "📦 Creating XCFramework..."
xcodebuild -create-xcframework \
    -library "$MERGED_DEVICE_LIB" -headers "$INCLUDE_DIR" \
    -library "$MERGED_SIM_LIB" -headers "$INCLUDE_DIR" \
    -output "$XCFRAMEWORK_OUT"

write_sha256_manifest \
    "$DIST_DIR/SHA256SUMS" \
    "$MERGED_DEVICE_LIB" \
    "$MERGED_SIM_LIB" \
    "$XCFRAMEWORK_OUT"
sha256_manifest_line "$DIST_DIR" "libgpuf_c_device.a" > "$DIST_DIR/libgpuf_c_device.a.sha256"
sha256_manifest_line "$DIST_DIR" "libgpuf_c_simulator_merged.a" > "$DIST_DIR/libgpuf_c_simulator_merged.a.sha256"

echo "✅ iOS SDK build completed!"
echo "📦 XCFramework: $XCFRAMEWORK_OUT"
echo "📁 Headers: $INCLUDE_DIR"
echo "🔒 SHA256 manifest: $DIST_DIR/SHA256SUMS"
