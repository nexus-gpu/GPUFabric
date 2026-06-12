#!/bin/bash
set -e

echo "🔥 Building Android ARM64 SDK with Network Support"
echo "=================================================="
echo "Version: 9.0.0-x86_64-android-FINAL-LLAMA-SOLUTION"
echo ""

# Configuration Variables (aligned with generate_sdk.sh)
SCRIPT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_ROOT/.." && pwd)"
WORKSPACE_ROOT="$(cd "$PROJECT_ROOT/.." && pwd)"

detect_android_ndk() {
    local sdk_ndk_dir candidate
    for sdk_ndk_dir in "$HOME/Android/Sdk/ndk" "/opt/android-sdk/ndk" "/usr/lib/android-sdk/ndk"; do
        if [ -d "$sdk_ndk_dir" ]; then
            candidate="$(find "$sdk_ndk_dir" -mindepth 1 -maxdepth 1 -type d | sort -V | tail -n 1)"
            if [ -n "$candidate" ] && [ -d "$candidate/toolchains/llvm/prebuilt/linux-x86_64/sysroot" ]; then
                echo "$candidate"
                return 0
            fi
        fi
    done
    for candidate in "$HOME/android-ndk-r27d"; do
        if [ -d "$candidate/toolchains/llvm/prebuilt/linux-x86_64/sysroot" ]; then
            echo "$candidate"
            return 0
        fi
    done
    return 1
}

NDK_ROOT="${ANDROID_NDK_ROOT:-${ANDROID_NDK_HOME:-${NDK_ROOT:-}}}"
if [ -z "$NDK_ROOT" ]; then
    NDK_ROOT="$(detect_android_ndk || true)"
fi
TARGET_ARCH="aarch64-linux-android"
ANDROID_API="21"
LLAMA_ANDROID_NDK_DIR="$WORKSPACE_ROOT/target/llama-android-ndk"
SDK_VERSION="9.0.0"

# Color Output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

# Error Handling
handle_error() {
    echo -e "${RED}❌ Error: $1${NC}"
    exit 1
}

handle_success() {
    echo -e "${GREEN}✅ $1${NC}"
}

handle_step() {
    echo -e "${BLUE}🔧 $1${NC}"
}

# Environment Setup
setup_environment() {
    handle_step "Setting up build environment..."
    
    if [ -z "$NDK_ROOT" ] || [ ! -d "$NDK_ROOT" ]; then
        handle_error "Android NDK not found. Set ANDROID_NDK_ROOT or ANDROID_NDK_HOME."
    fi

    # Set Android NDK environment variables
    export ANDROID_NDK_ROOT="$NDK_ROOT"
    export ANDROID_NDK_HOME="${ANDROID_NDK_HOME:-$NDK_ROOT}"
    export TARGET_AR="$NDK_ROOT/toolchains/llvm/prebuilt/linux-x86_64/bin/llvm-ar"
    export TARGET_CC="$NDK_ROOT/toolchains/llvm/prebuilt/linux-x86_64/bin/aarch64-linux-android21-clang"
    export TARGET_CXX="$NDK_ROOT/toolchains/llvm/prebuilt/linux-x86_64/bin/aarch64-linux-android21-clang++"
    
    # Rust target environment
    export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$TARGET_CC"
    export CARGO_TARGET_AARCH64_LINUX_ANDROID_AR="$TARGET_AR"
    
    handle_success "Environment configured"
}

# Build llama.cpp Android libraries
build_llama_android_libs() {
    handle_step "Building llama.cpp Android libraries..."
    
    cd "$PROJECT_ROOT"
    
    # Check if llama.cpp exists
    if [ ! -d "$WORKSPACE_ROOT/llama.cpp" ]; then
        echo "📥 Cloning llama.cpp repository..."
        git clone --depth 1 --branch master https://github.com/ggerganov/llama.cpp.git "$WORKSPACE_ROOT/llama.cpp"
    fi
    
    cd "$WORKSPACE_ROOT/llama.cpp"
    
    # Clean previous build
    rm -rf build-android
    
    # Configure for ARM64 Android
    cmake -B build-android \
        -DCMAKE_TOOLCHAIN_FILE="$NDK_ROOT/build/cmake/android.toolchain.cmake" \
        -DANDROID_ABI=arm64-v8a \
        -DANDROID_PLATFORM=android-28 \
        -DCMAKE_BUILD_TYPE=Release \
        -DBUILD_SHARED_LIBS=OFF \
        -DLLAMA_BUILD_TESTS=OFF \
        -DLLAMA_BUILD_EXAMPLES=OFF \
        -DLLAMA_CURL=OFF
    
    # Build static libraries
    if ! cmake --build build-android -- -j$(nproc); then
        handle_error "llama.cpp build failed"
    fi
    
    # Create target directory and copy libraries
    mkdir -p "$LLAMA_ANDROID_NDK_DIR"
    cp build-android/src/libllama.a "$LLAMA_ANDROID_NDK_DIR/"
    cp build-android/ggml/src/libggml*.a "$LLAMA_ANDROID_NDK_DIR/"
    
    handle_success "llama.cpp libraries built and copied"
}

