# gpuf-s API Server

## Security Defaults

The standalone management API binds to loopback by default:

```bash
cargo run --release -p gpuf-s --bin api_server -- \
  --bind-addr 127.0.0.1 \
  --port 18081 \
  --database-url "$DATABASE_URL" \
  --redis-url "redis://127.0.0.1:6379"
```

Use `--bind-addr 0.0.0.0` only behind a reverse proxy, firewall, TLS, and deployment access control. Frontend integration details live in `../../../docs/api_server.md` and `../../../gui/doc.md`.
This management API is independent of gpuf-s worker control TLS. Remote worker deployments should enable `gpuf-s --control-tls` separately on the main server process. Mobile native TLS worker startup is exposed through the gpuf-c SDK and does not change frontend REST integration.

## API Documentation

The server provides a comprehensive RESTful API for monitoring and management:

### Client Management
- `POST /api/user/insert_client` - insert a client
- `GET /api/user/client_list` - Get all active clients
- `GET /api/user/client_device_detail` - Get specific client information
- `POST /api/user/edit_client_info` - edit a client info


### client Monitoring
- `GET /api/user/client_stat` - Get client client status
- `GET /api/user/client_monitor` - Get system metrics for clients
- `GET /api/user/client_health` - client health check


### Model Management APIs
- `POST /api/models/insert` - insert a model
- `GET /api/models/get` - Get all models

### Statistics & Connections
- `GET /api/stats` - Get server statistics (uptime, connections, etc.)
- `GET /api/connections` - Get current connection information
- `GET /api/connections/pending` - Get pending connections count

### Authentication
- `GET /api/users` - Get registered users list
- `GET /api/tokens/active` - Get active authentication tokens

### API Response Format
All API responses follow this format:
```json
{
  "success": true,
  "data": { /* response data */ },
  "message": "Operation successful",
  "timestamp": "2025-07-29T17:55:48.826362Z"
}
```

## Getting Started
# Monitor Redis cache hits
redis-cli monitor

# Check API endpoints
curl http://localhost:18081/api/user/client_list?user_id=12
curl http://localhost:18081/api/user/client_stat?user_id=12
curl http://localhost:18081/api/user/client_monitor?user_id=12
curl http://localhost:18081/api/user/client_health?user_id=12
```

Observe the server logs showing:
- Database token validation with Redis caching
- Random client selection: `Chose client 'client_A' for the new connection`
- Cache hits reducing database queries

### 6. Monitor Client System Information

#### Command Line Monitoring
To view the system information reported by clients, use the `--monitor` flag with the server:

```bash
cargo run --release --bin gpuf-s -- --monitor
```

This will display a table with the latest system metrics reported by each active client.

#### API Monitoring
You can also use the HTTP API to monitor clients:

```bash
# Get clients
curl http://localhost:18081/api/user/client_list?user_id=12

# Get server statistics
curl http://localhost:18081/api/user/client_stat?user_id=12

# Get system monitoring data
curl http://localhost:18081/api/user/client_monitor?user_id=12

# Health check
curl http://localhost:18081/api/user/client_health?user_id=12
```


## API Examples

Here are some practical examples of using the API:

### Monitor All Clients
```bash
curl -s http://localhost:18081/api/user/client_list?user_id=12 | jq '.'
```

### Check Server Health and Uptime
```bash
curl -s http://localhost:18081/api/health | jq '.data'
```

### Get Detailed Server Statistics
```bash
curl -s http://localhost:18081/api/user/client_stat?user_id=12 | jq '.data'
```

### Monitor System Resources of All Clients
```bash
curl -s http://localhost:18081/api/user/client_monitor?user_id=12 | jq '.data'
```

### Disconnect a Specific Client
```bash
curl -X DELETE http://localhost:18081/api/user/client_list?user_id=12
```

### Check Configuration
```bash
curl -s http://localhost:18081/api/config | jq '.data'
```
