#!/bin/bash

echo "🚀 GPUFabric New Version SDK Generation Script"
echo "================================="
echo "Build Time: $(date)"
echo "Version: 9.0.0-x86_64-android-FINAL-LLAMA-SOLUTION"
echo ""

# Configuration Variables
PROJECT_ROOT="/home/jack/codedir/GPUFabric/gpuf-c"
WORKSPACE_ROOT="/home/jack/codedir/GPUFabric"
NDK_ROOT="/home/jack/android-ndk-r27d"
TARGET_ARCH="aarch64-linux-android"
ANDROID_API="21"
LLAMA_ANDROID_NDK_DIR="$WORKSPACE_ROOT/target/llama-android-ndk"
SDK_VERSION="9.0.0"
SDK_RELEASE_DIR="$WORKSPACE_ROOT/target/gpufabric-android-sdk-v$SDK_VERSION"

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

# Check Environment
check_environment() {
    handle_step "Checking build environment..."
    
    if [ ! -d "$NDK_ROOT" ]; then
        handle_error "Android NDK not found: $NDK_ROOT"
    fi
    
    if [ ! -d "$PROJECT_ROOT" ]; then
        handle_error "Project directory not found: $PROJECT_ROOT"
    fi
    
    # Check llama.cpp source directory
    LLAMA_CPP_ROOT="$WORKSPACE_ROOT/llama.cpp"
    LLAMA_CPP_COMMIT="16cc3c606efe1640a165f666df0e0dc7cc2ad869"  # Fixed version: 2025-12-03
    
    if [ ! -d "$LLAMA_CPP_ROOT" ]; then
        echo "📥 llama.cpp source directory not found, starting auto-clone..."
        echo "🔒 Using pinned version: $LLAMA_CPP_COMMIT"
        git clone https://github.com/ggerganov/llama.cpp.git "$LLAMA_CPP_ROOT"
        if [ $? -eq 0 ]; then
            cd "$LLAMA_CPP_ROOT"
            git checkout "$LLAMA_CPP_COMMIT"
            if [ $? -eq 0 ]; then
                echo "✅ llama.cpp cloned and checked out to pinned version successfully!"
            else
                handle_error "llama.cpp checkout to pinned version failed"
            fi
        else
            handle_error "llama.cpp clone failed"
        fi
    else
        echo "✅ llama.cpp source directory already exists"
        echo "🔍 Verifying pinned version..."
        cd "$LLAMA_CPP_ROOT"
        CURRENT_COMMIT=$(git rev-parse HEAD)
        if [ "$CURRENT_COMMIT" != "$LLAMA_CPP_COMMIT" ]; then
            echo "⚠️  Current commit ($CURRENT_COMMIT) differs from pinned version"
            echo "🔄 Checking out to pinned version: $LLAMA_CPP_COMMIT"
            git fetch origin
            git checkout "$LLAMA_CPP_COMMIT"
            if [ $? -eq 0 ]; then
                echo "✅ Checked out to pinned version successfully!"
            else
                handle_error "Failed to checkout to pinned version"
            fi
        else
            echo "✅ Already on pinned version: $LLAMA_CPP_COMMIT"
        fi
    fi
    
    handle_success "Environment check passed"
}

