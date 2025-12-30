use crate::util::protoc::ClientId;
use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use redis::{AsyncCommands, Client as RedisClient, Commands};
use sqlx::{postgres::Postgres, FromRow, Pool};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{debug, info, warn};

#[derive(FromRow)]
struct ClientRecord {
    client_id: [u8; 16],
}

#[derive(FromRow)]
struct TokenInfo {
    user_id: String,
    access_level: i32,
}

pub async fn get_user_client_by_token(
    pool: &Pool<Postgres>,
    token: &str,
) -> Result<(Vec<ClientId>, i32)> {
    // First, get the token details including user_id and access_level
    let token_info = match sqlx::query_as::<_, TokenInfo>(
        r#"
        SELECT user_id::text as user_id, access_level 
        FROM tokens 
        WHERE key = $1::varchar(48)
          AND status = 1
          AND (expired_time = -1 OR expired_time > EXTRACT(EPOCH FROM NOW())::bigint)
          AND deleted_at IS NULL
        "#,
    )
    .bind(token)
    .fetch_optional(pool)
    .await?
    {
        Some(info) => info,
        None => return Err(anyhow!("Invalid or expired token")),
    };

    // Then query devices based on access level
    let query = if token_info.access_level == -1 {
        // Access to all devices
        "SELECT client_id FROM gpu_assets 
         WHERE client_status = 'online' AND valid_status = 'valid'"
    } else {
        // Access only to user's devices
        "SELECT client_id FROM gpu_assets 
         WHERE user_id = $1 AND client_status = 'online' AND valid_status = 'valid'"
    };

    let mut query = sqlx::query_as::<_, ClientRecord>(query);

    // Only bind user_id parameter if access_level is not -1
    if token_info.access_level != -1 {
        query = query.bind(&token_info.user_id);
    }

    let rows = query.fetch_all(pool).await?;

    // Convert each row's client_id (BYTEA) to ClientId
    let client_ids = rows
        .into_iter()
        .map(|row| {
            let client_id = row.client_id;

            client_id.try_into().map(ClientId).map_err(|_| {
                anyhow!(
                    "invalid client_id length: expected 16 bytes, actual {}",
                    client_id.len()
                )
            })
        })
        .collect::<Result<Vec<ClientId>>>()?;

    if client_ids.is_empty() {
        return Ok((vec![], token_info.access_level));
    }

    Ok((client_ids, token_info.access_level))
}

pub async fn update_client_db(
    pool: &Pool<Postgres>,
    client_id: &ClientId,
    os_type: &str,
) -> Result<()> {
    debug!(
        "update model for client {:?}: os_type: {}",
        client_id, os_type
    );

    let _row  = sqlx::query(
                            "UPDATE \"public\".\"gpu_assets\" SET os_type = $1  WHERE client_id = $2 AND valid_status = 'valid'"
                        )
                        .bind(os_type)
                        .bind(client_id)
                        .execute(pool)
                        .await.map_err(|e| anyhow!("Database query failed: {}", e))?;
    Ok(())
}

#[derive(FromRow)]
struct ClientStatusRow {
    client_id: [u8; 16],
    client_name: String,
    os_type: Option<String>,
    client_status: String,
    created_at: DateTime<Utc>,
    last_online: DateTime<Utc>,
    device_name: Option<String>,
    cpu_usage: Option<i16>,
    memory_usage: Option<i16>,
    storage_usage: Option<i16>,
    total_tflops: Option<i32>,
    health_rate: f64,
    uptime_days: Option<i32>,
}

