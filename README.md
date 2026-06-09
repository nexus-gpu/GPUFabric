<div align="center">

# GPUFabric

**AI Inference Delivery Network**

*The First AI-Native CDN for Model Inference - Secure, Fast & Easy-to-Deploy*

[English](README.md) · [简体中文](docs/README_CN.md)

[![Rust](https://img.shields.io/badge/rust-stable-brightgreen.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-BSD--3--Clause-blue.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-Linux%20%7C%20macOS%20%7C%20Windows-lightgrey)]()

</div>

---

A distributed AI inference delivery network that brings your models closer to users. Like a CDN for AI inference, GPUFabric intelligently routes requests across your distributed model instances, providing low-latency, high-availability AI services while keeping your models private and secure on your own infrastructure.

> 📖 **Quick Start Guide**: For a concise getting started guide, see [docs/README_CN.md](docs/README_CN.md) (Simplified Chinese version)

## 🌟 Core Features

- **Distributed Inference Architecture**: Intelligent routing like CDN, reducing latency and improving availability
- **Model Privacy & Security**: Keep models and data in your infrastructure with TLS 1.3 end-to-end encryption
- **Easy Deployment**: One command `docker compose up -d` to start complete service stack
- **Observability**: System/network/heartbeat metrics with API monitoring endpoints

## 🚀 Quick Start

### Prerequisites

- **Rust** (stable) - [Install Rust](https://www.rust-lang.org/tools/install)
- **PostgreSQL** - Database server
- **Redis** (optional) - Cache server for performance
- **Kafka** (optional) - Message queue for heartbeat processing

### Installation

#### 1. Clone the Repository

```bash
git clone https://github.com/nexus-gpu/GPUFabric.git
cd GPUFabric
```

#### 2. Build the Project

```bash
# Build all components
cargo build --release

# Build specific binary
cargo build --release --bin gpuf-s
cargo build --release --bin gpuf-c
```

#### 3. Set Up Database

```bash
# Create database
createdb GPUFabric

# Initialize schema
psql -U postgres -d GPUFabric -f scripts/db.sql
```

#### 4. Generate TLS Certificates

```bash
# Generate self-signed certificates
./scripts/create_cert.sh

# This creates:
# - cert.pem (certificate chain)
# - key.pem (private key)
```

#### 5. Start Services

**Start Redis (optional):**
```bash
redis-server
# Or using Docker
docker run -d -p 6379:6379 redis:alpine
```

**Start Kafka (optional):**
```bash
docker compose -f kafka_compose.yaml up -d

# Create required topics
docker exec -it <kafka-container> kafka-topics --create \
  --topic client-heartbeats \
  --bootstrap-server localhost:9092 \
  --partitions 1 \
  --replication-factor 1
```

## 💻 Usage

### Start the Server (gpuf-s)

```bash
# Basic usage with defaults
cargo run --release --bin gpuf-s

# With full configuration
cargo run --release --bin gpuf-s -- \
  --control-port 17000 \
  --control-tls \
  --proxy-port 17001 \
  --public-port 18080 \
  --api-port 18081 \
  --database-url "postgres://postgres:password@localhost:5432/GPUFabric" \
  --redis-url "redis://127.0.0.1:6379" \
  --bootstrap-server "localhost:9092" \
  --api-key "your-secure-api-key" \
  --proxy-cert-chain-path "cert.pem" \
  --proxy-private-key-path "key.pem"
```

### Start a Client (gpuf-c)

```bash
# Basic client
cargo run --release --bin gpuf-c -- --client-id client_A

# With custom configuration
cargo run --release --bin gpuf-c -- \
  --client-id client_A \
  --server-addr 192.168.1.100 \
  --control-tls \
  --control-tls-server-name "gpuf.example.internal" \
  --cert-chain-path "ca-cert.pem" \
  --local-addr 127.0.0.1 \
  --local-port 11434
```

### Docker Build

#### Build gpuf-s Image
```bash
docker build -f docker/Dockerfile.runtime -t GPUFabric/gpuf-s:latest --build-arg BIN=gpuf-s .
```

#### Build api_server Image
```bash
docker build -f docker/Dockerfile.runtime -t GPUFabric/api_server:latest --build-arg BIN=api_server .
```

#### Build heartbeat_consumer Image
```bash
docker build -f docker/Dockerfile.runtime -t GPUFabric/heartbeat_consumer:latest --build-arg BIN=heartbeat_consumer .
```

#### Run Docker Compose (redis, postgres, kafka, gpuf-s, api_server, heartbeat_consumer)
```bash
docker compose -f docker/gpuf_s_compose.yaml up -d
```

### Start Heartbeat Consumer

```bash
cargo run --release --bin heartbeat_consumer -- \
  --database-url "postgres://postgres:password@localhost:5432/GPUFabric" \
  --bootstrap-server "localhost:9092" \
  --batch-size 100 \
  --batch-timeout 5
```

### Test the System

```bash
# Test with API key
curl -H "Authorization: Bearer your-api-key" http://localhost:18080

# Test Ollama integration
curl -H "Authorization: Bearer your-api-key" \
  -H "Content-Type: application/json" \
  http://localhost:18080/v1/chat/completions \
  -d '{
    "model": "llama2",
    "messages": [{"role": "user", "content": "Hello!"}]
  }'

# Test streaming (SSE)
curl -N -H "Authorization: Bearer your-api-key" \
  -H "Content-Type: application/json" \
  http://localhost:18080/v1/chat/completions \
  -d '{
    "model": "llama2",
    "stream": true,
    "messages": [{"role": "user", "content": "Hello!"}]
  }'

# Notes:
# - Streaming chunks use OpenAI-compatible SSE payloads.
# - Token deltas are split into `delta.reasoning_content` (analysis) and `delta.content` (final).
# - `usage` includes `analysis_tokens` and `final_tokens`.
```

## 📚 Documentation

Comprehensive documentation is available in the `docs/` directory:

### Core Components
- **[gpuf-s Documentation](./docs/gpuf-s.md)** - Server component documentation
- **[gpuf-c Documentation](./docs/gpuf-c.md)** - Client component documentation
- **[API Server Documentation](./docs/api_server.md)** - RESTful API reference
- **[Frontend/GUI Integration](./gui/doc.md)** - Local dashboard and browser frontend API defaults
- **[Heartbeat Consumer Documentation](./docs/heartbeat_consumer.md)** - Kafka consumer documentation
- **[XDP Documentation](./docs/xdp.md)** - Kernel-level packet filtering

### Mobile SDK
- **[Mobile SDK Build Guide](./docs/mobile-sdk/BUILD_GUIDE.md)** - Build and packaging guide
- **[Mobile SDK Integration Guide](./docs/mobile-sdk/INTEGRATION_GUIDE_EN.md)** - Android/iOS integration steps
- **[Mobile SDK Checklist](./gpuf-c/docs/mobile/MOBILE_SDK_CHECKLIST.md)** - Development progress tracker

Current Android SDK evidence: the packaged SDK builds with NDK 25.1.8937393 and passes real-device ARM64 inference through `gpuf-c/scripts/test_android_inference.sh`; iOS release evidence still requires macOS/Xcode platform jobs.

## 🛠️ Configuration

### Server Configuration

The gpuf-s server supports comprehensive configuration via command-line arguments:

| Argument | Type | Default | Description |
|----------|------|---------|-------------|
| `--control-port` | u16 | 17000 | Port for client control connections |
| `--control-tls` | bool | false | Serve client control connections over TLS; enable for remote production deployments |
| `--proxy-port` | u16 | 17001 | Port for client proxy connections |
| `--public-port` | u16 | 18080 | Port for public user connections |
| `--api-port` | u16 | 18081 | Port for HTTP API server |
| `--database-url` | string | `postgres://...` | PostgreSQL connection string |
| `--redis-url` | string | `redis://127.0.0.1:6379` | Redis connection string |
| `--bootstrap-server` | string | `localhost:9092` | Kafka broker address |
| `--api-key` | string | `abc123` | Fallback API key |
| `--proxy-cert-chain-path` | string | `cert.pem` | TLS certificate chain |
| `--proxy-private-key-path` | string | `key.pem` | TLS private key |

### Environment Variables

You can also configure using environment variables:

```bash
export DATABASE_URL="postgres://postgres:password@localhost:5432/GPUFabric"
export REDIS_URL="redis://localhost:6379"
export API_KEY="your-api-key"
export RUST_LOG="gpuf-s=info"
```

### Management API Server

The standalone management API used by dashboards/frontends binds to loopback by default:

```bash
cargo run --release -p gpuf-s --bin api_server -- \
  --bind-addr 127.0.0.1 \
  --port 18081 \
  --database-url "$DATABASE_URL"
```

Use `--bind-addr 0.0.0.0` only behind a protected reverse proxy or private network boundary.

### Control Connection TLS

For remote deployments, enable TLS on both sides of the gpuf-s/gpuf-c control connection:

```bash
# Server
gpuf-s --control-tls --proxy-cert-chain-path cert.pem --proxy-private-key-path key.pem

# Client
gpuf-c --server-addr gpuf.example.internal --control-tls \
  --control-tls-server-name gpuf.example.internal --cert-chain-path ca-cert.pem
```

Plaintext control TCP remains the default for compatibility in v1.1.0. Non-loopback plaintext connections emit a security/deprecation warning.

## 🔧 Development

### Development Workflow

```bash
# Run tests
cargo test

# Run with logging
RUST_LOG=debug cargo run --release --bin gpuf-s

# Format code
cargo fmt

# Run linter
cargo clippy
```

### Project Structure

```
GPUFabric/
├── gpuf-s/              # Server component
│   └── src/
│       ├── main.rs            # Server entry point
│       ├── handle/            # Connection handlers
│       ├── api_server/        # REST API server
│       ├── consumer/           # Kafka consumer
│       ├── db/                 # Database operations
│       └── util/               # Utilities
├── gpuf-c/              # Client component
│   └── src/
│       ├── main.rs            # Client entry point
│       ├── handle/            # Connection handlers
│       ├── llm_engine/        # LLM engine integration
│       └── util/               # Utilities
├── common/            # Shared protocol library
│   └── src/lib.rs     # Protocol definitions
└── docs/              # Documentation
```


## 🎯 Detailed Capabilities

### 🌐 AI Inference Delivery Network
- **Distributed Inference Architecture**: Deploy model instances anywhere, route requests intelligently like a CDN
- **Geographic Distribution**: Bring AI inference closer to your users for minimal latency
- **Intelligent Request Routing**: Automatic load balancing across distributed model instances
- **Edge Inference Support**: Run models at the edge, reduce data transfer and improve response times
- **Dynamic Scaling**: Add or remove inference nodes on-demand without service interruption
- **Health Monitoring**: Automatic failover and traffic rerouting when nodes become unavailable

### 🔐 Model Privacy & Security
- **Local Model Hosting**: Models stay on your local servers, complete control over your model assets
- **Data Privacy Protection**: Inference data never passes through third parties, end-to-end encryption
- **TLS 1.3 Encryption**: Enterprise-grade encryption standards for secure communication
- **Multi-Layer Authentication**: Database authentication + Redis caching + API Key validation
- **Kernel-Level Protection**: XDP (eBPF) kernel-level packet filtering, DDoS attack mitigation

### ⚡ Fast Access (NAT Traversal)
- **NAT Traversal Technology**: No public IP required, internal services directly accessible
- **P2P Direct Connection**: Under development, peer-to-peer connections reduce latency
- **Sub-Millisecond Routing**: Built with Rust + Tokio for ultra-low latency request routing
- **Redis Cache Acceleration**: 90% database query caching, significantly improved response speed
- **Connection Pooling**: Persistent connections reduce handshake overhead

### 🚀 Easy Deployment
- **One-Click Docker Deployment**: `docker compose up -d` launches complete service stack
- **Pre-Built Images**: Provides gpuf-s, api_server, heartbeat_consumer images
- **Automated Scripts**: One-click TLS certificate generation and database initialization
- **Zero-Config Startup**: Sensible defaults, ready to use out of the box
- **Flexible Configuration**: Supports command-line arguments, environment variables, and config files

### 🌍 Cross-Platform Support
- **Full Platform Compatibility**: Native support for Linux, macOS, and Windows
- **Unified Binary**: Single executable file, no complex dependencies
- **Containerized Deployment**: Docker images support all mainstream platforms
- **ARM64 Support**: Compatible with Apple Silicon (M1/M2/M3) and ARM servers for performance

## 🏗️ Architecture

![GPUFabric System Architecture](./docs/svg/GPUFabric.svg)

### System Components

GPUFabric consists of three main components:

- **gpuf-s** - Server application that handles load balancing, client management, and request routing
- **gpuf-c** - Client application that connects to the server and forwards to local services
- **common** - Shared protocol library with binary command definitions

### Four-Port Design

```
┌─────────────────────────────────────────────────────────┐
│                      gpuf-s Server                         │
│                                                           │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐     │
│  │ Control     │  │ Proxy       │  │ Public      │     │
│  │ Port 17000  │  │ Port 17001  │  │ Port 18080  │     │
│  │ (Registration)│  │ (Data       │  │ (External   │     │
│  │             │  │ Forwarding) │  │ Users)      │     │
│  └─────────────┘  └─────────────┘  └─────────────┘     │
│                                                           │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐     │
│  │ API Server  │  │ PostgreSQL  │  │ Redis Cache │     │
│  │ Port 18081  │  │ Database    │  │             │     │
│  │ (REST API)  │  │             │  │             │     │
│  └─────────────┘  └─────────────┘  └─────────────┘     │
│                                                           │
│  ┌─────────────┐                                         │
│  │ Kafka       │                                         │
│  │ (Message    │                                         │
│  │ Queue)      │                                         │
│  └─────────────┘                                         │
└─────────────────────────────────────────────────────────┘
```

| Port | Purpose | Protocol | Description |
|------|---------|----------|-------------|
| **17000** | Control | TCP | Persistent connections for client registration and command dispatch |
| **17001** | Proxy | TCP | Temporary connections for bidirectional data forwarding |
| **18080** | Public | TCP/HTTP | External user entry point with API key validation |
| **18081** | API | HTTP | RESTful API server for monitoring and management |

### Request Flow

```
1. User connects to Public Port (18080)
   ↓
2. gpuf-s validates API key (database or static fallback)
   ↓
3. gpuf-s randomly selects active client from pool
   ↓
4. gpuf-s generates unique proxy_conn_id
   ↓
5. gpuf-s sends RequestNewProxyConn to chosen client
   ↓
6. gpuf-c connects to Proxy Port (17001) with NewProxyConn
   ↓
7. gpuf-c connects to local service
   ↓
8. gpuf-s matches connections using proxy_conn_id
   ↓
9. Bidirectional data forwarding begins
```

## 🔬 Tech Stack

### Core Technologies
- **Language**: Rust (stable) with Tokio async runtime
- **Network**: TLS 1.3, TCP/HTTP protocols
- **Serialization**: Bincode for efficient binary protocol

### Infrastructure Components
- **Database**: PostgreSQL - Persistent storage, authentication, and statistics
- **Cache**: Redis - 5-minute TTL caching, ~90% database load reduction
- **Message Queue**: Apache Kafka - Asynchronous heartbeat processing and request tracking
- **Containerization**: Docker & Docker Compose for deployment

### High-Performance Features

#### XDP (eXpress Data Path) - Kernel-Level Packet Filtering
- **eBPF-based** packet processing at network driver level for ultra-low latency
- **API Key Validation** at kernel level before reaching user space
- **Use Case**: High-performance request validation and DDoS protection

For detailed XDP setup and usage, see [XDP Documentation](./docs/xdp.md)

### Monitoring & Observability
- **System Metrics**: CPU, memory, disk, network monitoring
- **Power Metrics**: GPU/CPU/ANE power consumption tracking (macOS M-series)
- **Network Stats**: Real-time bandwidth monitoring with session tracking
- **RESTful API**: Comprehensive metrics endpoints for external monitoring



## 🗺️ Roadmap

### ✅ Current Features (Production Ready)
- ✅ High-performance reverse proxy with load balancing
- ✅ Database-backed authentication with Redis caching
- ✅ Kafka-based asynchronous heartbeat processing
- ✅ TLS 1.3 secure connections
- ✅ AI/LLM model routing (Ollama, vLLM)
- ✅ Real-time system monitoring and metrics
- ✅ XDP kernel-level packet filtering (Linux)

### 🚧 In Development

#### P2P Hybrid Architecture
Migrating from pure client-server to hybrid P2P model for improved performance and reduced server load.

**Technical Implementation:**
- **NAT Traversal**: STUN/TURN/ICE protocols for peer discovery
- **libp2p Integration**: Rust-native P2P networking library
  - AutoNAT for automatic NAT detection
  - Relay protocol for fallback connections
  - Hole punching for direct peer connections
  - DHT (Distributed Hash Table) for peer discovery
- **Signaling Server**: gpuf-s acts as signaling server for peer connection establishment
- **Smart Routing**: Automatic selection between P2P direct, relay, or TURN based on network conditions

**Protocol Design** (CommandV2):
```rust
// Already implemented in common/src/lib.rs
CommandV2::P2PConnectionRequest      // Initiate P2P handshake
CommandV2::P2PConnectionInfo         // Exchange peer addresses
CommandV2::P2PConnectionEstablished  // Confirm connection type
CommandV2::P2PConnectionFailed       // Fallback to relay mode
```

**Benefits:**
- 🚀 Lower latency through direct peer connections
- 💰 Reduced server bandwidth costs
- 📈 Better scalability for large deployments
- 🔄 Automatic fallback to relay mode

**Planned Modules:**
```
gpuf-c/src/p2p/
├── mod.rs              # P2P module entry
├── peer.rs             # Peer connection management
├── nat_traversal.rs    # NAT Traversal
├── connection.rs       # P2P Connection
└── discovery.rs        # Node Discovery

gpuf-s/src/signaling/
├── mod.rs              # Signaling Server
└── peer_registry.rs    # Peer Address Registry
```

#### XDP Enhanced Features
- **Dynamic Rule Updates**: Hot-reload XDP rules without service restart
- **Rate Limiting**: Per-IP rate limiting at kernel level
- **GeoIP Filtering**: Geographic-based access control
- **DDoS Protection**: SYN flood and connection flood mitigation

### 📋 Future Enhancements
- [ ] WebSocket support for browser clients
- [ ] Multi-region deployment with geo-routing
- [ ] Enhanced metrics with Prometheus/Grafana integration
- [ ] HTTP/3 (QUIC) protocol support
- [ ] Advanced load balancing algorithms (least connections, weighted round-robin)
- [ ] Client-side load prediction and smart routing
- [ ] Distributed tracing with OpenTelemetry

### 🔬 Research & Exploration
- Blockchain-based decentralized authentication
- Zero-knowledge proof for privacy-preserving authentication
- FPGA acceleration for packet processing
- eBPF-based traffic shaping and QoS

## 🤝 Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

1. Fork the repository
2. Create your feature branch (`git checkout -b feature/AmazingFeature`)
3. Commit your changes (`git commit -m 'Add some AmazingFeature'`)
4. Push to the branch (`git push origin feature/AmazingFeature`)
5. Open a Pull Request

### Development Guidelines

- Follow Rust best practices and style guide
- Add tests for new features
- Update documentation as needed
- Ensure all tests pass before submitting

## 📊 Performance

- **Throughput**: High-performance async I/O with Tokio
- **Latency**: Sub-millisecond request routing
- **Scalability**: Supports unlimited client connections
- **Caching**: Redis caching reduces database load by ~90%
- **Batch Processing**: Efficient heartbeat processing with configurable batching

## 🔒 Security

- Loopback defaults for local management APIs and explicit public-listen deployment choices
- Database-backed authentication with token validation
- Redis caching for performance without compromising security
- Input validation, SQL injection prevention, and sanitized service logs
- SHA256 verification for model downloads and SDK/release artifacts; MD5 is not used as a trust mechanism
- Security release gates for cargo audit/deny, secret scan, targeted tests, and source grep checks

## 🌟 Use Cases

- **AI Model Serving**: Route requests to distributed AI inference engines
- **Service Exposure**: Expose local services to the internet securely
- **Load Balancing**: Distribute traffic across multiple backend instances
- **Monitoring**: Real-time system and application monitoring
- **Development**: Access local development servers from anywhere

## 📝 License

This project is licensed under the BSD 3-Clause License - see the [LICENSE](LICENSE) file for details.


## 📮 Support

- 📖 [Documentation](./docs/)
- 🐛 [Issue Tracker](https://github.com/nexus-gpu/GPUFabric/issues)
- 💬 [Discussions](https://github.com/nexus-gpu/GPUFabric/discussions)

---

Made with ❤️ using Rust