# Clean Old Files
clean_build() {
    handle_step "Cleaning old build files..."
    
    cd "$PROJECT_ROOT"
    
    # Clean Rust build cache
    cargo clean --target $TARGET_ARCH
    
    # Clean old SDK files
    rm -f libgpuf_c_*.so
    rm -rf "$WORKSPACE_ROOT/target/sdk_output" "$WORKSPACE_ROOT/target/gpufabric-android-sdk-v*"
    
    # Clean workspace target files
    if [ -d "$WORKSPACE_ROOT/target/$TARGET_ARCH" ]; then
        rm -rf "$WORKSPACE_ROOT/target/$TARGET_ARCH"
    fi
    
    # Clean llama-android-ndk from target directory
    if [ -d "$WORKSPACE_ROOT/target/llama-android-ndk" ]; then
        # 🆕 Preserve libmtmd.a before cleaning
        if [ -f "$WORKSPACE_ROOT/target/llama-android-ndk/libmtmd.a" ]; then
            echo "🔧 Preserving libmtmd.a before clean..."
            cp "$WORKSPACE_ROOT/target/llama-android-ndk/libmtmd.a" "/tmp/libmtmd_backup.a"
        fi
        rm -rf "$WORKSPACE_ROOT/target/llama-android-ndk"
    fi
    
    # Clean models from target directory ( optional - usually keep models)
    # if [ -d "$WORKSPACE_ROOT/target/models" ]; then
    #     echo "⚠️ Preserving model files in $WORKSPACE_ROOT/target/models/"
    # fi
    
    handle_success "Cleanup completed"
}

# Build Rust Static Library with static linking for Android compatibility
build_rust_library() {
    handle_step "Building Rust static library..."
    
    cd "$PROJECT_ROOT"
    
    # Add Android NDK toolchain to PATH for OpenSSL build system
    echo "🔧 Adding Android NDK toolchain to PATH..."
    export PATH="$NDK_ROOT/toolchains/llvm/prebuilt/linux-x86_64/bin:$PATH"
    
    echo "🔧 Building static library with multimodal support..."
    cargo rustc --target $TARGET_ARCH --release --lib --crate-type=staticlib \
        --features android \
        -- -C link-arg=-static-libstdc++ -C link-arg=-static-libgcc
    
    if [ $? -ne 0 ]; then
        handle_error "Rust library build failed"
    fi
    
    # Check library files
    echo "Checking generated library files..."
    ls -la "$WORKSPACE_ROOT/target/$TARGET_ARCH/release/libgpuf_c."*
    
    if [ ! -f "$WORKSPACE_ROOT/target/$TARGET_ARCH/release/libgpuf_c.a" ]; then
        echo "❌ Static library file not found at expected location, checking other locations..."
        # Check deps directory
        if [ -d "$WORKSPACE_ROOT/target/$TARGET_ARCH/release/deps" ]; then
            DEPS_LIB=$(find "$WORKSPACE_ROOT/target/$TARGET_ARCH/release/deps/" -name "libgpuf_c-*.a" | head -1)
            if [ -n "$DEPS_LIB" ]; then
                echo "✅ Found static library in deps directory: $DEPS_LIB"
                # Copy to expected location
                cp "$DEPS_LIB" "$WORKSPACE_ROOT/target/$TARGET_ARCH/release/libgpuf_c.a"
            else
                handle_error "Rust static library file not generated"
            fi
        else
            handle_error "Rust static library file not generated"
        fi
    fi
    
    handle_success "Rust library build completed"
}

