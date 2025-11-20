# GPUFabric Mobile SDK Scripts

This directory contains all scripts for building and testing the GPUFabric Mobile SDK.

## 📁 Script Overview

### 🔧 `build_mobile.ps1` - Main Build Script
**Purpose**: Build Android and iOS library files
```powershell
# Build all platforms
.\build_mobile.ps1

# Build Android only
.\build_mobile.ps1 -Platform android

# Build iOS only (requires macOS)
.\build_mobile.ps1 -Platform ios
```

**Features**:
- ✅ Android NDK build (arm64-v8a, armeabi-v7a, x86_64)
- ✅ iOS build (aarch64-apple-ios, x86_64-apple-ios)
- ✅ Automatic UPX compression (if installed)
- ✅ Generate C header files

### ⚙️ `setup_ndk.ps1` - Environment Setup
**Purpose**: Configure Android NDK environment
```powershell
# Modify NDK_PATH in the script, then run
.\setup_ndk.ps1
```

**Features**:
- ✅ Check NDK installation
- ✅ Set ANDROID_NDK_HOME environment variable
- ✅ Verify configuration

### 📱 `test_android.ps1` - Test Preparation
**Purpose**: Prepare Android test files
```powershell
.\test_android.ps1
```

**Features**:
- ✅ Copy .so files to test directory
- ✅ Generate test project structure
- ✅ Verify file integrity

## 🚀 Quick Start

### 1. Environment Setup
```powershell
# Install NDK (if not already installed)
.\setup_ndk.ps1

# Install UPX (optional, for compression)
# Download: https://upx.github.io/
# Or run: winget install UPX
```

### 2. Build SDK
```powershell
# Build Android library
.\build_mobile.ps1 -Platform android

# Prepare test files
.\test_android.ps1
```

### 3. Testing
1. Open Android Studio
2. Import `C:\temp\android_test` project
3. Connect ARM64 device
4. Run tests

## 📂 Output Files

After build completion, important files are located at:

```
gpuf-c/
├── target/aarch64-linux-android/release/
│   └── libgpuf_c.so                    # Android ARM64 library
├── target/armv7-linux-androideabi/release/
│   └── libgpuf_c.so                    # Android ARMv7 library
├── target/x86_64-linux-android/release/
│   └── libgpuf_c.so                    # Android x86_64 library
└── gpuf_c.h                            # C header file

C:\temp\android_test\                    # Test project
├── jniLibs/arm64-v8a/libgpuf_c.so      # Test library files
└── README.md                            # Test instructions
```

## ⚠️ Important Notes

1. **Windows Only**: These scripts are designed for Windows PowerShell
2. **Admin Rights**: Some operations may require administrator privileges
3. **Network Required**: First build requires downloading dependencies
4. **Disk Space**: Complete build requires approximately 2GB space

## 🔍 Troubleshooting

### NDK Related Issues
```powershell
# Check if NDK is correctly configured
echo $env:ANDROID_NDK_HOME

# Reconfigure NDK
.\setup_ndk.ps1
```

### Build Failures
```powershell
# Clean build cache
cargo clean

# Rebuild
.\build_mobile.ps1 -Platform android
```

### UPX Compression Issues
```powershell
# Check if UPX is installed
upx --version

# Manual compression
upx --best --lzma libgpuf_c.so
```

## 📝 Changelog

- **2025-11-18**: Created scripts directory, organized build process
- **2025-11-18**: Added automatic UPX compression
- **2025-11-18**: Integrated llama.cpp support
