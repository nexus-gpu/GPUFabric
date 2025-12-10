#!/bin/bash
#
# Build and Test Script for Multimodal Android Testing
# This script compiles the test program and deploys it to an Android device
#

set -e

echo "🚀 GPUFabric Multimodal Android Test Builder"
echo "=============================================="
echo ""

# Configuration
NDK_ROOT="${NDK_ROOT:-/home/jack/android-ndk-r27d}"
PROJECT_ROOT="/home/jack/codedir/GPUFabric/gpuf-c"
WORKSPACE_ROOT="/home/jack/codedir/GPUFabric"
SDK_LIB="$PROJECT_ROOT/libgpuf_c_sdk_v9.so"
TEST_SOURCE="$PROJECT_ROOT/examples/test_multimodal_android.c"
TEST_BINARY="test_multimodal_android"

# Model paths
TEXT_MODEL="/home/jack/SmolVLM-500M-Instruct-Q8_0.gguf"
MMPROJ_MODEL="/home/jack/mmproj-SmolVLM-500M-Instruct-Q8_0.gguf"

# Device paths
DEVICE_DIR="/data/local/tmp"
DEVICE_SDK="$DEVICE_DIR/libgpuf_c_sdk_v9.so"
DEVICE_TEST="$DEVICE_DIR/$TEST_BINARY"
DEVICE_TEXT_MODEL="$DEVICE_DIR/SmolVLM-500M-Instruct-Q8_0.gguf"
DEVICE_MMPROJ="$DEVICE_DIR/mmproj-SmolVLM-500M-Instruct-Q8_0.gguf"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

print_step() {
    echo -e "${BLUE}🔧 $1${NC}"
}

print_success() {
    echo -e "${GREEN}✅ $1${NC}"
}

print_error() {
    echo -e "${RED}❌ $1${NC}"
}

print_warning() {
    echo -e "${YELLOW}⚠️  $1${NC}"
}

# Check prerequisites
check_prerequisites() {
    print_step "Checking prerequisites..."
    
    if [ ! -d "$NDK_ROOT" ]; then
        print_error "Android NDK not found at: $NDK_ROOT"
        echo "Please set NDK_ROOT environment variable"
        exit 1
    fi
    
    if [ ! -f "$SDK_LIB" ]; then
        print_error "SDK library not found: $SDK_LIB"
        echo "Please run ./generate_sdk.sh first"
        exit 1
    fi
    
    if [ ! -f "$TEST_SOURCE" ]; then
        print_error "Test source not found: $TEST_SOURCE"
        exit 1
    fi
    
    if [ ! -f "$TEXT_MODEL" ]; then
        print_warning "Text model not found: $TEXT_MODEL"
        echo "You'll need to provide model files manually"
    fi
    
    if [ ! -f "$MMPROJ_MODEL" ]; then
        print_warning "MMProj model not found: $MMPROJ_MODEL"
        echo "You'll need to provide model files manually"
    fi
    
    # Check if device is connected
    if ! adb devices | grep -q "device$"; then
        print_error "No Android device connected"
        echo "Please connect a device via ADB"
        exit 1
    fi
    
    print_success "Prerequisites check passed"
}

# Compile test program
compile_test() {
    print_step "Compiling test program..."
    
    cd "$PROJECT_ROOT/examples"
    
    CLANG="$NDK_ROOT/toolchains/llvm/prebuilt/linux-x86_64/bin/aarch64-linux-android21-clang"
    
    $CLANG \
        test_multimodal_android.c \
        -o $TEST_BINARY \
        -L"$PROJECT_ROOT" \
        -lgpuf_c_sdk_v9 \
        -llog -ldl -lm \
        -pie \
        -Wl,-rpath,'$ORIGIN'
    
    if [ $? -ne 0 ]; then
        print_error "Compilation failed"
        exit 1
    fi
    
    print_success "Test program compiled: $TEST_BINARY"
    ls -lh $TEST_BINARY
}

