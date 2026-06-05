<div align="center">

# GPUFabric

**AI 推理分发网络**

*首个 AI 原生的模型推理 CDN - 安全、快速、易部署*

[English](../README.md) · [简体中文](README_CN.md)

[![Rust](https://img.shields.io/badge/rust-stable-brightgreen.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-BSD--3--Clause-blue.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-Linux%20%7C%20macOS%20%7C%20Windows-lightgrey)]()

</div>

---

一个分布式 AI 推理分发网络，将您的模型更靠近用户。就像 AI 推理的 CDN，GPUFabric 智能地在分布式模型实例之间路由请求，提供低延迟、高可用的 AI 服务，同时保持模型在您自己的基础设施上私有和安全。


## 🌟 核心特性

- **分布式推理架构**：像 CDN 一样智能路由，降低延迟、提升可用性
- **模型隐私与安全**：模型与数据留在你的基础设施内，TLS 1.3 全链路加密
- **易用部署**：一条命令 `docker compose up -d` 即可启动完整服务
- **可观测性**：系统/网络/心跳指标，API 暴露监控端点

## 🚀 快速开始

### 前置要求

- **Rust**（稳定版）- [安装 Rust](https://www.rust-lang.org/tools/install)
- **PostgreSQL** - 数据库服务器
- **Redis**（可选）- 性能缓存服务
- **Kafka**（可选）- 用于心跳处理的消息队列

### 安装

#### 1. 克隆仓库

```bash
git clone https://github.com/nexus-gpu/GPUFabric.git
cd GPUFabric
```

#### 2. 构建项目

```bash
# 构建所有组件
cargo build --release

# 构建特定二进制
cargo build --release --bin gpuf-s
cargo build --release --bin gpuf-c
```

#### 3. 设置数据库

```bash
# 创建数据库
createdb GPUFabric

# 初始化数据库架构
psql -U postgres -d GPUFabric -f scripts/db.sql
```

#### 4. 生成 TLS 证书

```bash
# 生成自签名证书
./scripts/create_cert.sh

# 这会创建：
# - cert.pem（证书链）
# - key.pem（私钥）
```

#### 5. Start Services

**启动 Redis（可选）：**
```bash
redis-server
# 或使用 Docker 启动
docker run -d -p 6379:6379 redis:alpine
```

**启动 Kafka（可选）：**
```bash
docker compose -f kafka_compose.yaml up -d

# 创建必需的主题
docker exec -it <kafka-container> kafka-topics --create \
  --topic client-heartbeats \
  --bootstrap-server localhost:9092 \
  --partitions 1 \
  --replication-factor 1
```

## 💻 快速开始（极简）

```bash
# 1) 克隆
git clone https://github.com/nexus-gpu/GPUFabric.git
cd GPUFabric

# 2) 生成 TLS 证书（cert.pem/key.pem 会在项目根目录生成）
./scripts/create_cert.sh

# 3) 一键启动（Postgres/Redis/Kafka/gpuf-s/api-server/heartbeat-consumer）
docker compose -f docker/gpuf_s_compose.yaml up -d

# 4) 验证服务
curl -H "Authorization: Bearer your-api-key" http://localhost:18080
```

更多使用示例（gpuf-s 参数、gpuf-c 启动、Docker 构建等），请见下方文档索引。

### 启动服务端（gpuf-s）

详见 [gpuf-s 文档](./gpuf-s.md)

```bash
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

### 启动客户端（gpuf-c）

详见 [gpuf-c 文档](./gpuf-c.md)

```bash
cargo run --release --bin gpuf-c -- \
  --client-id client_A \
  --server-addr 192.168.1.100 \
  --control-tls \
  --control-tls-server-name "gpuf.example.internal" \
  --cert-chain-path "ca-cert.pem" \
  --local-addr 127.0.0.1 \
  --local-port 11434
```

## 📚 文档索引
- **[gpuf-s 文档](./gpuf-s.md)** - 服务器组件文档
- **[gpuf-c 文档](./gpuf-c.md)** - 客户端组件文档
- **[API Server 文档](./api_server.md)** - RESTful API 参考
- **[前端/GUI 接入文档](../gui/doc.md)** - 本地 dashboard 与浏览器前端 API 默认值
- **[Heartbeat Consumer 文档](./heartbeat_consumer.md)** - Kafka 消费者文档
- **[XDP 文档](./xdp.md)** - 内核级数据包过滤


### Docker 构建

#### 构建 gpuf-s 镜像
```bash
docker build -f docker/Dockerfile.runtime -t GPUFabric/gpuf-s:latest --build-arg BIN=gpuf-s .
```

#### 构建 api_server 镜像
```bash
docker build -f docker/Dockerfile.runtime -t GPUFabric/api_server:latest --build-arg BIN=api_server .
```

#### 构建 heartbeat_consumer 镜像
```bash
docker build -f docker/Dockerfile.runtime -t GPUFabric/heartbeat_consumer:latest --build-arg BIN=heartbeat_consumer .
```

<!-- 一键 compose 示例已在“快速开始（极简）”提供 -->
```bash
docker compose -f docker/gpuf_s_compose.yaml up -d
```

<!-- Heartbeat Consumer 启动示例移至文档索引对应页面 -->

```bash
cargo run --release --bin heartbeat_consumer -- \
  --database-url "postgres://postgres:password@localhost:5432/GPUFabric" \
  --bootstrap-server "localhost:9092" \
  --batch-size 100 \
  --batch-timeout 5
```

<!-- 测试示例移至各服务文档页面 -->

```bash
# 使用 API Key 测试
curl -H "Authorization: Bearer your-api-key" http://localhost:18080

# 测试 Ollama 集成
curl -H "Authorization: Bearer your-api-key" \
  -H "Content-Type: application/json" \
  http://localhost:18080/v1/chat/completions \
  -d '{
    "model": "llama2",
    "messages": [{"role": "user", "content": "Hello!"}]
  }'
```


## 🛠️ 配置

### 服务器配置
详见 [gpuf-s 文档](./gpuf-s.md)

gpuf-s 支持通过命令行参数进行完整配置：

| 参数 | 类型 | 默认值 | 描述 |
|------|------|--------|------|
| `--control-port` | u16 | 17000 | 客户端控制连接端口 |
| `--control-tls` | bool | false | 控制连接启用 TLS；远程生产部署应开启 |
| `--proxy-port` | u16 | 17001 | 客户端代理连接端口 |
| `--public-port` | u16 | 18080 | 面向外部用户的访问端口 |
| `--api-port` | u16 | 18081 | HTTP API 端口 |
| `--database-url` | string | `postgres://...` | PostgreSQL 连接串 |
| `--redis-url` | string | `redis://127.0.0.1:6379` | Redis 连接串 |
| `--bootstrap-server` | string | `localhost:9092` | Kafka Broker 地址 |
| `--api-key` | string | `abc123` | 兜底 API Key |
| `--proxy-cert-chain-path` | string | `cert.pem` | TLS 证书链路径 |
| `--proxy-private-key-path` | string | `key.pem` | TLS 私钥路径 |

### 控制连接 TLS

远程生产部署建议同时在服务端和客户端启用控制连接 TLS：

```bash
# 服务端
gpuf-s --control-tls --proxy-cert-chain-path cert.pem --proxy-private-key-path key.pem

# 客户端
gpuf-c --server-addr gpuf.example.internal --control-tls \
  --control-tls-server-name gpuf.example.internal --cert-chain-path ca-cert.pem
```

v1.1.0 为兼容保留明文控制 TCP 默认值；非 loopback 明文连接会输出安全/弃用告警。

### Environment Variables

你也可以使用环境变量进行配置：

```bash
export DATABASE_URL="postgres://postgres:password@localhost:5432/GPUFabric"
export REDIS_URL="redis://localhost:6379"
export API_KEY="your-api-key"
export RUST_LOG="gpuf-s=info"
```

### 管理 API Server

dashboard/前端使用的独立管理 API 默认绑定 loopback：

```bash
cargo run --release -p gpuf-s --bin api_server -- \
  --bind-addr 127.0.0.1 \
  --port 18081 \
  --database-url "$DATABASE_URL"
```

只有在反向代理、防火墙和访问控制已经配置好的部署环境中，才使用 `--bind-addr 0.0.0.0`。

## 🔧 开发

### 开发工作流

```bash
# 运行测试
cargo test

# 带日志运行
RUST_LOG=debug cargo run --release --bin gpuf-s

# 格式化代码
cargo fmt

# 运行 Linter
cargo clippy
```

### 项目结构
详见根目录 [README_CN.md](../README_CN.md)

```
GPUFabric/
├── gpuf-s/              # 服务器组件
│   └── src/
│       ├── main.rs            # 服务器入口
│       ├── handle/            # 连接处理器
│       ├── api_server/        # REST API 服务
│       ├── consumer/           # Kafka 消费者
│       ├── db/                 # 数据库操作
│       └── util/               # 实用工具
├── gpuf-c/              # 客户端组件
│   └── src/
│       ├── main.rs            # 客户端入口
│       ├── handle/            # 连接处理器
│       ├── llm_engine/        # LLM 引擎集成
│       └── util/               # 实用工具
├── common/            # 共享协议库
│   └── src/lib.rs     # 协议定义
└── docs/              # 文档
```


## 🎯 详细能力

### 🌐 AI 推理分发网络
- **分布式推理架构**：像 CDN 一样智能路由，请求按就近、负载与健康度分发
- **地理分布**：让推理更靠近用户，显著降低跨区网络时延
- **智能调度**：对多实例自动负载均衡，支持健康检查与权重调整
- **边缘侧支持**：边缘节点就地推理，减少数据回传、缩短响应链路
- **弹性伸缩**：按需横向扩容/缩容，不中断服务
- **健康治理**：实例异常自动摘除并回流流量，快速故障转移

### 🔐 模型隐私与安全
- **本地模型托管**：模型留存在自有基础设施，资产与权限完全可控
- **数据隐私保护**：请求与结果不经第三方，中端到端加密传输
- **TLS 1.3**：采用企业级加密标准，传输安全有保障
- **多重认证**：数据库认证 + Redis 缓存校验 + API Key 兜底
- **内核级防护**：基于 XDP/eBPF 的包级过滤与限流，缓解 DDoS

### ⚡ 快速访问（NAT 穿透）
- **NAT 穿透**：无需公网 IP，内网服务可安全暴露与访问
- **P2P 直连**（规划中）：优先点对点建立链路，进一步降低时延
- **亚毫秒路由**：Rust + Tokio 架构，极致低时延调度
- **缓存加速**：Redis 命中率高，降低数据库压力、提升响应速度
- **连接池化**：长连接复用，减少握手与重建开销

### 🚀 易于部署
- **一键部署**：`docker compose up -d` 启动完整服务栈
- **预构建镜像**：内置 gpuf-s、api_server、heartbeat_consumer 镜像
- **自动化脚本**：一键生成 TLS 证书与初始化数据库
- **零配置即用**：默认参数可开箱即用，可按需覆盖
- **灵活配置**：支持命令行、环境变量与配置文件多种方式

### 🌍 跨平台支持
- **多平台兼容**：原生支持 Linux、macOS、Windows
- **统一发布**：单一二进制，无额外复杂依赖
- **容器化部署**：Docker 镜像覆盖主流运行环境
- **ARM64 友好**：兼容 Apple Silicon（M1/M2/M3）与 ARM 服务器

## 🏗️ 架构

![GPUFabric System Architecture](./svg/GPUFabric.svg)

### 系统组件

GPUFabric 包含三个主要组件：

- **gpuf-s** —— 服务器程序，负责负载均衡、客户端管理与请求路由
- **gpuf-c** —— 客户端程序，连接服务器并转发到本地服务
- **common** —— 共享协议库，定义二进制命令与数据结构

### 四端口设计

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

| 端口 | 用途 | 协议 | 描述 |
|------|------|------|------|
| **17000** | 控制 | TCP | 客户端注册与指令下发的持久连接 |
| **17001** | 代理 | TCP | 双向数据转发的临时连接 |
| **18080** | 公共 | TCP/HTTP | 外部用户入口，带 API Key 校验 |
| **18081** | API | HTTP | 监控与管理用的 RESTful API 服务 |

### 请求流程

```
1. 用户连接到公共端口（18080）
   ↓
2. gpuf-s 校验 API Key（数据库校验或静态兜底）
   ↓
3. gpuf-s 从活跃客户端池中随机选择一个客户端
   ↓
4. gpuf-s 生成唯一的 proxy_conn_id
   ↓
5. gpuf-s 向选定客户端发送 RequestNewProxyConn
   ↓
6. gpuf-c 使用 NewProxyConn 连接代理端口（17001）
   ↓
7. gpuf-c 连接本地实际服务
   ↓
8. gpuf-s 基于 proxy_conn_id 匹配两端连接
   ↓
9. 开始双向数据转发

## 🔬 技术栈

### 核心技术
- **语言**：Rust（稳定版）+ Tokio 异步运行时
- **网络**：TLS 1.3，TCP/HTTP 协议
- **序列化**：Bincode 高效二进制协议

### 基础设施组件
- **数据库**：PostgreSQL —— 持久化存储、认证与统计
- **缓存**：Redis —— 5 分钟 TTL 缓存，降低 ~90% 数据库负载
- **消息队列**：Apache Kafka —— 异步心跳处理与请求跟踪
- **容器化**：Docker & Docker Compose —— 部署与运维

### 高性能特性

#### XDP (eXpress Data Path) - 内核级数据包过滤
- **基于 eBPF** 的网络驱动层数据包处理，超低延迟
- **内核级 API Key 校验**，在到达用户空间前完成验证
- **使用场景**：高性能请求验证与 DDoS 防护

详细 XDP 设置与使用，请参见 [XDP 文档](./xdp.md)

### 监控与可观测性
- **系统指标**：CPU、内存、磁盘、网络监控
- **能耗指标**：GPU/CPU/ANE 功耗（macOS M 系列）
- **网络统计**：带会话追踪的实时带宽监控
- **RESTful API**：对外暴露的指标查询接口

## 🗺️ 路线图

### ✅ 当前功能（生产可用）
- ✅ 高性能反向代理与负载均衡
- ✅ 基于数据库的认证（配合 Redis 缓存）
- ✅ 基于 Kafka 的异步心跳处理
- ✅ TLS 1.3 安全连接
- ✅ AI/LLM 模型路由（Ollama、vLLM）
- ✅ 实时系统监控与指标
- ✅ XDP 内核级数据包过滤（Linux）

### 🚧 开发中

#### P2P 混合架构
从纯粹的客户端-服务器模式迁移到混合 P2P 架构，以提升性能并降低服务器带宽开销。

**技术实现：**
- **NAT 穿透**：使用 STUN/TURN/ICE 协议进行对等发现
- **libp2p 集成**：Rust 原生 P2P 网络库
  - AutoNAT 自动识别 NAT 类型
  - Relay 协议用于中继回退
  - Hole Punching 进行打洞直连
  - DHT（分布式哈希表）用于对等发现
- **信令服务器**：由 gpuf-s 充当对等连接建立的信令服务器
- **智能路由**：依据网络条件在直连/中继/TURN 之间自动选择

**Protocol Design** (CommandV2):
```rust
// Already implemented in common/src/lib.rs
CommandV2::P2PConnectionRequest      // Initiate P2P handshake
CommandV2::P2PConnectionInfo         // Exchange peer addresses
CommandV2::P2PConnectionEstablished  // Confirm connection type
CommandV2::P2PConnectionFailed       // Fallback to relay mode
```

**收益：**
- 🚀 直连显著降低端到端时延
- 💰 降低服务器带宽成本
- 📈 大规模部署可扩展性更好
- 🔄 网络不佳时自动回退到中继模式

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

#### XDP 增强功能
- **动态规则更新**：热更新 XDP 规则，无需重启服务
- **速率限制**：内核级按 IP 进行限速
- **GeoIP 过滤**：基于地理位置的访问控制
- **DDoS 防护**：防御 SYN 洪泛与连接洪泛攻击

### 📋 未来增强
- [ ] 浏览器客户端的 WebSocket 支持
- [ ] 多区域部署与地理路由
- [ ] 指标增强并集成 Prometheus/Grafana
- [ ] 支持 HTTP/3（QUIC）协议
- [ ] 高级负载均衡算法（最少连接、加权轮询）
- [ ] 客户端侧负载预测与智能路由
- [ ] 使用 OpenTelemetry 的分布式追踪

### 🔬 研究与探索
- 基于区块链的去中心化认证
- 零知识证明的隐私保护认证
- 基于 FPGA 的报文处理加速
- 基于 eBPF 的流量整形与服务质量（QoS）

## 🤝 参与贡献

欢迎任何形式的贡献！欢迎提交 Pull Request。

1. Fork 本仓库
2. 创建特性分支（`git checkout -b feature/AmazingFeature`）
3. 提交更改（`git commit -m 'Add some AmazingFeature'`）
4. 推送分支（`git push origin feature/AmazingFeature`）
5. 提交 Pull Request

### 开发指南

- 遵循 Rust 最佳实践与代码风格
- 为新功能补充测试
- 按需更新文档
- 提交前确保所有测试通过

## 📊 性能

- **吞吐**：基于 Tokio 的高性能异步 I/O
- **时延**：亚毫秒级路由调度
- **扩展性**：支持海量客户端连接
- **缓存**：Redis 缓存可降低约 90% 的数据库负载
- **批处理**：可配置批量心跳处理，效率更高

## 🔒 安全

- 管理 API 默认绑定 loopback，公开监听必须显式部署配置
- 基于数据库的认证与 Token 校验
- Redis 缓存提升性能且不降低安全性
- 输入校验、SQL 注入防护与服务日志脱敏
- 模型下载和 SDK/release artifact 使用 SHA256 校验，MD5 不作为信任依据
- 发布 gate 覆盖 cargo audit/deny、secret scan、targeted tests 和源码 grep

## 🌟 使用场景

- **AI 模型服务**：将请求路由到分布式推理引擎
- **服务暴露**：安全地将本地服务暴露到公网
- **负载均衡**：在多个后端实例间分发流量
- **监控告警**：实时监控系统与应用
- **开发联调**：随时随地访问本地开发环境

## 📝 许可证

本项目基于 BSD 3-Clause 许可证，详情见 [LICENSE](LICENSE)。


## 📮 支持

- 📖 [文档索引](./)
- 🐛 [问题追踪](https://github.com/nexus-gpu/GPUFabric/issues)
- 💬 [讨论区](https://github.com/nexus-gpu/GPUFabric/discussions)

---

用 ❤️ 和 Rust 打造
