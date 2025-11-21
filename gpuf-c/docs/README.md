# 🚀 GPUFabric Client (gpuf-c)

High-performance distributed LLM inference client with multi-engine and cross-platform support.

## ✨ Core Features

- 🤖 **Multi-engine support**: llama.cpp, Ollama, VLLM
- 🌐 **Distributed inference**: Cluster mode and standalone mode
- 📱 **Cross-platform**: Android, Windows, Linux, macOS
- ⚡ **GPU acceleration**: Vulkan, CUDA, Metal support
- 🔌 **OpenAI-compatible**: Standard API interface
- 📊 **Real-time monitoring**: Performance metrics and status management

## 🚀 Quick Start

### Basic Build
```bash
# Clone project
git clone https://github.com/your-org/GPUFabric.git
cd GPUFabric/gpuf-c

# Build project
cargo build --release

# Run examples
./target/release/gpuf-c --help
```

### Android Integration
```bash
# Build Android SDK
cargo ndk -t arm64-v8a build --release --features android

# Integrate into Android project
cp target/aarch64-linux-android/release/libgpuf_c.so \
   your-android-app/app/src/main/jniLibs/arm64-v8a/
```

### Usage Examples
```java
// Android Java example
GPUFabricClientSDK sdk = new GPUFabricClientSDK();
sdk.init();

// Initialize LLM model
if (sdk.initializeModel("/path/to/model.gguf")) {
    String response = sdk.generateResponse("Hello, GPUFabric!");
    System.out.println(response);
}
```

```rust
// Rust example
use gpuf_c::{init, gpuf_llm_init, gpuf_llm_generate};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    init()?;
    
    // Initialize LLM (requires unsafe block)
    let model_path = std::ffi::CString::new("model.gguf")?;
    unsafe {
        gpuf_llm_init(model_path.as_ptr(), 2048, 0);
        let result = gpuf_llm_generate(
            std::ffi::CString::new("Hello!")?.as_ptr(), 
            100
        );
        // Handle result...
    }
    
    Ok(())
}
```

## 📱 Platform Support

| Platform | Architecture | GPU Support | Status |
|----------|--------------|-------------|--------|
| Android | ARM64 | Vulkan | ✅ Fully Supported |
| Windows | x64 | CUDA/Vulkan | ✅ Fully Supported |
| Linux | x64/ARM64 | CUDA/Vulkan | ✅ Fully Supported |
| macOS | x64/ARM64 | Metal | ✅ Fully Supported |

## 🛠️ Build Options

### Features
```bash
# CPU version (minimal)
cargo build --release --features cpu

# Vulkan version (cross-platform GPU)
cargo build --release --features "cpu,vulkan"

# CUDA version (NVIDIA GPU)
cargo build --release --features "cpu,cuda"

# Metal version (Apple GPU)
cargo build --release --features "cpu,metal"

# Full version (all GPU backends)
cargo build --release --features "cpu,vulkan,cuda,metal"
```

### Android Optimized Versions
```bash
# Full version (83.5 MB)
cargo ndk -t arm64-v8a build --release --features android

# Balanced version (25-35 MB)
cargo ndk -t arm64-v8a build --release --features android-balanced

# Minimal version (15-25 MB)
cargo ndk -t arm64-v8a build --release --features android-minimal
```

## 📚 Documentation

### 🎯 Integration Guides
- [Android Integration Guide](mobile/ANDROID_DEVELOPMENT_GUIDE.md) - Complete Android SDK integration
- [Build Guide](BUILD_GUIDE.md) - Detailed build configuration and optimization
- [API Reference](api/API_REFERENCE.md) - Complete API interface documentation

### 🖥️ Platform Guides
- [Windows Build](PLATFORM_GUIDES/WINDOWS_BUILD.md) - Windows-specific build instructions
- [Examples](../examples/README.md) - Multi-language usage examples

## 🏗️ Project Structure

