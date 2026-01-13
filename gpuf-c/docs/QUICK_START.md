# 🚀 GPUFabric Android Quick Start Guide

## 📋 Project Structure

```
gpuf-c/
├── docs/           # 📚 Documentation directory
│   ├── README.md
│   ├── BUILD_GUIDE.md
│   ├── STREAMING_API_GUIDE.md
│   └── ANDROID_X86_64_DEPLOYMENT_GUIDE.md
│
├── scripts/        # 🔧 Build scripts directory
│   ├── README.md
│   ├── build_arm64_with_android.sh
│   └── test_android_inference.sh
│
└── src/            # 💻 Source code directory
```

## 🎯 Quick Build

### ARM64 Real Device (Recommended for Production)
```bash
# Execute from project root directory
./scripts/build_arm64_with_android.sh
```

**Build Artifacts:**
- `libgpuf_c.so` (50MB) - ARM64 dynamic library
- Complete LLM inference functionality
- Network support

### x86_64 Development Environment
```bash
# Refer to docs/ANDROID_X86_64_DEPLOYMENT_GUIDE.md
# Use compatibility layer solution (current real llama.cpp build fails)
```

## 📖 Detailed Documentation

- **Docs Index**: `docs/README.md`
- **Build Guide**: `docs/BUILD_GUIDE.md`
- **Deployment Guide**: `docs/ANDROID_X86_64_DEPLOYMENT_GUIDE.md`
- **Streaming (token callback)**: `docs/STREAMING_API_GUIDE.md`
- **Script Documentation**: `scripts/README.md`
- **P2P example client**: `examples/p2p_sdk_client.rs`

## ⚙️ Environment Requirements

- Android NDK r27d+
- Rust toolchain
- CMake
- Linux environment

## 🔧 Environment Variables (Optional)

```bash
export ANDROID_NDK_ROOT="/path/to/android-ndk"
export LLAMA_CPP_ROOT="/path/to/llama.cpp"
```

---

> 💡 **Tip**: Start from `docs/README.md` to find the latest, valid entry points.
