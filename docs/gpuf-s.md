# gpuf-s - Fast Reverse Proxy Server

[![Rust](https://img.shields.io/badge/rust-stable-brightgreen.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

gpuf-s is a high-performance reverse proxy server with enterprise-grade features including load balancing, database-backed authentication, Redis caching, Kafka integration, and comprehensive monitoring capabilities.

## Overview

gpuf-s is the server component of the frpx reverse proxy system. It manages client connections, routes traffic with load balancing, provides RESTful APIs for monitoring, and integrates with PostgreSQL, Redis, and Kafka for a complete enterprise solution.

For system architecture and integration details, see the [main README](../README.md).

## Features

- **Random Load Balancing**: Intelligent request distribution across active clients
- **High Availability**: Automatic failover with client health monitoring
- **Database Integration**: PostgreSQL for authentication, client management, and statistics
- **Redis Caching**: 5-minute TTL caching for reduced database load
- **Kafka Integration**: Message queue for heartbeat processing and request tracking
- **TLS Support**: Secure connections with TLS 1.3 encryption
- **RESTful API**: Comprehensive HTTP API for monitoring and management
- **Model Management**: AI model routing and client selection based on model availability
- **Real-time Monitoring**: System metrics, client status, and performance tracking
- **Cross-Platform**: Linux, macOS, and Windows support

## Installation

### Prerequisites

- Rust toolchain (stable)
- PostgreSQL database
- Redis server (optional but recommended)
- Kafka broker (optional, for message queue)
- TLS certificates (for secure connections)

### Building

```bash
# Build in release mode
cargo build --release --bin gpuf-s

# The binary will be located at:
# target/release/gpuf-s
```

### Generate TLS Certificates

```bash
# Generate self-signed certificates
../scripts/create_cert.sh
```

This creates:
- cert.pem (certificate chain)
- key.pem (private key)

## Usage

### Basic Usage

```bash
# Start with default configuration
./gpuf-s

# Start with custom ports
./gpuf-s \
  --control-port 17000 \
  --proxy-port 17001 \
  --public-port 18080 \
  --api-port 18081
```

### Command Line Arguments

| Argument | Type | Default | Description |
|----------|------|---------|-------------|
| `--control-port` | u16 | 17000 | Port for client control connections |
| `--proxy-port` | u16 | 17001 | Port for client proxy connections |
| `--public-port` | u16 | 18080 | Port for public user connections |
| `--api-port` | u16 | 18081 | Port for HTTP API server |
| `--api-key` | string | `abc123` | Fallback API key for authentication |
| `--database-url` | string | `postgres://username:password@localhost/database` | PostgreSQL connection string |
| `--redis-url` | string | `redis://127.0.0.1:6379` | Redis connection string |
| `--bootstrap-server` | string | `localhost:9092` | Kafka broker address |
| `--proxy-cert-chain-path` | string | `cert.pem` | Path to TLS certificate chain |
| `--proxy-private-key-path` | string | `key.pem` | Path to TLS private key |
| `--monitor` | flag | false | Print client monitoring data and exit |

### Complete Example

```bash
./gpuf-s \
  --control-port 17000 \
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

### Environment Variables

You can also configure using environment variables:

```bash
export DATABASE_URL="postgres://user:pass@localhost/frpx"
export REDIS_URL="redis://localhost:6379"
export API_KEY="your-api-key"
```

## Configuration

### Database Setup

Initialize the PostgreSQL database:

```bash
# Create database
createdb GPUFabric

# Run schema migrations
psql -U postgres -d GPUFabric -f ../scripts/db.sql
```

The database stores:
- API keys and tokens
- Client information
- System statistics
- Model information
- Heartbeat data

### Redis Configuration

Redis is used for caching token validations with a 5-minute TTL:

```bash
# Start Redis (if not already running)
redis-server

# Or using Docker
docker run -d -p 6379:6379 redis:alpine
```

### Kafka Configuration

Kafka is used for message queuing:

```bash
# Start Kafka using Docker Compose
docker compose -f ../kafka_compose.yaml up -d

# Create required topics
docker exec -it <kafka-container> kafka-topics --create \
  --topic client-heartbeats \
  --bootstrap-server localhost:9092 \
  --partitions 1 \
  --replication-factor 1

docker exec -it <kafka-container> kafka-topics --create \
  --topic request-message \
  --bootstrap-server localhost:9092 \
  --partitions 1 \
  --replication-factor 1
```

## Core Components

### Server State

The server maintains shared state across all connections:

```rust
pub struct ServerState {
    pub active_clients: Arc<Mutex<HashMap<ClientId, ClientInfo>>>,
    pub pending_connections: Arc<Mutex<HashMap<ProxyConnId, (TcpStream, BytesMut)>>>,
    pub user_db: Arc<Mutex<HashMap<String, User>>>,
    pub token_db: Arc<Mutex<HashMap<String, String>>>,
    pub db_pool: Arc<Pool<Postgres>>,
    pub redis_client: Arc<RedisClient>,
    pub producer: Arc<FutureProducer>,
    pub cert_chain: Arc<Vec<CertificateDer<'static>>>,
    pub priv_key: Arc<PrivateKeyDer<'static>>,
    pub buffer_pool: Arc<BufferPool>,
}
```

### Client Information

```rust
pub struct ClientInfo {
    pub writer: Arc<Mutex<OwnedWriteHalf>>,
    pub authed: bool,
    pub version: u32,
    pub system_info: Option<SystemInfo>,
    pub devices_info: Vec<DevicesInfo>,
    pub connected_at: DateTime<Utc>,
    pub models: Option<Vec<Model>>,
}
```

### Command Protocol

The server communicates with clients using JSON-based commands:

- **Login**: Client authentication and registration
- **LoginResult**: Authentication response with model information
- **RequestNewProxyConn**: Request proxy connection from client
- **NewProxyConn**: Client establishes proxy connection
- **Heartbeat**: Periodic health check from clients
- **SystemInfo**: Client system metrics

## Load Balancing

### Random Selection Algorithm

The server uses random selection for load balancing:

```rust
let client_ids: Vec<ClientId> = active_clients.keys().cloned().collect();
let chosen_client_id = client_ids.choose(&mut rand::thread_rng())?;
```

### Model-Based Routing

When a model is specified in the request, the server selects clients that have that model available:

1. Filter clients by model availability
2. Select from available clients
3. Fall back to random selection if no model match

### High Availability

- **Automatic Failover**: Failed clients are removed from the pool
- **Health Monitoring**: Heartbeat system detects client disconnections
- **Connection Recovery**: Automatic cleanup on connection errors

## Authentication

### API Key Validation

The server validates API keys in this order:

1. **Database Lookup**: Check PostgreSQL for valid API key
2. **Redis Cache**: 5-minute TTL cache for performance
3. **Static Fallback**: Use `--api-key` if database unavailable

### Token-Based Authentication

Clients can authenticate using:
- Email/password (first-time login)
- Token (stored in `token.json` on client)

### Access Levels

API keys can have different access levels:
- `-1`: Shared access (requests logged to Kafka)
- `0+`: Dedicated client access

## Monitoring

### RESTful API

The API server runs on port 18081 and provides endpoints for:

- Client management
- System statistics
- Health checks
- Model information

See [API Server Documentation](./api_server.md) for detailed API reference.

### Client Monitoring

Use the `--monitor` flag to print client monitoring data:

```bash
./gpuf-s --monitor

# Output:
# Client ID              CPU (%)    Memory (%) Disk (%)  Last Heartbeat
# --------------------------------------------------------------------------------
# 6e1131b4...           45.00      60.00      30.00      15s ago
```

### Logging

The server provides structured logging:

```bash
# Set log level
RUST_LOG=gpuf-s=info ./gpuf-s
RUST_LOG=gpuf-s=debug ./gpuf-s
```

# Log levels:
# - error: Critical errors only
# - warn: Warnings and errors
# - info: General information (default)
# - debug: Detailed debugging information

## Performance

### Connection Pooling

- **PostgreSQL**: Connection pool with configurable limits
- **Redis**: Connection reuse with connection pooling
- **Kafka**: Producer with async message sending

### Buffer Pool

Efficient memory management with a buffer pool:

```rust
pub struct BufferPool {
    pool: Vec<BytesMut>,
    capacity: usize,
}
```

### Keepalive Settings

TCP keepalive configured for:
- Time: 30 seconds
- Interval: 10 seconds
- Retries: 3

## Security

### TLS Encryption

- TLS 1.3 support
- Certificate chain validation
- Secure key exchange

### Input Validation

- Comprehensive parameter validation
- SQL injection prevention
- XSS protection in API responses

### Rate Limiting

Consider implementing rate limiting for production use.

## Troubleshooting

### Common Issues

#### 1. Database Connection Failures

**Symptoms**: Error messages about database connection

**Solutions**:
- Verify PostgreSQL is running
- Check connection string format
- Ensure database user has permissions
- Verify network connectivity

#### 2. No Clients Available

**Symptoms**: "No available clients" errors

**Solutions**:
- Verify clients are connected to control port
- Check client authentication
- Ensure clients are in active pool
- Check server logs for client registration

#### 3. Port Already in Use

**Symptoms**: "Address already in use" errors

**Solutions**:
- Check if another instance is running
- Change port numbers
- Kill existing process: `lsof -ti:17000 | xargs kill`

#### 4. TLS Certificate Errors

**Symptoms**: Certificate validation failures

**Solutions**:
- Verify certificate files exist
- Check certificate format (PEM)
- Ensure private key matches certificate
- Regenerate certificates if needed

#### 5. Redis Connection Issues

**Symptoms**: Cache misses, performance degradation

**Solutions**:
- Verify Redis is running
- Check Redis URL format
- Test connection: `redis-cli ping`
- Server will fall back to database if Redis unavailable

## Development

### Running Tests

```bash
# Run all tests
cargo test

# Run specific test
cargo test test_load_balancing
```

### Code Structure

```
gpuf-s/src/
├── main.rs                    # Entry point and server initialization
├── handle/                    # Connection handlers
│   ├── mod.rs                 # Server state and shared types
│   ├── handle_connections.rs  # Client and proxy connection handlers
│   └── handle_agent.rs        # Public connection routing
├── api_server/                # RESTful API server
│   ├── mod.rs                 # API server initialization
│   ├── handle_api.rs          # API router setup
│   ├── client.rs              # Client management endpoints
│   └── models.rs              # Model management endpoints
├── consumer/                  # Kafka consumer (heartbeat processing)
├── db/                        # Database operations
│   ├── client.rs              # Client database operations
│   ├── stats.rs               # Statistics operations
│   └── models.rs              # Model database operations
├── util/                      # Utility functions
│   ├── cmd.rs                 # Command-line argument parsing
│   ├── db.rs                  # Database initialization
│   ├── protoc.rs              # Protocol definitions
│   └── msg.rs                 # Message types
└── xdp/                       # XDP support (Linux only)
```

### Adding Features

1. **New API Endpoints**: Add to `api_server/` modules
2. **Database Operations**: Extend `db/` modules
3. **Protocol Changes**: Update `util/protoc.rs`
4. **Load Balancing**: Modify `handle_agent.rs`

## Best Practices

1. **Production Deployment**:
   - Use environment variables for sensitive configuration
   - Enable TLS for all connections
   - Set up proper firewall rules
   - Monitor resource usage

2. **Database**:
   - Use connection pooling efficiently
   - Set up regular backups
   - Monitor query performance
   - Index frequently queried columns

3. **Redis**:
   - Configure appropriate TTL values
   - Monitor memory usage
   - Set up Redis persistence if needed

4. **Kafka**:
   - Configure appropriate partition counts
   - Monitor consumer lag
   - Set up retention policies

5. **Monitoring**:
   - Set up log aggregation
   - Monitor API response times
   - Track client connection counts
   - Alert on failures

## Limitations

- **Single Server**: Currently runs as a single instance
- **Random Load Balancing**: No weighted or round-robin options
- **No Session Persistence**: Requests may go to different clients
- **Limited Protocol Support**: Currently TCP/HTTP only

## Future Enhancements

- [ ] Weighted load balancing
- [ ] Session persistence
- [ ] WebSocket support
- [ ] gRPC support
- [ ] Multi-region deployment
- [ ] Advanced metrics (Prometheus)
- [ ] Distributed tracing
- [ ] Rate limiting
- [ ] WebSocket support for control channel

## Related Documentation

- [Main README](../README.md) - System architecture and overview
- [gpuf-c Documentation](./gpuf-c.md) - Client component documentation
- [API Server Documentation](./api_server.md) - RESTful API reference
- [Heartbeat Consumer Documentation](./heartbeat_consumer.md) - Kafka consumer documentation

## Changelog

### Initial Version
- Load balancing with random selection
- PostgreSQL and Redis integration
- Kafka message queue support
- RESTful API server
- TLS encryption
- Model-based routing
- Comprehensive monitoring

## License

See [LICENSE](../LICENSE) file for details.
```
