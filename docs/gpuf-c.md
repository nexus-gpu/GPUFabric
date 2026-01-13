# gpuf-c - Fast Reverse Proxy Client

[![Rust](https://img.shields.io/badge/rust-stable-brightgreen.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

A high-performance reverse proxy client that establishes secure connections to a central server (gpuf-s) to expose local services.

## Features

- **Multiple Protocol Support**: TCP and WebSocket (WS) worker types
- **Engine Integration**: Supports multiple inference engines (vLLM, Ollama)
- **Automatic Device Discovery**: Collects system and device information
- **Secure Communication**: TLS encryption for all connections
- **Cross-platform**: Works on Linux, Windows, and macOS

## Architecture
![gpuf-c_code_map](svg/gpuf-c_code_map.svg)

## Configuration
Create a config.toml file:

```toml
[server]
address = "127.0.0.1"
control_port = 17000
proxy_port = 17001

[client]
client_id = "6e1131b4b9cc454aa6ce3294ab860b2d"
local_addr = "127.0.0.1"
local_port = 11434
worker_type = "tcp"  # or "ws" for WebSocket
engine_type = "ollama"  # or "vllm"
cert_chain_path = "ca-cert.pem"
```

## Usage

### Basic Usage
```bash
./gpuf-c --config config.toml
```

### Command Line Arguments

| Argument | Description | Default |
|----------|-------------|---------|
| `--config` | Path to config file | None |
| `--server-addr` | Address of the gpuf-s server | 127.0.0.1 |
| `--control-port` | Port for control connection | 17000 |
| `--proxy-port` | Port for proxy connection | 17001 |
| `--local-addr` | Local service address to expose | 127.0.0.1 |
| `--local-port` | Local service port to expose | 11434 |
| `--worker-type` | Worker type (tcp/ws) | tcp |
| `--engine-type` | Inference engine (ollama/vllm) | ollama |
| `--cert-chain-path` | Path to certificate chain for TLS | ca-cert.pem |
| `--client-id` | Unique ID for this client instance | Auto-generated |

### Worker Types
- `tcp`: Standard TCP connection
- `ws`: WebSocket connection

### Engine Types
- `ollama`: Ollama inference engine (default)
- `vllm`: vLLM inference engine

## Development

### Prerequisites

- Rust toolchain (stable)
- Cargo
- System dependencies for building native extensions

### Building
```bash
# Debug build
cargo build

# Release build
cargo build --release

# Release build for Linux
rustup target add x86_64-linux-gnu-gcc
RUSTFLAGS="-C linker=x86_64-linux-gnu-gcc" cargo build --bin gpuf-c --target=x86_64-unknown-linux-gnu --release

# Release build for Windows
rustup target add x86_64-pc-windows-gnu
RUSTFLAGS="-C linker=x86_64-w64-mingw32-gcc" OPENSSL_DIR="$(brew --prefix openssl@3)" OPENSSL_STATIC=1 cargo build --target=x86_64-pc-windows-gnu --release --bin gpuf-c 
```

### Testing

```bash
cargo test
```