# Generate llama-android-ndk static libraries
build_llama_android_libs() {
    handle_step "Generating llama.cpp Android static libraries..."
    
    cd "$PROJECT_ROOT"
    
    # Check if llama.cpp needs to be rebuilt
    LLAMA_CPP_ROOT="$WORKSPACE_ROOT/llama.cpp"
    
    if [ ! -d "$LLAMA_ANDROID_NDK_DIR" ] || [ ! -f "$LLAMA_ANDROID_NDK_DIR/libllama.a" ]; then
        echo "📦 llama.cpp static libraries not found, starting build..."
        
        # Ensure llama.cpp source exists
        if [ ! -d "$LLAMA_CPP_ROOT" ]; then
            handle_error "llama.cpp source directory not found: $LLAMA_CPP_ROOT"
        fi
        
        # Create llama-android-ndk directory
        mkdir -p "$LLAMA_ANDROID_NDK_DIR"
        
        # Check for pre-built libraries
        if [ -d "$LLAMA_CPP_ROOT/build-android" ] && [ -f "$LLAMA_CPP_ROOT/build-android/src/libllama.a" ]; then
            echo "🔄 Using existing llama.cpp build results..."
            cp "$LLAMA_CPP_ROOT/build-android/src/libllama.a" "$LLAMA_ANDROID_NDK_DIR/"
            cp "$LLAMA_CPP_ROOT/build-android/ggml/src/libggml"*.a "$LLAMA_ANDROID_NDK_DIR/"
            echo "✅ Existing static libraries copied successfully!"
        elif [ -d "$LLAMA_CPP_ROOT/build-android-new" ] && [ -f "$LLAMA_CPP_ROOT/build-android-new/src/libllama.a" ]; then
            echo "🔄 Using new llama.cpp build results..."
            cp "$LLAMA_CPP_ROOT/build-android-new/src/libllama.a" "$LLAMA_ANDROID_NDK_DIR/"
            cp "$LLAMA_CPP_ROOT/build-android-new/ggml/src/libggml"*.a "$LLAMA_ANDROID_NDK_DIR/"
            echo "✅ New build static libraries copied successfully!"
        else
            echo "🔨 No valid pre-built libraries found, starting llama.cpp compilation..."
            build_llama_cpp_from_source
        fi
        
        # Verify key files
        if [ ! -f "$LLAMA_ANDROID_NDK_DIR/libllama.a" ]; then
            handle_error "libllama.a generation failed"
        fi
        
        if [ ! -f "$LLAMA_ANDROID_NDK_DIR/libggml.a" ]; then
            handle_error "libggml.a generation failed"
        fi
        
    else
        echo "✅ llama.cpp static libraries already exist, skipping generation"
    fi
    
    # Display generated files
    echo "📋 llama-android-ndk file list:"
    ls -lh "$LLAMA_ANDROID_NDK_DIR/"
    
    handle_success "llama.cpp static libraries preparation completed"
}

