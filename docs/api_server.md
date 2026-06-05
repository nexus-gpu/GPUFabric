# gpuf-s API Server Documentation

[![Rust](https://img.shields.io/badge/rust-stable-brightgreen.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

The gpuf-s API Server provides a comprehensive set of RESTful APIs for managing clients, monitoring system status, and managing models.

## Basic Information

- **Base URL**: `http://127.0.0.1:18081` (default local development port)
- **Content-Type**: `application/json`
- **CORS**: Cross-origin requests are supported
- **Bind Address**: the standalone `api_server` binary now binds to `127.0.0.1` by default. Use `--bind-addr 0.0.0.0` only behind a reverse proxy, firewall, and deployment-level access control.

## Frontend Integration Contract

Frontend clients should treat `http://127.0.0.1:18081` as the default development API origin. For browser deployments, put the API behind a same-origin reverse proxy or configure an environment variable such as `GPUFABRIC_API_BASE_URL` / `VITE_GPUFABRIC_API_BASE_URL` instead of hardcoding a public host.

The security remediation keeps the existing management REST contract compatible: `/api/user/*`, `/api/models/*`, `/api/apk/*`, `/api/user/points`, and the unified response envelope remain unchanged. New model metadata fields (`download_url`, `checksum`, `expected_size`) are additive and optional for older frontends. Native `gpuf-c` SDK/FFI signatures are also compatible; integrations only need to account for stricter defaults such as explicit server addresses and SHA256-verified artifacts.

Do not put long-lived service credentials, database passwords, TURN credentials, or release signing keys into browser code. If a public frontend needs access, terminate TLS/auth at the deployment edge and forward only the required API calls to the loopback-bound API server.

Worker control-plane note: frontend REST paths are unchanged by gpuf-s/gpuf-c control TLS or by the native mobile `startRemoteWorkerWithTls` API. For deployments that also onboard remote workers over non-loopback networks, run gpuf-s with `--control-tls` and configure gpuf-c with `--control-tls --control-tls-server-name <name> --cert-chain-path <ca.pem>`; mobile apps should use the additive native TLS SDK entry point rather than putting worker credentials or pins in browser code.


## API Response Format

All API responses follow a unified format:

```json
{
  "success": true,
  "data": { /* response data */ },
  "message": "Operation successful",
  "timestamp": "2025-07-29T17:55:48.826362Z"
}
```

### Error Response

```json
{
  "success": false,
  "data": null,
  "message": "Error description",
  "timestamp": "2025-07-29T17:55:48.826362Z"
}
```

## Client Management APIs

### 1. Create/Update Client

**POST** `/api/user/insert_client`

Create or update client information.

#### Request Body

```json
{
  "user_id": "string (1-32 characters)",
  "client_id": "string (1-32 characters)",
  "client_status": "string",
  "os_type": "string (optional, 1-64 characters)",
  "name": "string (1-32 characters)"
}
```

#### Request Parameters

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `user_id` | string | Yes | User ID, 1-32 characters |
| `client_id` | string | Yes | Client ID, 1-32 characters, must be a valid 16-byte hexadecimal string |
| `client_status` | string | Yes | Client status |
| `os_type` | string | No | Operating system type, 1-64 characters |
| `name` | string | Yes | Client name, 1-32 characters |

#### Response Example

```json
{
  "success": true,
  "data": [],
  "message": "Operation successful",
  "timestamp": "2025-07-29T17:55:48.826362Z"
}
```

#### Status Codes

- `200 OK`: Create/update successful
- `400 Bad Request`: Invalid request parameters (user_id or client_id is empty, or client_id format is incorrect)

---

### 2. Get Client List

**GET** `/api/user/client_list`

Get all clients for a user with support for multiple filter conditions.

#### Query Parameters

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `user_id` | string | Yes | User ID |
| `client_id` | string | No | Filter by client ID |
| `status` | string | No | Filter by client status |
| `name` | string | No | Search by name (case-insensitive partial match) |
| `valid_status` | string | No | Filter by valid status (valid/invalid) |

#### Response Example

```json
{
  "success": true,
  "data": {
    "total": 5,
    "devices": [
      {
        "client_id": "6e1131b4b9cc454aa6ce3294ab860b2d",
        "client_name": "GPU Server 1",
        "client_status": "online",
        "os_type": "Linux",
        "device_name": "NVIDIA GeForce RTX 4090",
        "tflops": 83,
        "cpu_usage": 45,
        "memory_usage": 60,
        "storage_usage": 30,
        "health": 95,
        "last_online": "2025-07-29T17:55:48.826362Z",
        "created_at": "2025-07-01T10:00:00.000000Z",
        "uptime_days": 28
      }
    ]
  },
  "message": "Operation successful",
  "timestamp": "2025-07-29T17:55:48.826362Z"
}
```

#### Response Fields

| Field | Type | Description |
|-------|------|-------------|
| `total` | number | Total number of devices |
| `devices[].client_id` | string | Client ID |
| `devices[].client_name` | string | Client name |
| `devices[].client_status` | string | Client status |
| `devices[].os_type` | string | Operating system type |
| `devices[].device_name` | string | Device name |
| `devices[].tflops` | number | Total TFLOPS |
| `devices[].cpu_usage` | number | CPU usage percentage (0-100) |
| `devices[].memory_usage` | number | Memory usage percentage (0-100) |
| `devices[].storage_usage` | number | Storage usage percentage (0-100) |
| `devices[].health` | number | Health score (0-100) |
| `devices[].last_online` | string | Last online time |
| `devices[].created_at` | string | Creation time |
| `devices[].uptime_days` | number | Uptime in days |

#### Request Example

```bash
curl "http://localhost:18081/api/user/client_list?user_id=12&status=online"
```

---

### 3. Get Client Status List

**GET** `/api/user/client_status_list`

Get client status list. Functionality is similar to `client_list`, but may have different business logic.

#### Query Parameters

Same as `/api/user/client_list`.

---

### 4. Get Client Details

**GET** `/api/user/client_device_detail`

Get detailed device information for a specific client, including system information and all device information.

#### Query Parameters

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `user_id` | string | Yes | User ID |
| `client_id` | string | Yes | Client ID (16-byte hexadecimal string) |
| `status` | string | No | Status filter |
| `name` | string | No | Name filter |

#### Response Example

```json
{
  "success": true,
  "data": {
    "system_info": {
      "health": 95,
      "cpu_usage": 45,
      "memory_usage": 60,
      "storage_usage": 30,
      "device_memsize": 24576,
      "uptime_days": 28
    },
    "device_info": [
      {
        "device_index": 0,
        "name": "NVIDIA GeForce RTX 4090",
        "temp": 72,
        "usage": 85,
        "mem_usage": 75,
        "power_usage": 350
      }
    ]
  },
  "message": "Operation successful",
  "timestamp": "2025-07-29T17:55:48.826362Z"
}
```

#### Response Fields

**system_info**:
- `health`: Health score (0-100)
- `cpu_usage`: CPU usage percentage (0-100)
- `memory_usage`: Memory usage percentage (0-100)
- `storage_usage`: Storage usage percentage (0-100)
- `device_memsize`: Device memory size (MB)
- `uptime_days`: Uptime in days

**device_info[]**:
- `device_index`: Device index
- `name`: Device name
- `temp`: Temperature (degrees Celsius)
- `usage`: GPU usage percentage (0-100)
- `mem_usage`: GPU memory usage percentage (0-100)
- `power_usage`: Power consumption (watts)

#### Request Example

```bash
curl "http://localhost:18081/api/user/client_device_detail?user_id=12&client_id=6e1131b4b9cc454aa6ce3294ab860b2d"
```

---

### 5. Edit Client Information

**POST** `/api/user/edit_client_info`

Update client information with support for partial field updates.

#### Request Body

```json
{
  "user_id": "string (required, 1-255 characters)",
  "client_id": "string (required, 1-255 characters)",
  "os_type": "string (optional, max 50 characters)",
  "name": "string (optional, max 255 characters)",
  "client_status": "string (optional, max 20 characters)",
  "valid_status": "string (optional, max 10 characters, must be 'valid' or 'invalid')"
}
```

#### Status Values

**client_status** valid values:
- `active`: Active
- `online`: Online
- `offline`: Offline
- `maintenance`: Under maintenance
- `error`: Error

**valid_status** valid values:
- `valid`: Valid
- `invalid`: Invalid

#### Response Example

```json
{
  "success": true,
  "data": null,
  "message": "Operation successful",
  "timestamp": "2025-07-29T17:55:48.826362Z"
}
```

#### Status Codes

- `200 OK`: Update successful
- `400 Bad Request`: Invalid request parameters or incorrect status values

#### Request Example

```bash
curl -X POST http://localhost:18081/api/user/edit_client_info \
  -H "Content-Type: application/json" \
  -d '{
    "user_id": "12",
    "client_id": "6e1131b4b9cc454aa6ce3294ab860b2d",
    "name": "Updated Client Name",
    "client_status": "maintenance"
  }'
```

---

## Client Monitoring APIs

### 6. Get Client Statistics

**GET** `/api/user/client_stat`

Get client statistics for a user, including total count, online count, maintenance count, warning count, and total TFLOPS.

#### Query Parameters

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `user_id` | string | Yes | User ID |

#### Response Example

```json
{
  "success": true,
  "data": {
    "systems_total_number": 10,
    "systems_online_number": 8,
    "systems_maintenance_number": 1,
    "systems_warnings_number": 1,
    "total_tflops": 830,
    "uptime_rate": 95
  },
  "message": "Operation successful",
  "timestamp": "2025-07-29T17:55:48.826362Z"
}
```

#### Response Fields

| Field | Type | Description |
|-------|------|-------------|
| `systems_total_number` | number | Total number of systems |
| `systems_online_number` | number | Number of online systems (active in last 2 minutes) |
| `systems_maintenance_number` | number | Number of systems under maintenance |
| `systems_warnings_number` | number | Number of systems with warnings |
| `total_tflops` | number | Total TFLOPS |
| `uptime_rate` | number | Average uptime rate (0-100, based on yesterday's data) |

#### Request Example

```bash
curl "http://localhost:18081/api/user/client_stat?user_id=12"
```

---

### 7. Get Client Monitoring Information

**GET** `/api/user/client_monitor`

Get historical monitoring data for clients, including daily statistics.

#### Query Parameters

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `user_id` | string | Yes | User ID, 1-32 characters |
| `client_id` | string | No | Client ID to filter for a specific client |

#### Response Example

```json
{
  "success": true,
  "data": [
    {
      "client_id": "6e1131b4b9cc454aa6ce3294ab860b2d",
      "client_name": "GPU Server 1",
      "created_at": "2025-07-01T10:00:00",
      "updated_at": "2025-07-29T17:55:48",
      "date": "2025-07-29",
      "avg_cpu_usage": 45.5,
      "avg_memory_usage": 60.2,
      "avg_disk_usage": 30.1,
      "total_network_in_bytes": 1073741824,
      "total_network_out_bytes": 2147483648,
      "total_heartbeats": 1440,
      "last_heartbeat": "2025-07-29T17:55:48.826362Z",
      "avg_network_in_bytes": 745496.5,
      "avg_network_out_bytes": 1490993.0
    }
  ],
  "message": "Operation successful",
  "timestamp": "2025-07-29T17:55:48.826362Z"
}
```

#### Response Fields

| Field | Type | Description |
|-------|------|-------------|
| `client_id` | string | Client ID (hexadecimal string) |
| `client_name` | string | Client name |
| `created_at` | string | Creation time |
| `updated_at` | string | Update time |
| `date` | string | Statistics date |
| `avg_cpu_usage` | number | Average CPU usage percentage |
| `avg_memory_usage` | number | Average memory usage percentage |
| `avg_disk_usage` | number | Average disk usage percentage |
| `total_network_in_bytes` | number | Total network inbound traffic (bytes) |
| `total_network_out_bytes` | number | Total network outbound traffic (bytes) |
| `total_heartbeats` | number | Total number of heartbeats |
| `last_heartbeat` | string | Last heartbeat time |
| `avg_network_in_bytes` | number | Average network inbound traffic per heartbeat |
| `avg_network_out_bytes` | number | Average network outbound traffic per heartbeat |

#### Request Example

```bash
curl "http://localhost:18081/api/user/client_monitor?user_id=12"
curl "http://localhost:18081/api/user/client_monitor?user_id=12&client_id=6e1131b4b9cc454aa6ce3294ab860b2d"
```

---

### 8. Get Client Health Information

**GET** `/api/user/client_health`

Get client heartbeat health information with support for date range queries.

#### Query Parameters

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `user_id` | string | Yes | User ID, 1-32 characters |
| `client_id` | string | No | Client ID to filter for a specific client |
| `start_date` | string | No | Start date (format: YYYY-MM-DD) |
| `end_date` | string | No | End date (format: YYYY-MM-DD) |

#### Response Example

```json
{
  "success": true,
  "data": [
    {
      "client_id": "6e1131b4b9cc454aa6ce3294ab860b2d",
      "client_name": "GPU Server 1",
      "timestamp": "2025-07-29T17:55:48.826362Z",
      "cpu_usage": 45,
      "mem_usage": 60,
      "disk_usage": 30,
      "network_up": 1073741824,
      "network_down": 2147483648
    }
  ],
  "message": "Operation successful",
  "timestamp": "2025-07-29T17:55:48.826362Z"
}
```

#### Response Fields

| Field | Type | Description |
|-------|------|-------------|
| `client_id` | string | Client ID (hexadecimal string) |
| `client_name` | string | Client name |
| `timestamp` | string | Heartbeat timestamp |
| `cpu_usage` | number | CPU usage percentage |
| `mem_usage` | number | Memory usage percentage |
| `disk_usage` | number | Disk usage percentage |
| `network_up` | number | Network upload traffic (bytes) |
| `network_down` | number | Network download traffic (bytes) |

#### Request Example

```bash
curl "http://localhost:18081/api/user/client_health?user_id=12"
curl "http://localhost:18081/api/user/client_health?user_id=12&client_id=6e1131b4b9cc454aa6ce3294ab860b2d&start_date=2025-07-01&end_date=2025-07-29"
```

---

## Model Management APIs

### 9. Create or Update Model

**POST** `/api/models/insert`

Create or update model information.

#### Request Body

```json
{
  "name": "string (required)",
  "version": "string (required)",
  "version_code": 1,
  "engine_type": 0,
  "is_active": true,
  "min_memory_mb": 1024,
  "min_gpu_memory_gb": 8,
  "download_url": "string (optional)",
  "checksum": "sha256:<64 hex chars> (optional)",
  "expected_size": 123456789
}
```

#### Request Parameters

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `name` | string | Yes | Model name |
| `version` | string | Yes | Model version |
| `version_code` | number | Yes | Version code (integer) |
| `engine_type` | number | Yes | Engine type (integer) |
| `is_active` | boolean | No | Whether the model is active |
| `min_memory_mb` | number | No | Minimum memory requirement (MB) |
| `min_gpu_memory_gb` | number | No | Minimum GPU memory requirement (GB) |
| `download_url` | string | No | HTTPS/model artifact URL. Frontends should not include secrets in query strings. |
| `checksum` | string | No | SHA256 checksum, recommended format `sha256:<64 hex chars>`. MD5 is not a trust mechanism. |
| `expected_size` | number | No | Expected artifact size in bytes. |

#### Response Example

```json
{
  "success": true,
  "data": null,
  "message": "Operation successful",
  "timestamp": "2025-07-29T17:55:48.826362Z"
}
```

#### Status Codes

- `200 OK`: Create/update successful
- `400 Bad Request`: Invalid request parameters (name or version is empty)
- `500 Internal Server Error`: Internal server error

#### Request Example

```bash
curl -X POST http://localhost:18081/api/models/insert \
  -H "Content-Type: application/json" \
  -d '{
    "name": "llama-2-7b",
    "version": "1.0",
    "version_code": 1,
    "engine_type": 0,
    "is_active": true,
    "min_memory_mb": 8192,
    "min_gpu_memory_gb": 16
  }'
```

---

### 10. Get Model List

**GET** `/api/models/get`

Get model list with support for conditional filtering.

#### Query Parameters

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `is_active` | boolean | No | Whether to return only active models |
| `min_gpu_memory_gb` | number | No | Minimum GPU memory requirement (GB) |

#### Response Example

```json
{
  "success": true,
  "data": [
    {
      "id": 1,
      "name": "llama-2-7b",
      "version": "1.0",
      "version_code": 1,
      "is_active": true,
      "min_memory_mb": 8192,
      "min_gpu_memory_gb": 16,
      "created_at": "2025-07-01T10:00:00.000000Z"
    }
  ],
  "message": "Operation successful",
  "timestamp": "2025-07-29T17:55:48.826362Z"
}
```

#### Response Fields

| Field | Type | Description |
|-------|------|-------------|
| `id` | number | Model ID |
| `name` | string | Model name |
| `version` | string | Model version |
| `version_code` | number | Version code |
| `is_active` | boolean | Whether the model is active |
| `min_memory_mb` | number | Minimum memory requirement (MB) |
| `min_gpu_memory_gb` | number | Minimum GPU memory requirement (GB) |
| `download_url` | string | Optional model artifact URL |
| `checksum` | string | Optional SHA256 checksum |
| `expected_size` | number | Optional expected artifact size in bytes |
| `created_at` | string | Creation time |

#### Request Example

```bash
curl "http://localhost:18081/api/models/get"
curl "http://localhost:18081/api/models/get?is_active=true&min_gpu_memory_gb=16"
```

---

## Usage Examples

### Complete Client Management Workflow

```bash
# 1. Create client
curl -X POST http://localhost:18081/api/user/insert_client \
  -H "Content-Type: application/json" \
  -d '{
    "user_id": "12",
    "client_id": "6e1131b4b9cc454aa6ce3294ab860b2d",
    "client_status": "online",
    "os_type": "Linux",
    "name": "GPU Server 1"
  }'

# 2. Get client list
curl "http://localhost:18081/api/user/client_list?user_id=12"

# 3. Get client details
curl "http://localhost:18081/api/user/client_device_detail?user_id=12&client_id=6e1131b4b9cc454aa6ce3294ab860b2d"

# 4. Update client information
curl -X POST http://localhost:18081/api/user/edit_client_info \
  -H "Content-Type: application/json" \
  -d '{
    "user_id": "12",
    "client_id": "6e1131b4b9cc454aa6ce3294ab860b2d",
    "name": "Updated GPU Server 1",
    "client_status": "maintenance"
  }'

# 5. Get client statistics
curl "http://localhost:18081/api/user/client_stat?user_id=12"

# 6. Get monitoring data
curl "http://localhost:18081/api/user/client_monitor?user_id=12"

# 7. Get health information
curl "http://localhost:18081/api/user/client_health?user_id=12"
```

### Format Output with jq

```bash
# Format client list output
curl -s "http://localhost:18081/api/user/client_list?user_id=12" | jq '.'

# Show only client name and status
curl -s "http://localhost:18081/api/user/client_list?user_id=12" | jq '.data.devices[] | {name: .client_name, status: .client_status}'

# Get online client count
curl -s "http://localhost:18081/api/user/client_stat?user_id=12" | jq '.data.systems_online_number'
```

---

## Error Handling

### Common Error Codes

- `400 Bad Request`: Invalid request parameters
  - Missing required parameters
  - Invalid parameter format
  - Parameter values outside allowed range

- `500 Internal Server Error`: Internal server error
  - Database connection failure
  - Query execution failure
  - Other internal errors

### Error Response Example

```json
{
  "success": false,
  "data": null,
  "message": "Failed to get user clients: connection timeout",
  "timestamp": "2025-07-29T17:55:48.826362Z"
}
```

---

## Notes

1. **Client ID Format**: All `client_id` values must be 16-byte hexadecimal strings (32 characters)
2. **Time Format**: All time fields use ISO 8601 format (UTC timezone)
3. **Character Limits**: Note the length limits for each field, exceeding limits will cause request failures
4. **Status Values**: When using `client_status` and `valid_status`, you must use predefined valid values
5. **Pagination**: The current API does not support pagination and will return all matching results
6. **Caching**: Some data may use Redis caching, updates may require waiting for cache expiration

---

## Development Notes

### Starting the API Server

```bash
cargo run --release -p gpuf-s --bin api_server -- \
  --bind-addr 127.0.0.1 \
  --port 18081 \
  --database-url "$DATABASE_URL" \
  --redis-url "redis://127.0.0.1:6379"
```

The API server listens on `127.0.0.1:18081` by default. Change the port with `--port`; use `--bind-addr 0.0.0.0` only when the deployment is protected by a reverse proxy, firewall, and access-control policy.

### Database Connection

The API server requires connections to PostgreSQL database and Redis cache. Ensure database connections are working properly.

### Monitor Redis Cache

```bash
# Monitor Redis cache hits
redis-cli monitor
```

---

## Changelog

- 2026-06-05: Documented that mobile native TLS worker APIs are additive and do not change frontend REST contracts.
- 2026-06-04: Documented security remediation defaults, frontend integration contract, loopback API bind default, additive model checksum fields, and SDK compatibility impact.
- Initial version: Support for client management, monitoring, and model management APIs