#[derive(serde::Serialize)]
pub struct ClientDeviceInfo {
    pub client_id: String,
    pub client_name: String,
    pub client_status: String,
    pub os_type: String,
    pub device_name: String,
    pub tflops: u16,
    pub cpu_usage: u8,
    pub memory_usage: u8,
    pub storage_usage: u8,
    pub health: u8,
    pub last_online: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub uptime_days: u32,
    //pub os_type: Option<String>,
}
#[allow(dead_code)]
pub async fn get_user_client_status_list(
    pool: &Pool<Postgres>,
    user_id: &str,
    client_id: Option<&String>,
    status: Option<&String>,
    name: Option<&String>,
    valid_status: Option<&String>,
) -> Result<Vec<ClientDeviceInfo>> {
    // Build the base query
    let mut query = r#"
    SELECT 
        ga.client_id as client_id,
        ga.client_name as client_name,
        ga.os_type as os_type,
        ga.client_status as client_status,
        ga.created_at::TIMESTAMPTZ as created_at,
        ga.updated_at::TIMESTAMPTZ as last_online,
        di.device_name as device_name,
        si.cpu_usage as cpu_usage,
        si.mem_usage as memory_usage,
        si.disk_usage as storage_usage,
        si.total_tflops as total_tflops,
        COALESCE((
            SELECT (ROUND((AVG(LEAST(1.0, total_heartbeats::FLOAT / 720) * 100.0))::numeric, 2))::FLOAT8
            FROM client_daily_stats 
            WHERE client_id = ga.client_id
            GROUP BY client_id
        ), 0) as health_rate,
        (
            SELECT COUNT(*)::INTEGER
            FROM client_daily_stats
            WHERE client_id = ga.client_id
        ) as uptime_days
    FROM gpu_assets ga
    LEFT JOIN device_info di ON ga.client_id = di.client_id AND di.device_index = 0
    LEFT JOIN (
        SELECT 
            client_id,
            cpu_usage,
            mem_usage,
            disk_usage,
            total_tflops,
            ROW_NUMBER() OVER (PARTITION BY client_id ORDER BY created_at DESC) as rn
        FROM system_info
    ) si ON ga.client_id = si.client_id AND si.rn = 1
    WHERE ga.user_id = $1 AND ga.valid_status = 'valid'
    "#
    .to_string();

    // 2. Collect all parameters in a vector
    let mut param_count = 1;
    let mut query_builder = sqlx::query_as::<_, ClientStatusRow>(&query).bind(user_id);

    // 3. Add optional conditions
    if let Some(client_id) = client_id {
        param_count += 1;
        query.push_str(&format!(" AND ga.client_id = ${}", param_count));
        // Create a new query builder with the updated query
        query_builder = sqlx::query_as::<_, ClientStatusRow>(&query)
            .bind(user_id)
            .bind(client_id.parse::<ClientId>()?);
    }
    if let Some(status) = status {
        param_count += 1;
        query.push_str(&format!(" AND ga.client_status = ${}", param_count));
        query_builder = sqlx::query_as::<_, ClientStatusRow>(&query).bind(user_id);
        if let Some(cid) = client_id {
            query_builder = query_builder.bind(cid.parse::<ClientId>()?);
        }
        query_builder = query_builder.bind(status);
    }
    if let Some(valid_status) = valid_status {
        param_count += 1;
        query.push_str(&format!(" AND ga.valid_status = ${}", param_count));

        query_builder = sqlx::query_as::<_, ClientStatusRow>(&query).bind(user_id);
        if let Some(cid) = client_id {
            query_builder = query_builder.bind(cid.parse::<ClientId>()?);
        }
        if let Some(s) = status {
            query_builder = query_builder.bind(s);
        }
        query_builder = query_builder.bind(valid_status);
    }
    if let Some(name) = name {
        param_count += 1;
        query.push_str(&format!(" AND ga.client_name ILIKE ${}", param_count));
        let mut temp_builder = sqlx::query_as::<_, ClientStatusRow>(&query).bind(user_id);

        if let Some(cid) = client_id {
            temp_builder = temp_builder.bind(cid.parse::<ClientId>()?);
        }
        if let Some(s) = status {
            temp_builder = temp_builder.bind(s);
        }
        if let Some(vs) = valid_status {
            temp_builder = temp_builder.bind(vs);
        }
        query_builder = temp_builder.bind(format!("%{}%", name));
    }
    // 4. Execute the query
    let mut conn = pool
        .acquire()
        .await
        .map_err(|_| anyhow!("Failed to acquire database connection"))?;

    // get online info in DB
    let devices = query_builder
        .fetch_all(&mut *conn)
        .await
        .map_err(|e| anyhow!("Failed to fetch user client list: {}", e))?;

    let devices: Vec<ClientDeviceInfo> = devices
        .into_iter()
        .map(|row: ClientStatusRow| {
            let client_id = ClientId(row.client_id);
            ClientDeviceInfo {
                client_id: client_id.to_string(),
                client_name: row.client_name,
                os_type: row.os_type.unwrap_or("".to_string()),
                client_status: row.client_status,
                health: row.health_rate as u8,
                cpu_usage: row.cpu_usage.unwrap_or(0) as u8,
                memory_usage: row.memory_usage.unwrap_or(0) as u8,
                storage_usage: row.storage_usage.unwrap_or(0) as u8,
                device_name: row.device_name.unwrap_or("".to_string()),
                tflops: row.total_tflops.unwrap_or(0) as u16,
                last_online: row.last_online,
                created_at: row.created_at,
                uptime_days: row.uptime_days.unwrap_or(0) as u32,
            }
        })
        .collect();

    Ok(devices)
}