# Build llama.cpp from source
build_llama_cpp_from_source() {
    echo "🔨 Starting llama.cpp compilation from source..."
    
    cd "$LLAMA_CPP_ROOT"
    
    # Create build directory
    mkdir -p build-android
    cd build-android
    
    # Configure CMake
    echo "📋 Configuring CMake with multimodal support..."
    cmake .. \
        -DCMAKE_TOOLCHAIN_FILE="$NDK_ROOT/build/cmake/android.toolchain.cmake" \
        -DANDROID_ABI="arm64-v8a" \
        -DANDROID_PLATFORM="android-28" \
        -DCMAKE_BUILD_TYPE=Release \
        -DBUILD_SHARED_LIBS=OFF \
        -DLLAMA_BUILD_TESTS=OFF \
        -DLLAMA_BUILD_EXAMPLES=OFF \
        -DLLAMA_BUILD_SERVER=OFF \
        -DLLAMA_STATIC=ON \
        -DLLAMA_CURL=OFF \
        -DLLAMA_BUILD_MTMD=ON \
        -DCMAKE_C_FLAGS="-O3 -fno-finite-math-only -DNDEBUG" \
        -DCMAKE_CXX_FLAGS="-O3 -fno-finite-math-only -DNDEBUG"
    
    if [ $? -ne 0 ]; then
        handle_error "CMake configuration failed"
    fi
    
    # Compile
    echo "🔨 Starting compilation..."
    make -j$(nproc)
    
    if [ $? -ne 0 ]; then
        handle_error "llama.cpp compilation failed"
    fi
    
    # Copy static libraries to gpuf-c project
    echo "📦 Copying generated static libraries..."
    cp src/libllama.a "$LLAMA_ANDROID_NDK_DIR/"
    cp ggml/src/libggml*.a "$LLAMA_ANDROID_NDK_DIR/"
    
    # 🆕 Copy multimodal libraries if they exist
    if [ -f "tools/mtmd/libmtmd.a" ]; then
        echo "🎨 Copying multimodal libraries..."
        cp tools/mtmd/libmtmd.a "$LLAMA_ANDROID_NDK_DIR/"
        echo "✅ libmtmd.a copied successfully"
    elif [ -f "../build-android/tools/mtmd/libmtmd.a" ]; then
        echo "🎨 Copying multimodal libraries from build directory..."
        cp ../build-android/tools/mtmd/libmtmd.a "$LLAMA_ANDROID_NDK_DIR/"
        echo "✅ libmtmd.a copied successfully"
    elif [ -f "$LLAMA_CPP_ROOT/build-android/tools/mtmd/libmtmd.a" ]; then
        echo "🎨 Copying multimodal libraries from llama.cpp build directory..."
        cp "$LLAMA_CPP_ROOT/build-android/tools/mtmd/libmtmd.a" "$LLAMA_ANDROID_NDK_DIR/"
        echo "✅ libmtmd.a copied successfully"
    else
        echo "⚠️ libmtmd.a not found - multimodal support disabled"
        echo "   Expected at: tools/mtmd/libmtmd.a"
        echo "              or ../build-android/tools/mtmd/libmtmd.a"
        echo "              or $LLAMA_CPP_ROOT/build-android/tools/mtmd/libmtmd.a"
        echo "   Actual search results:"
        echo "     tools/mtmd/libmtmd.a: $([ -f "tools/mtmd/libmtmd.a" ] && echo "EXISTS" || echo "NOT FOUND")"
        echo "     ../build-android/tools/mtmd/libmtmd.a: $([ -f "../build-android/tools/mtmd/libmtmd.a" ] && echo "EXISTS" || echo "NOT FOUND")"
        echo "     $LLAMA_CPP_ROOT/build-android/tools/mtmd/libmtmd.a: $([ -f "$LLAMA_CPP_ROOT/build-android/tools/mtmd/libmtmd.a" ] && echo "EXISTS" || echo "NOT FOUND")"
    fi
    
    # Extract key object files
    echo "🔧 Extracting ggml-backend-reg.cpp.o..."
    cd "$PROJECT_ROOT"
    if [ -f "$LLAMA_ANDROID_NDK_DIR/libggml.a" ]; then
        ar -x "$LLAMA_ANDROID_NDK_DIR/libggml.a" ggml-backend-reg.cpp.o 2>/dev/null || true
        if [ -f "ggml-backend-reg.cpp.o" ]; then
            mv ggml-backend-reg.cpp.o "$LLAMA_ANDROID_NDK_DIR/"
        fi
    fi
    
    echo "✅ llama.cpp compilation from source completed!"
}

# Extract key object files
extract_objects() {
    handle_step "Extracting key object files..."
    
    cd "$PROJECT_ROOT"
    
    # Ensure ggml-backend-reg.cpp.o exists
    if [ ! -f "$LLAMA_ANDROID_NDK_DIR/ggml-backend-reg.cpp.o" ]; then
        echo "Extracting ggml-backend-reg.cpp.o..."
        ar -x "$LLAMA_ANDROID_NDK_DIR/libggml.a" ggml-backend-reg.cpp.o
        mv ggml-backend-reg.cpp.o "$LLAMA_ANDROID_NDK_DIR/"
    fi
    
    # 🆕 Copy libmtmd.a after llama.cpp build
    if [ -f "$LLAMA_CPP_ROOT/build-android/tools/mtmd/libmtmd.a" ]; then
        echo "🎨 Copying libmtmd.a for multimodal support..."
        cp "$LLAMA_CPP_ROOT/build-android/tools/mtmd/libmtmd.a" "$LLAMA_ANDROID_NDK_DIR/"
        echo "✅ libmtmd.a copied successfully"
    elif [ -f "/tmp/libmtmd_backup.a" ]; then
        echo "🔄 Restoring libmtmd.a from backup..."
        cp "/tmp/libmtmd_backup.a" "$LLAMA_ANDROID_NDK_DIR/"
        echo "✅ libmtmd.a restored from backup"
    else
        echo "⚠️ libmtmd.a not found after build"
    fi
    
    handle_success "Object files preparation completed"
}

