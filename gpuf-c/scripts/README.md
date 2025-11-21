# 🔧 GPUFabric Build Scripts

This directory contains build and deployment scripts for the GPUFabric project.

## 📁 Script Overview

### 🚀 `build_mobile.ps1` - Mobile Platform Build Script
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
- Automatic NDK and toolchain detection
- Cross-compile Android ARM64/x86_64
- Generate iOS static library (macOS)
- Output artifacts to `target/mobile/`

### ⚙️ `setup_ndk.ps1` - NDK Environment Configuration
**Purpose**: Automatically download and configure Android NDK
```powershell
# Install latest NDK
.\setup_ndk.ps1

# Specify NDK version
.\setup_ndk.ps1 -Version 21
```

**Features**:
- Download Android NDK 21+
- Automatically configure environment variables
- Verify toolchain integrity

### ✅ `verify_client_sdk.ps1` - SDK Integration Verification
**Purpose**: Verify client SDK integration status
```powershell
# Complete verification
.\verify_client_sdk.ps1

# Quick check
.\verify_client_sdk.ps1 -Quick
```

**Verification Items**:
- Compilation environment check
- Dependency library integrity
- Platform compatibility testing
- Example code execution

## 🛠️ Environment Requirements

### Basic Environment
- PowerShell 5.1+ (Windows) or PowerShell Core 7+
- Rust 1.70+ with Cargo
- Git

### Platform-Specific Requirements
- **Android Development**: Android Studio or Android SDK
- **iOS Development**: Xcode 14+ (macOS only)
- **Linux Development**: GCC/Clang toolchain

## 🚀 Quick Start

### 1. Environment Preparation
```powershell
# Install Rust (if not already installed)
winget install Rustlang.Rust.MSVC

# Clone project
git clone https://github.com/your-org/GPUFabric.git
cd GPUFabric/gpuf-c
```

### 2. Build Mobile Libraries
```powershell
# Build Android library
.\scripts\build_mobile.ps1 -Platform android

# Build iOS library (macOS)
.\scripts\build_mobile.ps1 -Platform ios
```

### 3. Verify Integration
```powershell
# Run complete verification
.\scripts\verify_client_sdk.ps1
```

## 📦 Build Artifacts

After successful build, artifacts are located at:
```
target/mobile/
├── android/
│   ├── arm64-v8a/
│   │   └── libgpuf_c.so
│   ├── x86_64/
│   │   └── libgpuf_c.so
│   └── java/
│       └── GPUFabricClientSDK.java
└── ios/ (macOS only)
    ├── libgpuf_c.a
    └── GPUFabricClientSDK.h
```

## 🔧 Troubleshooting

### Common Issues

**Q: NDK download failed**
```powershell
# Manually set NDK path
$env:ANDROID_NDK_ROOT = "C:\Android\NDK\21.4.7075529"
```

**Q: iOS build failed**
- Ensure running on macOS
- Check Xcode command line tools: `xcode-select --install`

**Q: Cross-compilation error**
```powershell
# Clean and rebuild
cargo clean
.\scripts\build_mobile.ps1 -Clean
```

### Debug Mode
```powershell
# Enable verbose output
.\scripts\build_mobile.ps1 -Verbose

# Debug mode
.\scripts\build_mobile.ps1 -Debug
```

## 📋 Script Parameters

### build_mobile.ps1
| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| Platform | String | all | Build platform (android/ios/all) |
| Clean | Switch | false | Clean build cache |
| Debug | Switch | false | Enable debug mode |
| Verbose | Switch | false | Verbose output |

### setup_ndk.ps1
| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| Version | String | 21 | NDK version |
| Force | Switch | false | Force reinstall |

### verify_client_sdk.ps1
| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| Quick | Switch | false | Quick verification mode |
| Platform | String | all | Verification platform |

## 🤝 Contributing

When adding new scripts:
1. Follow existing naming conventions
2. Add detailed comments and help information
3. Include error handling and logging
4. Update this README file

---

*Last updated: 2025-11-21*
