use crate::db::GPU_ASSETS_TABLE;
use crate::util::protoc::ClientId;
use anyhow::Result;
use chrono::{DateTime, Utc};
use common::{DevicesInfo, EngineType, OsType, PodModel};
use lru::LruCache;
use sqlx::{Pool, Postgres};
use std::num::NonZeroUsize;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info, warn};

#[derive(Clone)]
pub struct HotModelClass {
    pool: Arc<Pool<Postgres>>,
    #[allow(dead_code)] // TODO: Implement caching functionality
    cache: Arc<RwLock<LruCache<u32, String>>>,
}

#[allow(dead_code)]
const GB50_IN_MB: u32 = 50 * 1024;

impl HotModelClass {
    pub fn new(pool: Arc<Pool<Postgres>>) -> Self {
        Self {
            pool,
            cache: Arc::new(RwLock::new(LruCache::new(NonZeroUsize::new(1000).unwrap()))),
        }
    }
    #[allow(dead_code)]
    fn align_gpu_memory(mem_mb: u32) -> u32 {
        (mem_mb / GB50_IN_MB) * GB50_IN_MB
    }
    pub async fn get_hot_model(&self, mem_total_gb: u32, engine_type: i16) -> Result<String> {
        // TODO:
        // let aligned_mem = Self::align_gpu_memory(mem_total_mb);
        // {
        //     let cache = self.cache.read().await;
        //     if let Some(model) = cache.peek(&aligned_mem) {  // Use peek instead of get
        //         debug!("Found model for memory {} MB: {}", mem_total_mb, model);
        //         return Ok(model.cloneReceived empty models from server());
        //     }
        // }

        let model = match get_models_list(
            &self.pool,
            Some(true),
            Some(engine_type),
            Some(mem_total_gb as i32),
        )
        .await
        {
            Ok(model) => model,
            Err(e) => {
                warn!("Failed to get client model: {}", e);
                return Err(anyhow::anyhow!("Failed to get client model"));
            }
        };
        if model.is_empty() {
            warn!("No compatible models found for memory {} GB", mem_total_gb);
            return Ok("".to_string());
        }

        // TODO:
        // let mut cache = self.cache.write().await;
        // cache.put(aligned_mem, format!("{}:{}", model[0].name, model[0].version));

        Ok(model[0].name.clone())
    }
}

#[derive(Clone)]
#[allow(dead_code)] // Client model classification system, partially implemented
pub struct ClientModelClass {
    #[allow(dead_code)] // Database connection pool for model queries
    pool: Arc<Pool<Postgres>>,
    #[allow(dead_code)] // Cache for client model mappings
    cache: Arc<RwLock<LruCache<ClientId, String>>>,
}

impl ClientModelClass {
    #[allow(dead_code)] // Constructor for client model classification
    pub fn new(pool: Arc<Pool<Postgres>>) -> Self {
        Self {
            pool,
            cache: Arc::new(RwLock::new(LruCache::new(NonZeroUsize::new(1000).unwrap()))),
        }
    }

    #[allow(dead_code)] // Client model retrieval with caching
    pub async fn get_client_model(&self, client_id: &ClientId) -> Result<String> {
        {
            let cache = self.cache.read().await;
            if let Some(model) = cache.peek(client_id) {
                return Ok(model.clone());
            }
        }

        let model = match get_client_model_impl(&self.pool, client_id).await {
            Ok(model) => model,
            Err(e) => {
                warn!("Failed to get client model: {}", e);
                return Err(anyhow::anyhow!("Failed to get client model"));
            }
        };

        let mut cache = self.cache.write().await;
        cache.put(client_id.clone(), model.clone());

        Ok(model)
    }
}

#[derive(sqlx::FromRow)]
#[allow(dead_code)] // Database row mapping for client model queries
struct ClientModel {
    #[allow(dead_code)] // Client model name
    model: Option<String>,
    #[allow(dead_code)] // Client model version
    model_version: Option<String>,
}

// Get client model
#[allow(dead_code)] // Database query implementation for client model retrieval
async fn get_client_model_impl(pool: &Pool<Postgres>, client_id: &ClientId) -> Result<String> {
    let client = sqlx::query_as::<_, ClientModel>(&format!(
        "
             SELECT 
            model,
            model_version
            FROM {} 
            WHERE client_id = $1 AND outo_set_model = $2
            ",
        GPU_ASSETS_TABLE
    ))
    .bind(client_id)
    .bind(false)
    .fetch_optional(pool)
    .await
    .map_err(|e| anyhow::anyhow!("Failed to query client model: {}", e))?;

    match client {
        Some(record) => match (record.model, record.model_version) {
            (Some(model), Some(version)) if !model.is_empty() && !version.is_empty() => {
                let model_str = format!("{}:{}", model, version);
                info!("Found model for client {}: {}", client_id, model_str);
                Ok(model_str)
            }
            _ => Err(anyhow::anyhow!("Client has no valid model information")),
        },
        None => Err(anyhow::anyhow!("Client not found")),
    }
}