# Link final SDK
link_sdk() {
    handle_step "Linking final SDK dynamic library..."
    
    cd "$PROJECT_ROOT"
    
    # Set compiler
    CLANG="$NDK_ROOT/toolchains/llvm/prebuilt/linux-x86_64/bin/$TARGET_ARCH$ANDROID_API-clang++"
    
    # Set library paths
    RUST_LIB="$WORKSPACE_ROOT/target/$TARGET_ARCH/release/libgpuf_c.a"
    LLAMA_LIB="$LLAMA_ANDROID_NDK_DIR/libllama.a"
    GGML_LIB="$LLAMA_ANDROID_NDK_DIR/libggml.a"
    GGML_CPU_LIB="$LLAMA_ANDROID_NDK_DIR/libggml-cpu.a"
    GGML_BASE_LIB="$LLAMA_ANDROID_NDK_DIR/libggml-base.a"
    # 🆕 Add multimodal library
    MTMD_LIB="$LLAMA_ANDROID_NDK_DIR/libmtmd.a"
    BACKEND_OBJ="$LLAMA_ANDROID_NDK_DIR/ggml-backend-reg.cpp.o"
    OMP_LIB="$NDK_ROOT/toolchains/llvm/prebuilt/linux-x86_64/lib/clang/18/lib/linux/aarch64/libomp.a"
    
    # Execute linking
    echo "Linking command executing..."
    $CLANG -shared \
        -o libgpuf_c_sdk_v9.so \
        $RUST_LIB \
        $BACKEND_OBJ \
        $LLAMA_LIB \
        $GGML_LIB \
        $GGML_CPU_LIB \
        $GGML_BASE_LIB \
        -llog -ldl -lm \
        -static-libstdc++ -static-libgcc \
        $OMP_LIB \
        -Wl,--exclude-libs,libomp.a \
        -Wl,--whole-archive $MTMD_LIB -Wl,--no-whole-archive
    
    if [ $? -ne 0 ]; then
        handle_error "SDK linking failed"
    fi
    
    # Check output file
    if [ ! -f "libgpuf_c_sdk_v9.so" ]; then
        handle_error "SDK file not generated"
    fi
    
    handle_success "SDK linking completed"
}

# Verify SDK
verify_sdk() {
    handle_step "Verifying SDK functionality..."
    
    cd "$PROJECT_ROOT"
    
    echo "📊 SDK file information:"
    ls -lh libgpuf_c_sdk_v9.so
    
    echo ""
    echo "🔍 Checking key symbols:"
    
    # Check Rust symbols
    echo "- gpuf_init: $(nm libgpuf_c_sdk_v9.so | grep " T gpuf_init" | wc -l)"
    
    # Check JNI symbols
    echo "- Java_com_gpuf_c_GPUEngine_loadModel: $(nm libgpuf_c_sdk_v9.so | grep " T Java_com_gpuf_c_GPUEngine_loadModel" | wc -l)"
    echo "- Java_com_gpuf_c_GPUEngine_getVersion: $(nm libgpuf_c_sdk_v9.so | grep " T Java_com_gpuf_c_GPUEngine_getVersion" | wc -l)"
    echo "- Java_com_gpuf_c_GPUEngine_getSystemInfo: $(nm libgpuf_c_sdk_v9.so | grep " T Java_com_gpuf_c_GPUEngine_getSystemInfo" | wc -l)"
    
    # Check llama.cpp symbols
    LLAMA_SYMBOLS=$(nm libgpuf_c_sdk_v9.so | grep -E " T (llama_|ggml_)" | wc -l)
    echo "- llama.cpp symbols: $LLAMA_SYMBOLS"
    
    # Check dependencies
    echo ""
    echo "📋 Dependencies:"
    readelf -d libgpuf_c_sdk_v9.so | grep NEEDED | sed 's/Shared library:/Shared library:/'
    
    handle_success "SDK verification completed"
}

