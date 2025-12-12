# GPUFabric Android SDK

High-performance LLM inference library for Android with integrated llama.cpp engine and full JNI support.

## 🚀 Quick Start

```bash
# Generate Android SDK
./generate_sdk.sh

# Deploy to device
cd ../target/gpufabric-android-sdk-v9.0.0
./build.sh
```

## 📁 Project Structure

```
GPUFabric/
├── gpuf-c/                    # Main Android library
│   ├── src/                   # Rust source code
│   ├── generate_sdk.sh        # SDK build script
│   ├── build.rs               # Build configuration
│   └── docs/                  # Documentation
├── target/                    # Build outputs
│   ├── gpufabric-android-sdk-v9.0.0/    # Release SDK
│   ├── llama-android-ndk/     # llama.cpp libraries
│   └── models/                # Model files
└── llama.cpp/                 # llama.cpp source
```

## 📚 Documentation

- **[Quick Start Guide](docs/QUICK_START.md)** - Get started in minutes
- **[Project Overview](docs/README_PROJECT.md)** - Detailed project information
- **[Android Build Guide](docs/ANDROID_BUILD_LESSONS_LEARNED.md)** - Build lessons and best practices
- **[JNI Network Guide](docs/ANDROID_JNI_NETWORK_BUILD_GUIDE.md)** - Network integration guide
- **[Deployment Guide](docs/ANDROID_X86_64_DEPLOYMENT_GUIDE.md)** - Multi-platform deployment

## 🎯 Features

- ✅ **Complete llama.cpp integration** - Latest LLaMA.cpp engine
- ✅ **Full-featured JNI API** - Java/Kotlin native interface
- ✅ **Android ARM64 optimization** - Native ARM64 performance
- ✅ **Static linking** - Minimal runtime dependencies
- ✅ **Multi-threading support** - Parallel inference
- ✅ **Memory optimization** - Efficient memory management

## 📋 Requirements

- Android NDK r27d
- Rust toolchain (stable)
- CMake 3.16+
- Linux build environment

## 🔧 Build

```bash
# Clean and build
./generate_sdk.sh

# Output: target/gpufabric-android-sdk-v9.0.0.tar.gz
```

## 📦 SDK Contents

- `libgpuf_c_sdk_v9.so` - Main library (51MB)
- `libc++_shared.so` - Android C++ runtime
- `gpuf_c.h` - C header file
- Java/C examples and documentation

## 📄 License

[License information]

---

> 📖 **Documentation**: See `docs/` directory for detailed guides and API references.
