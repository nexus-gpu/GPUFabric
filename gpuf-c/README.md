# 🚀 GPUFabric Client (gpuf-c)

High-performance distributed LLM inference client with multi-engine and cross-platform support.

## 📖 Documentation

For complete documentation, see [docs/README.md](docs/README.md)

### 🎯 Quick Links
- [Android Integration Guide](docs/mobile/ANDROID_DEVELOPMENT_GUIDE.md)
- [Build Guide](docs/BUILD_GUIDE.md)  
- [API Reference](docs/api/API_REFERENCE.md)
- [Examples](examples/README.md)

## 🚀 Quick Start

```bash
# Build
cargo build --release

# Android SDK
cargo ndk -t arm64-v8a build --release --features android

# Run examples
cargo run --example test_client_sdk
```

## ✨ Key Features

- 🤖 Multi-engine support (llama.cpp, Ollama, VLLM)
- 📱 Cross-platform support (Android, Windows, Linux, macOS)
- ⚡ GPU acceleration (Vulkan, CUDA, Metal)
- 🌐 Distributed inference
- 🔌 OpenAI-compatible API

---

**See [docs/README.md](docs/README.md) for complete documentation**
