# 🔧 GPUFabric Build Scripts

## 📁 Script Overview

This directory contains build scripts for the GPUFabric Android SDK project.

## 🚀 Main Scripts

### `build_arm64_with_android.sh`
- **Purpose**: Build Android ARM64 SDK with network support
- **Usage**: `./build_arm64_with_android.sh`
- **Output**: `libgpuf_c_sdk_v9.so` (33MB)
- **Features**: 
  - Complete llama.cpp integration
  - JNI API support
  - Android ARM64 optimization
  - Static linking

## 📦 Build Workflow

1. **Environment Setup**: Configure Android NDK and Rust toolchain
2. **llama.cpp Build**: Compile static libraries for Android
3. **Rust Compilation**: Build Rust static library
4. **Linking**: Create final dynamic library with all dependencies
5. **Verification**: Check symbols and functionality

## 🎯 Usage

### Quick Build
```bash
# Build Android ARM64 library
./scripts/build_arm64_with_android.sh
```

### Complete SDK Package
```bash
# Generate full SDK with examples and documentation
./generate_sdk.sh
```

## 📋 Requirements

- Android NDK r27d
- Rust toolchain (stable)
- CMake 3.16+
- Linux build environment

## 🔗 Related Files

- `../generate_sdk.sh` - Main SDK generation script
- `../build.rs` - Rust build configuration
- `../docs/` - Complete documentation

---

> 💡 **Note**: This script is optimized for Android ARM64 targets. For other platforms, use the main `generate_sdk.sh` script.
