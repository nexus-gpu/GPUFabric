# 🚀 GPUFabric Android Quick Start Guide

## 📋 Project Structure

```
gpuf-c/
├── docs/           # 📚 Documentation directory
│   ├── README_ANDROID.md
│   ├── ANDROID_BUILD_LESSONS_LEARNED.md
│   ├── ANDROID_JNI_NETWORK_BUILD_GUIDE.md
│   └── ANDROID_X86_64_DEPLOYMENT_GUIDE.md
│
├── scripts/        # 🔧 Build scripts directory
│   ├── README_ANDROID.md
│   ├── build_arm64_with_android.sh
│   └── build_x86_64_with_android.sh
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
- `libgpuf_c.so` (40MB) - ARM64 dynamic library
- Complete LLM inference functionality
- Network support

### x86_64 Development Environment
```bash
# Refer to docs/ANDROID_X86_64_DEPLOYMENT_GUIDE.md
# Use compatibility layer solution (current real llama.cpp build fails)
```

## 📖 Detailed Documentation

- **Build Experience**: `docs/ANDROID_BUILD_LESSONS_LEARNED.md`
- **Deployment Guide**: `docs/ANDROID_X86_64_DEPLOYMENT_GUIDE.md`
- **Advanced Build**: `docs/ANDROID_JNI_NETWORK_BUILD_GUIDE.md`
- **Script Documentation**: `scripts/README_ANDROID.md`

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

> 💡 **Tip**: It's recommended to read `docs/ANDROID_BUILD_LESSONS_LEARNED.md` first to understand architecture limitations and best practices.
