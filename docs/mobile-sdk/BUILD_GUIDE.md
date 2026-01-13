# GPUFabric Mobile SDK Build Guide

This guide provides detailed instructions for building GPUFabric Mobile SDK, supporting Android and iOS platforms.

## 📋 Table of Contents

- [Environment Requirements](#environment-requirements)
- [Quick Start](#quick-start)
- [Detailed Build Steps](#detailed-build-steps)
- [Script Documentation](#script-documentation)
- [Troubleshooting](#troubleshooting)

## 🔧 Environment Requirements

### Basic Requirements
- **Operating System**: Windows 10/11 (build), macOS (iOS build)
- **Rust**: 1.70+ 
- **PowerShell**: 5.1+ (Windows)

### Android Requirements
- **Android Studio**: Latest version
- **Android NDK**: r21+ (recommended r26d)
- **Android SDK**: API 24+ 
- **CMake**: 3.18+ (auto-installed)

### iOS Requirements (macOS only)
- **Xcode**: 14.0+
- **iOS SDK**: 14.0+
- **Rust iOS targets**: `aarch64-apple-ios`, `x86_64-apple-ios`

### Optional Tools
- **UPX**: For compressing .so files (recommended)
- **cbindgen**: For generating C header files (auto-installed)

## 🚀 Quick Start

### 1. Clone Project
```bash
git clone https://github.com/your-repo/GPUFabric.git
cd GPUFabric/gpuf-c
```

### 2. Environment Setup
```powershell
# Navigate to scripts directory
cd scripts

# Configure Android NDK
.\setup_ndk.ps1

# Install UPX (optional)
winget install UPX
```

### 3. Build SDK
```powershell
# Build Android
.\build_mobile.ps1 -Platform android

# Build all platforms (requires macOS)
.\build_mobile.ps1 -Platform all
```

### 4. Prepare Tests
```powershell
# Prepare Android test files
.\test_android.ps1
```

## 📖 Detailed Build Steps

### Android Build

#### Step 1: Install Android NDK

1. Open Android Studio
2. Go to `Tools` → `SDK Manager` → `SDK Tools`
3. Check `NDK (Side by side)` and `CMake`
4. Click `Apply` to install

#### Step 2: Set Environment Variables
```powershell
# Manual setup (if script fails)
$env:ANDROID_NDK_HOME = "C:\Users\admin\AppData\Local\Android\Sdk\ndk\26.1.10909125"

# Permanent setup
[System.Environment]::SetEnvironmentVariable('ANDROID_NDK_HOME', "C:\Users\admin\AppData\Local\Android\Sdk\ndk\26.1.10909125", 'User')
```

#### Step 3: Install Rust Targets
```bash
rustup target add aarch64-linux-android
rustup target add armv7-linux-androideabi  
rustup target add x86_64-linux-android
```

#### Step 4: Build Library Files
```powershell
# Navigate to gpuf-c directory
cd D:\codedir\GPUFabric\gpuf-c

# Build Android libraries
cargo ndk -t arm64-v8a -t armeabi-v7a -t x86_64 build --release --features vulkan

# Or use script
cd scripts
.\build_mobile.ps1 -Platform android
```

### iOS Build (macOS only)

#### Step 1: Install iOS Targets
```bash
rustup target add aarch64-apple-ios
rustup target add x86_64-apple-ios
rustup target add aarch64-apple-ios-sim
```

#### Step 2: Build iOS Libraries
```bash
# Build iOS device libraries
cargo build --target aarch64-apple-ios --release --features metal
cargo build --target x86_64-apple-ios --release --features metal

# Build iOS simulator libraries
cargo build --target aarch64-apple-ios-sim --release --features metal
```

## 📁 Output Files

### Android Output
```
target/
├── aarch64-linux-android/release/
│   └── libgpuf_c.so              # ARM64 library (2.2MB → ~1.5MB compressed)
├── armv7-linux-androideabi/release/
│   └── libgpuf_c.so              # ARMv7 library
└── x86_64-linux-android/release/
    └── libgpuf_c.so              # x86_64 library
```

### iOS Output
```
target/
├── aarch64-apple-ios/release/
│   └── libgpuf_c.a               # iOS ARM64 static library
├── x86_64-apple-ios/release/
│   └── libgpuf_c.a               # iOS x86_64 static library
└── aarch64-apple-ios-sim/release/
    └── libgpuf_c.a               # iOS simulator static library
```

### Header Files
```
gpuf_c.h                           # C header file (common for all platforms)
```

## 📜 Script Documentation

### build_mobile.ps1
**Purpose**: Main build script supporting multi-platform builds and auto-compression

**Parameters**:
- `-Platform`: `android`, `ios`, `all` (default: `all`)

**Features**:
- ✅ Automatic NDK environment detection
- ✅ Support for multiple GPU backends (Vulkan/Metal/CPU)
- ✅ Automatic UPX compression (if available)
- ✅ C header file generation
- ✅ Parallel multi-architecture builds

**Usage Examples**:
```powershell
# Build Android (Vulkan acceleration)
.\build_mobile.ps1 -Platform android

# Build iOS (Metal acceleration) 
.\build_mobile.ps1 -Platform ios

# Build all platforms
.\build_mobile.ps1 -Platform all
```

### setup_ndk.ps1
**Purpose**: Android NDK environment configuration

**Steps**:
1. Check NDK installation status
2. Set `ANDROID_NDK_HOME` environment variable
3. Verify configuration validity
4. Provide installation guide

### test_android.ps1
**Purpose**: Android test environment preparation

**Features**:
- ✅ Copy .so files to test directory
- ✅ Generate Android Studio project structure
- ✅ Create sample code
- ✅ Verify file integrity

## ⚡ Performance Optimization

### UPX Compression
```bash
# Manual compression (higher compression ratio)
upx --best --lzma libgpuf_c.so

# Compression effect
# Original: 2.23 MB
# Compressed: 1.49 MB (33% savings)
```

### Build Optimization
```bash
# Release build (highest performance)
cargo ndk build --release

# Reduce size (remove debug info)
cargo ndk build --release
# (Rust strips debug symbols by default in release)

# Feature selection
--features vulkan    # GPU acceleration (Android)
--features metal     # GPU acceleration (iOS)  
--features cpu       # CPU only (best compatibility)
```

## 🔧 GPU Acceleration Configuration

### Android (Vulkan)
```toml
# Cargo.toml
[dependencies]
llama-cpp-2 = { version = "0.1", features = ["vulkan"] }
```

### iOS (Metal)
```toml
# Cargo.toml  
[dependencies]
llama-cpp-2 = { version = "0.1", features = ["metal"] }
```

### CPU Only (Universal)
```toml
# Cargo.toml
[dependencies] 
llama-cpp-2 = { version = "0.1", features = ["cpu"] }
```

## 🐛 Troubleshooting

### Common Errors

#### 1. "Could not find any NDK"
```powershell
# Solution
echo $env:ANDROID_NDK_HOME  # Check path
.\setup_ndk.ps1             # Reconfigure
```

#### 2. "Missing dependency: cmake"  
```powershell
# Solution - install via Android Studio
# Or manual download: https://cmake.org/download/
```

#### 3. "Unsupported target architecture"
```bash
# Solution - install correct targets
rustup target add aarch64-linux-android
rustup target add armv7-linux-androideabi
rustup target add x86_64-linux-android
```

#### 4. "UPX compression failed"
```bash
# Some .so files cannot be compressed, can skip
# Or try different parameters
upx --best libgpuf_c.so
upx --lzma libgpuf_c.so
```

### Debugging Tips

#### Enable Verbose Logging
```bash
# Rust build logs
RUST_LOG=debug cargo ndk build --release

# NDK build logs  
cargo ndk build --release --verbose
```

#### Check Library Dependencies
```bash
# Android
readelf -d libgpuf_c.so | grep NEEDED

# iOS  
otool -L libgpuf_c.a

# Strip debug symbols (newer NDK versions use llvm-strip)
llvm-strip libgpuf_c.so
```

#### Verify Symbol Exports
```bash
# Check exported C functions
nm -D libgpuf_c.so | grep gpuf_
```

## 📊 Architecture Compatibility

### Android
| Architecture | ABI | Device Support | Recommendation |
|--------------|-----|----------------|----------------|
| ARM64 | arm64-v8a | Modern phones/tablets | ⭐⭐⭐⭐⭐ |
| ARMv7 | armeabi-v7a | Older/low-end devices | ⭐⭐⭐ |
| x86_64 | x86_64 | Emulators/ChromeOS | ⭐⭐ |

### iOS
| Architecture | Device Support | Recommendation |
|--------------|----------------|----------------|
| ARM64 | iPhone 6s+/iPad Air 2+ | ⭐⭐⭐⭐⭐ |
| x86_64 | Simulators/Mac Catalyst | ⭐⭐⭐ |

## 📚 Related Documentation

- [Mobile SDK Index](./README.md)
- [Integration Guide (EN)](./INTEGRATION_GUIDE_EN.md)
- [Example Projects](../../gpuf-c/examples/mobile/)

## 🤝 Contributing

Welcome to submit Issues and Pull Requests to improve the build process!

## 📄 License

This project is licensed under the MIT License. See [LICENSE](../../LICENSE) file for details.
