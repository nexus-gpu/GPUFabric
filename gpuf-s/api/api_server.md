# gpuf-s API Server

## Base
- **Base URL**: `http://127.0.0.1:18081` by default; use `http://<host>:18081` only for a protected deployment
- **Content-Type**: `application/json`

## Frontend Integration And Security Defaults

The standalone management API now binds to `127.0.0.1` by default. Start it with `--bind-addr 127.0.0.1` for local frontend development; choose `--bind-addr 0.0.0.0` only behind a reverse proxy/firewall and with deployment-level access control.

Existing REST paths and response envelopes remain compatible for frontends. The model APIs add optional `download_url`, `checksum`, and `expected_size` fields so UIs can show SHA256-verified artifact metadata without breaking older clients.
Control TLS is separate from this REST API. If the same deployment accepts remote gpuf-c workers over non-loopback networks, enable `gpuf-s --control-tls` and configure clients with `gpuf-c --control-tls --control-tls-server-name <name> --cert-chain-path <ca.pem>`. Mobile native workers can use the additive `startRemoteWorkerWithTls` SDK entry point; this does not change frontend REST paths or response envelopes.

## Common Response Envelope
All endpoints return this envelope type:

```json
{
  "success": true,
  "data": {},
  "message": "success",
  "timestamp": "2026-02-03T06:35:28.784161Z"
}
```

- **success**: `bool`
- **data**: `T | null`
- **message**: `string`
- **timestamp**: `RFC3339 string`

---

# Client / User APIs

## POST `/api/user/insert_client`
Create or update a client record for a user.

### Request Body (JSON)
| Field | Type | Optional | Notes |
|---|---:|:---:|---|
| user_id | string | No | length 1..32 |
| client_id | string | No | parsed as `ClientId` (expected 16 bytes) |
| client_status | string | No | e.g. `online/offline/...` |
| os_type | string | Yes | length 1..64 |
| name | string | No | length 1..32 |

### Response `ApiResponse<Vec<ClientInfoResponse>>`
Current implementation returns an empty list (`[]`).

`ClientInfoResponse`:
| Field | Type | Notes |
|---|---|---|
| client_id | string | client id string |
| authed | bool | |
| connected_at | RFC3339 string | |
| system_info | object\|null | currently internal fields |

### Example
```bash
curl -X POST "http://<host>:18081/api/user/insert_client" \
  -H "Content-Type: application/json" \
  -d '{
    "user_id": "1",
    "client_id": "3c04a52d9e424dcc83c06573227a7bf6",
    "client_status": "online",
    "os_type": "linux",
    "name": "node-1"
  }'
```

---

## GET `/api/user/client_list`
Get a user’s client list.

### Query Parameters
| Param | Type | Optional | Notes |
|---|---:|:---:|---|
| user_id | string | No | |
| client_id | string | Yes | if provided, parsed as `ClientId` |
| status | string | Yes | client status filter |
| name | string | Yes | matched by `ILIKE` |
| valid_status | string | Yes | e.g. `valid/invalid/warning` |

### Response `ApiResponse<ClientListResponse>`
`ClientListResponse`:
| Field | Type |
|---|---|
| total | number |
| devices | ClientDeviceInfo[] |

`ClientDeviceInfo`:
| Field | Type |
|---|---|
| client_id | string |
| client_name | string |
| client_status | string |
| os_type | string |
| device_name | string |
| tflops | number |
| cpu_usage | number |
| memory_usage | number |
| storage_usage | number |
| health | number |
| last_online | RFC3339 string |
| created_at | RFC3339 string |
| uptime_days | number |
| loaded_models | object[] |

### Example
```bash
curl "http://<host>:18081/api/user/client_list?user_id=1"
```

---

## GET `/api/user/client_device_detail`
Get one client’s system and device detail.

### Query Parameters
| Param | Type | Optional | Notes |
|---|---:|:---:|---|
| user_id | string | No | |
| client_id | string | No | parsed as `ClientId` |
| status | string | Yes | currently unused |
| name | string | Yes | currently unused |

### Response `ApiResponse<ClientDeviceDetailResponse>`
`ClientDeviceDetailResponse`:
| Field | Type |
|---|---|
| system_info | SystemInfoDetailResponse |
| device_info | DeviceInfoResponse[] |

