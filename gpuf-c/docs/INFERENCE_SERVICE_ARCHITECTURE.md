# Inference Service Architecture Design

## Overview

This document describes the architecture design of the inference service in the gpuf-c project, implementing decoupling between the LLM inference engine and the gpuf-c client.

## Architecture Comparison

### Option 1: Global Shared Engine (Original Approach)
```
┌─────────────────┐
│   gpuf-c        │
│  ┌───────────┐  │
│  │ GLOBAL    │  │
│  │ LLAMA     │  │
│  └───────────┘  │
│  ┌───────────┐  │
│  │ Network   │  │
│  │ Client    │  │
│  └───────────┘  │
└─────────────────┘
```

**Advantages**:
- Minimal memory footprint
- Unified resource management
- Fast startup speed

**Disadvantages**:
- High coupling
- Cannot independently control model lifecycle
- Poor scalability

### Option 2: Service-based Architecture (Recommended Approach)
```
┌─────────────────┐    ┌─────────────────┐
│   gpuf-c        │    │ Inference       │
│  ┌───────────┐  │    │ Service         │
│  │ Network   │  │    │  ┌───────────┐  │
│  │ Client    │◄─┼────┤  │ LLM       │  │
│  └───────────┘  │    │  │ Engine    │  │
│  ┌───────────┐  │    │  └───────────┘  │
│  │ Device    │  │    │  ┌───────────┐  │
│  │ Monitor   │  │    │  │ HTTP      │  │
│  └───────────┘  │    │  │ Server    │  │
└─────────────────┘    │  └───────────┘  │
                       └─────────────────┘
```

**Advantages**:
- Complete decoupling, can be deployed independently
- Supports multiple models and instances
- Better error isolation
- Standardized API interface
- Easy to extend and maintain

**Disadvantages**:
- Requires additional network communication overhead
- Memory footprint may increase
- Higher implementation complexity

## Inference Service Detailed Design

### Core Components

1. **InferenceService**: Main service class
2. **HTTP API**: OpenAI-compatible REST interface
3. **LLM Engine**: llama.cpp inference engine wrapper
4. **Configuration**: Service configuration management

### API Interface

#### Health Check
```http
GET /health
```

#### Text Completion (OpenAI Compatible)
```http
POST /v1/completions
Content-Type: application/json

{
  "prompt": "Rust is a programming language that",
  "max_tokens": 100,
  "temperature": 0.7
}
```

#### Chat Completion (OpenAI Compatible)
```http
POST /v1/chat/completions
Content-Type: application/json

{
  "messages": [
    {"role": "user", "content": "Hello, how are you?"}
  ],
  "max_tokens": 100
}
```

#### Service Statistics
```http
GET /stats
```

### Configuration Parameters

```rust
pub struct InferenceServiceConfig {
    pub port: u16,                    // Service port (default: 8082)
    pub model_path: String,           // Model file path
    pub n_ctx: u32,                   // Context size (default: 4096)
    pub n_gpu_layers: u32,            // GPU layers (default: 999)
    pub max_concurrent_requests: usize, // Max concurrent requests (default: 10)
}
```

## Usage

### 1. Start Inference Service

```bash
# Basic startup
cargo run --bin inference_service --release --features vulkan -- \
  --model-path "/path/to/model.gguf" \
  --port 8082

# Full parameters
cargo run --bin inference_service --release --features vulkan -- \
  --model-path "/path/to/model.gguf" \
  --port 8082 \
  --n-ctx 4096 \
  --n-gpu-layers 999 \
  --max-concurrent-requests 10 \
  --log-level info
```

### 2. Start gpuf-c Client

```bash
cargo run --bin gpuf-c --release --features vulkan -- \
  --server-addr "<your-server-host>" \
  --control-port 17000 \
  --local-port 8081 \
  --client-id "your-client-id"
```

### 3. Client Calls Inference Service

```rust
use reqwest::Client;

let client = Client::new();
let response = client
    .post("http://127.0.0.1:8082/v1/completions")
    .json(&serde_json::json!({
        "prompt": "Hello, world!",
        "max_tokens": 50
    }))
    .send()
    .await?;
```

## Deployment Modes

### Mode 1: Single Machine Deployment
```
Running on the same device:
- gpuf-c client (port 8081)
- Inference service (port 8082)
```

### Mode 2: Distributed Deployment
```
Device A (Resource Provider):
- gpuf-c client + inference service

Device B (Compute Node):
- Inference service only

Device C (Control Node):
- Calls APIs of Device A and B
```

### Mode 3: Containerized Deployment
```dockerfile
# Inference service container
FROM ubuntu:22.04
COPY target/release/inference_service /usr/local/bin/
EXPOSE 8082
CMD ["inference_service", "--model-path", "/models/model.gguf"]
```

## Performance Optimization

### 1. Memory Management
- Use mmap for lazy loading of model files
- Support model unloading and reloading
- Memory pool management to reduce allocation overhead

### 2. Concurrent Processing
- Asynchronous HTTP server (axum)
- Request queue and rate limiting
- GPU computation pipeline

### 3. Caching Strategy
- KV cache reuse
- Model weight caching
- Response caching (optional)

## Monitoring and Debugging

### Logging
```rust
// Enable detailed logging
RUST_LOG=debug cargo run --bin inference_service
```

### Performance Metrics
- Request processing time
- Memory usage
- GPU utilization
- Throughput (tokens/second)

### Health Check
```bash
curl http://localhost:8082/health
```

## Security Considerations

1. **Access Control**: Can add API authentication
2. **Input Validation**: Strict request parameter validation
3. **Resource Limits**: Prevent resource exhaustion attacks
4. **Network Security**: TLS support, IP whitelist

## Scalability

### Multi-Model Support
```rust
// Future extension to support multiple models
pub struct MultiModelService {
    models: HashMap<String, LlamaEngine>,
}
```

### Load Balancing
- Support multi-instance deployment
- Request distribution strategies
- Health check and failover

### Plugin System
- Custom preprocessing/postprocessing
- Model hot swapping
- Dynamic configuration updates

## Summary

The service-based architecture provides better flexibility, scalability, and maintainability, especially suitable for production environment deployment. Although there is some complexity overhead, the architectural benefits it brings are worthwhile.