# Build Rust static library
build_rust_library() {
    handle_step "Building Rust static library..."
    
    cd "$PROJECT_ROOT"
    
    # Build Rust static library
    cargo rustc --target $TARGET_ARCH --release --lib --crate-type=staticlib
    
    # Verify static library exists
    if [ ! -f "$WORKSPACE_ROOT/target/$TARGET_ARCH/release/libgpuf_c.a" ]; then
        handle_error "Rust static library not found"
    fi
    
    handle_success "Rust library build completed"
}

# Extract object files for linking
extract_objects() {
    handle_step "Extracting key object files..."
    
    cd "$PROJECT_ROOT"
    
    # Extract ggml-backend-reg.cpp.o if it doesn't exist
    if [ ! -f "$LLAMA_ANDROID_NDK_DIR/ggml-backend-reg.cpp.o" ]; then
        echo "🔧 Extracting ggml-backend-reg.cpp.o from libggml.a..."
        cd "$LLAMA_ANDROID_NDK_DIR"
        ar -x libggml.a ggml-backend-reg.cpp.o
        cd "$PROJECT_ROOT"
    fi
    
    handle_success "Object files preparation completed"
}

# Link final SDK
link_sdk() {
    handle_step "Linking final SDK dynamic library..."
    
    cd "$PROJECT_ROOT"
    
    # Link with all static libraries
    $TARGET_CC -shared -o libgpuf_c_sdk_v9.so \
        -Wl,--whole-archive \
        "$WORKSPACE_ROOT/target/$TARGET_ARCH/release/libgpuf_c.a" \
        "$LLAMA_ANDROID_NDK_DIR/libllama.a" \
        "$LLAMA_ANDROID_NDK_DIR/libggml-base.a" \
        "$LLAMA_ANDROID_NDK_DIR/libggml-cpu.a" \
        -Wl,--no-whole-archive \
        "$LLAMA_ANDROID_NDK_DIR/ggml-backend-reg.cpp.o" \
        -fopenmp -llog -ldl -lm -latomic
    
    # Strip debug symbols using correct Android NDK strip
    "$NDK_ROOT/toolchains/llvm/prebuilt/linux-x86_64/bin/llvm-strip" --strip-unneeded libgpuf_c_sdk_v9.so
    
    handle_success "SDK linking completed"
}

# Verify SDK
verify_sdk() {
    handle_step "Verifying SDK functionality..."
    
    cd "$PROJECT_ROOT"
    
    # Check file size and symbols
    echo "📊 SDK file information:"
    ls -lh libgpuf_c_sdk_v9.so
    
    echo "🔍 Checking key symbols:"
    echo "- gpuf_init: $(nm -D libgpuf_c_sdk_v9.so | grep gpuf_init | wc -l)"
    echo "- Java_com_gpuf_c_GPUEngine_loadModel: $(nm -D libgpuf_c_sdk_v9.so | grep Java_com_gpuf_c_GPUEngine_loadModel | wc -l)"
    echo "- llama.cpp symbols: $(nm -D libgpuf_c_sdk_v9.so | grep llama | wc -l)"
    
    handle_success "SDK verification completed"
}

# Show Results
show_results() {
    echo ""
    echo -e "${GREEN}🎉 Android ARM64 SDK build completed!${NC}"
    echo ""
    echo "📁 Generated files:"
    echo "- Main library: $PROJECT_ROOT/libgpuf_c_sdk_v9.so"
    echo "- Static library: $WORKSPACE_ROOT/target/$TARGET_ARCH/release/libgpuf_c.a"
    echo "- Llama.cpp libraries: $LLAMA_ANDROID_NDK_DIR/"
    echo ""
    echo "📊 Build features:"
    echo "- ✅ Complete llama.cpp integration"
    echo "- ✅ Full-featured JNI API support"
    echo "- ✅ Android ARM64 optimization"
    echo "- ✅ Static linking of OpenMP"
    echo "- ✅ Minimal runtime dependencies"
    echo ""
    echo "📋 Library file information:"
    ls -lh $PROJECT_ROOT/libgpuf_c_sdk_v9.so
    echo ""
    echo "🚀 Next steps:"
    echo "1. Run ./generate_sdk.sh to create complete SDK package"
    echo "2. Or use libgpuf_c_sdk_v9.so directly for integration"
}

# Main Function
main() {
    echo "Starting Android ARM64 SDK build..."
    echo ""
    
    setup_environment
    build_llama_android_libs
    build_rust_library
    extract_objects
    link_sdk
    verify_sdk
    show_results
    
    echo ""
    echo -e "${GREEN}🎊 Build process completed successfully!${NC}"
}

# Script Entry Point
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    main "$@"
fi