`SystemInfoDetailResponse`:
| Field | Type |
|---|---|
| health | number |
| cpu_usage | number |
| memory_usage | number |
| storage_usage | number |
| device_memsize | number |
| uptime_days | number |

`DeviceInfoResponse`:
| Field | Type |
|---|---|
| device_index | number |
| name | string |
| temp | number |
| usage | number |
| mem_usage | number |
| power_usage | number |

### Example
```bash
curl "http://<host>:18081/api/user/client_device_detail?user_id=1&client_id=3c04a52d9e424dcc83c06573227a7bf6"
```

---

## POST `/api/user/edit_client_info`
Edit client info fields.

### Request Body (JSON)
`EditClientRequest`:
| Field | Type | Optional |
|---|---:|:---:|
| user_id | string | No |
| client_id | string | No |
| os_type | string | Yes |
| name | string | Yes |
| client_status | string | Yes |
| valid_status | string | Yes |

### Response `ApiResponse<()>`

### Example
```bash
curl -X POST "http://<host>:18081/api/user/edit_client_info" \
  -H "Content-Type: application/json" \
  -d '{
    "user_id": "1",
    "client_id": "3c04a52d9e424dcc83c06573227a7bf6",
    "client_status": "online"
  }'
```

---

## GET `/api/user/client_status_list`
Alias of client list with status.

### Query Parameters
Same as `/api/user/client_list`.

### Response
Same as `/api/user/client_list`.

---

## GET `/api/user/client_stat`
Get overall user client statistics.

### Query Parameters
| Param | Type | Optional |
|---|---:|:---:|
| user_id | string | No |

### Response `ApiResponse<ClientStatResponse>`
| Field | Type |
|---|---|
| systems_total_number | number |
| systems_online_number | number |
| systems_maintenance_number | number |
| systems_warnings_number | number |
| total_tflops | number |
| uptime_rate | number |

---

## GET `/api/user/client_monitor`
Get monitoring summary for user’s clients.

### Query Parameters
| Param | Type | Optional | Notes |
|---|---:|:---:|---|
| user_id | string | No | length 1..32 |
| client_id | string | Yes | **hex string**; server uses `hex::decode()` to bind to `BYTEA` |

### Response `ApiResponse<Vec<ClientMonitorInfo>>`
`ClientMonitorInfo`:
| Field | Type |
|---|---|
| client_id | string(hex) |
| client_name | string\|null |
| created_at | string\|null |
| updated_at | string\|null |
| date | string\|null |
| avg_cpu_usage | number\|null |
| avg_memory_usage | number\|null |
| avg_disk_usage | number\|null |
| total_network_in_bytes | number\|null |
| total_network_out_bytes | number\|null |
| total_heartbeats | number\|null |
| last_heartbeat | RFC3339 string\|null |
| avg_network_in_bytes | number\|null |
| avg_network_out_bytes | number\|null |

---

## GET `/api/user/client_health`
Get heartbeat records (time series).

### Query Parameters
| Param | Type | Optional | Notes |
|---|---:|:---:|---|
| user_id | string | No | |
| client_id | string | Yes | **hex string**; server decodes to `BYTEA` |
| start_date | string | Yes | passed through to SQL, recommended `YYYY-MM-DD` |
| end_date | string | Yes | passed through to SQL, recommended `YYYY-MM-DD` |

### Response `ApiResponse<Vec<ClientHeartbeatInfo>>`
`ClientHeartbeatInfo`:
| Field | Type |
|---|---|
| client_id | string(hex) |
| client_name | string\|null |
| timestamp | RFC3339 string |
| cpu_usage | number\|null |
| mem_usage | number\|null |
| disk_usage | number\|null |
| network_up | number |
| network_down | number |

---

## GET `/api/user/model_download_progress`
Get model download progress from Redis.

### Query Parameters
| Param | Type | Optional |
|---|---:|:---:|
| client_id | string | No |

### Response `ApiResponse<ModelDownloadProgressResponse>`
| Field | Type |
|---|---|
| client_id | string |
| model_name | string\|null |
| downloaded_bytes | number\|null |
| total_bytes | number\|null |
| percentage | number\|null |
| speed_bps | number\|null |
| status | string\|null |
| error | string\|null |
| timestamp | number\|null |

---

# Points APIs

## GET `/api/user/points`
Query a user’s points list (based on materialized view `device_points_daily`).