# Push files to device
push_to_device() {
    print_step "Pushing files to Android device..."
    
    # Push SDK library
    echo "Pushing SDK library..."
    adb push "$SDK_LIB" "$DEVICE_SDK"
    
    # Push test binary
    echo "Pushing test binary..."
    adb push "$PROJECT_ROOT/examples/$TEST_BINARY" "$DEVICE_TEST"
    adb shell chmod +x "$DEVICE_TEST"
    
    # Push models if available
    if [ -f "$TEXT_MODEL" ]; then
        echo "Pushing text model (this may take a while)..."
        adb push "$TEXT_MODEL" "$DEVICE_TEXT_MODEL"
    else
        print_warning "Skipping text model push (file not found)"
    fi
    
    if [ -f "$MMPROJ_MODEL" ]; then
        echo "Pushing mmproj model..."
        adb push "$MMPROJ_MODEL" "$DEVICE_MMPROJ"
    else
        print_warning "Skipping mmproj model push (file not found)"
    fi
    
    print_success "Files pushed to device"
}

# Verify files on device
verify_device_files() {
    print_step "Verifying files on device..."
    
    echo "Files in $DEVICE_DIR:"
    adb shell "ls -lh $DEVICE_DIR/*.so $DEVICE_DIR/$TEST_BINARY $DEVICE_DIR/*.gguf 2>/dev/null || true"
    
    # Check if models exist
    if ! adb shell "[ -f $DEVICE_TEXT_MODEL ]" 2>/dev/null; then
        print_warning "Text model not found on device"
    fi
    
    if ! adb shell "[ -f $DEVICE_MMPROJ ]" 2>/dev/null; then
        print_warning "MMProj model not found on device"
    fi
    
    print_success "File verification complete"
}

# Run test on device
run_test() {
    print_step "Running test on Android device..."
    echo ""
    echo "╔════════════════════════════════════════════════════════════╗"
    echo "║  Starting Multimodal Test on Device                       ║"
    echo "╚════════════════════════════════════════════════════════════╝"
    echo ""
    
    # Run the test with proper library path
    adb shell "cd $DEVICE_DIR && LD_LIBRARY_PATH=. ./$TEST_BINARY"
    
    TEST_RESULT=$?
    
    echo ""
    if [ $TEST_RESULT -eq 0 ]; then
        print_success "Test completed successfully"
    else
        print_error "Test failed with exit code: $TEST_RESULT"
    fi
    
    return $TEST_RESULT
}

# Collect logs
collect_logs() {
    print_step "Collecting device logs..."
    
    LOG_FILE="$PROJECT_ROOT/multimodal_test_$(date +%Y%m%d_%H%M%S).log"
    
    echo "Saving logcat to: $LOG_FILE"
    adb logcat -d | grep -E "(GPUFabric|llama|mtmd|ggml)" > "$LOG_FILE" || true
    
    if [ -s "$LOG_FILE" ]; then
        print_success "Logs saved to: $LOG_FILE"
        echo "Last 20 lines:"
        tail -20 "$LOG_FILE"
    else
        print_warning "No relevant logs found"
    fi
}

# Cleanup device files (optional)
cleanup_device() {
    if [ "$1" = "--cleanup" ]; then
        print_step "Cleaning up device files..."
        
        adb shell "rm -f $DEVICE_TEST"
        # Optionally remove models (they're large)
        # adb shell "rm -f $DEVICE_TEXT_MODEL $DEVICE_MMPROJ"
        
        print_success "Cleanup complete"
    fi
}

# Main execution
main() {
    echo "Starting build and test process..."
    echo ""
    
    check_prerequisites
    compile_test
    push_to_device
    verify_device_files
    
    echo ""
    read -p "Press Enter to run test on device (or Ctrl+C to cancel)..."
    echo ""
    
    run_test
    TEST_EXIT_CODE=$?
    
    collect_logs
    cleanup_device "$1"
    
    echo ""
    echo "╔════════════════════════════════════════════════════════════╗"
    if [ $TEST_EXIT_CODE -eq 0 ]; then
        echo "║  ✅ BUILD AND TEST COMPLETED SUCCESSFULLY                 ║"
    else
        echo "║  ❌ BUILD AND TEST FAILED                                 ║"
    fi
    echo "╚════════════════════════════════════════════════════════════╝"
    echo ""
    
    exit $TEST_EXIT_CODE
}

# Show usage
if [ "$1" = "--help" ] || [ "$1" = "-h" ]; then
    echo "Usage: $0 [--cleanup]"
    echo ""
    echo "Options:"
    echo "  --cleanup    Remove test binary from device after running"
    echo "  --help       Show this help message"
    echo ""
    echo "Environment variables:"
    echo "  NDK_ROOT     Path to Android NDK (default: /home/jack/android-ndk-r27d)"
    echo ""
    exit 0
fi

main "$@"
