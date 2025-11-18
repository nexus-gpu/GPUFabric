use crate::db::{
    CLIENT_DAILY_STATS_TABLE, DEVICE_DAILY_STATS_TABLE, DEVICE_INFO_TABLE, GPU_ASSETS_TABLE,
    HEARTBEAT_TABLE, SYSTEM_INFO_TABLE,
};
use crate::util::protoc::ClientId;
use anyhow::Result;
use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use common::{get_u16_from_u128, get_u8_from_u64, DevicesInfo, SystemInfo};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool, Pool, Postgres, QueryBuilder, Transaction};
use tracing::{debug, info};
use validator::Validate;

#[derive(Debug, FromRow, Serialize, Deserialize)]
pub struct ClientDailyStats {
    pub id: i64,
    pub date: NaiveDate,
    pub client_id: Vec<u8>,
    pub total_heartbeats: i32,
    pub avg_cpu_usage: Option<f64>,
    pub avg_memory_usage: Option<f64>,
    pub avg_disk_usage: Option<f64>,
    pub total_network_in_bytes: Option<i64>,
    pub total_network_out_bytes: Option<i64>,
    pub last_heartbeat: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, FromRow, Serialize, Deserialize)]
pub struct DeviceDailyStats {
    pub id: i64,
    pub date: NaiveDate,
    pub client_id: [u8; 16],
    pub device_index: i16,
    pub device_name: Option<String>,
    pub total_heartbeats: i32,
    pub avg_utilization: Option<f64>,
    pub avg_temperature: Option<f64>,
    pub avg_power_usage: Option<f64>,
    pub avg_memory_usage: Option<f64>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl ClientDailyStats {

    pub async fn upsert(
        tx: &mut Transaction<'_, Postgres>,
        client_id: &ClientId,
        cpu_usage: Option<f64>,
        memory_usage: Option<f64>,
        disk_usage: Option<f64>,
        network_in: Option<i64>,
        network_out: Option<i64>,
    ) -> Result<Self, sqlx::Error> {
        let today = Utc::now().date_naive();

        let record = sqlx::query_as(
            r#"
            INSERT INTO client_daily_stats (
                date, client_id, 
                avg_cpu_usage, avg_memory_usage, avg_disk_usage,
                total_network_in_bytes, total_network_out_bytes,
                total_heartbeats, last_heartbeat
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, 1, NOW())
            ON CONFLICT (client_id, date) 
            DO UPDATE SET
                avg_cpu_usage = (COALESCE(client_daily_stats.avg_cpu_usage, 0) * client_daily_stats.total_heartbeats + $3) / 
                               (client_daily_stats.total_heartbeats + 1),
                avg_memory_usage = (COALESCE(client_daily_stats.avg_memory_usage, 0) * client_daily_stats.total_heartbeats + $4) / 
                                 (client_daily_stats.total_heartbeats + 1),
                avg_disk_usage = (COALESCE(client_daily_stats.avg_disk_usage, 0) * client_daily_stats.total_heartbeats + $5) / 
                               (client_daily_stats.total_heartbeats + 1),
                total_network_in_bytes = COALESCE($6, 0) + COALESCE(client_daily_stats.total_network_in_bytes, 0),
                total_network_out_bytes = COALESCE($7, 0) + COALESCE(client_daily_stats.total_network_out_bytes, 0),
                total_heartbeats = client_daily_stats.total_heartbeats + 1,
                last_heartbeat = NOW(),
                updated_at = NOW()
            RETURNING *
            "#,
        )
        .bind(today)
        .bind(client_id)
        .bind(cpu_usage)
        .bind(memory_usage)
        .bind(disk_usage)
        .bind(network_in)
        .bind(network_out)
        .fetch_one(&mut **tx)
        .await?;

        Ok(record)
    }

    pub async fn get_stats(
        pool: &PgPool,
        client_id: &[u8; 16],
        start_date: NaiveDate,
        end_date: NaiveDate,
    ) -> Result<Vec<Self>, sqlx::Error> {
        sqlx::query_as(
            format!("SELECT * FROM {} WHERE client_id = $1 AND date BETWEEN $2 AND $3 ORDER BY date DESC", CLIENT_DAILY_STATS_TABLE).as_str()
        )
        .bind(client_id)
        .bind(start_date)
        .bind(end_date)
        .fetch_all(pool)
        .await
    }

    pub async fn delete(
        pool: &PgPool,
        client_id: &[u8; 16],
        date: NaiveDate,
    ) -> Result<u64, sqlx::Error> {
        let result = sqlx::query(
            format!(
                "DELETE FROM {} WHERE client_id = $1 AND date = $2",
                CLIENT_DAILY_STATS_TABLE
            )
            .as_str(),
        )
        .bind(client_id)
        .bind(date)
        .execute(pool)
        .await?;
        Ok(result.rows_affected())
    }
}

impl DeviceDailyStats {
    pub async fn upsert_batch(
        tx: &mut Transaction<'_, Postgres>,
        client_id: &ClientId,
        devices: &Vec<common::DevicesInfo>,
    ) -> Result<i32, sqlx::Error> {
        if devices.is_empty() {
            return Ok(0);
        }

        let today = Utc::now().date_naive();
        let mut query_builder = QueryBuilder::new(
            format!(
                "
            INSERT INTO {} (
                date, client_id, device_index, device_name,
                avg_utilization, avg_temperature, avg_power_usage, avg_memory_usage,
                total_heartbeats
            )
            ",
                DEVICE_DAILY_STATS_TABLE
            )
            .as_str(),
        );
        //batch insert
        query_builder.push_values(devices, |mut b, device| {
            for index in 0..device.num {
                b.push_bind(today)
                    .push_bind(client_id)
                    .push_bind(index as i16)
                    .push_bind(format!(
                        "{} {}",
                        common::id_to_vendor(get_u16_from_u128(device.vendor_id, index as usize))
                            .unwrap_or("Unknown"),
                        common::id_to_model(get_u16_from_u128(device.device_id, index as usize))
                            .unwrap_or("Unknown".to_string())
                    ))
                    .push_bind(Some(device.usage as f64))
                    .push_bind(Some(device.temp as f64))
                    .push_bind(Some(device.power_usage as f64))
                    .push_bind(Some(device.mem_usage as f64))
                    .push_bind(1); 
            }
        });

        query_builder.push(
            format!("
            ON CONFLICT (date, client_id, device_index) 
            DO UPDATE SET
                  device_name = EXCLUDED.device_name,
                avg_utilization = (COALESCE({}.avg_utilization, 0) * {}.total_heartbeats + EXCLUDED.avg_utilization) / 
                                ({}.total_heartbeats + 1),
                avg_temperature = (COALESCE({}.avg_temperature, 0) * {}.total_heartbeats + EXCLUDED.avg_temperature) / 
                                ({}.total_heartbeats + 1),
                avg_power_usage = (COALESCE({}.avg_power_usage, 0) * {}.total_heartbeats + EXCLUDED.avg_power_usage) / 
                                ({}.total_heartbeats + 1),
                avg_memory_usage = (COALESCE({}.avg_memory_usage, 0) * {}.total_heartbeats + EXCLUDED.avg_memory_usage) / 
                                 ({}.total_heartbeats + 1),
                total_heartbeats = {}.total_heartbeats + 1,
                updated_at = NOW()
            RETURNING *
            ", DEVICE_DAILY_STATS_TABLE, DEVICE_DAILY_STATS_TABLE, DEVICE_DAILY_STATS_TABLE, DEVICE_DAILY_STATS_TABLE, DEVICE_DAILY_STATS_TABLE, DEVICE_DAILY_STATS_TABLE, DEVICE_DAILY_STATS_TABLE, DEVICE_DAILY_STATS_TABLE, DEVICE_DAILY_STATS_TABLE, DEVICE_DAILY_STATS_TABLE, DEVICE_DAILY_STATS_TABLE, DEVICE_DAILY_STATS_TABLE, DEVICE_DAILY_STATS_TABLE).as_str(),
        );

        let records = query_builder.build().execute(&mut **tx).await?;

        Ok(records.rows_affected() as i32)
    }

    pub async fn get_stats(
        pool: &PgPool,
        client_id: &[u8; 16],
        device_index: Option<i32>,
        start_date: NaiveDate,
        end_date: NaiveDate,
    ) -> Result<Vec<Self>, sqlx::Error> {
        let mut query = QueryBuilder::new(
            format!(
                "SELECT * FROM {} WHERE client_id = ",
                DEVICE_DAILY_STATS_TABLE
            )
            .as_str(),
        );

        query.push_bind(client_id);
        query.push(" AND date BETWEEN ");
        query.push_bind(start_date);
        query.push(" AND ");
        query.push_bind(end_date);

        if let Some(idx) = device_index {
            query.push(" AND device_index = ");
            query.push_bind(idx);
        }

        query.push(" ORDER BY date, device_index");

        query.build_query_as().fetch_all(pool).await
    }
}

pub async fn insert_heartbeat(
    tx: &mut Transaction<'_, Postgres>,
    client_id: &ClientId,
    system_info: &SystemInfo,
    devices_info: &Vec<DevicesInfo>,
    device_memtotal_gb: i32,
    device_count: i32,
    total_tflops: i32,
    timestamp: Option<DateTime<Utc>>,
) -> anyhow::Result<()> {
    let timestamp = timestamp.unwrap_or_else(Utc::now);
    // Insert heartbeat record using the transaction
    let _ = sqlx::query(
        format!("
        INSERT INTO {} (client_id, cpu_usage, mem_usage, disk_usage, network_up, network_down, timestamp)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        ", HEARTBEAT_TABLE).as_str(),
    )
    .bind(client_id)
    .bind(system_info.cpu_usage as i16)
    .bind(system_info.memory_usage as i16)
    .bind(system_info.disk_usage as i16)
    .bind(system_info.network_tx as i64)
    .bind(system_info.network_rx as i64)
    .bind(timestamp)
    .execute(&mut **tx)
    .await?;

    // Update GPU assets status
    sqlx::query(&format!("UPDATE  {} SET client_status = $1, updated_at = NOW() WHERE client_id = $2 AND valid_status = 'valid' ",GPU_ASSETS_TABLE))
    .bind("online")
    .bind(client_id)
    .execute(&mut **tx)  // Properly deref the transaction
    .await?;
    // let device_count = devices_info.len() as i32;
    // 1. Insert system info
    sqlx::query(&format!(
        "
        INSERT INTO {} (
            client_id,
            cpu_usage,
            mem_usage,
            disk_usage,
            total_tflops,
            device_memsize,
            device_count,
            created_at,
            updated_at
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, NOW(), NOW())
        ON CONFLICT (client_id) 
        DO UPDATE SET
            cpu_usage = EXCLUDED.cpu_usage,
            mem_usage = EXCLUDED.mem_usage,
            disk_usage = EXCLUDED.disk_usage,
            total_tflops = EXCLUDED.total_tflops,
            device_memsize = EXCLUDED.device_memsize,
            device_count = EXCLUDED.device_count,
            updated_at = NOW()
        ",
        SYSTEM_INFO_TABLE
    ))
    .bind(client_id)
    .bind(system_info.cpu_usage as i16)
    .bind(system_info.memory_usage as i16)
    .bind(system_info.disk_usage as i16)
    .bind(total_tflops)
    .bind(device_memtotal_gb)
    .bind(device_count)
    .execute(&mut **tx)
    .await?;

    // 2. Delete old device info
    sqlx::query(format!(r#"DELETE FROM {} WHERE client_id = $1"#, DEVICE_INFO_TABLE).as_str())
        .bind(client_id)
        .execute(&mut **tx) // Properly deref the transaction
        .await?;

    // 3. Batch insert device information
    if !devices_info.is_empty() {
        let mut values_placeholder = String::new();
        let params_per_device = 9;
        // Build the values part of the query
        for device_info in devices_info {
            values_placeholder.push_str(
                &((0..device_info.num)
                    .map(|i| {
                        let base = i * params_per_device;
                        format!(
                            "(${}, ${}, ${}, ${}, ${}, ${}, ${}, ${}, ${}, NOW(), NOW())",
                            base + 1,
                            base + 2,
                            base + 3,
                            base + 4,
                            base + 5,
                            base + 6,
                            base + 7,
                            base + 8,
                            base + 9,
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", ")),
            );
        }

        // Build the full query
        let query_str = format!(
            "INSERT INTO {} (
                client_id,
                device_name,
                device_index,
                device_id,
                vendor_id,
                device_memusage,
                device_gpuusage,
                device_powerusage,
                device_temp,
                created_at,
                updated_at
            ) VALUES {}",
            DEVICE_INFO_TABLE, values_placeholder
        );

        let mut query = sqlx::query(&query_str);
        // Bind all parameters in order
        for device_info in devices_info {
            for device_index in 0..device_info.num {
                query = query
                    .bind(&client_id)
                    .bind(format!(
                        "{} {}",
                        common::id_to_vendor(get_u16_from_u128(
                            device_info.vendor_id,
                            device_index as usize
                        ))
                        .unwrap_or("Unknown"),
                        common::id_to_model(get_u16_from_u128(
                            device_info.device_id,
                            device_index as usize
                        ))
                        .unwrap_or("Unknown".to_string())
                    ))
                    .bind(device_index as i16)
                    .bind(get_u16_from_u128(device_info.device_id, device_index as usize) as i32)
                    .bind(get_u16_from_u128(device_info.vendor_id, device_index as usize) as i32)
                    .bind(get_u8_from_u64(device_info.mem_usage, device_index as usize) as i16)
                    .bind(get_u8_from_u64(device_info.usage, device_index as usize) as i16)
                    .bind(get_u8_from_u64(device_info.power_usage, device_index as usize) as i16)
                    .bind(get_u8_from_u64(device_info.temp, device_index as usize) as i16);
            }
        }
        query.execute(&mut **tx).await?;
    }
    Ok(())
}

#[derive(Debug, Serialize)]
pub struct ClientStatResponse {
    pub systems_total_number: i64,
    pub systems_online_number: i64,
    pub systems_maintenance_number: i64,
    pub systems_warnings_number: i64,
    pub total_tflops: i64,
    pub uptime_rate: i32,
}

#[derive(sqlx::FromRow)]
pub struct ClientStatsDb {
    pub total: i64,
    pub maintenance_count: i64,
    pub warning_count: i64,
    pub total_tflops: i64,
}

pub async fn get_client_stats(
    pool: &sqlx::PgPool,
    user_id: &str,
    recent_interval: Option<time::Duration>, 
    _analysis_window: Option<time::Duration>, 
) -> Result<ClientStatResponse> {

    let recent_interval = recent_interval.unwrap_or_else(|| time::Duration::minutes(5));
    //let analysis_window = analysis_window.unwrap_or_else(|| time::Duration::hours(24));

    let stats = sqlx::query_as::<_, ClientStatsDb>(
        &format!("
        WITH client_stats AS (
            SELECT 
                COUNT(*) as total,
                COUNT(CASE WHEN ga.client_status = 'maintenance' THEN 1 END) as maintenance_count,
                COUNT(CASE WHEN ga.valid_status = 'warning' OR ga.valid_status = 'invalid' THEN 1 END) as warning_count,
                COALESCE(SUM(si.total_tflops), 0) as total_tflops
            FROM {} ga
            LEFT JOIN {} si ON ga.client_id = si.client_id
            WHERE ga.user_id = $1 and ga.valid_status = 'valid'
        )
        SELECT 
            total,
            maintenance_count,
            warning_count,
            total_tflops
        FROM client_stats
        ", GPU_ASSETS_TABLE, SYSTEM_INFO_TABLE)
    )
    .bind(user_id)
    .fetch_one(pool)
    .await?;

    let online_count = sqlx::query_scalar(&format!(
        "
        SELECT COUNT(DISTINCT ga.client_id)
        FROM {} ga
        JOIN {} h ON ga.client_id = h.client_id
        WHERE ga.user_id = $1 and ga.valid_status = 'valid'
        AND h.timestamp > NOW() - $2::interval",
        GPU_ASSETS_TABLE, HEARTBEAT_TABLE
    ))
    .bind(user_id)
    .bind(format!("{} seconds", recent_interval.whole_seconds()))
    .fetch_one(pool)
    .await?;

    let avg_uptime: f64 = sqlx::query_scalar(&format!(
        "
        SELECT COALESCE(AVG(total_heartbeats::float / 720), 0.0) * 100 as avg_uptime
        FROM {} cds
        JOIN {} ga ON cds.client_id = ga.client_id
        WHERE ga.user_id = $1 and ga.valid_status = 'valid'
        AND cds.date = CURRENT_DATE - INTERVAL '1 day'
        AND cds.total_heartbeats > 0
        ",
        CLIENT_DAILY_STATS_TABLE, GPU_ASSETS_TABLE
    ))
    .bind(user_id)
    .fetch_one(pool)
    .await?;

    Ok(ClientStatResponse {
        systems_total_number: stats.total,
        systems_online_number: online_count,
        systems_maintenance_number: stats.maintenance_count,
        systems_warnings_number: stats.warning_count,
        total_tflops: stats.total_tflops,
        uptime_rate: avg_uptime.round() as i32,
    })
}

#[tokio::test]
async fn test_device_daily_stats() {
    //let pool = PgPool::connect("postgres://postgres:postgres@localhost:5432/postgres").unwrap();
    let pool = PgPool::connect("postgres://postgres:postgres@localhost:5432/postgres")
        .await
        .unwrap();
    let client_id = [0; 16];
    let device_index = 1;
    let device_info = common::DevicesInfo {
        os_type: common::OsType::LINUX,
        engine_type: common::EngineType::None,
        port: 0,
        ip: 0,
        memtotal_gb: 1,
        pod_id: 0,
        num: 0,
        vendor_id: 0,
        device_id: 0,
        usage: 1,
        temp: 1,
        power_usage: 1,
        mem_usage: 1,
        memsize_gb: 1,
        powerlimit_w: 1,
        total_tflops: 1,
    };
    let start_date = Utc::now().date_naive();
    let end_date = Utc::now().date_naive();
    let mut tx = pool.begin().await.unwrap();
    let _ = DeviceDailyStats::upsert_batch(&mut tx, &ClientId(client_id), &vec![device_info])
        .await
        .unwrap();
    tx.commit().await.unwrap();

    let stats = DeviceDailyStats::get_stats(
        &pool,
        &client_id,
        Some(device_index.into()),
        start_date,
        end_date,
    )
    .await
    .unwrap();
    assert_eq!(stats.len(), 1);
    assert_eq!(stats[0].total_heartbeats, 1);
    assert_eq!(stats[0].avg_utilization, Some(1.0));
    assert_eq!(stats[0].avg_temperature, Some(1.0));
    assert_eq!(stats[0].avg_power_usage, Some(1.0));
    assert_eq!(stats[0].avg_memory_usage, Some(1.0));
}

#[derive(Debug, Validate, Serialize, Deserialize)]
pub struct EditClientRequest {
    #[validate(length(min = 1, max = 255))]
    pub user_id: String,
    #[validate(length(min = 1, max = 255))]
    pub client_id: String,
    #[validate(length(max = 50))]
    pub os_type: Option<String>,
    #[validate(length(max = 255))]
    pub name: Option<String>,
    #[validate(length(max = 20))]
    pub client_status: Option<String>,
    #[validate(length(max = 10))]
    pub valid_status: Option<String>,

    model: Option<String>,
    model_version: Option<String>,
}

pub async fn update_gpu_asset_status(
    pool: &Pool<Postgres>,
    payload: &EditClientRequest,
) -> Result<String> {
    // Build the update query dynamically based on provided fields
    let mut query = format!("UPDATE {} SET updated_at = NOW() ", GPU_ASSETS_TABLE);

    // Build the parameter values in a vector first
    let mut param_values = Vec::new();
    let mut param_count = 1;
    if let Some(os_type) = &payload.os_type {
        debug!("update_gpu_asset_status set os_type: {}", os_type);
        query.push_str(&format!(", os_type = ${} ", param_count));
        param_values.push(os_type.clone());
        param_count += 1;
    }

    if let Some(name) = &payload.name {
        query.push_str(&format!(", client_name = ${} ", param_count));
        param_values.push(name.clone());
        param_count += 1;
    }

    if let Some(status) = &payload.client_status {
        query.push_str(&format!(", client_status = ${} ", param_count));
        param_values.push(status.clone());
        param_count += 1;
    }

    if let Some(valid_status) = &payload.valid_status {
        query.push_str(&format!(", valid_status = ${} ", param_count));
        param_values.push(valid_status.clone());
        param_count += 1;
    }

    query.push_str(&format!(
        " WHERE user_id = ${} AND client_id = ${} RETURNING client_id",
        param_count,
        param_count + 1
    ));

    param_values.push(payload.user_id.clone());

    // Create a query builder with parameters
    let mut query_builder = sqlx::query(&query);

    // Add parameters to the query
    for param in &param_values {
        query_builder = query_builder.bind(param);
    }

    query_builder = query_builder.bind(
        payload
            .client_id
            .clone()
            .parse::<ClientId>()
            .map_err(|_| anyhow::anyhow!("Invalid client_id"))?,
    );

    // Execute the query
    query_builder.execute(pool).await?;

    Ok(payload.client_id.clone())
}

#[derive(Debug, FromRow, Serialize, Deserialize)]
pub struct ClientMonitorInfo {
    // From gpu_assets
    #[serde(serialize_with = "serialize_bytes_as_hex")]
    pub client_id: Vec<u8>,
    pub client_name: Option<String>,
    pub created_at: Option<NaiveDateTime>,
    pub updated_at: Option<NaiveDateTime>,

    // From client_daily_stats
    pub date: Option<NaiveDate>,
    pub avg_cpu_usage: Option<f64>,
    pub avg_memory_usage: Option<f64>,
    pub avg_disk_usage: Option<f64>,
    pub total_network_in_bytes: Option<i64>,
    pub total_network_out_bytes: Option<i64>,
    pub total_heartbeats: Option<i32>,
    pub last_heartbeat: Option<DateTime<Utc>>,

    // Calculated fields
    pub avg_network_in_bytes: Option<f64>,
    pub avg_network_out_bytes: Option<f64>,
}

fn serialize_bytes_as_hex<S>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(&hex::encode(bytes))
}

pub async fn get_client_monitor(
    pool: &Pool<Postgres>,
    user_id: &str,
    client_id: Option<String>,
) -> Result<Vec<ClientMonitorInfo>> {
    let mut query_builder = sqlx::QueryBuilder::new(&format!(
        "
     WITH stats_with_avg AS (
        SELECT 
            ga.client_id,
            ga.client_name,
            ga.created_at,
            ga.updated_at,
            cds.date,
            cds.avg_cpu_usage,
            cds.avg_memory_usage,
            cds.avg_disk_usage,
            cds.total_network_in_bytes,
            cds.total_network_out_bytes,
            cds.total_heartbeats,
            cds.last_heartbeat,
            CASE 
                WHEN cds.total_heartbeats > 0 
                THEN cds.total_network_in_bytes::float8 / NULLIF(cds.total_heartbeats, 0) 
                ELSE 0 
            END as avg_network_in_bytes,
            CASE 
                WHEN cds.total_heartbeats > 0 
                THEN cds.total_network_out_bytes::float8 / NULLIF(cds.total_heartbeats, 0) 
                ELSE 0 
            END as avg_network_out_bytes
        FROM {} ga
        LEFT JOIN {} cds ON ga.client_id = cds.client_id
        WHERE ga.user_id = $1
        AND ga.valid_status = 'valid'
  
   
    ",
        GPU_ASSETS_TABLE, CLIENT_DAILY_STATS_TABLE
    ));

    // Add client_id filter if provided
    if let Some(_cid) = &client_id {
        query_builder.push(" AND ga.client_id = $2");
    }

    // Order by most recent dates first
    query_builder
        .push(" ) SELECT * FROM stats_with_avg  ORDER BY date DESC NULLS LAST, updated_at DESC");

    // Build the query
    let mut query = query_builder.build_query_as::<ClientMonitorInfo>();

    // Bind parameters
    query = query.bind(user_id);
    if let Some(cid) = client_id {
        query = query.bind(hex::decode(cid)?);
    }

    // Execute the query
    let mut results = query.fetch_all(pool).await?;

    // Convert the average values to f64 for consistency
    for result in &mut results {
        // The database already calculated these, but we need to ensure they're f64
        if let (Some(in_bytes), Some(heartbeats)) =
            (result.total_network_in_bytes, result.total_heartbeats)
        {
            if heartbeats > 0 {
                result.avg_network_in_bytes = Some(in_bytes as f64 / heartbeats as f64);
            }
        }
        if let (Some(out_bytes), Some(heartbeats)) =
            (result.total_network_out_bytes, result.total_heartbeats)
        {
            if heartbeats > 0 {
                result.avg_network_out_bytes = Some(out_bytes as f64 / heartbeats as f64);
            }
        }
    }

    Ok(results)
}

#[derive(Debug, FromRow, Serialize, Deserialize)]
pub struct ClientHeartbeatInfo {
    #[serde(serialize_with = "serialize_bytes_as_hex")]
    pub client_id: Vec<u8>,
    pub client_name: Option<String>,
    pub timestamp: DateTime<Utc>,
    pub cpu_usage: Option<i16>,
    pub mem_usage: Option<i16>,
    pub disk_usage: Option<i16>,
    pub network_up: i64,
    pub network_down: i64,
}

pub async fn get_client_heartbeats(
    pool: &Pool<Postgres>,
    user_id: &str,
    client_id: Option<String>,
    start_date: Option<String>,
    end_date: Option<String>,
) -> Result<Vec<ClientHeartbeatInfo>> {
    let mut query = format!(
        "
        SELECT 
            h.client_id,
            COALESCE(ga.client_name, '') as client_name,
            h.timestamp,
            h.cpu_usage,
            h.mem_usage,
            h.disk_usage,
            h.network_up,
            h.network_down
        FROM {} h
        INNER JOIN {} ga ON h.client_id = ga.client_id
        WHERE ga.user_id = $1 
        AND ga.valid_status = 'valid'
    ",
        HEARTBEAT_TABLE, GPU_ASSETS_TABLE
    );

    let mut params: Vec<String> = vec![user_id.to_string()];
    let mut param_count = 2; // Start from $2

    // Add client_id filter if provided
    if let Some(cid) = &client_id {
        query.push_str(&format!(" AND h.client_id = ${}", param_count));
        params.push(
            hex::decode(cid)?
                .into_iter()
                .map(|b| format!("{:02x}", b))
                .collect(),
        );
        param_count += 1;
    }
    
    let start_datetime = start_date.as_ref().and_then(|d| {
        NaiveDateTime::parse_from_str(d, "%Y-%m-%dT%H:%M:%S")
            .ok()
            .map(|ndt| ndt.and_utc().naive_utc()) 
    });
    let end_datetime = end_date.as_ref().and_then(|d| {
        NaiveDateTime::parse_from_str(d, "%Y-%m-%dT%H:%M:%S")
            .ok()
            .map(|ndt| ndt.and_utc().naive_utc())
    });

    // Add start date filter if provided
    if let Some(start) = &start_datetime {
        query.push_str(&format!(" AND h.timestamp >= ${}::timestamp", param_count));
        info!("start_datetime: {}", start);
        params.push(start.to_string());
        param_count += 1;
    }

    // Add end date filter if provided
    if let Some(end) = &end_datetime {
        query.push_str(&format!(" AND h.timestamp < ${}::timestamp", param_count));
        params.push(end.to_string());
        let _ = param_count + 1; // param_count is only used for SQL parameter numbering
    }

    // Order by timestamp in descending order
    query.push_str(" ORDER BY h.timestamp DESC");

    // Execute the query with parameters
    let mut query_builder = sqlx::query_as::<_, ClientHeartbeatInfo>(&query);

    // Bind all parameters
    for param in params {
        query_builder = query_builder.bind(param);
    }

    let heartbeats = query_builder.fetch_all(pool).await?;
    Ok(heartbeats)
}
