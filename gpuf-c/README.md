# GPUFabric Android SDK

High-performance LLM inference library for Android with integrated llama.cpp engine and full JNI support.

## 🚀 Quick Start

All commands below are intended to be run from the `GPUFabric/gpuf-c` directory.

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

## 🌐 Networking: SSE & P2P

This crate can act as a compute node in the GPUFabric network.

- **SSE (OpenAI-compatible streaming)** is provided by **gpuf-s** over HTTP (`/v1/chat/completions` with `"stream": true`).
  - Streaming deltas are split into:
    - `delta.reasoning_content` (analysis/thinking)
    - `delta.content` (final answer)
  - gpuf-c is responsible for producing and reporting phase-aware chunks upstream to gpuf-s.

- **P2P inference** is supported in the gpuf-c protocol for direct peer streaming.
  - See example: `examples/p2p_sdk_client.rs`

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

## 🔌 Optional DLLM Plugin Probe

Linux/macOS `gpuf-c` builds can probe a local DLLM shared library at startup:

```bash
cargo run --release --bin gpuf-c -- \
  --client-id <client-id-32-hex> \
  --dllm-enable \
  --dllm-lib-path /opt/dllm/lib/libdllm.so \
  --dllm-server-key 0xA1FDFFFFFF01FAFAFAFA
```

This only verifies the DLLM C ABI and `server-key` parsing. It does not put
DLLM calls in the inference hot path, and failure falls back to normal
GPUFabric behavior.

## 📦 SDK Contents

- `libgpuf_c_sdk_v9.so` - Main library (51MB)
- `libc++_shared.so` - Android C++ runtime
- `gpuf_c.h` - C header file
- Java/C examples and documentation

## 📄 License

[License information]

---

> 📖 **Documentation**: See `docs/` directory for detailed guides and API references.