#[derive(serde::Serialize)]
pub struct SystemInfoDetailResponse {
    pub health: u8,
    pub cpu_usage: u8,
    pub memory_usage: u8,
    pub storage_usage: u8,
    pub device_memsize: u32,
    pub uptime_days: u16,
}

#[derive(serde::Serialize)]
#[allow(dead_code)]
pub struct ClientDeviceDetailResponse {
    pub system_info: SystemInfoDetailResponse,
    pub device_info: Vec<DeviceInfoResponse>,
}

#[derive(serde::Serialize)]
pub struct DeviceInfoResponse {
    pub device_index: u8,
    pub name: String,
    pub temp: u8,
    pub usage: u8,
    pub mem_usage: u8,
    pub power_usage: u8,
}

#[allow(dead_code)] // API endpoints and database utility functions
#[derive(sqlx::FromRow)]
pub struct SystemInfoDetail {
    pub cpu_usage: i16,
    pub mem_usage: i16,
    pub disk_usage: i16,
    pub device_memsize: i64,
    pub total_tflops: i32,
}

#[derive(sqlx::FromRow)]
#[allow(dead_code)] // Fields are used by sqlx::FromRow for database mapping
struct ClientInfo {
    client_id: [u8; 16],
    client_name: String,
    os_type: Option<String>,
    client_status: String,
    created_at: chrono::DateTime<chrono::Utc>,
    last_online: chrono::DateTime<chrono::Utc>,
}

#[allow(dead_code)] // API endpoints and database utility functions
#[derive(sqlx::FromRow)]
pub struct GpuDeviceInfo {
    pub device_index: i16,
    pub device_name: String,
    pub device_gpuusage: i16,
    pub device_memusage: i16,
    pub device_temp: i16,
    pub device_powerusage: i16,
}

