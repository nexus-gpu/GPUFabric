# 🔨 Build Guide

## 🎯 Overview

GPUFabric supports multi-platform builds, including Android, Windows, Linux, and macOS. This guide provides detailed build steps and configuration instructions.

## 📋 System Requirements

### Basic Environment
- **Rust**: 1.70+ (recommended to install with rustup)
- **Git**: For code management
- **CMake**: 3.16+ (required for llama.cpp build)
- **Python**: 3.8+ (required for some dependencies)

### Platform-Specific Requirements

#### Windows
- **Visual Studio**: 2019+ or Build Tools
- **Windows SDK**: 10.0+
- **LLVM**: Optional, for optimized compilation

#### Linux
- **GCC**: 9.0+ or Clang 10.0+
- **build-essential**: Contains necessary build tools
- **pkg-config**: For dependency management

#### macOS
- **Xcode**: 12.0+
- **Command Line Tools**: Latest version
- **Homebrew**: Recommended for dependency management

#### Android
- **Android Studio**: 4.0+
- **Android NDK**: 21+
- **Android SDK**: API Level 21+

## 🚀 Quick Start

### Clone and Build
```bash
# Clone repository
git clone https://github.com/your-org/GPUFabric.git
cd GPUFabric/gpuf-c

# Build release version
cargo build --release

# Run tests
cargo test
```

## 🔧 Build Options

### Feature Flags
```bash
# CPU only (minimal)
cargo build --release --features cpu

# Vulkan support (cross-platform GPU)
cargo build --release --features "cpu,vulkan"

# CUDA support (NVIDIA GPU)
cargo build --release --features "cpu,cuda"

# ROCm support (AMD GPU)
cargo build --release --features "cpu,rocm"

# Metal support (Apple GPU)
cargo build --release --features "cpu,metal"

# Full features (all GPU backends)
cargo build --release --features "cpu,vulkan,cuda,rocm,metal"
```

### Android Builds
```bash
# Android ARM64
cargo ndk -t arm64-v8a build --release --features android

# Android ARMv7
cargo ndk -t armeabi-v7a build --release --features android

# Android x86_64
cargo ndk -t x86_64 build --release --features android
```

## 📱 Android Build Guide

### Prerequisites
1. Install Android Studio
2. Install Android NDK 21+
3. Set ANDROID_NDK_HOME environment variable

### Build Steps
```bash
# Set NDK path
export ANDROID_NDK_HOME="/path/to/android/ndk"

# Build for all Android architectures
./scripts/build_mobile.ps1 -Platform android

# Build specific architecture
cargo ndk -t arm64-v8a build --release --features android
```

### Android Optimized Variants
```bash
# Full version (83.5 MB)
cargo ndk -t arm64-v8a build --release --features android

# Balanced version (25-35 MB)
cargo ndk -t arm64-v8a build --release --features android-balanced

# Minimal version (15-25 MB)
cargo ndk -t arm64-v8a build --release --features android-minimal
```

## 🖥️ Platform-Specific Instructions

### Windows
```powershell
# Install Visual Studio Build Tools
winget install Microsoft.VisualStudio.2022.BuildTools

# Install LLVM (optional)
winget install LLVM.LLVM

# Build with MSVC
cargo build --release --target x86_64-pc-windows-msvc
```

### Linux
```bash
# Install dependencies
sudo apt-get update
sudo apt-get install build-essential cmake pkg-config

# Build
cargo build --release

# Install Vulkan drivers (Ubuntu/Debian)
sudo apt-get install libvulkan-dev vulkan-tools
```

### macOS
```bash
# Install Xcode Command Line Tools
xcode-select --install

# Install dependencies with Homebrew
brew install cmake

# Build
cargo build --release

# For Metal support, ensure Xcode is installed
```

## 🎯 GPU Acceleration Setup

### Vulkan Setup
```bash
# Ubuntu/Debian
sudo apt-get install libvulkan-dev vulkan-tools

# Fedora
sudo dnf install vulkan-devel vulkan-tools

# Arch Linux
sudo pacman -S vulkan-devel vulkan-tools

# Windows
# Install Vulkan SDK from https://vulkan.lunarg.com/
```

