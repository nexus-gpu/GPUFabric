# GPUFabric Documentation

Complete documentation for the GPUFabric Android SDK project.

## 📚 Documentation Index

### 🚀 Getting Started
- **[Quick Start Guide](QUICK_START.md)** - Setup and build in 5 minutes

### 🔧 Android Development
- **[Android Build Lessons](ANDROID_BUILD_LESSONS_LEARNED.md)** - Architecture limitations and best practices
- **[JNI Network Build Guide](ANDROID_JNI_NETWORK_BUILD_GUIDE.md)** - Network integration and JNI setup
- **[Android x86_64 Deployment Guide](ANDROID_X86_64_DEPLOYMENT_GUIDE.md)** - Multi-platform deployment

### 🏗️ Architecture & Design
- **[Build Guide](BUILD_GUIDE.md)** - Complete build system documentation
- **[Initialization Guide](INITIALIZATION_GUIDE.md)** - System initialization procedures
- **[Inference Service Architecture](INFERENCE_SERVICE_ARCHITECTURE.md)** - Service design and patterns

### 📊 Model Management
- **[Model Management Guide](MODEL_MANAGEMENT_GUIDE.md)** - Model loading and management
- **[Model Status Examples](MODEL_STATUS_EXAMPLES.md)** - Practical model usage examples

### 🔌 API Reference
- **[API Documentation](api/)** - Low-level API reference
- **[Mobile Integration](mobile/)** - Mobile-specific APIs

### 📈 Platform Guides
- **[Platform Guides](PLATFORM_GUIDES/)** - Cross-platform compatibility
- **[Platform Documentation](platform/)** - Platform-specific details

### 🔄 Advanced Features
- **[Compute Sharing Diagrams](COMPUTE_SHARING_DIAGRAMS.md)** - Resource sharing architecture
- **[Offline Mode Guide](OFFLINE_MODE_GUIDE.md)** - Offline inference capabilities

### 📊 Architecture Diagrams
- **[SDK Compute Sharing Flow](sdk-compute-sharing-flow.mmd)** - Resource flow visualization
- **[SDK Compute Sharing Sequence](sdk-compute-sharing-sequence.mmd)** - Sequence diagrams

## 🎯 Android Development Focus

### 📱 Android Platform Support
- ✅ **ARM64**: Full functionality with real llama.cpp API
- ⚠️ **x86_64**: Compatibility layer API for development and testing

### �� Core Android Documents
| Document | Description | Use Case |
|----------|-------------|----------|
| `ANDROID_BUILD_LESSONS_LEARNED.md` | Build experience and lessons | Technical decisions and problem solving |
| `ANDROID_JNI_NETWORK_BUILD_GUIDE.md` | JNI network build guide | Complex dependency builds |
| `ANDROID_X86_64_DEPLOYMENT_GUIDE.md` | x86_64 deployment guide | Emulator development and testing |

### 🏗️ Build Scripts Location
All build scripts are located in the project root:
- `generate_sdk.sh` - Main SDK generation script
- `test_jni_android.sh` - Android JNI testing

### 📖 Usage Guidelines
1. **Production Deployment**: Refer to `ANDROID_X86_64_DEPLOYMENT_GUIDE.md`
2. **Troubleshooting**: Refer to `ANDROID_BUILD_LESSONS_LEARNED.md`
3. **Advanced Builds**: Refer to `ANDROID_JNI_NETWORK_BUILD_GUIDE.md`

## 🎯 Quick Navigation

### For Android Developers
1. Start with [Quick Start Guide](QUICK_START.md)
2. Read [Android Build Lessons](ANDROID_BUILD_LESSONS_LEARNED.md)
3. Follow [JNI Network Guide](ANDROID_JNI_NETWORK_BUILD_GUIDE.md)

### For System Architects
1. Study [Inference Service Architecture](INFERENCE_SERVICE_ARCHITECTURE.md)
2. Explore [Compute Sharing Diagrams](COMPUTE_SHARING_DIAGRAMS.md)
3. Review [Initialization Guide](INITIALIZATION_GUIDE.md)

### For Model Engineers
1. Read [Model Management Guide](MODEL_MANAGEMENT_GUIDE.md)
2. Check [Model Status Examples](MODEL_STATUS_EXAMPLES.md)
3. Review [Offline Mode Guide](OFFLINE_MODE_GUIDE.md)

---

> 💡 **Tip**: Most guides include practical examples and code snippets. Start with the Quick Start Guide for hands-on experience.
