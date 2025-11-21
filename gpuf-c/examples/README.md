# 📚 GPUFabric Examples

This directory contains usage examples and test cases for the GPUFabric SDK.

## 📁 Directory Structure

```
examples/
├── device_info/                    # Device information collection examples
│   └── test_device_info_collection.rs    # Real-time device info collection test
├── platform_testing/              # Platform compatibility tests
│   ├── test_vulkan_device.rs            # Vulkan device test
│   ├── test_cross_platform_vulkan.rs    # Cross-platform Vulkan test
│   └── test_rocm_gpu_metrics.rs         # ROCm GPU metrics test
├── mobile/                         # Mobile platform examples
│   └── test_android_device_info.rs      # Android device info test
├── android/                        # Android integration examples
│   ├── GPUFabricClientSDK.java          # Complete Android SDK interface
│   └── GPUFabricClientExample.java      # Android usage example
├── rust/                           # Rust test examples
│   └── test_client_sdk.rs               # Complete functionality test
└── README.md                       # This file
```

## 🚀 Quick Start

### Device Information Collection
```bash
# Test real-time device information collection
cargo run --example test_device_info_collection

# Test Android device information
cargo run --example test_android_device_info
```

### Platform Compatibility Tests
```bash
# Test Vulkan device support
cargo run --example test_vulkan_device --features vulkan

# Test cross-platform Vulkan support
cargo run --example test_cross_platform_vulkan --features vulkan

# Test ROCm GPU metrics (Linux + AMD GPU)
cargo run --example test_rocm_gpu_metrics --features rocm
```

### Mobile Platform Integration
Refer to the Java example code in the `android/` directory to learn how to integrate GPUFabric SDK into Android applications.

## 📋 Example Descriptions

### 🔧 Device Information Tests
- **test_device_info_collection.rs**: Tests real-time device information collection, validates cache-free architecture
- **test_android_device_info.rs**: Specifically tests device information collection on Android platform

### 🎯 Platform Compatibility
- **test_vulkan_device.rs**: Basic Vulkan device detection and functionality testing
- **test_cross_platform_vulkan.rs**: Cross-platform Vulkan API compatibility verification
- **test_rocm_gpu_metrics.rs**: AMD GPU ROCm SMI metrics collection testing

### 📱 Mobile Integration
- **GPUFabricClientSDK.java**: Complete Android SDK wrapper
- **GPUFabricClientExample.java**: Android application integration example

## 🛠️ Requirements

### Basic Requirements
- Rust 1.70+
- CMake 3.16+

### Platform-Specific Requirements
- **Windows**: Visual Studio Build Tools
- **Linux**: Development tools (`build-essential`)
- **Android**: Android NDK 21+
- **ROCm testing**: AMD GPU + ROCm 5.0+

### Feature Flags
```bash
# Enable Vulkan support
--features vulkan

# Enable ROCm support
--features rocm

# Enable NVML support
--features nvml
```

## 📊 Test Coverage

| Platform | Device Info | Vulkan | ROCm | Android |
|----------|-------------|--------|------|---------|
| Windows | ✅ | ✅ | ❌ | ❌ |
| Linux | ✅ | ✅ | ✅ | ❌ |
| Android | ✅ | ✅ | ❌ | ✅ |
| macOS | ✅ | ✅ | ❌ | ❌ |

## 🤝 Contributing Guidelines

When adding new examples:
1. Choose the appropriate subdirectory
2. Add detailed comments and documentation
3. Update this README file
4. Ensure cross-platform compatibility

---

*Last updated: 2025-11-21*
