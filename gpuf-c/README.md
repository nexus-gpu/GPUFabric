# GPUFabric Client (gpuf-c)

GPUFabric client supporting distributed inference with multiple LLM engines.

## 🚀 Quick Start

### Build
```powershell
cargo build --release
```

### Standalone LLAMA Mode
```powershell
.\target\release\gpuf-c.exe --standalone-llama
```

### Worker Mode
```powershell
.\target\release\gpuf-c.exe `
    --engine-type llama `
    --llama-model-path ./model.gguf `
    --server-addr 192.168.1.100
```

## 📁 Project Structure

```
gpuf-c/
├── src/           # Source code
├── docs/          # Documentation
├── scripts/       # Build scripts
├── tests/         # Test scripts
├── examples/      # Example code
└── jniLibs/       # Android libraries
```

## 📖 Documentation

- [Windows Build Guide](docs/WINDOWS_BUILD.md) - Build instructions for Windows

## 🧪 Testing

```powershell
# Run LLAMA tests
.\tests\test_llama_worker.ps1

# Run API tests
.\tests\test_api.ps1

# Run Vulkan tests
.\tests\test_vulkan.ps1
```

## 🔧 Supported Engines

- **llama.cpp** - High-performance local inference
- **Ollama** - Containerized LLM service
- **VLLM** - High-performance inference service

## 🎯 Features

- ✅ Standalone and cluster modes
- ✅ OpenAI compatible API
- ✅ GPU acceleration (Vulkan/CUDA)
- ✅ Automatic model download
- ✅ Cross-platform support

## 🤝 Contributing

Issues and Pull Requests are welcome!

## 📄 License

[MIT License](LICENSE)