# Create SDK output directory
create_sdk_package() {
    handle_step "Creating SDK package..."
    
    cd "$PROJECT_ROOT"
    
    # Create output directory
    mkdir -p "$SDK_RELEASE_DIR"/{libs,include,examples,docs}
    
    # Copy main files
    cp libgpuf_c_sdk_v9.so "$SDK_RELEASE_DIR/libs/"
    
    # Copy dependency libraries
    cp /home/jack/android-ndk-r27d/toolchains/llvm/prebuilt/linux-x86_64/sysroot/usr/lib/aarch64-linux-android/libc++_shared.so "$SDK_RELEASE_DIR/libs/"
    
    # Copy header files (if any)
    if [ -f "gpuf_c.h" ]; then
        cp gpuf_c.h "$SDK_RELEASE_DIR/include/"
    fi
    
    # Copy example files
    if [ -f "TestGPUEngine.java" ]; then
        cp TestGPUEngine.java "$SDK_RELEASE_DIR/examples/"
    fi
    if [ -f "test_jni_symbols.c" ]; then
        cp test_jni_symbols.c "$SDK_RELEASE_DIR/examples/"
    fi
    
    # Create placeholder files if examples don't exist
    if [ ! -f "$SDK_RELEASE_DIR/examples/TestGPUEngine.java" ]; then
        cat > "$SDK_RELEASE_DIR/examples/TestGPUEngine.java" << 'EOF'
// Placeholder Java JNI Example
public class TestGPUEngine {
    static {
        System.loadLibrary("gpuf_c_sdk_v9");
    }
    
    public native String getVersion();
    public native String getSystemInfo();
    public native boolean loadModel(String modelPath);
    
    public static void main(String[] args) {
        TestGPUEngine engine = new TestGPUEngine();
        System.out.println("Version: " + engine.getVersion());
        System.out.println("System Info: " + engine.getSystemInfo());
    }
}
EOF
    fi
    
    if [ ! -f "$SDK_RELEASE_DIR/examples/test_jni_symbols.c" ]; then
        cat > "$SDK_RELEASE_DIR/examples/test_jni_symbols.c" << 'EOF'
// Placeholder C JNI Test
#include <stdio.h>
#include <dlfcn.h>

int main() {
    void *handle = dlopen("libgpuf_c_sdk_v9.so", RTLD_LAZY);
    if (!handle) {
        fprintf(stderr, "Failed to load library: %s\n", dlerror());
        return 1;
    }
    
    printf("✅ Library loaded successfully\n");
    dlclose(handle);
    return 0;
}
EOF
    fi
    
    # Create README
    cat > "$SDK_RELEASE_DIR/README.md" << 'EOF'
# GPUFabric Android SDK v9.0.0

## Overview
GPUFabric Android SDK is a high-performance LLM inference library integrated with llama.cpp engine.

## File Description
- `libs/libgpuf_c_sdk_v9.so` - Main dynamic library
- `libs/libc++_shared.so` - C++ runtime library
- `examples/` - Example code

## Quick Start
1. Push library files from `libs/` directory to Android device
2. Set LD_PRELOAD environment variable
3. Call JNI API or C API

## JNI API
- `Java_com_gpuf_c_GPUEngine_loadModel` - Load model
- `Java_com_gpuf_c_GPUEngine_createContext` - Create context
- `Java_com_gpuf_c_GPUEngine_getVersion` - Get version
- `Java_com_gpuf_c_GPUEngine_getSystemInfo` - Get system information
- `Java_com_gpuf_c_GPUEngine_cleanup` - Clean up resources

## C API
- `gpuf_init` - Initialize
- `gpuf_load_model` - Load model
- `gpuf_create_context` - Create context
- `gpuf_version` - Get version
- `gpuf_system_info` - Get system information
- `gpuf_cleanup` - Clean up resources

## Version Information
Version: 9.0.0-x86_64-android-FINAL-LLAMA-SOLUTION
Build Time: $(date)
Target Platform: Android ARM64
EOF
    
    # Create build script
    cat > "$SDK_RELEASE_DIR/build.sh" << 'EOF'
#!/bin/bash
echo "GPUFabric SDK Quick Deployment Script"
echo "========================="

# Check device connection
if ! adb devices | grep -q "device$"; then
    echo "❌ No Android device detected"
    exit 1
fi

# Push library files
echo "📤 Pushing library files to device..."
adb push libs/libgpuf_c_sdk_v9.so /data/local/tmp/libgpuf_c.so
adb push libs/libc++_shared.so /data/local/tmp/ndk_shared_libcpp.so

# Push examples
echo "📤 Pushing example programs..."
adb push examples/test_jni_symbols /data/local/tmp/

# Run tests
echo "🧪 Running functionality tests..."
adb shell "LD_PRELOAD=/data/local/tmp/ndk_shared_libcpp.so /data/local/tmp/test_jni_symbols"

echo "✅ Deployment completed!"
EOF
    
    chmod +x "$SDK_RELEASE_DIR/build.sh"
    
    # Create version information file
    cat > "$SDK_RELEASE_DIR/VERSION" << EOF
GPUFabric Android SDK
Version: 9.0.0-x86_64-android-FINAL-LLAMA-SOLUTION
Build Date: $(date)
Target: Android ARM64
API Level: 21
LLaMA.cpp: Integrated
JNI: Full Support
EOF
    
    # Create compressed archive for distribution
    handle_step "Creating compressed SDK archive..."
    cd "$WORKSPACE_ROOT/target/"
    ARCHIVE_NAME="gpufabric-android-sdk-v$SDK_VERSION.tar.gz"
    tar -czf "$ARCHIVE_NAME" "gpufabric-android-sdk-v$SDK_VERSION/"
    
    echo "📦 Created archive: $WORKSPACE_ROOT/target/$ARCHIVE_NAME"
    echo "📊 Archive size: $(du -sh $WORKSPACE_ROOT/target/$ARCHIVE_NAME | cut -f1)"
    
    handle_success "SDK package creation completed"
}

