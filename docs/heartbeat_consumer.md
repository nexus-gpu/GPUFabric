# Heartbeat Consumer Documentation

[![Rust](https://img.shields.io/badge/rust-stable-brightgreen.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

The Heartbeat Consumer is a high-performance Kafka consumer service that processes client heartbeat messages from the `client-heartbeats` topic and stores them in PostgreSQL database. It uses batch processing for optimal performance and supports configurable batch size and timeout settings.

## Overview

The Heartbeat Consumer is a critical component of the gpuf-s system that:
- Consumes heartbeat messages from Kafka
- Processes messages in batches for efficiency
- Stores heartbeat data, system information, and device statistics in PostgreSQL
- Maintains daily statistics for clients and devices

## Features

- **Batch Processing**: Groups multiple messages together for efficient database writes
- **Configurable Batching**: Adjustable batch size and timeout settings
- **Asynchronous Processing**: Non-blocking message processing using Tokio
- **Transaction Support**: Database operations are wrapped in transactions
- **Error Handling**: Robust error handling with detailed logging
- **Auto-recovery**: Continues processing even if individual messages fail

## Architecture

![Heartbeat Consumer Architecture](./svg/heartbeat_consumer.svg)

The Heartbeat Consumer follows a producer-consumer pattern with the following flow:

```markdown:GPUFabric/docs/heartbeat_consumer.md
┌─────────────┐
│   Kafka     │
│   Topic:    │
│ client-     │
│ heartbeats  │
└──────┬──────┘
       │
       ▼
┌─────────────────┐
│  Consumer       │
│  (Batching)     │
└──────┬──────────┘
       │
       ▼
┌─────────────────┐
│  Processor      │
│  (Database)     │
└──────┬──────────┘
       │
       ▼
┌─────────────────┐
│  PostgreSQL     │
│  - heartbeat    │
│  - client_daily_│
│    stats        │
│  - device_daily_│
│    stats        │
│  - device_info  │
│  - system_info  │
└─────────────────┘
```

The system consists of two main components:

1. **Consumer**: Reads messages from Kafka and batches them for efficient processing
2. **Processor**: Deserializes messages and writes data to PostgreSQL database tables

Messages flow from Kafka through the consumer, are batched, and then processed in transactions to ensure data consistency.

## Message Format

### HeartbeatMessage Structure

Heartbeat messages are encoded using bincode (binary encoding) and contain the following structure:

```rust
pub struct HeartbeatMessage {
    pub client_id: ClientId,           // 16-byte client identifier
    pub system_info: SystemInfo,      // System metrics
    pub device_memtotal_gb: u32,      // Total device memory in GB
    pub device_count: u32,            // Number of devices
    pub total_tflops: u32,            // Total TFLOPS
    pub devices_info: Vec<DevicesInfo>, // Device information array
}
```

### SystemInfo Structure

```rust
pub struct SystemInfo {
    pub cpu_usage: u8,        // CPU usage percentage (0-100)
    pub memory_usage: u8,     // Memory usage percentage (0-100)
    pub disk_usage: u8,       // Disk usage percentage (0-100)
    pub network_rx: u64,      // Network receive bytes
    pub network_tx: u64,      // Network transmit bytes
}
```

### DevicesInfo Structure

```rust
pub struct DevicesInfo {
    // Pod information
    pub num: u16,              // Device number
    pub pod_id: u16,           // Pod identifier
    pub total_tflops: u16,     // Device TFLOPS
    pub memtotal_gb: u16,      // Total memory in GB
    pub port: u16,             // Port number
    pub ip: u32,               // IP address
    pub os_type: OsType,       // Operating system type
    pub engine_type: EngineType, // Inference engine type
    
    // Device metrics
    pub usage: u64,            // GPU usage percentage (0-100)
    pub mem_usage: u64,        // Memory usage percentage (0-100)
    pub power_usage: u64,      // Power usage (watts)
    pub temp: u64,             // Temperature (degrees Celsius)
    pub vendor_id: u128,       // GPU vendor ID
    pub device_id: u128,       // GPU device ID
    pub memsize_gb: u128,      // Memory size in GB
    pub powerlimit_w: u128,   // Power limit in watts
}
```

## Installation

### Prerequisites

- Rust toolchain (stable)
- PostgreSQL database
- Kafka broker (for message queue)
- Access to the `client-heartbeats` Kafka topic

### Building

```bash
# Build the heartbeat consumer binary
cargo build --release --bin heartbeat_consumer

# The binary will be located at:
# target/release/heartbeat_consumer
```

## Usage

### Command Line Arguments

```bash
./heartbeat_consumer \
  --batch-size 100 \
  --batch-timeout 5 \
  --database-url "postgres://<db-user>:<db-password>@localhost/<db-name>" \
  --bootstrap-server "localhost:9092"
```

| Argument | Type | Default | Description |
|----------|------|---------|-------------|
| `--batch-size` | usize | 100 | Number of messages to batch before processing |
| `--batch-timeout` | u64 | 5 | Timeout in seconds before flushing incomplete batches |
| `--database-url` | string | `postgres://<db-user>:<db-password>@localhost/<db-name>` | PostgreSQL connection string |
| `--bootstrap-server` | string | `localhost:9092` | Kafka broker address |

### Environment Variables

The consumer uses the following environment variables for logging:

```bash
# Set log level (default: info)
RUST_LOG=gpuf-s=info

# More verbose logging
RUST_LOG=gpuf-s=debug

# Include all dependencies
RUST_LOG=info
```

### Running the Consumer

```bash
# Basic usage with defaults
./heartbeat_consumer

# Custom configuration
./heartbeat_consumer \
  --batch-size 200 \
  --batch-timeout 10 \
  --database-url "postgres://<db-user>:<db-password>@localhost:5432/<db-name>" \
  --bootstrap-server "kafka.example.com:9092"

# With custom log level
RUST_LOG=debug ./heartbeat_consumer --batch-size 50
```

## Configuration

### Kafka Consumer Configuration

The consumer is configured with the following Kafka settings:

| Setting | Value | Description |
|---------|-------|-------------|
| `enable.partition.eof` | `false` | Don't signal EOF when reaching end of partition |
| `enable.auto.commit` | `false` | Manual commit control |
| `session.timeout.ms` | `30000` | Session timeout (30 seconds) |
| `max.poll.interval.ms` | `300000` | Maximum poll interval (5 minutes) |
| `fetch.max.bytes` | `1048576` | Maximum fetch size (1 MB) |
| `max.partition.fetch.bytes` | `1048576` | Maximum bytes per partition (1 MB) |
| `group.id` | `heartbeat-consumer-group` | Consumer group identifier |

### Batch Processing

The consumer implements intelligent batching:

1. **Size-based batching**: Messages are collected until the batch size is reached
2. **Time-based flushing**: Incomplete batches are flushed after the timeout period (default: 1 second)
3. **Shutdown handling**: Remaining messages are processed on graceful shutdown

### Database Connection Pool

- **Max connections**: 10
- **Connection string**: Provided via `--database-url` argument

## Data Processing

### Processing Flow

1. **Consumption**: Consumer reads messages from Kafka topic `client-heartbeats`
2. **Batching**: Messages are collected into batches
3. **Deserialization**: Binary messages are decoded using bincode
4. **Database Operations**: Multiple database operations are performed in a transaction:
   - Insert heartbeat record
   - Update client daily statistics
   - Update device daily statistics
   - Update device information
5. **Commit**: Transaction is committed on successful processing

### Database Tables

#### heartbeat

Stores individual heartbeat records:

```sql
CREATE TABLE heartbeat (
  id SERIAL,
  client_id BYTEA NOT NULL,
  cpu_usage SMALLINT,
  mem_usage SMALLINT,
  disk_usage SMALLINT,
  network_up BIGINT NOT NULL DEFAULT 0,
  network_down BIGINT NOT NULL DEFAULT 0,
  timestamp TIMESTAMPTZ NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
  PRIMARY KEY (client_id, timestamp)
);
```

#### client_daily_stats

Stores aggregated daily statistics for clients:

```sql
CREATE TABLE client_daily_stats (
    id BIGSERIAL PRIMARY KEY,
    date DATE NOT NULL,
    client_id BYTEA NOT NULL,
    total_heartbeats INTEGER DEFAULT 0,
    avg_cpu_usage FLOAT,
    avg_memory_usage FLOAT,
    avg_disk_usage FLOAT,
    total_network_in_bytes BIGINT,
    total_network_out_bytes BIGINT,
    last_heartbeat TIMESTAMPTZ,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (client_id, date)
);
```

#### device_daily_stats

Stores aggregated daily statistics for devices:

```sql
CREATE TABLE device_daily_stats (
    id BIGSERIAL PRIMARY KEY,
    date DATE NOT NULL,
    client_id BYTEA NOT NULL,
    device_index SMALLINT NOT NULL,
    device_name VARCHAR(255),
    total_heartbeats INTEGER DEFAULT 0,
    avg_utilization FLOAT,
    avg_temperature FLOAT,
    avg_power_usage FLOAT,
    avg_memory_usage FLOAT,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (client_id, device_index, date)
);
```

#### device_info

Stores current device information:

```sql
CREATE TABLE device_info (
    client_id BYTEA NOT NULL,
    device_index SMALLINT,
    device_name VARCHAR(255),
    device_id INTEGER,
    vendor_id INTEGER,
    device_memusage SMALLINT,
    device_gpuusage SMALLINT,
    device_powerusage SMALLINT,
    device_temp SMALLINT,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (client_id, device_index)
);
```

#### system_info

Stores current system information:

```sql
CREATE TABLE system_info (
    client_id BYTEA NOT NULL,
    cpu_usage SMALLINT,
    mem_usage SMALLINT,
    disk_usage SMALLINT,
    total_tflops INTEGER,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (client_id)
);
```

### Statistics Aggregation

The consumer automatically aggregates statistics:

- **Client Daily Stats**: Calculates rolling averages for CPU, memory, and disk usage
- **Device Daily Stats**: Tracks device utilization, temperature, power usage, and memory usage
- **Network Traffic**: Accumulates total network inbound and outbound bytes

## Performance Tuning

### Batch Size Optimization

Adjust batch size based on message volume:

```bash
# High throughput scenario
--batch-size 500

# Low latency scenario
--batch-size 50

# Balanced (default)
--batch-size 100
```

### Batch Timeout

Adjust timeout to balance latency and throughput:

```bash
# Lower latency (process more frequently)
--batch-timeout 2

# Higher throughput (wait longer for full batches)
--batch-timeout 10
```

### Database Connection Pool

Modify the connection pool size in the source code if needed:

```rust
let db_pool = sqlx::postgres::PgPoolOptions::new()
    .max_connections(20)  // Increase for higher throughput
    .connect(&args.database_url)
    .await?;
```

## Monitoring

### Logging

The consumer provides detailed logging:

```bash
# Info level (default)
INFO Starting heartbeat consumer with batch size: 100
INFO Heartbeat received from client 6e1131b4... cpu_usage 45% memory_usage 60% disk_usage 30%

# Debug level
DEBUG Heartbeat consumer timeout
DEBUG Processed batch of 100 messages
DEBUG Successfully processed heartbeat for client: 6e1131b4...
```

### Error Handling

Errors are logged but don't stop processing:

- **Message deserialization errors**: Logged and skipped
- **Database errors**: Logged, transaction rolled back, continue processing
- **Kafka connection errors**: Logged and retried

### Health Checks

Monitor the consumer process:

```bash
# Check if process is running
ps aux | grep heartbeat_consumer

# Monitor logs
tail -f /var/log/heartbeat_consumer.log

# Check Kafka consumer lag
kafka-consumer-groups.sh --bootstrap-server localhost:9092 \
  --group heartbeat-consumer-group --describe
```

## Troubleshooting

### Common Issues

#### 1. Database Connection Failures

**Symptoms**: Error messages about database connection

**Solutions**:
- Verify database is running and accessible
- Check connection string format
- Ensure database user has proper permissions
- Check network connectivity

#### 2. Kafka Connection Issues

**Symptoms**: No messages being consumed

**Solutions**:
- Verify Kafka broker is running
- Check bootstrap server address
- Verify topic exists: `kafka-topics.sh --list --bootstrap-server localhost:9092`
- Check consumer group status

#### 3. High Memory Usage

**Symptoms**: Process consuming excessive memory

**Solutions**:
- Reduce batch size
- Increase batch timeout to process fewer batches
- Monitor message size and count

#### 4. Slow Processing

**Symptoms**: Messages lagging behind

**Solutions**:
- Increase batch size for better throughput
- Increase database connection pool size
- Optimize database indexes
- Check database performance

#### 5. Message Deserialization Errors

**Symptoms**: "Failed to deserialize heartbeat" errors

**Solutions**:
- Verify message format matches expected structure
- Check bincode version compatibility
- Verify message producer is using correct encoding

## Development

### Running Tests

```bash
# Run all tests
cargo test

# Run specific test
cargo test test_heartbeat_processing
```

### Code Structure

```
gpuf-s/src/consumer/
├── mod.rs                    # Main consumer service entry point
├── heartbeat_consumer.rs     # Kafka consumer implementation
└── heartbeat_processor.rs    # Message processing and database operations

gpuf-s/src/bin/
└── heartbeat_consumer.rs     # Binary entry point with CLI
```

### Adding Features

1. **Custom Processing Logic**: Modify `heartbeat_processor.rs`
2. **Additional Metrics**: Update database schema and processing logic
3. **New Message Formats**: Update deserialization in processor

## Integration

### With gpuf-s

The heartbeat consumer works alongside the main gpuf-s server:

1. **gpuf-s** receives heartbeat messages from clients
2. **gpuf-s** publishes messages to Kafka topic `client-heartbeats`
3. **heartbeat_consumer** consumes and processes messages
4. **API Server** queries processed data for monitoring

### With Kafka

The consumer subscribes to the `client-heartbeats` topic:

```bash
# Create topic if it doesn't exist
kafka-topics.sh --create \
  --bootstrap-server localhost:9092 \
  --topic client-heartbeats \
  --partitions 3 \
  --replication-factor 1
```

### With PostgreSQL

Ensure database tables exist:

```bash
# Run database migrations
psql -U postgres -d GPUFabric -f scripts/db.sql
```

## Best Practices

1. **Monitor Consumer Lag**: Regularly check Kafka consumer lag
2. **Batch Size Tuning**: Adjust based on message volume and latency requirements
3. **Database Indexing**: Ensure proper indexes on frequently queried columns
4. **Error Monitoring**: Set up alerts for persistent errors
5. **Resource Management**: Monitor CPU, memory, and database connections
6. **Backup Strategy**: Regularly backup statistics tables
7. **Scaling**: Run multiple consumer instances for high-throughput scenarios

## Limitations

- **Single Topic**: Currently only processes `client-heartbeats` topic
- **No Message Ordering**: Messages are processed in batches, not strictly ordered
- **No Retry Logic**: Failed messages are logged but not retried
- **Fixed Schema**: Message format must match expected structure

## Future Enhancements

- [ ] Dead letter queue for failed messages
- [ ] Message retry mechanism
- [ ] Support for multiple topics
- [ ] Metrics export (Prometheus)
- [ ] Distributed tracing support
- [ ] Configurable retry policies
- [ ] Message filtering capabilities

## Changelog

### Initial Version
- Basic Kafka consumer with batch processing
- PostgreSQL storage for heartbeat data
- Daily statistics aggregation
- Device and system information tracking

## License

See [LICENSE](../LICENSE) file for details.
```