#[allow(dead_code)] // API endpoint for client device details
pub async fn get_client_device_detail(
    pool: &Pool<Postgres>,
    user_id: &str,

    client_id: &ClientId,
) -> Result<ClientDeviceDetailResponse> {
    // Get client basic info
    let client: ClientInfo = sqlx::query_as(
        r#"
        SELECT 
            client_id,
            client_name,
            os_type,
            client_status,
            created_at AT TIME ZONE 'UTC' as created_at,
            updated_at AT TIME ZONE 'UTC' as last_online
        FROM gpu_assets
        WHERE user_id = $1 AND client_id = $2 and valid_status = 'valid'
        "#,
    )
    .bind(user_id)
    .bind(client_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| anyhow!("Device not found"))?;

    //TODO: get device info from db
    // Calculate health rate from device_daily_stats
    let health_rate: f64 = sqlx::query_scalar(
        r#"
        SELECT COALESCE((
            SELECT (ROUND((AVG(LEAST(1.0, total_heartbeats::FLOAT / 720) * 100.0))::numeric, 2))::FLOAT8
            FROM client_daily_stats 
            WHERE client_id = $1
            GROUP BY client_id
        ), 0.0)
        "#,
    )
    .bind(client_id)
    .fetch_one(pool)
    .await?;

    // If device is online, use the calculated health rate, otherwise use a fraction of it
    let health = {
        let offline_duration = Utc::now().signed_duration_since(client.last_online);
        let offline_factor = if offline_duration.num_hours() < 1 {
            0.9 // 90% of health_rate
        } else if offline_duration.num_hours() < 24 {
            0.7 // 70% of health_rate
        } else {
            0.5 // 50% of health_rate
        };
        (health_rate * offline_factor) as u8
    };

    let uptime_days: i64 = sqlx::query_scalar(
        r#"
        SELECT 
            COUNT(id)::BIGINT
        FROM device_daily_stats
        WHERE client_id = $1
        "#,
    )
    .bind(client_id)
    .fetch_one(pool)
    .await?;

    let system_info = sqlx::query_as::<_, SystemInfoDetail>(
        r#"
            SELECT 
                cpu_usage,
                mem_usage,
                device_memsize,
                disk_usage,
                total_tflops
            FROM system_info
            WHERE client_id = $1
            ORDER BY created_at DESC
            LIMIT 1
            "#,
    )
    .bind(client_id)
    .fetch_optional(pool)
    .await?
    .unwrap_or_else(|| SystemInfoDetail {
        cpu_usage: 0,
        mem_usage: 0,
        disk_usage: 0,
        device_memsize: 0,
        total_tflops: 0,
    });

    let system_info = SystemInfoDetailResponse {
        health,
        device_memsize: system_info.device_memsize as u32,
        cpu_usage: system_info.cpu_usage as u8,
        memory_usage: system_info.mem_usage as u8,
        storage_usage: system_info.disk_usage as u8,
        uptime_days: uptime_days.try_into().unwrap_or(0),
    };

    let device_info = sqlx::query_as::<_, GpuDeviceInfo>(
        r#"
            SELECT 
                device_index,
                device_name,
                device_memusage,
                device_gpuusage,
                device_powerusage,
                device_temp 
            FROM device_info
            WHERE client_id = $1
            ORDER BY device_index
            "#,
    )
    .bind(client_id)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|d| DeviceInfoResponse {
        device_index: d.device_index as u8,
        name: d.device_name,
        usage: d.device_gpuusage as u8,
        mem_usage: d.device_memusage as u8,
        temp: d.device_temp as u8,
        power_usage: d.device_powerusage as u8,
    })
    .collect::<Vec<_>>();

    let client_info = ClientDeviceDetailResponse {
        system_info,
        device_info,
    };

    Ok(client_info)
}

//@# upsert client info
#[allow(dead_code)]
pub async fn upsert_client_info(
    pool: &Pool<Postgres>,
    user_id: &str,
    client_id: &ClientId,
    _os_type: &Option<String>,
    client_status: &str,
    name: &str,
) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO "public"."gpu_assets" ("user_id", "client_id", "client_status", "client_name",  "created_at", "updated_at")
        VALUES ($1, $2, $3, $4, NOW(), NOW())
        ON CONFLICT ("client_id")
        DO UPDATE SET
            "client_name" = EXCLUDED."client_name",
            "client_status" = EXCLUDED."client_status",
            "updated_at" = NOW();
        "#,
    )
    .bind(user_id)
    .bind(client_id.0.to_vec())
    .bind(client_status)
    .bind(name)
    .execute(pool)
    .await?;

    Ok(())
}