### Query Parameters
| Param | Type | Optional | Notes |
|---|---:|:---:|---|
| user_id | string | No | joins via `gpu_assets.user_id` |
| client_id | string | Yes | client id **hex string (32 chars)**; filters by exact client |
| client_name | string | Yes | fuzzy match by `gpu_assets.client_name` using `ILIKE '%...%'` |
| device_id | number | Yes | `INT` device id |
| start_date | string | Yes | `YYYY-MM-DD` |
| end_date | string | Yes | `YYYY-MM-DD` |
| page | number | Yes | 1..100, default 1 |
| page_size | number | Yes | 1..100, default 20 |

### Response `ApiResponse<PointsListResponse>`
`PointsListResponse`:
| Field | Type |
|---|---|
| points | DevicePointsResponse[] |
| total_points | number |
| total_count | number |
| page | number |
| page_size | number |

`DevicePointsResponse`:
| Field | Type |
|---|---|
| client_id | string | hex string (`encode(bytea,'hex')`) |
| client_name | string | from `gpu_assets.client_name` |
| date | string | `YYYY-MM-DD` |
| total_heartbeats | number |
| device_name | string |
| device_id | number |
| points | number |

### Example
```bash
curl "http://<host>:18081/api/user/points?user_id=1&page=1&page_size=20"
curl "http://<host>:18081/api/user/points?user_id=1&client_id=50ef7b5e7b5b4c79991087bb9f62cef1"
curl "http://<host>:18081/api/user/points?user_id=1&client_name=node"
curl "http://<host>:18081/api/user/points?user_id=1&device_id=9860&start_date=2026-02-01&end_date=2026-02-03"
```

---

# Model APIs

## POST `/api/models/insert`
Create or update a model.

### Request Body (JSON)
| Field | Type | Optional |
|---|---:|:---:|
| name | string | No |
| version | string | No |
| version_code | number | No |
| engine_type | number | No |
| is_active | bool | Yes |
| min_memory_mb | number | Yes |
| min_gpu_memory_gb | number | Yes |
| download_url | string | Yes |
| checksum | string | Yes |
| expected_size | number | Yes |

### Response `ApiResponse<()>`

---

## GET `/api/models/get`
Get models list.

### Query Parameters
| Param | Type | Optional |
|---|---:|:---:|
| is_active | bool | Yes |
| min_gpu_memory_gb | number | Yes |

### Response `ApiResponse<Vec<ModelResponse>>`
`ModelResponse`:
| Field | Type |
|---|---|
| id | number |
| name | string |
| version | string |
| version_code | number |
| is_active | bool |
| min_memory_mb | number\|null |
| min_gpu_memory_gb | number\|null |
| created_at | RFC3339 string |
| download_url | string\|null |
| checksum | string\|null |
| expected_size | number\|null |

---

# APK APIs

## POST `/api/apk/upsert`
Upsert an APK version.

### Request Body (JSON)
| Field | Type | Optional |
|---|---:|:---:|
| package_name | string | No |
| version_name | string | No |
| version_code | number | No |
| download_url | string | No |
| channel | string | Yes |
| min_os_version | string | Yes |
| sha256 | string | Yes |
| file_size_bytes | number | Yes |
| is_active | bool | Yes |
| released_at | RFC3339 string | Yes |

### Response `ApiResponse<ApkResponse>`
`ApkResponse`:
| Field | Type |
|---|---|
| id | number |
| package_name | string |
| version_name | string |
| version_code | number |
| download_url | string |
| channel | string\|null |
| min_os_version | string\|null |
| sha256 | string\|null |
| file_size_bytes | number\|null |
| is_active | bool |
| released_at | RFC3339 string\|null |
| created_at | RFC3339 string |
| updated_at | RFC3339 string |

---

## GET `/api/apk/get`
Get one APK version.

### Query Parameters
| Param | Type | Optional |
|---|---:|:---:|
| package_name | string | No |
| version_code | number | No |

### Response `ApiResponse<ApkResponse|null>`

---

## GET `/api/apk/list`
List APK versions.

### Query Parameters
| Param | Type | Optional | Default |
|---|---:|:---:|---|
| package_name | string | Yes | |
| channel | string | Yes | |
| is_active | bool | Yes | |
| limit | number | Yes | 50 (max 200) |

### Response `ApiResponse<Vec<ApkResponse>>`