pub async fn create_or_update_model(
    pool: &Pool<Postgres>,
    name: &str,
    version: &str,
    version_code: i64,
    engine_type: i16,
    is_active: Option<bool>,
    min_memory_mb: Option<i32>,
    min_gpu_memory_gb: Option<i32>,
) -> Result<()> {
    let _result   = sqlx::query(
        &format!("
        INSERT INTO {} (name, version, version_code, is_active, min_memory_mb, engine_type, min_gpu_memory_gb)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        ON CONFLICT (name, version) 
        DO UPDATE SET 
            version_code = EXCLUDED.version_code,
            is_active = COALESCE(EXCLUDED.is_active, client_models.is_active),
            min_memory_mb = EXCLUDED.min_memory_mb,
            engine_type = EXCLUDED.engine_type,
            min_gpu_memory_gb = EXCLUDED.min_gpu_memory_gb
        RETURNING id, name, version, version_code, is_active, min_memory_mb, engine_type, min_gpu_memory_gb, created_at
        ",GPU_ASSETS_TABLE)
    )
    .bind(name)
    .bind(version)
    .bind(version_code)
    .bind(is_active)
    .bind(min_memory_mb)
    .bind(engine_type)
    .bind(min_gpu_memory_gb)
    .fetch_one(pool)
    .await
    .map_err(|e| {
        error!("Failed to create/update model: {}", e);
        e
    })?;
    Ok(())
}

#[derive(sqlx::FromRow)]
pub struct Models {
    pub id: i32,
    pub name: String,
    pub version: String,
    pub version_code: i64,
    pub is_active: bool,
    pub min_memory_mb: Option<i32>,
    pub min_gpu_memory_gb: Option<i32>,
    pub created_at: DateTime<Utc>,
}

pub async fn get_models_list(
    pool: &Pool<Postgres>,
    is_active: Option<bool>,
    engine_type: Option<i16>,
    min_gpu_memory_gb: Option<i32>,
) -> Result<Vec<Models>> {
    let mut query_builder = sqlx::QueryBuilder::new("SELECT id,name,version,version_code,is_active,min_memory_mb,min_gpu_memory_gb,created_at  FROM client_models WHERE 1=1");

    if let Some(active) = is_active {
        query_builder.push(" AND is_active = ").push_bind(active);
    }

    if let Some(mem) = min_gpu_memory_gb {
        query_builder
            .push(" AND (min_gpu_memory_gb IS NULL OR min_gpu_memory_gb <= ")
            .push_bind(mem)
            .push(")");
    }
    if let Some(engine_type) = engine_type {
        query_builder
            .push(" AND engine_type = ")
            .push_bind(engine_type);
    }
    query_builder.push(" ORDER BY version_code DESC, created_at DESC");
    let models = query_builder
        .build_query_as::<Models>()
        .fetch_all(pool)
        .await?;

    Ok(models)
}

pub async fn get_models_batch(
    hot_models: &Arc<HotModelClass>,
    devices_info: &Vec<DevicesInfo>,
) -> Result<Vec<PodModel>> {
    let mut pod_model = Vec::new();

    for device_info in devices_info {
        if device_info.engine_type == EngineType::None
            || device_info.os_type == OsType::NONE
            || device_info.memtotal_gb == 0
        {
            pod_model.push(PodModel {
                pod_id: device_info.pod_id,
                model_name: None,
            });
            continue;
        }
        match hot_models
            .get_hot_model(
                device_info.memtotal_gb as u32,
                device_info.engine_type.to_i16(),
            )
            .await
        {
            Ok(model) => {
                if model.is_empty() {
                    pod_model.push(PodModel {
                        pod_id: device_info.pod_id,
                        model_name: None,
                    });
                } else {
                    pod_model.push(PodModel {
                        pod_id: device_info.pod_id,
                        model_name: Some(model),
                    });
                }
            }
            Err(e) => {
                warn!("Failed to get hot model: {}", e);
                pod_model.push(PodModel {
                    pod_id: device_info.pod_id,
                    model_name: None,
                });
            }
        }
    }
    Ok(pod_model)
}