//@# upsert client info
pub async fn upsert_client_status(
    pool: &Pool<Postgres>,
    client_id: &ClientId,
    status: &str,
) -> Result<()> {
    sqlx::query("UPDATE \"public\".\"gpu_assets\" SET client_status = $1, \"updated_at\" = NOW() WHERE \"client_id\" = $2")
    .bind(status)
    .bind(client_id)
    .execute(pool)
    .await?;
    Ok(())
}

// redis
#[allow(dead_code)] // Redis utility function for heartbeat info
pub async fn upsert_heartbeat_info_in_redis<F, Fut>(
    redis_client: &RedisClient,
    client_id: &ClientId,
    client_timestamp: u64,
    func: F,
) where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: std::future::Future<Output = ()> + Send,
{
    let (server_timestamp, network_delay_ms) = calculate_network_delay(client_timestamp);
    if let Ok(mut conn) = redis_client.get_async_connection().await {
        let ts = chrono::Utc::now().timestamp();
        let key = format!("client:{}:status", client_id);
        let hset_multiple = vec![
            ("status", "online".to_string()),
            ("ts", ts.to_string()),
            ("client_ts", client_timestamp.to_string()),
            ("server_ts", server_timestamp.to_string()),
            ("network_delay_ms", network_delay_ms.to_string()),
        ];
        let _: Result<(), _> = conn.hset_multiple(&key, &hset_multiple).await;
        let _: Result<(), _> = conn.expire(&key, 24 * 60 * 60).await; // TTL 24h

        let gate_key = format!("hb:gate:{}", client_id);
        let allow: bool = conn.set_nx(&gate_key, 1).await.unwrap_or(false);
        if allow {
            let _: Result<(), _> = conn.expire(&gate_key, 300).await; // 5 min
            func().await;
        }
    }
}

#[allow(dead_code)] // Network delay calculation utility
fn calculate_network_delay(client_timestamp: u64) -> (u64, u64) {
    let server_timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_millis() as u64;

    if client_timestamp > server_timestamp {
        debug!(
            "Client time is ahead of server time by {}ms",
            client_timestamp - server_timestamp
        );
        return (server_timestamp, 0);
    }

    return (server_timestamp, server_timestamp - client_timestamp);
}

// TODO: hot_models is Arc
pub async fn validate_client(
    pool: &Pool<Postgres>,
    redis_client: &RedisClient,
    os_type: &str,
    client_id: &ClientId,
) -> Result<bool> {
    let cache_key = format!("client_status:{}", client_id);

    let mut redis_conn = redis_client
        .get_connection()
        .expect("Failed to get Redis connection");

    // Check if token is cached
    let cached_result: Option<String> = redis_conn.get(&cache_key).unwrap_or(None);
    if let Some(model) = cached_result {
        if model == "invalid" {
            warn!("Client ID  {}  found  invalid in cache.", client_id);
            return Ok(false);
        } else {
            debug!("Client ID  {} found  model {} in cache.", client_id, model);
            return Ok(true);
        }
    }

    match update_client_db(pool, &client_id, os_type).await {
        Ok(_) => {
            info!("update client db success");
        }
        Err(e) => {
            warn!("Failed to update client db: {}", e);
        }
    };

    let row: Option<sqlx::postgres::PgRow>  = sqlx::query(
            "SELECT client_id, model, model_version FROM \"public\".\"gpu_assets\" WHERE client_id = $1  AND valid_status = 'valid' "
        )
        .bind(client_id)
        .fetch_optional(pool)
        .await.map_err(|e| anyhow!("Database query failed: {}", e))?;

    let is_valid = match row {
        Some(_row) => true,
        None => false,
    };

    let valid = if is_valid { "valid" } else { "invalid" };

    if let Err(e) = redis_conn.set_ex::<_, _, ()>(&cache_key, valid, 300) {
        warn!(
            "first pull model is empty Failed to cache result for client {:?}: {}",
            client_id, e
        );
    }

    Ok(is_valid)
}