### CUDA Setup
```bash
# Install CUDA Toolkit (Linux)
wget https://developer.download.nvidia.com/compute/cuda/repos/ubuntu2004/x86_64/cuda-ubuntu2004.pin
sudo mv cuda-ubuntu2004.pin /etc/apt/preferences.d/cuda-repository-pin-600
wget https://developer.download.nvidia.com/compute/cuda/12.1.0/local_installers/cuda-repo-ubuntu2004-12-1-local_12.1.0-530.30.02-1_amd64.deb
sudo dpkg -i cuda-repo-ubuntu2004-12-1-local_12.1.0-530.30.02-1_amd64.deb
sudo cp /var/cuda-repo-ubuntu2004-12-1-local/cuda-*-keyring.gpg /usr/share/keyrings/
sudo apt-get update
sudo apt-get install cuda

# Set environment variables
export PATH=/usr/local/cuda/bin:$PATH
export LD_LIBRARY_PATH=/usr/local/cuda/lib64:$LD_LIBRARY_PATH
```

### ROCm Setup
```bash
# Ubuntu/Debian
sudo apt-get update
sudo apt-get install rocm-dkms

# Add user to render group
sudo usermod -a -G render,video $LOGNAME

# Set environment variables
export PATH=/opt/rocm/bin:$PATH
export LD_LIBRARY_PATH=/opt/rocm/lib:$LD_LIBRARY_PATH
```

## 🔨 Build Optimization

### Compiler Optimizations
```bash
# Native CPU optimizations
export RUSTFLAGS="-C target-cpu=native"

# Link-time optimizations
export RUSTFLAGS="-C target-cpu=native -C link-arg=-flto"

# Build with optimizations
cargo build --release

# Profile-guided optimization (advanced)
cargo build --release --config profile.release.lto = true
```

### Size Optimization
```bash
# Strip debug symbols
cargo build --release
strip target/release/gpuf-c

# Use UPX compression (if installed)
upx --best target/release/gpuf-c

# Minimize binary size
cargo build --release --config profile.release.strip = true
```

## 🧪 Testing and Verification

### Run Tests
```bash
# Unit tests
cargo test

# Integration tests
cargo test --test integration_tests

# Documentation tests
cargo test --doc

# Benchmark tests
cargo test --release --features benchmarks
```

### Verification Scripts
```powershell
# Windows
.\scripts\verify_client_sdk.ps1

# Cross-platform verification
cargo run --example test_device_info_collection
cargo run --example test_client_sdk
```

## 📦 Build Artifacts

### Output Locations
```
target/
├── release/
│   ├── gpuf-c                 # Main executable
│   ├── libgpuf_c.a            # Static library
│   └── libgpuf_c.so           # Shared library (Linux)
├── aarch64-linux-android/
│   └── release/
│       └── libgpuf_c.so       # Android ARM64 library
└── x86_64-pc-windows-msvc/
    └── release/
        └── gpuf-c.exe         # Windows executable
```

### Package Creation
```bash
# Create distribution package
cargo build --release
mkdir dist
cp target/release/gpuf-c dist/
cp -r examples dist/
cp -r docs dist/
tar -czf gpuf-c-v1.0.0.tar.gz dist/
```

## 🐛 Troubleshooting

### Common Issues

**Build fails with linking errors**
```bash
# Clean and rebuild
cargo clean
cargo build --release

# Check for missing dependencies
cargo check
```

**Vulkan not found**
```bash
# Verify Vulkan installation
vulkaninfo

# Check library paths
find /usr -name "libvulkan*" 2>/dev/null
```

**Android NDK issues**
```bash
# Verify NDK installation
echo $ANDROID_NDK_HOME
ls $ANDROID_NDK_HOME

# Rebuild with clean
cargo clean
cargo ndk -t arm64-v8a build --release --features android
```

**CUDA compilation errors**
```bash
# Check CUDA installation
nvcc --version
nvidia-smi

# Set correct CUDA path
export CUDA_HOME=/usr/local/cuda
export PATH=$CUDA_HOME/bin:$PATH
export LD_LIBRARY_PATH=$CUDA_HOME/lib64:$LD_LIBRARY_PATH
```

## 📚 Additional Resources

- [Rust Book](https://doc.rust-lang.org/book/)
- [Cargo Book](https://doc.rust-lang.org/cargo/)
- [Vulkan Specification](https://www.khronos.org/vulkan/)
- [CUDA Toolkit Documentation](https://docs.nvidia.com/cuda/)
- [Android NDK Guide](https://developer.android.com/ndk/guides)

---

*Last updated: 2025-11-21*
