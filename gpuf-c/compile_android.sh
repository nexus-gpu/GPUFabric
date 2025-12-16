#!/bin/bash

# Android NDK compilation script for android_test
set -e

echo "🔥 Compiling android_test for Android..."

# NDK paths
NDK_PATH="/home/jack/android-ndk-r27d"
if [ ! -d "$NDK_PATH" ]; then
    NDK_PATH="/home/jack/Android/Sdk/ndk/25.1.8937393"
fi

if [ ! -d "$NDK_PATH" ]; then
    echo "❌ Android NDK not found!"
    exit 1
fi

echo "📱 Using NDK: $NDK_PATH"

# Create build directory
mkdir -p build_android
cd build_android

# Copy minimal header
cp ../gpuf_c_minimal.h .

# Compile using clang directly
$NDK_PATH/toolchains/llvm/prebuilt/linux-x86_64/bin/aarch64-linux-android21-clang \
    -I.. \
    -I$NDK_PATH/sysroot/usr/include \
    -L.. \
    -lgpuf_c_sdk_v9 \
    -llog \
    -landroid \
    -pie \
    -o android_test ../examples/android_test.c

echo "✅ Compilation completed!"
echo "📦 Binary: build_android/android_test"