# Show Results
show_results() {
    echo ""
    echo -e "${GREEN}🎉 New version SDK generation completed!${NC}"
    echo ""
    echo "📁 SDK file locations:"
    echo "- Main library: $PROJECT_ROOT/libgpuf_c_sdk_v9.so"
    echo "- SDK package: $SDK_RELEASE_DIR"
    echo "- Archive: $WORKSPACE_ROOT/target/gpufabric-android-sdk-v$SDK_VERSION.tar.gz"
    echo ""
    echo "📊 SDK features:"
    echo "- ✅ Complete llama.cpp integration"
    echo "- ✅ Full-featured JNI API support"
    echo "- ✅ Android ARM64 optimization"
    echo "- ✅ Static linking of OpenMP"
    echo "- ✅ Minimal runtime dependencies"
    echo "- ✅ Versioned release packaging"
    echo "- ✅ Compressed distribution archive"
    echo ""
    echo "🚀 Usage instructions:"
    echo "1. cd $SDK_RELEASE_DIR"
    echo "2. ./build.sh"
    echo ""
    echo "📦 Distribution:"
    echo "Archive: $WORKSPACE_ROOT/target/gpufabric-android-sdk-v$SDK_VERSION.tar.gz"
    echo "Size: $(du -sh $WORKSPACE_ROOT/target/gpufabric-android-sdk-v$SDK_VERSION.tar.gz | cut -f1)"
    echo ""
    echo "📋 Library file information:"
    ls -lh $PROJECT_ROOT/libgpuf_c_sdk_v9.so
}

# Main Function
main() {
    echo "Starting new version SDK generation..."
    echo ""
    
    check_environment
    clean_build
    build_llama_android_libs
    build_rust_library
    extract_objects
    link_sdk
    verify_sdk
    create_sdk_package
    show_results
}

# Script Entry Point
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    main "$@"
fi