```
gpuf-c/
├── src/                    # Rust source code
│   ├── lib.rs             # Library entry point
│   ├── client_sdk.rs      # Client SDK
│   ├── llama_wrapper.rs   # LLM wrapper
│   └── util/              # Utility modules
├── docs/                   # 📚 Documentation directory
│   ├── README.md          # Main documentation (this file)
│   ├── mobile/            # Mobile development docs
│   ├── platform/          # Platform-specific docs
│   ├── api/               # API documentation
│   ├── BUILD_GUIDE.md
│   └── PLATFORM_GUIDES/
├── examples/               # Example code
│   ├── android/           # Android examples
│   └── rust/              # Rust examples
├── scripts/               # Build scripts
└── tests/                 # Test code
```

## 🎯 Use Cases

### 🤖 Mobile AI Applications
- Chatbots and conversational assistants
- Text generation and content creation
- Offline AI inference services

### ☁️ Distributed Computing
- Edge device clusters
- Hybrid cloud inference
- Load balancing and scheduling

### 🔬 Enterprise Deployment
- Private LLM services
- High-concurrency inference
- Real-time performance monitoring

## 📊 Performance Metrics

### Inference Performance (ARM64 Android)
| Model Size | GPU Layers | Inference Speed | Memory Usage |
|------------|------------|-----------------|--------------|
| 3B | 0 (CPU) | 5-8 tokens/s | 2GB |
| 3B | 10 (Vulkan) | 15-25 tokens/s | 2.5GB |
| 7B | 0 (CPU) | 2-4 tokens/s | 5GB |
| 7B | 20 (Vulkan) | 8-15 tokens/s | 6GB |

### Desktop Performance (RTX 3080)
| Model Size | GPU Layers | Inference Speed | Memory Usage |
|------------|------------|-----------------|--------------|
| 13B | 0 (CPU) | 1-2 tokens/s | 8GB |
| 13B | 40 (CUDA) | 40-60 tokens/s | 10GB |
| 34B | 0 (CPU) | 0.5 tokens/s | 20GB |
| 34B | 40 (CUDA) | 15-25 tokens/s | 24GB |

## 🧪 Testing

### Running Tests
```bash
# Rust unit tests
cargo test

# Integration tests
cargo test --test integration_tests

# Android tests
cargo ndk -t arm64-v8a test --release --features android
```

### Performance Tests
```bash
# LLM inference tests
./tests/test_llama_performance.sh

# Network connectivity tests
./tests/test_client_connectivity.sh

# GPU acceleration tests
./tests/test_gpu_acceleration.sh
```

## 🔧 Configuration Options

### Environment Variables
```bash
# Rust compilation optimization
export RUSTFLAGS="-C target-cpu=native"

# Android NDK path
export ANDROID_NDK_HOME="/path/to/android/ndk"

# CUDA path (optional)
export CUDA_ROOT="/usr/local/cuda"
```

### Runtime Configuration
```json
{
  "server_addr": "127.0.0.1",
  "control_port": 17000,
  "proxy_port": 17001,
  "client_id": "device-12345",
  "device_name": "GPUFabric Device",
  "llm": {
    "model_path": "./model.gguf",
    "context_size": 2048,
    "gpu_layers": 10,
    "batch_size": 512
  },
  "monitoring": {
    "enable_metrics": true,
    "heartbeat_interval": 30
  }
}
```

## 🤝 Contributing Guidelines

### Development Environment Setup
```bash
# Install Rust toolchain
rustup update stable
rustup component add clippy rustfmt

# Install development dependencies
cargo install cargo-watch cargo-tarpaulin

# Run development server
cargo watch -x run
```

### Code Standards
```bash
# Format code
cargo fmt

# Static analysis
cargo clippy -- -D warnings

# Run tests and generate coverage
cargo tarpaulin --out Html
```

## 📄 License

This project is licensed under the MIT License - see the [LICENSE](../../LICENSE) file for details.

## 🔗 Related Links

- [GPUFabric Main Project](../../README.md)
- [Issue Tracker](../../issues)
- [Changelog](../../CHANGELOG.md)
- [Community Discussions](../../discussions)

---

**Version**: v1.0.0  
**Last Updated**: 2025-11-21  
**Maintainers**: GPUFabric Development Team
